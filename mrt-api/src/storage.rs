// localStorage, as Gecko's LocalStorage (dom/localstorage, LSNG) answers it, on the partition's
// SQLite file: one store per origin (bridge.js sends the window's origin), seeded from the test
// profile at the origin's first call. The quota is LSSnapshot::SetItem's and Datastore::UpdateUsage's:
// an item costs its value's UTF-16 length, and its key's once; an origin holds at most
// dom.storage.default_quota (5 MB) of them. Keys are in insertion order, as the runtime's
// sessionStorage (GAPS B49); Gecko's are in its hash table's.
//
// A request: {area: "localStorage", call, args, origin} (bridge.js has converted the arguments:
// strings, and an unsigned long for key()). The replies are the bridge tab's: setItem false when
// the quota is exceeded (the runtime throws QuotaExceededError), getItem and key null when absent.

use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use crate::store::Store;

const ORIGIN_LIMIT: i64 = 5 * 1024 * 1024;

/// (a lone surrogate counts one: surrogates.rs keeps it for localStorage)
fn utf16_len(s: &str) -> i64 {
    crate::surrogates::utf16_len(s) as i64
}

pub fn handle(req: &Value, store: &Store) -> Result<Value, String> {
    let origin = req.get("origin").and_then(Value::as_str).ok_or("mrt-api: storage without an origin")?;
    store.seed_local_storage(origin);
    let args = req.get("args").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let arg = |i: usize| args.get(i).and_then(Value::as_str).unwrap_or("").to_string();
    let db = &store.conn;
    let err = |e: rusqlite::Error| format!("mrt-api: localStorage: {e}");
    let item = |key: &str| -> Result<Option<String>, String> {
        db.query_row("SELECT value FROM ls WHERE origin = ?1 AND key = ?2", params![origin, key], |r| r.get(0))
            .optional()
            .map_err(err)
    };
    match req.get("call").and_then(Value::as_str).unwrap_or("") {
        "length" => {
            let n: i64 = db.query_row("SELECT count(*) FROM ls WHERE origin = ?1", [origin], |r| r.get(0)).map_err(err)?;
            Ok(json!(n))
        }
        "names" => {
            let mut st = db.prepare_cached("SELECT key FROM ls WHERE origin = ?1 ORDER BY seq").map_err(err)?;
            let names: Vec<String> = st.query_map([origin], |r| r.get(0)).map_err(err)?.flatten().collect();
            Ok(json!(names))
        }
        // (B222) the origin's items in order, for the page's snapshot of a task (Gecko's LSSnapshot
        // prefill), when their keys and values come to at most `limit` UTF-16 units; null otherwise
        "items" => {
            let limit = args.first().and_then(Value::as_i64).unwrap_or(0);
            let usage: i64 = db.query_row("SELECT coalesce(sum(len), 0) FROM ls WHERE origin = ?1", [origin], |r| r.get(0)).map_err(err)?;
            if usage > limit {
                return Ok(Value::Null);
            }
            let mut st = db.prepare_cached("SELECT key, value FROM ls WHERE origin = ?1 ORDER BY seq").map_err(err)?;
            let items: Vec<Value> = st.query_map([origin], |r| Ok(json!([r.get::<_, String>(0)?, r.get::<_, String>(1)?]))).map_err(err)?.flatten().collect();
            Ok(json!(items))
        }
        "key" => {
            let i = args.first().and_then(Value::as_u64).unwrap_or(0) as i64;
            let k: Option<String> = db
                .query_row("SELECT key FROM ls WHERE origin = ?1 ORDER BY seq LIMIT 1 OFFSET ?2", params![origin, i], |r| r.get(0))
                .optional()
                .map_err(err)?;
            Ok(k.map(Value::String).unwrap_or(Value::Null))
        }
        "getItem" => Ok(item(&arg(0))?.map(Value::String).unwrap_or(Value::Null)),
        "setItem" => {
            let (key, value) = (arg(0), arg(1));
            let old = item(&key)?;
            if old.as_deref() == Some(value.as_str()) {
                return Ok(json!(true));
            }
            let mut delta = utf16_len(&value) - old.as_deref().map(utf16_len).unwrap_or(0);
            if old.is_none() {
                delta += utf16_len(&key);
            }
            let usage: i64 = db.query_row("SELECT coalesce(sum(len), 0) FROM ls WHERE origin = ?1", [origin], |r| r.get(0)).map_err(err)?;
            if usage + delta > ORIGIN_LIMIT {
                return Ok(json!(false));
            }
            let len = utf16_len(&key) + utf16_len(&value);
            if old.is_some() {
                // an overwrite keeps the key's place
                db.execute("UPDATE ls SET value = ?3, len = ?4 WHERE origin = ?1 AND key = ?2", params![origin, key, value, len]).map_err(err)?;
            } else {
                db.execute("INSERT INTO ls (origin, key, value, len) VALUES (?1, ?2, ?3, ?4)", params![origin, key, value, len]).map_err(err)?;
            }
            Ok(json!(true))
        }
        "removeItem" => {
            db.execute("DELETE FROM ls WHERE origin = ?1 AND key = ?2", params![origin, arg(0)]).map_err(err)?;
            Ok(Value::Null)
        }
        "clear" => {
            db.execute("DELETE FROM ls WHERE origin = ?1", [origin]).map_err(err)?;
            Ok(Value::Null)
        }
        call => Err(format!("mrt-api: localStorage has no call {call:?}")),
    }
}
