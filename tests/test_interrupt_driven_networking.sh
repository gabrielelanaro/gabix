#!/bin/sh
# E2E proof for Lesson 29's deferred virtio-net path. Queue work runs in
# a blockable kernel task, woken by MSI-X when available and by the timer
# backstop in the current QEMU setup. No busy-wait loop remains.

. "$(dirname "$0")/common.sh"

echo "=== TEST interrupt-driven virtio-net I/O ==="

build_kernel "test-mode,test-udp"

out=$(mktemp /tmp/mini-os-test-net-irq.XXXXXX)
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
        -netdev user,id=n0,hostfwd=udp:127.0.0.1:5555-10.0.2.15:5555 \
        -device virtio-net-pci,netdev=n0,disable-legacy=on,disable-modern=off,vectors=3 \
        -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
        -display none \
        -serial "file:$out" \
        -no-reboot
) > "$qemu_log" 2>&1 &
qemu_pid=$!

ready=0
i=0
while [ "$i" -lt 400 ]; do
    if grep -q "\[echo_server\] bound UDP port 5555" "$out"; then
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
    echo "[net-irq-test] FAIL; guest never bound port 5555; serial log left at $out" >&2
    wait "$qemu_pid" 2>/dev/null || true
    qemu_pid=""
    exit 1
fi

if ! python3 <<'PY'
import socket

sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.settimeout(5)
try:
    for payload in (bytes(range(64)), bytes(range(256))):
        sock.sendto(payload, ("127.0.0.1", 5555))
        reply, address = sock.recvfrom(2048)
        if reply != payload:
            raise SystemExit(f"echo mismatch from {address}: {reply!r}")
    print("[net-irq-test] host received two byte-for-byte echoes")
except OSError as error:
    raise SystemExit(f"echo receive failed: {error}")
finally:
    sock.close()
PY
then
    echo "[net-irq-test] FAIL; host did not receive the expected echoes; serial log left at $out" >&2
    wait "$qemu_pid" 2>/dev/null || true
    qemu_pid=""
    exit 1
fi

wait "$qemu_pid"
exit_code=$?
qemu_pid=""

if [ -s "$qemu_log" ]; then
    cat "$qemu_log" >> "$out"
fi
rm -f "$qemu_log"

echo "[net-irq-test] QEMU exit code: $exit_code"
echo "[net-irq-test] === interrupt/network lines ==="
grep -E "MSI-X|first RX wakeup|first TX completion|\[echo_server\]" "$out" | tail -20

rx_msix_count=$(grep -c "MSI-X queue 0 (RX) -> table 0 -> vector 0x30" "$out")
tx_msix_count=$(grep -c "MSI-X queue 1 (TX) -> table 1 -> vector 0x31" "$out")
task_start_count=$(grep -c "\[net\] deferred RX task started" "$out")
echo_count=$(grep -c "\[echo_server\] echoed two datagrams from ring 3" "$out")

echo "[net-irq-test] RX MSI-X programming count: $rx_msix_count"
echo "[net-irq-test] TX MSI-X programming count: $tx_msix_count"
echo "[net-irq-test] deferred task start count: $task_start_count"
echo "[net-irq-test] echo server proof count: $echo_count"

if qemu_exit_was_success "$exit_code" \
   && [ "$rx_msix_count" -ge 1 ] \
   && [ "$tx_msix_count" -ge 1 ] \
   && [ "$task_start_count" -ge 1 ] \
   && [ "$echo_count" -ge 1 ]; then
    echo "[net-irq-test] PASS"
    rm "$out"
    exit 0
else
    echo "[net-irq-test] FAIL; serial log left at $out" >&2
    exit 1
fi
