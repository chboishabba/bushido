"""Operator-local Bushido MCP. No browser rebuild, no arbitrary shell or remote listener."""
import argparse
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time
from mcp.server.fastmcp import FastMCP

mcp = FastMCP("bushido-local")
ROOT = Path(__file__).resolve().parent
CONFIG = None

def identity():
    p = Path("/proc") / str(CONFIG.pid)
    exe = (p / "exe").resolve(strict=True)
    if exe.name != "bushido-browser" or p.stat().st_uid != os.getuid():
        raise ValueError("Expected same-UID Bushido process")
    start = (p / "stat").read_text().rsplit(")", 1)[1].split()[19]
    return {"pid": CONFIG.pid, "start_ticks": start, "exe": str(exe)}

def run(argv, timeout=20):
    return subprocess.run(argv, check=True, timeout=timeout,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True).stdout.strip()

def trace():
    with CONFIG.log.open("rb") as f:
        f.seek(0, 2)
        size = f.tell()
        f.seek(max(0, size - 262144))
        data = f.read(262144).decode("utf-8", errors="replace")
    lines = [s[:2048] for s in data.splitlines()
             if s.startswith(("BUSHIDO_LAYOUT ", "BUSHIDO_GEOMETRY "))]
    return lines[-120:]

def x11_window():
    if os.environ.get("XDG_SESSION_TYPE") != "x11" or not shutil.which("xdotool"):
        raise ValueError("Verified X11 action backend unavailable; Wayland actions not implemented")
    w = run(["xdotool", "getactivewindow"])
    if not w.isdecimal() or run(["xdotool", "getwindowpid", w]) != str(CONFIG.pid):
        raise ValueError("Active window is not the selected Bushido process")
    return w

@mcp.tool()
def capabilities() -> dict:
    """Report actual local backend availability, not inferred attachment."""
    return {"process": identity(), "session": os.environ.get("XDG_SESSION_TYPE", "unknown"),
            "geometry_trace": CONFIG.log.is_file(),
            "window_capture": bool(shutil.which("spectacle") and CONFIG.allow_capture),
            "window_identity_verified": False,
            "x11_actions": bool(CONFIG.allow_actions and os.environ.get("XDG_SESSION_TYPE") == "x11" and shutil.which("xdotool")),
            "wayland_actions": False, "page_snapshot": False, "live_gtk_inspection": False}

@mcp.tool()
def geometry_trace() -> dict:
    """Read bounded existing instrumentation; no new browser build or live GTK query."""
    before = identity()
    lines = trace()
    if identity() != before:
        raise ValueError("Process changed")
    return {"process": before, "lines": lines, "source": "operator-selected log; process linkage not attested"}

@mcp.tool()
def inspect_geometry() -> dict:
    """Summarize observed trace boundaries; absence of dispatch is not proof of rejection."""
    receipt = geometry_trace()
    lines = receipt["lines"]
    receipt["last_request"] = next((s for s in reversed(lines) if s.startswith("BUSHIDO_LAYOUT sequence=")), None)
    receipt["last_dispatch"] = next((s for s in reversed(lines) if s.startswith("BUSHIDO_LAYOUT dispatch ")), None)
    receipt["last_allocation"] = next((s for s in reversed(lines) if s.startswith("BUSHIDO_GEOMETRY ") and "gtk-size-allocate" in s), None)
    receipt["status"] = "unjoined observations; inspect sequence and view identity before attributing cause"
    return receipt

@mcp.tool()
def window_capture() -> dict:
    """Explicit active-window screenshot. Operator must focus Bushido during countdown."""
    if not CONFIG.allow_capture:
        raise ValueError("Restart sidecar with --allow-capture after operator consent")
    before = identity()
    out = Path(tempfile.mkdtemp(prefix="capture-", dir=CONFIG.output)) / "window"
    run(["bash", str(ROOT / "geometry-capture.sh"), str(out)], 25)
    png = out / "window.png"
    if identity() != before:
        raise ValueError("Process changed during capture")
    if png.stat().st_size > 64 * 1024 * 1024:
        raise ValueError("Capture too large")
    data = png.read_bytes()
    if not data.startswith(b"\x89PNG\r\n\x1a\n"):
        raise ValueError("Not a PNG")
    return {"path": str(png), "sha256": hashlib.sha256(data).hexdigest(),
            "process": before, "window_identity_verified": False,
            "capture_type": "compositor active window; operator verification required"}

@mcp.tool()
def page_snapshot() -> dict:
    """Report the unsupported internal snapshot attachment explicitly."""
    return {"status": "unsupported", "reason": "No trusted native child snapshot endpoint attached; do not substitute compositor capture"}

@mcp.tool()
def action_batch(actions: list[str]) -> dict:
    """Bounded X11-only navigation keys for an explicitly selected, focused Bushido."""
    if not CONFIG.allow_actions:
        raise ValueError("Actions require explicit startup opt-in")
    allowed = {"reload": "ctrl+r", "back": "alt+Left", "forward": "alt+Right", "escape": "Escape"}
    if not 1 <= len(actions) <= 8 or any(a not in allowed for a in actions):
        raise ValueError("Unsupported action batch")
    before = identity()
    receipts = []
    for a in actions:
        if identity() != before:
            raise ValueError("Process changed")
        w = x11_window()
        run(["xdotool", "key", "--window", w, allowed[a]], 5)
        receipts.append({"action": a, "status": "sent; effect unverified"})
    return {"steps": receipts, "process": before}

@mcp.tool()
def run_geometry_scenario(capture: bool = False) -> dict:
    """Collect an observation packet from existing running instance; no implicit navigation."""
    before = inspect_geometry()
    shot = window_capture() if capture else None
    after = inspect_geometry()
    return {"before": before, "capture": shot, "after": after,
            "status": "observation only; not a verified replay"}

def main():
    global CONFIG
    p = argparse.ArgumentParser()
    p.add_argument("--pid", type=int, required=True)
    p.add_argument("--log", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--allow-capture", action="store_true")
    p.add_argument("--allow-actions", action="store_true")
    CONFIG = p.parse_args()
    identity()
    CONFIG.log = CONFIG.log.resolve(strict=True)
    if not CONFIG.log.is_file() or CONFIG.log.stat().st_uid != os.getuid():
        p.error("Expected same-UID regular log")
    CONFIG.output.mkdir(mode=0o700, parents=True, exist_ok=True)
    CONFIG.output = CONFIG.output.resolve(strict=True)
    if CONFIG.output.stat().st_uid != os.getuid() or CONFIG.output.stat().st_mode & 0o077:
        p.error("Output must be private and same-UID")
    os.umask(0o077)
    mcp.run(transport="stdio")

if __name__ == "__main__":
    main()
