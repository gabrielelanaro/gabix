#!/bin/sh
# E2E test: parse ARP, learn slirp's gateway MAC, and hit the ARP cache.

. "$(dirname "$0")/common.sh"

echo "=== TEST arp (parse + resolve + cache) ==="

build_kernel "test-mode"

out=$(mktemp /tmp/mini-os-test-arp.XXXXXX)
boot_qemu "$out"
exit_code=$QEMU_EXIT_CODE

echo "[arp-test] QEMU exit code: $exit_code"
echo "[arp-test] === ARP lines ==="
grep -E "\[arp\]" "$out" | tail -30

miss_count=$(grep -c "\[arp\] cache miss: asking for 10.0.2.2" "$out")
learn_count=$(grep -c "\[arp\] learned 10.0.2.2 -> 52:55:0a:00:02:02" "$out")
resolve_count=$(grep -c "\[arp\] 10.0.2.2 is at 52:55:0a:00:02:02" "$out")
hit_count=$(grep -c "\[arp\] cache hit: 10.0.2.2 is at 52:55:0a:00:02:02" "$out")
size_count=$(grep -c "\[arp\] cache entries=1" "$out")

echo "[arp-test] cache miss count: $miss_count"
echo "[arp-test] learned count: $learn_count"
echo "[arp-test] resolve count: $resolve_count"
echo "[arp-test] cache hit count: $hit_count"
echo "[arp-test] cache size count: $size_count"

if qemu_exit_was_success "$exit_code" \
   && [ "$miss_count" -ge 1 ] \
   && [ "$learn_count" -ge 1 ] \
   && [ "$resolve_count" -ge 1 ] \
   && [ "$hit_count" -ge 1 ] \
   && [ "$size_count" -ge 1 ]; then
    echo "[arp-test] PASS"
    rm "$out"
    exit 0
else
    echo "[arp-test] FAIL; serial log left at $out" >&2
    exit 1
fi
