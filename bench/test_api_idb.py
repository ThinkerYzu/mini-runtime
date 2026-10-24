"""IndexedDB protocol integration: durability, locking, origin scope and upgrades."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

BIN = Path(__file__).resolve().parents[1] / 'mrt-api/target/release/mrt-api'

# A Gecko IndexedDB file for the profile-import test (GAPS B168): DBSchema.cpp's tables, keys as Key.cpp
# encodes them, and records as SpiderMonkey structured clones in raw snappy (literals only).
def snappy_literal(data):
    out, n = bytearray(), len(data)
    while True:
        out.append((n & 0x7F) | (0x80 if n > 0x7F else 0)); n >>= 7
        if not n: break
    for i in range(0, len(data), 65536):
        chunk = data[i:i + 65536]; m = len(chunk) - 1
        out += bytes([m << 2]) if m < 60 else bytes([60 << 2, m]) if m < 256 else bytes([61 << 2]) + m.to_bytes(2, 'little')
        out += chunk
    return bytes(out)

def sc_pair(tag, data=0):
    return ((tag << 32) | data).to_bytes(8, 'little')

def sc_string(s):
    b = s.encode('latin-1')
    return sc_pair(0xFFFF0004, len(b) | 1 << 31) + b + bytes(-len(b) % 8)

def sc_object(**props):
    out = sc_pair(0xFFF10000, 3) + sc_pair(0xFFFF0008)
    for k, v in props.items():
        out += sc_string(k) + (sc_string(v) if isinstance(v, str) else sc_pair(0xFFFF0003, v))
    return out + sc_pair(0xFFFF0013)

def gecko_database(path, name, version, stores):
    import sqlite3
    path.parent.mkdir(parents=True, exist_ok=True)
    db = sqlite3.connect(path)
    db.executescript("""CREATE TABLE database (name TEXT PRIMARY KEY, origin TEXT NOT NULL, version INTEGER NOT NULL DEFAULT 0,
          last_vacuum_time INTEGER NOT NULL DEFAULT 0, last_analyze_time INTEGER NOT NULL DEFAULT 0, last_vacuum_size INTEGER NOT NULL DEFAULT 0) WITHOUT ROWID;
        CREATE TABLE object_store (id INTEGER PRIMARY KEY, auto_increment INTEGER NOT NULL DEFAULT 0, name TEXT NOT NULL, key_path TEXT);
        CREATE TABLE object_store_index (id INTEGER PRIMARY KEY, object_store_id INTEGER NOT NULL, name TEXT NOT NULL, key_path TEXT NOT NULL,
          unique_index INTEGER NOT NULL, multientry INTEGER NOT NULL, locale TEXT, is_auto_locale BOOLEAN NOT NULL);
        CREATE TABLE object_data (object_store_id INTEGER NOT NULL, key BLOB NOT NULL, index_data_values BLOB DEFAULT NULL, file_ids TEXT,
          data BLOB NOT NULL, PRIMARY KEY (object_store_id, key)) WITHOUT ROWID;""")
    db.execute("INSERT INTO database (name, origin, version) VALUES (?, ?, ?)", (name, 'https://example.test', version))
    for sid, (store, next_key, key_path, indexes, records) in enumerate(stores, 1):
        db.execute("INSERT INTO object_store VALUES (?, ?, ?, ?)", (sid, next_key, store, key_path))
        for iname, ipath, locale in indexes:
            db.execute("INSERT INTO object_store_index (object_store_id, name, key_path, unique_index, multientry, locale, is_auto_locale) VALUES (?, ?, ?, 0, 0, ?, 0)", (sid, iname, ipath, locale))
        for key, clone in records:
            db.execute("INSERT INTO object_data (object_store_id, key, data) VALUES (?, ?, ?)", (sid, key, snappy_literal(clone)))
    db.commit(); db.close()


class IndexedDB(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='mrt-idb-')
        self.root = Path(self.tmp.name)
        self.start()

    def start(self):
        self.proc = subprocess.Popen([str(BIN)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, text=True, env=dict(os.environ, API_STORE=str(self.root/'store'), API_PROFILE=str(self.root/'empty')))

    def stop(self):
        self.proc.stdin.close()
        self.assertEqual(self.proc.wait(timeout=5), 0, self.proc.stderr.read())
        self.proc.stdout.close()
        self.proc.stderr.close()

    def tearDown(self):
        self.stop()
        self.tmp.cleanup()

    def reply(self, call, **extra):
        req=dict(op='idb',site='https://example.test',top='https://example.test',origin='https://example.test',call=call)
        req.update(extra)
        self.proc.stdin.write(json.dumps(req)+'\n')
        self.proc.stdin.flush()
        line=self.proc.stdout.readline()
        self.assertTrue(line, self.proc.stderr.read() if self.proc.poll() is not None else 'API ended')
        reply=json.loads(line)
        self.assertNotIn('err',reply,reply)
        return reply['ok']

    def call(self, call, **extra):
        reply=self.reply(call, **extra)
        self.assertNotIn('throw',reply,reply)
        return reply['value']

    def open(self, name='db', **extra):
        out=self.call('open', name=name, version=1, **extra)
        state=out['state']
        if state['kind']=='upgradeneeded':
            tx=state['tx']
            for name in ('a','b'):
                self.call('schema', tx=tx, method='createObjectStore',name=name,options={}, **extra)
            self.call('commit', tx=tx, **extra)
            state=self.call('openWait', req=out['req'], **extra)['state']
        self.assertEqual(state['kind'],'success',state)
        return state['db']['id']

    def tx(self, db, mode='readwrite', stores=('a',)):
        return self.call('transaction', db=db,stores=list(stores),mode=mode)['tx']

    def request(self, tx, method, values=(), store='a'):
        return self.call('request',tx=tx,store=store,method=method,args=[{'v':x} for x in values])

    def test_restart_keeps_committed_and_discards_uncommitted(self):
        db=self.open()
        tx=self.tx(db)
        self.request(tx,'put',('saved',1))
        self.call('commit',tx=tx)
        tx=self.tx(db)
        self.request(tx,'put',('lost',2))
        self.stop();self.start()
        db=self.open();tx=self.tx(db,'readonly')
        self.assertEqual(self.request(tx,'get',(1,))['result'],'saved')
        self.assertEqual(self.request(tx,'get',(2,))['result'],['u'])
        self.call('commit',tx=tx)

    def test_origin_port_subdomain_and_top_site_isolation(self):
        self.open()
        for extra in ({'origin':'https://example.test:444'}, {'origin':'https://sub.example.test'}, {'top':'https://other.test'}):
            self.assertEqual(self.call('databases',**extra),[])
            self.open(**extra)
        self.assertEqual(len(self.call('databases')),1)
        self.assertEqual(self.reply('databases',origin='https://viewer.invalid')['throw']['name'],'SecurityError')
        self.assertEqual(self.reply('databases',origin='null')['throw']['name'],'SecurityError')

    def test_queued_writer_reader_and_disjoint_merge(self):
        db=self.open();first=self.tx(db);second=self.tx(db)
        self.request(first,'put',('one',1))
        pending=self.request(second,'get',(1,))['pending']
        self.assertEqual(self.call('result',id=pending),{'pending':pending})
        other=self.tx(db,stores=('b',));self.request(other,'put',('other',1),store='b');self.call('commit',tx=other)
        self.call('commit',tx=first)
        self.assertEqual(self.call('result',id=pending)['result'],'one')
        self.call('commit',tx=second)
        tx=self.tx(db,'readonly',('a','b'))
        self.assertEqual(self.request(tx,'get',(1,),store='b')['result'],'other')
        self.call('commit',tx=tx)
        reader=self.tx(db,'readonly');writer=self.tx(db)
        pending=self.request(writer,'put',('two',2))['pending']
        self.call('commit',tx=reader)
        self.assertEqual(self.call('result',id=pending)['result'],2.0)
        self.call('commit',tx=writer)

    def test_versionchange_blocking_and_upgrade_rollback(self):
        db=self.open();other=self.open()
        reply=self.reply('open',name='db',version=2)
        state=reply['value'];self.assertEqual(state['state']['kind'],'blocked')
        self.assertEqual({e['db'] for e in reply['events']},{db,other})
        self.call('close',db=db);self.call('close',db=other)
        upgrade=self.call('openWait',req=state['req'])['state'];self.assertEqual(upgrade['kind'],'upgradeneeded')
        waiting=self.call('open',name='db',version=1);self.assertEqual(waiting['state']['kind'],'pending')
        self.call('schema',tx=upgrade['tx'],method='deleteObjectStore',name='b')
        self.call('abort',tx=upgrade['tx'])
        self.assertEqual(self.call('openWait',req=state['req'])['state']['kind'],'error')
        success=self.call('openWait',req=waiting['req'])['state']
        self.assertEqual(success['db']['version'],1)
        self.assertEqual(success['db']['stores'],['a','b'])

    def test_numeric_key_precision_and_truthy_empty_range(self):
        for a,b in ((1.0,1.0000000000000002),(1e-300,1.0000000000000002e-300),(-1.0000000000000002,-1.0)):
            self.assertEqual(self.call('cmp',a={'v':a},b={'v':b}),-1)
        reply=self.reply('keyRange',method='bound',args=[{'v':1},{'v':1},{'v':1},{'v':False}])
        self.assertEqual(reply['throw']['name'],'DataError')

    def test_idle_client_receives_versionchange(self):
        db=self.open(client='worker')
        blocked=self.reply('open',name='db',version=2,client='page')
        self.assertEqual(blocked['value']['state']['kind'],'blocked')
        self.assertEqual(blocked['events'],[])
        events=self.reply('poll',client='worker')['events']
        self.assertEqual([(e['db'],e['oldVersion'],e['newVersion']) for e in events],[(db,1,2)])
        self.call('close',db=db,client='worker')
        upgrade=self.call('openWait',req=blocked['value']['req'],client='page')['state']
        self.assertEqual(upgrade['kind'],'upgradeneeded')
        self.call('commit',tx=upgrade['tx'],client='page')
        self.assertEqual(self.call('openWait',req=blocked['value']['req'],client='page')['state']['kind'],'success')

    def test_clone_graph_order_and_abort_atomicity(self):
        db=self.open();tx=self.tx(db)
        value=['o',0,{'z':['r',0],'a':['m',1,[[1,['bl',2,'AAH/','application/test',None,None]]]],'m':['a',3,[['r',1],['n','NaN']]]}]
        self.request(tx,'put',(value,'graph'))
        self.call('commit',tx=tx)
        tx=self.tx(db)
        got=self.request(tx,'get',('graph',))['result']
        self.assertEqual(list(got[2]),['z','a','m'])
        self.assertEqual(got,value)
        self.request(tx,'clear');self.call('abort',tx=tx)
        tx=self.tx(db,'readonly')
        self.assertEqual(self.request(tx,'get',('graph',))['result'],value)
        self.assertEqual(self.reply('request',tx=tx,store='a',method='clear',args=[])['throw']['name'],'ReadOnlyError')
        self.call('commit',tx=tx)
        opening=self.call('open',name='alias-db',version=1)
        upgrade=opening['state']
        self.call('schema',tx=upgrade['tx'],method='createObjectStore',name='a',options={'keyPath':'alias.id'})
        self.call('commit',tx=upgrade['tx'])
        db=self.call('openWait',req=opening['req'])['state']['db']['id']
        tx=self.tx(db)
        aliased=['o',0,{'map':['m',1,[[0,2]]],'object':['o',2,{'id':9}],'alias':['r',2]}]
        self.assertEqual(self.request(tx,'put',(aliased,))['result'],9)
        self.call('commit',tx=tx)

    def test_profile_databases_imported_once(self):
        idb = self.root/'empty'/'storage'/'default'/'https+++example.test'/'idb'
        one, key_k = b'\x10\xbf\xf0', b'\x30' + bytes([ord('k') + 1])
        gecko_database(idb/'1234imported.sqlite', 'imported', 2, [
            ('s', 6, None, [('byN', 'n', None), ('byLocale', 'a', 'en')],
             [(one, sc_object(a='x', n=5)), (key_k, sc_object(a='y', n=7)),
              (b'\x10\xc0', sc_pair(0xFFF10000, 3) + sc_pair(0xFFFF800A))]),   # a CryptoKey: left out
            ('inline', 0, ',id,sub', [], []),
        ])
        self.assertEqual(self.call('databases'), [{'name': 'imported', 'version': 2}])
        state = self.call('open', name='imported', version=2)['state']
        self.assertEqual(state['kind'], 'success', state)
        self.assertEqual(state['db']['stores'], ['inline', 's'])
        db = state['db']['id']
        tx = self.call('transaction', db=db, stores=['s', 'inline'], mode='readwrite')
        self.assertEqual([s['keyPath'] for s in tx['stores']], [['id', 'sub'], None])
        self.assertEqual(self.call('store', tx=tx['tx'], store='s')['indexNames'], ['byN'])
        self.assertEqual(self.request(tx['tx'], 'getAll', store='s')['result'],
                         ['a', 0, [['o', 0, {'a': 'x', 'n': 5}], ['o', 0, {'a': 'y', 'n': 7}]]])
        self.assertEqual(self.request(tx['tx'], 'getAllKeys', store='s')['result'], ['a', 0, [1, 'k']])
        self.assertEqual(self.request(tx['tx'], 'add', ('new',), store='s')['result'], 6)
        self.call('commit', tx=tx['tx'])
        # what was left out is reported; the native row is the database from now on, not imported
        # again after a restart
        self.proc.stdin.close()
        self.assertEqual(self.proc.wait(timeout=5), 0)
        err = self.proc.stderr.read()
        self.proc.stdout.close(); self.proc.stderr.close()
        self.assertIn('locale-aware index byLocale not imported', err)
        self.assertIn('record not imported: clone tag 0xffff800a', err)
        self.start()
        state = self.call('open', name='imported', version=2)['state']
        tx = self.call('transaction', db=state['db']['id'], stores=['s'], mode='readonly')['tx']
        self.assertEqual(self.request(tx, 'count', store='s')['result'], 3)
        self.call('commit', tx=tx)

if __name__=='__main__':
    unittest.main()
