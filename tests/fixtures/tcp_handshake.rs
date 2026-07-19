#![no_std]
#![no_main]

const CONNECT: u64 = 905;
const SEND: u64 = 906;
const RECV: u64 = 907;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write(b"[tcp-client] connect 10.0.2.3:53\n");
    if syscall1(CONNECT, 0) != 0 { exit(10); }
    write(b"[tcp-client] ESTABLISHED\n");

    // DNS over TCP: a two-byte message length followed by an A query for example.com.
    let query = [0, 29, 0x31, 0x31, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0,
        7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0, 0, 1, 0, 1];
    if syscall3(SEND, query.as_ptr() as u64, query.len() as u64, 0) != query.len() as u64 { exit(11); }
    write(b"[tcp-client] sent DNS query; sequence advanced by payload length\n");
    let first = syscall1(RECV, 0);
    if first > 255 { exit(12); }
    write(b"[tcp-client] received and ACKed first response byte\n");
    exit(0)
}

fn syscall1(n:u64,a:u64)->u64 { let r; unsafe { core::arch::asm!("syscall",in("rax")n,in("rdi")a,lateout("rax")r,lateout("rcx")_,lateout("r11")_) } r }
fn syscall3(n:u64,a:u64,b:u64,c:u64)->u64 { let r; unsafe { core::arch::asm!("syscall",in("rax")n,in("rdi")a,in("rsi")b,in("rdx")c,lateout("rax")r,lateout("rcx")_,lateout("r11")_) } r }
fn write(s:&[u8]) { let r:u64; unsafe { core::arch::asm!("syscall",in("rax")1u64,in("rdi")1u64,in("rsi")s.as_ptr() as u64,in("rdx")s.len() as u64,lateout("rax")r,lateout("rcx")_,lateout("r11")_) }; let _=r; }
fn exit(c:u64)->! { unsafe { core::arch::asm!("syscall",in("rax")60u64,in("rdi")c,options(noreturn)) } }
#[panic_handler] fn panic(_: &core::panic::PanicInfo)->! { exit(99) }
