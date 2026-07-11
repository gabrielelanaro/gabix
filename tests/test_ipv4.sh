#!/bin/sh
# E2E test: build an IPv4 header and verify its checksum/parser path.

. "$(dirname "$0")/common.sh"

echo "=== TEST ipv4 (checksum + parse) ==="

build_kernel "test-mode"

out=$(mktemp /tmp/mini-os-test-ipv4.XXXXXX)
boot_qemu "$out"
exit_code=$QEMU_EXIT_CODE

echo "[ipv4-test] QEMU exit code: $exit_code"
echo "[ipv4-test] === IPv4 lines ==="
grep -E "\[ip\]" "$out" | tail -30

checksum_count=$(grep -c "\[ip\] checksum example 00 01 f2 03 -> 0x0DFB" "$out")
parse_count=$(grep -c "\[ip\] parsed header proto=17 src=10.0.2.15 dst=10.0.2.2 len=24 checksum ok" "$out")
proof_count=$(grep -c "\[ip\] \*\*\* IPv4 header built, checksum verified, parse proof complete \*\*\*" "$out")

echo "[ipv4-test] checksum count: $checksum_count"
echo "[ipv4-test] parse count: $parse_count"
echo "[ipv4-test] proof count: $proof_count"

if qemu_exit_was_success "$exit_code" \
   && [ "$checksum_count" -ge 1 ] \
   && [ "$parse_count" -ge 1 ] \
   && [ "$proof_count" -ge 1 ]; then
    echo "[ipv4-test] PASS"
    rm "$out"
    exit 0
else
    echo "[ipv4-test] FAIL; serial log left at $out" >&2
    exit 1
fi
