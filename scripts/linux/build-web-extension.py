#!/usr/bin/env python3
"""Build one debug extension outside a clean source checkout. No install."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile


def run(*args, cwd):
    return subprocess.check_output(args, cwd=cwd, text=True, timeout=30).strip()


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output_parent",type=Path)
    args=parser.parse_args()
    root=Path(__file__).resolve().parents[2]
    parent=args.output_parent.resolve(strict=True)
    if not parent.is_dir() or parent==root or root in parent.parents:
        parser.error("existing output parent outside repository required")
    if run("git","status","--porcelain","--untracked-files=all",cwd=root):
        parser.error("clean checkout required")
    head=run("git","rev-parse","HEAD",cwd=root)
    source=root/"scripts/linux/web-extension/scope-fixture.c"
    source_hash=digest(source)
    package="webkit2gtk-web-extension-4.1"
    version=run("pkg-config","--modversion",package,cwd=root)
    flags=shlex.split(run("pkg-config","--cflags","--libs",package,cwd=root))
    os.umask(0o077)
    out=Path(tempfile.mkdtemp(prefix="bushido-extension-",dir=parent))
    extension_dir=out/"extension"
    extension_dir.mkdir(mode=0o700)
    binary=extension_dir/"libbushido-scope-fixture.so"
    command=["cc","-std=c11","-Wall","-Wextra","-Werror","-shared","-fPIC",
             "-fvisibility=hidden","-Wl,-z,relro,-z,now","-Wl,--no-undefined",
             str(source),"-o",str(binary),*flags]
    subprocess.run(command,cwd=root,check=True,timeout=60)
    if head!=run("git","rev-parse","HEAD",cwd=root) or digest(source)!=source_hash or run(
        "git","status","--porcelain","--untracked-files=all",cwd=root):
        raise SystemExit("source changed during build; do not use output")
    binary.chmod(0o500)
    extension_dir.chmod(0o500)
    sha=digest(binary)
    receipt={"schema":1,"state":"proposal_only","commit_claim":head,
             "source_sha256":source_hash,"binary_sha256":sha,"webkit_extension_version":version,
             "compiler":run("cc","--version",cwd=root).splitlines()[0],
             "native_acceptance":"OPEN","runtime_binary_linkage_verified":False,
             "publication_authorized":False}
    (out/"receipt.json").write_text(json.dumps(receipt,sort_keys=True,indent=2)+"\n",encoding="utf-8")
    print("BUSHIDO_LINUX_EXTENSION_FIXTURE_DIR="+str(extension_dir))
    print("BUSHIDO_LINUX_EXTENSION_FIXTURE_SHA256="+sha)
    print("Private build receipt: "+str(out/"receipt.json"))


if __name__=="__main__":
    main()
