"""Look inside a replayed site in Firefox: load it (no scripted use) and evaluate a script in the page
every few seconds, printing what it returns. The Firefox counterpart of MRT_EVAL_AFTER (session 44:
Amazon's nav modules, `$Nav.stats(true)`, and its viewport sizes).

The script is the body of a function run in the content context; `window.wrappedJSObject` is the page's
own window (its globals, such as jQuery). It returns something JSON can encode.

usage (from a Firefox source tree, under xvfb-run like the harnesses):
  REPLAY=replay FIREFOX_BIN=... ./mach python /path/to/bench/firefox_probe.py SITE URL PROBE.js
  STEPS=8 (evaluations), EVERY=4 (seconds between them), PROBE_CONTEXT=chrome (the probe runs in
  the browser's chrome instead of the page: ChromeUtils.requestProcInfo lists every process's workers),
  INPUT_ACTIONS=FILE (WebDriver actions done in the page a second after load, before the probes:
  input_actions.py), MARKS_OUT=FILE (memory and a screenshot at each of the actions' marks),
  SAVE_PROFILE=DIR (a copy of the profile as Firefox left it after an in-app quit: Marionette runs
  Firefox on a clone of REALSITES_PROFILE, so this is how a page's writes are kept)
The proxy's log is realsites-replay/SITE/probe-replay.jsonl, apart from the harnesses' logs.
"""
import json, os, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import replay
import firefox_exit
from marionette_driver.marionette import Marionette

# firefox_realsites.py's preferences, without the prototype's
PREFS = {
    "marionette.allowSystemAccess": True,
    "app.normandy.enabled": False, "messaging-system.rsexperimentloader.enabled": False,
    "app.update.disabledForTesting": True, "app.update.auto": False, "app.update.enabled": False,
    "datareporting.policy.dataSubmissionEnabled": False, "toolkit.telemetry.enabled": False,
    "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False,
    "media.autoplay.default": 5,
}
VIEW_W, VIEW_H = 1489, 844  # as firefox_realsites.py sizes its window (the viewport is 1477 x 827, GAPS B28)


# MARKS_OUT=FILE: at each ["mark", LABEL] of INPUT_ACTIONS, the browser's unique memory by process
# (proc_uss.firefox_mb) and a screenshot (SHOTS_DIR, default realsites-mini) as LABEL-firefox.png;
# written to FILE with the marks' times (task 0e92: Firefox's side of the end-to-end run on X)
MARKS_OUT = os.environ.get("MARKS_OUT")
MARKS = {}


def on_mark(m, label):
    import base64, proc_uss
    MARKS[label] = proc_uss.firefox_mb(m.process_id)
    shots = os.path.join(HERE, os.environ.get("SHOTS_DIR", "realsites-mini"))
    os.makedirs(shots, exist_ok=True)
    open(os.path.join(shots, f"{label}-firefox.png"), "wb").write(base64.b64decode(m.screenshot()))
    print("mark", label, MARKS[label], flush=True)


def save_profile(m, dest):
    """Quit Firefox in-app, so that its databases are closed and checkpointed, and copy the profile
    it ran on to `dest`."""
    import shutil
    try:
        m.quit()
    except Exception:
        pass
    shutil.copytree(m.instance.profile.profile, dest, ignore=shutil.ignore_patterns("lock", ".parentlock"))


def main():
    site, url, probe = sys.argv[1], sys.argv[2], open(sys.argv[3]).read()
    proxy = replay.start(site, "probe")
    m = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={**PREFS, **(proxy.prefs if proxy else {})},
                   profile=os.environ.get("REALSITES_PROFILE", os.path.expanduser("~/.cache/mrt-realsites-profile")))
    m.start_session(proxy.capabilities if proxy else None)
    try:
        m.set_context("content")
        m.set_window_rect(width=VIEW_W + 40, height=VIEW_H + 120)
        m.timeout.page_load = 120
        t0 = time.time()
        m.navigate(url)
        print(f"loaded after {time.time() - t0:.1f} s", flush=True)
        if os.environ.get("INPUT_ACTIONS"):
            import input_actions
            time.sleep(1)
            steps = input_actions.perform(m, json.load(open(os.environ["INPUT_ACTIONS"])), lambda label: on_mark(m, label))
            print("actions done:", steps.done, "of", len(steps.steps), "failed:", steps.failed, flush=True)
            if MARKS_OUT:
                json.dump({"marks": [[label, round(t - t0, 1)] for label, t in steps.marks], "memory": MARKS,
                           "done": steps.done, "of": len(steps.steps), "failed": steps.failed}, open(MARKS_OUT, "w"), indent=1)
        for _ in range(int(os.environ.get("STEPS", "8"))):
            # PROBE_CONTEXT=chrome runs the probe in the browser's chrome (ChromeUtils, every process)
            with m.using_context(os.environ.get("PROBE_CONTEXT", "content")):
                print(f"{time.time() - t0:.1f}", json.dumps(m.execute_script(probe)), flush=True)
            time.sleep(float(os.environ.get("EVERY", "4")))
    finally:
        if os.environ.get("SAVE_PROFILE"):
            save_profile(m, os.environ["SAVE_PROFILE"])
        firefox_exit.close(m)
        if proxy:
            proxy.close()


if __name__ == "__main__":
    main()
