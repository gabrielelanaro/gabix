import binascii
import os
import pty
import select
import struct
import subprocess
import sys
import time
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / "scripts" / "upload-pi2-user.py"


def test_user_uploader_sends_elf_and_waits_for_exit(tmp_path):
    user_elf = tmp_path / "init.elf"
    user_elf.write_bytes(b"gabix-user")
    master, slave = pty.openpty()
    uart = os.ttyname(slave)
    os.close(slave)
    process = subprocess.Popen(
        [
            sys.executable,
            str(SCRIPT),
            uart,
            str(user_elf),
            "--inter-byte-delay-us",
            "0",
            "--retries",
            "1",
            "--wait-for-exit",
            "--exit-timeout",
            "1",
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    try:
        expected = (
            b"GABIXLOAD"
            + struct.pack("<III", 1, 10, binascii.crc32(b"gabix-user"))
            + b"gabix-user"
        )
        received = bytearray()
        deadline = time.monotonic() + 2
        while len(received) < len(expected) and time.monotonic() < deadline:
            readable, _, _ = select.select([master], [], [], 0.2)
            if readable:
                received.extend(os.read(master, 4096))
        assert bytes(received) == expected

        os.write(
            master,
            b"USER UPLOAD OK\r\ngabix arm userspace wrote\r\nGABIX USER READY\r\n",
        )
        stdout, stderr = process.communicate(timeout=2)
        assert process.returncode == 0, stderr.decode(errors="replace")
        assert b"gabix arm userspace wrote" in stdout
        assert b"GABIX USER READY" in stdout
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)


def test_user_uploader_waits_for_loader_after_reboot(tmp_path):
    user_elf = tmp_path / "reboot.elf"
    user_elf.write_bytes(b"gabix-reboot")
    master, slave = pty.openpty()
    uart = os.ttyname(slave)
    os.close(slave)
    process = subprocess.Popen(
        [
            sys.executable,
            str(SCRIPT),
            uart,
            str(user_elf),
            "--inter-byte-delay-us",
            "0",
            "--retries",
            "1",
            "--wait-for-reboot",
            "--reboot-timeout",
            "1",
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    try:
        expected = (
            b"GABIXLOAD"
            + struct.pack("<III", 1, 12, binascii.crc32(b"gabix-reboot"))
            + b"gabix-reboot"
        )
        received = bytearray()
        deadline = time.monotonic() + 2
        while len(received) < len(expected) and time.monotonic() < deadline:
            readable, _, _ = select.select([master], [], [], 0.2)
            if readable:
                received.extend(os.read(master, 4096))
        assert bytes(received) == expected

        os.write(
            master,
            b"USER UPLOAD OK\r\ngabix arm userspace rebooting\r\n"
            b"GABIX LOADER READY\r\n",
        )
        stdout, stderr = process.communicate(timeout=2)
        assert process.returncode == 0, stderr.decode(errors="replace")
        assert b"gabix arm userspace rebooting" in stdout
        assert b"GABIX LOADER READY" in stdout
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
