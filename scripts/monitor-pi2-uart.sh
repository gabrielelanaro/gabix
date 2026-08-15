#!/bin/sh

set -eu

session=${GABIX_UART_SESSION:-gabix-pi2-uart}
baud=${GABIX_UART_BAUD:-115200}

usage() {
    cat >&2 <<EOF
usage: $0 start [serial-device] [log-directory]
       $0 status [serial-device]
       $0 stop

The default serial device is the only /dev/cu.usbserial-* device present.
Each start creates a unique log directory unless log-directory is supplied.
EOF
}

session_exists() {
    screen -ls 2>/dev/null | grep -Eq "\.${session}[[:space:]]"
}

resolve_device() {
    if [ -n "${1:-}" ]; then
        printf '%s\n' "$1"
        return
    fi

    set -- /dev/cu.usbserial-*
    if [ ! -e "$1" ]; then
        echo "no /dev/cu.usbserial-* device found" >&2
        exit 1
    fi
    if [ "$#" -ne 1 ]; then
        echo "multiple USB serial devices found; pass one explicitly:" >&2
        printf '  %s\n' "$@" >&2
        exit 1
    fi
    printf '%s\n' "$1"
}

device_owner() {
    lsof -t "$1" 2>/dev/null || true
}

start_monitor() {
    device=$(resolve_device "${1:-}")
    log_dir=${2:-}

    if [ ! -c "$device" ]; then
        echo "serial device is not a character device: $device" >&2
        exit 1
    fi
    if session_exists; then
        echo "UART monitor session already exists: $session" >&2
        echo "Use '$0 status $device' or '$0 stop' first." >&2
        exit 1
    fi

    owners=$(device_owner "$device")
    if [ -n "$owners" ]; then
        echo "serial device is already open by process(es): $owners" >&2
        echo "Close the existing UART monitor before starting another one." >&2
        exit 1
    fi

    if [ -z "$log_dir" ]; then
        temporary_root=${TMPDIR:-/tmp}
        log_dir=$(mktemp -d "$temporary_root/gabix-pi2-uart.XXXXXX")
    else
        mkdir -p "$log_dir"
        if [ -e "$log_dir/screenlog.0" ]; then
            echo "log already exists; choose a new directory: $log_dir/screenlog.0" >&2
            exit 1
        fi
    fi

    (
        cd "$log_dir"
        exec screen -L -dmS "$session" "$device" "$baud"
    )
    sleep 1

    if ! session_exists; then
        echo "screen failed to start" >&2
        exit 1
    fi

    printf 'session=%s\n' "$session"
    printf 'device=%s\n' "$device"
    printf 'baud=%s\n' "$baud"
    printf 'log=%s/screenlog.0\n' "$log_dir"
}

status_monitor() {
    device=$(resolve_device "${1:-}")
    if session_exists; then
        echo "session=$session (running)"
    else
        echo "session=$session (stopped)"
    fi
    echo "device=$device"
    owners=$(device_owner "$device")
    if [ -n "$owners" ]; then
        echo "owners=$owners"
    else
        echo "owners=none"
    fi
}

stop_monitor() {
    if session_exists; then
        screen -S "$session" -X quit
        attempts=0
        while session_exists && [ "$attempts" -lt 20 ]; do
            sleep 0.1
            attempts=$((attempts + 1))
        done
        if session_exists; then
            echo "screen session did not stop: $session" >&2
            exit 1
        fi
        echo "stopped=$session"
    else
        echo "stopped=$session (already stopped)"
    fi
}

command=${1:-}
case "$command" in
    start)
        command -v screen >/dev/null 2>&1 || {
            echo "screen is required" >&2
            exit 1
        }
        start_monitor "${2:-}" "${3:-}"
        ;;
    status)
        command -v lsof >/dev/null 2>&1 || {
            echo "lsof is required" >&2
            exit 1
        }
        status_monitor "${2:-}"
        ;;
    stop)
        command -v screen >/dev/null 2>&1 || {
            echo "screen is required" >&2
            exit 1
        }
        stop_monitor
        ;;
    -h|--help)
        usage
        exit 0
        ;;
    *)
        usage
        exit 2
        ;;
esac
