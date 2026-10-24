// The browser's identity (Phase 6.4, DESIGN "Phase 6"): what the runtime's `env` request asks the
// bridge tab for (mini_realsites.py OP, "env"), served from a recording of the test profile's
// Firefox: navigator's fields, screen, the window's outer size and position, and the interface
// names. API_ENV is the file mini_realsites.py recorded for this Firefox build, an entry per kind of
// global ("secure=1,isolated=0": the names a secure context or a cross-origin isolated one has
// differ). innerWidth and innerHeight are the request's viewport (the driver's: the bridge tab's
// for a page, its frame's size for a frame's runtime), as the tab answered for the window it was in.

use serde_json::{Value, json};

pub fn key(secure: bool, isolated: bool) -> String {
    format!("secure={},isolated={}", secure as u8, isolated as u8)
}

pub fn handle(req: &Value) -> Result<Value, String> {
    let path = std::env::var("API_ENV").map_err(|_| "mrt-api: API_ENV is not set")?;
    let file: Value = serde_json::from_slice(&std::fs::read(&path).map_err(|e| format!("mrt-api: {path}: {e}"))?)
        .map_err(|e| format!("mrt-api: {path}: {e}"))?;
    let flag = |k: &str| req.get(k).and_then(Value::as_bool).unwrap_or(false);
    let k = key(flag("secure"), flag("isolated"));
    let mut env = file.get("entries").and_then(|e| e.get(&k)).cloned().ok_or_else(|| format!("mrt-api: {path} has no {k}"))?;
    if let Some([w, h]) = req.get("viewport").and_then(Value::as_array).map(Vec::as_slice) {
        env["innerWidth"] = json!(w);
        env["innerHeight"] = json!(h);
    }
    Ok(env)
}
