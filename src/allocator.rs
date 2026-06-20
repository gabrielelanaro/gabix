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

use linked_list_allocator::LockedHeap;

/// The kernel's global allocator. Marking this `#[global_allocator]` means
/// every `Box::new`, `vec!`, `String::from`, etc. in the kernel routes
/// through it.
///
/// Starts empty (no heap region). `memory::init_heap` calls
/// `ALLOCATOR.lock().init(start, size)` once the heap pages are mapped.
#[global_allocator]
pub static ALLOCATOR: LockedHeap = LockedHeap::empty();
