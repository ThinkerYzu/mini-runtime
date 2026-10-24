#!/usr/bin/env python3
"""Measure text in a real Firefox, to compare with `cargo run --example textdump`.

Starts a headless Firefox (selected by FIREFOX_BIN) on a throwaway profile,
talks to it over the Marionette socket directly (no marionette_driver, so no Firefox python
environment is needed), and prints, for one font family and size, the width of each test string
and the font metrics layout uses (line box height, and the ascent/descent of the font as seen through
an inline-block baseline probe).

usage: firefox_text.py [family] [size-px] [weight]      env FIREFOX_BIN overrides the binary
"""
import json, os, shutil, socket, subprocess, sys, tempfile, time

BIN = os.environ["FIREFOX_BIN"]
FAMILY = sys.argv[1] if len(sys.argv) > 1 else "Noto Sans"
SIZE = float(sys.argv[2]) if len(sys.argv) > 2 else 16
WEIGHT = sys.argv[3] if len(sys.argv) > 3 else "400"
PORT = 2899
STRINGS = ["Hello world", "AV", "The quick brown fox jumps over the lazy dog", "office fi fl",
           "Wikipedia, the free encyclopedia", "W.A.V.E. To", " "]

class Marionette:
    def __init__(self, port):
        for _ in range(100):
            try:
                self.s = socket.create_connection(("127.0.0.1", port)); break
            except OSError:
                time.sleep(0.3)
        else:
            raise SystemExit("cannot connect to Marionette")
        self.f = self.s.makefile("rb"); self.n = 0
        self._read()  # hello
    def _read(self):
        n = b""
        while True:
            c = self.f.read(1)
            if c == b":": break
            n += c
        return json.loads(self.f.read(int(n)))
    def call(self, name, params=None):
        self.n += 1
        msg = json.dumps([0, self.n, name, params or {}]).encode()
        self.s.sendall(b"%d:%s" % (len(msg), msg))
        r = self._read()
        if r[2]: raise RuntimeError(r[2])
        return r[3]

JS = """
const [fam, size, weight, strings] = arguments;
const mk = (css) => { const e = document.createElement('div'); e.style.cssText = css; document.body.appendChild(e); return e; };
const base = `font-family:'${fam}'; font-size:${size}px; font-weight:${weight}; line-height:normal; position:absolute; white-space:pre; margin:0; padding:0;`;
const out = {widths: {}};
for (const s of strings) { const e = mk(base); e.textContent = s; out.widths[s] = e.getBoundingClientRect().width; }
const line = mk(base); line.textContent = 'Hx'; out.lineHeight = line.getBoundingClientRect().height;
// ascent: distance from the top of the line to the baseline of a zero-height inline-block
const probe = mk(base); probe.innerHTML = 'Hx<span style="display:inline-block;width:1px;height:0"></span>';
out.baselineFromTop = probe.querySelector('span').getBoundingClientRect().bottom - probe.getBoundingClientRect().top;
// ex and ch units in px
const ex = mk(base + 'width:1ex'); out.ex = ex.getBoundingClientRect().width;
const ch = mk(base + 'width:1ch'); out.ch = ch.getBoundingClientRect().width;
const sp = mk(base); sp.textContent = ' '; out.space = sp.getBoundingClientRect().width;
return out;
"""

def main():
    prof = tempfile.mkdtemp(prefix="mrt-text-ff-")
    open(os.path.join(prof, "user.js"), "w").write(
        'user_pref("marionette.port", %d);\nuser_pref("marionette.allowSystemAccess", true);\n'
        'user_pref("app.update.enabled", false);\nuser_pref("datareporting.policy.dataSubmissionEnabled", false);\n'
        'user_pref("browser.shell.checkDefaultBrowser", false);\nuser_pref("services.settings.server", "http://localhost:1/");\n'
        'user_pref("network.trr.mode", 5);\nuser_pref("app.normandy.enabled", false);\n' % PORT)
    env = dict(os.environ, MOZ_HEADLESS="1")
    p = subprocess.Popen([BIN, "--headless", "-marionette", "-no-remote", "-profile", prof, "about:blank"],
                         env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        m = Marionette(PORT)
        m.call("WebDriver:NewSession", {"capabilities": {}})
        m.call("WebDriver:Navigate", {"url": "data:text/html,<body></body>"})
        r = m.call("WebDriver:ExecuteScript", {"script": JS, "args": [FAMILY, SIZE, WEIGHT, STRINGS]})["value"]
        print(json.dumps(r, indent=1))
        m.call("Marionette:Quit", {"flags": ["eForceQuit"]})
    finally:
        try: p.wait(10)
        except Exception: p.kill()
        shutil.rmtree(prof, ignore_errors=True)

main()
