// IndexedDB requests from the mini runtime, run against the bridge tab's real IndexedDB (SPEC decision
// 12, DESIGN "IndexedDB, forwarded to Firefox"). mini_realsites.py runs this in Marionette's content
// sandbox "idb", kept between calls (new_sandbox=False), with clone_codec.js before it as CODEC. State
// lives in `globalThis.__mrtIdb`: Firefox's databases, transactions, open requests and cursors by id.
//
// A transaction stays open across calls by a chain of requests: each `get` of a key no record has
// re-issues itself from its success callback, which is the only moment Firefox accepts new requests
// for an inactive transaction. Requests the runtime sends wait in the transaction's queue and are
// issued from that callback. Stopping the chain (commit) lets Firefox commit after the last request,
// as it does a page's transaction. When the runtime's transaction is not active, a request is made
// directly, outside the chain, so that Firefox raises its own TransactionInactiveError.
//
// Every reply carries `events`: what Firefox fired at the databases meanwhile (versionchange, close).
const [req] = arguments;
const done = arguments[arguments.length - 1];
const W = window.wrappedJSObject;
const KEEP = "\u0000mrt-keepalive";
const TEMP_STORE = "\u0000mrt-keepalive";
// ids start at the time the sandbox was made, so that a bridge tab made after another one was dropped
// never gives an id the runtime still holds from the old one (bridge.js bridgeLost)
const S = globalThis.__mrtIdb || (globalThis.__mrtIdb = { next: Date.now() * 1000, objs: new Map(), events: [] });
const put = o => { const id = S.next++; S.objs.set(id, o); return id; };
const get = id => {
  const o = S.objs.get(id);
  if (!o) throw new Error("idb: no object " + id);
  return o;
};
const errOf = e => (e ? { name: String(e.name), message: String(e.message) } : null);
const unwrap = v => (v && typeof v === "object" && v.wrappedJSObject) || v;
const FAIL = {};
// a buffer's bytes, viewed in the buffer's own realm: a request's result is the page's (typed array data
// cannot be read over Xrays), a cursor's value is this sandbox's (Gecko deserializes it for the caller)
const bytes = (b, o, l) => (b instanceof ArrayBuffer ? new Uint8Array(b, o, l) : unwrap(new W.Uint8Array(b, o, l)));
// A Blob in a result is read through a Response of this sandbox (Firefox reads a Blob only
// asynchronously); the reads of one reply are collected in `blobReads`, and the reply waits for them
let blobReads = null;
const readBlob = (v, set) => {
  if (!(v instanceof W.Blob)) return false;
  blobReads.push(new window.Response(v).arrayBuffer().then(ab => set(bytes(ab, 0, ab.byteLength))));
  return true;
};
const enc = v => CODEC.encode(v, { unwrap, bytes, readBlob, fail: FAIL });
// A key's buffers made in this sandbox: Gecko's Key::EncodeJSVal takes a binary key only when it is an
// ArrayBuffer or view of the caller's compartment (ArrayBufferOrView::fromObject does not unwrap), so a
// buffer of the page's realm is not a valid key here (DataError); the rest of a key is the page's
const KEY_G = new Proxy(W, { get: (w, name) => (/^(\w+Array|ArrayBuffer|DataView)$/.test(name) && name !== "Array" ? globalThis[name] : w[name]) });
// the arguments that are values, not keys: put's and add's first, a cursor's update
const VALUE_ARG = { put: 0, add: 0, update: 0 };
// an argument: a key range, a key, a value (`isValue`), or a value the runtime could not encode (Firefox
// raises its own DataCloneError for a function)
const arg = (a, isValue) => {
  if (a.fail) return () => {};
  if (!a.range) return CODEC.decode(a.v, isValue ? W : KEY_G);
  const [lower, upper, lowerOpen, upperOpen] = a.range.map((x, i) => (i < 2 ? CODEC.decode(x, KEY_G) : x));
  if (lower === undefined) return window.IDBKeyRange.upperBound(upper, upperOpen);
  if (upper === undefined) return window.IDBKeyRange.lowerBound(lower, lowerOpen);
  return window.IDBKeyRange.bound(lower, upper, lowerOpen, upperOpen);
};
const list = l => Array.from(l);
const keyPath = k => (k === null ? null : typeof k === "string" ? k : Array.from(unwrap(k)));
const storeInfo = s => ({ name: s.name, keyPath: keyPath(s.keyPath), autoIncrement: s.autoIncrement, indexNames: list(s.indexNames) });
const indexInfo = i => ({ name: i.name, keyPath: keyPath(i.keyPath), unique: i.unique, multiEntry: i.multiEntry });
const dbInfo = (db, id) => ({ id, name: db.name, version: db.version, stores: list(db.objectStoreNames).filter(n => n !== TEMP_STORE) });

function watchDatabase(db) {
  const id = put({ db });
  db.onversionchange = ev => S.events.push({ db: id, type: "versionchange", oldVersion: ev.oldVersion, newVersion: ev.newVersion });
  db.onclose = () => S.events.push({ db: id, type: "close" });
  return id;
}

// A transaction kept open until the runtime commits or aborts it.
function keepOpen(tx, db, versionChange) {
  // the store and index objects the page holds are Firefox's own, kept per transaction: after the
  // transaction ends Firefox's objectStore() fails, but a store the page got earlier still answers
  // (TransactionInactiveError for a request)
  const rec = { tx, db, q: [], done: false, finished: false, temp: false, stores: new Map(), indexes: new Map() };
  rec.result = new Promise(resolve => {
    tx.addEventListener("complete", () => { rec.finished = true; resolve({ complete: true }); });
    tx.addEventListener("abort", () => { rec.finished = true; resolve({ aborted: errOf(tx.error) }); });
  });
  rec.keepStore = list(tx.objectStoreNames)[0];
  if (rec.keepStore === undefined) {
    // a versionchange transaction of a database with no store: a temporary one until the page makes
    // its first (keepStoreChanged), so that Firefox's view of the stores (their order in its error
    // messages) is the page's
    db.createObjectStore(TEMP_STORE);
    rec.temp = true;
    rec.keepStore = TEMP_STORE;
  }
  const spin = () => {
    // counted for the bridge's memory (task 4898: a transaction kept open while the runtime waits spins)
    rec.spins = (rec.spins || 0) + 1;
    while (rec.q.length) rec.q.shift()();
    // S.stopSpin (BRIDGE_IDB_STOP, task 4898): no more keep-alive requests, so Firefox commits it
    if (rec.done || S.stopSpin) {
      if (rec.temp) { try { db.deleteObjectStore(TEMP_STORE); } catch (e) {} }
      return;
    }
    try {
      const r = tx.objectStore(rec.keepStore).get(KEEP);
      r.onsuccess = spin;
      r.onerror = ev => { ev.preventDefault(); spin(); };
    } catch (e) {
      // the transaction finished (Firefox aborted it): what is queued runs and fails
      while (rec.q.length) rec.q.shift()();
    }
  };
  spin();
  return rec;
}

// After a schema change in a versionchange transaction: the chain needs a store that exists, and the
// temporary one goes once the page has one of its own.
function storeOf(rec, name) {
  if (!rec.stores.has(name)) rec.stores.set(name, rec.tx.objectStore(name));
  return rec.stores.get(name);
}
function indexOf(rec, store, name) {
  const k = store + "\u0000" + name;
  if (!rec.indexes.has(k)) rec.indexes.set(k, storeOf(rec, store).index(name));
  return rec.indexes.get(k);
}

function keepStoreChanged(rec) {
  const names = list(rec.db.objectStoreNames).filter(n => n !== TEMP_STORE);
  if (rec.temp && names.length) {
    rec.db.deleteObjectStore(TEMP_STORE);
    rec.temp = false;
  }
  if (!rec.temp && !names.includes(rec.keepStore)) {
    if (names.length) rec.keepStore = names[0];
    else {
      rec.db.createObjectStore(TEMP_STORE);
      rec.temp = true;
      rec.keepStore = TEMP_STORE;
    }
  }
}

// Runs `fn(resolve)` inside the transaction (from the chain), or at once when the runtime's
// transaction is not active or Firefox's has finished. A synchronous throw is Firefox's error.
function inTx(rec, inactive, fn) {
  return new Promise(resolve => {
    const run = () => { try { fn(resolve); } catch (e) { resolve({ throw: errOf(e) }); } };
    if (inactive || rec.finished) run();
    else rec.q.push(run);
  });
}

// An answer Firefox may not have yet: a transaction waits for an earlier one on the same stores to
// finish, and that one may be waiting for the runtime's next task. What is not there within `ms` is
// kept under an id the runtime asks for again ("result"), so a call never blocks the runtime.
function answer(promise, ms) {
  const o = { done: false, value: null };
  promise.then(v => { o.done = true; o.value = v; });
  return new Promise(resolve => {
    const timer = setTimeout(() => resolve({ pending: put(o) }), ms);
    promise.then(v => { clearTimeout(timer); resolve(v); });
  });
}
async function result(id, ms) {
  const o = get(id);
  if (!o.done) await new Promise(resolve => { const t = setTimeout(resolve, ms); const poll = () => (o.done ? (clearTimeout(t), resolve()) : setTimeout(poll, 5)); poll(); });
  if (!o.done) return { pending: id };
  S.objs.delete(id);
  return o.value;
}
const WAIT = 200;

// The states of an open or delete request, in the order Firefox fires them.
function watchOpen(r, isDelete) {
  const o = { r, states: [], wake: null };
  const push = st => { o.states.push(st); if (o.wake) { const w = o.wake; o.wake = null; w(); } };
  r.onblocked = ev => push({ kind: "blocked", oldVersion: ev.oldVersion, newVersion: ev.newVersion });
  r.onerror = ev => { ev.preventDefault(); push({ kind: "error", error: errOf(r.error) }); };
  if (isDelete) {
    r.onsuccess = ev => push({ kind: "success", oldVersion: ev.oldVersion, newVersion: ev.newVersion });
  } else {
    r.onupgradeneeded = ev => {
      const db = r.result, id = watchDatabase(db);
      o.db = id;
      const rec = keepOpen(r.transaction, db, true);
      push({ kind: "upgradeneeded", oldVersion: ev.oldVersion, newVersion: ev.newVersion, db: dbInfo(db, id), tx: put(rec) });
    };
    r.onsuccess = () => {
      const db = r.result, id = o.db || watchDatabase(db);
      push({ kind: "success", db: dbInfo(db, id) });
    };
  }
  return put(o);
}
function nextState(id, ms) {
  const o = get(id);
  if (o.states.length) return Promise.resolve(o.states.shift());
  return new Promise(resolve => {
    const timer = setTimeout(() => { o.wake = null; resolve({ kind: "pending" }); }, ms);
    o.wake = () => { clearTimeout(timer); resolve(o.states.shift()); };
  });
}

// A request's result, or the cursor's next position.
function settle(r, resolve, cursor) {
  // a result the codec cannot carry is reported, never left unanswered (an exception here would also
  // abort Firefox's transaction)
  const failed = e => resolve({ error: { name: "UnknownError", message: "bridge: " + e + " at " + String(e && e.stack).split("\n").slice(0, 4).join(" | ") } });
  r.onsuccess = () => {
    blobReads = [];
    try {
      const out = reply(), reads = blobReads;
      if (reads.length) Promise.all(reads).then(() => resolve(out), failed);
      else resolve(out);
    } catch (e) { failed(e); }
    blobReads = null;
  };
  const reply = () => {
    if (!cursor) return { result: enc(r.result) };
    const c = r.result;
    if (!c) return { result: null };
    cursor.c = c;
    const out = { cursor: cursor.id, key: enc(c.key), primaryKey: enc(c.primaryKey) };
    if (cursor.withValue) out.value = enc(c.value);
    return out;
  };
  r.onerror = ev => { ev.preventDefault(); resolve({ error: errOf(r.error) }); };
}

async function run() {
  switch (req.call) {
    case "open": {
      const r = req.version === undefined ? window.indexedDB.open(req.name) : window.indexedDB.open(req.name, req.version);
      const id = watchOpen(r, false);
      return { req: id, state: await nextState(id, 5000) };
    }
    case "reopen": {
      // a database the page had open when its bridge tab was dropped, opened again at its version in
      // this one (bridge.js reopenDatabases): its new id, or closed when the version moved on (VersionError)
      // or the database was deleted meanwhile (an upgrade from 0, aborted so that nothing is made)
      const r = window.indexedDB.open(req.name, req.version);
      return new Promise(resolve => {
        r.onupgradeneeded = () => { r.transaction.abort(); };
        r.onerror = ev => { ev.preventDefault(); resolve({ closed: errOf(r.error) }); };
        r.onsuccess = () => resolve({ id: watchDatabase(r.result) });
      });
    }
    case "deleteDatabase": {
      const id = watchOpen(window.indexedDB.deleteDatabase(req.name), true);
      return { req: id, state: await nextState(id, 5000) };
    }
    case "openWait":
      return { state: await nextState(req.req, req.ms || 2000) };
    case "databases": {
      const l = await window.indexedDB.databases();
      return Array.from(l, d => ({ name: d.name, version: d.version }));
    }
    case "cmp":
      return window.indexedDB.cmp(arg(req.a), arg(req.b));
    case "keyRange": {
      // the runtime's IDBKeyRange is checked by Firefox's (DataError for a bad key or order)
      const args = req.args.map(a => arg(a));
      window.IDBKeyRange[req.method](...args);
      return null;
    }
    case "includes":
      return arg(req.range).includes(arg(req.key));
    case "close":
      get(req.db).db.close();
      return null;
    case "transaction": {
      const { db } = get(req.db);
      const tx = req.durability ? db.transaction(req.stores, req.mode, { durability: req.durability }) : db.transaction(req.stores, req.mode);
      const rec = keepOpen(tx, db, false);
      const stores = list(tx.objectStoreNames).map(n => storeInfo(storeOf(rec, n)));
      return { tx: put(rec), stores, mode: tx.mode, durability: tx.durability };
    }
    case "store": {
      const rec = get(req.tx);
      return storeInfo(storeOf(rec, req.store));
    }
    case "index": {
      const rec = get(req.tx);
      return indexInfo(indexOf(rec, req.store, req.name));
    }
    case "request": {
      const rec = get(req.tx);
      return answer(inTx(rec, req.inactive, resolve => {
        let src;
        if (req.cursor) src = get(req.cursor).c;
        else {
          src = req.index != null ? indexOf(rec, req.store, req.index) : storeOf(rec, req.store);
        }
        const r = src[req.method](...req.args.map((a, i) => arg(a, VALUE_ARG[req.method] === i)));
        const cursor = req.method === "openCursor" || req.method === "openKeyCursor" ? { withValue: req.method === "openCursor" } : null;
        if (cursor) cursor.id = put(cursor);
        settle(r, resolve, cursor);
      }), WAIT);
    }
    case "cursor": {
      // continue, advance or continuePrimaryKey: the same request fires again
      const rec = get(req.tx), cursor = get(req.cursor);
      return answer(inTx(rec, req.inactive, resolve => {
        cursor.c[req.method](...req.args.map((a, i) => arg(a, VALUE_ARG[req.method] === i)));
        settle(cursor.c.request, resolve, cursor);
      }), WAIT);
    }
    case "schema": {
      // createObjectStore, deleteObjectStore, createIndex, deleteIndex, and renames, in a
      // versionchange transaction
      const rec = get(req.tx);
      return inTx(rec, req.inactive, resolve => {
        switch (req.method) {
          case "createObjectStore": {
            const store = rec.db.createObjectStore(req.name, req.options);
            rec.stores.set(req.name, store);
            keepStoreChanged(rec);
            resolve(storeInfo(store));
            break;
          }
          case "deleteObjectStore": rec.db.deleteObjectStore(req.name); rec.stores.delete(req.name); keepStoreChanged(rec); resolve(null); break;
          case "renameStore": {
            const store = storeOf(rec, req.store);
            store.name = req.name;
            rec.stores.delete(req.store);
            rec.stores.set(req.name, store);
            keepStoreChanged(rec);
            resolve(null);
            break;
          }
          case "createIndex": {
            const index = storeOf(rec, req.store).createIndex(req.name, req.keyPath, req.options);
            rec.indexes.set(req.store + "\u0000" + req.name, index);
            resolve(indexInfo(index));
            break;
          }
          case "deleteIndex": storeOf(rec, req.store).deleteIndex(req.name); resolve(null); break;
          case "renameIndex": indexOf(rec, req.store, req.index).name = req.name; resolve(null); break;
        }
      });
    }
    case "commit": {
      const rec = get(req.tx);
      rec.done = true;
      return answer(rec.result, WAIT);
    }
    case "result":
      return result(req.id, req.ms || WAIT);
    case "abort": {
      const rec = get(req.tx);
      rec.done = true;
      try { rec.tx.abort(); } catch (e) { return { throw: errOf(e) }; }
      return answer(rec.result, WAIT);
    }
  }
  throw new Error("idb: unknown call " + req.call);
}

// `ms`: how long Firefox took over the call, which the runtime lets pass on its clock before it
// delivers the answer (SPEC decision 20)
const t0 = Date.now();
run().then(ok => done({ ok: { value: ok === undefined ? null : ok, events: S.events.splice(0), ms: Date.now() - t0 } }),
           e => done({ ok: { throw: errOf(e), events: S.events.splice(0), ms: Date.now() - t0 } }));
