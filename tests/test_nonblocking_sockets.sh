#!/bin/sh
. "$(dirname "$0")/common.sh"

echo "=== TEST Lesson 27 nonblocking sockets and poll ==="
build_kernel "test-mode,test-nonblocking"

out=$(mktemp /tmp/mini-os-test-nonblocking.XXXXXX)
qemu_pid=""
cleanup() {
    if [ -n "$qemu_pid" ] && kill -0 "$qemu_pid" 2>/dev/null; then kill "$qemu_pid" 2>/dev/null || true; fi
}
trap cleanup EXIT INT TERM

(
    cd "$REPO_ROOT"
    "$TIMEOUT_CMD" "$QEMU_TIMEOUT" qemu-system-x86_64 \
        -drive format=raw,file="$KERNEL" \
        -drive file="$DISK_IMG",format=raw,if=none,id=drv0 \
        -device virtio-blk-pci,drive=drv0 \
        -netdev user,id=n0,hostfwd=udp::5555-:5555,hostfwd=udp::7777-:7777 \
        -device virtio-net-pci,netdev=n0,disable-legacy=on,disable-modern=off \
        -device isa-debug-exit,iobase=0xf4,iosize=0x04 -display none -serial "file:$out" -no-reboot
) >/dev/null 2>&1 &
qemu_pid=$!

i=0
while [ "$i" -lt 400 ] && ! grep -q "bound UDP ports 5555 and 7777" "$out"; do
    kill -0 "$qemu_pid" 2>/dev/null || break
    sleep 0.05
    i=$((i + 1))
done
grep -q "bound UDP ports 5555 and 7777" "$out" || { echo "[nonblocking-test] guest did not become ready; log: $out" >&2; exit 1; }

python3 <<'PY'
import socket
for port, payload in ((7777, b"event on second socket"), (5555, b"event on first socket")):
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.settimeout(5)
    sock.sendto(payload, ("127.0.0.1", port))
    reply, _ = sock.recvfrom(2048)
    sock.close()
    if reply != payload:
        raise SystemExit(f"echo mismatch on {port}")
PY

wait "$qemu_pid"
exit_code=$?
qemu_pid=""
grep -E "\[nonblocking\]|\[udp\] rx" "$out" | tail -30

if qemu_exit_was_success "$exit_code" \
   && grep -q "poll woke for both sockets" "$out" \
   && grep -q "EAGAIN observed after draining each ready socket" "$out" \
   && grep -q "dst=.*:5555" "$out" \
   && grep -q "dst=.*:7777" "$out"; then
    echo "[nonblocking-test] PASS"
    rm "$out"
    exit 0
fi
echo "[nonblocking-test] FAIL; serial log left at $out" >&2
exit 1
