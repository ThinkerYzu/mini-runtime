//! Original-origin permission decisions; no viewer-origin UI or implicit grants.
use crate::store::{Store, site_of};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::collections::HashMap;
use std::sync::OnceLock;
use serde_json::{Value, json};

const QUOTA: i64 = 10 * 1024 * 1024; // bytes: localStorage's 5 Mi UTF-16-unit limit
fn error(name: &str, message: &str) -> Value {
    json!({"error": {"name": name, "message": message}, "ms": 0})
}
fn invalid_name(name: &str) -> Value {
    error(
        "TypeError",
        &format!(
            "'{name}' (value of 'name' member of PermissionDescriptor) is not a valid value for enumeration PermissionName."
        ),
    )
}

/// Snapshot persistent exact-origin decisions. Never open the owner's database writable.
fn seed(store: &Store, origin: &str, first_party: bool) -> Result<(), String> {
    let db = &store.conn;
    let done = db
        .query_row(
            "SELECT 1 FROM permissions_seeded WHERE origin=?1",
            [origin],
            |_| Ok(()),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .is_some();
    if done {
        return Ok(());
    }
    let mut rows = Vec::new();
    if first_party {
        if let Some(profile) = &store.profile {
            let src = profile.join("permissions.sqlite");
            if src.exists() {
                let tmp = std::env::temp_dir()
                    .join(format!("mrt-api-permissions-{}.sqlite", std::process::id()));
                let wal = tmp.with_extension("sqlite-wal");
                let shm = tmp.with_extension("sqlite-shm");
                for path in [&tmp, &wal, &shm] {
                    let _ = std::fs::remove_file(path);
                }
                let result = (|| -> Result<Vec<(String, i64, i64)>, String> {
                    std::fs::copy(&src, &tmp).map_err(|e| e.to_string())?;
                    if profile.join("permissions.sqlite-wal").exists() {
                        std::fs::copy(profile.join("permissions.sqlite-wal"), &wal)
                            .map_err(|e| e.to_string())?;
                    }
                    let source =
                        Connection::open_with_flags(&tmp, OpenFlags::SQLITE_OPEN_READ_WRITE)
                            .map_err(|e| e.to_string())?;
                    let mut stmt = source.prepare("SELECT type, permission, CASE WHEN expireType=1 THEN expireTime ELSE 0 END FROM moz_perms WHERE origin=?1 AND expireType IN (0,1) AND permission IN (1,2,3)").map_err(|e| e.to_string())?;
                    stmt.query_map([origin], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                        .map_err(|e| e.to_string())?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| e.to_string())
                })();
                for path in [&tmp, &wal, &shm] {
                    let _ = std::fs::remove_file(path);
                }
                rows = result.map_err(|e| format!("mrt-api: permissions seed: {e}"))?;
            }
        }
    }
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    for (kind, action, expires) in rows {
        tx.execute(
            "INSERT OR IGNORE INTO permissions VALUES (?1,?2,?3,?4)",
            params![origin, kind, action, expires],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.execute("INSERT INTO permissions_seeded VALUES (?1)", [origin])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}
/// The profile's preferences Gecko's permission manager reads: prefs.js, then user.js over it
/// (Preferences' load order). Integers and booleans only; read once per process.
fn prefs(store: &Store) -> &'static HashMap<String, i64> {
    static PREFS: OnceLock<HashMap<String, i64>> = OnceLock::new();
    PREFS.get_or_init(|| {
        let mut prefs = HashMap::new();
        let Some(profile) = &store.profile else { return prefs };
        for file in ["prefs.js", "user.js"] {
            let text = std::fs::read_to_string(profile.join(file)).unwrap_or_default();
            for line in text.lines() {
                // user_pref("name", value);
                let Some(rest) = line.trim().strip_prefix("user_pref(\"") else { continue };
                let Some((name, rest)) = rest.split_once('"') else { continue };
                let value = rest.trim_start_matches([',', ' ']).trim_end_matches([')', ';', ' ']);
                let value = match value {
                    "true" => 1,
                    "false" => 0,
                    v => match v.parse() {
                        Ok(n) => n,
                        Err(_) => continue,
                    },
                };
                prefs.insert(name.to_string(), value);
            }
        }
        prefs
    })
}

/// The origin and, unless `exact`, the origins of its parent domains, nearest first: Gecko's
/// GetNextSubDomainPrincipal drops the first label while the host is longer than its base domain
/// (eTLD+1), keeping the scheme and port. An IP address has no parent.
fn origin_chain(origin: &str, exact: bool) -> Vec<String> {
    let mut chain = vec![origin.to_string()];
    let Ok(mut url) = url::Url::parse(origin) else { return chain };
    if exact || !matches!(url.host(), Some(url::Host::Domain(_))) {
        return chain;
    }
    loop {
        let host = url.host_str().unwrap_or("").to_string();
        let base = psl::domain_str(&host).unwrap_or(&host).to_string();
        let Some((_, parent)) = host.split_once('.') else { break };
        if host.len() <= base.len() || url.set_host(Some(parent)).is_err() {
            break;
        }
        chain.push(url.origin().ascii_serialization());
    }
    chain
}

/// PermissionManager::CommonTestPermission: the nearest saved, unexpired action of the origin's
/// chain (origin_chain); else, for the types that have one (HasDefaultPref), the profile's
/// permissions.default.<type>, then Firefox's own default (screen-wake-lock allowed, the others
/// UNKNOWN); 0 UNKNOWN, 1 ALLOW, 2 DENY, 3 PROMPT.
fn action(store: &Store, origin: &str, kind: &str, exact: bool, first_party: bool) -> Result<i64, String> {
    for candidate in origin_chain(origin, exact) {
        seed(store, &candidate, first_party)?;
        let saved: Option<i64> = store.conn.query_row("SELECT action FROM permissions WHERE origin=?1 AND kind=?2 AND action<>0 AND (expires=0 OR expires>?3)", params![candidate,kind,crate::cookie::now_us()/1000], |r| r.get(0)).optional().map_err(|e| e.to_string())?;
        if let Some(saved) = saved {
            return Ok(saved);
        }
    }
    const WITH_DEFAULTS: [&str; 6] = ["camera", "microphone", "geo", "desktop-notification", "shortcuts", "screen-wake-lock"];
    if !WITH_DEFAULTS.contains(&kind) {
        return Ok(0);
    }
    let builtin = if kind == "screen-wake-lock" { 1 } else { 0 };
    let value = prefs(store).get(&format!("permissions.default.{kind}")).copied().unwrap_or(builtin);
    Ok(if (0..=3).contains(&value) { value } else { 0 })
}

/// A permissions.query state (ActionToPermissionState): camera and microphone saved as "Always
/// Ask" read granted, so that a site cannot prime the user, except with resistFingerprinting.
fn state(store: &Store, origin: &str, kind: &str, first_party: bool) -> Result<&'static str, String> {
    let rfp = prefs(store).get("privacy.resistFingerprinting") == Some(&1);
    Ok(match action(store, origin, kind, false, first_party)? {
        1 => "granted",
        2 => "denied",
        3 if matches!(kind, "camera" | "microphone") && !rfp => "granted",
        _ => "prompt",
    })
}

/// What a request's front end sees (SitePermissions, TestExactPermissionFromPrincipal) and what
/// Notification.permission reads (GetRawNotificationPermission): the origin's own entry only.
fn exact_state(store: &Store, origin: &str, kind: &str, first_party: bool) -> Result<&'static str, String> {
    Ok(match action(store, origin, kind, true, first_party)? {
        1 => "granted",
        2 => "denied",
        _ => "prompt",
    })
}

pub fn handle(req: &Value, store: &Store) -> Result<Value, String> {
    let origin = req.get("origin").and_then(Value::as_str).unwrap_or("");
    let url = url::Url::parse(origin).ok();
    let valid = url.as_ref().is_some_and(|u| {
        matches!(u.scheme(), "http" | "https")
            && u.origin().ascii_serialization() == origin
            && site_of(u) == req["site"].as_str().unwrap_or("")
    });
    // Validate against the site partition selected by the driver, never the viewer origin.
    if !valid {
        return Ok(error(
            "SecurityError",
            "Permission origin is opaque or does not belong to this site.",
        ));
    }
    let first_party = req["site"] == req["top"];
    let secure = req.get("secure").and_then(Value::as_bool).unwrap_or(false);
    let call = req.get("call").and_then(Value::as_str).unwrap_or("");
    let value = match call {
        "query" => {
            let name = req["desc"]["name"].as_str().unwrap_or("");
            let kind = match name {
                "geolocation" => "geo",
                "notifications" | "push" => "desktop-notification",
                "persistent-storage" => "persistent-storage",
                "midi" if req["desc"]["sysex"] == true => "midi-sysex",
                "midi" => "midi",
                "storage-access" => "storage-access",
                "screen-wake-lock" => "screen-wake-lock",
                "camera" => "camera",
                "microphone" => "microphone",
                "loopback-network" => "loopback-network",
                "local-network" => "local-network",
                _ => return Ok(invalid_name(name)),
            };
            // No storage-access API grant is imported or synthesized. Firefox
            // reports prompt even for this first-party query in the test profile.
            let decision = if name == "storage-access" {
                "prompt"
            } else {
                state(store, origin, kind, first_party)?
            };
            // The public name stays the descriptor name; aliases only select saved decisions.
            json!({"name": name, "state": decision})
        }
        "notification" | "requestNotification" => {
            let decision = if secure {
                exact_state(store, origin, "desktop-notification", first_party)?
            } else {
                "denied"
            };
            json!(if decision == "prompt" {
                "default"
            } else {
                decision
            })
        }
        "geolocation" => {
            json!({"code": if !secure || exact_state(store,origin,"geo",first_party)?=="denied" {1} else {2}})
        }
        "estimate" | "persisted" | "persist" => {
            if !secure {
                return Ok(error("SecurityError", "Storage requires a secure context."));
            }
            match call {
                "estimate" => {
                    store.seed_local_storage(origin);
                    let usage: i64 = store
                        .conn
                        .query_row(
                            "SELECT 2*coalesce(sum(len),0) FROM ls WHERE origin=?1",
                            [origin],
                            |r| r.get(0),
                        )
                        .map_err(|e| e.to_string())?;
                    json!({"quota": QUOTA, "usage": usage})
                }
                "persisted" => json!(
                    store
                        .conn
                        .query_row(
                            "SELECT 1 FROM persistent_origins WHERE origin=?1",
                            [origin],
                            |_| Ok(())
                        )
                        .optional()
                        .map_err(|e| e.to_string())?
                        .is_some()
                ),
                _ => {
                    let decision = exact_state(store, origin, "persistent-storage", first_party)?;
                    if decision == "prompt" {
                        return Ok(json!({"pending":true,"ms":0}));
                    }
                    if decision == "granted" {
                        store
                            .conn
                            .execute(
                                "INSERT OR IGNORE INTO persistent_origins VALUES (?1)",
                                [origin],
                            )
                            .map_err(|e| e.to_string())?;
                    }
                    json!(decision == "granted")
                }
            }
        }
        _ => return Err(format!("mrt-api: permissions has no call {call:?}")),
    };
    Ok(json!({"value":value,"ms":0}))
}
