#!/bin/sh
# E2E test: exec /inherit_wait.elf and verify fork FD inheritance plus wait(pid).
#
# The user program opens /hello.txt, reads "hello", forks, the child reads
# " from" through the inherited fd, and the parent waits for that exact PID
# before reading " disk" from the same open file cursor.

. "$(dirname "$0")/common.sh"

echo "=== TEST inherit_wait (FD inheritance + wait by PID) ==="

build_kernel "test-mode,test-inherit"

out=$(mktemp /tmp/mini-os-test-inherit-wait.XXXXXX)
boot_qemu "$out"
exit_code=$QEMU_EXIT_CODE

echo "[inherit] QEMU exit code: $exit_code"
echo "[inherit] === tail of serial output ==="
tail -20 "$out"

child_count=$(grep -c "\[inherit\] child read shared fd" "$out")
wait_count=$(grep -c "\[inherit\] wait specific status=7" "$out")
cursor_count=$(grep -c "\[inherit\] parent saw child advance cursor" "$out")
closed_count=$(grep -c "closed_fds=1" "$out")

echo "[inherit] child marker count: $child_count"
echo "[inherit] wait marker count: $wait_count"
echo "[inherit] cursor marker count: $cursor_count"
echo "[inherit] closed_fds=1 count: $closed_count"

if qemu_exit_was_success "$exit_code" \
   && [ "$child_count" -ge 1 ] \
   && [ "$wait_count" -ge 1 ] \
   && [ "$cursor_count" -ge 1 ] \
   && [ "$closed_count" -ge 2 ]; then
    echo "[inherit] PASS"
    rm "$out"
    exit 0
else
    echo "[inherit] FAIL; serial log left at $out" >&2
    exit 1
fi
