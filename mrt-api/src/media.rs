//! Native API-process capability reporting: this process has no media decoder.
//! HTTP viewer mode routes queries to its attached browser transport instead.
//! Plain standalone mode still has no playback backend.
use serde_json::{Value, json};

pub fn handle() -> Value {
    // [MediaSource.isTypeSupported(type), HTMLMediaElement.canPlayType(type)]
    // The supported set is empty for every input until a playback backend exists.
    json!([false, ""])
}
