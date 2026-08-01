//! Heap allocator for the kernel.
//!
//! Uses the `linked_list_allocator` crate, which keeps a linked list of free
//! blocks and supports actual freeing (unlike our earlier bump allocator).
//!
//! History: slice 2.3 shipped a hand-written `BumpAllocator` to learn the
//! `GlobalAlloc` plumbing — one pointer, no freeing. That proved `Box`,
//! `Vec`, and `String` worked end-to-end. Slice 2.3b swaps in this crate
//! so the heap doesn't fill up under repeated allocation.
//!
//! See docs/heap.html for the design of bump vs. linked-list allocators.

use core::alloc::{GlobalAlloc, Layout};
use linked_list_allocator::LockedHeap;

use crate::arch::interrupts;

pub struct IrqLockedHeap {
    inner: LockedHeap,
}

impl IrqLockedHeap {
    pub const fn empty() -> Self {
        Self {
            inner: LockedHeap::empty(),
        }
    }

    /// Initialize the heap after its virtual pages have been mapped.
    ///
    /// # Safety
    ///
    /// The heap range must be mapped, writable, and exclusively owned by
    /// this allocator.
    pub unsafe fn init(&self, heap_bottom: *mut u8, heap_size: usize) {
        interrupts::without_interrupts(|| unsafe {
            self.inner.lock().init(heap_bottom, heap_size)
        });
    }
}

unsafe impl GlobalAlloc for IrqLockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        interrupts::without_interrupts(|| unsafe { self.inner.alloc(layout) })
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        interrupts::without_interrupts(|| unsafe { self.inner.dealloc(ptr, layout) });
    }
}

/// The kernel's global allocator. Marking this `#[global_allocator]` means
/// every `Box::new`, `vec!`, `String::from`, etc. in the kernel routes
/// through it.
///
/// Starts empty (no heap region). `memory::init_heap` calls
/// `ALLOCATOR.lock().init(start, size)` once the heap pages are mapped.
#[global_allocator]
pub static ALLOCATOR: IrqLockedHeap = IrqLockedHeap::empty();
