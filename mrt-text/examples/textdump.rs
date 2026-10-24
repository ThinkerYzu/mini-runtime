// Prints font metrics (CSS px and app units) and widths of a few strings, for comparing with Firefox.
// usage: textdump [family[,family...]] [size-px] [weight]
use mrt_text::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let fam = a.get(1).cloned().unwrap_or("sans-serif".into());
    let size: f64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(16.0);
    let weight: f32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(400.0);
    let fams: Vec<&str> = fam.split(',').collect();
    let json = a.iter().any(|x| x == "--json");
    let t0 = std::time::Instant::now();
    let f = gfxFont::Lookup(&fams, gfxFontStyle { size, weight, italic: false }).expect("font");
    if json {
        let fm = nsFontMetrics::new(&f);
        let strings = ["Hello world", "AV", "The quick brown fox jumps over the lazy dog", "office fi fl",
            "Wikipedia, the free encyclopedia", "W.A.V.E. To", " "];
        let widths: Vec<String> = strings.iter().map(|s| format!("{:?}: {}", s, fm.GetWidthUtf8(s) as f64 / 60.0)).collect();
        println!("{{\"widths\": {{{}}}, \"lineHeight\": {}, \"baselineFromTop\": {}, \"ex\": {}, \"space\": {}}}",
            widths.join(", "), (fm.MaxAscent() + fm.MaxDescent() + fm.ExternalLeading()) as f64 / 60.0,
            fm.MaxAscent() as f64 / 60.0, fm.XHeight() as f64 / 60.0, fm.GetWidthUtf8(" ") as f64 / 60.0);
        return;
    }
    println!("file {:?}  lookup+open+metrics {:?}", f.GetFontEntry().mFile, t0.elapsed());
    println!("{:#?}", f.GetMetrics());
    let fm = nsFontMetrics::new(&f);
    println!("app units: em {} maxAscent {} maxDescent {} extLeading {} intLeading {} xHeight {} space {} aveChar {}",
        fm.EmHeight(), fm.MaxAscent(), fm.MaxDescent(), fm.ExternalLeading(), fm.InternalLeading(), fm.XHeight(), fm.SpaceWidth(), fm.AveCharWidth());
    for s in ["Hello world", "AV", "The quick brown fox jumps over the lazy dog", "office fi fl", "Wikipedia, the free encyclopedia", "W.A.V.E. To"] {
        println!("{:>5} au {:>8.3}px  {s}", fm.GetWidthUtf8(s), fm.GetWidthUtf8(s) as f64 / 60.0);
    }
}
