#!/usr/bin/env python3
"""Local fixed-tool ITIR candidate projection, never acceptance or approval."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import unittest

TOOL="itir.zkperf.bushido_ab_candidate"
MAX_BYTES=65536
MAX_REPLY=262144
UTC_PATTERN=re.compile(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?Z")


def reject(message):
    raise ValueError(message)


def unique(pairs):
    result={}
    for key,value in pairs:
        if key in result: reject("Duplicate JSON key")
        result[key]=value
    return result


def read_candidate(raw):
    if len(raw)>MAX_BYTES: reject("Oversized candidate")
    data=json.loads(raw,object_pairs_hook=unique,parse_constant=lambda _:reject("Nonfinite JSON"))
    if type(data) is not dict or data.get("version")!="bushido.extension_ab_reconciliation.v1":
        reject("Expected Bushido reconciliation")
    return data


def time_value(raw):
    if not isinstance(raw,str) or not UTC_PATTERN.fullmatch(raw):
        reject("Expected explicit UTC timestamp")
    try:
        value=datetime.fromisoformat(raw.replace("Z","+00:00"))
    except ValueError:
        reject("Invalid timestamp")
    if value.tzinfo != timezone.utc:
        reject("Expected UTC")
    return raw


def canonical_hash(obj):
    return hashlib.sha256(json.dumps(obj,sort_keys=True,separators=(",",":"),
        ensure_ascii=True,allow_nan=False).encode("utf-8")).hexdigest()


def request(candidate, asserted_at):
    time_value(asserted_at)
    content={"op":"safe_call","name":TOOL,
             "payload":{"reconciliation":candidate,"asserted_at":asserted_at}}
    raw=json.dumps(content,sort_keys=True,separators=(",",":"),allow_nan=False).encode()
    if len(raw)>MAX_BYTES: reject("ITIR request exceeds bound")
    return raw+b"\n"


def validate(response, candidate):
    if len(response)>MAX_REPLY: reject("Oversized ITIR response")
    payload=json.loads(response,object_pairs_hook=unique,
                       parse_constant=lambda _:reject("Nonfinite response"))
    if type(payload) is not dict or payload.get("ok") is not True:
        reject("ITIR safe_call unavailable or rejected")
    envelope=payload.get("result")
    if type(envelope) is not dict or envelope.get("tool")!=TOOL or envelope.get("decision")!="abstained":
        reject("Unexpected ITIR authority")
    result=envelope.get("result")
    if type(result) is not dict or result.get("version")!=TOOL+".v1":
        reject("Unexpected ITIR tool result")
    for key in ("candidate_only","non_authoritative"):
        if result.get(key) is not True: reject("Candidate authority mismatch")
    for key in ("execution_authorized","publication_authorized",
                "supplied_input_hashes_verified","files_verified","same_run_linkage_verified",
                "request_identity_verified","binary_linkage_verified",
                "physical_enforcement_verified","proof_verified"):
        if result.get(key) is not False: reject("Unexpected verification or permission")
    if result.get("state")!="proposal_only" or result.get("native_acceptance")!="OPEN":
        reject("Unexpected acceptance state")
    digest=canonical_hash(candidate)
    if result.get("reconciliation_sha256")!=digest or result.get("reconciliation_decision")!=candidate.get("decision"):
        reject("Reconciliation identity mismatch")
    observation=result.get("observation")
    if type(observation) is not dict or observation.get("hash")!="sha256:"+digest or observation.get("source_ref")!="receipt:sha256:"+digest:
        reject("Observation hash mismatch")
    if result.get("stream_contract")!="zkperf-stream/v1" or not isinstance(result.get("windows"),list) or len(result["windows"])!=1:
        reject("Unexpected stream")
    window=result["windows"][0]
    if type(window) is not dict or window.get("observationIds")!=[observation.get("zkperf_observation_id")]:
        reject("Window identity mismatch")
    if window.get("runId")!=observation.get("run_id") or window.get("traceId")!=observation.get("trace_id"):
        reject("Window source mismatch")
    # The bridge receipt is itself candidate-only, not signed or verified.
    return {"schema":1,"state":"proposal_only","native_acceptance":"OPEN",
            "authority":"candidate_only","non_authoritative":True,
            "reconciliation_sha256":digest,"source_bridge":"itir_mcp_safe_call",
            "itir_tool":TOOL,"itir_response":envelope,
            "execution_authorized":False,"publication_authorized":False,
            "same_run_linkage_verified":False,"physical_enforcement_verified":False}


def invoke(candidate, timestamp, interpreter, suite):
    if not (interpreter.is_absolute() and interpreter.is_file() and suite.is_absolute()
            and suite.is_dir()):
        reject("Explicit local interpreter and suite paths required")
    proc=subprocess.run([str(interpreter),"-m","itir_mcp","--bridge"],
                        cwd=suite,input=request(candidate,timestamp),stdout=subprocess.PIPE,
                        stderr=subprocess.DEVNULL,timeout=20,check=False)
    if proc.returncode!=0:reject("ITIR invocation failed")
    return validate(proc.stdout,candidate)


class Tests(unittest.TestCase):
    def candidate(self):
        return {"version":"bushido.extension_ab_reconciliation.v1","decision":"inconclusive"}
    def test_request_fixed_and_utc(self):
        row=self.candidate()
        raw=json.loads(request(row,"2026-11-23T03:41:00Z"))
        self.assertEqual(raw["name"],TOOL)
        self.assertEqual(raw["op"],"safe_call")
        for value in ("2026-02-30T00:00:00Z","2026-11-23","2026-11-23T03:41:00+00:00"):
            with self.assertRaises(ValueError):request(row,value)
    def test_duplicate_and_oversize(self):
        with self.assertRaises(ValueError):read_candidate(b'{"version":"bushido.extension_ab_reconciliation.v1","x":1,"x":2}')
        with self.assertRaises(ValueError):read_candidate(b"x"*(MAX_BYTES+1))
    def test_only_abstained_and_exact_observation(self):
        row=self.candidate();digest=canonical_hash(row)
        obs={"hash":"sha256:"+digest,"source_ref":"receipt:sha256:"+digest,
             "zkperf_observation_id":"o","run_id":"r","trace_id":"t"}
        result={"version":TOOL+".v1","candidate_only":True,"non_authoritative":True,
                "execution_authorized":False,"publication_authorized":False,
                "supplied_input_hashes_verified":False,"files_verified":False,
                "same_run_linkage_verified":False,"request_identity_verified":False,
                "binary_linkage_verified":False,"physical_enforcement_verified":False,
                "proof_verified":False,"state":"proposal_only","native_acceptance":"OPEN",
                "reconciliation_sha256":digest,"reconciliation_decision":"inconclusive",
                "observation":obs,"stream_contract":"zkperf-stream/v1",
                "windows":[{"observationIds":["o"],"runId":"r","traceId":"t"}]}
        envelope={"tool":TOOL,"decision":"abstained","result":result}
        def reply():
            return json.dumps({"ok":True,"result":envelope}).encode()
        self.assertEqual(validate(reply(),row)["native_acceptance"],"OPEN")
        for key,bad in (("execution_authorized",True),("reconciliation_sha256","0"*64)):
            previous=result[key];result[key]=bad
            with self.assertRaises(ValueError):validate(reply(),row)
            result[key]=previous
        envelope["decision"]="verified"
        with self.assertRaises(ValueError):validate(reply(),row)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test",action="store_true")
    parser.add_argument("--reconciliation",type=Path)
    parser.add_argument("--asserted-at")
    parser.add_argument("--python",type=Path)
    parser.add_argument("--suite-root",type=Path)
    parser.add_argument("--output",type=Path)
    args=parser.parse_args()
    if args.self_test:
        return int(not unittest.main(argv=[sys.argv[0]],exit=False).result.wasSuccessful())
    if not all((args.reconciliation,args.asserted_at,args.python,args.suite_root,args.output)):
        parser.error("all five arguments required")
    try:
        with args.reconciliation.open("rb") as f:
            candidate=read_candidate(f.read(MAX_BYTES+1))
        receipt=invoke(candidate,args.asserted_at,args.python,args.suite_root)
        fd=os.open(args.output,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
        with os.fdopen(fd,"w") as f:
            json.dump(receipt,f,sort_keys=True,indent=2);f.write("\n")
        print("Candidate zkPERF bridge receipt written; native acceptance OPEN")
        return 0
    except (OSError,ValueError,TypeError,RecursionError,subprocess.SubprocessError):
        print("ITIR candidate bridge refused; no acceptance inferred",file=sys.stderr)
        return 1


if __name__=="__main__":
    sys.exit(main())
