#!/bin/sh
# Shared helpers for test scripts. Sourced by tests/test_*.sh.
#
# Provides:
#   build_kernel <feature-flags>           cargo bootimage with the given features
#   boot_qemu <output-log-path>           run QEMU with isa-debug-exit, capture serial
#   qemu_exit_was_success <qemu-exit-code> 0 if exit 33 (PASS), 1 otherwise
#   serial_contains <log-path> <pattern>  0 if pattern found in serial log
#
# Convention: each test_*.sh script calls these helpers, then exits 0 on
# pass or 1 on fail. tests/run.sh sources this file and runs every test.

# Default timeout for QEMU runs (seconds). Most tests should finish in 5-15s;
# anything past 30s is almost certainly a hang.
QEMU_TIMEOUT=${QEMU_TIMEOUT:-30}

# Pick the right `timeout` coreutil. macOS doesn't ship one; users install
# `gtimeout` via `brew install coreutils`. Linux distributions have `timeout`
# in coreutils already.
if command -v timeout >/dev/null 2>&1; then
    TIMEOUT_CMD=timeout
elif command -v gtimeout >/dev/null 2>&1; then
    TIMEOUT_CMD=gtimeout
else
    echo "[common] neither 'timeout' nor 'gtimeout' found." >&2
    echo "[common]   Linux: already in coreutils" >&2
    echo "[common]   macOS: brew install coreutils" >&2
    exit 1
fi

DISK_IMG=disk.img
KERNEL=target/x86_64-mini-os/debug/bootimage-mini-os.bin

# Resolve repo root (parent of tests/).
TESTS_DIR=$(cd "$(dirname "$0")" && pwd)
REPO_ROOT=$(cd "$TESTS_DIR/.." && pwd)

build_kernel() {
    features=$1
    echo "[test] building kernel with features: $features"
    (
        cd "$REPO_ROOT"
        # `cargo bootimage` doesn't always invalidate the kernel crate when
        # only --features changes (the fingerprint doesn't track feature
        # sets the same way `cargo build` does). Clean the mini-os crate
        # before each build to guarantee the feature set actually applied.
        # Cost: ~6s rebuild per test. Better than silently running the
        # previous test's binary.
        cargo clean -p mini-os 2>&1 | tail -1
        cargo bootimage --features "$features" 2>&1 | tail -3
    )
}

# Boot QEMU with isa-debug-exit. Captures serial + QEMU's own stderr to $1.
# Returns QEMU's exit code via global QEMU_EXIT_CODE.
boot_qemu() {
    out_log=$1

    if [ ! -f "$REPO_ROOT/$DISK_IMG" ]; then
        echo "[test] missing $DISK_IMG — run ./scripts/make-disk.sh first" >&2
        return 1
    fi

    (
        cd "$REPO_ROOT"
        # `timeout` kills QEMU if it hangs. Without isa-debug-exit firing,
        # the only ways QEMU ends are: guest writes to 0xf4 (clean exit),
        # or timeout kills it (hang). The `-no-reboot` flag ensures triple
        # faults terminate instead of rebooting the guest.
        "$TIMEOUT_CMD" "$QEMU_TIMEOUT" qemu-system-x86_64 \
            -drive format=raw,file="$KERNEL" \
            -drive file="$DISK_IMG",format=raw,if=none,id=drv0 \
            -device virtio-blk-pci,drive=drv0 \
            -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
            -display none \
            -serial stdio \
            -no-reboot
    ) > "$out_log" 2>&1
    QEMU_EXIT_CODE=$?

    # 124 = timeout killed QEMU (didn't exit cleanly within the window).
    # 137 = SIGKILL (kill -9); shouldn't happen but log if it does.
    if [ "$QEMU_EXIT_CODE" -eq 124 ]; then
        echo "[test] QEMU timed out after ${QEMU_TIMEOUT}s — likely a hang" >&2
    fi
}

# QEMU exit code 33 means isa-debug-exit was written with 0x10 (success).
# Anything else (including 124 from timeout) is a failure.
qemu_exit_was_success() {
    [ "$1" -eq 33 ]
}

serial_contains() {
    log=$1
    pattern=$2
    grep -q "$pattern" "$log"
}
