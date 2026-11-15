//! x86 boot orchestration.

use crate::println;
use alloc::boxed::Box;
use bootloader::BootInfo;

const TIMER_TICKS_PER_SECOND: u32 = 100;
const DEVICE_QUEUE_ENTRIES: u16 = 64;
const CR4_OS_FX_SAVE_RESTORE: u64 = 1 << 9;
const CR4_OS_SIMD_EXCEPTIONS: u64 = 1 << 10;
const RFLAGS_FIXED_BIT: u64 = 1 << 1;
const RFLAGS_INTERRUPTS_ENABLED: u64 = 1 << 9;
const INITIAL_USER_FLAGS: u64 = RFLAGS_FIXED_BIT | RFLAGS_INTERRUPTS_ENABLED;

const INIT_PATH: &str = "/init.elf";

pub fn run(boot_info: &'static BootInfo) -> ! {
    gabix_kernel::announce_boot(
        &mut *crate::serial::SERIAL1.lock(),
        gabix_kernel::Platform::X86Qemu,
    );
    enable_sse();
    super::gdt::init();
    super::interrupts::init();
    super::pic::init();
    super::pit::set_frequency(TIMER_TICKS_PER_SECOND);

    crate::frame_allocator::init(&boot_info.memory_map);
    crate::memory::init(boot_info);
    super::apic::init();
    crate::process::init();
    crate::task::init_main();
    x86_64::instructions::interrupts::enable();

    mount_root_filesystem();
    start_network();
    super::syscall::init();
    enter_init();
}

fn enable_sse() {
    unsafe {
        core::arch::asm!(
            "mov rax, cr4",
            "or rax, {sse_support}",
            "mov cr4, rax",
            sse_support = const CR4_OS_FX_SAVE_RESTORE | CR4_OS_SIMD_EXCEPTIONS,
            out("rax") _,
            options(nomem, nostack),
        );
    }
}

fn mount_root_filesystem() {
    let mut device =
        crate::drivers::virtio::VirtioBlk::probe().expect("virtio-blk device is required");
    device.reset_and_acknowledge();
    device.negotiate_features();
    device.set_driver_ok();
    device.setup_queue(DEVICE_QUEUE_ENTRIES);
    crate::fs::mount(Box::new(crate::fs::ext2::Ext2::new(Box::new(device))));
}

fn start_network() {
    let Some(mut device) = crate::drivers::virtio::VirtioNet::probe() else {
        println!("[net] virtio-net device is unavailable");
        return;
    };

    device.reset_and_acknowledge();
    device.negotiate_features();
    device.read_mac();
    device.setup_queues(DEVICE_QUEUE_ENTRIES);
    device.post_initial_rx_buffers();
    device.set_driver_ok();

    // Fixed initial configuration for QEMU user networking.
    crate::net::configure(crate::net::NetworkConfig {
        address: [10, 0, 2, 15],
        subnet_mask: [255, 255, 255, 0],
        gateway: [10, 0, 2, 2],
        dns: [10, 0, 2, 3],
        lease_seconds: 86_400,
    });

    crate::net::device::install_net_device(crate::net::device::NetDevice::new(Box::new(device)));
    let pid = crate::task::spawn("net-rx", crate::net::device::network_task);
    crate::task::block(pid);
    crate::net::device::register_net_task(pid);
}

fn enter_init() -> ! {
    let context = crate::exec::exec_elf(INIT_PATH)
        .unwrap_or_else(|error| panic!("failed to load {}: {}", INIT_PATH, error));
    let entry = context.entry;
    let stack_top = context.stack_top;
    let user_code = super::gdt::GDT.1.user_code.0 as u64;
    let user_data = super::gdt::GDT.1.user_data.0 as u64;

    crate::task::replace_current_address_space(context.aspace, "init.elf");
    let pml4_frame = crate::task::current_pml4_frame();
    unsafe {
        x86_64::registers::control::Cr3::write(
            pml4_frame,
            x86_64::registers::control::Cr3Flags::empty(),
        );
        x86_64::instructions::interrupts::disable();
        // IRETQ consumes RIP, CS, RFLAGS, RSP and SS from the stack.
        // Push in reverse order, using the selectors returned by GDT setup.
        core::arch::asm!(
            "push {user_data}",
            "push {rsp}",
            "push {user_flags}",
            "push {user_code}",
            "push {rip}",
            "iretq",
            user_data = in(reg) user_data,
            user_code = in(reg) user_code,
            user_flags = const INITIAL_USER_FLAGS,
            rsp = in(reg) stack_top,
            rip = in(reg) entry,
            options(noreturn),
        );
    }
}
