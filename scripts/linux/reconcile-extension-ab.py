#!/usr/bin/env python3
"""Strict comparison of operator-collected synthetic A/B evidence.
No network, execution grant, attestation, or native PASS promotion.
"""
import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import unittest

MAX_BYTES=65536
ROUTES=("extension_allow_route","extension_deny_route",
        "extension_redirect_route","extension_redirect_target_route")
TABLE=dict(zip(ROUTES,"ADAD"))
HEX32=re.compile(r"[0-9a-f]{32}")
POLICY=re.compile(r"bushido\.fixture\.policy\.v2:[0-9a-f]{32}:[ADX]{4}")


def require(condition):
    if not condition:
        raise ValueError("invalid or inconsistent receipt contract")


def unique(pairs):
    result={}
    for key,value in pairs:
        require(key not in result)
        result[key]=value
    return result


def load(raw):
    require(len(raw)<=MAX_BYTES)
    def invalid(_):
        raise ValueError("nonfinite JSON")
    value=json.loads(raw,object_pairs_hook=unique,parse_constant=invalid)
    require(type(value) is dict)
    return value


def count(x):
    require(type(x) is int and 0<=x<=2**64-1)
    return x


def port(x):
    require(type(x) is int and 1024<=x<=65535)
    return x


def origin(value):
    require(type(value.get("schema")) is int and value.get("schema")==1)
    require(value.get("fixture")=="fixed_loopback_tls_tunnel_v3")
    require(value.get("state")=="proposal_only" and value.get("native_acceptance")=="OPEN")
    for k in ("external_forwarding","dns_bound","extension_enforcement_verified",
              "native_resource_correlation_verified"):
        require(value.get(k) is False)
    identity=value.get("fixture_run_id")
    require(isinstance(identity,str) and HEX32.fullmatch(identity))
    require(type(value.get("finalized")) is bool)
    endpoint=port(value.get("origin_port"))
    all_routes=value.get("origin_routes")
    controls=value.get("extension_control_routes")
    require(type(all_routes) is dict and type(controls) is dict and set(controls)==set(ROUTES))
    require(len(all_routes)<=13)
    require(sum(count(v) for v in all_routes.values())==count(value.get("origin_http_observed")))
    require(all(count(controls[k])==all_routes.get(k,0) for k in ROUTES))
    return identity,endpoint,controls,value["finalized"]


def preview(value, enabled):
    require(value.get("version")=="bushido.egress.scope_preview.v9")
    require(value.get("state")=="proposal_only" and value.get("native_acceptance")=="OPEN")
    for k in ("non_authoritative","native_owner_matches","operator_scope_matches",
              "original_document_policy_allows","fixed_loopback_endpoint","native_load_epoch_observed"):
        require(value.get(k) is True)
    for k in ("extension_policy_delivery_verified","document_epoch_verified",
              "request_attribution_verified","execution_authorized"):
        require(value.get(k) is False)
    require(value.get("fixed_dns_name")=="bushido-tunnel.test.")
    endpoint=port(value.get("fixture_origin_port"))
    ext=value.get("extension_observation")
    ack=value.get("extension_policy_ack")
    require(type(ext) is dict and ext.get("configured") is enabled and type(ack) is dict)
    policy=value.get("extension_policy_snapshot")
    if not enabled:
        require(policy is None and ack.get("status")=="not_configured" and ext.get("observation") is None)
        return endpoint,None,None
    require(isinstance(policy,str) and POLICY.fullmatch(policy))
    return endpoint,policy,ack


def reconcile(baseline,enabled,baseline_preview,enabled_preview):
    ai,ap,a,af=origin(baseline)
    bi,bp,b,bf=origin(enabled)
    require(ai!=bi)
    a_port,_,_=preview(baseline_preview,False)
    b_port,policy,ack=preview(enabled_preview,True)
    require(a_port==ap and b_port==bp)
    reasons=[]
    contradictions=[]
    if not af or not bf:
        reasons.append("origin_receipt_not_finalized")
    if any(not a[k] for k in ROUTES):
        reasons.append("baseline_positive_missing")
    if not b[ROUTES[0]] or not b[ROUTES[2]]:
        reasons.append("enabled_positive_missing")
    if b[ROUTES[1]] or b[ROUTES[3]]:
        contradictions.append("denied_route_arrived")
    if not policy.endswith(":ADAD"):
        reasons.append("expected_policy_not_projected")
    native=Counter()
    flags=Counter()
    if ack.get("status")!="matching_reply_observed":
        reasons.append("matching_native_reply_missing")
    else:
        require(ack.get("matching_reply_observed") is True and ack.get("document_observation_rechecked") is True)
        require(ack.get("runtime_binary_linkage_verified") is False and ack.get("applied_request_policy_verified") is False)
        trace=ack.get("applied_policy_observation")
        require(type(trace) is dict and trace.get("scope")=="reported_native_page_lifetime")
        require(trace.get("policy_snapshot")==policy)
        identity=trace.get("page_instance")
        require(isinstance(identity,str) and HEX32.fullmatch(identity))
        require(trace.get("document_epoch_linkage_verified") is False and trace.get("physical_dispatch_verified") is False)
        events=trace.get("events")
        omitted=count(trace.get("omitted"))
        require(type(events) is list and len(events)<=128)
        require(omitted==0 or len(events)==128)
        require(trace.get("reported_prefix_matches_table") is True)
        require(trace.get("reported_prefix_nonempty") is bool(events))
        require(trace.get("reported_prefix_untruncated") is (omitted==0))
        table=dict(zip(ROUTES,policy.rsplit(":",1)[1]))
        for i,event in enumerate(events,1):
            require(type(event) is dict and set(event)=={"ordinal","route","decision","previous_response_present"})
            require(type(event["ordinal"]) is int and event["ordinal"]==i)
            route=event["route"]
            require(isinstance(route,str) and route in table and event["decision"]==table[route])
            require(type(event["previous_response_present"]) is bool)
            native[route]+=1
            flags[(route,event["previous_response_present"])]+=1
        if omitted:
            reasons.append("native_prefix_truncated")
        if any(not native[k] for k in ROUTES) or not flags[(ROUTES[3],True)]:
            reasons.append("native_control_missing")
    # Separate aggregate surfaces. No assumption of equal counts or a join
    # across page lifetime, origin process, document or physical request.
    decision="contradiction_observed" if contradictions else "inconclusive" if reasons else "consistent_controls_observed"
    return {"version":"bushido.extension_ab_reconciliation.v1","state":"proposal_only",
            "non_authoritative":True,"candidate_only":True,"native_acceptance":"OPEN",
            "decision":decision,"missing_controls":reasons,"contradictions":contradictions,
            "baseline_origin":a,"enabled_origin":b,"native_reported_counts":dict(native),
            "fixture_run_ids":[ai,bi],"execution_authorized":False,"publication_authorized":False,
            "same_run_linkage_verified":False,"request_identity_verified":False,
            "runtime_binary_linkage_verified":False,"physical_enforcement_verified":False}


class Tests(unittest.TestCase):
    def inputs(self):
        def origin_fixture(identity,endpoint,counts):
            return dict(schema=1,fixture="fixed_loopback_tls_tunnel_v3",state="proposal_only",
                        native_acceptance="OPEN",external_forwarding=False,dns_bound=False,
                        extension_enforcement_verified=False,native_resource_correlation_verified=False,
                        fixture_run_id=identity,origin_port=endpoint,finalized=True,
                        origin_routes=counts.copy(),extension_control_routes=counts.copy(),
                        origin_http_observed=sum(counts.values()))
        a=origin_fixture("a"*32,8765,dict.fromkeys(ROUTES,1))
        b=origin_fixture("b"*32,8766,dict(zip(ROUTES,[1,0,1,0])))
        def view(enabled,endpoint):
            return dict(version="bushido.egress.scope_preview.v9",state="proposal_only",
                        native_acceptance="OPEN",non_authoritative=True,native_owner_matches=True,
                        operator_scope_matches=True,original_document_policy_allows=True,
                        fixed_loopback_endpoint=True,native_load_epoch_observed=True,
                        extension_policy_delivery_verified=False,document_epoch_verified=False,
                        request_attribution_verified=False,execution_authorized=False,
                        fixed_dns_name="bushido-tunnel.test.",fixture_origin_port=endpoint,
                        extension_observation={"configured":enabled,"observation":{} if enabled else None},
                        extension_policy_snapshot=None,extension_policy_ack={"status":"not_configured"})
        av=view(False,8765)
        bv=view(True,8766)
        policy="bushido.fixture.policy.v2:"+"c"*32+":ADAD"
        bv["extension_policy_snapshot"]=policy
        events=[dict(ordinal=i,route=k,decision=TABLE[k],previous_response_present=i==4)
                for i,k in enumerate(ROUTES,1)]
        bv["extension_policy_ack"]=dict(status="matching_reply_observed",matching_reply_observed=True,
          document_observation_rechecked=True,runtime_binary_linkage_verified=False,
          applied_request_policy_verified=False,applied_policy_observation=dict(
          scope="reported_native_page_lifetime",policy_snapshot=policy,page_instance="d"*32,
          document_epoch_linkage_verified=False,physical_dispatch_verified=False,events=events,
          omitted=0,reported_prefix_matches_table=True,reported_prefix_nonempty=True,
          reported_prefix_untruncated=True))
        return a,b,av,bv
    def test_consistency_not_authority(self):
        r=reconcile(*self.inputs())
        self.assertEqual(r["decision"],"consistent_controls_observed")
        self.assertIs(r["physical_enforcement_verified"],False)
        self.assertEqual(r["native_acceptance"],"OPEN")
    def test_missing_and_denied_arrival(self):
        a,b,av,bv=self.inputs()
        a["finalized"]=False
        self.assertEqual(reconcile(a,b,av,bv)["decision"],"inconclusive")
        b["origin_routes"][ROUTES[1]]=b["extension_control_routes"][ROUTES[1]]=1
        b["origin_http_observed"]+=1
        self.assertEqual(reconcile(a,b,av,bv)["decision"],"contradiction_observed")
    def test_wrong_port_reused_run_wrong_event(self):
        a,b,av,bv=self.inputs()
        for key,bad in (("fixture_origin_port",9999),):
            original=bv[key];bv[key]=bad
            with self.assertRaises(ValueError): reconcile(a,b,av,bv)
            bv[key]=original
        b["fixture_run_id"]=a["fixture_run_id"]
        with self.assertRaises(ValueError): reconcile(a,b,av,bv)
        b["fixture_run_id"]="b"*32
        bv["extension_policy_ack"]["applied_policy_observation"]["events"][1]["decision"]="A"
        with self.assertRaises(ValueError): reconcile(a,b,av,bv)
    def test_duplicate_json_and_boolean_count(self):
        with self.assertRaises(ValueError): load(b'{"a":1,"a":2}')
        with self.assertRaises(ValueError): count(True)
    def test_empty_prefix_and_missing_reply(self):
        a,b,av,bv=self.inputs()
        ack=bv["extension_policy_ack"]
        trace=ack["applied_policy_observation"]
        trace["events"]=[];trace["reported_prefix_nonempty"]=False
        self.assertEqual(reconcile(a,b,av,bv)["decision"],"inconclusive")
        bv["extension_policy_ack"]={"status":"no_reply"}
        self.assertEqual(reconcile(a,b,av,bv)["decision"],"inconclusive")


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--self-test",action="store_true")
    for k in ("baseline-origin","enabled-origin","baseline-preview","enabled-preview","output"):
        p.add_argument("--"+k,type=Path)
    a=p.parse_args()
    if a.self_test:
        return int(not unittest.main(argv=[sys.argv[0]],exit=False).result.wasSuccessful())
    paths=[a.baseline_origin,a.enabled_origin,a.baseline_preview,a.enabled_preview]
    if not all(paths) or not a.output:
        p.error("four input receipts and exclusive output required")
    try:
        raws=[]
        for path in paths:
            with path.open("rb") as f: raws.append(f.read(MAX_BYTES+1))
        receipt=reconcile(*(load(raw) for raw in raws))
        receipt["input_sha256"]={k:hashlib.sha256(raw).hexdigest() for k,raw in zip(
            ("baseline_origin","enabled_origin","baseline_preview","enabled_preview"),raws)}
        fd=os.open(a.output,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
        with os.fdopen(fd,"w") as f:
            json.dump(receipt,f,sort_keys=True,indent=2);f.write("\n")
        print(receipt["decision"]+"; native acceptance OPEN")
        return {"consistent_controls_observed":0,"contradiction_observed":1,"inconclusive":2}[receipt["decision"]]
    except (OSError,ValueError,TypeError,RecursionError,KeyError):
        print("Invalid receipt or output; no acceptance inferred",file=sys.stderr)
        return 1


if __name__=="__main__":
    sys.exit(main())
