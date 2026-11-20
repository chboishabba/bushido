#!/usr/bin/env python3
"""Operator-run, local ITIR bridge client for the Bushido evidence index only."""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import unittest

MAX_BYTES = 65536
TOOL = "itir.zkperf.evidence_index_check"


def request(index):
    if not isinstance(index, dict):
        raise ValueError("Expected index object")
    raw = json.dumps({"op": "safe_call", "name": TOOL, "payload": {"index": index}},
                     separators=(",", ":"), allow_nan=False).encode()
    if len(raw) > MAX_BYTES:
        raise ValueError("Evidence request exceeds local bound")
    return raw + b"\n"


def check_response(raw, index):
    if len(raw) > 256 * 1024:
        raise ValueError("Bridge response exceeds bound")
    reply = json.loads(raw)
    if not isinstance(reply, dict) or reply.get("ok") is not True:
        raise ValueError("ITIR bridge rejected or unavailable")
    envelope = reply.get("result")
    if not isinstance(envelope, dict) or envelope.get("tool") != TOOL or envelope.get("decision") != "abstained":
        raise ValueError("Expected non-promoting safe_call receipt")
    result = envelope.get("result")
    if not isinstance(result, dict) or result.get("version") != TOOL + ".v1":
        raise ValueError("Unexpected tool contract")
    if result.get("state") != "proposal_only" or result.get("non_authoritative") is not True:
        raise ValueError("Evidence authority mismatch")
    for key in ("execution_authorized", "publication_authorized", "files_verified",
                "binary_linkage_verified", "proof_verified"):
        if result.get(key) is not False:
            raise ValueError("Unexpected authority or verification claim")
    if (result.get("self_commitment_matches") is not True
            or result.get("index_sha256") != index.get("index_sha256")
            or result.get("commit_claim") != index.get("commit_claim")):
        raise ValueError("Evidence index identity mismatch")
    # This is a contract receipt, not a security/performance/native test pass.
    return {"schema": 1, "state": "proposal_only", "native_acceptance": "OPEN",
            "itir_response": envelope}


def invoke(index, python, root):
    # Operator explicitly selects a trusted local interpreter and suite root.
    # No shell, no caller-controlled tool name or arbitrary command.
    if not python.is_absolute() or not python.is_file() or not root.is_absolute() or not root.is_dir():
        raise ValueError("Absolute local interpreter and suite root required")
    response = subprocess.run([str(python), "-m", "itir_mcp", "--bridge"],
                              cwd=root, input=request(index), stdout=subprocess.PIPE,
                              stderr=subprocess.DEVNULL, timeout=20, check=False)
    if response.returncode != 0:
        raise ValueError("Local ITIR bridge failed")
    return check_response(response.stdout, index)


class Tests(unittest.TestCase):
    def test_request_fixed(self):
        r = json.loads(request({"schema": 1}))
        self.assertEqual(r["op"], "safe_call")
        self.assertEqual(r["name"], TOOL)
        with self.assertRaises(ValueError):
            request({"large": "x" * MAX_BYTES})

    def test_receipt_no_promotion(self):
        idx = {"index_sha256": "a"*64, "commit_claim": "b"*40}
        result = dict(idx, version=TOOL+".v1", state="proposal_only",
                      non_authoritative=True, self_commitment_matches=True,
                      execution_authorized=False, publication_authorized=False,
                      files_verified=False,binary_linkage_verified=False,proof_verified=False)
        envelope = dict(tool=TOOL, decision="abstained", result=result)
        reply = dict(ok=True, result=envelope)
        self.assertEqual(check_response(json.dumps(reply).encode(), idx)["native_acceptance"], "OPEN")
        result["execution_authorized"] = True
        with self.assertRaises(ValueError):
            check_response(json.dumps(reply).encode(), idx)
        result["execution_authorized"] = False
        envelope["decision"] = "verified"
        with self.assertRaises(ValueError):
            check_response(json.dumps(reply).encode(), idx)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--index", type=Path)
    parser.add_argument("--python", type=Path)
    parser.add_argument("--suite-root", type=Path)
    parser.add_argument("--output", type=Path)
    a = parser.parse_args()
    if a.self_test:
        return int(not unittest.main(argv=[sys.argv[0]], exit=False).result.wasSuccessful())
    if not (a.index and a.python and a.suite_root and a.output):
        parser.error("all four paths required")
    try:
        with a.index.open("rb") as f:
            raw = f.read(MAX_BYTES + 1)
        if len(raw) > MAX_BYTES:
            raise ValueError("Oversized local index")
        receipt = invoke(json.loads(raw), a.python, a.suite_root)
        import os
        fd = os.open(a.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w") as f:
            json.dump(receipt, f, indent=2, sort_keys=True)
            f.write("\n")
        print("Local proposal receipt written; native acceptance OPEN")
        return 0
    except (OSError, ValueError, subprocess.SubprocessError, TypeError, RecursionError):
        print("Local ITIR evidence check failed; no acceptance inferred", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
