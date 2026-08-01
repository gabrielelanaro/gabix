use core::alloc::{GlobalAlloc, Layout};
use core::arch::asm;
use linked_list_allocator::LockedHeap;

const HEAP_SIZE: usize = 1024 * 1024;

#[repr(C, align(16))]
struct HeapStorage([u8; HEAP_SIZE]);

static mut HEAP_STORAGE: HeapStorage = HeapStorage([0; HEAP_SIZE]);

pub struct IrqLockedHeap {
    inner: LockedHeap,
}

impl IrqLockedHeap {
    pub const fn empty() -> Self {
        Self {
            inner: LockedHeap::empty(),
        }
    }

    pub unsafe fn init(&self, start: *mut u8, size: usize) {
        without_interrupts(|| self.inner.lock().init(start, size));
    }
}

fn without_interrupts<R>(operation: impl FnOnce() -> R) -> R {
    let status: u32;
    unsafe {
        asm!("mrs {value}, cpsr", value = out(reg) status);
        asm!("cpsid i");
    }

    let result = operation();
    if status & (1 << 7) == 0 {
        unsafe {
            asm!("cpsie i", "isb");
        }
    }
    result
}

unsafe impl GlobalAlloc for IrqLockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        without_interrupts(|| self.inner.alloc(layout))
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        without_interrupts(|| self.inner.dealloc(ptr, layout));
    }
}

#[global_allocator]
static ALLOCATOR: IrqLockedHeap = IrqLockedHeap::empty();

pub unsafe fn init() {
    ALLOCATOR.init(
        core::ptr::addr_of_mut!(HEAP_STORAGE).cast::<u8>(),
        HEAP_SIZE,
    );
}
