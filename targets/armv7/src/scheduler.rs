//! Single-core kernel tasks with explicit yields and timer preemption.
//! Scheduler mutations and context switches run with IRQs masked.
use crate::context::{switch, TaskContext, SAVED_REGISTER_WORDS, SAVED_RETURN_ADDRESS_WORD};
use core::arch::asm;
use core::sync::atomic::{AtomicUsize, Ordering};

const MAX_TASKS: usize = 4;
// Slot indices are 0..MAX_TASKS; this value identifies the dispatcher.
const NO_CURRENT_TASK: usize = MAX_TASKS;
const STACK_SIZE: usize = 16 * 1024;
const CPSR_IRQ_MASK: u32 = 1 << 7; // I bit: one disables IRQ exceptions.

#[derive(Clone, Copy, PartialEq)]
enum TaskState {
    Empty,
    Ready,
    Running,
    Finished,
}

#[derive(Clone, Copy)]
struct Task {
    context: TaskContext,
    state: TaskState,
    entry: Option<fn()>,
    process: Option<usize>,
}
impl Task {
    const EMPTY: Self = Self {
        context: TaskContext::empty(),
        state: TaskState::Empty,
        entry: None,
        process: None,
    };
}
#[repr(C, align(8))]
struct TaskStack([u8; STACK_SIZE]);
// Fixed storage lives for the entire kernel run; reset only frees task slots.
static mut STACKS: [TaskStack; MAX_TASKS] = [const { TaskStack([0; STACK_SIZE]) }; MAX_TASKS];
static mut TASKS: [Task; MAX_TASKS] = [Task::EMPTY; MAX_TASKS];
static mut DISPATCHER: TaskContext = TaskContext::empty();
static mut CURRENT_TASK: usize = NO_CURRENT_TASK;

// A switched-out task resumes with IRQs masked, then restores its own prior
// IRQ setting. The dispatcher always runs masked; new tasks inherit this setting.
static mut TASK_IRQS_ENABLED: bool = false;
static PREEMPTIONS: AtomicUsize = AtomicUsize::new(0);

/// Disable IRQ delivery and return whether it was enabled before this call.
unsafe fn mask_irqs() -> bool {
    let status: u32;
    asm!("mrs {status}, cpsr", "cpsid i", status = out(reg) status);
    status & CPSR_IRQ_MASK == 0
}

// Caller must still have IRQs masked. A false argument leaves that state alone.
unsafe fn restore_irqs(were_enabled: bool) {
    if were_enabled {
        asm!("cpsie i", "isb");
    }
}

/// Called after acknowledging the timer, with the IRQ frame on this task's
/// SVC stack. yield_now saves the handler's call chain on that same stack.
/// Returning here later lets the assembly wrapper restore the interrupted code.
pub unsafe fn preempt_on_timer() {
    if CURRENT_TASK < MAX_TASKS {
        PREEMPTIONS.fetch_add(1, Ordering::Relaxed);
        if let Some(id) = current_process() {
            crate::process::record_preemption(id);
        }
        yield_now();
    }
}

/// Register a task with a private stack. Finished slots are reused only by reset.
/// Safety: single core, outside a running scheduler. IRQs are masked here.
pub unsafe fn spawn(entry: fn()) -> Option<usize> {
    let were_enabled = mask_irqs();
    if CURRENT_TASK != NO_CURRENT_TASK {
        restore_irqs(were_enabled);
        return None;
    }
    for id in 0..MAX_TASKS {
        let task = core::ptr::addr_of_mut!(TASKS).cast::<Task>().add(id);
        if (*task).state != TaskState::Empty {
            continue;
        }
        let stack = core::ptr::addr_of_mut!(STACKS).cast::<TaskStack>().add(id);
        let top = stack.cast::<u8>().add(STACK_SIZE).cast::<u32>();
        let saved = top.sub(SAVED_REGISTER_WORDS);
        for word in 0..SAVED_REGISTER_WORDS {
            saved.add(word).write(0);
        }
        // Build the frame that context::switch will pop. Its saved lr starts
        // a fresh task at task_entry; later switches resume a suspended call.
        // Popping all nine words restores the eight-byte-aligned stack top.
        saved
            .add(SAVED_RETURN_ADDRESS_WORD)
            .write(task_entry as *const () as u32);
        (*task).context.sp = saved as u32;
        (*task).entry = Some(entry);
        (*task).state = TaskState::Ready;
        restore_irqs(were_enabled);
        return Some(id);
    }
    restore_irqs(were_enabled);
    None
}

/// Run ready tasks in slot order until a full scan finds none.
/// A yielding task becomes Ready and is selected on a later scan; there is no
/// blocked/sleeping state. If one task never yields and IRQs are disabled, this
/// call cannot regain control. New tasks inherit the caller's IRQ setting.
/// Safety: single core, initialized mapping tables, no nested scheduler run.
pub unsafe fn run_until_finished() {
    let were_enabled = mask_irqs();
    assert!(CURRENT_TASK == NO_CURRENT_TASK);
    TASK_IRQS_ENABLED = were_enabled;
    loop {
        let mut ran_task = false;
        for id in 0..MAX_TASKS {
            let task = core::ptr::addr_of_mut!(TASKS).cast::<Task>().add(id);
            if (*task).state != TaskState::Ready {
                continue;
            }
            ran_task = true;
            CURRENT_TASK = id;
            (*task).state = TaskState::Running;
            // Switch address spaces before stacks. Kernel code and every kernel
            // stack have identical mappings in each process table.
            crate::memory::activate_process((*task).process);
            // Keep raw pointers, not live mutable Rust references, across a switch.
            switch(
                core::ptr::addr_of_mut!(DISPATCHER),
                core::ptr::addr_of!((*task).context),
            );
            // The task yielded or finished. We are back on the dispatcher stack
            // with IRQs masked, but still using that task's address space.
            CURRENT_TASK = NO_CURRENT_TASK;
            crate::memory::activate_process(None);
        }
        if !ran_task {
            restore_irqs(were_enabled);
            return;
        }
    }
}

/// Save this task's call stack and mark it ready for another turn.
/// The suspended call keeps its own previous IRQ setting on its kernel stack.
/// Timer callers resume inside the IRQ handler; its epilogue restores user state.
pub fn yield_now() {
    unsafe {
        let were_enabled = mask_irqs();
        assert!(CURRENT_TASK < MAX_TASKS);
        let task = core::ptr::addr_of_mut!(TASKS)
            .cast::<Task>()
            .add(CURRENT_TASK);
        (*task).state = TaskState::Ready;
        switch(
            core::ptr::addr_of_mut!((*task).context),
            core::ptr::addr_of!(DISPATCHER),
        );
        // Execution resumes here when the dispatcher selects this task again.
        restore_irqs(were_enabled);
    }
}

extern "C" fn task_entry() -> ! {
    unsafe {
        let task = core::ptr::addr_of_mut!(TASKS)
            .cast::<Task>()
            .add(CURRENT_TASK);
        let entry = (*task).entry.expect("registered task has an entry");
        restore_irqs(TASK_IRQS_ENABLED);
        entry();
        mask_irqs();
        (*task).state = TaskState::Finished;
        switch(
            core::ptr::addr_of_mut!((*task).context),
            core::ptr::addr_of!(DISPATCHER),
        );
        unreachable!("finished tasks are never resumed");
    }
}

static mut TRACE: [u32; 6] = [0; 6];
static mut TRACE_LEN: usize = 0;
fn record(value: u32) {
    unsafe {
        TRACE[TRACE_LEN] = value;
        TRACE_LEN += 1;
    }
}
fn first_task() {
    let mut local = 10;
    for _ in 0..3 {
        record(local);
        local += 1;
        yield_now();
    }
}
fn second_task() {
    let mut local = 20;
    for _ in 0..2 {
        record(local);
        local += 1;
        yield_now();
    }
    record(local);
}

/// Boot-time behavioral check: independent local counters, round-robin yields,
/// task return, and a second scheduler run after every task has finished.
pub unsafe fn task_resumes() -> bool {
    TASKS = [Task::EMPTY; MAX_TASKS];
    CURRENT_TASK = NO_CURRENT_TASK;
    TRACE_LEN = 0;
    if spawn(first_task) != Some(0) || spawn(second_task) != Some(1) {
        return false;
    }
    run_until_finished();
    run_until_finished();
    TRACE_LEN == 6
        && TRACE == [10, 20, 11, 21, 12, 22]
        && TASKS[0].state == TaskState::Finished
        && TASKS[1].state == TaskState::Finished
}

// Each task waits for progress from the other task. Neither task yields.
// With preemption broken, the first task waits forever and the test times out.
const PREEMPTION_PROBE_ROUNDS: usize = 8;
static PROGRESS: [AtomicUsize; 2] = [const { AtomicUsize::new(0) }; 2];
fn wait_for_other_task(id: usize) {
    for round in 1..=PREEMPTION_PROBE_ROUNDS {
        PROGRESS[id].store(round, Ordering::SeqCst);
        while PROGRESS[1 - id].load(Ordering::SeqCst) < round {
            core::hint::spin_loop();
        }
    }
}
fn first_busy_task() {
    wait_for_other_task(0);
}
fn second_busy_task() {
    wait_for_other_task(1);
}

/// Requires an initialized timer and enabled IRQs. Call outside the scheduler.
/// Repeated handshakes prove both non-yielding tasks resume across timer IRQs.
pub unsafe fn timer_preempts_tasks() -> bool {
    TASKS = [Task::EMPTY; MAX_TASKS];
    PROGRESS[0].store(0, Ordering::SeqCst);
    PROGRESS[1].store(0, Ordering::SeqCst);
    PREEMPTIONS.store(0, Ordering::SeqCst);
    if spawn(first_busy_task) != Some(0) || spawn(second_busy_task) != Some(1) {
        return false;
    }
    run_until_finished();
    PROGRESS[0].load(Ordering::SeqCst) == PREEMPTION_PROBE_ROUNDS
        && PROGRESS[1].load(Ordering::SeqCst) == PREEMPTION_PROBE_ROUNDS
        && PREEMPTIONS.load(Ordering::SeqCst) >= PREEMPTION_PROBE_ROUNDS
        && TASKS[0].state == TaskState::Finished
        && TASKS[1].state == TaskState::Finished
}

/// The current process identity comes from scheduler-owned state, never userspace.
pub unsafe fn current_process() -> Option<usize> {
    if CURRENT_TASK < MAX_TASKS {
        TASKS[CURRENT_TASK].process
    } else {
        None
    }
}

/// Reuse task slots only after every task is Empty or Finished.
/// Stack bytes remain in static storage; future spawn() writes a fresh frame.
/// Safety: single core, outside the dispatcher and all task execution.
pub unsafe fn reset_finished_tasks() {
    let enabled = mask_irqs();
    assert!(CURRENT_TASK == NO_CURRENT_TASK);
    let tasks = TASKS;
    assert!(tasks
        .iter()
        .all(|task| matches!(task.state, TaskState::Empty | TaskState::Finished)));
    TASKS = [Task::EMPTY; MAX_TASKS];
    restore_irqs(enabled);
}

/// Attach a prepared process identity to a new kernel task while IRQs are masked.
/// Safety: id must identify a prepared process table, and the same process must
/// not already have another task. Neither requirement is checked here.
pub unsafe fn spawn_process(id: usize) -> Option<usize> {
    let enabled = mask_irqs();
    let result = spawn(crate::process::enter_current_process);
    if let Some(slot) = result {
        TASKS[slot].process = Some(id);
    }
    restore_irqs(enabled);
    result
}

/// Exit/fault abandons the current kernel call chain by returning to DISPATCHER.
/// Finished tasks are skipped. Their static stacks can be reused after reset;
/// this does not run destructors for locals on the abandoned call chain.
/// Safety: called from the current process on its SVC stack, on the single core.
pub unsafe fn finish_current_process() -> ! {
    mask_irqs();
    assert!(current_process().is_some());
    let task = core::ptr::addr_of_mut!(TASKS)
        .cast::<Task>()
        .add(CURRENT_TASK);
    (*task).state = TaskState::Finished;
    switch(
        core::ptr::addr_of_mut!((*task).context),
        core::ptr::addr_of!(DISPATCHER),
    );
    unreachable!("finished process resumed");
}
