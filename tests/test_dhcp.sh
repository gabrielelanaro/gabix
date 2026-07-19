#!/bin/sh
. "$(dirname "$0")/common.sh"

echo "=== TEST DHCP lease acquisition ==="
echo "[test] building kernel with features: test-mode,test-dhcp"
build_log=$(mktemp /tmp/mini-os-dhcp-build.XXXXXX)
if ! (cd "$REPO_ROOT" && cargo bootimage --features "test-mode,test-dhcp" >"$build_log" 2>&1); then
    tail -20 "$build_log" >&2
    rm -f "$build_log"
    exit 1
fi
tail -3 "$build_log"
rm -f "$build_log"
out=$(mktemp /tmp/mini-os-test-dhcp.XXXXXX)
boot_qemu_no_hostfwd "$out" || true
exit_code=$QEMU_EXIT_CODE

grep -E "\[dhcp-client\]|\[dhcp\] configured" "$out"
if qemu_exit_was_success "$exit_code" \
   && grep -q "\[dhcp-client\] DISCOVER" "$out" \
   && grep -q "\[dhcp-client\] OFFER" "$out" \
   && grep -q "\[dhcp-client\] REQUEST" "$out" \
   && grep -q "\[dhcp-client\] ACK" "$out" \
   && grep -q "lease 10.0.2.15 installed from ring 3" "$out"; then
    echo "[dhcp-test] PASS"; rm "$out"; exit 0
fi
echo "[dhcp-test] FAIL; serial log left at $out" >&2; exit 1
