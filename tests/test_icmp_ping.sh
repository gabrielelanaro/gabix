#!/bin/sh
# E2E test: a ring-3 fixture asks the kernel to ping QEMU's slirp gateway.

. "$(dirname "$0")/common.sh"

echo "=== TEST ICMP echo (ring-3 gateway ping) ==="

build_kernel "test-mode,test-icmp"

out=$(mktemp /tmp/mini-os-test-icmp-ping.XXXXXX)
boot_qemu_no_hostfwd "$out"
exit_code=$QEMU_EXIT_CODE

echo "[icmp-test] QEMU exit code: $exit_code"
echo "[icmp-test] === ICMP lines ==="
grep -E "\[icmp_ping\]|\[icmp\]|\[ip\] tx proto=1" "$out" | tail -40

wire_count=$(grep -c "\[icmp\] \*\*\* request/reply layout builds, parses, matches, and rejects corruption \*\*\*" "$out")
request_count=$(grep -c "\[icmp_ping\] request gateway echo" "$out")
tx_count=$(grep -c "\[ip\] tx proto=1 .* dst=10.0.2.2" "$out")
seq7_count=$(grep -c "\[icmp\] reply id=0x2400 seq=7 payload_len=7" "$out")
seq8_count=$(grep -c "\[icmp\] reply id=0x2400 seq=8 payload_len=7" "$out")
seq9_count=$(grep -c "\[icmp\] reply id=0x2400 seq=9 payload_len=7" "$out")
seq10_count=$(grep -c "\[icmp\] reply id=0x2400 seq=10 payload_len=7" "$out")
proof_count=$(grep -c "\[icmp\] \*\*\* four user-triggered echo round trips verified \*\*\*" "$out")
done_count=$(grep -c "\[icmp_ping\] gateway echo verified" "$out")

echo "[icmp-test] wire proof count: $wire_count"
echo "[icmp-test] request count: $request_count"
echo "[icmp-test] ICMP TX count: $tx_count"
echo "[icmp-test] sequence 7 reply count: $seq7_count"
echo "[icmp-test] sequence 8 reply count: $seq8_count"
echo "[icmp-test] sequence 9 reply count: $seq9_count"
echo "[icmp-test] sequence 10 reply count: $seq10_count"
echo "[icmp-test] round-trip proof count: $proof_count"
echo "[icmp-test] fixture success count: $done_count"

if qemu_exit_was_success "$exit_code" \
   && [ "$wire_count" -ge 1 ] \
   && [ "$request_count" -ge 1 ] \
   && [ "$tx_count" -eq 4 ] \
   && [ "$seq7_count" -eq 1 ] \
   && [ "$seq8_count" -eq 1 ] \
   && [ "$seq9_count" -eq 1 ] \
   && [ "$seq10_count" -eq 1 ] \
   && [ "$proof_count" -ge 1 ] \
   && [ "$done_count" -ge 1 ]; then
    echo "[icmp-test] PASS"
    rm "$out"
    exit 0
else
    echo "[icmp-test] FAIL; serial log left at $out" >&2
    exit 1
fi
