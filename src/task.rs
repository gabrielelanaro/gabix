//! Task abstraction: data structures + constructor + context switch.
//!
//! - Slice 3.1: `Task`, `TaskState`, `TaskContext`, `Task::new`.
//! - Slice 3.2: `switch()` (the actual register swap) and a minimal
//!   cooperative scheduler (`spawn`, `yield_now`).
//!
//! See `docs/context-switch.html` for the visual walkthrough of how
//! the saved state and the switch work.

use crate::frame_allocator;
use crate::memory;
use crate::println;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::arch::naked_asm;
use lazy_static::lazy_static;
use spin::Mutex;
use x86_64::structures::paging::PageTableFlags;
use x86_64::{PhysAddr, VirtAddr};

/// Size of each task's kernel stack: 64 KB.
pub const STACK_SIZE: u64 = 64 * 1024;

/// Virtual address where task stacks begin. Each new task gets a 64 KB
/// region at `TASK_STACK_BASE + id * STACK_SIZE`. Picked to be far from
/// the kernel image, the heap (`0x4444_4444_0000`), and the bootloader's
/// physical-memory-offset mapping (`0x18000000000`).
const TASK_STACK_BASE: u64 = 0x5000_0000_0000;

/// Task states for the scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// Paused but ready to run, waiting for its turn.
    Ready,
    /// Currently executing on the CPU.
    Running,
    /// Waiting for something (sleep, I/O). Not yet used.
    Blocked,
    /// Exited but not yet reaped by a parent via `wait`. The task struct
    /// stays around so the parent can read its exit status. Reaped by
    /// `wait` -> Dead.
    Zombie,
    /// Fully done — can be cleaned up.
    Dead,
}

/// Saved register context for a paused task.
///
/// Just RSP — the actual register values (RBX, RBP, R12-R15) live on the
/// paused task's stack, right above this address. When we resume the task,
/// we load this RSP into the CPU's RSP, then pop the registers off the stack.
///
/// See `docs/context-switch.html` for why this single field is enough.
#[derive(Debug, Clone, Copy)]
pub struct TaskContext {
    pub rsp: u64,
}

/// A schedulable task. Started small in slice 3.1; extended in 4.3.a
/// with process-tree fields (parent, children, exit_status) in
/// preparation for `fork`/`wait` semantics.
pub struct Task {
    pub id: u64,
    pub name: &'static str,
    pub state: TaskState,
    pub context: TaskContext,
    /// Virtual address of the very top of this task's stack. Kept so we
    /// can free the stack frames later when the task exits.
    pub stack_top: u64,
    /// The user-provided entry function. Not called directly — `switch`
    /// jumps into `trampoline`, which looks this up and calls it after
    /// enabling interrupts. See `trampoline` for why.
    pub entry_fn: unsafe extern "C" fn(),
    /// Parent task ID, if any. `None` for the initial main task. Set by
    /// `fork` (when implemented). Used by `wait` and for orphan
    /// reparenting on parent exit.
    pub parent: Option<u64>,
    /// Child task IDs. Empty until `fork` creates children. Used by
    /// `wait(-1)` (wait for any child) and for reparenting on exit.
    pub children: Vec<u64>,
    /// Exit code set by `sys_exit`. `None` while the task is still
    /// running. Read by the parent's `wait` call.
    pub exit_status: Option<i64>,
}

impl Task {
    /// Create a new task ready to be scheduled.
    ///
    /// - Allocates 16 physical frames (64 KB) for the kernel stack.
    /// - Maps them into a fresh virtual range.
    /// - Pre-populates the top of the stack with faked "saved state" so
    ///   the same context-switch routine that resumes old tasks can also
    ///   start this brand-new one. See `docs/context-switch.html`.
    pub fn new(id: u64, name: &'static str, entry_fn: unsafe extern "C" fn()) -> Self {
        let stack_top = TASK_STACK_BASE + (id + 1) * STACK_SIZE;
        let stack_bottom = stack_top - STACK_SIZE;

        // Allocate 16 frames and map them into the virtual range. The
        // frames don't need to be contiguous in physical RAM — only in
        // virtual. The mapper translates per-page, so 16 scattered
        // physical frames appear perfectly contiguous virtually.
        let mut frames_allocated = 0;
        let mut virt = stack_bottom;
        while virt < stack_top {
            let frame_number = frame_allocator::allocate()
                .expect("out of frames while allocating task stack");
            let phys = PhysAddr::new(frame_number * 4096);
            // SAFETY: virtual range is freshly picked (unmapped), frame is fresh.
            unsafe {
                memory::map_to(
                    VirtAddr::new(virt),
                    phys,
                    PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                )
                .expect("map_to failed while allocating task stack");
            }
            frames_allocated += 1;
            virt += 4096;
        }

        // Pre-populate the stack with faked saved state. After this, the
        // stack looks identical to a task that was just paused inside
        // switch() right before the `ret` instruction.
        //
        // Layout (from high address down):
        //   stack_top -  8: entry_fn address   ← popped by `ret` into RIP
        //   stack_top - 16: 0 (saved RBX)      ← popped by `pop rbx`
        //   stack_top - 24: 0 (saved RBP)      ← popped by `pop rbp`
        //   stack_top - 32: 0 (saved R12)      ← popped by `pop r12`
        //   stack_top - 40: 0 (saved R13)      ← popped by `pop r13`
        //   stack_top - 48: 0 (saved R14)      ← popped by `pop r14`
        //   stack_top - 56: 0 (saved R15)      ← popped by `pop r15` ← RSP starts here
        //
        // write_volatile so the compiler doesn't elide these writes (it
        // can't see that the context-switch routine will later read them).
        let init_rsp = stack_top - 56;
        let base_ptr = init_rsp as *mut u64;
        unsafe {
            core::ptr::write_volatile(base_ptr.add(0), 0);               // saved R15
            core::ptr::write_volatile(base_ptr.add(1), 0);               // saved R14
            core::ptr::write_volatile(base_ptr.add(2), 0);               // saved R13
            core::ptr::write_volatile(base_ptr.add(3), 0);               // saved R12
            core::ptr::write_volatile(base_ptr.add(4), 0);               // saved RBP
            core::ptr::write_volatile(base_ptr.add(5), 0);               // saved RBX
            // Note: switch's `ret` lands in `trampoline`, not the user's
            // entry_fn directly. The trampoline enables interrupts then
            // looks up entry_fn from this task's struct.
            core::ptr::write_volatile(base_ptr.add(6), trampoline as *const () as u64); // RIP -> trampoline
        }

        println!(
            "[task] created id={} name=\"{}\" stack=0x{:x}-0x{:x} ({} frames), context.rsp=0x{:x}, entry_fn=0x{:x}",
            id,
            name,
            stack_bottom,
            stack_top,
            frames_allocated,
            init_rsp,
            entry_fn as u64
        );

        Task {
            id,
            name,
            state: TaskState::Ready,
            context: TaskContext { rsp: init_rsp },
            stack_top,
            entry_fn,
            parent: None,
            children: Vec::new(),
            exit_status: None,
        }
    }
}

// ============================================================
// Trampoline (slice 3.4)
// ============================================================

/// Trampoline function that every brand-new task enters via `switch`'s
/// `ret` instruction.
///
/// Why this exists (the alternative was to put `sti` directly inside
/// `switch` right before `ret`). The trampoline approach keeps `switch`
/// minimal — pure register swap, no interrupt flag management. The
/// trampoline is also the natural extension point later for things like
/// enabling floating-point state or handling task exit.
///
/// What it does (in order):
///   1. Look up this task's user-provided `entry_fn` from SCHEDULER.
///      Done with interrupts still disabled (the state we entered with).
///   2. Enable interrupts (`sti`). This is the boundary between
///      "scheduler world" (IF=0) and "task world" (IF=1).
///   3. Call the user's `entry_fn`. If it returns, panic (later we'll
///      mark the task Dead instead).
unsafe extern "C" fn trampoline() {
    // Look up our entry function. We're definitely the current task
    // (switch just made us so), and interrupts are disabled so the
    // timer can't preempt us mid-lookup.
    let entry_fn = {
        let sched = SCHEDULER.lock();
        sched.tasks[sched.current].entry_fn
    };

    // Now flip interrupts on — we're entering task world.
    x86_64::instructions::interrupts::enable();

    // Call the user's entry function. It typically loops forever.
    entry_fn();

    // If it returned, the task is done. For now, panic. Later we'll
    // mark the task Dead and exit cleanly.
    panic!("task entry_fn returned");
}

// ============================================================
// Context switch (slice 3.2)
// ============================================================

/// Switch from one task's saved context to another.
///
/// This is the heart of multitasking. It's the routine that physically
/// pauses the calling task and resumes another. See `docs/context-switch.html`
/// for the step-by-step animation.
///
/// What it does (in order):
///   1. Push the 6 callee-saved registers onto the *current* stack
///      (RBX, RBP, R12-R15). Caller-saved registers don't need saving
///      because the calling convention already preserves them across
///      function calls.
///   2. Save the current RSP into `old.rsp`. This is the bookmark that
///      lets us find this task's saved state later.
///   3. Load `new.rsp` into the CPU's RSP. **This is the switch.** The
///      CPU is now operating on the new task's stack.
///   4. Pop the 6 callee-saved registers from the new stack.
///   5. `ret` pops the top of the new stack into RIP — jumping to
///      wherever the new task was paused (or, for a brand-new task,
///      to its entry function address).
///
/// # Why naked?
/// A normal Rust function has a prologue (push rbp; mov rbp, rsp; sub rsp, N)
/// and an epilogue. Those add extra stuff to the stack that would mess up
/// our careful layout. `#[unsafe(naked)]` means: no prologue, no epilogue,
/// just the asm we write. The function is effectively a thin wrapper around
/// the asm block.
///
/// # Calling convention
/// `extern "C"` on x86-64 is the System V AMD64 ABI: first arg in RDI,
/// second in RSI. We reference them directly in the asm.
///
/// # Safety
/// Caller must guarantee:
/// - `old` and `new` point to valid `TaskContext` structs
/// - `new.rsp` points to a stack with valid saved state (either from a
///   previous switch, or faked at `Task::new` time)
/// - No concurrent access to either context during the switch
#[unsafe(naked)]
pub unsafe extern "C" fn switch(old: *mut TaskContext, new: *const TaskContext) {
    naked_asm!(
        // Step 1: push callee-saved registers onto the current stack.
        // Push order is the reverse of pop order so that the last-pushed
        // register (R15) ends up at the lowest address — matching the
        // faked initial state layout in Task::new.
        "push rbx",
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        // Step 2: save current RSP into old.context.rsp.
        // rdi = old (first arg per System V AMD64 ABI).
        "mov [rdi], rsp",
        // Step 3: load new.context.rsp into CPU's RSP.
        // rsi = new (second arg).
        // **This is the switch moment.**
        "mov rsp, [rsi]",
        // Step 4: pop callee-saved registers from the new stack.
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",
        // Step 5: ret pops the top of the new stack into RIP.
        // For a paused task: returns to wherever it was when it last yielded.
        // For a brand-new task: jumps to its entry function.
        "ret",
    );
}

// ============================================================
// Cooperative scheduler (slice 3.2 / 3.3)
// ============================================================

/// The scheduler state. Round-robin: each yield advances to the next task
/// in the list, wrapping around.
struct Scheduler {
    /// All tasks, including the main task at index 0. Boxed so the Task
    /// addresses are stable even if the Vec reallocates.
    tasks: Vec<Box<Task>>,
    /// Index into `tasks` of the currently-running task.
    current: usize,
}

impl Scheduler {
    const fn new() -> Self {
        Scheduler {
            tasks: Vec::new(),
            current: 0,
        }
    }
}

lazy_static! {
    static ref SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());
}

/// Register the currently-running code as the "main" task (id 0) and
/// add it to the scheduler. Must be called once, from main, before any
/// `yield_now()`.
///
/// The main task has no allocated stack of its own — it uses whatever
/// stack the bootloader set up. Its `context.rsp` starts at 0 and gets
/// filled in by `switch()` the first time main yields. The `entry_fn`
/// field is never used because main never goes through the trampoline
/// (it's already running when init_main is called).
pub fn init_main() {
    let mut sched = SCHEDULER.lock();
    sched.tasks.push(Box::new(Task {
        id: 0,
        name: "main",
        state: TaskState::Running,
        context: TaskContext { rsp: 0 },
        stack_top: 0,
        entry_fn: main_never_called,
        parent: None,
        children: Vec::new(),
        exit_status: None,
    }));
    sched.current = 0;
    println!("[sched] main task registered (id=0)");
}

/// Placeholder entry function for the main task. Never actually called
/// — main is already running by the time it's registered. The field
/// needs *something* to satisfy the struct.
unsafe extern "C" fn main_never_called() {
    panic!("main_never_called: should never run");
}

/// Spawn a new task. Allocates its stack, fakes its initial state, and
/// adds it to the scheduler. Returns the task's id.
pub fn spawn(name: &'static str, entry_fn: unsafe extern "C" fn()) -> u64 {
    let mut sched = SCHEDULER.lock();
    let id = sched.tasks.len() as u64;
    sched.tasks.push(Box::new(Task::new(id, name, entry_fn)));
    println!("[sched] spawned \"{}\" (id={}), {} tasks total", name, id, sched.tasks.len());
    id
}

/// Return the currently-running task's ID.
///
/// Used by syscalls that need to know "who called me?" — `getpid`,
/// eventually `fork` (to know what to clone), `exit` (to know who to
/// mark as exited). Locks SCHEDULER briefly, copies the ID out.
pub fn current_pid() -> u64 {
    let sched = SCHEDULER.lock();
    sched.tasks[sched.current].id
}

/// Yield the CPU to the next task. Round-robin: advances to
/// `(current + 1) % tasks.len()`.
///
/// Used both for cooperative yields (a task calls this directly) and
/// preemptive yields (the timer interrupt handler calls this on the
/// current task's behalf). Same code path, same `switch`.
///
/// **Re-entrancy safety:** we disable interrupts at the top and restore
/// them at the end. Without this, if `yield_now` was running with
/// interrupts enabled and the timer fired again mid-switch, the handler
/// would call `yield_now` again and try to lock SCHEDULER → deadlock.
/// Disabling interrupts makes the whole pick-and-switch atomic with
/// respect to other timer-driven yields.
///
/// Locks the scheduler only long enough to pick the next task and grab
/// raw pointers to the two contexts. The lock is dropped *before* the
/// switch, so we never hold it across a context switch (which would
/// deadlock the system — the other task couldn't lock it either).
pub fn yield_now() {
    // Disable interrupts for the duration. Remember whether they were
    // on so we can restore the original state on the way out.
    let was_enabled = x86_64::instructions::interrupts::are_enabled();
    if was_enabled {
        x86_64::instructions::interrupts::disable();
    }

    // Pick the next task and grab raw pointers to both contexts.
    let switch_result: Option<(*mut TaskContext, *const TaskContext)> = {
        let mut sched = SCHEDULER.lock();
        if sched.tasks.len() < 2 {
            None // nothing to switch to
        } else {
            let old_index = sched.current;
            let new_index = (sched.current + 1) % sched.tasks.len();
            sched.tasks[old_index].state = TaskState::Ready;
            sched.tasks[new_index].state = TaskState::Running;
            sched.current = new_index;
            let old_ptr = &mut sched.tasks[old_index].context as *mut TaskContext;
            let new_ptr = &sched.tasks[new_index].context as *const TaskContext;
            Some((old_ptr, new_ptr))
        }
    };

    if let Some((old_ptr, new_ptr)) = switch_result {
        // SAFETY: pointers are valid (Box'd tasks), new context has
        // either valid saved state or faked initial state, interrupts
        // are disabled so no concurrent access.
        unsafe { switch(old_ptr, new_ptr); }
    }

    // We're back (either we switched away and have now been switched
    // back to, or we never switched because there was nothing to do).
    // Restore IF to its original state.
    if was_enabled {
        x86_64::instructions::interrupts::enable();
    }
}
