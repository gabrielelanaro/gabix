//! Heap allocator: lets us use `Box`, `Vec`, `String`, etc. in the kernel.
//!
//! This is a **bump allocator** — the dumbest possible design. One pointer
//! (`next`) moves forward on each allocation. Frees are no-ops. Once the
//! heap fills up, allocations fail. Good enough to prove the plumbing; we
//! swap in a real allocator (`linked_list_allocator`) later.
//!
//! See docs/heap.html for the visual walkthrough of this design and the
//! alternatives.

use core::alloc::{GlobalAlloc, Layout};
use spin::Mutex;

/// The heap allocator. Wrap the actual state in a Mutex so concurrent
/// allocations (eventually from interrupt handlers too) don't corrupt it.
pub struct BumpAllocator {
    inner: Mutex<BumpInner>,
}

struct BumpInner {
    /// Virtual address of the first byte of the heap.
    heap_start: usize,
    /// Virtual address one past the last byte of the heap.
    heap_end: usize,
    /// Next free byte. Allocations come from here and bump it forward.
    next: usize,
    /// Number of bytes currently allocated. Useful for diagnostics.
    allocated: usize,
}

impl BumpAllocator {
    /// Construct an empty allocator. Real initialization happens via
    /// `init()` once the heap region is mapped.
    pub const fn new() -> Self {
        BumpAllocator {
            inner: Mutex::new(BumpInner {
                heap_start: 0,
                heap_end: 0,
                next: 0,
                allocated: 0,
            }),
        }
    }

    /// Set the heap range. Call this exactly once, after the heap region
    /// is mapped into the address space.
    ///
    /// SAFETY: caller must guarantee `[heap_start, heap_start + heap_size)`
    /// is mapped, writable, and not used for anything else.
    pub unsafe fn init(&self, heap_start: usize, heap_size: usize) {
        let mut inner = self.inner.lock();
        inner.heap_start = heap_start;
        inner.heap_end = heap_start + heap_size;
        inner.next = heap_start;
        inner.allocated = 0;
    }

    /// Bytes still available for new allocations.
    pub fn bytes_free(&self) -> usize {
        let inner = self.inner.lock();
        inner.heap_end.saturating_sub(inner.next)
    }

    /// Total bytes ever allocated (since init). Goes up only — bump
    /// allocator never frees.
    pub fn bytes_allocated(&self) -> usize {
        self.inner.lock().allocated
    }
}

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut inner = self.inner.lock();

        // Bump `next` to the next aligned position that fits.
        let align = layout.align();
        let size = layout.size();
        let aligned_next = (inner.next + align - 1) & !(align - 1);
        let new_next = aligned_next.checked_add(size).unwrap_or(usize::MAX);

        if new_next > inner.heap_end {
            return core::ptr::null_mut();
        }

        inner.next = new_next;
        inner.allocated += size;
        aligned_next as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator: no-op. Memory is reclaimed only by resetting the
        // whole heap (which we never do). Once full, we're full.
    }
}

/// The kernel's global allocator. Marking this `#[global_allocator]` means
/// every `Box::new`, `vec!`, `String::from`, etc. in the kernel routes
/// through it.
#[global_allocator]
pub static ALLOCATOR: BumpAllocator = BumpAllocator::new();
