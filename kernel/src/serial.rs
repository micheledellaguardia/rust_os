//! Tiny 16550 UART driver for COM1, used to mirror kernel output to a serial
//! console (e.g. `qemu -serial stdio`) which makes the kernel testable headless.

use core::fmt;

use spin::Mutex;
use x86_64::instructions::port::Port;

const COM1: u16 = 0x3F8;

pub fn init() {
    unsafe {
        // Disable interrupts while configuring.
        Port::<u8>::new(COM1 + 1).write(0x00);
        // Enable DLAB to set the baud rate divisor.
        Port::<u8>::new(COM1 + 3).write(0x80);
        Port::<u8>::new(COM1).write(0x03); // divisor low  -> 38400 baud
        Port::<u8>::new(COM1 + 1).write(0x00); // divisor high
        // 8 bits, no parity, one stop bit.
        Port::<u8>::new(COM1 + 3).write(0x03);
        // Enable and clear FIFOs, 14-byte threshold.
        Port::<u8>::new(COM1 + 2).write(0xC7);
        // IRQs enabled, RTS/DSR set.
        Port::<u8>::new(COM1 + 4).write(0x0B);
    }
}

fn write_byte(byte: u8) {
    unsafe {
        // Wait until the transmit holding register is empty (bit 5).
        while Port::<u8>::new(COM1 + 5).read() & 0x20 == 0 {}
        Port::<u8>::new(COM1).write(byte);
    }
}

/// Reads a byte from COM1 if available (bit 0 of Line Status Register).
pub fn read_byte() -> Option<u8> {
    unsafe {
        if Port::<u8>::new(COM1 + 5).read() & 0x01 != 0 {
            Some(Port::<u8>::new(COM1).read())
        } else {
            None
        }
    }
}

static SERIAL: Mutex<()> = Mutex::new(());

pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;

    struct SerialWriter;
    impl fmt::Write for SerialWriter {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            for byte in s.bytes() {
                if byte == b'\n' {
                    write_byte(b'\r');
                }
                write_byte(byte);
            }
            Ok(())
        }
    }

    let _guard = SERIAL.lock();
    let _ = SerialWriter.write_fmt(args);
}
