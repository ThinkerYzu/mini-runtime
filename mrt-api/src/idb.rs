//! IndexedDB protocol endpoint. JS keeps WebIDL/activity/events; this service owns
//! origin-partitioned database state, transaction scheduling and atomic storage.
use crate::store::{Store, site_of};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
#[path = "idb_key.rs"]
pub mod key;
#[path = "idb_ops.rs"]
mod ops;
#[path = "idb_profile.rs"]
mod profile;
use key::{Key, Range};

#[derive(Debug, Clone)]
pub struct Error {
    pub name: String,
    pub message: String,
}
pub type Result<T> = std::result::Result<T, Error>;
pub fn fail<T>(name: &str) -> Result<T> {
    Err(Error::named(name))
}
impl Error {
    pub fn new(name: &str, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            message: message.into(),
        }
    }
    pub fn named(name: &str) -> Self {
        Self::new(
            name,
            match name {
                "DataError" => "Data provided to an operation does not meet requirements.",
                "ConstraintError" => {
                    "A mutation operation in the transaction failed because a constraint was not satisfied."
                }
                "TransactionInactiveError" => {
                    "A request was placed against a transaction which is currently not active, or which is finished."
                }
                "ReadOnlyError" => "A mutation operation was attempted in a READ_ONLY transaction.",
                "NotFoundError" => {
                    "The operation failed because the requested database object could not be found. For example, an object store did not exist but was being opened."
                }
                "InvalidStateError" => {
                    "A mutation operation was attempted on a database that did not allow mutations."
                }
                "AbortError" => {
                    "A request was aborted, for example through a call to IDBTransaction.abort."
                }
                "VersionError" => {
                    "The operation failed because the stored database is a higher version than the version requested."
                }
                "DataCloneError" => "Function object could not be cloned.",
                "SyntaxError" => "An invalid or illegal string was specified",
                _ => {
                    "The operation failed for reasons unrelated to the database itself and not covered by any other error code."
                }
            },
        )
    }
    pub fn value(&self) -> Value {
        json!({"name":self.name,"message":self.message})
    }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::new("UnknownError", format!("IndexedDB storage: {e}"))
    }
}
fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn number(v: &Value, k: &str) -> u64 {
    v[k].as_u64().unwrap_or(0)
}
fn names(v: &Value) -> Vec<String> {
    let mut n = v
        .as_object()
        .map(|m| m.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    n.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    n
}
pub(super) fn store_info(name: &str, s: &Value) -> Value {
    json!({"name":name,"keyPath":s["keyPath"],"autoIncrement":s["autoIncrement"],"indexNames":names(&s["indexes"])})
}
pub(super) fn index_info(name: &str, i: &Value) -> Value {
    json!({"name":name,"keyPath":i["keyPath"],"unique":i["unique"],"multiEntry":i["multiEntry"]})
}
#[derive(Clone)]
struct Db {
    name: String,
    closed: bool,
    client: String,
}
struct Tx {
    db: u64,
    name: String,
    scope: Vec<String>,
    mode: String,
    data: Option<Value>,
    finished: bool,
    aborted: bool,
    open: Option<u64>,
}
struct Opening {
    name: String,
    version: Option<u64>,
    delete: bool,
    old: u64,
    db: Option<u64>,
    tx: Option<u64>,
    states: VecDeque<Value>,
    notified: bool,
    finished: bool,
    client: String,
}
#[derive(Clone)]
struct Cursor {
    tx: u64,
    store: String,
    index: Option<String>,
    range: Range,
    direction: String,
    with_value: bool,
    key: Key,
    primary: Key,
}
struct Pending {
    req: Value,
    result: Option<Value>,
}
pub struct State {
    next: u64,
    dbs: BTreeMap<u64, Db>,
    txs: BTreeMap<u64, Tx>,
    opens: BTreeMap<u64, Opening>,
    cursors: BTreeMap<u64, Cursor>,
    pending: BTreeMap<u64, Pending>,
    events: Vec<Value>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            next: (crate::cookie::now_us() / 1000) as u64 * 1000,
            dbs: BTreeMap::new(),
            txs: BTreeMap::new(),
            opens: BTreeMap::new(),
            cursors: BTreeMap::new(),
            pending: BTreeMap::new(),
            events: vec![],
        }
    }
}
impl State {
    fn id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
    fn load(store: &Store, origin: &str, name: &str) -> Result<Value> {
        let value: Option<String> = store
            .conn
            .query_row(
                "SELECT data FROM idb_databases WHERE origin=?1 AND name=?2",
                params![origin, name],
                |r| r.get(0),
            )
            .optional()?;
        match value {
            Some(s) => {
                serde_json::from_str(&s).map_err(|e| Error::new("UnknownError", e.to_string()))
            }
            None => Ok(json!({"name":name,"version":0,"stores":{}})),
        }
    }
    fn save(store: &Store, origin: &str, data: &Value) -> Result<()> {
        store.conn.execute("INSERT INTO idb_databases VALUES (?1,?2,?3) ON CONFLICT(origin,name) DO UPDATE SET data=excluded.data",params![origin,text(data,"name"),data.to_string()])?;
        Ok(())
    }
    fn db_info(&self, id: u64, data: &Value) -> Value {
        json!({"id":id,"name":data["name"],"version":data["version"],"stores":names(&data["stores"])})
    }
    fn tx(&self, id: u64) -> Result<&Tx> {
        self.txs
            .get(&id)
            .ok_or_else(|| Error::named("InvalidStateError"))
    }
    fn ready(&self, id: u64) -> bool {
        let Some(t) = self.txs.get(&id) else {
            return false;
        };
        !self.txs.range(..id).any(|(_, a)| {
            !a.finished
                && a.name == t.name
                && (a.mode == "versionchange"
                    || t.mode == "versionchange"
                    || ((a.mode != "readonly" || t.mode != "readonly")
                        && a.scope.iter().any(|n| t.scope.contains(n))))
        })
    }
    fn activate(&mut self, id: u64, store: &Store, origin: &str) -> Result<bool> {
        if !self.ready(id) {
            return Ok(false);
        }
        let t = self.tx(id)?;
        if t.finished {
            return fail("TransactionInactiveError");
        }
        if t.data.is_none() {
            let data = Self::load(store, origin, &t.name)?;
            self.txs.get_mut(&id).unwrap().data = Some(data);
        }
        Ok(true)
    }
    fn tx_info(&self, id: u64, data: &Value) -> Result<Value> {
        let t = self.tx(id)?;
        Ok(
            json!({"tx":id,"stores":t.scope.iter().filter_map(|n|data["stores"].get(n).map(|s|store_info(n,s))).collect::<Vec<_>>(),"mode":t.mode,"durability":"default"}),
        )
    }
    fn start_tx(
        &mut self,
        db: u64,
        scope: Vec<String>,
        mode: &str,
        data: Value,
        open: Option<u64>,
    ) -> u64 {
        let id = self.id();
        let name = text(&data, "name").to_string();
        self.txs.insert(
            id,
            Tx {
                db,
                name,
                scope,
                mode: mode.into(),
                data: None,
                finished: false,
                aborted: false,
                open,
            },
        );
        if self.ready(id) {
            self.txs.get_mut(&id).unwrap().data = Some(data)
        }
        id
    }
    fn finish(&mut self, id: u64, abort: bool, store: &Store, origin: &str) -> Result<Value> {
        let t = self.tx(id)?;
        if t.finished {
            return Ok(if t.aborted {
                json!({"aborted":null})
            } else {
                json!({"complete":true})
            });
        }
        let (name, mode, scope, db, open) = (
            t.name.clone(),
            t.mode.clone(),
            t.scope.clone(),
            t.db,
            t.open,
        );
        if !abort {
            if !self.activate(id, store, origin)? {
                return fail("TransactionInactiveError");
            }
            let data = self.tx(id)?.data.as_ref().unwrap();
            if mode == "versionchange" {
                Self::save(store, origin, data)?
            } else if mode != "readonly" {
                let mut current = Self::load(store, origin, &name)?;
                for n in &scope {
                    current["stores"][n] = data["stores"][n].clone();
                }
                Self::save(store, origin, &current)?;
            }
        }
        let t = self.txs.get_mut(&id).unwrap();
        t.finished = true;
        t.aborted = abort;
        t.data = None;
        self.cursors.retain(|_, c| c.tx != id);
        if let Some(o) = open {
            let data = Self::load(store, origin, &name)?;
            let st = if abort {
                self.dbs.get_mut(&db).unwrap().closed = true;
                json!({"kind":"error","error":Error::named("AbortError").value()})
            } else {
                json!({"kind":"success","db":self.db_info(db,&data)})
            };
            self.opens.get_mut(&o).unwrap().states.push_back(st);
        }
        Ok(if abort {
            json!({"aborted":null})
        } else {
            json!({"complete":true})
        })
    }
    fn open_step(&mut self, id: u64, store: &Store, origin: &str) -> Result<Value> {
        let o = self
            .opens
            .get_mut(&id)
            .ok_or_else(|| Error::named("InvalidStateError"))?;
        if let Some(s) = o.states.pop_front() {
            if s["kind"] == "success" || s["kind"] == "error" {
                o.finished = true;
            }
            return Ok(s);
        }
        if o.finished || o.tx.is_some() {
            return Ok(json!({"kind":"pending"}));
        }
        let (name, version, delete, notified, client) = (
            o.name.clone(),
            o.version,
            o.delete,
            o.notified,
            o.client.clone(),
        );
        let mut data = Self::load(store, origin, &name)?;
        let old = number(&data, "version");
        // A later open/delete waits for an upgrade to finish, even when its
        // requested version matches the previously committed version.
        if self
            .txs
            .values()
            .any(|t| !t.finished && t.mode == "versionchange" && t.name == name)
        {
            return Ok(json!({"kind":"pending"}));
        }
        let version = version.unwrap_or(old.max(1));
        if !delete && version < old {
            return Ok(json!({"kind":"error","error":Error::named("VersionError").value()}));
        }
        let upgrade = !delete && version > old;
        if delete || upgrade {
            let conn = self
                .dbs
                .iter()
                .filter(|(_, d)| !d.closed && d.name == name)
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            let active = self.txs.values().any(|t| !t.finished && t.name == name);
            if !conn.is_empty() || active {
                if !notified {
                    for db in conn {
                        self.events.push(json!({"client":self.dbs[&db].client,"db":db,"type":"versionchange","oldVersion":old,"newVersion":if delete{Value::Null}else{json!(version)}}));
                    }
                    self.opens.get_mut(&id).unwrap().notified = true;
                    return Ok(
                        json!({"kind":"blocked","oldVersion":old,"newVersion":if delete{Value::Null}else{json!(version)}}),
                    );
                }
                return Ok(json!({"kind":"pending"}));
            }
        }
        if delete {
            store.conn.execute(
                "DELETE FROM idb_databases WHERE origin=?1 AND name=?2",
                params![origin, name],
            )?;
            self.opens.get_mut(&id).unwrap().finished = true;
            return Ok(json!({"kind":"success","oldVersion":old,"newVersion":null}));
        }
        let db = self.id();
        self.dbs.insert(
            db,
            Db {
                name,
                closed: false,
                client,
            },
        );
        if upgrade {
            data["version"] = json!(version);
            let scope = names(&data["stores"]);
            let tx = self.start_tx(db, scope, "versionchange", data.clone(), Some(id));
            let o = self.opens.get_mut(&id).unwrap();
            o.old = old;
            o.db = Some(db);
            o.tx = Some(tx);
            Ok(
                json!({"kind":"upgradeneeded","oldVersion":old,"newVersion":version,"db":self.db_info(db,&data),"tx":tx}),
            )
        } else {
            self.opens.get_mut(&id).unwrap().finished = true;
            Ok(json!({"kind":"success","db":self.db_info(db,&data)}))
        }
    }
    fn enqueue(&mut self, req: Value) -> Value {
        let id = self.id();
        self.pending.insert(id, Pending { req, result: None });
        json!({"pending":id})
    }
    fn pump(&mut self, store: &Store, origin: &str) {
        let ids = self.pending.keys().copied().collect::<Vec<_>>();
        for id in ids {
            let p = &self.pending[&id];
            if p.result.is_some() {
                continue;
            }
            let req = p.req.clone();
            let tx = number(&req, "tx");
            if !self.ready(tx) {
                continue;
            }
            let result = match text(&req, "call") {
                "commit" => self.finish(tx, false, store, origin),
                _ => self.execute(&req, store, origin),
            };
            self.pending.get_mut(&id).unwrap().result =
                Some(result.unwrap_or_else(|e| json!({"throw":e.value()})));
        }
    }
    pub fn handle(&mut self, req: &Value, store: &Store) -> std::result::Result<Value, String> {
        let origin = text(req, "origin");
        let valid = url::Url::parse(origin).is_ok_and(|u| {
            matches!(u.scheme(), "http" | "https")
                && u.origin().ascii_serialization() == origin
                && site_of(&u) == text(req, "site")
        });
        if valid {
            profile::seed(store, origin);
        }
        let result = if valid || matches!(text(req, "call"), "cmp" | "keyRange" | "includes") {
            self.run(req, store, origin)
        } else {
            Err(Error::new(
                "SecurityError",
                "IndexedDB requires a non-opaque origin in its site partition.",
            ))
        };
        if valid {
            self.pump(store, origin);
        }
        let mut events = vec![];
        let client = text(req, "client");
        self.events.retain(|event| {
            if text(event, "client") == client {
                events.push(event.clone());
                false
            } else {
                true
            }
        });
        Ok(match result {
            Ok(value) => {
                let pending = value.get("pending").is_some() || value["state"]["kind"] == "pending";
                json!({"value":value,"events":events,"ms":if pending{200}else{1},"native":true})
            }
            Err(e) => json!({"throw":e.value(),"events":events,"ms":1,"native":true}),
        })
    }
    fn run(&mut self, r: &Value, store: &Store, origin: &str) -> Result<Value> {
        match text(r, "call") {
            "poll" => Ok(Value::Null),
            "cmp" => Ok(json!(
                match key::parse(&r["a"]["v"])?.cmp(&key::parse(&r["b"]["v"])?) {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                }
            )),
            "keyRange" => {
                let a = r["args"].as_array().cloned().unwrap_or_default();
                let method = text(r, "method");
                let k = |i: usize| {
                    key::parse(a.get(i).and_then(|a| a.get("v")).unwrap_or(&Value::Null))
                };
                let lo = k(0)?;
                if method == "bound" {
                    let hi = k(1)?;
                    let range = Range {
                        lower: Some(lo),
                        upper: Some(hi),
                        lo: a.get(2).is_some_and(|v| key::truthy(&v["v"])),
                        hi: a.get(3).is_some_and(|v| key::truthy(&v["v"])),
                    };
                    range.validate()?;
                }
                Ok(Value::Null)
            }
            "includes" => Ok(json!(
                Range::arg(Some(&r["range"]), false)?.contains(&key::parse(&r["key"]["v"])?)
            )),
            "open" | "deleteDatabase" => {
                let version = if r.get("version").is_some() {
                    let x = match &r["version"] {
                        Value::Number(n) => n.as_f64().unwrap_or(0.0),
                        Value::String(s) => s.parse().unwrap_or(f64::NAN),
                        Value::Bool(b) => {
                            if *b {
                                1.0
                            } else {
                                0.0
                            }
                        }
                        _ => 0.0,
                    };
                    if !x.is_finite() || x < 1.0 || x >= 18446744073709551616.0 {
                        return Err(Error::new(
                            "TypeError",
                            "IDBFactory.open: Version must be greater than 0.",
                        ));
                    }
                    Some(x.trunc() as u64)
                } else {
                    None
                };
                let id = self.id();
                self.opens.insert(
                    id,
                    Opening {
                        name: text(r, "name").into(),
                        version,
                        delete: text(r, "call") == "deleteDatabase",
                        old: 0,
                        db: None,
                        tx: None,
                        states: VecDeque::new(),
                        notified: false,
                        finished: false,
                        client: text(r, "client").into(),
                    },
                );
                let state = self.open_step(id, store, origin)?;
                Ok(json!({"req":id,"state":state}))
            }
            "openWait" => Ok(json!({"state":self.open_step(number(r,"req"),store,origin)?})),
            "reopen" => {
                let name = text(r, "name");
                let data = Self::load(store, origin, name)?;
                if number(&data, "version") != number(r, "version") {
                    return Ok(json!({"closed":Error::named("VersionError").value()}));
                }
                let id = self.id();
                self.dbs.insert(
                    id,
                    Db {
                        name: name.into(),
                        closed: false,
                        client: text(r, "client").into(),
                    },
                );
                Ok(json!({"id":id}))
            }
            "databases" => {
                let mut st = store
                    .conn
                    .prepare("SELECT data FROM idb_databases WHERE origin=?1 ORDER BY name")?;
                let rows = st.query_map([origin], |r| r.get::<_, String>(0))?;
                let mut out = vec![];
                for s in rows {
                    let d: Value = serde_json::from_str(&s?)
                        .map_err(|e| Error::new("UnknownError", e.to_string()))?;
                    out.push(json!({"name":d["name"],"version":d["version"]}));
                }
                Ok(json!(out))
            }
            "close" => {
                self.dbs
                    .get_mut(&number(r, "db"))
                    .ok_or_else(|| Error::named("InvalidStateError"))?
                    .closed = true;
                Ok(Value::Null)
            }
            "transaction" => {
                let db = number(r, "db");
                let conn = self
                    .dbs
                    .get(&db)
                    .ok_or_else(|| Error::named("InvalidStateError"))?;
                let mode = text(r, "mode");
                if !matches!(mode, "readonly" | "readwrite" | "readwriteflush") {
                    return Err(Error::new(
                        "TypeError",
                        format!(
                            "IDBDatabase.transaction: '{mode}' (value of argument 2) is not a valid value for enumeration IDBTransactionMode."
                        ),
                    ));
                }
                if conn.closed {
                    return Err(Error::new(
                        "InvalidStateError",
                        "IDBDatabase.transaction: Can't start a transaction on a closed database",
                    ));
                }
                let scope = r["stores"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|v| v.as_str().unwrap_or("").to_string())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();
                if scope.is_empty() {
                    return Err(Error::new(
                        "InvalidAccessError",
                        "IDBDatabase.transaction: Empty scope passed in",
                    ));
                }
                let data = Self::load(store, origin, &conn.name)?;
                for name in &scope {
                    if data["stores"].get(name).is_none() {
                        return Err(Error::new(
                            "NotFoundError",
                            format!(
                                "IDBDatabase.transaction: '{name}' is not a known object store name"
                            ),
                        ));
                    }
                }
                let durability = r["durability"].as_str().unwrap_or("default");
                if !matches!(durability, "default" | "relaxed" | "strict") {
                    return Err(Error::new(
                        "TypeError",
                        format!(
                            "IDBDatabase.transaction: '{durability}' is not a valid value for enumeration IDBTransactionDurability."
                        ),
                    ));
                }
                let id = self.start_tx(db, scope, mode, data.clone(), None);
                let mut out = self.tx_info(id, &data)?;
                out["durability"] = json!(durability);
                Ok(out)
            }
            "schema" => self.schema(r),
            "store" | "index" => {
                let t = self.tx(number(r, "tx"))?;
                if t.finished {
                    return fail("InvalidStateError");
                }
                let data = t
                    .data
                    .clone()
                    .unwrap_or(Self::load(store, origin, &t.name)?);
                if t.mode != "versionchange" && !t.scope.contains(&text(r, "store").to_string()) {
                    return fail("NotFoundError");
                }
                let s = data["stores"]
                    .get(text(r, "store"))
                    .ok_or_else(|| Error::named("NotFoundError"))?;
                if text(r, "call") == "store" {
                    Ok(store_info(text(r, "store"), s))
                } else {
                    let i = s["indexes"]
                        .get(text(r, "name"))
                        .ok_or_else(|| Error::named("NotFoundError"))?;
                    Ok(index_info(text(r, "name"), i))
                }
            }
            "request" | "cursor" => {
                self.validate_request(r, store, origin)?;
                if !self.activate(number(r, "tx"), store, origin)? {
                    Ok(self.enqueue(r.clone()))
                } else {
                    self.execute(r, store, origin)
                }
            }
            "commit" => {
                let id = number(r, "tx");
                if !self.ready(id) {
                    Ok(self.enqueue(r.clone()))
                } else {
                    self.finish(id, false, store, origin)
                }
            }
            "abort" => self.finish(number(r, "tx"), true, store, origin),
            "result" => {
                self.pump(store, origin);
                let id = number(r, "id");
                let p = self
                    .pending
                    .get_mut(&id)
                    .ok_or_else(|| Error::named("InvalidStateError"))?;
                if let Some(result) = p.result.take() {
                    self.pending.remove(&id);
                    Ok(result)
                } else {
                    Ok(json!({"pending":id}))
                }
            }
            _ => Err(Error::new(
                "UnknownError",
                format!("IndexedDB: unknown call {}", text(r, "call")),
            )),
        }
    }
}
