#!/usr/bin/env python3
"""Build a fresh consumer copy in a Linux mount namespace that cannot see Firefox or private inputs.

Requires bubblewrap, cached locked registry crates, and a native Rust toolchain. Results remain in
--output for inspection/regression tests. Locked public dependencies are fetched before isolation; no network is allowed during the build.
"""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--sdk', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--toolchain', type=Path, required=True, help='Rust toolchain directory containing bin/rustc and bin/cargo')
    parser.add_argument('--registry', type=Path, default=Path.home() / '.cargo/registry')
    args = parser.parse_args()
    if args.output.exists():
        parser.error('output must not exist (a fresh target directory is required)')
    sdk = args.sdk.resolve(strict=True)
    subprocess.run([sys.executable, ROOT / 'tools/sdk/sdk.py', 'validate', sdk], check=True)
    output = args.output.absolute()
    output.mkdir(parents=True)
    source = output / 'source'
    source.mkdir()
    files = subprocess.check_output(['git', 'ls-files', '-z'], cwd=ROOT).decode().split('\0')
    for name in filter(None, files):
        if name.startswith('.cargo/'):
            raise RuntimeError('private root Cargo configuration must not be tracked')
        src, dest = ROOT / name, source / name
        if src.is_symlink():
            raise RuntimeError(f'source symlink forbidden: {name}')
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src, dest)
    cargo = output / 'cargo-home'
    cargo.mkdir()
    # Only the public registry cache is copied, never config.toml, credentials, git deps or targets.
    if args.registry.is_dir():
        subprocess.run(['cp', '-a', '--reflink=auto', str(args.registry.resolve()), str(cargo / 'registry')], check=True)
    # Resolve only the manifests' locked public inputs into this empty Cargo home. In particular,
    # mrt-api pins a public mp4parse Git revision, which is not in a registry cache.
    fetch_env = {**os.environ, 'CARGO_HOME': str(cargo),
                 'PATH': str(args.toolchain.resolve() / 'bin') + os.pathsep + '/usr/bin:/bin'}
    for crate in ('mrt-js', 'mrt-api'):
        subprocess.run([str(args.toolchain / 'bin/cargo'), 'fetch', '--locked',
                        '--manifest-path', str(source / crate / 'Cargo.toml')],
                       cwd=source, env=fetch_env, check=True)
    command = ['bwrap', '--unshare-all', '--die-with-parent', '--new-session', '--clearenv',
               '--ro-bind', '/usr', '/usr', '--ro-bind', '/bin', '/bin',
               '--ro-bind', '/lib', '/lib', '--ro-bind', '/lib64', '/lib64',
               '--ro-bind', '/etc', '/etc', '--proc', '/proc', '--dev', '/dev', '--tmpfs', '/tmp',
               '--bind', str(output), '/work', '--ro-bind', str(sdk), '/sdk',
               '--ro-bind', str(args.toolchain.resolve(strict=True)), '/toolchain',
               '--setenv', 'PATH', '/toolchain/bin:/usr/bin:/bin',
               '--setenv', 'HOME', '/tmp/consumer', '--setenv', 'CARGO_HOME', '/work/cargo-home',
               '--setenv', 'MRT_SDK_DIR', '/sdk', '--setenv', 'CXX', '/usr/bin/c++',
               '--setenv', 'CARGO_BUILD_JOBS', '4', '--chdir', '/work/source',
               '/bin/sh', '-ec',
               '(cd mrt-js && cargo build --release --locked --offline --bin realsite); '
               '(cd mrt-api && cargo build --release --locked --offline); '
               '(cd mrt-js && cargo test --release --locked --offline); '
               '(cd mrt-api && cargo test --release --locked --offline); '
               'python3 bench/test_no_firefox.py; python3 bench/test_http_viewer.py']
    subprocess.run(command, check=True)
    # Relocate the runnable application, then launch without an SDK mount or producer tree.
    app = output / 'relocated'
    (app / 'lib').mkdir(parents=True)
    shutil.copyfile(source / 'mrt-js/target/release/realsite', app / 'realsite')
    shutil.copyfile(source / 'mrt-api/target/release/mrt-api', app / 'mrt-api')
    for binary in ('realsite', 'mrt-api'):
        (app / binary).chmod(0o755)
    for lib in (source / 'mrt-js/target/release/lib').iterdir():
        shutil.copyfile(lib, app / 'lib' / lib.name)
    dynamic = subprocess.check_output(['readelf', '-d', app / 'realsite'], text=True)
    if '$ORIGIN/lib:$ORIGIN/../lib' not in dynamic or str(sdk) in dynamic or '/home/' in dynamic:
        raise RuntimeError('non-relocatable runtime loader path')
    isolation = command[:command.index('--bind')]
    isolation += ['--setenv', 'PATH', '/usr/bin:/bin', '--setenv', 'HOME', '/tmp/consumer',
                  '--ro-bind', str(app), '/app', '--ro-bind', str(source), '/source',
                  '--tmpfs', '/source/mrt-js/target', '--tmpfs', '/source/mrt-api/target',
                  '--tmpfs', '/source/bench/realsites-console',
                  '--setenv', 'REALSITE_BIN', '/app/realsite', '--setenv', 'API_BIN', '/app/mrt-api',
                  '--chdir', '/source', '/bin/sh', '-ec',
                  'ldd /app/realsite; python3 bench/test_no_firefox.py NoFirefox.test_fetch_xhr_script_and_cookie']
    subprocess.run(isolation, check=True)
    print(f'Clean SDK build and relocation passed. Consumer source: {source}')


if __name__ == '__main__':
    main()
