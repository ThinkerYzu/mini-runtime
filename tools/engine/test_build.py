#!/usr/bin/env python3
"""Bootstrap source lifecycle tests with disposable local Git repositories, no Mozilla downloads."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('mrt_build', ROOT / 'tools/build.py')
build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)


class Source(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        self.upstream = self.base / 'upstream'
        self.upstream.mkdir()
        self.git('init', '--quiet')
        self.git('config', 'user.name', 'Synthetic Fixture')
        self.git('config', 'user.email', 'fixture@example.test')
        for name in ('mach', 'js/moz.configure', 'js/public/HeapAPI.h', 'build/moz.configure/init.configure'):
            p = self.upstream / name
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text('synthetic source fixture\n')
        self.git('add', '.')
        self.git('commit', '--quiet', '-m', 'fixture')
        self.pin = {**build.PIN, 'repository': self.upstream.as_uri(), 'revision': self.git('rev-parse', 'HEAD')}
        self.root = self.base / 'runtime'
        self.root.mkdir()
        self.source = self.root / 'firefox-source'
        root_patch = patch.object(build, 'ROOT', self.root)
        root_patch.start()
        self.addCleanup(root_patch.stop)

    def git(self, *args):
        return subprocess.check_output(['git', '-C', str(self.upstream), *args], text=True).strip()

    def test_fetch_and_reuse_without_contacting_remote(self):
        source = build.acquire(pin=self.pin)
        self.assertEqual(source, self.source)
        self.assertEqual(build.git(source, 'rev-parse', 'HEAD'), self.pin['revision'])
        self.assertEqual(build.git(source, 'rev-parse', '--is-shallow-repository'), 'true')
        self.upstream.rename(self.base / 'offline')
        self.assertEqual(build.acquire(pin=self.pin), source)

    def test_existing_default_directory_is_reused(self):
        self.upstream.rename(self.source)
        with patch.object(build, 'run', side_effect=AssertionError('unexpected fetch')):
            self.assertEqual(build.acquire(pin=self.pin), self.source)

    def test_default_symlink_resolves_to_existing_checkout(self):
        self.source.symlink_to(self.upstream, target_is_directory=True)
        with patch.object(build, 'run', side_effect=AssertionError('unexpected fetch')):
            self.assertEqual(build.acquire(pin=self.pin), self.upstream)
        self.assertTrue(self.source.is_symlink())

    def test_broken_default_symlink_is_preserved(self):
        target = self.base / 'missing'
        self.source.symlink_to(target, target_is_directory=True)
        with patch.object(build, 'run', side_effect=AssertionError('unexpected fetch')):
            with self.assertRaises(FileNotFoundError):
                build.acquire(pin=self.pin)
        self.assertEqual(self.source.readlink(), target)
        self.assertFalse(target.exists())

    def test_default_symlink_rejects_incompatible_source(self):
        self.source.symlink_to(self.upstream, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'refusing to switch'):
            build.acquire(pin={**self.pin, 'revision': '0' * 40})
        (self.upstream / 'mach').write_text('user changes')
        with self.assertRaisesRegex(ValueError, 'modified checkout'):
            build.acquire(pin=self.pin)
        self.assertEqual(self.git('rev-parse', 'HEAD'), self.pin['revision'])
        self.assertEqual((self.upstream / 'mach').read_text(), 'user changes')
        self.assertTrue(self.source.is_symlink())

    def test_supplied_checkout_is_not_switched(self):
        with self.assertRaisesRegex(ValueError, 'refusing to switch'):
            build.acquire(self.upstream, {**self.pin, 'revision': '0' * 40})
        self.assertEqual(self.git('rev-parse', 'HEAD'), self.pin['revision'])
        self.assertFalse(self.source.exists())

    def test_dirty_checkout_is_preserved(self):
        path = self.upstream / 'mach'
        path.write_text('user changes')
        with self.assertRaisesRegex(ValueError, 'modified checkout'):
            build.acquire(self.upstream, self.pin)
        self.assertEqual(path.read_text(), 'user changes')

    def test_partial_fetch_is_not_installed(self):
        pin = {**self.pin, 'repository': (self.base / 'missing').as_uri()}
        with self.assertRaises(subprocess.CalledProcessError):
            build.acquire(pin=pin)
        self.assertFalse(self.source.exists())
        self.assertFalse(list(self.root.glob('.source-*')))

    def test_incomplete_source(self):
        (self.upstream / 'mach').unlink()
        self.git('add', '-u')
        self.git('commit', '--quiet', '-m', 'incomplete')
        pin = {**self.pin, 'revision': self.git('rev-parse', 'HEAD')}
        with self.assertRaisesRegex(ValueError, 'incomplete source'):
            build.validate_source(self.upstream, pin)

    def test_objdir_checks_engine_version(self):
        obj = self.base / 'obj'
        names = ('dist/include/jsapi.h', 'dist/include/js-config.h', 'js/src/js-confdefs.h', 'dist/bin/' + self.pin['soname'])
        for name in names:
            p = obj / name
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text('fixture')
        with self.assertRaisesRegex(ValueError, 'SpiderMonkey 154'):
            build.engine_outputs(obj)
        (obj / 'dist/include/js-config.h').write_text('#define MOZJS_MAJOR_VERSION 154\n')
        self.assertEqual(build.engine_outputs(obj), obj)
        (obj / 'dist/include/jsapi.h').unlink()
        with self.assertRaisesRegex(ValueError, 'incomplete SpiderMonkey'):
            build.engine_outputs(obj)

    def test_sdk_mode_does_not_acquire_source(self):
        sdk = self.base / 'sdk'
        sdk.mkdir()
        with patch.object(build, 'acquire', side_effect=AssertionError('unexpected source fetch')):
            with patch.object(build, 'build_runtime') as runtime:
                with patch('sys.argv', ['build.py', '--sdk', str(sdk)]):
                    build.main()
                runtime.assert_called_once_with(sdk=sdk)


if __name__ == '__main__':
    unittest.main()
