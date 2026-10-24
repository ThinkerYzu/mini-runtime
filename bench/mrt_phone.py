"""The mini runtime on a phone (MRT_ADB=1 in mini_realsites.py; task 6da8 of the tab-hibernation project).

realsite runs on the phone under /data/local/tmp/mrt, started by `adb shell`, its stdin and stdout
carried over adb, so mini_realsites.py serves it as it serves a local one. The bridge is release Fenix,
driven by Marionette over adb forward as the project's android/realsites_phone.py drives it (fresh
profile, the replay proxy reached through adb reverse with its CA trusted in the profile, a desktop
user agent so the recorded pages are what is asked for).

On the phone, next to realsite: libmozjs (stripped), fonts.txt (the phone's fonts as a table,
android/mrt_fonts.py; the runtime has no fc-match there) and memdump (android/memdump.c: a process's
resident anonymous pages, for the zstd scan, as heap_compress.py reads them on desktop). push() puts
them there; clean() removes the directory.
"""
import os, re, shlex, struct, subprocess, sys, threading, time

PROJECT_ANDROID = os.environ.get("MRT_PHONE_SUPPORT")
if not PROJECT_ANDROID:
    raise SystemExit("MRT_ADB needs an external Android harness; set MRT_PHONE_SUPPORT to its directory. The standalone HTTP viewer does not need it.")
sys.path.insert(0, PROJECT_ANDROID)
import realsites_phone  # noqa: E402  fresh_fenix, SET_PREFS, DESKTOP_UA, PORT
import heap_compress  # noqa: E402

ADB = realsites_phone.ADB
DIR = "/data/local/tmp/mrt"
PAGE = 4096
JS_OFF = "window.browser.browsingContext.allowJavascript = false;"


def adb(*args):
    return subprocess.run([ADB, *args], capture_output=True, text=True).stdout


def push(files):
    """The runtime's files to DIR: realsite, libmozjs-*.so, fonts.txt, memdump (local paths)."""
    adb("shell", "mkdir", "-p", DIR)
    for f in files:
        subprocess.run([ADB, "push", f, DIR + "/"], capture_output=True, check=True)


def clean():
    adb("shell", "rm", "-rf", DIR)


# PACKAGE=org.mozilla.geckoview_example: the bridge is GeckoView's example app (CI's aarch64 opt build,
# 138 or 158 in task 4898), which is debuggable, so `run-as` reads its content processes' smaps (task 4898); release Fenix's cannot be read
DEBUGGABLE = realsites_phone.PKG == "org.mozilla.geckoview_example"


def launch_geckoview():
    """geckoview_example with a wiped profile, one tab on example.com, Marionette on host port PORT
    (as android/minimize_smaps.py launches it)."""
    pkg = realsites_phone.PKG
    adb("shell", "am", "force-stop", pkg)
    adb("shell", "pm", "clear", pkg)
    adb("push", os.path.join(PROJECT_ANDROID, "cfg-default.yaml"), f"/data/local/tmp/{pkg}-geckoview-config.yaml")
    adb("shell", "am", "set-debug-app", "--persistent", pkg)
    adb("shell", "am", "start", "-W", "-a", "android.intent.action.VIEW", "-d", "https://example.com",
        "-n", f"{pkg}/.GeckoViewActivity")
    time.sleep(8)
    adb("forward", f"tcp:{realsites_phone.PORT}", "tcp:2828")


def bridge(proxy):
    """Fresh Fenix (or geckoview_example, see DEBUGGABLE) as the bridge: a marionette_driver session on
    its selected tab, through `proxy`."""
    from marionette_driver.marionette import Marionette
    if DEBUGGABLE:
        launch_geckoview()
    else:
        realsites_phone.fresh_fenix()
    # YouTube and Docs ask whether to open their app, which holds the bridge tab's navigation: "Stay in
    # Firefox" is tapped for the first minute, while the bridge tab is set up
    stop = threading.Event()
    threading.Thread(target=realsites_phone.stay_in_firefox, args=(stop,), daemon=True).start()
    threading.Timer(60, stop.set).start()
    m = Marionette(host="127.0.0.1", port=realsites_phone.PORT)
    m.start_session({"acceptInsecureCerts": True})
    prefs = {"media.autoplay.default": 5}
    if proxy:
        adb("reverse", f"tcp:{proxy.port}", f"tcp:{proxy.port}")
        prefs.update(proxy.prefs)
        prefs["general.useragent.override"] = realsites_phone.DESKTOP_UA
        pem = open(os.path.expanduser("~/.mitmproxy/mitmproxy-ca-cert.pem")).read()
        der = "".join(l for l in pem.splitlines() if not l.startswith("-----"))
        m.set_context("chrome")
        m.execute_script('Cc["@mozilla.org/security/x509certdb;1"].getService(Ci.nsIX509CertDB).addCertFromBase64(arguments[0], "C,,");',
                         script_args=(der,), sandbox="system")
    m.set_context("chrome")
    m.execute_script(realsites_phone.SET_PREFS, script_args=(prefs,), sandbox="system")
    m.set_context("content")
    return m


def close_bridge(m):
    try:
        m.delete_session()
    except Exception:
        pass
    adb("shell", "am", "force-stop", realsites_phone.PKG)
    if DEBUGGABLE:
        adb("shell", "am", "clear-debug-app")
    adb("reverse", "--remove-all")
    adb("forward", "--remove", f"tcp:{realsites_phone.PORT}")


def remote(path):
    return DIR + "/" + os.path.basename(path)


def popen(args, log, env=None):
    """realsite `args` (args[0] is the local binary, args[1] the local document) run on the phone; the
    document is pushed first. Variables of `env` that name a local file (MRT_DUMP_*) are given the
    phone's path; fetch() brings such a file back. Its pid goes to `log` first (MRT_PID n)."""
    subprocess.run([ADB, "push", args[1], remote(args[1])], capture_output=True, check=True)
    env = dict(env or {})
    env["MRT_FONT_TABLE"] = DIR + "/fonts.txt"
    for k in ("MRT_DUMP_CONSOLE", "MRT_DUMP_DOM"):
        if k in env:
            env[k] = remote(env[k])
    rest = [shlex.quote(a) for a in args[2:]]
    sets = " ".join(f"{k}={shlex.quote(str(v))}" for k, v in env.items())
    cmd = f"cd {DIR} && echo MRT_PID $$ >&2 && exec env {sets} ./realsite {shlex.quote(remote(args[1]))} {' '.join(rest)}"
    return subprocess.Popen([ADB, "shell", "-T", cmd], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, text=True, bufsize=1)


def fetch(local):
    """A file the runtime wrote on the phone (see popen) to its local path."""
    subprocess.run([ADB, "pull", remote(local), local], capture_output=True)


def pid_in(log_path):
    for line in open(log_path, errors="replace"):
        if line.startswith("MRT_PID "):
            return int(line.split()[1])
    return None


def scan(pid):
    """heap_compress.scan for a process on the phone: its resident anonymous pages through memdump,
    sorted by SpiderMonkey chunk kind and compressed on the host with lz4 -1 and zstd -1."""
    raw = subprocess.run([ADB, "exec-out", DIR + "/memdump", str(pid)], capture_output=True).stdout
    rec = 8 + PAGE
    pages = {}
    for i in range(0, len(raw) - rec + 1, rec):
        (addr,) = struct.unpack_from("<Q", raw, i)
        pages[addr] = raw[i + 8:i + rec]
    kinds = {}

    def kind(base):
        if base not in kinds:
            k = None
            head = pages.get(base)
            if head:
                store, runtime, ck, index = struct.unpack_from("<QQBB", head, 0)
                # Android tags heap pointers in their top byte (bionic's pointer tagging)
                runtime &= (1 << 56) - 1
                tenured = ck in (1, 2) and store == 0 and index == 255
                nursery = ck in (3, 4) and store != 0 and index != 255
                if (tenured or nursery) and runtime and runtime % 8 == 0 and runtime < 1 << 48:
                    k = heap_compress.KINDS[ck]
            kinds[base] = k
        return kinds[base]

    data = {k: bytearray() for k in ("gc-tenured", "gc-buffers", "gc-nursery", "other")}
    zero = dict.fromkeys(data, 0)
    zero_page = bytes(PAGE)
    for addr, page in pages.items():
        cls = kind(addr & ~((1 << 20) - 1)) or "other"
        if page == zero_page:
            zero[cls] += 1
        else:
            data[cls] += page
    mb = lambda n: round(n / 1e6, 3)
    out = {}
    for cls, d in data.items():
        out[cls] = {"pages": len(d) // PAGE + zero[cls], "zero_pages": zero[cls], "mb": mb(len(d)),
                    "lz4_mb": mb(heap_compress.compressed(d, ["lz4", "-1", "-c"])) if d else 0.0,
                    "zstd1_mb": mb(heap_compress.compressed(d, ["zstd", "-1", "-c", "-q"])) if d else 0.0}
    return out


def zram():
    """dumpsys meminfo's ZRAM line: physical KB zram uses, KB in swap (uncompressed)."""
    out = adb("shell", "dumpsys", "meminfo")
    m = re.search(r"ZRAM:\s+([\d,]+)K physical used for\s+([\d,]+)K in swap", out)
    if not m:
        return None
    return {"physical_kb": int(m.group(1).replace(",", "")), "swap_kb": int(m.group(2).replace(",", ""))}


def watch_pageout(log_path, out, timeout=120):
    """In a thread: once the runtime has paged itself out (its MRT_PAGEOUT line in `log_path`), zram's
    use while its pages are still there, into out["zram_after"], and the line's data into out["process"]."""
    import json

    def run():
        t0 = time.time()
        while time.time() - t0 < timeout:
            for line in open(log_path, errors="replace"):
                if line.startswith("MRT_PAGEOUT "):
                    out["process"] = json.loads(line[len("MRT_PAGEOUT "):])
                    # zram's counters lag the pageout: sampled through the runtime's hold, the last kept
                    out["zram_samples"] = []
                    for wait in (0, 5, 10, 10):
                        time.sleep(wait)
                        out["zram_samples"].append({"s": round(time.time() - t0, 1), **(zram() or {})})
                    out["zram_after"] = out["zram_samples"][-1]
                    return
            time.sleep(0.5)
        out["error"] = "no MRT_PAGEOUT line"

    t = threading.Thread(target=run, daemon=True)
    t.start()
    return t


class _Chrome:
    """realsites_phone's session interface (chrome(script, args, is_async)) over a marionette_driver
    session, for its reports(); back in the content context after each script."""

    def __init__(self, m):
        self.m = m

    def chrome(self, script, args=(), is_async=False):
        self.m.set_context("chrome")
        try:
            self.m.timeout.script = 300
            if is_async:
                return self.m.execute_async_script(script, script_args=args, sandbox="system")
            return self.m.execute_script(script, script_args=args, sandbox="system")
        finally:
            self.m.set_context("content")


# Fenix's memory reports for BRIDGE_MEMORY (task bbc6), kept next to the other phone reports
# MRT_REPORTS: another directory under bench/ (task 4898 kept its own, realsites-reports-phone-4898)
REPORTS = os.path.join(os.path.dirname(os.path.abspath(__file__)), os.environ.get("MRT_REPORTS", "realsites-reports-phone-bbc6"))


def memory_reports(m, tag, minimize):
    """A memory report of every Fenix process after a GC and CC everywhere (minimized first with
    `minimize`, as realsites_phone.point() takes it: a minimize alone and 10 s, then the minimizing
    report) and dumpsys meminfo of each process right after: (reports, {process: dumpsys})."""
    s = _Chrome(m)
    s.chrome(realsites_phone.GC_ALL, is_async=True)
    time.sleep(1)
    if minimize:
        s.chrome(realsites_phone.MINIMIZE, is_async=True)
        time.sleep(10)
    realsites_phone.REPORTS = REPORTS
    reps, ds = realsites_phone.reports(s, tag, minimize)
    if DEBUGGABLE:
        for proc, d in ds.items():
            pid = re.search(r"\(pid (\d+)\)", proc)
            if d and proc.startswith(("web", "prealloc")) and pid:
                d["smaps"] = smaps(int(pid.group(1)))
    return reps, ds


SMAPS_FIELDS = ("Rss", "Private_Dirty", "LazyFree", "Swap")


def smaps(pid):
    """A geckoview_example process's smaps summed by mapping, MB, for Rss, Private_Dirty, LazyFree and
    Swap: named anonymous mappings by their name ([anon:jemalloc], [anon:js-gc-heap],
    [anon:scudo:primary], ...), unnamed ones split by whether they are 1 MiB aligned and sized
    (`anon-1MiB`) or not, files by kind (.so, other file), shared memory and stacks as one each."""
    text = adb("shell", "run-as", realsites_phone.PKG, "cat", f"/proc/{pid}/smaps")
    out, cur = {}, None
    for line in text.splitlines():
        m = re.match(r"([0-9a-f]+)-([0-9a-f]+) \S+ \S+ \S+ \d+\s*(.*)$", line)
        if m:
            lo, hi, name = int(m.group(1), 16), int(m.group(2), 16), m.group(3).strip()
            if not name:
                cls = "anon-1MiB" if lo % (1 << 20) == 0 and (hi - lo) % (1 << 20) == 0 else "anon"
            elif name.startswith("[anon:"):
                # bionic and scudo number some names (thread stacks, scudo regions): the kind only
                cls = re.sub(r"[:\s]*(\d+|0x[0-9a-f]+)?\]$", "]", re.sub(r"\s+\d+.*\]$", "]", name))
                if cls.startswith(("[anon:stack_and_tls", "[anon:thread")):
                    cls = "[anon:stack]"
            elif name.startswith("[stack"):
                cls = "[anon:stack]"
            elif "memfd:" in name or "ashmem" in name or name.startswith("/dev/"):
                cls = "shared-mem"
            elif ".so" in name:
                cls = ".so"
            else:
                cls = "file" if name.startswith("/") else name
            cur = out.setdefault(cls, dict.fromkeys(SMAPS_FIELDS, 0.0))
            continue
        m = re.match(r"(\w+):\s+(\d+) kB", line)
        if m and cur is not None and m.group(1) in SMAPS_FIELDS:
            cur[m.group(1)] += int(m.group(2)) / 1024
    out = {k: {f: round(v, 2) for f, v in d.items()} for k, d in sorted(out.items(), key=lambda kv: -kv[1]["Private_Dirty"])}
    out["TOTAL"] = {f: round(sum(d[f] for d in out.values()), 2) for f in SMAPS_FIELDS}
    return out


# dumpsys meminfo's anonymous categories: Gecko's malloc heap and SpiderMonkey's chunks are Unknown;
# no mapped files, no Ashmem (as mini_phone_summary.py counts Fenix's site process)
ANON = ("Native Heap", "Dalvik Heap", "Dalvik Other", "Stack", "Unknown")


def dumpsys_anon(ds):
    """A process's unique (dumpsys private) and anonymous memory in bytes, from realsites_phone.dumpsys(),
    and each category's private MB; None without a reading."""
    if not ds:
        return None
    b = lambda mb: int(mb * 1048576)
    return {"unique": b(ds["total"]["private"]),
            "anonymous": b(sum(ds["cats"].get(c, {}).get("private", 0) for c in ANON)),
            "dumpsys": {k: round(v["private"], 2) for k, v in ds["cats"].items() if v["private"] >= 0.1},
            **({"smaps": ds["smaps"]} if "smaps" in ds else {})}


def _content_reading(pid):
    """dumpsys's anonymous and unique MB of process `pid`, and its smaps (geckoview_example only)."""
    split = dumpsys_anon(realsites_phone.dumpsys(pid))
    out = {"anonymous": split and round(split["anonymous"] / 1048576, 2), "unique": split and round(split["unique"] / 1048576, 2)}
    if DEBUGGABLE:
        out["smaps"] = smaps(pid)
    return out


def quiet_series(m, steps=(0, 15, 30, 45, 60)):
    """The bridge tab's process with no memory report in between (task 4898; a report's own work
    leaves garbage in the process it reads): a GC and CC everywhere, then a reading at each of `steps` s, then
    a minimize alone and a reading 10 s later. {"pid", "series": [{"s", ...}], "minimized"}."""
    pid = m.execute_script("return Services.appinfo.processID;", sandbox="system")
    s = _Chrome(m)
    s.chrome(realsites_phone.GC_ALL, is_async=True)
    t0 = time.time()
    series = []
    for t in steps:
        time.sleep(max(0, t0 + t - time.time()))
        series.append({"s": t, **_content_reading(pid), "idb": idb_transactions(m)})
    s.chrome(realsites_phone.MINIMIZE, is_async=True)
    time.sleep(10)
    return {"pid": pid, "series": series, "minimized": _content_reading(pid)}


# The IndexedDB transactions idb_bridge.js keeps open (keepOpen), read from its sandbox: each one's
# requests so far (`spins`), and whether the runtime has ended it (`done`) or Firefox has (`finished`)
IDB_OPEN = """const S = globalThis.__mrtIdb;
if (!S) return null;
return [...S.objs.values()].filter(o => o && "spins" in o).map(o => ({spins: o.spins, done: o.done, finished: o.finished}));"""


def idb_transactions(m):
    try:
        return m.execute_script(IDB_OPEN, sandbox="idb", new_sandbox=False)
    except Exception as e:
        return {"error": str(e)[:200]}


def pressure_point(pid, hold_s=5):
    """Memory pressure on the phone, short of a kill of anything above the cached apps: android/hog
    grows until lmkd's first kill below adj 900 (press.sh, task a764; the hog is frozen at lmkd's
    first kill and ended hold_s later), then the bridge process `pid` read again. The phone's cached
    apps may be killed. {"press": press.sh's output, "after": reading, "zram": before/after}."""
    subprocess.run([ADB, "push", os.path.join(PROJECT_ANDROID, "press.sh"), os.path.join(PROJECT_ANDROID, "hog"),
                    "/data/local/tmp/"], capture_output=True, check=True)
    before = zram()
    press = adb("shell", f"sh /data/local/tmp/press.sh 11000 32 500 {hold_s}")
    adb("shell", "pkill -9 -x hog; pkill -f 'logcat -[T] 1'")
    time.sleep(2)
    return {"press": press.strip()[-600:], "zram_before": before, "zram_after": zram(), "after": _content_reading(pid)}
