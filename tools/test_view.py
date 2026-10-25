#!/usr/bin/env python3
"""Local launcher acceptance: no installed browser, display, recording or third-party Python packages."""
import functools
import http.server
import json
import os
from pathlib import Path
import queue
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from urllib.parse import parse_qs, urlsplit
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]


class Launcher(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='mrt-launcher-')
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.children = []
        self.env = {**os.environ, 'XDG_DATA_HOME': str(self.root / 'data'), 'XDG_CACHE_HOME': str(self.root / 'cache'),
                    # These inherited benchmark settings must not affect the launcher.
                    'API_ENV': '/nonexistent/recording.json', 'FIREFOX_BIN': '/nonexistent/firefox',
                    'INTERACTIVE': '1', 'REPLAY': 'replay', 'ONLY': 'wrong', 'MRT_EVAL_AFTER': '/nonexistent/eval.js'}
        (self.root / 'page.html').write_text('''<!doctype html><output id="state"></output><script>
const visits = Number(localStorage.getItem('visits') || '0');
const cookie = document.cookie;
const session = sessionStorage.getItem('session');
localStorage.setItem('visits', String(visits + 1));
document.cookie = 'visits=' + (visits + 1) + '; Max-Age=3600; Path=/';
sessionStorage.setItem('session', 'active');
document.getElementById('state').textContent = JSON.stringify({visits, cookie, session, ua:navigator.userAgent});
</script>''')

        class Quiet(http.server.SimpleHTTPRequestHandler):
            def log_message(self, *args):
                pass
        self.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), functools.partial(Quiet, directory=str(self.root)))
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f'http://localhost:{self.server.server_port}/page.html'
        self.addCleanup(self.shutdown)

    def shutdown(self):
        for child in self.children:
            if child['proc'].poll() is None:
                try:
                    self.call(child, 'stop', {})
                    child['proc'].wait(timeout=10)
                except Exception:
                    child['proc'].kill()
                    child['proc'].wait()
            child['proc'].stdout.close()
            child['stderr'].close()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()

    def start(self, *args):
        stderr = (self.root / f'driver-{len(self.children)}.log').open('w')
        proc = subprocess.Popen([sys.executable, '-S', str(ROOT / 'tools/view.py'), self.url, *args],
                                cwd=self.root, env=self.env, stdout=subprocess.PIPE, stderr=stderr, text=True)
        child = {'proc': proc, 'stderr': stderr}
        self.children.append(child)
        lines = queue.Queue()
        def output():
            for line in proc.stdout:
                lines.put(line.strip())
            lines.put(None)
        threading.Thread(target=output, daemon=True).start()
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            line = lines.get(timeout=max(.1, deadline-time.monotonic()))
            self.assertIsNotNone(line, 'launcher exited before serving a viewer')
            if line.startswith('Session files: '):
                child['session'] = Path(line[len('Session files: '):])
            if line.startswith('VIEWER_URL '):
                parsed = urlsplit(line.split()[1])
                child.update(origin=f'{parsed.scheme}://{parsed.netloc}', token=parse_qs(parsed.query)['token'][0])
                return child
        self.fail('launcher startup timed out')

    def call(self, child, name, data=None):
        req = Request(child['origin'] + '/api/' + name, headers={'X-Viewer-Token': child['token'], 'Content-Type': 'application/json'},
                      data=json.dumps(data).encode() if data is not None else None)
        with urlopen(req, timeout=10) as response:
            return json.load(response)

    def state(self, child):
        attached = self.call(child, 'attach', {'runtime': 'page', 'viewport': [900, 650]})
        session = attached['session']
        deadline = time.monotonic() + 10
        after = 0
        while time.monotonic() < deadline:
            data = self.call(child, f'poll?session={session}&after={after}')
            for batch in data['batches']:
                after = batch['seq']
                for op in batch['ops']:
                    if op[0] == 'c':
                        def find(spec):
                            if dict(spec.get('a', [])).get('id') == 'state':
                                return json.loads(''.join(c.get('t', '') for c in spec.get('c', [])))
                            for node in spec.get('c', []):
                                got = find(node)
                                if got is not None:
                                    return got
                        for spec in op[2]:
                            got = find(spec)
                            if got is not None:
                                return got
        self.fail('no document snapshot')

    def stop(self, child, interrupt=False, recording=None):
        if interrupt:
            child['proc'].send_signal(signal.SIGINT)
        else:
            self.call(child, 'stop', {})
        self.assertEqual(child['proc'].wait(timeout=20), 0)
        result = json.loads((child['session'] / 'report.json').read_text())['page']
        self.assertNotIn('error', result)
        self.assertEqual(result['exit'], 0)
        self.assertEqual(result['js_errors'], 0)
        self.assertFalse(result['firefox_started'])
        self.assertFalse(result['bridge_tab']['made'])
        self.assertFalse(result['unsupported_ops'])
        self.assertEqual(result['environment_file'], str(recording) if recording else None)
        self.assertTrue((child['session'] / 'console').is_dir())

    def test_default_profile_persistence_and_isolation(self):
        first = self.start()
        seen = self.state(first)
        self.assertEqual(seen['visits'], 0)
        self.assertEqual(seen['cookie'], '')
        self.assertIsNone(seen['session'])
        self.assertIn('MiniRuntime/', seen['ua'])
        self.stop(first)
        profile = self.root / 'data/mini-runtime/profiles/default'
        self.assertTrue((profile / 'store').is_dir())
        self.assertTrue(first['session'].is_relative_to(self.root / 'cache'))
        second = self.start()
        seen = self.state(second)
        self.assertEqual(seen['visits'], 1)
        self.assertIn('visits=1', seen['cookie'])
        self.assertIsNone(seen['session'])
        self.assertNotEqual(first['session'], second['session'])
        self.stop(second, interrupt=True)
        third = self.start('--profile', str(self.root / 'other'))
        self.assertEqual(self.state(third)['visits'], 0)
        self.stop(third)

    def test_profile_lock_and_release(self):
        first = self.start()
        blocked = subprocess.run([sys.executable, '-S', str(ROOT / 'tools/view.py'), self.url],
                                 cwd=self.root, env=self.env, capture_output=True, text=True, timeout=10)
        self.assertEqual(blocked.returncode, 1)
        self.assertIn('Profile already in use', blocked.stderr)
        self.stop(first)
        second = self.start()
        self.stop(second)

    def test_explicit_environment(self):
        recording = self.root / 'comparison.json'
        entry = {'navigator': {'userAgent': 'launcher-comparison', 'languages': ['en-US']},
                 'names': {k: [] for k in ('window', 'Document', 'HTMLElement', 'Element', 'Node', 'Navigator')},
                 'screen': {'width': 800, 'height': 600}}
        recording.write_text(json.dumps({'viewport': [800, 600], 'entries': {'secure=1,isolated=0': entry}}))
        child = self.start('--environment', str(recording), '--port', '0')
        self.assertEqual(self.state(child)['ua'], 'launcher-comparison')
        self.stop(child, recording=recording)

    def test_invalid_arguments_do_not_create_profiles(self):
        for args in ([], ['file:///tmp/page'], ['https://'], ['example.com'], ['http://localhost:bad/'],
                     [self.url, '--port', '-1'], [self.url, '--port', '65536']):
            with self.subTest(args=args):
                result = subprocess.run([sys.executable, '-S', str(ROOT / 'tools/view.py'), *args],
                                        cwd=self.root, env=self.env, capture_output=True, text=True, timeout=5)
                self.assertEqual(result.returncode, 2)
                self.assertFalse((self.root / 'data').exists())


if __name__ == '__main__':
    unittest.main()
