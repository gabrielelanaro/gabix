#!/bin/sh
. "$(dirname "$0")/common.sh"
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-1}

echo "=== TEST Lesson 31 TCP handshake and first response byte ==="
build_kernel "test-mode,test-tcp"
out=$(mktemp /tmp/mini-os-test-tcp.XXXXXX)
boot_qemu_no_hostfwd "$out" || true
exit_code=$QEMU_EXIT_CODE
grep -E "\[tcp(-client)?\]" "$out" | tail -60
if qemu_exit_was_success "$exit_code" \
   && grep -q "state CLOSED -> SYN_SENT" "$out" \
   && grep -q "state SYN_SENT -> ESTABLISHED" "$out" \
   && grep -q "sent DNS query; sequence advanced by payload length" "$out" \
   && grep -q "received and ACKed first response byte" "$out"; then
    echo "[tcp-test] PASS"; rm "$out"; exit 0
fi
echo "[tcp-test] FAIL; serial log left at $out" >&2; exit 1
