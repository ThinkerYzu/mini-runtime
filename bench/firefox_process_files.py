"""How much of each Firefox process's unique memory is its binaries' pages (GAPS A16, DESIGN "The
mini process's memory, attributed"): release Firefox on a test page served from localhost, then
every process's unique memory (Private_Clean + Private_Dirty, /proc/PID/smaps) split into
file-backed, by file, and anonymous. Content processes come from the fork server, which has already
loaded and relocated libxul, so they share those pages and their file-backed share is small.

usage (under Xvfb): python3 bench/firefox_process_files.py [PAGE.html]   (default wikipedia.html)
"""
import collections, http.server, os, shutil, subprocess, sys, tempfile, threading, time


def split(pid):
    """A process's unique memory (Private_Clean + Private_Dirty, /proc/PID/smaps) in bytes:
    `file_backed` with `files` by name (basename; memfd and the like by their name too) and
    `anonymous`. Used by bench/firefox_realsites.py at its live measuring point."""
    agg = collections.Counter()
    cur = None
    for line in open(f"/proc/{pid}/smaps"):
        parts = line.split()
        if "-" in parts[0] and ":" not in parts[0] and len(parts) >= 5:
            path = parts[5] if len(parts) > 5 else ""
            cur = ("file:" + os.path.basename(path)) if path.startswith("/") else "anon"
        elif parts[0] in ("Private_Clean:", "Private_Dirty:"):
            agg[cur] += int(parts[1]) * 1024
    files = {k[5:]: v for k, v in agg.most_common() if k.startswith("file:") and v}
    return {"unique": sum(agg.values()), "file_backed": sum(files.values()), "anonymous": agg["anon"], "files": files}


def main():
    FF = os.path.expanduser("~/.cache/mrt-firefox-release/firefox/firefox")
    PAGES = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "pages")

    class Quiet(http.server.SimpleHTTPRequestHandler):
        def __init__(self, *a, **k): super().__init__(*a, directory=PAGES, **k)
        def log_message(self, *a): pass

    srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Quiet)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    prof = tempfile.mkdtemp()
    open(os.path.join(prof, "user.js"), "w").write('user_pref("browser.shell.checkDefaultBrowser", false);\nuser_pref("datareporting.policy.dataSubmissionEnabled", false);\nuser_pref("browser.aboutwelcome.enabled", false);\nuser_pref("app.update.disabledForTesting", true);\n')
    page = sys.argv[1] if len(sys.argv) > 1 else "wikipedia.html"
    # As Marionette starts it: a crash leaves its dump in the profile's minidumps/ (reported below)
    # instead of opening the crash reporter, which files it with the owner's own Firefox's reports.
    # Without WAYLAND_DISPLAY, as replay.py does: Firefox would open on the desktop, not under Xvfb.
    # TMPDIR inside the profile: Firefox leaves a Remote Settings bundle (a 4 MB <uuid>.zip) in it
    # (task 69a4), so it goes with the profile
    os.mkdir(os.path.join(prof, "tmp"))
    env = dict(os.environ, MOZ_CRASHREPORTER="1", MOZ_CRASHREPORTER_NO_REPORT="1", MOZ_CRASHREPORTER_SHUTDOWN="1",
               TMPDIR=os.path.join(prof, "tmp"))
    env.pop("WAYLAND_DISPLAY", None)
    p = subprocess.Popen([FF, "--no-remote", "--profile", prof, f"http://127.0.0.1:{srv.server_address[1]}/{page}"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env)
    time.sleep(25)

    def descendants(pid):
        out = [int(x) for x in subprocess.run(["pgrep", "-P", str(pid)], capture_output=True, text=True).stdout.split()]
        return out + [d for k in out for d in descendants(k)]

    for pid in [p.pid] + descendants(p.pid):
        cmd = open(f"/proc/{pid}/cmdline").read().replace("\0", " ")
        if pid == p.pid:
            kind = "main"
        elif "-contentproc" in cmd:
            kind = cmd.split()[-1]
        else:  # a helper program, such as GTK's image loaders (bwrap, glycin-*)
            kind = os.path.basename(cmd.split()[0])
        r = split(pid)
        top = ", ".join(f"{k} {v / 1048576:.1f}" for k, v in r["files"].items() if v > 1048576)
        print(f"pid {pid} {kind}: unique {r['unique'] / 1048576:.1f} MB, file-backed {r['file_backed'] / 1048576:.1f} ({top}), anonymous {r['anonymous'] / 1048576:.1f}")
    p.terminate()
    p.wait()
    srv.shutdown()
    dumps = os.path.join(prof, "minidumps")
    if os.path.isdir(dumps) and any(f.endswith(".dmp") for f in os.listdir(dumps)):
        kept = shutil.copytree(dumps, tempfile.mkdtemp(prefix="firefox-crash-"), dirs_exist_ok=True)
        print(f"Firefox crashed: its dumps are kept in {kept}")
    shutil.rmtree(prof, ignore_errors=True)


if __name__ == "__main__":
    main()
