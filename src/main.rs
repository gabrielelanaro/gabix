#![no_std] // Don't link the Rust standard library.
#![no_main] // Don't use the normal Rust entry point chain.
#![feature(alloc_error_handler)] // Required for `#[alloc_error_handler]` on nightly.

// Pull in `alloc` (Box, Vec, String, ...) — it's not included automatically
// in `no_std` crates. Requires our `#[global_allocator]` to be wired up.
extern crate alloc;

mod vga_buffer;
mod serial;
mod gdt;
mod interrupts;
mod pic;
mod pit;
mod frame_allocator;
mod memory;
mod allocator;
mod task;

use bootloader::BootInfo;
use core::alloc::Layout;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU64, Ordering};

/// Called on panic. Prints the panic message and location, then halts.
///
/// If we panicked while WRITER was locked (e.g., during a println! call),
/// the spinlock would deadlock. We avoid this by force_unlocking WRITER
/// before printing — we're about to halt anyway, so safety doesn't matter.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe {
        vga_buffer::WRITER.force_unlock();
        serial::SERIAL1.force_unlock();
    }
    println!("{}", info);
    loop {}
}

/// Called when the global allocator can't satisfy a request (out of memory).
/// The default behavior loops forever silently — we'd rather see why.
#[alloc_error_handler]
fn alloc_error(layout: Layout) -> ! {
    panic!("allocation failed: size={}, align={}", layout.size(), layout.align());
}

/// The kernel entry point. The bootloader jumps here after switching
/// to long mode and setting up paging. It passes us a pointer to a
/// `BootInfo` struct in `rdi` (first arg per the System V AMD64 ABI).
#[no_mangle]
pub extern "C" fn _start(boot_info: &'static BootInfo) -> ! {
    println!("[boot] gdt...");
    gdt::init();
    println!("[boot] idt...");
    interrupts::init();
    println!("[boot] pic...");
    pic::init();
    println!("[boot] pit...");
    pit::set_frequency(100); // 100 Hz = Linux default; handler throttles to ~1 print/sec
    println!("[boot] hello");
    println!("Hello, World{}", "!");

    println!("[boot] memory map:");
    for region in boot_info.memory_map.iter() {
        // Each line: physical start - physical end (exclusive), and the kind.
        // The "Usable" regions are what we'll later feed to the frame allocator.
        // The bootloader even tags kernel/stack/page-table/bootloader regions
        // separately, so we know which "Usable" frames are actually free.
        println!(
            "  {:>16x} - {:>16x}   {:?}",
            region.range.start_addr(),
            region.range.end_addr(),
            region.region_type,
        );
    }

    println!("[boot] frame allocator...");
    frame_allocator::init(&boot_info.memory_map);

    println!("[boot] memory mapper...");
    memory::init(boot_info);

    // M3 slice 3.4: preemptive scheduling setup.
    //   1. Register main as id=0.
    //   2. Spawn two workers that loop forever incrementing counters.
    //   3. (After the rest of init) enable interrupts and let the timer
    //      preempt everyone fairly. Workers never call yield_now — the
    //      only way they trade places with main is via the timer.
    println!("[test] spawning workers for preemptive scheduling (3.4)");
    task::init_main();
    task::spawn("worker_a", worker_a_entry);
    task::spawn("worker_b", worker_b_entry);

    // Sanity test: Box::new goes through our #[global_allocator].
    // If the heap isn't set up correctly, this calls alloc_error_handler.
    println!("[test] heap allocator (Box, Vec, String)");
    {
        let x: alloc::boxed::Box<u32> = alloc::boxed::Box::new(42);
        println!("[test]   Box<u32> = {}, at {:p}", *x, &*x);

        let v: alloc::vec::Vec<u8> = alloc::vec![1u8, 2, 3, 4, 5];
        println!("[test]   Vec<u8> len {} = {:?}", v.len(), &v[..]);

        let s: alloc::string::String = alloc::string::String::from("hello heap");
        println!("[test]   String = {:?} (len {})", s, s.len());
    }

    // Free-reuse test: allocate 20,000 Boxes in a loop, dropping each.
    // Total bytes if not freed: 20000 × 8 = 160 KB > our 100 KB heap.
    // Bump allocator would panic around iteration 12,800. The linked-list
    // allocator reuses freed blocks, so this completes cleanly.
    println!("[test] free-reuse loop (20000 Boxes, would crash bump allocator)");
    let mut total: u64 = 0;
    for i in 0..20_000u64 {
        let b: alloc::boxed::Box<u64> = alloc::boxed::Box::new(i);
        total = total.wrapping_add(*b);
    }
    println!("[test]   completed; sum (mod 2^64) = {}", total);

    // Sanity test: allocate three frames, free the middle one, allocate
    // again — the new allocation should reuse the freed frame (lowest free
    // frame wins in a linear scan).
    println!("[test] frame allocator reuse check");
    let a = frame_allocator::allocate();
    let b = frame_allocator::allocate();
    let c = frame_allocator::allocate();
    println!("[test]   allocated a={:?}, b={:?}, c={:?}", a, b, c);
    println!("[test]   {} frames free after 3 allocations", frame_allocator::frames_free());

    if let Some(f) = b {
        frame_allocator::deallocate(f);
        println!("[test]   freed b={:?}", f);
    }
    let d = frame_allocator::allocate();
    println!("[test]   allocated d={:?} (should equal previous b)", d);

    if let Some(f) = a {
        frame_allocator::deallocate(f);
        println!("[test]   freed a={:?}", f);
    }
    let e = frame_allocator::allocate();
    println!("[test]   allocated e={:?} (should equal previous a)", e);

    println!("[test]   {} frames free after test", frame_allocator::frames_free());

    println!("[boot] enabling interrupts");
    x86_64::instructions::interrupts::enable();
    println!(
        "[boot] COUNTER_A at {:p}, initial = {}",
        &COUNTER_A,
        COUNTER_A.load(Ordering::Relaxed)
    );
    println!("[boot] entering main monitoring loop");

    // Monitoring loop: periodically print the worker counters. If both
    // counters are increasing, it means both workers got CPU time —
    // which can only happen via preemption (they never yield on their own).
    let mut last_total_millions: u64 = 0;
    let mut loop_count = 0u64;
    loop {
        let a = COUNTER_A.load(Ordering::Relaxed);
        let b = COUNTER_B.load(Ordering::Relaxed);
        let total_millions = (a.saturating_add(b)) / 1_000_000;
        if total_millions > last_total_millions || loop_count % 5_000_000 == 0 {
            println!("[main] iter={} counters: a={}, b={}", loop_count, a, b);
            last_total_millions = total_millions;
        }
        loop_count += 1;
    }
}

/// Counter incremented by worker_a every loop iteration. Atomic so it's
/// safe to read from main while worker_a writes (no Mutex needed).
static COUNTER_A: AtomicU64 = AtomicU64::new(0);

/// Counter incremented by worker_b.
static COUNTER_B: AtomicU64 = AtomicU64::new(0);

/// Worker A entry function. Loops forever incrementing a counter. Never
/// calls yield_now — any context switching here is the timer's doing.
unsafe extern "C" fn worker_a_entry() {
    loop {
        COUNTER_A.fetch_add(1, Ordering::Relaxed);
    }
}

/// Worker B entry function. Same as A but for a separate counter.
unsafe extern "C" fn worker_b_entry() {
    loop {
        COUNTER_B.fetch_add(1, Ordering::Relaxed);
    }
}
