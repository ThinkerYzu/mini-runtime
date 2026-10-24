// Reproduces the crash seen in mrt-layout's tests: several threads each build and style their own
// StyleDom at the same time.
use mrt_style::mrt_dom::Dom;
use mrt_style::StyleDom;

// Custom element and attribute names have no static atom, so every thread adds some to the shared table.
const HTML: &str = "<!DOCTYPE html><html><head><style>.a{font-size:20px}</style></head><body><div class=a style='float:left'>x</div><my-widget data-x=1 foo-bar=2>y</my-widget><p>z</p></body></html>";

#[test]
fn styles_in_parallel_threads() {
    let threads: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                for _ in 0..20 {
                    let mut sd = StyleDom::new(Dom::parse(HTML), (800.0, 600.0));
                    sd.resolve();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
}
