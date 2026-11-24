#!/usr/bin/env python3
"""Local evidence index, inspired by zkperf five layers and zkSEC proposal boundary.

Never emits a zk proof, native acceptance, or an authority-bearing mu_exec.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import sys
import tempfile
import unittest

LAYERS = ("binary_closure", "source_debug", "traces", "models", "build_events")
MAX_INPUT = 1024 * 1024


def digest(path):
    with path.open("rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()


def index(root, spec):
    if not isinstance(spec, dict) or set(spec) != {"commit", "layers"}:
        raise ValueError("Expected commit and layers only")
    commit = spec["commit"]
    if not isinstance(commit, str) or not re.fullmatch("[0-9a-f]{40}", commit):
        raise ValueError("Expected exact Git commit")
    layers = spec["layers"]
    if not isinstance(layers, dict) or set(layers) != set(LAYERS):
        raise ValueError("All five layer keys required; absent evidence is []")
    indexed = {}
    for layer in LAYERS:
        paths = layers[layer]
        if not isinstance(paths, list) or len(paths) > 128:
            raise ValueError("Invalid or oversized layer")
        records = []
        seen = set()
        for name in paths:
            if not isinstance(name, str) or not name or len(name) > 512 or "\\" in name:
                raise ValueError("Invalid relative evidence path")
            parts = name.split("/")
            if any(p in ("", ".", "..") for p in parts) or any(ord(c) < 32 for c in name):
                raise ValueError("Invalid relative evidence path")
            path = root
            for part in parts:
                path = path / part
                if path.is_symlink():
                    raise ValueError("Symlink evidence rejected")
            if not path.is_file() or name in seen:
                raise ValueError("Missing or duplicate evidence")
            seen.add(name)
            records.append({"path": name, "sha256": digest(path), "bytes": path.stat().st_size})
        indexed[layer] = sorted(records, key=lambda r: r["path"])
    payload = {"schema": 1, "commit_claim": commit, "layers": indexed,
               "missing_layers": [k for k in LAYERS if not indexed[k]],
               "state": "proposal_only", "grounding": "OPEN",
               "native_acceptance": "OPEN", "publication_authorized": False}
    canonical = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()
    return dict(payload, index_sha256=hashlib.sha256(canonical).hexdigest())


class Tests(unittest.TestCase):
    def test_complete_is_still_proposal(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / "fixture").write_bytes(b"synthetic")
            spec = {"commit": "a" * 40, "layers": {k: ["fixture"] for k in LAYERS}}
            result = index(root, spec)
            self.assertEqual(result, index(root, spec))
            self.assertEqual(result["state"], "proposal_only")
            self.assertEqual(result["grounding"], "OPEN")
            self.assertEqual(result["missing_layers"], [])
            (root / "fixture").write_bytes(b"changed")
            self.assertNotEqual(result["index_sha256"], index(root, spec)["index_sha256"])

    def test_missing_and_external(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            spec = {"commit": "a" * 40, "layers": {k: [] for k in LAYERS}}
            self.assertEqual(index(root, spec)["missing_layers"], list(LAYERS))
            for bad in ("/etc/passwd", "../outside", "a//b"):
                spec["layers"]["traces"] = [bad]
                with self.assertRaises(ValueError):
                    index(root, spec)
            (root / "link").symlink_to("/etc/passwd")
            spec["layers"]["traces"] = ["link"]
            with self.assertRaises(ValueError):
                index(root, spec)

    def test_no_authority_fields(self):
        with tempfile.TemporaryDirectory() as d:
            spec = {"commit": "a" * 40, "layers": {k: [] for k in LAYERS}, "state": "grounded"}
            with self.assertRaises(ValueError):
                index(Path(d), spec)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--self-test", action="store_true")
    p.add_argument("--root", type=Path)
    p.add_argument("--spec", type=Path)
    p.add_argument("--output", type=Path)
    a = p.parse_args()
    if a.self_test:
        return 0 if unittest.main(argv=[sys.argv[0]], exit=False).result.wasSuccessful() else 1
    if not (a.root and a.spec and a.output):
        p.error("--root, --spec and --output required")
    try:
        with a.spec.open("rb") as f:
            raw = f.read(MAX_INPUT + 1)
        if len(raw) > MAX_INPUT:
            raise ValueError("Oversized specification")
        root = a.root.resolve(strict=True)
        if not root.is_dir():
            raise ValueError("Evidence root must be a directory")
        result = index(root, json.loads(raw))
        # Explicit new local file, private mode; never publish or upload.
        import os
        fd = os.open(a.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w") as f:
            json.dump(result, f, indent=2, sort_keys=True)
            f.write("\n")
        print("Local proposal index written; grounding and acceptance OPEN")
        return 0
    except (OSError, ValueError, TypeError) as e:
        print("Evidence index failed: " + str(e), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
