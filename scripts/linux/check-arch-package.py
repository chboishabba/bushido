#!/usr/bin/env python3
"""Static Arch package/source candidate inspection; never installs or executes."""
import argparse
from pathlib import Path
import hashlib
import re
import subprocess
import sys
import tarfile
import tempfile
import unittest

MAX_DESKTOP=8192
REQUIRED={
    "usr/bin/bushido-browser",
    "usr/share/applications/bushido-browser.desktop",
    "usr/share/pixmaps/bushido-browser.png",
    "usr/share/licenses/bushido-browser/LICENSE",
}
SHA=re.compile(r"[0-9a-f]{64}")


def source_check(pkgbuild, archive):
    text=pkgbuild.read_text()
    match=re.search(r"^sha256sums=\('([0-9a-f]{64})'\)$",text,re.M)
    if not match:raise ValueError("Unresolved or invalid source checksum")
    if hashlib.sha256(archive.read_bytes()).hexdigest()!=match[1]:
        raise ValueError("Source checksum mismatch")
    with tarfile.open(archive,"r:gz") as tar:
        names=tar.getnames()
        if not names or len(names)>20000 or any(
            (n != "bushido-source" and not n.startswith("bushido-source/")) or
            any(p in ("..","") for p in n.rstrip("/").split("/")) for n in names
        ):raise ValueError("Unexpected source path")
        for member in tar:
            if not (member.isfile() or member.isdir()):
                raise ValueError("Nonregular source archive entry")
    return True


def package_check(path):
    result=subprocess.run(["bsdtar","-tf",str(path)],stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE,check=True,timeout=20)
    names=[s.removeprefix("./") for s in result.stdout.decode().splitlines()]
    if len(names)>4096 or len(names)!=len(set(names)) or not REQUIRED<=set(names):
        raise ValueError("Required files missing or archive ambiguous")
    for name in names:
        if name.startswith("/") or any(p==".." for p in name.split("/")):
            raise ValueError("Unsafe archive path")
    meta=subprocess.run(["bsdtar","-xOf",str(path),".PKGINFO"],
                        stdout=subprocess.PIPE,check=True,timeout=20).stdout.decode()
    if "pkgname = bushido-browser" not in meta or "pkgver = 0.14.0-1" not in meta:
        raise ValueError("Unexpected Arch package identity")
    raw=subprocess.run(["bsdtar","-xOf",str(path),
                        "usr/share/applications/bushido-browser.desktop"],
                       stdout=subprocess.PIPE,check=True,timeout=20).stdout
    if len(raw)>MAX_DESKTOP:raise ValueError("Desktop metadata oversized")
    desktop=raw.decode()
    if ("Exec=bushido-browser" not in desktop or
        "Icon=bushido-browser" not in desktop or
        "MimeType=" in desktop or "{{icon}}" in desktop or
        re.search(r"^Exec=.*%[fFuU]",desktop,re.M)):
        raise ValueError("Desktop activation policy mismatch")
    return True


class Tests(unittest.TestCase):
    def test_checksum_fail_closed(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d)
            pkg=root/"PKGBUILD"
            src=root/"source.tar.gz"
            src.write_bytes(b"not an archive")
            pkg.write_text("sha256sums=('"+("a"*64)+"')\n")
            with self.assertRaises(ValueError):source_check(pkg,src)
    def test_unresolved_source_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d)
            (root/"PKGBUILD").write_text("sha256sums=('__LOCAL_SOURCE_SHA256__')\n")
            (root/"a").write_bytes(b"abc")
            with self.assertRaises(ValueError):
                source_check(root/"PKGBUILD",root/"a")
    def test_missing_package_fails(self):
        with self.assertRaises((OSError,subprocess.CalledProcessError)):
            package_check(Path("/nonexistent/bushido-package.pkg.tar.zst"))


def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--self-test",action="store_true")
    ap.add_argument("--source",type=Path)
    ap.add_argument("--archive",type=Path)
    ap.add_argument("--package",type=Path)
    args=ap.parse_args()
    if args.self_test:
        return int(not unittest.main(argv=[sys.argv[0]],exit=False).result.wasSuccessful())
    try:
        if args.source or args.archive:
            if not (args.source and args.archive):ap.error("Both source and archive required")
            source_check(args.source,args.archive)
        elif args.package:
            package_check(args.package)
        else:ap.error("Source/archive or package required")
        print("Arch static candidate inspection PASS (not native acceptance)")
        return 0
    except (OSError,ValueError,UnicodeError,tarfile.TarError,
            subprocess.CalledProcessError,subprocess.TimeoutExpired):
        print("Arch static candidate inspection FAIL",file=sys.stderr)
        return 1


if __name__=="__main__":
    sys.exit(main())
