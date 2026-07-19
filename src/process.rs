//! The process table — the single source of truth for "which processes
//! exist, and whose child is this one?"
//!
//! Lesson 17 pulls process identity out of the scheduler. Up to this
//! lesson the scheduler's `Vec<Task>` doubled as the process table, and
//! a task's identifier was `tasks.len() as u64`. That works while the
//! vector only grows, but the moment a task is removed the next fork
//! recycles its index — two different processes seen as the same PID.
//!
//! This module owns the stable-identifier side of that story. The
//! scheduler keeps holding `Task`s (the runnable register-frame side);
//! `Process` holds the identity / process-tree / per-process-memory side.
//! A `Task` carries a `pid: Pid` back-reference so the scheduler can ask
//!
//! PID allocation never recycles. `next_pid` only goes up; a reaped slot
//! becomes `None` permanently. The simplest shape that satisfies the
//! "once a process has been named N, no other process is ever N" rule.

use crate::fs::FdTable;
use crate::irq_mutex::IrqMutex;
use crate::memory::AddressSpace;
use crate::println;
use alloc::vec::Vec;
use lazy_static::lazy_static;

/// Maximum bytes kept for a process name. This is a fixed kernel-side
/// display name, not a full path.
pub const PROCESS_NAME_MAX: usize = 32;

// ============================================================
// Pid
// ============================================================

/// A process identifier. A newtype over `u64` so the type system can
/// tell "a PID" apart from "some u64" (a task index, a frame number, a
/// file descriptor, ...). Once a process has been assigned `Pid(N)`, no
/// other process is ever `Pid(N)` — `ProcessTable` never recycles.
///
/// `Copy` because a PID is just a name; handing it around by value is
/// fine and never affects the process it names.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pid(pub u64);

impl Pid {
    /// The kernel's first process — the one booting the system.
    pub const ZERO: Pid = Pid(0);

    /// Construct a `Pid` from a raw `u64`. Prefer
    /// `ProcessTable::allocate` for new processes; this is for callers
    /// that already hold a pid read out of a `Task`.
    pub const fn new(value: u64) -> Self {
        Pid(value)
    }

    /// The raw `u64` form. Used where the syscall ABI hands a pid back
    /// to user code as a plain integer (`sys_getpid`, `sys_fork`).
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl From<Pid> for u64 {
    fn from(pid: Pid) -> u64 {
        pid.0
    }
}

impl core::fmt::Display for Pid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

// ============================================================
// Process
// ============================================================

/// Fixed-size process display name. Owns its bytes inside `Process`, so
/// exec can store a name read from user memory without leaking it into a
/// static buffer.
#[derive(Copy, Clone)]
pub struct ProcessName {
    bytes: [u8; PROCESS_NAME_MAX],
    len: usize,
}

impl ProcessName {
    pub const fn empty() -> Self {
        ProcessName {
            bytes: [0; PROCESS_NAME_MAX],
            len: 0,
        }
    }

    pub fn new(name: &str) -> Self {
        let mut process_name = ProcessName::empty();
        process_name.set(name);
        process_name
    }

    pub fn set(&mut self, name: &str) {
        let bytes = name.as_bytes();
        let len = bytes.len().min(PROCESS_NAME_MAX);
        self.bytes[..len].copy_from_slice(&bytes[..len]);
        if len < PROCESS_NAME_MAX {
            self.bytes[len..].fill(0);
        }
        self.len = len;
    }

    pub fn as_str(&self) -> &str {
        // SAFETY: the only way to populate `bytes[..len]` is through
        // `set`, which copies from a valid UTF-8 `&str`.
        unsafe { core::str::from_utf8_unchecked(&self.bytes[..self.len]) }
    }
}

impl core::fmt::Display for ProcessName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The identity / process-tree / per-process-memory side of "a program
/// in flight." Everything that answers "who am I, what do I own" lives
/// here; everything that answers "how do I resume this paused register
/// frame" stays on `Task`.
///
/// Owned by `ProcessTable`. Looked up by the `Pid` that `Task` carries
/// as its back-reference.
pub struct Process {
    /// This process's own identifier. Set by `ProcessTable::allocate`
    /// (the value passed at construction is overwritten), so callers
    /// never need to know their own pid ahead of time.
    pub pid: Pid,
    /// Parent process, or `None` for the boot process. Set at fork
    /// time; walked by `wait` and for orphan reparenting (later).
    pub parent: Option<Pid>,
    /// Child process identifiers. Empty until `fork` adds one. Walked
    /// by `wait(-1)` (wait for any child).
    pub children: Vec<Pid>,
    /// Exit code set by `sys_exit`. `None` while the process runs;
    /// read by the parent's `wait`. Kept until the parent reaps.
    pub exit_status: Option<i64>,
    /// Human-readable name, for log lines. Owned by the process as a
    /// fixed-size buffer; updated by `exec`.
    pub name: ProcessName,
    /// This process's address space — the PML4 the CPU should use when
    /// running it. Switched via CR3 on context switch.
    pub address_space: AddressSpace,
    /// Per-process file descriptor table. Empty at creation in this
    /// lesson; fork inheritance lands later.
    pub fd_table: FdTable,
}

impl Process {
    /// Build a `Process` ready for `ProcessTable::allocate`. `pid` is
    /// filled in by the table; callers pass whatever (it is ignored).
    pub fn new(
        name: &str,
        parent: Option<Pid>,
        address_space: AddressSpace,
        fd_table: FdTable,
    ) -> Self {
        Process {
            pid: Pid::ZERO, // overwritten by ProcessTable::allocate
            parent,
            children: Vec::new(),
            exit_status: None,
            name: ProcessName::new(name),
            address_space,
            fd_table,
        }
    }
}

// ============================================================
// ProcessTable
// ============================================================

/// The process table. Indexed by `Pid` (which is also the slot index),
/// so lookup is `O(1)`. We pay for that with sparse slots whenever a
/// process is reaped — acceptable in a teaching kernel that won't live
/// long enough to wrap even a 32-bit counter, and the right call over
/// `BTreeMap` for mini-os because of the constant-time lookup.
///
/// `next_pid` only ever increases; slots go `Some` → `None` on reap,
/// never back. This is the invariant that makes PID references held
/// anywhere else in the kernel stable across scheduler-vector mutation.
pub struct ProcessTable {
    /// The next pid `allocate` will hand out.
    next_pid: Pid,
    /// Slots indexed by pid. `None` once the process that lived here
    /// has been reaped.
    processes: Vec<Option<Process>>,
}

impl ProcessTable {
    const fn new() -> Self {
        ProcessTable {
            next_pid: Pid::ZERO,
            processes: Vec::new(),
        }
    }

    /// Insert `proc` at the next pid, bump the counter, and return the
    /// pid it was assigned. The counter never wraps silently; if it
    /// ever did we'd rather panic than hand out a duplicate.
    pub fn allocate(&mut self, mut proc: Process) -> Pid {
        let pid = self.next_pid;
        self.next_pid = Pid::new(
            self.next_pid
                .as_u64()
                .checked_add(1)
                .expect("PID counter wrapped"),
        );
        let needed = pid.as_u64() as usize;
        // Grow with `push(None)` rather than `resize(_, None)`: `resize`
        // needs `Option<Process>: Clone`, and `Process` (holding an
        // `AddressSpace` and an `FdTable`) deliberately isn't `Clone`.
        while self.processes.len() <= needed {
            self.processes.push(None);
        }
        proc.pid = pid;
        self.processes[pid.as_u64() as usize] = Some(proc);
        pid
    }

    /// Read-only lookup by pid.
    pub fn get(&self, pid: Pid) -> Option<&Process> {
        self.processes
            .get(pid.as_u64() as usize)
            .and_then(|slot| slot.as_ref())
    }

    /// Mutable lookup by pid.
    pub fn get_mut(&mut self, pid: Pid) -> Option<&mut Process> {
        self.processes
            .get_mut(pid.as_u64() as usize)
            .and_then(|slot| slot.as_mut())
    }

    /// Reap a process: pull it out of its slot. The pid is now retired
    /// for good — a later `allocate` gets a higher pid, never this one.
    pub fn remove(&mut self, pid: Pid) -> Option<Process> {
        self.processes
            .get_mut(pid.as_u64() as usize)
            .and_then(|slot| slot.take())
    }
}

lazy_static! {
    /// The kernel's one process table. Mirrors the `SCHEDULER` pattern:
    /// an `IrqMutex` behind `lazy_static!`. The guard disables interrupts
    /// before acquiring the spinlock and restores the previous IF state
    /// after unlocking, so timer preemption cannot strand the lock owner.
    pub static ref PROCESS_TABLE: IrqMutex<ProcessTable> = IrqMutex::new(ProcessTable::new());
}

/// Register the kernel's first process as PID 0. Called once, from
/// `_start`, before `task::init_main`. The boot process owns the
/// address space the bootloader set up (`AddressSpace::from_current`),
/// has no parent, and starts with an empty FD table.
///
/// Nothing reads the table until Lesson 17 step 3 wires the accessors
/// at it; this just establishes the slot so `init_main` can point the
/// main `Task` at `Pid(0)`.
pub fn init() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut table = PROCESS_TABLE.lock();
        let boot = Process::new("main", None, AddressSpace::from_current(), FdTable::new());
        let pid = table.allocate(boot);
        // The boot process is, by construction, the first allocation.
        assert_eq!(pid, Pid::ZERO, "boot process must be PID 0");
        println!("[process] boot process registered as PID 0");
    });
}
