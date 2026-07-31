#!/bin/sh
. "$(dirname "$0")/common.sh"
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-1}

echo "=== TEST Lesson 32 TCP reliable stream and timeout recovery ==="
build_kernel "test-mode,test-tcp-reliable"
out=$(mktemp /tmp/mini-os-test-tcp-reliable.XXXXXX)
boot_qemu_no_hostfwd "$out" || true
exit_code=$QEMU_EXIT_CODE
grep -E "\[tcp(-client|-test)?\]" "$out" | tail -80

data_segments=$(grep -c "\[tcp\] data tx" "$out" || true)
if qemu_exit_was_success "$exit_code" \
   && [ "$data_segments" -ge 3 ] \
   && grep -q "injected loss" "$out" \
   && grep -q "RTO expired" "$out" \
   && grep -q "Karn: skipped ambiguous RTT sample" "$out" \
   && grep -q "retransmit_queue=0" "$out" \
   && grep -q "received and ACKed first response byte" "$out"; then
    echo "[tcp-reliable-test] PASS"
    rm "$out"
    exit 0
fi
echo "[tcp-reliable-test] FAIL; serial log left at $out" >&2
exit 1
