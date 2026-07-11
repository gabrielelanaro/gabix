#!/bin/sh
# E2E test: host UDP enters QEMU, reaches guest port 5555, and returns
# byte-for-byte from the in-kernel UDP echo handler.
#
# This test owns the host-forwarded QEMU port and runs as one serial QEMU
# session. tests/run.sh invokes test scripts sequentially.

. "$(dirname "$0")/common.sh"

echo "=== TEST UDP echo (host → QEMU → guest → host) ==="

build_kernel "test-mode,test-udp"

out=$(mktemp /tmp/mini-os-test-udp.XXXXXX)
qemu_log="${out}.qemu"
qemu_pid=""

cleanup() {
    if [ -n "$qemu_pid" ] && kill -0 "$qemu_pid" 2>/dev/null; then
        kill "$qemu_pid" 2>/dev/null || true
        wait "$qemu_pid" 2>/dev/null || true
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
) > "$qemu_log" 2>&1 &
qemu_pid=$!

ready=0
i=0
while [ "$i" -lt 400 ]; do
    if grep -q "\[udp\] rx wait: user requested UDP echo on guest port 5555" "$out"; then
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
    echo "[udp-test] FAIL; guest never reached UDP echo wait; serial log left at $out" >&2
    wait "$qemu_pid" 2>/dev/null || true
    qemu_pid=""
    exit 1
fi

if ! python3 <<'PY'
import socket

payload = b"hello from lesson 25"
sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.settimeout(5)
try:
    sock.sendto(payload, ("127.0.0.1", 5555))
    reply, address = sock.recvfrom(2048)
    if reply != payload:
        raise SystemExit(f"UDP echo mismatch from {address}: {reply!r}")
    print(f"[udp-test] host received byte-for-byte echo ({len(reply)} bytes)")
except OSError as error:
    raise SystemExit(f"UDP echo receive failed: {error}")
finally:
    sock.close()
PY
then
    echo "[udp-test] FAIL; host did not receive the expected echo; serial log left at $out" >&2
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

echo "[udp-test] QEMU exit code: $exit_code"
echo "[udp-test] === UDP lines ==="
grep -E "\[udp(_echo|-sys)?\]" "$out" | tail -40

wire_count=$(grep -c "\[udp\] \*\*\* header, length rules, and IPv4 pseudo-header checksum proof complete \*\*\*" "$out")
wait_count=$(grep -c "\[udp\] rx wait: user requested UDP echo on guest port 5555" "$out")
rx_count=$(grep -c "\[udp\] rx src=.* dst=.*:5555 .*checksum ok" "$out")
echo_count=$(grep -c "\[udp\] \*\*\* guest port 5555 echoed one datagram \*\*\*" "$out")
proof_count=$(grep -c "\[udp\] \*\*\* user-triggered UDP datagram received and echoed:" "$out")
done_count=$(grep -c "\[udp_echo\] guest UDP echo verified" "$out")

echo "[udp-test] wire proof count: $wire_count"
echo "[udp-test] wait marker count: $wait_count"
echo "[udp-test] RX count: $rx_count"
echo "[udp-test] kernel echo count: $echo_count"
echo "[udp-test] user proof count: $proof_count"
echo "[udp-test] fixture success count: $done_count"

if qemu_exit_was_success "$exit_code" \
   && [ "$wire_count" -ge 1 ] \
   && [ "$wait_count" -ge 1 ] \
   && [ "$rx_count" -ge 1 ] \
   && [ "$echo_count" -ge 1 ] \
   && [ "$proof_count" -ge 1 ] \
   && [ "$done_count" -ge 1 ]; then
    echo "[udp-test] PASS"
    rm "$out"
    exit 0
else
    echo "[udp-test] FAIL; serial log left at $out" >&2
    exit 1
fi
