// The standalone profile is owned by mini-runtime. Firefox comparison runs can override it
// with API_ENV, an explicit recording with entries keyed by secure/isolated context.
use serde_json::{Value, json};

pub const DEFAULT_UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:154.0) Gecko/20100101 Firefox/154.0 MiniRuntime/0.1";

pub fn key(secure: bool, isolated: bool) -> String {
    format!("secure={},isolated={}", secure as u8, isolated as u8)
}

// Also returned by --default-env for the driver: network identity and initial layout must
// agree with the environment exposed to page scripts. No host/browser state is sampled.
pub fn defaults() -> Value {
    let entry = json!({
        "navigator": {
            "userAgent": DEFAULT_UA,
            "appVersion": "5.0 (X11)", "platform": "Linux x86_64",
            "vendor": "", "vendorSub": "", "product": "Gecko", "productSub": "20100101",
            "appName": "Netscape", "appCodeName": "Mozilla",
            "language": "en-US", "languages": ["en-US", "en"],
            "cookieEnabled": true, "onLine": true, "hardwareConcurrency": 4,
            "maxTouchPoints": 0, "doNotTrack": "unspecified", "oscpu": "Linux x86_64",
            "pdfViewerEnabled": false
        },
        "screen": {"width": 1280, "height": 720, "availWidth": 1280, "availHeight": 720,
                   "availLeft": 0, "availTop": 0, "colorDepth": 24, "pixelDepth": 24},
        "innerWidth": 1280, "innerHeight": 720, "outerWidth": 1280, "outerHeight": 720,
        "screenX": 0, "screenY": 0,
        // Reference names drive missing-API instrumentation and touch exposure. Empty lists
        // leave the runtime's implemented interfaces in place without borrowing Firefox's list.
        "names": {"window": [], "Document": [], "HTMLElement": [], "Element": [], "Node": [], "Navigator": []}
    });
    let mut entries = serde_json::Map::new();
    for (secure, isolated) in [(false, false), (true, false), (true, true)] {
        entries.insert(key(secure, isolated), entry.clone());
    }
    json!({"profile": "mini-runtime-desktop-v1", "viewport": [1280, 720], "entries": entries})
}

fn from_profile(req: &Value, profile: &Value) -> Result<Value, String> {
    let flag = |k: &str| req.get(k).and_then(Value::as_bool).unwrap_or(false);
    let k = key(flag("secure"), flag("isolated"));
    let mut env = profile
        .get("entries")
        .and_then(|e| e.get(&k))
        .filter(|e| e.is_object())
        .cloned()
        .ok_or_else(|| format!("environment profile has no {k}"))?;
    if let Some([w, h]) = req
        .get("viewport")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
    {
        env["innerWidth"] = json!(w);
        env["innerHeight"] = json!(h);
    }
    Ok(env)
}

pub fn handle(req: &Value) -> Result<Value, String> {
    let profile = match std::env::var_os("API_ENV") {
        Some(path) => {
            let data =
                std::fs::read(&path).map_err(|e| format!("mrt-api: API_ENV {path:?}: {e}"))?;
            serde_json::from_slice(&data).map_err(|e| format!("mrt-api: API_ENV {path:?}: {e}"))?
        }
        None => defaults(),
    };
    from_profile(req, &profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_contexts_and_viewport() {
        let profile = defaults();
        for (secure, isolated) in [(false, false), (true, false), (true, true)] {
            let env = from_profile(
                &json!({"secure": secure, "isolated": isolated, "viewport": [960, 540]}),
                &profile,
            )
            .unwrap();
            assert_eq!(env["navigator"]["userAgent"], DEFAULT_UA);
            assert_eq!(env["innerWidth"], 960);
            assert_eq!(env["innerHeight"], 540);
            assert_eq!(env["navigator"]["maxTouchPoints"], 0);
            assert!(env["names"]["HTMLElement"].as_array().unwrap().is_empty());
        }
        assert_eq!(profile["viewport"], json!([1280, 720]));
        assert_eq!(profile, defaults());
        assert!(from_profile(&json!({"secure": false, "isolated": true}), &profile).is_err());
    }

    #[test]
    fn explicit_profile_is_not_filled_from_defaults() {
        let profile =
            json!({"entries": {"secure=1,isolated=0": {"navigator": {"userAgent": "comparison"}}}});
        let env = from_profile(&json!({"secure": true, "viewport": [800, 600]}), &profile).unwrap();
        assert_eq!(env["navigator"]["userAgent"], "comparison");
        assert!(env.get("screen").is_none());
        assert!(from_profile(&json!({"secure": false}), &profile).is_err());
    }
}
