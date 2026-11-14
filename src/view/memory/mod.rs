//! Task-local ABI 11 publication and retained mutation results.

use core::marker::PhantomData;
use erra::Error;
use kernel_api::{
    abi::{call::pend::PendFail, memory::*},
    address::{PAGE_SIZE, PageCount},
    model::{
        cell::{CellLevel, Placement},
        identity::PendId,
        nest::{MemoPhase, MemoPosition, ReservationPhase, ReserveRequest},
        pend::{PendRange, PendScope, PendStatus, Target, ViewTarget},
    },
};
use kernel_client::memory::{self, Response};

mod context;
pub use context::{Memory, Native};

/// Locator for the actual Task's retained result; not resource authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub request: RequestId,
    pub domain: Domain,
}

impl Receipt {
    fn of(request: Request) -> Self {
        Self {
            request: request.id().unwrap(),
            domain: request.action().opcode().domain(),
        }
    }

    pub fn query(self) -> Call {
        Call::from_request(
            Request::new(
                None,
                Action::InspectResult {
                    domain: self.domain,
                    request: self.request,
                },
            )
            .unwrap(),
        )
    }
}

#[derive(Debug)]
pub enum MappingError {
    Pend(Error<PendFail>),
    Transport(memory::Error),
    /// The mutation may have committed even though delivery failed.
    Uncertain {
        receipt: Receipt,
        error: memory::Error,
    },
    Rejected(Failure),
    /// Accepted work remains outstanding. Retry with the same mapping owner.
    Pending {
        receipt: Receipt,
    },
    /// Accepted failure is retained. Do not infer rollback or safe reclamation.
    Failed {
        receipt: Receipt,
        failure: Failure,
        retained: Option<Value>,
    },
    Expired {
        receipt: Receipt,
    },
    Exhausted,
    InvalidMapping,
    Returning,
}

impl MappingError {
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Rejected(Failure::Busy)
                | Self::Pending { .. }
                | Self::Transport(memory::Error::Delivery(Failure::Busy))
                | Self::Uncertain {
                    error: memory::Error::Delivery(Failure::Busy),
                    ..
                }
        ) || matches!(self, Self::Pend(error) if error.source.is_busy())
    }
}

/// Owns exactly one unresolved mutation. An uncertain delivery never becomes
/// a fresh mutation, even if the caller switches from opening to cleanup.
#[derive(Debug, Default)]
struct Mutation(Option<Request>);

impl Mutation {
    fn run(&mut self, memory: &mut impl Memory, action: Action) -> Result<Value, MappingError> {
        let original = if let Some(request) = self.0 {
            request
        } else {
            let id = memory.next_request().ok_or(MappingError::Exhausted)?;
            let request = Request::new(Some(id), action).ok_or(MappingError::InvalidMapping)?;
            self.0 = Some(request);
            request
        };
        if original.action() != action {
            return Err(MappingError::Returning);
        }
        // Replaying the exact immutable request cannot repeat an accepted
        // mutation: the Task journal returns its original retained result.
        let call = Call::from_request(original);
        let Response::Reply(reply) =
            memory
                .invoke(call)
                .map_err(|error| MappingError::Uncertain {
                    receipt: Receipt::of(original),
                    error,
                })?
        else {
            return Err(MappingError::InvalidMapping);
        };
        match reply.outcome() {
            Outcome::Committed(value) => {
                self.0 = None;
                Ok(value)
            }
            Outcome::Rejected(failure) if failure != Failure::RequestMismatch => {
                self.0 = None;
                Err(MappingError::Rejected(failure))
            }
            Outcome::Rejected(failure) => Err(MappingError::Rejected(failure)),
            Outcome::Pending(_) => Err(MappingError::Pending {
                receipt: Receipt::of(original),
            }),
            Outcome::Failed { failure, retained } => Err(MappingError::Failed {
                receipt: Receipt::of(original),
                failure,
                retained,
            }),
            Outcome::Expired => Err(MappingError::Expired {
                receipt: Receipt::of(original),
            }),
        }
    }
}

fn observe(memory: &mut impl Memory, query: Query) -> Result<Snapshot, MappingError> {
    let call = Call::from_request(Request::new(None, Action::Inspect(query)).unwrap());
    match memory.invoke(call).map_err(MappingError::Transport)? {
        Response::Observation(value) => Ok(value.snapshot()),
        Response::Reply(reply) => match reply.outcome() {
            Outcome::Rejected(failure) => Err(MappingError::Rejected(failure)),
            _ => Err(MappingError::InvalidMapping),
        },
    }
}

#[derive(Clone, Copy, Debug)]
struct Reservation {
    memo: MemoPosition,
    address: usize,
}

#[derive(Clone, Copy, Debug)]
enum Stage {
    Reserve,
    Insert(usize),
    Publish(usize),
    Ready,
    Release,
    Return,
    Closed,
}

/// One private CPU reservation over a Share source. Publication authority is
/// always a Nest Pend, independently of the source Pend. No Rust reference is
/// exposed; transport endpoints use the address only within an operation.
#[derive(Debug)]
#[must_use = "finish the mapping to observe and retry retirement failures"]
pub(crate) struct Mapping {
    token: PendId,
    bytes: usize,
    writable: bool,
    source: Option<Source>,
    reservation: Option<Reservation>,
    stage: Stage,
    mutation: Mutation,
    task: PhantomData<*mut ()>,
}

impl Mapping {
    pub(crate) fn new(token: PendId, bytes: usize, writable: bool) -> Result<Self, MappingError> {
        if token == PendId::NONE || bytes == 0 || bytes > isize::MAX as usize {
            return Err(MappingError::InvalidMapping);
        }
        Ok(Self {
            token,
            bytes,
            writable,
            source: None,
            reservation: None,
            stage: Stage::Reserve,
            mutation: Mutation::default(),
            task: PhantomData,
        })
    }

    fn pages(&self) -> usize {
        self.bytes.div_ceil(PAGE_SIZE)
    }

    /// Validate every publication before accessing storage. A snapshot is not
    /// a lifetime pin: shared-ring callers must still coordinate revocation.
    pub(crate) fn address(&mut self, memory: &mut impl Memory) -> Result<usize, MappingError> {
        while !matches!(self.stage, Stage::Ready) {
            if matches!(self.stage, Stage::Release | Stage::Return | Stage::Closed) {
                return Err(MappingError::Returning);
            }
            self.advance(memory)?;
        }
        let reservation = self.reservation.ok_or(MappingError::InvalidMapping)?;
        for page in 0..self.pages() {
            let Snapshot::Memo(status) = observe(memory, Query::Memo(self.memo(page)?))? else {
                return Err(MappingError::InvalidMapping);
            };
            let access = if self.writable { 6 } else { 2 };
            if status.phase != MemoPhase::Published || status.access & access != access {
                return Err(MappingError::InvalidMapping);
            }
        }
        Ok(reservation.address)
    }

    fn memo(&self, page: usize) -> Result<MemoPosition, MappingError> {
        let memo = self.reservation.ok_or(MappingError::InvalidMapping)?.memo;
        MemoPosition::new(memo.token(), memo.nest(), memo.reservation(), page)
            .ok_or(MappingError::InvalidMapping)
    }

    fn advance(&mut self, memory: &mut impl Memory) -> Result<(), MappingError> {
        // Retained descriptors include their original expected generation.
        // Do not re-observe and accidentally change an unresolved request.
        let action = if let Some(request) = self.mutation.0 {
            request.action()
        } else {
            match self.stage {
                Stage::Reserve => {
                    if self.source.is_none() {
                        let info = memory
                            .inspect_pend(self.token)
                            .map_err(MappingError::Pend)?;
                        let (
                            Target::View(ViewTarget::Share(view)),
                            PendScope::Pages(PendRange::Pages { offset, count }),
                        ) = (info.target, info.limits.scope)
                        else {
                            return Err(MappingError::InvalidMapping);
                        };
                        if info.status != PendStatus::Active || self.pages() > count {
                            return Err(MappingError::InvalidMapping);
                        }
                        self.source = Some(Source::Share {
                            token: self.token,
                            view,
                            page: offset,
                            count: 1,
                        });
                    }
                    let (token, nest) = memory.space();
                    let Snapshot::Nest(status) = observe(memory, Query::Nest { token, nest })?
                    else {
                        return Err(MappingError::InvalidMapping);
                    };
                    Action::Reserve {
                        token,
                        nest,
                        expected: status.version,
                        reservation: ReserveRequest {
                            placement: Placement::lowest(PAGE_SIZE).unwrap(),
                            count: PageCount::new(self.pages()).unwrap(),
                            level: CellLevel::BASE,
                            access: if self.writable { 6 } else { 2 },
                        },
                    }
                }
                Stage::Insert(page) => {
                    let target = self.memo(page)?;
                    let Snapshot::Memo(status) = observe(memory, Query::Memo(target))? else {
                        return Err(MappingError::InvalidMapping);
                    };
                    let Some(Source::Share {
                        token,
                        view,
                        page: origin,
                        ..
                    }) = self.source
                    else {
                        return Err(MappingError::InvalidMapping);
                    };
                    Action::Insert {
                        target,
                        expected: status.generation,
                        source: Source::Share {
                            token,
                            view,
                            page: origin + page,
                            count: 1,
                        },
                    }
                }
                Stage::Publish(page) => {
                    let memo = self.memo(page)?;
                    let Snapshot::Memo(status) = observe(memory, Query::Memo(memo))? else {
                        return Err(MappingError::InvalidMapping);
                    };
                    Action::Publish {
                        memo,
                        expected: status.generation,
                        maintenance: None,
                    }
                }
                Stage::Release => {
                    let memo = self.memo(0)?;
                    let Snapshot::Reservation(status) = observe(
                        memory,
                        Query::Reservation {
                            token: memo.token(),
                            nest: memo.nest(),
                            reservation: memo.reservation(),
                        },
                    )?
                    else {
                        return Err(MappingError::InvalidMapping);
                    };
                    Action::Release {
                        token: memo.token(),
                        nest: memo.nest(),
                        reservation: memo.reservation(),
                        expected: status.version,
                    }
                }
                Stage::Return => {
                    memory
                        .return_pend(self.memo(0)?.token())
                        .map_err(MappingError::Pend)?;
                    self.reservation = None;
                    self.stage = Stage::Closed;
                    return Ok(());
                }
                Stage::Ready | Stage::Closed => return Ok(()),
            }
        };
        let value = self.mutation.run(memory, action)?;
        match (self.stage, value) {
            (
                Stage::Reserve,
                Value::Reservation {
                    token,
                    nest,
                    reservation,
                    address,
                    ..
                },
            ) => {
                self.reservation = Some(Reservation {
                    memo: MemoPosition::new(token, nest, reservation, 0)
                        .ok_or(MappingError::InvalidMapping)?,
                    address: address.get(),
                });
                // Record the resource before validating its address, so cleanup
                // remains possible even on an unusable successful result.
                self.stage = Stage::Insert(0);
                if address.get() == 0 || address.get().checked_add(self.bytes).is_none() {
                    return Err(MappingError::InvalidMapping);
                }
            }
            (Stage::Insert(page), Value::Memo { .. }) => self.stage = Stage::Publish(page),
            (Stage::Publish(page), Value::Memo { .. }) => {
                self.stage = if page + 1 == self.pages() {
                    Stage::Ready
                } else {
                    Stage::Insert(page + 1)
                }
            }
            (
                Stage::Release,
                Value::ReservationLifecycle {
                    phase: ReservationPhase::Released,
                    ..
                },
            ) => self.stage = Stage::Return,
            _ => return Err(MappingError::InvalidMapping),
        }
        Ok(())
    }

    /// Settle any outstanding operation before releasing the whole reservation.
    /// Source authority is borrowed and is never returned here. Failure retains
    /// the owner for retry; no timeout is interpreted as hardware completion.
    pub(crate) fn finish(&mut self, memory: &mut impl Memory) -> Result<(), MappingError> {
        if self.mutation.0.is_some() {
            self.advance(memory)?;
        }
        if matches!(self.stage, Stage::Closed) {
            return Ok(());
        }
        if self.reservation.is_none() {
            self.stage = Stage::Closed;
            return Ok(());
        }
        if !matches!(self.stage, Stage::Release | Stage::Return) {
            self.stage = Stage::Release;
        }
        while !matches!(self.stage, Stage::Closed) {
            self.advance(memory)?;
        }
        Ok(())
    }
}
