#!/usr/bin/env python3
"""Join independent live /proc observation to finalized origin + A/B candidate.

Comparison is of *stable named identities*, not the changing live/final
receipt SHA. This is a producer-neutral candidate provenance edge only.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import unittest

LIMIT=65536
HEX32=re.compile(r"[0-9a-f]{32}")
SHA=re.compile(r"[0-9a-f]{64}")
RUN_KEYS=("fixture_run_id","proxy_port")


def fail(message):
    raise ValueError(message)


def read(raw):
    if len(raw)>LIMIT:fail("Oversized input")
    def pairs(items):
        d={}
        for k,v in items:
            if k in d:fail("Duplicate key")
            d[k]=v
        return d
    result=json.loads(raw,object_pairs_hook=pairs,
                      parse_constant=lambda _:fail("Nonfinite JSON"))
    if type(result) is not dict:fail("Expected object")
    return result


def integer(value,lo=0,hi=2**63-1):
    if type(value) is not int or not lo<=value<=hi:fail("Invalid integer")
    return value


def proc_identity(value):
    if type(value) is not dict or set(value)!={"pid","start_ticks","ppid","netns"}:
        fail("Missing live process fields")
    for key in ("pid","start_ticks"):
        integer(value[key],1)
    integer(value["ppid"])
    if not isinstance(value["netns"],str) or not re.fullmatch(r"net:\[\d+\]",value["netns"]):
        fail("Invalid namespace")
    return value


def claimed_identity(value):
    if type(value) is not dict or set(value)!={"pid","start_ticks","netns","listener_inode"}:
        fail("Invalid producer claim")
    integer(value["pid"],1)
    integer(value["start_ticks"],1)
    integer(value["listener_inode"],1)
    if not isinstance(value["netns"],str) or not re.fullmatch(r"net:\[\d+\]",value["netns"]):
        fail("Invalid claimed namespace")
    return value


def bind(reconciliation,origin,witness,origin_sha,witness_sha,reconciliation_sha):
    if reconciliation.get("version")!="bushido.extension_ab_reconciliation.v1":
        fail("Invalid reconciliation")
    if reconciliation.get("state")!="proposal_only" or reconciliation.get("candidate_only") is not True or reconciliation.get("native_acceptance")!="OPEN":
        fail("Unexpected reconciliation authority")
    for key in ("same_run_linkage_verified","request_identity_verified",
                "runtime_binary_linkage_verified","physical_enforcement_verified",
                "execution_authorized","publication_authorized"):
        if reconciliation.get(key) is not False:fail("Unexpected promoted reconciliation")
    runs=reconciliation.get("fixture_run_ids")
    if not isinstance(runs,list) or len(runs)!=2 or not all(
        isinstance(v,str) and HEX32.fullmatch(v) for v in runs) or runs[0]==runs[1]:
        fail("Invalid producer runs")
    if origin.get("fixture")!="fixed_loopback_tls_tunnel_v3" or origin.get("state")!="proposal_only" or origin.get("native_acceptance")!="OPEN":
        fail("Invalid origin producer")
    if origin.get("finalized") is not True:fail("Finalized origin required")
    if witness.get("version")!="bushido.proc_socket_witness.v1" or witness.get("state")!="proposal_only" or witness.get("native_acceptance")!="OPEN":
        fail("Invalid witness")
    if witness.get("non_authoritative") is not True or witness.get("sample_scope")!="live_procfs_snapshot":
        fail("Invalid witness scope")
    for key in ("request_identity_verified","same_run_linkage_verified","transport_payload_verified",
                "execution_authorized","publication_authorized"):
        if witness.get(key) is not False:fail("Unexpected witness authority")
    if origin.get("fixture_run_id")!=runs[1] or witness.get("fixture_run_id")!=runs[1]:
        fail("Distinct run identity required")
    for key in RUN_KEYS:
        if origin.get(key)!=witness.get(key):fail("Producer/witness identity mismatch")
    integer(origin["proxy_port"],1024,65535)
    claim=claimed_identity(origin.get("proxy_process"))
    observed=proc_identity(witness.get("proxy_process"))
    browser=proc_identity(witness.get("browser_process"))
    if any(claim[k]!=observed[k] for k in ("pid","start_ticks","netns")):
        fail("Reused or mismatched proxy process")
    if type(witness.get("listener_owned_and_listening")) is not bool or type(witness.get("browser_socket_endpoint_observed")) is not bool:
        fail("Invalid socket observations")
    if witness["browser_socket_endpoint_observed"] and not witness["listener_owned_and_listening"]:
        fail("Socket observation without listener")
    status=witness.get("status")
    if status not in ("socket_endpoint_observed","no_browser_socket_observed","network_namespace_mismatch"):
        fail("Unknown witness result")
    if (status=="socket_endpoint_observed")!=witness["browser_socket_endpoint_observed"]:
        fail("Contradictory witness result")
    if observed["netns"]!=browser["netns"] and status!="network_namespace_mismatch":
        fail("Network namespace contradictory")
    if observed["netns"]==browser["netns"] and status=="network_namespace_mismatch":
        fail("False namespace mismatch")
    if any(not isinstance(h,str) or not SHA.fullmatch(h) for h in (origin_sha,witness_sha,reconciliation_sha)):
        fail("Invalid source hashes")
    live_sha=witness.get("fixture_receipt_sha256")
    if not isinstance(live_sha,str) or not SHA.fullmatch(live_sha):
        fail("Missing original live snapshot digest")
    # A live mutable snapshot is not byte-equal to finalized origin receipt.
    # The explicit source hashes are retained as separate manifestations.
    return {"version":"bushido.socket_provenance_join.v1",
            "state":"proposal_only","candidate_only":True,"non_authoritative":True,
            "native_acceptance":"OPEN","decision":"socket_endpoint_observed" if witness["browser_socket_endpoint_observed"] else "inconclusive",
            "fixture_run_id":runs[1],"proxy_port":origin["proxy_port"],
            "proxy_pid":observed["pid"],"proxy_start_ticks":observed["start_ticks"],
            "browser_pid":browser["pid"],"browser_start_ticks":browser["start_ticks"],
            "network_namespace":observed["netns"],
            "listener_owned_and_listening":witness["listener_owned_and_listening"],
            "browser_socket_endpoint_observed":witness["browser_socket_endpoint_observed"],
            "source_sha256":{"final_origin":origin_sha,"live_origin_snapshot":live_sha,
                "live_socket_witness":witness_sha,"ab_reconciliation":reconciliation_sha},
            "same_run_linkage_verified":False,"request_identity_verified":False,
            "runtime_binary_linkage_verified":False,"physical_enforcement_verified":False,
            "execution_authorized":False,"publication_authorized":False}


class Tests(unittest.TestCase):
    def sample(self):
        run="a"*32
        claim={"pid":101,"start_ticks":1001,"netns":"net:[42]","listener_inode":333}
        origin={"fixture":"fixed_loopback_tls_tunnel_v3","state":"proposal_only","native_acceptance":"OPEN",
                "fixture_run_id":run,"finalized":True,"proxy_port":9000,"proxy_process":claim}
        rec={"version":"bushido.extension_ab_reconciliation.v1","state":"proposal_only",
             "candidate_only":True,"native_acceptance":"OPEN",
             "fixture_run_ids":["b"*32,run],
             **{k:False for k in ("same_run_linkage_verified","request_identity_verified",
                  "runtime_binary_linkage_verified","physical_enforcement_verified",
                  "execution_authorized","publication_authorized")}}
        proc={"pid":101,"start_ticks":1001,"ppid":1,"netns":"net:[42]"}
        witness={"version":"bushido.proc_socket_witness.v1","state":"proposal_only",
                 "native_acceptance":"OPEN","non_authoritative":True,
                 "sample_scope":"live_procfs_snapshot","fixture_run_id":run,
                 "proxy_port":9000,"proxy_process":proc,
                 "browser_process":{"pid":202,"start_ticks":999,"ppid":1,"netns":"net:[42]"},
                 "listener_owned_and_listening":True,
                 "browser_socket_endpoint_observed":True,"status":"socket_endpoint_observed",
                 "fixture_receipt_sha256":"c"*64,
                 **{k:False for k in ("same_run_linkage_verified","request_identity_verified",
                     "transport_payload_verified","execution_authorized","publication_authorized")}}
        return rec,origin,witness
    def join(self,inputs):
        return bind(*inputs,"d"*64,"e"*64,"f"*64)
    def test_observed_connection_is_not_promotion(self):
        result=self.join(self.sample())
        self.assertEqual(result["decision"],"socket_endpoint_observed")
        self.assertFalse(result["same_run_linkage_verified"])
        self.assertNotEqual(result["source_sha256"]["final_origin"],result["source_sha256"]["live_origin_snapshot"])
    def test_missing_connection_is_inconclusive(self):
        inputs=self.sample()
        inputs[2]["browser_socket_endpoint_observed"]=False
        inputs[2]["status"]="no_browser_socket_observed"
        self.assertEqual(self.join(inputs)["decision"],"inconclusive")
    def test_replay_and_pid_reuse_rejected(self):
        for field,value in (("fixture_run_id","f"*32),("proxy_port",9001)):
            inputs=self.sample()
            inputs[2][field]=value
            with self.assertRaises(ValueError):self.join(inputs)
        inputs=self.sample()
        inputs[2]["proxy_process"]["start_ticks"]=1
        with self.assertRaises(ValueError):self.join(inputs)
    def test_false_authority_rejected(self):
        inputs=self.sample()
        inputs[2]["same_run_linkage_verified"]=True
        with self.assertRaises(ValueError):self.join(inputs)
        inputs=self.sample()
        inputs[1]["finalized"]=False
        with self.assertRaises(ValueError):self.join(inputs)
    def test_duplicate_json_and_boolean_rejected(self):
        with self.assertRaises(ValueError):read(b'{"a":1,"a":2}')
        with self.assertRaises(ValueError):integer(True)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--self-test",action="store_true")
    for name in ("reconciliation","final-origin","live-witness","output"):
        p.add_argument("--"+name,type=Path)
    a=p.parse_args()
    if a.self_test:
        return int(not unittest.main(argv=[sys.argv[0]],exit=False).result.wasSuccessful())
    if not all((a.reconciliation,a.final_origin,a.live_witness,a.output)):
        p.error("reconciliation, finalized origin, live witness, exclusive output required")
    try:
        raws=[]
        for path in (a.reconciliation,a.final_origin,a.live_witness):
            with path.open("rb") as fd:raws.append(fd.read(LIMIT+1))
        payloads=[read(raw) for raw in raws]
        sha=[hashlib.sha256(raw).hexdigest() for raw in raws]
        out=bind(*payloads,sha[1],sha[2],sha[0])
        os.umask(0o077)
        fd=os.open(a.output,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
        with os.fdopen(fd,"w") as target:
            json.dump(out,target,sort_keys=True,indent=2);target.write("\n")
        print(out["decision"]+"; native acceptance OPEN")
        return 0 if out["decision"]=="socket_endpoint_observed" else 2
    except (OSError,ValueError,TypeError,KeyError,RecursionError):
        print("Cannot bind procfs source manifestations; no identity inferred",file=sys.stderr)
        return 1


if __name__=="__main__":
    sys.exit(main())
