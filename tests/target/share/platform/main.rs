#![no_std]
#![no_main]
//! Deliberately narrow, RAM-only platform paired with run.py's QEMU machine.
extern crate alloc;
use kernel::entry::boot::{
    self,
    memory::topology::{Fabric, Ram, Topology},
};
use kernel_api::boot::platform::{
    Memory, Region, Resources,
    riscv::{ClockAccess, Cpu, FirmwareId, Harts, Paging, Platform, devices::Devices},
};
use spin::Once;
static MEMORY: Once<Memory> = Once::new();
static HARTS: Once<Harts> = Once::new();

fn property(tree: &fdt::Fdt<'_>, name: &str) -> usize {
    let chosen = tree.find_node("/chosen").unwrap();
    let value = chosen.property(name).unwrap().value;
    match value.len() {
        4 => u32::from_be_bytes(value.try_into().unwrap()) as usize,
        8 => u64::from_be_bytes(value.try_into().unwrap()) as usize,
        _ => panic!("invalid chosen property"),
    }
}

#[unsafe(no_mangle)]
extern "C" fn main(hart: usize, dtp: usize) -> ! {
    // SAFETY: The paired runner supplies QEMU's immutable DTB in guest RAM.
    let tree = unsafe { fdt::Fdt::from_ptr(dtp as *const u8) }.unwrap();
    assert!(
        tree.root()
            .compatible()
            .all()
            .any(|v| matches!(v, "riscv-virtio,qemu" | "riscv-virtio"))
    );
    assert_eq!(hart, 0);
    assert_eq!(tree.cpus().count(), 1);
    let memory_node = tree.memory();
    let mut regions = memory_node.regions();
    let ram = regions.next().unwrap();
    assert!(regions.next().is_none());
    assert_eq!(ram.starting_address as usize, 0x80000000);
    assert_eq!(ram.size, Some(128 * 1024 * 1024));
    let memory = MEMORY.call_once(|| {
        let start = property(&tree, "linux,initrd-start");
        let end = property(&tree, "linux,initrd-end");
        Memory::new(
            &[Region::new(0x80000000, 128 * 1024 * 1024)],
            &[Region::new(0x80000000, 0x200000)],
            &[],
            Region::new(dtp, tree.total_size()),
            boot::image_region(),
            Some(Region::new(
                start,
                end.checked_sub(start).unwrap().next_multiple_of(4096),
            )),
        )
        .unwrap()
    });
    let harts = HARTS.call_once(|| {
        Harts::new(
            &[Cpu {
                id: FirmwareId::new(0),
                paging: Paging::Sv39,
                floating: true,
                vector: false,
                sstc: false,
            }],
            FirmwareId::new(0),
            u32::from_be_bytes(
                tree.find_node("/cpus")
                    .unwrap()
                    .property("timebase-frequency")
                    .unwrap()
                    .value
                    .try_into()
                    .unwrap(),
            ) as usize,
        )
        .unwrap()
    });
    // SAFETY: This fixture is restricted to the paired single-Hart RV64GC
    // QEMU machine. Memory::new checked the image/DTB/initrd reservations.
    let result = unsafe {
        boot::init(Platform {
            hart: harts,
            memory,
            clock: ClockAccess::default(),
        })
    }
    .and_then(|_| {
        // No MMIO authority, device bindings or external interrupts exist.
        unsafe {
            boot::configure(Devices {
                resources: Resources {
                    authorities: &[],
                    protected: &[],
                    external: &[],
                },
                controllers: &[],
                interrupt: None,
            })
        }
        .unwrap();
        let ram = memory
            .identity()
            .iter()
            .map(|r| Ram {
                base: r.base as u64,
                bytes: r.size as u64,
            })
            .collect();
        // SAFETY: One TCG Hart and ordinary coherent RAM; no DMA devices or
        // passthrough. Kernel rejects SMP and fences executable publication.
        let topology = unsafe {
            Topology::new(alloc::vec![Fabric {
                ram,
                routes: alloc::vec![]
            }])
            .map(|topology| topology.with_local_instructions())
        }
        .unwrap();
        boot::qualify_memory(topology).unwrap();
        boot::start()
    });
    match result {
        Ok(()) => boot::run(),
        Err(error) => boot::fail(error),
    }
}
