#!/usr/bin/env python3
"""Local, observable WebKitGTK request-enforcement fixture.

stdlib-only: python3 scripts/linux/network_fixture.py serve
Then visit http://127.0.0.1:8765/ with HTTPS-only temporarily disabled
in a disposable browser profile, and inspect /__assert and /__events.

This is an acceptance harness, not a filter or a claim of enforcement.
"""
import argparse
import json
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.request import urlopen

BLOCKED = frozenset({
    "/blocked.js",
    "/blocked-pixel",
    "/blocked-fetch",
    "/blocked-frame",
    "/blocked-worker.js",
    "/blocked-redirect",
    "/blocked-redirect-destination",
    "/blocked-nested-script.js",
    "/blocked-worker-import.js",
})
REQUIRED_ALLOWED = frozenset({"/", "/allowed.js", "/allowed-pixel",
                              "/allowed-nested-frame", "/allowed-worker.js",
                              "/allowed-redirect-to-blocked"})
PAGE = """<!doctype html><meta charset="utf-8">
<title>Bushido WebKitGTK network gate fixture</title>
<script src="/allowed.js"></script>
<script src="/blocked.js"></script>
<img src="/allowed-pixel"><img src="/blocked-pixel">
<iframe src="/blocked-frame"></iframe>
<iframe src="/allowed-nested-frame"></iframe>
<script>
fetch('/blocked-fetch').catch(() => {});
try { new Worker('/blocked-worker.js'); } catch (_) {}
try { new Worker('/allowed-worker.js'); } catch (_) {}
fetch('/blocked-redirect').catch(() => {});
fetch('/allowed-redirect-to-blocked').catch(() => {});
</script>
<p>Check /__assert on the fixture server; UI counters do not establish blocking.</p>
"""

class FixtureServer(ThreadingHTTPServer):
    daemon_threads = True
    def __init__(self, address):
        super().__init__(address, Handler)
        self.events = []
        self.lock = threading.Lock()
    def record(self, path):
        with self.lock:
            self.events.append(path)
    def snapshot(self):
        with self.lock:
            return list(self.events)
    def reset(self):
        with self.lock:
            self.events.clear()

class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        return
    def reply(self, status, body, mime="text/plain; charset=utf-8"):
        raw = body.encode("utf-8") if isinstance(body, str) else body
        self.send_response(status)
        self.send_header("Content-Type", mime)
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)
    def do_GET(self):
        path = self.path.split("?", 1)[0]
        if path == "/__events":
            return self.reply(200, json.dumps(self.server.snapshot()), "application/json")
        if path == "/__reset":
            self.server.reset()
            return self.reply(200, "reset\n")
        if path == "/__assert":
            observed = set(self.server.snapshot())
            missing = sorted(REQUIRED_ALLOWED - observed)
            leaked = sorted(BLOCKED & observed)
            outcome = {
                "accepted": not missing and not leaked,
                "missing_allowed": missing,
                "blocked_reached_server": leaked,
                "observed": self.server.snapshot(),
            }
            return self.reply(200 if outcome["accepted"] else 409,
                              json.dumps(outcome), "application/json")
        self.server.record(path)
        if path == "/":
            return self.reply(200, PAGE, "text/html; charset=utf-8")
        if path in ("/blocked-redirect", "/allowed-redirect-to-blocked"):
            self.send_response(302)
            self.send_header("Location", "/blocked-redirect-destination")
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            return
        if path == "/allowed-worker.js":
            return self.reply(200, "importScripts('/blocked-worker-import.js');\n",
                              "text/javascript")
        if path.endswith(".js"):
            return self.reply(200, "window.__bushido_fixture = true;\n",
                              "text/javascript")
        if path == "/blocked-frame":
            return self.reply(200, "<html><body>blocked iframe</body></html>",
                              "text/html")
        if path == "/allowed-nested-frame":
            return self.reply(200,
                              "<html><body>allowed frame<script src='/blocked-nested-script.js'></script></body></html>",
                              "text/html")
        if path.endswith("pixel"):
            return self.reply(200, b"\x89PNG\r\n\x1a\n", "image/png")
        return self.reply(404, "not found")

class FixtureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = FixtureServer(("127.0.0.1", 0))
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.url = "http://127.0.0.1:%d" % cls.server.server_port
    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=5)
    def read(self, path):
        with urlopen(self.url + path) as response:
            return response.status, response.read().decode("utf-8")
    def test_fixture_rejects_vacuous_absence(self):
        self.read("/__reset")
        from urllib.error import HTTPError
        with self.assertRaises(HTTPError) as err:
            self.read("/__assert")
        self.assertEqual(err.exception.code, 409)
        self.assertEqual(json.loads(err.exception.read())["missing_allowed"],
                         sorted(REQUIRED_ALLOWED))
    def test_fixture_records_blocked_request(self):
        self.read("/__reset")
        self.read("/")
        self.read("/allowed.js")
        self.read("/allowed-pixel")
        self.read("/blocked.js")
        from urllib.error import HTTPError
        with self.assertRaises(HTTPError) as err:
            self.read("/__assert")
        data = json.loads(err.exception.read())
        self.assertIn("/blocked.js", data["blocked_reached_server"])
        self.assertFalse(data["missing_allowed"])
    def test_redirect_destination_is_observable(self):
        from urllib.error import HTTPError
        self.read("/__reset")
        for path in REQUIRED_ALLOWED - {"/allowed-redirect-to-blocked"}:
            self.read(path)
        self.read("/allowed-redirect-to-blocked")
        with self.assertRaises(HTTPError) as err:
            self.read("/__assert")
        details = json.loads(err.exception.read())
        self.assertIn("/blocked-redirect-destination", details["blocked_reached_server"])

    def test_nested_worker_and_frame_leaks_are_observable(self):
        from urllib.error import HTTPError
        self.read("/__reset")
        for path in REQUIRED_ALLOWED - {"/allowed-redirect-to-blocked"}:
            self.read(path)
        # Stub the allowed redirect as recorded without following it.
        self.server.record("/allowed-redirect-to-blocked")
        self.read("/blocked-nested-script.js")
        self.read("/blocked-worker-import.js")
        with self.assertRaises(HTTPError) as err:
            self.read("/__assert")
        data = json.loads(err.exception.read())
        self.assertIn("/blocked-nested-script.js", data["blocked_reached_server"])
        self.assertIn("/blocked-worker-import.js", data["blocked_reached_server"])

    def test_fixture_accepts_allowed_only(self):
        self.read("/__reset")
        for path in sorted(REQUIRED_ALLOWED):
            self.read(path)
        status, data = self.read("/__assert")
        self.assertEqual(status, 200)
        self.assertTrue(json.loads(data)["accepted"])

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("serve", "test"))
    parser.add_argument("--port", type=int, default=8765)
    args = parser.parse_args()
    if args.action == "test":
        unittest.main(argv=["network_fixture.py"], exit=False)
    else:
        server = FixtureServer(("127.0.0.1", args.port))
        print(f"Listening on http://127.0.0.1:{server.server_port}/", flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass
        finally:
            server.server_close()

if __name__ == "__main__":
    main()
