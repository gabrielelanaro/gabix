#!/bin/sh
# E2E test: initialize virtio-net through DRIVER_OK with RX buffers posted.
#
# Lesson 20 stops before parsing frames. The test checks that the network card
# accepts our feature subset, exposes its MAC, enables RX/TX queues, receives
# an initial RX buffer supply, and reaches DRIVER_OK.

. "$(dirname "$0")/common.sh"

echo "=== TEST virtio_net_init (MAC + queues + RX supply) ==="

build_kernel "test-mode"

out=$(mktemp /tmp/mini-os-test-virtio-net-init.XXXXXX)
boot_qemu "$out"
exit_code=$QEMU_EXIT_CODE

echo "[net-init] QEMU exit code: $exit_code"
echo "[net-init] === virtio-net init lines ==="
grep -E "virtio-net.*(FEATURES_OK|MAC|queue [01]|posted 8 RX|DRIVER_OK|device is live)" "$out" | tail -30

features_count=$(grep -c "\[virtio-net\] device_status after FEATURES_OK" "$out")
mac_count=$(grep -c "\[virtio-net\] MAC" "$out")
rx_queue_count=$(grep -c "\[virtio-net\] queue 0 (RX) active" "$out")
tx_queue_count=$(grep -c "\[virtio-net\] queue 1 (TX) active" "$out")
rx_supply_count=$(grep -c "\[virtio-net\] posted 8 RX buffers" "$out")
driver_ok_count=$(grep -c "\[virtio-net\] device_status after DRIVER_OK: 0x0F" "$out")

echo "[net-init] FEATURES_OK count: $features_count"
echo "[net-init] MAC count: $mac_count"
echo "[net-init] RX queue count: $rx_queue_count"
echo "[net-init] TX queue count: $tx_queue_count"
echo "[net-init] RX supply count: $rx_supply_count"
echo "[net-init] DRIVER_OK count: $driver_ok_count"

if qemu_exit_was_success "$exit_code" \
   && [ "$features_count" -ge 1 ] \
   && [ "$mac_count" -ge 1 ] \
   && [ "$rx_queue_count" -ge 1 ] \
   && [ "$tx_queue_count" -ge 1 ] \
   && [ "$rx_supply_count" -ge 1 ] \
   && [ "$driver_ok_count" -ge 1 ]; then
    echo "[net-init] PASS"
    rm "$out"
    exit 0
else
    echo "[net-init] FAIL; serial log left at $out" >&2
    exit 1
fi
