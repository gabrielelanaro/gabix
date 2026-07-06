#!/bin/sh
# E2E test: send an ARP request through virtio-net and receive slirp's reply.

. "$(dirname "$0")/common.sh"

echo "=== TEST virtio_net_frames (TX ARP + RX reply + recycle) ==="

build_kernel "test-mode"

out=$(mktemp /tmp/mini-os-test-virtio-net-frames.XXXXXX)
boot_qemu "$out"
exit_code=$QEMU_EXIT_CODE

echo "[net-frames] QEMU exit code: $exit_code"
echo "[net-frames] === Lesson 21 network lines ==="
grep -E "\[net\] (TX|RX|first|\\*\\*\\*)" "$out" | tail -30

tx_count=$(grep -c "\[net\] TX broadcast frame sent, 54 bytes" "$out")
rx_count=$(grep -c "\[net\] RX ethertype=0x0806" "$out")
recycle_count=$(grep -c "\[net\] RX buffer desc=.*recycled" "$out")
proof_count=$(grep -c "\[net\] \*\*\* first Ethernet frame received and RX buffer recycled \*\*\*" "$out")

echo "[net-frames] TX count: $tx_count"
echo "[net-frames] RX ARP count: $rx_count"
echo "[net-frames] recycle count: $recycle_count"
echo "[net-frames] proof count: $proof_count"

if qemu_exit_was_success "$exit_code" \
   && [ "$tx_count" -ge 1 ] \
   && [ "$rx_count" -ge 1 ] \
   && [ "$recycle_count" -ge 1 ] \
   && [ "$proof_count" -ge 1 ]; then
    echo "[net-frames] PASS"
    rm "$out"
    exit 0
else
    echo "[net-frames] FAIL; serial log left at $out" >&2
    exit 1
fi
