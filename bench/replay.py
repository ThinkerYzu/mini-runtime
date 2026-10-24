"""Runs a site through mitmproxy with bench/replay_addon.py, for bench/firefox_realsites.py and
bench/mini_realsites.py (DESIGN "Replay: the same content on both sides").

  REPLAY=record|fill|replay   off when unset
  REPLAY_DIR=dir              the recordings (default bench/realsites-replay; one directory per site)
  REPLAY_DETERMINISTIC=0      no deterministic.js in replay
  REPLAY_TIMING=instant       answers from the recording sent at once (before session 70); by default
                              Firefox's are held for their recorded duration and the mini runtime's carry
                              it in a header (SPEC decision 20)
  REPLAY_INJECT=file          a probe script put after deterministic.js at the top of each HTML document
                              (fill and replay; firefox_probe.py's counterpart of MRT_EVAL_BEFORE)
  MITMDUMP=path               default bench/.venv-mitm/bin/mitmdump
"""
import os, socket, subprocess, time

HERE = os.path.dirname(os.path.abspath(__file__))
# The harnesses run Firefox under xvfb-run; from a Wayland session Firefox would open on the desktop
# instead (it prefers WAYLAND_DISPLAY to xvfb-run's DISPLAY), at another window size
os.environ.pop("WAYLAND_DISPLAY", None)
MODE = os.environ.get("REPLAY") or None
ROOT = os.environ.get("REPLAY_DIR", os.path.join(HERE, "realsites-replay"))
MITMDUMP = os.environ.get("MITMDUMP", os.path.join(HERE, ".venv-mitm", "bin", "mitmdump"))
if MODE not in (None, "record", "fill", "replay"):
    raise SystemExit(f"REPLAY={MODE}: record, fill or replay")


def timing(harness):
    """How the proxy gives a recorded answer its recorded duration (SPEC decision 20): Firefox waits for
    it; the mini runtime, whose bridge calls block and whose clock is virtual, gets it in a header."""
    if os.environ.get("REPLAY_TIMING") == "instant":
        return "instant"
    return "header" if harness == "mini" else "hold"


class Proxy:
    """mitmdump for one site and one harness run; `prefs` and `capabilities` point Firefox at it."""

    def __init__(self, site, harness):
        self.dir = os.path.join(ROOT, site)
        os.makedirs(self.dir, exist_ok=True)
        self.log = os.path.join(self.dir, f"{harness}-{MODE}.jsonl")
        if os.path.exists(self.log):
            os.remove(self.log)
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            self.port = s.getsockname()[1]
        args = [MITMDUMP, "-q", "--listen-host", "127.0.0.1", "--listen-port", str(self.port),
                "-s", os.path.join(HERE, "replay_addon.py"),
                "--set", f"replay_mode={MODE}", "--set", f"replay_dir={self.dir}", "--set", f"replay_log={self.log}",
                "--set", f"replay_deterministic={'false' if os.environ.get('REPLAY_DETERMINISTIC') == '0' else 'true'}",
                "--set", f"replay_timing={timing(harness)}",
                "--set", f"replay_inject={os.path.abspath(os.environ['REPLAY_INJECT']) if os.environ.get('REPLAY_INJECT') else ''}",
                # bodies over 50 MB (video) are streamed through and not kept in the recording
                "--set", "stream_large_bodies=50m"]
        self.stderr = open(os.path.join(self.dir, f"{harness}-{MODE}.mitm.log"), "w")
        self.proc = subprocess.Popen(args, stdout=self.stderr, stderr=subprocess.STDOUT)
        for _ in range(100):
            try:
                socket.create_connection(("127.0.0.1", self.port), timeout=0.2).close()
                break
            except OSError:
                if self.proc.poll() is not None:
                    raise RuntimeError(f"mitmdump exited ({self.proc.returncode}); see {self.stderr.name}")
                time.sleep(0.1)
        self.prefs = {
            "network.proxy.type": 1,
            "network.proxy.http": "127.0.0.1", "network.proxy.http_port": self.port,
            "network.proxy.ssl": "127.0.0.1", "network.proxy.ssl_port": self.port,
            "network.proxy.no_proxies_on": "",
            # what would bypass the proxy or answer without it
            "network.http.http3.enable": False,
            "browser.cache.disk.enable": False,
            "dom.serviceWorkers.enabled": False,
        }
        # the proxy's certificates (Marionette turns off certificate checks for the session)
        self.capabilities = {"acceptInsecureCerts": True}

    def close(self):
        self.proc.terminate()
        try:
            self.proc.wait(10)
        except subprocess.TimeoutExpired:
            self.proc.kill()
        self.stderr.close()


def start(site, harness):
    """A Proxy for the site when REPLAY is set, else None."""
    return Proxy(site, harness) if MODE else None
