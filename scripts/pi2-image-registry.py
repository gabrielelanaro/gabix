#!/usr/bin/env python3
"""Record, preserve, and verify Raspberry Pi 2 disk-image artifacts."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import sys
import time


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = REPO_ROOT / "artifacts" / "pi2-images.json"
DEFAULT_STORE = Path.home() / "Documents" / "Gabix-Images"
VALID_STATUSES = {"candidate", "hardware-passed", "hardware-failed"}
HASH_CHUNK = 16 * 1024 * 1024
PROGRESS_INTERVAL = 512 * 1024 * 1024


def load_manifest(path: Path) -> dict:
    data = json.loads(path.read_text())
    if data.get("schema_version") != 1 or not isinstance(data.get("images"), list):
        raise ValueError(f"unsupported image manifest: {path}")
    seen = set()
    for image in data["images"]:
        image_id = image.get("id")
        if not image_id or image_id in seen:
            raise ValueError(f"missing or duplicate image id: {image_id!r}")
        seen.add(image_id)
        if image.get("status") not in VALID_STATUSES:
            raise ValueError(f"invalid status for {image_id}: {image.get('status')!r}")
        digest = image.get("sha256", "")
        if len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest):
            raise ValueError(f"invalid SHA-256 for {image_id}")
        if not isinstance(image.get("size"), int) or image["size"] <= 0:
            raise ValueError(f"invalid size for {image_id}")
        if image.get("status") == "hardware-passed" and not image.get("protected"):
            raise ValueError(f"hardware-passed image is not protected: {image_id}")
    return data


def find_image(manifest: dict, image_id: str) -> dict:
    for image in manifest["images"]:
        if image["id"] == image_id:
            return image
    raise ValueError(f"unknown image id: {image_id}")


def progress(label: str, completed: int, total: int, started: float) -> None:
    elapsed = max(time.monotonic() - started, 0.001)
    rate = completed / elapsed
    eta = (total - completed) / rate if rate else 0
    print(
        f"{label} bytes={completed}/{total} "
        f"percent={100 * completed / total:.1f} "
        f"rate_mib_s={rate / (1024 * 1024):.1f} eta_s={eta:.1f}",
        flush=True,
    )


def hash_image(path: Path, label: str = "hash") -> tuple[int, str]:
    total = path.stat().st_size
    completed = 0
    report_at = 0
    started = time.monotonic()
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(HASH_CHUNK):
            digest.update(chunk)
            completed += len(chunk)
            if completed >= report_at or completed == total:
                progress(label, completed, total, started)
                report_at = completed + PROGRESS_INTERVAL
    return total, digest.hexdigest()


def verify_image(image: dict, path: Path, require_protection: bool = True) -> None:
    if not path.is_file():
        raise ValueError(f"artifact is missing: {path}")
    size, digest = hash_image(path, f"verify:{image['id']}")
    if size != image["size"]:
        raise ValueError(f"size mismatch: expected {image['size']}, found {size}")
    if digest != image["sha256"]:
        raise ValueError(
            f"SHA-256 mismatch: expected {image['sha256']}, found {digest}"
        )
    if require_protection and image.get("protected"):
        mode = stat.S_IMODE(path.stat().st_mode)
        if mode & 0o222:
            raise ValueError(f"protected artifact is writable: {path}")
        immutable_flag = getattr(stat, "UF_IMMUTABLE", 0)
        if immutable_flag and not path.stat().st_flags & immutable_flag:
            raise ValueError(f"protected artifact lacks the user-immutable flag: {path}")
    print(f"artifact_status=PASS id={image['id']} path={path}")


def protect_path(path: Path) -> None:
    # Managed kernels embed a private LAN authentication key.
    path.chmod(0o400)
    immutable_flag = getattr(stat, "UF_IMMUTABLE", 0)
    if immutable_flag:
        os.chflags(path, path.stat().st_flags | immutable_flag)


def preserve_image(image: dict, source: Path, store: Path) -> Path:
    if image["status"] != "hardware-passed" or not image.get("protected"):
        raise ValueError("only protected hardware-passed images may enter the store")
    verify_image(image, source, require_protection=False)
    store.mkdir(parents=True, exist_ok=True)
    destination = store / image["filename"]
    if destination.exists():
        verify_image(image, destination)
        return destination

    partial = destination.with_name(destination.name + ".partial")
    if partial.exists():
        partial.unlink()
    total = source.stat().st_size
    completed = 0
    report_at = 0
    started = time.monotonic()
    try:
        with source.open("rb") as input_stream, partial.open("xb") as output_stream:
            while chunk := input_stream.read(HASH_CHUNK):
                output_stream.write(chunk)
                completed += len(chunk)
                if completed >= report_at or completed == total:
                    progress("preserve", completed, total, started)
                    report_at = completed + PROGRESS_INTERVAL
            output_stream.flush()
            os.fsync(output_stream.fileno())
        os.replace(partial, destination)
        protect_path(destination)
    finally:
        if partial.exists():
            partial.unlink()
    verify_image(image, destination)
    return destination


def save_manifest(path: Path, manifest: dict) -> None:
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(json.dumps(manifest, indent=2) + "\n")
    os.replace(temporary, path)


def record_result(
    manifest: dict,
    manifest_path: Path,
    image_id: str,
    status_value: str,
    evidence: str,
    source: Path | None,
    store: Path,
) -> None:
    image = find_image(manifest, image_id)
    if image["status"] != "candidate":
        raise ValueError(
            f"result is already final for {image_id}: {image['status']}; "
            "use a new id for different bytes"
        )
    if status_value == "hardware-passed":
        if source is None:
            raise ValueError("a hardware-passed result requires --image")
        image["status"] = status_value
        image["protected"] = True
        image["evidence"] = evidence
        preserve_image(image, source, store)
    else:
        image["status"] = status_value
        image["protected"] = False
        image["evidence"] = evidence
    save_manifest(manifest_path, manifest)
    print(f"recorded_status={status_value} id={image_id}")


def command_list(manifest: dict) -> None:
    for image in manifest["images"]:
        print(
            f"{image['id']}\t{image['status']}\t{image['size']}\t"
            f"{image['sha256']}\t{image['filename']}"
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--store", type=Path, default=DEFAULT_STORE)
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("list")
    verify_parser = subparsers.add_parser("verify")
    verify_parser.add_argument("image_id")
    path_parser = subparsers.add_parser("path")
    path_parser.add_argument("image_id")
    preserve_parser = subparsers.add_parser("preserve")
    preserve_parser.add_argument("image_id")
    preserve_parser.add_argument("source", type=Path)
    result_parser = subparsers.add_parser("record-result")
    result_parser.add_argument("image_id")
    result_parser.add_argument(
        "--status", required=True, choices=("hardware-passed", "hardware-failed")
    )
    result_parser.add_argument("--evidence", required=True)
    result_parser.add_argument("--image", type=Path)
    args = parser.parse_args()

    try:
        manifest = load_manifest(args.manifest)
        if args.command == "list":
            command_list(manifest)
            return 0
        image = find_image(manifest, args.image_id)
        destination = args.store / image["filename"]
        if args.command == "path":
            verify_image(image, destination)
            print(destination)
        elif args.command == "verify":
            verify_image(image, destination)
        elif args.command == "preserve":
            print(preserve_image(image, args.source, args.store))
        elif args.command == "record-result":
            record_result(
                manifest,
                args.manifest,
                args.image_id,
                args.status,
                args.evidence,
                args.image,
                args.store,
            )
        return 0
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"pi2-image-registry: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
