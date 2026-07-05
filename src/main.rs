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
mod process;
mod user;
mod syscall;
mod fs;
mod pci;
mod virtio;
mod ext2;
mod elf;
mod exec;
mod qemu_exit;
mod test_runner;

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
///
/// In test mode (cargo bootimage --features test-mode): the handler also
/// calls `qemu_exit::exit_qemu(EXIT_FAILURE)` after printing. That lets the
/// test wrapper translate the failure into a Unix exit
/// code. Without test-mode, the handler loops forever so you can read the
/// message on screen before killing QEMU manually.
#[cfg(feature = "test-mode")]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    use crate::qemu_exit;
    unsafe {
        vga_buffer::WRITER.force_unlock();
        serial::SERIAL1.force_unlock();
    }
    println!("[panic] {}", info);
    qemu_exit::exit_qemu(qemu_exit::EXIT_FAILURE);
}

#[cfg(not(feature = "test-mode"))]
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
    process::init();
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

    println!("[test] filesystem setup — skipped (old in-memory ROOT_INODE removed in Lesson 14; ext2 is the real filesystem now)");

    println!("[test] path resolution — skipped (3.4/3.6 tests replaced by Lesson 14 ext2 mount)");
    // The old tests used resolve_path against the in-memory ROOT_INODE.
    // Lesson 14 replaced resolve_path to use MOUNTED_FS (ext2 on-demand),
    // so these tests now run as part of the Lesson 14 mount verification
    // (after ext2 is mounted, later in boot).

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
                // Sector 0 is the boot sector. ext2's mkfs leaves it mostly
                // empty (zeros). Earlier lessons wrote "MINIOS_DISK_v0\n"
                // here; now that the disk is ext2-formatted we just verify
                // read_sector returns 512 bytes successfully.
                println!(
                    "[virtio]   read_sector(0) returned {} bytes (boot sector)",
                    block0.len()
                );
                println!("[virtio] *** Keystone 1: bytes flowed from disk ***");

                println!("[test] block layer API — read multiple sectors (9.3)");
                let block1 = virtio_dev.read_sector(1);
                let block2 = virtio_dev.read_sector(2);
                // Sectors 1 and 2 now contain ext2's superblock (1024 bytes
                // starting at sector 2) and adjacent metadata. Earlier
                // lessons checked they were zero — that was before the disk
                // had a filesystem. Now they're populated by mkfs.ext2.
                println!(
                    "[virtio]   read_sector(1) returned {} bytes",
                    block1.len()
                );
                println!(
                    "[virtio]   read_sector(2) returned {} bytes (superblock lives here)",
                    block2.len()
                );
                println!("[virtio] *** block layer ready — ext2 can mount on top ***");

                println!("[test] ext2 superblock parser (10.1-10.4)");
                let sb = ext2::read_superblock(&mut virtio_dev);
                sb.print();

                assert!(sb.is_ext2(), "ext2 magic not found — disk isn't ext2");
                assert_eq!(sb.block_size, 1024, "expected 1024-byte blocks for small disk");
                assert_eq!(sb.first_data_block, 1, "first_data_block should be 1 for 1024 B blocks");
                assert_eq!(sb.state, 1, "filesystem should be clean (state=1)");
                assert!(sb.inodes_count > 0 && sb.inodes_count < 1000,
                    "inodes_count {} looks wrong", sb.inodes_count);
                assert!(sb.blocks_count > 0 && sb.blocks_count < 10000,
                    "blocks_count {} looks wrong", sb.blocks_count);

                println!("[ext2] *** superblock parsed — ext2 filesystem recognized ***");

                println!("[test] ext2 inode lookup (12.2-12.5)");
                let bgd = ext2::read_block_group_descriptor(&mut virtio_dev, &sb, 0);
                bgd.print(0);

                assert!(bgd.inode_table != 0, "BGD inode_table must be non-zero");
                assert!(bgd.free_inodes_count > 0, "should have free inodes");

                let root_ino = ext2::EXT2_ROOT_INO;
                let root_inode = ext2::read_inode(&mut virtio_dev, &sb, &bgd, root_ino);
                root_inode.print(root_ino);

                assert!(root_inode.is_dir(),
                    "inode 2 should be a directory, got mode 0x{:04X} ({})",
                    root_inode.mode, root_inode.type_name());
                assert!(root_inode.size > 0, "root directory should be non-empty");
                assert!(root_inode.block_pointers[0] != 0,
                    "root directory needs at least one data block");

                println!("[ext2] *** inode {} is the root directory — ready to walk (Lesson 13) ***",
                    root_ino);

                println!("[test] ext2 directory walk (13.1-13.5)");
                let entries = ext2::walk_directory(&mut virtio_dev, &root_inode);
                println!("[ext2] walk complete: {} live entries", entries.len());

                // Verify we got exactly the entries mkfs + Lesson 14 + fixture builds created
                assert_eq!(entries.len(), 8, "expected 8 entries (., .., lost+found, hello.txt, secret.txt, hello.elf, fork_smoke.elf, inherit_wait.elf)");

                // Find by name to verify each
                let dot = entries.iter().find(|e| e.name == b".").expect("missing .");
                let dotdot = entries.iter().find(|e| e.name == b"..").expect("missing ..");
                let lf = entries.iter().find(|e| e.name == b"lost+found").expect("missing lost+found");

                assert_eq!(dot.inode, ext2::EXT2_ROOT_INO,
                    ". should point at root inode (2)");
                assert_eq!(dotdot.inode, ext2::EXT2_ROOT_INO,
                    ".. should point at root inode (2) — root's parent is itself");
                assert_eq!(lf.inode, 11,
                    "lost+found should be inode 11 (first user inode after reserved 1-10)");
                assert_eq!(dot.file_type, ext2::EXT_FT_DIR, ". should be a directory");
                assert_eq!(lf.file_type, ext2::EXT_FT_DIR, "lost+found should be a directory");

                // Test lookup_in_dir via the API
                println!("[ext2] testing lookup_in_dir");
                let found = ext2::lookup_in_dir(&mut virtio_dev, &root_inode, b"lost+found");
                println!("[ext2]   lookup_in_dir(\"lost+found\") = {:?}", found);
                assert_eq!(found, Some(11));

                let missing = ext2::lookup_in_dir(&mut virtio_dev, &root_inode, b"does-not-exist");
                println!("[ext2]   lookup_in_dir(\"does-not-exist\") = {:?}", missing);
                assert_eq!(missing, None);

                println!("[ext2] *** directory walk works — ready to mount (Lesson 14) ***");

                println!("[test] mount ext2 via Filesystem trait (14.2-14.4)");
                // Move virtio_dev into Ext2 — after this, all I/O goes
                // through the trait, transparently to fs.rs.
                let ext2_fs = ext2::Ext2::new(virtio_dev);
                fs::mount(alloc::boxed::Box::new(ext2_fs));

                // Verify: resolve /hello.txt and read its contents
                let hello = fs::resolve_path("/hello.txt")
                    .expect("resolve_path(/hello.txt) should succeed");
                assert_eq!(hello.kind, fs::InodeKind::File,
                    "hello.txt should be a regular file");
                let data = hello.data.lock();
                let data_str = core::str::from_utf8(&data).unwrap_or("<non-utf8>");
                println!("[ext2] resolve_path(\"/hello.txt\") → ino={}, {} bytes: {:?}",
                    hello.ino, data.len(), data_str);
                assert!(data.starts_with(b"hello from disk"),
                    "hello.txt contents mismatch: {:?}", data_str);
                drop(data);

                // Root directory
                let root = fs::resolve_path("/")
                    .expect("resolve_path(/) should succeed");
                assert_eq!(root.kind, fs::InodeKind::Dir);

                // Missing path
                let missing = fs::resolve_path("/does-not-exist");
                assert!(missing.is_none(), "missing path should return None");
                println!("[ext2] resolve_path(\"/does-not-exist\") → None ✓");

                // Cache hit: second resolve of /hello.txt returns same Arc
                let hello2 = fs::resolve_path("/hello.txt")
                    .expect("second resolve should also succeed");
                assert!(alloc::sync::Arc::ptr_eq(&hello, &hello2),
                    "second resolve should hit cache (same Arc)");
                println!("[ext2] second resolve_path(\"/hello.txt\") → cache hit ✓");

                println!("[ext2] *** ext2 mounted via trait — VFS is filesystem-agnostic ***");

                println!("[test] ELF header parser (15.1-15.4)");
                {
                    let elf_inode = fs::resolve_path("/hello.elf")
                        .expect("resolve_path(\"/hello.elf\") should succeed");
                    assert_eq!(elf_inode.kind, fs::InodeKind::File,
                        "hello.elf should be a regular file");
                    let bytes = elf_inode.data.lock();
                    println!("[elf] /hello.elf: {} bytes", bytes.len());

                    let header = elf::ElfHeader::parse(&bytes)
                        .expect("ELF header parse failed — magic mismatch or short read");
                    header.print();

                    assert!(header.is_x86_64_exec(),
                        "not a 64-bit little-endian ET_EXEC x86-64 ELF \
                         (class={}, data={}, e_type={}, machine=0x{:X})",
                        header.class, header.data, header.e_type, header.machine);
                    assert_eq!(header.phentsize, 56,
                        "ELF64 program headers must be 56 bytes, got {}", header.phentsize);
                    assert!(header.phnum > 0, "expected at least one program header");

                    // Walk the PHT. Each entry is at e_phoff + i * e_phentsize.
                    println!("[elf] program headers ({} entries):", header.phnum);
                    let mut loadable = 0u16;
                    for i in 0..header.phnum {
                        let off = header.phoff as usize
                            + (i as usize) * (header.phentsize as usize);
                        let ph = elf::ProgramHeader::parse(&bytes[off..])
                            .expect("program header parse failed — short read");

                        let kind = ph.type_str();
                        let marker = if ph.is_load() { "★" } else { " " };
                        println!(
                            "[elf]   {}[{}] {:<5} off=0x{:>06X} vaddr=0x{:>09X} filesz={:>5} memsz={:>5} {}",
                            marker, i, kind, ph.p_offset, ph.p_vaddr,
                            ph.p_filesz, ph.p_memsz, ph.perms_str()
                        );
                        if ph.is_load() {
                            loadable += 1;
                        }
                    }

                    println!("[elf] {} loadable segment(s) — Lesson 16 will map these", loadable);
                    assert!(loadable >= 1, "expected at least one PT_LOAD segment");

                    // Sanity: entry point should fall inside a loadable + executable segment.
                    let entry_in_r_x = (0..header.phnum)
                        .map(|i| {
                            let off = header.phoff as usize
                                + (i as usize) * (header.phentsize as usize);
                            elf::ProgramHeader::parse(&bytes[off..]).unwrap()
                        })
                        .any(|ph| ph.is_load()
                            && ph.p_flags & elf::PF_X != 0
                            && header.entry >= ph.p_vaddr
                            && header.entry < ph.p_vaddr + ph.p_memsz);
                    assert!(entry_in_r_x,
                        "e_entry (0x{:X}) not inside any r-x PT_LOAD segment", header.entry);

                    println!("[elf] entry 0x{:X} ✓ inside an r-x PT_LOAD segment", header.entry);
                    println!("[elf] *** ELF header parsed — ready for exec() (Lesson 16) ***");
                }
            } else {
                println!("[virtio] failed to probe device");
            }
        }
        None => {
            println!("[pci] virtio-blk not found on bus 0");
        }
    }

    println!("[test] find virtio-net and decode BARs (19.1)");
    match pci::find_virtio_device(pci::VIRTIO_NET_NON_TRANSITIONAL) {
        Some(dev) => {
            println!(
                "[pci] virtio-net found at {:02x}:{:02x}.{} device={:04x}",
                dev.bus,
                dev.device,
                dev.func,
                pci::VIRTIO_NET_NON_TRANSITIONAL
            );
            pci::print_bars(dev.bus, dev.device, dev.func);
        }
        None => {
            println!("[pci] virtio-net not found on bus 0");
        }
    }

    println!("[test] virtio-net initialization (20.1-20.4)");
    if let Some(mut net_dev) = virtio::VirtioNet::probe() {
        println!("[virtio-net] handshake phase 1 — reset + ACK + DRIVER");
        net_dev.reset_and_acknowledge();

        println!("[virtio-net] handshake phase 2 — feature negotiation");
        net_dev.negotiate_features();

        println!("[virtio-net] read MAC address from device config");
        net_dev.read_mac();

        println!("[virtio-net] configure RX queue 0 and TX queue 1");
        net_dev.setup_queues(64);

        println!("[virtio-net] post initial RX buffer supply");
        net_dev.post_initial_rx_buffers();

        println!("[virtio-net] handshake phase 3 — DRIVER_OK");
        net_dev.set_driver_ok();
    } else {
        println!("[virtio-net] device not present; skipping init");
    }

    println!("[boot] enabling interrupts");
    x86_64::instructions::interrupts::enable();

    // M3 slice 3.6.c: set up the syscall mechanism (MSRs + entry point).
    // After this, user code can execute the `syscall` instruction to
    // request kernel services.
    syscall::init();

    // ========================================================
    // Lesson 16: exec /hello.elf, with feature-selected fixture programs for tests
    // ========================================================
    // This is the mission landing. The kernel:
    //   1. Resolves the user ELF via VFS (Lessons 1-14)
    //   2. Parses its header (Lesson 15)
    //   3. Maps PT_LOAD segments into a fresh AddressSpace (Lesson 16)
    //   4. Maps a user stack
    //   5. Switches CR3 and iretqs to e_entry
    // The program runs in ring 3 from a binary that came from disk.
    //
    // Under test fixtures, exec the selected user program from disk.
    #[cfg(feature = "test-inherit")]
    let user_elf_path: &str = "/inherit_wait.elf";
    #[cfg(all(not(feature = "test-inherit"), feature = "test-fork"))]
    let user_elf_path: &str = "/fork_smoke.elf";
    #[cfg(all(not(feature = "test-inherit"), not(feature = "test-fork")))]
    let user_elf_path: &str = "/hello.elf";

    println!("[test] exec {} (16.1-16.4) — the mission", user_elf_path);
    let ctx = match exec::exec_elf(user_elf_path) {
        Ok(c) => c,
        Err(e) => {
            panic!("[exec] failed: {}", e);
        }
    };

    // Switch CR3 to the new AS, then iretq to user mode.
    //
    // Update the process table before writing CR3. `ctx.aspace.switch_to()`
    // writes CR3 directly and leaves the current Process record pointing
    // at the boot PML4 (frame 1). Later, when `yield_now` switches from a
    // child back to main, it reads main's stale pml4_frame and switches CR3
    // back to a PML4 with no user code/stack mappings. Parent user code
    // then crashes on the next instruction fetch.
    //
    // `replace_current_address_space` updates the scheduler's record;
    // then we write CR3 via the scheduler's now-correct view.
    let entry = ctx.entry;
    let stack_top = ctx.stack_top;
    let user_name = program_name_from_path(user_elf_path);
    crate::task::replace_current_address_space(ctx.aspace, user_name);
    let pml4_frame = crate::task::current_pml4_frame();
    // SAFETY: pml4_frame comes from the current task's AddressSpace,
    // which was just initialized via exec_elf.
    unsafe {
        x86_64::registers::control::Cr3::write(
            pml4_frame,
            x86_64::registers::control::Cr3Flags::empty(),
        );
    }
    println!("[exec] CR3 := new AS; jumping to user mode at rip=0x{:X}", entry);
    println!("[exec] *** mission running: disk → ext2 → VFS → ELF → ring 3 ***");

    // Disable interrupts during the iretq setup. The RFLAGS value
    // pushed in the iretq frame re-enables them in user mode.
    x86_64::instructions::interrupts::disable();

    // Same iretq frame layout as user.rs: SS, RSP, RFLAGS, CS, RIP.
    // Seg selectors 0x23/0x1B match the slice 4.5 GDT layout
    // (USER_CODE64 = index 4 = 0x20 | RPL 3 = 0x23,
    //  USER_DATA  = index 3 = 0x18 | RPL 3 = 0x1B).
    unsafe {
        core::arch::asm!(
            "push 0x1B",            // SS (user data | RPL=3)
            "push {rsp}",           // RSP (top of user stack)
            "push 0x202",           // RFLAGS (reserved bit 1 + IF)
            "push 0x23",            // CS (user code | RPL=3)
            "push {rip}",           // RIP (= e_entry)
            "iretq",
            rsp = in(reg) stack_top,
            rip = in(reg) entry,
            options(nostack, nomem, preserves_flags),
        );
    }
    unreachable!("iretq transfers control to user mode");
}

fn program_name_from_path(path: &'static str) -> &'static str {
    match path.rsplit_once('/') {
        Some((_, name)) if !name.is_empty() => name,
        _ => path,
    }
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
