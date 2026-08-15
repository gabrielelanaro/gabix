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
        UPLOADER.validate_transfer_options(0, 3)
    except ValueError:
        return
    raise AssertionError("zero chunk size was accepted")
