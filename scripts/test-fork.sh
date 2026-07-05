#!/bin/sh
# Run the fork_smoke end-to-end test.
#
# Builds the kernel with --features test-mode,test-fork so that:
#   - main.rs execs /fork_smoke.elf instead of /hello.elf
#   - panics propagate through qemu_exit::exit_qemu(EXIT_FAILURE)
#   - scheduler's "all done" path calls exit_qemu(EXIT_SUCCESS)
#
# Boots QEMU with isa-debug-exit attached, captures serial output, and
# greps for the [fork_smoke] markers from both parent and child branches.
# Both markers must appear; either order is acceptable (the scheduler is
# preemptive, so the two branches can race).
#
# Pass: QEMU exit 33 AND both markers found.
# Fail: anything else.

DISK_IMG=disk.img
KERNEL=target/x86_64-mini-os/debug/bootimage-mini-os.bin

if [ ! -f "$DISK_IMG" ]; then
    echo "Run ./scripts/make-disk.sh first to create $DISK_IMG" >&2
    exit 1
fi

echo "[test-fork] building with --features test-mode,test-fork"
cargo bootimage --features test-mode,test-fork 2>&1 | tail -3

OUTPUT=$(mktemp /tmp/mini-os-test-fork-XXXXXX.log)

qemu-system-x86_64 \
    -drive format=raw,file="$KERNEL" \
    -drive file="$DISK_IMG",format=raw,if=none,id=drv0 \
    -device virtio-blk-pci,drive=drv0 \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    -display none \
    -serial stdio \
    -no-reboot \
    > "$OUTPUT" 2>&1

qemu_exit=$?

echo "[test-fork] === tail of serial output ==="
tail -25 "$OUTPUT"

child_found=$(grep -c "\[fork_smoke\] child" "$OUTPUT")
parent_found=$(grep -c "\[fork_smoke\] parent" "$OUTPUT")

echo "[test-fork] QEMU exit code: $qemu_exit"
echo "[test-fork] child marker count: $child_found"
echo "[test-fork] parent marker count: $parent_found"

if [ "$qemu_exit" -eq 33 ] \
   && [ "$child_found" -ge 1 ] \
   && [ "$parent_found" -ge 1 ]; then
    echo "[test-fork] PASS"
    rm "$OUTPUT"
    exit 0
else
    echo "[test-fork] FAIL" >&2
    echo "[test-fork] full serial output left at $OUTPUT for inspection" >&2
    exit 1
fi
