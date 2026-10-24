//! Font lookup by CSS family list, weight and style, answered by `fc-match` and cached.
//! Stands in for gfxFcPlatformFontList (which talks to Fontconfig's library): same question, asked
//! through the command line so that no Fontconfig or FreeType code is linked.
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FontFile {
    pub mPath: String,
    /// Face number inside a .ttc collection (0 for a plain font file).
    pub mIndex: u32,
}

// Gecko: gfx/thebes/gfxFcPlatformFontList.cpp (FcWeightFromOpenType, via FcWeightFromOpenTypeDouble)
// Fontconfig's own weight scale: 0..=215 for CSS/OpenType 100..=1000 (400 is 80, 700 is 200).
pub fn FcWeightFromOpenType(aWeight: f32) -> i32 {
    const MAP: [(f32, f32); 12] = [
        (0.0, -1.0), (100.0, 0.0), (200.0, 40.0), (300.0, 50.0), (350.0, 55.0), (380.0, 75.0),
        (400.0, 80.0), (500.0, 100.0), (600.0, 180.0), (700.0, 200.0), (800.0, 205.0), (900.0, 210.0),
    ];
    let w = aWeight.clamp(1.0, 1000.0);
    for pair in MAP.windows(2) {
        let (x0, y0) = pair[0];
        let (x1, y1) = pair[1];
        if w <= x1 {
            return (y0 + (y1 - y0) * (w - x0) / (x1 - x0)).round() as i32;
        }
    }
    let (x0, y0) = MAP[MAP.len() - 1];
    (y0 + (215.0 - y0) * (w - x0) / (1000.0 - x0)).round() as i32
}

/// The output of `fc-match <aArgs>`, or `None` if it failed. On Android, which has no Fontconfig, the
/// same answers come from a table of the phone's fonts (AndroidFontTable.rs).
fn FcMatch(aArgs: &[&str]) -> Option<String> {
    #[cfg(target_os = "android")]
    return crate::AndroidFontTable_rs::Run(aArgs);
    #[cfg(not(target_os = "android"))]
    {
        let out = std::process::Command::new("fc-match").args(aArgs).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

static sCache: Mutex<Option<HashMap<String, Option<FontFile>>>> = Mutex::new(None);

/// Quote one family for a Fontconfig pattern: backslash-escape the characters that mean something
/// in the pattern syntax.
fn EscapeFamily(aFamily: &str) -> String {
    let mut out = String::new();
    for c in aFamily.chars() {
        if matches!(c, '\\' | '-' | ':' | ',' | '=') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Pattern text for `fc-match`: `family1,family2:weight=80:slant=0`.
pub fn BuildPattern(aFamilies: &[&str], aWeight: f32, aItalic: bool) -> String {
    BuildPatternForLanguage(aFamilies, aWeight, aItalic, None)
}

/// BuildPattern with the language of the text (`family1,family2:lang=my:weight=80:slant=0`): fontconfig
/// prefers the fonts of the families that cover the language, which is how a generic family such as `serif`
/// becomes a different font for a different language.
pub fn BuildPatternForLanguage(aFamilies: &[&str], aWeight: f32, aItalic: bool, aLanguage: Option<&str>) -> String {
    let families: Vec<String> = aFamilies.iter().map(|f| EscapeFamily(f.trim_matches(|c| c == '"' || c == '\''))).collect();
    let lang = match aLanguage {
        // a language tag has letters, digits and hyphens; anything else is left out rather than quoted
        Some(l) if !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') => format!(":lang={l}"),
        _ => String::new(),
    };
    format!(
        "{}{}:weight={}:slant={}",
        families.join(","),
        lang,
        FcWeightFromOpenType(aWeight),
        if aItalic { 100 } else { 0 }
    )
}

// Gecko: gfx/thebes/gfxPlatformFontList.cpp gfxPlatformFontList::FindAndAddFamilies (the idea)
/// Find the font file for a CSS family list ("Helvetica, sans-serif"), weight and style. Fontconfig
/// always answers with its best substitute, so `None` only means `fc-match` itself failed (not
/// installed, no fonts). Results are cached by pattern.
pub fn FindFontFile(aFamilies: &[&str], aWeight: f32, aItalic: bool) -> Option<FontFile> {
    FindFontFileForLanguage(aFamilies, aWeight, aItalic, None)
}

/// FindFontFile for text of a language (see BuildPatternForLanguage).
pub fn FindFontFileForLanguage(aFamilies: &[&str], aWeight: f32, aItalic: bool, aLanguage: Option<&str>) -> Option<FontFile> {
    if aLanguage.is_some() {
        PrefetchLanguages(aFamilies, aWeight, aItalic);
    }
    let pattern = BuildPatternForLanguage(aFamilies, aWeight, aItalic, aLanguage);
    {
        let mut cache = sCache.lock().unwrap();
        if let Some(hit) = cache.get_or_insert_with(HashMap::new).get(&pattern) {
            return hit.clone();
        }
    }
    let result = RunFcMatch(&pattern);
    sCache.lock().unwrap().get_or_insert_with(HashMap::new).insert(pattern, result.clone());
    result
}

fn RunFcMatch(aPattern: &str) -> Option<FontFile> {
    let text = FcMatch(&["-f", "%{file}\n%{index}\n", aPattern])?;
    let mut lines = text.lines();
    let path = lines.next()?.trim().to_string();
    if path.is_empty() {
        return None;
    }
    let index = lines.next().and_then(|l| l.trim().parse::<u32>().ok()).unwrap_or(0);
    Some(FontFile { mPath: path, mIndex: index })
}

static sKnownLanguages: Mutex<Vec<String>> = Mutex::new(Vec::new());
// per family list, weight and slant: how many languages were asked for, and whether the rest were fetched
static sPrefetched: Mutex<Option<HashMap<String, (u32, bool)>>> = Mutex::new(None);

/// The languages of the page (the values of its `lang` attributes). The first lookup of a family list for a
/// language runs fc-match for the same family list and every language of the page at once, in parallel,
/// because a page can have hundreds (the language list of a Wikimedia page has 300) and a run takes 20 ms.
pub fn SetKnownLanguages(aLanguages: Vec<String>) {
    *sKnownLanguages.lock().unwrap() = aLanguages;
    // a new page: what was prefetched for the last one stays valid (the cache is by pattern), the set of
    // shapes asked for is forgotten so that the new languages are fetched too
    *sPrefetched.lock().unwrap() = None;
}

fn PrefetchLanguages(aFamilies: &[&str], aWeight: f32, aItalic: bool) {
    // the same family list, weight and slant, whatever the language
    let shape = BuildPatternForLanguage(aFamilies, aWeight, aItalic, None);
    {
        // a few languages are fetched one by one; the 24th asked for in the same shape starts the rest
        let mut done = sPrefetched.lock().unwrap();
        let entry = done.get_or_insert_with(Default::default).entry(shape).or_insert((0, false));
        if entry.1 {
            return;
        }
        entry.0 += 1;
        if entry.0 < 24 {
            return;
        }
        entry.1 = true;
    }
    let patterns: Vec<String> = {
        let langs = sKnownLanguages.lock().unwrap();
        let cache = sCache.lock().unwrap();
        langs
            .iter()
            .map(|l| BuildPatternForLanguage(aFamilies, aWeight, aItalic, Some(l)))
            .filter(|p| !cache.as_ref().map_or(false, |c| c.contains_key(p)))
            .collect()
    };
    if patterns.len() < 2 {
        return;
    }
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).min(8);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Mutex<Vec<(String, Option<FontFile>)>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(pattern) = patterns.get(i) else { break };
                let found = RunFcMatch(pattern);
                results.lock().unwrap().push((pattern.clone(), found));
            });
        }
    });
    let mut cache = sCache.lock().unwrap();
    let cache = cache.get_or_insert_with(HashMap::new);
    for (pattern, found) in results.into_inner().unwrap() {
        cache.insert(pattern, found);
    }
}

static sInstalledCache: Mutex<Option<HashMap<(String, i32, bool), Option<FontFile>>>> = Mutex::new(None);

// Gecko: gfx/thebes/gfxPlatformFontList.cpp gfxPlatformFontList::FindFamilyByCanonicalName (the idea)
/// The font file of a face of the family named aFamily (the closest face to the weight and slant), or `None`
/// if no installed family has that name: `fc-match` always answers with its best substitute, so the answer
/// is checked against the family names it gives.
pub fn FindInstalledFamilyFontFile(aFamily: &str, aWeight: f32, aItalic: bool) -> Option<FontFile> {
    let key = (aFamily.to_string(), FcWeightFromOpenType(aWeight), aItalic);
    {
        let mut cache = sInstalledCache.lock().unwrap();
        if let Some(hit) = cache.get_or_insert_with(HashMap::new).get(&key) {
            return hit.clone();
        }
    }
    let result = RunFcMatchFamily(&BuildPattern(&[aFamily], aWeight, aItalic), aFamily);
    sInstalledCache.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, result.clone());
    result
}

fn RunFcMatchFamily(aPattern: &str, aFamily: &str) -> Option<FontFile> {
    let text = FcMatch(&["-f", "%{family}|%{file}|%{index}\n", aPattern])?;
    let mut parts = text.trim_end().splitn(3, '|');
    let (families, path, index) = (parts.next()?, parts.next()?, parts.next()?);
    // the family names of the face, comma separated, any of which may be the one asked for
    if !families.split(',').any(|f| f.trim().eq_ignore_ascii_case(aFamily)) || path.is_empty() {
        return None;
    }
    Some(FontFile { mPath: path.to_string(), mIndex: index.trim().parse::<u32>().unwrap_or(0) })
}

static sFallbackCache: Mutex<Option<HashMap<(u32, i32, bool), Vec<FontFile>>>> = Mutex::new(None);

/// Does a fontconfig `%{charset}` string ("20-7e a0-377 1000-109f 2026 ...": hex ranges and single
/// code points) contain the code point?
pub(crate) fn CharsetHasChar(aCharset: &str, aCh: u32) -> bool {
    aCharset.split_whitespace().any(|part| match part.split_once('-') {
        Some((lo, hi)) => match (u32::from_str_radix(lo, 16), u32::from_str_radix(hi, 16)) {
            (Ok(lo), Ok(hi)) => lo <= aCh && aCh <= hi,
            _ => false,
        },
        None => u32::from_str_radix(part, 16).map_or(false, |c| c == aCh),
    })
}

// Gecko: gfx/thebes/gfxFcPlatformFontList.cpp gfxFcPlatformFontList::GlobalFontFallback (the idea)
/// The fonts that have a character that none of the fonts of the group has, best first: fontconfig's sorted
/// list of fonts for `sans-serif` with the character's charset in the pattern (FcFontSort, which is what
/// `fc-match -s` prints), without the fonts whose charset lacks the character. Empty if no installed font has
/// it. Results are cached by character, weight and slant.
/// NOT PORTED: the `lang` of the pattern (the language of the text), Gecko's pref fonts tried before
/// the system fallback, and the font visibility levels.
pub fn FindFallbackFontFiles(aCh: u32, aWeight: f32, aItalic: bool) -> Vec<FontFile> {
    let key = (aCh, FcWeightFromOpenType(aWeight), aItalic);
    {
        let mut cache = sFallbackCache.lock().unwrap();
        if let Some(hit) = cache.get_or_insert_with(HashMap::new).get(&key) {
            return hit.clone();
        }
    }
    let pattern = format!("sans-serif:charset={:x}:weight={}:slant={}", aCh, key.1, if aItalic { 100 } else { 0 });
    let result = RunFcMatchSorted(&pattern, aCh);
    sFallbackCache.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, result.clone());
    result
}

fn RunFcMatchSorted(aPattern: &str, aCh: u32) -> Vec<FontFile> {
    let Some(text) = FcMatch(&["-s", "-f", "%{file}|%{index}|%{charset}\n", aPattern]) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for line in text.lines() {
        let mut parts = line.splitn(3, '|');
        let (Some(path), Some(index), Some(charset)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if CharsetHasChar(charset, aCh) {
            files.push(FontFile { mPath: path.to_string(), mIndex: index.trim().parse::<u32>().unwrap_or(0) });
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    // written from fontconfig's documented %{charset} format
    #[test]
    fn a_charset_string_is_ranges_and_single_code_points() {
        let cs = "20-7e a0-377 2026 1000-109f";
        assert!(CharsetHasChar(cs, 0x20) && CharsetHasChar(cs, 0x7e) && CharsetHasChar(cs, 0x2026) && CharsetHasChar(cs, 0x1050));
        assert!(!CharsetHasChar(cs, 0x7f) && !CharsetHasChar(cs, 0x2027) && !CharsetHasChar(cs, 0x10a0) && !CharsetHasChar(cs, 0x1f));
        assert!(!CharsetHasChar("", 0x20));
    }

    // the Myanmar letter KA is in no font of the Latin text but fontconfig knows one; skipped where none is installed
    #[test]
    fn a_fallback_font_has_the_character() {
        if let Some(file) = FindFallbackFontFiles(0x1000, 400.0, false).into_iter().next() {
            let entry = crate::gfxFontEntry::new(file).expect("font file");
            let font = crate::gfxFont::new(entry, crate::gfxFontStyle { size: 16.0, ..Default::default() });
            assert!(font.HasCharacter(0x1000));
        }
    }
}
