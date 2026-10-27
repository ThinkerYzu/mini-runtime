#!/usr/bin/env python3
"""Open an HTTP(S) site in the standalone HTTP viewer using a native mini-runtime profile."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import sys
import tempfile
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[1]


def port_number(value):
    try:
        port = int(value)
    except ValueError:
        raise argparse.ArgumentTypeError('port must be an integer') from None
    if not 0 <= port <= 65535:
        raise argparse.ArgumentTypeError('port must be between 0 and 65535')
    return port


def site_url(value):
    try:
        parsed = urlsplit(value)
        if parsed.scheme not in ('http', 'https') or not parsed.hostname or any(c.isspace() for c in value):
            raise ValueError()
        parsed.port  # Validate a supplied port before starting a session.
    except ValueError:
        raise argparse.ArgumentTypeError('site must be an absolute http:// or https:// URL') from None
    return value


def xdg_home(name, fallback):
    value = os.environ.get(name)
    path = Path(value) if value else fallback
    return path if path.is_absolute() else fallback


def main():
    data = xdg_home('XDG_DATA_HOME', Path.home() / '.local/share')
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('url', type=site_url, help='site to visit, for example https://en.wikipedia.org/wiki/Main_Page')
    parser.add_argument('--profile', type=Path, default=data / 'mini-runtime/profiles/default',
                        help='native profile directory (default: %(default)s)')
    parser.add_argument('--port', type=port_number, default=0, help='loopback viewer port; 0 chooses a free port (default)')
    parser.add_argument('--environment', type=Path, help='optional comparison environment recording instead of built-in defaults')
    args = parser.parse_args()
    for binary in (ROOT / 'mrt-js/target/release/realsite', ROOT / 'mrt-api/target/release/mrt-api'):
        if not binary.is_file() or not os.access(binary, os.X_OK):
            parser.exit(1, f'Missing executable: {binary}\nBuild first: python3 {ROOT / "tools/build.py"}\n')
    try:
        environment = args.environment.resolve(strict=True) if args.environment is not None else None
        profile = args.profile.resolve()
        profile.mkdir(mode=0o700, parents=True, exist_ok=True)
        lock_fd = os.open(profile / '.viewer.lock', os.O_CREAT | os.O_RDWR, 0o600)
        with os.fdopen(lock_fd, 'a') as lock:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                parser.exit(1, f'Profile already in use: {profile}\nChoose another with --profile DIRECTORY.\n')
            seed = profile / 'seed'
            seed.mkdir(mode=0o700, exist_ok=True)
            cache = xdg_home('XDG_CACHE_HOME', Path.home() / '.cache')
            sessions = (cache / 'mini-runtime/sessions').absolute()
            sessions.mkdir(mode=0o700, parents=True, exist_ok=True)
            session = Path(tempfile.mkdtemp(prefix='viewer-', dir=sessions))
            # Do not let a benchmark shell select another site, evaluator, browser, replay or
            # environment. Ordinary system/network settings and the public media quota opt-in remain.
            owned = ('API_', 'MRT_', 'REALSITE', 'REPLAY', 'BRIDGE_', 'MINI_', 'FIREFOX_', 'VIEWER_')
            flags = {'ONLY', 'EXTRA_SITES', 'NO_FIREFOX', 'INTERACTIVE', 'INTERACTIVE_S',
                     'INTERACTIVE_DEBUG', 'RENDER', 'HEAP_SCAN', 'WALL_LIMIT', 'INPUT_ACTIONS', 'MARIONETTE_PORT',
                     'GDOC_URL', 'DISPLAY', 'WAYLAND_DISPLAY'}
            env = {k: v for k, v in os.environ.items()
                   if (not k.startswith(owned) and k not in flags) or k in ('VIEWER_MEDIA_REPLAY_UNLIMITED', 'MRT_LOOP_TRACE')}
            env.update(VIEWER_HTTP=f'127.0.0.1:{args.port}', NO_FIREFOX='1', API_PROCESS='all',
                       ONLY='page', EXTRA_SITES=json.dumps({'page': args.url}), REALSITES_PROFILE=str(seed),
                       MRT_VIEWER_STORE=str(profile / 'store'), MRT_CONSOLE_DIR=str(session / 'console'),
                       MINI_DIR=str(session), MINI_OUT=str(session / 'report.json'))
            if environment is not None:
                env['API_ENV'] = str(environment)
            print(f'Profile: {profile}', flush=True)
            print(f'Session files: {session}', flush=True)
            # The driver owns service signals/cleanup. Retain the profile lock until it exits,
            # including if its launching terminal dies. API/runtime children close this descriptor.
            os.set_inheritable(lock.fileno(), True)
            os.execve(sys.executable, [sys.executable, '-S', str(ROOT / 'bench/mini_realsites.py')], env)
    except OSError as error:
        parser.exit(1, f'Viewer setup failed: {error}\n')


if __name__ == '__main__':
    main()
