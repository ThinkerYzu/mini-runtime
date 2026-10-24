#!/usr/bin/env python3
"""Record API_ENV locally with an installed Firefox and ordinary Python. No runtime build needed."""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import threading

import firefox_exit

HERE = Path(__file__).resolve().parent


class ContextPage(BaseHTTPRequestHandler):
    def do_GET(self):
        body = b'<!doctype html><meta charset="utf-8"><title>Runtime environment recording</title>'
        self.send_response(200)
        self.send_header('Content-Type', 'text/html; charset=utf-8')
        self.send_header('Content-Length', str(len(body)))
        if self.path == '/isolated':
            self.send_header('Cross-Origin-Opener-Policy', 'same-origin')
            self.send_header('Cross-Origin-Embedder-Policy', 'require-corp')
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--firefox', default=os.environ.get('FIREFOX_BIN'))
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--port', type=int, default=2828, help='Marionette port, choose another for concurrent automation')
    args = parser.parse_args()
    if not args.firefox:
        parser.error('set FIREFOX_BIN or --firefox to an installed Firefox executable')
    if args.output.exists():
        parser.error('output exists; choose a new file to avoid replacing a local recording')
    try:
        from marionette_driver.marionette import Marionette
    except ImportError:
        parser.error('install tools/sdk/requirements-browser.txt in a Python virtual environment')
    server = ThreadingHTTPServer(('127.0.0.1', 0), ContextPage)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    browser = Marionette(bin=args.firefox, port=args.port, prefs={
        'marionette.allowSystemAccess': True,
        'network.dns.localDomains': 'mrt-env.test',
        'browser.aboutwelcome.enabled': False,
        'browser.shell.checkDefaultBrowser': False,
    })
    try:
        browser.start_session()
        browser.set_context('chrome')
        build = browser.execute_script('return Services.appinfo.appBuildID;')
        browser.set_context('content')
        data = {'firefox': build, 'entries': {}}
        for host, path, expected in [('mrt-env.test', '/', 'secure=0,isolated=0'),
                                     ('localhost', '/', 'secure=1,isolated=0'),
                                     ('localhost', '/isolated', 'secure=1,isolated=1')]:
            browser.navigate(f'http://{host}:{server.server_port}{path}')
            secure, isolated = browser.execute_script('return [window.isSecureContext, window.crossOriginIsolated];')
            key = f'secure={int(secure)},isolated={int(isolated)}'
            if key != expected:
                raise RuntimeError(f'context {host}{path}: expected {expected}, got {key}')
            entry = browser.execute_script((HERE / 'browser_env.js').read_text(), sandbox='bridge')
            data['viewport'] = [entry.pop('innerWidth'), entry.pop('innerHeight')]
            data['entries'][key] = entry
        args.output.parent.mkdir(parents=True, exist_ok=True)
        with args.output.open('x') as output:
            json.dump(data, output, indent=1, sort_keys=True)
            output.write('\n')
        print(f'Recorded three contexts in {args.output.resolve()}')
        print('Set API_ENV to this path for subsequent standalone runs.')
    finally:
        firefox_exit.close(browser)
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


if __name__ == '__main__':
    main()
