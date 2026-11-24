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

The service accepts one ELF at a time, up to 512 KiB, with ARM-state entry code and load segments in the existing user image window (0x41000000 to 0x41200000, end exclusive). Authentication covers every upload chunk. The loader verifies the complete CRC and the ELF segments before writing user memory. It queues entry, finishes the signed start reply, and leaves network polling before entering user mode. On exit, the foreground loop resumes and the Mac can fetch the exit status. A ten-second watchdog covers a program that fails to return. Network polling pauses while user code runs. Supporting a persistent userspace TCP server requires that polling to continue.

For one-time kernel setup use the existing managed Wi-Fi build and deployment procedure in [Pi Wi-Fi](pi2-wifi.md). Subsequent program changes use only the ELF upload command.
