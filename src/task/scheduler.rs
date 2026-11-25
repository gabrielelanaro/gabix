//! Task abstraction: data structures + constructor + context switch.
//!
//! Each Task holds its scheduling state and saved kernel-stack pointer.
//! Its PID selects the Process containing memory, file and parent/child state.
//! `yield_now` serves both voluntary yields and timer-driven preemption.

use crate::arch::x86_64::context::{fork_trampoline, saved_stack, switch, TaskContext};
use crate::frame_allocator;
use crate::irq_mutex::IrqMutex;
use crate::memory;
use crate::println;
use crate::process::{Pid, Process, PROCESS_TABLE};
use alloc::boxed::Box;
use alloc::vec::Vec;
use lazy_static::lazy_static;
use gabix_kernel::memory::FRAME_SIZE;
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
    /// Waiting for a wakeup, such as socket data or a child exiting.
    Blocked,
    /// Exited and awaiting wait/reap. The Process stores the exit status.
    /// Reaping removes that Process and changes this task to Dead.
    Zombie,
    /// Reaped and never scheduled. Task storage and stack frames remain allocated.
    Dead,
}

impl TaskState {
    /// States that the round-robin scan can select. Blocked tasks need a
    /// wakeup; Zombie and Dead tasks have finished executing.
    fn is_runnable(self) -> bool {
        matches!(self, Self::Ready | Self::Running)
    }
}

/// A schedulable task — the scheduler-side view of a paused register
/// frame. Lesson 17 moved every process-shaped field (identity, tree,
/// memory, files) to `Process` in `src/process.rs`. `Task` keeps a `pid`
/// back-reference so the scheduler can reach the process table when it
/// needs address-space or fd-table information.
pub struct Task {
    /// Back-reference to this task's process. Set at task creation and
    /// never changes. The scheduler locks SCHEDULER to read this, drops
    /// the lock, then locks PROCESS_TABLE to read whatever process-side
    /// field it needs — never holds both at once.
    pub pid: Pid,
    pub state: TaskState,
    pub context: TaskContext,
    /// Virtual address of the very top of this task's kernel stack. Kept
    /// so we can free the stack frames later when the task exits.
    pub stack_top: u64,
    /// The user-provided entry function. Not called directly — `switch`
    /// jumps into `trampoline`, which looks this up and calls it after
    /// enabling interrupts. See `trampoline` for why. Unused for fork
    /// children (they go through `fork_trampoline` instead).
    pub entry_fn: unsafe extern "C" fn(),
    /// Top of this task's ring-3 entry stack. Syscall entry and timer
    /// interrupts from user mode both land here. Per-task because shared
    /// entry stacks corrupt paused syscall frames during preemption.
    /// Updated into `KERNEL_RSP` by `yield_now` on every context switch.
    pub entry_stack_top: u64,
}

impl Task {
    /// Create a new task ready to be scheduled.
    ///
    /// - Allocates 16 physical frames (64 KB) for the kernel stack.
    /// - Maps them into a fresh virtual range.
    /// - Pre-populates the top of the stack with faked "saved state" so
    ///   the same context-switch routine that resumes old tasks can also
    ///   start this brand-new one.
    ///
    /// This uses the boot-time global mapper. The executing address space
    /// must also expose the resulting range before the stack is populated.
    /// No guard page or stack reclamation is provided here.
    ///
    /// Process-side state (parent, address space, fd table, name) is
    /// allocated separately via `Process::new` + `PROCESS_TABLE::allocate`
    /// — the caller passes the resulting `Pid` here.
    pub fn new(pid: Pid, entry_fn: unsafe extern "C" fn()) -> Self {
        let stack_top = TASK_STACK_BASE + (pid.as_u64() + 1) * STACK_SIZE;
        let stack_bottom = stack_top - STACK_SIZE;

        // Allocate 16 frames and map them into the virtual range. The
        // frames don't need to be contiguous in physical RAM — only in
        // virtual. The mapper translates per-page, so 16 scattered
        // physical frames appear perfectly contiguous virtually.
        let mut frames_allocated = 0;
        let mut virt = stack_bottom;
        while virt < stack_top {
            let frame_number =
                frame_allocator::allocate().expect("out of frames while allocating task stack");
            let phys = PhysAddr::new(frame_number * FRAME_SIZE);
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
            virt += FRAME_SIZE;
        }

        // Pre-populate the stack with faked saved state. After this, the
        // stack looks identical to a task that was just paused inside
        // switch() right before the `ret` instruction.
        //
        // Layout (from high address down):
        //   stack_top - 16: trampoline address   ← popped by `ret` into RIP
        //   stack_top - 24: 0 (saved RBX)      ← popped by `pop rbx`
        //   stack_top - 32: 0 (saved RBP)      ← popped by `pop rbp`
        //   stack_top - 40: 0 (saved R12)      ← popped by `pop r12`
        //   stack_top - 48: 0 (saved R13)      ← popped by `pop r13`
        //   stack_top - 56: 0 (saved R14)      ← popped by `pop r14`
        //   stack_top - 64: 0 (saved R15)      ← popped by `pop r15` ← RSP starts here
        //
        // The extra 8-byte gap leaves the trampoline with RSP % 16 == 8
        // after `ret`, which is the System V AMD64 function-entry contract.
        //
        // write_volatile so the compiler doesn't elide these writes (it
        // can't see that the context-switch routine will later read them).
        let init_rsp = stack_top - saved_stack::SWITCH_BYTES - saved_stack::RUST_ENTRY_GAP_BYTES;
        let base_ptr = init_rsp as *mut u64;
        unsafe {
            core::ptr::write_volatile(base_ptr.add(saved_stack::R15), 0); // saved R15
            core::ptr::write_volatile(base_ptr.add(saved_stack::R14), 0); // saved R14
            core::ptr::write_volatile(base_ptr.add(saved_stack::R13), 0); // saved R13
            core::ptr::write_volatile(base_ptr.add(saved_stack::R12), 0); // saved R12
            core::ptr::write_volatile(base_ptr.add(saved_stack::RBP), 0); // saved RBP
            core::ptr::write_volatile(base_ptr.add(saved_stack::RBX), 0); // saved RBX
            // `ret` starts the trampoline, which enables interrupts and calls
            // the entry_fn stored in this task.
            core::ptr::write_volatile(
                base_ptr.add(saved_stack::RETURN_ADDRESS),
                trampoline as *const () as u64,
            );
            // RIP -> trampoline
        }

        println!(
            "[task] created pid={} stack=0x{:x}-0x{:x} ({} frames), context.rsp=0x{:x}, entry_fn=0x{:x}",
            pid,
            stack_bottom,
            stack_top,
            frames_allocated,
            init_rsp,
            entry_fn as u64
        );

        Task {
            pid,
            state: TaskState::Ready,
            context: TaskContext { rsp: init_rsp },
            stack_top,
            entry_fn,
            // Kernel threads don't make syscalls (no user mode), so they
            // can share the dedicated syscall stack. Set to 0; yield_now
            // will leave KERNEL_RSP alone if it sees 0.
            entry_stack_top: 0,
        }
    }
}

// ============================================================
// Trampoline (slice 3.4)
// ============================================================

/// First entry for newly spawned kernel tasks. Read entry_fn while interrupts
/// are disabled, release the scheduler lock, enable interrupts and call it.
/// Returning from entry_fn currently panics. Fork children use fork_trampoline.
unsafe extern "C" fn trampoline() {
    // Look up our entry function. We're definitely the current task
    // (switch just made us so), and interrupts are disabled so the
    // timer can't preempt us mid-lookup.
    let entry_fn = {
        let sched = SCHEDULER.lock();
        sched.tasks[sched.current].entry_fn
    };

    // Allow timer preemption while the kernel task runs.
    x86_64::instructions::interrupts::enable();

    // Call the user's entry function. It typically loops forever.
    entry_fn();

    // If it returned, the task is done. For now, panic. Later we'll
    // mark the task Dead and exit cleanly.
    panic!("task entry_fn returned");
}

// ============================================================
// Cooperative scheduler (slice 3.2 / 3.3)
// ============================================================

/// Round-robin scheduler storage. Selection scans for runnable tasks,
/// wrapping around the list; blocked and completed tasks are skipped.
pub(super) struct Scheduler {
    /// All tasks, including the main task at index 0. Boxed so the Task
    /// addresses are stable even if the Vec reallocates.
    pub(super) tasks: Vec<Box<Task>>,
    /// Index into `tasks` of the currently-running task.
    pub(super) current: usize,
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
    pub(super) static ref SCHEDULER: IrqMutex<Scheduler> = IrqMutex::new(Scheduler::new());
}

/// Register the currently-running code as the "main" task and add it to
/// the scheduler. Must be called once, from main, before any `yield_now()`.
///
/// The main task has no allocated stack of its own — it uses whatever
/// stack the bootloader set up. Its `context.rsp` starts at 0 and gets
/// filled in by `switch()` the first time main yields. The `entry_fn`
/// field is never used because main never goes through the trampoline
/// (it's already running when init_main is called).
///
/// Lesson 17: the matching `Process` for the boot process is allocated
/// by `process::init()` (PID 0) before this runs. `init_main` only
/// registers the scheduler-side `Task` and points it at `Pid::ZERO`.
pub fn init_main() {
    let mut sched = SCHEDULER.lock();
    sched.tasks.push(Box::new(Task {
        pid: Pid::ZERO,
        state: TaskState::Running,
        context: TaskContext { rsp: 0 },
        stack_top: 0,
        entry_fn: main_never_called,
        // Main uses the dedicated syscall stack allocated by `syscall::init`.
        entry_stack_top: crate::arch::x86_64::syscall::SYSCALL_STACK_TOP,
    }));
    sched.current = 0;
    println!("[sched] main task registered (pid={})", Pid::ZERO);
}

/// Placeholder entry function for the main task. Never actually called
/// — main is already running by the time it's registered. The field
/// needs *something* to satisfy the struct.
unsafe extern "C" fn main_never_called() {
    panic!("main_never_called: should never run");
}

/// Spawn a new kernel task. Allocates a `Process` (with the current
/// task as parent) and the matching `Task`, then returns the new pid.
///
/// Used for kernel threads. User processes are created by `sys_fork`
/// via `spawn_fork_child` instead.
pub fn spawn(name: &str, entry_fn: unsafe extern "C" fn()) -> Pid {
    // Get the parent pid before locking PROCESS_TABLE — current_pid
    // locks SCHEDULER, and we never hold SCHEDULER + PROCESS_TABLE
    // at the same time.
    let parent_pid = current_pid();

    let pid = x86_64::instructions::interrupts::without_interrupts(|| {
        let mut table = PROCESS_TABLE.lock();
        let proc = Process::new(
            name,
            Some(parent_pid),
            crate::memory::AddressSpace::from_current(),
            crate::fs::FdTable::new(),
        );
        table.allocate(proc)
    });

    // Build the stack before publishing the Task to the runnable list.
    // IrqMutex protects scheduler access; the outer interrupt-disabled block
    // also keeps publication and its diagnostic together.
    let task = Box::new(Task::new(pid, entry_fn));
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        sched.tasks.push(task);
        println!(
            "[sched] spawned \"{}\" (pid={}), {} tasks total",
            name,
            pid,
            sched.tasks.len()
        );
    });
    pid
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
/// `AddressSpace::new()` copies the parent's kernel-half PML4 entries
/// (256–511), and `deep_clone_from_current()` walks the user half
/// (0–255), sharing non-user frames via entry copy. For the child to
/// access its own kernel stack (which we allocate here at a low-half
/// address), those mappings must already exist in the parent's PML4
/// at the moment deep_clone runs. So we allocate+map the kernel stack
/// FIRST, then create the AS + deep_clone (which inherits the new
/// mappings), then build the Task struct using that AS.
pub fn spawn_fork_child(parent_pid: Pid, saved_callee_saves: SavedCalleeSaves) -> Pid {
    // Keep the child unpublished to the scheduler until its stacks, address
    // space and parent link are ready. The syscall caller already disables
    // interrupts; other callers must also keep the supplied snapshot valid.
    let was_enabled = x86_64::instructions::interrupts::are_enabled();
    if was_enabled {
        x86_64::instructions::interrupts::disable();
    }

    // Step 1: allocate the child Process with a placeholder AS. We get
    // the child's pid out of this; we'll overwrite address_space below
    // once deep_clone has built the real one.
    let pid = {
        let mut table = PROCESS_TABLE.lock();
        let child_fd_table = table
            .get(parent_pid)
            .expect("spawn_fork_child: parent process missing")
            .fd_table
            .clone_shallow();
        let proc = Process::new(
            "fork-child",
            Some(parent_pid),
            // Placeholder — deep_clone_from_current() below produces the
            // real AS, then we write it back into the Process.
            memory::AddressSpace::from_current(),
            child_fd_table,
        );
        table.allocate(proc)
    };

    // Derive a unique stack range from the monotonically allocated PID.
    // Reaped process slots are never reused, so their ranges remain reserved.
    let id = pid.as_u64();

    // Compute stack region (same layout as Task::new).
    let stack_top = TASK_STACK_BASE + (id + 1) * STACK_SIZE;
    let stack_bottom = stack_top - STACK_SIZE;

    // Allocate 16 frames and map them into the virtual range, in the
    // CURRENT (parent's) address space.
    //
    // AddressSpace::from_current maps into the PML4 selected by CR3. The
    // global mapper still targets the boot-time table; after exec, these can
    // differ. These stack writes require mappings in the currently active one.
    let current_as = memory::AddressSpace::from_current();
    let mut frames_allocated = 0;
    let mut virt = stack_bottom;
    while virt < stack_top {
        let frame_number =
            frame_allocator::allocate().expect("out of frames while allocating fork-child stack");
        let phys = PhysAddr::new(frame_number * FRAME_SIZE);
        // SAFETY: virtual range is freshly picked, frame is fresh.
        unsafe {
            current_as
                .map_to(
                    VirtAddr::new(virt),
                    phys,
                    PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                )
                .expect("map_to failed while allocating fork-child stack");
        }
        frames_allocated += 1;
        virt += FRAME_SIZE;
    }

    // Allocate a dedicated entry stack for the child. Shared entry stacks
    // collide when the parent is preempted mid-syscall and the child then
    // makes its own syscalls: the child overwrites the parent's paused
    // state on the shared stack.
    //
    // Stack placement: SYSCALL_STACK_BOTTOM + (id+1) * SYSCALL_STACK_SIZE.
    // Main uses slot 0. A child uses slot pid+1, leaving slot 1 unused by
    // fork children. Kernel-task PIDs can also leave gaps in this range.
    let child_entry_stack_bottom = crate::arch::x86_64::syscall::SYSCALL_STACK_BOTTOM
        + (id + 1) * crate::arch::x86_64::syscall::SYSCALL_STACK_SIZE;
    let child_entry_stack_top =
        child_entry_stack_bottom + crate::arch::x86_64::syscall::SYSCALL_STACK_SIZE;
    {
        let mut virt = child_entry_stack_bottom;
        while virt < child_entry_stack_top {
            let frame_number = frame_allocator::allocate()
                .expect("out of frames while allocating child syscall stack");
            let phys = PhysAddr::new(frame_number * FRAME_SIZE);
            // SAFETY: virtual range is freshly picked, frame is fresh.
            // Uses `current_as` (live CR3) — see comment above the kernel-
            // stack loop for why the global `memory::map_to` is wrong here.
            unsafe {
                current_as
                    .map_to(
                        VirtAddr::new(virt),
                        phys,
                        PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                    )
                    .expect("map_to failed while allocating child syscall stack");
            }
            virt += FRAME_SIZE;
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
    let init_rsp = stack_top - saved_stack::SWITCH_BYTES;
    let base_ptr = init_rsp as *mut u64;
    unsafe {
        // Callee-saved area (consumed by switch's pop instructions).
        core::ptr::write_volatile(base_ptr.add(saved_stack::R15), saved_callee_saves.r15);
        core::ptr::write_volatile(base_ptr.add(saved_stack::R14), saved_callee_saves.r14);
        core::ptr::write_volatile(base_ptr.add(saved_stack::R13), saved_callee_saves.r13);
        core::ptr::write_volatile(base_ptr.add(saved_stack::R12), saved_callee_saves.r12);
        core::ptr::write_volatile(base_ptr.add(saved_stack::RBP), saved_callee_saves.rbp);
        core::ptr::write_volatile(base_ptr.add(saved_stack::RBX), saved_callee_saves.rbx);
        core::ptr::write_volatile(
            base_ptr.add(saved_stack::RETURN_ADDRESS),
            fork_trampoline as *const () as u64,
        );

        // Extra user-state slots below init_rsp. NOT touched by switch();
        // read by fork_trampoline after ret via fixed offsets from RSP.
        core::ptr::write_volatile(
            base_ptr.sub(saved_stack::USER_RIP_BELOW_FRAME),
            saved_callee_saves.user_rip,
        );
        core::ptr::write_volatile(
            base_ptr.sub(saved_stack::USER_FLAGS_BELOW_FRAME),
            saved_callee_saves.user_rflags,
        );
        core::ptr::write_volatile(
            base_ptr.sub(saved_stack::USER_RSP_BELOW_FRAME),
            saved_callee_saves.user_rsp,
        );
    }

    // Clone after mapping both child stacks. The low-half walk allocates
    // independent intermediate tables and copies 4 KiB user frames. Low-half
    // kernel frames (including both new stacks), huge pages and high-half
    // subtrees stay shared, as documented by deep_clone_from_current.
    let mut address_space = crate::memory::AddressSpace::new();
    address_space.deep_clone_from_current();

    // Write the real AS back into the child Process (replacing the
    // placeholder we allocated in step 1) and add the child to the
    // parent's children list. Both process-table mutations in one lock
    // acquisition.
    {
        let mut table = PROCESS_TABLE.lock();
        let child_proc = table
            .get_mut(pid)
            .expect("spawn_fork_child: child process disappeared");
        child_proc.address_space = address_space;
        let parent_proc = table
            .get_mut(parent_pid)
            .expect("spawn_fork_child: parent process disappeared");
        parent_proc.children.push(pid);
    }

    // Build the Task struct. entry_fn is unused for fork children (they
    // go through fork_trampoline, not trampoline), but the field needs
    // *something* to satisfy the struct.
    extern "C" fn fork_child_unused_entry() {
        panic!("fork_child_unused_entry: should never run (fork children use fork_trampoline)");
    }

    let task = Task {
        pid,
        state: TaskState::Ready,
        context: TaskContext { rsp: init_rsp },
        stack_top,
        entry_fn: fork_child_unused_entry,
        entry_stack_top: child_entry_stack_top,
    };

    let mut sched = SCHEDULER.lock();
    sched.tasks.push(Box::new(task));

    println!(
        "[sched] spawned fork-child pid={} (parent={}), stack=0x{:x}-0x{:x} ({} frames), context.rsp=0x{:x}",
        pid, parent_pid, stack_bottom, stack_top, frames_allocated, init_rsp,
    );

    // Drop the lock BEFORE re-enabling interrupts. Otherwise the timer
    // could fire between sti and the implicit drop-at-scope-exit, and the
    // timer handler would try to lock SCHEDULER → deadlock.
    drop(sched);

    // Restore the caller's interrupt state after publishing the ready child.
    if was_enabled {
        x86_64::instructions::interrupts::enable();
    }

    pid
}

/// Snapshot copied by sys_fork from arch::x86_64::syscall's global storage.
/// It contains six callee-saved registers plus the user return location and
/// flags. Copying it onto the child's stack isolates the child from later
/// changes to those globals. Preemption before the snapshot is taken remains
/// a separate concurrency concern; this struct does not resolve it.
pub struct SavedCalleeSaves {
    // The six callee-saved registers restored by the switch stack. Other
    // general-purpose registers are absent from the child's initial snapshot.
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
/// Used by getpid, fork, exit and other callers to select process state.
///
/// **Interrupt safety:** wraps the SCHEDULER access in `without_interrupts`
/// so the timer cannot fire mid-access and try to re-lock SCHEDULER from
/// the handler (which would deadlock). This is the Rust equivalent of
/// Linux's `spin_lock_irqsave` — disable interrupts on this CPU before
/// acquiring the lock, restore on release. All public SCHEDULER accessors
/// that syscall handlers can reach must do this.
pub fn current_pid() -> Pid {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let sched = SCHEDULER.lock();
        sched.tasks[sched.current].pid
    })
}

/// Mark the current task Blocked. Execution continues until the caller yields.
pub fn block_current() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let current = sched.current;
        sched.tasks[current].state = TaskState::Blocked;
    });
}

/// Publish a waiter and, when `register` returns true, mark the task Blocked
/// in one interrupt-disabled critical section. The callback runs with SCHEDULER
/// locked, so it must not yield or reacquire that lock. This function does not
/// switch tasks; the caller decides when to yield and how to protect that gap.
pub fn block_current_if(register: impl FnOnce(Pid) -> bool) -> bool {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = SCHEDULER.lock();
        let current = sched.current;
        let pid = sched.tasks[current].pid;
        if register(pid) {
            sched.tasks[current].state = TaskState::Blocked;
            true
        } else {
            false
        }
    })
}

/// Change a matching Blocked task to Ready. It runs on a later scheduling
/// decision. Missing PIDs and tasks in other states are left unchanged.
pub fn wake(pid: Pid) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(task) = SCHEDULER
            .lock()
            .tasks
            .iter_mut()
            .find(|task| task.pid == pid)
        {
            if task.state == TaskState::Blocked {
                task.state = TaskState::Ready;
            }
        }
    });
}

/// Set the matching task to Blocked, regardless of its previous state.
/// Callers must use this only for live tasks; it performs no state validation.
pub fn block(pid: Pid) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(task) = SCHEDULER
            .lock()
            .tasks
            .iter_mut()
            .find(|task| task.pid == pid)
        {
            task.state = TaskState::Blocked;
        }
    });
}

/// Read the current PID under SCHEDULER, release that lock, then run `f`
/// with its Process's descriptor table under PROCESS_TABLE. Interrupts stay
/// disabled across both lookups and the closure. `f` must not yield or reenter
/// PROCESS_TABLE; it may clone handles for use after this function returns.
pub fn with_current_fd_table<F, R>(f: F) -> R
where
    F: FnOnce(&mut crate::fs::FdTable) -> R,
{
    x86_64::instructions::interrupts::without_interrupts(|| {
        let pid = {
            let sched = SCHEDULER.lock();
            sched.tasks[sched.current].pid
        };
        let mut table = PROCESS_TABLE.lock();
        let proc = table
            .get_mut(pid)
            .expect("with_current_fd_table: current process missing");
        f(&mut proc.fd_table)
    })
}

/// Read the PML4 frame of the currently-running task's address space.
/// Used by `sys_exec` to switch CR3 to the (just-replaced) AS.
/// Returns the frame as a `PhysFrame` so it can be passed to `Cr3::write`.
///
/// **Interrupt safety:** see `current_pid` — `without_interrupts` around
/// the SCHEDULER access.
pub fn current_pml4_frame() -> x86_64::structures::paging::PhysFrame {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let pid = {
            let sched = SCHEDULER.lock();
            sched.tasks[sched.current].pid
        };
        let table = PROCESS_TABLE.lock();
        table
            .get(pid)
            .expect("current_pml4_frame: current process missing")
            .address_space
            .pml4_frame()
    })
}

/// Scan forward, wrapping around the task list, for the next runnable task.
/// Keep the current task if it is the only runnable one.
///
/// Used both for cooperative yields (a task calls this directly) and
/// preemptive yields (the timer interrupt handler calls this on the
/// current task's behalf). Same code path, same `switch`.
///
/// **Re-entrancy safety:** we disable interrupts at the top. The ordinary
/// return path restores their prior state; the no-runnable/blocked-task branch
/// enables interrupts to halt and returns without that restoration. Without this, if `yield_now` was running with
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
    // Scan forward from the current task to find the next runnable task.
    // This lets sys_exit mark the current task as Zombie and then yield,
    // knowing yield_now will pick somebody else (or halt if nobody can).
    let switch_result: Option<(*mut TaskContext, *const TaskContext, Pid, u64)> = {
        let mut sched = SCHEDULER.lock();
        // Note: no `len < 2` shortcut here. If current is Zombie/Blocked/Dead
        // and there's nothing else to run, we must halt — that check lives in
        // the `None` arm below and needs to fire even when len==1.
        if sched.tasks.is_empty() {
            None
        } else {
            let old_index = sched.current;
            // Find next runnable task. Skip Zombie (exited, awaiting reap),
            // Blocked (waiting for an event), and Dead (already reaped).
            let mut new_index_opt = None;
            for offset in 1..sched.tasks.len() {
                let candidate = (old_index + offset) % sched.tasks.len();
                let state = sched.tasks[candidate].state;
                if state.is_runnable() {
                    new_index_opt = Some(candidate);
                    break;
                }
            }
            match new_index_opt {
                None => {
                    // No other runnable task.
                    let current_state = sched.tasks[old_index].state;
                    if !current_state.is_runnable() {
                        let any_blocked = sched
                            .tasks
                            .iter()
                            .any(|task| task.state == TaskState::Blocked);
                        if any_blocked {
                            // Tasks are sleeping on events a device
                            // interrupt can deliver (a socket datagram, a
                            // disk completion). Sleep the CPU until the
                            // next interrupt, then let the caller rescan.
                            // enable_and_hlt uses STI;HLT to avoid a gap
                            // between enabling interrupts and halting.
                            // This branch returns with interrupts enabled and
                            // does not recheck the current task's state here.
                            drop(sched);
                            x86_64::instructions::interrupts::enable_and_hlt();
                            return;
                        }
                        // Every task is finished for good — nothing can
                        // wake them. System is effectively done.
                        println!(
                            "[sched] no runnable tasks (current state={:?}) — done",
                            current_state
                        );
                        drop(sched);
                        #[cfg(feature = "qemu-exit")]
                        {
                            // Report a clean idle shutdown through the QEMU
                            // target adapter.
                            crate::qemu_exit::exit_qemu(crate::qemu_exit::EXIT_SUCCESS);
                        }
                        #[cfg(not(feature = "qemu-exit"))]
                        {
                            // Interactive mode: halt forever so the user
                            // can read the serial output before killing
                            // QEMU manually.
                            x86_64::instructions::interrupts::disable();
                            loop {
                                x86_64::instructions::hlt();
                            }
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
                    if old_state.is_runnable() {
                        sched.tasks[old_index].state = TaskState::Ready;
                    }
                    sched.tasks[new_index].state = TaskState::Running;
                    sched.current = new_index;
                    let old_ptr = &mut sched.tasks[old_index].context as *mut TaskContext;
                    let new_ptr = &sched.tasks[new_index].context as *const TaskContext;
                    let new_pid = sched.tasks[new_index].pid;
                    let new_entry_rsp = sched.tasks[new_index].entry_stack_top;
                    Some((old_ptr, new_ptr, new_pid, new_entry_rsp))
                }
            }
        }
    };

    if let Some((old_ptr, new_ptr, new_pid, new_entry_rsp)) = switch_result {
        // Update KERNEL_RSP to the new task's entry stack. Kernel threads
        // use 0, which leaves KERNEL_RSP at the previous user task's
        // stack.
        if new_entry_rsp != 0 {
            crate::arch::x86_64::syscall::set_kernel_rsp(new_entry_rsp);
            // Mirror the same update into TSS.RSP0. The CPU uses RSP0 on
            // any ring-3 → ring-0 interrupt (timer firing in user mode).
            // If RSP0 stays at the previous task's stack, the next timer
            // preemption in user mode lands on the wrong stack and
            // corrupts whichever task "owns" it. See `gdt::set_rsp0`.
            crate::arch::x86_64::gdt::set_rsp0(new_entry_rsp);
        }

        let new_as_frame = {
            let table = PROCESS_TABLE.lock();
            table
                .get(new_pid)
                .expect("yield_now: next process missing")
                .address_space
                .pml4_frame()
                .start_address()
                .as_u64()
        };

        // Switch address space if the new task's PML4 differs from the
        // one currently in CR3. Skipping the write when ASes match saves
        // a TLB flush — important when switching between kernel threads
        // (which all share the kernel AS).
        let current_as_frame = x86_64::registers::control::Cr3::read()
            .0
            .start_address()
            .as_u64();
        if current_as_frame != new_as_frame {
            // SAFETY: new_as_frame came from a valid AddressSpace that
            // owns a properly initialized PML4. Writing CR3 to it is safe.
            unsafe {
                x86_64::registers::control::Cr3::write(
                    x86_64::structures::paging::PhysFrame::containing_address(
                        x86_64::PhysAddr::new(new_as_frame),
                    ),
                    x86_64::registers::control::Cr3Flags::empty(),
                );
            }
        }

        // SAFETY: pointers are valid (Box'd tasks), new context has
        // either valid saved state or faked initial state, interrupts
        // are disabled so no concurrent access.
        unsafe {
            switch(old_ptr, new_ptr);
        }
    }

    // We're back (either we switched away and have now been switched
    // back to, or we never switched because there was nothing to do).
    // Restore IF to its original state.
    if was_enabled {
        x86_64::instructions::interrupts::enable();
    }
}
