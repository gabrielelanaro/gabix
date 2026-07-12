#![no_std]
#![no_main]

const AF_INET: u64 = 2;
const SOCK_DGRAM: u64 = 2;
const SOCK_NONBLOCK: u64 = 0x800;
const POLLIN: i16 = 1;
const EAGAIN: u64 = (-11i64) as u64;

#[repr(C)]
struct SockAddrIn { family: u16, port: u16, address: [u8; 4], zero: [u8; 8] }

#[repr(C)]
struct PollFd { fd: i32, events: i16, revents: i16 }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let a = bind_socket(5555);
    let b = bind_socket(7777);
    write(b"[nonblocking] bound UDP ports 5555 and 7777\n");

    let mut watched = [
        PollFd { fd: a as i32, events: POLLIN, revents: 0 },
        PollFd { fd: b as i32, events: POLLIN, revents: 0 },
    ];
    let mut buffer = [0u8; 1472];
    let mut echoes = 0;
    let mut drained = 0;
    while echoes < 2 {
        let ready = syscall3(7, watched.as_mut_ptr() as u64, 2, u64::MAX);
        if ready == 0 || ready == u64::MAX { exit(20); }
        for item in &mut watched {
            if item.revents & POLLIN == 0 { continue; }
            loop {
                let mut peer = SockAddrIn { family: 0, port: 0, address: [0; 4], zero: [0; 8] };
                let mut peer_len = 16u32;
                let count = syscall6(45, item.fd as u64, buffer.as_mut_ptr() as u64, buffer.len() as u64, 0, &mut peer as *mut _ as u64, &mut peer_len as *mut _ as u64);
                if count == EAGAIN { drained += 1; break; }
                if count == u64::MAX || syscall6(44, item.fd as u64, buffer.as_ptr() as u64, count, 0, &peer as *const _ as u64, 16) != count { exit(21); }
                echoes += 1;
            }
        }
    }
    if drained < 2 { exit(22); }
    write(b"[nonblocking] poll woke for both sockets\n");
    write(b"[nonblocking] EAGAIN observed after draining each ready socket\n");
    exit(0)
}

fn bind_socket(port: u16) -> u64 {
    let fd = syscall3(41, AF_INET, SOCK_DGRAM | SOCK_NONBLOCK, 0);
    let local = SockAddrIn { family: AF_INET as u16, port: port.to_be(), address: [0; 4], zero: [0; 8] };
    if fd == u64::MAX || syscall3(49, fd, &local as *const _ as u64, 16) != 0 { exit(10); }
    fd
}

fn syscall3(n:u64,a:u64,b:u64,c:u64)->u64 { let r; unsafe { core::arch::asm!("syscall",in("rax")n,in("rdi")a,in("rsi")b,in("rdx")c,lateout("rax")r,lateout("rcx")_,lateout("r11")_); } r }
fn syscall6(n:u64,a:u64,b:u64,c:u64,d:u64,e:u64,f:u64)->u64 { let r; unsafe { core::arch::asm!("syscall",in("rax")n,in("rdi")a,in("rsi")b,in("rdx")c,in("r10")d,in("r8")e,in("r9")f,lateout("rax")r,lateout("rcx")_,lateout("r11")_); } r }
fn write(s:&[u8]) { syscall3(1,1,s.as_ptr() as u64,s.len() as u64); }
fn exit(c:u64)->! { unsafe { core::arch::asm!("syscall",in("rax")60,in("rdi")c,options(noreturn)); } }
#[panic_handler] fn panic(_: &core::panic::PanicInfo)->! { exit(99) }
