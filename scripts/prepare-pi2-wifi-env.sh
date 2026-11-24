# Sourced by the ARM build scripts after they set repo_root.
# User credentials and all binaries containing them stay in the private build.
: "${GABIX_WIFI_FIRMWARE:?Set GABIX_WIFI_FIRMWARE to the RTL8188C TSMC firmware file}"
if [ ! -f "$GABIX_WIFI_FIRMWARE" ]; then
    echo "Wi-Fi firmware file is missing" >&2
    exit 1
fi
umask 077
private_build="$repo_root/target/private-wifi"
mkdir -p "$private_build"
chmod 700 "$private_build"
export CARGO_TARGET_DIR="$private_build/build"
export GABIX_WIFI_PMK_FILE="$private_build/pmk.bin"
GABIX_WIFI_SSID=$(python3 - <<'PY'
import json, os, sys
from pathlib import Path
config = json.loads((Path.home() / '.config/gabix/pi2-wifi.json').read_text())
ssid = config['ssid']
key = bytes.fromhex(config['pmk_hex'])
assert 1 <= len(ssid.encode()) <= 32 and '\n' not in ssid
assert len(key) == 32
assert os.environ.get('GABIX_WIFI_SSID', ssid) == ssid
fd = os.open(os.environ['GABIX_WIFI_PMK_FILE'], os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
with os.fdopen(fd, 'wb') as output:
    output.write(key)
os.chmod(os.environ['GABIX_WIFI_PMK_FILE'], 0o600)
sys.stdout.write(ssid)
PY
)
export GABIX_WIFI_SSID GABIX_WIFI_FIRMWARE
