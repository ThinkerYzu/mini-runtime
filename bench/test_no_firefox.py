"""Integration checks for NO_FIREFOX=1. Build realsite and mrt-api first, then:

    python3 bench/test_no_firefox.py

Runs with Python -S (no Marionette/site packages), no display, a nonexistent Firefox
binary and an empty profile. All traffic goes to a local HTTP server.
"""
import functools
import http.server
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import unittest

import ws_echo

HERE = Path(__file__).resolve().parent


class NoFirefox(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory(prefix="mrt-no-firefox-")
        cls.root = Path(cls.tmp.name)
        # A deliberately small environment fixture, not a browser compatibility claim.
        entry = {"navigator": {"userAgent": "mrt-test", "languages": ["en-US"], "platform": "Linux"},
                 "names": {k: [] for k in ("window", "Document", "HTMLElement", "Element", "Node", "Navigator")},
                 "screen": {"width": 800, "height": 600}}
        cls.recording = cls.root / "env.json"
        cls.recording.write_text(json.dumps({"viewport": [800, 600], "entries": {"secure=1,isolated=0": entry}}))
        cls.root.joinpath("network.html").write_text('''<!doctype html><script src="source.js"></script>
<script>
(async () => {
  document.cookie = "test=api; Path=/";
  const body = await (await fetch("source.js")).text();
  const xhr = new XMLHttpRequest();
  xhr.open("GET", "source.js");
  xhr.onload = () => console.log("NETWORK " + JSON.stringify([
    window.externalScript, body.includes("externalScript"), xhr.status,
    document.cookie.includes("test=api")]));
  xhr.send();
})();
</script>''')
        cls.root.joinpath("source.js").write_text('window.externalScript = "loaded";')
        cls.root.joinpath("webgl.html").write_text('''<!doctype html><script>
const gl = document.createElement("canvas").getContext("webgl");
const s = gl.createShader(gl.FRAGMENT_SHADER);
gl.shaderSource(s, "precision mediump float; void main() { gl_FragColor = vec4(1.0); }");
gl.compileShader(s);
gl.clearColor(1, 0, 0, 1);
gl.clear(gl.COLOR_BUFFER_BIT);
const px = new Uint8Array(4);
gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
console.log("WEBGL " + JSON.stringify([Array.from(px), gl.getShaderParameter(s, gl.COMPILE_STATUS), gl.getParameter(gl.VERSION), gl.getError()]));
</script>''')
        # Serve the established storage/crypto cases alongside the network fixture.
        for name in ("storage", "crypto", "websocket", "images", "images-api", "permissions-storage", "media-types", "idb", "idb-native", "fetch-bytes", "document-input"):
            cls.root.joinpath(name + ".html").write_bytes((HERE / "bridge-cases" / (name + ".html")).read_bytes())
        for name in ("images-a.png", "images-b.png", "images-bad.png",
                     "images-api.png", "images-api.jpg", "images-api.gif", "images-api.webp",
                     "images-api.bmp", "images-api.ico", "images-api-rotated.jpg"):
            cls.root.joinpath(name).write_bytes((HERE / "bridge-cases" / name).read_bytes())

        cls.root.joinpath("idb-native-worker.js").write_bytes((HERE / "bridge-cases/idb-native-worker.js").read_bytes())
        cls.root.joinpath("permissions-storage-worker.js").write_bytes((HERE / "bridge-cases/permissions-storage-worker.js").read_bytes())

        class Quiet(http.server.SimpleHTTPRequestHandler):
            def log_message(self, *args):
                pass

        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Quiet, directory=str(cls.root)))
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.ws = ws_echo.Server(("127.0.0.1", 0), ws_echo.Handler)
        cls.ws_thread = threading.Thread(target=cls.ws.serve_forever, daemon=True)
        cls.ws_thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join()
        cls.ws.shutdown()
        cls.ws.server_close()
        cls.ws_thread.join()
        cls.tmp.cleanup()

    def run_page(self, case, **overrides):
        url = f"http://localhost:{self.server.server_port}/{case}.html"
        if case in ("websocket", "images-api", "fetch-bytes"):
            url += f"?ws={self.ws.server_address[1]}"
        out = self.root / (case + "-result.json")
        env = {k: v for k, v in os.environ.items()
               if not k.startswith(("MRT_", "API_", "REALSITE", "REPLAY", "BRIDGE_", "MINI_"))
               and k not in ("INTERACTIVE", "INTERACTIVE_S", "RENDER", "HEAP_SCAN", "DISPLAY", "WAYLAND_DISPLAY")}
        env.update(NO_FIREFOX="1", FIREFOX_BIN="/nonexistent/firefox", API_ENV=str(self.recording),
                   REALSITES_PROFILE=str(self.root / "empty-profile"), ONLY=case,
                   EXTRA_SITES=json.dumps({case: url}), MINI_OUT=str(out), MINI_DIR=str(self.root / "logs"),
                   REALSITE_ARGS="--console", WALL_LIMIT="30")
        after = self.root / "after.js"
        after.write_text('console.log("CASELOG " + JSON.stringify(window.__log));')
        env["MRT_EVAL_AFTER"] = str(after)
        env.update(overrides)
        result = subprocess.run([sys.executable, "-S", str(HERE / "mini_realsites.py")],
                                env=env, capture_output=True, text=True, timeout=60)
        data = json.loads(out.read_text())[case] if out.exists() else {}
        return result, data

    def assert_standalone(self, result, data):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn("error", data, data)
        self.assertEqual(data["exit"], 0)
        self.assertEqual(data["js_errors"], 0)
        self.assertFalse(data["firefox_started"])
        self.assertEqual(data["bridge_tab"], {"made": False})
        self.assertEqual(data["tab_ops"], {})
        self.assertEqual(data["unsupported_ops"], {})

    def test_storage_and_crypto(self):
        for case, count in (("storage", 29), ("crypto", 100)):
            with self.subTest(case=case):
                result, data = self.run_page(case)
                self.assert_standalone(result, data)
                log = (self.root / "logs" / (case + ".log")).read_text()
                lines = next(json.loads(line.split("CASELOG ", 1)[1]) for line in log.splitlines() if "CASELOG " in line)
                self.assertEqual(len(lines), count)
                self.assertEqual(lines[-1], "done")
                self.assertGreater(data["api_process"]["calls"].get("storage" if case == "storage" else "crypto", 0), 0)

    def test_fetch_xhr_script_and_cookie(self):
        result, data = self.run_page("network")
        self.assert_standalone(result, data)
        log = (self.root / "logs" / "network.log").read_text()
        self.assertIn('NETWORK ["loaded",true,200,true]', log)
        self.assertGreaterEqual(data["api_process"]["calls"]["fetch"], 4)

    def test_fetch_preserves_response_bytes(self):
        result, data = self.run_page("fetch-bytes")
        self.assert_standalone(result, data)
        log = (self.root / "logs" / "fetch-bytes.log").read_text()
        lines = next(json.loads(line.split("CASELOG ", 1)[1]) for line in log.splitlines() if "CASELOG " in line)
        self.assertEqual(len(lines), 36, lines)
        self.assertEqual(lines[-1], "done")
        self.assertTrue(all(line.endswith(" true") for line in lines[:-1]), lines)

    def test_document_input_handlers(self):
        result, data = self.run_page("document-input")
        self.assert_standalone(result, data)
        log = (self.root / "logs" / "document-input.log").read_text()
        lines = next(json.loads(line.split("CASELOG ", 1)[1]) for line in log.splitlines() if "CASELOG " in line)
        self.assertEqual(len(lines), 16, lines)
        self.assertEqual(lines[-1], "done")
        self.assertTrue(all(line.endswith(" true") for line in lines[:-1]), lines)

    def test_webgl_in_the_api_process(self):
        # WebGL runs on mrt-api's own contexts (GAPS B169): real pixels and shader state, no Firefox
        result, data = self.run_page("webgl")
        self.assert_standalone(result, data)
        log = (self.root / "logs" / "webgl.log").read_text()
        self.assertIn('WEBGL [[255,0,0,255],true,"WebGL 1.0",0]', log)
        self.assertGreater(data["api_process"]["calls"]["webgl"], 0)

    def test_images(self):
        result, data = self.run_page("images")
        self.assert_standalone(result, data)
        log = (self.root / "logs" / "images.log").read_text()
        lines = next(json.loads(line.split("CASELOG ", 1)[1]) for line in log.splitlines() if "CASELOG " in line)
        self.assertEqual(lines[-1], "done")
        self.assertTrue(any(line.startswith("bad error ") for line in lines))
        self.assertTrue(any(line.startswith("blob load ") for line in lines))
        self.assertTrue(any(line.startswith("data load ") for line in lines))
        self.assertGreater(data["api_process"]["calls"]["image"], 0)

    def test_native_image_formats_and_network(self):
        result, data = self.run_page("images-api")
        self.assert_standalone(result, data)
        log = (self.root / "logs" / "images-api.log").read_text()
        lines = next(json.loads(line.split("CASELOG ", 1)[1]) for line in log.splitlines() if "CASELOG " in line)
        for ext in ("png", "jpg", "gif", "webp", "bmp", "ico"):
            self.assertIn(ext + " load 7 3", lines)
        for expected in ("rotated load 3 7", "svg-default load 300 150", "svg-zero load 0 7",
                         "anonymous-reject error 0 0", "credentialed-reject error 0 0",
                         "corp-reject error 0 0", "404-valid-image load 7 3", "opaque-fetch opaque 0 0"):
            self.assertIn(expected, lines)
        self.assertEqual(lines[-1], "done")

    def test_permissions_and_storage(self):
        result, data = self.run_page("permissions-storage")
        self.assert_standalone(result, data)
        log = (self.root / "logs" / "permissions-storage.log").read_text()
        lines = next(json.loads(line.split("CASELOG ", 1)[1]) for line in log.splitlines() if "CASELOG " in line)
        self.assertEqual(lines[-1], "done")
        self.assertFalse(any(line.startswith("uncaught") for line in lines), lines)
        self.assertGreater(data["api_process"]["calls"]["permissions"], 0)

    def test_media_types_without_playback_backend(self):
        result, data = self.run_page("media-types")
        self.assert_standalone(result, data)
        log = (self.root / "logs" / "media-types.log").read_text()
        lines = next(json.loads(line.split("MEDIA_TYPES ", 1)[1]) for line in log.splitlines() if "MEDIA_TYPES " in line)
        self.assertGreater(len(lines), 30)
        for line in lines:
            media_type, mse, video, audio = json.loads(line)
            self.assertEqual([mse, video, audio], [False, "", ""], media_type)
        self.assertGreater(data["api_process"]["calls"]["mediaType"], 0)

    def test_indexeddb(self):
        for case in ("idb", "idb-native"):
            with self.subTest(case=case):
                result, data = self.run_page(case)
                self.assert_standalone(result, data)
                log = (self.root / "logs" / (case + ".log")).read_text()
                lines = next(json.loads(line.split("CASELOG ", 1)[1]) for line in log.splitlines() if "CASELOG " in line)
                self.assertIn(lines[-1], ("done", '\"done\"'))
                self.assertFalse(any("uncaught" in line or "main threw" in line for line in lines), lines)
                self.assertGreater(data["api_process"]["calls"]["idb"], 0)

    def test_websocket(self):
        result, data = self.run_page("websocket")
        self.assert_standalone(result, data)
        self.assertGreater(data["api_process"]["calls"]["ws"], 0)
        log = (self.root / "logs" / "websocket.log").read_text()
        lines = next(json.loads(line.split("CASELOG ", 1)[1]) for line in log.splitlines() if "CASELOG " in line)
        self.assertIn("done", lines)

    def test_missing_context_fails(self):
        recording = self.root / "insecure-only.json"
        data = json.loads(self.recording.read_text())
        data["entries"]["secure=0,isolated=0"] = data["entries"].pop("secure=1,isolated=0")
        recording.write_text(json.dumps(data))
        result, data = self.run_page("network", API_ENV=str(recording))
        self.assertEqual(result.returncode, 1)
        self.assertIn("has no environment", data["error"])

    def test_missing_environment_fails(self):
        result, data = self.run_page("network", API_ENV=str(self.root / "missing.json"))
        self.assertEqual(result.returncode, 1)
        self.assertIn("invalid environment recording", data["error"])

    def test_interactive_rejected_without_marionette(self):
        result, _ = self.run_page("network", INTERACTIVE="1")
        self.assertEqual(result.returncode, 1)
        self.assertIn("cannot use INTERACTIVE", result.stderr)


if __name__ == "__main__":
    unittest.main()
