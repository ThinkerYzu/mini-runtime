"""Protocol and runtime integration checks: python3 bench/test_http_viewer.py.

Optional real-browser check, from a Firefox tree under xvfb-run:
VIEWER_BROWSER=1 FIREFOX_BIN=... ./mach python .../bench/test_http_viewer.py
VIEWER_TRANSPORT=poll (or ws) loads the browser viewer with that ?transport= instead of the default.
"""
import base64
import functools
import http.server
import json
import os
from pathlib import Path
import hashlib
import selectors
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from urllib.error import HTTPError
from urllib.parse import parse_qs, urlsplit
from urllib.request import Request, urlopen

import http_viewer
from http_viewer import Service, Viewer

HERE = Path(__file__).resolve().parent
TRANSPORT = os.environ.get("VIEWER_TRANSPORT")


def page_url(url):
    """The viewer's URL with VIEWER_TRANSPORT's ?transport=, when set"""
    return url + "&transport=" + TRANSPORT if TRANSPORT else url


class Socket:
    """A WebSocket client of the viewer service, for the protocol checks: `status` is the
    handshake's HTTP status; frames are masked unless told otherwise."""
    def __init__(self, service, token=None, origin=None, rcvbuf=None):
        host, port = urlsplit(service.origin).netloc.split(":")
        self.sock = socket.socket()
        if rcvbuf:
            self.sock.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, rcvbuf)
        self.sock.settimeout(10)
        self.sock.connect((host, int(port)))
        key = base64.b64encode(os.urandom(16)).decode()
        origin = service.origin if origin is None else origin
        self.sock.sendall((f"GET /api/ws?token={service.token if token is None else token} HTTP/1.1\r\n"
                           f"Host: {host}:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
                           f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n" +
                           (f"Origin: {origin}\r\n" if origin else "") + "\r\n").encode())
        self.buf = b""
        while b"\r\n\r\n" not in self.buf:
            data = self.sock.recv(4096)
            if not data:
                break
            self.buf += data
        head, _, self.buf = self.buf.partition(b"\r\n\r\n")
        lines = head.decode().split("\r\n")
        self.status = int(lines[0].split()[1])
        self.headers = dict(line.split(": ", 1) for line in lines[1:])
        self.accept = base64.b64encode(hashlib.sha1((key + http_viewer.WS_GUID).encode()).digest()).decode()
        self.next = 0

    def frame(self, payload, opcode=1, mask=True):
        n = len(payload)
        head = bytes([0x80 | opcode]) + bytes([(0x80 if mask else 0) | (n if n < 126 else 126 if n < 65536 else 127)])
        head += b"" if n < 126 else struct.pack("!H", n) if n < 65536 else struct.pack("!Q", n)
        if not mask:
            return head + payload
        key = os.urandom(4)
        return head + key + bytes(b ^ key[i % 4] for i, b in enumerate(payload))

    def send(self, payload, opcode=1, mask=True):
        self.sock.sendall(self.frame(payload, opcode, mask))

    def exact(self, n):
        while len(self.buf) < n:
            data = self.sock.recv(65536)
            if not data:
                raise EOFError
            self.buf += data
        data, self.buf = self.buf[:n], self.buf[n:]
        return data

    def recv(self):
        """The next frame: (opcode, payload)"""
        head = self.exact(2)
        n = head[1] & 0x7F
        if n == 126:
            n = struct.unpack("!H", self.exact(2))[0]
        elif n == 127:
            n = struct.unpack("!Q", self.exact(8))[0]
        return head[0] & 0x0F, self.exact(n)

    def request(self, op, body):
        self.next += 1
        self.send(json.dumps({"id": self.next, "op": op, "body": body}).encode())
        return self.next

    def answer(self, ident):
        opcode, payload = self.recv()
        assert opcode == 1, (opcode, payload)
        answer = json.loads(payload)
        assert answer["id"] == ident, answer
        return answer

    def call(self, op, body):
        return self.answer(self.request(op, body))

    def close(self):
        self.sock.close()


class WebSocketTransport(unittest.TestCase):
    def setUp(self):
        self.viewer = Viewer("demo", "https://example.test/", [800, 600])
        self.service = Service("127.0.0.1:0", self.viewer).start()
        self.sockets = []

    def tearDown(self):
        for ws in self.sockets:
            ws.close()
        self.service.close()

    def open(self, **kw):
        ws = Socket(self.service, **kw)
        self.sockets.append(ws)
        return ws

    def attached(self, ws):
        """Attach over `ws`, publish an empty snapshot and acknowledge it: the session"""
        session = ws.call("attach", {"runtime": "demo", "viewport": [800, 600]})["ok"]["session"]
        self.viewer.wait({"now": 0, "busy": True})
        self.viewer.publish([["c", 0, []]], session)
        seq = ws.call("poll", {"session": session, "after": 0})["ok"]["batches"][0]["seq"]
        self.assertTrue(ws.call("input", {"session": session, "ack": seq, "events": []})["ok"]["ready"])
        return session, seq

    def test_handshake_access_policy(self):
        for kw, code in (({"token": "wrong"}, 403), ({"origin": ""}, 403), ({"origin": "http://elsewhere.test"}, 403)):
            self.assertEqual(self.open(**kw).status, code, kw)
        ws = self.open()
        self.assertEqual(ws.status, 101)
        self.assertEqual(ws.headers["Sec-WebSocket-Accept"], ws.accept)
        self.assertEqual(self.viewer.stats["ws_connections"], 1)
        # a plain request to the endpoint, and an unknown runtime or op
        with self.assertRaises(HTTPError) as error:
            urlopen(Request(self.service.origin + "/api/ws?token=" + self.service.token, headers={"Origin": self.service.origin}))
        self.assertEqual(error.exception.code, 400)
        error.exception.close()
        self.assertEqual(ws.call("attach", {"runtime": "other", "viewport": [800, 600]})["error"], "Unknown runtime")
        self.assertEqual(ws.call("nonsense", {})["error"], "Not found")

    def test_calls_match_http_semantics(self):
        ws = self.open()
        session, seq = self.attached(ws)
        self.assertEqual(ws.call("input", {"session": session, "ack": seq, "events": [{"type": "click"}]})["ok"],
                         {"ready": True, "inputPending": False})
        self.assertEqual(len(self.viewer.wait({"now": 0, "busy": True})["ok"]["events"]), 1)
        # stale sessions, navigation and the snapshot gate give the HTTP errors
        self.assertIn("attach again", ws.call("input", {"session": "stale", "ack": 0, "events": []})["error"])
        self.viewer.navigation_committed("https://example.test/next", 7)
        self.assertRegex(ws.call("poll", {"session": session, "after": seq})["error"], "^Document replaced by a navigation")
        replacement = ws.call("attach", {"runtime": "demo", "viewport": [800, 600]})["ok"]["session"]
        self.assertIn("snapshot", ws.call("input", {"session": replacement, "ack": 0, "events": [{"type": "click"}]})["error"])
        # the same Viewer through HTTP, on the same session
        req = Request(self.service.origin + "/api/input", data=json.dumps({"session": replacement, "ack": 0}).encode(),
                      headers={"X-Viewer-Token": self.service.token, "Content-Type": "application/json"})
        with urlopen(req) as response:
            self.assertEqual(json.load(response), {"ready": False, "inputPending": False})
        stats = self.viewer.stats
        self.assertEqual(stats["http_api_calls"], 1)
        self.assertGreater(stats["http_api_bytes_in"], 0)
        self.assertGreater(stats["http_api_bytes_out"], 0)
        self.assertEqual(stats["ws_calls"], 8)
        self.assertGreater(stats["ws_bytes_out"], stats["ws_calls"] * 10)

    def test_one_poll_waits_while_other_calls_answer(self):
        ws = self.open()
        session, seq = self.attached(ws)
        waiting = ws.request("poll", {"session": session, "after": seq})
        second = ws.request("poll", {"session": session, "after": seq})
        self.assertEqual(ws.answer(second)["error"], "A poll is already waiting")
        started = time.monotonic()
        self.assertTrue(ws.call("input", {"session": session, "ack": seq, "events": []})["ok"]["ready"])
        self.assertLess(time.monotonic() - started, 0.5, "input waited for the poll")
        self.viewer.publish([["d", 1, "x"]])
        batches = ws.answer(waiting)["ok"]["batches"]
        self.assertEqual([b["seq"] for b in batches], [seq + 1])

    def test_frames(self):
        ws = self.open()
        ws.send(b"hello", opcode=9)
        self.assertEqual(ws.recv(), (10, b"hello"))
        # a message in two fragments
        call = json.dumps({"id": 5, "op": "nonsense", "body": {}}).encode()
        ws.sock.sendall(ws.frame(call[:7], opcode=1)[:1].replace(b"\x81", b"\x01") + ws.frame(call[:7])[1:])
        ws.send(call[7:], opcode=0)
        self.assertEqual(ws.answer(5)["error"], "Not found")
        for opcode, payload, mask, code in ((2, b"binary", True, 1003), (1, b"{}", False, 1002), (1, b"not json", True, 1007),
                                            (1, json.dumps({"id": 1, "op": "input", "body": {"pad": "x" * (2 << 20)}}).encode(), True, 1009)):
            ws = self.open()
            ws.send(payload, opcode, mask)
            self.assertEqual(ws.recv(), (8, struct.pack("!H", code)), opcode)
            with self.assertRaises(EOFError):
                ws.recv()
        ws = self.open()
        ws.send(b"", opcode=8)
        self.assertEqual(ws.recv(), (8, struct.pack("!H", 1000)))

    def test_slow_reader_is_not_buffered(self):
        """A browser that stops reading stops the service reading its calls, and is closed"""
        old = http_viewer.WS_TIMEOUT
        http_viewer.WS_TIMEOUT = 1
        try:
            ws = self.open(rcvbuf=4096)
            session, seq = self.attached(ws)
            self.viewer.publish([["d", 1, "x" * (4 << 20)]])
            ws.request("poll", {"session": session, "after": seq})
            ws.sock.setblocking(False)
            sent = 0
            try:
                for _ in range(20000):
                    ws.sock.send(ws.frame(json.dumps({"id": 1, "op": "input", "body": {"session": session, "ack": seq}}).encode()))
                    sent += 1
            except (BlockingIOError, ConnectionResetError, BrokenPipeError):
                pass  # (the service may already have timed the socket out)
            time.sleep(0.3)
            answered = self.viewer.stats["ws_calls"]
            self.assertLess(answered, sent, "the service kept reading calls it could not answer")
            end = time.monotonic() + 5
            while self.service.sockets and time.monotonic() < end:
                time.sleep(0.05)
            self.assertFalse(self.service.sockets, "a stalled socket stayed open")
            self.assertLessEqual(self.viewer.stats["ws_calls"], answered + 1)
            self.assertEqual(self.viewer.session, session)  # the lease still holds the session
        finally:
            http_viewer.WS_TIMEOUT = old

    def test_connection_loss_keeps_the_session(self):
        ws = self.open()
        session, seq = self.attached(ws)
        waiting = ws.request("poll", {"session": session, "after": seq})
        self.service.drop_sockets()
        with self.assertRaises((EOFError, OSError)):
            ws.answer(waiting)
        again = self.open()
        self.assertTrue(again.call("input", {"session": session, "ack": seq, "events": [{"type": "click"}]})["ok"]["ready"])
        self.assertEqual(self.viewer.stats["attachments"], 1)
        self.assertEqual(self.viewer.stats["ws_connections"], 2)



class Protocol(unittest.TestCase):
    def test_snapshot_gate_replacement_and_resize(self):
        v = Viewer("demo", "http://example.test/", [800, 600])
        a = v.attach([800, 600])["session"]
        self.assertEqual(v.wait({"now": 0, "busy": True})["ok"]["snapshot"], a)
        with self.assertRaises(ValueError):
            v.input(a, 0, [{"type": "click"}])
        v.publish([["c", 0, []]], a)
        seq = v.poll(a, 0)["batches"][0]["seq"]
        self.assertTrue(v.input(a, seq, [])['ready'])
        v.input(a, seq, [{"type": "click"}])
        self.assertEqual(len(v.wait({"now": 0, "busy": True})["ok"]["events"]), 1)
        self.assertFalse(v.input(a, seq, [], [900, 700])["ready"])
        self.assertEqual(v.wait({"now": 0, "busy": True})["ok"]["viewport"], [900, 700])
        b = v.attach([900, 700])["session"]
        with self.assertRaises(ValueError):
            v.input(a, seq, [{"type": "click"}])
        v.publish([["c", 0, []]], a)  # obsolete snapshot in flight
        self.assertEqual(len(v.journal), 0)
        v.detach(b)
        v.publish([["d", 1, "detached update"]])
        self.assertEqual(len(v.journal), 0)
        self.assertTrue(v.wait({"now": 0, "busy": True})["ok"]["hidden"])
        self.assertFalse(v.snapshot_needed)

    def test_input_backpressure_preserves_order_and_resumes(self):
        v = Viewer("demo", "https://example.test/", [800, 600])
        session = v.attach([800, 600])["session"]
        v.wait({"now": 0, "busy": True})
        v.publish([["c", 0, []]], session)
        events = [{"type": "click", "task": i} for i in range(1024)]
        self.assertTrue(v.input(session, 1, events[:256])["ready"])
        self.assertTrue(v.input(session, 1, events[256:511])["ready"])
        result = v.input(session, 1, events[511:767])
        self.assertFalse(result["ready"])
        self.assertTrue(result["inputPending"])
        # Worst case: crossing the threshold, then another full batch captured
        # before the pause response arrived. Both fit without dropping input.
        self.assertFalse(v.input(session, 1, events[767:1023])["ready"])
        self.assertFalse(v.input(session, 1, [])["ready"])
        self.assertEqual(len(v.events), 1023)
        v.input(session, 1, events[1023:])  # direct clients still obey the hard cap
        with self.assertRaisesRegex(ValueError, "input queue full"):
            v.input(session, 1, [{"type": "click"}])
        self.assertEqual(v.wait({"now": 0, "busy": True})["ok"]["events"], events)
        self.assertEqual(v.input(session, 1, []), {"ready": True, "inputPending": False})
        self.assertEqual(v.session, session)

    @unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 to drive Firefox")
    def test_browser_input_backpressure(self):
        from marionette_driver.marionette import Marionette
        import firefox_exit
        v = Viewer("demo", "https://example.test/", [800, 600])
        service = Service("127.0.0.1:0", v).start()
        browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
            "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
        def until(check):
            end = time.monotonic() + 10
            while time.monotonic() < end:
                if check(): return
                time.sleep(0.03)
            self.fail("browser did not reach expected input state")
        def status():
            return browser.execute_script('return document.getElementById("status").textContent')
        def value():
            return browser.execute_script('return document.getElementById("field").value')
        try:
            browser.start_session()
            browser.set_context("content")
            browser.navigate(page_url(service.url))
            until(lambda: v.session is not None)
            session = v.session
            v.wait({"now": 0, "busy": True})
            field = {"i": 4, "n": "input", "a": [["id", "field"]], "c": []}
            html = {"i": 1, "n": "html", "a": [], "c": [
                {"i": 2, "n": "head", "a": [], "c": []},
                {"i": 3, "n": "body", "a": [], "c": [field]}]}
            v.publish([["c", 0, [html]]], session)
            until(lambda: status().startswith("Connected"))
            # No runtime waits drain input during these native edits.
            while len(v.events) < 511:
                v.input(session, 1, [{"type": "test-padding"}] * min(256, 511 - len(v.events)))
            browser.switch_to_frame(browser.find_element("id", "page"))
            browser.find_element("id", "field").send_keys("a")
            browser.switch_to_frame()
            until(lambda: status() == "Processing input…")
            self.assertEqual(browser.execute_script('return document.getElementById("page").style.pointerEvents'), "none")
            browser.switch_to_frame(browser.find_element("id", "page"))
            field = browser.find_element("id", "field")
            before = value()
            field.send_keys("blocked")
            self.assertEqual(value(), before, "native typing escaped the input pause")
            browser.switch_to_frame()
            accepted = list(v.events)
            self.assertLessEqual(len(accepted), 1024)
            self.assertEqual(v.wait({"now": 0, "busy": True})["ok"]["events"], accepted)
            until(lambda: status().startswith("Connected"))
            self.assertEqual(v.session, session)
            self.assertEqual(v.stats["attachments"], 1)
            browser.switch_to_frame(browser.find_element("id", "page"))
            browser.find_element("id", "field").send_keys("z")
            until(lambda: any(e.get("value", [None])[0] == before + "z" for e in v.events))
        finally:
            firefox_exit.close(browser)
            service.close()

    @unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 to drive Firefox")
    def test_browser_local_input_burst_and_navigation(self):
        from marionette_driver.marionette import Marionette
        import firefox_exit
        v = Viewer("demo", "https://example.test/", [800, 600])
        service = Service("127.0.0.1:0", v).start()
        browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
            "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
        def until(check):
            end = time.monotonic() + 10
            while time.monotonic() < end:
                if check(): return
                time.sleep(0.03)
            self.fail(f"browser did not recover from local input burst; status: {status()}, queued: {len(v.events)}")
        def status():
            return browser.execute_script('return document.getElementById("status").textContent')
        try:
            browser.start_session()
            browser.set_context("content")
            browser.navigate(page_url(service.url))
            until(lambda: v.session is not None)
            session = v.session
            v.wait({"now": 0, "busy": True})
            target = {"i": 200, "n": "input", "a": [["id", "field"]], "c": []}
            for i in range(160, 3, -1):
                target = {"i": i, "n": "div", "a": [["style", "width:200px;height:100px"]], "c": [target]}
            html = {"i": 1, "n": "html", "a": [], "c": [
                {"i": 2, "n": "head", "a": [], "c": []},
                {"i": 3, "n": "body", "a": [], "c": [target]}]}
            v.publish([["c", 0, [html]]], session)
            until(lambda: status().startswith("Connected"))
            # Hold one successful response while the next local batch fills.
            # The old response must not resume capture before that batch drains.
            # The service holds the answer, so this works on either transport; the
            # page records its state when it sends the next input call.
            held, release, original = threading.Event(), threading.Event(), v.input
            def holding(*args, **kw):
                result = original(*args, **kw)
                if not held.is_set():
                    held.set()
                    release.wait(10)
                return result
            v.input = holding
            browser.execute_script("""
                const w = window.wrappedJSObject, fetch = w.fetch, send = w.WebSocket.prototype.send;
                w.__released = false; w.__nextInput = null;
                const record = body => {
                    if (w.__released && !w.__nextInput)
                        w.__nextInput = {pointer: document.getElementById("page").style.pointerEvents,
                            status: document.getElementById("status").textContent, count: body.events.length};
                };
                w.fetch = function(path, options) {
                    if (path === "/api/input") record(JSON.parse(options.body));
                    return fetch(path, options);
                };
                w.WebSocket.prototype.send = function(data) {
                    const call = JSON.parse(data);
                    if (call.op === "input") record(call.body);
                    return send.call(this, data);
                };
            """)
            self.assertTrue(held.wait(5), "no input call was held; status: " + status())
            # Native pointer entry crosses 157 nested elements synchronously,
            # producing pointerenter and mouseenter before the send timer runs.
            browser.switch_to_frame(browser.find_element("id", "page"))
            browser.find_element("id", "field").click()
            browser.switch_to_frame()
            self.assertEqual(status(), "Processing input…")
            self.assertEqual(browser.execute_script('return document.getElementById("page").style.pointerEvents'), "none")
            browser.switch_to_frame(browser.find_element("id", "page"))
            browser.find_element("id", "field").send_keys("blocked")
            self.assertEqual(browser.execute_script('return document.getElementById("field").value'), "")
            browser.switch_to_frame()
            browser.execute_script('window.wrappedJSObject.__released = true;')
            release.set()
            until(lambda: browser.execute_script('return !!window.wrappedJSObject.__nextInput'))
            observed = browser.execute_script('return window.wrappedJSObject.__nextInput')
            self.assertEqual(observed, {"pointer": "none", "status": "Processing input…", "count": 256})
            self.assertNotIn("queue full", status().lower())
            self.assertEqual(v.session, session)
            # Drain as a runtime would. While the pointer rests over the nested
            # content, each resume can make Firefox send the enter burst again.
            def draining(into):
                stop = threading.Event()
                def drain():
                    while not stop.is_set():
                        into.extend(v.wait({"now": 0, "busy": False})["ok"]["events"])
                thread = threading.Thread(target=drain, daemon=True)
                thread.start()
                def end():
                    stop.set()
                    thread.join()
                return end
            accepted = []
            stop = draining(accepted)
            until(lambda: status().startswith("Connected"))
            stop()
            with v.lock:
                accepted.extend(v.events)
                self.assertEqual(len(accepted), v.stats["events"])
            tasks = [e["task"] for e in accepted]
            self.assertGreaterEqual(len(tasks), 256)
            self.assertEqual(tasks, sorted(set(tasks)))
            self.assertEqual(v.stats["attachments"], 1)
            v.navigation_started("https://example.test/all")
            v.navigation_committed("https://example.test/all", 42)
            until(lambda: v.session is not None and v.session != session)
            replacement = v.session
            v.wait({"now": 0, "busy": True})
            later = []
            stop = draining(later)
            v.publish([["c", 0, [html]]], replacement)
            until(lambda: status().startswith("Connected"))
            self.assertIn("/all", status())
            browser.switch_to_frame(browser.find_element("id", "page"))
            browser.find_element("id", "field").send_keys("z")
            until(lambda: any(e.get("value", [None])[0] == "z" for e in later))
            stop()
            self.assertEqual(v.stats["attachments"], 2)
        finally:
            firefox_exit.close(browser)
            service.close()

    @unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 to drive Firefox")
    def test_browser_websocket_loss_and_fallback(self):
        """The socket's loss ends the session in the page and Connect opens a new one; a socket that
        cannot open falls back to long polling, unless ?transport=ws (DESIGN "WebSocket transport")"""
        from marionette_driver.marionette import Marionette
        import firefox_exit
        v = Viewer("demo", "https://example.test/", [800, 600])
        service = Service("127.0.0.1:0", v).start()
        browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
            "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
        def until(check, what):
            end = time.monotonic() + 10
            while time.monotonic() < end:
                if check(): return
                time.sleep(0.03)
            self.fail(what + "; status: " + status())
        def status():
            return browser.execute_script('return document.getElementById("status").textContent')
        html = {"i": 1, "n": "html", "a": [], "c": [
            {"i": 2, "n": "head", "a": [], "c": []},
            {"i": 3, "n": "body", "a": [], "c": [{"i": 4, "n": "input", "a": [["id", "field"]], "c": []}]}]}
        def connected(old):
            until(lambda: v.session is not None and v.session != old, "no new attachment")
            session = v.session
            v.wait({"now": 0, "busy": True})
            v.publish([["c", 0, [html]]], session)
            until(lambda: status().startswith("Connected"), "not connected")
            return session
        def typed(text):
            browser.switch_to_frame(browser.find_element("id", "page"))
            browser.find_element("id", "field").send_keys(text)
            browser.switch_to_frame()
            until(lambda: any(e.get("value", [None])[0] == text for e in v.wait({"now": 0, "busy": True})["ok"]["events"]), "typing not delivered")
            browser.execute_script('document.getElementById("page").contentDocument.getElementById("field").value = ""')
        try:
            browser.start_session()
            browser.set_context("content")
            browser.navigate(service.url)
            first = connected(None)
            self.assertEqual(v.stats["ws_connections"], 1)
            http_calls = v.stats["http_api_calls"]  # the runtime list only
            typed("a")
            service.drop_sockets()
            until(lambda: status() == "Viewer connection lost. Click Connect to reattach.", "loss not shown")
            self.assertEqual(v.session, first)  # the service still holds it for the lease
            browser.find_element("id", "attach").click()
            connected(first)
            self.assertEqual(v.stats["ws_connections"], 2)
            self.assertEqual(v.stats["http_api_calls"], http_calls)
            typed("b")
            # the socket refused: auto falls back to long polling, ws does not
            service.server.RequestHandlerClass.websocket = lambda self: self.reply(404, {"error": "Not found"})
            before = v.session
            browser.navigate(service.url)
            polled = connected(before)
            self.assertEqual(v.stats["ws_connections"], 2)
            calls = v.stats["http_api_calls"]
            typed("c")
            self.assertGreater(v.stats["http_api_calls"], calls)
            attachments = v.stats["attachments"]
            browser.navigate(service.url + "&transport=ws")
            until(lambda: status() == "WebSocket unavailable. Click Connect to reattach.", "ws mode fell back")
            self.assertEqual(v.stats["attachments"], attachments)
        finally:
            firefox_exit.close(browser)
            service.close()

    def test_navigation_state_and_commit(self):
        viewer = Viewer("demo", "https://a.test/", [800, 600])
        session = viewer.attach([800, 600])["session"]
        viewer.wait({"now": 0, "until": 0})
        viewer.publish([["c", 0, []]], session)
        viewer.input(session, 1, [])
        click = {"type": "click", "target": 1}
        viewer.input(session, 1, [click])
        viewer.media_resource({"url": "https://a.test/v.mp4"}, {"site": "https://a.test", "top": "https://a.test", "doc": None})
        # loading: polls say so, input is dropped
        viewer.navigation_started("https://b.test/")
        self.assertEqual(viewer.poll(session, 1, timeout=0)["navigation"], {"state": "loading", "url": "https://b.test/"})
        self.assertEqual(viewer.input(session, 1, [click]), {"ready": False})
        self.assertEqual(viewer.stats["navigation_input_dropped"], 1)
        # a failure: the document stays, the notice says why
        viewer.navigation_failed("https://b.test/", "refused")
        reply = viewer.poll(session, 1, timeout=0)
        self.assertNotIn("navigation", reply)
        self.assertIn("refused", reply["notice"]["text"])
        # a commit: the session ends with its own reason; the old document's input and media go
        viewer.navigation_started("https://b.test/")
        viewer.navigation_committed("https://b.test/", 42)
        for call in (lambda: viewer.poll(session, 1, timeout=0), lambda: viewer.input(session, 1, [])):
            with self.assertRaisesRegex(ValueError, "^Document replaced by a navigation"):
                call()
        self.assertEqual((viewer.events, viewer.resources, viewer.page_url, viewer.runtime_pid), ([], {}, "https://b.test/", 42))
        # the new page does not hibernate while its viewer comes back, and gets a snapshot then
        self.assertFalse(viewer.wait({"now": 0, "until": 0})["ok"]["hidden"])
        again = viewer.attach([800, 600])
        self.assertEqual((again["url"], again["pid"]), ("https://b.test/", 42))
        self.assertEqual(viewer.wait({"now": 0, "until": 0})["ok"]["snapshot"], again["session"])
        self.assertEqual(viewer.stats["navigations"], 1)

    def test_snapshot_precedes_updates(self):
        v = Viewer("demo", "http://example.test/", [800, 600])
        session = v.attach([800, 600])["session"]
        v.publish([["d", 1, "too early"]])
        self.assertFalse(v.journal)
        v.publish([["c", 0, []]], session)
        v.publish([["d", 1, "after snapshot"]])
        batches = v.poll(session, 0)["batches"]
        self.assertEqual([b["snapshot"] for b in batches], [True, False])
        self.assertEqual([b["seq"] for b in batches], [1, 2])
        v.input(session, 2, [])
        self.assertFalse(v.journal)
        v.attach([800, 600])
        v.detach(v.session)  # detach before the runtime processes the snapshot request
        self.assertFalse(v.snapshot_needed)

    def test_bounded_journal_resynchronizes(self):
        v = Viewer("demo", "http://example.test/", [800, 600], max_batches=2)
        session = v.attach([800, 600])["session"]
        v.wait({"now": 0, "busy": True})
        v.publish([["c", 0, []]], session)
        v.publish([["d", 1, "one"]])
        v.publish([["d", 1, "two"]])
        self.assertEqual(len(v.journal), 0)
        self.assertEqual(v.wait({"now": 0, "busy": True})["ok"]["snapshot"], session)
        v.publish([["d", 1, "must wait for snapshot"]])
        self.assertEqual(len(v.journal), 0)
        v.publish([["c", 0, []]], session)
        self.assertTrue(v.poll(session, 0)["batches"][0]["snapshot"])

    def test_lease_and_access_policy(self):
        v = Viewer("demo", "http://example.test/", [800, 600], lease=0.01)
        session = v.attach([800, 600])["session"]
        time.sleep(0.02)
        with self.assertRaises(ValueError):
            v.poll(session, 0)
        self.assertIsNone(v.session)
        with self.assertRaises(ValueError):
            Service("0.0.0.0:0", v)
        service = Service("127.0.0.1:0", v).start()
        try:
            with self.assertRaises(HTTPError) as error:
                urlopen(service.origin + "/api/runtimes")
            self.assertEqual(error.exception.code, 403)
            error.exception.close()
            request = Request(service.origin + "/api/runtimes", headers={"X-Viewer-Token": service.token, "Origin": "http://elsewhere.test"})
            with self.assertRaises(HTTPError) as error:
                urlopen(request)
            self.assertEqual(error.exception.code, 403)
            error.exception.close()
        finally:
            service.close()


    def test_webgl_dummy_replay_and_replacement(self):
        v = Viewer("demo", "http://example.test/", [800, 600])
        create = {"cmds": [[1, 0, "$create", ["webgl", {}, 16, 16, 9], 0]], "sync": True}
        self.assertEqual(v.webgl(create)["ok"]["result"]["exts"], ["WEBGL_lose_context"])
        read = {"cmds": [[1, 0, "readPixels", [0, 0, 1, 1, 6408, 5121, {"$out": "Uint8Array", "n": 4}], 0]], "sync": True}
        self.assertEqual(v.webgl(read)["ok"]["outs"], ["AAAAAA=="])
        session = v.attach([800, 600])["session"]
        for expected in (create, read):
            job = v.poll(session, 0)["webgl"]
            self.assertEqual(job["request"]["cmds"], expected["cmds"])
            v.webgl_reply(session, job["id"], {"result": {} if expected is create else None, "outs": [], "throws": [], "lost": [], "lastThrew": None})
            with self.assertRaises(ValueError):
                v.webgl_reply(session, job["id"], {})
        self.assertNotIn("webgl", v.poll(session, 0, timeout=0))
        result = []
        thread = threading.Thread(target=lambda: result.append(v.webgl(read, timeout=1)))
        thread.start()
        job = v.poll(session, 0)["webgl"]
        v.webgl_reply(session, job["id"], {"result": None, "outs": ["/wAA/w=="], "throws": [], "lost": [], "lastThrew": None})
        thread.join(2)
        self.assertFalse(thread.is_alive())
        self.assertEqual(result[0]["ok"]["outs"], ["/wAA/w=="])
        other = v.attach([800, 600])["session"]
        with self.assertRaises(ValueError):
            v.webgl_reply(session, job["id"], {})
        self.assertEqual(v.poll(other, 0)["webgl"]["request"]["cmds"], create["cmds"])
        v.detach(other)
        self.assertEqual(v.webgl(read)["ok"]["outs"], ["AAAAAA=="])
        self.assertFalse(v.gl_lost)

    def test_webgl_timeout_stop_overflow_and_drop(self):
        create = {"cmds": [[1, 0, "$create", ["webgl", {}, 4, 4], 0]], "sync": True}
        v = Viewer("demo", "http://example.test/", [800, 600])
        session = v.attach([800, 600])["session"]
        result = v.webgl(create, timeout=0.02)
        self.assertIsNotNone(result["ok"]["result"])
        self.assertIsNone(v.session)
        self.assertEqual(v.stats["webgl_timeouts"], 1)
        with self.assertRaises(ValueError):
            v.webgl_reply(session, 1, {})
        self.assertEqual(len(v.gl_history), 1)
        v.webgl({"cmds": [[1, 0, "$drop", [], 0]], "sync": False})
        self.assertFalse(v.gl_history)
        self.assertFalse(v.gl_dummy.contexts)
        v = Viewer("demo", "http://example.test/", [800, 600], gl_max_batches=1)
        v.webgl(create)
        reply = v.webgl({"cmds": [[1, 0, "clear", [16384], 0]], "sync": False})["ok"]
        self.assertEqual(reply["lost"], [1])
        self.assertFalse(v.gl_history)
        v.stop()
        self.assertIsNotNone(v.webgl(create)["ok"]["result"])


    def test_webgl_pending_cancel_and_native_loss(self):
        create = {"cmds": [[1, 0, "$create", ["webgl", {}, 4, 4], 0]], "sync": True}
        for mode in ("replace", "stop", "expire"):
            v = Viewer("demo", "http://example.test/", [800, 600], lease=0.03 if mode == "expire" else 5)
            session = v.attach([800, 600])["session"]
            results = []
            thread = threading.Thread(target=lambda: results.append(v.webgl(create, timeout=1)))
            thread.start()
            job = v.poll(session, 0)["webgl"]
            if mode == "replace":
                v.attach([800, 600])
            elif mode == "stop":
                v.stop()
            thread.join(2)
            self.assertFalse(thread.is_alive(), mode)
            self.assertIsNotNone(results[0]["ok"]["result"], mode)
            self.assertEqual(len(v.gl_history), 1)
            if mode != "stop":
                with self.assertRaises(ValueError):
                    v.webgl_reply(session, job["id"], {})
        v = Viewer("demo", "http://example.test/", [800, 600])
        session = v.attach([800, 600])["session"]
        v.webgl_lost(session, [1])
        self.assertEqual(v.wait({"now": 0, "busy": True})["ok"]["webgl"]["lost"], [1])
        self.assertEqual(v.wait({"now": 0, "busy": True})["ok"]["webgl"]["lost"], [])


    def test_media_capabilities_and_event_epochs(self):
        v = Viewer("demo", "http://example.test/", [800, 600])
        self.assertEqual(v.media_type({"type":"video/mp4"})["ok"][:2], [False, ""])
        session = v.attach([800, 600])["session"]
        results = []
        thread = threading.Thread(target=lambda: results.append(v.media_type({"type":"video/mp4"})))
        thread.start()
        query = v.poll(session, 0)["mediaType"]  # no DOM/snapshot ack required
        v.media_type_reply(session, query["id"], [True, "probably"])
        thread.join(1); self.assertFalse(thread.is_alive())
        self.assertEqual(results[0]["ok"][:2], [True, "probably"])
        epoch = v.media_epoch
        event = {"k":"sb", "sb":1, "n":1, "buffered":[[0,4]], "dur":4, "error":None}
        v.media_input(session, epoch, [event])
        state = v.wait({"now":0,"busy":True})["ok"]
        self.assertEqual(state["media"], [event]); self.assertEqual(state["mediaBackend"], {"epoch":epoch,"attached":True})
        self.assertEqual(v.wait({"now":0,"busy":True})["ok"]["media"], [])
        v.detach(session)
        with self.assertRaises(ValueError): v.media_input(session, epoch, [event])
        self.assertEqual(v.media_type({"type":"video/mp4"})["ok"][:2], [False, ""])

    def test_media_timeout_replacement_and_bounded_events(self):
        v = Viewer("demo", "http://example.test/", [800, 600])
        session = v.attach([800, 600])["session"]
        self.assertEqual(v.media_type({"type":"video/mp4"}, timeout=.01)["ok"][:2], [False, ""])
        self.assertIsNone(v.session)
        with self.assertRaises(ValueError): v.media_type_reply(session, 1, [True,"probably"])
        session = v.attach([800, 600])["session"]
        results = []
        thread = threading.Thread(target=lambda: results.append(v.media_type({"type":"audio/mp4"})))
        thread.start(); query = v.poll(session, 0)["mediaType"]
        other = v.attach([800, 600])["session"]
        thread.join(1); self.assertFalse(thread.is_alive())
        self.assertEqual(results[0]["ok"][:2], [False, ""])
        with self.assertRaises(ValueError): v.media_type_reply(session, query["id"], [True,"probably"])
        epoch = v.media_epoch
        for _ in range(4): v.media_input(other, epoch, [{"k":"play","p":1,"ok":True}]*256)
        with self.assertRaises(ValueError): v.media_input(other, epoch, [{"k":"play","p":2,"ok":True}])
        self.assertGreater(v.media_epoch, epoch); self.assertEqual(v.media_events, [])
        with self.assertRaises(ValueError): v.media_input(other, epoch, [])
        self.assertTrue(v.snapshot_needed)

    def test_media_journal_overflow_resets_backend(self):
        v = Viewer("demo", "http://example.test/", [800, 600], max_batches=1)
        session = v.attach([800, 600])["session"]
        v.publish([["c",0,[]]],session)
        epoch = v.media_epoch
        v.publish([["m",{"o":"append","b64":"AAAA"}]])
        self.assertGreater(v.media_epoch,epoch)
        self.assertFalse(v.journal); self.assertTrue(v.snapshot_needed)


class MediaResources(unittest.TestCase):
    """GAPS B172: /media/<handle> against a stand-in for the API process's `media` op."""
    DATA = bytes(range(256)) * 200

    def setUp(self):
        import http_viewer
        self.module = http_viewer
        self.saved = {k: getattr(http_viewer, k) for k in ("MEDIA_CHUNK", "MAX_RESOURCES", "MAX_RESOURCE_ONE")}
        http_viewer.MEDIA_CHUNK = 4096
        self.viewer = Viewer("demo", "http://example.test/", [800, 600])
        self.service = Service("127.0.0.1:0", self.viewer).start()
        self.calls = []
        self.on_call = None
        self.viewer.media_fetch = self.fetch

    def tearDown(self):
        self.service.close()
        for k, v in self.saved.items():
            setattr(self.module, k, v)

    def fetch(self, entry, rng):
        self.calls.append((entry["url"], rng, entry.get("site"), entry.get("origin")))
        if self.on_call:
            self.on_call(len(self.calls))
        if entry["url"].endswith("/cors"):
            return {"err": "TypeError: NetworkError when attempting to fetch resource. (CORS)"}
        if entry["url"].endswith("/missing"):
            return {"ok": {"status": 404, "headers": [], "bodyBase64": ""}}
        data, total = self.DATA, len(self.DATA)
        if entry["url"].endswith("/plain"):
            return {"ok": {"status": 200, "headers": [["content-type", "video/mp4"]], "bodyBase64": base64.b64encode(data).decode()}}
        a, b = rng[len("bytes="):].split("-")
        if not a:
            a, b = total - int(b), total - 1
        a, b = int(a), min(int(b), total - 1)
        if a >= total:
            return {"ok": {"status": 416, "headers": [["content-range", f"bytes */{total}"]], "bodyBase64": ""}}
        return {"ok": {"status": 206, "headers": [["content-type", "video/mp4"], ["content-range", f"bytes {a}-{b}/{total}"]],
                       "bodyBase64": base64.b64encode(data[a:b + 1]).decode()}}

    def register(self, url, **extra):
        r = self.viewer.media_resource(dict({"url": url, "kind": "video", "origin": "https://site.test"}, **extra),
                                       {"site": "https://site.test", "top": "https://site.test", "doc": None})
        self.assertIn("ok", r, r)
        self.assertTrue(r["ok"]["url"].startswith(self.service.origin + "/media/"))
        return r["ok"]

    def get(self, url, rng=None):
        req = Request(url, headers={"Range": rng} if rng else {})
        try:
            with urlopen(req, timeout=10) as response:
                return response.status, dict(response.headers), response.read()
        except HTTPError as e:
            with e:
                return e.code, dict(e.headers), e.read()

    def test_ranges_stream_in_chunks_through_the_api_process(self):
        res = self.register("https://site.test/v.mp4")
        self.assertEqual(self.get(res["url"], "bytes=0-")[0], 409)  # no viewer attached
        self.assertEqual(self.calls, [])
        self.viewer.attach([800, 600])
        status, headers, body = self.get(res["url"], "bytes=100-")
        self.assertEqual((status, headers["Content-Range"], headers["Accept-Ranges"]), (206, "bytes 100-51199/51200", "bytes"))
        self.assertEqual(body, self.DATA[100:])
        self.assertEqual(len(self.calls), 13)  # 51100 bytes in 4096-byte API requests
        self.assertEqual(self.calls[0][1:], ("bytes=100-4195", "https://site.test", "https://site.test"))
        self.assertEqual(self.calls[1][1], "bytes=4196-8291")
        status, headers, body = self.get(res["url"], "bytes=10-19")
        self.assertEqual((status, headers["Content-Range"], body), (206, "bytes 10-19/51200", self.DATA[10:20]))
        self.assertEqual(self.get(res["url"], "bytes=-10")[2], self.DATA[-10:])
        status, headers, body = self.get(res["url"])
        self.assertEqual((status, "Content-Range" in headers, body), (200, False, self.DATA))
        status, headers, _ = self.get(res["url"], "bytes=60000-")
        self.assertEqual((status, headers["Content-Range"]), (416, "bytes */51200"))
        self.assertEqual(self.get(self.service.origin + "/media/nosuchhandle", "bytes=0-")[0], 404)
        self.assertEqual(self.get(self.register("https://other.test/cors", crossOrigin="anonymous")["url"], "bytes=0-")[0], 502)
        self.assertEqual(self.get(self.register("https://site.test/missing")["url"], "bytes=0-")[0], 404)
        self.viewer.media_resource({"release": [res["handle"]]}, {})
        self.assertEqual(self.get(res["url"], "bytes=0-")[0], 404)
        self.assertGreater(self.viewer.stats["media_resource_chunks"], 13)

    def test_blobs_and_servers_that_ignore_range_are_held(self):
        self.viewer.attach([800, 600])
        blob = self.register("blob:https://site.test/1", b64=base64.b64encode(b"0123456789").decode(), type="video/webm")
        status, headers, body = self.get(blob["url"], "bytes=5-")
        self.assertEqual((status, headers["Content-Type"], headers["Content-Range"], body), (206, "video/webm", "bytes 5-9/10", b"56789"))
        self.assertEqual(self.calls, [])
        self.assertEqual(self.viewer.resource_bytes, 10)
        plain = self.register("https://site.test/plain")
        self.assertEqual(self.get(plain["url"], "bytes=10-19")[2], self.DATA[10:20])
        self.assertEqual(self.get(plain["url"], "bytes=20-29")[2], self.DATA[20:30])
        self.assertEqual(len(self.calls), 1)  # the whole answer is kept
        self.assertEqual(self.viewer.resource_bytes, 10 + len(self.DATA))
        self.viewer.media_resource({"release": [blob["handle"], plain["handle"]]}, {})
        self.assertEqual(self.viewer.resource_bytes, 0)

    def test_detach_cancels_a_stream_and_bounds(self):
        res = self.register("https://site.test/v.mp4")
        session = self.viewer.attach([800, 600])["session"]
        self.on_call = lambda n: n == 3 and self.viewer.detach(session)
        try:
            status, _, body = self.get(res["url"], "bytes=0-")
        except Exception:
            body = b""  # the connection closed short of its Content-Length
        self.assertLess(len(body), len(self.DATA))
        self.assertEqual(len(self.calls), 3)  # nothing fetched once the viewer went
        self.assertEqual(self.viewer.stats["media_resource_cancels"], 1)
        self.on_call = None
        self.module.MAX_RESOURCE_ONE = 4
        self.assertIn("err", self.viewer.media_resource({"url": "blob:x", "b64": base64.b64encode(b"12345").decode()}, {}))
        self.module.MAX_RESOURCES = len(self.viewer.resources) + 1
        self.register("https://site.test/a")
        self.assertIn("err", self.viewer.media_resource({"url": "https://site.test/b"}, {}))
        self.assertIn("err", self.viewer.media_resource({"url": "javascript:alert(1)"}, {}))
        self.viewer.attach([800, 600])
        self.viewer.streams = threading.BoundedSemaphore(1)
        self.viewer.streams.acquire()
        self.assertEqual(self.get(res["url"], "bytes=0-")[0], 503)


class Runtime(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="mrt-http-viewer-")
        self.root = Path(self.tmp.name)
        self.proc = None

        runtime = self
        self.css_requested = threading.Event()
        self.css_release = threading.Event()

        self.media_requests = []
        # the navigation test's document requests, as its server saw them
        self.nav_requests = []
        self.slow_requested = threading.Event()
        self.slow_cookies = []

        class Quiet(http.server.SimpleHTTPRequestHandler):
            def navigation(self, body=b""):
                """The navigation fixtures (test_navigation*): their documents, each request recorded"""
                url = urlsplit(self.path)
                query = parse_qs(url.query)
                runtime.nav_requests.append({"method": self.command, "path": self.path, "host": self.headers.get("Host"), "body": body.decode(),
                                             **{k: self.headers.get(k) for k in ("Cookie", "Referer", "Origin", "Content-Type", "Sec-Fetch-Dest",
                                                                                  "Sec-Fetch-Mode", "Sec-Fetch-Site", "Sec-Fetch-User", "Upgrade-Insecure-Requests", "Accept")}})
                def send(code, data=b"", kind="text/html; charset=utf-8", extra=()):
                    self.send_response(code)
                    for k, v in extra:
                        self.send_header(k, v)
                    if code not in (204, 205):
                        self.send_header("Content-Type", kind)
                    self.send_header("Content-Length", str(len(data)))
                    self.end_headers()
                    self.wfile.write(data)
                if url.path == "/nav-start":
                    send(200, (HERE / "http-viewer" / "nav-start.html").read_bytes(),
                         extra=[("Set-Cookie", "strict=1; SameSite=Strict; Path=/"), ("Set-Cookie", "lax=1; SameSite=Lax; Path=/")])
                elif url.path == "/nav-echo":
                    page = (HERE / "http-viewer" / "nav-echo.html").read_text().replace("@PORT@", str(runtime.server.server_port))
                    page = page.replace("@ENCTYPE@", query.get("enctype", ["application/x-www-form-urlencoded"])[0])
                    send(200, page.replace("@REQUIRED@", "required" if query.get("required") else "").encode())
                elif url.path == "/nav-startup":
                    step = query.get("step", ["one"])[0]
                    target = {"one": "/nav-startup?step=two", "two": "/nav-echo?step=startup-final", "bad": "/nav-json"}[step]
                    send(200, ("<!doctype html><meta charset=utf-8><p id=state>startup-" + step +
                        "</p><button id=increment>Count</button><output id=count>0</output><script>" +
                        "sessionStorage.setItem('k', (sessionStorage.getItem('k') || '') + '+' + " + json.dumps(step) + ");" +
                        "location.replace(" + json.dumps(target) + ");" +
                        "let count=0;document.getElementById('increment').onclick=()=>{document.getElementById('count').textContent=String(++count)};" +
                        "</script>").encode())
                elif url.path == "/nav-empty":
                    script = "document.open();"
                    if query.get("recover"):
                        document = ('<!doctype html><button id="recovered">Recovered</button><output id="count">0</output>'
                                    '<script>let n=0;document.getElementById("recovered").onclick=()=>'
                                    '{document.getElementById("count").textContent=String(++n)};</script>')
                        script += "setTimeout(()=>{document.write(" + json.dumps(document).replace("</script>", "<\\/script>") + ");document.close();},2500);"
                    send(200, ("<!doctype html><script>" + script + "</script>").encode())
                elif url.path == "/nav-redirect":
                    send(302, extra=[("Location", query["to"][0])])
                elif url.path == "/nav-nocontent":
                    send(204)
                elif url.path == "/nav-json":
                    send(200, b"{}", "application/json")
                else:
                    return False
                return True

            def do_POST(self):
                body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
                if not self.navigation(body):
                    self.send_response(405); self.send_header("Content-Length", "0"); self.end_headers()

            def do_GET(self):
                if self.path.startswith("/nav-") and self.navigation():
                    return
                media_file = self.path.rsplit("/", 1)[-1]
                if media_file in ("media-auth.mp4", "media-cors.mp4"):
                    # needs the cookie the page set (in the API process's jar), or (the cross-origin
                    # one) allows CORS; both serve ranges
                    cors = media_file == "media-cors.mp4"
                    runtime.media_requests.append(dict({k: self.headers.get(k) for k in ("Cookie", "Range", "Sec-Fetch-Dest", "Sec-Fetch-Mode", "Referer", "Accept", "Origin")}, path=media_file))
                    if not cors and "mediaauth=page-secret" not in (self.headers.get("Cookie") or ""):
                        self.send_response(403); self.send_header("Content-Length", "0"); self.end_headers()
                        return
                    data = (HERE / "bridge-cases" / "media-plain.mp4").read_bytes()
                    rng = self.headers.get("Range", "")
                    start, _, end = rng[len("bytes="):].partition("-") if rng.startswith("bytes=") else ("", "", "")
                    if not start:
                        self.send_response(200); self.send_header("Content-Type", "video/mp4")
                        self.send_header("Content-Length", str(len(data))); self.end_headers(); self.wfile.write(data)
                        return
                    start, end = int(start), min(int(end) if end else len(data) - 1, len(data) - 1)
                    self.send_response(206); self.send_header("Content-Type", "video/mp4"); self.send_header("Accept-Ranges", "bytes")
                    if cors:
                        self.send_header("Access-Control-Allow-Origin", "*")
                    self.send_header("Content-Range", f"bytes {start}-{end}/{len(data)}")
                    self.send_header("Content-Length", str(end + 1 - start)); self.end_headers(); self.wfile.write(data[start:end + 1])
                    return
                if media_file in ("media-plain.mp4", "media-v.mp4", "media-a.mp4"):
                    data = (HERE / "bridge-cases" / media_file).read_bytes()
                    self.send_response(200); self.send_header("Content-Type", "video/mp4")
                    self.send_header("Content-Length", str(len(data))); self.end_headers(); self.wfile.write(data)
                    return
                if self.path.startswith("/slow?"):
                    runtime.slow_requested.set()
                    runtime.slow_cookies.append(self.headers.get("Cookie", ""))
                    self.send_response(200)
                    self.send_header("Content-Type", "text/plain")
                    self.send_header("Set-Cookie", "slowreply=yes; Path=/")
                    self.send_header("Content-Length", "3"); self.end_headers()
                    time.sleep(6.2)  # Headers alone are not whole-body completion.
                    try: self.wfile.write(bytes([255, 0, 65]))
                    except (BrokenPipeError, ConnectionResetError): pass
                    return
                if self.path == "/fast-reply":
                    self.send_response(200); self.send_header("Content-Length", "4"); self.end_headers()
                    self.wfile.write(b"fast"); return
                if self.path == "/broken-response":
                    self.send_response(200); self.send_header("Content-Length", "20"); self.end_headers()
                    self.wfile.write(b"short"); self.close_connection = True
                    return
                # Hold only the viewer browser's CSS, not the runtime's Firefox-compatible UA.
                ua = self.headers.get("User-Agent", "")
                if self.path == "/scroll-restore.css" and "Firefox" in ua and "MiniRuntime/" not in ua:
                    runtime.css_requested.set()
                    runtime.css_release.wait(8)
                    self.send_response(200)
                    self.send_header("Content-Type", "text/css")
                    self.send_header("Cache-Control", "no-store")
                    self.end_headers()
                    self.wfile.write((HERE / "http-viewer" / "scroll-restore.css").read_bytes())
                    return
                super().do_GET()

            def log_message(self, *args):
                pass

        media_case = self._testMethodName == "test_media_types_before_attachment"
        source = HERE / ("bridge-cases" if media_case else "http-viewer")
        page = "media-types.html" if media_case else ("webgl-demo.html" if self._testMethodName == "test_browser_webgl" else "demo.html")
        if self._testMethodName == "test_browser_editable":
            page = "editable.html"
        if self._testMethodName == "test_browser_shadow_adopted":
            page = "shadow-adopted.html"
        if self._testMethodName == "test_browser_slow_fetch":
            page = "slow-fetch.html"
        if self._testMethodName == "test_browser_svg_search":
            source, page = HERE / "bridge-cases", "svg-element.html"
        if self._testMethodName == "test_browser_rule_style":
            source, page = HERE / "bridge-cases", "rule-style.html"
        if self._testMethodName == "test_browser_document_input":
            source, page = HERE / "bridge-cases", "document-input.html"
        if self._testMethodName in ("test_browser_delayed_scroll", "test_input_before_reconnect_snapshot"):
            page = "scroll-restore.html"
        if self._testMethodName in ("test_media_replay_quota", "test_media_replay_unlimited", "test_media_replay_quota_with_zero"):
            page = "media-replay-quota.html"
            if self._testMethodName == "test_media_replay_unlimited":
                page += "?unlimited=1"
        if self._testMethodName == "test_browser_media":
            page = "media-demo.html"
        if self._testMethodName.startswith(("test_navigation", "test_browser_navigation")):
            page = "nav-start"
        if self._testMethodName in ("test_frames", "test_browser_frames"):
            page = "frames.html"
        if self._testMethodName in ("test_fragment_scroll", "test_browser_fragment_scroll"):
            page = "fragment-scroll.html#initial"
        resources = self._testMethodName in ("test_media_resources", "test_browser_media_resources")
        if resources:
            page = "media-origin.html"
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Quiet, directory=str(source)))
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        env = {k: v for k, v in os.environ.items()
               if not k.startswith(("MRT_", "API_", "REALSITE", "REPLAY", "BRIDGE_", "MINI_"))
               and k not in ("VIEWER_HTTP", "VIEWER_MEDIA_REPLAY_UNLIMITED", "NO_FIREFOX", "INTERACTIVE", "INTERACTIVE_S", "RENDER", "HEAP_SCAN", "DISPLAY", "WAYLAND_DISPLAY")}
        # Acceptance can select SDK-built/relocated binaries while all page/profile state stays isolated.
        for key in ("REALSITE_BIN", "API_BIN"):
            if os.environ.get(key):
                env[key] = os.environ[key]
        env.update(VIEWER_HTTP="127.0.0.1:0", FIREFOX_BIN="/nonexistent/firefox", ONLY="demo",
                   EXTRA_SITES=json.dumps({"demo": f"http://localhost:{self.server.server_port}/{page}"}),
                   REALSITES_PROFILE=str(self.root / "empty-profile"), MINI_OUT=str(self.root / "result.json"),
                   MINI_DIR=str(self.root / "logs"), REALSITE_ARGS="--console")
        if self._testMethodName == "test_media_replay_unlimited":
            env["VIEWER_MEDIA_REPLAY_UNLIMITED"] = "1"
        elif self._testMethodName == "test_media_replay_quota_with_zero":
            env["VIEWER_MEDIA_REPLAY_UNLIMITED"] = "0"
        if resources:
            env["VIEWER_MEDIA_CHUNK"] = "8192"  # the 41 KB file in several API-process ranges
        self.stderr = open(self.root / "driver.log", "w")
        self.proc = subprocess.Popen([sys.executable, "-S", str(HERE / "mini_realsites.py")], env=env,
                                     stdout=subprocess.PIPE, stderr=self.stderr, text=True)
        selector = selectors.DefaultSelector()
        selector.register(self.proc.stdout, selectors.EVENT_READ)
        try:
            self.assertTrue(selector.select(15), "service did not start")
            line = self.proc.stdout.readline()
            self.assertTrue(line.startswith("VIEWER_URL "), line + (self.root / "driver.log").read_text())
            self.viewer_url = page_url(line.split()[1])
        finally:
            selector.close()
        parsed = urlsplit(self.viewer_url)
        self.origin = f"{parsed.scheme}://{parsed.netloc}"
        self.token = parse_qs(parsed.query)["token"][0]

    def call(self, path, body=None):
        req = Request(self.origin + "/api/" + path,
                      data=None if body is None else json.dumps(body).encode(),
                      headers={"X-Viewer-Token": self.token, "Content-Type": "application/json"})
        with urlopen(req, timeout=10) as response:
            return json.load(response)

    def tearDown(self):
        self.css_release.set()
        if self.proc:
            if self.proc.poll() is None:
                try:
                    self.call("stop", {})
                    self.proc.wait(timeout=10)
                except Exception:
                    self.proc.terminate()
                    try:
                        self.proc.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        self.proc.kill()
                        self.proc.wait()
            self.proc.stdout.close()
        self.stderr.close()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()
        self.tmp.cleanup()

    def snapshot(self, viewport=(800, 600)):
        session = self.call("attach", {"runtime": "demo", "viewport": list(viewport)})["session"]
        end = time.monotonic() + 10
        while time.monotonic() < end:
            batches = self.call("poll?session=" + session + "&after=0")["batches"]
            if batches:
                self.assertTrue(batches[0]["snapshot"])
                return session, batches
        self.fail("no snapshot")

    @staticmethod
    def specs(batches):
        result = {}

        def visit(spec):
            if not isinstance(spec, dict):
                return
            result[spec["i"]] = spec
            for child in spec.get("c", []):
                visit(child)
            if spec.get("r"):
                visit(spec["r"])
            if isinstance(spec.get("f"), dict):
                visit(spec["f"])

        for batch in batches:
            for op in batch["ops"]:
                if op[0] == "c":
                    for spec in op[2]:
                        visit(spec)
        return result

    def test_attach_input_and_reconnect(self):
        pid = self.call("runtimes")[0]["pid"]
        time.sleep(1.3)  # Detached timers use the runtime's background throttling.
        a, batches = self.snapshot()
        specs = self.specs(batches)
        elements = {dict(s.get("a", [])).get("id"): s for s in specs.values() if "n" in s}
        self.assertGreater(int(elements["ticks"]["c"][0]["t"]), 0)
        button = elements["increment"]["i"]
        seq = batches[-1]["seq"]
        self.call("input", {"session": a, "ack": seq, "events": []})
        self.call("input", {"session": a, "ack": seq, "events": [{"type": "click", "iface": "MouseEvent", "target": button,
                  "active": button, "task": 1, "bubbles": True, "cancelable": True, "composed": True, "fields": {"button": 0}}]})
        time.sleep(0.25)
        self.call("detach", {"session": a})
        time.sleep(1.1)
        b, batches = self.snapshot((950, 650))
        specs = self.specs(batches)
        elements = {dict(s.get("a", [])).get("id"): s for s in specs.values() if "n" in s}
        self.assertEqual(elements["count"]["c"][0]["t"], "1")
        self.assertEqual(elements["starts"]["c"][0]["t"], "1")
        self.assertEqual(elements["increment"]["i"], button)
        self.assertEqual(self.call("runtimes")[0]["pid"], pid)
        with self.assertRaises(HTTPError) as error:
            self.call("input", {"session": a, "ack": seq, "events": []})
        self.assertEqual(error.exception.code, 409)
        error.exception.close()
        self.call("stop", {})
        self.proc.wait(timeout=10)
        result = json.loads((self.root / "result.json").read_text())["demo"]
        self.assertEqual(self.proc.returncode, 0, result)
        self.assertEqual(result["js_errors"], 0)
        self.assertFalse(result["firefox_started"])
        self.assertEqual(result["viewer"]["attachments"], 2)
        self.assertEqual(result["viewer"]["events"], 1)
        self.assertTrue(any("hibernated_at" in item for item in result["interactive_hibernations"]))
        self.assertEqual((self.root / "logs" / "demo.log").read_text().count("DEMO_STARTED"), 1)

    def frame_snapshot(self, ready):
        """A snapshot once `ready(elements by id)` holds (the frames load after the page)."""
        end = time.monotonic() + 15
        while True:
            session, batches = self.snapshot()
            elements = {dict(s.get("a", [])).get("id"): s for s in self.specs(batches).values() if "n" in s}
            if ready(elements) or time.monotonic() > end:
                return session, batches, elements
            self.call("detach", {"session": session})
            time.sleep(0.3)

    def test_frames(self):
        text = lambda elements, name: "".join(c.get("t", "") for c in elements[name]["c"] if isinstance(c, dict))
        loaded = lambda e: "nested-button" in e and "inline-text" in e and text(e, "child-state").startswith("child ")
        a, batches, elements = self.frame_snapshot(loaded)
        self.assertTrue(loaded(elements), sorted(k for k in elements if k))
        # each frame element carries its runtime document; ids are unique across the documents
        child, inline, nested = elements["child"]["f"], elements["inline"]["f"], elements["nested"]["f"]
        self.assertTrue(child["u"].endswith("/frames-child.html"), child["u"])
        self.assertTrue(inline["u"].endswith("/frames.html"), inline["u"])  # about:srcdoc resolves against its parent
        self.assertEqual(nested["u"], child["u"])
        ids = []
        def collect(spec):
            if isinstance(spec, dict):
                ids.append(spec["i"])
                for c in spec.get("c", []):
                    collect(c)
                collect(spec.get("r"))
                collect(spec.get("f"))
        for batch in batches:
            for op in batch["ops"]:
                if op[0] == "c":
                    for spec in op[2]:
                        collect(spec)
        self.assertEqual(len(ids), len(set(ids)))
        self.assertEqual(text(elements, "child-state"), "child true")  # reCAPTCHA's sibling lookup by name
        send = elements["send"]["i"]
        seq = batches[-1]["seq"]
        self.call("input", {"session": a, "ack": seq, "events": []})
        def click(name, task, outer):
            return {"type": "click", "iface": "MouseEvent", "target": elements[name]["i"], "active": elements[name]["i"],
                    "outerActive": elements[outer]["i"], "task": task, "bubbles": True, "cancelable": True, "composed": True,
                    "fields": {"button": 0}}
        self.call("input", {"session": a, "ack": seq, "events": [click("send", 1, "child"), click("inline-button", 2, "inline"),
                                                               click("nested-button", 3, "child")]})
        done = lambda e: text(e, "child-reply").startswith("reply 1") and text(e, "nested-count") != "0"
        self.call("detach", {"session": a})
        b, batches, elements = self.frame_snapshot(done)
        self.assertEqual(text(elements, "child-reply"), "reply 1 sum 6 owned true detached true")
        self.assertEqual(text(elements, "replies"), "1")
        self.assertEqual(text(elements, "bytes"), "1,2,3")
        self.assertEqual(text(elements, "focus"), "true true")
        self.assertEqual(text(elements, "inline-count"), "1 true")
        self.assertEqual(text(elements, "nested-count"), "1 true true")
        # reconnected: the same documents and ids, and input still reaches the frame's window
        self.assertEqual(elements["child"]["f"]["i"], child["i"])
        self.assertEqual(elements["send"]["i"], send)
        seq = batches[-1]["seq"]
        self.call("input", {"session": b, "ack": seq, "events": []})
        self.call("input", {"session": b, "ack": seq, "events": [click("send", 4, "child")]})
        self.call("detach", {"session": b})
        _, batches, elements = self.frame_snapshot(lambda e: text(e, "child-reply").startswith("reply 2"))
        self.assertEqual(text(elements, "child-reply"), "reply 2 sum 7 owned true detached true")
        self.call("stop", {})
        self.proc.wait(timeout=10)
        result = json.loads((self.root / "result.json").read_text())["demo"]
        self.assertEqual(self.proc.returncode, 0, result)
        self.assertEqual(result["js_errors"], 0, result)
        self.assertFalse(result["unsupported_ops"], result)
        self.assertEqual(result["bridge_tab"], {"made": False})

    def test_input_before_reconnect_snapshot(self):
        log = self.root / "logs" / "demo.log"
        self.until(lambda: log.exists() and "SCROLL_STATE 700" in log.read_text())
        session, batches = self.snapshot()
        specs = self.specs(batches)
        elements = {dict(s.get("a", [])).get("id"): s for s in specs.values() if "n" in s}
        seq = batches[-1]["seq"]
        scroll_seq = next(op[1] for b in batches for op in b["ops"] if op[0] == "scroll")
        self.call("input", {"session": session, "ack": seq, "events": []})
        def click(name, task):
            return {"type": "click", "iface": "MouseEvent", "target": elements[name]["i"], "active": None,
                    "task": task, "bubbles": True, "cancelable": True, "composed": True, "fields": {"button": 0}}
        self.call("input", {"session": session, "ack": seq, "events": [click("busy", 1)]})
        self.until(lambda: "BUSY_START" in log.read_text())
        events = [{"type": "scroll", "iface": "UIEvent", "target": 0, "active": None, "task": 2,
                   "bubbles": True, "cancelable": False, "composed": False, "fields": {},
                   "scroll": [0, 900], "scrollSeq": scroll_seq}, click("poke", 3)]
        self.call("input", {"session": session, "ack": seq, "events": events})
        self.call("detach", {"session": session})
        _, batches = self.snapshot()
        specs = self.specs(batches)
        count = next(s for s in specs.values() if dict(s.get("a", [])).get("id") == "count")
        self.assertEqual(count["c"][0]["t"], "1", "accepted click was lost or snapshot preceded its task")
        position = [op[3] for b in batches for op in b["ops"] if op[0] == "scroll"]
        self.assertEqual(position, [900], "accepted scroll was lost or snapshot preceded its task")

    def test_media_replay_quota(self):
        self.check_media_replay_quota()

    def test_media_replay_quota_with_zero(self):
        self.check_media_replay_quota()

    def test_media_replay_unlimited(self):
        self.check_media_replay_quota(unlimited=True)

    def check_media_replay_quota(self, unlimited=False):
        session = self.call("attach", {"runtime":"demo", "viewport":[800,600]})["session"]
        after = 0
        log = self.root / "logs" / "demo.log"
        end = time.monotonic() + 45
        while time.monotonic() < end:
            reply = self.call("poll?session=" + session + "&after=" + str(after))
            if reply.get("mediaType"):
                self.call("media-type", {"session":session, "id":reply["mediaType"]["id"], "reply":[True,"probably"]})
            events = []
            for batch in reply["batches"]:
                after = batch["seq"]
                for op in batch["ops"]:
                    if op[0] != "m": continue
                    m = op[1]
                    if m["o"] == "attach": events.append({"k":"ms", "ms":m["ms"], "ev":"sourceopen"})
                    elif m["o"] == "append": events.append({"k":"sb", "sb":m["sb"], "n":m["n"], "buffered":[], "dur":1})
            if events:
                self.call("media-events", {"session":session, "epoch":reply["mediaEpoch"], "events":events})
            if after: self.call("input", {"session":session, "ack":after, "events":[]})
            text = log.read_text() if log.exists() else ""
            self.assertNotIn("QUOTA_ERROR", text)
            if unlimited and "UNLIMITED_DONE" in text:
                for marker in ("UNLIMITED_BYTES", "UNLIMITED_COMMANDS", "LIVE_APPEND_QUOTA"):
                    self.assertIn(marker, text)
                break
            if not unlimited and "CACHE_RELEASED" in text:
                self.assertIn("BYTE_QUOTA 11", text)
                self.assertIn("COMMAND_QUOTA", text)
                self.assertIn("QUOTA_SOURCE_CLOSED", text)
                break
        else:
            self.fail("quota fixture timed out: " + log.read_text())
        self.call("stop", {})
        self.proc.wait(timeout=10)
        result = json.loads((self.root / "result.json").read_text())["demo"]
        self.assertEqual(self.proc.returncode, 0, result)
        self.assertEqual(result["exit"], 0)
        self.assertEqual(result["js_errors"], 0)
        self.assertFalse(result["firefox_started"])
        self.assertEqual(result["bridge_tab"], {"made": False})
        self.assertFalse(result["unsupported_ops"])


    def test_media_types_before_attachment(self):
        # These queries precede attachment, so the recorded output is negative.
        # Later protocol-only sessions do not rerun the page's initial queries.
        log_path = self.root / "logs" / "demo.log"
        self.until(lambda: log_path.exists() and "MEDIA_TYPES " in log_path.read_text())
        pid = self.call("runtimes")[0]["pid"]
        previous = None
        for turn in range(3):
            session, batches = self.snapshot()
            specs = self.specs(batches)
            caps = next(s for s in specs.values() if dict(s.get("a", [])).get("id") == "caps")
            text = "".join(s.get("t", "") for s in caps.get("c", []))
            lines = json.loads(text)
            self.assertGreater(len(lines), 30)
            for line in lines:
                self.assertEqual(json.loads(line)[1:], [False, "", ""])
            self.assertEqual(self.call("runtimes")[0]["pid"], pid)
            if previous:
                with self.assertRaises(HTTPError) as error:
                    self.call("poll?session=" + previous + "&after=0")
                self.assertEqual(error.exception.code, 409)
                error.exception.close()
            previous = session
            if turn == 0:
                self.call("detach", {"session": session})
                previous = None
        self.call("stop", {})
        self.proc.wait(timeout=10)
        result = json.loads((self.root / "result.json").read_text())["demo"]
        self.assertEqual(self.proc.returncode, 0, result)
        self.assertFalse(result["unsupported_ops"])
        self.assertGreater(result["requests"]["mediaType"], 0)

    @unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 with the pinned Python environment to drive Firefox")
    def test_browser_slow_fetch(self):
        from marionette_driver.marionette import Marionette
        import firefox_exit
        browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
            "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
        browser.start_session()
        try:
            browser.set_context("content"); browser.navigate(self.viewer_url)
            self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
            browser.switch_to_frame(browser.find_element("id", "page"))
            browser.find_element("id", "start").click()
            self.assertTrue(self.slow_requested.wait(2), "native fetch did not start")
            browser.find_element("id", "increment").click()
            self.until(lambda: browser.execute_script('return document.getElementById("count").textContent === "1"'), timeout=2)
            self.assertEqual(browser.execute_script('return document.getElementById("fetch").textContent'), "pending")
            browser.switch_to_frame()
            pid = self.call("runtimes")[0]["pid"]
            browser.find_element("id", "detach").click()
            browser.find_element("id", "attach").click()
            self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
            browser.switch_to_frame(browser.find_element("id", "page"))
            self.until(lambda: browser.execute_script('return document.getElementById("fetch").textContent === "255,0,65 true" && document.getElementById("xhr").textContent === "255,0,65"'), timeout=15)
            self.assertEqual(browser.execute_script('return document.getElementById("error").textContent'), "rejected")
            self.assertEqual(browser.execute_script('return document.getElementById("aborted").textContent'), "reused fast")
            browser.find_element("id", "increment").click()
            self.until(lambda: browser.execute_script('return document.getElementById("count").textContent === "2"'))
            self.assertEqual(self.call("runtimes")[0]["pid"], pid)
            self.assertEqual(len(self.slow_cookies), 3)
            self.assertTrue(all("slow=page" in c for c in self.slow_cookies), self.slow_cookies)
        finally:
            firefox_exit.close(browser)
        self.call("stop", {})
        self.proc.wait(timeout=10)
        result = json.loads((self.root / "result.json").read_text())["demo"]
        self.assertEqual(self.proc.returncode, 0, result)
        self.assertEqual(result["exit"], 0, result)
        self.assertEqual(result["js_errors"], 0, result)
        self.assertFalse(result["unsupported_ops"], result)
        self.assertEqual(result["bridge_tab"], {"made": False})

    @unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 with the pinned Python environment to drive Firefox")
    def test_browser_document_input(self):
        from marionette_driver.marionette import Marionette
        import firefox_exit
        browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
            "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
        browser.start_session()
        try:
            browser.set_context("content")
            browser.navigate(self.viewer_url)
            self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
            for count, text in enumerate(("first prompt", "second prompt"), 1):
                browser.switch_to_frame(browser.find_element("id", "page"))
                field = browser.find_element("id", "draft")
                field.click()
                field.send_keys(text)
                self.until(lambda: browser.execute_script('return document.getElementById("echo").textContent === arguments[0] && !document.getElementById("send").disabled', script_args=(text,)))
                browser.find_element("id", "send").click()
                self.until(lambda: browser.execute_script('return document.querySelectorAll("#submitted li").length === arguments[0] && document.getElementById("draft").value === "" && document.getElementById("send").disabled', script_args=(count,)))
                browser.switch_to_frame()
            browser.find_element("id", "detach").click()
            browser.find_element("id", "attach").click()
            self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
            browser.switch_to_frame(browser.find_element("id", "page"))
            self.assertEqual(browser.execute_script('return [...document.querySelectorAll("#submitted li")].map(e => e.textContent)'), ["first prompt", "second prompt"])
            self.assertEqual(browser.execute_script('return document.getElementById("draft").value'), "")
        finally:
            firefox_exit.close(browser)
        self.call("stop", {})
        self.proc.wait(timeout=10)
        result = json.loads((self.root / "result.json").read_text())["demo"]
        self.assertEqual(self.proc.returncode, 0, result)
        self.assertEqual(result["exit"], 0, result)
        self.assertEqual(result["js_errors"], 0, result)
        self.assertFalse(result["unsupported_ops"], result)
        self.assertEqual(result["bridge_tab"], {"made": False})

    @unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 to drive Firefox")
    def test_browser_rule_style(self):
        from marionette_driver.marionette import Marionette
        import firefox_exit
        browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
            "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
        browser.start_session()
        def shown(step):
            return browser.execute_script('''const p=getComputedStyle(document.querySelector('.probe'));
                return [document.getElementById('stage').textContent,p.width,p.height,
                        getComputedStyle(document.querySelector('.adopt')).opacity];''') == [
                            str(step), str(32 + step) + "px", str(33 + step) + "px",
                            "0.7" if step == 0 else ("0.8" if step % 2 else "0.4")]
        try:
            browser.set_context("content")
            browser.navigate(self.viewer_url)
            self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
            browser.switch_to_frame(browser.find_element("id", "page"))
            self.until(lambda: shown(0))
            for step in range(1, 7):
                browser.find_element("id", "advance").click()
                self.until(lambda: shown(step))
                if step == 3:
                    browser.switch_to_frame()
                    browser.find_element("id", "detach").click()
                    browser.find_element("id", "attach").click()
                    self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
                    browser.switch_to_frame(browser.find_element("id", "page"))
                    self.until(lambda: shown(step))
            self.assertEqual(browser.execute_script('return window.__log || null'), None, "page scripts ran in the viewer")
        finally:
            firefox_exit.close(browser)

    @unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 with the pinned Python environment to drive Firefox")
    def test_browser_svg_search(self):
        from marionette_driver.marionette import Marionette
        import firefox_exit
        browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
            "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
        browser.start_session()
        try:
            browser.set_context("content")
            browser.navigate(self.viewer_url)
            self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
            browser.switch_to_frame(browser.find_element("id", "page"))
            browser.find_element("id", "fallback").click()
            self.until(lambda: browser.execute_script('return !!document.getElementById("search")'))
            search = browser.find_element("id", "search")
            self.assertTrue(search.is_displayed())
            self.assertEqual(search.get_attribute("data-namespace"), "html")
            search.click(); search.send_keys("Wikipedia")
            self.until(lambda: browser.execute_script('return document.getElementById("echo").textContent === "Wikipedia"'))
            browser.switch_to_frame()
            browser.find_element("id", "detach").click()
            browser.find_element("id", "attach").click()
            self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
            browser.switch_to_frame(browser.find_element("id", "page"))
            self.assertEqual(browser.find_element("id", "search").get_property("value"), "Wikipedia")
        finally:
            firefox_exit.close(browser)
        self.call("stop", {})
        self.proc.wait(timeout=10)
        result = json.loads((self.root / "result.json").read_text())["demo"]
        self.assertEqual(self.proc.returncode, 0, result)
        self.assertEqual(result["js_errors"], 0, result)
        self.assertFalse(result["unsupported_ops"], result)
        self.assertEqual(result["bridge_tab"], {"made": False})

    @unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 with the pinned Python environment to drive Firefox")
    def test_browser_editable(self):
        from marionette_driver.marionette import Marionette
        import firefox_exit
        browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
            "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
        browser.start_session()
        try:
            browser.set_context("content")
            browser.navigate(self.viewer_url)
            self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
            browser.switch_to_frame(browser.find_element("id", "page"))
            editor=browser.find_element("id", "editor")
            editor.click()
            for count,c in enumerate("abcdef",1):
                editor.send_keys(c)
                self.until(lambda: browser.execute_script('return document.getElementById("model").textContent') == "abcdef"[:count])
                self.assertEqual(browser.execute_script('return document.getElementById("editor").textContent'), "abcdef"[:count])
            editor.send_keys("ghijklmnopqrstuvwxyz")
            self.until(lambda: browser.execute_script('return document.getElementById("model").textContent') == "abcdefghijklmnopqrstuvwxyz")
            def state(expected):
                self.until(lambda: browser.execute_script('return [document.getElementById("model").textContent,document.getElementById("editor").textContent]') == [expected,expected])
            def select(anchor,focus):
                browser.execute_script('const t=document.getElementById("editor").firstChild.firstChild;getSelection().setBaseAndExtent(t,arguments[0],t,arguments[1]);',script_args=(anchor,focus))
            from marionette_driver.keys import Keys
            select(3,3)
            editor.send_keys("XY")
            expected="abcXYdefghijklmnopqrstuvwxyz"
            state(expected)
            # Backward selection retains its direction through replacement text nodes.
            select(7,2)
            editor.send_keys("Q")
            expected=expected[:2]+"Q"+expected[7:]
            state(expected)
            editor.send_keys(Keys.BACK_SPACE)
            expected=expected[:2]+expected[3:]
            state(expected)
            select(2,2)
            editor.send_keys(Keys.DELETE)
            expected=expected[:2]+expected[3:]
            state(expected)
            select(len(expected),len(expected))
            editor.send_keys(" \U0001f642")
            expected+=" \U0001f642"
            state(expected)
            browser.switch_to_frame()
            browser.find_element("id","detach").click()
            browser.find_element("id","attach").click()
            self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
            browser.switch_to_frame(browser.find_element("id","page"))
            state(expected)
            editor=browser.find_element("id","editor")
            editor.click()
            select(len(expected.encode("utf-16-le"))//2,len(expected.encode("utf-16-le"))//2)
            editor.send_keys(" done")
            state(expected+" done")
            native=browser.find_element("id","native")
            native.click()
            native.send_keys("Native editor abc 123")
            self.until(lambda: browser.execute_script('return document.getElementById("native-model").textContent') == "Native editor abc 123")
            browser.switch_to_frame()
            browser.find_element("id","detach").click()
            browser.find_element("id","attach").click()
            self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
            browser.switch_to_frame(browser.find_element("id","page"))
            self.assertEqual(browser.execute_script('return document.getElementById("native").textContent'),"Native editor abc 123")


        finally:
            firefox_exit.close(browser)
        self.call("stop", {})
        self.proc.wait(timeout=20)
        result=json.loads((self.root / "result.json").read_text())["demo"]
        self.assertEqual(self.proc.returncode,0,result)
        self.assertEqual(result["exit"],0,result)
        self.assertEqual(result["js_errors"],0,result)
        self.assertFalse(result["unsupported_ops"],result)
        self.assertEqual(result["bridge_tab"],{"made":False})

    @unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 with the pinned Python environment to drive Firefox")
    def test_browser_frames(self):
        from marionette_driver.marionette import Marionette
        import firefox_exit
        browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
            "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
        browser.start_session()
        text = lambda id: browser.execute_script("return document.getElementById(arguments[0])?.textContent ?? null", script_args=(id,))
        connected = lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")')
        def into(*path):
            """The copy's document, then each frame of `path` in it."""
            browser.switch_to_frame()
            browser.switch_to_frame(browser.find_element("id", "page"))
            for id in path:
                browser.switch_to_frame(browser.find_element("id", id))
        def reached(path, id, value):
            def check():
                try:
                    into(*path)
                    return text(id) == value
                except Exception:
                    return False
            self.until(check)
        try:
            browser.set_context("content")
            browser.navigate(self.viewer_url)
            self.until(connected)
            reached(["child"], "child-state", "child true")
            reached(["child", "nested"], "nested-count", "0")
            # scripts disabled in every copied document; frames have no src; the service's origin
            into()
            service = browser.execute_script("return window.origin")
            self.assertEqual(browser.execute_script("return [window.__frameStarts ?? null, document.getElementById('child').hasAttribute('src'),"
                                                    " document.getElementById('child').contentWindow.origin]"), [None, False, service])
            for path in (["child"], ["inline"], ["child", "nested"]):
                into(*path)
                self.assertIsNone(browser.execute_script("return window.__frameStarts ?? null"), path)
            into("child")
            self.assertEqual(browser.execute_script('return getComputedStyle(document.getElementById("child-state")).color'), "rgb(150, 40, 20)")
            into("inline")
            self.assertEqual(browser.execute_script('return getComputedStyle(document.getElementById("inline-text")).color'), "rgb(10, 120, 30)")
            # trusted clicks in each displayed frame reach the window that owns the node
            into("child")
            browser.find_element("id", "send").click()
            reached(["child"], "child-reply", "reply 1 sum 6 owned true detached true")
            reached([], "focus", "true true")
            self.assertEqual([text("replies"), text("bytes")], ["1", "1,2,3"])
            into("inline")
            browser.find_element("id", "inline-button").click()
            reached(["inline"], "inline-count", "1 true")
            into("child", "nested")
            browser.find_element("id", "nested-button").click()
            reached(["child", "nested"], "nested-count", "1 true true")
            # reconnect: the frames' state is the runtime's, and input still reaches them
            browser.switch_to_frame()
            browser.find_element("id", "detach").click()
            browser.find_element("id", "attach").click()
            self.until(connected)
            reached(["child"], "child-reply", "reply 1 sum 6 owned true detached true")
            browser.find_element("id", "send").click()
            reached(["child"], "child-reply", "reply 2 sum 7 owned true detached true")
            reached([], "replies", "2")
        finally:
            firefox_exit.close(browser)
        self.call("stop", {})
        self.proc.wait(timeout=10)
        result = json.loads((self.root / "result.json").read_text())["demo"]
        self.assertEqual(self.proc.returncode, 0, result)
        self.assertEqual(result["js_errors"], 0, result)
        self.assertFalse(result["unsupported_ops"], result)
        self.assertEqual(result["bridge_tab"], {"made": False})

    @unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 with the pinned Python environment to drive Firefox")
    def test_browser_shadow_adopted(self):
        from marionette_driver.marionette import Marionette
        import firefox_exit
        browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
            "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
        browser.start_session()
        state = """return ['one','two','outside'].map(id => {
            const host=document.getElementById(id), el=host.shadowRoot?.querySelector('.probe') || host;
            return getComputedStyle(el).color;
        });"""
        rgb = lambda c: "rgb(%s)" % ", ".join(str(n) for n in c)
        try:
            browser.set_context("content")
            browser.navigate(self.viewer_url)
            self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
            browser.switch_to_frame(browser.find_element("id", "page"))
            self.assertEqual(browser.execute_script(state), [rgb((50,60,70)), rgb((20,30,40)), rgb((9,9,9))])
            expected = [(50,60,70), (50,60,70), (50,60,70), (80,90,100), (100,110,120), (1,2,3), (100,110,120), (160,170,180)]
            shared = [(100,110,120), (130,140,150), (100,110,120), (100,110,120), (100,110,120), (100,110,120), (100,110,120), (160,170,180)]
            for step, (a,b) in enumerate(zip(expected, shared), 1):
                browser.find_element("id", "advance").click()
                self.until(lambda: browser.execute_script(state) == [rgb(a), rgb(b), rgb((9,9,9))])
                self.assertEqual(browser.execute_script('return document.getElementById("stage").textContent'), str(step))
                if step == 7:
                    browser.switch_to_frame()
                    browser.find_element("id", "detach").click()
                    browser.find_element("id", "attach").click()
                    self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
                    browser.switch_to_frame(browser.find_element("id", "page"))
                    self.assertEqual(browser.execute_script(state), [rgb(a), rgb(b), rgb((9,9,9))])
            self.assertEqual(browser.execute_script('return document.getElementById("one").shadowRoot.children.length'), 2)
            self.assertEqual(browser.execute_script('return window.__shadowFixtureStarts || 0'), 0, "page scripts ran in viewer")
            # Exercise the driver's exact privileged consumer in its persistent Xray sandbox.
            # Extract its constant without importing the driver (which launches a session).
            import ast
            tree = ast.parse((HERE / "mini_realsites.py").read_text())
            value = next(n.value for n in tree.body if isinstance(n, ast.Assign)
                         and any(isinstance(t, ast.Name) and t.id == "MIRROR_APPLY" for t in n.targets))
            apply = (HERE / "http-viewer" / "adopted.js").read_text() + ast.literal_eval(value.right)
            browser.switch_to_frame()
            browser.navigate("about:blank")
            sheet = {"text": "p{color:rgb(20,30,40)}", "media": "", "disabled": False}
            child = {"i": 3, "n": "p", "a": [], "c": [{"i": 4, "t": "Shadow"}]}
            host = {"i": 1, "n": "div", "a": [["id", "host"]], "c": [],
                    "r": {"i": 2, "o": "open", "c": [child], "adopted": [sheet]}}
            def privileged(ops):
                result = browser.execute_script(apply, script_args=(ops,), sandbox="mirror", new_sandbox=False)
                self.assertEqual(result["refused"]["count"], 0)
            def color():
                return browser.execute_script('return getComputedStyle(document.getElementById("host").shadowRoot.firstChild).color')
            privileged([["c", 0, [host]]])
            self.assertEqual(color(), rgb((20,30,40)))
            sheet = dict(sheet, text="p{color:rgb(50,60,70)}")
            privileged([["adopted-root", 2, [sheet]], ["c", 2, [child]]])
            self.assertEqual(color(), rgb((50,60,70)))
            privileged([["adopted-root", 2, []]])
            self.assertEqual(browser.execute_script('return document.getElementById("host").shadowRoot.adoptedStyleSheets.length'), 0)
        finally:
            firefox_exit.close(browser)
        self.call("stop", {})
        self.proc.wait(timeout=10)
        result = json.loads((self.root / "result.json").read_text())["demo"]
        self.assertEqual(self.proc.returncode, 0, result)
        self.assertEqual(result["exit"], 0, result)
        self.assertEqual(result["js_errors"], 0, result)
        self.assertFalse(result["unsupported_ops"], result)
        self.assertEqual(result["bridge_tab"], {"made": False})

    @unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 with the pinned Python environment to drive Firefox")
    def test_browser(self):
        from marionette_driver.marionette import Marionette
        import firefox_exit
        browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
            "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
        browser.start_session()
        try:
            browser.set_context("content")
            browser.navigate(self.viewer_url)
            self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
            browser.switch_to_frame(browser.find_element("id", "page"))
            browser.find_element("id", "increment").click()
            browser.find_element("id", "draft").send_keys("a surviving draft")
            self.until(lambda: browser.execute_script('return document.getElementById("count").textContent === "1" && document.getElementById("echo").textContent === "a surviving draft"'))
            self.assertEqual(browser.execute_script('return window.__starts || 0'), 0, "page scripts ran in the viewer")
            self.assertEqual(browser.execute_script('return document.getElementById("starts").textContent'), "1")
            self.assertEqual(browser.execute_script('return document.getElementById("shadow").shadowRoot.textContent'), "Shadow tree: ready")
            self.assertEqual(browser.execute_script('return getComputedStyle(document.querySelector("h1")).color'), "rgb(35, 66, 120)")
            self.until(lambda: browser.execute_script('return getComputedStyle(document.getElementById("shadow")).borderLeftWidth === "3px"'))
            browser.execute_script('window.scrollTo(0, 500)')
            self.until(lambda: browser.execute_script('return Number(document.getElementById("scroll").textContent) > 0'))
            count_before = browser.execute_script('return Number(document.getElementById("ticks").textContent)')
            browser.switch_to_frame()
            pid = self.call("runtimes")[0]["pid"]
            spare = browser.open(type="tab")["handle"]
            browser.close()
            browser.switch_to_window(spare)
            time.sleep(1.2)
            new_tab = browser.open(type="tab", focus=True)["handle"]
            browser.switch_to_window(new_tab)
            browser.navigate(self.viewer_url)
            self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
            browser.switch_to_frame(browser.find_element("id", "page"))
            self.assertEqual(browser.execute_script('return document.getElementById("count").textContent'), "1")
            self.assertEqual(browser.execute_script('return document.getElementById("draft").value'), "a surviving draft")
            self.assertGreater(browser.execute_script('return Number(document.getElementById("ticks").textContent)'), count_before)
            self.assertGreater(browser.execute_script('return window.scrollY'), 0)
            browser.find_element("id", "increment").click()
            self.until(lambda: browser.execute_script('return document.getElementById("count").textContent === "2"'))
            browser.switch_to_frame()
            browser.set_window_rect(width=1100, height=800)
            try:
                self.until(lambda: browser.execute_script('const f=document.getElementById("page");return f.contentDocument.getElementById("size").textContent === f.clientWidth+" × "+f.clientHeight'))
            except AssertionError:
                print("VIEWER STATE", browser.execute_script('const f=document.getElementById("page");return {status:document.getElementById("status").textContent, size:f.contentDocument.getElementById("size").textContent, viewport:[f.clientWidth,f.clientHeight]}'))
                print("RUNTIME LOG", (self.root / "logs" / "demo.log").read_text()[-6000:])
                raise
            self.assertEqual(self.call("runtimes")[0]["pid"], pid)
            # Viewport text can arrive before B170's geometry acknowledgement.
            self.until(lambda: browser.execute_script('return document.getElementById("page").style.pointerEvents === "auto"'))
            browser.switch_to_frame(browser.find_element("id", "page"))
            browser.find_element("id", "increment").click()
            self.until(lambda: browser.execute_script('return document.getElementById("count").textContent === "3"'))
            browser.switch_to_frame()
            if os.environ.get("VIEWER_SHOT"):
                import base64
                Path(os.environ["VIEWER_SHOT"]).write_bytes(base64.b64decode(browser.screenshot()))
        finally:
            firefox_exit.close(browser)

    @unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 with the pinned Python environment to drive Firefox")
    def test_browser_webgl(self):
        from marionette_driver.marionette import Marionette
        import firefox_exit
        log = self.root / "logs" / "demo.log"
        # before any viewer: the API process's context (GAPS B169) drew the real pixels
        self.until(lambda: log.exists() and "WEBGL_READY" in log.read_text())
        self.assertIn("WEBGL_PIXELS 255,0,0,255", log.read_text())
        pid = self.call("runtimes")[0]["pid"]
        browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
            "webgl.force-enabled": True, "webgl.disabled": False,
            "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
        browser.start_session()
        sample = '''const c=document.getElementById("page").contentDocument.getElementById("surface");
          if(!c) return null;const gl=c.getContext("webgl"); if(!gl) return null;
          const p=new Uint8Array(4);gl.readPixels(8,8,1,1,gl.RGBA,gl.UNSIGNED_BYTE,p);return Array.from(p);'''
        try:
            browser.set_context("content")
            browser.navigate(self.viewer_url)
            self.until(lambda: browser.execute_script(sample) == [255,0,0,255])
            self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
            browser.switch_to_frame(browser.find_element("id", "page"))
            browser.find_element("id", "read").click()
            self.until(lambda: browser.execute_script('return document.getElementById("pixels").textContent') == "255,0,0,255")
            self.assertEqual(browser.execute_script('return document.getElementById("losses").textContent'), "0")
            browser.switch_to_frame()
            # A fresh snapshot must preserve the actual context and its pixels.
            browser.set_window_rect(width=1050, height=760)
            self.until(lambda: browser.execute_script(sample) == [255,0,0,255])
            spare = browser.open(type="tab")["handle"]
            browser.close(); browser.switch_to_window(spare)
            self.until(lambda: "WEBGL_DETACHED_DRAW" in log.read_text())
            self.assertIn("WEBGL_PIXELS 0,255,0,255", log.read_text())
            # Connect in a fresh tab: shader, typed buffer and stable uniform id
            # were created before the first viewer existed and replay again.
            tab = browser.open(type="tab", focus=True)["handle"]
            browser.switch_to_window(tab); browser.navigate(self.viewer_url)
            self.until(lambda: browser.execute_script(sample) == [0,255,0,255])
            self.assertEqual(self.call("runtimes")[0]["pid"], pid)
            self.assertEqual(browser.execute_script('return document.getElementById("page").contentDocument.getElementById("losses").textContent'), "0")
            browser.execute_script('document.getElementById("page").contentDocument.getElementById("surface").getContext("webgl").getExtension("WEBGL_lose_context").loseContext()')
            self.until(lambda: browser.execute_script('return document.getElementById("page").contentDocument.getElementById("losses").textContent') == "1")
        finally:
            firefox_exit.close(browser)
        self.call("stop", {}); self.proc.wait(timeout=10)
        result = json.loads((self.root / "result.json").read_text())["demo"]
        self.assertEqual(self.proc.returncode, 0, result)
        self.assertEqual(result["js_errors"], 0)
        self.assertGreater(result["viewer"]["webgl_native"], 0)
        self.assertEqual(result["viewer"]["webgl_dummy"], 0)
        self.assertGreater(result["viewer"]["webgl_replays"], 0)
        self.assertFalse(result["unsupported_ops"])

    @unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 with the pinned Python environment to drive Firefox")
    def test_browser_delayed_scroll(self):
        from marionette_driver.marionette import Marionette
        import firefox_exit
        log = self.root / "logs" / "demo.log"
        self.until(lambda: log.exists() and "SCROLL_STATE 700" in log.read_text())
        browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
            "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
        browser.start_session()
        try:
            browser.set_context("content")
            pid = self.call("runtimes")[0]["pid"]
            wanted = 700
            for turn in range(3):
                self.css_requested.clear(); self.css_release.clear()
                browser.navigate(self.viewer_url)
                self.assertTrue(self.css_requested.wait(5), "viewer did not request delayed stylesheet")
                if turn == 1:
                    browser.set_window_rect(width=1100, height=800)
                browser.switch_to_frame(browser.find_element("id", "page"))
                self.until(lambda: browser.execute_script('return document.getElementById("poke") !== null'))
                self.assertEqual(browser.execute_script('return scrollY'), 0)
                # Focus is trusted in Firefox and carries the viewer offset just
                # as real input does. It cannot overwrite saved scroll while
                # resources prevent the snapshot from reaching that offset.
                browser.execute_script('document.getElementById("poke").focus()')
                time.sleep(0.2)
                self.css_release.set()
                self.until(lambda: browser.execute_script('return scrollY') == wanted)
                browser.switch_to_frame()
                self.until(lambda: browser.execute_script('return document.getElementById("page").style.pointerEvents === "auto"'))
                browser.switch_to_frame(browser.find_element("id", "page"))
                self.assertEqual(browser.execute_script('return document.getElementById("scroll").textContent'), str(wanted))
                # A subsequent user scroll must be forwarded normally.
                wanted += 100
                browser.execute_script('window.scrollTo(0, arguments[0])', script_args=[wanted])
                self.until(lambda: browser.execute_script('return document.getElementById("scroll").textContent') == str(wanted))
                browser.switch_to_frame()
                self.assertEqual(self.call("runtimes")[0]["pid"], pid)
                spare = browser.open(type="tab")["handle"]
                browser.close(); browser.switch_to_window(spare)
                tab = browser.open(type="tab", focus=True)["handle"]
                browser.switch_to_window(tab)
            self.assertNotIn("SCROLL_STATE 0", log.read_text())
        finally:
            self.css_release.set()
            firefox_exit.close(browser)
        self.call("stop", {}); self.proc.wait(timeout=10)
        result = json.loads((self.root / "result.json").read_text())["demo"]
        self.assertEqual(self.proc.returncode, 0, result)
        self.assertEqual(result["js_errors"], 0)


    def media_get(self, url, rng):
        try:
            with urlopen(Request(url, headers={"Range": rng}), timeout=10) as response:
                return response.status, dict(response.headers), response.read()
        except HTTPError as e:
            with e:
                return e.code, dict(e.headers), e.read()

    def test_media_resources(self):
        """GAPS B172: media srcs go to the viewer as service URLs, loaded with the page's cookies."""
        log = self.root / "logs" / "demo.log"
        self.until(lambda: log.exists() and "BLOB_SET" in log.read_text())
        session, batches = self.snapshot()
        videos = {spec["a"][0][1] if spec["a"] and spec["a"][0][0] == "id" else None: dict(spec["a"])
                  for spec in self.specs(batches).values() if spec.get("n") in ("video", "audio")}
        for name in ("cookie", "blob", "cors", "allowed"):
            self.assertTrue(videos[name]["src"].startswith(self.origin + "/media/"), videos[name])
        data = (HERE / "bridge-cases" / "media-plain.mp4").read_bytes()
        status, headers, body = self.media_get(videos["cookie"]["src"], "bytes=0-")
        self.assertEqual((status, headers["Content-Range"], body), (206, f"bytes 0-{len(data) - 1}/{len(data)}", data))
        cookie = [r for r in self.media_requests if r["path"] == "media-auth.mp4"]
        first = cookie[0]
        self.assertEqual((first["Cookie"], first["Range"], first["Sec-Fetch-Dest"], first["Sec-Fetch-Mode"]),
                         ("mediaauth=page-secret", "bytes=0-8191", "video", "no-cors"))
        self.assertTrue(first["Referer"].endswith("/media-origin.html"))
        self.assertTrue(first["Accept"].startswith("video/webm"))
        self.assertEqual(len(cookie), 6)  # 41 KB in 8 KB ranges
        status, _, body = self.media_get(videos["blob"]["src"], "bytes=100-199")
        self.assertEqual((status, body), (206, data[100:200]))  # revoked by the page once set
        self.assertEqual(self.media_get(videos["cors"]["src"], "bytes=0-")[0], 502)  # no Access-Control-Allow-Origin
        status, _, body = self.media_get(videos["allowed"]["src"], "bytes=8000-8999")
        self.assertEqual((status, body), (206, data[8000:9000]))  # Content-Range read past CORS's filter
        allowed = [r for r in self.media_requests if r["path"] == "media-cors.mp4"]
        self.assertEqual((allowed[0]["Sec-Fetch-Mode"], allowed[0]["Cookie"], allowed[0]["Origin"].startswith("http://localhost:")),
                         ("cors", None, True))
        self.call("detach", {"session": session})
        count = len(self.media_requests)
        self.assertEqual(self.media_get(videos["cookie"]["src"], "bytes=0-")[0], 409)
        self.assertEqual(len(self.media_requests), count)

    @unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 with the pinned Python environment to drive Firefox")
    def test_browser_media_resources(self):
        """GAPS B172 in Firefox: the cookie-gated and runtime-Blob videos decode in the viewer and
        play; the CORS-refused audio errors, in the viewer and in the runtime; all survive reconnect."""
        from marionette_driver.marionette import Marionette
        import firefox_exit
        log = self.root / "logs" / "demo.log"
        self.until(lambda: log.exists() and "BLOB_SET" in log.read_text())
        browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
            "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
        browser.start_session()
        loaded = 'const v=document.getElementById(arguments[0]);return v.readyState>=2&&v.videoWidth>0&&!v.error'
        try:
            for attempt in range(2):
                if attempt:
                    # reconnect from a new tab: the same handles load again
                    spare = browser.open(type="tab", focus=True)["handle"]; browser.close(); browser.switch_to_window(spare)
                browser.set_context("content"); browser.navigate(self.viewer_url)
                self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
                browser.switch_to_frame(browser.find_element("id", "page"))
                for name in ("cookie", "blob"):
                    self.until(lambda: browser.execute_script(loaded, script_args=(name,)), timeout=20)
                self.until(lambda: browser.execute_script('const a=document.getElementById("allowed");return a.readyState>=1&&a.duration>0&&!a.error'))
                self.until(lambda: browser.execute_script('const a=document.getElementById("cors");return !!a.error'))
                browser.switch_to_frame()
            self.until(lambda: "EVENT cookie loadedmetadata" in log.read_text() and "EVENT blob loadedmetadata" in log.read_text()
                       and "EVENT cors error" in log.read_text())
            self.assertGreater(len(self.media_requests), 1)
            self.assertTrue(all(r["Cookie"] == "mediaauth=page-secret" and r["Range"] for r in self.media_requests if r["path"] == "media-auth.mp4"))
            browser.switch_to_frame(browser.find_element("id", "page"))
            self.assertTrue(browser.execute_script('return document.getElementById("cookie").src.includes("/media/")'))
            browser.execute_script('document.getElementById("cookie").play()')
            self.until(lambda: browser.execute_script('return document.getElementById("cookie").currentTime > 0.3'))
        except Exception:
            print("RUNTIME MEDIA LOG", log.read_text()[-8000:], self.media_requests[-5:]); raise
        finally:
            firefox_exit.close(browser)

    @unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 with the pinned Python environment to drive Firefox")
    def test_browser_media(self):
        from marionette_driver.marionette import Marionette
        import firefox_exit
        log = self.root / "logs" / "demo.log"
        self.until(lambda: log.exists() and "MEDIA_INITIAL" in log.read_text())
        self.assertIn("MEDIA_INITIAL false", log.read_text())
        browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
            "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
        browser.start_session()
        try:
            browser.set_context("content"); browser.navigate(self.viewer_url)
            self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
            browser.switch_to_frame(browser.find_element("id", "page"))
            self.until(lambda: browser.execute_script('return document.getElementById("log").textContent.includes("MSE_READY")'), timeout=20)
            browser.find_element("id", "play").click()
            self.until(lambda: browser.execute_script('return document.getElementById("log").textContent.includes("PLAY_OK")'))
            self.until(lambda: browser.execute_script('return document.getElementById("mse").currentTime > 0.5'))
            browser.find_element("id", "pause").click()
            self.until(lambda: browser.execute_script('return document.getElementById("mse").paused'))
            browser.find_element("id", "seek").click()
            self.until(lambda: browser.execute_script('return Math.abs(document.getElementById("mse").currentTime-2)<0.1'))
            self.until(lambda: browser.execute_script('return document.getElementById("state").textContent.includes("2.0")'))
            browser.find_element("id", "remove").click()
            self.until(lambda: browser.execute_script('return document.getElementById("log").textContent.includes("REMOVE_OK")'))
            # Hold the real append in the old backend; only replay can complete it.
            browser.execute_script('window.wrappedJSObject.SourceBuffer.prototype.appendBuffer = function() {};')
            browser.find_element("id", "append").click()
            self.until(lambda: "APPEND_PENDING" in log.read_text())
            browser.find_element("id", "pending").click()
            self.until(lambda: browser.execute_script('return document.getElementById("log").textContent.includes("PENDING_START")'))
            browser.switch_to_frame()
            pid = self.call("runtimes")[0]["pid"]
            browser.set_window_rect(width=1100, height=800)
            self.until(lambda: browser.execute_script('const v=document.getElementById("page").contentDocument.getElementById("mse");return v&&v.readyState>=3&&Math.abs(v.currentTime-2)<0.1'))
            spare=browser.open(type="tab")["handle"]; browser.close(); browser.switch_to_window(spare)
            self.until(lambda: "PENDING_ERROR AbortError" in log.read_text())
            self.assertNotIn("SOURCE_CLOSED", log.read_text())
            self.assertNotIn("UNEXPECTED_PENDING_PLAY", log.read_text())
            tab=browser.open(type="tab",focus=True)["handle"]; browser.switch_to_window(tab); browser.navigate(self.viewer_url)
            self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
            browser.switch_to_frame(browser.find_element("id", "page"))
            self.until(lambda: browser.execute_script('const p=document.getElementById("plain");return p.readyState>=1&&Math.abs(p.currentTime-1)<0.1&&p.paused'))
            self.until(lambda: browser.execute_script('const v=document.getElementById("mse");return v.readyState>=3&&v.paused&&Math.abs(v.currentTime-2)<0.1'), timeout=20)
            self.assertTrue(browser.execute_script('const v=document.getElementById("mse");return v.buffered.length>0&&v.videoWidth>0'))
            self.until(lambda: "APPEND_RECOVERED" in log.read_text())
            self.assertEqual(log.read_text().count("APPEND_RECOVERED"), 1)
            self.assertTrue(browser.execute_script('const r=document.getElementById("mse").buffered;return r.length===2&&r.end(r.length-1)>3.9'))
            self.assertEqual(log.read_text().count("MSE_READY"), 1)
            self.until(lambda: browser.execute_script('return window.frameElement.style.pointerEvents === "auto"'))
            browser.find_element("id", "play").click()
            self.until(lambda: browser.execute_script('const v=document.getElementById("mse");return !v.paused&&v.currentTime>2.1'))
            browser.switch_to_frame()
            # Replacement without an observed detached wait must also reconstruct.
            browser.navigate(self.viewer_url)
            self.until(lambda: browser.execute_script('const v=document.getElementById("page").contentDocument.getElementById("mse");return v&&v.readyState>=3&&!v.paused&&v.currentTime>2.1'), timeout=20)
            self.assertEqual(log.read_text().count("MSE_READY"), 1)
            self.assertNotIn("SOURCE_CLOSED", log.read_text())
            self.assertEqual(self.call("runtimes")[0]["pid"],pid)
        except Exception:
            print("RUNTIME MEDIA LOG", log.read_text()[-8000:]); raise
        finally:
            firefox_exit.close(browser)

    # ---- navigation (DESIGN "Top-level navigation in the HTTP viewer") ----

    def document(self, viewport=(800, 600)):
        """Attach to the runtime's current document: (session, its last sequence number, its
        elements' specs by id)"""
        session, batches = self.snapshot(viewport)
        seq = batches[-1]["seq"]
        self.call("input", {"session": session, "ack": seq, "events": []})
        return session, seq, {dict(s.get("a", [])).get("id"): s for s in self.specs(batches).values() if "n" in s}

    @staticmethod
    def text(spec):
        return "".join(c["t"] if "t" in c else Runtime.text(c) for c in spec.get("c", []) if isinstance(c, dict))

    def info(self, elements):
        return json.loads(self.text(elements["info"]))

    def send_input(self, session, seq, el, kind="click", task=1, **extra):
        iface = {"click": "MouseEvent", "keypress": "KeyboardEvent", "input": "InputEvent"}[kind]
        ev = {"type": kind, "iface": iface, "target": el["i"], "active": el["i"], "task": task, "bubbles": True,
              "cancelable": kind != "input", "composed": True, "fields": extra.pop("fields", {"button": 0} if kind == "click" else {})}
        ev.update(extra)
        self.call("input", {"session": session, "ack": seq, "events": [ev]})

    def navigated(self, session, timeout=30):
        """Wait until a navigation replaced `session`'s document, then attach to the new one"""
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            try:
                self.call("poll?session=" + session + "&after=999999")
            except HTTPError as e:
                message = json.load(e)["error"]
                e.close()
                self.assertTrue(message.startswith("Document replaced by a navigation"), message)
                return self.document()
        self.fail("no navigation; driver log: " + (self.root / "driver.log").read_text()[-3000:])

    def notice(self, session, seq, contains, timeout=15):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            reply = self.call("poll?session=" + session + "&after=" + str(seq))
            if contains in (reply.get("notice") or {}).get("text", ""):
                return reply["notice"]["text"]
        self.fail("no notice containing " + contains)

    @staticmethod
    def children(pid):
        """The names of a process's child processes"""
        out = []
        for d in os.listdir("/proc"):
            try:
                with open(f"/proc/{d}/stat") as f:
                    stat = f.read()
            except (OSError, ValueError):
                continue
            name, rest = stat[stat.index("(") + 1:stat.rindex(")")], stat[stat.rindex(")") + 2:].split()
            if d.isdigit() and int(rest[1]) == pid and rest[0] != "Z":
                out.append(name)
        return out

    def requests_for(self, step):
        return [r for r in self.nav_requests if f"step={step}" in r["path"]]

    def test_fragment_scroll(self):
        pid = self.call("runtimes")[0]["pid"]
        session, batches = self.snapshot()
        self.assertEqual([op[3] for b in batches for op in b["ops"] if op[0] == "scroll"], [1200])
        task = 0

        def state():
            return json.loads(self.text(self.document()[2]["state"]))

        def click(name):
            nonlocal task
            session, seq, elements = self.document()
            task += 1
            self.send_input(session, seq, elements[name], task=task)

        def at(y):
            self.until(lambda: state()["y"] == y)

        click("encoded"); at(1600)
        self.assertEqual(state(), {"y": 1600, "hash": "#caf%C3%A9", "length": 2, "hashes": 1})
        click("reset"); at(300)
        click("encoded"); at(1600)
        self.assertEqual((state()["length"], state()["hashes"]), (2, 1))
        click("named"); at(2000)
        click("literal"); at(2400)
        click("missing")
        self.until(lambda: state()["hash"] == "#missing")
        self.assertEqual(state()["y"], 2400)
        click("cancel"); click("report")
        self.assertEqual(state()["hash"], "#missing")
        click("top"); at(0)
        click("assign"); at(2000)
        click("replace"); at(1200)
        self.assertEqual(state()["length"], 7)
        click("reset"); at(300)
        click("back"); at(0)
        click("forward"); at(300)
        click("push"); click("report")
        self.until(lambda: state()["hash"] == "#caf%C3%A9")
        self.assertEqual(state()["y"], 300, "pushState must not scroll to a fragment")
        click("assign"); at(2000)
        click("empty"); at(0)
        click("manual"); click("back"); click("report")
        self.until(lambda: state()["hash"] == "#legacy")
        self.assertEqual(state()["y"], 0, "manual restoration must leave scroll unchanged")
        self.assertEqual(self.call("runtimes")[0]["pid"], pid)
        self.call("stop", {}); self.proc.wait(timeout=10)
        result = json.loads((self.root / "result.json").read_text())["demo"]
        self.assertEqual(result["js_errors"], 0, result)

    @unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 with the pinned Python environment to drive Firefox")
    def test_browser_fragment_scroll(self):
        from marionette_driver.marionette import Marionette
        import firefox_exit
        browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
            "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
        browser.start_session()
        try:
            browser.set_context("content")
            # Establish the same target positions in an ordinary reference document.
            browser.navigate(f"http://localhost:{self.server.server_port}/fragment-scroll.html#initial")
            self.until(lambda: browser.execute_script('return scrollY') == 1200)
            for name, y in (("encoded", 1600), ("reset", 300), ("encoded", 1600), ("named", 2000), ("literal", 2400)):
                browser.find_element("id", name).click()
                self.until(lambda: browser.execute_script('return scrollY') == y)

            def at(y):
                self.until(lambda: browser.execute_script('return scrollY') == y)
                browser.switch_to_frame()
                self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
                browser.switch_to_frame(browser.find_element("id", "page"))

            for turn in range(2):
                browser.navigate(self.viewer_url)
                self.until(lambda: browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'))
                browser.switch_to_frame(browser.find_element("id", "page"))
                at(1200 if turn == 0 else 2000)
                if turn == 0:
                    browser.find_element("id", "encoded").click()
                    at(1600)
                    browser.find_element("id", "reset").click()
                    at(300)
                    browser.find_element("id", "encoded").click()
                    at(1600)
                    browser.find_element("id", "named").click()
                    at(2000)
                browser.switch_to_frame()
        finally:
            firefox_exit.close(browser)

    def test_navigation(self):
        port = self.server.server_port
        local, other = f"http://localhost:{port}", f"http://127.0.0.1:{port}"
        pid = self.call("runtimes")[0]["pid"]
        a, seq, el = self.document()
        # in the document: a fragment link (an entry of its own), a click the page cancels
        self.send_input(a, seq, el["frag"])
        self.send_input(a, seq, el["cancelled"], task=2)
        self.until(lambda: (lambda d: self.text(d[2]["hashes"]) == "#part" and self.text(d[2]["state"]) == "cancelled click stayed")(self.document()))
        a, seq, el = self.document()
        # refused and failed ones keep the document and say why
        for name, says in (("blank", "new window"), ("broken", "failed"), ("nocontent", "HTTP 204"), ("json", "application/json")):
            self.send_input(a, seq, el[name])
            self.notice(a, seq, says)
        self.assertEqual(self.call("runtimes")[0]["pid"], pid)
        self.assertFalse(self.requests_for("cancelled") + self.requests_for("blank"))
        self.send_input(a, seq, el["increment"], task=3)
        self.until(lambda: self.text(self.document()[2]["count"]) == "1")
        # a same-origin link: a new runtime
        a, seq, el = self.document()
        self.send_input(a, seq, el["same"])
        b, seq, el = self.navigated(a)
        same = self.call("runtimes")[0]
        self.assertNotEqual(same["pid"], pid)
        self.assertEqual(same["url"], f"{local}/nav-echo?step=same")
        self.assertEqual(self.info(el), {"href": f"{local}/nav-echo?step=same", "referrer": f"{local}/nav-start", "length": 3,
                                         "ss": "start", "type": 0, "origin": local})
        r = self.requests_for("same")[-1]
        self.assertEqual((r["Sec-Fetch-Dest"], r["Sec-Fetch-Mode"], r["Sec-Fetch-Site"], r["Sec-Fetch-User"], r["Upgrade-Insecure-Requests"]),
                         ("document", "navigate", "same-origin", "?1", "1"))
        self.assertIn("strict=1", r["Cookie"])
        self.assertIn("lax=1", r["Cookie"])
        self.assertTrue(r["Accept"].startswith("text/html"))
        # the old session's input is refused
        with self.assertRaises(HTTPError) as error:
            self.call("input", {"session": a, "ack": 0, "events": []})
        self.assertEqual(error.exception.code, 409)
        error.exception.close()
        # a script's assign to another origin (in an input task: the user's)
        self.send_input(b, seq, el["cross"])
        c, seq, el = self.navigated(b)
        self.assertEqual(self.info(el), {"href": f"{other}/nav-echo?step=cross", "referrer": f"{local}/", "length": 4,
                                         "ss": None, "type": 0, "origin": other})
        r = self.requests_for("cross")[-1]
        self.assertEqual((r["host"], r["Sec-Fetch-Site"], r["Sec-Fetch-User"], r["Cookie"]), (f"127.0.0.1:{port}", "cross-site", "?1", None))
        # a cross-site link back: Lax cookies, not Strict ones
        self.send_input(c, seq, el["sameSite"])
        d, seq, el = self.navigated(c)
        self.assertEqual(self.info(el)["length"], 5)
        r = self.requests_for("lax")[-1]
        self.assertEqual((r["Sec-Fetch-Site"], "lax=1" in r["Cookie"], "strict=1" in r["Cookie"]), ("cross-site", True, False))
        # back: the earlier document's URL loaded again, with its first load's referrer
        self.send_input(d, seq, el["back"])
        e, seq, el = self.navigated(d)
        self.assertEqual(self.info(el), {"href": f"{other}/nav-echo?step=cross", "referrer": f"{local}/", "length": 5,
                                         "ss": "+cross", "type": 2, "origin": other})
        # a cross-site POST form: neither SameSite kind
        self.send_input(e, seq, el["submit"])
        f, seq, el = self.navigated(e)
        self.assertEqual(self.info(el)["length"], 5)
        self.assertEqual(self.info(el)["ss"], "start+same+lax")
        r = self.requests_for("post")[-1]
        self.assertEqual((r["method"], r["body"], r["Content-Type"], r["Origin"], r["Sec-Fetch-Site"]),
                         ("POST", "a=1&b=x+y%0D%0Az&r=&s=go", "application/x-www-form-urlencoded", other, "cross-site"))
        self.assertNotIn("lax=1", r["Cookie"] or "")
        self.assertNotIn("strict=1", r["Cookie"] or "")
        # replace through a redirect, then reload: the entry count stays
        self.send_input(f, seq, el["replace"])
        g, seq, el = self.navigated(f)
        self.assertEqual(self.info(el)["href"], f"{local}/nav-echo?step=redirected")
        self.assertEqual(self.info(el)["length"], 5)
        self.assertTrue(any(x["path"].startswith("/nav-redirect") for x in self.nav_requests))
        self.send_input(g, seq, el["reload"])
        h, seq, el = self.navigated(g)
        self.assertEqual((self.info(el)["type"], self.info(el)["length"], self.info(el)["href"]), (1, 5, f"{local}/nav-echo?step=redirected"))
        # the old documents' runtimes and API processes are gone: one runtime, and the API process of
        # the one site the last document used
        children = self.children(self.proc.pid)
        self.assertEqual(sorted(children), ["mrt-api", "realsite"], children)
        self.call("stop", {})
        self.proc.wait(timeout=20)
        result = json.loads((self.root / "result.json").read_text())["demo"]
        self.assertEqual(self.proc.returncode, 0, result)
        self.assertEqual(result["exit"], 0, result)
        self.assertEqual(result["js_errors"], 0, result)
        self.assertFalse(result["unsupported_ops"], result)
        self.assertEqual(result["viewer"]["navigations"], 7, result["navigations"])
        self.assertEqual(result["final_url"], f"{local}/nav-echo?step=redirected")
        kinds = [n.get("refused") or ("error" if "error" in n else n["how"]) for n in result["navigations"]]
        self.assertEqual(kinds, ["new window _blank", "error", "error", "error", "push", "push", "push", "traverse", "push", "replace", "reload"])
        self.assertEqual(len({n["pid"] for n in result["navigations"] if "pid" in n}), 7)

    def test_navigation_toolbar_commands(self):
        session,seq,el=self.document()
        pid=self.call("runtimes")[0]["pid"]
        # Each command completes even at a history boundary. A replacement
        # viewer may reuse a task number, so completion cannot be state-change-only.
        for _ in range(2):
            self.call("input",{"session":session,"ack":seq,"events":[{"type":"viewer-history","delta":-1,"task":1}]})
            reply=self.call(f"poll?session={session}&after={seq}")
            ops=[op for batch in reply["batches"] for op in batch["ops"]]
            self.assertIn(["history",0,1,1],ops)
            seq=reply["batches"][-1]["seq"]
            self.call("input",{"session":session,"ack":seq,"events":[]})
            self.assertEqual(self.call("runtimes")[0]["pid"],pid)
        self.call("stop",{})
        self.proc.wait(timeout=20)
        result=json.loads((self.root/"result.json").read_text())["demo"]
        self.assertEqual(self.proc.returncode,0,result)
        self.assertEqual(result["exit"],0,result)
        self.assertEqual(result["js_errors"],0,result)
        self.assertFalse(result["unsupported_ops"],result)

    def test_navigation_startup_redirects(self):
        local = f"http://localhost:{self.server.server_port}"
        session, seq, elements = self.document()
        self.send_input(session, seq, elements["startup"])
        final = local + "/nav-echo?step=startup-final"
        self.until(lambda: self.call("runtimes")[0]["url"] == final)
        session, seq, elements = self.document()
        info = self.info(elements)
        self.assertEqual((info["href"], info["length"], info["ss"]), (final, 2, "start+one+two"))
        self.assertEqual(info["referrer"], local + "/nav-startup?step=two")
        requests = [r for r in self.nav_requests if r["path"] in
            ("/nav-startup?step=one", "/nav-startup?step=two", "/nav-echo?step=startup-final")]
        self.assertEqual([r["path"] for r in requests],
            ["/nav-startup?step=one", "/nav-startup?step=two", "/nav-echo?step=startup-final"])
        self.assertEqual([r["Referer"] for r in requests],
            [local + "/nav-start", local + "/nav-startup?step=one", local + "/nav-startup?step=two"])
        self.assertEqual([r["Sec-Fetch-User"] for r in requests], ["?1", None, None])
        # A refused redirect keeps the document that initiated it, with its
        # scripts and ordinary input still usable after the startup request.
        self.send_input(session, seq, elements["home"])
        session, seq, elements = self.navigated(session)
        self.send_input(session, seq, elements["startup-failure"])
        self.until(lambda: self.call("runtimes")[0]["url"] == local + "/nav-startup?step=bad")
        session, seq, elements = self.document()
        self.notice(session, seq, "application/json")
        self.assertEqual(self.text(elements["state"]), "startup-bad")
        self.send_input(session, seq, elements["increment"])
        self.until(lambda: self.text(self.document()[2]["count"]) == "1")
        self.assertEqual(self.children(self.proc.pid).count("realsite"), 1)
        self.assertEqual(self.children(self.proc.pid).count("mrt-api"), 1)
        self.call("stop", {})
        self.proc.wait(timeout=10)
        result = json.loads((self.root / "result.json").read_text())["demo"]
        self.assertEqual(self.proc.returncode, 0, result)
        self.assertEqual(result["js_errors"], 0, result)
        self.assertEqual(result["viewer"]["navigations"], 5)
        self.assertEqual(result["viewer"]["navigation_failures"], 1)

    def test_navigation_forms(self):
        port = self.server.server_port
        a, seq, el = self.document()
        # implicit submission: Enter in the search field clicks the form's button
        self.send_input(a, seq, el["q"], kind="keypress", fields={"key": "Enter", "keyCode": 13})
        b, seq, el = self.navigated(a)
        self.assertEqual(self.info(el)["href"], f"http://localhost:{port}/nav-echo?step=get&q=a+b%26c")
        self.assertEqual(self.requests_for("get")[-1]["method"], "GET")
        # from the start page each time: a multipart form (whose required field, left empty, first
        # stops the submission) and a text/plain one
        session, seq, el = b, seq, el
        for link, enctype in (("multipart", "multipart/form-data"), ("plain", "text/plain")):
            self.send_input(session, seq, el["home"])
            s1, seq1, el1 = self.navigated(session)
            self.send_input(s1, seq1, el1[link])
            s2, seq2, el2 = self.navigated(s1)
            before = len(self.requests_for("post"))
            if link == "multipart":
                self.send_input(s2, seq2, el2["submit"])
                time.sleep(1.5)
                self.assertEqual(len(self.requests_for("post")), before, "submitted with a required field empty")
                self.send_input(s2, seq2, el2["required"], kind="input", task=2, value=["filled", False])
            self.send_input(s2, seq2, el2["submit"], task=3)
            session, seq, el = self.navigated(s2)
            r = self.requests_for("post")[-1]
            self.assertTrue(r["Content-Type"].startswith(enctype), r)
            if link == "multipart":
                boundary = r["Content-Type"].split("boundary=")[1]
                self.assertIn(f'--{boundary}\r\nContent-Disposition: form-data; name="a"\r\n\r\n1\r\n', r["body"])
                self.assertIn('name="r"\r\n\r\nfilled\r\n', r["body"])
                self.assertTrue(r["body"].endswith(f"--{boundary}--\r\n"))
            else:
                self.assertEqual(r["body"], "a=1\r\nb=x y\r\nz\r\nr=\r\ns=go\r\n")

    @unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 with the pinned Python environment to drive Firefox")
    def test_browser_navigation(self):
        from marionette_driver.marionette import Marionette
        from marionette_driver.keys import Keys
        import firefox_exit
        port = self.server.server_port
        local, other = f"http://localhost:{port}", f"http://127.0.0.1:{port}"
        browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
            "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
        browser.start_session()
        status = lambda: browser.execute_script('return document.getElementById("status").textContent')

        def shown(test):
            """Wait until the viewer is connected and its frame's document passes `test` (a JS body)"""
            def check():
                browser.switch_to_frame()
                if not status().startswith("Connected"):
                    return False
                browser.switch_to_frame(browser.find_element("id", "page"))
                return browser.execute_script(test)
            waiting(check, 30)

        def waiting(check, timeout):
            """until(), with the viewer's and the driver's state when it times out"""
            try:
                self.until(check, timeout)
            except AssertionError:
                browser.switch_to_frame()
                seen = browser.execute_script('const d = document.getElementById("page").contentDocument; '
                                              'return [document.getElementById("status").textContent, d.documentElement.outerHTML.slice(0, 200)]')
                self.call("stop", {})
                self.proc.wait(timeout=20)
                result = json.loads((self.root / "result.json").read_text())["demo"]
                raise AssertionError(f"viewer status and frame {seen!r}; requests {[r['path'] for r in self.nav_requests]}; "
                                     f"navigations {result.get('navigations')} viewer {result.get('viewer')}; runtime logs "
                                     + "".join(p.name + ": " + p.read_text()[-1500:] for p in sorted((self.root / "logs").glob("demo*.log"))))

        def info():
            return browser.execute_script('return JSON.parse(document.getElementById("info").textContent)')

        def click(element_id):
            browser.find_element("id", element_id).click()
        try:
            browser.set_context("content")
            browser.navigate(self.viewer_url)
            shown('return !!document.getElementById("same")')
            pids = [self.call("runtimes")[0]["pid"]]
            # a link
            click("same")
            shown('return !!document.getElementById("info")')
            self.assertEqual(info()["href"], f"{local}/nav-echo?step=same")
            # the viewer's frame itself never left the service, and runs no page script
            browser.switch_to_frame()
            self.assertEqual(browser.execute_script('return document.getElementById("page").contentWindow.location.pathname'), "/frame")
            pids.append(self.call("runtimes")[0]["pid"])
            # a script's assign to another origin
            browser.switch_to_frame(browser.find_element("id", "page"))
            click("cross")
            on = lambda key, value: f'const i = document.getElementById("info"); return !!i && JSON.parse(i.textContent).{key}.includes({json.dumps(value)})'
            shown(on("origin", other))
            self.assertEqual(info()["referrer"], f"{local}/")
            # a POST form's submit button
            click("submit")
            shown(on("href", "step=post"))
            self.assertEqual(self.requests_for("post")[-1]["method"], "POST")
            # Enter in the start page's search field
            click("home")
            shown('return !!document.getElementById("q")')
            field = browser.find_element("id", "q")
            field.click()
            field.send_keys(" d" + Keys.ENTER)
            shown('return !!document.getElementById("info")')
            self.assertEqual(info()["href"], f"{local}/nav-echo?step=get&q=a+b%26c+d")
            # failures and refusals: the document stays, the status line says why
            click("home")
            shown('return !!document.getElementById("broken")')
            click("broken")
            browser.switch_to_frame()
            waiting(lambda: "failed" in status(), 30)
            browser.switch_to_frame(browser.find_element("id", "page"))
            click("blank")
            browser.switch_to_frame()
            waiting(lambda: "new window" in status(), 15)
            start_pid = self.call("runtimes")[0]["pid"]
            # the viewer tab closed, and a new one attached: the current document, same runtime
            browser.switch_to_frame()
            first = browser.current_window_handle
            browser.switch_to_window(browser.open("tab")["handle"])
            browser.navigate(self.viewer_url)
            browser.switch_to_window(first)
            browser.close()
            browser.switch_to_window(browser.window_handles[0])
            shown('return !!document.getElementById("broken")')
            self.assertEqual(self.call("runtimes")[0]["pid"], start_pid)
            self.assertNotEqual(pids[0], pids[1])
            # and it navigates as before
            click("same")
            shown('return !!document.getElementById("info")')
            if os.environ.get("VIEWER_SHOT"):
                browser.switch_to_frame()
                Path(os.environ["VIEWER_SHOT"]).write_bytes(base64.b64decode(browser.screenshot()))
        finally:
            firefox_exit.close(browser)
        self.call("stop", {})
        self.proc.wait(timeout=20)
        result = json.loads((self.root / "result.json").read_text())["demo"]
        self.assertEqual(self.proc.returncode, 0, result)
        self.assertEqual(result["exit"], 0, result)
        self.assertEqual(result["js_errors"], 0, result)
        self.assertFalse(result["unsupported_ops"], result)
        self.assertEqual(result["bridge_tab"], {"made": False})
        self.assertEqual(result["viewer"]["navigations"], 7, result["navigations"])

    @unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 with the pinned Python environment to drive Firefox")
    def test_browser_navigation_empty_document(self):
        from marionette_driver.marionette import Marionette
        import firefox_exit
        browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
            "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
        browser.start_session()

        def shown(condition):
            browser.switch_to_frame()
            self.until(lambda: browser.execute_script(
                'const f=document.getElementById("page"),d=f.contentDocument;return '
                'document.getElementById("status").textContent.startsWith("Connected") && '
                'f.style.pointerEvents === "auto" && (' + condition + ')'), 10)

        def click(element):
            browser.switch_to_frame(browser.find_element("id", "page"))
            browser.find_element("id", element).click()
            browser.switch_to_frame()

        try:
            browser.set_context("content")
            browser.navigate(self.viewer_url)
            shown('!!d.getElementById("empty-document")')
            click("empty-document")
            shown('!d.documentElement && !document.getElementById("back").disabled')
            pid = self.call("runtimes")[0]["pid"]
            # Reconnect while the document remains empty, then recover through outer history UI.
            browser.navigate(self.viewer_url)
            shown('!d.documentElement && !document.getElementById("back").disabled')
            self.assertEqual(self.call("runtimes")[0]["pid"], pid)
            browser.find_element("id", "back").click()
            shown('!!d.getElementById("recover-document")')
            click("recover-document")
            shown('!d.documentElement && !document.getElementById("back").disabled')
            shown('!!d.getElementById("recovered")')
            click("recovered")
            shown('d.getElementById("count").textContent === "1"')
            browser.find_element("id", "back").click()
            shown('!!d.getElementById("empty-document")')
        finally:
            firefox_exit.close(browser)
        self.call("stop", {}); self.proc.wait(timeout=10)
        result = json.loads((self.root / "result.json").read_text())["demo"]
        self.assertEqual(self.proc.returncode, 0, result)
        self.assertFalse(result["unsupported_ops"], result)
        self.assertEqual(result["viewer"]["navigations"], 4, result)

    @unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 with the pinned Python environment to drive Firefox")
    def test_browser_navigation_controls(self):
        from marionette_driver.marionette import Marionette
        import firefox_exit
        browser=Marionette(bin=os.environ["FIREFOX_BIN"],prefs={"marionette.allowSystemAccess":True,
            "browser.aboutwelcome.enabled":False,"browser.shell.checkDefaultBrowser":False})
        browser.start_session()
        local=f"http://localhost:{self.server.server_port}"
        other=f"http://127.0.0.1:{self.server.server_port}"
        def shown(suffix,back,forward):
            browser.switch_to_frame()
            def check():
                return browser.execute_script("""return document.getElementById('status').textContent.startsWith('Connected')
                    && document.getElementById('status').textContent.includes(arguments[0])
                    && document.getElementById('back').disabled===arguments[1]
                    && document.getElementById('forward').disabled===arguments[2];""",script_args=(suffix,not back,not forward))
            self.until(check,30)
            self.assertEqual(browser.get_url(),self.viewer_url)
            self.assertEqual(browser.execute_script("return document.getElementById('page').contentWindow.location.pathname"),"/frame")
        def page_click(id):
            browser.switch_to_frame(browser.find_element("id","page"))
            browser.find_element("id",id).click()
            browser.switch_to_frame()
        def toolbar(id):
            browser.find_element("id",id).click()
        try:
            browser.set_context("content");browser.navigate(self.viewer_url)
            shown(local+"/nav-start",False,False)
            browser.execute_script("""const toolbar=document.querySelector('header');
              toolbar.setAttribute('data-history-violation','');
              new MutationObserver(()=>{
                const status=document.getElementById('status').textContent;
                if ((status.startsWith('Synchronizing')||status.startsWith('Loading')) &&
                    (!document.getElementById('back').disabled||!document.getElementById('forward').disabled))
                  toolbar.setAttribute('data-history-violation',status);
              }).observe(toolbar,{subtree:true,childList:true,characterData:true,attributes:true,attributeFilter:['disabled']});""")
            pid=self.call("runtimes")[0]["pid"]
            page_click("frag")
            shown(local+"/nav-start#part",True,False)
            toolbar("back")
            shown(local+"/nav-start",False,True)
            self.assertEqual(self.call("runtimes")[0]["pid"],pid)
            toolbar("forward")
            shown(local+"/nav-start#part",True,False)
            toolbar("back")
            shown(local+"/nav-start",False,True)
            # Pushing another document drops the forward fragment entry.
            page_click("same")
            shown(local+"/nav-echo?step=same",True,False)
            self.assertNotEqual(self.call("runtimes")[0]["pid"],pid)
            toolbar("back")
            shown(local+"/nav-start",False,True)
            pid=self.call("runtimes")[0]["pid"]
            toolbar("detach")
            self.assertTrue(browser.execute_script("return document.getElementById('back').disabled"))
            self.assertTrue(browser.execute_script("return document.getElementById('forward').disabled"))
            toolbar("attach")
            shown(local+"/nav-start",False,True)
            self.assertEqual(self.call("runtimes")[0]["pid"],pid)
            toolbar("forward")
            shown(local+"/nav-echo?step=same",True,False)
            page_click("cross")
            shown(other+"/nav-echo?step=cross",True,False)
            toolbar("back")
            shown(local+"/nav-echo?step=same",True,True)
            page_click("replace")
            shown(local+"/nav-echo?step=redirected",True,True)
            page_click("home")
            shown(local+"/nav-start",True,False)
            page_click("scroll-page")
            self.until(lambda: browser.execute_script("return document.getElementById('page').contentWindow.scrollY===200"))
            shown(local+"/nav-start",True,False)
            self.assertEqual(browser.execute_script("return document.querySelector('header').getAttribute('data-history-violation')"),"")
            page_click("broken")
            self.until(lambda: browser.execute_script("return document.getElementById('status').textContent.includes('failed')"),30)
            shown(local+"/nav-start",True,False)
            # A cancelled history traversal must acknowledge the command too.
            page_click("leave-guard")
            self.until(lambda: browser.execute_script("return document.getElementById('page').contentDocument.getElementById('leave-guard').textContent.endsWith('on')"))
            pid=self.call("runtimes")[0]["pid"]
            toolbar("back")
            self.until(lambda: browser.execute_script("return document.getElementById('status').textContent.includes('beforeunload canceled')"),30)
            shown(local+"/nav-start",True,False)
            self.assertEqual(self.call("runtimes")[0]["pid"],pid)
        finally:
            firefox_exit.close(browser)
        self.call("stop",{})
        self.proc.wait(timeout=20)
        result=json.loads((self.root/"result.json").read_text())["demo"]
        self.assertEqual(self.proc.returncode,0,result)
        self.assertEqual(result["exit"],0,result)
        self.assertEqual(result["js_errors"],0,result)
        self.assertFalse(result["unsupported_ops"],result)
        self.assertEqual(result["bridge_tab"],{"made":False})

    def until(self, predicate, timeout=10):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if predicate():
                return
            time.sleep(0.05)
        self.fail("browser condition timed out")


if __name__ == "__main__":
    unittest.main()
