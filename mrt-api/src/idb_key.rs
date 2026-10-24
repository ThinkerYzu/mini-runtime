//! IndexedDB keys: Gecko dom/indexedDB/Key.cpp type order and UTF-16 strings.
use crate::idb::{Error, Result, fail};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::cmp::Ordering;

#[derive(Clone, Debug)]
pub enum Key {
    Number(f64),
    Date(f64),
    String(String),
    Binary(Vec<u8>),
    Array(Vec<Key>),
}
impl Key {
    fn rank(&self) -> u8 {
        match self {
            Self::Number(_) => 0,
            Self::Date(_) => 1,
            Self::String(_) => 2,
            Self::Binary(_) => 3,
            Self::Array(_) => 4,
        }
    }
    pub fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) | (Self::Date(a), Self::Date(b)) => {
                a.partial_cmp(b).unwrap()
            }
            (Self::String(a), Self::String(b)) => a.encode_utf16().cmp(b.encode_utf16()),
            (Self::Binary(a), Self::Binary(b)) => a.cmp(b),
            (Self::Array(a), Self::Array(b)) => a
                .iter()
                .zip(b)
                .map(|(a, b)| a.cmp(b))
                .find(|o| !o.is_eq())
                .unwrap_or(a.len().cmp(&b.len())),
            _ => self.rank().cmp(&other.rank()),
        }
    }
    pub fn encoded(&self) -> Value {
        fn encode(k: &Key, id: &mut u64) -> Value {
            let n = *id;
            *id += 1;
            match k {
                Key::Number(x) if !x.is_finite() => json!([
                    "n",
                    if x.is_sign_positive() {
                        "Infinity"
                    } else {
                        "-Infinity"
                    }
                ]),
                Key::Number(x) => json!(if *x == 0.0 { 0.0 } else { *x }),
                Key::Date(x) => json!(["d", n, x]),
                Key::String(x) => json!(x),
                Key::Binary(x) => json!(["ab", n, STANDARD.encode(x)]),
                Key::Array(x) => {
                    json!(["a", n, x.iter().map(|x| encode(x, id)).collect::<Vec<_>>()])
                }
            }
        }
        encode(self, &mut 0)
    }
}
pub fn undefined(v: &Value) -> bool {
    v.as_array().is_some_and(|a| a.first() == Some(&json!("u")))
}
pub fn resolve<'a>(v: &'a Value, root: &'a Value) -> Option<&'a Value> {
    fn find(v: &Value, id: u64) -> Option<&Value> {
        if let Some(a) = v.as_array() {
            if a.get(1).and_then(Value::as_u64) == Some(id)
                && matches!(
                    a.first().and_then(Value::as_str),
                    Some("a" | "o" | "d" | "x" | "ab" | "m" | "s" | "w" | "e" | "ta" | "bl")
                )
            {
                return Some(v);
            }
            for x in a {
                if let Some(v) = find(x, id) {
                    return Some(v);
                }
            }
        } else if let Some(o) = v.as_object() {
            for x in o.values() {
                if let Some(v) = find(x, id) {
                    return Some(v);
                }
            }
        }
        None
    }
    if v[0] == "r" {
        find(root, v[1].as_u64()?)
    } else {
        Some(v)
    }
}
pub fn parse(v: &Value) -> Result<Key> {
    parse_root(v, v, 0)
}
pub fn parse_root(v: &Value, root: &Value, depth: u32) -> Result<Key> {
    if depth > 64 {
        return fail("DataError");
    }
    let v = resolve(v, root).ok_or_else(|| Error::named("DataError"))?;
    if let Some(n) = v.as_f64() {
        return Ok(Key::Number(n));
    }
    if let Some(s) = v.as_str() {
        return Ok(Key::String(s.into()));
    }
    match v[0].as_str().unwrap_or("") {
        "n" => match v[1].as_str() {
            Some("-0") => Ok(Key::Number(0.0)),
            Some("Infinity") => Ok(Key::Number(f64::INFINITY)),
            Some("-Infinity") => Ok(Key::Number(f64::NEG_INFINITY)),
            _ => fail("DataError"),
        },
        "d" => v[2]
            .as_f64()
            .filter(|n| n.is_finite())
            .map(Key::Date)
            .ok_or_else(|| Error::named("DataError")),
        "ab" | "ta" => {
            let b = if v[0] == "ab" { &v[2] } else { &v[3] };
            STANDARD
                .decode(b.as_str().unwrap_or(""))
                .map(Key::Binary)
                .map_err(|_| Error::named("DataError"))
        }
        "a" => Ok(Key::Array(
            v[2].as_array()
                .ok_or_else(|| Error::named("DataError"))?
                .iter()
                .map(|v| parse_root(v, root, depth + 1))
                .collect::<Result<_>>()?,
        )),
        _ => fail("DataError"),
    }
}
#[derive(Clone)]
pub struct Range {
    pub lower: Option<Key>,
    pub upper: Option<Key>,
    pub lo: bool,
    pub hi: bool,
}
impl Range {
    pub fn arg(arg: Option<&Value>, optional: bool) -> Result<Self> {
        let any = Self {
            lower: None,
            upper: None,
            lo: false,
            hi: false,
        };
        let Some(a) = arg else {
            return if optional { Ok(any) } else { fail("DataError") };
        };
        if let Some(r) = a.get("range").and_then(Value::as_array) {
            let lower = if undefined(&r[0]) {
                None
            } else {
                Some(parse(&r[0])?)
            };
            let upper = if undefined(&r[1]) {
                None
            } else {
                Some(parse(&r[1])?)
            };
            let x = Self {
                lower,
                upper,
                lo: r[2] == true,
                hi: r[3] == true,
            };
            x.validate()?;
            return Ok(x);
        }
        if optional && (a["v"].is_null() || undefined(&a["v"])) {
            return Ok(any);
        }
        let k = parse(&a["v"])?;
        Ok(Self {
            lower: Some(k.clone()),
            upper: Some(k),
            lo: false,
            hi: false,
        })
    }
    pub fn validate(&self) -> Result<()> {
        if let (Some(l), Some(u)) = (&self.lower, &self.upper) {
            let c = l.cmp(u);
            if c.is_gt() || (c.is_eq() && (self.lo || self.hi)) {
                return fail("DataError");
            }
        }
        Ok(())
    }
    pub fn contains(&self, k: &Key) -> bool {
        self.lower.as_ref().is_none_or(|x| {
            let c = k.cmp(x);
            c.is_gt() || (!self.lo && c.is_eq())
        }) && self.upper.as_ref().is_none_or(|x| {
            let c = k.cmp(x);
            c.is_lt() || (!self.hi && c.is_eq())
        })
    }
}

pub fn valid_path(path: &Value) -> bool {
    fn one(s: &str) -> bool {
        s.is_empty()
            || s.split('.').all(|p| {
                let mut c = p.chars();
                c.next()
                    .is_some_and(|c| c == '_' || c == '$' || c.is_alphabetic())
                    && c.all(|c| {
                        c == '_'
                            || c == '$'
                            || c == '\u{200c}'
                            || c == '\u{200d}'
                            || c.is_alphanumeric()
                    })
            })
    }
    path.as_str()
        .map(one)
        .or_else(|| {
            path.as_array()
                .map(|a| a.iter().all(|v| v.as_str().is_some_and(one)))
        })
        .unwrap_or(false)
}
pub fn lookup(value: &Value, path: &str) -> Option<Value> {
    if path.is_empty() {
        return Some(value.clone());
    }
    let mut current = value.clone();
    for name in path.split('.') {
        let node = resolve(&current, value)?;
        current = match node[0].as_str() {
            Some("o") => node[2].get(name)?.clone(),
            Some("a") if name == "length" => json!(node[2].as_array()?.len()),
            Some("a") => node[2].get(name.parse::<usize>().ok()?)?.clone(),
            Some("bl") if name == "size" => json!(STANDARD.decode(node[2].as_str()?).ok()?.len()),
            Some("bl") if name == "type" => node[3].clone(),
            Some("bl") if name == "name" => node[4].clone(),
            Some("bl") if name == "lastModified" => node[5].clone(),
            _ if name == "length" && node.is_string() => {
                json!(node.as_str()?.encode_utf16().count())
            }
            _ => return None,
        };
    }
    Some(current)
}
pub fn extract(value: &Value, path: &Value) -> Option<Key> {
    if let Some(paths) = path.as_array() {
        return Some(Key::Array(
            paths
                .iter()
                .map(|p| extract(value, p))
                .collect::<Option<_>>()?,
        ));
    }
    parse_root(&lookup(value, path.as_str()?)?, value, 0).ok()
}
pub fn inject(value: &mut Value, path: &str, key: &Key) -> Result<()> {
    // Generated numeric inline keys need no new graph node. Missing intermediate
    // objects use fresh codec ids, preserving existing references.
    fn max_id(v: &Value) -> u64 {
        match v {
            Value::Array(a) => a
                .iter()
                .map(max_id)
                .max()
                .unwrap_or(0)
                .max(a.get(1).and_then(Value::as_u64).unwrap_or(0)),
            Value::Object(o) => o.values().map(max_id).max().unwrap_or(0),
            _ => 0,
        }
    }
    let mut next = max_id(value) + 1;
    fn put(v: &mut Value, parts: &[&str], key: &Key, next: &mut u64) -> Result<()> {
        if v[0] != "o" || parts.is_empty() {
            return fail("DataError");
        }
        if parts.len() == 1 {
            v[2][parts[0]] = key.encoded();
            return Ok(());
        }
        if v[2].get(parts[0]).is_none() {
            v[2][parts[0]] = json!(["o", *next, {}]);
            *next += 1;
        }
        put(&mut v[2][parts[0]], &parts[1..], key, next)
    }
    put(value, &path.split('.').collect::<Vec<_>>(), key, &mut next)
}

pub fn truthy(v: &Value) -> bool {
    if undefined(v) || v.is_null() {
        return false;
    }
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::String(s) => !s.is_empty(),
        _ => !(v[0] == "n" && (v[1] == "NaN" || v[1] == "-0")),
    }
}
