//! `fc-match` answered in the process on Android, where there is no Fontconfig: from a table of the
//! phone's own fonts (MRT_FONT_TABLE, written on the host by the project's android/mrt_fonts.py from
//! the phone's fonts.xml and fc-scan of its /system/fonts). Gives the three answers
//! gfxFcPlatformFontList.rs asks fc-match for, in fc-match's output format, so its parsing is shared.
//!
//! The matching is Android's rather than Fontconfig's: a family is a fonts.xml family (`sans-serif`,
//! `serif`, ...), an alias of one (`arial` is `sans-serif`) or a font's own family name (`Roboto`);
//! the first family of the list that the phone has wins, else `sans-serif`, as Gecko on Android
//! falls back to its default font. A fallback for a character takes the fonts in fonts.xml's order,
//! which is Android's fallback order. NOT PORTED: the language of the pattern.
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::sync::OnceLock;

struct Face {
    mXmlFamily: String,
    mFamilies: String,
    mWeight: i32,
    mSlant: i32,
    mPath: String,
    mIndex: u32,
}

struct Table {
    mFaces: Vec<Face>,
    mAliases: HashMap<String, String>,
}

/// The table without the charsets (they are read from the file on a fallback lookup only, so the
/// process does not hold them).
fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = Table { mFaces: Vec::new(), mAliases: HashMap::new() };
        let Ok(path) = std::env::var("MRT_FONT_TABLE") else { return t };
        let Ok(file) = std::fs::File::open(path) else { return t };
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            let parts: Vec<&str> = line.splitn(8, '|').collect();
            match parts.as_slice() {
                ["A", alias, to] => {
                    t.mAliases.insert(alias.to_string(), to.to_string());
                }
                ["F", xml, fams, weight, slant, path, index, _charset] => t.mFaces.push(Face {
                    mXmlFamily: xml.to_string(),
                    mFamilies: fams.to_string(),
                    mWeight: weight.parse().unwrap_or(80),
                    mSlant: slant.parse().unwrap_or(0),
                    mPath: path.to_string(),
                    mIndex: index.parse().unwrap_or(0),
                }),
                _ => {}
            }
        }
        t
    })
}

/// A pattern as gfxFcPlatformFontList.rs builds it: unescaped families, weight, slant.
fn ParsePattern(aPattern: &str) -> (Vec<String>, i32, i32) {
    let (mut families, mut cur, mut chars) = (Vec::new(), String::new(), aPattern.chars());
    let mut rest = String::new();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            ',' => families.push(std::mem::take(&mut cur)),
            ':' => {
                rest = chars.collect();
                break;
            }
            _ => cur.push(c),
        }
    }
    families.push(cur);
    let (mut weight, mut slant) = (80, 0);
    for prop in rest.split(':') {
        match prop.split_once('=') {
            Some(("weight", v)) => weight = v.parse().unwrap_or(80),
            Some(("slant", v)) => slant = v.parse().unwrap_or(0),
            _ => {}
        }
    }
    (families.into_iter().map(|f| f.trim().to_lowercase()).filter(|f| !f.is_empty()).collect(), weight, slant)
}

/// The faces of one family name: a fonts.xml family (through its aliases), else a font's own family.
fn FacesOf<'a>(aTable: &'a Table, aFamily: &str) -> Vec<&'a Face> {
    let mut name = aFamily.to_string();
    for _ in 0..4 {
        match aTable.mAliases.get(&name) {
            Some(to) => name = to.clone(),
            None => break,
        }
    }
    // CSS generic families Android's fonts.xml has no entry for
    if matches!(name.as_str(), "system-ui" | "-apple-system" | "ui-sans-serif") {
        name = "sans-serif".into();
    }
    let xml: Vec<&Face> = aTable.mFaces.iter().filter(|f| f.mXmlFamily == name).collect();
    if !xml.is_empty() {
        return xml;
    }
    aTable.mFaces.iter().filter(|f| f.mFamilies.split(',').any(|n| n.trim().eq_ignore_ascii_case(&name))).collect()
}

/// The face closest to the weight and slant: the slant first, then the weight distance.
fn Closest<'a>(aFaces: &[&'a Face], aWeight: i32, aSlant: i32) -> Option<&'a Face> {
    aFaces.iter().copied().min_by_key(|f| ((f.mSlant != aSlant) as i32, (f.mWeight - aWeight).abs()))
}

/// What `fc-match <args>` would print for the three uses of gfxFcPlatformFontList.rs.
pub fn Run(aArgs: &[&str]) -> Option<String> {
    let t = table();
    let sorted = aArgs.contains(&"-s");
    let format = aArgs.iter().position(|a| *a == "-f").and_then(|i| aArgs.get(i + 1)).copied().unwrap_or("");
    let pattern = *aArgs.last()?;
    let (families, weight, slant) = ParsePattern(pattern);
    if sorted {
        // "sans-serif:charset=<hex>": every font with the character, in fonts.xml's order
        let ch = pattern.split(':').find_map(|p| p.strip_prefix("charset=")).and_then(|h| u32::from_str_radix(h, 16).ok())?;
        let file = std::fs::File::open(std::env::var("MRT_FONT_TABLE").ok()?).ok()?;
        let mut out = String::new();
        let mut seen = std::collections::HashSet::new();
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            let parts: Vec<&str> = line.splitn(8, '|').collect();
            if let ["F", _, _, _, _, path, index, charset] = parts.as_slice() {
                if seen.insert((path.to_string(), index.to_string())) && crate::gfxFcPlatformFontList_rs::CharsetHasChar(charset, ch) {
                    out.push_str(&format!("{path}|{index}|{ch:x}\n"));
                }
            }
        }
        return Some(out);
    }
    let face = families
        .iter()
        .find_map(|f| Closest(&FacesOf(t, f), weight, slant))
        .or_else(|| Closest(&FacesOf(t, "sans-serif"), weight, slant))?;
    Some(if format.starts_with("%{family}") {
        // the face's own names and the fonts.xml family it is in, any of which may be the one asked for
        format!("{},{}|{}|{}\n", face.mFamilies, face.mXmlFamily, face.mPath, face.mIndex)
    } else {
        format!("{}\n{}\n", face.mPath, face.mIndex)
    })
}
