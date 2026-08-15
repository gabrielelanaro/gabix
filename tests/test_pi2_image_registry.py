import hashlib
import importlib.util
import json
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / "scripts" / "pi2-image-registry.py"
SPEC = importlib.util.spec_from_file_location("pi2_image_registry", SCRIPT)
REGISTRY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(REGISTRY)


def manifest_for(contents: bytes, **overrides):
    image = {
        "id": "working",
        "filename": "working.img",
        "size": len(contents),
        "sha256": hashlib.sha256(contents).hexdigest(),
        "status": "hardware-passed",
        "protected": True,
        "source": "test",
        "evidence": "observed expected UART output",
    }
    image.update(overrides)
    return {"schema_version": 1, "images": [image]}


def test_manifest_rejects_unprotected_hardware_pass(tmp_path):
    path = tmp_path / "manifest.json"
    path.write_text(json.dumps(manifest_for(b"image", protected=False)))
    try:
        REGISTRY.load_manifest(path)
    except ValueError as error:
        assert "not protected" in str(error)
        return
    raise AssertionError("unprotected hardware-passed image was accepted")


def test_verify_rejects_changed_image(tmp_path):
    path = tmp_path / "working.img"
    path.write_bytes(b"changed")
    image = manifest_for(b"original")["images"][0]
    try:
        REGISTRY.verify_image(image, path, require_protection=False)
    except ValueError as error:
        assert "mismatch" in str(error)
        return
    raise AssertionError("changed image was accepted")


def test_preserve_refuses_failed_image(tmp_path):
    source = tmp_path / "failed.img"
    source.write_bytes(b"failed")
    image = manifest_for(
        b"failed", status="hardware-failed", protected=False
    )["images"][0]
    try:
        REGISTRY.preserve_image(image, source, tmp_path / "store")
    except ValueError as error:
        assert "hardware-passed" in str(error)
        return
    raise AssertionError("failed image entered the protected store")


def test_record_failed_result_updates_candidate(tmp_path):
    manifest = manifest_for(
        b"candidate", status="candidate", protected=False, evidence="untested"
    )
    manifest_path = tmp_path / "manifest.json"
    manifest_path.write_text(json.dumps(manifest))
    REGISTRY.record_result(
        manifest,
        manifest_path,
        "working",
        "hardware-failed",
        "four ACT flashes",
        None,
        tmp_path / "store",
    )
    recorded = json.loads(manifest_path.read_text())["images"][0]
    assert recorded["status"] == "hardware-failed"
    assert recorded["evidence"] == "four ACT flashes"


def test_record_passed_result_requires_image(tmp_path):
    manifest = manifest_for(
        b"candidate", status="candidate", protected=False, evidence="untested"
    )
    try:
        REGISTRY.record_result(
            manifest,
            tmp_path / "manifest.json",
            "working",
            "hardware-passed",
            "expected UART output",
            None,
            tmp_path / "store",
        )
    except ValueError as error:
        assert "requires --image" in str(error)
        return
    raise AssertionError("hardware pass without an image was accepted")
