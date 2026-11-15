#!/bin/sh
# Pause only Gabix's direct-link DHCP daemon, reboot through authenticated
# management, and always resume the daemon. No interface configuration changes.
set -eu
python=${1:?usage: sudo sh scripts/pi2-dhcp-fault-check-macos.sh /path/to/python3}
[ "$(id -u)" -eq 0 ] || { echo 'Administrator privileges are required'; exit 1; }
[ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != root ] || { echo 'Run using sudo from your normal account'; exit 1; }
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
pid=$(cat /tmp/gabix-pi2-bootpd.pid)
case "$pid" in ''|*[!0-9]*) echo 'Invalid DHCP daemon PID'; exit 1;; esac
command=$(ps -p "$pid" -o command=)
case "$command" in "/usr/libexec/bootpd -d -D -i en6 -f $script_dir/pi2-dhcp-peer.plist"*) ;;
    *) echo 'The PID does not belong to this repository’s en6 DHCP peer'; exit 1;; esac
sudo -H -u "$SUDO_USER" "$python" "$script_dir/pi2-manage.py" status --address 10.0.2.15
resume() { kill -CONT "$pid" 2>/dev/null || true; }
trap resume EXIT HUP INT TERM
kill -STOP "$pid"
echo 'Gabix DHCP paused; rebooting the Pi over Ethernet'
sudo -H -u "$SUDO_USER" "$python" "$script_dir/pi2-manage.py" reboot --address 10.0.2.15
sleep 150
resume
trap - EXIT HUP INT TERM
echo 'Gabix DHCP resumed. The Pi should acquire its address on a retry.'
