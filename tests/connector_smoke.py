"""Browser/bridge regression checks. Requires Python Playwright + Chromium.

Run after `cargo build --locked`:
  python3 tests/connector_smoke.py
JSNER_DIR and PSCON_BIN can point at independent checkouts/builds.
All scans use a temporary profile, database, and loopback fixture server.
"""
from contextlib import ExitStack
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from tempfile import TemporaryDirectory
import argparse
import fcntl
import json
import pty
import shutil
import struct
import termios
import os
import signal
import socket
import sqlite3
import subprocess
import threading
import time
import urllib.request

from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[1]
JSNER = Path(os.environ.get("JSNER_DIR", ROOT.parent / "JSner"))
BINARY = Path(os.environ.get("PSCON_BIN", ROOT / "target/debug/pscon"))


class Fixture(BaseHTTPRequestHandler):
    methods = []

    def do_GET(self):
        self.methods.append(("GET", self.path))
        if self.path == "/":
            content, kind, status = b'<title>Connector fixture</title><script src="/app.js"></script>', "text/html", 200
        elif self.path == "/app.js":
            content, kind, status = b'window.fixtureEndpoint = "/api/from-bundle";', "application/javascript", 200
        elif self.path.startswith("/api/") and self.path != "/api/missing":
            content, kind, status = b'{"ok":true}', "application/json", 200
        else:
            content, kind, status = b"not found", "text/plain", 404
        self.send_response(status)
        self.send_header("Content-Type", kind)
        self.end_headers()
        self.wfile.write(content)

    def log_message(self, *args):
        pass


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def start_bridge(stack, temporary, suffix, token, tui=False):
    state = temporary / suffix
    state.mkdir()
    port = free_port()
    config = state / "config.toml"
    config.write_text(f'''[general]
data_dir = {json.dumps(str(state))}
timeout_secs = 2
[bridge]
port = {port}
token = {json.dumps(token)}
[recon]
crawl_depth = 0
max_pages = 1
max_assets = 3
fetch_source_maps = false
verify = false
[decisions]
enabled = false
[notify]
desktop = false
''')
    log = stack.enter_context((state / "bridge.log").open("w"))
    if tui:
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
        proc = subprocess.Popen([str(BINARY), "--config", str(config), "tui"],
                                env={**os.environ, "PSCON_HOME": str(state), "TERM": "xterm-256color"},
                                stdin=slave, stdout=slave, stderr=log)
        os.close(slave)
        def drain():
            try:
                while os.read(master, 65536):
                    pass
            except OSError:
                pass
        threading.Thread(target=drain, daemon=True).start()
        stack.callback(os.close, master)
    else:
        proc = subprocess.Popen([str(BINARY), "--config", str(config), "bridge", "serve"],
                                env={**os.environ, "PSCON_HOME": str(state)}, stdout=log, stderr=log)

    def stop():
        if proc.poll() is None:
            proc.send_signal(signal.SIGINT)
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
    stack.callback(stop)
    for _ in range(100):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=.2):
                return port, state
        except OSError:
            if proc.poll() is not None:
                raise AssertionError((state / "bridge.log").read_text())
            time.sleep(.05)
    raise AssertionError("Bridge did not start")


def run(missing_collector=False, skip_idle=False):
    with ExitStack() as stack:
        temporary = Path(stack.enter_context(TemporaryDirectory(prefix="pscon-connector-")))
        server = ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
        stack.callback(server.server_close)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        stack.callback(server.shutdown)
        origin = f"http://127.0.0.1:{server.server_port}"
        port, state = start_bridge(stack, temporary, "first", "first-test-token")
        other_port, other_state = start_bridge(stack, temporary, "second", "second-test-token")
        tui_port, tui_state = start_bridge(stack, temporary, "tui", "tui-test-token", tui=True)
        extension_path = JSNER
        if missing_collector:
            extension_path = temporary / "extension-without-collector"
            shutil.copytree(JSNER, extension_path, ignore=shutil.ignore_patterns('.git', '.agents', '.cursor', 'dist', 'tests'))
            manifest = json.loads((extension_path / "manifest.json").read_text())
            manifest.pop("content_scripts", None)
            (extension_path / "manifest.json").write_text(json.dumps(manifest))
        playwright = stack.enter_context(sync_playwright())
        context = playwright.chromium.launch_persistent_context(
            str(temporary / "profile"), channel="chromium", headless=True,
            args=[f"--disable-extensions-except={extension_path}", f"--load-extension={extension_path}"],
            viewport={"width": 480, "height": 600},
        )
        stack.callback(context.close)
        worker = context.service_workers[0] if context.service_workers else context.wait_for_event("serviceworker")
        extension_origin = worker.url.rsplit("/", 1)[0]
        site = context.new_page()
        site.goto(origin)
        popup = context.new_page()
        errors = []
        popup.on("pageerror", lambda error: errors.append(str(error)))
        popup.goto(extension_origin + "/popup.html")
        popup.wait_for_function("() => typeof setupPanels === 'function'")

        def request(kind, **data):
            return popup.evaluate("message => chrome.runtime.sendMessage(message)", {"type": kind, **data})

        # Failed pairing must return an error; correcting token/port must replace
        # the existing connection rather than retaining the old socket.
        failed = request("pscon_connect", token="incorrect", port=port)
        assert failed.get("ok") is False, failed
        assert request("pscon_connect", token="first-test-token", port=port)["ok"]
        assert request("pscon_connect", token="second-test-token", port=other_port)["ok"]
        assert request("pscon_get_status")["port"] == other_port
        print("PASS pairing errors and token/port replacement", flush=True)

        site.bring_to_front()
        popup.evaluate("document.getElementById('scanButton').click()")
        popup.wait_for_function("() => !document.getElementById('scanButton').disabled")
        assert "/api/from-bundle" in popup.evaluate("extractedData.endpoints"), popup.locator("#status").inner_text()
        print("PASS quick scan reads linked JavaScript", flush=True)

        site.bring_to_front()
        reply = request("pscon_deep_scan", token="second-test-token", port=other_port)
        assert reply.get("ok"), reply
        popup.wait_for_function("() => document.getElementById('bridgeStage').textContent.startsWith('Complete:')", timeout=30000)
        with sqlite3.connect(other_state / "pscon.db") as db:
            assert db.execute("select count(*) from scans where status='completed'").fetchone()[0] == 1
        with sqlite3.connect(state / "pscon.db") as db:
            assert db.execute("select count(*) from scans").fetchone()[0] == 0
        print("PASS snapshot -> headless scan -> database -> completion in popup", flush=True)
        if missing_collector:
            print("PASS missing content script recovered by on-demand injection", flush=True)

        # Reopening a popup restores the latest completion instead of losing it.
        popup.reload()
        popup.wait_for_function("() => document.getElementById('bridgeStage').textContent.startsWith('Complete:')")
        assert request("pscon_get_status")["connected"]

        assert request("pscon_connect", token="tui-test-token", port=tui_port)["ok"]
        site.bring_to_front()
        assert request("pscon_deep_scan", token="tui-test-token", port=tui_port)["ok"]
        popup.wait_for_function("() => document.getElementById('bridgeStage').textContent.startsWith('Complete:')", timeout=30000)
        with sqlite3.connect(tui_state / "pscon.db") as db:
            assert db.execute("select count(*) from scans where status='completed'").fetchone()[0] == 1
        popup.bring_to_front()
        failed_scan = request("pscon_deep_scan", token="tui-test-token", port=tui_port)
        assert failed_scan.get("ok") is False and "website" in failed_scan["error"], failed_scan
        assert request("pscon_get_status")["connected"]
        print("PASS TUI snapshot scan and actionable errors for restricted tabs", flush=True)

        assert int(popup.locator("#bridgePort").input_value()) == tui_port
        # Failed requests must remain failures; only safe GET requests are used.
        popup.evaluate("""origin => {
          extractedData = {endpoints:['/api/reachable','/api/missing'],apis:[],graphql:[],config:[],other:[]};
          document.getElementById('baseUrl').value = origin;
          updateUI();
        }""", origin)
        popup.evaluate("verifyEndpoints()")
        verified = popup.evaluate("extractedData.verification")
        assert verified["/api/reachable"]["GET"]["accessible"] is True
        assert verified["/api/missing"]["GET"]["accessible"] is False
        assert all(method == "GET" for method, _ in Fixture.methods)
        print("PASS real HTTP verification status", flush=True)

        # Result counts/actions for categories without endpoints, live updates,
        # category filtering, theme toggle, and keyboard-operable row actions.
        results = context.new_page()
        results.on("pageerror", lambda error: errors.append(str(error)))
        results.goto(extension_origin + "/results.html")
        popup.evaluate("chrome.storage.local.set({extractedData:{endpoints:[],apis:[],graphql:[],config:['config.json'],other:['soap-service']}})")
        results.wait_for_function("() => document.getElementById('configStat').textContent === '1'")
        results.locator('[data-filter="apis"]').click()
        assert results.locator(".category-section").count() == 0
        assert results.locator("#emptyState").is_visible()
        results.locator('[data-filter="all"]').click()
        assert results.locator(".category-section").count() == 2
        assert results.locator("button.action-icon").count() == 4
        results.locator("#themeToggle").click()
        assert results.evaluate("document.documentElement.dataset.mode") == "light"
        results.locator("#themeToggle").click()
        assert results.evaluate("document.documentElement.dataset.theme") == "dark"
        print("PASS results live updates, category filters, and theme controls", flush=True)

        screenshots = ROOT / "target/connector-checks"
        screenshots.mkdir(exist_ok=True)
        for width in [480, 320]:
            popup.set_viewport_size({"width": width, "height": 600})
            for theme in ["light", "dark"]:
                popup.evaluate("mode => chrome.storage.local.set({uiTheme:mode})", theme)
                popup.wait_for_function("theme => document.documentElement.dataset.theme === theme", arg=theme)
                for panel in ["scan", "results", "bridge"]:
                    popup.locator(f"#tab-{panel}").click()
                    bounds = popup.evaluate("({width:document.body.scrollWidth,height:document.body.scrollHeight})")
                    assert bounds["width"] <= width and bounds["height"] <= 600, bounds
                    if width == 480:
                        assert popup.evaluate("() => { const panel=document.querySelector('.panel-scroll'); return panel.scrollHeight <= panel.clientHeight; }"), panel
                    popup.screenshot(path=str(screenshots / f"popup-{width}-{theme}-{panel}.png"), animations="disabled")
            results.set_viewport_size({"width": width, "height": 600})
            assert results.evaluate("document.body.scrollWidth") <= width
        assert not errors, errors
        print("PASS popup layout at 320/480px, light/dark, and no page errors", flush=True)
        # The documented MV3 idle window is 30s: verify the live socket survives it.
        if not skip_idle:
            popup.wait_for_timeout(35000)
            assert request("pscon_get_status")["connected"]
            print("PASS bridge remains connected beyond the MV3 idle window", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--missing-collector", action="store_true", help="Exercise recovery for tabs without a content script")
    parser.add_argument("--skip-idle-check", action="store_true")
    args = parser.parse_args()
    run(args.missing_collector, args.skip_idle_check)
