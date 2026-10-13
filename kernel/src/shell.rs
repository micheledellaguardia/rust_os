//! A minimal interactive shell that runs as a scheduled task.
//!
//! Input comes from the PS/2 keyboard ring buffer (see [`crate::keyboard`]) and
//! output goes to both the framebuffer console and the serial port via the
//! `print!`/`println!` macros. The task only yields the CPU through `hlt` while
//! waiting for a key, so the rest of the system keeps running.

use alloc::vec::Vec;

use crate::{keyboard, memory, print, println, process};

const MAX_LINE: usize = 128;

pub fn run() -> ! {
    println!("[task] shell: entered");
    println!("Type 'help' for a list of commands.");

    let mut line: Vec<u8> = Vec::new();
    loop {
        print!("> ");
        line.clear();

        loop {
            match wait_for_key() {
                b'\n' => {
                    println!();
                    break;
                }
                0x08 => {
                    if line.pop().is_some() {
                        // Move back, overwrite with a space, move back again.
                        print!("\x08 \x08");
                    }
                }
                byte if (byte.is_ascii_graphic() || byte == b' ') && line.len() < MAX_LINE => {
                    line.push(byte);
                    print!("{}", byte as char);
                }
                _ => {}
            }
        }

        execute(&line);
    }
}

/// Blocks (halting the CPU) until a decoded key is available.
fn wait_for_key() -> u8 {
    loop {
        if let Some(byte) = keyboard::read_char() {
            return byte;
        }
        x86_64::instructions::hlt();
    }
}

fn execute(line: &[u8]) {
    let input = core::str::from_utf8(line).unwrap_or("");
    let mut parts = input.splitn(2, ' ');
    let command = parts.next().unwrap_or("");
    let args = parts.next().unwrap_or("").trim();

    match command {
        "" => {}
        "help" => {
            println!("commands:");
            println!("  help          show this help");
            println!("  echo <text>   print <text>");
            println!("  clear         clear the screen");
            println!("  info          memory and uptime information");
            println!("  pci           list discovered PCI devices");
            println!("  tasks         list scheduled tasks");
        }
        "pci" => {
            crate::pci::scan_bus(|dev| {
                println!(
                    "  {:02x}:{:02x}.{} -> Vendor: {:04x}, Device: {:04x}, Class: {:02x}.{:02x}",
                    dev.bus, dev.device, dev.function, dev.vendor_id, dev.device_id, dev.class_code, dev.subclass
                );
            });
        }
        "echo" => println!("{args}"),
        "clear" => crate::framebuffer::clear(),
        "info" => {
            println!(
                "uptime      : {} timer ticks",
                crate::interrupts::timer_ticks()
            );
            println!("kernel heap : {} KiB", memory::HEAP_SIZE / 1024);
            println!("tasks       : {}", process::snapshot().len());
        }
        "tasks" => {
            for task in process::snapshot() {
                println!("  #{} {:<10} runs={}", task.id, task.name, task.ticks);
            }
        }
        other => println!("unknown command: {other} (try 'help')"),
    }
}
