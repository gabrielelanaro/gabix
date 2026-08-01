#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST TCP echoes payload ==="
prepare_test_disk tests/fixtures/tcp_echo.rs
build_kernel

out=$(mktemp /tmp/mini-os-test-tcp-echo.XXXXXX)
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
        -netdev user,id=n0,hostfwd=tcp:127.0.0.1:8080-:8080 \
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
    if grep -Fq "[tcp-echo] listening" "$out" 2>/dev/null; then
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
    echo "[tcp-echo] FAIL; guest did not listen on port 8080; log left at $out" >&2
    exit 1
fi

if ! python3 <<'PY'
import socket

payload = b"mini-os-tcp\x00\xff" + bytes(range(64))
with socket.create_connection(("127.0.0.1", 8080), timeout=5) as sock:
    sock.sendall(payload)
    received = b""
    while len(received) < len(payload):
        chunk = sock.recv(len(payload) - len(received))
        if not chunk:
            break
        received += chunk
    if received != payload:
        raise SystemExit("echo payload was not returned")
    sock.shutdown(socket.SHUT_WR)
PY
then
    echo "[tcp-echo] FAIL; host did not receive the matching payload; log left at $out" >&2
    exit 1
fi

wait "$qemu_pid"
exit_code=$?
qemu_pid=""

if qemu_exit_was_success "$exit_code"; then
    echo "[tcp-echo] PASS"
    trap - EXIT INT TERM
    cleanup
    rm "$out"
    exit 0
fi

echo "[tcp-echo] FAIL; serial log left at $out" >&2
exit 1
