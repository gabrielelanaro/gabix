//! File-descriptor syscall handlers.

use super::socket::{active_tcp_recv, active_tcp_send, transmit_tcp, ACTIVE_TCP_FD};
use super::user::{is_user_range, read_user_path};
use crate::fs;
use crate::println;
use crate::serial;
use alloc::sync::Arc;
use core::sync::atomic::Ordering;

pub(super) fn read(fd: u64, buffer_ptr: u64, length: u64) -> u64 {
    if !is_user_range(buffer_ptr, length) {
        println!(
            "[syscall] sys_read: bad user pointer buf=0x{:x} len={}",
            buffer_ptr, length
        );
        return u64::MAX;
    }

    let output = unsafe { core::slice::from_raw_parts_mut(buffer_ptr as *mut u8, length as usize) };
    if ACTIVE_TCP_FD.load(Ordering::Acquire) == fd {
        return active_tcp_recv(output) as u64;
    }
    if let Some(socket) = crate::task::with_current_fd_table(|table| table.get_tcp_socket(fd)) {
        loop {
            if let Some(count) = socket.read(output) {
                return count as u64;
            }
            crate::task::block_current_if(|pid| socket.register_recv_waiter(pid));
            crate::task::yield_now();
        }
    }

    let file = match crate::task::with_current_fd_table(|table| table.get(fd)) {
        Some(file) => file,
        None => return u64::MAX,
    };
    let data = file.inode.data.lock();
    let mut cursor = file.cursor.lock();
    let start = *cursor as usize;
    let count = (length as usize).min(data.len().saturating_sub(start));
    if count > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr().add(start), buffer_ptr as *mut u8, count);
        }
    }
    *cursor += count as u64;
    count as u64
}

pub(super) fn write(fd: u64, buffer_ptr: u64, length: u64) -> u64 {
    if !is_user_range(buffer_ptr, length) {
        return u64::MAX;
    }
    let input = unsafe { core::slice::from_raw_parts(buffer_ptr as *const u8, length as usize) };

    if fd == 1 || fd == 2 {
        let mut serial = serial::SERIAL1.lock();
        for &byte in input {
            serial.write_byte(byte);
        }
        return length;
    }
    if ACTIVE_TCP_FD.load(Ordering::Acquire) == fd {
        return active_tcp_send(input)
            .map(|count| count as u64)
            .unwrap_or(u64::MAX);
    }
    if let Some(socket) = crate::task::with_current_fd_table(|table| table.get_tcp_socket(fd)) {
        let Some(segments) = socket.send(input) else {
            return u64::MAX;
        };
        if segments.into_iter().all(transmit_tcp) {
            return length;
        }
        return u64::MAX;
    }

    let file = match crate::task::with_current_fd_table(|table| table.get(fd)) {
        Some(file) => file,
        None => return u64::MAX,
    };
    let mut data = file.inode.data.lock();
    let mut cursor = file.cursor.lock();
    let start = *cursor as usize;
    let Some(end) = start.checked_add(length as usize) else {
        return u64::MAX;
    };
    if end > data.len() {
        data.resize(end, 0);
    }
    data[start..end].copy_from_slice(input);
    *cursor += length;
    length
}

pub(super) fn close(fd: u64) -> u64 {
    if let Some(socket) = crate::task::with_current_fd_table(|table| table.get_tcp_socket(fd)) {
        if let Some(fin) = socket.close_segment() {
            if !transmit_tcp(fin) {
                return u64::MAX;
            }
            while !socket.is_closed() {
                crate::task::block_current_if(|pid| socket.register_close_waiter(pid));
                crate::task::yield_now();
            }
        }
    }
    if crate::task::with_current_fd_table(|table| table.close(fd)) {
        0
    } else {
        u64::MAX
    }
}

pub(super) fn open(name_ptr: u64) -> u64 {
    let Some(path) = read_user_path(name_ptr) else {
        return u64::MAX;
    };
    let Some(inode) = fs::resolve_path(&path) else {
        return u64::MAX;
    };
    crate::task::with_current_fd_table(|table| table.open(Arc::new(fs::File::new(inode))))
}
