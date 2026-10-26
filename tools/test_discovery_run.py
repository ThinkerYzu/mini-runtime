#!/usr/bin/env python3
"""Discovery run helper acceptance (tools/discovery_run.py) on local fixtures.

Without a browser: records and step rules, run ids, redaction, profiles, the real viewer's
running binaries and preserved launcher files, interrupted and SIGKILLed runs. With
VIEWER_BROWSER=1 and FIREFOX_BIN (Marionette's Python environment): a reference and viewer
journey with a deliberate failure, its screenshot, consoles and logs. DISCOVERY_SCHEMAS (the
documentation repository's discovery/schemas) also checks every run's records against them.
"""
import functools
import hashlib
import http.server
import json
import os
from pathlib import Path
import signal
import shutil
import stat
import subprocess
import sys
import tempfile
import textwrap
import threading
import time
import unittest
from unittest import mock

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))
import discovery_run  # noqa: E402

PAGE = """<!doctype html><title>Fixture</title><h1 id="title">Fixture page</h1>
<button id="add">Add</button><output id="count">0</output>
<script>
let n = 0;
document.getElementById('add').onclick = () => { document.getElementById('count').textContent = String(++n); };
console.error('fixture error token=abc123 for the console capture');
</script>"""


def plan(url, revision=1, exclusions=()):
    step = lambda id, requires=(): {"id": id, "kind": "interaction", "action": f"Do {id}", "inputs": {}, "expected": f"{id} works",
                                     "timeout_s": 10, "requires": list(requires), "viewer_only": False}
    return {"schema": "discovery-plan/1", "service": "fixture", "revision": revision,
            "journey": {"name": "fixture", "purpose": "Exercise the run helper on a local page"}, "url": url,
            "baseline": {"firefox_version": "160.0a1", "observed_at": "2026-10-25T00:00:00Z", "task": "abcde",
                         "run_id": "20261025T000000Z-fixture-abcde"},
            "setup": {"profile": "fresh discovery profiles", "account": "none", "viewport": {"width": 800, "height": 600},
                      "starting_state": "The fixture page", "cleanup": "Stop both sessions"},
            "steps": [step("open"), step("add", ["open"]), step("missing", ["open"]), step("after", ["missing"])],
            "exclusions": list(exclusions), "changes": [{"revision": revision, "task": "abcde", "summary": "Fixture plan"}]}


class Fixture(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="mrt-discovery-")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        env = {"XDG_DATA_HOME": str(self.root / "data"), "XDG_CACHE_HOME": str(self.root / "cache"), "DISCOVERY_HOST": "fixture-host"}
        patcher = mock.patch.dict(os.environ, env)
        patcher.start()
        self.addCleanup(patcher.stop)
        (self.root / "site").mkdir()
        (self.root / "site/page.html").write_text(PAGE)

        class Quiet(http.server.SimpleHTTPRequestHandler):
            def log_message(self, *args):
                pass
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Quiet, directory=str(self.root / "site")))
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.addCleanup(self.server.server_close)
        self.addCleanup(self.server.shutdown)
        self.url = f"http://localhost:{self.server.server_port}/page.html"
        self.plan_file = self.root / "docs/discovery/plans/fixture.plan.json"
        self.plan_file.parent.mkdir(parents=True)
        self.plan_file.write_text(json.dumps(plan(self.url), indent=2))

    def conforms(self, run_dir):
        schemas = os.environ.get("DISCOVERY_SCHEMAS")
        if schemas:
            try:
                self.assertEqual(discovery_run.check(run_dir, schemas, self.plan_file), [])
            except RuntimeError:
                # this Python's jsonschema is too old (Marionette's environment): the system one checks
                out = subprocess.run([os.environ.get("DISCOVERY_CHECK_PYTHON", "/usr/bin/python3"), str(HERE / "discovery_run.py"), "check", str(run_dir), "--plan", str(self.plan_file)],
                                     capture_output=True, text=True)
                self.assertEqual((out.returncode, out.stdout.strip()), (0, "records conform"), out.stderr)

    def events(self, run_dir):
        return [json.loads(line) for line in (Path(run_dir) / "events.jsonl").read_text().splitlines()]


class Records(Fixture):
    def test_records_steps_and_report(self):
        with discovery_run.Run.start(self.plan_file, "abcdef0123", "explore") as run:
            self.assertTrue(run.dir.is_relative_to(self.root / "data/mini-runtime/discovery/runs"))
            self.assertEqual(stat.S_IMODE(run.dir.stat().st_mode), 0o700)
            self.assertRegex(run.dir.name, r"^\d{8}T\d{6}Z-fixture-abcde$")
            manifest = json.loads((run.dir / "manifest.json").read_text())
            self.assertEqual(manifest["status"], "running")
            self.assertEqual(manifest["plan"], {"path": "discovery/plans/fixture.plan.json", "revision": 1,
                                                "sha256": hashlib.sha256(self.plan_file.read_bytes()).hexdigest()})
            with run.step("viewer", "open") as s:
                s.pass_("page shown")
            with run.step("viewer", "missing") as s:
                s.fail("no #missing element", "expected #missing, found none")
            with run.step("viewer", "add") as s:
                raise RuntimeError("driver lost the element")
            with run.step("viewer", "after") as s:
                s.blocked("needs missing", ["missing"])
            with run.step("reference", "open") as s:
                pass  # no result recorded
            results = json.loads((run.dir / "results.json").read_text())
            viewer = {r["step_id"]: r for r in results["viewer"]}
            self.assertEqual({k: v["result"] for k, v in viewer.items()},
                             {"open": "pass", "missing": "fail", "add": "inconclusive", "after": "blocked"})
            self.assertNotIn("bug_task", viewer["missing"])  # pending until the bug task exists
            kinds = {e["kind"]: e for e in viewer["missing"]["evidence"]}
            self.assertEqual(set(kinds), set(discovery_run.FAILURE_KINDS))
            self.assertIn("no browser", kinds["screenshot"]["unavailable_reason"])
            self.assertEqual(Path(kinds["error-text"]["path"]).read_text(), "expected #missing, found none")
            self.assertEqual(kinds["error-text"]["sha256"], discovery_run.sha256_file(kinds["error-text"]["path"]))
            self.assertEqual(kinds["error-text"]["host"], "fixture-host")
            self.assertIn("RuntimeError: driver lost the element", viewer["add"]["reason"])
            trace = next(e for e in viewer["add"]["evidence"] if e["kind"] == "error-text")
            self.assertIn("Traceback", Path(trace["path"]).read_text())
            self.assertEqual(results["reference"][0]["result"], "inconclusive")
            # the schema's rules are refused when recorded, not left for the validator
            for bad in (dict(step_id="open", result="pass"),  # a second result
                        dict(step_id="x1", result="fail"),  # failed viewer step without evidence
                        dict(step_id="x2", result="blocked", blocked_by=["open"]),  # its cause passed
                        dict(step_id="x3", result="blocked", blocked_by=["nowhere"]),
                        dict(step_id="x4", result="inconclusive", reason="short")):
                with self.subTest(bad=bad), self.assertRaises(ValueError):
                    step_id, result = bad.pop("step_id"), bad.pop("result")
                    run.record("viewer", step_id, result, "observed", discovery_run.now(), **bad)
            self.assertEqual(discovery_run.report(run.dir)["bug_links_pending"], ["missing"])
            run.link_bug("missing", "f00d1")
            run.finish()
        manifest = json.loads((run.dir / "manifest.json").read_text())
        # no viewer was started, so the run cannot be complete
        self.assertEqual(manifest["status"], "interrupted")
        self.assertIn("ended_at", manifest)
        out = discovery_run.report(run.dir)
        self.assertEqual(out["steps"], [{"step_id": "open", "result": "pass"},
                                        {"step_id": "missing", "result": "fail", "bug_task": "f00d1"},
                                        {"step_id": "add", "result": "inconclusive", "reason": viewer["add"]["reason"]},
                                        {"step_id": "after", "result": "blocked", "blocked_by": ["missing"]}])
        self.assertEqual(out["bug_links_pending"], [])
        self.assertEqual(out["run"]["manifest_path"], str(run.dir / "manifest.json"))
        events = self.events(run.dir)
        self.assertEqual(events[0]["event"], "run-started")
        self.assertEqual(events[-1]["event"], "run-ended")
        self.assertEqual([e["at"] for e in events], sorted(e["at"] for e in events))
        self.assertEqual([(e["step"], e["event"]) for e in events if e["event"].startswith("step-") and e["side"] == "viewer"],
                         [("open", "step-started"), ("open", "step-ended"), ("missing", "step-started"), ("missing", "step-ended"),
                          ("add", "step-started"), ("add", "step-ended"), ("after", "step-started"), ("after", "step-ended")])
        self.assertIn("| missing | fail f00d1 |", (run.dir / "summary.md").read_text())

    def test_exclusions_and_plan_location(self):
        excluded = {"bug_tasks": ["b0b01"], "step_ids": ["after"], "reason": "b0b01 is deferred by the owner",
                    "restore_when": "b0b01 has a fix", "status": "excluded", "excluded_at_revision": 2}
        self.plan_file.write_text(json.dumps(plan(self.url, 2, [excluded])))
        with discovery_run.Run.start(self.plan_file, "abcde", "verify") as run:
            self.assertEqual(run.results["excluded"], [{"step_ids": ["after"], "bug_tasks": ["b0b01"]}])
            self.assertEqual(discovery_run.report(run.dir)["excluded"], ["after"])
            run.finish()
        elsewhere = self.root / "fixture.plan.json"
        shutil.copy(self.plan_file, elsewhere)
        with self.assertRaises(ValueError):
            discovery_run.Run.start(elsewhere, "abcde", "explore")
        with self.assertRaises(ValueError):
            discovery_run.Run.start(self.plan_file, "ABCDE", "explore")

    def test_run_ids_are_never_reused(self):
        fixed = time.gmtime(1_800_000_000)
        with mock.patch.object(discovery_run.time, "gmtime", return_value=fixed):
            first = discovery_run.Run.start(self.plan_file, "abcde", "explore")
            with self.assertRaises(FileExistsError):
                discovery_run.Run.start(self.plan_file, "abcde99", "explore")
            first.finish()
        self.assertEqual(len(list((self.root / "data/mini-runtime/discovery/runs").iterdir())), 1)

    def test_redaction(self):
        text = textwrap.dedent("""\
            GET http://127.0.0.1:4000/?token=SECRETVIEWER&x=1 and https://a.example/v?key=AIzaKEY&sig=S1G#access_token=AT
            Authorization: Bearer abcdefghijklmnop
            Cookie: SID=cookie-value; HSID=x
            {"set-cookie": "a=b; Secure", "X-Viewer-Token": "tok"} ["Cookie", "c=d"]
            console.error: "failed with token=t0k3n here"
            plain text stays""")
        out = discovery_run.redact(text)
        for secret in ("SECRETVIEWER", "AIzaKEY", "S1G", "=AT", "abcdefghijklmnop", "cookie-value", "a=b", "\"tok\"", "c=d", "t0k3n"):
            self.assertNotIn(secret, out)
        self.assertIn("x=1", out)
        self.assertIn("plain text stays", out)
        self.assertIn("token=[redacted]", out)

    def test_profiles_are_dedicated(self):
        with discovery_run.Run.start(self.plan_file, "abcde", "explore") as first:
            fresh = discovery_run.Profiles(first)
            self.assertEqual(fresh.dir.name, first.dir.name)
            named = discovery_run.Profiles(first, "fixture-account")
            with self.assertRaises(RuntimeError):
                discovery_run.Profiles(first, "fixture-account")  # locked by this run
            named.close()
            again = discovery_run.Profiles(first, "fixture-account")  # kept between runs
            self.assertEqual(again.reference, named.reference)
            again.close()
            fresh.close()
            first.finish()
        for owner in (self.root / "data/mini-runtime/profiles/default", Path.home() / ".mozilla/firefox/x.default",
                      self.root / "elsewhere"):
            with self.subTest(owner=owner), self.assertRaises(ValueError):
                discovery_run.check_profile(owner)


class Viewer(Fixture):
    def setUp(self):
        super().setUp()
        self.require_binaries()

    def require_binaries(self):
        for binary in (ROOT / "mrt-js/target/release/realsite", ROOT / "mrt-api/target/release/mrt-api"):
            if not os.access(binary, os.X_OK):
                self.skipTest(f"build first: {binary}")

    def test_running_binaries_and_preserved_launcher_files(self):
        with discovery_run.Run.start(self.plan_file, "abcde", "explore") as run:
            profiles = discovery_run.Profiles(run)
            session = run.viewer(self.url, profiles.viewer)
            build = json.loads((run.dir / "manifest.json").read_text())["viewer"]["build"]
            names = {b["name"]: b for b in build["binaries"]}
            self.assertEqual(set(names), {"realsite", "mrt-api"})
            for name, path in (("realsite", ROOT / "mrt-js/target/release/realsite"), ("mrt-api", ROOT / "mrt-api/target/release/mrt-api")):
                self.assertEqual(names[name]["path"], str(path.resolve()))
                self.assertEqual(names[name]["sha256"], discovery_run.sha256_file(path))
            self.assertEqual(build["runtime_commit"], subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True).strip())
            self.assertEqual(discovery_run.process_environ(session.proc.pid)["NO_FIREFOX"], "1")
            self.assertTrue(session.session_dir.is_relative_to(self.root / "cache/mini-runtime/sessions"))
            self.assertTrue(session.session_files()["runtime-log"])
            with run.step("viewer", "open") as s:
                s.pass_("runtime started; " + session.call("runtimes")[0]["url"])
            run.finish()
            profiles.close()
        self.assertIsNotNone(session.proc.returncode)
        shutil.rmtree(session.session_dir)  # the launcher's own copy is gone; the run keeps its files
        kept = run.dir / "viewer/launcher-0/session"
        report = json.loads((kept / "report.json").read_text())["page"]
        self.assertFalse(report["firefox_started"])
        self.assertTrue(any(kept.glob("*-api-*.log")))
        self.assertNotIn("token=", (run.dir / "events.jsonl").read_text().replace("token=[redacted]", ""))
        self.assertEqual(json.loads((run.dir / "manifest.json").read_text())["status"], "complete")
        self.conforms(run.dir)

    def child(self, body):
        """A process that starts a run with a viewer, enters a step and waits to be stopped"""
        script = self.root / "child.py"
        script.write_text(textwrap.dedent(f"""\
            import sys, time
            sys.path.insert(0, {str(HERE)!r})
            import discovery_run
            with discovery_run.Run.start({str(self.plan_file)!r}, "abcde", "explore") as run:
                profiles = discovery_run.Profiles(run)
                session = run.viewer({self.url!r}, profiles.viewer)
                with run.step("viewer", "open") as s:
                    s.pass_("shown")
                with run.step("viewer", "add") as s:
                    print(run.dir, session.proc.pid, session.session_dir, flush=True)
                    time.sleep(120)
            """))
        proc = subprocess.Popen([sys.executable, str(script)], stdout=subprocess.PIPE, text=True)
        self.addCleanup(proc.stdout.close)
        self.addCleanup(lambda: proc.poll() is None and proc.kill())
        run_dir, pid, session_dir = proc.stdout.readline().split()
        return proc, Path(run_dir), int(pid), Path(session_dir)

    def test_terminated_run_is_interrupted(self):
        proc, run_dir, pid, session_dir = self.child("")
        proc.send_signal(signal.SIGTERM)
        proc.wait(timeout=30)
        manifest = json.loads((run_dir / "manifest.json").read_text())
        self.assertEqual(manifest["status"], "interrupted")
        self.assertIn("signal 15", self.events(run_dir)[-1]["reason"])
        self.assertFalse(Path(f"/proc/{pid}").exists())
        self.assertTrue((run_dir / "viewer/launcher-0/session/report.json").is_file())
        results = json.loads((run_dir / "results.json").read_text())
        self.assertEqual([r["step_id"] for r in results["viewer"]], ["open"])  # no result claimed for "add"
        self.conforms(run_dir)

    def test_killed_run_is_recovered(self):
        proc, run_dir, pid, session_dir = self.child("")
        self.assertEqual(discovery_run.recover(run_dir), "alive")
        proc.kill()
        proc.wait()
        self.assertEqual(json.loads((run_dir / "manifest.json").read_text())["status"], "running")
        self.assertTrue(Path(f"/proc/{pid}").exists())  # the driver outlived its run
        self.assertEqual(discovery_run.recover(run_dir), "interrupted")
        time.sleep(.5)
        self.assertFalse(Path(f"/proc/{pid}").exists())
        manifest = json.loads((run_dir / "manifest.json").read_text())
        self.assertEqual(manifest["status"], "interrupted")
        abandoned = next(e for e in self.events(run_dir) if e["event"] == "steps-abandoned")
        self.assertEqual([(s["side"], s["step"]) for s in abandoned["steps"]], [("viewer", "add")])
        self.assertTrue((run_dir / "viewer/launcher-0/session").is_dir())
        self.assertIn("Interrupted during: viewer add", (run_dir / "summary.md").read_text())
        self.assertEqual(discovery_run.recover(run_dir), "interrupted")  # idempotent
        self.conforms(run_dir)


@unittest.skipUnless(os.environ.get("VIEWER_BROWSER") == "1", "set VIEWER_BROWSER=1 with the pinned Python environment to drive Firefox")
class Browser(Fixture):
    def setUp(self):
        super().setUp()
        Viewer.require_binaries(self)

    def test_reference_and_viewer_journey(self):
        firefox = os.environ["FIREFOX_BIN"]
        with discovery_run.Run.start(self.plan_file, "abcde", "explore") as run:
            profiles = discovery_run.Profiles(run)
            reference = run.reference(firefox, profiles.reference)
            try:
                with run.step("reference", "open", capture=reference) as s:
                    reference.m.navigate(self.url)
                    s.pass_(reference.m.find_element("id", "title").text)
                with run.step("reference", "add", capture=reference) as s:
                    reference.m.find_element("id", "add").click()
                    s.pass_("count " + reference.m.find_element("id", "count").text)
                reference_evidence = reference.capture("open")  # what a failed reference step would keep
                with run.step("reference", "missing", capture=reference) as s:
                    s.pass_("the reference has no #missing either; the viewer step is the failure under test")
            finally:
                reference.close()
            self.assertTrue((profiles.reference / "prefs.js").is_file())  # the dedicated profile was used and kept
            session = run.viewer(self.url, profiles.viewer)
            display = discovery_run.Browser(run, "viewer", firefox, viewer=session)
            try:
                display.open_viewer()
                with run.step("viewer", "open", capture=display) as s:
                    display.m.switch_to_frame(display.m.find_element("id", "page"))
                    s.pass_(display.m.find_element("id", "title").text)
                with run.step("viewer", "add", capture=display) as s:
                    display.m.find_element("id", "add").click()
                    deadline = time.monotonic() + 10
                    while display.m.find_element("id", "count").text != "1" and time.monotonic() < deadline:
                        time.sleep(.1)
                    s.pass_("count " + display.m.find_element("id", "count").text)
                with run.step("viewer", "missing", capture=display) as s:
                    found = display.m.execute_script('return !!document.getElementById("missing")')
                    s.fail("no #missing element in the viewer", "expected #missing in the mirrored page, found none")
                with run.step("viewer", "after", capture=display) as s:
                    s.blocked("needs #missing", ["missing"])
            finally:
                display.close()
            run.finish()
            profiles.close()
        self.assertFalse(found)
        reference_console = next(e for e in reference_evidence if e.get("path", "").endswith("firefox-output.log"))
        self.assertIn("fixture error token=abc123", Path(reference_console["path"]).read_text())
        self.assertIn("fixture error token=[redacted]", reference_console["excerpt"])
        self.assertIn("plain-Firefox reference runs no mini-runtime",
                      next(e for e in reference_evidence if e["kind"] == "runtime-log")["unavailable_reason"])
        shutil.rmtree(session.session_dir)
        results = json.loads((run.dir / "results.json").read_text())
        self.assertEqual([(r["step_id"], r["result"]) for r in results["reference"]], [("open", "pass"), ("add", "pass"), ("missing", "pass")])
        self.assertEqual([(r["step_id"], r["result"]) for r in results["viewer"]],
                         [("open", "pass"), ("add", "pass"), ("missing", "fail"), ("after", "blocked")])
        evidence = results["viewer"][2]["evidence"]
        files = {e["kind"]: e for e in evidence if "path" in e}
        self.assertTrue(set(discovery_run.FAILURE_KINDS) <= set(files), [e for e in evidence if "unavailable_reason" in e])
        for e in evidence:
            if "path" in e:
                self.assertEqual(discovery_run.sha256_file(e["path"]), e["sha256"], e["path"])
        self.assertEqual(Path(files["screenshot"]["path"]).read_bytes()[:8], b"\x89PNG\r\n\x1a\n")
        console = [e for e in evidence if e["kind"] == "console"]
        self.assertTrue(any("browser-console" in e.get("path", "") for e in console))
        self.assertTrue(any("page console only when the session stops" in e.get("unavailable_reason", "") for e in console))
        # the page's scripts run in the runtime, not in the viewer's Firefox: its console error is the runtime's
        kept = list((run.dir / "viewer/launcher-0/session/console").glob("*.json"))
        self.assertIn("fixture error", kept[0].read_text())
        self.assertIn("Fixture page", Path(next(e["path"] for e in evidence if e["path"].endswith("viewer-frame.html"))).read_text())
        self.assertTrue(any(e["kind"] == "driver-log" for e in evidence))
        url = json.loads(Path(files["url"]["path"]).read_text())
        self.assertEqual(url["runtimes"][0]["url"], self.url)
        self.assertNotIn("SECRET", files["url"]["excerpt"])
        self.assertIn("token=[redacted]", files["url"]["excerpt"])
        manifest = json.loads((run.dir / "manifest.json").read_text())
        self.assertTrue(manifest["firefox"]["reference_used"])
        self.assertEqual(manifest["firefox"]["profile_id"], run.dir.name)
        self.assertEqual(manifest["status"], "complete")
        self.assertEqual(discovery_run.report(run.dir)["bug_links_pending"], ["missing"])
        # the bug task is filed after the run, with its evidence, then linked
        subprocess.run([sys.executable, str(HERE / "discovery_run.py"), "link-bug", str(run.dir), "missing", "f00d1"], check=True)
        self.assertEqual(discovery_run.report(run.dir)["steps"][2], {"step_id": "missing", "result": "fail", "bug_task": "f00d1"})
        self.conforms(run.dir)


if __name__ == "__main__":
    unittest.main()
