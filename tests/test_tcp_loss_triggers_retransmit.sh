#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST TCP loss triggers retransmit ==="
prepare_test_disk tests/fixtures/tcp_handshake.rs
build_kernel

out=$(mktemp /tmp/mini-os-test-tcp-loss.XXXXXX)
qemu_log="${out}.qemu"
peer_log="${out}.peer"
socket_path="${out}.sock"
qemu_pid=""
peer_pid=""

cleanup() {
    if [ -n "$qemu_pid" ] && kill -0 "$qemu_pid" 2>/dev/null; then
        kill "$qemu_pid" 2>/dev/null || true
        wait "$qemu_pid" 2>/dev/null || true
    fi
    if [ -n "$peer_pid" ] && kill -0 "$peer_pid" 2>/dev/null; then
        kill "$peer_pid" 2>/dev/null || true
        wait "$peer_pid" 2>/dev/null || true
    fi
    rm -f "$socket_path"
}
trap cleanup EXIT INT TERM

python3 "$TESTS_DIR/support/tcp_loss_peer.py" "$socket_path" >"$peer_log" 2>&1 &
peer_pid=$!

i=0
while [ "$i" -lt 100 ] && [ ! -S "$socket_path" ]; do
    sleep 0.02
    i=$((i + 1))
done
if [ ! -S "$socket_path" ]; then
    echo "[tcp-loss] FAIL; external peer did not start; log left at $peer_log" >&2
    exit 1
fi

(
    cd "$REPO_ROOT"
    "$TIMEOUT_CMD" "$QEMU_TIMEOUT" qemu-system-x86_64 \
        -drive format=raw,file="$KERNEL" \
        -drive file="$DISK_IMG",format=raw,if=none,id=drv0 \
        -device virtio-blk-pci,drive=drv0 \
        -netdev stream,id=n0,server=off,addr.type=unix,addr.path="$socket_path" \
        -device virtio-net-pci,netdev=n0,disable-legacy=on,disable-modern=off \
        -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
        -display none \
        -serial "file:$out" \
        -no-reboot
) >"$qemu_log" 2>&1 &
qemu_pid=$!

wait "$qemu_pid"
exit_code=$?
qemu_pid=""
wait "$peer_pid"
peer_exit=$?
peer_pid=""

if qemu_exit_was_success "$exit_code" \
    && [ "$peer_exit" -eq 0 ] \
    && grep -Fq "[peer] retransmission observed" "$peer_log" \
    && grep -Fq "[tcp-client] response arrived after retransmit" "$out"; then
    echo "[tcp-loss] PASS"
    trap - EXIT INT TERM
    cleanup
    rm -f "$out" "$qemu_log" "$peer_log"
    exit 0
fi

cat "$qemu_log" >>"$out"
cat "$peer_log" >>"$out"
echo "[tcp-loss] FAIL; log left at $out" >&2
exit 1
