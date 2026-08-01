//! Process lifecycle syscall handlers.

use super::user::read_user_path;
use crate::arch::x86_64::syscall as syscall_arch;
use crate::println;

pub(super) fn getpid() -> u64 {
    let pid = crate::task::current_pid();
    println!("[syscall] sys_getpid -> {}", pid);
    pid.into()
}

pub(super) fn exit(exit_code: u64) -> u64 {
    println!("[syscall] sys_exit({})", exit_code);
    crate::task::mark_current_zombie(exit_code as i64);
    #[cfg(feature = "qemu-exit")]
    if crate::task::current_pid() == crate::process::Pid(0) {
        crate::qemu_exit::exit_qemu(if exit_code == 0 {
            crate::qemu_exit::EXIT_SUCCESS
        } else {
            crate::qemu_exit::EXIT_FAILURE
        });
    }
    crate::task::yield_now();
    panic!("sys_exit: zombie task was rescheduled (this should never happen)");
}

pub(super) fn wait(target_pid_raw: u64, exit_status_ptr: u64) -> u64 {
    use crate::process::Pid;
    use crate::task::{WaitResult, WaitTarget};

    let parent_pid = crate::task::current_pid();
    let target = if target_pid_raw == u64::MAX {
        WaitTarget::Any
    } else if target_pid_raw > 0 {
        WaitTarget::Specific(Pid::new(target_pid_raw))
    } else {
        println!(
            "[syscall] sys_wait4 unsupported target pid={} (parent pid={})",
            target_pid_raw, parent_pid
        );
        return u64::MAX;
    };

    println!(
        "[syscall] sys_wait4 (parent pid={}, target={:?})",
        parent_pid, target
    );

    loop {
        match crate::task::wait_for_zombie_child(parent_pid, target) {
            WaitResult::Reaped {
                child_pid,
                exit_status,
            } => {
                println!(
                    "[syscall] sys_wait4 -> child pid={} (exit={})",
                    child_pid, exit_status
                );
                if exit_status_ptr != 0 {
                    unsafe {
                        core::ptr::write_volatile(exit_status_ptr as *mut i64, exit_status);
                    }
                }
                return child_pid.into();
            }
            WaitResult::NoChildren => return u64::MAX,
            WaitResult::WouldBlock => crate::task::yield_now(),
        }
    }
}

pub(super) fn fork() -> u64 {
    use crate::task::{spawn_fork_child, SavedCalleeSaves};

    println!("[syscall] sys_fork — cloning current process");
    let was_enabled = crate::arch::interrupts::are_enabled();
    if was_enabled {
        crate::arch::interrupts::disable();
    }

    let saved_callee_saves = SavedCalleeSaves {
        rbx: syscall_arch::USER_RBX_SAVE.load(core::sync::atomic::Ordering::SeqCst),
        rbp: syscall_arch::USER_RBP_SAVE.load(core::sync::atomic::Ordering::SeqCst),
        r12: syscall_arch::USER_R12_SAVE.load(core::sync::atomic::Ordering::SeqCst),
        r13: syscall_arch::USER_R13_SAVE.load(core::sync::atomic::Ordering::SeqCst),
        r14: syscall_arch::USER_R14_SAVE.load(core::sync::atomic::Ordering::SeqCst),
        r15: syscall_arch::USER_R15_SAVE.load(core::sync::atomic::Ordering::SeqCst),
        user_rip: syscall_arch::USER_RIP_SAVE.load(core::sync::atomic::Ordering::SeqCst),
        user_rsp: syscall_arch::USER_RSP_SAVE.load(core::sync::atomic::Ordering::SeqCst),
        user_rflags: syscall_arch::USER_RFLAGS_SAVE.load(core::sync::atomic::Ordering::SeqCst),
    };

    let parent_pid = crate::task::current_pid();
    let child_pid = spawn_fork_child(parent_pid, saved_callee_saves);
    println!(
        "[syscall] sys_fork -> child pid={} (parent pid={})",
        child_pid, parent_pid
    );

    if was_enabled {
        crate::arch::interrupts::enable();
    }
    child_pid.into()
}

pub(super) fn exec_elf(path_ptr: u64) -> u64 {
    let path = match read_user_path(path_ptr) {
        Some(path) => path,
        None => return u64::MAX,
    };
    let context = match crate::exec::exec_elf_replace(&path) {
        Ok(context) => context,
        Err(error) => {
            println!("[exec] failed to load {}: {}", path, error);
            return u64::MAX;
        }
    };

    let entry = context.entry;
    let stack_top = context.stack_top;
    let name = path.rsplit('/').next().unwrap_or(path.as_str());
    crate::task::replace_current_address_space(context.aspace, name);
    let pml4_frame = crate::task::current_pml4_frame();
    unsafe {
        x86_64::registers::control::Cr3::write(
            pml4_frame,
            x86_64::registers::control::Cr3Flags::empty(),
        );
        core::arch::asm!(
            "mov rsp, {rsp}",
            "xor rax, rax",
            "cli",
            "sysretq",
            in("rcx") entry,
            in("r11") 0x202u64,
            rsp = in(reg) stack_top,
            options(noreturn),
        );
    }
}
