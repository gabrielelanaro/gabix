//! Bounded first userspace-process implementation: two private address spaces,
//! scheduler-owned kernel stacks, user entry, exit status, and fault termination.
//! Fork/exec/wait and dynamic allocation are separate lifecycle work.
use crate::{memory, scheduler};
use core::arch::{asm, global_asm};

global_asm!(include_str!("process_probe.S"), options(raw));
extern "C" {
    static process_probe_start: u8;
    static process_probe_end: u8;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    Running,
    Exited(u32),
    Fault {
        address: u32,
        status: u32,
        instruction: bool,
    },
}
static mut OUTCOMES: [Outcome; 2] = [Outcome::Running; 2];
static mut PREEMPTIONS: [u32; 2] = [0; 2];
const DATA: u32 = memory::PROCESS_BASE + 0x10000;

pub unsafe fn record_preemption(id: usize) {
    PREEMPTIONS[id] += 1;
}

pub fn enter_current_process() {
    unsafe {
        let id = scheduler::current_process().expect("process task has identity");
        // Different initial stack positions make missing SP_usr restoration
        // observable even though both processes use the same stack section VA.
        crate::user::enter(
            memory::PROCESS_BASE,
            memory::PROCESS_STACK_TOP - id as u32 * 0x100,
        );
    }
}

pub unsafe fn exit_current(status: u32) -> ! {
    let id = scheduler::current_process().expect("process exit has identity");
    OUTCOMES[id] = Outcome::Exited(status);
    scheduler::finish_current_process();
}

/// User faults end only that process. Kernel faults keep the diagnostic halt.
pub unsafe fn handle_memory_abort(
    address: u32,
    status: u32,
    saved_status: u32,
    instruction: bool,
) -> ! {
    if saved_status & 0x1f == 0x10 {
        if let Some(id) = scheduler::current_process() {
            OUTCOMES[id] = Outcome::Fault {
                address,
                status,
                instruction,
            };
            scheduler::finish_current_process();
        }
    }
    let mut console = crate::console::Pl011::new(crate::platform::UART_BASE);
    crate::write_line(&mut console, b"gabix arm unexpected kernel memory abort");
    loop {
        asm!("wfi");
    }
}

/// Load a small position-independent flat image into a stopped process.
/// Tables and backing regions are fixed for this first implementation.
/// Loading through the noncacheable user mapping avoids a cached physical alias.
unsafe fn prepare(id: usize, image: &[u8], forbidden_address: u32, access_kind: u32) {
    assert!(image.len() < 0x10000);
    memory::prepare_process_table(id);
    memory::activate_process(Some(id));
    core::ptr::write_bytes(
        memory::PROCESS_BASE as *mut u8,
        0,
        (memory::PROCESS_END - memory::PROCESS_BASE) as usize,
    );
    core::ptr::copy_nonoverlapping(image.as_ptr(), memory::PROCESS_BASE as *mut u8, image.len());
    core::ptr::write_volatile((DATA + 4) as *mut u32, forbidden_address);
    core::ptr::write_volatile((DATA + 8) as *mut u32, access_kind);
    memory::synchronize_process_code();
    memory::activate_process(None);
    OUTCOMES[id] = Outcome::Running;
    PREEMPTIONS[id] = 0;
}

/// Tests real user-mode execution. Both processes must retain their private
/// sentinel at the same VA, survive multiple timer switches, and exit/fault.
pub unsafe fn verify_isolation() -> bool {
    let start = core::ptr::addr_of!(process_probe_start);
    let length = core::ptr::addr_of!(process_probe_end) as usize - start as usize;
    let image = core::slice::from_raw_parts(start, length);
    #[cfg(feature = "virt")]
    let kernel_address = 0x4001_0000;
    #[cfg(feature = "raspi2b")]
    let kernel_address = 0x0010_0000;
    // Baseline, other-process read, other-process write, kernel read.
    for (forbidden, access_kind) in [
        (0, 0),
        (memory::process_physical_base(1), 0),
        (memory::process_physical_base(1), 1),
        (kernel_address, 0),
        (memory::PROCESS_STACK_TOP - 0x200, 2),
    ] {
        #[cfg(feature = "pi-hardware")]
        crate::raspi2b::reset::arm_deadman(15);
        asm!("cpsid i");
        scheduler::reset_finished_tasks();
        prepare(0, image, forbidden, access_kind);
        prepare(1, image, 0, 0);
        if scheduler::spawn_process(0).is_none() || scheduler::spawn_process(1).is_none() {
            return false;
        }
        asm!("cpsie i", "isb");
        scheduler::run_until_finished();
        let outcomes = OUTCOMES;
        let preemptions = PREEMPTIONS;
        let mut console = crate::console::Pl011::new(crate::platform::UART_BASE);
        crate::write_line(&mut console, alloc::format!("gabix isolation address={forbidden:#x} access={access_kind} outcomes={outcomes:?} ticks={preemptions:?}").as_bytes());
        if OUTCOMES[1] != Outcome::Exited(0) || preemptions.iter().any(|&ticks| ticks < 2) {
            return false;
        }
        if forbidden == 0 {
            if OUTCOMES[0] != Outcome::Exited(0) {
                return false;
            }
        } else {
            match OUTCOMES[0] {
                Outcome::Fault {
                    address,
                    status,
                    instruction,
                } if address == forbidden
                    && (status & 0xf) == 0xd
                    && status & (1 << 10) == 0
                    && instruction == (access_kind == 2)
                    && (instruction || (status & (1 << 11) != 0) == (access_kind == 1)) => {}
                _ => return false,
            }
        }
    }
    true
}
