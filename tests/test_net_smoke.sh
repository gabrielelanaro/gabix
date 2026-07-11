#!/bin/sh
# E2E test: a ring-3 user program triggers IPv4 TX and waits for a UDP packet
# sent from the host through QEMU user networking.

. "$(dirname "$0")/common.sh"

echo "=== TEST net_smoke (user-triggered IPv4 TX/RX) ==="

build_kernel "test-mode,test-net"

out=$(mktemp /tmp/mini-os-test-net-smoke.XXXXXX)
qemu_pid=""

cleanup() {
    if [ -n "$qemu_pid" ] && kill -0 "$qemu_pid" 2>/dev/null; then
        kill "$qemu_pid" 2>/dev/null || true
        wait "$qemu_pid" 2>/dev/null || true
    fi
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
        -serial stdio \
        -no-reboot
) > "$out" 2>&1 &
qemu_pid=$!

ready=0
i=0
while [ "$i" -lt 200 ]; do
    if grep -q "\[ip\] rx wait: user requested UDP receive on host 127.0.0.1:5555" "$out"; then
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
    echo "[net-smoke-test] FAIL; user program never reached IPv4 RX wait; serial log left at $out" >&2
    wait "$qemu_pid" 2>/dev/null || true
    qemu_pid=""
    exit 1
fi

python3 - <<'PY'
import socket
import time

payload = b"mini-os-net-smoke"
sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
for _ in range(30):
    sock.sendto(payload, ("127.0.0.1", 5555))
    time.sleep(0.05)
sock.close()
PY

wait "$qemu_pid"
exit_code=$?
qemu_pid=""

echo "[net-smoke-test] QEMU exit code: $exit_code"
echo "[net-smoke-test] === user/network lines ==="
grep -E "\[net_smoke\]|\\[net-sys\\]|\\[ip\\] (rx|tx|\\*\\*\\*)|\\[net\\] RX ethertype=0x0800" "$out" | tail -60

user_tx_count=$(grep -c "\[net_smoke\] request ipv4 tx" "$out")
user_rx_count=$(grep -c "\[net_smoke\] request ipv4 rx" "$out")
tx_count=$(grep -c "\[ip\] \*\*\* user-triggered IPv4 packet transmitted through QEMU \*\*\*" "$out")
wait_count=$(grep -c "\[ip\] rx wait: user requested UDP receive on host 127.0.0.1:5555" "$out")
rx_eth_count=$(grep -c "\[net\] RX ethertype=0x0800" "$out")
ip_rx_count=$(grep -c "\[ip\] rx proto=17 .* dst=10.0.2.15" "$out")
proof_count=$(grep -c "\[ip\] \*\*\* user-triggered host-forwarded UDP packet received through QEMU:" "$out")
done_count=$(grep -c "\[net_smoke\] user-triggered network rx ok" "$out")

echo "[net-smoke-test] user tx marker count: $user_tx_count"
echo "[net-smoke-test] user rx marker count: $user_rx_count"
echo "[net-smoke-test] TX proof count: $tx_count"
echo "[net-smoke-test] wait marker count: $wait_count"
echo "[net-smoke-test] IPv4 Ethernet RX count: $rx_eth_count"
echo "[net-smoke-test] IPv4 RX count: $ip_rx_count"
echo "[net-smoke-test] RX proof count: $proof_count"
echo "[net-smoke-test] done marker count: $done_count"

if qemu_exit_was_success "$exit_code" \
   && [ "$user_tx_count" -ge 1 ] \
   && [ "$user_rx_count" -ge 1 ] \
   && [ "$tx_count" -ge 1 ] \
   && [ "$wait_count" -ge 1 ] \
   && [ "$rx_eth_count" -ge 1 ] \
   && [ "$ip_rx_count" -ge 1 ] \
   && [ "$proof_count" -ge 1 ] \
   && [ "$done_count" -ge 1 ]; then
    echo "[net-smoke-test] PASS"
    rm "$out"
    exit 0
else
    echo "[net-smoke-test] FAIL; serial log left at $out" >&2
    exit 1
fi
