#!/usr/bin/env python3
"""Dev-only conservative ABP ||hostname^ -> WebKit content-filter subset.

Not a full EasyList/ABP converter: unsupported rules are omitted, exact-host
exceptions suppress that host, and omissions are counted explicitly.
"""
import argparse
import json
import re
import sys
import unittest
from pathlib import Path

LABEL = r"[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?"
HOST = re.compile(LABEL + r"(?:\." + LABEL + r")+", re.ASCII)
RULE = re.compile(r"(@@)?\|\|([a-zA-Z0-9.-]+)\^", re.ASCII)

# A native unconditional hostname block can overrule an ABP exception.
# Recognize host-scoped @@ rules with or without options.
EXCEPTION_HOST = re.compile(r"^@@\|\|([a-zA-Z0-9.-]+)\^", re.ASCII)

def related_hosts(block, exception):
    return block == exception or block.endswith("." + exception) or exception.endswith("." + block)

def exception_host(line):
    m = EXCEPTION_HOST.match(line.strip())
    if not m:
        return None
    host = m.group(1).lower()
    return host if HOST.fullmatch(host) else None

def parse_line(line):
    m = RULE.fullmatch(line.strip())
    if not m:
        return None
    host = m.group(2).lower()
    if not HOST.fullmatch(host) or host.endswith((".local", ".localhost", ".test", ".invalid", ".example")):
        return None
    return ("exception" if m.group(1) else "block", host)

def regex_for_host(host):
    return r"^https?://([a-z0-9-]+\.)*" + re.escape(host) + r"([:/?#]|$)"

def compile_rules(texts, limit=2048):
    blocks, exceptions = set(), set()
    stats = dict(lines=0, unsupported=0, duplicates=0, exceptions=0,
                 conditional_exceptions=0, unscoped_exceptions=0,
                 excluded_by_exception=0, truncated=0, emitted=0)
    for text in texts:
        for line in text.splitlines():
            stats["lines"] += 1
            stripped = line.strip()
            if stripped.startswith("@@"):
                host = exception_host(stripped)
                if host is None:
                    stats["unscoped_exceptions"] += 1
                else:
                    exceptions.add(host)
                    stats["exceptions"] += 1
                    if parse_line(stripped) is None:
                        stats["conditional_exceptions"] += 1
                continue
            parsed = parse_line(stripped)
            if parsed is None:
                stats["unsupported"] += 1
                continue
            _, host = parsed
            if host in blocks:
                stats["duplicates"] += 1
            else:
                blocks.add(host)
    # Unknown exception scope could override any block. Do not emit a
    # potentially overblocking rule set from incomplete exception evidence.
    if stats["unscoped_exceptions"]:
        stats["error"] = "Unscoped ABP exception; unsafe conversion"
        return [], stats
    eligible = sorted(host for host in blocks
                      if not any(related_hosts(host, exc) for exc in exceptions))
    stats["excluded_by_exception"] = len(blocks)-len(eligible)
    stats["truncated"] = max(0, len(eligible)-limit)
    selected = eligible[:limit]
    stats["emitted"] = len(selected)
    rules = [{"trigger":{"url-filter":regex_for_host(h)},
              "action":{"type":"block"}} for h in selected]
    return rules, stats

class ConverterTests(unittest.TestCase):
    def test_accepted_and_rejected_syntax(self):
        self.assertEqual(parse_line("||Ads.Example.NET^"), ("block", "ads.example.net"))
        for unsupported in ("||ads.example.net^$third-party", "||ads.example.net/path^",
                            "||*.example.net^", "example.net##.ad", "||a..net^", "||localhost^"):
            self.assertIsNone(parse_line(unsupported))
    def test_exceptions_cross_lists(self):
        rules, stats = compile_rules(["||ads.example.net^\n||tracker.example.org^",
                                       "@@||ads.example.net^"])
        self.assertEqual(len(rules), 1)
        self.assertIn("tracker", rules[0]["trigger"]["url-filter"])
        self.assertEqual(stats["excluded_by_exception"], 1)
    def test_subdomain_exception_suppresses_parent_block(self):
        rules, audit = compile_rules([
            "||example.net^\n||independent.org^",
            "@@||login.example.net^$script"
        ])
        self.assertEqual(len(rules), 1)
        self.assertIn("independent", rules[0]["trigger"]["url-filter"])
        self.assertEqual(audit["excluded_by_exception"], 1)
        self.assertEqual(audit["conditional_exceptions"], 1)

    def test_parent_exception_suppresses_child_block(self):
        rules, audit = compile_rules([
            "||ads.example.net^\n||independent.org^",
            "@@||example.net^$document"
        ])
        self.assertEqual(len(rules), 1)
        self.assertEqual(audit["excluded_by_exception"], 1)

    def test_unscoped_exception_prevents_output(self):
        rules, audit = compile_rules(["||ads.example.net^", "@@/ad[sx]/"])
        self.assertEqual(rules, [])
        self.assertEqual(audit["unscoped_exceptions"], 1)
        self.assertIn("error", audit)

    def test_unrelated_exception_does_not_remove_rule(self):
        rules, audit = compile_rules(["||ads.example.net^",
                                      "@@||videos.other.org^$media"])
        self.assertEqual(len(rules), 1)
        self.assertEqual(audit["excluded_by_exception"], 0)

    def test_dns_boundaries(self):
        pattern = regex_for_host("ads.example.net")
        self.assertIsNotNone(re.search(pattern, "https://cdn.ads.example.net/a.js"))
        self.assertIsNotNone(re.search(pattern, "http://ads.example.net:8080/a"))
        for url in ("https://notads.example.net/", "https://ads.example.net.evil/",
                    "ftp://ads.example.net/"):
            self.assertIsNone(re.search(pattern, url))
    def test_stable_bounded_output(self):
        a, stats = compile_rules(["||z.example.net^\n||a.example.net^\n||z.example.net^"], 1)
        b, _ = compile_rules(["||a.example.net^\n||z.example.net^"], 1)
        self.assertEqual(a, b)
        self.assertEqual(stats["duplicates"], 1)
        self.assertEqual(stats["truncated"], 1)
    def test_unsupported_never_emitted(self):
        rules, stats = compile_rules(["||ads.example.net^$image\n@@||ads.example.net^$document\n"])
        self.assertEqual(rules, [])
        self.assertEqual(stats["unsupported"], 1)
        self.assertEqual(stats["conditional_exceptions"], 1)

def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("sources", nargs="*", type=Path)
    ap.add_argument("--output", type=Path)
    ap.add_argument("--max-rules", type=int, default=2048)
    ap.add_argument("--test", action="store_true")
    args = ap.parse_args()
    if args.test:
        result = unittest.main(argv=[sys.argv[0]], exit=False)
        return int(not result.result.wasSuccessful())
    if not args.sources or args.output is None:
        ap.error("provide ABP source files and --output")
    if not 1 <= args.max_rules <= 8192:
        ap.error("--max-rules must be between 1 and 8192")
    rules, stats = compile_rules([p.read_text(encoding="utf-8") for p in args.sources], args.max_rules)
    if stats["unscoped_exceptions"]:
        print(json.dumps(stats, sort_keys=True), file=sys.stderr)
        ap.error("Unscoped ABP exceptions: refusing potentially overblocking output")
    if not rules:
        ap.error("no safe subset rules: refusing empty native filter")
    args.output.write_text(json.dumps(rules, separators=(",", ":"))+"\n", encoding="utf-8")
    print(json.dumps({"sources":list(map(str,args.sources)), **stats}, sort_keys=True), file=sys.stderr)
    print("WARNING: restricted ABP hostname subset; not full filter parity", file=sys.stderr)
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
