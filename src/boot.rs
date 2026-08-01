//! x86 boot orchestration.

use crate::println;
use alloc::boxed::Box;
use bootloader::BootInfo;

const INIT_PATH: &str = "/init.elf";

pub fn run(boot_info: &'static BootInfo) -> ! {
    enable_sse();
    crate::gdt::init();
    crate::interrupts::init();
    crate::pic::init();
    crate::pit::set_frequency(100);

    crate::frame_allocator::init(&boot_info.memory_map);
    crate::memory::init(boot_info);
    crate::apic::init();
    crate::process::init();
    crate::task::init_main();
    x86_64::instructions::interrupts::enable();

    mount_root_filesystem();
    start_network();
    crate::syscall::init();
    enter_init();
}

fn enable_sse() {
    unsafe {
        core::arch::asm!(
            "mov rax, cr4",
            "or rax, 0x600",
            "mov cr4, rax",
            out("rax") _,
            options(nomem, nostack, preserves_flags),
        );
    }
}

fn mount_root_filesystem() {
    let mut device = crate::virtio::VirtioBlk::probe().expect("virtio-blk device is required");
    device.reset_and_acknowledge();
    device.negotiate_features();
    device.set_driver_ok();
    device.setup_queue(64);
    crate::fs::mount(Box::new(crate::ext2::Ext2::new(device)));
}

fn start_network() {
    let Some(mut device) = crate::virtio::VirtioNet::probe() else {
        println!("[net] virtio-net device is unavailable");
        return;
    };

    device.reset_and_acknowledge();
    device.negotiate_features();
    device.read_mac();
    device.setup_queues(64);
    device.post_initial_rx_buffers();
    device.set_driver_ok();

    crate::net::configure(crate::net::NetworkConfig {
        address: [10, 0, 2, 15],
        subnet_mask: [255, 255, 255, 0],
        gateway: [10, 0, 2, 2],
        dns: [10, 0, 2, 3],
        lease_seconds: 86_400,
    });

    crate::virtio::install_net_device(crate::virtio::NetDevice::from_virtio_net(device));
    let pid = crate::task::spawn("net-rx", crate::virtio::network_task);
    crate::task::block(pid);
    crate::virtio::register_net_task(pid);
}

fn enter_init() -> ! {
    let context = crate::exec::exec_elf(INIT_PATH)
        .unwrap_or_else(|error| panic!("failed to load {}: {}", INIT_PATH, error));
    let entry = context.entry;
    let stack_top = context.stack_top;

    crate::task::replace_current_address_space(context.aspace, "init.elf");
    let pml4_frame = crate::task::current_pml4_frame();
    unsafe {
        x86_64::registers::control::Cr3::write(
            pml4_frame,
            x86_64::registers::control::Cr3Flags::empty(),
        );
        x86_64::instructions::interrupts::disable();
        core::arch::asm!(
            "push 0x1B",
            "push {rsp}",
            "push 0x202",
            "push 0x23",
            "push {rip}",
            "iretq",
            rsp = in(reg) stack_top,
            rip = in(reg) entry,
            options(nostack, nomem, preserves_flags),
        );
    }
    unreachable!("iretq transfers control to userspace");
}
