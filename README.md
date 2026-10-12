# rust_os

A small, educational operating system kernel written in Rust, built on a pinned
nightly toolchain and booted from a BIOS image in QEMU.

rust_os runs entirely bare-metal (no OS underneath): it sets up its own GDT and
TSS, installs an interrupt descriptor table, drives the PIT timer and the PS/2
keyboard directly, manages its own heap with a frame allocator, and runs several
kernel tasks with real preemptive multitasking. A tiny interactive shell greets
you on the framebuffer console and the serial port.

## Features

- BIOS boot via the
  [bootloader](https://docs.rs/bootloader/0.11.10/bootloader/index.html) crate;
  physical memory mapped at a fixed virtual address.
- Framebuffer console with a built-in 8x8 font, plus a separate serial console
  (both go through the same `print!`/`println!` macros).
- Global Descriptor Table with a TSS, a kernel data segment, and full reload of
  `SS`/`DS`/`ES`.
- Interrupt-related things handled through a custom IDT:
  - PIT timer (round-robin scheduler tick),
  - PS/2 keyboard ring buffer,
  - page-fault and double-fault handlers with panic reporting.
- Heap allocator backed by a linked-list allocator and a `BootInfo` frame
  allocator.
- Preemptive, round-robin scheduler: every task owns a dedicated 16 KiB kernel
  stack, and the timer ISR performs a real context switch on every tick.
- Interactive shell running as a scheduled task, with commands such as `help`,
  `echo`, `clear`, `info`, and `tasks`.

## Architecture

The kernel lives in `kernel/src/`:

| Module        | Responsibility                                                  |
| ------------- | --------------------------------------------------------------- |
| `main.rs`     | Entry point, boot info setup, module wiring and boot sequence.  |
| `serial.rs`   | Initialization for the 16550-style COM1 serial port.            |
| `framebuffer` | Bootloader framebuffer, text rasterization and `clear`.         |
| `gdt`         | GDT with TSS and kernel data segment.                           |
| `interrupts`  | IDT, PIT timer, keyboard queue, page-fault/double-fault.        |
| `keyboard`    | PS/2 keyboard driver feeding an in-memory input ring buffer.    |
| `memory`      | Page-frame allocator and heap initialization.                   |
| `process`     | Task structs, per-task stacks, preemptive scheduler and tasks.  |
| `shell`       | The interactive shell task.                                     |

### Multitasking

Each task has its own 16 KiB stack. The timer interrupt is handled by a naked
ISR which saves the general-purpose registers on the current task's stack, asks
the scheduler for the next runnable task, and restores that task's registers via
`iretq`. Because the switch happens inside the timer handler, the CPU is taken
from a task at any tick, not only when the task cooperates.

## Requirements

- Rust toolchain from `rust-toolchain.toml` (pinned nightly,
  `nightly-2025-02-01`; `rustup` installs it automatically).
- [QEMU](https://www.qemu.org) for running the image.
- A Unix-like environment with a C linker for the `bootloader` build script.

## Build and run

```sh
./build.sh                 # compile the kernel and assemble bootable images
./run.sh                   # build and boot in QEMU with a graphical window
./run.sh -display none     # headless boot, console output on the terminal
```

The toolchain pin is honored through `RUSTUP_TOOLCHAIN`, so there is no need to
manage the nightly version manually.

## Using the shell

Once booted, the shell prints a `>` prompt. Type a command and press Enter:

```
help
echo hello world
clear
info
tasks
```

## Project layout

```
Cargo.toml            build harness (bootloader crate + kernel artifact)
kernel/               the kernel itself
  src/                kernel sources (see the table above)
  Cargo.toml          kernel dependencies
build.sh              build the kernel and images
run.sh                build and boot in QEMU
rust-toolchain.toml   pinned nightly toolchain
```

## Status

Educational project under active development. The kernel boots, initializes its
hardware and heap, and starts the preemptive scheduler; debugging of the first
context switch is currently in progress.

## License

MIT.