#!/usr/bin/env python3
"""Compare every Raspberry Pi boot file on a mounted card with a built image."""

import argparse
import importlib.util
import mmap
from pathlib import Path


FILES = {
    b"BOOTCODEBIN": "BOOTCODE.BIN",
    b"START   ELF": "START.ELF",
    b"FIXUP   DAT": "FIXUP.DAT",
    b"KERNEL7 IMG": "KERNEL7.IMG",
    b"CONFIG  TXT": "CONFIG.TXT",
}


def load_fat_helpers(repo_root: Path):
    helper_path = repo_root / "scripts" / "make-pi2-uart-smoke-from-official.py"
    spec = importlib.util.spec_from_file_location("pi2_fat", helper_path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def verify(image_path: Path, mount_path: Path) -> None:
    repo_root = Path(__file__).resolve().parents[1]
    fat = load_fat_helpers(repo_root)
    with image_path.open("rb") as stream, mmap.mmap(
        stream.fileno(), 0, access=mmap.ACCESS_READ
    ) as image:
        for short_name, filename in FILES.items():
            expected = fat.read_file(image, short_name)
            actual = (mount_path / filename).read_bytes()
            if actual != expected:
                raise ValueError(f"card file differs from image: {filename}")
            print(f"{filename}=PASS")
    print("loader_card_status=PASS")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("image", type=Path)
    parser.add_argument("mount", type=Path)
    args = parser.parse_args()
    verify(args.image, args.mount)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
