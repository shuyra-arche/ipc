//! Scripted native transport: exercises Twine ownership/retry decisions only.
//! RV64/QEMU separately validates actual mappings and privileged calls.
use kernel_api::{
    abi::{call::pend::PendFail, memory::*},
    address::VirtAddr,
    model::{
        cell::CellLevel,
        flat::FlatId,
        identity::{Mark, PendId, TaskId},
        modifiable::Modifiable,
        nest::*,
        pend::*,
        view::ViewId,
    },
};
use kernel_client::memory::{Error as TransportError, Response};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use twine::view::{
    memory::{MappingError as NativeError, Memory},
    share::{MappedWriter, MappingError, Pages, Write},
};

#[repr(align(4096))]
struct Page([u8; 4096]);
#[derive(Clone, Copy)]
enum Fault {
    None,
    Lost(Opcode),
    BusyReserve,
    PendingRelease,
    FailedRelease,
    ExpiredRelease,
}
#[derive(Clone, Copy)]
struct Reservation {
    address: usize,
    source: Option<(FlatId, usize)>,
    generation: MemoGeneration,
    version: ReservationVersion,
    phase: MemoPhase,
    access: u64,
}
struct State {
    pages: Box<[Page; 3]>,
    reservations: BTreeMap<usize, Reservation>,
    records: BTreeMap<RequestId, (Request, Reply)>,
    calls: Vec<Request>,
    returned: Vec<PendId>,
    next: u64,
    fault: Fault,
    revoked: bool,
}
impl State {
    fn new(fault: Fault) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            pages: Box::new(std::array::from_fn(|_| Page([0; 4096]))),
            reservations: BTreeMap::new(),
            records: BTreeMap::new(),
            calls: vec![],
            returned: vec![],
            next: 1,
            fault,
            revoked: false,
        }))
    }
    fn nest() -> NestId {
        NestId::new(1).unwrap()
    }
    fn observe(&self, query: Query) -> Snapshot {
        match query {
            Query::Nest { .. } => Snapshot::Nest(NestStatus {
                info: NestInfo {
                    id: Self::nest(),
                    kind: NestKind::Internal,
                    phase: NestPhase::Bound,
                    fault_sequence: 0,
                    fault_cause: 0,
                    fault_address: 0,
                    fault_requester: 0,
                },
                version: NestVersion::INITIAL,
            }),
            Query::Memo(memo) => {
                let r = self.reservations[&memo.reservation().get()];
                Snapshot::Memo(MemoStatus {
                    nest: Self::nest(),
                    reservation: memo.reservation(),
                    coordinate: memo.page(),
                    generation: r.generation,
                    phase: r.phase,
                    source: r.source,
                    page: if r.phase == MemoPhase::Published {
                        1
                    } else {
                        0
                    },
                    access: if r.phase == MemoPhase::Published {
                        r.access
                    } else {
                        0
                    },
                    level: CellLevel::BASE,
                })
            }
            Query::Reservation { reservation, .. } => {
                let r = self.reservations[&reservation.get()];
                Snapshot::Reservation(ReservationStatus {
                    nest: Self::nest(),
                    reservation,
                    version: r.version,
                    offset: 0,
                    count: 1,
                    address: VirtAddr::new(r.address),
                    level: CellLevel::BASE,
                    access: r.access,
                    phase: ReservationPhase::Open,
                })
            }
            _ => panic!("unexpected query {query:?}"),
        }
    }
    fn effect(&mut self, action: Action) -> Value {
        match action {
            Action::Reserve { reservation, .. } => {
                let id = self.reservations.len() + 1;
                assert_eq!(reservation.count.get(), 1);
                assert!(id <= 3);
                let address = self.pages[id - 1].0.as_mut_ptr().addr();
                let version = ReservationVersion::INITIAL;
                self.reservations.insert(
                    id,
                    Reservation {
                        address,
                        source: None,
                        generation: MemoGeneration::INITIAL,
                        version,
                        phase: MemoPhase::Empty,
                        access: reservation.access,
                    },
                );
                Value::Reservation {
                    token: PendId::mint(100 + id),
                    nest: Self::nest(),
                    nest_version: NestVersion::INITIAL,
                    reservation: ReservationId::from_bytes(&(id as u64).to_le_bytes()).unwrap(),
                    version,
                    address: VirtAddr::new(address),
                }
            }
            Action::Insert {
                target,
                expected,
                source,
            } => {
                let r = self
                    .reservations
                    .get_mut(&target.reservation().get())
                    .unwrap();
                assert_eq!(expected, r.generation);
                let Source::Share {
                    token, page, count, ..
                } = source
                else {
                    panic!("Share source");
                };
                // PendInfo reports an absolute narrowed origin. Re-basing it to
                // zero would access outside the authorized source window.
                assert_eq!((page, count), (7, 1));
                assert_eq!(token.get(), target.reservation().get());
                r.source = Some((FlatId::new(token.get()).unwrap(), page));
                r.phase = MemoPhase::Associated;
                r.generation = r.generation.next().unwrap();
                r.version = r.version.next().unwrap();
                Value::Memo {
                    generation: r.generation,
                }
            }
            Action::Publish { memo, expected, .. } => {
                let r = self
                    .reservations
                    .get_mut(&memo.reservation().get())
                    .unwrap();
                assert_eq!(expected, r.generation);
                assert_eq!(r.phase, MemoPhase::Associated);
                r.phase = MemoPhase::Published;
                r.generation = r.generation.next().unwrap();
                r.version = r.version.next().unwrap();
                Value::Memo {
                    generation: r.generation,
                }
            }
            Action::Release {
                reservation,
                expected,
                ..
            } => {
                let r = self.reservations.get_mut(&reservation.get()).unwrap();
                assert_eq!(expected, r.version);
                r.phase = MemoPhase::Empty;
                r.source = None;
                r.version = r.version.next().unwrap();
                Value::ReservationLifecycle {
                    nest: Self::nest(),
                    reservation,
                    version: r.version,
                    phase: ReservationPhase::Released,
                }
            }
            _ => panic!("unexpected action {action:?}"),
        }
    }
}
struct Transport(Rc<RefCell<State>>);
// SAFETY: This host transport supplies real, aligned, exclusively owned backing
// storage held throughout each endpoint's lifetime. Its descriptors and retained
// results model the same publication/retirement contract; it never executes a trap.
unsafe impl Memory for Transport {
    fn space(&self) -> (PendId, NestId) {
        (PendId::mint(50), State::nest())
    }
    fn next_request(&mut self) -> Option<RequestId> {
        let mut s = self.0.borrow_mut();
        let id = s.next;
        s.next += 1;
        RequestId::new(id)
    }
    fn invoke(&mut self, call: Call) -> Result<Response, TransportError> {
        let request = call.request();
        let mut s = self.0.borrow_mut();
        s.calls.push(request);
        if let Action::Inspect(query) = request.action() {
            if s.revoked && matches!(query, Query::Memo(_)) {
                return Err(TransportError::Delivery(Failure::Denied));
            }
            return Ok(Response::Observation(
                Observation::new(s.observe(query)).unwrap(),
            ));
        }
        let id = request.id().unwrap();
        if let Some((old, reply)) = s.records.get(&id).copied() {
            assert_eq!(old, request, "a retained ID must never change descriptors");
            if matches!(reply.outcome(), Outcome::Pending(_)) {
                let value = s.effect(request.action());
                let reply =
                    Reply::new(id, request.action().opcode(), Outcome::Committed(value)).unwrap();
                s.records.insert(id, (request, reply));
                return Ok(Response::Reply(reply));
            }
            return Ok(Response::Reply(reply));
        }
        let opcode = request.action().opcode();
        let outcome = if matches!(s.fault, Fault::BusyReserve) && opcode == Opcode::Reserve {
            s.fault = Fault::None;
            Outcome::Rejected(Failure::Busy)
        } else if matches!(s.fault, Fault::PendingRelease) && opcode == Opcode::Release {
            s.fault = Fault::None;
            let Action::Release {
                nest,
                reservation,
                expected,
                ..
            } = request.action()
            else {
                unreachable!();
            };
            Outcome::Pending(Value::ReservationLifecycle {
                nest,
                reservation,
                version: expected,
                phase: ReservationPhase::Releasing,
            })
        } else if matches!(s.fault, Fault::FailedRelease) && opcode == Opcode::Release {
            s.fault = Fault::None;
            let Action::Release {
                nest,
                reservation,
                expected,
                ..
            } = request.action()
            else {
                unreachable!();
            };
            Outcome::Failed {
                failure: Failure::Hardware,
                retained: Some(Value::ReservationLifecycle {
                    nest,
                    reservation,
                    version: expected,
                    phase: ReservationPhase::Failed,
                }),
            }
        } else if matches!(s.fault, Fault::ExpiredRelease) && opcode == Opcode::Release {
            s.fault = Fault::None;
            Outcome::Expired
        } else {
            Outcome::Committed(s.effect(request.action()))
        };
        let reply = Reply::new(id, opcode, outcome).unwrap();
        s.records.insert(id, (request, reply));
        if matches!(s.fault, Fault::Lost(at) if at == opcode) {
            s.fault = Fault::None;
            return Err(TransportError::Delivery(Failure::ReplyFault));
        }
        Ok(Response::Reply(reply))
    }
    fn inspect_pend(&mut self, token: PendId) -> Result<PendInfo, erra::Error<PendFail>> {
        Ok(PendInfo {
            token,
            target: Target::View(ViewTarget::Share(ViewId::new(token.get()))),
            limits: Limits {
                operations: Operations::Share(ShareRights::ASSOCIATE),
                modifiable: Modifiable::Yes,
                distributable: Distributable::No,
                scope: PendScope::Pages(PendRange::pages(7, 1).unwrap()),
            },
            owner: TaskId::new(1),
            lender: TaskId::new(0),
            mark: Mark::NONE,
            status: PendStatus::Active,
        })
    }
    fn return_pend(&mut self, token: PendId) -> Result<(), erra::Error<PendFail>> {
        assert!(token.get() > 100, "source Pends are borrowed");
        self.0.borrow_mut().returned.push(token);
        Ok(())
    }
}
fn writer(state: &Rc<RefCell<State>>) -> MappedWriter<Transport> {
    // SAFETY: Three disjoint, zeroed, page-aligned regions have the public ring
    // layout. There is one writer; tests never create another endpoint.
    unsafe {
        MappedWriter::new(
            Transport(state.clone()),
            Pages {
                data: PendId::mint(1),
                read: PendId::mint(2),
                write: PendId::mint(3),
            },
            4,
        )
        .unwrap()
    }
}
#[test]
fn uncertain_delivery_replays_the_original_descriptor_without_duplicate_effects() {
    for opcode in [Opcode::Reserve, Opcode::Insert, Opcode::Publish] {
        let state = State::new(Fault::Lost(opcode));
        let mut writer = writer(&state);
        assert!(matches!(
            writer.write(b"a"),
            Err(MappingError::Access {
                error: NativeError::Uncertain {
                    error: TransportError::Delivery(Failure::ReplyFault),
                    ..
                },
                ..
            })
        ));
        assert_eq!(state.borrow().pages[0].0[0], 0);
        assert_eq!(writer.write(b"a").unwrap(), Write::Bytes(1));
        writer.finish().unwrap();
        let s = state.borrow();
        assert_eq!(s.reservations.len(), 3);
        assert_eq!(s.returned.len(), 3);
        assert_eq!(s.pages[0].0[0], b'a');
        let first = s
            .calls
            .iter()
            .find(|r| r.action().opcode() == opcode)
            .unwrap();
        assert_eq!(s.calls.iter().filter(|r| **r == *first).count(), 2);
    }
}
#[test]
fn cleanup_settles_a_lost_reserve_before_releasing_without_publishing() {
    let state = State::new(Fault::Lost(Opcode::Reserve));
    let mut writer = writer(&state);
    assert!(writer.write(b"a").is_err());
    writer.finish().unwrap();
    let s = state.borrow();
    assert_eq!(s.reservations.len(), 1);
    assert_eq!(s.returned.len(), 1);
    assert!(
        !s.calls
            .iter()
            .any(|r| matches!(r.action(), Action::Insert { .. } | Action::Publish { .. }))
    );
}
#[test]
fn pending_and_lost_release_keep_the_same_request_until_settled() {
    for fault in [Fault::PendingRelease, Fault::Lost(Opcode::Release)] {
        let state = State::new(fault);
        let mut writer = writer(&state);
        writer.write(b"abcd").unwrap();
        assert!(writer.finish().is_err());
        assert!(state.borrow().returned.is_empty());
        writer.finish().unwrap();
        let s = state.borrow();
        assert_eq!(s.returned.len(), 3);
        let releases: Vec<_> = s
            .calls
            .iter()
            .filter(|r| r.action().opcode() == Opcode::Release)
            .collect();
        assert_eq!(releases.len(), 4);
        assert_eq!(releases[0], releases[1]);
    }
}
#[test]
fn rejected_busy_uses_a_new_id_and_revocation_prevents_further_byte_progress() {
    let state = State::new(Fault::BusyReserve);
    let mut writer = writer(&state);
    assert!(writer.write(b"a").unwrap_err().is_busy());
    assert_eq!(writer.write(b"a").unwrap(), Write::Bytes(1));
    state.borrow_mut().revoked = true;
    assert!(writer.write(b"b").is_err());
    assert_eq!(state.borrow().pages[0].0[1], 0);
    writer.finish().unwrap();
    let s = state.borrow();
    let reserves: Vec<_> = s
        .calls
        .iter()
        .filter(|r| r.action().opcode() == Opcode::Reserve)
        .collect();
    assert_ne!(reserves[0].id(), reserves[1].id());
    assert_eq!(s.returned.len(), 3);
}

#[test]
fn failed_or_expired_cleanup_never_returns_resources_or_issues_a_fresh_mutation() {
    for fault in [Fault::FailedRelease, Fault::ExpiredRelease] {
        let state = State::new(fault);
        let mut writer = writer(&state);
        writer.write(b"a").unwrap();
        for _ in 0..2 {
            assert!(writer.finish().is_err());
        }
        assert!(writer.write(b"b").is_err());
        let s = state.borrow();
        assert!(s.returned.is_empty());
        assert_eq!(s.pages[0].0[1], 0);
        let releases: Vec<_> = s
            .calls
            .iter()
            .filter(|r| r.action().opcode() == Opcode::Release)
            .collect();
        assert_eq!(releases.len(), 2);
        assert_eq!(releases[0], releases[1]);
    }
}
