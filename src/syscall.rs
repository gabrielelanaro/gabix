//! System call number dispatch.

mod file;
mod process;
mod socket;
mod time;
mod user;

const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_POLL: u64 = 7;
const SYS_GETPID: u64 = 39;
const SYS_SOCKET: u64 = 41;
const SYS_CONNECT: u64 = 42;
const SYS_ACCEPT: u64 = 43;
const SYS_SENDTO: u64 = 44;
const SYS_RECVFROM: u64 = 45;
const SYS_BIND: u64 = 49;
const SYS_LISTEN: u64 = 50;
const SYS_FORK: u64 = 57;
const SYS_EXECVE: u64 = 59;
const SYS_EXIT: u64 = 60;
const SYS_REBOOT: u64 = 169;
const SYS_WAIT4: u64 = 61;
const SYS_CLOCK_GETTIME: u64 = 228;
const SYS_NET_CONFIGURE: u64 = 904;

pub(crate) extern "C" fn dispatch(
    num: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    arg6: u64,
) -> u64 {
    match num {
        SYS_READ => file::read(arg1, arg2, arg3),
        SYS_WRITE => file::write(arg1, arg2, arg3),
        SYS_OPEN => file::open(arg1),
        SYS_CLOSE => file::close(arg1),
        SYS_POLL => socket::poll(arg1, arg2, arg3),
        SYS_GETPID => process::getpid(),
        SYS_FORK => process::fork(),
        SYS_EXECVE => process::exec_elf(arg1),
        SYS_EXIT => process::exit(arg1),
        SYS_REBOOT => crate::reboot::reboot(),
        SYS_WAIT4 => process::wait(arg1, arg2),
        SYS_SOCKET => socket::socket(arg1, arg2, arg3),
        SYS_CONNECT => socket::connect(arg1, arg2, arg3),
        SYS_BIND => socket::bind(arg1, arg2, arg3),
        SYS_SENDTO => socket::sendto(arg1, arg2, arg3, arg4, arg5, arg6),
        SYS_RECVFROM => socket::recvfrom(arg1, arg2, arg3, arg4, arg5, arg6),
        SYS_NET_CONFIGURE => socket::net_configure(arg1, arg2),
        SYS_LISTEN => socket::listen(arg1, arg2),
        SYS_ACCEPT => socket::accept(arg1),
        SYS_CLOCK_GETTIME => time::clock_gettime(arg1, arg2),
        _ => u64::MAX,
    }
}
