"""Placement policy for gabix kernel packages. All outputs describe proposed ownership."""

import json
import posixpath
import re
import tomllib
from pathlib import Path, PurePosixPath

ARM = "targets/armv7/"
X86 = "targets/x86_64/"
CORE = "crates/kernel-core/"
RULES = {
    "kernel.root-package": "Place target kernel packages under targets/<architecture>/.",
    "kernel.cargo-scope": "Scope target-specific runner and linker settings to their target.",
    "kernel.target-spec": "Keep an architecture's custom target specification with its package.",
    "kernel.probe-bin": "Keep diagnostic executables and their private assembly in the target's probes directory.",
    "kernel.bin-under-tests": "Keep separately built board images outside Cargo's integration-test directory.",
    "kernel.forwarder": "Import a shared module directly when a target file only re-exports it.",
    "kernel.driver-format": "Keep device-specific USB framing with the driver, outside the network protocol namespace.",
    "kernel.upload-socket": "Keep userspace socket syscall dispatch outside the program uploader.",
    "kernel.boot-service": "Keep recurring board network service and its state outside the kernel entry file.",
    "kernel.core-hardware": "Keep inline assembly in target adapters.",
    "kernel.core-target-dependency": "Shared kernel runtime and build dependencies must not import a target package.",
}

_probe_sources = {}
_probe_assembly = {}
_board_fixtures = {}
_root_x86_package = False
_shared_modules = set()
_driver_consumers = {}
_manifest_errors = {}


def prepare(root: Path, paths: list[str]):
    """Read manifest ownership and literal include edges from this checkout."""
    global _root_x86_package
    _probe_sources.clear()
    _probe_assembly.clear()
    _board_fixtures.clear()
    _shared_modules.clear()
    _driver_consumers.clear()
    _manifest_errors.clear()
    files = set(paths)

    def read(path):
        file = root / path
        if (path not in files or file.is_symlink() or not file.is_file()
                or not file.resolve().is_relative_to(root.resolve())):
            return ""
        return file.read_text(errors="replace")

    def parse_manifest(path):
        try:
            return tomllib.loads(read(path))
        except tomllib.TOMLDecodeError as error:
            _manifest_errors[path] = str(error)
            return {}

    manifest = parse_manifest("Cargo.toml")
    _root_x86_package = "package" in manifest and "x86_64" in manifest.get("dependencies", {})
    _shared_modules.update(path for path in paths if path.startswith(CORE + "src/"))
    arm_manifest = ARM + "Cargo.toml"
    manifest_text = read(arm_manifest)
    for entry in parse_manifest(arm_manifest).get("bin", []):
        if not isinstance(entry, dict) or "name" not in entry:
            continue
        name = entry["name"]
        path = entry.get("path", "")
        is_fixture = path.startswith("tests/") and entry.get("test") is False
        is_probe = name.endswith("-probe") and path.startswith("src/")
        if not (is_fixture or is_probe):
            continue
        registered = ARM + path
        line = next((n for n, value in enumerate(manifest_text.splitlines(), 1)
                     if value.strip() == f'path = "{path}"'), None)
        evidence = f'{arm_manifest}:{line or 1} registers {name} at {path}'
        if is_probe:
            _probe_sources[registered] = evidence
        else:
            boot_trial = 'env!("GABIX_TRIAL_LOADER")' in read(registered)
            dest = ARM + ("fixtures/boot/" if boot_trial else "probes/") + PurePosixPath(path).name
            consumer = next((p for p in sorted(files, key=lambda p: (not p.startswith("tests/"), p))
                             if p.startswith(("tests/", "scripts/")) and p.endswith((".sh", ".py"))
                             and name in read(p)), None)
            if consumer:
                evidence += f"; {consumer} references the image"
            owner = "boot trial image" if boot_trial else "board diagnostic image"
            _board_fixtures[registered] = (owner, dest, evidence)
        for included in re.findall(r'include_str!\(\s*"([^"/]+\.[sS])"\s*\)', read(registered)):
            assembly = str(PurePosixPath(registered).parent / included)
            if assembly in files:
                _probe_assembly[assembly] = registered

    # The shell toolchain directly invokes rustc for the separate UART smoke binary.
    smoke = ARM + "src/uart_smoke.rs"
    script = "scripts/build-pi2-uart-smoke.sh"
    if smoke in read(script) and "#![no_main]" in read(smoke):
        _probe_sources[smoke] = f"{script} builds the standalone UART smoke executable"
        if 'include_str!("uart_smoke.S")' in read(smoke):
            _probe_assembly[ARM + "src/uart_smoke.S"] = smoke

    for path in sorted(files):
        if path.startswith("targets/") and path.endswith(".rs"):
            for module in re.findall(r'gabix_kernel::net::(\w+)::', read(path)):
                _driver_consumers.setdefault(module, set()).add(path)


def result(owner, destination, status="covered", findings=None):
    return dict(owner=owner, destination=destination, status=status, findings=findings or [])


def finding(owner, destination, rule, reason, evidence, line=None):
    issue = dict(rule=rule, reason=reason, evidence=evidence, destination=destination)
    if line:
        issue["line"] = line
    return result(owner, destination, "finding", [issue])


def first_line(source, pattern):
    return next((n for n, text in enumerate(source.splitlines(), 1) if re.search(pattern, text)), None)


def core_dependency_edges(path, source):
    """Report ordinary Cargo path dependencies, including target-qualified sections."""
    try:
        manifest = tomllib.loads(source)
    except tomllib.TOMLDecodeError:
        return result("Shared kernel manifest requires TOML review", path, "review")
    sections = [("", manifest)]
    sections.extend((f"target.{selector}.", table) for selector, table in manifest.get("target", {}).items())
    findings = []
    for prefix, table in sections:
        for group in ("dependencies", "build-dependencies"):
            for alias, declaration in table.get(group, {}).items():
                if not isinstance(declaration, dict) or not isinstance(declaration.get("path"), str):
                    continue
                resolved = posixpath.normpath(posixpath.join(posixpath.dirname(path), declaration["path"]))
                if not (resolved.startswith("targets/") or (resolved == "." and _root_x86_package)):
                    continue
                findings.append(dict(rule="kernel.core-target-dependency", destination=path,
                                     reason="Shared kernel code declares a dependency on target-owned code. Move its portable contract into the shared crate and implement the adapter in the target.",
                                     evidence=f'[{prefix}{group}] {alias}: path={declaration["path"]!r} resolves to {resolved}/Cargo.toml'))
    if findings:
        return result("shared kernel package", path, "finding", findings)
    return result("shared kernel package", path)


def classify(path: str, source: str):
    if path in _manifest_errors:
        return result(f"Package manifest requires TOML review ({_manifest_errors[path]})", path, "review")
    if path == "Cargo.toml" and _root_x86_package:
        package = finding("x86 target package", X86 + "Cargo.toml", "kernel.root-package",
                       "The root package builds x86 while ARM has a named target package. Move the package definition and keep root workspace configuration here.",
                       'Root [dependencies] contains x86_64; see the package manifest and its src/ tree.',
                       first_line(source, r'^x86_64\s*='))
        package["findings"][0]["package_move"] = {"from": "src", "to": X86 + "src"}
        return package
    if path == ".cargo/config.toml":
        dest = ".cargo/config.toml"
        try:
            config = tomllib.loads(source)
        except tomllib.TOMLDecodeError:
            return result("Cargo configuration requires TOML review", dest, "review")
        settings = config.get("target", {}).get('cfg(target_os = "none")', {})
        if settings.get("runner") == "bootimage runner" or any("--image-base=" in flag for flag in settings.get("rustflags", [])):
            return finding("Cargo build configuration", dest, "kernel.cargo-scope",
                           "The bare-metal selector includes ARM while its runner or linker flags are specific to the PC build. Use the exact x86 target selector in this file.",
                           f'cfg(target_os = "none"): {settings}', first_line(source, r'^\[target\..*target_os'))
        return result("Cargo build configuration", dest)
    if "/" not in path and path.endswith(".json"):
        try:
            spec = json.loads(source)
        except ValueError:
            return None
        if isinstance(spec, dict) and spec.get("arch") == "x86_64" and spec.get("os") == "none":
            return finding("x86 build target", X86 + path, "kernel.target-spec",
                           "The x86 bare-metal specification belongs beside the x86 package. Update its consumers in the same move.",
                           f'arch=x86_64, os=none, llvm-target={spec.get("llvm-target")}', first_line(source, r'"arch"'))
    if path in _probe_sources:
        return finding("ARM hardware probes", ARM + "probes/" + PurePosixPath(path).name,
                       "kernel.probe-bin", "This standalone diagnostic binary lives among the running kernel’s modules. Move its source and private assembly together and update its build paths.",
                       _probe_sources[path], first_line(source, r'^#!\[no_main\]'))
    if path in _board_fixtures:
        owner, dest, evidence = _board_fixtures[path]
        return finding(owner, dest, "kernel.bin-under-tests",
                       "The manifest builds this file as a standalone board executable with test=false. Keep its host-side behavioral tests in tests/.",
                       evidence, first_line(source, r'^#!\[no_main\]'))
    if path in _probe_assembly:
        return result("ARM hardware probes (assembly included by " + _probe_assembly[path] + ")",
                      ARM + "probes/" + PurePosixPath(path).name, "review")

    if (path.startswith("src/") and _root_x86_package) or path.startswith(X86 + "src/"):
        # Recognize the entire Rust item, including absence of any other implementation.
        code = "\n".join(s for s in source.splitlines() if not s.lstrip().startswith("//")).strip()
        forwarding = re.fullmatch(r"pub use gabix_kernel::([a-zA-Z0-9_:]+)::\*;", code)
        if forwarding:
            module = forwarding[1].replace("::", "/")
            canonical = CORE + "src/" + module + ".rs"
            if canonical in _shared_modules:
                return finding("shared kernel module", canonical, "kernel.forwarder",
                               "This file contains only a re-export of an existing shared implementation. Import or re-export the module from its parent and remove this forwarding file.",
                               code, first_line(source, "^pub use "))
        if path.startswith(X86):
            return result("x86 target kernel", path)
        # Each path records the mechanical package move. One manifest finding covers the entire relocation.
        return result("x86 package member", X86 + path, "review")

    if path.startswith(X86):
        return result("x86 target package", path)

    if path.startswith(CORE):
        if path == CORE + "Cargo.toml":
            return core_dependency_edges(path, source)
        module = PurePosixPath(path).stem
        consumers = _driver_consumers.get(module, set())
        if path.startswith(CORE + "src/net/") and re.search(r'^const \w+_VENDOR_ID:', source, re.M) and consumers:
            board = ARM + "src/raspi2b/"
            dest = (board if any(p.startswith(board) for p in consumers)
                    and all(p.startswith(ARM + "src/") for p in consumers) else CORE + "src/drivers/")
            return finding("device framing", dest + module + ".rs", "kernel.driver-format",
                           "This network module encodes USB device identity and framing. Relocate it with the driver or expose a dedicated driver-format module for host tests.",
                           f"Vendor-ID constant and target references in {', '.join(sorted(consumers))}.",
                           first_line(source, r'^const \w+_VENDOR_ID:'))
        raw_hw = first_line(source, r'^\s*(?:core::arch::)?(?:asm|global_asm)!\(')
        if path.startswith(CORE + "src/") and raw_hw:
            return finding("target-specific CPU adapter", "targets/", "kernel.core-hardware",
                           "A shared implementation issues assembly directly. Identify its target, then pass the operation through that target's adapter.",
                           source.splitlines()[raw_hw - 1].strip(), raw_hw)
        return result("shared kernel package", path)

    if path.startswith(ARM):
        if path.startswith(ARM + "src/user_upload/") and re.search(r'^pub fn syscall\(', source, re.M):
            gate = first_line(source, r'if !super::wifi::running\(')
            if gate:
                return finding("ARM socket syscall adapter", ARM + "src/syscall/socket.rs",
                               "kernel.upload-socket", "Socket syscall dispatch is implemented inside the uploader and depends on its running flag. Split the syscall adapter from protocol state and board polling.",
                               source.splitlines()[gate - 1].strip(), gate)
        if path == ARM + "src/main.rs":
            poll = first_line(source, r'^pub fn poll_ethernet_and_reply\(')
            if poll and re.search(r'^static mut PI_NETWORK:', source, re.M):
                return finding("ARM kernel entry and board services", ARM + "src/raspi2b/",
                               "kernel.boot-service", "The kernel entry file owns both the board network device and a recurring poll loop. Extract that service and leave entry and wiring here.",
                               "static mut PI_NETWORK and poll_ethernet_and_reply in the same kernel entry file.", poll)
        owner = "ARM target kernel"
        for prefix, label in (("src/raspi2b/", "Pi 2B board"), ("src/virt/", "QEMU ARM virt board"),
                              ("probes/", "ARM hardware probes"), ("tests/", "ARM behavioral fixtures"),
                              ("boot/", "Pi boot configuration")):
            if path.startswith(ARM + prefix):
                owner = label
                break
        return result(owner, path)
    return None
