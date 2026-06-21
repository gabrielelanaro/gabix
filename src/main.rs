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

    // M3 slice 3.2: cooperative scheduling round trip.
    //   1. Register main as id=0 in the scheduler.
    //   2. Spawn worker as id=1.
    //   3. yield_now() switches main → worker.
    //   4. Worker prints, then yields back.
    //   5. Main resumes — we should see "back in main".
    println!("[test] cooperative yield round trip (3.2)");
    task::init_main();
    task::spawn("worker", worker_entry);
    println!("[main] about to yield to worker...");
    task::yield_now();
    println!("[main] back in main after yield");

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
    println!("[boot] entering main loop");

    loop {}
}

/// Example task entry function. Called by the scheduler when the worker
/// task first runs. Prints a message, then yields back to main. In real
/// use this would loop forever doing work, but for the round-trip test
/// we just yield once.
unsafe extern "C" fn worker_entry() {
    println!("[worker] running! yielding back...");
    task::yield_now();
    println!("[worker] resumed again — exiting");
}
