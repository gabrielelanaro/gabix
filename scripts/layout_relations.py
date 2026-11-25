"""Layout rules that need evidence from more than one inventoried file."""
from collections import defaultdict
from hashlib import sha256
from pathlib import Path

RULES = {
    "copied-probe-implementation": "Share identical implementation source between a device driver and its standalone probe.",
}


def add_finding(record, rule, reason, destination, evidence):
    record["status"] = "finding"
    record["findings"].append(dict(
        rule=rule, reason=reason, destination=destination, evidence=evidence
    ))


def apply_relations(root: Path, files: list[dict]) -> None:
    """Attach one finding to each excess copy, keeping subtree counts additive."""
    implementations = defaultdict(list)
    for record in files:
        path = Path(record["path"])
        if (len(path.parts) < 4 or path.parts[0] != "targets"
                or path.parts[2] not in {"src", "probes"}
                or path.suffix.lower() not in {".s", ".rs", ".c"}
                or record["kind"] != "file"):
            continue
        local = root / path
        if not local.is_file() or local.is_symlink():
            continue
        data = local.read_bytes()
        if not data.strip():
            continue
        key = (path.parts[1], path.suffix.lower(), sha256(data).hexdigest())
        implementations[key].append(record)

    for (_, _, digest), group in implementations.items():
        runtime = sorted(r["path"] for r in group if Path(r["path"]).parts[2] == "src")
        if runtime:
            for record in group:
                if Path(record["path"]).parts[2] == "probes":
                    add_finding(record, "copied-probe-implementation",
                                "A standalone probe carries a byte-identical copy of runtime source.",
                                runtime[0], f"Identical to {', '.join(runtime)} (SHA-256 {digest}).")
