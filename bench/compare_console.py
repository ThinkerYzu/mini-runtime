"""GAPS B20: the mini runtime's console against Firefox's for the same replayed site.

Firefox's side is bench/realsites-console/<site>-firefox.json (firefox_realsites.py at its live
measuring point: the console API calls of the page's window, and its script errors and warnings);
the mini runtime's is <site>-mini.json (realsite's console lines, MRT_DUMP_CONSOLE, written by
mini_realsites.py). Both not in git (third-party text).

The mini runtime's lines carry no level, so they are sorted by their text:
  rejection   "uncaught (in promise): R"           Firefox: an error "uncaught exception: R" or R's message
  exception   "uncaught exception in a ...: E"      Firefox: an error with E's message
  console     anything else (console.log, ...)      Firefox: a console API call with the same text
Firefox's warnings are the browser's own (autoplay refused, deprecations, ORB, cookies) and have no
counterpart in the mini runtime; they are listed apart.

usage: python3 bench/compare_console.py [SITE ...]   (default: every site with both files)
"""
import collections, json, os, re, sys

DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "realsites-console")


def mini_entries(path):
    out = []
    for line in json.load(open(path)):
        first = line.split("\n", 1)[0]
        if first.startswith("uncaught (in promise): "):
            out.append(("rejection", first[len("uncaught (in promise): "):]))
        elif first.startswith("uncaught exception in a"):
            out.append(("exception", first.split(": ", 1)[1] if ": " in first else first))
        else:
            out.append(("console", first))
    return out


def firefox_entries(path):
    d = json.load(open(path))
    # the capture's own check line (firefox_realsites.py logs it through the page's console) is not the page's
    out = [("console", e["text"].split("\n", 1)[0]) for e in d["api"] if e["text"] != "mrt-console-probe"]
    warnings = []
    for e in d["errors"]:
        if e["level"] == "warning":
            warnings.append(f'{e["category"]}: {e["text"]}')
            continue
        text = e["text"]
        # Firefox writes a rejection whose reason is not an Error as "uncaught exception: R"
        if text.startswith("uncaught exception: "):
            out.append(("rejection", text[len("uncaught exception: "):]))
        else:
            out.append(("error", text))
    return out, warnings


def norm(text):
    # numbers, ids and URLs with cache-busters differ between runs; compare their shape
    return re.sub(r"\d+", "N", text)[:160]


def compare(site):
    mini = mini_entries(os.path.join(DIR, site + "-mini.json"))
    ff, warnings = firefox_entries(os.path.join(DIR, site + "-firefox.json"))
    print(f"== {site}: mini {len(mini)} lines, Firefox {len(ff)} messages and {len(warnings)} warnings")
    count = lambda entries: collections.Counter(k for k, _ in entries)
    print("   mini by kind:   ", dict(count(mini)))
    print("   Firefox by kind:", dict(count(ff)))
    # an error in Firefox can be a rejection or an exception in the mini runtime: match on the text
    ff_texts = collections.Counter(norm(t) for _, t in ff)
    mini_texts = collections.Counter(norm(t) for _, t in mini)
    both = ff_texts & mini_texts
    print(f"   in both: {sum(both.values())}")
    for t, n in sorted(both.items(), key=lambda x: -x[1]):
        print(f"      {n:3}  {t}")
    print(f"   only in the mini runtime: {sum((mini_texts - ff_texts).values())}")
    for kind, text in mini:
        t = norm(text)
        if (mini_texts - ff_texts)[t]:
            print(f"      {(mini_texts - ff_texts)[t]:3}  [{kind}] {t}")
            mini_texts[t] = ff_texts[t]  # print each once
    print(f"   only in Firefox: {sum((ff_texts - collections.Counter(norm(t) for _, t in mini)).values())}")
    mt = collections.Counter(norm(t) for _, t in mini)
    for kind, text in ff:
        t = norm(text)
        if (ff_texts - mt)[t]:
            print(f"      {(ff_texts - mt)[t]:3}  [{kind}] {t}")
            ff_texts[t] = mt[t]
    print("   Firefox's warnings (the browser's own):")
    for t, n in collections.Counter(norm(w) for w in warnings).most_common():
        print(f"      {n:3}  {t}")


if __name__ == "__main__":
    from recorded_tests import require_recordings
    require_recordings(os.path.join(DIR, "*-mini.json"), os.path.join(DIR, "*-firefox.json"),
                       record="matching Firefox and runtime runs producing realsites-console/<site>-*.json")
    sites = sys.argv[1:] or sorted({f.rsplit("-", 1)[0] for f in os.listdir(DIR) if f.endswith("-mini.json")
                                    and os.path.exists(os.path.join(DIR, f.replace("-mini.json", "-firefox.json")))})
    if not sites:
        sys.exit("No matching console recordings. Record both sides for the same site first.")
    require_recordings(*(os.path.join(DIR, site + suffix) for site in sites
                         for suffix in ("-mini.json", "-firefox.json")),
                       record="matching Firefox and runtime console captures for each selected site")
    for site in sites:
        compare(site)
