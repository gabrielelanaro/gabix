//! Kernel heap backed by a fixed RAM array. linked_list_allocator handles free
//! regions; this wrapper masks IRQs around its lock so a timer cannot switch to
//! another task that waits for the lock while the interrupted task still owns it.

use core::alloc::{GlobalAlloc, Layout};
use core::arch::asm;
use linked_list_allocator::LockedHeap;

const CPSR_IRQ_MASK: u32 = 1 << 7;
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

    /// Safety: the writable region must belong exclusively to this heap and
    /// remain valid for every allocation until the kernel stops using it.
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
    if status & CPSR_IRQ_MASK == 0 {
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
