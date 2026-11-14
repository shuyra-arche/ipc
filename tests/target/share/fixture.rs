//! Native ABI 11 fixture. All mutation users share NEXT within each Team image.
use core::{
    fmt, ptr,
    sync::atomic::{AtomicU64, Ordering},
};
use kernel_api::{
    abi::memory::*,
    address::{PAGE_SIZE, PageCount, VirtAddr},
    model::{
        cell::{CellLevel, Placement},
        flat::*,
        identity::{Mark, PendId, TaskId, TeamId},
        modifiable::Modifiable,
        nest::*,
        pend::*,
        unit::*,
        wait::Wait,
    },
};
use kernel_client::{
    call::{debug, pend, room, share_view, unit},
    memory::Response,
};
use twine::view::memory::Native;

static NEXT: AtomicU64 = AtomicU64::new(1);
pub type Context = Native<fn() -> Option<RequestId>>;
fn next() -> Option<RequestId> {
    NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .ok()
        .and_then(RequestId::new)
}
pub fn context(token: PendId) -> Context {
    let Target::Nest(nest) = pend_info(token).target else {
        panic!("Nest");
    };
    // SAFETY: The boot entry or parent grants this Task its own Internal Nest.
    unsafe { Native::new(token, nest, next as fn() -> Option<RequestId>) }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Drain,
    Revoke,
    Exit,
}
pub struct Console;
impl fmt::Write for Console {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let mut bytes = text.as_bytes();
        let end = end_time();
        while !bytes.is_empty() {
            let written = debug::put(addr(bytes), bytes.len().min(256)).map_err(|_| fmt::Error)?;
            if written == 0 {
                if room::clock() >= end {
                    return Err(fmt::Error);
                }
                room::yield_();
            }
            bytes = &bytes[written..];
        }
        Ok(())
    }
}
fn end_time() -> u64 {
    room::clock().saturating_add(5_000_000_000)
}
pub fn deadline() -> Wait {
    Wait::At(end_time())
}
pub fn exit(value: u64) -> ! {
    unit::exit(value, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
pub fn addr(bytes: &[u8]) -> VirtAddr {
    VirtAddr::new(bytes.as_ptr().addr())
}
pub fn pend_info(token: PendId) -> PendInfo {
    let mut bytes = [0; PendInfo::SIZE];
    pend::inspect(token, VirtAddr::new(bytes.as_mut_ptr().addr())).unwrap();
    PendInfo::from_bytes(&bytes).unwrap()
}
pub fn find(test: impl Fn(PendInfo) -> bool) -> PendId {
    let mut after = PendId::NONE;
    loop {
        let mut words = [0; PendInfo::WORDS];
        assert_eq!(
            pend::collect(after, VirtAddr::new(words.as_mut_ptr().addr()), 1).unwrap(),
            1
        );
        let info = PendInfo::from_words(words).unwrap();
        if test(info) {
            return info.token;
        }
        after = info.token;
    }
}
pub fn license() -> PendId {
    find(|info| info.target == Target::None)
}
pub fn space() -> PendId {
    find(|info| info.limits.scope == PendScope::Nest(NestScope::Whole))
}
pub fn collect<const N: usize>() -> [PendId; N] {
    let mut words = [[0; PendInfo::WORDS]; N];
    assert_eq!(
        pend::collect(PendId::NONE, VirtAddr::new(words.as_mut_ptr().addr()), N).unwrap(),
        N
    );
    words.map(|info| PendInfo::from_words(info).unwrap().token)
}
pub fn grant(
    src: PendId,
    task: Option<TaskId>,
    operations: Operations,
    modifiable: Modifiable,
) -> PendId {
    let request = AccordRequest {
        src,
        target: task.map_or(AccordTarget::Caller, AccordTarget::Task),
        mark: Mark::NONE,
        limits: Limits {
            operations,
            modifiable,
            distributable: Distributable::No,
            scope: match operations {
                Operations::Share(_) => PendScope::Pages(PendRange::Inherit),
                Operations::Nest(_) => pend_info(src).limits.scope,
                _ => PendScope::Object,
            },
        },
    };
    pend::accord(addr(&request.to_bytes())).unwrap()
}
pub fn call(action: Action) -> Response {
    let id = if matches!(action, Action::Inspect(_)) {
        None
    } else {
        next()
    };
    let call = Call::from_request(Request::new(id, action).unwrap());
    match call {
        Call::Flat(call) => kernel_client::memory::flat::invoke(call),
        Call::Nest(call) => kernel_client::memory::nest::invoke(call),
    }
    .unwrap_or_else(|error| panic!("{action:?}: {error:?}"))
}
pub fn mutate(action: Action) -> Value {
    let Response::Reply(reply) = call(action) else {
        panic!("reply");
    };
    let Outcome::Committed(value) = reply.outcome() else {
        panic!("{action:?}: {:?}", reply.outcome());
    };
    value
}
pub fn observe(query: Query) -> Snapshot {
    let Response::Observation(value) = call(Action::Inspect(query)) else {
        panic!("observation");
    };
    value.snapshot()
}
pub fn allocate(license: PendId, count: usize) -> SlotPosition {
    let Value::CreatedFlat { token, flat, .. } = mutate(Action::CreateFlat {
        license,
        count,
        level: CellLevel::BASE,
    }) else {
        panic!("Flat");
    };
    for page in 0..count {
        let slot = SlotPosition::new(token, flat, page).unwrap();
        let Snapshot::Slot(status) = observe(Query::Slot(slot)) else {
            panic!("Slot");
        };
        mutate(Action::Occupy {
            slot,
            expected: status.version,
        });
    }
    SlotPosition::new(token, flat, 0).unwrap()
}
pub fn delete_slot(slot: SlotPosition) {
    let Snapshot::Flat(status) = observe(Query::Flat {
        token: slot.token(),
        flat: slot.flat(),
    }) else {
        panic!("Flat");
    };
    mutate(Action::DeleteFlat {
        token: slot.token(),
        flat: slot.flat(),
        expected: status.version,
    });
    retry(|| pend::return_(slot.token()));
}
pub fn share(license: PendId, slot: SlotPosition, modifiable: Modifiable) -> PendId {
    let bytes = AccordRequest {
        src: slot.token(),
        target: AccordTarget::Caller,
        mark: Mark::NONE,
        limits: Limits {
            operations: Operations::Share(ShareRights::ASSOCIATE),
            modifiable,
            distributable: Distributable::Yes,
            scope: PendScope::Pages(PendRange::Inherit),
        },
    }
    .to_bytes();
    share_view::derive(license, addr(&bytes)).unwrap()
}
pub fn inspect(query: UnitQuery) -> UnitInfo {
    let mut bytes = [0; UnitInfo::WIDTH];
    assert!(unit::inspect(query, VirtAddr::new(bytes.as_mut_ptr().addr())).unwrap());
    UnitInfo::from_bytes(&bytes).unwrap()
}
pub fn wait_until_blocked(target: UnitTarget) {
    let end = end_time();
    while inspect(UnitQuery::Target(target)).state != UnitState::Waiting {
        assert!(room::clock() < end, "reader did not block");
        room::yield_();
    }
}
pub fn retry<E: core::fmt::Debug>(mut operation: impl FnMut() -> Result<(), E>) {
    let end = end_time();
    while let Err(error) = operation() {
        assert!(room::clock() < end, "{error:?}");
        room::yield_();
    }
}
pub fn reap(child: &Child) {
    let end = end_time();
    while !inspect(UnitQuery::Target(child.target)).reclaimed {
        assert!(room::clock() < end, "Task reclamation stalled");
        room::yield_();
    }
    unit::delete(child.target).unwrap();
    for memo in child.memos.iter().flatten().copied() {
        release(memo);
    }
    for slot in child.slots.iter().flatten().copied() {
        delete_slot(slot);
    }
    retry(|| pend::return_(child.whole));
    unit::finish(child.team).unwrap();
    retry(|| unit::delete(UnitTarget::Team(child.team)));
}
unsafe extern "C" {
    static __code_start: u8;
    static __data_start: u8;
    static __image_end: u8;
}

/// One captured image. Text is copied and retired before executable publication;
/// mutable data is copied into private Cells for each new Team.
const MAX_IMAGE_PAGES: usize = 128;
pub struct Child {
    pub team: TeamId,
    pub target: UnitTarget,
    whole: PendId,
    memos: [Option<MemoPosition>; MAX_IMAGE_PAGES],
    slots: [Option<SlotPosition>; MAX_IMAGE_PAGES],
}
pub struct Code {
    start: usize,
    text: usize,
    pages: usize,
}
impl Code {
    pub fn new() -> Self {
        let start = ptr::addr_of!(__code_start).addr();
        Self {
            start,
            text: (ptr::addr_of!(__data_start).addr() - start) / PAGE_SIZE,
            pages: (ptr::addr_of!(__image_end).addr() - start).div_ceil(PAGE_SIZE),
        }
    }
    pub fn spawn(
        &self,
        license: PendId,
        mode: Mode,
        entry: extern "C" fn(usize, usize) -> !,
    ) -> Child {
        let create = UnitCreate::Team {
            member_exit: false,
            nest: None,
            guard: ControlId::NONE,
        };
        let CreatedUnit::Team { team, nest: whole } =
            unit::create(addr(&create.to_bytes())).unwrap()
        else {
            panic!("Team");
        };
        // Each Team receives its own complete code/data image. Native Flat/Nest
        // operations enforce writer retirement before executable publication.
        assert!(self.pages <= MAX_IMAGE_PAGES);
        let mut memos = [None; MAX_IMAGE_PAGES];
        let mut slots = [None; MAX_IMAGE_PAGES];
        for page in 0..self.pages {
            let slot = allocate(license, 1);
            let (stage, address) = reserve(space(), Placement::lowest(PAGE_SIZE).unwrap(), 1, 6);
            insert(
                stage,
                Source::Flat {
                    position: slot,
                    expected: slot_version(slot),
                },
            );
            publish(stage);
            unsafe {
                ptr::copy_nonoverlapping(
                    (self.start + page * PAGE_SIZE) as *const u8,
                    address.get() as *mut u8,
                    PAGE_SIZE,
                );
            }
            release(stage);
            let access = if page < self.text { 10 } else { 6 };
            let (memo, _) = reserve(
                whole,
                Placement::exact(VirtAddr::new(self.start + page * PAGE_SIZE), PAGE_SIZE).unwrap(),
                1,
                access,
            );
            insert(
                memo,
                Source::Flat {
                    position: slot,
                    expected: slot_version(slot),
                },
            );
            publish(memo);
            slots[page] = Some(slot);
            memos[page] = Some(memo);
        }
        let args = [mode as usize];
        let request = UnitCreate::Task {
            team: Some(team),
            entry: VirtAddr::new(entry as *const () as usize),
            args: VirtAddr::new(args.as_ptr().addr()),
            count: 1,
            stack: VirtAddr::new(0),
        };
        let CreatedUnit::Task(task) = unit::create(addr(&request.to_bytes())).unwrap() else {
            panic!("Task");
        };
        grant(
            whole,
            Some(task),
            Operations::Nest(NestRights::INSPECT | NestRights::RESERVE),
            Modifiable::Yes,
        );
        Child {
            team,
            target: UnitTarget::Task(task),
            whole,
            memos,
            slots,
        }
    }
}
fn slot_version(slot: SlotPosition) -> SlotVersion {
    let Snapshot::Slot(status) = observe(Query::Slot(slot)) else {
        panic!("Slot");
    };
    status.version
}
pub fn reserve(
    token: PendId,
    placement: Placement,
    count: usize,
    access: u64,
) -> (MemoPosition, VirtAddr) {
    let Target::Nest(nest) = pend_info(token).target else {
        panic!("Nest");
    };
    let Snapshot::Nest(status) = observe(Query::Nest { token, nest }) else {
        panic!("Nest");
    };
    let Value::Reservation {
        token,
        nest,
        reservation,
        address,
        ..
    } = mutate(Action::Reserve {
        token,
        nest,
        expected: status.version,
        reservation: ReserveRequest {
            placement,
            count: PageCount::new(count).unwrap(),
            level: CellLevel::BASE,
            access,
        },
    })
    else {
        panic!("Reservation");
    };
    (
        MemoPosition::new(token, nest, reservation, 0).unwrap(),
        address,
    )
}
pub fn insert(target: MemoPosition, source: Source) {
    let Snapshot::Memo(status) = observe(Query::Memo(target)) else {
        panic!("Memo");
    };
    mutate(Action::Insert {
        target,
        source,
        expected: status.generation,
    });
}
pub fn publish(memo: MemoPosition) {
    let Snapshot::Memo(status) = observe(Query::Memo(memo)) else {
        panic!("Memo");
    };
    mutate(Action::Publish {
        memo,
        expected: status.generation,
        maintenance: None,
    });
}
pub fn release(memo: MemoPosition) {
    let Snapshot::Reservation(status) = observe(Query::Reservation {
        token: memo.token(),
        nest: memo.nest(),
        reservation: memo.reservation(),
    }) else {
        panic!("Reservation");
    };
    mutate(Action::Release {
        token: memo.token(),
        nest: memo.nest(),
        reservation: memo.reservation(),
        expected: status.version,
    });
    retry(|| pend::return_(memo.token()));
}
pub fn ring<T>(
    mut operation: impl FnMut() -> Result<T, twine::view::share::MappingError>,
) -> Result<T, twine::view::share::MappingError> {
    let end = end_time();
    loop {
        match operation() {
            Err(error) if error.is_busy() && room::clock() < end => room::yield_(),
            result => return result,
        }
    }
}
