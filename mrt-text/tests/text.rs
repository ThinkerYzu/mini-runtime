// Tests for the font and shaping layer. They need `fc-match` and at least one installed font; the
// kerning test skips itself when the system sans-serif does not kern "AV".
use mrt_text::*;

fn sans(aSize: f64) -> gfxFont {
    gfxFont::Lookup(&["sans-serif"], gfxFontStyle { size: aSize, ..Default::default() }).expect("no sans-serif font (fc-match)")
}

fn utf16(s: &str) -> Vec<u16> { s.encode_utf16().collect() }

#[test]
fn harfbuzz_is_firefoxs() {
    assert!(mrt_text::harfbuzz::version().starts_with("14."), "{}", mrt_text::harfbuzz::version());
}

#[test]
fn fc_match_finds_a_file_and_caches() {
    let a = FindFontFile(&["sans-serif"], 400.0, false).expect("fc-match");
    assert!(std::path::Path::new(&a.mPath).exists(), "{a:?}");
    assert_eq!(FindFontFile(&["sans-serif"], 400.0, false), Some(a));
    // A family that does not exist still gets Fontconfig's substitute.
    assert!(FindFontFile(&["NoSuchFamilyXyz", "serif"], 700.0, true).is_some());
}

#[test]
fn fc_weight_scale() {
    assert_eq!(FcWeightFromOpenType(400.0), 80);
    assert_eq!(FcWeightFromOpenType(700.0), 200);
    assert_eq!(FcWeightFromOpenType(100.0), 0);
    assert_eq!(BuildPattern(&["Foo Bar", "sans-serif"], 700.0, true), "Foo Bar,sans\\-serif:weight=200:slant=100");
}

#[test]
fn metrics_are_plausible() {
    let f = sans(16.0);
    let m = f.GetMetrics();
    assert!(f.IsValid());
    assert!(m.maxAscent + m.maxDescent > 0.0, "{m:?}");
    assert_eq!(m.emHeight, 16.0);
    assert!((m.emAscent + m.emDescent - m.emHeight).abs() < 1e-9);
    assert!(m.xHeight > 4.0 && m.xHeight < 12.0, "{m:?}");
    assert!(m.spaceWidth > 2.0 && m.spaceWidth < 8.0, "{m:?}");
    // The line is a whole number of pixels, as gfxFT2FontBase makes it.
    let line = m.internalLeading + m.emHeight + m.externalLeading;
    assert_eq!(line, line.round());

    let fm = nsFontMetrics::new(&f);
    assert_eq!(fm.EmHeight(), 16 * 60);
    assert!(fm.MaxAscent() > 0 && fm.MaxDescent() > 0);
    assert_eq!(fm.MaxHeight(), fm.MaxAscent() + fm.MaxDescent());
    assert!(fm.SpaceWidth() > 0 && fm.XHeight() > 0);
}

#[test]
fn metrics_scale_with_size() {
    let a = sans(16.0);
    let b = sans(32.0);
    let r = b.GetMetrics().xHeight / a.GetMetrics().xHeight;
    assert!((r - 2.0).abs() < 0.01, "{r}");
}

#[test]
fn hello_world_advances_add_up() {
    let f = sans(16.0);
    let text = utf16("Hello world");
    let run = f.ShapeText(&text, false, Some("en"));
    assert_eq!(run.mCharAdvance.len(), 11);
    assert!(run.mGlyphs.len() >= 10);
    let by_char: i32 = run.mCharAdvance.iter().sum();
    let by_glyph: i32 = run.mGlyphAdvance.iter().sum();
    assert_eq!(run.mWidth, by_char);
    assert_eq!(by_char, by_glyph);
    // 11 characters at 16px: between 3px and 14px each.
    let px = run.mWidth as f64 / 60.0;
    assert!(px > 11.0 * 3.0 && px < 11.0 * 14.0, "{px}");
    // The space is as wide as the metrics say (no kerning on a plain space here, so allow 1 app unit).
    let sp = f.ShapeText(&utf16(" "), false, None).mWidth;
    assert!((sp - nsFontMetrics::new(&f).SpaceWidth()).abs() <= 1, "{sp}");
    assert_eq!(nsFontMetrics::new(&f).GetWidthUtf8("Hello world"), run.mWidth);
}

#[test]
fn kerning_av_is_narrower_than_a_plus_v() {
    let f = sans(48.0);
    let w = |s: &str| f.ShapeText(&utf16(s), false, Some("en")).mWidth;
    let (av, a, v) = (w("AV"), w("A"), w("V"));
    if av >= a + v {
        eprintln!("skipped: this font does not kern AV ({av} vs {})", a + v);
        return;
    }
    assert!(av < a + v);
}

#[test]
fn doubling_the_size_doubles_the_width() {
    let w16 = sans(16.0).ShapeText(&utf16("The quick brown fox"), false, None).mWidth as f64;
    let w32 = sans(32.0).ShapeText(&utf16("The quick brown fox"), false, None).mWidth as f64;
    assert!((w32 / w16 - 2.0).abs() < 0.02, "{w16} {w32}");
}

#[test]
fn supplementary_characters_and_clusters() {
    // U+1F600 is two UTF-16 units; the second must get advance 0 (cluster continuation).
    let f = sans(16.0);
    let t = utf16("a\u{1F600}b");
    assert_eq!(t.len(), 4);
    let run = f.ShapeText(&t, false, None);
    assert_eq!(run.mCharAdvance.len(), 4);
    assert_eq!(run.mCharAdvance[2], 0);
    assert_eq!(run.mWidth, run.mCharAdvance.iter().sum::<i32>());
}

#[test]
fn combining_marks_and_empty() {
    let f = sans(16.0);
    assert_eq!(f.ShapeText(&[], false, None).mWidth, 0);
    let run = f.ShapeText(&utf16("e\u{0301}"), false, None);
    assert_eq!(run.mCharAdvance.len(), 2);
    assert_eq!(run.mWidth, run.mCharAdvance.iter().sum::<i32>());
}
