#![no_std]
#![no_main]

const SYS_NET_CONFIGURE: u64 = 904;
const XID: u32 = 0x4d49_4e49;

#[repr(C)]
struct SockAddrIn { family: u16, port: u16, address: [u8; 4], zero: [u8; 8] }

#[repr(C)]
struct NetworkConfig {
    address: [u8; 4], subnet_mask: [u8; 4], gateway: [u8; 4], dns: [u8; 4],
    lease_seconds: u32, mac: [u8; 6], padding: [u8; 2],
}

struct Reply { message_type: u8, address: [u8; 4], server: [u8; 4], mask: [u8; 4], gateway: [u8; 4], dns: [u8; 4], lease: u32 }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut config = NetworkConfig { address:[0;4], subnet_mask:[0;4], gateway:[0;4], dns:[0;4], lease_seconds:0, mac:[0;6], padding:[0;2] };
    if syscall2(SYS_NET_CONFIGURE, &mut config as *mut _ as u64, 0) != 0 { exit(10); }
    let fd = syscall3(41, 2, 2, 0);
    let local = SockAddrIn { family:2, port:68u16.to_be(), address:[0;4], zero:[0;8] };
    let server = SockAddrIn { family:2, port:67u16.to_be(), address:[255;4], zero:[0;8] };
    if fd == u64::MAX || syscall3(49, fd, &local as *const _ as u64, 16) != 0 { exit(11); }

    let mut packet = [0u8; 300];
    let discover_len = build(&mut packet, config.mac, 1, None, None);
    write(b"[dhcp-client] DISCOVER\n");
    if syscall6(44,fd,packet.as_ptr() as u64,discover_len as u64,0,&server as *const _ as u64,16) != discover_len as u64 { exit(12); }
    let offer = receive(fd, &mut packet);
    if offer.message_type != 2 { exit(13); }
    write(b"[dhcp-client] OFFER\n");

    let request_len = build(&mut packet, config.mac, 3, Some(offer.address), Some(offer.server));
    write(b"[dhcp-client] REQUEST\n");
    if syscall6(44,fd,packet.as_ptr() as u64,request_len as u64,0,&server as *const _ as u64,16) != request_len as u64 { exit(14); }
    let ack = receive(fd, &mut packet);
    if ack.message_type != 5 || ack.address != offer.address { exit(15); }
    write(b"[dhcp-client] ACK\n");

    config.address=ack.address; config.subnet_mask=ack.mask; config.gateway=ack.gateway;
    config.dns=ack.dns; config.lease_seconds=ack.lease;
    if syscall2(SYS_NET_CONFIGURE, &mut config as *mut _ as u64, 1) != 0 { exit(16); }
    if config.address != [10,0,2,15] || config.gateway != [10,0,2,2] { exit(17); }
    write(b"[dhcp-client] lease 10.0.2.15 installed from ring 3\n");
    exit(0)
}

fn build(out:&mut [u8], mac:[u8;6], kind:u8, requested:Option<[u8;4]>, server:Option<[u8;4]>)->usize {
    out.fill(0); out[0]=1; out[1]=1; out[2]=6; out[4..8].copy_from_slice(&XID.to_be_bytes());
    out[10..12].copy_from_slice(&0x8000u16.to_be_bytes()); out[28..34].copy_from_slice(&mac);
    out[236..240].copy_from_slice(&[99,130,83,99]); let mut p=240;
    option(out,&mut p,53,&[kind]);
    if let Some(ip)=requested { option(out,&mut p,50,&ip); }
    if let Some(ip)=server { option(out,&mut p,54,&ip); }
    option(out,&mut p,55,&[1,3,6,51,54]); out[p]=255; p+1
}
fn option(out:&mut[u8], p:&mut usize, code:u8, value:&[u8]) { out[*p]=code; out[*p+1]=value.len() as u8; out[*p+2..*p+2+value.len()].copy_from_slice(value); *p+=2+value.len(); }
fn receive(fd:u64, packet:&mut[u8])->Reply {
    let count=syscall6(45,fd,packet.as_mut_ptr() as u64,packet.len() as u64,0,0,0);
    if count==u64::MAX { exit(20); } parse(&packet[..count as usize]).unwrap_or_else(|| exit(21))
}
fn parse(bytes:&[u8])->Option<Reply> {
    if bytes.len()<241 || bytes[0]!=2 || u32::from_be_bytes(bytes[4..8].try_into().ok()?)!=XID || bytes[236..240]!=[99,130,83,99] { return None; }
    let mut r=Reply { message_type:0,address:bytes[16..20].try_into().ok()?,server:[0;4],mask:[0;4],gateway:[0;4],dns:[0;4],lease:0 };
    let mut p=240; while p<bytes.len() { let code=bytes[p]; p+=1; if code==255 { break } if code==0 { continue } if p>=bytes.len(){return None} let n=bytes[p] as usize;p+=1;if p+n>bytes.len(){return None} let v=&bytes[p..p+n];
        match (code,n) { (53,1)=>r.message_type=v[0], (54,4)=>r.server.copy_from_slice(v), (1,4)=>r.mask.copy_from_slice(v), (3,4)=>r.gateway.copy_from_slice(v), (6,4)=>r.dns.copy_from_slice(v), (51,4)=>r.lease=u32::from_be_bytes(v.try_into().ok()?), _=>{} } p+=n;
    } Some(r)
}
fn syscall2(n:u64,a:u64,b:u64)->u64 { syscall3(n,a,b,0) }
fn syscall3(n:u64,a:u64,b:u64,c:u64)->u64 { let r;unsafe{core::arch::asm!("syscall",in("rax")n,in("rdi")a,in("rsi")b,in("rdx")c,lateout("rax")r,lateout("rcx")_,lateout("r11")_)}r }
fn syscall6(n:u64,a:u64,b:u64,c:u64,d:u64,e:u64,f:u64)->u64 { let r;unsafe{core::arch::asm!("syscall",in("rax")n,in("rdi")a,in("rsi")b,in("rdx")c,in("r10")d,in("r8")e,in("r9")f,lateout("rax")r,lateout("rcx")_,lateout("r11")_)}r }
fn write(s:&[u8]) { syscall3(1,1,s.as_ptr() as u64,s.len() as u64); }
fn exit(c:u64)->! { unsafe{core::arch::asm!("syscall",in("rax")60,in("rdi")c,options(noreturn))} }
#[panic_handler] fn panic(_: &core::panic::PanicInfo)->! { exit(99) }
