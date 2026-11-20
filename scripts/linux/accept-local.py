#!/usr/bin/env python3
"""Local, non-GUI acceptance orchestrator. Never installs a system package."""
import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]


def git(root, *args):
    return subprocess.check_output(["git", *args], cwd=root, text=True).strip()


def clean(root, sha):
    return git(root, "rev-parse", "HEAD") == sha and not git(
        root, "status", "--porcelain", "--untracked-files=normal")


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


class Runner:
    def __init__(self, root, out):
        self.root, self.out = root, out
        self.env = dict(os.environ, LC_ALL="C", TZ="UTC")
        self.results = {}

    def stage(self, name, argv, needs=(), probe=False):
        blocked = [n for n in needs if self.results[n]["status"] != "PASS"]
        row = {"argv": argv}
        if blocked:
            row.update(status="BLOCKED", reason="Prerequisites: " + ", ".join(blocked))
        elif not shutil.which(argv[0], path=self.env.get("PATH")):
            row.update(status="BLOCKED", reason="Missing executable: " + argv[0])
        else:
            path = self.out / (name + ".log")
            start = time.monotonic()
            with path.open("xb") as log:
                try:
                    rc = subprocess.run(argv, cwd=self.root, env=self.env,
                                        stdout=log, stderr=subprocess.STDOUT).returncode
                    row.update(status="PASS" if rc == 0 else ("BLOCKED" if probe else "FAIL"), exit_code=rc)
                except OSError as exc:
                    row.update(status="FAIL", reason=str(exc))
            row.update(log=path.name, sha256=digest(path),
                       elapsed_seconds=round(time.monotonic() - start, 3))
        self.results[name] = row
        print(name + ": " + row["status"], flush=True)
        return row

    def receipt(self, metadata):
        path = self.out / "receipt.json"
        tmp = self.out / "receipt.tmp"
        tmp.write_text(json.dumps(dict(metadata, stages=self.results), indent=2) + "\n")
        tmp.replace(path)


def exit_status(results, unchanged):
    if not unchanged or any(r["status"] == "FAIL" for r in results.values()):
        return 1
    if any(r["status"] != "PASS" for r in results.values()):
        return 2
    return 0


def execute(root, out):
    sha = git(root, "rev-parse", "HEAD")
    if not clean(root, sha):
        raise ValueError("Use a clean committed checkout")
    r = Runner(root, out)
    meta = {"schema": 1, "commit": sha, "host": platform.platform(),
            "os_release": platform.freedesktop_os_release(),
            "lock_sha256": {p: digest(root / p) for p in
                            ("package-lock.json", "src-tauri/Cargo.lock")},
            "scope": "P0 local build and developer fixtures only; GUI/security/package acceptance OPEN",
            "complete": False}
    r.receipt(meta)

    def stage(name, argv, needs=(), probe=False):
        # Never start another source-dependent stage after a checkout change.
        if not clean(root, sha):
            r.results[name] = {"argv": argv, "status": "BLOCKED",
                               "reason": "Checkout changed during execution"}
            print(name + ": BLOCKED", flush=True)
        else:
            r.stage(name, argv, needs, probe)
        r.receipt(meta)

    for tool in ("node", "npm", "rustc", "cargo"):
        stage(tool, [tool, "--version"], probe=True)
    stage("webkit_min", ["pkg-config", "--atleast-version=2.40", "webkit2gtk-4.1"], probe=True)
    stage("native_deps", ["pkg-config", "--modversion", "gtk+-3.0",
                          "webkit2gtk-4.1", "libsoup-3.0", "openssl"], probe=True)
    stage("js_policy", ["node", "--test", "scripts/linux/test-pip-policy.cjs",
                        "scripts/linux/test-vault-fill.cjs", "scripts/linux/test-vault-save.cjs"], ("node",))
    for name, file, flag in (
        ("package_fixture", "check-package.py", "--self-test"),
        ("network_fixture", "network_fixture.py", "test"),
        ("filter_fixture", "compile_webkit_subset.py", "--test"),
    ):
        stage(name, [sys.executable, "scripts/linux/" + file, flag])
    stage("source_candidate_fixture", [sys.executable, "scripts/linux/test-source-candidate.py"])
    stage("witness_receipt_fixture", [sys.executable, "scripts/linux/witness-receipt.py", "--self-test"])
    stage("itir_evidence_fixture", [sys.executable, "scripts/linux/itir-evidence-check.py", "--self-test"])
    stage("egress_proxy_fixture", [sys.executable, "scripts/linux/egress_proxy_fixture.py", "test"])
    stage("egress_tls_tunnel_fixture", [sys.executable, "scripts/linux/egress_tls_tunnel_fixture.py", "test"])
    stage("npm_install", ["npm", "ci"], ("node", "npm"))
    stage("frontend_build", ["npm", "run", "build"], ("npm_install",))
    stage("frontend_test", ["npm", "test"], ("npm_install",))
    stage("rust_check", ["cargo", "check", "--manifest-path", "src-tauri/Cargo.toml",
                         "--locked"], ("rustc", "cargo", "webkit_min", "native_deps", "frontend_build"))
    stage("rust_test", ["cargo", "test", "--manifest-path", "src-tauri/Cargo.toml",
                        "--locked"], ("rust_check",))
    unchanged = clean(root, sha)
    rc = exit_status(r.results, unchanged)
    meta.update(complete=True, checkout_unchanged=unchanged, exit_code=rc)
    r.receipt(meta)
    return rc


class RunnerTests(unittest.TestCase):
    def test_success_failure_and_block(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            r = Runner(root, root)
            ok = r.stage("ok", [sys.executable, "-c", "print('receipt marker')"])
            self.assertEqual(ok["status"], "PASS")
            self.assertEqual(ok["sha256"], digest(root / ok["log"]))
            self.assertIn("receipt marker", (root / ok["log"]).read_text())
            fail = r.stage("fail", [sys.executable, "-c", "raise SystemExit(7)"])
            self.assertEqual(fail["exit_code"], 7)
            probe = r.stage("probe", [sys.executable, "-c", "raise SystemExit(7)"], probe=True)
            self.assertEqual(probe["status"], "BLOCKED")
            self.assertEqual(probe["exit_code"], 7)
            self.assertEqual(r.stage("blocked", [sys.executable], ("fail",))["status"], "BLOCKED")
            self.assertFalse((root / "blocked.log").exists())
            missing = r.stage("missing", ["/nonexistent/bushido-test-command"])
            self.assertEqual(missing["status"], "BLOCKED")
            r.receipt({"commit": "fixture"})
            report = json.loads((root / "receipt.json").read_text())
            self.assertEqual(report["stages"], r.results)
            self.assertEqual(exit_status(r.results, True), 1)

    def test_exit_codes(self):
        self.assertEqual(exit_status({"a": {"status": "PASS"}}, True), 0)
        self.assertEqual(exit_status({"a": {"status": "BLOCKED"}}, True), 2)
        self.assertEqual(exit_status({"a": {"status": "PASS"}}, False), 1)

    def test_clean_checkout(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            git(root, "init", "-q")
            git(root, "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                "commit", "--allow-empty", "-qm", "fixture")
            sha = git(root, "rev-parse", "HEAD")
            self.assertTrue(clean(root, sha))
            (root / "dirty").touch()
            self.assertFalse(clean(root, sha))
            (root / "dirty").unlink()
            git(root, "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                "commit", "--allow-empty", "-qm", "changed")
            self.assertFalse(clean(root, sha))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return 0 if unittest.main(argv=[sys.argv[0]], exit=False).result.wasSuccessful() else 1
    if sys.platform != "linux":
        parser.error("Linux host required")
    os.umask(0o077)
    # Outside checkout; no profile access, sudo, auto-upload or hosted CI.
    out = Path(tempfile.mkdtemp(prefix="bushido-acceptance-", dir="/tmp"))
    print("Private receipt directory: " + str(out), flush=True)
    try:
        return execute(ROOT, out)
    except (OSError, ValueError, subprocess.CalledProcessError) as exc:
        print("Acceptance setup failed: " + str(exc), file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
