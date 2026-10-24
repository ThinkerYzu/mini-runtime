#!/usr/bin/env python3
"""Acquire pinned Mozilla source if needed, build SpiderMonkey, then build the standalone viewer.

No system packages are installed. Existing user checkouts are never switched or cleaned.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
PIN = json.loads((ROOT / 'tools/engine/source.json').read_text())


def run(args, **kwargs):
    print('+ ' + shlex.join(map(str, args)), flush=True)
    return subprocess.run(list(map(str, args)), check=True, **kwargs)


def git(source, *args):
    return subprocess.check_output(['git', '-C', str(source), *args], text=True).strip()


def validate_source(source, pin=PIN):
    if git(source, 'rev-parse', 'HEAD') != pin['revision']:
        raise ValueError(f'{source}: expected pinned revision {pin["revision"]}; refusing to switch an existing checkout')
    if git(source, 'status', '--porcelain', '--untracked-files=no'):
        raise ValueError(f'{source}: tracked source changes; refusing to build a modified checkout')
    for name in ('mach', 'js/moz.configure', 'js/public/HeapAPI.h', 'build/moz.configure/init.configure'):
        if not (source / name).is_file():
            raise ValueError(f'{source}: incomplete source checkout, missing {name}')
    return source


def acquire(supplied=None, pin=PIN):
    if supplied is not None:
        return validate_source(supplied.resolve(strict=True), pin)
    source = ROOT / 'firefox-source'
    if source.exists() or source.is_symlink():
        return validate_source(source.resolve(strict=True), pin)
    source.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.source-', dir=source.parent) as tmp:
        stage = Path(tmp) / 'source'
        run(['git', 'init', '--quiet', stage])
        run(['git', '-C', stage, 'remote', 'add', 'origin', pin['repository']])
        # GitHub supports fetching a pinned commit without downloading repository history.
        # The working tree includes Mozilla's engine build dependencies outside js/.
        # A direct shallow fetch avoids a second enormous lazy-blob request during checkout.
        # We compile only the JS project, not the browser.
        run(['git', '-C', stage, 'fetch', '--progress', '--no-tags', '--depth=1', 'origin', pin['revision']])
        run(['git', '-C', stage, 'checkout', '--progress', '--detach', 'FETCH_HEAD'])
        validate_source(stage, pin)
        stage.rename(source)
    return source


def engine_outputs(obj, pin=PIN):
    for name in ('dist/include/jsapi.h', 'dist/include/js-config.h', 'js/src/js-confdefs.h', 'dist/bin/' + pin['soname']):
        if not (obj / name).is_file():
            raise ValueError(f'incomplete SpiderMonkey object directory: missing {obj / name}')
    config = (obj / 'dist/include/js-config.h').read_text()
    if '#define MOZJS_MAJOR_VERSION 154' not in config or '#define JS_DEBUG 1' in config:
        raise ValueError('expected optimized, non-debug SpiderMonkey 154 headers')
    return obj


def cbindgen(cache):
    version = PIN['build_tools']['cbindgen']
    minimum = tuple(map(int, version.split('.')))
    def compatible(path):
        output = subprocess.check_output([str(path), '--version'], text=True)
        match = re.search(r'cbindgen (\d+)\.(\d+)\.(\d+)', output)
        return match is not None and tuple(map(int, match.groups())) >= minimum
    if os.environ.get('CBINDGEN'):
        path = os.environ['CBINDGEN']
        if not compatible(path):
            raise ValueError(f'CBINDGEN must be >= {version}')
        return path
    path = shutil.which('cbindgen')
    if path and compatible(path):
        return path
    prefix = cache / 'tools' / f'cbindgen-{version}'
    binary = prefix / 'bin/cbindgen'
    if not binary.exists():
        run(['cargo', 'install', '--locked', '--version', version, '--root', prefix, 'cbindgen'])
    if not compatible(binary):
        raise ValueError(f'{binary}: incompatible cached tool; choose another --cache directory')
    return str(binary)


def build_engine(source, cache, jobs):
    work = cache / PIN['revision']
    obj = work / 'obj'
    work.mkdir(parents=True, exist_ok=True)
    config = work / 'mozconfig'
    settings = [
        'ac_add_options --enable-project=js',
        'ac_add_options --enable-optimize',
        'ac_add_options --disable-debug',
        'ac_add_options --disable-jemalloc',
        'ac_add_options --disable-bootstrap',
        f'mk_add_options MOZ_OBJDIR={obj}',
    ]
    content = '\n'.join(settings) + '\n'
    if config.exists() and config.read_text() != content:
        raise ValueError(f'{config}: existing build configuration differs; choose another --cache directory')
    config.write_text(content)
    env = {**os.environ, 'MOZCONFIG': str(config), 'MOZBUILD_STATE_PATH': str(work / 'mozbuild-state')}
    # Use the caller's declared compiler, not a private Mozilla toolchain installation.
    env['CC'] = os.environ.get('CC', 'clang')
    env['CXX'] = os.environ.get('CXX', 'clang++')
    env.pop('MRT_SDK_DIR', None)
    env['CBINDGEN'] = cbindgen(cache)
    run([sys.executable, source / 'mach', 'build', f'-j{jobs}'], cwd=source, env=env)
    return engine_outputs(obj)


def build_runtime(obj=None, sdk=None):
    env = dict(os.environ)
    for name in ('MRT_SDK_DIR', 'MRT_ENGINE_SOURCE', 'MRT_JS_OBJDIR', 'JS'):
        env.pop(name, None)
    if sdk:
        run([sys.executable, ROOT / 'tools/sdk/sdk.py', 'validate', sdk])
        env['MRT_SDK_DIR'] = str(sdk)
    else:
        env.update(MRT_ENGINE_SOURCE='1', MRT_JS_OBJDIR=str(obj), JS=str(obj))
    env['CXX'] = os.environ.get('CXX', 'c++')
    run(['cargo', 'build', '--release', '--locked', '--bin', 'realsite'], cwd=ROOT / 'mrt-js', env=env)
    run(['cargo', 'build', '--release', '--locked'], cwd=ROOT / 'mrt-api', env=env)
    print('Built mrt-js/target/release/realsite and mrt-api/target/release/mrt-api.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    default_cache = Path(os.environ.get('XDG_CACHE_HOME', Path.home() / '.cache')) / 'mini-runtime/spidermonkey'
    parser.add_argument('--cache', type=Path, default=default_cache, help='engine object and build-tool storage')
    choice = parser.add_mutually_exclusive_group()
    choice.add_argument('--source', type=Path, help='reuse an existing clean checkout at the pinned revision')
    choice.add_argument('--objdir', type=Path, help='reuse an existing compatible built SpiderMonkey')
    choice.add_argument('--sdk', type=Path, help='use the optional pinned binary SDK instead of building the engine')
    parser.add_argument('--fetch-only', action='store_true', help='acquire/check source without compiling')
    parser.add_argument('--jobs', type=int, default=min(os.cpu_count() or 2, 8))
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error('--jobs must be positive')
    if args.fetch_only and (args.objdir or args.sdk):
        parser.error('--fetch-only needs source mode')
    if platform.system() != 'Linux' or platform.machine() != 'x86_64':
        parser.error('initial bootstrap supports native Linux x86_64')
    try:
        if args.sdk:
            build_runtime(sdk=args.sdk.resolve(strict=True))
        elif args.objdir:
            build_runtime(obj=engine_outputs(args.objdir.resolve(strict=True)))
        else:
            source = acquire(args.source)
            print(f'Pinned Mozilla source: {source}', flush=True)
            if not args.fetch_only:
                build_runtime(obj=build_engine(source, args.cache.absolute(), args.jobs))
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f'Build setup failed: {error}\nSee docs/build.md for native prerequisites. Existing checkouts are never switched or cleaned.\n')


if __name__ == '__main__':
    main()
