#!/usr/bin/env python3
"""Non-forwarding proxy sentinel for isolated Linux WebKit acceptance.

No DNS, upstream sockets, TLS interception, external forwarding, or user data.
Only fixed synthetic routes can produce responses; all other traffic is denied.
"""
import argparse
from collections import Counter
from http.server import BaseHTTPRequestHandler, HTTPServer
import http.client
import json
import os
from pathlib import Path
import tempfile
import threading
import unittest
from urllib.parse import urlsplit

HOST = "bushido-egress.test"
OTHER = "bushido-egress-frame.test"
PAGE = b"""<!doctype html><meta charset=utf-8><title>EGRESS FIXTURE ONLY</title>
<script src="/script.js"></script><img src="/pixel">
<iframe src="http://bushido-egress-frame.test/frame"></iframe>
<script>
fetch('/fetch').catch(()=>{});
fetch('/redirect').catch(()=>{});
try { new Worker('/worker.js'); } catch(_) {}
</script><p>Native proxy synthetic response; not full egress acceptance.</p>
<a href="https://bushido-egress.test/">HTTPS CONNECT negative control</a>"""
ROUTES = {
    (HOST, "/"): ("document", 200, "text/html", PAGE),
    (HOST, "/script.js"): ("script", 200, "text/javascript", b"window.fixtureScript=true;"),
    (HOST, "/pixel"): ("image", 200, "image/svg+xml", b'<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>'),
    (HOST, "/fetch"): ("fetch", 200, "text/plain", b"fixture"),
    (HOST, "/worker.js"): ("worker", 200, "text/javascript", b"importScripts('/worker-import.js');"),
    (HOST, "/worker-import.js"): ("worker_import", 200, "text/javascript", b"self.fixture=true;"),
    (OTHER, "/frame"): ("cross_origin_frame", 200, "text/html", b"<p>Fixture cross-origin frame</p>"),
    (HOST, "/redirect"): ("redirect", 302, "text/plain", b"fixture redirect"),
    (HOST, "/redirect-target"): ("redirect_target", 200, "text/plain", b"fixture redirected"),
}
REQUIRED = sorted(route[0] for route in ROUTES.values())


class Server(HTTPServer):
    request_queue_size = 16
    def __init__(self, address, receipt=None):
        self.counts = Counter()
        self.receipt = receipt
        super().__init__(address, Handler)

    def get_request(self):
        sock, addr = super().get_request()
        sock.settimeout(3)
        return sock, addr

    def record(self, category):
        self.counts[category] += 1
        self.write_receipt()

    def snapshot(self):
        return {"schema": 1, "state": "proposal_only", "fixture": "webkit_proxy_sentinel_v1",
                "counts": dict(self.counts),
                "missing_positive": [k for k in REQUIRED if not self.counts[k]],
                "connect_denied_observed": bool(self.counts["fixture_connect_denied"]),
                "native_acceptance": "OPEN", "upstream_forwarding": False}

    def write_receipt(self):
        if self.receipt is not None:
            tmp = self.receipt.with_suffix(".tmp")
            with tmp.open("w", encoding="utf-8") as f:
                json.dump(self.snapshot(), f, sort_keys=True, indent=2)
                f.write("\n")
            os.replace(tmp, self.receipt)


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.0"
    def log_message(self, *args):
        pass
    def reply(self, status, mime="text/plain", body=b"denied", location=None):
        self.send_response(status)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("Connection", "close")
        if location:
            self.send_header("Location", location)
        self.end_headers()
        self.wfile.write(body)

    def deny(self, category="denied_other"):
        self.server.record(category)
        self.reply(403)

    def do_CONNECT(self):
        self.deny("fixture_connect_denied" if self.path == HOST + ":443" else "denied_other")

    def do_GET(self):
        # Absolute form only, exact fixed origin and path; no cookies,
        # authorization or query strings processed or retained.
        if len(self.path) > 512 or any(ord(c) < 32 for c in self.path):
            return self.deny()
        if self.headers.get_all("Transfer-Encoding") or self.headers.get_all("Content-Length"):
            return self.deny()
        try:
            u = urlsplit(self.path)
            if u.scheme != "http" or u.netloc not in (HOST, OTHER) or u.query or u.fragment:
                return self.deny()
        except ValueError:
            return self.deny()
        if self.path != "http://" + u.netloc + u.path or self.headers.get_all("Host") != [u.netloc]:
            return self.deny()
        route = ROUTES.get((u.netloc, u.path))
        if route is None:
            return self.deny()
        category, status, mime, body = route
        self.server.record(category)
        self.reply(status, mime, body, "/redirect-target" if status == 302 else None)

    do_POST = do_PUT = do_DELETE = do_PATCH = do_HEAD = do_OPTIONS = lambda self: self.deny()


class Tests(unittest.TestCase):
    def setUp(self):
        self.server = Server(("127.0.0.1", 0))
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(5)
    def request(self, method, target, host):
        c = http.client.HTTPConnection("127.0.0.1", self.server.server_port, timeout=3)
        try:
            c.request(method,target,headers={"Host":host})
            r = c.getresponse()
            return r.status,r.read()
        finally:
            c.close()
    def test_positive_routes_nonvacuous(self):
        self.assertEqual(self.server.snapshot()["missing_positive"],REQUIRED)
        for (host,path),(_,status,_,_) in ROUTES.items():
            self.assertEqual(self.request("GET","http://"+host+path,host)[0],status)
        self.assertEqual(self.server.snapshot()["missing_positive"],[])
        self.assertFalse(self.server.snapshot()["connect_denied_observed"])
        self.assertEqual(self.server.snapshot()["native_acceptance"],"OPEN")
    def test_deny_other_and_connect(self):
        for target in ["/","http://example.test/","http://"+HOST+"/?x=1","http://user@"+HOST+"/","http://"+HOST+":80/"]:
            self.assertEqual(self.request("GET",target,HOST)[0],403)
        self.assertEqual(self.request("GET","http://"+HOST+"/",OTHER)[0],403)
        self.assertEqual(self.request("POST","http://"+HOST+"/",HOST)[0],403)
        self.assertEqual(self.request("CONNECT",HOST+":443",HOST)[0],403)
        self.assertTrue(self.server.snapshot()["connect_denied_observed"])
        self.assertEqual(self.server.counts["document"],0)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("action",choices=("serve","test"))
    a=p.parse_args()
    if a.action=="test":
        return int(not unittest.main(argv=[__file__],exit=False).result.wasSuccessful())
    os.umask(0o077)
    root=Path(tempfile.mkdtemp(prefix="bushido-egress-proxy-",dir="/tmp"))
    server=Server(("127.0.0.1",0),root/"receipt.json")
    server.write_receipt()
    print("BUSHIDO_LINUX_EGRESS_FIXTURE_PORT="+str(server.server_port),flush=True)
    print("Private aggregate receipt: "+str(server.receipt),flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
        server.write_receipt()
    return 0


if __name__=="__main__":
    raise SystemExit(main())
