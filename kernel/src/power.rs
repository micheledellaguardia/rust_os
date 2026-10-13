//! System power control: reboot via the 8042 keyboard controller and shutdown
//! via QEMU's `isa-debug-exit` device.

use x86_64::instructions::port::Port;

/// Triggers a CPU reset through the 8042 keyboard controller (port 0x64,
/// command 0xFE "pulse output line 0"), the standard real-mode-free way to
/// reboot an x86 machine.
pub fn reboot() -> ! {
    x86_64::instructions::interrupts::disable();
    unsafe {
        let mut status_port: Port<u8> = Port::new(0x64);
        let mut cmd_port: Port<u8> = Port::new(0x64);
        // Wait until the controller's input buffer is empty before pulsing.
        while status_port.read() & 0x02 != 0 {}
        cmd_port.write(0xFEu8);
    }
    loop {
        x86_64::instructions::hlt();
    }
}

/// Shuts the machine down. Relies on the `isa-debug-exit` device registered by
/// the runner (`-device isa-debug-exit,iobase=0xf4,iosize=0x04`): writing to
/// it makes QEMU exit the process, which is the closest equivalent to a power
/// off for this bare-metal, bootloader-only kernel.
pub fn shutdown() -> ! {
    x86_64::instructions::interrupts::disable();
    unsafe {
        let mut port: Port<u32> = Port::new(0xf4);
        port.write(0x10u32);
    }
    // If QEMU is ever run without isa-debug-exit, just halt forever.
    loop {
        x86_64::instructions::hlt();
    }
}
