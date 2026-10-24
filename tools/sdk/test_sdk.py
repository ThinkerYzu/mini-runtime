#!/usr/bin/env python3
"""SDK integrity, identity and extraction tests with synthetic, non-engine fixtures."""
import argparse
from functools import partial
import io
from pathlib import Path
import tempfile
import tarfile
import unittest
from unittest.mock import patch

import sdk


class SDK(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        self.root = self.base / 'sdk'
        self.root.mkdir()
        for name in ('include/jsapi.h', 'include/js-config.h', 'include/js-confdefs.h',
                     f'lib/{sdk.SONAME}', 'share/licenses/Firefox-LICENSE', 'share/licenses/license.html'):
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('synthetic fixture\n')
        self.meta = dict(schema=1, id=sdk.IDENTITY, target=sdk.TARGET, engine_revision=sdk.ENGINE_REV,
                         soname=sdk.SONAME, abi={'glibc': '2.18'}, files=sdk.inventory(self.root))
        self.lock = self.base / 'lock.json'
        self.pin()

    def pin(self):
        sdk.write_json(self.root / 'sdk.json', self.meta)
        lock = {k: self.meta[k] for k in ('schema', 'id', 'target', 'engine_revision', 'soname')}
        lock['manifest_sha256'] = sdk.digest(self.root / 'sdk.json')
        sdk.write_json(self.lock, lock)

    def check(self, target=sdk.TARGET):
        return sdk.validate(self.root, target, self.lock)

    def test_valid_and_relocation(self):
        self.check()
        self.root.rename(self.base / 'moved')
        self.root = self.base / 'moved'
        self.check()

    def test_corrupt_missing_extra(self):
        path = self.root / 'include/jsapi.h'
        path.write_text('corrupt')
        with self.assertRaisesRegex(ValueError, 'integrity failure'):
            self.check()
        path.unlink()
        with self.assertRaisesRegex(ValueError, 'missing='):
            self.check()
        (self.root / 'extra').write_text('undeclared')
        with self.assertRaisesRegex(ValueError, 'extra='):
            self.check()

    def test_manifest_tamper(self):
        (self.root / 'sdk.json').write_text('{}')
        with self.assertRaisesRegex(ValueError, 'manifest checksum'):
            self.check()

    def test_target_and_engine_mismatch(self):
        with self.assertRaisesRegex(ValueError, 'unsupported SDK target'):
            self.check('aarch64-unknown-linux-gnu')
        self.meta['engine_revision'] = 'wrong'
        self.pin()
        with self.assertRaisesRegex(ValueError, 'unsupported SDK identity'):
            self.check()

    def test_symlinks_rejected(self):
        for dest in ('/etc/passwd', '../../../outside', 'js-config.h'):
            link = self.root / 'include/link'
            link.symlink_to(dest)
            with self.assertRaisesRegex(ValueError, 'symlinks are forbidden'):
                self.check()
            link.unlink()

    def test_archive_roundtrip(self):
        archive = self.base / 'sdk.tar'
        check = partial(sdk.validate, lock_path=self.lock)
        with patch.object(sdk, 'validate', check):
            sdk.pack(argparse.Namespace(sdk=self.root, archive=archive))
            dest = self.base / 'installed'
            sdk.install(argparse.Namespace(archive=archive, sha256=sdk.digest(archive), output=dest))
            check(dest)

    def test_archive_inside_sdk(self):
        with self.assertRaisesRegex(ValueError, 'outside the SDK'):
            sdk.pack(argparse.Namespace(sdk=self.root, archive=self.root / 'bad.tar'))
        self.assertFalse((self.root / 'bad.tar').exists())

    def test_host_and_libc(self):
        with patch.object(sdk.platform, 'machine', return_value='aarch64'):
            with self.assertRaisesRegex(ValueError, 'native Linux'):
                self.check()
        with patch.object(sdk.platform, 'libc_ver', return_value=('glibc', '2.17')):
            with self.assertRaisesRegex(ValueError, 'requires glibc'):
                self.check()

    def test_archive_bad_checksum(self):
        archive = self.base / 'sdk.tar'
        archive.write_bytes(b'invalid')
        with self.assertRaisesRegex(ValueError, 'archive checksum'):
            sdk.install(argparse.Namespace(archive=archive, sha256='wrong', output=self.base / 'installed'))

    def test_archive_traversal_links_duplicates(self):
        for name, kind in (('../outside', tarfile.REGTYPE), ('/outside', tarfile.REGTYPE),
                           ('link', tarfile.SYMTYPE), ('hard', tarfile.LNKTYPE),
                           ('device', tarfile.CHRTYPE), ('duplicate', tarfile.REGTYPE)):
            with self.subTest(name=name):
                archive = self.base / 'bad.tar'
                with tarfile.open(archive, 'w') as stream:
                    info = tarfile.TarInfo(name)
                    info.type = kind
                    info.linkname = '/etc/passwd'
                    stream.addfile(info, io.BytesIO())
                    if name == 'duplicate':
                        stream.addfile(info, io.BytesIO())
                with self.assertRaises(ValueError):
                    sdk.install(argparse.Namespace(archive=archive, sha256=sdk.digest(archive), output=self.base / 'installed'))
                self.assertFalse((self.base / 'installed').exists())
                self.assertFalse((self.base / 'outside').exists())


if __name__ == '__main__':
    unittest.main()
