"""Recorded-data opt-in checks, using isolated scripts and synthetic inputs only."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
SCRIPTS = {
    'compare_height.py': [], 'compare_memory.py': [], 'compare_js.py': [],
    'compare_hibernate.py': [], 'compare_boxes.py': ['synthetic'],
    'compare_frames.py': [], 'compare_workers.py': [], 'compare_console.py': [],
    'compare_realsites.py': [], 'textrun_profile.py': [], 'oracle/cmp.py': ['synthetic'],
}


class RecordedTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='mrt-recorded-test-policy-')
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        for name in [*SCRIPTS, 'recorded_tests.py']:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(HERE / name, path)

    def run_script(self, name, setting=None):
        env = dict(os.environ, PYTHONDONTWRITEBYTECODE='1')
        env.pop('MRT_RECORDED_TESTS', None)
        env.pop('REPORTS_DIR', None)
        if setting is not None:
            env['MRT_RECORDED_TESTS'] = setting
        return subprocess.run([sys.executable, '-S', str(self.root / name), *SCRIPTS[name]],
                              env=env, cwd=self.root, capture_output=True, text=True, timeout=10)

    def test_default_skips_without_recordings(self):
        for name in SCRIPTS:
            with self.subTest(script=name):
                result = self.run_script(name)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn('SKIP', result.stdout)
                self.assertIn('MRT_RECORDED_TESTS=1', result.stdout)
                self.assertEqual(result.stderr, '')
        self.assertFalse(list(self.root.glob('mrt_*.jsonl')))

    def test_existing_data_does_not_enable_checks(self):
        # Invalid inputs would fail immediately if any consumer read them.
        for name in ('firefox_height.json', 'firefox_static.json', 'firefox_js.json',
                     'firefox_hibernate.json', 'mini_realsites_replay.json',
                     'firefox_realsites_replay_nouse.json', 'firefox_realsites_0a_xvfb_run1.json',
                     'firefox_boxes/synthetic.json', 'oracle/oout/synthetic.json',
                     'realsites-console/synthetic-mini.json', 'realsites-console/synthetic-firefox.json',
                     'realsites-reports-nouse/synthetic-live.json.gz'):
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('not recorded JSON or gzip')
        for name in SCRIPTS:
            with self.subTest(script=name):
                result = self.run_script(name, '0')
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn('SKIP', result.stdout)

    def test_enabled_missing_inputs_fail_with_recording_guidance(self):
        for name in SCRIPTS:
            with self.subTest(script=name):
                result = self.run_script(name, '1')
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertIn('missing recorded input', result.stderr)
                self.assertIn('Record first:', result.stderr)
                self.assertNotIn('Traceback', result.stderr)
        self.assertFalse(list(self.root.glob('mrt_*.jsonl')))

    def test_enabled_console_uses_synthetic_recordings(self):
        path = self.root / 'realsites-console'
        path.mkdir()
        (path / 'synthetic-mini.json').write_text('[]')
        (path / 'synthetic-firefox.json').write_text('{"api": [], "errors": []}')
        result = self.run_script('compare_console.py', '1')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('== synthetic:', result.stdout)
        self.assertNotIn('SKIP', result.stdout)


if __name__ == '__main__':
    unittest.main()
