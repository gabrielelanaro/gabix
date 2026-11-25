"""Build the offline gabix architecture map from the current Git working tree."""
import json
from runpy import run_path
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TEMPLATE = ROOT / "scripts/templates/architecture-map.html"
OUTPUT = ROOT / "target/architecture-map.html"

# One count per reviewed design issue. All anchors must still match in order.
# Re-review the evidence when either implementation changes.
CHECKS = [
    ("Filesystem", "x86 truncates reads at sparse holes", "The x86 direct-block reader stops at a hole. The shared ARM reader zero-fills it and continues. Put direct-block reading in kernel-core.", "src/fs/ext2.rs",
     ["pub fn read_file_data(dev:", "if block_n == 0 {", "break;"]),
    ("Filesystem", "Mounted inode lookup always uses group zero", "The x86 mount passes its cached group-zero descriptor for every inode. The shared reader calculates the group from the inode number.", "src/fs/ext2.rs",
     ["self.with_dev(|dev| read_inode(dev, &self.sb, &self.bgd0, inode_num))"]),
    ("TCP", "Accepted sends bypass flow control and retry tracking", "The x86 server advances its send sequence for each outgoing chunk. The Pi sender retains pending data and limits writes to the advertised window.", "src/net/tcp.rs",
     ["pub fn send(&self, payload:", "for chunk in payload.chunks(SEND_MSS)", "connection.send_next = connection.send_next.wrapping_add(chunk.len() as u32)"]),
    ("Descriptors", "Fixed descriptor numbers for Pi sockets", "The Pi listener and stream have global descriptor numbers. x86 resolves socket handles through the calling process’s table.", "targets/armv7/src/user_upload/tcp.rs",
     ["const LISTENER: u32 = 3;", "const STREAM: u32 = 4;"]),
    ("Syscalls", "Socket availability depends on the Wi-Fi uploader", "The Pi socket syscall handler checks the uploader’s running flag. Process state should determine access to descriptors.", "targets/armv7/src/user_upload/tcp.rs",
     ["pub fn syscall(", "if !super::wifi::running()"]),
    ("Networking", "A socket wait drives board management", "Waiting on a socket constructs a UART console and calls the board network loop. This couples socket blocking to board servicing.", "targets/armv7/src/user_upload/tcp.rs",
     ["fn wait_for(", "crate::console::Pl011::new(", "crate::poll_ethernet_and_reply("]),
]


def findings(path, lines):
    result = []
    for category, title, reason, owner, anchors in CHECKS:
        if path != owner:
            continue
        evidence = []
        start = 0
        for anchor in anchors:
            index = next((i for i in range(start, len(lines)) if anchor in lines[i]), None)
            if index is None:
                break
            evidence.append([index + 1, lines[index].strip()])
            start = index + 1
        else:
            result.append(dict(category=category, title=title, reason=reason, evidence=evidence))
    return result


def build_map(root, audit):
    """Use the audit's inventory and findings as the only source of layout data."""
    directories = {}
    for record in audit["directories"]:
        path = Path(record["path"])
        directories[path] = {
            **record,
            "name": path.name or root.name,
            "children": {},
            "behavior_count": 0,
        }
    for path, directory in directories.items():
        if path != Path("."):
            directories[path.parent]["children"][path.name] = directory

    behavior_paths = {check[3] for check in CHECKS}
    for record in audit["files"]:
        path = Path(record["path"])
        if path in directories and record["kind"] == "repository":
            # A submodule Git entry also has a directory row. Preserve that boundary in the index.
            directories[path]["kind"] = "repository"
            continue
        issues = []
        if record["kind"] == "file" and record["path"] in behavior_paths:
            issues = findings(record["path"], (root / path).read_text(errors="replace").splitlines())
        directories[path.parent]["children"][path.name] = {
            **record,
            "name": path.name,
            "behavior": issues,
            "behavior_count": len(issues),
        }
        if issues:
            for parent in path.parents:
                directories[parent]["behavior_count"] += len(issues)

    return {"root": directories[Path(".")], "rules": audit["rules"]}


def main():
    build_audit = run_path(str(ROOT / "scripts/audit-layout.py"))["build_audit"]
    audit = build_audit(ROOT)
    data = build_map(ROOT, audit)
    # Escaping '<' prevents a file name or evidence line from ending the JSON script.
    payload = json.dumps(data, ensure_ascii=True, separators=(",", ":")).replace("<", "\\u003c")
    OUTPUT.parent.mkdir(exist_ok=True)
    OUTPUT.write_text(TEMPLATE.read_text().replace("<!-- AUDIT_JSON -->", payload))
    root = data["root"]
    print(f"{OUTPUT}: {len(audit['directories']):,} directories, {root['files']:,} files, "
          f"{root['findings']:,} layout findings, {root['behavior_count']} behavioral observations")


if __name__ == "__main__":
    main()
