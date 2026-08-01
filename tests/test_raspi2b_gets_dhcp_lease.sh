#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST Raspberry Pi 2B gets DHCP lease ==="
kernel=$(sh "$REPO_ROOT/scripts/build-arm.sh" raspi2b | tail -1)
out=$(mktemp /tmp/mini-os-test-raspi2b-dhcp.XXXXXX)
peer_log="${out}.peer"
qemu_log="${out}.qemu"
socket_path="${out}.sock"
qemu=${QEMU_SYSTEM_ARM:-/tmp/gabix-qemu-arm/root/usr/bin/qemu-system-arm}
peer_pid=""

cleanup() {
    if [ -n "$peer_pid" ] && kill -0 "$peer_pid" 2>/dev/null; then
        kill "$peer_pid" 2>/dev/null || true
        wait "$peer_pid" 2>/dev/null || true
    fi
    rm -f "$socket_path"
}
trap cleanup EXIT INT TERM

python3 "$TESTS_DIR/support/arm_dhcp_peer.py" "$socket_path" >"$peer_log" 2>&1 &
peer_pid=$!

i=0
while [ "$i" -lt 100 ] && [ ! -S "$socket_path" ]; do
    sleep 0.02
    i=$((i + 1))
done

QEMU_MODULE_DIR=/tmp/gabix-qemu-arm/root/usr/lib/qemu \
"$TIMEOUT_CMD" "$QEMU_TIMEOUT" "$qemu" \
    -M raspi2b \
    -kernel "$kernel" \
    -netdev stream,id=n0,server=off,addr.type=unix,addr.path="$socket_path" \
    -device usb-net,netdev=n0,mac=52:54:00:12:34:56 \
    -semihosting-config enable=on,target=native \
    -display none \
    -audio none \
    -serial "file:$out" \
    -no-reboot >"$qemu_log" 2>&1
exit_code=$?

wait "$peer_pid"
peer_exit=$?
peer_pid=""

if [ "$exit_code" -eq 0 ] \
    && [ "$peer_exit" -eq 0 ] \
    && grep -Fqx "gabix raspi2b dhcp configured" "$out"; then
    echo "[raspi2b-dhcp] PASS"
    trap - EXIT INT TERM
    cleanup
    rm -f "$out" "$peer_log" "$qemu_log"
    exit 0
fi

cat "$peer_log" "$qemu_log" >>"$out"
echo "[raspi2b-dhcp] FAIL; log left at $out" >&2
exit 1
