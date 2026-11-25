"""Check the layout report against an isolated Git working tree."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "audit-layout.py"


class LayoutAuditTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name) / "repo"
        self.root.mkdir()
        self.git("init", "--quiet")

    def git(self, *args):
        subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True)

    def write(self, path, text):
        file = self.root / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(text)
        return file

    def audit(self):
        output = subprocess.check_output(
            [sys.executable, str(SCRIPT), "--root", str(self.root)], cwd=self.root
        )
        return json.loads(output)

    def test_scan_accounts_for_git_visible_files(self):
        self.write(".gitignore", "ignored/\n")
        self.write(".hidden/deep/empty.rs", "")
        (self.root / "empty-directory").mkdir()
        self.write("tracked.rs", "fn a() {}\n\nfn b() {}\n")
        self.git("add", "tracked.rs")
        self.write("ignored/local.rs", "fn local() {}\n")
        self.write("ignored/kept.rs", "fn kept() {}\n")
        self.git("add", "--force", "ignored/kept.rs")
        self.write("memos/notes.md", "Documentation\n")
        external = self.root.parent / "outside"
        external.mkdir()
        (external / "private.rs").write_text("fn outside() {}\n")
        (self.root / "alias").symlink_to(external, target_is_directory=True)
        (self.root / "alias.rs").symlink_to(external / "private.rs")
        stale = self.write("replaced/private.rs", "fn tracked() {}\n")
        self.git("add", "replaced/private.rs")
        stale.unlink()
        stale.parent.rmdir()
        stale.parent.symlink_to(external, target_is_directory=True)

        report = self.audit()
        files = {record["path"]: record for record in report["files"]}
        self.assertIn("ignored/kept.rs", files)
        self.assertNotIn("ignored/local.rs", files)
        self.assertNotIn("alias/private.rs", files)
        self.assertEqual(files["tracked.rs"]["loc"], 2)
        self.assertEqual(files[".hidden/deep/empty.rs"]["loc"], 0)
        self.assertEqual(files["memos/notes.md"]["loc"], 0)
        for name in ("alias", "alias.rs"):
            self.assertEqual(files[name]["loc"], 0)
        if "replaced/private.rs" in files:
            self.assertEqual(files["replaced/private.rs"]["loc"], 0)

        directories = {record["path"] for record in report["directories"]}
        self.assertTrue({".hidden", ".hidden/deep", "memos", "ignored", "empty-directory"} <= directories)

    def test_scan_preserves_directory_totals(self):
        self.write("README.md", "Gabix\n")
        self.write("learn/learning-records/0001-first.md", "First\n")
        self.write("learn/learning-records/0002-second.md", "Second\n")
        self.write("tests/fixtures/hello.rs", "fn main() {}\n")
        report = self.audit()
        files = report["files"]
        directories = report["directories"]
        self.assertEqual(
            {f["owner"] for f in files if f["path"].startswith("learn/learning-records/")},
            {"Learning records"},
        )
        self.assertIn(".", {d["path"] for d in directories})
        for directory in directories:
            parent = Path(directory["path"])
            direct = [f for f in files if Path(f["path"]).parent == parent]
            nested = [f for f in files if parent in Path(f["path"]).parents]
            for records, prefix in ((direct, "direct_"), (nested, "")):
                with self.subTest(path=str(parent), prefix=prefix):
                    self.assertEqual(directory[prefix + "files"], len(records))
                    self.assertEqual(directory[prefix + "loc"], sum(f["loc"] for f in records))
                    self.assertEqual(directory[prefix + "findings"],
                                     sum(len(f["findings"]) for f in records))

    def test_move_target_spec_clears_placement_finding(self):
        source = self.write("x86_64-mini-os.json", '{"arch":"x86_64","os":"none"}\n')
        target = self.root / "targets/x86_64/x86_64-mini-os.json"
        target.parent.mkdir(parents=True)

        def misplaced():
            return [
                record["path"]
                for record in self.audit()["files"]
                if any(issue["rule"] == "kernel.target-spec" for issue in record["findings"])
            ]

        self.assertEqual(misplaced(), [source.name])
        source.rename(target)
        self.assertEqual(misplaced(), [])

    def test_scan_locates_duplicate_probe_source(self):
        body = ".global draw\ndraw:\n  bx lr\n"
        driver = "targets/armv7/src/raspi2b/draw.S"
        probe = "targets/armv7/probes/triangle.S"
        self.write(driver, body)
        self.write(probe, body)
        self.write("targets/armv7/probes/draw.S", ".global setup\nsetup:\n  bx lr\n")

        def duplicates():
            report = self.audit()
            return [
                (record["path"], issue["destination"])
                for record in report["files"] for issue in record["findings"]
                if issue["rule"] == "copied-probe-implementation"
            ]

        self.assertEqual(duplicates(), [(probe, driver)])
        (self.root / probe).unlink()
        self.assertEqual(duplicates(), [])


if __name__ == "__main__":
    unittest.main()
