//! Task abstraction: data structures + constructor.
//!
//! This slice (3.1) defines `Task`, `TaskState`, `TaskContext`, and
//! `Task::new`. Context switching (3.2) and the scheduler (3.3) come
//! next — for now we just need to be able to *create* a task.
//!
//! See `docs/context-switch.html` for the visual walkthrough of how
//! the saved state works.

use crate::frame_allocator;
use crate::memory;
use crate::println;
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
    /// Waiting for something (sleep, I/O) — not yet used.
    Blocked,
    /// Finished, can be cleaned up.
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

/// A schedulable task.
pub struct Task {
    pub id: u64,
    pub name: &'static str,
    pub state: TaskState,
    pub context: TaskContext,
    /// Virtual address of the very top of this task's stack. Kept so we
    /// can free the stack frames later when the task exits.
    pub stack_top: u64,
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
            core::ptr::write_volatile(base_ptr.add(0), 0); // saved R15
            core::ptr::write_volatile(base_ptr.add(1), 0); // saved R14
            core::ptr::write_volatile(base_ptr.add(2), 0); // saved R13
            core::ptr::write_volatile(base_ptr.add(3), 0); // saved R12
            core::ptr::write_volatile(base_ptr.add(4), 0); // saved RBP
            core::ptr::write_volatile(base_ptr.add(5), 0); // saved RBX
            core::ptr::write_volatile(base_ptr.add(6), entry_fn as u64); // RIP (popped by ret)
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
        }
    }
}
