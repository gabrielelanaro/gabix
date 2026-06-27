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
///
/// Slice 4.4.b added `address_space`: every task holds one. Kernel
/// threads (main, idle, workers) all share the kernel bootstrap AS via
/// `AddressSpace::from_current()`. User processes (eventually created by
/// `fork`) get their own via `AddressSpace::new()`.
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
    /// This task's address space — the PML4 the CPU should use when
    /// running this task. Switched via CR3 on context switch.
    pub address_space: crate::memory::AddressSpace,
    /// Top of this task's dedicated syscall stack. When this task is
    /// running and user code executes `syscall`, `syscall_entry` switches
    /// to this stack. Per-task because a shared syscall stack gets
    /// overwritten when one task's syscall context is paused (via timer
    /// preemption) and another task's syscall runs on top of it.
    /// Updated into `KERNEL_RSP` by `yield_now` on every context switch.
    pub syscall_stack_top: u64,
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
            address_space: crate::memory::AddressSpace::from_current(),
            // Kernel threads don't make syscalls (no user mode), so they
            // can share the dedicated syscall stack. Set to 0; yield_now
            // will leave KERNEL_RSP alone if it sees 0.
            syscall_stack_top: 0,
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
// fork_trampoline (slice 4.4.c.2)
// ============================================================

/// Entry point for a forked child task's first run.
///
/// When `sys_fork` creates a child, it populates the child's kernel stack
/// so that the first `switch()` into the child pops 6 callee-saved
/// registers and then `ret`s into this function. (Contrast with `trampoline`
/// above, which is the entry point for fresh tasks created via `Task::new`.)
///
/// What it does:
///   1. Disable interrupts (defensive — they should already be off since
///      `yield_now` disables them around the switch).
///   2. Set RAX = 0. This is the child's distinguishing value — the
///      "return value" the child sees from the `syscall` instruction that
///      the parent executed. The parent gets the child's PID; the child
///      gets 0.
///   3. Load RCX with the saved user RIP (where to resume in user code).
///   4. Load R11 with the saved user RFLAGS (CPU state flags, including
///      the interrupt-enable bit — sysretq will restore them).
///   5. Load RSP with the saved user RSP (switches to the user stack,
///      which exists at the same virtual address in the child's address
///      space, but is backed by the child's own physical frames).
///   6. `sysretq` — transfers control to user mode at the saved RIP.
///
/// After sysretq, the child is running in user mode at the instruction
/// immediately after the parent's `syscall` instruction, with RAX = 0.
/// To user code, it looks like the syscall just returned 0.
/// Naked version of fork_trampoline. Reads the saved user state from
/// fixed offsets on the kernel stack (NOT from globals — see the spawn
/// _fork_child comment for why), loads them into the right registers,
/// and sysretq's.
///
/// At entry, RSP = stack_top (we just got ret'd into by switch(), which
/// popped the callee-saved area + the ret address). The saved user state
/// lives just below where switch()'s pops reached:
///   [rsp - 64] = saved user RIP
///   [rsp - 72] = saved user RFLAGS
///   [rsp - 80] = saved user RSP
#[unsafe(naked)]
unsafe extern "C" fn fork_trampoline() {
    naked_asm!(
        "mov rcx, [rsp - 64]",          // user RIP for sysretq
        "mov r11, [rsp - 72]",          // user RFLAGS for sysretq
        "mov rax, [rsp - 80]",          // user RSP (use rax as scratch)
        "mov rsp, rax",                 // switch to user stack
        "xor rax, rax",                 // RAX = 0 (child's distinguishing fork return)
        "cli",                          // (defensive) ensure IF=0 for sysretq
        "sysretq",
    );
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
        address_space: crate::memory::AddressSpace::from_current(),
        // Main uses the dedicated syscall stack allocated by `syscall::init`.
        syscall_stack_top: crate::syscall::SYSCALL_STACK_TOP,
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

/// Spawn a fork-child task. Like `spawn`, but the new task:
///   - Has its own `AddressSpace` (created INSIDE this function — see
///     the comment below for why the ordering matters).
///   - Has a kernel stack populated with the parent's saved callee-saved
///     register values (so the child sees the same values the parent
///     had at the moment of the `syscall` instruction).
///   - Has its "return address" slot pointing at `fork_trampoline` instead
///     of `trampoline` — when the child first runs, it does `sysretq`
///     back to user mode at the saved RIP with RAX=0.
///   - Has `parent` set to the parent's PID and is added to the parent's
///     `children` list.
///
/// Returns the child's PID.
///
/// **Why AddressSpace is created INSIDE this function** (not passed in):
/// the child's AS is built by `AddressSpace::new()`, which copies all 512
/// entries from the currently-active PML4. For the child to be able to
/// access its own kernel stack (which we allocate here), those mappings
/// must already exist in the parent's PML4 at the moment we copy. So we
/// allocate+map the kernel stack FIRST, then create the AS (which inherits
/// the new mappings via the copy), then build the Task struct using that AS.
pub fn spawn_fork_child(
    parent_pid: u64,
    saved_callee_saves: SavedCalleeSaves,
) -> u64 {
    // Disable interrupts for the duration. While we hold SCHEDULER, the
    // timer must not fire — its handler would try to lock SCHEDULER and
    // deadlock. (Caller is sys_fork, which runs with interrupts on because
    // syscall_entry does `sti` before calling the handler.)
    let was_enabled = x86_64::instructions::interrupts::are_enabled();
    if was_enabled {
        x86_64::instructions::interrupts::disable();
    }

    let mut sched = SCHEDULER.lock();
    let id = sched.tasks.len() as u64;

    // Compute stack region (same layout as Task::new).
    let stack_top = TASK_STACK_BASE + (id + 1) * STACK_SIZE;
    let stack_bottom = stack_top - STACK_SIZE;

    // Allocate 16 frames and map them into the virtual range, in the
    // CURRENT (parent's) address space.
    let mut frames_allocated = 0;
    let mut virt = stack_bottom;
    while virt < stack_top {
        let frame_number = frame_allocator::allocate()
            .expect("out of frames while allocating fork-child stack");
        let phys = PhysAddr::new(frame_number * 4096);
        // SAFETY: virtual range is freshly picked, frame is fresh.
        unsafe {
            memory::map_to(
                VirtAddr::new(virt),
                phys,
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
            )
            .expect("map_to failed while allocating fork-child stack");
        }
        frames_allocated += 1;
        virt += 4096;
    }

    // Allocate a dedicated syscall stack for the child. Without this,
    // the child shares the parent's syscall stack and the two collide:
    // when the parent is preempted mid-syscall and the child then makes
    // its own syscalls, the child overwrites the parent's paused state
    // on the shared stack. Per-task stacks fix this.
    //
    // Stack placement: SYSCALL_STACK_BOTTOM + (id+1) * SYSCALL_STACK_SIZE.
    // Task 0 (main) uses slot 0 (the dedicated stack allocated by
    // syscall::init). Each fork child takes the next slot.
    let child_syscall_stack_bottom =
        crate::syscall::SYSCALL_STACK_BOTTOM + (id + 1) * crate::syscall::SYSCALL_STACK_SIZE;
    let child_syscall_stack_top =
        child_syscall_stack_bottom + crate::syscall::SYSCALL_STACK_SIZE;
    {
        let mut virt = child_syscall_stack_bottom;
        while virt < child_syscall_stack_top {
            let frame_number = frame_allocator::allocate()
                .expect("out of frames while allocating child syscall stack");
            let phys = PhysAddr::new(frame_number * 4096);
            // SAFETY: virtual range is freshly picked, frame is fresh.
            unsafe {
                memory::map_to(
                    VirtAddr::new(virt),
                    phys,
                    PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                )
                .expect("map_to failed while allocating child syscall stack");
            }
            virt += 4096;
        }
    }

    // Populate the kernel stack with the parent's callee-saved values + the
    // fork_trampoline return address + the saved user-mode state. The
    // layout extends Task::new's layout with three extra slots BELOW
    // init_rsp for the user RIP/RSP/RFLAGS — these don't get touched by
    // `switch()` (which only pops the callee-saved area), but they're
    // readable from `fork_trampoline` via `[rsp - offset]` after the ret.
    //
    // Layout (high → low):
    //   stack_top -  8: fork_trampoline address   ← popped by `ret` into RIP
    //   stack_top - 16: saved RBX                 ← popped by `pop rbx`
    //   stack_top - 24: saved RBP
    //   stack_top - 32: saved R12
    //   stack_top - 40: saved R13
    //   stack_top - 48: saved R14
    //   stack_top - 56: saved R15                 ← init_rsp (RSP starts here)
    //   stack_top - 64: saved user RIP            ← extra (read by fork_trampoline)
    //   stack_top - 72: saved user RFLAGS         ← extra
    //   stack_top - 80: saved user RSP            ← extra
    //
    // After switch()'s ret, RSP = stack_top. fork_trampoline reads the
    // extra slots via [rsp - 64], [rsp - 72], [rsp - 80].
    let init_rsp = stack_top - 56;
    let base_ptr = init_rsp as *mut u64;
    unsafe {
        // Callee-saved area (consumed by switch's pop instructions).
        core::ptr::write_volatile(base_ptr.add(0), saved_callee_saves.r15);
        core::ptr::write_volatile(base_ptr.add(1), saved_callee_saves.r14);
        core::ptr::write_volatile(base_ptr.add(2), saved_callee_saves.r13);
        core::ptr::write_volatile(base_ptr.add(3), saved_callee_saves.r12);
        core::ptr::write_volatile(base_ptr.add(4), saved_callee_saves.rbp);
        core::ptr::write_volatile(base_ptr.add(5), saved_callee_saves.rbx);
        core::ptr::write_volatile(base_ptr.add(6), fork_trampoline as *const () as u64);

        // Extra user-state slots below init_rsp. NOT touched by switch();
        // read by fork_trampoline after ret via fixed offsets from RSP.
        core::ptr::write_volatile(base_ptr.sub(1), saved_callee_saves.user_rip);
        core::ptr::write_volatile(base_ptr.sub(2), saved_callee_saves.user_rflags);
        core::ptr::write_volatile(base_ptr.sub(3), saved_callee_saves.user_rsp);
    }

    // Deep-clone the current (parent's) address space into a fresh AS
    // for the child. This walks the parent's full page-table tree and
    // allocates fresh PDPT/PD/PT frames for the child, so the child has
    // independent intermediate page tables (modifications to child's
    // PTs don't affect parent). At the PT level, user pages get
    // private-copy-on-clone (fresh frame + memcpy); kernel pages share
    // the frame.
    //
    // Both the kernel stack and syscall stack we just allocated are
    // already mapped in the parent's AS, so they'll be in the child's
    // too after the deep clone (as kernel-page shares).
    let mut address_space = crate::memory::AddressSpace::new();
    address_space.deep_clone_from_current();

    // Build the Task struct. entry_fn is unused for fork children (they
    // go through fork_trampoline, not trampoline), but the field needs
    // *something* to satisfy the struct.
    extern "C" fn fork_child_unused_entry() {
        panic!("fork_child_unused_entry: should never run (fork children use fork_trampoline)");
    }

    let task = Task {
        id,
        name: "fork-child",
        state: TaskState::Ready,
        context: TaskContext { rsp: init_rsp },
        stack_top,
        entry_fn: fork_child_unused_entry,
        parent: Some(parent_pid),
        children: Vec::new(),
        exit_status: None,
        address_space,
        syscall_stack_top: child_syscall_stack_top,
    };

    sched.tasks.push(Box::new(task));

    // Add the child to the parent's children list.
    let parent_task = sched
        .tasks
        .iter_mut()
        .find(|t| t.id == parent_pid)
        .expect("parent task not found in spawn_fork_child");
    parent_task.children.push(id);

    println!(
        "[sched] spawned fork-child id={} (parent={}), stack=0x{:x}-0x{:x} ({} frames), context.rsp=0x{:x}",
        id, parent_pid, stack_bottom, stack_top, frames_allocated, init_rsp,
    );

    // Drop the lock BEFORE re-enabling interrupts. Otherwise the timer
    // could fire between sti and the implicit drop-at-scope-exit, and the
    // timer handler would try to lock SCHEDULER → deadlock.
    drop(sched);

    // Re-enable interrupts if they were on when we entered. (They almost
    // always were, since syscall_entry does `sti` before calling handlers.
    // But the check makes the function safe to call from any context.)
    if was_enabled {
        x86_64::instructions::interrupts::enable();
    }

    id
}

/// Saved register values from the moment of a `syscall` instruction,
/// captured by `syscall_entry` into global atomics (see src/syscall.rs),
/// read by `sys_fork` ONCE at fork time, and passed here so the child
/// task starts life seeing the same values the parent had.
///
/// **Why this is a snapshot, not a live read:** the globals in syscall.rs
/// get OVERWRITTEN by every subsequent syscall. If the parent makes more
/// syscalls between fork and the child's first run, the globals would
/// have stale (wrong) values. By snapshotting at fork time and storing on
/// the child's kernel stack, the child always sees the values from THE
/// fork syscall, regardless of what the parent does later.
pub struct SavedCalleeSaves {
    // The 6 callee-saved registers (preserved across syscalls per the
    // System V AMD64 ABI — the child needs them to look like a true copy).
    pub rbx: u64,
    pub rbp: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    // User-mode state at the moment of the syscall. The child needs these
    // to do `sysretq` back to user mode at the right place.
    pub user_rip: u64,
    pub user_rsp: u64,
    pub user_rflags: u64,
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

/// Read the PML4 frame of the currently-running task's address space.
/// Used by `sys_exec` to switch CR3 to the (just-replaced) AS.
/// Returns the frame as a `PhysFrame` so it can be passed to `Cr3::write`.
pub fn current_pml4_frame() -> x86_64::structures::paging::PhysFrame {
    let sched = SCHEDULER.lock();
    sched.tasks[sched.current].address_space.pml4_frame()
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
    // Also grab the new task's AS frame so we can switch CR3 if needed.
    //
    // Scan forward from the current task to find the next non-Zombie task.
    // This lets sys_exit mark the current task as Zombie and then yield,
    // knowing yield_now will pick somebody else.
    let switch_result: Option<(*mut TaskContext, *const TaskContext, u64, u64)> = {
        let mut sched = SCHEDULER.lock();
        if sched.tasks.len() < 2 {
            None // nothing to switch to
        } else {
            let old_index = sched.current;
            // Find next runnable task. Skip Zombie (exited, awaiting reap),
            // Blocked (sleeping, e.g. waiting on wait()), and Dead (already
            // reaped, awaiting cleanup).
            let mut new_index_opt = None;
            for offset in 1..sched.tasks.len() {
                let candidate = (old_index + offset) % sched.tasks.len();
                let state = sched.tasks[candidate].state;
                if state != TaskState::Zombie
                    && state != TaskState::Blocked
                    && state != TaskState::Dead
                {
                    new_index_opt = Some(candidate);
                    break;
                }
            }
            match new_index_opt {
                None => {
                    // No other runnable task.
                    let current_state = sched.tasks[old_index].state;
                    if current_state == TaskState::Zombie
                        || current_state == TaskState::Blocked
                        || current_state == TaskState::Dead
                    {
                        // Current is un-runnable AND nothing else can run.
                        // System is effectively done. Halt.
                        println!(
                            "[sched] no runnable tasks (current state={:?}) — halting",
                            current_state
                        );
                        drop(sched);
                        x86_64::instructions::interrupts::disable();
                        loop {
                            x86_64::instructions::hlt();
                        }
                    }
                    // Current is the only runnable task. Just return.
                    None
                }
                Some(new_index) => {
                    // Don't change current's state if it's un-runnable
                    // (Zombie, Blocked, Dead) — those states must persist
                    // across yield so future yields still skip them.
                    let old_state = sched.tasks[old_index].state;
                    if old_state != TaskState::Zombie
                        && old_state != TaskState::Blocked
                        && old_state != TaskState::Dead
                    {
                        sched.tasks[old_index].state = TaskState::Ready;
                    }
                    sched.tasks[new_index].state = TaskState::Running;
                    sched.current = new_index;
                    let old_ptr = &mut sched.tasks[old_index].context as *mut TaskContext;
                    let new_ptr = &sched.tasks[new_index].context as *const TaskContext;
                    let new_as_frame = sched.tasks[new_index]
                        .address_space
                        .pml4_frame()
                        .start_address()
                        .as_u64();
                    let new_syscall_rsp = sched.tasks[new_index].syscall_stack_top;
                    Some((old_ptr, new_ptr, new_as_frame, new_syscall_rsp))
                }
            }
        }
    };

    if let Some((old_ptr, new_ptr, new_as_frame, new_syscall_rsp)) = switch_result {
        // Update KERNEL_RSP to the new task's syscall stack. If the new
        // task has no dedicated syscall stack (kernel threads: field is 0),
        // leave KERNEL_RSP alone so it stays at whatever the previous
        // user task set.
        if new_syscall_rsp != 0 {
            crate::syscall::set_kernel_rsp(new_syscall_rsp);
        }

        // Switch address space if the new task's PML4 differs from the
        // one currently in CR3. Skipping the write when ASes match saves
        // a TLB flush — important when switching between kernel threads
        // (which all share the kernel AS).
        let current_as_frame = x86_64::registers::control::Cr3::read().0.start_address().as_u64();
        if current_as_frame != new_as_frame {
            // SAFETY: new_as_frame came from a valid AddressSpace that
            // owns a properly initialized PML4. Writing CR3 to it is safe.
            unsafe {
                x86_64::registers::control::Cr3::write(
                    x86_64::structures::paging::PhysFrame::containing_address(x86_64::PhysAddr::new(new_as_frame)),
                    x86_64::registers::control::Cr3Flags::empty(),
                );
            }
        }

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

/// Mark the currently-running task as Zombie with the given exit code.
///
/// Called by `sys_exit` BEFORE calling `yield_now`. After this call:
///   - The task's state is `Zombie`.
///   - The task's `exit_status` is set.
///   - Future `yield_now` calls will skip this task when picking what to
///     run next (so it never gets rescheduled).
///
/// Disables interrupts for the lock+update — if the timer fired while we
/// held the SCHEDULER lock, the timer handler would deadlock trying to
/// acquire it.
pub fn mark_current_zombie(exit_code: i64) {
    let was_enabled = x86_64::instructions::interrupts::are_enabled();
    if was_enabled {
        x86_64::instructions::interrupts::disable();
    }
    let parent_pid = {
        let mut sched = SCHEDULER.lock();
        let current = sched.current;
        sched.tasks[current].state = TaskState::Zombie;
        sched.tasks[current].exit_status = Some(exit_code);
        let parent_pid = sched.tasks[current].parent;
        println!(
            "[sched] task id={} \"{}\" marked Zombie (exit={})",
            sched.tasks[current].id, sched.tasks[current].name, exit_code
        );
        parent_pid
    };

    // If our parent is Blocked (e.g. inside sys_wait), wake it — its child
    // just exited, so it has a Zombie to reap.
    if let Some(parent) = parent_pid {
        let mut sched = SCHEDULER.lock();
        let parent_task = sched
            .tasks
            .iter_mut()
            .find(|t| t.id == parent);
        if let Some(task) = parent_task {
            if task.state == TaskState::Blocked {
                task.state = TaskState::Ready;
                println!(
                    "[sched] task id={} \"{}\" woken (child exited)",
                    task.id, task.name
                );
            }
        }
    }
    // Note: don't re-enable here. yield_now (called next by sys_exit)
    // will handle the interrupt state via its own was_enabled check.
}

/// Mark the currently-running task as Blocked. The task won't be picked
/// by `yield_now` until something else flips its state back to Ready
/// (e.g. `mark_current_zombie` waking a parent that's waiting on a child).
pub fn mark_current_blocked() {
    let was_enabled = x86_64::instructions::interrupts::are_enabled();
    if was_enabled {
        x86_64::instructions::interrupts::disable();
    }
    {
        let mut sched = SCHEDULER.lock();
        let current = sched.current;
        sched.tasks[current].state = TaskState::Blocked;
        println!(
            "[sched] task id={} \"{}\" marked Blocked",
            sched.tasks[current].id, sched.tasks[current].name
        );
    }
    // Like mark_current_zombie: leave interrupts disabled; caller (sys_wait)
    // calls yield_now next, which manages interrupt state itself.
}

/// Replace the current task's address space with `new_as`. Used by
/// `sys_exec`: the task struct stays (same PID, same kernel stack, same
/// scheduler entry), but everything user-side — code, stack, page tables —
/// is replaced.
///
/// Returns nothing — caller is expected to immediately jump to user mode
/// at the new program's entry point (via inline sysretq-style asm).
pub fn replace_current_address_space(new_as: crate::memory::AddressSpace, new_name: &'static str) {
    let was_enabled = x86_64::instructions::interrupts::are_enabled();
    if was_enabled {
        x86_64::instructions::interrupts::disable();
    }
    let old_as_frame = {
        let mut sched = SCHEDULER.lock();
        let current = sched.current;
        let old = sched.tasks[current].address_space.pml4_frame();
        sched.tasks[current].address_space = new_as;
        sched.tasks[current].name = new_name;
        println!(
            "[sched] task id={} exec: now \"{}\", AS pml4 frame {} → {}",
            sched.tasks[current].id,
            new_name,
            old.start_address().as_u64() / 4096,
            sched.tasks[current].address_space.pml4_frame().start_address().as_u64() / 4096,
        );
        old
    };
    let _ = old_as_frame; // old AS leaks (no cleanup yet)
}

/// Find a Zombie child of the given parent. Returns the child's PID and
/// exit status if one exists, else None.
pub fn find_zombie_child_of(parent_pid: u64) -> Option<(u64, i64)> {
    let sched = SCHEDULER.lock();
    for task in sched.tasks.iter() {
        if task.parent == Some(parent_pid) && task.state == TaskState::Zombie {
            return Some((task.id, task.exit_status.unwrap_or(0)));
        }
    }
    None
}

/// Does the given parent have any child(ren) — in ANY state?
/// Used by sys_wait to detect ECHILD (no children to wait for).
pub fn has_any_children(parent_pid: u64) -> bool {
    let sched = SCHEDULER.lock();
    sched.tasks.iter().any(|t| t.parent == Some(parent_pid))
}

/// Reap a Zombie child: copy out its exit status, mark it as Dead (so
/// yield_now skips it permanently). Caller has already verified the child
/// is a Zombie child of the current task.
pub fn reap_child(child_pid: u64) -> i64 {
    let mut sched = SCHEDULER.lock();
    let current_pid = sched.tasks[sched.current].id;
    let child = sched
        .tasks
        .iter_mut()
        .find(|t| t.id == child_pid);
    let exit_status = match child {
        Some(task) => {
            assert!(
                task.parent == Some(current_pid),
                "reap_child: not your child"
            );
            assert!(task.state == TaskState::Zombie, "reap_child: not Zombie");
            task.state = TaskState::Dead;
            task.exit_status.unwrap_or(0)
        }
        None => panic!("reap_child: pid {} not found", child_pid),
    };
    // Remove reaped child from parent's children list (separate mutable
    // borrow — couldn't keep both `task` and `parent_task` alive at once).
    {
        let parent_task = sched
            .tasks
            .iter_mut()
            .find(|t| t.id == current_pid);
        if let Some(parent) = parent_task {
            parent.children.retain(|&c| c != child_pid);
        }
    }
    println!(
        "[sched] task id={} reaped by parent {} (exit={})",
        child_pid, current_pid, exit_status
    );
    exit_status
}
