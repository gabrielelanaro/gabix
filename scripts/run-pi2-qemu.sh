#!/bin/sh
# Run an ARM kernel as a Raspberry Pi 2B in QEMU.
#
# Usage:
#   ./scripts/run-pi2-qemu.sh [path/to/kernel7.img]
#
# Environment:
#   QEMU_SYSTEM_ARM  Override the qemu-system-arm executable.
#   QEMU_DISPLAY     QEMU display backend (default: gtk; use none for headless).
#   QEMU_SERIAL      Serial backend (default: stdio; e.g. file:pi2.serial.log).
#   QEMU_TIMEOUT     Optional timeout in seconds for automated runs.
#
# The current kernel is still x86-64. This runner becomes usable after the
# ARM port produces an ARMv7 kernel image named kernel7.img.

set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
KERNEL=${1:-"$REPO_ROOT/target/armv7a-none-eabihf/debug/kernel7.img"}

if [ -n "${QEMU_SYSTEM_ARM:-}" ]; then
    QEMU="$QEMU_SYSTEM_ARM"
elif command -v qemu-system-arm >/dev/null 2>&1; then
    QEMU=$(command -v qemu-system-arm)
elif [ -x /tmp/gabix-qemu-arm/root/usr/bin/qemu-system-arm ]; then
    # User-local fallback used by this workspace when sudo is unavailable.
    QEMU=/tmp/gabix-qemu-arm/root/usr/bin/qemu-system-arm
else
    echo "qemu-system-arm was not found." >&2
    echo "  Debian: sudo apt install qemu-system-arm" >&2
    echo "  macOS:  brew install qemu" >&2
    exit 1
fi

if [ ! -f "$KERNEL" ]; then
    echo "ARM kernel image not found: $KERNEL" >&2
    echo "Build the ARM port first, then pass its kernel7.img path." >&2
    exit 1
fi

if ! "$QEMU" -machine help | grep -q '^raspi2b[[:space:]]'; then
    echo "$QEMU does not provide the raspi2b machine." >&2
    exit 1
fi

set -- "$QEMU" \
    -M raspi2b \
    -kernel "$KERNEL" \
    -serial "${QEMU_SERIAL:-stdio}" \
    -display "${QEMU_DISPLAY:-gtk}" \
    -no-reboot

if [ -n "${QEMU_TIMEOUT:-}" ]; then
    exec timeout "$QEMU_TIMEOUT" "$@"
else
    exec "$@"
fi
