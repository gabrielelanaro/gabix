//! Process exit, wait, and address-space replacement operations.

use super::scheduler::{TaskState, SCHEDULER};
use crate::fs::FdTable;
use crate::println;
use crate::process::{Pid, PROCESS_TABLE};
use alloc::vec::Vec;

pub fn mark_current_zombie(exit_code: i64) {
    let was_enabled = crate::arch::interrupts::are_enabled();
    if was_enabled {
        crate::arch::interrupts::disable();
    }
    let pid = {
        let mut scheduler = SCHEDULER.lock();
        let current = scheduler.current;
        scheduler.tasks[current].state = TaskState::Zombie;
        scheduler.tasks[current].pid
    };

    let (parent_pid, old_fd_table) = {
        let mut table = PROCESS_TABLE.lock();
        let process = table
            .get_mut(pid)
            .expect("mark_current_zombie: current process missing");
        process.exit_status = Some(exit_code);
        let old_fd_table = core::mem::replace(&mut process.fd_table, FdTable::new());
        (process.parent, old_fd_table)
    };
    drop(old_fd_table);

    if let Some(parent_pid) = parent_pid {
        let mut scheduler = SCHEDULER.lock();
        if let Some(parent) = scheduler
            .tasks
            .iter_mut()
            .find(|task| task.pid == parent_pid)
        {
            if parent.state == TaskState::Blocked {
                parent.state = TaskState::Ready;
            }
        }
    }
}

pub enum WaitResult {
    Reaped { child_pid: Pid, exit_status: i64 },
    NoChildren,
    WouldBlock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitTarget {
    Any,
    Specific(Pid),
}

impl WaitTarget {
    fn matches(self, child_pid: Pid) -> bool {
        match self {
            WaitTarget::Any => true,
            WaitTarget::Specific(target_pid) => child_pid == target_pid,
        }
    }
}

pub fn wait_for_zombie_child(parent_pid: Pid, target: WaitTarget) -> WaitResult {
    crate::arch::interrupts::without_interrupts(|| {
        let zombie_pids = {
            let scheduler = SCHEDULER.lock();
            scheduler
                .tasks
                .iter()
                .filter(|task| task.state == TaskState::Zombie)
                .map(|task| task.pid)
                .collect::<Vec<_>>()
        };

        let reaped = {
            let mut table = PROCESS_TABLE.lock();
            zombie_pids
                .into_iter()
                .find(|&pid| {
                    table
                        .get(pid)
                        .map(|process| process.parent == Some(parent_pid) && target.matches(pid))
                        .unwrap_or(false)
                })
                .map(|child_pid| {
                    let child = table
                        .remove(child_pid)
                        .expect("wait_for_zombie_child: child process disappeared");
                    let parent = table
                        .get_mut(parent_pid)
                        .expect("wait_for_zombie_child: parent process missing");
                    parent.children.retain(|&pid| pid != child_pid);
                    (child_pid, child.exit_status.unwrap_or(0))
                })
        };

        if let Some((child_pid, exit_status)) = reaped {
            let mut scheduler = SCHEDULER.lock();
            let child = scheduler
                .tasks
                .iter_mut()
                .find(|task| task.pid == child_pid)
                .expect("wait_for_zombie_child: child task disappeared");
            child.state = TaskState::Dead;
            return WaitResult::Reaped {
                child_pid,
                exit_status,
            };
        }

        let has_matching_child = {
            let table = PROCESS_TABLE.lock();
            table
                .get(parent_pid)
                .expect("wait_for_zombie_child: parent process missing")
                .children
                .iter()
                .any(|&child_pid| target.matches(child_pid))
        };
        if !has_matching_child {
            return WaitResult::NoChildren;
        }

        let mut scheduler = SCHEDULER.lock();
        let parent = scheduler
            .tasks
            .iter_mut()
            .find(|task| task.pid == parent_pid)
            .expect("wait_for_zombie_child: parent task not found");
        parent.state = TaskState::Blocked;
        WaitResult::WouldBlock
    })
}

pub fn replace_current_address_space(new_as: crate::memory::AddressSpace, new_name: &str) {
    let was_enabled = crate::arch::interrupts::are_enabled();
    if was_enabled {
        crate::arch::interrupts::disable();
    }
    let pid = {
        let scheduler = SCHEDULER.lock();
        scheduler.tasks[scheduler.current].pid
    };
    let mut table = PROCESS_TABLE.lock();
    let process = table
        .get_mut(pid)
        .expect("replace_current_address_space: current process missing");
    process.address_space = new_as;
    process.name.set(new_name);
    println!("[process] pid={} exec: now \"{}\"", pid, process.name);
}
