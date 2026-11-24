#!/usr/bin/env python3
"""Fixed local ITIR socket-candidate tool. No arbitrary MCP action or promotion."""
import argparse
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import unittest

TOOL="itir.zkperf.bushido_socket_candidate"
LIMIT=65536
REPLY_LIMIT=262144
UTC=re.compile(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?Z")


def bad(message):
    raise ValueError(message)


def pairs(rows):
    result={}
    for k,v in rows:
        if k in result:bad("Duplicate JSON key")
        result[k]=v
    return result


def parse(raw):
    if len(raw)>LIMIT:bad("Input exceeds bound")
    value=json.loads(raw,object_pairs_hook=pairs,
                     parse_constant=lambda _:bad("Nonfinite JSON"))
    if type(value) is not dict or value.get("version")!="bushido.socket_provenance_join.v1":
        bad("Expected socket provenance join")
    return value


def canonical_sha(value):
    return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(",",":"),
        ensure_ascii=True,allow_nan=False).encode("utf-8")).hexdigest()


def make_request(join,stamp):
    if not isinstance(stamp,str) or not UTC.fullmatch(stamp):
        bad("Expected explicit UTC timestamp")
    try:
        datetime.fromisoformat(stamp.replace("Z","+00:00"))
    except ValueError:
        bad("Invalid UTC timestamp")
    value={"op":"safe_call","name":TOOL,"payload":{"join":join,"asserted_at":stamp}}
    raw=json.dumps(value,sort_keys=True,separators=(",",":"),allow_nan=False).encode()
    if len(raw)>LIMIT:bad("Request exceeds bound")
    return raw+b"\n"


def validate(raw,join):
    if len(raw)>REPLY_LIMIT:bad("Oversized bridge reply")
    data=json.loads(raw,object_pairs_hook=pairs,
                    parse_constant=lambda _:bad("Nonfinite response"))
    if type(data) is not dict or data.get("ok") is not True:bad("Bridge rejected")
    envelope=data.get("result")
    if type(envelope) is not dict or envelope.get("tool")!=TOOL or envelope.get("decision")!="abstained":
        bad("Unexpected MCP authority")
    result=envelope.get("result")
    if type(result) is not dict or result.get("version")!=TOOL+".v1" or result.get("state")!="proposal_only":
        bad("Unexpected result version")
    if result.get("native_acceptance")!="OPEN" or result.get("candidate_only") is not True or result.get("non_authoritative") is not True:
        bad("Unexpected acceptance")
    for k in ("execution_authorized","publication_authorized","supplied_input_hashes_verified",
              "files_verified","same_run_linkage_verified","request_identity_verified",
              "binary_linkage_verified","physical_enforcement_verified","proof_verified"):
        if result.get(k) is not False:bad("Unexpected promotion or verification")
    digest=canonical_sha(join)
    if result.get("join_sha256")!=digest or result.get("reconciliation_decision")!=join.get("decision") or result.get("source_sha256")!=join.get("source_sha256"):
        bad("Join echo mismatch")
    row=result.get("observation")
    if type(row) is not dict or row.get("hash")!="sha256:"+digest or row.get("source_ref")!="receipt:sha256:"+digest:
        bad("Observation digest mismatch")
    if result.get("stream_contract")!="zkperf-stream/v1" or not isinstance(result.get("windows"),list) or len(result["windows"])!=1:
        bad("Unexpected stream")
    window=result["windows"][0]
    if type(window) is not dict or window.get("observationIds")!=[row.get("zkperf_observation_id")] or window.get("runId")!=row.get("run_id") or window.get("traceId")!=row.get("trace_id"):
        bad("Window identity mismatch")
    return {"schema":1,"state":"proposal_only","candidate_only":True,
        "non_authoritative":True,"native_acceptance":"OPEN","join_sha256":digest,
        "source_bridge":"itir_mcp_safe_call","itir_tool":TOOL,
        "itir_response":envelope,"execution_authorized":False,
        "publication_authorized":False,"same_run_linkage_verified":False,
        "request_identity_verified":False,"physical_enforcement_verified":False}


def invoke(join,stamp,python,suite):
    if not python.is_absolute() or not python.is_file() or not suite.is_absolute() or not suite.is_dir():
        bad("Explicit local interpreter and suite required")
    run=subprocess.run([str(python),"-m","itir_mcp","--bridge"],cwd=suite,
        input=make_request(join,stamp),stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,timeout=20,check=False)
    if run.returncode:bad("Local ITIR bridge failed")
    return validate(run.stdout,join)


class Tests(unittest.TestCase):
    def joint(self):
        return {"version":"bushido.socket_provenance_join.v1","decision":"inconclusive",
                "source_sha256":{"final_origin":"b"*64}}
    def test_fixed_request_and_timestamp(self):
        raw=json.loads(make_request(self.joint(),"2026-11-23T03:41:00Z"))
        self.assertEqual(raw["name"],TOOL)
        self.assertEqual(set(raw["payload"]),{"join","asserted_at"})
        for stamp in ("2026-02-30T03:41:00Z","2026-11-23","2026-11-23T03:41:00+00:00"):
            with self.assertRaises(ValueError):make_request(self.joint(),stamp)
    def test_duplicate_and_large_input(self):
        with self.assertRaises(ValueError):parse(b'{"version":"bushido.socket_provenance_join.v1","x":1,"x":2}')
        with self.assertRaises(ValueError):parse(b"z"*(LIMIT+1))
    def test_exact_echo_and_abstention(self):
        join=self.joint()
        digest=canonical_sha(join)
        row={"hash":"sha256:"+digest,"source_ref":"receipt:sha256:"+digest,
             "zkperf_observation_id":"observation","run_id":"run","trace_id":"trace"}
        result={"version":TOOL+".v1","state":"proposal_only","native_acceptance":"OPEN",
          "candidate_only":True,"non_authoritative":True,"join_sha256":digest,
          "source_sha256":join["source_sha256"],"reconciliation_decision":"inconclusive",
          "observation":row,"stream_contract":"zkperf-stream/v1",
          "windows":[{"observationIds":["observation"],"runId":"run","traceId":"trace"}],
          **{k:False for k in ("execution_authorized","publication_authorized",
              "supplied_input_hashes_verified","files_verified","same_run_linkage_verified",
              "request_identity_verified","binary_linkage_verified",
              "physical_enforcement_verified","proof_verified")}}
        envelope={"tool":TOOL,"decision":"abstained","result":result}
        def reply():return json.dumps({"ok":True,"result":envelope}).encode()
        self.assertEqual(validate(reply(),join)["native_acceptance"],"OPEN")
        result["same_run_linkage_verified"]=True
        with self.assertRaises(ValueError):validate(reply(),join)
        result["same_run_linkage_verified"]=False
        envelope["decision"]="verified"
        with self.assertRaises(ValueError):validate(reply(),join)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--self-test",action="store_true")
    p.add_argument("--join",type=Path)
    p.add_argument("--asserted-at")
    p.add_argument("--python",type=Path)
    p.add_argument("--suite-root",type=Path)
    p.add_argument("--output",type=Path)
    a=p.parse_args()
    if a.self_test:
        return int(not unittest.main(argv=[sys.argv[0]],exit=False).result.wasSuccessful())
    if not all((a.join,a.asserted_at,a.python,a.suite_root,a.output)):
        p.error("All five arguments required")
    try:
        with a.join.open("rb") as f:
            join=parse(f.read(LIMIT+1))
        receipt=invoke(join,a.asserted_at,a.python,a.suite_root)
        os.umask(0o077)
        fd=os.open(a.output,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
        with os.fdopen(fd,"w") as output:
            json.dump(receipt,output,sort_keys=True,indent=2);output.write("\n")
        print("Candidate socket provenance window written; native acceptance OPEN")
        return 0
    except (OSError,ValueError,TypeError,subprocess.SubprocessError,RecursionError):
        print("ITIR socket candidate refused; no authority inferred",file=sys.stderr)
        return 1


if __name__=="__main__":
    sys.exit(main())
