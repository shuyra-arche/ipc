use core::{fmt, ptr};
use kernel_api::{
    address::{PAGE_SIZE, PageCount, VirtAddr},
    model::{
        cell::{
            Allocation, CellAssociation, CellInfo, CellLevel, CellObject, CellPosition, Placement,
            SlotId,
        },
        identity::{Mark, PendId, TaskId, TeamId},
        modifiable::Modifiable,
        pend::{
            AccordRequest, AccordTarget, Distributable, Limits, NarrowRequest, Operations,
            PendInfo, PendRange, PendScope, Target,
        },
        unit::{
            ControlId, CreatedUnit, ExitScope, UnitCreate, UnitInfo, UnitQuery, UnitState,
            UnitTarget,
        },
        wait::Wait,
    },
};
use kernel_client::call::{cell, debug, nest, pend, room, share_view, unit};

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
            let size = bytes.len().min(256);
            let written = debug::put(addr(bytes), size).map_err(|_| fmt::Error)?;
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
    VirtAddr::new(bytes.as_ptr() as usize)
}

pub fn license() -> PendId {
    let mut after = PendId::NONE;
    loop {
        let mut words = [0; PendInfo::WORDS];
        let address = VirtAddr::new(words.as_mut_ptr() as usize);
        assert_eq!(pend::collect(after, address, 1).unwrap(), 1);
        let info = PendInfo::from_words(words).unwrap();
        if info.target == Target::None {
            return info.token;
        }
        after = info.token;
    }
}

pub fn collect<const N: usize>() -> [PendId; N] {
    let mut words = [[0; PendInfo::WORDS]; N];
    let address = VirtAddr::new(words.as_mut_ptr() as usize);
    assert_eq!(pend::collect(PendId::NONE, address, N).unwrap(), N);
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
                _ => PendScope::Object,
            },
        },
    };
    pend::accord(addr(&request.to_bytes())).unwrap()
}

pub fn allocate(license: PendId, count: usize) -> SlotId {
    cell::create(
        license,
        Allocation::new(count * PAGE_SIZE, PAGE_SIZE).unwrap(),
    )
    .unwrap()
}

pub fn delete_slot(slot: SlotId) {
    cell::delete(slot).unwrap();
}

pub fn inspect(query: UnitQuery) -> UnitInfo {
    let mut bytes = [0; UnitInfo::WIDTH];
    assert!(unit::inspect(query, VirtAddr::new(bytes.as_mut_ptr() as usize)).unwrap());
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

pub fn reap(target: UnitTarget, team: TeamId, whole: PendId, mapping: PendId) {
    let end = end_time();
    while !inspect(UnitQuery::Target(target)).reclaimed {
        assert!(room::clock() < end, "Task reclamation stalled");
        room::yield_();
    }
    unit::delete(target).unwrap();
    retry(|| pend::return_(mapping));
    retry(|| pend::return_(whole));
    unit::finish(team).unwrap();
    retry(|| unit::delete(UnitTarget::Team(team)));
}

unsafe extern "C" {
    static __code_start: u8;
    static __code_end: u8;
}

/// Load the same read-only code and constants into each independent Team.
/// The worker has no mutable image state; its stack and Share mappings are
/// allocated separately by the Kernel.
pub struct Code {
    slot: SlotId,
    token: PendId,
    start: VirtAddr,
    pages: usize,
}

impl Code {
    pub fn new(license: PendId) -> Self {
        let start = ptr::addr_of!(__code_start);
        let len = ptr::addr_of!(__code_end).addr() - start.addr();
        let pages = len.div_ceil(PAGE_SIZE);
        let slot = allocate(license, pages);
        let token = share_view::create(license, slot, Modifiable::Yes).unwrap();
        let (address, span) = share_view::open(token).unwrap();
        assert!(span >= len);
        unsafe { ptr::copy_nonoverlapping(start, address.get() as *mut u8, len) };
        retry(|| share_view::shut(token));
        let mut words = [0; PendInfo::WORDS];
        pend::inspect(token, VirtAddr::new(words.as_mut_ptr() as usize)).unwrap();
        let mut limits = PendInfo::from_words(words).unwrap().limits;
        limits.modifiable = Modifiable::No;
        let narrow = NarrowRequest { token, limits };
        pend::narrow(addr(&narrow.to_bytes())).unwrap();
        Self {
            slot,
            token,
            start: VirtAddr::new(start.addr()),
            pages,
        }
    }

    pub fn spawn(
        &self,
        mode: Mode,
        entry: extern "C" fn(usize, usize) -> !,
    ) -> (TeamId, UnitTarget, PendId, PendId) {
        self.spawn_args(entry, &[mode as usize])
    }

    pub fn spawn_args(
        &self,
        entry: extern "C" fn(usize, usize) -> !,
        args: &[usize],
    ) -> (TeamId, UnitTarget, PendId, PendId) {
        let create = UnitCreate::Team {
            member_exit: false,
            nest: None,
            guard: ControlId::NONE,
        };
        let CreatedUnit::Team { team, nest: whole } =
            unit::create(addr(&create.to_bytes())).unwrap()
        else {
            panic!("expected Team")
        };
        let request = kernel_api::model::nest::ReserveRequest {
            placement: Placement::exact(self.start, PAGE_SIZE).unwrap(),
            count: PageCount::new(self.pages).unwrap(),
            level: CellLevel::BASE,
            access: 10,
        };
        let (token, address) = nest::reserve(
            whole,
            request.placement,
            request.count,
            request.level,
            request.access,
        )
        .unwrap();
        assert_eq!(address, self.start);
        for page in 0..self.pages {
            let mut position = CellPosition {
                object: CellObject::Pend(token),
                page,
                generation: 0,
            };
            let mut info = [0; CellInfo::WIDTH];
            cell::inspect(
                addr(&position.to_bytes()),
                VirtAddr::new(info.as_mut_ptr() as usize),
            )
            .unwrap();
            position.generation = CellInfo::from_bytes(&info).unwrap().generation;
            cell::insert(addr(
                &CellAssociation {
                    source: CellPosition {
                        object: CellObject::Pend(self.token),
                        page,
                        generation: 0,
                    },
                    target: position,
                }
                .to_bytes(),
            ))
            .unwrap();
            cell::inspect(
                addr(&position.to_bytes()),
                VirtAddr::new(info.as_mut_ptr() as usize),
            )
            .unwrap();
            position.generation = CellInfo::from_bytes(&info).unwrap().generation;
            cell::publish(addr(&position.to_bytes())).unwrap();
        }
        let request = UnitCreate::Task {
            team: Some(team),
            entry: VirtAddr::new(entry as *const () as usize),
            args: VirtAddr::new(args.as_ptr() as usize),
            count: args.len() as u64,
            stack: VirtAddr::new(0),
        };
        let CreatedUnit::Task(task) = unit::create(addr(&request.to_bytes())).unwrap() else {
            panic!("expected Task")
        };
        (team, UnitTarget::Task(task), whole, token)
    }

    pub fn finish(self) {
        retry(|| pend::return_(self.token));
        delete_slot(self.slot);
    }
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
