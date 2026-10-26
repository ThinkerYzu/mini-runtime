#!/usr/bin/env python3
"""Run records and diagnostics for a service discovery run (plain Firefox reference, then the viewer).

A run is one replay of a discovery plan for one task. It lives in a private directory,
$XDG_DATA_HOME/mini-runtime/discovery/runs/<run id>/, outside every repository:

    manifest.json   discovery-run-manifest/1: task, plan identity, binaries actually running, profiles
    results.json    discovery-run-results/1: step results, rewritten as each step finishes
    events.jsonl    timestamped run, step, capture and launch events, appended as they happen
    artifacts/      screenshots, URLs, DOM, consoles and logs, per side and step
    launcher/       the viewer launcher's session files, copied before its cleanup
    summary.md      redacted summary, written when the run ends or is recovered

The record formats are defined in the documentation repository's discovery/schemas/; set
DISCOVERY_SCHEMAS to that directory to check runs against them (`check`). Raw files stay
unredacted and private; excerpts, events and reports are redacted.

The viewer is started through tools/view.py, the standalone launcher: it never starts a
Firefox bridge, and this module refuses a driver whose environment says otherwise. Browsers
(the plain-Firefox reference and the Firefox that displays the viewer) are driven with
Marionette, which only the browser parts import.

    python3 tools/discovery_run.py recover RUN       # mark a dead run interrupted, keep its logs
    python3 tools/discovery_run.py link-bug RUN STEP TASK
    python3 tools/discovery_run.py report RUN        # redacted JSON for the task and submission
    python3 tools/discovery_run.py check RUN         # records against DISCOVERY_SCHEMAS
"""
import argparse
import base64
import contextlib
import fcntl
import hashlib
import json
import os
from pathlib import Path
import queue
import re
import shutil
import signal
import socket
import subprocess
import sys
import threading
import time
import traceback
from urllib.parse import parse_qs, urlsplit
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]
RESULTS = ("pass", "fail", "blocked", "inconclusive")
SIDES = ("reference", "viewer")
# A failed viewer step needs each of these, as a file or with a reason (discovery-run-results/1).
FAILURE_KINDS = ("error-text", "url", "screenshot", "console", "runtime-log")
PLAN_PATH = re.compile(r"^discovery/plans/[a-z0-9][a-z0-9-]*\.plan\.json$")
TASK = re.compile(r"^[0-9a-f]{5,96}$")
STEP = re.compile(r"^[a-z][a-z0-9-]{0,47}$")


def xdg_home(name, fallback):
    value = os.environ.get(name)
    path = Path(value) if value else fallback
    return path if path.is_absolute() else fallback


def discovery_home():
    return xdg_home("XDG_DATA_HOME", Path.home() / ".local/share") / "mini-runtime/discovery"


def host_name():
    """The host the records name; the validator compares it with its own (DISCOVERY_HOST)."""
    return os.environ.get("DISCOVERY_HOST") or socket.gethostname()


def now():
    t = time.time()
    return time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime(t)) + f".{int(t % 1 * 1000):03d}Z"


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def write_json(path, value):
    """Replace `path` with `value` so a reader sees the old or the new record, never half of one."""
    tmp = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    with open(tmp, "w") as f:
        json.dump(value, f, indent=2)
        f.write("\n")
        f.flush()
        os.fsync(f.fileno())
    os.replace(tmp, path)


# Redaction of excerpts, events and reports. Raw files keep everything and stay private.
SECRET_PARAMS = (r"token|access_token|id_token|refresh_token|auth|authuser_token|authorization|key|api_key|apikey|"
                 r"sig|signature|session|sessionid|sid|password|passwd|pwd|code|state|secret|client_secret")
SECRET_HEADERS = r"authorization|proxy-authorization|cookie|set-cookie|x-viewer-token|x-api-key"
REDACTIONS = [
    # query parameters, and the same `name=value` in free text (a console message, a log line)
    (re.compile(r"(?<![A-Za-z0-9_.-])((?:" + SECRET_PARAMS + r")=)[^&#\s\"'<>]*", re.I), r"\1[redacted]"),
    # header lines and JSON forms: `Cookie: a=b`, "Cookie": "a=b", ["Cookie", "a=b"]
    (re.compile(r"(?im)^(\s*(?:" + SECRET_HEADERS + r")\s*:\s*).+$"), r"\1[redacted]"),
    (re.compile(r"(\"(?:" + SECRET_HEADERS + r")\"\s*[:,]\s*\")(?:[^\"\\]|\\.)*", re.I), r"\1[redacted]"),
    (re.compile(r"\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]{8,}", re.I), r"\1 [redacted]"),
]


def redact(text):
    """`text` without credentials, cookies, authorization headers or tokenized URLs, for excerpts"""
    for pattern, replacement in REDACTIONS:
        text = pattern.sub(replacement, text)
    return text


def redact_all(value):
    """`value` (JSON data) with redact() applied to every string in it"""
    if isinstance(value, str):
        return redact(value)
    if isinstance(value, list):
        return [redact_all(v) for v in value]
    if isinstance(value, dict):
        return {k: redact_all(v) for k, v in value.items()}
    return value


def plan_identity(path):
    """The plan's discovery-plan/1 record and its reference (path, revision, sha256 of its bytes).

    The plan must be at discovery/plans/<service>.plan.json in a checkout of the documentation
    repository; the reference names it by that repository-relative path."""
    path = Path(path).resolve()
    relative = "/".join(path.parts[-3:])
    if not PLAN_PATH.match(relative):
        raise ValueError(f"plan must be discovery/plans/<service>.plan.json, not {path}")
    data = path.read_bytes()
    plan = json.loads(data)
    if plan.get("schema") != "discovery-plan/1" or not isinstance(plan.get("revision"), int):
        raise ValueError(f"{path} is not a discovery-plan/1 record with a revision")
    return plan, {"path": relative, "revision": plan["revision"], "sha256": hashlib.sha256(data).hexdigest()}


def git_identity(repo=ROOT):
    """The runtime commit, whether the tree differs from it, and a hash of the difference"""
    git = lambda *args: subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True).stdout
    commit = git("rev-parse", "HEAD").decode().strip()
    diff = git("diff", "--binary", "HEAD")
    untracked = [n for n in git("ls-files", "--others", "--exclude-standard", "-z").decode().split("\0") if n]
    identity = {"runtime_commit": commit, "runtime_dirty": bool(diff or untracked)}
    if identity["runtime_dirty"]:
        digest = hashlib.sha256(diff)
        for name in sorted(untracked):
            digest.update(b"\0" + name.encode() + b"\0")
            with contextlib.suppress(OSError):
                digest.update((repo / name).read_bytes())
        identity["dirty_diff_sha256"] = digest.hexdigest()
    return identity


def process_started_at(pid):
    """When process `pid` started, as a UTC timestamp (from /proc)"""
    stat = Path(f"/proc/{pid}/stat").read_text()
    ticks = int(stat.rsplit(")", 1)[1].split()[19])
    boot = next(int(line.split()[1]) for line in Path("/proc/stat").read_text().splitlines() if line.startswith("btime "))
    t = boot + ticks / os.sysconf("SC_CLK_TCK")
    return time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime(t)) + f".{int(t % 1 * 1000):03d}Z"


def descendants(pid):
    """The live processes below `pid`"""
    children = {}
    for entry in Path("/proc").iterdir():
        if entry.name.isdigit():
            with contextlib.suppress(OSError, IndexError, ValueError):
                ppid = int((entry / "stat").read_text().rsplit(")", 1)[1].split()[1])
                children.setdefault(ppid, []).append(int(entry.name))
    found, todo = [], [pid]
    while todo:
        for child in children.get(todo.pop(), []):
            found.append(child)
            todo.append(child)
    return found


def running_binary(pid):
    """{name, path, sha256} of the executable process `pid` is running now.

    Hashes /proc/<pid>/exe, the image the process runs, even if the file on disk was rebuilt
    or removed since (its path then ends with " (deleted)")."""
    path = os.readlink(f"/proc/{pid}/exe")
    return {"name": Path(path.removesuffix(" (deleted)")).name, "path": path, "sha256": sha256_file(f"/proc/{pid}/exe")}


def process_environ(pid):
    raw = Path(f"/proc/{pid}/environ").read_bytes()
    return dict(item.split("=", 1) for item in raw.decode(errors="replace").split("\0") if "=" in item)


class Run:
    """One discovery run's private directory and records.

    Start one with Run.start(); it holds the directory's lock until finish() or until the
    process dies, so `recover` can tell a dead run from a live one. Use it as a context
    manager: leaving the block with an exception, or a SIGTERM/SIGINT, marks the run
    interrupted, with the results recorded so far kept. Steps are recorded with step()."""

    def __init__(self, directory):
        self.dir = Path(directory)
        self.manifest = json.loads((self.dir / "manifest.json").read_text())
        self.results = json.loads((self.dir / "results.json").read_text())
        self.host = self.manifest["host"]
        self.artifacts = Path(self.manifest["artifacts_dir"])
        self.lock = None
        self.viewer_sessions = []
        self.binaries = {}   # (path, sha256) -> binary seen running during this run
        self.capture_seq = 0

    @classmethod
    def start(cls, plan_file, task, role, *, host=None):
        """Create the run directory for `task` (a prefix of at least five characters) replaying
        `plan_file` in `role` (explore, fix or verify), and write the first records.

        The run id is the UTC second, the plan's service and the task prefix; a second run
        started in the same second for the same task is refused, never merged."""
        if not TASK.match(task):
            raise ValueError(f"task must be a tracker id prefix of 5 to 96 hex characters, not {task!r}")
        if role not in ("explore", "fix", "verify"):
            raise ValueError(f"role must be explore, fix or verify, not {role!r}")
        plan, plan_ref = plan_identity(plan_file)
        home = discovery_home()
        run_id = f"{time.strftime('%Y%m%dT%H%M%SZ', time.gmtime())}-{plan['service']}-{task[:5]}"
        runs = home / "runs"
        runs.mkdir(mode=0o700, parents=True, exist_ok=True)
        directory = runs / run_id
        directory.mkdir(mode=0o700)  # FileExistsError: that run exists
        (directory / "artifacts").mkdir(mode=0o700)
        lock = open(directory / "run.lock", "w")
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        lock.write(json.dumps({"pid": os.getpid(), "host": host or host_name()}) + "\n")
        lock.flush()
        started = now()
        baseline = plan.get("baseline", {})
        write_json(directory / "manifest.json", {
            "schema": "discovery-run-manifest/1", "run_id": run_id, "task": task, "role": role,
            "host": host or host_name(), "plan": plan_ref,
            # replaced by the reference browser's own when one is started (reference())
            "firefox": {"version": baseline.get("firefox_version") or "unknown", "reference_used": False},
            "started_at": started, "status": "running", "artifacts_dir": str(directory / "artifacts")})
        excluded = [{"step_ids": e["step_ids"], "bug_tasks": e["bug_tasks"]}
                    for e in plan.get("exclusions", []) if e.get("status") == "excluded"]
        write_json(directory / "results.json", {
            "schema": "discovery-run-results/1", "run_id": run_id, "task": task,
            "plan_revision": plan_ref["revision"], "reference": [], "viewer": [], "excluded": excluded})
        run = cls(directory)
        run.lock = lock
        run.plan = plan
        run.event("run-started", plan=plan_ref, role=role, pid=os.getpid())
        return run

    def __enter__(self):
        self._signals = {s: signal.signal(s, self._on_signal) for s in (signal.SIGTERM, signal.SIGINT)}
        return self

    def _on_signal(self, signum, frame):
        raise SystemExit(f"stopped by signal {signum}")

    def __exit__(self, kind, error, tb):
        for s, handler in self._signals.items():
            signal.signal(s, handler)
        if self.manifest["status"] == "running":
            why = "".join(traceback.format_exception_only(kind, error)).strip() if kind else "run ended without finish()"
            self.finish("interrupted", reason=why)
        return False

    # Records

    def event(self, name, **fields):
        """Append one timestamped event to events.jsonl (redacted), durably"""
        line = json.dumps(redact_all({"at": now(), "event": name, **fields}))
        with open(self.dir / "events.jsonl", "a") as f:
            f.write(line + "\n")
            f.flush()
            os.fsync(f.fileno())

    def save_manifest(self):
        write_json(self.dir / "manifest.json", self.manifest)

    def save_results(self):
        write_json(self.dir / "results.json", self.results)

    def evidence_file(self, kind, side, step_id, name, data, excerpt=None):
        """Save `data` (bytes or str) as a file of this run's artifacts, and return its evidence entry"""
        folder = self.artifacts / side / step_id
        folder.mkdir(mode=0o700, parents=True, exist_ok=True)
        path = folder / name
        path.write_bytes(data.encode() if isinstance(data, str) else data)
        return self._entry(kind, path, excerpt)

    def evidence_copy(self, kind, side, step_id, source, name=None, excerpt=None):
        """Copy the file `source` into this run's artifacts, and return its evidence entry"""
        folder = self.artifacts / side / step_id
        folder.mkdir(mode=0o700, parents=True, exist_ok=True)
        path = folder / (name or Path(source).name)
        shutil.copyfile(source, path)
        return self._entry(kind, path, excerpt)

    def _entry(self, kind, path, excerpt):
        entry = {"kind": kind, "path": str(path), "host": self.host, "sha256": sha256_file(path), "captured_at": now()}
        if excerpt:
            entry["excerpt"] = redact(excerpt)[:2000]
        return entry

    @staticmethod
    def unavailable(kind, reason):
        """An evidence entry for a diagnostic that could not be captured, and why"""
        reason = redact(str(reason)).strip() or "no reason given"
        return {"kind": kind, "unavailable_reason": reason if len(reason) >= 10 else f"unavailable: {reason}"}

    def record(self, side, step_id, result, observed, started_at, *, ended_at=None, bug_task=None,
               blocked_by=None, reason=None, evidence=(), actual_inputs=None):
        """Add one finished step's result to results.json and return it.

        The schema's rules are checked here rather than left for the validator: fail names its
        bug task (or `pending` until link_bug()), blocked names earlier steps of the same side
        that did not pass, inconclusive has a reason, and a failed viewer step has each of
        FAILURE_KINDS as a file or an explicit unavailable reason. A step has one result per side."""
        if side not in SIDES or result not in RESULTS:
            raise ValueError(f"side {side!r} / result {result!r}")
        if not STEP.match(step_id):
            raise ValueError(f"step id {step_id!r}")
        done = {r["step_id"]: r for r in self.results[side]}
        if step_id in done:
            raise ValueError(f"{side} step {step_id} already has a result")
        entry = {"step_id": step_id, "result": result, "started_at": started_at, "ended_at": ended_at or now(),
                 "observed": redact(observed or "")}
        if not entry["observed"]:
            raise ValueError("observed must say what was seen")
        if result == "fail":
            if bug_task is not None and bug_task != "pending" and not TASK.match(bug_task):
                raise ValueError(f"bug task {bug_task!r}")
            if bug_task not in (None, "pending"):
                entry["bug_task"] = bug_task
            missing = sorted(set(FAILURE_KINDS) - {e["kind"] for e in evidence}) if side == "viewer" else []
            if missing:
                raise ValueError(f"failed viewer step {step_id} needs evidence or unavailable reasons for: {', '.join(missing)}")
        if result == "blocked":
            if not blocked_by:
                raise ValueError("blocked needs blocked_by")
            for cause in blocked_by:
                if cause not in done or done[cause]["result"] == "pass":
                    raise ValueError(f"{step_id} cannot be blocked by {cause}: it has no {side} result that did not pass")
            entry["blocked_by"] = list(blocked_by)
        if result == "inconclusive":
            if not reason or len(reason) < 10:
                raise ValueError("inconclusive needs a reason of at least ten characters")
            entry["reason"] = redact(reason)
        if actual_inputs:
            entry["actual_inputs"] = actual_inputs
        if evidence:
            entry["evidence"] = list(evidence)
        self.results[side].append(entry)
        self.save_results()
        self.event("step-ended", side=side, step=step_id, result=result, bug_task=entry.get("bug_task"),
                   evidence=[e.get("path") or f"{e['kind']}: unavailable" for e in evidence])
        return entry

    def link_bug(self, step_id, bug_task, side="viewer"):
        """Name the bug task of a failed step recorded before the task existed"""
        if not TASK.match(bug_task):
            raise ValueError(f"bug task {bug_task!r}")
        entry = next((r for r in self.results[side] if r["step_id"] == step_id), None)
        if entry is None or entry["result"] != "fail":
            raise ValueError(f"{side} step {step_id} has no failed result")
        entry["bug_task"] = bug_task
        self.save_results()
        self.event("bug-linked", side=side, step=step_id, bug_task=bug_task)

    @contextlib.contextmanager
    def step(self, side, step_id, *, capture=None):
        """Run one step's body and record its result.

        The body calls one of the yielded Step's pass_(), fail(), blocked() or inconclusive().
        `capture` (a Browser) collects diagnostics on fail, and when the body raises: an
        exception is recorded as inconclusive (an automation failure is not a runtime
        difference until evidence shows one) and does not propagate, so independent steps can
        go on. SystemExit and KeyboardInterrupt propagate and interrupt the run."""
        s = Step(self, side, step_id, capture)
        self.event("step-started", side=side, step=step_id)
        try:
            yield s
        except Exception as error:
            if s.recorded:
                self.event("step-error-after-result", side=side, step=step_id, error=f"{type(error).__name__}: {error}")
                return
            text = "".join(traceback.format_exception(error))
            evidence = s.diagnostics(text)
            self.record(side, step_id, "inconclusive", f"automation error: {type(error).__name__}: {error}", s.started_at,
                        reason=f"automation error before an outcome was observed: {type(error).__name__}: {error}",
                        evidence=evidence)
            return
        if not s.recorded:
            self.record(side, step_id, "inconclusive", "the step ended without recording a result", s.started_at,
                        reason="the step's code recorded no result", evidence=s.diagnostics(None))

    # Browsers and the viewer

    def reference(self, firefox_bin, profile, launch_options=()):
        """Start the plain-Firefox reference on `profile` (a Profiles.reference path) and record it"""
        browser = Browser(self, "reference", firefox_bin, profile)
        self.manifest["firefox"] = {"version": browser.version, "reference_used": True, "binary_path": str(Path(firefox_bin).resolve()),
                                    "profile_id": profile.name if profile.name != "reference" else profile.parent.name,
                                    "launch_options": ["--marionette", "--profile", "<reference-profile>", *launch_options]}
        self.save_manifest()
        self.event("reference-started", version=browser.version, binary=str(firefox_bin), profile=str(profile))
        return browser

    def viewer(self, url, profile, *, port=0, timeout=60):
        """Start the standalone viewer for `url` on the native profile `profile`, and record the
        build that is actually running"""
        session = ViewerSession(self, url, profile, port=port)
        session.start(timeout)
        self.viewer_sessions.append(session)
        build = dict(git_identity(), binaries=session.binaries(), started_at=process_started_at(session.proc.pid))
        self.manifest["viewer"] = {"build": build, "profile_id": profile.name if profile.name != "viewer" else profile.parent.name,
                                   "launch_options": ["tools/view.py", "<url>", "--profile", "<viewer-profile>", "--port", str(port),
                                                      "NO_FIREFOX=1", "API_PROCESS=all"]}
        if session.session_dir:
            self.manifest["viewer"]["launcher_session_dir"] = str(session.session_dir)
        self.save_manifest()
        return session

    def note_binaries(self, binaries):
        """Add binaries seen running to the manifest's build identity"""
        build = self.manifest.get("viewer", {}).get("build")
        if build is None:
            return
        known = {(b["path"], b["sha256"]) for b in build["binaries"]}
        new = [b for b in binaries if (b["path"], b["sha256"]) not in known]
        if new:
            build["binaries"].extend(new)
            self.save_manifest()
            self.event("binaries-seen", binaries=new)

    def finish(self, status="complete", reason=None):
        """Stop the viewer sessions this run started, keep their files, and close the records.

        `complete` needs a viewer (schema); a run that never reached the viewer is interrupted."""
        for session in self.viewer_sessions:
            with contextlib.suppress(Exception):
                session.stop()
        if status == "complete" and "viewer" not in self.manifest:
            status, reason = "interrupted", "the run never started the viewer"
        self.manifest.update(status=status, ended_at=now())
        self.save_manifest()
        self.event("run-ended", status=status, reason=reason)
        write_summary(self.dir)
        if self.lock:
            self.lock.close()
            self.lock = None


class Step:
    """One step in progress; see Run.step()"""

    def __init__(self, run, side, step_id, capture):
        self.run, self.side, self.step_id, self.capture = run, side, step_id, capture
        self.started_at = now()
        self.recorded = False

    def diagnostics(self, error_text):
        if self.capture is not None:
            return self.capture.capture(self.step_id, error_text)
        evidence = [self.run.unavailable("screenshot", "no browser was attached to this step"),
                    self.run.unavailable("url", "no browser was attached to this step"),
                    self.run.unavailable("console", "no browser was attached to this step"),
                    self.run.unavailable("runtime-log", "no viewer session was attached to this step")]
        if error_text:
            evidence.insert(0, self.run.evidence_file("error-text", self.side, self.step_id, "error.txt", error_text, error_text[-600:]))
        else:
            evidence.insert(0, self.run.unavailable("error-text", "the step reported no error text"))
        return evidence

    def _record(self, result, observed, **kw):
        if self.recorded:
            raise ValueError(f"{self.step_id} already has a result")
        entry = self.run.record(self.side, self.step_id, result, observed, self.started_at, **kw)
        self.recorded = True
        return entry

    def pass_(self, observed, **kw):
        return self._record("pass", observed, **kw)

    def fail(self, observed, error_text, *, bug_task="pending", evidence=(), **kw):
        """A difference from the reference: diagnostics are captured now, before anything is torn down"""
        return self._record("fail", observed, bug_task=bug_task, evidence=[*self.diagnostics(error_text), *evidence], **kw)

    def blocked(self, observed, blocked_by, **kw):
        return self._record("blocked", observed, blocked_by=blocked_by, **kw)

    def inconclusive(self, observed, reason, *, capture=True, **kw):
        evidence = [*self.diagnostics(None), *kw.pop("evidence", ())] if capture else kw.pop("evidence", ())
        return self._record("inconclusive", observed, reason=reason, evidence=evidence, **kw)


class Profiles:
    """Dedicated profiles for one run: a Firefox profile for the reference, a native one for the viewer.

    By default both are new, under discovery/profiles/<run id>/. With `name` they are the
    persistent pair discovery/profiles/<name>/, kept between runs (an account signed in for
    discovery), and locked so two runs never share them. Nothing is imported from or
    written to the owner's profiles: a path outside discovery/profiles is refused."""

    def __init__(self, run, name=None):
        home = Path(run.manifest["artifacts_dir"]).parents[2] / "profiles"
        if name is not None and not STEP.match(name):
            raise ValueError(f"profile name {name!r}")
        self.dir = home / (name or run.manifest["run_id"])
        self.dir.mkdir(mode=0o700, parents=True, exist_ok=name is not None)
        self.lock = open(self.dir / ".discovery.lock", "w")
        try:
            fcntl.flock(self.lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            self.lock.close()
            raise RuntimeError(f"profiles in use by another run: {self.dir}") from None
        self.reference = self.dir / "reference"
        self.viewer = self.dir / "viewer"
        for path in (self.reference, self.viewer):
            path.mkdir(mode=0o700, exist_ok=True)
            check_profile(path)
        run.event("profiles", dir=str(self.dir), persistent=name is not None)

    def close(self):
        self.lock.close()


def check_profile(path):
    """Refuse a profile that is not a dedicated discovery profile"""
    path = Path(path).resolve()
    home = discovery_home().resolve()
    owner = [Path.home() / ".mozilla", Path.home() / "snap", Path.home() / ".var",
             xdg_home("XDG_DATA_HOME", Path.home() / ".local/share") / "mini-runtime/profiles"]
    if any(path == o.resolve() or o.resolve() in path.parents for o in owner):
        raise ValueError(f"refusing an owner profile: {path}")
    if (home / "profiles") not in path.parents:
        raise ValueError(f"discovery profiles live under {home / 'profiles'}, not {path}")


class ViewerSession:
    """The standalone viewer for one site, started through tools/view.py"""

    def __init__(self, run, url, profile, port=0):
        check_profile(profile)
        self.run, self.url, self.profile, self.port = run, url, Path(profile), port
        self.proc = None
        self.session_dir = None
        self.viewer_url = None
        self.copied = False
        n = len(run.viewer_sessions)
        self.logs = run.dir / "viewer" / f"launcher-{n}"
        self.logs.mkdir(mode=0o700, parents=True, exist_ok=True)

    def start(self, timeout):
        env = dict(os.environ)
        stderr = open(self.logs / "driver-stderr.log", "w")
        self.proc = subprocess.Popen([sys.executable, "-S", str(ROOT / "tools/view.py"), self.url,
                                      "--profile", str(self.profile), "--port", str(self.port)],
                                     env=env, stdout=subprocess.PIPE, stderr=stderr, text=True)
        stderr.close()
        lines = queue.Queue()

        def output():
            # keep the pipe drained for the driver's lifetime, and its stdout on disk
            with open(self.logs / "driver-stdout.log", "w") as out:
                for line in self.proc.stdout:
                    out.write(redact(line))
                    out.flush()
                    lines.put(line.strip())
            self.proc.stdout.close()
            lines.put(None)
        threading.Thread(target=output, daemon=True).start()
        deadline = time.monotonic() + timeout
        while self.viewer_url is None:
            try:
                line = lines.get(timeout=max(.1, deadline - time.monotonic()))
            except queue.Empty:
                line = None
            if line is None:
                self.stop()
                raise RuntimeError(f"viewer did not start (exit {self.proc.poll()}); see {self.logs}")
            if line.startswith("Session files: "):
                self.session_dir = Path(line.split(": ", 1)[1])
            elif line.startswith("VIEWER_URL "):
                self.viewer_url = line.split()[1]
        parsed = urlsplit(self.viewer_url)
        self.origin = f"{parsed.scheme}://{parsed.netloc}"
        self.token = parse_qs(parsed.query)["token"][0]
        environ = process_environ(self.proc.pid)
        if environ.get("NO_FIREFOX") != "1" or environ.get("API_PROCESS") != "all":
            self.stop()
            raise RuntimeError("the viewer driver is not standalone (NO_FIREFOX=1, API_PROCESS=all); refusing a Firefox bridge")
        self.run.event("viewer-started", pid=self.proc.pid, session_dir=str(self.session_dir), viewer_url=self.viewer_url,
                       profile=str(self.profile))

    def call(self, path, body=None, timeout=10):
        req = Request(self.origin + "/api/" + path, data=None if body is None else json.dumps(body).encode(),
                      headers={"X-Viewer-Token": self.token, "Content-Type": "application/json"})
        with urlopen(req, timeout=timeout) as response:
            return json.load(response)

    def binaries(self):
        """The runtime and API binaries running under the driver now"""
        found = {}
        for pid in descendants(self.proc.pid):
            with contextlib.suppress(OSError):
                binary = running_binary(pid)
                if binary["name"] in ("realsite", "mrt-api"):
                    found[(binary["path"], binary["sha256"])] = binary
        return sorted(found.values(), key=lambda b: (b["name"], b["path"]))

    def session_files(self):
        """The launcher session's files by evidence kind"""
        kinds = {"runtime-log": [], "api-log": [], "request-log": [], "console": [], "other": []}
        if not self.session_dir or not self.session_dir.is_dir():
            return kinds
        for path in sorted(self.session_dir.rglob("*")):
            if not path.is_file():
                continue
            if path.parent.name == "console":
                kinds["console"].append(path)
            elif re.search(r"-api-\d+\.log$", path.name):
                kinds["api-log"].append(path)
            elif path.name.endswith("-requests.jsonl"):
                kinds["request-log"].append(path)
            elif path.suffix == ".log":
                kinds["runtime-log"].append(path)
            else:
                kinds["other"].append(path)
        return kinds

    def stop(self):
        """Stop the driver (it stops its runtime and API processes), then keep its session files"""
        if self.proc is None:
            return
        if self.proc.poll() is None:
            try:
                self.call("stop", {})
                self.proc.wait(timeout=10)
            except Exception:
                self.proc.terminate()
                try:
                    self.proc.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    self.proc.kill()
                    self.proc.wait()
        self.preserve()
        self.run.event("viewer-stopped", pid=self.proc.pid, exit=self.proc.returncode)

    def preserve(self):
        """Copy the launcher's session directory into the run (its logs, consoles, report)"""
        if self.copied or not self.session_dir or not self.session_dir.is_dir():
            return
        shutil.copytree(self.session_dir, self.logs / "session", dirs_exist_ok=True)
        self.copied = True
        self.run.event("launcher-preserved", source=str(self.session_dir), copy=str(self.logs / "session"))


CONSOLE_SCRIPT = """
const out = [];
for (const m of Services.console.getMessageArray() || []) {
  if (m instanceof Ci.nsIScriptError) {
    out.push({source: "error", at: m.timeStamp, flags: m.flags, category: m.category,
              text: m.errorMessage, file: m.sourceName, line: m.lineNumber});
  } else {
    out.push({source: "message", text: m.message});
  }
}
try {
  const storage = Cc["@mozilla.org/consoleAPI-storage;1"].getService(Ci.nsIConsoleAPIStorage);
  for (const e of storage.getEvents()) {
    let args;
    try { args = e.arguments.map(a => typeof a === "string" ? a : JSON.stringify(a)); } catch (_) { args = ["<unserializable>"]; }
    out.push({source: "console-api", at: e.timeStamp, level: e.level, text: args.join(" "), file: e.filename, line: e.lineNumber});
  }
} catch (e) { out.push({source: "capture", text: "console API storage: " + e}); }
return out;
"""


class Browser:
    """A Firefox driven with Marionette: the plain reference, or the Firefox displaying the viewer.

    The reference runs on its dedicated profile (kept; Marionette's own preferences are
    restored when it stops). The viewer's display Firefox gets a temporary profile, removed
    when it stops: the viewer's state is in the native profile, not here."""

    def __init__(self, run, side, firefox_bin, profile=None, viewer=None):
        from marionette_driver.marionette import Marionette
        prefs = {"marionette.allowSystemAccess": True, "browser.aboutwelcome.enabled": False,
                 "browser.shell.checkDefaultBrowser": False, "devtools.console.stdout.content": True}
        self.run, self.side, self.viewer = run, side, viewer
        # Firefox's stdout and stderr, which include the content's console calls
        # (devtools.console.stdout.content): the parent's console storage does not hold them
        (run.dir / side).mkdir(mode=0o700, exist_ok=True)
        self.output = run.dir / side / f"firefox-{sum(1 for _ in (run.dir / side).glob('firefox-*.log'))}.log"
        if profile is not None:
            check_profile(profile)
            from mozprofile import Profile
            profile = Profile(profile=str(profile), preferences=prefs)
        # chrome context, for the browser console (capture()), needs system access since Firefox 138
        previous = os.environ.get("MOZ_REMOTE_ALLOW_SYSTEM_ACCESS")
        os.environ["MOZ_REMOTE_ALLOW_SYSTEM_ACCESS"] = "1"
        try:
            self.m = Marionette(bin=str(firefox_bin), profile=profile, prefs=prefs, gecko_log=str(self.output))
            self.m.start_session()
        finally:
            if previous is None:
                del os.environ["MOZ_REMOTE_ALLOW_SYSTEM_ACCESS"]
            else:
                os.environ["MOZ_REMOTE_ALLOW_SYSTEM_ACCESS"] = previous
        self.version = self.m.session_capabilities.get("browserVersion", "unknown")
        self.m.set_context("content")

    def open_viewer(self, timeout=30):
        """Load the viewer's page and wait until it is connected to the runtime"""
        self.m.navigate(self.viewer.viewer_url)
        deadline = time.monotonic() + timeout
        while not self.m.execute_script('const s = document.getElementById("status");'
                                        'return !!s && s.textContent.startsWith("Connected")'):
            if time.monotonic() > deadline:
                raise TimeoutError("the viewer did not connect")
            time.sleep(.1)

    def capture(self, step_id, error_text=None):
        """Collect this side's diagnostics for `step_id` now, before any restart or teardown.

        Each kind that fails is recorded as unavailable with the error. Afterwards the browser
        is in its top-level content context."""
        run, side = self.run, self.side
        run.capture_seq += 1
        folder = f"{step_id}-{run.capture_seq}"
        evidence = []

        def attempt(kind, get):
            try:
                result = get()
                if isinstance(result, dict):
                    evidence.append(result)
                else:
                    evidence.extend(result)
            except Exception as error:
                evidence.append(run.unavailable(kind, f"{type(error).__name__}: {error}"))

        if error_text:
            evidence.append(run.evidence_file("error-text", side, folder, "error.txt", error_text, error_text[-600:]))
        else:
            evidence.append(run.unavailable("error-text", "the step reported no error text"))

        def frame_top():
            self.m.set_context("content")
            self.m.switch_to_frame()
        with contextlib.suppress(Exception):
            frame_top()

        def urls():
            text = {"browser": self.m.get_url()}
            if self.viewer is not None:
                with contextlib.suppress(Exception):
                    text["runtimes"] = self.viewer.call("runtimes")
            body = json.dumps(text, indent=2)
            return run.evidence_file("url", side, folder, "url.json", body, body)
        attempt("url", urls)
        attempt("screenshot", lambda: run.evidence_file("screenshot", side, folder, "screenshot.png",
                                                         base64.b64decode(self.m.screenshot())))
        attempt("dom", lambda: run.evidence_file("dom", side, folder, "dom.html", self.m.page_source))
        if self.viewer is not None:
            def viewer_state():
                state = self.m.execute_script("""
                    const f = document.getElementById("page");
                    let frame = null;
                    try { frame = f && f.contentDocument ? f.contentDocument.documentElement.outerHTML : null; } catch (e) { frame = String(e); }
                    const status = document.getElementById("status");
                    return {status: status ? status.textContent : null, frame};""")
                frame = state.pop("frame") or ""
                files = [run.evidence_file("viewer-state", side, folder, "viewer-state.json", json.dumps(state, indent=2),
                                           f"viewer status: {state.get('status')}")]
                files.append(run.evidence_file("dom", side, folder, "viewer-frame.html", frame))
                return files
            attempt("viewer-state", viewer_state)

        def console():
            self.m.set_context("chrome")
            try:
                messages = self.m.execute_script(CONSOLE_SCRIPT)
            finally:
                self.m.set_context("content")
            errors = [m for m in messages if m.get("source") == "error" or m.get("level") == "error"]
            excerpt = "\n".join(f"{m.get('text')} ({m.get('file')}:{m.get('line')})" for m in errors[-5:])
            return run.evidence_file("console", side, folder, "browser-console.json", json.dumps(messages, indent=2),
                                     excerpt or f"{len(messages)} console messages, no errors")
        attempt("console", console)

        def content_console():
            calls = [line for line in self.output.read_text(errors="replace").splitlines()
                     if line.startswith(("console.error:", "console.warn:", "JavaScript error:"))]
            return run.evidence_copy("console", side, folder, self.output, "firefox-output.log",
                                     "\n".join(calls[-5:]) or "no console errors or warnings in Firefox's output")
        attempt("console", content_console)
        if self.viewer is None:
            for kind in ("runtime-log",):
                evidence.append(run.unavailable(kind, "the plain-Firefox reference runs no mini-runtime"))
        else:
            def logs():
                files = self.viewer.session_files()
                entries = []
                for kind in ("runtime-log", "api-log", "request-log", "console"):
                    for path in files[kind]:
                        name = path.name if kind != "console" else "runtime-console-" + path.name
                        tail = path.read_text(errors="replace")[-800:] if kind == "runtime-log" else None
                        entries.append(run.evidence_copy(kind, side, folder, path, name, tail))
                if not files["console"]:
                    # realsite writes MRT_DUMP_CONSOLE when it reports, at the session's end
                    entries.append(run.unavailable("console", "the runtime writes its page console only when the session stops; "
                                                   "see the preserved launcher session's console/ after the run"))
                if not files["runtime-log"]:
                    entries.append(run.unavailable("runtime-log", f"no runtime log in the launcher session {self.viewer.session_dir}"))
                driver = self.viewer.logs / "driver-stderr.log"
                if driver.is_file():
                    entries.append(run.evidence_copy("driver-log", side, folder, driver, None,
                                                     driver.read_text(errors="replace")[-800:]))
                return entries
            attempt("runtime-log", logs)
            try:
                run.note_binaries(self.viewer.binaries())
            except Exception as error:
                run.event("binaries-unavailable", error=str(error))
        run.event("captured", side=side, step=step_id, files=[e.get("path") for e in evidence if "path" in e],
                  unavailable=[e["kind"] for e in evidence if "unavailable_reason" in e])
        return evidence

    def close(self):
        sys.path.insert(0, str(ROOT / "bench"))
        import firefox_exit
        firefox_exit.close(self.m)


def write_summary(directory):
    """summary.md: the run's status and step results, redacted"""
    directory = Path(directory)
    manifest = json.loads((directory / "manifest.json").read_text())
    results = json.loads((directory / "results.json").read_text())
    lines = [f"# Run {manifest['run_id']}", "",
             f"Task {manifest['task']} ({manifest['role']}), plan {manifest['plan']['path']} revision {manifest['plan']['revision']},"
             f" host {manifest['host']}, status **{manifest['status']}**, {manifest['started_at']} to {manifest.get('ended_at', '-')}.", ""]
    build = manifest.get("viewer", {}).get("build")
    if build:
        dirty = " (dirty)" if build["runtime_dirty"] else ""
        lines += [f"Runtime {build['runtime_commit'][:12]}{dirty}: " + ", ".join(f"{b['name']} {b['sha256'][:12]}" for b in build["binaries"]), ""]
    for side in SIDES:
        lines += [f"## {side}", "", "| Step | Result | Observed |", "|---|---|---|"]
        for r in results[side]:
            extra = r.get("bug_task") or ",".join(r.get("blocked_by", [])) or r.get("reason", "")
            lines.append(f"| {r['step_id']} | {r['result']} {extra} | {r['observed']} |".replace("\n", " "))
        lines.append("")
    if results["excluded"]:
        lines += ["Excluded: " + "; ".join(f"{', '.join(e['step_ids'])} ({', '.join(e['bug_tasks'])})" for e in results["excluded"]), ""]
    (directory / "summary.md").write_text(redact("\n".join(lines)))


def find_run(name):
    path = Path(name)
    if not path.is_dir():
        path = discovery_home() / "runs" / name
    if not (path / "manifest.json").is_file():
        raise SystemExit(f"no run at {name}")
    return path


def recover(directory):
    """Close a run whose process died: mark it interrupted, record unfinished steps, stop and keep its viewers.

    A run whose lock is still held is alive and is left alone. Steps that started and never
    ended get no result (nothing was observed); they are listed in an event and the summary.
    A viewer driver left running is stopped only when its environment proves it is this
    run's (its MINI_DIR is the recorded session directory)."""
    directory = Path(directory)
    with open(directory / "run.lock", "a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return "alive"
        run = Run(directory)
        if run.manifest["status"] != "running":
            return run.manifest["status"]
        events = [json.loads(line) for line in (directory / "events.jsonl").read_text().splitlines() if line.strip()]
        open_steps = {}
        for e in events:
            if e["event"] == "step-started":
                open_steps[(e["side"], e["step"])] = e["at"]
            elif e["event"] == "step-ended":
                open_steps.pop((e["side"], e["step"]), None)
        for n, e in enumerate(x for x in events if x["event"] == "viewer-started"):
            session_dir, pid = Path(e["session_dir"]), e["pid"]
            with contextlib.suppress(OSError):
                if process_environ(pid).get("MINI_DIR") == str(session_dir):
                    os.kill(pid, signal.SIGTERM)
                    for _ in range(100):
                        if not Path(f"/proc/{pid}").exists() or "Z" in Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1][:3]:
                            break
                        time.sleep(.1)
                    run.event("viewer-stopped", pid=pid, by="recover")
            target = directory / "viewer" / f"launcher-{n}" / "session"
            if session_dir.is_dir() and not target.exists():
                shutil.copytree(session_dir, target)
                run.event("launcher-preserved", source=str(session_dir), copy=str(target), by="recover")
            elif not target.exists():
                run.event("launcher-unavailable", source=str(session_dir), reason="the launcher session directory no longer exists")
        run.event("steps-abandoned", steps=[{"side": s, "step": i, "started_at": at} for (s, i), at in open_steps.items()])
        run.manifest.update(status="interrupted", ended_at=now())
        run.save_manifest()
        run.event("run-ended", status="interrupted", reason="recovered after its process died")
        write_summary(directory)
        if open_steps:
            with open(directory / "summary.md", "a") as f:
                f.write("Interrupted during: " + ", ".join(f"{s} {i}" for s, i in open_steps) + " (no result recorded)\n")
        return "interrupted"


def report(directory):
    """Redacted JSON for the task comment and the completion submission"""
    directory = Path(directory)
    manifest = json.loads((directory / "manifest.json").read_text())
    results = json.loads((directory / "results.json").read_text())
    outcome = lambda r: {k: r[k] for k in ("step_id", "result", "bug_task", "blocked_by", "reason") if k in r}
    pending = [r["step_id"] for side in SIDES for r in results[side] if r["result"] == "fail" and "bug_task" not in r]
    unavailable = [{"side": side, "step": r["step_id"], "kind": e["kind"], "reason": e["unavailable_reason"]}
                   for side in SIDES for r in results[side] for e in r.get("evidence", []) if "unavailable_reason" in e]
    excerpts = [{"side": side, "step": r["step_id"], "kind": e["kind"], "path": e["path"], "excerpt": e["excerpt"]}
                for side in SIDES for r in results[side] if r["result"] != "pass" for e in r.get("evidence", []) if e.get("excerpt")]
    return redact_all(({
        "run": {"run_id": manifest["run_id"], "host": manifest["host"],
                "manifest_path": str(directory / "manifest.json"), "results_path": str(directory / "results.json")},
        "status": manifest["status"], "plan": manifest["plan"], "build": manifest.get("viewer", {}).get("build"),
        "firefox": manifest["firefox"],
        "steps": [outcome(r) for r in results["viewer"]],
        "reference_steps": [outcome(r) for r in results["reference"]],
        "excluded": sorted({s for e in results["excluded"] for s in e["step_ids"]}),
        "bug_links_pending": pending, "unavailable": unavailable, "excerpts": excerpts}))


def check(directory, schemas, plan=None):
    """Problems of the run's records (and of the plan file `plan`, if given) against the discovery schemas, as strings"""
    import jsonschema
    resources = [json.loads(p.read_text()) for p in sorted(Path(schemas).glob("*.schema.json"))]
    by_name = {r["$id"]: r for r in resources}
    try:
        from referencing import Registry, Resource
        registry = Registry().with_resources((r["$id"], Resource.from_contents(r)) for r in resources)
        jsonschema.Draft202012Validator({}, registry=registry)
    except (ImportError, TypeError):  # Firefox's mach environment has an older one
        raise RuntimeError("checking the schemas needs jsonschema 4.18 or later") from None
    problems = []
    records = [(name, json.loads((Path(directory) / name).read_text()), schema_id) for name, schema_id in
               (("manifest.json", "urn:discovery:run-manifest:1"), ("results.json", "urn:discovery:run-results:1"))]
    if plan is not None:
        records.append((str(plan), json.loads(Path(plan).read_text()), "urn:discovery:plan:1"))
    for name, record, schema_id in records:
        validator = jsonschema.Draft202012Validator(by_name[schema_id], registry=registry)
        problems += [f"{name}: /{'/'.join(map(str, e.absolute_path))}: {e.message}" for e in validator.iter_errors(record)]
    return problems


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    for name in ("recover", "report", "check"):
        sub.add_parser(name).add_argument("run", help="run id or directory")
    sub.choices["check"].add_argument("--plan", type=Path, help="also check this plan file")
    link = sub.add_parser("link-bug")
    link.add_argument("run")
    link.add_argument("step")
    link.add_argument("task")
    link.add_argument("--side", default="viewer", choices=SIDES)
    args = parser.parse_args()
    directory = find_run(args.run)
    if args.command == "recover":
        print(recover(directory))
    elif args.command == "report":
        print(json.dumps(report(directory), indent=2))
    elif args.command == "link-bug":
        with open(directory / "run.lock", "a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            run = Run(directory)
            run.link_bug(args.step, args.task, args.side)
            write_summary(directory)
    else:
        schemas = os.environ.get("DISCOVERY_SCHEMAS")
        if not schemas:
            parser.exit(2, "set DISCOVERY_SCHEMAS to the documentation repository's discovery/schemas\n")
        problems = check(directory, schemas, args.plan)
        print("\n".join(problems) or "records conform")
        parser.exit(1 if problems else 0)


if __name__ == "__main__":
    main()
