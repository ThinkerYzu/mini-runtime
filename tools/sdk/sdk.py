#!/usr/bin/env python3
"""Produce, validate and safely install the pinned Linux engine SDK (stdlib only)."""
import argparse
import ast
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[2]
LOCK = ROOT / 'tools/sdk/sdk.lock.json'
TARGET = 'x86_64-unknown-linux-gnu'
ENGINE_REV = 'a8d334e3bde6e6572e3d162db5202def666e6206'
SONAME = 'libmozjs-154a1.so'
IDENTITY = 'mrt-mozjs-154a1-linux-x86_64-v1'


def digest(path):
    h = hashlib.sha256()
    with open(path, 'rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def write_json(path, data):
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + '\n')


def run(*args):
    return subprocess.check_output(args, text=True).strip()


def safe_name(name):
    p = PurePosixPath(name)
    if not name or p.is_absolute() or '..' in p.parts or str(p) != name or '\\' in name:
        raise ValueError(f'unsafe SDK path: {name!r}')
    return p


def inventory(root):
    result = {}
    for p in sorted(root.rglob('*')):
        if p.is_symlink():
            # v1 uses regular files only, including the library's SONAME file.
            raise ValueError(f'SDK symlinks are forbidden: {p.relative_to(root)}')
        if p.is_file() and p != root / 'sdk.json':
            result[p.relative_to(root).as_posix()] = digest(p)
        elif not p.is_dir() and not p.is_file():
            raise ValueError(f'non-regular SDK file: {p}')
    return result


def version_tuple(value):
    return tuple(int(x) for x in value.split('.'))


def validate(root, target=TARGET, lock_path=LOCK):
    root = Path(root).resolve(strict=True)
    if (root / 'sdk.json').is_symlink():
        raise ValueError('SDK manifest must be a regular file')
    lock = json.loads(Path(lock_path).read_text())
    if digest(root / 'sdk.json') != lock['manifest_sha256']:
        raise ValueError('SDK manifest checksum mismatch; use the SDK pinned in tools/sdk/sdk.lock.json')
    meta = json.loads((root / 'sdk.json').read_text())
    for key in ('schema', 'id', 'target', 'engine_revision', 'soname'):
        if meta[key] != lock[key]:
            raise ValueError(f'incompatible SDK {key}: {meta[key]!r}')
    if meta['schema'] != 1 or meta['id'] != IDENTITY or meta['engine_revision'] != ENGINE_REV or meta['soname'] != SONAME:
        raise ValueError('unsupported SDK identity/schema/engine')
    if target != TARGET or meta['target'] != target:
        raise ValueError(f'unsupported SDK target {target}; v1 requires {TARGET}')
    if platform.system() != 'Linux' or platform.machine() != 'x86_64':
        raise ValueError('SDK v1 supports native Linux x86_64 builds only')
    libc, version = platform.libc_ver()
    if libc != 'glibc' or version_tuple(version) < version_tuple(meta['abi']['glibc']):
        raise ValueError(f"SDK requires glibc >= {meta['abi']['glibc']}")
    for name in meta['files']:
        safe_name(name)
    actual = inventory(root)
    if actual != meta['files']:
        missing = sorted(set(meta['files']) - set(actual))
        extra = sorted(set(actual) - set(meta['files']))
        changed = sorted(k for k in set(actual) & set(meta['files']) if actual[k] != meta['files'][k])
        raise ValueError(f'SDK integrity failure: missing={missing[:4]}, extra={extra[:4]}, changed={changed[:4]}')
    for required in ('include/jsapi.h', 'include/js-config.h', 'include/js-confdefs.h',
                     f'lib/{SONAME}', 'share/licenses/Firefox-LICENSE', 'share/licenses/license.html'):
        if required not in actual:
            raise ValueError(f'incomplete SDK: missing {required}')
    return meta


def elf_info(lib):
    if 'Advanced Micro Devices X86-64' not in run('readelf', '-h', str(lib)):
        raise ValueError('SDK v1 requires an x86_64 ELF engine')
    dynamic = run('readelf', '-d', str(lib))
    sonames = re.findall(r'\(SONAME\).*?\[(.*?)\]', dynamic)
    if sonames != [SONAME] or re.search(r'\((?:RPATH|RUNPATH)\)', dynamic):
        raise ValueError('engine needs the pinned SONAME and no producer RPATH/RUNPATH')
    needed = sorted(re.findall(r'\(NEEDED\).*?\[(.*?)\]', dynamic))
    allowed = {'libdl.so.2', 'libz.so.1', 'libm.so.6', 'libstdc++.so.6', 'libgcc_s.so.1',
               'libpthread.so.0', 'libc.so.6', 'ld-linux-x86-64.so.2'}
    if set(needed) - allowed:
        raise ValueError(f'undeclared engine libraries: {set(needed) - allowed}')
    versions = run('readelf', '--version-info', str(lib))
    abi = {}
    for prefix in ('GLIBC', 'GLIBCXX', 'CXXABI'):
        found = re.findall(r'\b' + prefix + r'_([0-9]+(?:\.[0-9]+)+)\b', versions)
        abi[{'GLIBC': 'glibc', 'GLIBCXX': 'glibcxx', 'CXXABI': 'cxxabi'}[prefix]] = max(found, key=version_tuple)
    return needed, abi


def produce(args):
    source, obj = args.source.resolve(), args.obj.resolve()
    output = args.output.absolute()
    if output.exists():
        raise ValueError('SDK output already exists; choose a new directory')
    # Engine-bearing source must match the public revision. Gecko-only prototype patches do not
    # affect this standalone JS build. Never publish a private commit as engine provenance.
    if run('git', '-C', str(source), 'diff', ENGINE_REV, '--', '.', ':!dom', ':!layout'):
        raise ValueError('source differs from pinned engine revision outside Gecko-only dom/layout')
    # Read a few scalar configure values, never execute a producer's config.status.
    values = {}
    for node in ast.walk(ast.parse((obj / 'config.status').read_text())):
        if isinstance(node, ast.Dict):
            for key, value in zip(node.keys, node.values):
                if isinstance(key, ast.Constant) and isinstance(value, ast.Constant):
                    values[key.value] = value.value
                elif isinstance(key, ast.Constant) and key.value == 'CC_TYPE' and isinstance(value, ast.Call):
                    values[key.value] = ast.literal_eval(value.args[0])
    if values.get('JS_SHARED_LIBRARY') != '1' or values.get('MOZ_APP_VERSION') != '154.0a1':
        raise ValueError('producer must be the compatible shared SpiderMonkey 154.0a1 build')
    configuration = json.loads((obj / '.mozconfig.json').read_text())['mozconfig']['configure_args']
    expected = ['--enable-project=js', '--enable-optimize', '--disable-debug', '--disable-jemalloc']
    if configuration != expected:
        raise ValueError(f'unsupported engine build configuration: expected {expected}')
    needed, abi = elf_info(obj / 'dist/bin' / SONAME)
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.sdk-', dir=output.parent) as tmp:
        stage = Path(tmp) / 'sdk'
        # Exported headers are symlinks into source and object trees. Dereference all of them.
        shutil.copytree(obj / 'dist/include', stage / 'include', symlinks=False)
        shutil.copyfile(obj / 'js/src/js-confdefs.h', stage / 'include/js-confdefs.h')
        (stage / 'lib').mkdir()
        shutil.copyfile(obj / 'dist/bin' / SONAME, stage / 'lib' / SONAME)
        licenses = stage / 'share/licenses'
        licenses.mkdir(parents=True)
        shutil.copyfile(source / 'LICENSE', licenses / 'Firefox-LICENSE')
        shutil.copyfile(source / 'toolkit/content/license.html', licenses / 'license.html')
        write_json(licenses / 'provenance.json', {
            'source': f'https://github.com/mozilla-firefox/firefox/tree/{ENGINE_REV}',
            'engine_patches': [], 'license': 'MPL-2.0 and bundled third-party licenses; see license.html',
            'source_offer': 'Obtain the exact corresponding source from the revision URL. Retain notices when redistributing.',
        })
        meta = dict(schema=1, id=IDENTITY, target=TARGET, engine_revision=ENGINE_REV,
                    engine_version='154.0a1', engine_patches=[], soname=SONAME, abi=abi,
                    system_libraries=needed,
                    compiler={'family': values['CC_TYPE'], 'version': values['CC_VERSION'], 'cxx_abi': 'GNU libstdc++'},
                    configuration=configuration,
                    files=inventory(stage))
        write_json(stage / 'sdk.json', meta)
        # Pin updates are explicit reviewable outputs, never silently change an existing lock.
        lock = {k: meta[k] for k in ('schema', 'id', 'target', 'engine_revision', 'soname')}
        lock['manifest_sha256'] = digest(stage / 'sdk.json')
        if args.lock_output:
            if args.lock_output.exists():
                raise ValueError('lock output already exists; compare/review rather than overwrite')
            write_json(args.lock_output, lock)
        else:
            validate(stage)
        stage.rename(output)
    print(f'Produced SDK: {output}')


def pack(args):
    if args.archive.resolve().is_relative_to(args.sdk.resolve()):
        raise ValueError('archive destination must be outside the SDK')
    validate(args.sdk)
    if args.archive.exists():
        raise ValueError('archive already exists')
    # Uncompressed tar has deterministic metadata and needs no optional compression library.
    with tarfile.open(args.archive, 'w') as archive:
        for p in sorted(args.sdk.rglob('*')):
            name = p.relative_to(args.sdk).as_posix()
            info = archive.gettarinfo(str(p), arcname=name)
            info.uid = info.gid = info.mtime = 0
            info.uname = info.gname = ''
            info.mode = 0o755 if p.is_dir() else 0o644
            if p.is_file():
                with p.open('rb') as stream:
                    archive.addfile(info, stream)
            else:
                archive.addfile(info)
    print(digest(args.archive))


def install(args):
    if digest(args.archive) != args.sha256:
        raise ValueError('SDK archive checksum mismatch')
    if args.output.exists():
        raise ValueError('SDK destination already exists')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.sdk-install-', dir=args.output.parent) as tmp:
        stage = Path(tmp) / 'sdk'
        stage.mkdir()
        with tarfile.open(args.archive) as archive:
            names = set()
            for member in archive:
                safe_name(member.name)
                if member.name in names or not (member.isfile() or member.isdir()):
                    raise ValueError(f'duplicate/link/special archive entry: {member.name}')
                names.add(member.name)
                dest = stage / member.name
                if member.isdir():
                    dest.mkdir(parents=True, exist_ok=True)
                else:
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    with archive.extractfile(member) as stream, dest.open('xb') as out:
                        shutil.copyfileobj(stream, out)
        validate(stage)
        stage.rename(args.output)
    print(f'Installed SDK: {args.output}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    p = sub.add_parser('produce')
    for flag in ('source', 'obj', 'output'):
        p.add_argument('--' + flag, type=Path, required=True)
    p.add_argument('--lock-output', type=Path, help='candidate lock for explicit maintainer review')
    p.set_defaults(action=produce)
    p = sub.add_parser('validate')
    p.add_argument('sdk', type=Path)
    p.add_argument('--target', default=TARGET)
    p.add_argument('--build-fields', action='store_true')
    def check(args):
        meta = validate(args.sdk, args.target)
        if args.build_fields:
            print(args.sdk.resolve() / 'include')
            print(args.sdk.resolve() / 'lib')
            print(args.sdk.resolve() / 'include/js-confdefs.h')
            print(meta['soname'][3:-3])
        else:
            print('SDK validated: ' + meta['id'])
    p.set_defaults(action=check)
    p = sub.add_parser('pack')
    p.add_argument('sdk', type=Path)
    p.add_argument('archive', type=Path)
    p.set_defaults(action=pack)
    p = sub.add_parser('install')
    p.add_argument('archive', type=Path)
    p.add_argument('--sha256', required=True)
    p.add_argument('--output', type=Path, required=True)
    p.set_defaults(action=install)
    args = parser.parse_args()
    try:
        args.action(args)
    except (ValueError, OSError, KeyError, TypeError, subprocess.CalledProcessError, tarfile.TarError) as error:
        parser.exit(1, f'SDK error: {error}\nSee docs/build.md. No Firefox-tree fallback is performed.\n')


if __name__ == '__main__':
    main()
