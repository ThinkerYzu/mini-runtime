use super::*;

fn get_store<'a>(data: &'a Value, name: &str) -> Result<&'a Value> {
    data["stores"]
        .get(name)
        .ok_or_else(|| Error::named("NotFoundError"))
}
fn remove(map: &mut Value, name: &str) -> Value {
    map.as_object_mut()
        .and_then(|m| m.shift_remove(name))
        .unwrap_or(Value::Null)
}
fn array(values: Vec<Value>) -> Value {
    json!(["a", 0, values])
}
fn undef() -> Value {
    json!(["u"])
}
fn args(r: &Value) -> Vec<Value> {
    r["args"].as_array().cloned().unwrap_or_default()
}
fn index_keys(record: &Value, index: &Value) -> Vec<Key> {
    let path = &index["keyPath"];
    if index["multiEntry"] == true {
        if let Some(Key::Array(items)) = key::extract(record, path) {
            let mut keys: Vec<Key> = vec![];
            for k in items {
                if !keys.iter().any(|x| x.cmp(&k).is_eq()) {
                    keys.push(k)
                }
            }
            return keys;
        }
        // Invalid members of a multiEntry array are individually ignored.
        if let Some(p) = path.as_str() {
            if record[0] == "o" {
                if let Some(v) = record[2].get(p) {
                    if v[0] == "a" {
                        let mut keys: Vec<Key> = vec![];
                        for v in v[2].as_array().into_iter().flatten() {
                            if let Ok(k) = key::parse_root(v, record, 0) {
                                if !keys.iter().any(|x| x.cmp(&k).is_eq()) {
                                    keys.push(k)
                                }
                            }
                        }
                        return keys;
                    }
                }
            }
        }
    }
    key::extract(record, path).into_iter().collect()
}
#[derive(Clone)]
struct Entry {
    key: Key,
    primary: Key,
    value: Value,
}
fn entries(s: &Value, index: Option<&str>, range: &Range) -> Result<Vec<Entry>> {
    let i = match index {
        Some(n) => Some(
            s["indexes"]
                .get(n)
                .ok_or_else(|| Error::named("NotFoundError"))?,
        ),
        None => None,
    };
    let mut rows = vec![];
    for record in s["records"].as_array().into_iter().flatten() {
        let primary = key::parse(&record["key"])?;
        let keys = match i {
            Some(i) => index_keys(&record["value"], i),
            None => vec![primary.clone()],
        };
        for k in keys {
            if range.contains(&k) {
                rows.push(Entry {
                    key: k,
                    primary: primary.clone(),
                    value: record["value"].clone(),
                })
            }
        }
    }
    rows.sort_by(|a, b| a.key.cmp(&b.key).then(a.primary.cmp(&b.primary)));
    Ok(rows)
}
fn duplicate(s: &Value, index: &Value) -> bool {
    let mut seen: Vec<Key> = vec![];
    for r in s["records"].as_array().into_iter().flatten() {
        for k in index_keys(&r["value"], index) {
            if seen.iter().any(|x| x.cmp(&k).is_eq()) {
                return true;
            }
            seen.push(k)
        }
    }
    false
}
impl State {
    pub(super) fn schema(&mut self, r: &Value) -> Result<Value> {
        let id = number(r, "tx");
        let t = self.tx(id)?;
        if t.finished || r["inactive"] == true {
            return fail("TransactionInactiveError");
        }
        if t.mode != "versionchange" {
            return fail("InvalidStateError");
        }
        let method = text(r, "method");
        let name = text(r, "name");
        let store_name = text(r, "store");
        let t = self.txs.get_mut(&id).unwrap();
        let data = t
            .data
            .as_mut()
            .ok_or_else(|| Error::named("TransactionInactiveError"))?;
        match method {
            "createObjectStore" => {
                let opt = &r["options"];
                let path = opt.get("keyPath").cloned().unwrap_or(Value::Null);
                if !path.is_null() && !key::valid_path(&path) {
                    return fail("SyntaxError");
                }
                if opt["autoIncrement"] == true && (path == "" || path.is_array()) {
                    return fail("InvalidAccessError");
                }
                if data["stores"].get(name).is_some() {
                    let n = data["stores"]
                        .as_object()
                        .unwrap()
                        .keys()
                        .position(|n| n == name)
                        .unwrap_or(0);
                    return Err(Error::new(
                        "ConstraintError",
                        format!(
                            "IDBDatabase.createObjectStore: Object store named '{name}' already exists at index '{n}'"
                        ),
                    ));
                }
                let s = json!({"keyPath":path,"autoIncrement":opt["autoIncrement"]==true,"indexes":{},"next":1,"records":[]});
                let info = store_info(name, &s);
                data["stores"][name] = s;
                t.scope = names(&data["stores"]);
                Ok(info)
            }
            "deleteObjectStore" => {
                if data["stores"].get(name).is_none() {
                    return fail("NotFoundError");
                }
                remove(&mut data["stores"], name);
                t.scope = names(&data["stores"]);
                Ok(Value::Null)
            }
            "renameStore" => {
                if data["stores"].get(store_name).is_none() {
                    return fail("InvalidStateError");
                }
                if name != store_name && data["stores"].get(name).is_some() {
                    return fail("ConstraintError");
                }
                let s = remove(&mut data["stores"], store_name);
                data["stores"][name] = s;
                t.scope = names(&data["stores"]);
                Ok(Value::Null)
            }
            "createIndex" => {
                let s = get_store(data, store_name)?;
                if !key::valid_path(&r["keyPath"]) {
                    return fail("SyntaxError");
                }
                if r["keyPath"].is_array() && r["options"]["multiEntry"] == true {
                    return fail("InvalidAccessError");
                }
                if s["indexes"].get(name).is_some() {
                    return fail("ConstraintError");
                }
                let index = json!({"keyPath":r["keyPath"],"unique":r["options"]["unique"]==true,"multiEntry":r["options"]["multiEntry"]==true});
                if index["unique"] == true && duplicate(s, &index) {
                    return fail("ConstraintError");
                }
                let info = index_info(name, &index);
                data["stores"][store_name]["indexes"][name] = index;
                Ok(info)
            }
            "deleteIndex" => {
                let s = get_store(data, store_name)?;
                if s["indexes"].get(name).is_none() {
                    return fail("NotFoundError");
                }
                remove(&mut data["stores"][store_name]["indexes"], name);
                Ok(Value::Null)
            }
            "renameIndex" => {
                let old = text(r, "index");
                let s = get_store(data, store_name)?;
                if s["indexes"].get(old).is_none() {
                    return fail("InvalidStateError");
                }
                if old != name && s["indexes"].get(name).is_some() {
                    return fail("ConstraintError");
                }
                let index = remove(&mut data["stores"][store_name]["indexes"], old);
                data["stores"][store_name]["indexes"][name] = index;
                Ok(Value::Null)
            }
            _ => fail("UnknownError"),
        }
    }
    fn target(&self, r: &Value) -> Result<(String, Option<String>, Option<Cursor>)> {
        if let Some(id) = r["cursor"].as_u64() {
            let c = self
                .cursors
                .get(&id)
                .ok_or_else(|| Error::named("InvalidStateError"))?;
            if c.tx != number(r, "tx") {
                return fail("InvalidStateError");
            }
            Ok((c.store.clone(), c.index.clone(), Some(c.clone())))
        } else {
            Ok((
                text(r, "store").into(),
                r["index"].as_str().map(str::to_string),
                None,
            ))
        }
    }
    pub(super) fn validate_request(&self, r: &Value, store: &Store, origin: &str) -> Result<()> {
        let t = self.tx(number(r, "tx"))?;
        if r["inactive"] == true || t.finished {
            return fail("TransactionInactiveError");
        }
        let (name, index, cursor) = self.target(r)?;
        if t.mode != "versionchange" && !t.scope.contains(&name) {
            return fail("NotFoundError");
        }
        let data = t
            .data
            .clone()
            .unwrap_or(Self::load(store, origin, &t.name)?);
        let s = get_store(&data, &name)?;
        if let Some(i) = &index {
            if s["indexes"].get(i).is_none() {
                return fail("NotFoundError");
            }
        }
        let method = text(r, "method");
        let a = args(r);
        if matches!(method, "put" | "add" | "update" | "delete" | "clear") && t.mode == "readonly" {
            return fail("ReadOnlyError");
        }
        if text(r, "call") == "cursor" {
            let c = cursor.ok_or_else(|| Error::named("InvalidStateError"))?;
            if method == "advance" {
                let n = a
                    .first()
                    .map(|v| crate::js::to_number(&v["v"]))
                    .unwrap_or(0.0);
                if !n.is_finite() || n.trunc() < 1.0 {
                    return Err(Error::new(
                        "TypeError",
                        "IDBCursor.advance: 0 (value of argument 1) is outside the valid range of 1 - 4294967295.",
                    ));
                }
            }
            if method == "continuePrimaryKey"
                && (c.index.is_none() || c.direction.ends_with("unique"))
            {
                return fail("InvalidAccessError");
            }
            if let Some(arg) = a.first().filter(|a| !key::undefined(&a["v"])) {
                let k = key::parse(&arg["v"])?;
                let cmp = k.cmp(&c.key);
                let forward = !c.direction.starts_with("prev");
                if method == "continue" && (if forward { !cmp.is_gt() } else { !cmp.is_lt() }) {
                    return fail("DataError");
                }
                if method == "continuePrimaryKey" {
                    let pk = key::parse(&a.get(1).unwrap_or(&Value::Null)["v"])?;
                    let cmp = cmp.then(pk.cmp(&c.primary));
                    if if forward { !cmp.is_gt() } else { !cmp.is_lt() } {
                        return fail("DataError");
                    }
                }
            }
            return Ok(());
        }
        if matches!(method, "put" | "add" | "update") {
            if a.is_empty() {
                return Err(Error::new(
                    "TypeError",
                    format!(
                        "IDBObjectStore.{method}: At least 1 argument required, but only 0 passed"
                    ),
                ));
            }
            if a[0]["fail"] == true {
                return fail("DataCloneError");
            }
            let inline = !s["keyPath"].is_null();
            if inline && method != "update" && a.get(1).is_some_and(|a| !key::undefined(&a["v"])) {
                return fail("DataError");
            }
            let key = if method == "update" && !inline {
                cursor.as_ref().map(|c| c.primary.clone())
            } else if inline {
                key::extract(&a[0]["v"], &s["keyPath"])
            } else {
                a.get(1)
                    .filter(|a| !key::undefined(&a["v"]))
                    .map(|a| key::parse(&a["v"]))
                    .transpose()?
            };
            if key.is_none() {
                if s["autoIncrement"] != true {
                    return fail("DataError");
                }
                // An existing invalid inline key is not a missing generator target.
                if inline {
                    if let Some(existing) = s["keyPath"]
                        .as_str()
                        .and_then(|p| key::lookup(&a[0]["v"], p))
                    {
                        if !key::undefined(&existing) {
                            return fail("DataError");
                        }
                    }
                    let mut v = a[0]["v"].clone();
                    key::inject(
                        &mut v,
                        s["keyPath"]
                            .as_str()
                            .ok_or_else(|| Error::named("DataError"))?,
                        &Key::Number(1.0),
                    )?;
                }
            }
            if let (Some(c), Some(k)) = (&cursor, &key) {
                if !c.primary.cmp(k).is_eq() {
                    return fail("DataError");
                }
            }
        } else if matches!(
            method,
            "get"
                | "getKey"
                | "delete"
                | "getAll"
                | "getAllKeys"
                | "count"
                | "openCursor"
                | "openKeyCursor"
        ) && cursor.is_none()
        {
            Range::arg(a.first(), !matches!(method, "get" | "getKey" | "delete"))?;
            if matches!(method, "getAll" | "getAllKeys") {
                if let Some(v) = a.get(1).filter(|v| !key::undefined(&v["v"])) {
                    let n = crate::js::to_number(&v["v"]);
                    if !n.is_finite() || n.trunc() < 0.0 || n.trunc() > 4294967295.0 {
                        return Err(Error::new(
                            "TypeError",
                            "IndexedDB count is outside the unsigned long range.",
                        ));
                    }
                }
            }
            if method.starts_with("open") {
                let dir = a.get(1).and_then(|a| a["v"].as_str()).unwrap_or("next");
                if !matches!(dir, "next" | "nextunique" | "prev" | "prevunique") {
                    return Err(Error::new(
                        "TypeError",
                        format!("'{dir}' is not a valid value for enumeration IDBCursorDirection."),
                    ));
                }
            }
        }
        Ok(())
    }
    pub(super) fn execute(&mut self, r: &Value, store: &Store, origin: &str) -> Result<Value> {
        let tx = number(r, "tx");
        self.activate(tx, store, origin)?;
        if self.tx(tx)?.finished {
            return fail("TransactionInactiveError");
        }
        let (name, index, cursor) = self.target(r)?;
        let a = args(r);
        let method = text(r, "method");
        let data = self.tx(tx)?.data.as_ref().unwrap();
        let s = get_store(data, &name)?;
        if text(r, "call") == "cursor" {
            let c = cursor.unwrap();
            let mut rows = entries(s, c.index.as_deref(), &c.range)?;
            let prev = c.direction.starts_with("prev");
            let unique = c.direction.ends_with("unique");
            if unique {
                rows.dedup_by(|a, b| a.key.cmp(&b.key).is_eq());
            }
            if prev {
                rows.reverse()
            }
            rows.retain(|e| {
                let cmp = if unique {
                    e.key.cmp(&c.key)
                } else {
                    e.key.cmp(&c.key).then(e.primary.cmp(&c.primary))
                };
                if prev { cmp.is_lt() } else { cmp.is_gt() }
            });
            let advance = if method == "advance" {
                a.first()
                    .map(|v| crate::js::to_number(&v["v"]) as usize)
                    .unwrap_or(1)
                    .max(1)
            } else {
                1
            };
            if method != "advance" {
                if let Some(v) = a.first().filter(|a| !key::undefined(&a["v"])) {
                    let k = key::parse(&v["v"])?;
                    let p = if method == "continuePrimaryKey" {
                        Some(key::parse(&a[1]["v"])?)
                    } else {
                        None
                    };
                    rows.retain(|e| {
                        let cmp = e.key.cmp(&k).then(
                            p.as_ref()
                                .map(|p| e.primary.cmp(p))
                                .unwrap_or(std::cmp::Ordering::Equal),
                        );
                        if prev { !cmp.is_gt() } else { !cmp.is_lt() }
                    });
                }
            }
            return Ok(self.cursor_reply(number(r, "cursor"), rows.into_iter().nth(advance - 1)));
        }
        if matches!(method, "put" | "add" | "update") {
            let mut value = a[0]["v"].clone();
            let inline = !s["keyPath"].is_null();
            let mut k = if method == "update" && !inline {
                cursor.as_ref().map(|c| c.primary.clone())
            } else if inline {
                key::extract(&value, &s["keyPath"])
            } else {
                a.get(1)
                    .filter(|a| !key::undefined(&a["v"]))
                    .map(|a| key::parse(&a["v"]))
                    .transpose()?
            };
            let mut next = number(s, "next").max(1);
            if k.is_none() {
                if next > 9007199254740992 {
                    return Ok(json!({"error":Error::named("ConstraintError").value()}));
                }
                let generated = Key::Number(next as f64);
                if inline {
                    key::inject(&mut value, s["keyPath"].as_str().unwrap_or(""), &generated)?
                }
                k = Some(generated);
                next += 1;
            }
            let k = k.unwrap();
            if s["autoIncrement"] == true {
                if let Key::Number(n) = k {
                    if n >= next as f64 {
                        next = if n >= 9007199254740992.0 {
                            9007199254740993
                        } else {
                            n.floor() as u64 + 1
                        };
                    }
                }
            }
            let old = s["records"]
                .as_array()
                .unwrap()
                .iter()
                .position(|r| key::parse(&r["key"]).is_ok_and(|x| x.cmp(&k).is_eq()));
            if method == "add" && old.is_some() {
                return Ok(json!({"error":Error::named("ConstraintError").value()}));
            }
            for i in s["indexes"]
                .as_object()
                .into_iter()
                .flat_map(|o| o.values())
                .filter(|i| i["unique"] == true)
            {
                let keys = index_keys(&value, i);
                for (n, r) in s["records"].as_array().unwrap().iter().enumerate() {
                    if Some(n) == old {
                        continue;
                    }
                    if index_keys(&r["value"], i)
                        .iter()
                        .any(|existing| keys.iter().any(|k| k.cmp(existing).is_eq()))
                    {
                        return Ok(json!({"error":Error::named("ConstraintError").value()}));
                    }
                }
            }
            let s = &mut self.txs.get_mut(&tx).unwrap().data.as_mut().unwrap()["stores"][&name];
            let record = json!({"key":k.encoded(),"value":value});
            let rows = s["records"].as_array_mut().unwrap();
            if let Some(n) = old {
                rows[n] = record
            } else {
                rows.push(record)
            }
            s["next"] = json!(next);
            return Ok(json!({"result":k.encoded()}));
        }
        if method == "clear" {
            self.txs.get_mut(&tx).unwrap().data.as_mut().unwrap()["stores"][&name]["records"] =
                json!([]);
            return Ok(json!({"result":undef()}));
        }
        let range = if let Some(c) = &cursor {
            Range {
                lower: Some(c.primary.clone()),
                upper: Some(c.primary.clone()),
                lo: false,
                hi: false,
            }
        } else {
            Range::arg(a.first(), !matches!(method, "get" | "getKey" | "delete"))?
        };
        let mut rows = entries(
            s,
            if cursor.is_some() {
                None
            } else {
                index.as_deref()
            },
            &range,
        )?;
        if method == "delete" {
            self.txs.get_mut(&tx).unwrap().data.as_mut().unwrap()["stores"][&name]["records"]
                .as_array_mut()
                .unwrap()
                .retain(|r| !key::parse(&r["key"]).is_ok_and(|k| range.contains(&k)));
            return Ok(json!({"result":undef()}));
        }
        if matches!(method, "openCursor" | "openKeyCursor") {
            let direction = a
                .get(1)
                .and_then(|v| v["v"].as_str())
                .unwrap_or("next")
                .to_string();
            if direction.ends_with("unique") {
                rows.dedup_by(|a, b| a.key.cmp(&b.key).is_eq());
            }
            if direction.starts_with("prev") {
                rows.reverse()
            }
            let Some(first) = rows.into_iter().next() else {
                return Ok(json!({"result":null}));
            };
            let id = self.id();
            self.cursors.insert(
                id,
                Cursor {
                    tx,
                    store: name,
                    index,
                    range,
                    direction,
                    with_value: method == "openCursor",
                    key: first.key.clone(),
                    primary: first.primary.clone(),
                },
            );
            return Ok(self.cursor_reply(id, Some(first)));
        }
        let result = match method {
            "get" => rows.first().map(|r| r.value.clone()).unwrap_or_else(undef),
            "getKey" => rows
                .first()
                .map(|r| r.primary.encoded())
                .unwrap_or_else(undef),
            "count" => json!(rows.len()),
            "getAll" | "getAllKeys" => {
                let count = a
                    .get(1)
                    .map(|v| crate::js::to_number(&v["v"]) as usize)
                    .filter(|n| *n > 0)
                    .unwrap_or(usize::MAX);
                array(
                    rows.into_iter()
                        .take(count)
                        .map(|r| {
                            if method == "getAll" {
                                r.value
                            } else {
                                r.primary.encoded()
                            }
                        })
                        .collect(),
                )
            }
            _ => return fail("UnknownError"),
        };
        Ok(json!({"result":result}))
    }
    fn cursor_reply(&mut self, id: u64, row: Option<Entry>) -> Value {
        let Some(row) = row else {
            return json!({"result":null});
        };
        let c = self.cursors.get_mut(&id).unwrap();
        c.key = row.key.clone();
        c.primary = row.primary.clone();
        let mut out =
            json!({"cursor":id,"key":row.key.encoded(),"primaryKey":row.primary.encoded()});
        if c.with_value {
            out["value"] = row.value;
        }
        out
    }
}
