#![no_std]
#![no_main]

const AF_INET: u64 = 2;
const SOCK_DGRAM: u64 = 2;

#[repr(C)]
struct SockAddrIn { family: u16, port: u16, address: [u8; 4], zero: [u8; 8] }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let fd = syscall3(41, AF_INET, SOCK_DGRAM, 0);
    let local = SockAddrIn { family: AF_INET as u16, port: 5555u16.to_be(), address: [0; 4], zero: [0; 8] };
    if fd == u64::MAX || syscall3(49, fd, &local as *const _ as u64, 16) != 0 { exit(10); }
    write(b"[echo_server] bound UDP port 5555\n");
    let mut buffer = [0u8; 1472];
    for _ in 0..2 {
        let mut peer = SockAddrIn { family: 0, port: 0, address: [0; 4], zero: [0; 8] };
        let mut peer_len: u32 = 16;
        let count = syscall6(45, fd, buffer.as_mut_ptr() as u64, buffer.len() as u64, 0, &mut peer as *mut _ as u64, &mut peer_len as *mut _ as u64);
        if count == u64::MAX || syscall6(44, fd, buffer.as_ptr() as u64, count, 0, &peer as *const _ as u64, 16) != count { exit(11); }
    }
    write(b"[echo_server] echoed two datagrams from ring 3\n"); exit(0)
}

fn syscall3(n:u64,a:u64,b:u64,c:u64)->u64 { let r; unsafe { core::arch::asm!("syscall",in("rax")n,in("rdi")a,in("rsi")b,in("rdx")c,lateout("rax")r,lateout("rcx")_,lateout("r11")_); } r }
fn syscall6(n:u64,a:u64,b:u64,c:u64,d:u64,e:u64,f:u64)->u64 { let r; unsafe { core::arch::asm!("syscall",in("rax")n,in("rdi")a,in("rsi")b,in("rdx")c,in("r10")d,in("r8")e,in("r9")f,lateout("rax")r,lateout("rcx")_,lateout("r11")_); } r }
fn write(s:&[u8]) { syscall3(1,1,s.as_ptr() as u64,s.len() as u64); }
fn exit(c:u64)->! { unsafe { core::arch::asm!("syscall",in("rax")60,in("rdi")c,options(noreturn)); } }
#[panic_handler] fn panic(_: &core::panic::PanicInfo)->! { exit(99) }
