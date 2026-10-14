#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]

extern crate alloc;

mod framebuffer;
mod gdt;
mod interrupts;
mod keyboard;
mod memory;
mod pci;
mod e1000;
mod net;
mod power;
mod process;
mod serial;
mod shell;

use core::panic::PanicInfo;

use bootloader_api::config::Mapping;
use bootloader_api::{entry_point, BootInfo, BootloaderConfig};
use x86_64::VirtAddr;

/// Bootloader configuration used by the `entry_point!` macro.
///
/// We request a fixed-address mapping of the whole physical memory so the kernel
/// can access page tables and arbitrary frames through `physical_memory_offset`.
pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::FixedAddress(0x0000_4000_0000_0000));
    config
};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    serial::init();
    if let Some(framebuffer) = boot_info.framebuffer.take() {
        framebuffer::init(framebuffer);
    }

    println!("Rust OS v0.1.1");
    println!("=================");
    println!("Booted via bootloader 0.11");

    gdt::init();
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();
    println!("[ok] GDT/TSS loaded, interrupts enabled (timer + keyboard).");

    let physical_memory_offset = VirtAddr::new(
        boot_info
            .physical_memory_offset
            .into_option()
            .expect("bootloader did not map the physical memory"),
    );
    let mut mapper = unsafe { memory::init(physical_memory_offset) };
    let mut frame_allocator =
        unsafe { memory::BootInfoFrameAllocator::init(&mut boot_info.memory_regions) };

    memory::init_heap(&mut mapper, &mut frame_allocator).expect("heap initialization failed");
    println!("[ok] Heap initialized ({} KiB).", memory::HEAP_SIZE / 1024);

    println!("[pci] Scanning PCI devices...");
    pci::scan_bus(|dev| {
        println!(
            "  - {:02x}:{:02x}.{} -> Vendor: {:04x}, Device: {:04x}, Class: {:02x}.{:02x}",
            dev.bus, dev.device, dev.function, dev.vendor_id, dev.device_id, dev.class_code, dev.subclass
        );
    });

    if let Some(e1000) = pci::find_device(0x8086, 0x100E) {
        println!("[pci] Found Intel e1000 NIC: {:02x}:{:02x}.{}", e1000.bus, e1000.device, e1000.function);
        let bar0 = e1000.read_bar(0);
        let mmio_phys = (bar0 & 0xFFFF_FFF0) as u64;
        println!("[pci] e1000 BAR0 PhysAddr = {:#010x}", mmio_phys);

        // The bootloader maps all physical memory at physical_memory_offset using huge pages.
        // We can access MMIO space directly through this offset mapping!
        let mmio_virt = physical_memory_offset + mmio_phys;
        e1000::init(&e1000, mmio_virt, physical_memory_offset);
        net::init();
    } else {
        println!("[pci] Warning: e1000 NIC (0x8086:0x100E) not found!");
    }

    process::init();
    process::spawn("idle", process::idle_task);
    process::spawn("shell", shell::run);
    process::spawn("net", net::net_task);
    process::spawn("worker", process::worker_task);
    println!(
        "[ok] {} tasks spawned; starting preemptive scheduler.",
        process::snapshot().len()
    );

    println!();
    println!("Boot complete. Preemptive multitasking active (round-robin on timer).");
    process::start();
}

#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    panic!("allocation error: {layout:?}");
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!();
    println!("RUST OS KERNEL PANIC: {info}");
    loop {
        x86_64::instructions::hlt();
    }
}
