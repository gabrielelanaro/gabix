# Run an ARM program in RAM over Wi-Fi

Boot the Pi 2B with `pi2-wifi-dev`. The Mac can then upload a small ARM program, call it repeatedly with different inputs, and read its output. New uploads replace the program in RAM. A reboot clears the session and starts the installed kernel from SD.

## Try the installed runner

From the repository root, build the timer probe and request a 1,000 µs delay.

```sh
sh scripts/build-pi2-ram-probe.sh targets/armv7/probes/timer.rs target/probes/timer.img
python3 scripts/pi2-ram-probe.py --image target/probes/timer.img --input-hex e8030000
```

The client prints a session number and two little-endian 32-bit timer readings. Their unsigned difference gives the elapsed microseconds. Reuse the printed number to request 2,000 µs.

```sh
python3 scripts/pi2-ram-probe.py --session "$session" --input-hex d0070000
```

To build and test the graphics probe, run the physical acceptance test. It measures both delays, reads the V3D ID, clears the target twice and verifies each pixel of two triangles. It saves the returned 64 × 64 frames as four bytes per pixel in blue, green, red, alpha order.

```sh
python3 tests/test_v3d_renders_triangle.py --out-dir "target/runs/$(date +%Y%m%d)-v3d"
```

For a single draw, pass `--image` with the built `v3d.img`, `--input-hex 02964219ff` and `--output triangle.bin`. The first byte selects the operation. `00` returns the identification register. `01` clears the frame and `02` draws the triangle. Clear and draw take four more bytes specifying the background color. Draw accepts four optional foreground bytes and defaults to gold.

## Calling convention

The entry point is an ARM-state AAPCS function at `0x00e00000`. The image and its zero-initialized data must fit in 256 KiB. The loader clears that region on each new upload. Globals persist across calls to the same image.

```rust
unsafe extern "C" fn probe(input: *const u8, len: usize, output: *mut u8, cap: usize) -> i32
```

The kernel passes up to 512 input bytes and a 32 KiB output buffer. Return the number of output bytes or a negative error code. The program runs synchronously in kernel space. It must preserve the calling convention, finish DMA before returning, and release ownership of devices used by the network stack. The kernel arms a ten-second hardware watchdog before the call.

The authenticated management protocol checks the boot identifier, sequence and HMAC. A retransmitted request receives the cached response. The runner checks the image CRC before its first call. One image and one result occupy RAM at a time. The host retrieves results in 512-byte chunks.

## Install the development kernel once

Use the same private credentials and RTL8188CU firmware as the managed Wi-Fi kernel. The generated kernel contains credentials and belongs in the private build directory. [Wi-Fi setup](pi2-wifi.md)

```sh
GABIX_WIFI_FIRMWARE=/absolute/path/to/urtwn-rtl8192cfwT \
GABIX_ARM_PROFILE=release sh scripts/build-arm.sh pi2-wifi-dev
python3 scripts/pi2-sd-transfer.py --address gabix-pi2.local --kernel \
  target/private-wifi/build/armv7a-none-eabihf/release/kernel-pi2-wifi-dev.img
python3 scripts/pi2-manage.py reboot --address gabix-pi2.local
python3 scripts/pi2-manage.py status --address gabix-pi2.local
```

The installer writes and verifies the inactive SD kernel copy. Once Wi-Fi rejoins, the development loop uses only the RAM probe client for uploads and calls. The client defaults to `gabix-pi2.local` and reads the same management key as the updater.

## GPU references

The probe gives V3D three vertices in screen coordinates. The binner writes a list for one 64 × 64 tile. The render thread executes that list, runs the fragment shader and stores the finished tile in RAM. The ARM waits for the frame counter to advance before reading the pixels. [Broadcom VideoCore IV 3D Architecture Reference Guide, control records and shader state](https://docs.broadcom.com/doc/12358545) · [Linux VC4 command submission](https://docs.kernel.org/gpu/vc4.html#memory-management-and-3d-command-submission) · [Peter Lemon's bare-metal Pi 2 triangle](https://github.com/PeterLemon/RaspberryPi/blob/master/V3D/ControlList/NV/Vertex_Array/Triangle/VertexColor/kernel7.asm)

The foreground bytes change an immediate constant in the fragment shader. On the physical Pi, updating the ARM copy alone sometimes left the QPU executing the previous color at the same address. The probe now asks firmware whether DMA channel 5 is available, then uses that channel to copy the shader from the direct RAM alias to the VideoCore L2-coherent alias. The control lists and framebuffer use the direct alias. The hardware test changes both colors on the second draw to check shader replacement and framebuffer readback together. [Probe implementation](../targets/armv7/probes/v3d.rs) · [Broadcom peripheral manual, address map and DMA](https://pip-assets.raspberrypi.com/categories/579-raspberry-pi-zero/documents/RP-008249-DS-1-bcm2835-peripherals.pdf)
