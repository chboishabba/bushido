#!/usr/bin/env python3
"""Operator-owned, bounded /proc evidence of a live fixture/socket relationship.

The identity is observation-only: PID startticks defeat simple PID reuse;
socket inode and netns are independent of claimed HTTP receipt port.
No attribution to a particular WebKit request or TLS record is inferred.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import sys
import unittest

MAX_BYTES = 65536
MAX_TASKS = 128
MAX_FDS = 1024
SOCKET = re.compile(r"socket:\[(\d+)\]")
HEX32 = re.compile(r"[0-9a-f]{32}")
PROC = Path("/proc")


def reject(msg):
    raise ValueError(msg)


def strict_json(data):
    if len(data)>MAX_BYTES: reject("Oversized receipt")
    def unique(pairs):
        result={}
        for k,v in pairs:
            if k in result: reject("Duplicate key")
            result[k]=v
        return result
    return json.loads(data,object_pairs_hook=unique,
        parse_constant=lambda _:reject("Nonfinite JSON"))


def stat_fields(raw):
    # /proc/PID/stat comm is parenthesized and may contain spaces/parentheses.
    left,sep,right=raw.rpartition(") ")
    if not sep or "(" not in left: reject("Malformed proc stat")
    fields=right.split()
    if len(fields)<20: reject("Incomplete proc stat")
    return {"state":fields[0],"ppid":int(fields[1]),"start_ticks":int(fields[19])}


def identity(pid,proc=PROC):
    if type(pid) is not int or pid<=1 or pid>2**31-1: reject("Invalid PID")
    base=proc/str(pid)
    before=stat_fields((base/"stat").read_text())
    ns=os.readlink(base/"ns/net")
    after=stat_fields((base/"stat").read_text())
    if before!=after or before["state"] in ("Z","X","x"): reject("Unstable or dead process")
    return dict(pid=pid,start_ticks=before["start_ticks"],ppid=before["ppid"],
                netns=ns)


def socket_inodes(pid,proc=PROC):
    root=proc/str(pid)/"fd"
    entries=list(root.iterdir())
    if len(entries)>MAX_FDS: reject("FD enumeration bound")
    found=set()
    for entry in entries:
        try:
            link=os.readlink(entry)
        except (FileNotFoundError,PermissionError):
            continue
        match=SOCKET.fullmatch(link)
        if match: found.add(int(match[1]))
    return found


def descendant_pids(root,proc=PROC):
    # Bounded procfs snapshot; no arbitrary process command line or env reads.
    tasks=[]
    for entry in proc.iterdir():
        if entry.name.isdecimal():
            if len(tasks)>=16384: reject("System proc enumeration bound")
            tasks.append(int(entry.name))
    parent={}
    for pid in tasks:
        try:
            parent[pid]=stat_fields((proc/str(pid)/"stat").read_text())["ppid"]
        except (OSError,ValueError):
            continue
    selected={root}
    for _ in range(16):
        new={pid for pid,ppid in parent.items() if ppid in selected}
        if len(selected|new)>MAX_TASKS: reject("Descendant process bound")
        if new<=selected: break
        selected|=new
    return sorted(selected)


def tcp_rows(raw,ipv6=False):
    rows=[]
    for line in raw.splitlines()[1:]:
        fields=line.split()
        if len(fields)<10: continue
        try:
            local,remote=fields[1:3]
            laddr,lport=local.split(":")
            raddr,rport=remote.split(":")
            state=int(fields[3],16)
            inode=int(fields[9])
            lp,rp=int(lport,16),int(rport,16)
        except (ValueError,IndexError):
            continue
        # IPv4 loopback exact. IPv6 loopback is handled separately below.
        loopback=("00000000000000000000000001000000" if ipv6 else "0100007F")
        if laddr!=loopback and raddr!=loopback: continue
        rows.append((lp,rp,state,inode))
    return rows


def observe(fixture,browser_pid,proc=PROC):
    if type(fixture) is not dict or fixture.get("fixture")!="fixed_loopback_tls_tunnel_v3":
        reject("Unexpected fixture producer")
    if fixture.get("state")!="proposal_only" or fixture.get("native_acceptance")!="OPEN":
        reject("Authority mismatch")
    run=fixture.get("fixture_run_id")
    port=fixture.get("origin_port")
    proxy_port=fixture.get("proxy_port")
    claimed=fixture.get("proxy_process")
    if not isinstance(run,str) or not HEX32.fullmatch(run):reject("Bad fixture run")
    if type(port) is not int or not 1024<=port<=65535:reject("Bad origin port")
    if type(proxy_port) is not int or not 1024<=proxy_port<=65535:reject("Bad proxy port")
    if not isinstance(claimed,dict) or set(claimed)!={"pid","start_ticks","netns","listener_inode"}:
        reject("Missing proxy identity claim")
    pid=claimed["pid"]
    p0=identity(pid,proc)
    browser0=identity(browser_pid,proc)
    if (p0["start_ticks"]!=claimed["start_ticks"] or p0["netns"]!=claimed["netns"]):
        reject("Proxy PID identity changed")
    listener=claimed["listener_inode"]
    if type(listener) is not int or listener<=0 or listener not in socket_inodes(pid,proc):
        reject("Claimed proxy listener inode not owned")
    if p0["netns"]!=browser0["netns"]:
        return result(run,proxy_port,p0,browser0,False,False,"network_namespace_mismatch")
    raw=(proc/str(pid)/"net/tcp").read_text()
    rows=tcp_rows(raw)
    # Must see the claimed listening socket, bound to the advertised port.
    if (proxy_port,0,10,listener) not in rows:
        reject("Claimed listener is not listening on the advertised port")
    target_inodes=set()
    sampled=descendant_pids(browser_pid,proc)
    for candidate in sampled:
        try:
            if identity(candidate,proc)["netns"]!=p0["netns"]:continue
            target_inodes.update(socket_inodes(candidate,proc))
        except (OSError,ValueError):continue
    # A browser-side ESTABLISHED connection with remote proxy port is
    # evidence of a socket endpoint, not of a particular request.
    connected=any(rp==proxy_port and state==1 and inode in target_inodes
                  for _,rp,state,inode in rows)
    p1=identity(pid,proc)
    browser1=identity(browser_pid,proc)
    if p0!=p1 or browser0!=browser1: reject("Process identity changed during sample")
    status="socket_endpoint_observed" if connected else "no_browser_socket_observed"
    return result(run,proxy_port,p0,browser0,True,connected,status)


def result(run,port,p,b,listener_ok,connected,status):
    return dict(version="bushido.proc_socket_witness.v1",state="proposal_only",
        non_authoritative=True,fixture_run_id=run,proxy_port=port,
        proxy_process=p,browser_process=b,listener_owned_and_listening=listener_ok,
        browser_socket_endpoint_observed=connected,status=status,
        sample_scope="live_procfs_snapshot",request_identity_verified=False,
        same_run_linkage_verified=False,transport_payload_verified=False,
        native_acceptance="OPEN",execution_authorized=False,
        publication_authorized=False)


class Tests(unittest.TestCase):
    def test_stat_comm_parsing(self):
        # pid, (complex comm), state, ppid, then 18 filler fields.
        line="123 (browser) child) S 7 "+("0 "*17)+"44 0 0"
        self.assertEqual(stat_fields(line)["start_ticks"],44)
        self.assertEqual(stat_fields(line)["ppid"],7)
        with self.assertRaises(ValueError):stat_fields("garbage")
    def test_tcp_listener_and_established(self):
        raw="sl local_address rem_address st tx_queue rx_queue tr tm->when retrnsmt uid timeout inode\n"
        raw+="0: 0100007F:3039 00000000:0000 0A 0 0 0 0 0 1234\n"
        raw+="1: 0100007F:AAAA 0100007F:3039 01 0 0 0 0 0 5678\n"
        self.assertIn((12345,0,10,1234),tcp_rows(raw))
        self.assertIn((43690,12345,1,5678),tcp_rows(raw))
    def test_identity_result_is_nonpromoting(self):
        r=result("a"*32,12345,{}, {},True,True,"socket_endpoint_observed")
        self.assertFalse(r["same_run_linkage_verified"])
        self.assertFalse(r["request_identity_verified"])
        self.assertEqual(r["native_acceptance"],"OPEN")
    def test_duplicate_and_nonfinite_rejected(self):
        for raw in (b'{"a":1,"a":2}',b'{"value":NaN}',b"x"*(MAX_BYTES+1)):
            with self.assertRaises(ValueError):strict_json(raw)


def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--self-test",action="store_true")
    ap.add_argument("--fixture-receipt",type=Path)
    ap.add_argument("--browser-pid",type=int)
    ap.add_argument("--output",type=Path)
    a=ap.parse_args()
    if a.self_test:
        return 0 if unittest.main(argv=[sys.argv[0]],exit=False).result.wasSuccessful() else 1
    if not a.fixture_receipt or not a.browser_pid or not a.output:
        ap.error("fixture receipt, live browser PID and exclusive output required")
    try:
        with a.fixture_receipt.open("rb") as source:
            raw=source.read(MAX_BYTES+1)
        fixture=strict_json(raw)
        witness=observe(fixture,a.browser_pid)
        witness["fixture_receipt_sha256"]=hashlib.sha256(raw).hexdigest()
        os.umask(0o077)
        fd=os.open(a.output,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
        with os.fdopen(fd,"w") as output:
            json.dump(witness,output,sort_keys=True,indent=2);output.write("\n")
        print(witness["status"]+"; no network authority",flush=True)
        return 0 if witness["browser_socket_endpoint_observed"] else 2
    except (OSError,ValueError,TypeError,RecursionError):
        print("Unverifiable procfs provenance; no acceptance inferred",file=sys.stderr)
        return 1


if __name__=="__main__":
    sys.exit(main())
