#!/bin/sh
# E2E test: exec /fork_smoke.elf, verify both parent and child markers appear.
#
# fork_smoke.elf calls sys_fork. Child prints "[fork_smoke] child" and exits.
# Parent prints "[fork_smoke] parent", sys_wait4s the child, then exits.
# The scheduler is preemptive so either branch can run first — the assertion
# is that BOTH markers appear, in any order.
#
# Pass: QEMU exits 33 AND both [fork_smoke] markers appear in serial.
# Fail: anything else (panic, hang, missing marker, only one marker).
#
# This test exercises the full fork machinery for the first time end-to-end:
# sys_fork handler, spawn_fork_child, fork_trampoline, deep_clone_from_current,
# sys_wait4, zombie reaping. If anything in that chain is broken, this test
# will catch it.

. "$(dirname "$0")/common.sh"

echo "=== TEST fork_smoke (sys_fork + sys_wait4 end-to-end) ==="

build_kernel "test-mode,test-fork"

out=$(mktemp /tmp/mini-os-test-fork-smoke.XXXXXX)
boot_qemu "$out"
exit_code=$QEMU_EXIT_CODE

echo "[fork_smoke] QEMU exit code: $exit_code"
echo "[fork_smoke] === tail of serial output ==="
tail -15 "$out"

child_count=$(grep -c "\[fork_smoke\] child" "$out")
parent_count=$(grep -c "\[fork_smoke\] parent" "$out")
echo "[fork_smoke] child marker count: $child_count"
echo "[fork_smoke] parent marker count: $parent_count"

if qemu_exit_was_success "$exit_code" \
   && [ "$child_count" -ge 1 ] \
   && [ "$parent_count" -ge 1 ]; then
    echo "[fork_smoke] PASS"
    rm "$out"
    exit 0
else
    echo "[fork_smoke] FAIL — serial log left at $out" >&2
    exit 1
fi
