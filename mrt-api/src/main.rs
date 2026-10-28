// mrt-api: an API process beside the bridge tab (SPEC decision 27; DESIGN "Phase 6: an API process
// beside the bridge tab"). The driver (bench/mini_realsites.py, API_PROCESS) starts one per site and
// sends it the runtime's forwarded calls of the kinds it serves, one JSON line each, as the runtime
// wrote them, with the site and the top-level site of the runtime that made the call added. Each
// reply is one JSON line, as the bridge tab's would be: {"ok": value} or {"err": message}.
//
// Nothing is global: every store is keyed by the request's site and top-level site (the partition
// key of Firefox's Total Cookie Protection), so one process can later serve several sites. The
// cookie jar and localStorage are on disk, one SQLite file per partition in API_STORE, seeded from
// the test profile in API_PROFILE (store.rs). fetch, XHR and script sources go to the network
// (fetch.rs), through API_PROXY when the run is a replay, and so do WebSockets (ws.rs) and the
// media element resources an HTTP viewer loads through the driver (`media`, fetch.rs, GAPS B172).
// Browser identity (env.rs) uses standalone defaults or an explicit API_ENV comparison profile. The page's WebGL
// contexts are real OpenGL ES contexts here (webgl.rs, GAPS B169).
//
// `mrt-api --site URL...` prints each URL's site (Services.eTLD.getSite's form, store.rs site_of),
// one a line, for a driver that has no Firefox to ask.

mod cookie;
mod crypto;
mod env;
mod fetch;
mod fetch_jobs;
mod gl;
mod image;
mod idb;
mod media;
mod permissions;
mod policy;
mod js;
mod prims;
mod prtime;
mod storage;
mod store;
mod surrogates;
mod webgl;
mod ws;

use std::collections::HashMap;
use std::io::{BufRead, Write};

use serde_json::{Value, json};

unsafe extern "C" {
    fn malloc_trim(pad: usize) -> i32;
}

/// The `trim` op, for the driver's memory measurement (BRIDGE_MEMORY, DESIGN "Phase 6"): what memory
/// pressure would take from the process. The stores' caches and the idle connections go, then glibc
/// gives its free pages back. The stores' contents, the crypto keys and open WebSockets stay.
fn trim(stores: &store::Stores, net: &mut fetch::Net, jobs: &mut fetch_jobs::Jobs) -> Value {
    stores.shrink();
    net.drop_idle();
    jobs.drop_idle();
    // SAFETY: malloc_trim only returns free heap memory to the system
    unsafe { malloc_trim(0) };
    Value::Null
}

fn main() {
    webgl::prefer_software();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--site") {
        for u in &args[1..] {
            println!("{}", url::Url::parse(u).map(|u| store::site_of(&u)).unwrap_or_default());
        }
        return;
    }
    if args.as_slice() == ["--default-env"] {
        println!("{}", env::defaults());
        return;
    }
    // the crypto keys of each partition (a site, under one top-level site); the cookie jars and
    // localStorage, on disk, are the stores'
    let mut keys: HashMap<(String, String), crypto::Keys> = HashMap::new();
    let shared_stores = std::sync::Arc::new(std::sync::Mutex::new(store::Stores::default()));
    let mut fetch_jobs = fetch_jobs::Jobs::default();
    let mut databases = std::collections::HashMap::<(String, String, bool, String), idb::State>::new();
    let mut net = fetch::Net::from_env();
    let ws_net = ws::Network::from_env();
    let mut sockets = ws::Sockets::default();
    // the page's WebGL contexts (GAPS B169), made at the first `webgl` request
    let mut gl = webgl::Gl::default();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&surrogates::read_line(&line)) {
            Err(e) => json!({ "err": format!("mrt-api: bad request: {e}") }),
            Ok(mut req) => {
                // localStorage and IndexedDB keep a lone surrogate; the others see U+FFFD
                if !matches!(req.get("op").and_then(Value::as_str), Some("storage" | "idb")) {
                    surrogates::make_lossy(&mut req);
                }
                let field = |k: &str| req.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                let (site, top) = (field("site"), field("top"));
                // a window whose site is the top-level site's but which has another site's frame
                // above it (the driver's `ancestor`): Gecko's partition key with the foreign-ancestor bit
                let ancestor = req.get("ancestor").and_then(Value::as_bool).unwrap_or(false);
                // A live job's poll or cancel reads only the job table (B222): it does not wait for
                // the stores, which a fetch worker may hold while it opens a partition's store or
                // writes the cookies a response set; the page's thread waits on this answer.
                let op = req.get("op").and_then(Value::as_str);
                if matches!(op, Some("fetch" | "image")) && matches!(req.get("call").and_then(Value::as_str), Some("poll" | "cancel")) {
                    let r = fetch_jobs.handle(&req, &shared_stores).map(|mut value| { value.sort_all_objects(); value });
                    let reply = match r { Ok(v) => json!({ "ok": v }), Err(e) => json!({ "err": e }) };
                    if writeln!(out, "{}", surrogates::write_line(reply.to_string())).and_then(|_| out.flush()).is_err() {
                        break;
                    }
                    continue;
                }
                let mut stores = shared_stores.lock().unwrap();
                // the cookies a WebSocket's handshake set since the last request
                sockets.take_cookies(&mut stores);
                let r = match req.get("op").and_then(Value::as_str) {
                    Some("crypto") => crypto::handle(&req, keys.entry((site, top)).or_default()),
                    Some("cookie") => stores.get(&site, &top, ancestor).and_then(|s| cookie::handle(&req, s)),
                    Some("storage") => stores.get(&site, &top, ancestor).and_then(|s| storage::handle(&req, s)),
                    Some("fetch") if req.get("call").is_some() => fetch_jobs.handle(&req, &shared_stores),
                    Some("fetch") => fetch::handle(&req, &mut net, &mut *stores),
                    Some("ws") => ws::handle(&req, &ws_net, &mut sockets, &mut stores),
                    Some("env") => env::handle(&req),
                    Some("image") if req.get("call").is_some() => fetch_jobs.handle(&req, &shared_stores),
                    Some("image") => image::handle(&req, &mut net, &mut stores),
                    Some("media") => fetch::media(&req, &mut net, &mut stores),
                    Some("idb") => stores.get(&site, &top, ancestor).and_then(|s| databases.entry((site.clone(),top.clone(),ancestor,field("origin"))).or_default().handle(&req,s)),
                    Some("mediaType") => Ok(media::handle()),
                    Some("webgl") => webgl::handle(&req, &mut gl, &mut net, &mut stores),
                    Some("permissions") => stores.get(&site, &top, ancestor).and_then(|s| permissions::handle(&req, s)),
                    Some("trim") => Ok(trim(&stores, &mut net, &mut fetch_jobs)),
                    op => Err(format!("mrt-api: no op {op:?}")),
                };
                // Existing API replies used serde_json's sorted object order.
                // IndexedDB clone graphs must instead retain JS property order.
                let r = r.map(|mut value| { if req["op"] != "idb" { value.sort_all_objects(); } value });
                match r {
                    Ok(v) => json!({ "ok": v }),
                    Err(e) => json!({ "err": e }),
                }
            }
        };
        if writeln!(out, "{}", surrogates::write_line(reply.to_string())).and_then(|_| out.flush()).is_err() {
            break;
        }
    }
}
