// Strings with lone surrogates on the JSON lines (GAPS B161). A page's string is UTF-16 and may
// hold a surrogate without its pair; JSON writes it as a "\uD800" escape, which serde_json refuses
// in a str. The request line is rewritten before it is parsed: each lone surrogate becomes ESC (the
// noncharacter U+10FFFF) followed by U+10F000 plus its offset from U+D800, and a real U+10FFFF
// becomes ESC twice, so nothing collides. The reply line is rewritten back after it is written.
//
// What keeps the private form is what Firefox keeps the surrogate in: localStorage (LSValue stores
// a value that is not valid UTF-16 as UTF-16 code units, and its keys are nsString), and IndexedDB's
// stored values. Every other op is lossy (`make_lossy`), as Gecko's UTF-16 to UTF-8 conversion
// replaces a lone surrogate with U+FFFD (a cookie string, a request body, a URL).

use serde_json::Value;

const ESC: char = '\u{10FFFF}';
const BASE: u32 = 0x10F000;

fn surrogate_char(unit: u32) -> char {
    char::from_u32(BASE + (unit - 0xD800)).unwrap()
}

/// The request line with its lone surrogate escapes in the private form, for serde_json.
pub fn read_line(line: &str) -> String {
    let b = line.as_bytes();
    let hex = |i: usize| -> Option<u32> {
        if b.get(i) == Some(&b'\\') && b.get(i + 1) == Some(&b'u') {
            u32::from_str_radix(line.get(i + 2..i + 6)?, 16).ok()
        } else {
            None
        }
    };
    let mut out = String::with_capacity(line.len());
    let mut in_string = false;
    let mut i = 0;
    while i < b.len() {
        let c = line[i..].chars().next().unwrap();
        if !in_string {
            in_string = c == '"';
            out.push(c);
            i += c.len_utf8();
            continue;
        }
        match c {
            '"' => {
                in_string = false;
                out.push(c);
                i += 1;
            }
            '\\' => {
                let Some(unit) = hex(i) else {
                    // another escape: the backslash and the character it escapes
                    out.push_str(line.get(i..i + 2).unwrap_or(&line[i..]));
                    i += 2;
                    continue;
                };
                match unit {
                    0xD800..=0xDBFF => match hex(i + 6) {
                        Some(low @ 0xDC00..=0xDFFF) => {
                            if unit == 0xDBFF && low == 0xDFFF {
                                out.push(ESC);
                                out.push(ESC);
                            } else {
                                out.push_str(&line[i..i + 12]);
                            }
                            i += 12;
                        }
                        _ => {
                            out.push(ESC);
                            out.push(surrogate_char(unit));
                            i += 6;
                        }
                    },
                    0xDC00..=0xDFFF => {
                        out.push(ESC);
                        out.push(surrogate_char(unit));
                        i += 6;
                    }
                    _ => {
                        out.push_str(&line[i..i + 6]);
                        i += 6;
                    }
                }
            }
            ESC => {
                out.push(ESC);
                out.push(ESC);
                i += c.len_utf8();
            }
            _ => {
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
    out
}

/// The reply line with the private form written as JSON escapes again.
pub fn write_line(line: String) -> String {
    if !line.contains(ESC) {
        return line;
    }
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != ESC {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(ESC) => out.push(ESC),
            Some(n) => out.push_str(&format!("\\u{:04x}", 0xD800 + (n as u32 - BASE))),
            None => {}
        }
    }
    out
}

/// The length in UTF-16 code units of a string in the private form (a lone surrogate is one).
pub fn utf16_len(s: &str) -> usize {
    let mut n = 0;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == ESC {
            n += if chars.next() == Some(ESC) { 2 } else { 1 };
        } else {
            n += c.len_utf16();
        }
    }
    n
}

/// UTF-16 code units, as Firefox's profile stores a localStorage value, in the private form.
pub fn from_utf16(units: &[u16]) -> String {
    let mut out = String::with_capacity(units.len());
    for r in char::decode_utf16(units.iter().copied()) {
        match r {
            Ok(ESC) => {
                out.push(ESC);
                out.push(ESC);
            }
            Ok(c) => out.push(c),
            Err(e) => {
                out.push(ESC);
                out.push(surrogate_char(e.unpaired_surrogate() as u32));
            }
        }
    }
    out
}

fn lossy(s: &str) -> Option<String> {
    if !s.contains(ESC) {
        return None;
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != ESC {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(ESC) => out.push(ESC),
            Some(_) => out.push('\u{FFFD}'),
            None => {}
        }
    }
    Some(out)
}

/// Every string of the request with its lone surrogates replaced by U+FFFD.
pub fn make_lossy(v: &mut Value) {
    match v {
        Value::String(s) => {
            if let Some(l) = lossy(s) {
                *s = l;
            }
        }
        Value::Array(a) => a.iter_mut().for_each(make_lossy),
        Value::Object(o) => o.values_mut().for_each(make_lossy),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(line: &str) -> String {
        let v: Value = serde_json::from_str(&read_line(line)).unwrap();
        write_line(v.to_string())
    }

    #[test]
    fn lone_surrogates_round_trip() {
        assert_eq!(round_trip(r#"["a\ud800b"]"#), r#"["a\ud800b"]"#);
        assert_eq!(round_trip(r#"["\udfff\ud800"]"#), r#"["\udfff\ud800"]"#);
        // a pair stays a character; a real U+10FFFF stays itself, escaped or not
        assert_eq!(round_trip(r#"["😀"]"#), "[\"\u{1F600}\"]");
        assert_eq!(round_trip(r#"["􏿿"]"#), "[\"\u{10FFFF}\"]");
        assert_eq!(round_trip("[\"\u{10FFFF}x\"]"), "[\"\u{10FFFF}x\"]");
        // an escaped backslash before a u is not an escape
        assert_eq!(round_trip(r#"["\\ud800"]"#), r#"["\\ud800"]"#);
        // keys and other values are untouched
        assert_eq!(round_trip(r#"{"k":"\ud800","n":1}"#), r#"{"k":"\ud800","n":1}"#);
    }

    #[test]
    fn lengths_and_lossy() {
        let v: Value = serde_json::from_str(&read_line(r#""a\ud800􏿿""#)).unwrap();
        assert_eq!(utf16_len(v.as_str().unwrap()), 4);
        let mut l = v.clone();
        make_lossy(&mut l);
        assert_eq!(l.as_str().unwrap(), "a\u{FFFD}\u{10FFFF}");
        assert_eq!(from_utf16(&[0x61, 0xD800]), lossy_free("a", 0xD800));
    }

    fn lossy_free(prefix: &str, unit: u32) -> String {
        format!("{prefix}{ESC}{}", surrogate_char(unit))
    }
}
