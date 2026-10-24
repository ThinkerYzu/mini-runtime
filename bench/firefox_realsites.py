"""Gecko's hibernation on real sites (SPEC decision 8): memory live and hibernated, wake time, and whether the
page still works after waking.

For each site, in a fresh Firefox run on a copy of a persistent test profile (the owner logs in to the
sites that need it once, by hand, in that profile; this script never handles passwords):
  1. load the page, wait for it to settle, do the site's short scripted use (scroll, ...), settle again
  2. set a marker variable in the page, record the scroll position, the element count, a screenshot
  3. GC and CC in the content process, the page's console (GAPS B20), dump memory ("live", plain)
  4. docViewer.hide() (drops the frame tree and style data; with the prototype build, a layout read while
     hidden wakes the page), GC and CC, dump memory ("hidden")
  5. docViewer.show() timed with a layout read ("wake_ms")
  6. check the marker, the scroll position and the element count, take a second screenshot
  7. the page and then the blank tab again, measured after Firefox's "Minimize memory usage" (GAPS A17;
     `live_minimized`, `browser_minimized`, `unique_split_minimized`; skipped with EXTRA=1)
Memory is reported for every content process that has a window of the tab (with site isolation, frames
from other sites, such as ads and embeds, are in other processes), and for the top-level page's process:
the process's heap-allocated, resident and resident-unique, and the window categories (dom, layout,
js-realm, js-zone, images, ...), each summed over the tab's windows.

usage (from a Firefox source tree):
  FIREFOX_BIN=.../obj-hib/dist/bin/firefox ./mach python /path/to/firefox_realsites.py
  ONLY=youtube,reddit          sites (default: all ten); SITES_NO_LOGIN=1 for the five that need no login
  REALSITES_PROFILE=dir        the master profile (default ~/.cache/mrt-realsites-profile)
  HEADLESS=1                   no window on the desktop (some sites behave differently headless)
To make the profile and log in, once: FIREFOX_BIN... --profile ~/.cache/mrt-realsites-profile (a normal
run, not this script; automation is detected by some sign-in pages).
Writes bench/firefox_realsites.json and screenshots in bench/realsites/.
  REPLAY=record|fill|replay    through mitmproxy, recording the site or answering from its recording
  NO_USE=1                     skip the site's scripted use (scroll, ...): the page as loaded, which is
                               what the mini runtime builds (it does not replay the use)
                               (bench/replay.py; DESIGN "Replay"); results default to firefox_realsites_replay.json
  HEAP_SCAN=1                  after the live dump, the site's top content process's resident anonymous memory
                               split into SpiderMonkey's GC chunks and the rest, each compressed (bench/heap_compress.py)
  (every run)                  after the live dump, the same process's unique memory split into file-backed, by file, and
                               anonymous (`unique_split`, bench/firefox_process_files.py), and by mapping class
                               (`unknown_split`, and `unknown_split_minimized` with the minimized state:
                               bench/ff_unknown.py, task 178d)
  DMD_DUMP=1                   with a DMD build run with DMD="--mode=dark-matter --stacks=full" (obj-hib;
                               MOZ_ALLOW_DOWNGRADE=1 for the profile release 157 last used; MOZ_DISABLE_CONTENT_SANDBOX=1 so the child
                               can write its file): after the minimized state, a DMD dump of every process; the
                               site's process's is kept as bench/realsites-dmd/<site>.json.gz (task 178d)
"""
import base64, gzip, json, os, re, shutil, sys, time
import firefox_exit
from marionette_driver.marionette import Marionette

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import replay
import heap_compress
import firefox_process_files
import ff_unknown
HEAP_SCAN = os.environ.get("HEAP_SCAN") == "1"
DMD_DUMP = os.environ.get("DMD_DUMP") == "1"
DMDS = os.path.join(HERE, "realsites-dmd")
BIN = os.environ["FIREFOX_BIN"]
PROFILE = os.environ.get("REALSITES_PROFILE", os.path.expanduser("~/.cache/mrt-realsites-profile"))
HEADLESS = os.environ.get("HEADLESS") == "1"
SHOTS = os.path.join(HERE, "realsites")
DOMS = os.path.join(HERE, "realsites-dom")
# REPORTS_DIR: where KEEP_REPORTS=1 puts them (a replay run would otherwise replace the live ones)
REPORTS = os.path.join(HERE, os.environ.get("REPORTS_DIR", "realsites-reports"))
# the page's console at the live measuring point, <site>-firefox.json (GAPS B20; not in git)
CONSOLES = os.path.join(HERE, "realsites-console")
KEEP_REPORTS = os.environ.get("KEEP_REPORTS") == "1"
# EXTRA=1: the second-tab and background steps (session 34): the same URL in a second tab, another
# site in a second tab, the page in the background for BG_WAIT s, then Firefox's "minimize memory usage"
EXTRA = os.environ.get("EXTRA") == "1"
BG_WAIT = int(os.environ.get("BG_WAIT", "60"))  # phase 0b's DOM files (third-party content, not in git)
VIEW_W, VIEW_H = 1489, 844
BLANK_SETTLE = int(os.environ.get("BLANK_SETTLE", "45"))

# The scripted use of each site, run in the page: (description, JS). Scrolling is done in steps with a
# pause, as a reader would, so that lazy content loads.
SCROLL = "for (let i = 0; i < 5; i++) { window.scrollBy(0, innerHeight); await new Promise(r => setTimeout(r, 1500)); }"
SITES = [
    # name, url, needs login, use
    ("facebook", "https://www.facebook.com/", True, SCROLL),
    ("instagram", "https://www.instagram.com/", True, SCROLL),
    # Supply your own document through GDOC_URL when selecting this site.
    ("gdocs", os.environ.get("GDOC_URL", "https://docs.google.com/"), True, ""),
    ("gmail", "https://mail.google.com/mail/u/0/#inbox", True, ""),
    ("x", "https://x.com/home", True, SCROLL),
    ("youtube", "https://www.youtube.com/watch?v=jNQXAC9IVRw", False, "document.querySelector('video')?.pause();"),
    ("gmaps", "https://www.google.com/maps/@48.8566,2.3522,15z", False, ""),
    ("reddit", "https://www.reddit.com/r/AskReddit/top/?t=week", False, SCROLL),
    ("amazon", "https://www.amazon.com/s?k=laptop", False, SCROLL),
    ("cnn", "https://www.cnn.com/", False, SCROLL),
]
if os.environ.get("NO_USE") == "1":
    SITES = [(name, url, login, None) for name, url, login, _ in SITES]
SITE_URL = {s[0]: (s[1], s[3]) for s in SITES}
OTHER_SITE = {"cnn": "youtube"}  # the second site opened next to each page (default cnn)
NAMES = os.environ.get("ONLY")
if NAMES:
    SITES = [s for s in SITES if s[0] in NAMES.split(",")]
if any(s[0] == "gdocs" for s in SITES) and not os.environ.get("GDOC_URL"):
    raise SystemExit("Set GDOC_URL to your own document, or select other sites with ONLY.")
if os.environ.get("SITES_NO_LOGIN") == "1":
    SITES = [s for s in SITES if not s[2]]

PREFS = {
    "marionette.allowSystemAccess": True,
    "app.normandy.enabled": False, "messaging-system.rsexperimentloader.enabled": False,
    "app.update.disabledForTesting": True, "app.update.auto": False, "app.update.enabled": False,
    "datareporting.policy.dataSubmissionEnabled": False, "toolkit.telemetry.enabled": False,
    "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False,
    # the prototype: a layout read while hidden wakes the page (only in the obj-hib build)
    "layout.hibernation.wake_on_demand": True,
    # media must not start by itself (a playing video keeps decoding while hidden)
    "media.autoplay.default": 5,
}
FRAME = """addMessageListener("mrt:gc", () => {
  for (let i = 0; i < 2; i++) content.windowUtils.garbageCollect();
  sendAsyncMessage("hib:done", {});
});
addMessageListener("hib:hide", () => {
  const v = docShell.docViewer; v.sticky = false; v.hide();
  sendAsyncMessage("hib:done", {});
});
// Phase 0b: the finished DOM with every shadow root, open or closed, as declarative shadow DOM
// (<template shadowrootmode>), and the counts the mini DOM is checked against. Frames are not included
// (their windows are measured apart on both sides).
addMessageListener("mrt:dom", () => {
  const doc = content.document, roots = [];
  let shadowElements = 0;
  const walk = root => {
    for (const el of root.querySelectorAll("*")) {
      const sr = el.openOrClosedShadowRoot;
      if (sr) { roots.push(sr); shadowElements += sr.querySelectorAll("*").length; walk(sr); }
    }
  };
  walk(doc);
  const de = doc.documentElement;
  const attrs = [...de.attributes].map(a => ` ${a.name}="${a.value.replace(/&/g, "&amp;").replace(/"/g, "&quot;")}"`).join("");
  const html = (doc.doctype ? `<!DOCTYPE ${doc.doctype.name}>` : "") + `<html${attrs}>` + de.getHTML({shadowRoots: roots}) + "</html>";
  sendAsyncMessage("hib:done", {html, elements: doc.querySelectorAll("*").length, shadowRoots: roots.length,
                                shadowElements, textLength: de.textContent.length});
});
// GAPS B20: the page's console as Firefox keeps it, to set against the mini runtime's: the console
// API calls (console.log, warn, error, ...) from ConsoleAPIStorage and the script errors and warnings
// (uncaught exceptions, unhandled promise rejections) from the console service, both for the top
// window only (the mini runtime runs no frames).
addMessageListener("mrt:console", () => {
  const id = content.windowGlobalChild.innerWindowId;
  // a check of the capture itself: whether the page replaced the console's methods, and a probe
  // logged through the page's own console object (it must come back in `api`)
  const pageConsole = content.wrappedJSObject.console;
  const replaced = ["log", "info", "warn", "error", "debug"].filter(m => {
    try { return !/\\[native code\\]/.test(String(pageConsole[m])); } catch (e) { return true; }
  });
  try { pageConsole.log("mrt-console-probe"); } catch (e) {}
  const text = v => { try { return typeof v === "string" ? v : String(v); } catch (e) { return "<" + typeof v + ">"; } };
  const api = Cc["@mozilla.org/consoleAPI-storage;1"].getService(Ci.nsIConsoleAPIStorage)
    .getEvents(String(id)).map(e => ({level: e.level, text: (e.arguments || []).map(text).join(" "),
                                       source: e.filename || "", line: e.lineNumber || 0}));
  const errors = [];
  for (const m of Services.console.getMessageArray() || []) {
    if (!(m instanceof Ci.nsIScriptError) || m.innerWindowID != id) continue;
    errors.push({level: m.flags & Ci.nsIScriptError.warningFlag ? "warning" : "error", category: m.category,
                 text: m.errorMessage, source: m.sourceName || "", line: m.lineNumber || 0,
                 column: m.columnNumber || 0, stack: m.stack ? String(m.stack) : ""});
  }
  // every window's stored console calls in this process, by inner window id: a call stored under
  // another id than the page's current one would be missed above
  const byWindow = {};
  for (const e of Cc["@mozilla.org/consoleAPI-storage;1"].getService(Ci.nsIConsoleAPIStorage).getEvents()) {
    const k = String(e.innerID);
    byWindow[k] = byWindow[k] || {count: 0, first: []};
    byWindow[k].count++;
    if (byWindow[k].first.length < 5) byWindow[k].first.push(e.level + ": " + (e.arguments || []).map(text).join(" ").slice(0, 120));
  }
  sendAsyncMessage("hib:done", {api, errors, replaced, windowId: String(id), byWindow});
});
addMessageListener("hib:show", () => {
  const t0 = content.performance.now();
  docShell.docViewer.show();
  const h = content.document.documentElement.offsetHeight;
  sendAsyncMessage("hib:done", {ms: content.performance.now() - t0, h});
});"""
LOAD_FS = 'gBrowser.selectedBrowser.messageManager.loadFrameScript("data:application/javascript," + encodeURIComponent(arguments[0]), false);'
CALL = """const [msg] = arguments; const done = arguments[arguments.length - 1];
const mm = gBrowser.selectedBrowser.messageManager;
mm.addMessageListener("hib:done", function l(m) { mm.removeMessageListener("hib:done", l); done(m.data); });
mm.sendAsyncMessage(msg);"""
# The last argument is minimizeMemoryUsage: true runs Firefox's "Minimize memory usage" in every process
# (memory-pressure "heap-minimize", three rounds) before the reports are collected. Until session 39 every
# dump passed true (GAPS A17); now a measuring point is taken plain (false), and minimized where asked.
DUMP = """const [path, minimize] = arguments; const done = arguments[arguments.length - 1];
Components.classes["@mozilla.org/memory-info-dumper;1"].getService(Components.interfaces.nsIMemoryInfoDumper)
  .dumpMemoryReportsToNamedFile(path, {callback() { done(true); }}, null, false, minimize);"""
# The page's state as a script sees it; `wrappedJSObject` reaches the page's own globals.
PROBE = """const w = window.wrappedJSObject; if (arguments[0]) w.__hibCanary = arguments[0];
return {canary: w.__hibCanary || null, scrollY: Math.round(scrollY), elements: document.querySelectorAll('*').length,
        url: location.href, height: document.documentElement.scrollHeight};"""


def chrome(m, script, args=(), run_async=False):
    m.set_context("chrome")
    try:
        if run_async:
            m.timeout.script = 180
            return m.execute_async_script(script, script_args=args, sandbox="system")
        return m.execute_script(script, script_args=args, sandbox="system")
    finally:
        m.set_context("content")


def keep(path):
    """Moves a memory report to bench/realsites-reports/ with KEEP_REPORTS=1 (not in git: it names the
    sites' URLs), or deletes it."""
    if KEEP_REPORTS:
        os.makedirs(REPORTS, exist_ok=True)
        os.replace(path, os.path.join(REPORTS, os.path.basename(path)))
    else:
        os.remove(path)


def reports(m, tag, minimize):
    """One memory report of every process (a plain one, or after "Minimize memory usage")."""
    path = os.path.join(HERE, tag + ".json.gz")
    chrome(m, DUMP, (path, minimize), run_async=True)
    reps = json.load(gzip.open(path))["reports"]
    keep(path)
    return reps


def dump(m, tag, top_url, minimize=False):
    """Memory of the tab: every process with a window under top(<the tab's URL>), and the top-level process."""
    return windows_of(reports(m, tag, minimize), top_url)


def windows_of(reps, top_url):
    """The tab's windows in a memory report: see dump()."""
    # the top(...) entry of the tab: Firefox names it by the top-level document's URL
    # (a URL in a report path has its "/" written as "\\")
    key = "top(" + top_url.replace("/", "\\")
    procs, whole, subs = {}, {}, {}
    for r in reps:
        p = r["path"]
        if p in ("heap-allocated", "resident", "resident-unique"):
            whole.setdefault(r["process"], {})[p] = r["amount"]
        if not p.startswith("explicit/window-objects/") or key not in p:
            continue
        parts = p.split("/")
        zi = next((i for i, s in enumerate(parts) if s.startswith("js-zone(")), None)
        wi = next((i for i, s in enumerate(parts) if s.startswith("window(")), None)
        if zi is not None:
            cat = "js-zone"
        elif wi is not None and wi + 1 < len(parts):
            cat = re.sub(r"\(.*", "", parts[wi + 1])
        else:
            cat = "other"
        d = procs.setdefault(r["process"], {})
        d[cat] = d.get(cat, 0) + r["amount"]
        # one level down under layout/ and dom/ (style-sheets, pres-arena, element-nodes, ...)
        if cat in ("layout", "dom") and wi is not None and wi + 2 < len(parts):
            sk = cat + "/" + re.sub(r"\(.*", "", parts[wi + 2])
            subs[sk] = subs.get(sk, 0) + r["amount"]
    mb = lambda x: round(x / 1048576, 2)
    top = max(procs, key=lambda p: procs[p].get("dom", 0)) if procs else None
    return {
        "top_process": top,
        "processes": {p: {"window": {k: mb(v) for k, v in sorted(c.items())}, "window_total": mb(sum(c.values())),
                          "process": {k: mb(v) for k, v in sorted(whole.get(p, {}).items())}}
                      for p, c in procs.items()},
        "tab_window_total": mb(sum(sum(c.values()) for c in procs.values())),
        "sub": {k: mb(v) for k, v in sorted(subs.items()) if v >= 1024 * 100},
    }


# Firefox's own "Minimize memory usage" (about:memory's button): the memory-pressure path in every child,
# then in the parent.
MINIMIZE = """const done = arguments[arguments.length - 1];
Services.obs.notifyObservers(null, "child-mmu-request");
Cc["@mozilla.org/memory-reporter-manager;1"].getService(Ci.nsIMemoryReporterManager).minimizeMemoryUsage(() => done(true));"""


# A memory report and, under DMD, a DMD file of every process, in $TMPDIR/memory-reports/ (no callback)
DMD_ALL = """Cc["@mozilla.org/memory-info-dumper;1"].getService(Ci.nsIMemoryInfoDumper).dumpMemoryInfoToTempDir(arguments[0], false, false);"""


def dmd_dump(m, name, pid):
    """Task 178d: DMD's live blocks of every process, the site's process's (`pid`) moved to
    bench/realsites-dmd/<name>.json.gz; its path, or None after 10 minutes (DMD dumps take a while)."""
    ident = f"{name}-{int(time.time())}"
    chrome(m, DMD_ALL, (ident,))
    tmp = os.environ.get("TMPDIR", "/tmp")
    want = f"dmd-{ident}-{pid}.json.gz"
    # every process's DMD file is closed once the parent's unified report has its final name (it is
    # written as incomplete-..., and renamed after the last process has answered)
    done = f"unified-memory-report-{ident}-"
    for _ in range(600):
        for d, _subs, files in os.walk(tmp):
            if want in files and any(f.startswith(done) for f in files):
                os.makedirs(DMDS, exist_ok=True)
                dest = os.path.join(DMDS, name + ".json.gz")
                shutil.move(os.path.join(d, want), dest)  # $TMPDIR may be another file system
                # the other processes' DMD files and the unified report (task 69a4)
                for f in files:
                    if f.startswith((f"dmd-{ident}-", done)) and f != want:
                        os.remove(os.path.join(d, f))
                return os.path.relpath(dest, HERE)
        time.sleep(1)
    return None


# Every Firefox process: a GC and a CC in the content processes (the observer topics Firefox's own
# about:memory "GC" and "CC" buttons use) and in the parent.
GC_ALL = """const done = arguments[arguments.length - 1];
Services.obs.notifyObservers(null, "child-gc-request");
Cu.forceGC(); Cu.forceCC();
Services.obs.notifyObservers(null, "child-cc-request");
setTimeout(() => { Services.obs.notifyObservers(null, "child-gc-request"); Cu.forceGC(); done(true); }, 2000);"""


def smaps(pid):
    """Rss, Pss, unique (USS = Private_Clean + Private_Dirty) and Swap of a process, in bytes."""
    out = {}
    try:
        for line in open(f"/proc/{pid}/smaps_rollup"):
            k, _, v = line.partition(":")
            if k in ("Rss", "Pss", "Private_Clean", "Private_Dirty", "Swap"):
                out[k] = int(v.split()[0]) * 1024
    except OSError:
        return None
    return {"rss": out.get("Rss", 0), "pss": out.get("Pss", 0), "uss": out.get("Private_Clean", 0) + out.get("Private_Dirty", 0), "swap": out.get("Swap", 0)}


def whole_browser(m, tag, minimize=False):
    """Phase 0a: every process of this Firefox, after a GC and CC everywhere (and after "Minimize memory
    usage" with `minimize`): its kind (the name in the memory report, without the pid), heap-allocated from
    the report, and USS, PSS and RSS from /proc/<pid>/smaps_rollup. MB."""
    chrome(m, GC_ALL, run_async=True)
    time.sleep(1)
    return browser_of(reports(m, tag, minimize))


def minimized(m, tag, top_url):
    """The tab's windows and the whole browser from one report taken after "Minimize memory usage"
    (GAPS A17): the state a page is left in under memory pressure."""
    chrome(m, GC_ALL, run_async=True)
    time.sleep(1)
    reps = reports(m, tag, True)
    return windows_of(reps, top_url), browser_of(reps)


def browser_of(reps):
    """Every process in a memory report with its smaps figures: see whole_browser()."""
    heap = {r["process"]: r["amount"] for r in reps if r["path"] == "heap-allocated"}
    # explicit/ one level down per process (window-objects, gfx, network, images, heap-unclassified, ...),
    # to say where a page's cost goes; leaves only, so the sums do not count a node twice
    cats = {}
    for r in reps:
        if r["path"].startswith("explicit/") and r["units"] == 0:
            parts = r["path"].split("/")
            # gfx one level further (the parent's gfx reports more than its whole growth on some sites)
            c = "/".join(parts[1:3]) if parts[1] == "gfx" and len(parts) > 3 else parts[1]
            d = cats.setdefault(r["process"], {})
            d[c] = d.get(c, 0) + r["amount"]
    mb = lambda x: round(x / 1048576, 2)
    procs = {}
    for proc, h in heap.items():
        pm = re.search(r"\(pid (\d+)\)", proc)
        pid = int(pm.group(1)) if pm else None
        if pid is None:
            # the parent process is reported as "Main Process (pid N)" in recent builds, "" in older ones
            continue
        sm = smaps(pid)
        if sm is None:
            continue
        procs[proc] = {"kind": re.sub(r"\s*\(pid \d+\)", "", proc), "pid": pid, "heap": mb(h), **{k: mb(v) for k, v in sm.items()},
                       "explicit": {c: mb(v) for c, v in sorted(cats.get(proc, {}).items()) if v >= 1048576}}
    total = {k: round(sum(p[k] for p in procs.values()), 2) for k in ("heap", "uss", "pss", "rss", "swap")}
    # the same summed by kind of process (all web content processes together), for the cost by kind
    by_kind = {}
    for p in procs.values():
        k = kind_group(p["kind"])
        d = by_kind.setdefault(k, {"n": 0, "heap": 0, "uss": 0, "pss": 0, "explicit": {}})
        d["n"] += 1
        for f in ("heap", "uss", "pss"):
            d[f] = round(d[f] + p[f], 2)
        for c, v in p["explicit"].items():
            d["explicit"][c] = round(d["explicit"].get(c, 0) + v, 2)
    return {"processes": procs, "total": total, "by_kind": by_kind}


def kind_group(kind):
    """The process kinds of a memory report, with web content of any site (and the spare one) as one."""
    if kind.startswith("webServiceWorker"):
        return "service worker"
    if kind == "web" or kind.startswith("webIsolated") or kind.startswith("webCOOP"):
        return "content"
    return kind


def kind_cost(a, b):
    """b minus a, by kind of process: USS, PSS, heap and the explicit/ categories that moved by 1 MB or more."""
    out = {}
    for k in sorted(set(a) | set(b)):
        x, y = a.get(k, {"heap": 0, "uss": 0, "pss": 0, "explicit": {}}), b.get(k, {"heap": 0, "uss": 0, "pss": 0, "explicit": {}})
        ex = {c: round(y["explicit"].get(c, 0) - x["explicit"].get(c, 0), 2) for c in set(x["explicit"]) | set(y["explicit"])}
        out[k] = {f: round(y[f] - x[f], 2) for f in ("heap", "uss", "pss")}
        out[k]["explicit"] = dict(sorted(((c, v) for c, v in ex.items() if abs(v) >= 1), key=lambda t: -abs(t[1])))
    return out


def shot(m, name, tag):
    os.makedirs(SHOTS, exist_ok=True)
    png = base64.b64decode(m.screenshot())
    path = os.path.join(SHOTS, f"{name}-{tag}{os.environ.get('REALSITES_SHOT_SUFFIX', '')}.png")
    open(path, "wb").write(png)
    return path


def open_tab(m, url, use):
    """A new foreground tab on url with the site's scripted use; returns (handle, the page's tab handle)."""
    page = m.current_window_handle
    h = m.open(type="tab", focus=True)["handle"]
    m.switch_to_window(h)
    m.navigate(url)
    time.sleep(15)
    if use:
        m.timeout.script = 120
        m.execute_async_script("const done = arguments[arguments.length - 1]; (async () => { " + use + " })().then(() => done(true), e => done(String(e)));")
        time.sleep(5)
    return h, page


def close_tab(m, h, page):
    m.switch_to_window(h)
    m.close()
    m.switch_to_window(page)
    time.sleep(3)


def delta(a, b):
    return {"total": {k: round(b["total"][k] - a["total"][k], 2) for k in a["total"]}, "by_kind": kind_cost(a["by_kind"], b["by_kind"])}


def run_extra(m, name, url, use):
    """The extra steps, each against the whole browser measured just before it:
    same_site   the same URL in a second tab (what a second tab of the site costs)
    other_site  another site in a second tab (CNN, or YouTube for CNN), to compare with its cost alone
    background  the page behind a blank tab for BG_WAIT s (what Firefox frees by itself in the background)
    minimized   then Firefox's "minimize memory usage" in every process (what it frees under pressure)"""
    out = {}
    other = OTHER_SITE.get(name, "cnn")
    for step, u, us in (("same_site", url, use), ("other_site", SITE_URL[other][0], SITE_URL[other][1])):
        pre = whole_browser(m, f"{name}-{step}-pre")
        h, page = open_tab(m, u, us)
        post = whole_browser(m, f"{name}-{step}")
        close_tab(m, h, page)
        out[step] = {"url": u, "cost": delta(pre, post)}
    out["other_site"]["site"] = other
    pre = whole_browser(m, f"{name}-bg-pre")
    page = m.current_window_handle
    h = m.open(type="tab", focus=True)["handle"]
    m.switch_to_window(h)
    m.navigate("about:blank")
    time.sleep(BG_WAIT)
    bg = whole_browser(m, f"{name}-bg")
    chrome(m, MINIMIZE, run_async=True)
    time.sleep(5)
    mini = whole_browser(m, f"{name}-minimized")
    close_tab(m, h, page)
    out["background"] = {"wait_s": BG_WAIT, "change": delta(pre, bg)}
    out["minimized"] = {"change": delta(pre, mini)}
    return out


def run(name, url, use):
    os.makedirs(PROFILE, exist_ok=True)
    proxy = replay.start(name, "firefox")
    # DMD_DUMP: a DMD build records a stack per allocation and starts several times slower
    m = Marionette(bin=BIN, headless=HEADLESS, prefs={**PREFS, **(proxy.prefs if proxy else {})}, profile=PROFILE,
                   startup_timeout=600 if DMD_DUMP else 120)
    m.start_session(proxy.capabilities if proxy else None)
    try:
        m.set_context("content")
        m.set_window_rect(width=VIEW_W + 40, height=VIEW_H + 120)
        m.timeout.page_load = 120
        # phase 0a's baseline: the same browser with a blank tab, settled. At 10 s the parent was in
        # one of two states 33 MB apart from run to run (session 34), so it gets longer
        m.navigate("about:blank")
        time.sleep(BLANK_SETTLE)
        blank = whole_browser(m, name + "-blank")
        m.navigate(url)
        time.sleep(15)
        if use:
            m.timeout.script = 120
            m.execute_async_script("const done = arguments[arguments.length - 1]; (async () => { " + use + " })().then(() => done(true), e => done(String(e)));")
            time.sleep(5)
        canary = f"hib-{int(time.time())}"
        before = m.execute_script(PROBE, script_args=(canary,))
        shot_before = shot(m, name, "before")
        chrome(m, LOAD_FS, (FRAME,))
        chrome(m, CALL, ("mrt:gc",), run_async=True)
        time.sleep(2)
        top_url = before["url"]
        # GAPS B20: the page's console, written apart (third-party text, not in git); counts in the results.
        # Before the memory dump: DUMP minimizes memory first, and its memory-pressure clears
        # ConsoleAPIStorage (session 39)
        cons = chrome(m, CALL, ("mrt:console",), run_async=True)
        os.makedirs(CONSOLES, exist_ok=True)
        json.dump(cons, open(os.path.join(CONSOLES, name + "-firefox.json"), "w"), indent=1)
        console_counts = {"api": len(cons["api"]), "errors": sum(e["level"] == "error" for e in cons["errors"]),
                          "warnings": sum(e["level"] == "warning" for e in cons["errors"])}
        live = dump(m, name + "-live", top_url)
        heap = None
        top_pid = int(re.search(r"pid (\d+)", live["top_process"]).group(1))
        # the site's content process's unique memory, file-backed (by file) and anonymous: how much
        # of it is binaries, to set against the mini process without its own (GAPS A16)
        try:
            unique_split = {"pid": top_pid, **firefox_process_files.split(top_pid)}
        except OSError:
            unique_split = None
        try:
            unknown_split = {"pid": top_pid, **ff_unknown.split(top_pid)}
        except OSError:
            unknown_split = None
        if HEAP_SCAN:
            heap = {"pid": top_pid, **heap_compress.scan(top_pid)}
        browser_live = whole_browser(m, name + "-browser-live")
        chrome(m, CALL, ("hib:hide",), run_async=True)
        time.sleep(1)
        chrome(m, CALL, ("mrt:gc",), run_async=True)
        time.sleep(2)
        hidden = dump(m, name + "-hidden", top_url)
        browser_hidden = whole_browser(m, name + "-browser-hidden")
        woke = chrome(m, CALL, ("hib:show",), run_async=True)
        time.sleep(2)
        after = m.execute_script(PROBE, script_args=(None,))
        shot_after = shot(m, name, "after")
        # phase 0b: the finished DOM written out (after the measurements, so its string is not in them)
        dom = chrome(m, CALL, ("mrt:dom",), run_async=True)
        os.makedirs(DOMS, exist_ok=True)
        dom_file = os.path.join(DOMS, name + ".html")
        open(dom_file, "w").write(dom.pop("html"))
        # the same page after Firefox's "Minimize memory usage" (GAPS A17), after every plain measurement
        # because it cannot be undone; not with EXTRA, whose steps must start from a plain state (its own
        # last step minimizes)
        live_min = browser_live_min = unique_split_min = unknown_split_min = None
        if not EXTRA:
            live_min, browser_live_min = minimized(m, name + "-live-minimized", top_url)
            try:
                unique_split_min = {"pid": top_pid, **firefox_process_files.split(top_pid)}
                unknown_split_min = {"pid": top_pid, **ff_unknown.split(top_pid)}
            except OSError:
                pass
        dmd_file = dmd_dump(m, name, top_pid) if DMD_DUMP else None
        extra = run_extra(m, name, url, use) if EXTRA else None
        # the blank tab again after the page: how far the baseline drifted (caches the page left behind
        # and the parent's own settling are in the difference)
        m.navigate("about:blank")
        time.sleep(10)
        blank_after = whole_browser(m, name + "-blank-after")
        browser_min = None
        if browser_live_min:
            # the page's cost with both states minimized: against the blank tab after the page, minimized
            # (the minimize drops most of what the page left behind; a blank tab minimized before the page
            # would change the plain measurements that follow it)
            _, blank_after_min = minimized(m, name + "-blank-after-minimized", "about:blank")
            browser_min = {"live": browser_live_min, "blank_after": blank_after_min,
                           "page_cost": {k: round(browser_live_min["total"][k] - blank_after_min["total"][k], 2) for k in blank_after_min["total"]},
                           "page_cost_by_kind": kind_cost(blank_after_min["by_kind"], browser_live_min["by_kind"])}
        return {
            "url": url, "final_url": top_url, "before": before, "after": after,
            # the page's own state survived; sites add and remove elements on their own, so the element
            # count is reported, not required to match
            "state_kept": after["canary"] == canary and after["scrollY"] == before["scrollY"],
            "elements_delta": after["elements"] - before["elements"],
            "live": live, "hidden": hidden, "wake_ms": round(woke["ms"], 1),
            # GAPS A17: the page and the browser after "Minimize memory usage" (None with EXTRA)
            "live_minimized": live_min, "browser_minimized": browser_min, "unique_split_minimized": unique_split_min,
            "unknown_split_minimized": unknown_split_min,
            # phase 0a: the whole browser with a blank tab, with the page, with the page hidden
            "browser": {"blank": blank, "live": browser_live, "hidden": browser_hidden, "blank_after": blank_after,
                        "drift": {k: round(blank_after["total"][k] - blank["total"][k], 2) for k in blank["total"]},
                        "page_cost": {k: round(browser_live["total"][k] - blank["total"][k], 2) for k in blank["total"]},
                        "hidden_cost": {k: round(browser_hidden["total"][k] - blank["total"][k], 2) for k in blank["total"]},
                        "page_cost_by_kind": kind_cost(blank["by_kind"], browser_live["by_kind"])},
            "screenshots": [os.path.relpath(shot_before, HERE), os.path.relpath(shot_after, HERE)],
            "dom_dump": {"file": os.path.relpath(dom_file, HERE), **dom},
            "extra": extra,
            "heap_scan": heap,
            "unique_split": unique_split,
            "unknown_split": unknown_split,
            "dmd_file": dmd_file,
            "console": console_counts,
            "replay": {"mode": replay.MODE, "log": os.path.relpath(proxy.log, HERE)} if proxy else None,
        }
    finally:
        # the script ends with os._exit, which skips mozprofile's own cleanup: 56 profile copies
        # (about 400 MB each) filled /tmp in session 34
        firefox_exit.close(m)
        if proxy:
            proxy.close()


# REALSITES_OUT: the result file (default firefox_realsites.json), so builds can be kept apart
out = os.path.join(HERE, os.environ.get("REALSITES_OUT", "firefox_realsites_replay.json" if replay.MODE else "firefox_realsites.json"))
results = json.load(open(out)) if os.path.exists(out) else {}
for name, url, login, use in SITES:
    try:
        results[name] = run(name, url, use)
        r = results[name]
        if r.get("extra"):
            e = r["extra"]
            print("  extra", name, "same site +", e["same_site"]["cost"]["total"]["uss"], "other site (" + e["other_site"]["site"] + ") +", e["other_site"]["cost"]["total"]["uss"],
                  "background", e["background"]["change"]["total"]["uss"], "minimized", e["minimized"]["change"]["total"]["uss"], flush=True)
        print("done", name, "tab live", r["live"]["tab_window_total"], "hidden", r["hidden"]["tab_window_total"], "wake", r["wake_ms"], "state kept", r["state_kept"],
              "| page cost USS", r["browser"]["page_cost"]["uss"], "drift USS", r["browser"]["drift"]["uss"], "PSS", r["browser"]["page_cost"]["pss"], "hidden USS", r["browser"]["hidden_cost"]["uss"], flush=True)
    except Exception as e:
        results[name] = {"url": url, "error": repr(e)[:300]}
        print("failed", name, repr(e)[:200], flush=True)
    json.dump(results, open(out, "w"), indent=1)
os._exit(0)
