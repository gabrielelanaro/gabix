use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use spin::Mutex;

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
    recv_waiter: Option<Pid>,
    poll_waiters: Vec<Pid>,
    nonblocking: bool,
}

pub struct UdpSocket {
    state: Mutex<State>,
}

lazy_static::lazy_static! {
    static ref PORTS: Mutex<BTreeMap<u16, Weak<UdpSocket>>> = Mutex::new(BTreeMap::new());
}

impl UdpSocket {
    pub fn new(nonblocking: bool) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                port: None,
                queue: VecDeque::new(),
                recv_waiter: None,
                poll_waiters: Vec::new(),
                nonblocking,
            }),
        })
    }

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

    pub fn pop(&self) -> Option<Datagram> {
        self.state.lock().queue.pop_front()
    }

    pub fn register_waiter_if_empty(&self, pid: Pid) -> bool {
        let mut state = self.state.lock();
        if !state.queue.is_empty() {
            return false;
        }
        state.recv_waiter = Some(pid);
        true
    }

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
        self.state.lock().poll_waiters.retain(|waiter| *waiter != pid);
    }
}

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
