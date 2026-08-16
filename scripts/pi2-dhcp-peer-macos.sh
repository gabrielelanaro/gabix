#!/bin/sh

set -eu

action=${1:-status}
interface=${2:-en6}
server_address=10.0.2.2
pid_file=/tmp/gabix-pi2-bootpd.pid
log_file=/tmp/gabix-pi2-bootpd.log
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
config="$script_dir/pi2-dhcp-peer.plist"

require_root() {
    if [ "$(id -u)" -ne 0 ]; then
        echo "run with sudo: sudo $0 $action $interface" >&2
        exit 1
    fi
}

running_pid() {
    [ -f "$pid_file" ] || return 1
    peer_pid=$(cat "$pid_file")
    ps -p "$peer_pid" -o command= | grep -Fq "/usr/libexec/bootpd -d -D -i $interface -f $config" || return 1
    printf '%s\n' "$peer_pid"
}

case "$action" in
    start)
        require_root
        if peer_pid=$(running_pid); then
            echo "DHCP peer already running: pid=$peer_pid interface=$interface log=$log_file"
            exit 0
        fi
        rm -f "$pid_file"
        ifconfig "$interface" inet "$server_address" netmask 255.255.255.0 alias
        /usr/libexec/bootpd -d -D -i "$interface" -f "$config" >"$log_file" 2>&1 &
        peer_pid=$!
        printf '%s\n' "$peer_pid" >"$pid_file"
        echo "DHCP peer started: pid=$peer_pid interface=$interface server=$server_address lease=10.0.2.15 log=$log_file"
        ;;
    stop)
        require_root
        if peer_pid=$(running_pid); then
            kill "$peer_pid"
            wait "$peer_pid" 2>/dev/null || true
        fi
        rm -f "$pid_file"
        ifconfig "$interface" -alias "$server_address" 2>/dev/null || true
        echo "DHCP peer stopped: interface=$interface"
        ;;
    status)
        if peer_pid=$(running_pid); then
            echo "DHCP peer running: pid=$peer_pid interface=$interface log=$log_file"
        else
            echo "DHCP peer stopped: interface=$interface log=$log_file"
        fi
        ifconfig "$interface" 2>/dev/null | awk '/inet 10\.0\.2\.2 |status:|media:/{print}'
        ;;
    *)
        echo "usage: $0 <start|stop|status> [interface]" >&2
        exit 2
        ;;
esac
