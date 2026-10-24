// The values a forwarded call carries, as the runtime encodes them (bridge.js's encodeCryptoValue,
// crypto_bridge.js has the same codec), and the WebIDL conversions Gecko's dictionaries do on them.
//
//   {"$u": 1}                     undefined (a dictionary member that is undefined is left out)
//   {"$b": base64, "$t": type}    a BufferSource: an ArrayBuffer, a typed array or a DataView
//   {"$k": id}                    one of the runtime's CryptoKeys
//   {"$o": [[name, value], ...]}  any other object, by the members a SubtleCrypto dictionary may read
//   [ ... ]                       an array
//   anything else                 a string, number, boolean or null as itself

use base64::Engine;
use base64::alphabet;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig, STANDARD, URL_SAFE_NO_PAD};
use base64::engine::DecodePaddingMode;
use serde_json::{Value, json};

pub fn is_undefined(v: &Value) -> bool {
    v.get("$u").is_some()
}

/// Whether `v` is a JS object (WebIDL's "object" in a union with DOMString).
pub fn is_object(v: &Value) -> bool {
    match v {
        Value::Array(_) => true,
        Value::Object(o) => o.contains_key("$o") || o.contains_key("$b") || o.contains_key("$k"),
        _ => false,
    }
}

/// The bytes of a BufferSource, with its type ("ArrayBuffer", "Uint8Array", "DataView", ...).
pub fn bytes(v: &Value) -> Option<(Vec<u8>, &str)> {
    let b = v.get("$b")?.as_str()?;
    let t = v.get("$t").and_then(Value::as_str).unwrap_or("ArrayBuffer");
    Some((STANDARD.decode(b).ok()?, t))
}

pub fn encode_bytes(b: &[u8], t: &str) -> Value {
    json!({ "$b": STANDARD.encode(b), "$t": t })
}

pub fn array_buffer(b: &[u8]) -> Value {
    encode_bytes(b, "ArrayBuffer")
}

/// An object's members, in order; empty for an object that has none the runtime sends (an array,
/// a buffer, a key).
pub fn members(v: &Value) -> &[Value] {
    v.get("$o").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

/// A member of an object, None when it is absent or undefined.
pub fn member<'a>(v: &'a Value, name: &str) -> Option<&'a Value> {
    members(v).iter().find_map(|m| {
        let pair = m.as_array()?;
        if pair.first()?.as_str()? != name {
            return None;
        }
        let x = pair.get(1)?;
        if is_undefined(x) { None } else { Some(x) }
    })
}

pub fn object(members: Vec<(&str, Value)>) -> Value {
    json!({ "$o": members.into_iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>() })
}

/// JS's Number::toString, for the integers and simple fractions a dictionary holds.
fn number_string(n: f64) -> String {
    if n.is_nan() {
        "NaN".into()
    } else if n.is_infinite() {
        if n > 0.0 { "Infinity".into() } else { "-Infinity".into() }
    } else if n == n.trunc() && n.abs() < 1e21 {
        format!("{}", n as i128)
    } else {
        format!("{}", n)
    }
}

/// JS's ToString.
pub fn to_string(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => number_string(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .map(|x| if x.is_null() || is_undefined(x) { String::new() } else { to_string(x) })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(o) => {
            if o.contains_key("$u") {
                "undefined".into()
            } else if o.contains_key("$k") {
                "[object CryptoKey]".into()
            } else if let Some(t) = o.get("$t").and_then(Value::as_str) {
                format!("[object {t}]")
            } else {
                "[object Object]".into()
            }
        }
    }
}

/// JS's ToNumber.
pub fn to_number(v: &Value) -> f64 {
    match v {
        Value::Null => 0.0,
        Value::Bool(b) => *b as u8 as f64,
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(s) => string_number(s),
        Value::Array(_) => string_number(&to_string(v)),
        Value::Object(_) => f64::NAN,
    }
}

fn string_number(s: &str) -> f64 {
    let t = s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if t.is_empty() {
        return 0.0;
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(digits) = t.strip_prefix(prefix) {
            return u128::from_str_radix(digits, radix).map(|n| n as f64).unwrap_or(f64::NAN);
        }
    }
    // Rust also reads "inf" and "nan", which JS does not
    if t.chars().any(|c| c.is_ascii_alphabetic() && c != 'e' && c != 'E') {
        return f64::NAN;
    }
    t.parse().unwrap_or(f64::NAN)
}

/// An [EnforceRange] integer type of at most `max`; None where the conversion throws.
pub fn enforce_range(v: &Value, max: f64) -> Option<u64> {
    let n = to_number(v);
    if !n.is_finite() {
        return None;
    }
    let n = n.trunc();
    if n < 0.0 || n > max { None } else { Some(n as u64) }
}

/// WebIDL's unsigned long without [EnforceRange]: modulo 2^32.
pub fn unsigned_long(v: &Value) -> u32 {
    let n = to_number(v);
    if !n.is_finite() {
        return 0;
    }
    n.trunc().rem_euclid(4294967296.0) as u32
}

/// JS's ToBoolean.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) => true,
        Value::Object(o) => !o.contains_key("$u"),
    }
}

/// A sequence<DOMString>: an array's elements as strings; None for a value that is not iterable.
pub fn string_sequence(v: &Value) -> Option<Vec<String>> {
    Some(v.as_array()?.iter().map(to_string).collect())
}

/// Base64url without padding, as a JWK member is (CryptoBuffer::ToJwkBase64).
pub fn jwk_base64(b: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(b)
}

/// A JWK member's bytes (CryptoBuffer::FromJwkBase64: whitespace stripped, padding refused).
pub fn from_jwk_base64(s: &str) -> Option<Vec<u8>> {
    let t: String = s.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if t.contains('=') {
        return None;
    }
    // Gecko's decoder takes the unused bits of the last character as they come
    const LENIENT: GeneralPurpose = GeneralPurpose::new(
        &alphabet::URL_SAFE,
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::RequireNone).with_decode_allow_trailing_bits(true),
    );
    LENIENT.decode(t).ok()
}

pub fn object_owned(members: Vec<(String, Value)>) -> Value {
    json!({ "$o": members.into_iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>() })
}
