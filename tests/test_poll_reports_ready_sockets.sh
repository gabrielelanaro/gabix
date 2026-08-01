#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST poll reports ready sockets ==="
prepare_test_disk tests/fixtures/nonblocking_echo.rs
build_kernel

out=$(mktemp /tmp/mini-os-test-poll.XXXXXX)
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
        -netdev user,id=n0,hostfwd=udp::5555-:5555,hostfwd=udp::7777-:7777 \
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
    if grep -Fq "[nonblocking] bound UDP ports 5555 and 7777" "$out" 2>/dev/null; then
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
    echo "[poll] FAIL; guest sockets were not ready; log left at $out" >&2
    exit 1
fi

if ! python3 <<'PY'
import socket

for port, payload in ((7777, b"second-socket"), (5555, b"first-socket")):
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.settimeout(0.25)
        for _ in range(20):
            sock.sendto(payload, ("127.0.0.1", port))
            try:
                reply, _ = sock.recvfrom(2048)
            except TimeoutError:
                continue
            if reply == payload:
                break
        else:
            raise SystemExit(f"socket {port} did not echo its payload")
PY
then
    echo "[poll] FAIL; a ready socket did not echo its payload; log left at $out" >&2
    exit 1
fi

wait "$qemu_pid"
exit_code=$?
qemu_pid=""

if qemu_exit_was_success "$exit_code"; then
    echo "[poll] PASS"
    trap - EXIT INT TERM
    cleanup
    rm "$out"
    exit 0
fi

echo "[poll] FAIL; serial log left at $out" >&2
exit 1
