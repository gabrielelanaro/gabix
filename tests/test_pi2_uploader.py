import binascii
import importlib.util
import struct
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / "scripts" / "upload-pi2-kernel.py"
SPEC = importlib.util.spec_from_file_location("pi2_uploader", SCRIPT)
UPLOADER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(UPLOADER)


def test_upload_encodes_little_endian_header_and_crc():
    kernel = b"gabix"
    upload = UPLOADER.build_upload(kernel)
    assert upload[:9] == b"GABIXLOAD"
    assert struct.unpack("<III", upload[9:21]) == (
        1,
        len(kernel),
        binascii.crc32(kernel),
    )
    assert upload[21:] == kernel


def test_upload_rejects_oversized_kernel():
    try:
        UPLOADER.build_upload(b"x" * (UPLOADER.MAX_KERNEL_SIZE + 1))
    except ValueError:
        return
    raise AssertionError("oversized kernel was accepted")


def test_upload_rejects_zero_chunk_size():
    try:
        UPLOADER.validate_transfer_options(0, 3, 0)
    except ValueError:
        return
    raise AssertionError("zero chunk size was accepted")


def test_upload_rejects_negative_inter_byte_delay():
    try:
        UPLOADER.validate_transfer_options(256, 3, -1)
    except ValueError:
        return
    raise AssertionError("negative inter-byte delay was accepted")


def test_paced_upload_writes_one_byte_at_a_time(monkeypatch):
    writes = []
    sleeps = []

    monkeypatch.setattr(UPLOADER.select, "select", lambda *_args: ([], [7], []))
    monkeypatch.setattr(
        UPLOADER.os,
        "write",
        lambda _fd, data: writes.append(data) or len(data),
    )
    monkeypatch.setattr(UPLOADER.time, "monotonic", lambda: 1.0)
    monkeypatch.setattr(UPLOADER.time, "sleep", sleeps.append)
    monkeypatch.setattr(UPLOADER.termios, "tcdrain", lambda _fd: None)

    UPLOADER.write_all(7, b"abc", chunk_size=256, inter_byte_delay_us=500)

    assert writes == [b"a", b"b", b"c"]
    assert sleeps == [0.0005, 0.0005]


def test_wait_for_reboot_accepts_ready_after_upload(monkeypatch):
    monkeypatch.setattr(UPLOADER.time, "monotonic", lambda: 1.0)

    assert UPLOADER.wait_for_reboot(
        7, b"UPLOAD OK\r\nGABIX LOADER READY\r\n", timeout=1
    )


def test_result_wait_accepts_user_upload_markers(monkeypatch):
    reads = iter([b"USER UPLOAD OK\r\n"])
    monkeypatch.setattr(UPLOADER.select, "select", lambda *_args: ([7], [], []))
    monkeypatch.setattr(UPLOADER.os, "read", lambda *_args: next(reads))

    accepted, received = UPLOADER.wait_for_result(
        7,
        timeout=1,
        success_marker=b"USER UPLOAD OK",
        failure_marker=b"USER UPLOAD FAILED",
    )

    assert accepted
    assert received == b"USER UPLOAD OK\r\n"


def test_marker_wait_handles_marker_split_between_reads(monkeypatch):
    reads = iter([b"ER READY\r\n"])
    monkeypatch.setattr(UPLOADER.select, "select", lambda *_args: ([7], [], []))
    monkeypatch.setattr(UPLOADER.os, "read", lambda *_args: next(reads))

    assert UPLOADER.wait_for_marker_after(
        7,
        initial=b"USER UPLOAD OK\r\nGABIX US",
        after_marker=b"USER UPLOAD OK",
        marker=b"GABIX USER READY",
        timeout=1,
    )
