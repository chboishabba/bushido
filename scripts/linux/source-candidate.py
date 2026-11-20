#!/usr/bin/env python3
"""Generate a local, exact-commit source candidate and unreviewed dependency inventory."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path, PurePosixPath
import subprocess
import sys
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]
FORKS = ("tauri", "tauri-runtime", "tauri-runtime-wry", "wry")


def git(root, *args):
    return subprocess.check_output(["git", *args], cwd=root)


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def tree(root, commit):
    entries = {}
    for record in git(root, "ls-tree", "-rz", "--full-tree", commit).split(b"\0"):
        if not record:
            continue
        header, raw_path = record.split(b"\t", 1)
        mode, kind, oid = header.decode("ascii").split()
        path = raw_path.decode("utf-8")
        if kind != "blob" or mode not in ("100644", "100755"):
            raise ValueError("Unsupported tracked entry (symlink/submodule/etc): " + path)
        if any(p in ("", ".", "..") for p in path.split("/")) or "\\" in path:
            raise ValueError("Unsupported tracked path: " + path)
        entries[path] = (mode, oid)
    return entries


def verify_archive(path, prefix, expected):
    seen, selected = set(), {}
    with tarfile.open(path, "r:gz") as archive:
        for member in archive:
            if member.isdir():
                continue
            if not member.isfile() or not member.name.startswith(prefix):
                raise ValueError("Unexpected archive member: " + member.name)
            name = member.name[len(prefix):]
            if name not in expected or name in seen:
                raise ValueError("Unexpected or duplicate archive path: " + name)
            mode, oid = expected[name]
            if bool(member.mode & 0o111) != (mode == "100755"):
                raise ValueError("Executable mode mismatch: " + name)
            stream = archive.extractfile(member)
            data = stream.read()
            actual = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
            if actual != oid:
                raise ValueError("Git blob mismatch: " + name)
            seen.add(name)
            if (name in ("Cargo.lock", "src-tauri/Cargo.lock", "package-lock.json",
                         "src-tauri/Cargo.toml") or
                    name in {f"src-tauri/forks/{f}/Cargo.toml" for f in FORKS} or
                    name.startswith("src-tauri/adblock/")):
                selected[name] = data
    if seen != set(expected):
        raise ValueError("Archive omitted tracked paths: " + ", ".join(sorted(set(expected) - seen)[:10]))
    return selected


def inventory(data, expected):
    npm = json.loads(data["package-lock.json"])
    if npm.get("lockfileVersion") != 3:
        raise ValueError("Expected npm lockfile version 3")
    js = []
    for path, pkg in sorted(npm["packages"].items()):
        if not path:
            continue
        js.append({"path": path, **{k: pkg.get(k) for k in
                   ("version", "resolved", "integrity", "license", "dev", "optional", "link")}})
    rust = []
    lock = tomllib.loads(data["src-tauri/Cargo.lock"].decode())
    for pkg in lock["package"]:
        rust.append({k: pkg.get(k) for k in ("name", "version", "source", "checksum")})
    local = []
    for name, path in [("bushido", "src-tauri/Cargo.toml")] + [
            (f, f"src-tauri/forks/{f}/Cargo.toml") for f in FORKS]:
        pkg = tomllib.loads(data[path].decode())["package"]
        local.append({"owner": name, "manifest": path, "name": pkg["name"],
                      "version": pkg["version"], "declared_license": pkg.get("license"),
                      "license_file": pkg.get("license-file")})
    assets = []
    for path, content in sorted(data.items()):
        if not path.startswith("src-tauri/adblock/"):
            continue
        # Preserve only declared header lines, not rule bodies or scripts.
        header = [line for line in content[:16384].decode("utf-8", errors="replace").splitlines()[:40]
                  if line.startswith("!") and any(k in line.lower() for k in
                     ("license:", "homepage:", "source:", "title:", "copyright"))]
        assets.append({"path": path, "sha256": hashlib.sha256(content).hexdigest(),
                       "header_evidence": header, "review": "OPEN"})
    return {"status": "UNREVIEWED", "npm_lock_entries": js, "cargo_lock_entries": rust,
            "local_manifests": local, "filter_and_scriptlet_assets": assets,
            "notice_paths": sorted(p for p in expected if any(
                PurePosixPath(p).name.upper().startswith(x)
                for x in ("LICENSE", "COPYING", "NOTICE"))),
            "limitations": ["Lock inventory includes build and other-platform entries, not a linked-binary closure.",
                            "Cargo lock does not establish third-party licenses.",
                            "Registry source archives, actual license texts, generated-source provenance and system dependency review are separate open work."]}


def generate(root, destination):
    commit = git(root, "rev-parse", "HEAD^{commit}").decode().strip()
    if git(root, "rev-parse", "--show-object-format").strip() != b"sha1":
        raise ValueError("Only SHA-1 Git repositories supported")
    if git(root, "status", "--porcelain", "--untracked-files=normal"):
        raise ValueError("Use a clean committed checkout")
    dest = destination.resolve()
    if dest == root or root in dest.parents:
        raise ValueError("Output directory must be outside the checkout")
    expected = tree(root, commit)
    required = {"LICENSE", "package-lock.json", "src-tauri/Cargo.lock",
                "src-tauri/Cargo.toml", "scripts/linux/build-deb.sh"}
    required.update(f"src-tauri/forks/{f}/Cargo.toml" for f in FORKS)
    if not required <= set(expected):
        raise ValueError("Missing required source paths")
    dest.mkdir(mode=0o700)  # refuse existing destination; never overwrite
    prefix = "bushido-" + commit + "/"
    archive = dest / "source-candidate.tar.gz"
    with archive.open("xb") as raw:
        with gzip.GzipFile(fileobj=raw, mode="wb", filename="", mtime=0) as zipped:
            with subprocess.Popen(["git", "archive", "--format=tar", "--prefix=" + prefix, commit],
                                  cwd=root, stdout=subprocess.PIPE) as proc:
                while chunk := proc.stdout.read(1024 * 1024):
                    zipped.write(chunk)
                if proc.wait():
                    raise ValueError("git archive failed")
    selected = verify_archive(archive, prefix, expected)
    inv = inventory(selected, expected)
    dependency_file = dest / "dependency-inventory.json"
    dependency_file.write_text(json.dumps(inv, indent=2, sort_keys=True) + "\n")
    if (git(root, "rev-parse", "HEAD").decode().strip() != commit or
            git(root, "status", "--porcelain", "--untracked-files=normal")):
        raise ValueError("Checkout changed during generation")
    receipt = {"schema": 1, "commit": commit, "tracked_file_count": len(expected),
               "archive_verified": True, "source_candidate_sha256": sha256(archive),
               "dependency_inventory_sha256": sha256(dependency_file),
               "lock_sha256": {p: hashlib.sha256(selected[p]).hexdigest()
                               for p in ("package-lock.json", "src-tauri/Cargo.lock")},
               "publication_authorized": False, "corresponding_source_review": "OPEN"}
    (dest / "receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path, help="New directory outside repository")
    args = parser.parse_args()
    try:
        result = generate(ROOT, args.destination)
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError, tarfile.TarError) as exc:
        print("Source candidate rejected; incomplete output is not a receipt: " + str(exc), file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
