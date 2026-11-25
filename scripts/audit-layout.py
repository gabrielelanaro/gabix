#!/usr/bin/env python3
"""Inventory project paths and evaluate placement rules without modifying the tree."""
import argparse
from collections import Counter
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys

# The CLI can also be loaded with runpy from a tool outside the scripts directory.
sys.path.insert(0, str(Path(__file__).resolve().parent))
import layout_kernel
import layout_relations

ROOT = Path(__file__).resolve().parents[1]
STATUSES = ("finding", "review", "covered", "external")
CODE = set(".rs .s .ld .py .sh .js .cjs .mjs .ts .java .css .html .tla .c .h .toml .json .yml .yaml .cfg .plist .dot .mmd .svg".split())
RULES = {
    "package-test-owner": "A host Rust test that directly includes source from one package belongs in that package's tests.",
    "pi-probe-command": "Pi-specific probe build commands that invoke Cargo and objcopy belong together in scripts/pi2/probes/.",
    "userspace-linker-owner": "A linker script required by a general ARM userspace build belongs with that target.",
    "generated-output": "Project build products and raw captures belong under a build directory, or in a deliberate test fixture.",
}
ROOT_DOCS = {"README.md", "AGENTS.md", "CLAUDE.md", "CONTEXT.md", "GLOSSARY.md", "LEARNING.md", "MISSION.md", "NOTES.md", "RESOURCES.md"}


def placement(owner, destination, status="covered", findings=None):
    return dict(owner=owner, destination=destination, status=status, findings=findings or [])


def violation(owner, destination, rule, reason, evidence, line=None):
    issue = dict(rule=rule, reason=reason, destination=destination, evidence=evidence)
    if line is not None:
        issue["line"] = line
    return placement(owner, destination, "finding", [issue])


def nonkernel(path, source):
    p = PurePosixPath(path)
    top = p.parts[0]
    ext = p.suffix.lower()
    if top == ".cursor" and len(p.parts) > 1 and p.parts[1] in {"skills", "agents", "PSTACK-LICENSE"}:
        return placement("Installed agent tooling", path, "external")
    if top == ".agents" and len(p.parts) > 2 and p.parts[1] == "skills":
        return placement("Project agent skills", path)
    if path in ROOT_DOCS:
        return placement("Project documentation", path)
    if path in {".gitignore", "Cargo.lock", "rust-toolchain.toml", "skills-lock.json"}:
        return placement("Repository configuration", path)
    if top == "tests" and ext == ".rs":
        links = list(re.finditer(r'(?m)^\s*#\[path\s*=\s*"\.\./((?:crates|targets)/[^/"]+)/src/[^"]+"\s*\]', source))
        owners = {m.group(1) for m in links}
        if len(owners) == 1 and len(p.parts) == 2:
            target = f"{owners.pop()}/tests/{p.name}"
            return violation("Package integration tests", target, "package-test-owner",
                             "Root test compiles another package’s source file directly.",
                             links[0].group().strip(), source.count("\n", 0, links[0].start()) + 1)
    if top == "tests":
        if len(p.parts) >= 3 and p.parts[1] in {"fixtures", "support"}:
            return placement("Test fixtures" if p.parts[1] == "fixtures" else "Test support", path)
        if len(p.parts) == 2 and (p.name.startswith("test_") or p.name in {"common.sh", "run.sh"}):
            return placement("Repository behavioral tests", path)
    if top == "scripts":
        if len(p.parts) == 2 and re.fullmatch(r"build-pi2-.+-probe\.sh", p.name):
            anchor = re.search(r"(?m)^.*--manifest-path.*targets/armv7/Cargo\.toml.*$", source)
            if anchor and "cargo build" in source and "llvm-objcopy" in source:
                return violation("Pi probe build tooling", f"scripts/pi2/probes/{p.name}", "pi-probe-command",
                                 "A board-specific probe build entry sits among general project commands.",
                                 anchor.group().strip(), source.count("\n", 0, anchor.start()) + 1)
        if ext in {".py", ".sh", ".plist"} or path == "scripts/templates/architecture-map.html":
            return placement("Host build and development tools", path)
    if top == "artifacts" and (ext == ".md" or path == "artifacts/pi2-images.json"):
        return placement("Hardware evidence and image registry", path)
    if top == "docs" and ext in {".html", ".md"}:
        return placement("Architecture decisions" if p.parts[1:2] == ("adr",) else "Engineering documentation", path)
    if top == "learning-records" and ext == ".md":
        return placement("Learning records", path)
    if top in {"lessons", "reference", "tours"} and ext in {".html", ".md", ".png", ".svg", ".css", ".js"}:
        return placement({"lessons": "Course lessons", "reference": "Reference pages", "tours": "Code tours"}[top], path)
    if top == "assets" and ext in {".css", ".js", ".png", ".svg", ".jpg", ".webp"}:
        return placement("Course assets", path)
    return placement("Unclassified", None, "review")


def output_placement(path):
    p = PurePosixPath(path)
    ext = p.suffix.lower()
    if ext not in {".img", ".elf", ".pcap", ".pcapng", ".log"}:
        return None
    if p.parts[0] == ".cursor" or any(pair == ("tests", "fixtures") for pair in zip(p.parts, p.parts[1:])):
        return None
    if "target" in p.parts[:-1]:
        return placement("Build output", path)
    where = "target/runs/<date>-<purpose>/" if ext in {".log", ".pcap", ".pcapng"} else "target/"
    return violation("Generated output", where + p.name, "generated-output",
                     "Disposable working output is present in the source inventory.",
                     f"{ext} file outside a fixture or build directory; see artifacts/README.md")


def git_paths(root, *args, input_bytes=None, allowed=(0,)):
    result = subprocess.run(["git", *args], cwd=root, input=input_bytes, stdout=subprocess.PIPE)
    if result.returncode not in allowed:
        raise RuntimeError(f"git {args[0]} exited with status {result.returncode}")
    return {os.fsdecode(p) for p in result.stdout.split(b"\0") if p}


def build_linker_inputs(file_paths, sources):
    """Resolve linker argument references in project commands, attaching findings to their inputs."""
    references = {}
    for path, source in sources.items():
        if not source or "--target armv7a-none-eabihf" not in source:
            continue
        for n, line in enumerate(source.splitlines(), 1):
            if "link-arg=-T" not in line:
                continue
            for target in re.findall(r"tests/fixtures/[A-Za-z0-9_.-]+\.ld\b", line):
                if target in file_paths:
                    references.setdefault(target, []).append(f"{path}:{n} {line.strip()}")
    return references


def inventory_dirs(root, files):
    """Keep empty first-party directories too. Prune Git metadata, ignored and nested repositories."""
    parents = {str(parent) for path in files for parent in PurePosixPath(path).parents}
    directories = {"."}
    boundary = set()
    pending = ["."]
    while pending:
        candidates = []
        for rel in pending:
            for child in (root / rel).iterdir():
                if child.name == ".git" or child.is_symlink() or not child.is_dir():
                    continue
                candidates.append(child.relative_to(root).as_posix())
        pending = []
        if not candidates:
            break
        data = b"\0".join(os.fsencode(p) for p in candidates) + b"\0"
        ignored = git_paths(root, "check-ignore", "--no-index", "-z", "--stdin", input_bytes=data, allowed=(0, 1))
        for path in candidates:
            if path in ignored and path not in parents:
                continue
            directories.add(path)
            if (root / path / ".git").exists() or path in files:
                boundary.add(path)
                continue
            pending.append(path)
    return directories, boundary


def summarize(files, directory_paths, boundaries):
    dirs = {}
    for path in directory_paths:
        dirs[path] = dict(path=path, direct_files=0, files=0, direct_loc=0, loc=0, direct_findings=0,
                          findings=0, status_counts=dict.fromkeys(STATUSES, 0), owners=Counter(), destinations=set())
    for file in files:
        p = PurePosixPath(file["path"])
        for depth, parent in enumerate(p.parents):
            d = dirs[str(parent)]
            count = len(file["findings"])
            d["files"] += 1
            d["loc"] += file["loc"]
            d["findings"] += count
            d["status_counts"][file["status"]] += 1
            d["owners"][file["owner"]] += 1
            destination = file["destination"]
            projected = None
            if destination:
                target = PurePosixPath(destination)
                tail = p.parts[-(depth + 1):]
                if target.parts[-len(tail):] == tail:
                    projected = str(target.parents[depth])
            d["destinations"].add(projected)
            if depth == 0:
                d["direct_files"] += 1
                d["direct_loc"] += file["loc"]
                d["direct_findings"] += count
    for d in dirs.values():
        totals = d["status_counts"]
        if d["path"] in boundaries:
            d["status"] = "external"
        elif d["findings"]:
            d["status"] = "finding"
        elif totals["review"] or not d["files"]:
            d["status"] = "review"
        elif totals["external"] == d["files"]:
            d["status"] = "external"
        else:
            d["status"] = "covered"
        owners = d.pop("owners")
        d["owner"] = next(iter(owners)) if len(owners) == 1 else "Multiple owners" if owners else "Unclassified"
        if d["path"] in boundaries:
            d["owner"] = "Nested repository"
        targets = d.pop("destinations")
        d["destination"] = next(iter(targets)) if len(targets) == 1 else None
    return [dirs[path] for path in sorted(dirs)]


def build_audit(root=ROOT):
    root = Path(root).resolve()
    tracked = git_paths(root, "ls-files", "--cached", "-z")
    others = git_paths(root, "ls-files", "--others", "--exclude-standard", "-z")
    paths = sorted({p.rstrip("/") for p in tracked | others})
    directory_paths, boundaries = inventory_dirs(root, set(paths))
    # A tracked parent can be removed or replaced by a symlink. Only parents
    # reached by the directory walk are safe to inspect in this working tree.
    inaccessible = {path for path in paths if str(PurePosixPath(path).parent) not in directory_paths
                    or str(PurePosixPath(path).parent) in boundaries}
    # Missing tracked files have a path in the Git index even when all their parents were removed.
    for path in paths:
        directory_paths.update(str(p) for p in PurePosixPath(path).parents)
    layout_kernel.prepare(root, [path for path in paths if path not in inaccessible])
    files = []
    build_commands = {}
    for path in paths:
        p = root / path
        source = ""
        if path in inaccessible:
            kind = "missing"
        elif p.is_symlink():
            kind = "symlink"
        elif path in boundaries:
            kind = "repository"
        elif not p.is_file():
            kind = "missing" if not p.exists() else "special"
        else:
            kind = "file"
            with p.open("rb") as stream:
                prefix = stream.read(8192)
                if b"\0" not in prefix:
                    source = (prefix + stream.read()).decode("utf-8", errors="replace")
                else:
                    kind = "binary"
        loc = sum(bool(line.strip()) for line in source.splitlines()) if p.suffix.lower() in CODE or source.startswith("#!") else 0
        if path.startswith("scripts/") and p.suffix == ".sh":
            build_commands[path] = source
        policy = output_placement(path) or layout_kernel.classify(path, source) or nonkernel(path, source)
        policy.setdefault("findings", [])
        policy.setdefault("status", "finding" if policy["findings"] else "covered")
        if kind in {"missing", "special"}:
            policy = placement("Unavailable working-tree entry", None, "review")
        if kind == "repository":
            policy = placement("Nested repository", path, "external")
        files.append(dict(path=path, kind=kind, tracked=path in tracked, loc=loc, **policy))
    linker_inputs = build_linker_inputs(set(paths), build_commands)
    for file in files:
        if file["path"] in linker_inputs and file["kind"] == "file":
            name = PurePosixPath(file["path"]).name.removeprefix("armv7-")
            file.update(violation("ARM userspace ABI", f"targets/armv7/{name}", "userspace-linker-owner",
                                  "ARM userspace build commands load the linker layout from test fixtures.",
                                  "\n".join(linker_inputs[file["path"]])))
    layout_relations.apply_relations(root, [f for f in files if f["kind"] == "file"])
    directories = summarize(files, directory_paths, boundaries)
    return dict(schema_version=1, rules={**RULES, **layout_kernel.RULES, **layout_relations.RULES},
                files=files, directories=directories)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--output", type=Path, help="Write JSON here (default: stdout).")
    args = parser.parse_args()
    payload = json.dumps(build_audit(args.root), indent=2, ensure_ascii=True) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(payload)
    else:
        sys.stdout.write(payload)


if __name__ == "__main__":
    main()
