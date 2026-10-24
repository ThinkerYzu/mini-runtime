"""mitmproxy addon: record a site's traffic, then answer from the recording, so that Firefox and the mini
runtime load the same HTML, CSS and JS (DESIGN "Replay: the same content on both sides").

Started by bench/replay.py, not by hand. Options (mitmdump --set name=value):
  replay_mode    record | fill | replay
  replay_dir     the site's directory: record.flows, fill.flows
  replay_log     a JSONL file, one line per request: how it was answered
  replay_deterministic   true: put deterministic.js at the top of HTML documents (fill and replay)
  replay_timing  hold | header | instant: a recorded answer is sent after its recorded duration (Firefox),
                 at once with that duration in an x-mrt-recorded-ms header (the mini runtime; long polls
                 still held), or at once (SPEC decision 20)
  replay_inject  a probe script's path, put after deterministic.js (REPLAY_INJECT in replay.py)
  replay_exact_query     a regex of URLs whose response is made from the whole query (a bundle of the
                         files it lists, as Reddit's /js/concat): matched by exact URL only, never by
                         path; default Reddit's concat endpoint

record   every request goes to the site; every flow is saved to record.flows
fill     answered from record.flows and fill.flows; the rest go to the site and are saved to fill.flows
replay   answered from both files; a versioned bundle (Google's k=, Reddit's concat) not in them goes to
         the site and is saved to fill.flows (SPEC decision 18); the rest get a 404
"""
import asyncio, base64, hashlib, json, os, re, time
from urllib.parse import parse_qsl, urlsplit

from mitmproxy import ctx, http, io

# Reddit's module loader asks for the chunks it needs in one request, batched by time (a 25 ms
# debounce), so the mini runtime's batches differ from Firefox's: another chunk list is another body.
EXACT_QUERY = r"^https://www\.redditstatic\.com/js/concat\?"

# A recorded answer slower than this is a long poll the server held open (Gmail's push channel,
# Docs' bind channel): it is replayed after its recorded time, not at once, or the page polls again
# at once and loops (Gmail made 14,590 channel requests in 15 s). The bridge gives up on a fetch
# after the same time and leaves it in flight (mini_realsites.py PENDING_MS).
LONG_POLL_S = 5
# the recorded duration of an answer, for the mini runtime's bridge (replay_timing=header)
RECORDED_MS = "x-mrt-recorded-ms"

DETERMINISTIC_JS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "deterministic.js")


def body_hash(req):
    return hashlib.sha1(req.raw_content or b"").hexdigest()


def query_name(req):
    """The GraphQL query a Meta site's form post names (fb_api_req_friendly_name), or None."""
    m = re.search(rb"(?:^|&)fb_api_req_friendly_name=([A-Za-z0-9_]+)", req.raw_content or b"")
    return m.group(1).decode() if m else None


# Google's module bundles (Closure's module manager on Gmail and the OneGoogle widgets) put in the
# path the modules asked for (m=) and the ones the page already has (exm=). The second depends on
# the order earlier bundles arrived in, which changes from run to run: a replay asks for paths the
# recording never saw, on either side, and the page loses the modules those bundles carry.
MODULE_PATH = re.compile(r"/m=[^/?]*")
EXCLUDED_PATH = re.compile(r"/exm=([^/?]*)")


# A versioned script bundle: its content is fixed by the version in the path (k=), whatever the module
# lists say. A replay that lacks one may fetch it from the site (SPEC decision 18, GAPS B60).
VERSIONED_BUNDLE = re.compile(r"/js/k=[^/]+/.*/m=[^/?]+")
# Reddit's chunk batch: the files named by `chunks=`, content hashes (the site answers it immutable).
# Which chunks go in one batch follows the page's timing (a debounce over deterministic.js's
# Date.now()), so a replay may ask for a batch the recording lacks (SPEC decision 18, task 612c).
CONCAT_BUNDLE = re.compile(r"^https://www\.redditstatic\.com/js/concat\?(?:.*&)?chunks=[^&]+")


def versioned_bundle(req):
    """Whether `req` asks for a bundle whose URL fixes its content (SPEC decision 18)."""
    return bool(VERSIONED_BUNDLE.search(req.path.split("?", 1)[0]) or CONCAT_BUNDLE.search(req.url))


def module_key(req):
    """path_key without the exm= segment, for a module bundle; None for any other URL."""
    path = req.path.split("?", 1)[0]
    if not MODULE_PATH.search(path) or not EXCLUDED_PATH.search(path):
        return None
    return (req.method, req.scheme, req.host, req.port, EXCLUDED_PATH.sub("/exm=", path))


def excluded_modules(req):
    m = EXCLUDED_PATH.search(req.path.split("?", 1)[0])
    return set(m.group(1).split(",")) if m else set()


def path_key(req):
    """The method, scheme, host, port and path: the loosest key, for URLs whose query changes per load."""
    return (req.method, req.scheme, req.host, req.port, req.path.split("?", 1)[0])


class Replay:
    def __init__(self):
        self.exact = {}  # (method, url, body hash) -> [flow]
        self.by_url = {}  # (method, url) -> [flow]
        self.by_path = {}  # path_key -> [flow]
        self.by_module = {}  # module_key -> [flow]
        self.served = {}  # key -> number of flows already served under it
        self.writer = None
        self.log = None
        self.record_ms = None

    def load(self, loader):
        loader.add_option("replay_mode", str, "replay", "record | fill | replay")
        loader.add_option("replay_dir", str, "", "the site's recording directory")
        loader.add_option("replay_log", str, "", "JSONL request log")
        loader.add_option("replay_deterministic", bool, True, "inject deterministic.js into HTML (replay)")
        loader.add_option("replay_timing", str, "hold", "hold | header | instant (SPEC decision 20)")
        loader.add_option("replay_inject", str, "", "a probe script put after deterministic.js (fill and replay)")
        loader.add_option("replay_exact_query", str, EXACT_QUERY, "URLs matched by exact URL only (regex)")

    def configure(self, updated):
        if "replay_dir" not in updated and "replay_mode" not in updated:
            return
        mode, d = ctx.options.replay_mode, ctx.options.replay_dir
        os.makedirs(d, exist_ok=True)
        if mode != "record":
            for name in ("record.flows", "fill.flows"):
                path = os.path.join(d, name)
                if os.path.exists(path):
                    with open(path, "rb") as f:
                        for flow in io.FlowReader(f).stream():
                            if isinstance(flow, http.HTTPFlow) and flow.response:
                                self.add(flow)
        if mode in ("record", "fill"):
            name = "record.flows" if mode == "record" else "fill.flows"
            self.writer = io.FlowWriter(open(os.path.join(d, name), "ab" if mode == "fill" else "wb"))
        if ctx.options.replay_log:
            self.log = open(ctx.options.replay_log, "a")
        ctx.log.info(f"replay: mode {mode}, {len(self.exact)} recorded keys, recorded at {self.record_ms}")

    def add(self, flow):
        r = flow.request
        self.exact.setdefault((r.method, r.url, body_hash(r)), []).append(flow)
        self.by_url.setdefault((r.method, r.url), []).append(flow)
        self.by_path.setdefault(path_key(r), []).append(flow)
        if module_key(r):
            self.by_module.setdefault(module_key(r), []).append(flow)
        ms = int(r.timestamp_start * 1000)
        self.record_ms = ms if self.record_ms is None else min(self.record_ms, ms)

    def take(self, key, flows):
        """The next flow under `key`, in recorded order; the last one again once they run out."""
        i = self.served.get(key, 0)
        self.served[key] = i + 1
        return flows[min(i, len(flows) - 1)]

    def find(self, req):
        """The recorded flow for `req` and how it matched, or (None, None)."""
        key = (req.method, req.url, body_hash(req))
        if key in self.exact:
            return self.take(key, self.exact[key]), "exact"
        key = (req.method, req.url)
        if key in self.by_url:
            # the same named query first: Meta's GraphQL posts share one URL, and served in recorded
            # order a query gets whichever answer comes next (task 85f3: Instagram's chat tab got a
            # quick promotion's answer in Firefox, failed on its missing mailbox and drew nothing)
            name = query_name(req)
            same = [f for f in self.by_url[key] if name and query_name(f.request) == name]
            if same:
                return self.take(key + (name,), same), "body-differs"
            return self.take(key, self.by_url[key]), "body-differs"
        key = path_key(req)
        if key in self.by_path and not re.search(ctx.options.replay_exact_query or r"(?!)", req.url):
            # the recorded flow with the most query parameters in common
            want = set(parse_qsl(urlsplit(req.url).query, keep_blank_values=True))
            flows = self.by_path[key]
            # the same body first: it says what is asked for where the query is a counter (Gmail's
            # /sync/u/0/i/fd?c=N names the threads it wants in its body; another thread's answer
            # makes it ask again, 316 times in 15 s)
            body = body_hash(req)
            common = [(body_hash(f.request) == body, len(want & set(parse_qsl(urlsplit(f.request.url).query, keep_blank_values=True))))
                      for f in flows]
            # those with the most in common, in recorded order as take() serves them: a long
            # poll's quick first answer, then the held ones
            best = [f for f, n in zip(flows, common) if n == max(common)]
            return self.take(("query",) + key, best), "query-differs"
        key = module_key(req)
        if key in self.by_module:
            # the same modules asked for, by a page that had at least the modules this one has: its
            # bundle carries what this page lacks, perhaps with a few it has already. Never one
            # made for a page with more: it would leave out modules this one needs (Gmail then
            # fails with "Module started executing setup but did not synchronously complete setup")
            have = excluded_modules(req)
            fits = [f for f in self.by_module[key] if excluded_modules(f.request) <= have]
            if fits:
                return max(fits, key=lambda f: len(excluded_modules(f.request))), "exm-differs"
        return None, None

    def write_log(self, flow, outcome, matched=None):
        if not self.log:
            return
        r = flow.request
        entry = {"t": round(time.time(), 3), "method": r.method, "url": r.url, "outcome": outcome,
                 "status": flow.response.status_code if flow.response else None}
        if matched is not None and matched.request.url != r.url:
            entry["recorded_url"] = matched.request.url
        # Meta's GraphQL posts name their query in the body; a body-differs answer may be another
        # query's (task 85f3: Instagram's chat tab got a quick promotion's answer in Firefox)
        if r.method == "POST":
            name = query_name(r)
            if name:
                entry["query"] = name
                if matched is not None and query_name(matched.request) != name:
                    entry["recorded_query"] = query_name(matched.request)
        self.log.write(json.dumps(entry) + "\n")
        self.log.flush()

    async def request(self, flow):
        mode = ctx.options.replay_mode
        if mode == "record":
            return
        if flow.request.headers.get("upgrade", "").lower() == "websocket":
            if mode == "replay":
                flow.response = http.Response.make(404, b"", {"content-type": "text/plain"})
                self.write_log(flow, "websocket-refused")
            return
        found, how = self.find(flow.request)
        if found:
            flow.response = found.response.copy()
            flow.metadata["replay"] = how
            self.write_log(flow, how, found)
            took = (found.response.timestamp_end or found.request.timestamp_start) - found.request.timestamp_start
            timing = ctx.options.replay_timing
            if timing == "header":
                h = flow.response.headers
                h[RECORDED_MS] = str(round(took * 1000))
                # the bridge tab's fetch of a cross-origin answer sees only the headers it exposes
                if "access-control-allow-origin" in h:
                    exposed = h.get("access-control-expose-headers")
                    # by name: "*" exposes nothing to a credentialed request
                    if exposed is None or RECORDED_MS not in exposed.lower():
                        h["access-control-expose-headers"] = (exposed + ", " if exposed else "") + RECORDED_MS
            if took > LONG_POLL_S or (timing == "hold" and took > 0):
                await asyncio.sleep(took)
        elif mode == "replay" and versioned_bundle(flow.request):
            # to the site; response() saves it to fill.flows and indexes it for this run
            flow.metadata["replay"] = "live-bundle"
        elif mode == "replay":
            flow.response = http.Response.make(404, b"not in the recording", {"content-type": "text/plain"})
            flow.metadata["replay"] = "missing"
            self.write_log(flow, "missing")

    def response(self, flow):
        if flow.metadata.get("replay") == "live-bundle":
            if not self.writer:
                self.writer = io.FlowWriter(open(os.path.join(ctx.options.replay_dir, "fill.flows"), "ab"))
            self.writer.add(flow)
            self.writer.fo.flush()
            self.add(flow)
            self.write_log(flow, "live-bundle")
        elif "replay" not in flow.metadata:
            # went to the site: saved before anything below changes it
            if self.writer:
                self.writer.add(flow)
                self.writer.fo.flush()
            self.write_log(flow, "forwarded")
        # fill too: it collects what a replay asks for, and a page whose requests depend on the
        # clock (Reddit batches its modules by Date.now()) asks for other URLs without it
        if ctx.options.replay_mode in ("replay", "fill") and ctx.options.replay_deterministic:
            self.inject(flow)

    def error(self, flow):
        """A request that got no response (connection or TLS failure on the way to the site)."""
        if self.log:
            self.log.write(json.dumps({"t": round(time.time(), 3), "method": flow.request.method, "url": flow.request.url,
                                       "outcome": "error", "error": str(flow.error)[:200]}) + "\n")
            self.log.flush()

    def inject(self, flow):
        """deterministic.js at the top of an HTML document, allowed by its CSP (as mozproxy's
        inject-deterministic.py does)."""
        resp = flow.response
        if "text/html" not in resp.headers.get("content-type", "") or self.record_ms is None:
            return
        resp.decode()
        html = resp.text
        js = open(DETERMINISTIC_JS).read().replace("REPLACE_LOAD_TIMESTAMP", str(self.record_ms))
        if ctx.options.replay_inject:
            js += "\n" + open(ctx.options.replay_inject).read()
        if js in html:
            return
        at = None
        for pat in (r"(?i)<head[^>]*>", r"(?i)<html[^>]*>", r"(?i)<!doctype html>"):
            mo = re.search(pat, html)
            if mo:
                at = mo.end()
                break
        if at is None:
            return
        # every policy must allow the script, and a response may carry several (Gmail sends three; the
        # second's 'strict-dynamic' makes the first's 'unsafe-inline' no help)
        nonce = None
        for header in ("Content-Security-Policy", "Content-Security-Policy-Report-Only"):
            policies = resp.headers.get_all(header)
            if not policies:
                continue
            for p, csp in enumerate(policies):
                directives = [d.strip() for d in csp.split(";")]
                for i, d in enumerate(directives):
                    if not d.startswith("script-src "):
                        continue
                    sources = d.split()[1:]
                    n = next((s[len("'nonce-"):-1] for s in sources if s.startswith("'nonce-")), None)
                    if n:
                        nonce = n
                    elif "'unsafe-inline'" not in sources or any(s.startswith("'sha") or s == "'strict-dynamic'" for s in sources):
                        # 'unsafe-inline' is ignored next to a hash or 'strict-dynamic' (CSP 3)
                        sha = base64.b64encode(hashlib.sha256(js.encode()).digest()).decode()
                        if any(s.startswith("'sha") for s in sources):
                            sources.append(f"'sha256-{sha}'")
                        else:
                            sources.append("'unsafe-inline'")
                        directives[i] = "script-src " + " ".join(sources)
                        policies[p] = "; ".join(directives)
            resp.headers.set_all(header, policies)
        tag = f'<script nonce="{nonce}">' if nonce else "<script>"
        resp.text = html[:at] + tag + js + "</script>" + html[at:]

    def done(self):
        if self.writer:
            self.writer.fo.close()


addons = [Replay()]
