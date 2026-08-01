#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST network delivers frame ==="
prepare_test_disk tests/fixtures/udp_echo.rs
build_kernel

out=$(mktemp /tmp/mini-os-test-network-frame.XXXXXX)
qemu_log="${out}.qemu"
qemu_pid=""

cleanup() {
    if [ -n "$qemu_pid" ] && kill -0 "$qemu_pid" 2>/dev/null; then
        kill "$qemu_pid" 2>/dev/null || true
        wait "$qemu_pid" 2>/dev/null || true
    fi
    if [ -s "$qemu_log" ]; then
        cat "$qemu_log" >> "$out"
    fi
    rm -f "$qemu_log"
}
trap cleanup EXIT INT TERM

(
    cd "$REPO_ROOT"
    "$TIMEOUT_CMD" "$QEMU_TIMEOUT" qemu-system-x86_64 \
        -drive format=raw,file="$KERNEL" \
        -drive file="$DISK_IMG",format=raw,if=none,id=drv0 \
        -device virtio-blk-pci,drive=drv0 \
        -netdev user,id=n0,hostfwd=udp::5555-:5555 \
        -device virtio-net-pci,netdev=n0,disable-legacy=on,disable-modern=off \
        -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
        -display none \
        -serial "file:$out" \
        -no-reboot
) >"$qemu_log" 2>&1 &
qemu_pid=$!

ready=0
i=0
while [ "$i" -lt 400 ]; do
    if grep -Fq "[echo_server] bound UDP port 5555" "$out" 2>/dev/null; then
        ready=1
        break
    fi
    if ! kill -0 "$qemu_pid" 2>/dev/null; then
        break
    fi
    sleep 0.05
    i=$((i + 1))
done

if [ "$ready" -ne 1 ]; then
    echo "[network-frame] FAIL; guest was not ready to receive; log left at $out" >&2
    exit 1
fi

python3 <<'PY'
import socket
import time

with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
    for _ in range(20):
        sock.sendto(b"mini-os-network-frame", ("127.0.0.1", 5555))
        time.sleep(0.05)
PY

wait "$qemu_pid"
exit_code=$?
qemu_pid=""

if qemu_exit_was_success "$exit_code"; then
    echo "[network-frame] PASS"
    trap - EXIT INT TERM
    cleanup
    rm "$out"
    exit 0
fi

echo "[network-frame] FAIL; serial log left at $out" >&2
exit 1
