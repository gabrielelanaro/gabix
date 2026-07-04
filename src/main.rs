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
mod user;
mod syscall;
mod fs;
mod pci;
mod virtio;
mod ext2;

use bootloader::BootInfo;
use alloc::vec::Vec;
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
    // Enable SSE support: set CR4.OSFXSR (bit 9) and CR4.OSXMMEXCPT (bit 10).
    // The bootloader leaves CR4 = 0x20 (only PAE). Without OSFXSR, the
    // Intel SDM says SSE instructions should #UD — QEMU is lax about this
    // most of the time but seems to enforce it in syscall entry context.
    unsafe {
        core::arch::asm!(
            "mov rax, cr4",
            "or rax, 0x600",      // bit 9 (OSFXSR) + bit 10 (OSXMMEXCPT)
            "mov cr4, rax",
            out("rax") _,
            options(nomem, nostack, preserves_flags),
        );
    }
    let cr4: u64;
    unsafe { core::arch::asm!("mov {}, cr4", out(reg) cr4); }
    println!("[boot] CR4 after SSE enable = 0x{:x}", cr4);
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

    // Slice 4.4.a test: create a fresh AddressSpace (kernel-only entries
    // 256-511), deep-clone the current AS into it (adds user-half walking
    // for low-half kernel mappings like VGA and heap), switch to it, print
    // something, switch back. This is the same pattern fork/exec uses.
    println!("[test] AddressSpace create + deep_clone + switch");
    {
        let original_cr3 = x86_64::registers::control::Cr3::read();
        let mut new_as = memory::AddressSpace::new();
        new_as.deep_clone_from_current();
        new_as.switch_to();
        println!(
            "[test]   hello from new AS! (CR3 was 0x{:x}, now 0x{:x})",
            original_cr3.0.start_address().as_u64(),
            x86_64::registers::control::Cr3::read().0.start_address().as_u64(),
        );
        // Switch back to the original so the rest of boot uses the kernel AS.
        unsafe {
            x86_64::registers::control::Cr3::write(
                original_cr3.0,
                original_cr3.1,
            );
        }
        println!("[test]   switched back to original CR3");
    }

    // M3 slice 3.4 + 3.5: preemptive scheduling with an idle task.
    // For slice 3.6.b (ring transition test) we don't spawn workers —
    // we want a clean isolation of the user-mode test. main will call
    // enter_user_mode directly.
    println!("[test] registering main task only (3.6.b isolation)");
    task::init_main();

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

    println!("[test] dirent encode/decode");
    {
        use crate::fs::{encode_dirent, lookup_in_dir};
        let mut dir = Vec::new();
        dir.extend_from_slice(&encode_dirent("foo.txt", 17));
        dir.extend_from_slice(&encode_dirent("hello", 42));

        let foo_ino = lookup_in_dir(&dir, "foo.txt");
        let hello_ino = lookup_in_dir(&dir, "hello");
        let missing_ino = lookup_in_dir(&dir, "missing");
        let empty_ino = lookup_in_dir(&[], "anything");

        println!("[test]   lookup foo.txt -> {:?} (expect Some(17))", foo_ino);
        println!("[test]   lookup hello   -> {:?} (expect Some(42))", hello_ino);
        println!("[test]   lookup missing -> {:?} (expect None)", missing_ino);
        println!("[test]   lookup in empty -> {:?} (expect None)", empty_ino);

        assert_eq!(foo_ino, Some(17));
        assert_eq!(hello_ino, Some(42));
        assert_eq!(missing_ino, None);
        assert_eq!(empty_ino, None);
    }

    println!("[test] filesystem setup (3.3)");
    {
        use crate::fs::{lookup_in_dir, Inode, InodeKind, HARDCODED_INODE, ROOT_INODE};

        let root = ROOT_INODE.clone(); // triggers lazy_static init
        println!("[test]   root inode: ino={} kind={:?}", root.ino, root.kind);

        let root_data = root.data.lock();
        let hello_ino_in_root = lookup_in_dir(&root_data, "hello.txt");
        println!("[test]   lookup 'hello.txt' in root -> {:?}", hello_ino_in_root);
        drop(root_data);

        assert_eq!(root.ino, 1);
        assert_eq!(root.kind, InodeKind::Dir);
        assert_eq!(hello_ino_in_root, Some(2));

        let hello_inode = Inode::lookup(2).expect("hello.txt inode should be registered");
        let hello_data = hello_inode.data.lock();
        println!(
            "[test]   inode 2: kind={:?}, {} bytes: {:?}",
            hello_inode.kind,
            hello_data.len(),
            &*hello_data
        );
        assert_eq!(hello_inode.kind, InodeKind::File);
        assert_eq!(&*hello_data, b"hello vfs\n");

        // HARDCODED_INODE alias must resolve to the same inode (ino=2)
        let hardcoded = HARDCODED_INODE.clone();
        assert_eq!(hardcoded.ino, 2);
        println!("[test]   HARDCODED_INODE.ino = {} (matches hello.txt)", hardcoded.ino);
    }

    println!("[test] path resolution (3.4)");
    {
        use crate::fs::{resolve_path, InodeKind};

        let root = resolve_path("/");
        let root_ino = root.as_ref().map(|i| i.ino);
        let root_kind = root.as_ref().map(|i| i.kind);
        println!("[test]   '/' -> ino={:?} kind={:?} (expect ino=1, kind=Dir)", root_ino, root_kind);
        assert_eq!(root_ino, Some(1));
        assert_eq!(root_kind, Some(InodeKind::Dir));

        let hello = resolve_path("/hello.txt");
        let hello_ino = hello.as_ref().map(|i| i.ino);
        let hello_kind = hello.as_ref().map(|i| i.kind);
        println!("[test]   '/hello.txt' -> ino={:?} kind={:?} (expect ino=2, kind=File)", hello_ino, hello_kind);
        assert_eq!(hello_ino, Some(2));
        assert_eq!(hello_kind, Some(InodeKind::File));

        let missing = resolve_path("/missing");
        println!("[test]   '/missing' -> is_none={} (expect true)", missing.is_none());
        assert!(missing.is_none());

        let relative = resolve_path("hello.txt");
        println!("[test]   'hello.txt' (relative) -> is_none={} (expect true)", relative.is_none());
        assert!(relative.is_none());

        let notdir = resolve_path("/hello.txt/foo");
        println!("[test]   '/hello.txt/foo' (ENOTDIR) -> is_none={} (expect true)", notdir.is_none());
        assert!(notdir.is_none());

        let trailing = resolve_path("/hello.txt/");
        let trailing_ino = trailing.as_ref().map(|i| i.ino);
        println!("[test]   '/hello.txt/' (trailing slash) -> ino={:?} (expect Some(2))", trailing_ino);
        assert_eq!(trailing_ino, Some(2));
    }

    println!("[test] multiple files (3.6)");
    {
        use crate::fs::{resolve_path};

        let hello = resolve_path("/hello.txt").expect("hello.txt should resolve");
        let secret = resolve_path("/secret.txt").expect("secret.txt should resolve");

        assert_ne!(hello.ino, secret.ino);
        println!(
            "[test]   hello.txt ino={}, secret.txt ino={} (must differ)",
            hello.ino, secret.ino
        );

        let hello_data = hello.data.lock();
        let secret_data = secret.data.lock();
        assert_eq!(&*hello_data, b"hello vfs\n");
        assert_eq!(&*secret_data, b"top secret\n");
        println!("[test]   hello.txt bytes = {:?}", &**hello_data);
        println!("[test]   secret.txt bytes = {:?}", &**secret_data);
    }

    println!("[test] PCI config space access (5.2)");
    {
        // Read the host bridge at 00:00.0 — should be Intel (vendor 0x8086).
        // Verifies that port I/O to 0xCF8/0xCFC works end-to-end.
        let word0 = pci::config_read(0, 0, 0, 0x00);
        let vendor_id = (word0 & 0xFFFF) as u16;
        let device_id = (word0 >> 16) as u16;
        println!(
            "[test]   00:00.0 vendor={:04x} device={:04x}",
            vendor_id, device_id
        );
        assert_eq!(
            vendor_id, 0x8086,
            "host bridge at 00:00.0 should be Intel (0x8086); \
             got 0x{:04x} — PCI config access is broken",
            vendor_id
        );
        println!("[test]   Intel host bridge found — PCI config access works");
    }

    pci::enumerate();

    println!("[test] find virtio-blk and decode BARs (5.4)");
    match pci::find_virtio_blk() {
        Some((bus, dev, func)) => {
            println!(
                "[pci] virtio-blk found at {:02x}:{:02x}.{}",
                bus, dev, func
            );
            pci::print_bars(bus, dev, func);

            println!("[test] parse virtio capability list (6.1)");
            println!("[virtio] parsing capability list of {:02x}:{:02x}.{}", bus, dev, func);
            let caps = virtio::parse_virtio_capabilities(bus, dev, func);
            println!("[virtio]   found {} virtio caps", caps.len());

            println!("[test] virtio handshake phase 1 — reset + ACK + DRIVER (6.2)");
            if let Some(mut virtio_dev) = virtio::VirtioBlk::probe() {
                virtio_dev.reset_and_acknowledge();

                println!("[test] virtio handshake phase 2 — feature negotiation (6.3)");
                virtio_dev.negotiate_features();

                println!("[test] virtio handshake phase 3 — DRIVER_OK (6.4)");
                virtio_dev.set_driver_ok();

                println!("[test] virtqueue allocation + configuration (7.1 + 7.2 + 9.1)");
                virtio_dev.setup_queue(64);

                println!("[test] descriptor free list smoke test (7.3)");
                {
                    let queue = virtio_dev.queue_mut();
                    println!(
                        "[virtio] free list size: {} (expect 64)",
                        queue.num_free()
                    );
                    let a = queue.alloc_descriptor().expect("alloc");
                    let b = queue.alloc_descriptor().expect("alloc");
                    println!("[virtio]   alloc = {}, {} (LIFO: 63, 62)", a, b);
                    queue.free_descriptor(a);
                    let c = queue.alloc_descriptor().expect("alloc");
                    println!("[virtio]   free + alloc = {} (stack reuse)", c);
                    queue.free_descriptor(b);
                    queue.free_descriptor(c);
                }

                println!("[test] virtio-blk request header struct (8.1)");
                {
                    use core::mem::{size_of, align_of};
                    let sz = size_of::<virtio::VirtioBlkReq>();
                    let al = align_of::<virtio::VirtioBlkReq>();
                    println!("[virtio]   size={} bytes (expect 16), align={}", sz, al);
                    assert_eq!(sz, 16, "VirtioBlkReq must be 16 bytes");
                }

                println!("[test] first disk read via read_sector (Keystone 1, recap from L8)");
                let block0 = virtio_dev.read_sector(0);
                let expected_sig = b"MINIOS_DISK_v0\n";
                let sig_str = core::str::from_utf8(&block0[0..15]).unwrap_or("<non-utf8>");
                let tail_zeros: usize = block0[15..].iter().filter(|&&b| b == 0).count();
                println!("[virtio]   read_sector(0)[0..15] = {:?}", sig_str);
                println!("[virtio]   read_sector(0)[15..512] = {} zero bytes (expect 497)", tail_zeros);
                assert_eq!(&block0[0..15], expected_sig, "signature mismatch");
                assert_eq!(tail_zeros, 497, "trailing bytes not all zero");
                println!("[virtio] *** Keystone 1: bytes flowed from disk ***");

                println!("[test] block layer API — read multiple sectors (9.3)");
                let block1 = virtio_dev.read_sector(1);
                let block2 = virtio_dev.read_sector(2);
                let block1_zeros: usize = block1.iter().filter(|&&b| b == 0).count();
                let block2_zeros: usize = block2.iter().filter(|&&b| b == 0).count();
                println!("[virtio]   read_sector(1): {} zero bytes (expect 512)", block1_zeros);
                println!("[virtio]   read_sector(2): {} zero bytes (expect 512)", block2_zeros);
                assert_eq!(block1_zeros, 512, "sector 1 should be all zero");
                assert_eq!(block2_zeros, 512, "sector 2 should be all zero");
                println!("[virtio] *** block layer ready — ext2 can mount on top ***");
            } else {
                println!("[virtio] failed to probe device");
            }
        }
        None => {
            println!("[pci] virtio-blk not found on bus 0");
        }
    }

    println!("[boot] enabling interrupts");
    x86_64::instructions::interrupts::enable();

    // M3 slice 3.6.c: set up the syscall mechanism (MSRs + entry point).
    // After this, user code can execute the `syscall` instruction to
    // request kernel services.
    syscall::init();

    // M3 slice 3.6.b/c: jump to user mode. main does NOT return from this
    // call — enter_user_mode is `-> !` because iretq transfers control
    // to user code, which runs until it loops forever or page-faults.
    user::enter_user_mode();
}

/// Counter incremented by worker_a every loop iteration. Atomic so it's
/// safe to read from main while worker_a writes (no Mutex needed).
static COUNTER_A: AtomicU64 = AtomicU64::new(0);

/// Counter incremented by worker_b.
static COUNTER_B: AtomicU64 = AtomicU64::new(0);

/// Counter incremented by the idle task once per hlt wake-up. Lets us
/// see how often the idle task actually gets scheduled.
static COUNTER_IDLE: AtomicU64 = AtomicU64::new(0);

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

/// Idle task entry function. The classic kernel idle pattern: halt in
/// a loop, waking only when an interrupt fires. Each wake-up increments
/// the idle counter so we can see how often it gets scheduled.
///
/// On real hardware this drops the CPU into a low-power state when
/// there's nothing useful to do. In QEMU it lets the emulated CPU rest
/// so QEMU itself uses less host CPU (no busy-loop burning a host core).
unsafe extern "C" fn idle_entry() {
    loop {
        COUNTER_IDLE.fetch_add(1, Ordering::Relaxed);
        x86_64::instructions::hlt();
    }
}
