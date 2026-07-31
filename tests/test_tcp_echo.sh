#!/bin/sh
. "$(dirname "$0")/common.sh"
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-1}

echo "=== TEST Lesson 33 TCP passive open, echo, and teardown ==="
build_kernel "test-mode,test-tcp-echo"
out=$(mktemp /tmp/mini-os-test-tcp-echo.XXXXXX)
qemu_log="${out}.qemu"

(
    cd "$REPO_ROOT"
    "$TIMEOUT_CMD" "$QEMU_TIMEOUT" qemu-system-x86_64 \
        -drive format=raw,file="$KERNEL" \
        -drive file="$DISK_IMG",format=raw,if=none,id=drv0 \
        -device virtio-blk-pci,drive=drv0 \
        -netdev user,id=n0,hostfwd=tcp:127.0.0.1:8080-:8080 \
        -device virtio-net-pci,netdev=n0,disable-legacy=on,disable-modern=off \
        -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
        -display none -serial "file:$out" -no-reboot
) >"$qemu_log" 2>&1 &
qemu_pid=$!

ready=0
for _ in $(seq 1 100); do
    if grep -q "\[tcp-echo\] listening" "$out" 2>/dev/null; then
        ready=1
        break
    fi
    sleep 0.1
done

echoed=""
if [ "$ready" -eq 1 ]; then
    echoed=$(python3 - <<'PY'
import socket
s = socket.create_connection(("127.0.0.1", 8080), timeout=5)
s.sendall(b"lesson-33-echo")
s.shutdown(socket.SHUT_WR)
data = b""
while True:
    chunk = s.recv(1024)
    if not chunk:
        break
    data += chunk
s.close()
print(data.decode("ascii"))
PY
)
fi

wait "$qemu_pid"
exit_code=$?
cat "$qemu_log" >>"$out"
rm -f "$qemu_log"
grep -E "\[tcp(-echo)?\]" "$out" | tail -80

if [ "$echoed" = "lesson-33-echo" ] \
    && qemu_exit_was_success "$exit_code" \
    && grep -q "state LISTEN -> SYN_RCVD" "$out" \
    && grep -q "state SYN_RCVD -> ESTABLISHED" "$out" \
    && grep -q "state CloseWait -> LastAck" "$out" \
    && grep -q "state LastAck -> Closed" "$out"; then
    echo "[tcp-echo-test] PASS"
    rm "$out"
    exit 0
fi
echo "[tcp-echo-test] FAIL; serial log left at $out" >&2
exit 1
