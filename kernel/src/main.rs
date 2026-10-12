#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![feature(naked_functions)]

extern crate alloc;

mod framebuffer;
mod gdt;
mod interrupts;
mod keyboard;
mod memory;
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

    println!("rust_os v0.1.0");
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

    process::init();
    process::spawn("idle", process::idle_task);
    process::spawn("shell", shell::run);
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
    println!("KERNEL PANIC: {info}");
    loop {
        x86_64::instructions::hlt();
    }
}
