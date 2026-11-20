#!/usr/bin/env python3
"""Synthetic Git fixtures; no real project build or dependency fetch."""
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("source_candidate", Path(__file__).with_name("source-candidate.py"))
source = importlib.util.module_from_spec(spec)
spec.loader.exec_module(source)


class CandidateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.root = self.base / "repo"
        self.root.mkdir()
        self.git("init", "-q", "--object-format=sha1")
        self.write("LICENSE", "fixture license\n")
        self.write("package-lock.json", json.dumps({"lockfileVersion": 3, "packages": {
            "": {}, "node_modules/example": {"version": "1.0", "license": "MIT"}}}))
        self.write("src-tauri/Cargo.lock", 'version = 4\n[[package]]\nname = "example"\nversion = "1.0"\n')
        for f in ("", *source.FORKS):
            path = f"src-tauri/forks/{f}/Cargo.toml" if f else "src-tauri/Cargo.toml"
            self.write(path, f'[package]\nname = "{f or "fixture"}"\nversion = "1.0"\nlicense = "MIT"\n')
        self.write("scripts/linux/build-deb.sh", "#!/bin/sh\n")
        (self.root / "scripts/linux/build-deb.sh").chmod(0o755)
        self.write("src-tauri/adblock/filter.txt", "! License: fixture\n||example.test^\n")
        self.commit()

    def git(self, *args):
        subprocess.run(["git", *args], cwd=self.root, check=True, stdout=subprocess.DEVNULL)

    def write(self, path, content):
        p = self.root / path
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(content)

    def commit(self):
        self.git("add", ".")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "fixture")

    def test_exact_and_repeatable(self):
        a, b = self.base / "a", self.base / "b"
        first = source.generate(self.root, a)
        second = source.generate(self.root, b)
        self.assertEqual(first, second)
        self.assertFalse(first["publication_authorized"])
        self.assertTrue(first["archive_verified"])
        inv = json.loads((a / "dependency-inventory.json").read_text())
        self.assertEqual(len(inv["local_manifests"]), 5)
        self.assertEqual(inv["npm_lock_entries"][0]["license"], "MIT")
        self.assertEqual(inv["filter_and_scriptlet_assets"][0]["header_evidence"], ["! License: fixture"])
        self.assertEqual(source.sha256(a / "source-candidate.tar.gz"), first["source_candidate_sha256"])
        with self.assertRaises(FileExistsError):
            source.generate(self.root, a)

    def test_dirty_and_inside(self):
        with self.assertRaises(ValueError):
            source.generate(self.root, self.root / "output")
        self.write("LICENSE", "changed")
        with self.assertRaises(ValueError):
            source.generate(self.root, self.base / "dirty")

    def test_export_ignore_fails(self):
        self.write(".gitattributes", "LICENSE export-ignore\n")
        self.commit()
        dest = self.base / "ignored"
        with self.assertRaises(ValueError):
            source.generate(self.root, dest)
        self.assertFalse((dest / "receipt.json").exists())

    def test_export_substitution_fails(self):
        self.write(".gitattributes", "LICENSE export-subst\n")
        self.write("LICENSE", "$Format:%H$\n")
        self.commit()
        with self.assertRaises(ValueError):
            source.generate(self.root, self.base / "substituted")

    def test_symlink_rejected(self):
        (self.root / "pointer").symlink_to("LICENSE")
        self.commit()
        with self.assertRaises(ValueError):
            source.generate(self.root, self.base / "symlink")


if __name__ == "__main__":
    unittest.main()
