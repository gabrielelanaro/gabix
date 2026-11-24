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
// Single-core state: callers serialize updates with the scheduler/exception path.
static mut OUTCOMES: [Outcome; memory::PROCESS_COUNT] = [Outcome::Running; memory::PROCESS_COUNT];
static mut PREEMPTIONS: [u32; memory::PROCESS_COUNT] = [0; memory::PROCESS_COUNT];
const CPSR_MODE_MASK: u32 = 0x1f;
const CPSR_USER_MODE: u32 = 0x10;
// Short-descriptor fault status is split between bits [3:0] and bit 10.
const FAULT_STATUS_LOW_MASK: u32 = 0xf;
const FAULT_STATUS_HIGH_BIT: u32 = 1 << 10;
const SECTION_PERMISSION_FAULT: u32 = 0xd;
const DATA_FAULT_WAS_WRITE: u32 = 1 << 11;

// Probe ABI shared with process_probe.S: sentinel, forbidden address, access kind.
const PROBE_DATA_OFFSET: u32 = 0x10000;
const PROBE_DATA: u32 = memory::PROCESS_BASE + PROBE_DATA_OFFSET;
const PROBE_ADDRESS_OFFSET: u32 = 4;
const PROBE_ACCESS_OFFSET: u32 = 8;
const PROBE_READ: u32 = 0;
const PROBE_WRITE: u32 = 1;
const PROBE_EXECUTE: u32 = 2;
const PROBE_STACK_SEPARATION: u32 = 0x100;
const PROBE_EXECUTE_STACK_OFFSET: u32 = 0x200;
const MINIMUM_PREEMPTIONS: u32 = 2;
#[cfg(feature = "pi-hardware")]
const PROBE_WATCHDOG_SECONDS: u32 = 15;

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
            memory::PROCESS_STACK_TOP - id as u32 * PROBE_STACK_SEPARATION,
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
    if saved_status & CPSR_MODE_MASK == CPSR_USER_MODE {
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
/// Caller must mask IRQs and stop both processes: this changes the current table,
/// clears the whole allocation, then returns to the boot table.
unsafe fn prepare(id: usize, image: &[u8], forbidden_address: u32, access_kind: u32) {
    assert!(image.len() < PROBE_DATA_OFFSET as usize);
    memory::prepare_process_table(id);
    memory::activate_process(Some(id));
    core::ptr::write_bytes(
        memory::PROCESS_BASE as *mut u8,
        0,
        (memory::PROCESS_END - memory::PROCESS_BASE) as usize,
    );
    core::ptr::copy_nonoverlapping(image.as_ptr(), memory::PROCESS_BASE as *mut u8, image.len());
    core::ptr::write_volatile(
        (PROBE_DATA + PROBE_ADDRESS_OFFSET) as *mut u32,
        forbidden_address,
    );
    core::ptr::write_volatile((PROBE_DATA + PROBE_ACCESS_OFFSET) as *mut u32, access_kind);
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
    // Baseline, other-process read/write, kernel read, and stack execution.
    for (forbidden, access_kind) in [
        (0, PROBE_READ),
        (memory::process_physical_base(1), PROBE_READ),
        (memory::process_physical_base(1), PROBE_WRITE),
        (kernel_address, PROBE_READ),
        (
            memory::PROCESS_STACK_TOP - PROBE_EXECUTE_STACK_OFFSET,
            PROBE_EXECUTE,
        ),
    ] {
        #[cfg(feature = "pi-hardware")]
        crate::raspi2b::reset::arm_deadman(PROBE_WATCHDOG_SECONDS);
        asm!("cpsid i");
        scheduler::reset_finished_tasks();
        prepare(0, image, forbidden, access_kind);
        prepare(1, image, 0, PROBE_READ);
        if scheduler::spawn_process(0).is_none() || scheduler::spawn_process(1).is_none() {
            return false;
        }
        asm!("cpsie i", "isb");
        scheduler::run_until_finished();
        let outcomes = OUTCOMES;
        let preemptions = PREEMPTIONS;
        let mut console = crate::console::Pl011::new(crate::platform::UART_BASE);
        crate::write_line(&mut console, alloc::format!("gabix isolation address={forbidden:#x} access={access_kind} outcomes={outcomes:?} ticks={preemptions:?}").as_bytes());
        if OUTCOMES[1] != Outcome::Exited(0)
            || preemptions.iter().any(|&ticks| ticks < MINIMUM_PREEMPTIONS)
        {
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
                    && (status & FAULT_STATUS_LOW_MASK) == SECTION_PERMISSION_FAULT
                    && status & FAULT_STATUS_HIGH_BIT == 0
                    && instruction == (access_kind == PROBE_EXECUTE)
                    && (instruction
                        || (status & DATA_FAULT_WAS_WRITE != 0)
                            == (access_kind == PROBE_WRITE)) => {}
                _ => return false,
            }
        }
    }
    true
}
