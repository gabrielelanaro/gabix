//! UDP socket ownership, datagram queues and scheduler wakeups.
//!
//! A port identifies one socket globally. File descriptors hold strong Arc
//! handles; the port registry holds Weak handles so it does not keep sockets
//! alive. Datagram payloads are copied into an unbounded FIFO queue.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;

use crate::irq_mutex::IrqMutex;
use crate::process::Pid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Endpoint {
    pub ip: [u8; 4],
    pub port: u16,
}

pub struct Datagram {
    pub sender: Endpoint,
    pub payload: Vec<u8>,
}

struct State {
    port: Option<u16>,
    queue: VecDeque<Datagram>,
    // One receive waiter: a second registration replaces the first PID.
    recv_waiter: Option<Pid>,
    // Poll can register multiple tasks; each PID appears at most once.
    poll_waiters: Vec<Pid>,
    nonblocking: bool,
}

/// Shared UDP state. Forked descriptors refer to the same queue and binding.
/// This object tracks a port only; IP/interface filtering occurs elsewhere.
pub struct UdpSocket {
    state: IrqMutex<State>,
}

// Dead Weak entries remain until that port is rebound. Delivery ignores them.
// Lock order for binding is PORTS then socket state; delivery drops PORTS
// before taking socket state and releases both before scheduler wakeups.
lazy_static::lazy_static! {
    static ref PORTS: IrqMutex<BTreeMap<u16, Weak<UdpSocket>>> =
        IrqMutex::new(BTreeMap::new());
}

impl UdpSocket {
    pub fn new(nonblocking: bool) -> Arc<Self> {
        Arc::new(Self {
            state: IrqMutex::new(State {
                port: None,
                queue: VecDeque::new(),
                recv_waiter: None,
                poll_waiters: Vec::new(),
                nonblocking,
            }),
        })
    }

    /// Reserve a nonzero port if no live socket owns it. A socket can bind
    /// only once. Port zero is rejected; no ephemeral-port allocation exists.
    pub fn bind(self: &Arc<Self>, port: u16) -> Result<(), ()> {
        if port == 0 {
            return Err(());
        }
        let mut ports = PORTS.lock();
        if ports.get(&port).and_then(Weak::upgrade).is_some() {
            return Err(());
        }
        let mut state = self.state.lock();
        if state.port.is_some() {
            return Err(());
        }
        state.port = Some(port);
        ports.insert(port, Arc::downgrade(self));
        Ok(())
    }

    pub fn local_port(&self) -> Option<u16> {
        self.state.lock().port
    }

    pub fn is_nonblocking(&self) -> bool {
        self.state.lock().nonblocking
    }

    pub fn is_readable(&self) -> bool {
        !self.state.lock().queue.is_empty()
    }

    /// Remove one complete datagram from the front. An empty Vec payload is
    /// still a datagram; None means the queue was empty at this lock acquisition.
    pub fn pop(&self) -> Option<Datagram> {
        self.state.lock().queue.pop_front()
    }

    /// If the queue is empty, record the receiving task and return true.
    /// This does not mark the task Blocked. The syscall caller must coordinate
    /// registration with its scheduler-state change to avoid a missed wakeup.
    pub fn register_waiter_if_empty(&self, pid: Pid) -> bool {
        let mut state = self.state.lock();
        if !state.queue.is_empty() {
            return false;
        }
        state.recv_waiter = Some(pid);
        true
    }

    /// Register a unique poll waiter only while empty. A false result means
    /// data is already available. The caller manages blocking and rechecking.
    pub fn register_poll_waiter_if_empty(&self, pid: Pid) -> bool {
        let mut state = self.state.lock();
        if !state.queue.is_empty() {
            return false;
        }
        if !state.poll_waiters.contains(&pid) {
            state.poll_waiters.push(pid);
        }
        true
    }

    pub fn remove_poll_waiter(&self, pid: Pid) {
        self.state
            .lock()
            .poll_waiters
            .retain(|waiter| *waiter != pid);
    }
}

/// Find the live socket bound to port, copy the payload, then wake and remove
/// all recorded waiters after releasing the socket lock. True means queued,
/// not consumed by a reader. False means no live binding. There is no queue
/// capacity/backpressure check; allocation failure follows the kernel's allocator
/// failure path. A wakeup makes a task Ready without selecting when it runs.
pub fn deliver(port: u16, sender: Endpoint, payload: &[u8]) -> bool {
    let socket = PORTS.lock().get(&port).and_then(Weak::upgrade);
    let Some(socket) = socket else {
        return false;
    };
    let (recv_waiter, poll_waiters) = {
        let mut state = socket.state.lock();
        state.queue.push_back(Datagram {
            sender,
            payload: payload.to_vec(),
        });
        (
            state.recv_waiter.take(),
            core::mem::take(&mut state.poll_waiters),
        )
    };
    if let Some(pid) = recv_waiter {
        crate::task::wake(pid);
    }
    for pid in poll_waiters {
        crate::task::wake(pid);
    }
    true
}
