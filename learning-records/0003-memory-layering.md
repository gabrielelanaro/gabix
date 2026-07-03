# Memory layering — three allocators over one physical RAM

User demonstrated clear understanding (during Lesson 2 sidebar discussion)
that **stack, heap, and frame allocator are different granularities over
the same physical RAM, not separate memory pools.**

Specifically absorbed:
- Frame allocator (bitmap) manages all ~120 MB of usable RAM as 4 KB frames
- Heap (`linked_list_allocator`) manages a 100 KB region (25 frames worth) and itself gets its backing from the frame allocator via `init_heap`
- Stack is per-task with a fixed 64 KB region set up at task creation
- The heap is *one consumer* of frames, not a separate memory space

User correctly intuited that "file data in memory" doesn't mean "file data in heap" — Linux's ramfs uses frames (via the buddy page allocator), not the heap. The heap (slab in Linux) is reserved for small kernel objects.

**Implications for future sessions:**
- Can refer to "the heap" vs "frames" without re-explaining the layering
- When discussing allocator choice for new abstractions, user will track which layer is appropriate (sub-page → heap, page-granular → frames)
- The page-cache discussion (file data lives in frames via address_space, not in heap) is now baseline vocabulary

See [[0004-allocator-routing]] for the follow-up about Vec/Box both using the heap.
