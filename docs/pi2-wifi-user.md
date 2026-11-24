# Run user programs over Pi Wi-Fi

A Pi running a kernel built with the Wi-Fi and managed-service features can receive a 32-bit ARM ELF executable through its authenticated Wi-Fi management interface. The upload occupies RAM. Each executable starts in ARM user mode and exits through the Gabix system call interface. The Mac reports the program’s exit status. Program stdout goes to the Pi UART.

Build a program on the Mac and run it on the Pi:

```sh
sh scripts/build-arm-user.sh tests/fixtures/arm_syscall.rs target/arm_syscall.elf
python3 scripts/pi2-wifi-user.py target/arm_syscall.elf
```

To check repeated uploads on a physical Pi, run:

```sh
python3 tests/test_pi2_wifi_runs_user_programs.py --address gabix-pi2.local
```

The check builds two different executables. One writes a UART message and exits with 0. The other reads the CPU status register and returns its mode bits. The expected user-mode value is 16. The client checks the boot ID and kernel checksum before and after each program on the same authenticated connection.

## Execution

The service accepts one ELF at a time, up to 512 KiB, with ARM-state entry code and load segments in the existing user image window (0x41000000 to 0x41200000, end exclusive). Authentication covers every upload chunk. The loader verifies the complete CRC and the ELF segments before writing user memory. It queues entry, finishes the signed start reply, and leaves network polling before entering user mode. On exit, the foreground loop resumes and the Mac can fetch the exit status. A ten-second watchdog covers a program that fails to return. When an uploaded program blocks in a TCP socket call, the kernel polls the Wi-Fi device, services management requests and refreshes the watchdog as it makes polling progress. A program doing uninterrupted CPU work still has the ten-second execution watchdog.

For one-time kernel setup use the existing managed Wi-Fi build and deployment procedure in [Pi Wi-Fi](pi2-wifi.md). Subsequent program changes use only the ELF upload command.

## A TCP service in user mode

Run the physical echo test from the Mac:

```sh
python3 tests/test_pi2_wifi_tcp_echo.py --address gabix-pi2.local
```

The test uploads an ARM ELF into RAM. It binds TCP port 8080, accepts one connection, reads incoming bytes and writes them back. The Mac checks the first echo, waits 12 seconds on that connection, requests authenticated management status, then checks the second echo. It half-closes the stream and checks the server’s exit status.

The foreground ELF can open one IPv4 stream listener and accept one peer at a time. The socket calls use the Gabix syscall numbers for socket, bind, listen, accept, read, write and close. Bind accepts a wildcard IPv4 address. Accept takes null peer-address arguments. Reads and writes can return partial counts. A zero-byte read indicates peer shutdown. The ARM sender keeps one segment in flight and retries it while the kernel polls. Testing a full framebuffer with packet loss and a slow reader is a separate milestone.
