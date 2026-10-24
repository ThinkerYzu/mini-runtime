// A partition's stores on disk: one SQLite file per (site, top-level site), holding its cookie jar
// and the localStorage of each origin in it (DESIGN "Phase 6": "SQLite per site and partition
// key"). The site is taken without its scheme, as Gecko keys a jar by base domain and partition
// key: an http://a.com frame in an https://a.com page shares the page's jar. A window with another
// site's frame above it although its own site is the top-level one's (an A frame in a B frame in an
// A page) has a partition of its own, the partition key's foreign-ancestor bit ("(https,a.com,f)"). A new file is seeded from the test profile Firefox was started with (API_PROFILE): the
// site's rows of cookies.sqlite under this partition key, and, at an origin's first localStorage
// call, that origin's storage/default/<origin>/ls/data.sqlite, and at its first IndexedDB call
// its idb/*.sqlite (idb_profile.rs), as Firefox has them at start.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, params};

pub struct Store {
    pub conn: Connection,
    /// The site's registrable domain (Gecko's cookie base domain) and the top-level site's
    pub base: String,
    pub top_base: String,
    pub top_scheme: String,
    /// The partition key has the foreign-ancestor bit: the window is a third party although its
    /// site is the top-level site
    pub foreign_ancestor: bool,
    pub(crate) profile: Option<PathBuf>,
}

/// A site ("https://example.com", Services.eTLD.getSite's form) as (scheme, host).
pub fn split_site(site: &str) -> (String, String) {
    match url::Url::parse(site) {
        Ok(u) => (u.scheme().to_string(), u.host_str().unwrap_or("").to_string()),
        Err(_) => (String::new(), site.to_string()),
    }
}

/// A URL's site as Services.eTLD.getSite gives it: the scheme and the registrable domain, or the
/// host when it has none (an IP address, localhost); "" for a URL with no host.
pub fn site_of(u: &url::Url) -> String {
    let host = match u.host() {
        Some(url::Host::Domain(d)) => d.to_ascii_lowercase(),
        Some(url::Host::Ipv4(a)) => a.to_string(),
        Some(url::Host::Ipv6(a)) => format!("[{a}]"),
        None => return String::new(),
    };
    let domain = if matches!(u.host(), Some(url::Host::Domain(_))) {
        psl::domain_str(&host).map(str::to_string).unwrap_or(host)
    } else {
        host
    };
    format!("{}://{}", u.scheme(), domain)
}

/// The partitions' stores of one process, opened at their first use: one per (site, top-level
/// site), in API_STORE, seeded from API_PROFILE.
#[derive(Default)]
pub struct Stores {
    open: HashMap<(String, String, bool), Store>,
}

impl Stores {
    /// Gives back each open store's SQLite page cache (PRAGMA shrink_memory), as memory pressure would.
    pub fn shrink(&self) {
        for store in self.open.values() {
            let _ = store.conn.execute_batch("PRAGMA shrink_memory");
        }
    }

    /// The store of `site` (its scheme ignored) under the top-level site `top`; `ancestor`: the
    /// foreign-ancestor partition (main.rs).
    pub fn get(&mut self, site: &str, top: &str, ancestor: bool) -> Result<&Store, String> {
        let key = (split_site(site).1, top.to_string(), ancestor);
        if !self.open.contains_key(&key) {
            let dir = std::env::var_os("API_STORE").map(PathBuf::from).ok_or("mrt-api: API_STORE is not set")?;
            let profile = std::env::var_os("API_PROFILE").map(PathBuf::from);
            let store = Store::open(&dir, site, top, ancestor, profile.as_deref())?;
            self.open.insert(key.clone(), store);
        }
        Ok(&self.open[&key])
    }
}

fn file_name(base: &str, top: &str, ancestor: bool) -> String {
    let clean = |s: &str| s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '+' }).collect::<String>();
    format!("{}^{}{}.sqlite", clean(base), clean(top), if ancestor { "^f" } else { "" })
}

impl Store {
    /// The partition's file under `dir`, made and seeded if it is new.
    pub fn open(dir: &Path, site: &str, top: &str, ancestor: bool, profile: Option<&Path>) -> Result<Store, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("mrt-api: store {dir:?}: {e}"))?;
        let path = dir.join(file_name(&split_site(site).1, top, ancestor));
        let new = !path.exists();
        let conn = Connection::open(&path).map_err(|e| format!("mrt-api: store {path:?}: {e}"))?;
        // two process sets share the files while an HTTP viewer navigates (the old document's and
        // the new one's): a write waits for the other's instead of failing
        conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(|e| format!("mrt-api: store {path:?}: {e}"))?;
        // the stores outlive the bridge, not a crash of the machine: no fsync per write
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=OFF;
             CREATE TABLE IF NOT EXISTS cookies (
               bucket INTEGER NOT NULL, name TEXT NOT NULL, value TEXT NOT NULL, host TEXT NOT NULL,
               path TEXT NOT NULL, expiry INTEGER NOT NULL, last_accessed INTEGER NOT NULL,
               creation INTEGER NOT NULL, secure INTEGER NOT NULL, http_only INTEGER NOT NULL,
               same_site INTEGER NOT NULL, scheme_map INTEGER NOT NULL, partitioned_attr INTEGER NOT NULL,
               session INTEGER NOT NULL, PRIMARY KEY (bucket, host, name, path));
             CREATE TABLE IF NOT EXISTS ls (
               seq INTEGER PRIMARY KEY AUTOINCREMENT, origin TEXT NOT NULL, key TEXT NOT NULL,
               value TEXT NOT NULL, len INTEGER NOT NULL, UNIQUE (origin, key));
             CREATE TABLE IF NOT EXISTS ls_seeded (origin TEXT PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS permissions (origin TEXT NOT NULL, kind TEXT NOT NULL,
               action INTEGER NOT NULL, expires INTEGER NOT NULL, PRIMARY KEY (origin, kind));
             CREATE TABLE IF NOT EXISTS permissions_seeded (origin TEXT PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS persistent_origins (origin TEXT PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS idb_databases (origin TEXT NOT NULL, name TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(origin,name));
             CREATE TABLE IF NOT EXISTS idb_seeded (origin TEXT PRIMARY KEY);",
        )
        .map_err(|e| format!("mrt-api: store schema: {e}"))?;
        let (_, base) = split_site(site);
        let (top_scheme, top_base) = split_site(top);
        let store = Store { conn, base, top_base, top_scheme, foreign_ancestor: ancestor, profile: profile.map(Path::to_path_buf) };
        if new {
            store.seed_cookies();
        }
        Ok(store)
    }

    /// ThirdPartyUtil::IsThirdPartyWindow: base domains compared, not schemes, and a window with a
    /// foreign ancestor is a third party
    pub fn first_party(&self) -> bool {
        self.base == self.top_base && !self.foreign_ancestor
    }

    /// The bucket a partitionKey of cookies.sqlite or Firefox's cookie manager falls in for this
    /// partition: 0 the jar a document of the site uses (unpartitioned first-party, the top-level
    /// site's partition for a third party), 1 a first party's own CHIPS partition; None another
    /// partition's.
    pub fn bucket_of(&self, partition_key: &str) -> Option<i64> {
        if partition_key.is_empty() {
            return if self.first_party() { Some(0) } else { None };
        }
        // "(scheme,baseDomain[,port][,f])"
        let inner = partition_key.strip_prefix('(')?.strip_suffix(')')?;
        let mut parts = inner.split(',');
        let scheme = parts.next()?;
        let base = parts.next()?;
        // the f flag: foreign by an ancestor (an A-in-B-in-A frame), only that partition's
        if parts.any(|p| p == "f") != self.foreign_ancestor {
            return None;
        }
        if base != self.top_base || scheme != self.top_scheme {
            return None;
        }
        Some(if self.first_party() { 1 } else { 0 })
    }

    /// The site's rows of the profile's cookies.sqlite, copied with its WAL so that the open
    /// Firefox's file is not touched.
    fn seed_cookies(&self) {
        let Some(profile) = &self.profile else { return };
        let src = profile.join("cookies.sqlite");
        if !src.exists() {
            return;
        }
        let tmp = std::env::temp_dir().join(format!("mrt-api-cookies-{}.sqlite", std::process::id()));
        let _ = std::fs::copy(&src, &tmp);
        let _ = std::fs::copy(profile.join("cookies.sqlite-wal"), tmp.with_extension("sqlite-wal"));
        let rows = (|| -> rusqlite::Result<Vec<SeedRow>> {
            let db = Connection::open_with_flags(&tmp, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
            let mut st = db.prepare(
                "SELECT originAttributes, name, value, host, path, expiry, lastAccessed, creationTime, isSecure,
                        isHttpOnly, sameSite, schemeMap, isPartitionedAttributeSet FROM moz_cookies",
            )?;
            let rows = st.query_map([], |r| {
                Ok(SeedRow {
                    attrs: r.get(0)?,
                    name: r.get(1)?,
                    value: r.get(2)?,
                    host: r.get(3)?,
                    path: r.get(4)?,
                    expiry: r.get(5)?,
                    last_accessed: r.get(6)?,
                    creation: r.get(7)?,
                    secure: r.get(8)?,
                    http_only: r.get(9)?,
                    same_site: r.get(10)?,
                    scheme_map: r.get(11)?,
                    partitioned_attr: r.get(12)?,
                })
            })?;
            rows.collect()
        })()
        .unwrap_or_default();
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(tmp.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(tmp.with_extension("sqlite-shm"));
        let now_ms = crate::cookie::now_us() / 1000;
        for row in rows {
            // the originAttributes suffix: only a partitionKey, or nothing (no containers)
            let key = if row.attrs.is_empty() {
                String::new()
            } else if let Some(k) = row.attrs.strip_prefix("^partitionKey=") {
                if k.contains('&') {
                    continue;
                }
                percent_decode(k)
            } else {
                continue;
            };
            let Some(bucket) = self.bucket_of(&key) else { continue };
            let host = row.host.strip_prefix('.').unwrap_or(&row.host);
            if !(host == self.base || host.ends_with(&format!(".{}", self.base))) || row.expiry <= now_ms {
                continue;
            }
            let _ = self.conn.execute(
                "INSERT OR REPLACE INTO cookies VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 0)",
                params![bucket, row.name, row.value, row.host, row.path, row.expiry, row.last_accessed, row.creation,
                        row.secure, row.http_only, row.same_site, row.scheme_map, row.partitioned_attr],
            );
        }
    }

    /// The origin's directories under the profile's storage/default that this partition reads
    /// (localStorage's ls/, IndexedDB's idb/): QuotaManager's directory name of the origin,
    /// "https+++www.example.com" with a port after a '+', and for a third party each partitionKey
    /// suffix (percent-encoded) that falls in this partition. None without a profile.
    pub(crate) fn profile_origin_dirs(&self, origin: &str) -> Vec<PathBuf> {
        let Some(profile) = &self.profile else { return vec![] };
        let Ok(u) = url::Url::parse(origin) else { return vec![] };
        let mut dir = format!("{}+++{}", u.scheme(), u.host_str().unwrap_or(""));
        if let Some(port) = u.port() {
            dir += &format!("+{port}");
        }
        let root = profile.join("storage").join("default");
        if self.first_party() {
            return vec![root.join(&dir)];
        }
        std::fs::read_dir(&root)
            .map(|it| {
                it.filter_map(|e| e.ok())
                    .filter(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        name.strip_prefix(&format!("{dir}^partitionKey="))
                            .is_some_and(|k| self.bucket_of(&percent_decode(k)) == Some(0))
                    })
                    .map(|e| e.path())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The origin's localStorage from the profile, once, at its first call in this partition.
    pub fn seed_local_storage(&self, origin: &str) {
        let done: bool = self
            .conn
            .query_row("SELECT 1 FROM ls_seeded WHERE origin = ?1", [origin], |_| Ok(true))
            .unwrap_or(false);
        if done {
            return;
        }
        let _ = self.conn.execute("INSERT INTO ls_seeded VALUES (?1)", [origin]);
        for d in self.profile_origin_dirs(origin) {
            let file = d.join("ls").join("data.sqlite");
            if !file.exists() {
                continue;
            }
            let uri = format!("file:{}?immutable=1", file.display());
            let Ok(db) = Connection::open_with_flags(&uri, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI) else { continue };
            let Ok(mut st) = db.prepare("SELECT key, conversion_type, compression_type, value FROM data") else { continue };
            let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, Vec<u8>>(3)?)));
            let Ok(rows) = rows else { continue };
            for (key, conversion, compression, value) in rows.flatten() {
                // LSValue: snappy (1) or none (0); UTF-8 (1) or the UTF-16 code units' bytes (0)
                let raw = if compression == 1 {
                    match snap::raw::Decoder::new().decompress_vec(&value) {
                        Ok(v) => v,
                        Err(_) => continue,
                    }
                } else {
                    value
                };
                let text = if conversion == 1 {
                    String::from_utf8_lossy(&raw).into_owned()
                } else {
                    // (a value with a lone surrogate is stored so: kept, surrogates.rs)
                    let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                    crate::surrogates::from_utf16(&units)
                };
                let len = crate::surrogates::utf16_len(&key) + crate::surrogates::utf16_len(&text);
                let _ = self.conn.execute(
                    "INSERT OR REPLACE INTO ls (origin, key, value, len) VALUES (?1, ?2, ?3, ?4)",
                    params![origin, key, text, len as i64],
                );
            }
        }
    }
}

struct SeedRow {
    attrs: String,
    name: String,
    value: String,
    host: String,
    path: String,
    expiry: i64,
    last_accessed: i64,
    creation: i64,
    secure: i64,
    http_only: i64,
    same_site: i64,
    scheme_map: i64,
    partitioned_attr: i64,
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
