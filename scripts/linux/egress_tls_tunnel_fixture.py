#!/usr/bin/env python3
"""Fixed-loopback CONNECT/TLS fixture; no arbitrary destinations or DNS.

Run in a disposable account/network namespace. The operator supplies a TLS
certificate for bushido-tunnel.test and private key for the synthetic origin.
The proxy does not read, modify or terminate TLS.
"""
import argparse
from collections import Counter
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import select
import socket
import ssl
import tempfile
import threading
import time
import unittest

HOST = "bushido-tunnel.test"
AUTHORITY = HOST + ":443"
MAX_HEADER = 8192
MAX_BYTES_EACH = 2 * 1024 * 1024
MAX_SECONDS = 12


def connect_request(raw):
    # Exact ASCII allowlist with no body, duplicate or ambiguous authority.
    if not raw.endswith(b"\r\n\r\n") or len(raw) > MAX_HEADER:
        return False
    try:
        lines = raw[:-4].decode("ascii").split("\r\n")
    except UnicodeDecodeError:
        return False
    if lines[0] not in ("CONNECT " + AUTHORITY + " HTTP/1.1",
                       "CONNECT " + AUTHORITY + " HTTP/1.0"):
        return False
    hosts = []
    for line in lines[1:]:
        if any(ord(c)<32 or ord(c)>126 for c in line) or ":" not in line:
            return False
        name, value = line.split(":",1)
        if not name or not name.isascii() or not all(c.isalnum() or c=="-" for c in name):
            return False
        name = name.lower()
        if name == "host":
            hosts.append(value.strip())
        elif name not in ("user-agent","proxy-connection","connection","accept"):
            return False
    return hosts == [AUTHORITY]


def read_header(sock, deadline):
    raw = bytearray()
    while len(raw) < MAX_HEADER:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError()
        sock.settimeout(remaining)
        byte = sock.recv(1)  # no overread of first opaque TLS bytes
        if not byte:
            raise EOFError()
        raw += byte
        if raw.endswith(b"\r\n\r\n"):
            return bytes(raw)
    raise ValueError("header bound")


def relay(left, right, deadline):
    """Fixed aggregate byte and absolute time bounds, including half-close."""
    sockets = [left,right]
    total = {left:0,right:0}
    while sockets:
        remaining = deadline-time.monotonic()
        if remaining <= 0:
            return "time_limit",total[left],total[right]
        ready,_,_ = select.select(sockets,[],[],remaining)
        if not ready:
            return "time_limit",total[left],total[right]
        for source in ready:
            target = right if source is left else left
            source.settimeout(max(0.001,deadline-time.monotonic()))
            data = source.recv(min(16384,MAX_BYTES_EACH-total[source]+1))
            if not data:
                sockets.remove(source)
                try:
                    target.shutdown(socket.SHUT_WR)
                except OSError:
                    pass
                continue
            if len(data)+total[source] > MAX_BYTES_EACH:
                return "byte_limit",total[left],total[right]
            remaining = deadline-time.monotonic()
            if remaining <= 0:
                return "time_limit",total[left],total[right]
            target.settimeout(remaining)
            target.sendall(data)
            total[source] += len(data)
    return "eof",total[left],total[right]


# Fixed HTTPS fixture routes; names mirror the native observation categories.
PAGE = b"""<!doctype html><meta charset=utf-8><title>Bushido TLS origin fixture</title>
<script src="/script.js"></script><img src="/pixel"><iframe src="/frame"></iframe>
<script>
fetch('/fetch').catch(()=>{});
fetch('/redirect').catch(()=>{});
try { new Worker('/worker.js'); } catch (_) {}
</script><p>Opaque TLS tunnel positive controls</p>"""
ROUTES = {
    "/": ("document_route",200,"text/html",PAGE),
    "/script.js": ("script_route",200,"text/javascript",b"window.fixtureScript=true;"),
    "/pixel": ("image_route",200,"image/svg+xml",b'<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>'),
    "/fetch": ("fetch_route",200,"text/plain",b"fixture"),
    "/frame": ("frame_route",200,"text/html",b"<p>Fixed same-origin frame</p>"),
    "/worker.js": ("worker_route",200,"text/javascript",b"importScripts('/worker-import.js');"),
    "/worker-import.js": ("worker_import_route",200,"text/javascript",b"self.fixture=true;"),
    "/redirect": ("redirect_route",302,"text/plain",b"fixture redirect"),
    "/redirect-target": ("redirect_target_route",200,"text/plain",b"fixture redirected"),
}
REQUIRED_ROUTES = sorted(route[0] for route in ROUTES.values())


def origin_route(path, hosts):
    if hosts != [HOST]:
        return None
    return ROUTES.get(path)


class OriginHandler(BaseHTTPRequestHandler):
    def log_message(self,*args):
        pass
    def do_GET(self):
        route=origin_route(self.path,self.headers.get_all("Host"))
        if route is None or self.headers.get_all("Transfer-Encoding") or self.headers.get_all("Content-Length"):
            self.send_error(403)
            return
        category,status,mime,body=route
        self.server.record(category)
        self.send_response(status)
        self.send_header("Content-Length",str(len(body)))
        self.send_header("Content-Type",mime)
        self.send_header("Cache-Control","no-store")
        self.send_header("Connection","close")
        if status==302:
            self.send_header("Location","/redirect-target")
        self.end_headers()
        self.wfile.write(body)


class Origin(HTTPServer):
    def __init__(self, context):
        self.context=context
        self.counts=Counter()
        self.count_lock=threading.Lock()
        super().__init__(("127.0.0.1",0),OriginHandler)
    def record(self, category):
        with self.count_lock:
            self.counts[category]+=1
    def snapshot(self):
        with self.count_lock:
            return dict(self.counts)
    def get_request(self):
        sock,addr=super().get_request()
        sock.settimeout(3)
        try:
            return self.context.wrap_socket(sock,server_side=True),addr
        except BaseException:
            sock.close()
            raise


class Proxy:
    def __init__(self, origin_port, receipt=None, origin=None):
        if type(origin_port) is not int or not 1024 <= origin_port <= 65535:
            raise ValueError("explicit loopback origin port required")
        self.destination=("127.0.0.1",origin_port)
        self.origin=origin
        self.receipt=receipt
        self.counts=Counter()
        self.listener=socket.socket(socket.AF_INET,socket.SOCK_STREAM)
        self.listener.bind(("127.0.0.1",0))
        self.listener.listen(8)
        self.listener.settimeout(0.5)
        self.stop=threading.Event()

    def snapshot(self):
        routes=self.origin.snapshot() if self.origin is not None else {}
        return {"schema":1,"fixture":"fixed_loopback_tls_tunnel_v2",
                "state":"proposal_only","native_acceptance":"OPEN",
                "external_forwarding":False,"dns_bound":False,
                "counts":dict(self.counts),
                "origin_http_observed":sum(routes.values()),
                "origin_routes":routes,
                "missing_origin_routes":[k for k in REQUIRED_ROUTES if not routes.get(k)],
                "native_resource_correlation_verified":False}

    def write_receipt(self):
        if self.receipt is not None:
            tmp=self.receipt.with_suffix(".tmp")
            tmp.write_text(json.dumps(self.snapshot(),sort_keys=True,indent=2)+"\n",encoding="utf-8")
            os.replace(tmp,self.receipt)

    def handle(self, client):
        deadline=time.monotonic()+MAX_SECONDS
        try:
            if not connect_request(read_header(client,min(deadline,time.monotonic()+3))):
                self.counts["denied"]+=1
                client.sendall(b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\nContent-Length: 0\r\n\r\n")
                return
            # Fixed numeric endpoint; no name resolution and no caller-selected port.
            with socket.socket(socket.AF_INET,socket.SOCK_STREAM) as target:
                target.settimeout(3)
                try:
                    target.connect(self.destination)
                except OSError:
                    self.counts["origin_unavailable"]+=1
                    client.sendall(b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\nContent-Length: 0\r\n\r\n")
                    return
                if target.getpeername()!=self.destination:
                    self.counts["peer_mismatch"]+=1
                    return
                self.counts["connect_established"]+=1
                client.sendall(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                reason,sent,received=relay(client,target,deadline)
                self.counts["client_to_origin_bytes"]+=sent
                self.counts["origin_to_client_bytes"]+=received
                self.counts["relay_"+reason]+=1
        except (OSError,EOFError,ValueError):
            self.counts["closed_or_error"]+=1
        finally:
            self.write_receipt()

    def serve(self):
        try:
            while not self.stop.is_set():
                try:
                    client,_=self.listener.accept()
                except TimeoutError:
                    continue
                with client:
                    self.handle(client)
        finally:
            self.listener.close()


def read_exact(sock, count):
    out = bytearray()
    while len(out) < count:
        part = sock.recv(count-len(out))
        if not part:
            raise EOFError()
        out.extend(part)
    return bytes(out)


class Tests(unittest.TestCase):
    def test_origin_route_allowlist(self):
        self.assertEqual(len(REQUIRED_ROUTES),9)
        for path,route in ROUTES.items():
            self.assertEqual(origin_route(path,[HOST]),route)
        for path in ('/?private=1','/unknown','http://'+HOST+'/','/redirect-target?x=1'):
            self.assertIsNone(origin_route(path,[HOST]))
        self.assertIsNone(origin_route('/',[HOST,HOST]))
        self.assertIsNone(origin_route('/',['other.test']))
        self.assertEqual(origin_route('/redirect',[HOST])[1],302)
        self.assertEqual(origin_route('/redirect-target',[HOST])[1],200)

    def test_exact_authority(self):
        valid=("CONNECT "+AUTHORITY+" HTTP/1.1\r\nHost: "+AUTHORITY+"\r\n\r\n").encode()
        self.assertTrue(connect_request(valid))
        for raw in (valid.replace(b":443",b":444"),valid.replace(b"Host:",b"Other:"),
                    valid.replace(b"\r\n\r\n",b"\r\nHost: "+AUTHORITY.encode()+b"\r\n\r\n"),
                    valid.replace(b"\r\n\r\n",b"\r\nContent-Length: 1\r\n\r\n"),
                    valid.replace(HOST.encode(),b"127.0.0.1")):
            self.assertFalse(connect_request(raw))
    def test_opaque_relay(self):
        a,b=socket.socketpair()
        c,d=socket.socketpair()
        result=[]
        def run():
            with b,c:
                result.append(relay(b,c,time.monotonic()+3))
        thread=threading.Thread(target=run)
        thread.start()
        with a,d:
            a.settimeout(2);d.settimeout(2)
            a.sendall(b"\x16\x03\x01fixture")
            self.assertEqual(read_exact(d,10),b"\x16\x03\x01fixture")
            d.sendall(b"opaque-reply")
            self.assertEqual(read_exact(a,12),b"opaque-reply")
            a.shutdown(socket.SHUT_WR);d.shutdown(socket.SHUT_WR)
        thread.join(4)
        self.assertEqual(result,[("eof",10,12)])
    def test_denied_route_no_origin_connection(self):
        proxy=Proxy(65534)
        thread=threading.Thread(target=proxy.serve)
        thread.start()
        try:
            with socket.create_connection(("127.0.0.1",proxy.listener.getsockname()[1]),timeout=2) as s:
                s.sendall(b"CONNECT other.test:443 HTTP/1.1\r\nHost: other.test:443\r\n\r\n")
                self.assertIn(b"403",s.recv(1024))
        finally:
            proxy.stop.set();thread.join(4)
        self.assertEqual(proxy.counts["denied"],1)
        self.assertEqual(proxy.counts["connect_established"],0)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("action",choices=("serve","test"))
    p.add_argument("--cert",type=Path)
    p.add_argument("--key",type=Path)
    a=p.parse_args()
    if a.action=="test":
        return int(not unittest.main(argv=[__file__],exit=False).result.wasSuccessful())
    if not a.cert or not a.key:
        p.error("operator-provided synthetic certificate and key required")
    os.umask(0o077)
    ctx=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.minimum_version=ssl.TLSVersion.TLSv1_2
    ctx.load_cert_chain(a.cert,a.key)
    origin=Origin(ctx)
    root=Path(tempfile.mkdtemp(prefix="bushido-tls-tunnel-",dir="/tmp"))
    proxy=Proxy(origin.server_port,root/"receipt.json",origin)
    worker=threading.Thread(target=origin.serve_forever,daemon=True)
    worker.start()
    proxy.write_receipt()
    print("BUSHIDO_LINUX_EGRESS_FIXTURE_PORT="+str(proxy.listener.getsockname()[1]),flush=True)
    print("BUSHIDO_LINUX_SCOPE_ORIGIN_PORT="+str(origin.server_port),flush=True)
    print("Private receipt: "+str(proxy.receipt),flush=True)
    try:
        proxy.serve()
    except KeyboardInterrupt:
        pass
    finally:
        origin.shutdown();origin.server_close();worker.join(5)
        proxy.write_receipt()
    return 0


if __name__=="__main__":
    raise SystemExit(main())
