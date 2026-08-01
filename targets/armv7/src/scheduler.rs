use crate::context::{switch, TaskContext};
use core::arch::asm;
use core::sync::atomic::{AtomicUsize, Ordering};

const STACK_SIZE: usize = 16 * 1024;
const SAVED_REGISTER_COUNT: usize = 9;

#[repr(C, align(8))]
struct TaskStack([u8; STACK_SIZE]);

static mut MAIN_CONTEXT: TaskContext = TaskContext::empty();
static mut WORKER_CONTEXT: TaskContext = TaskContext::empty();
static mut WORKER_STACK: TaskStack = TaskStack([0; STACK_SIZE]);
static STAGE: AtomicUsize = AtomicUsize::new(0);

unsafe fn prepare_worker() {
    let stack_bottom = core::ptr::addr_of_mut!(WORKER_STACK).cast::<u8>();
    let stack_top = (stack_bottom.add(STACK_SIZE) as usize & !7) as *mut u32;
    let saved = stack_top.sub(SAVED_REGISTER_COUNT);

    for index in 0..SAVED_REGISTER_COUNT - 1 {
        saved.add(index).write(0);
    }
    saved
        .add(SAVED_REGISTER_COUNT - 1)
        .write(worker as *const () as u32);
    WORKER_CONTEXT.sp = saved as u32;
}

unsafe extern "C" fn worker() -> ! {
    STAGE.store(1, Ordering::Release);
    switch(
        core::ptr::addr_of_mut!(WORKER_CONTEXT),
        core::ptr::addr_of!(MAIN_CONTEXT),
    );

    STAGE.store(2, Ordering::Release);
    switch(
        core::ptr::addr_of_mut!(WORKER_CONTEXT),
        core::ptr::addr_of!(MAIN_CONTEXT),
    );

    loop {
        asm!("wfi");
    }
}

pub unsafe fn task_resumes() -> bool {
    prepare_worker();
    switch(
        core::ptr::addr_of_mut!(MAIN_CONTEXT),
        core::ptr::addr_of!(WORKER_CONTEXT),
    );
    if STAGE.load(Ordering::Acquire) != 1 {
        return false;
    }

    switch(
        core::ptr::addr_of_mut!(MAIN_CONTEXT),
        core::ptr::addr_of!(WORKER_CONTEXT),
    );
    STAGE.load(Ordering::Acquire) == 2
}
