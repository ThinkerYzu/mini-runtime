"""Which Firefox state the comparison scripts read (GAPS A17).

Since session 39 every Firefox harness measures each page twice: plain (after GC and CC: the existing
fields) and after Firefox's "Minimize memory usage" (`minimized` in the test-page results;
`live_minimized`, `browser_minimized`, `unique_split_minimized` in firefox_realsites.py's).
FIREFOX_STATE=minimized makes a comparison script read the minimized ones; the default is plain.
"""
import os

STATE = os.environ.get("FIREFOX_STATE", "plain")
assert STATE in ("plain", "minimized"), "FIREFOX_STATE is plain or minimized"


def test_pages(results):
    """A test-page harness's results (firefox_static, firefox_js, firefox_hibernate) in the chosen state."""
    if STATE == "plain":
        return results
    missing = [p for p, v in results.items() if "minimized" not in v]
    assert not missing, f"no minimized measurement for {missing}: re-run the harness (session 39 or later)"
    return {p: {**v["minimized"], "elements": v.get("elements"), "height": v.get("height")} for p, v in results.items()}


def real_site(v):
    """One site's firefox_realsites.py result in the chosen state: `live`, `browser` and `unique_split`
    replaced by their minimized versions (the browser's blank baseline is then the blank tab after the
    page, minimized)."""
    if STATE == "plain" or "error" in v:
        return v
    assert v.get("live_minimized"), "no minimized measurement (a run before session 39, or EXTRA=1)"
    bm = v["browser_minimized"]
    browser = {**v["browser"], "live": bm["live"], "blank": bm["blank_after"], "page_cost": bm["page_cost"],
               "page_cost_by_kind": bm["page_cost_by_kind"]}
    return {**v, "live": v["live_minimized"], "browser": browser, "unique_split": v.get("unique_split_minimized")}


def report_suffix():
    """The kept memory report of the live page in the chosen state: <site>-live.json.gz or -live-minimized."""
    return "-live" if STATE == "plain" else "-live-minimized"
