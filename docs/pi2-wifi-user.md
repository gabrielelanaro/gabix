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

The foreground ELF can open one IPv4 stream listener and accept one peer at a time. The socket calls use the Gabix syscall numbers for socket, bind, listen, accept, read, write and close. Bind accepts a wildcard IPv4 address. Accept takes null peer-address arguments. Reads and writes can return partial counts. A zero-byte read indicates peer shutdown. The ARM sender keeps one segment in flight and retries it while the kernel polls.

## Transfer a V3D frame

The uploaded example listens on TCP port 18081. Each eight-byte request contains
the background color followed by the triangle color, both as four bytes in BGRA
order. The server asks the kernel to render a fresh triangle, then writes 64
rows of 64 BGRA pixels to the stream. Each reply is exactly 16,384 bytes.
The client can send another request on the same connection.

On a managed Wi-Fi Pi, syscall `0x4701` takes the user destination address in
`r0`, the packed background in `r1` and the packed foreground in `r2`. It returns
the byte count, or a negative error. The driver owns the GPU command lists,
shader and render target. After V3D signals frame completion the driver makes
the pixels visible to the ARM and copies them to the validated user buffer.

Run the physical test from the Mac with the usual Wi-Fi firmware environment:

```sh
python3 tests/test_pi2_v3d_frames.py --address gabix-pi2.local
```

To exercise TCP loss recovery, build the Wi-Fi kernel with
`GABIX_PI_FRAME_LOSS_TEST=1 sh scripts/build-arm.sh pi2-wifi-managed`, deploy
that image once, and run the same test with `--expect-drop`. In this build the
sender withholds one data segment on port 18081 after 16,384 bytes. The test
checks the Pi journal for a successful retransmission of the same sequence,
size and payload checksum. The Mac pauses before reading the second frame,
then reads up to 73 bytes at a time and compares both frames byte for byte.

## Drag the triangle in TigerVNC

With the managed Wi-Fi kernel that supports positioned draws running, build the VNC user program and upload it
to RAM. On the Mac, start TigerVNC Viewer and connect to the Pi.

```sh
sh scripts/build-arm-user.sh tests/fixtures/arm_vnc.rs
python3 scripts/pi2-wifi-user.py --detach target/arm_vnc.elf
open -a TigerVNC --args -SecurityTypes None gabix-pi2.local::5900
```

The **Gabix V3D triangle** window shows a gold triangle on a blue 512×512
desktop. The GPU renders 64×64 pixels. The server displays each pixel as an
8×8 block and sends the requested region using the viewer's color format.
Viewers can request RRE to receive runs of solid color. Raw pixels are also
supported.

The white hover dot has a black outline. TigerVNC receives its shape and
positions it locally. For viewers using framebuffer pixels for the pointer,
the server restores the old dot's background and paints the new position on
each update. Run `python3 tests/test_pi2_vnc.py` to check the enlarged frame,
drag and release, pointer updates, cursor shape and a cropped RGB565 region on the physical Pi.

Hold the left mouse button on a gold pixel, move the mouse, then release it.
The server keeps the point you grabbed under the cursor to the nearest GPU
pixel. Each move to a new GPU pixel changes the three vertex positions and
asks V3D to render again. The framebuffer clips the triangle at its edges.
Pressing the blue background leaves the triangle in place.

The positioned draw call (`0x4702`) uses the same first three arguments as
`0x4701`. Register `r3` contains signed pixel offsets packed into 16-bit halves,
X low and Y high. The original draw call always uses zero offsets.

This LAN demo uses RFB 3.8 security type None (no VNC password or
encryption). The ELF serves one viewer and exits when it disconnects. Repeat
the upload command for another session on the same Pi boot.
