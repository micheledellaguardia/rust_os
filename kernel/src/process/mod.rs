//! A small preemptive round-robin scheduler with one kernel stack per task.
//!
//! The timer interrupt is handled by a naked ISR (`timer_isr`): it pushes the
//! general purpose registers onto the current task's stack, calls `schedule` to
//! obtain the stack pointer of the next task, then restores that task's
//! registers and returns from the interrupt. Because every task owns its own
//! stack, this is real preemptive multitasking (the switch can happen at any
//! timer tick, not only when a task yields).

use alloc::alloc::{alloc_zeroed, Layout};
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::arch::naked_asm;

use spin::Mutex;
use x86_64::instructions::interrupts;

use crate::interrupts::{InterruptIndex, PICS};

/// Size of every task kernel stack, in bytes.
const STACK_SIZE: usize = 16 * 1024;

/// Number of 8-byte words in an initial context:
/// 15 general-purpose registers + RIP + CS + RFLAGS + RSP + SS.
const INITIAL_FRAME_WORDS: usize = 20;

/// A 16-byte aligned task stack.
#[repr(align(16))]
struct Stack([u8; STACK_SIZE]);

pub struct Task {
    pub id: usize,
    pub name: String,
    /// How many times the scheduler switched away from this task.
    pub ticks: u64,
    /// Owns the task's stack memory.
    _stack: Box<Stack>,
    /// Saved stack pointer, pointing at the task's saved `r15`.
    rsp: u64,
}

impl Task {
    fn new(id: usize, name: &str, entry: fn() -> !) -> Self {
        let layout = Layout::new::<Stack>();
        // SAFETY: `layout` has a non-zero size, so the allocation is valid. We
        // build the `Box` from the raw pointer to avoid materialising a 16 KiB
        // array on the (small) kernel stack.
        let mut stack = unsafe { Box::from_raw(alloc_zeroed(layout) as *mut Stack) };
        let rsp = unsafe { init_stack(&mut stack.0, entry) };
        Task {
            id,
            name: name.to_string(),
            ticks: 0,
            _stack: stack,
            rsp,
        }
    }
}

/// Builds the initial register/interrupt frame at the top of `stack` so that
/// the switch code drops straight into `entry`.
///
/// The `iretq` instruction pops five 64-bit words from the stack in order:
///   RIP, CS, RFLAGS, RSP, SS
/// We therefore need all five fields.  The new task's RSP is set to the very
/// top of its stack allocation (after the frame), and SS is the kernel data
/// segment selector (index 2 → 0x10).
///
/// # Safety
/// `stack` must be at least `INITIAL_FRAME_WORDS * 8` bytes long.
unsafe fn init_stack(stack: &mut [u8], entry: fn() -> !) -> u64 {
    let top = stack.as_mut_ptr().add(stack.len()) as usize;
    // Align the frame base to 16 bytes.
    let base = (top - INITIAL_FRAME_WORDS * 8) & !0xF;
    let frame = base as *mut u64;

    // 15 zeroed general-purpose registers, `r15` at the lowest address.
    for index in 0..15 {
        frame.add(index).write(0);
    }
    // `iretq` frame (5 fields).
    frame.add(15).write(entry as usize as u64); // RIP
    frame.add(16).write(0x08); // CS  – kernel code segment (GDT index 1)
    frame.add(17).write(0x202); // RFLAGS – IF set
    frame.add(18).write(top as u64); // RSP – top of this task's stack
    frame.add(19).write(0x10); // SS  – kernel data segment (GDT index 2)

    base as u64
}

struct Scheduler {
    tasks: Vec<Task>,
    current: usize,
    next_id: usize,
}

impl Scheduler {
    const fn new() -> Self {
        Scheduler {
            tasks: Vec::new(),
            current: 0,
            next_id: 0,
        }
    }

    fn spawn(&mut self, name: &str, entry: fn() -> !) {
        let id = self.next_id;
        self.next_id += 1;
        self.tasks.push(Task::new(id, name, entry));
    }

    /// Saves `current_rsp` for the running task and returns the stack pointer of
    /// the next task to run.
    fn schedule(&mut self, current_rsp: u64) -> u64 {
        let count = self.tasks.len();
        if count == 0 {
            return current_rsp;
        }

        if let Some(task) = self.tasks.get_mut(self.current) {
            task.rsp = current_rsp;
            task.ticks += 1;
        }

        self.current = (self.current + 1) % count;
        self.tasks[self.current].rsp
    }
}

static SCHEDULER: Mutex<Option<Scheduler>> = Mutex::new(None);

/// Called from the naked timer ISR. Must not allocate.
extern "C" fn schedule(current_rsp: u64) -> u64 {
    crate::interrupts::register_timer_tick();
    // SAFETY: we are in the timer interrupt handler, so the PIC owns the EOI.
    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }

    // Perform the round-robin switch.  The lock is always available here
    // because the timer ISR is not reentrant.
    if let Some(scheduler) = SCHEDULER.lock().as_mut() {
        scheduler.schedule(current_rsp)
    } else {
        current_rsp
    }
}

/// Naked timer interrupt handler. Saves all general purpose registers, performs
/// the context switch, restores the next task's registers and returns from the
/// interrupt.
#[unsafe(naked)]
pub extern "C" fn timer_isr() {
    naked_asm!(
        "push rax",
        "push rcx",
        "push rdx",
        "push rbx",
        "push rbp",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        // First argument to `schedule` is the current (unmodified) RSP.
        "mov rdi, rsp",
        // Align the stack to 16 bytes for the call ABI, saving the real RSP
        // in rbx (callee-saved) so we can restore it afterwards.
        "mov rbx, rsp",
        "and rsp, -16",
        "call {schedule}",
        // `rax` now holds the next task's RSP. Switch to it.
        "mov rsp, rax",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rbp",
        "pop rbx",
        "pop rdx",
        "pop rcx",
        "pop rax",
        "iretq",
        schedule = sym schedule,
    );
}

/// Enters a task for the first time by installing `rsp` and performing the same
/// restore-and-return sequence as `timer_isr`.
#[unsafe(naked)]
unsafe extern "C" fn restore_asm(rsp: u64) -> ! {
    naked_asm!(
        "mov rsp, rdi",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rbp",
        "pop rbx",
        "pop rdx",
        "pop rcx",
        "pop rax",
        "iretq",
    );
}

/// Debug wrapper around the naked restore helper, so we can observe the value
/// actually reaching the switch code.
#[inline(never)]
unsafe extern "C" fn switch_to(rsp: u64) -> ! {
    crate::println!("[sched] switch_to got rsp={rsp:#x}");
    // SAFETY: caller guarantees a valid initial interrupt frame at `rsp`.
    unsafe { restore_asm(rsp) }
}

/// Initialises an empty scheduler and disables interrupts until [`start`] hands
/// control to the first task.
pub fn init() {
    interrupts::disable();
    *SCHEDULER.lock() = Some(Scheduler::new());
}

/// Adds a task to the scheduler.
pub fn spawn(name: &str, entry: fn() -> !) {
    interrupts::without_interrupts(|| {
        if let Some(scheduler) = SCHEDULER.lock().as_mut() {
            scheduler.spawn(name, entry);
        }
    });
}

/// A snapshot of the scheduled tasks, safe to call from a running task.
pub fn snapshot() -> Vec<TaskInfo> {
    interrupts::without_interrupts(|| {
        SCHEDULER
            .lock()
            .as_ref()
            .map(|scheduler| {
                scheduler
                    .tasks
                    .iter()
                    .map(|task| TaskInfo {
                        id: task.id,
                        name: task.name.clone(),
                        ticks: task.ticks,
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// Starts preemptive multitasking. Never returns: control is handed to the
/// first task via its interrupt frame, which also re-enables interrupts.
pub fn start() -> ! {
    interrupts::disable();
    let rsp = {
        let mut guard = SCHEDULER.lock();
        let scheduler = guard.as_mut().expect("scheduler was not initialised");
        assert!(scheduler.tasks.len() > 0, "no tasks were spawned");
        crate::println!(
            "[sched] boot: {} tasks, initial rsp {:#x}",
            scheduler.tasks.len(),
            scheduler.tasks[0].rsp
        );
        let frame = scheduler.tasks[0].rsp as usize as *const u64;
        crate::print!("[sched] frame:");
        for i in 0..20 {
            crate::print!(" {:x}", unsafe { frame.add(i).read() });
        }
        crate::println!();
        scheduler.tasks[0].rsp
    };
    // SAFETY: `rsp` points at a valid initial interrupt frame.
    unsafe { switch_to(rsp) }
}

/// Information about a task, copied out for reporting.
pub struct TaskInfo {
    pub id: usize,
    pub name: String,
    pub ticks: u64,
}

/// The idle task: does nothing until the next interrupt.
pub fn idle_task() -> ! {
    loop {}
}

/// A background task used to demonstrate preemption. It only yields the CPU so
/// that the scheduler keeps giving it turns (visible via the `tasks` command).
pub fn worker_task() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}
