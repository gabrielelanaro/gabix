# Resources

# Networking (Phase 2)

## Knowledge

### Specifications (the ground truth)

- [OASIS VIRTIO 1.2 — section 5.1: Network Device](https://docs.oasis-open.org/virtio/virtio/v1.2/csd01/virtio-v1.2-csd01.html#x1-2170001)
  The device we already speak blk to. Use for: device ID 1 (modern PCI device ID `0x1041`), feature bits (`VIRTIO_NET_F_MAC`, `VIRTIO_NET_F_MRG_RXBUF`), the `virtio_net_hdr` that prefixes every frame, RX/TX queue layout (queue 0 = receiveq, queue 1 = transmitq).

- [RFC 826 — An Ethernet Address Resolution Protocol](https://www.rfc-editor.org/rfc/rfc826)
  ARP. Short, readable, from 1982. Use for: the request/reply packet layout and the merge/update algorithm.

- [RFC 791 — Internet Protocol](https://www.rfc-editor.org/rfc/rfc791)
  IPv4. Use for: header byte layout, fragmentation fields (which we will parse and ignore), TTL semantics.

- [RFC 792 — Internet Control Message Protocol](https://www.rfc-editor.org/rfc/rfc792)
  ICMP. Use for: echo request/reply layout — the network hello-world.

- [RFC 768 — User Datagram Protocol](https://www.rfc-editor.org/rfc/rfc768)
  UDP. Three pages. Use for: header layout and the pseudo-header checksum.

- [RFC 1071 — Computing the Internet Checksum](https://www.rfc-editor.org/rfc/rfc1071)
  The one's-complement checksum shared by IP, ICMP, and UDP. Use for: the fold-the-carries algorithm and its properties.

- [Linux man-pages — socket(2), bind(2), recvfrom(2), sendto(2)](https://man7.org/linux/man-pages/man2/socket.2.html)
  The user/kernel contract for Lesson 26. Use for: socket creation as an FD, local endpoint binding, whole-datagram receive with sender address, and destination-addressed send.

- [UC Berkeley — A 4.2BSD Interprocess Communication Primer (1983)](https://www2.eecs.berkeley.edu/Pubs/TechRpts/1983/5451.html)
  The historical API context for Lesson 26. Use for: why sockets were introduced as a general Unix interprocess-communication interface and how the classic lifecycle was presented to application programmers.

- [RFC 147 — Definition of a Socket](https://www.rfc-editor.org/rfc/rfc147) and [RFC 675 §2.2 — Sockets and Addressing](https://www.rfc-editor.org/rfc/rfc675#section-2.2)
  The pre-BSD network meaning of “socket”: one process-associated communication endpoint, with a connection identified by its two ends.

- [RFC 1122 — Requirements for Internet Hosts, section 4.1.4](https://www.rfc-editor.org/rfc/rfc1122#section-4.1.4)
  UDP/application interface requirements. Use for: information that must cross the transport/application boundary and request/response source-address behavior.

- [RFC 2131 — Dynamic Host Configuration Protocol](https://www.rfc-editor.org/rfc/rfc2131)
  DHCP. Use for: the DISCOVER/OFFER/REQUEST/ACK exchange (§3.1), broadcast addressing and the BROADCAST flag (§4.1), the client state machine (§4.4), and the T1/T2 lease timers (§4.4.5).

- [RFC 2132 — DHCP Options and BOOTP Vendor Extensions](https://www.rfc-editor.org/rfc/rfc2132)
  The TLV options area. Use for: option codes 53/50/54/51/58/59/1/3/6/255 and the `0x63825363` magic cookie (§3).

- [RFC 951 — Bootstrap Protocol](https://www.rfc-editor.org/rfc/rfc951)
  BOOTP. Use for: the 236-byte fixed header DHCP inherits — which fields are legacy (`sname`, `file`) and which still carry the conversation (`xid`, `chaddr`, `yiaddr`).

### Primary sources (Linux kernel)

- [Linux source: drivers/net/virtio_net.c](https://elixir.bootlin.com/linux/latest/source/drivers/net/virtio_net.c)
  The driver we are re-implementing. Use for: RX buffer pre-posting (`try_fill_recv`), TX path (`start_xmit`), header handling.

- [Linux source: net/ethernet/eth.c](https://elixir.bootlin.com/linux/latest/source/net/ethernet/eth.c)
  Ethernet header construction and parsing (`eth_header`, `eth_type_trans`).

- [Linux source: net/ipv4/arp.c](https://elixir.bootlin.com/linux/latest/source/net/ipv4/arp.c)
  ARP receive path (`arp_process`) — the RFC 826 algorithm in production form.

- [Linux source: net/ipv4/icmp.c](https://elixir.bootlin.com/linux/latest/source/net/ipv4/icmp.c)
  Echo handling (`icmp_echo`) — the reply is the request with two fields changed.

- [Linux source: net/ipv4/udp.c](https://elixir.bootlin.com/linux/latest/source/net/ipv4/udp.c)
  UDP receive and socket demultiplexing (`__udp4_lib_rcv`, `__udp4_lib_lookup`).

- [Linux source: net/socket.c](https://elixir.bootlin.com/linux/latest/source/net/socket.c)
  The syscall layer: `sys_socket`, `sys_bind`, `sys_sendto`, `sys_recvfrom` — how sockets become file descriptors.

- [Linux source: net/ipv4/ipconfig.c](https://elixir.bootlin.com/linux/latest/source/net/ipv4/ipconfig.c)
  The kernel's own DHCP/BOOTP client, used for diskless (NFS root) boot. The production counterpart to Lesson 30's user-space client.

### QEMU environment

- [QEMU documentation — Network emulation](https://www.qemu.org/docs/master/system/devices/net.html)
  User-mode (slirp) networking: guest network `10.0.2.0/24`, guest address `10.0.2.15`, gateway `10.0.2.2`, DNS `10.0.2.3`. `hostfwd` forwards host ports to the guest — the only way inbound traffic reaches a slirp guest. Constraint to design around: the guest can ping `10.0.2.2`, but the host cannot ping the guest, so the ICMP milestone is guest-initiated.

### Books

- [_TCP/IP Illustrated, Volume 1_ by W. Richard Stevens (2nd ed. with Kevin Fall)](https://www.oreilly.com/library/view/tcpip-illustrated-volume/9780132808200/)
  The classic protocol walkthrough with real packet captures. Use for: intuition per layer before reading the RFC. Chapters 3–4 (link layer, ARP), 5 (IP), 8 (ICMP), 10 (UDP).

### Companion Rust implementations

- [smoltcp](https://github.com/smoltcp-rs/smoltcp)
  A `no_std` TCP/IP stack in Rust, widely used in embedded. Use for: idiomatic Rust wire-format parsing (`wire/` module) and comparing our design decisions. Do not copy — read after building.

## Wisdom (Communities)

- [OSDev wiki — Network Stack](https://wiki.osdev.org/Network_Stack) and the [OSDev forum](https://forum.osdev.org/)
  Hobby-OS network stacks: virtio-net gotchas, slirp quirks, design threads.

- [r/osdev](https://reddit.com/r/osdev)
  Sanity checks on stack design and driver debugging.

## Gaps

- No single "network stack from scratch in Rust no_std" reference exists. smoltcp is the closest code artifact but it is a library, not a kernel driver + stack + sockets narrative. Our lessons fill this gap, as with the VFS.

---

# VFS Resources

## Knowledge

### Primary sources (Linux kernel)

- [Documentation/filesystems/vfs.rst — The Linux VFS Documentation](https://www.kernel.org/doc/html/latest/filesystems/vfs.html)
  The authoritative reference. Use for: the four VFS objects (superblock, inode, dentry, file), the method tables each one exposes, the syscall-to-filesystem dispatch path.

- [Linux source: fs/read_write.c](https://elixir.bootlin.com/linux/latest/source/fs/read_write.c)
  Implementation of `read`, `write`, `lseek`, `pread`, `pwrite` syscalls. Use for: seeing how a syscall walks from FD (File Descriptor) → `struct file` → `file_operations` → filesystem-specific handler.

- [Linux source: fs/ramfs/](https://elixir.bootlin.com/linux/latest/source/fs/ramfs)
  The simplest filesystem in the kernel (everything in memory, no disk). Use for: a minimal example of `inode_operations` and `file_operations`.

- [Linux source: include/linux/fs.h](https://elixir.bootlin.com/linux/latest/source/include/linux/fs.h)
  Definitions of `struct file`, `struct inode`, `struct file_operations`, etc. Use for: cross-referencing what each field means.

### Books

- [_Linux Kernel Development_ by Robert Love, Chapter 12: The Virtual Filesystem](https://rlove.org/)
  The most readable overview. Use for: intuition for the abstraction before reading kernel source.

- [_Understanding the Linux Kernel_ by Bovet & Cesati, Chapter 12: The Virtual Filesystem](https://www.oreilly.com/library/view/understanding-the-linux/0596005653/)
  Heavier than Love; useful for the locking and lifecycle details once we have the basics.

### Companion Rust kernels

- [Theseus OS — kernel/fs/](https://github.com/theseus-os/Theseus/tree/theseus_main/kernel/fs)
  Rust OS with a conventional VFS layer. Use for: idiomatic Rust patterns for these abstractions.

- [Redox OS — scheme/](https://doc.redox-os.org/book/ch046-schemes.html)
  Different design (unified resource scheme, not POSIX VFS), but useful for the comparison and the "what would a from-scratch design look like in Rust?" question.

## Wisdom (Communities)

- [OSDev forum — File Systems Design](https://forum.osdev.org/viewforum.php?f=1)
  Decades of discussion on VFS implementations for hobby OSes. Strong opinions, often well-reasoned.

- [r/osdev](https://reddit.com/r/osdev)
  Active subreddit; good for "is this design reasonable?" sanity checks.

## Gaps

- No single "VFS design from scratch" reference exists for a Rust no_std context. The Robert Love chapter plus Theseus source is the closest combination. We will be filling this gap with our own lessons.

## ARM scheduling parity
- ARM AAPCS32: https://github.com/ARM-software/abi-aa/blob/main/aapcs32/aapcs32.rst — integer register preservation and stack alignment.
- ARM32 scheduling, Linux maintainer walkthrough: https://people.kernel.org/linusw/the-arm32-scheduling-and-kernelspace-userspace-boundary

- ARM preemption: [Arm CMSIS ARMv7-A IRQ implementation](https://github.com/ARM-software/CMSIS_5/blob/develop/CMSIS/RTOS2/RTX/Source/GCC/irq_armv7a.S) — primary implementation reference for SRS/RFE exception state handling; read alongside gabix lesson 3.06.
