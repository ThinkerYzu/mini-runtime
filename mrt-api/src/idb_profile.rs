//! A Firefox profile's IndexedDB databases, copied once per origin and partition into the native
//! store (GAPS B168): at an origin's first idb call, each storage/default/<origin>/idb/*.sqlite of
//! API_PROFILE that has no native row of its name becomes one. Read as Gecko writes them: the
//! schema of dom/indexedDB/DBSchema.cpp, keys as dom/indexedDB/Key.cpp encodes them, and each
//! record a snappy-compressed SpiderMonkey structured clone (js/src/vm/StructuredClone.cpp), or for
//! a value over dom.indexedDB.dataThreshold a framed-snappy file in <base>.files/, with its Blobs'
//! bytes in that directory too. The clones become clone_codec.js JSON, the records' form here.
//!
//! What is not read: a locale-aware index (dropped, its store kept), encrypted (private browsing)
//! databases, and clone types the codec has no tag for (CryptoKey, ImageData, wasm modules and
//! other DOM objects): such a record is left out. Each is reported on stderr.
use super::key::Key;
use crate::store::Store;
use base64::{Engine, engine::general_purpose::STANDARD};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// Copies the profile's databases of `origin` into the store, the first time the origin asks.
pub fn seed(store: &Store, origin: &str) {
    let done = store
        .conn
        .query_row("SELECT 1 FROM idb_seeded WHERE origin=?1", [origin], |_| Ok(()))
        .optional()
        .ok()
        .flatten()
        .is_some();
    if done {
        return;
    }
    let _ = store.conn.execute("INSERT INTO idb_seeded VALUES (?1)", [origin]);
    for dir in store.profile_origin_dirs(origin) {
        let Ok(files) = std::fs::read_dir(dir.join("idb")) else { continue };
        let mut paths: Vec<PathBuf> = files
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "sqlite"))
            .collect();
        paths.sort();
        for path in paths {
            match read_database(&path) {
                Ok(data) => {
                    let name = data["name"].as_str().unwrap_or("").to_string();
                    let _ = store.conn.execute(
                        "INSERT OR IGNORE INTO idb_databases VALUES (?1,?2,?3)",
                        params![origin, name, data.to_string()],
                    );
                }
                Err(e) => eprintln!("mrt-api: IndexedDB profile {}: {e}", path.display()),
            }
        }
    }
}

/// One of Gecko's database files as the native store's JSON: {name, version, stores: {name:
/// {keyPath, autoIncrement, indexes, next, records: [{key, value}]}}}. Read from a copy, with its
/// WAL, so that the profile's file is not touched.
fn read_database(path: &Path) -> Result<Value, String> {
    let tmp = std::env::temp_dir().join(format!(
        "mrt-api-idb-{}-{}",
        std::process::id(),
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    let wal = |p: &Path| PathBuf::from(format!("{}-wal", p.display()));
    std::fs::copy(path, &tmp).map_err(|e| e.to_string())?;
    let _ = std::fs::copy(wal(path), wal(&tmp));
    let result = (|| {
        let db = Connection::open(&tmp).map_err(|e| e.to_string())?;
        read_tables(&db, &path.with_extension("files"))
    })();
    for p in [tmp.clone(), wal(&tmp), PathBuf::from(format!("{}-shm", tmp.display()))] {
        let _ = std::fs::remove_file(p);
    }
    result
}

fn read_tables(db: &Connection, files: &Path) -> Result<Value, String> {
    let e = |e: rusqlite::Error| e.to_string();
    let (name, version): (String, i64) = db
        .query_row("SELECT name, version FROM database", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(e)?;
    let mut stores = Map::new();
    let mut st = db
        .prepare("SELECT id, auto_increment, name, key_path FROM object_store ORDER BY id")
        .map_err(e)?;
    let rows = st
        .query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?))
        })
        .map_err(e)?;
    for row in rows {
        let (id, next, store_name, path) = row.map_err(e)?;
        let mut indexes = Map::new();
        let mut ist = db
            .prepare(
                "SELECT name, key_path, unique_index, multientry, locale FROM object_store_index
                 WHERE object_store_id=?1 ORDER BY id",
            )
            .map_err(e)?;
        let irows = ist
            .query_map([id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })
            .map_err(e)?;
        for irow in irows {
            let (index_name, index_path, unique, multi, locale) = irow.map_err(e)?;
            if locale.is_some() {
                eprintln!("mrt-api: IndexedDB profile: {name}/{store_name}: locale-aware index {index_name} not imported");
                continue;
            }
            indexes.insert(
                index_name,
                json!({"keyPath":key_path(Some(&index_path)),"unique":unique!=0,"multiEntry":multi!=0}),
            );
        }
        let mut records = vec![];
        let mut rst = db
            .prepare("SELECT key, data, file_ids FROM object_data WHERE object_store_id=?1 ORDER BY key")
            .map_err(e)?;
        let rrows = rst
            .query_map([id], |r| {
                Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, rusqlite::types::Value>(1)?, r.get::<_, Option<String>>(2)?))
            })
            .map_err(e)?;
        for rrow in rrows {
            let (key, data, file_ids) = rrow.map_err(e)?;
            let record = decode_key(&key).and_then(|k| {
                let tokens: Vec<String> = file_ids.unwrap_or_default().split(' ').filter(|t| !t.is_empty()).map(String::from).collect();
                let clone = match data {
                    rusqlite::types::Value::Blob(b) => snap::raw::Decoder::new().decompress_vec(&b).map_err(|e| e.to_string())?,
                    // over the threshold: the low 32 bits index file_ids, a '.' file of framed snappy
                    rusqlite::types::Value::Integer(n) => {
                        let token = tokens.get((n as u64 & 0xFFFF_FFFF) as usize).ok_or("external clone without a file")?;
                        unframe(&std::fs::read(files.join(token.trim_start_matches('.'))).map_err(|e| e.to_string())?)?
                    }
                    _ => return Err("record data of an unknown column type".into()),
                };
                let value = Reader::new(&clone, &tokens, files).read()?;
                Ok(json!({"key":k.encoded(),"value":value}))
            });
            match record {
                Ok(r) => records.push(r),
                Err(err) => eprintln!("mrt-api: IndexedDB profile: {name}/{store_name}: record not imported: {err}"),
            }
        }
        stores.insert(
            store_name,
            json!({"keyPath":key_path(path.as_deref()),"autoIncrement":next!=0,"indexes":indexes,"next":next.max(1),"records":records}),
        );
    }
    Ok(json!({"name":name,"version":version,"stores":stores}))
}

/// KeyPath::DeserializeFromString: NULL none, a leading ',' an array of paths, else one path.
fn key_path(s: Option<&str>) -> Value {
    match s {
        None => Value::Null,
        Some(s) if s.starts_with(',') => json!(s[1..].split(',').collect::<Vec<_>>()),
        Some(s) => json!(s),
    }
}

/// A key column as Key::DecodeJSVal reads it.
pub fn decode_key(b: &[u8]) -> Result<Key, String> {
    decode_key_at(b, &mut 0, 0, 0)
}

const KEY_FLOAT: i32 = 0x10;
const KEY_DATE: i32 = 0x20;
const KEY_STRING: i32 = 0x30;
const KEY_BINARY: i32 = 0x40;
const KEY_ARRAY: i32 = 0x50;

fn decode_key_at(b: &[u8], pos: &mut usize, mut offset: i32, depth: u32) -> Result<Key, String> {
    if depth == 64 || *pos >= b.len() {
        return Err("a key too deep or truncated".into());
    }
    let kind = b[*pos] as i32 - offset;
    if kind >= KEY_ARRAY {
        // an array's type byte also carries its first item's, up to three arrays deep
        offset += KEY_ARRAY;
        if offset == KEY_ARRAY * 3 {
            *pos += 1;
            offset = 0;
        }
        let mut items = vec![];
        while *pos < b.len() && b[*pos] as i32 - offset != 0 {
            items.push(decode_key_at(b, pos, offset, depth + 1)?);
            offset = 0;
        }
        *pos += 1;
        return Ok(Key::Array(items));
    }
    match kind {
        KEY_FLOAT => Ok(Key::Number(decode_number(b, pos))),
        KEY_DATE => Ok(Key::Date(decode_number(b, pos))),
        KEY_STRING => {
            let units = decode_stringy(b, pos, true);
            Ok(Key::String(crate::surrogates::from_utf16(&units)))
        }
        KEY_BINARY => Ok(Key::Binary(decode_stringy(b, pos, false).into_iter().map(|u| u as u8).collect())),
        _ => Err(format!("a key of type {kind:#x}")),
    }
}

/// Key::DecodeNumber: big-endian bits with the sign flipped for order; trailing zero bytes of the
/// key are trimmed.
fn decode_number(b: &[u8], pos: &mut usize) -> f64 {
    let mut raw = [0u8; 8];
    let start = (*pos + 1).min(b.len());
    let n = (b.len() - start).min(8);
    raw[..n].copy_from_slice(&b[start..start + n]);
    *pos += 9;
    let number = u64::from_be_bytes(raw);
    let sign = 1u64 << 63;
    f64::from_bits(if number & sign != 0 { number & !sign } else { 0u64.wrapping_sub(number) })
}

/// Key::DecodeAsStringy: one byte below 0x80, two for 0x80..0xBF (and always for binary), three
/// for the rest of the UTF-16 range; up to a 0 byte or the end.
fn decode_stringy(b: &[u8], pos: &mut usize, wide: bool) -> Vec<u16> {
    let mut i = *pos + 1;
    let mut out = vec![];
    let at = |i: usize| b.get(i).copied().unwrap_or(0) as u32;
    while i < b.len() && b[i] != 0 {
        let c = b[i] as u32;
        if c & 0x80 == 0 {
            out.push((c - 1) as u16);
            i += 1;
        } else if !wide || c & 0x40 == 0 {
            let v = (c << 8) | if i + 1 < b.len() { at(i + 1) } else { 0 };
            out.push((v + 0x7F - 0x8000) as u16);
            i += 2;
        } else {
            let mut v = c << 10;
            if i + 1 < b.len() {
                v |= at(i + 1) << 2;
            }
            if i + 2 < b.len() {
                v |= at(i + 2) >> 6;
            }
            out.push(v as u16);
            i += 3;
        }
    }
    *pos = i.min(b.len()) + 1;
    out
}

// js/src/vm/StructuredClone.cpp's tags, and dom/base/StructuredCloneTags.h's IndexedDB writes
const FLOAT_MAX: u32 = 0xFFF00000;
const HEADER: u32 = 0xFFF10000;
const NULL: u32 = 0xFFFF0000;
const UNDEFINED: u32 = NULL + 1;
const BOOLEAN: u32 = NULL + 2;
const INT32: u32 = NULL + 3;
const STRING: u32 = NULL + 4;
const DATE: u32 = NULL + 5;
const REGEXP: u32 = NULL + 6;
const ARRAY: u32 = NULL + 7;
const OBJECT: u32 = NULL + 8;
const ARRAY_BUFFER_V2: u32 = NULL + 9;
const BOOLEAN_OBJECT: u32 = NULL + 10;
const STRING_OBJECT: u32 = NULL + 11;
const NUMBER_OBJECT: u32 = NULL + 12;
const BACK_REFERENCE: u32 = NULL + 13;
const TYPED_ARRAY_V2: u32 = NULL + 16;
const MAP: u32 = NULL + 17;
const SET: u32 = NULL + 18;
const END_OF_KEYS: u32 = NULL + 19;
const DATA_VIEW_V2: u32 = NULL + 21;
const SAVED_FRAME: u32 = NULL + 22;
const JSPRINCIPALS: u32 = NULL + 23;
const BIGINT: u32 = NULL + 29;
const BIGINT_OBJECT: u32 = NULL + 30;
const ARRAY_BUFFER: u32 = NULL + 31;
const TYPED_ARRAY: u32 = NULL + 32;
const DATA_VIEW: u32 = NULL + 33;
const ERROR: u32 = NULL + 34;
const RESIZABLE_ARRAY_BUFFER: u32 = NULL + 35;
const IMMUTABLE_ARRAY_BUFFER: u32 = NULL + 37;
const TYPED_ARRAY_V1_MIN: u32 = 0xFFFF0100;
const TYPED_ARRAY_V1_MAX: u32 = TYPED_ARRAY_V1_MIN + 8;
const DOM_BLOB: u32 = 0xFFFF8001;
const DOM_FILE: u32 = 0xFFFF8005;
const DOM_EXPANDED_PRINCIPAL: u32 = 0xFFFF8012;
const DOM_SYSTEM_PRINCIPAL: u32 = 0xFFFF800C;

// Scalar::Type order (js/public/ScalarType.h), as clone_codec.js names the views
const VIEWS: [(&str, usize); 12] = [
    ("Int8Array", 1),
    ("Uint8Array", 1),
    ("Int16Array", 2),
    ("Uint16Array", 2),
    ("Int32Array", 4),
    ("Uint32Array", 4),
    ("Float32Array", 4),
    ("Float64Array", 8),
    ("Uint8ClampedArray", 1),
    ("BigInt64Array", 8),
    ("BigUint64Array", 8),
    ("Float16Array", 2),
];

/// What a back reference can point to: an object with its codec id, an ArrayBuffer (with an id
/// when it was a value of its own, without when only a view's), or a SavedFrame (not carried).
enum Slot {
    Object(u64),
    Buffer(Option<u64>, Vec<u8>),
    Frame,
}

/// A value as read: complete, or a container whose contents come later in the stream.
enum Item {
    Json(Value),
    Node(usize),
}

/// A container being filled, by its codec id. An Error's and a SavedFrame's children (cause,
/// errors, stack; a parent frame) are read past: the codec does not carry them.
enum Node {
    Array(u64, usize, Vec<(Item, Item)>),
    Object(u64, Vec<(Item, Item)>),
    Map(u64, Vec<(Item, Item)>),
    Set(u64, Vec<Item>),
    Error(u64, &'static str, String),
    Frame,
    Done,
}

/// JSStructuredCloneReader over one record, writing clone_codec.js JSON. The stream is in the
/// order of the writer's stack, not depth first: a Map's key and value headers come together, then
/// the value's contents, then the key's (traverseMap). So the reader keeps the same stack of open
/// containers (`open`) and the same list of objects (`slots`, the reader's allObjs) that back
/// references index; codec ids are given in that order.
struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
    slots: Vec<Slot>,
    nodes: Vec<Node>,
    open: Vec<usize>,
    next_id: u64,
    tokens: &'a [String],
    files: &'a Path,
}

type R<T> = Result<T, String>;

impl<'a> Reader<'a> {
    fn new(b: &'a [u8], tokens: &'a [String], files: &'a Path) -> Self {
        Self { b, pos: 0, slots: vec![], nodes: vec![], open: vec![], next_id: 0, tokens, files }
    }
    /// JSStructuredCloneReader::read: the header, the value, then its containers' contents.
    fn read(mut self) -> R<Value> {
        if self.peek()?.0 == HEADER {
            self.pair()?;
        }
        let root = self.start()?;
        while let Some(&top) = self.open.last() {
            if self.end_of_keys()? {
                self.open.pop();
                continue;
            }
            match self.nodes[top] {
                Node::Array(..) | Node::Object(..) | Node::Map(..) => {
                    let k = self.start()?;
                    // (old data ended an object's properties with a null)
                    if matches!(k, Item::Json(Value::Null)) && !matches!(self.nodes[top], Node::Map(..)) {
                        self.open.pop();
                        continue;
                    }
                    let v = self.start()?;
                    match &mut self.nodes[top] {
                        Node::Array(_, _, e) | Node::Object(_, e) | Node::Map(_, e) => e.push((k, v)),
                        _ => unreachable!(),
                    }
                }
                Node::Set(..) => {
                    let v = self.start()?;
                    if let Node::Set(_, items) = &mut self.nodes[top] {
                        items.push(v);
                    }
                }
                Node::Error(..) => {
                    for _ in 0..3 {
                        self.start()?;
                    }
                }
                Node::Frame => {
                    self.start()?;
                }
                Node::Done => unreachable!(),
            }
        }
        self.resolve(root)
    }
    fn resolve(&mut self, item: Item) -> R<Value> {
        let n = match item {
            Item::Json(v) => return Ok(v),
            Item::Node(n) => n,
        };
        Ok(match std::mem::replace(&mut self.nodes[n], Node::Done) {
            Node::Array(id, len, entries) => {
                let mut items = vec![json!(["h"]); len];
                for (k, v) in entries {
                    let v = self.resolve(v)?;
                    // an index; any other property of an array the codec does not carry
                    if let Some(i) = self.resolve(k)?.as_i64().filter(|i| *i >= 0 && (*i as usize) < len) {
                        items[i as usize] = v;
                    }
                }
                json!(["a", id, items])
            }
            Node::Object(id, entries) => {
                let mut props = Map::new();
                for (k, v) in entries {
                    let k = match self.resolve(k)? {
                        Value::String(s) => s,
                        Value::Number(n) => n.to_string(),
                        _ => return Err("a property key that is not a string or an index".into()),
                    };
                    let v = self.resolve(v)?;
                    props.insert(k, v);
                }
                json!(["o", id, props])
            }
            Node::Map(id, entries) => {
                let mut out = vec![];
                for (k, v) in entries {
                    out.push(json!([self.resolve(k)?, self.resolve(v)?]));
                }
                json!(["m", id, out])
            }
            Node::Set(id, items) => {
                let items = items.into_iter().map(|v| self.resolve(v)).collect::<R<Vec<_>>>()?;
                json!(["s", id, items])
            }
            Node::Error(id, name, message) => json!(["e", id, name, message]),
            Node::Frame | Node::Done => Value::Null,
        })
    }
    fn u64(&mut self) -> R<u64> {
        let s = self.b.get(self.pos..self.pos + 8).ok_or("truncated clone")?;
        self.pos += 8;
        Ok(u64::from_le_bytes(s.try_into().unwrap()))
    }
    fn peek(&self) -> R<(u32, u32)> {
        let s = self.b.get(self.pos..self.pos + 8).ok_or("truncated clone")?;
        let v = u64::from_le_bytes(s.try_into().unwrap());
        Ok(((v >> 32) as u32, v as u32))
    }
    fn pair(&mut self) -> R<(u32, u32)> {
        let v = self.u64()?;
        Ok(((v >> 32) as u32, v as u32))
    }
    /// SCInput::readBytes: the bytes, then zeros up to an 8-byte boundary
    fn bytes(&mut self, n: usize) -> R<Vec<u8>> {
        let s = self.b.get(self.pos..self.pos + n).ok_or("truncated clone")?.to_vec();
        self.pos += n.div_ceil(8) * 8;
        Ok(s)
    }
    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id - 1
    }
    /// A new object's slot and codec id
    fn object(&mut self) -> u64 {
        let id = self.id();
        self.slots.push(Slot::Object(id));
        id
    }
    /// A new container, open until its END_OF_KEYS
    fn node(&mut self, node: Node) -> Item {
        self.nodes.push(node);
        self.open.push(self.nodes.len() - 1);
        Item::Node(self.nodes.len() - 1)
    }
    fn string(&mut self, data: u32) -> R<String> {
        let n = (data & ((1 << 30) - 1)) as usize;
        if data & (1 << 30) != 0 {
            return Err("a string buffer pointer in a stored clone".into());
        }
        let units: Vec<u16> = if data & (1 << 31) != 0 {
            self.bytes(n)?.into_iter().map(u16::from).collect()
        } else {
            self.bytes(n * 2)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
        };
        Ok(crate::surrogates::from_utf16(&units))
    }
    /// A primitive a header nests (an Error's message, a frame's line)
    fn primitive(&mut self) -> R<Value> {
        match self.start()? {
            Item::Json(v) => Ok(v),
            Item::Node(_) => Err("an object where a primitive belongs".into()),
        }
    }
    fn end_of_keys(&mut self) -> R<bool> {
        if self.peek()?.0 == END_OF_KEYS {
            self.pair()?;
            return Ok(true);
        }
        Ok(false)
    }
    /// JSStructuredCloneReader::startRead: one value, or a container's header.
    fn start(&mut self) -> R<Item> {
        let (tag, data) = self.pair()?;
        Ok(Item::Json(match tag {
            NULL => Value::Null,
            UNDEFINED => json!(["u"]),
            BOOLEAN => json!(data != 0),
            INT32 => json!(data as i32),
            STRING => json!(self.string(data)?),
            BOOLEAN_OBJECT => {
                let id = self.object();
                json!(["w", id, "Boolean", data != 0])
            }
            STRING_OBJECT => {
                let s = self.string(data)?;
                let id = self.object();
                json!(["w", id, "String", s])
            }
            NUMBER_OBJECT => {
                let d = f64::from_bits(self.u64()?);
                let id = self.object();
                // (the codec's JSON of a non-finite wrapped number)
                json!(["w", id, "Number", if d.is_finite() { number(d) } else { Value::Null }])
            }
            BIGINT | BIGINT_OBJECT => {
                let digits = (0..(data & 0x7FFF_FFFF)).map(|_| self.u64()).collect::<R<Vec<_>>>()?;
                let text = bigint(digits, data >> 31 != 0);
                if tag == BIGINT {
                    json!(["b", text])
                } else {
                    let id = self.object();
                    json!(["w", id, "BigInt", text])
                }
            }
            DATE => {
                let d = f64::from_bits(self.u64()?);
                let id = self.object();
                json!(["d", id, if d.is_finite() { number(d) } else { Value::Null }])
            }
            REGEXP => {
                let (t, s) = self.pair()?;
                if t != STRING {
                    return Err("a RegExp without its source".into());
                }
                let source = self.string(s)?;
                let id = self.object();
                // RegExpFlag bits, in the order RegExp.prototype.flags lists them
                let flags: String = [(0x40, 'd'), (0x02, 'g'), (0x01, 'i'), (0x04, 'm'), (0x20, 's'), (0x10, 'u'), (0x80, 'v'), (0x08, 'y')]
                    .iter()
                    .filter(|(bit, _)| data & bit != 0)
                    .map(|(_, c)| *c)
                    .collect();
                json!(["x", id, source, flags])
            }
            ARRAY => {
                let id = self.object();
                return Ok(self.node(Node::Array(id, data as usize, vec![])));
            }
            OBJECT => {
                let id = self.object();
                return Ok(self.node(Node::Object(id, vec![])));
            }
            MAP => {
                let id = self.object();
                return Ok(self.node(Node::Map(id, vec![])));
            }
            SET => {
                let id = self.object();
                return Ok(self.node(Node::Set(id, vec![])));
            }
            BACK_REFERENCE => match self.slots.get(data as usize) {
                Some(Slot::Object(id)) | Some(Slot::Buffer(Some(id), _)) => json!(["r", id]),
                // a view's buffer met again on its own: a copy (the codec has no shared buffers)
                Some(Slot::Buffer(None, bytes)) => {
                    let b64 = STANDARD.encode(bytes);
                    let id = self.id();
                    json!(["ab", id, b64])
                }
                Some(Slot::Frame) => Value::Null,
                None => return Err("a back reference to nothing".into()),
            },
            ARRAY_BUFFER_V2 | ARRAY_BUFFER | RESIZABLE_ARRAY_BUFFER | IMMUTABLE_ARRAY_BUFFER => {
                let bytes = self.array_buffer(tag, data)?;
                let id = self.id();
                let b64 = STANDARD.encode(&bytes);
                self.slots.push(Slot::Buffer(Some(id), bytes));
                json!(["ab", id, b64])
            }
            TYPED_ARRAY | TYPED_ARRAY_V2 => {
                let (kind, n) = if tag == TYPED_ARRAY { (data as u64, self.u64()?) } else { (self.u64()?, data as u64) };
                self.view(kind as usize, n, false)?
            }
            TYPED_ARRAY_V1_MIN..=TYPED_ARRAY_V1_MAX => self.view((tag - TYPED_ARRAY_V1_MIN) as usize, data as u64, true)?,
            DATA_VIEW | DATA_VIEW_V2 => {
                let n = if tag == DATA_VIEW { self.u64()? } else { data as u64 };
                self.view(usize::MAX, n, false)?
            }
            SAVED_FRAME => {
                self.saved_frame(data)?;
                return Ok(self.node(Node::Frame));
            }
            ERROR => {
                // JSExnType: Error, InternalError, AggregateError, EvalError, RangeError,
                // ReferenceError, SuppressedError, SyntaxError, TypeError, URIError
                let name = match data {
                    0 => "Error",
                    2 => "AggregateError",
                    3 => "EvalError",
                    4 => "RangeError",
                    5 => "ReferenceError",
                    7 => "SyntaxError",
                    8 => "TypeError",
                    9 => "URIError",
                    _ => return Err(format!("an Error of type {data}")),
                };
                let message = self.primitive()?;
                // hasCause, fileName, lineNumber, columnNumber
                for _ in 0..4 {
                    self.primitive()?;
                }
                let id = self.object();
                let message = message.as_str().unwrap_or("").to_string();
                return Ok(self.node(Node::Error(id, name, message)));
            }
            DOM_BLOB | DOM_FILE => {
                let id = self.object();
                self.blob(id, tag == DOM_FILE, data)?
            }
            t if t <= FLOAT_MAX => number(f64::from_bits(((t as u64) << 32) | data as u64)),
            t => return Err(format!("clone tag {t:#x}")),
        }))
    }
    fn array_buffer(&mut self, tag: u32, data: u32) -> R<Vec<u8>> {
        let n = match tag {
            ARRAY_BUFFER_V2 => data as u64,
            RESIZABLE_ARRAY_BUFFER => {
                let n = self.u64()?;
                self.u64()?;
                n
            }
            _ => self.u64()?,
        };
        self.bytes(n as usize)
    }
    /// A typed array (`kind` a Scalar::Type) or a DataView (usize::MAX) of `n` items (u64::MAX:
    /// length-tracking, to the buffer's end): its slot comes before its buffer's, then the buffer
    /// (or a back reference to it) and the byte offset. The codec keeps the view's own bytes.
    fn view(&mut self, kind: usize, n: u64, v1: bool) -> R<Value> {
        let (cls, size) = if kind == usize::MAX { ("DataView", 1) } else { *VIEWS.get(kind).ok_or("a typed array of an unknown type")? };
        let id = self.object();
        let (buffer, offset) = if v1 {
            (self.bytes(n as usize * size)?, 0)
        } else {
            let (tag, data) = self.pair()?;
            let buffer = match tag {
                BACK_REFERENCE => match self.slots.get(data as usize) {
                    Some(Slot::Buffer(_, b)) => b.clone(),
                    _ => return Err("a view's back reference that is not a buffer".into()),
                },
                ARRAY_BUFFER_V2 | ARRAY_BUFFER | RESIZABLE_ARRAY_BUFFER | IMMUTABLE_ARRAY_BUFFER => {
                    let b = self.array_buffer(tag, data)?;
                    self.slots.push(Slot::Buffer(None, b.clone()));
                    b
                }
                _ => return Err("a view without a buffer".into()),
            };
            (buffer, self.u64()? as usize)
        };
        let len = if n == u64::MAX { buffer.len().saturating_sub(offset) } else { n as usize * size };
        let bytes = buffer.get(offset..offset + len).ok_or("a view outside its buffer")?;
        Ok(json!(["ta", id, cls, STANDARD.encode(bytes)]))
    }
    /// A SavedFrame's header (an Error's stack): its principals and fields, read past.
    fn saved_frame(&mut self, principals: u32) -> R<()> {
        if principals == JSPRINCIPALS {
            let (tag, _) = self.pair()?;
            self.principal(tag)?;
        }
        // mutedErrors (absent in old data: then it is the source), source, line, column, name, cause
        let first = self.primitive()?;
        for _ in 0..if first.is_boolean() { 5 } else { 4 } {
            self.primitive()?;
        }
        self.slots.push(Slot::Frame);
        Ok(())
    }
    /// nsJSPrincipals::ReadPrincipalInfo
    fn principal(&mut self, tag: u32) -> R<()> {
        match tag {
            DOM_SYSTEM_PRINCIPAL => Ok(()),
            DOM_EXPANDED_PRINCIPAL => {
                let (n, _) = self.pair()?;
                for _ in 0..n {
                    let (t, _) = self.pair()?;
                    self.principal(t)?;
                }
                Ok(())
            }
            _ => {
                let (suffix, spec) = self.pair()?;
                self.bytes(suffix as usize)?;
                self.bytes(spec as usize)?;
                let (origin, _) = self.pair()?;
                self.bytes(origin as usize)?;
                let (void, base) = self.pair()?;
                if void == 0 {
                    self.bytes(base as usize)?;
                }
                Ok(())
            }
        }
    }
    /// StructuredCloneReadString: a 32-bit length and UTF-8, each padded
    fn utf8(&mut self) -> R<String> {
        let n = u32::from_le_bytes(self.bytes(4)?.try_into().unwrap());
        Ok(String::from_utf8_lossy(&self.bytes(n as usize)?).into_owned())
    }
    /// ReadBlobOrFile: size, type, and a File's lastModified and name; the bytes are the file
    /// file_ids names at `index`.
    fn blob(&mut self, id: u64, file: bool, index: u32) -> R<Value> {
        self.bytes(8)?;
        let kind = self.utf8()?;
        let (name, modified) = if file {
            let m = i64::from_le_bytes(self.bytes(8)?.try_into().unwrap());
            (json!(self.utf8()?), json!(m))
        } else {
            (Value::Null, Value::Null)
        };
        let token = self.tokens.get(index as usize).ok_or("a Blob without its file")?;
        let bytes = std::fs::read(self.files.join(token)).map_err(|e| format!("a Blob's file {token}: {e}"))?;
        Ok(json!(["bl", id, STANDARD.encode(bytes), kind, name, modified]))
    }
}

/// SnappyUncompressInputStream's framing (xpcom/io/SnappyFrameUtils.cpp): chunks of a type byte
/// and a 24-bit length; the stream identifier and padding skipped, data chunks with a checksum
/// first. Gecko's crc32c is not inverted at the end (ComputeCrc32c from ~0), so the standard
/// frame decoder rejects it; the checksums are not checked here.
fn unframe(b: &[u8]) -> R<Vec<u8>> {
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        let kind = b[i];
        let len = b.get(i + 1..i + 4).map(|l| l[0] as usize | (l[1] as usize) << 8 | (l[2] as usize) << 16).ok_or("a cut frame")?;
        let chunk = b.get(i + 4..i + 4 + len).ok_or("a cut frame")?;
        match kind {
            0x00 => out.extend(snap::raw::Decoder::new().decompress_vec(chunk.get(4..).ok_or("a cut frame")?).map_err(|e| e.to_string())?),
            0x01 => out.extend_from_slice(chunk.get(4..).ok_or("a cut frame")?),
            0x02..=0x7F => return Err(format!("a frame of type {kind:#x}")),
            _ => {}
        }
        i += 4 + len;
    }
    Ok(out)
}

/// A number as clone_codec.js writes it: finite ones as themselves (integers without a fraction),
/// -0 and the non-finite ones tagged.
fn number(d: f64) -> Value {
    if d.is_nan() {
        json!(["n", "NaN"])
    } else if d.is_infinite() {
        json!(["n", if d > 0.0 { "Infinity" } else { "-Infinity" }])
    } else if d == 0.0 && d.is_sign_negative() {
        json!(["n", "-0"])
    } else if d.fract() == 0.0 && d.abs() < 9007199254740992.0 {
        json!(d as i64)
    } else {
        json!(d)
    }
}

/// A BigInt's 64-bit digits (least significant first) in decimal.
fn bigint(mut digits: Vec<u64>, negative: bool) -> String {
    let mut parts = vec![];
    while digits.iter().any(|d| *d != 0) {
        let mut rem: u128 = 0;
        for d in digits.iter_mut().rev() {
            let cur = (rem << 64) | *d as u128;
            *d = (cur / 10_000_000_000_000_000_000) as u64;
            rem = cur % 10_000_000_000_000_000_000;
        }
        parts.push(rem as u64);
    }
    let Some(last) = parts.pop() else { return "0".into() };
    let mut s = format!("{}{last}", if negative { "-" } else { "" });
    for p in parts.iter().rev() {
        s += &format!("{p:019}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        // Key.cpp's encodings of 1, "a", [1, "a"], new Uint8Array([0, 255]) and -1, trailing zeros trimmed
        let one = [0x10, 0xBF, 0xF0];
        assert!(matches!(decode_key(&one), Ok(Key::Number(n)) if n == 1.0));
        assert!(matches!(decode_key(&[0x30, 0x62]), Ok(Key::String(s)) if s == "a"));
        let arr = decode_key(&[0x60, 0xBF, 0xF0, 0, 0, 0, 0, 0, 0, 0x30, 0x62]).unwrap();
        assert!(matches!(&arr, Key::Array(items) if items.len() == 2));
        assert!(matches!(decode_key(&[0x40, 0x01, 0x80, 0x80]), Ok(Key::Binary(b)) if b == vec![0, 255]));
        assert!(matches!(decode_key(&[0x10, 0x40, 0x10]), Ok(Key::Number(n)) if n == -1.0));
    }

    #[test]
    fn bigints() {
        assert_eq!(bigint(vec![0], false), "0");
        assert_eq!(bigint(vec![u64::MAX, 1], true), "-36893488147419103231");
    }
}
