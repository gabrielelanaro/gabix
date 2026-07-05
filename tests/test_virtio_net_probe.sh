#!/bin/sh
# E2E test: boot with the Lesson 19 virtio-net card and verify PCI sees it.
#
# Lesson 19's contract is discovery. Later lessons initialize the same card,
# but this test stays focused on the PCI proof.

. "$(dirname "$0")/common.sh"

echo "=== TEST virtio_net_probe (PCI sees virtio-net) ==="

build_kernel "test-mode"

out=$(mktemp /tmp/mini-os-test-virtio-net.XXXXXX)
boot_qemu "$out"
exit_code=$QEMU_EXIT_CODE

echo "[net] QEMU exit code: $exit_code"
echo "[net] === virtio-net lines ==="
grep -E "virtio-net|vendor=1af4 device=1041|BAR" "$out" | tail -20

net_count=$(grep -c "\[pci\] virtio-net found" "$out")
id_count=$(grep -c "vendor=1af4 device=1041" "$out")
bar_count=$(grep -c "\[pci\]   BAR" "$out")

echo "[net] virtio-net marker count: $net_count"
echo "[net] PCI device-id marker count: $id_count"
echo "[net] BAR line count: $bar_count"

if qemu_exit_was_success "$exit_code" \
   && [ "$net_count" -ge 1 ] \
   && [ "$id_count" -ge 1 ] \
   && [ "$bar_count" -ge 6 ]; then
    echo "[net] PASS"
    rm "$out"
    exit 0
else
    echo "[net] FAIL; serial log left at $out" >&2
    exit 1
fi
