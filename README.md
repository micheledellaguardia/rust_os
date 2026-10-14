# RustOS

A small, educational operating system kernel written in Rust, built on a pinned
nightly toolchain and booted from a BIOS image in QEMU.

rust_os runs entirely bare-metal (no OS underneath): it sets up its own GDT and
TSS, installs an interrupt descriptor table, drives the PIT timer, PS/2 keyboard,
and COM1 serial port, manages its own heap with a frame allocator, and runs several
kernel tasks with real preemptive multitasking. It features PCI enumeration, an
Intel e1000 Gigabit network driver, a TCP/IP stack (`smoltcp`), and a minimal
built-in HTTP web server.

## Features

- **BIOS Boot**: Via the [bootloader](https://docs.rs/bootloader/0.11.10/bootloader/index.html) crate;
  physical memory mapped at a fixed virtual address offset using huge pages.
- **Dual Console Output**: Framebuffer console with a built-in 8x8 bitmap font and an interactive 16550 UART serial console (both accessible via `print!`/`println!`).
- **Segmentation & Protection**: Global Descriptor Table (GDT) with Task State Segment (TSS), dedicated double-fault interrupt stack (IST), kernel data segment, and reload of `SS`/`DS`/`ES`.
- **Interrupt Handling (IDT)**:
  - 8259 PIC initialization and mapping,
  - PIT timer (preemptive scheduler tick),
  - PS/2 keyboard ring buffer,
  - Page-fault and double-fault exception handlers with diagnostic panic reporting.
- **Memory Management**: 4 MiB kernel heap backed by `linked_list_allocator` and a `BootInfo` frame allocator, plus page-table physical address translation for DMA buffers.
- **Preemptive Multitasking**: Round-robin scheduler where every task owns a dedicated 16 KiB kernel stack. The naked timer ISR saves registers, switches stacks, and returns via `iretq`.
- **PCI Bus Enumeration**: Mechanism #1 PCI bus scanning (`0xCF8` / `0xCFC`) for device discovery and BAR probing.
- **Intel e1000 NIC Driver**: Gigabit Ethernet driver (`82540EM`) with EEPROM MAC address reading, circular DMA transmit/receive descriptor rings, and MMIO register control.
- **TCP/IP Network Stack**: Embedded `smoltcp` integration handling Ethernet frames, ARP, IPv4 routing, and TCP state machine.
- **Minimal HTTP Web Server**: Background kernel task listening on port 80 (forwarded to host `8080`) serving web requests.
- **Interactive Shell**: Scheduled task supporting `help`, `echo`, `clear`, `info`, `pci`, `netinfo`, and `tasks`.

## Architecture

The kernel sources live in `kernel/src/`:

| Module        | Responsibility                                                          |
| ------------- | ----------------------------------------------------------------------- |
| `main.rs`     | Kernel entry point, boot info setup, hardware init and task startup.    |
| `pci.rs`      | PCI Configuration Mechanism #1 access, bus scan, and device detection.  |
| `e1000.rs`    | Intel 82540EM NIC driver: MMIO, EEPROM, and DMA TX/RX descriptor rings. |
| `net.rs`      | `smoltcp` Device adapter, IP configuration, and minimal HTTP server.    |
| `serial.rs`   | Initialization and bidirectional I/O for 16550 COM1 serial port.        |
| `framebuffer` | Bootloader framebuffer console, 8x8 font rendering, and screen scroll.  |
| `gdt`         | Global Descriptor Table, TSS, and IST double-fault stack.               |
| `interrupts`  | IDT, 8259 PIC controllers, PIT timer ticks, and exception handlers.    |
| `keyboard`    | PS/2 keyboard driver decoding scancodes into an input ring buffer.      |
| `memory`      | Physical frame allocator, 4 MiB kernel heap, and address translation.   |
| `process`     | Per-task stacks, naked timer ISR, and preemptive round-robin scheduler. |
| `shell`       | Interactive command-line shell task.                                    |

### Multitasking

Each task has its own 16 KiB stack. The timer interrupt is handled by a naked
ISR (`timer_isr`) which pushes general-purpose registers onto the current task's
stack, calls `schedule` to pick the next runnable task, and restores the next task's
registers via `iretq`. Because preemption happens on the timer interrupt, tasks
do not need to cooperate to yield the CPU.

## Requirements

- Rust toolchain from `rust-toolchain.toml` (pinned nightly `nightly-2025-02-01`; `rustup` installs it automatically).
- [QEMU](https://www.qemu.org) (`qemu-system-x86_64`) with networking support.
- A Unix-like environment with a C linker for the `bootloader` build script.

## Build and Run

```sh
./build.sh                 # compile the kernel and assemble bootable images
./run.sh                   # build and boot in QEMU with a graphical window
./run.sh -display none     # headless boot, console output directly on the terminal
```

QEMU is preconfigured with user-mode networking and port forwarding:
- Guest IP: `10.0.2.15`
- Guest Gateway: `10.0.2.2`
- Host forwarding: Host port `8080` $\rightarrow$ Guest port `80`

### Accessing the Web Server

While `rust_os` is running in QEMU, open a browser or run `curl` on your host machine:

```sh
curl http://localhost:8080/
```

Response:
```html
<!DOCTYPE html><html><body><h1>Hello from RustOS bare-metal minimal web server!</h1></body></html>
```

## Using the Shell

The interactive shell runs as a scheduled task. At the `>` prompt:

```text
help          - show available commands
echo <text>   - print text
clear         - clear the screen
info          - system uptime, memory, and scheduled tasks count
pci           - list all discovered PCI devices
netinfo       - display network card MAC address and packet statistics
tasks         - list scheduled tasks and run counts
```

## Project Layout

```
Cargo.toml            build harness (bootloader crate + kernel artifact)
kernel/               the kernel itself
  src/                kernel source code
  Cargo.toml          kernel dependencies (x86_64, smoltcp, spin, etc.)
build.sh              build the kernel and disk image
run.sh                build and boot in QEMU with port forwarding
rust-toolchain.toml   pinned nightly toolchain
```

## License

MIT.