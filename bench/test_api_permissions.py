"""Native permission policy, origin isolation and restart/persistence checks."""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import time
import unittest

BIN = Path(__file__).resolve().parents[1] / 'mrt-api/target/release/mrt-api'


class Permissions(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='mrt-permissions-')
        self.root = Path(self.tmp.name)
        self.profile = self.root / 'profile'
        self.profile.mkdir()
        self.db = sqlite3.connect(self.profile / 'permissions.sqlite')
        self.db.execute('PRAGMA journal_mode=WAL')
        self.db.execute('CREATE TABLE moz_perms(origin TEXT,type TEXT,permission INT,expireType INT,expireTime INT)')
        self.db.executemany('INSERT INTO moz_perms VALUES (?,?,?,?,?)', [
            ('https://a.test', 'desktop-notification', 1, 0, 0),
            ('https://a.test', 'persistent-storage', 1, 0, 0),
            ('https://a.test', 'geo', 2, 0, 0),
            ('https://a.test', 'midi', 1, 1, int(time.time()*1000)-10000),
            ('https://a.test', 'midi-sysex', 1, 2, 0),
            ('https://a.test^userContextId=1', 'camera', 1, 0, 0),
            ('https://b.test', 'desktop-notification', 2, 0, 0),
        ])
        self.db.commit()  # leave WAL open: the native seed must include it
        self.before = tuple(self.db.iterdump())
        self.start()

    def start(self):
        self.proc = subprocess.Popen([str(BIN)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, text=True,
                                     env=dict(os.environ, API_STORE=str(self.root/'store'), API_PROFILE=str(self.profile)))

    def stop(self):
        self.proc.stdin.close()
        self.assertEqual(self.proc.wait(timeout=5), 0, self.proc.stderr.read())
        self.proc.stdout.close()
        self.proc.stderr.close()

    def tearDown(self):
        self.stop()
        self.db.close()
        self.tmp.cleanup()

    def call(self, call='query', origin='https://a.test', top=None, **extra):
        site = 'https://b.test' if origin.startswith('https://b.test') else 'https://a.test'
        req = dict(op='permissions', origin=origin, site=site, top=top or site, secure=True, call=call)
        req.update(extra)
        self.proc.stdin.write(json.dumps(req)+'\n')
        self.proc.stdin.flush()
        reply = json.loads(self.proc.stdout.readline())
        self.assertNotIn('err', reply, reply)
        return reply['ok']

    def query(self, name, **extra):
        return self.call(desc=dict(name=name), **extra)['value']['state']

    def test_profile_aliases_expiry_and_no_profile_writes(self):
        self.assertEqual(self.query('notifications'), 'granted')
        self.assertEqual(self.query('push'), 'granted')
        self.assertEqual(self.query('geolocation'), 'denied')
        self.assertEqual(self.query('midi'), 'prompt')
        self.assertEqual(self.call(desc=dict(name='midi', sysex=True))['value']['state'], 'prompt')
        self.assertEqual(self.query('camera'), 'prompt')
        self.assertEqual(tuple(self.db.iterdump()), self.before)
        self.assertEqual(self.call('notification')['value'], 'granted')

    def test_origin_port_subdomain_and_partition_isolation(self):
        self.assertEqual(self.query('notifications', origin='https://b.test'), 'denied')
        for extra in [dict(origin='https://a.test:444'), dict(top='https://b.test')]:
            self.assertEqual(self.query('notifications', **extra), 'prompt')
        # query() takes the parent domain's entry (same scheme and port); Notification and the
        # requests' front end test the origin's own entry only
        self.assertEqual(self.query('notifications', origin='https://x.sub.a.test'), 'granted')
        self.assertEqual(self.call('notification', origin='https://x.sub.a.test')['value'], 'default')
        self.assertTrue(self.call('persist', origin='https://sub.a.test')['pending'])
        self.assertEqual(self.query('notifications', origin='https://sub.a.test:444'), 'prompt')
        self.assertIn('error', self.call(origin='https://viewer.invalid'))
        self.assertIn('error', self.call(origin='null'))

    def test_persistence_restart_and_one_time_seed(self):
        self.assertFalse(self.call('persisted')['value'])
        self.assertTrue(self.call('persist')['value'])
        self.assertTrue(self.call('persisted')['value'])
        self.assertFalse(self.call('persisted', origin='https://a.test:444')['value'])
        self.db.execute("UPDATE moz_perms SET permission=2 WHERE type='persistent-storage'")
        self.db.commit()
        self.stop()
        self.start()
        self.assertTrue(self.call('persisted')['value'])
        self.assertEqual(self.query('persistent-storage'), 'granted')
        self.assertTrue(self.call('persist', origin='https://a.test:444')['pending'])

    def test_defaults_errors_and_no_prompt_grants(self):
        origin='https://a.test:444'
        self.assertEqual(self.query('notifications', origin=origin), 'prompt')
        for name in ('camera', 'microphone', 'storage-access', 'loopback-network', 'local-network'):
            answer = self.call(origin=origin, desc={'name': name})['value']
            self.assertEqual(answer, {'name':name, 'state':'prompt'})
        self.assertEqual(self.query('screen-wake-lock', origin=origin), 'granted')
        self.assertEqual(self.call('requestNotification', origin=origin)['value'], 'default')
        self.assertEqual(self.query('notifications', origin=origin), 'prompt')
        self.assertEqual(self.call('geolocation')['value'], {'code':1})
        self.assertEqual(self.call('geolocation', origin=origin)['value'], {'code':2})
        self.assertEqual(self.call('notification', secure=False)['value'], 'denied')
        self.assertEqual(self.call(desc={'name':'background-sync'})['error']['name'], 'TypeError')

    def test_default_prefs_and_always_ask(self):
        self.db.executemany('INSERT INTO moz_perms VALUES (?,?,?,?,?)', [
            ('https://a.test', 'microphone', 3, 0, 0),
            ('https://sub.a.test', 'camera', 3, 0, 0),
            ('https://sub.a.test', 'geo', 2, 1, int(time.time()*1000)-10000),
        ])
        self.db.commit()
        # prefs.js, then user.js over it; local-network has no default preference
        (self.profile/'prefs.js').write_text('user_pref("permissions.default.geo", 1);\nuser_pref("permissions.default.camera", 2);\n')
        (self.profile/'user.js').write_text('user_pref("permissions.default.geo", 2);\nuser_pref("permissions.default.screen-wake-lock", 2);\n'
                                            'user_pref("permissions.default.local-network", 2);\nuser_pref("permissions.default.desktop-notification", 9);\n')
        self.stop()
        self.start()
        sub = 'https://sub.a.test'
        self.assertEqual(self.query('geolocation', origin=sub), 'denied')
        self.assertEqual(self.call('geolocation', origin=sub)['value'], {'code':1})
        self.assertEqual(self.query('screen-wake-lock', origin=sub), 'denied')
        self.assertEqual(self.query('local-network', origin=sub), 'prompt')
        self.assertEqual(self.query('notifications', origin='https://a.test:444'), 'prompt')
        self.assertEqual(self.query('camera', origin=sub), 'granted')
        self.assertEqual(self.query('microphone', origin=sub), 'granted')
        self.assertEqual(self.query('camera', origin='https://a.test:444'), 'denied')
        (self.profile/'user.js').write_text('user_pref("privacy.resistFingerprinting", true);\n')
        self.stop()
        self.start()
        self.assertEqual(self.query('camera', origin=sub), 'prompt')

    def test_estimate_tracks_api_origin_payload(self):
        self.assertEqual(self.call('estimate')['value']['usage'], 0)
        path = next((self.root/'store').glob('*.sqlite'))
        with sqlite3.connect(path) as db:
            db.execute('INSERT INTO ls(origin,key,value,len) VALUES (?,?,?,?)', ('https://a.test','k','value',6))
        self.assertEqual(self.call('estimate')['value'], {'quota':10485760,'usage':12})
        self.assertEqual(self.call('estimate', origin='https://a.test:444')['value']['usage'], 0)
        self.assertEqual(self.call('estimate', secure=False)['error']['name'], 'SecurityError')


if __name__ == '__main__':
    unittest.main()
