// Tests of closed and open <details> / <summary> and of inline <svg>. Gecko has no gtest for them (its tests
// are reftests and mochitests); these are written from the HTML rendering section (details and summary
// elements, https://html.spec.whatwg.org/#the-details-and-summary-elements), CSS Contain 2
// (`content-visibility: hidden` skips the contents) and the CSS 2.2 rules for replaced elements with an
// intrinsic size (10.3.2, 10.6.2). Sizes are set in the snippets, so heights do not depend on a font.
//
// Not a translation of any Firefox file, so there is no `// Gecko:` marker.
#![cfg(test)]

use crate::driver::*;
use crate::*;
use mrt_style::mrt_dom::Dom;
use mrt_style::StyleDom;

fn page(css: &str, body: &str) -> String {
    format!("<!DOCTYPE html><html><head><style>body{{margin:0}} {css}</style></head><body>{body}</body></html>")
}

fn layout(html: &str) -> PageLayout {
    let mut sd = StyleDom::new(Dom::parse(html), (1000.0, 800.0));
    sd.resolve();
    // the frames read their content from the DOM (see nsIContent)
    let sd = KeepDocumentAlive(sd);
    LayoutDocument(&sd, 1000, 800)
}

// The frames of the tree in tree order (all child lists, the marker list first).
fn all_frames(aFrame: nsIFrame) -> Vec<nsIFrame> {
    fn walk(f: nsIFrame, out: &mut Vec<nsIFrame>) {
        out.push(f);
        let mut lists = Vec::new();
        f.GetChildLists(&mut lists);
        lists.sort_by_key(|l| l.mID != FrameChildListID::Marker);
        for l in lists {
            let mut c = l.mList.FirstChild();
            while let Some(k) = c {
                walk(k, out);
                c = k.GetNextSibling();
            }
        }
    }
    let mut out = Vec::new();
    walk(aFrame, &mut out);
    out
}

// The frame of the first element with this tag name (any namespace) that has one, and the number of such frames.
fn frames_of(aRoot: nsIFrame, aTag: &'static str) -> Vec<nsIFrame> {
    all_frames(aRoot)
        .into_iter()
        .filter(|f| f.Type() != LayoutFrameType::Placeholder && f.GetContent().map_or(false, |c| (c.IsHTMLElement(aTag) || (aTag == "svg" && c.IsSVGElement()))))
        .collect()
}

fn rect(aFrame: nsIFrame) -> (f64, f64, f64, f64) {
    BoxInDocument(aFrame)
}

fn svg_rect(html: &str) -> (f64, f64, f64, f64) {
    let result = layout(html);
    let svgs = frames_of(result.mRootElementFrame.unwrap(), "svg");
    assert_eq!(svgs.len(), 1, "one frame for the <svg>");
    rect(svgs[0])
}

fn marker_texts(aFrame: nsIFrame) -> Vec<String> {
    let mut out = Vec::new();
    for f in all_frames(aFrame) {
        if f.Type() == LayoutFrameType::Text {
            if let Some(buffer) = f.GetContent().and_then(|c| c.mCharacterDataBuffer()) {
                let mut units = Vec::new();
                buffer.AppendTo__nsAString(&mut units);
                out.push(String::from_utf16_lossy(&units));
            }
        }
    }
    out
}

const DETAILS: &str = "<details><summary style='line-height:20px;font-size:16px'>s</summary><p style='margin:0;height:100px'>hidden</p></details>";

// ---- details and summary -------------------------------------------------------------------------

#[test]
fn a_closed_details_has_frames_for_the_details_and_its_summary_only() {
    // HTML rendering, the details element: the first summary child is the summary; the other children are
    // in the details-content, which is `content-visibility: hidden` while the element is closed.
    let result = layout(&page("", DETAILS));
    let root = result.mRootElementFrame.unwrap();
    assert_eq!(frames_of(root, "details").len(), 1);
    assert_eq!(frames_of(root, "summary").len(), 1);
    assert_eq!(frames_of(root, "p").len(), 0, "the content of a closed details has no frames");
}

#[test]
fn an_open_details_shows_its_content_below_the_summary() {
    let result = layout(&page("", &DETAILS.replace("<details>", "<details open>")));
    let root = result.mRootElementFrame.unwrap();
    let summary = rect(frames_of(root, "summary")[0]);
    let p = rect(frames_of(root, "p")[0]);
    assert_eq!(summary.3, 20.0);
    assert_eq!(p.1, summary.1 + 20.0);
    assert_eq!(p.3, 100.0);
    assert_eq!(rect(frames_of(root, "details")[0]).3, 120.0);
}

#[test]
fn a_closed_details_is_as_tall_as_its_summary() {
    let result = layout(&page("", DETAILS));
    let root = result.mRootElementFrame.unwrap();
    assert_eq!(rect(frames_of(root, "details")[0]).3, 20.0);
    assert_eq!(rect(frames_of(root, "summary")[0]), (0.0, 0.0, 1000.0, 20.0));
}

#[test]
fn only_the_first_summary_is_the_summary_of_a_closed_details() {
    // A second <summary> is ordinary content of the details-content: hidden while closed.
    let html = page("", "<details><summary style='line-height:20px'>a</summary><summary style='line-height:20px'>b</summary></details>");
    let result = layout(&html);
    assert_eq!(frames_of(result.mRootElementFrame.unwrap(), "summary").len(), 1);
    assert_eq!(result.RootElementHeightInCSSPixels(), Some(20.0));
}

#[test]
fn a_closed_details_without_a_summary_shows_nothing() {
    // (Gecko makes an anonymous default summary with the text "Details"; that one is NOT PORTED here: the
    // element then has no frames below it.)
    let result = layout(&page("", "<details><p style='height:100px'>x</p></details>"));
    assert_eq!(frames_of(result.mRootElementFrame.unwrap(), "p").len(), 0);
}

#[test]
fn the_summary_has_a_disclosure_marker_that_follows_the_open_state() {
    // ua.css: `details > summary:first-of-type { display: list-item; list-style: disclosure-closed inside }`
    // and `details[open] > summary:first-of-type { list-style-type: disclosure-open }`.
    let closed = marker_texts(layout(&page("", DETAILS)).mRootElementFrame.unwrap());
    assert!(closed.iter().any(|t| t == "\u{25b8} "), "{closed:?}");
    let open = marker_texts(layout(&page("", &DETAILS.replace("<details>", "<details open>"))).mRootElementFrame.unwrap());
    assert!(open.iter().any(|t| t == "\u{25be} "), "{open:?}");
}

#[test]
fn a_flex_summary_is_a_flex_container_with_its_children_as_items() {
    // mdn-css: `summary { display: flex }`. The disclosure marker is not generated for it (it is not a list
    // item), the content is laid out as flex items.
    let html = page("summary{display:flex;height:32px}", "<details><summary><span style='display:block;width:50px;height:10px'></span><span style='display:block;width:30px;height:10px'></span></summary><p>x</p></details>");
    let result = layout(&html);
    let root = result.mRootElementFrame.unwrap();
    let summaries = frames_of(root, "summary");
    assert_eq!(summaries[0].Type(), LayoutFrameType::FlexContainer);
    assert_eq!(rect(summaries[0]).3, 32.0);
    let spans = frames_of(root, "span");
    assert_eq!(spans.len(), 2);
    assert_eq!(rect(spans[0]).0, 0.0);
    assert_eq!(rect(spans[1]).0, 50.0);
}

#[test]
fn nested_closed_details_are_hidden_with_their_parent() {
    let html = page("", "<details><summary style='line-height:20px'>a</summary><details><summary>b</summary></details></details>");
    let result = layout(&html);
    assert_eq!(frames_of(result.mRootElementFrame.unwrap(), "details").len(), 1);
}

// ---- inline svg ----------------------------------------------------------------------------------

#[test]
fn an_svg_without_size_is_300_by_150() {
    // SVG 2 "Sizing SVG in CSS" and CSS 2.2 10.3.2: no intrinsic width, height or ratio: 300 by 150.
    assert_eq!(svg_rect(&page("", "<svg></svg>")).2, 300.0);
    assert_eq!(svg_rect(&page("", "<svg></svg>")).3, 150.0);
}

#[test]
fn an_svg_takes_its_size_from_the_width_and_height_attributes() {
    let r = svg_rect(&page("", "<svg width=40 height=30></svg>"));
    assert_eq!((r.2, r.3), (40.0, 30.0));
    let r = svg_rect(&page("", "<svg width='2em' height='10px'></svg>"));
    assert_eq!((r.2, r.3), (32.0, 10.0), "em is the font size of the element, 16px");
}

#[test]
fn css_width_and_height_override_the_attributes() {
    let r = svg_rect(&page("svg{width:80px;height:20px}", "<svg width=40 height=30></svg>"));
    assert_eq!((r.2, r.3), (80.0, 20.0));
}

#[test]
fn an_svg_with_a_view_box_and_a_width_has_the_ratio_of_the_view_box() {
    // width given, height auto: the height follows the viewBox ratio (CSS 2.2 10.6.2): 200 * 50 / 100.
    let r = svg_rect(&page("", "<svg width=200 viewBox='0 0 100 50'></svg>"));
    assert_eq!((r.2, r.3), (200.0, 100.0));
    // height given, width auto
    let r = svg_rect(&page("", "<svg height=40 viewBox='0 0 100 50'></svg>"));
    assert_eq!((r.2, r.3), (80.0, 40.0));
}

#[test]
fn a_block_svg_with_a_view_box_only_fills_the_line_and_follows_the_ratio() {
    // display:block, no sizes, viewBox 2:1: the width is the containing block's (1000), the height half.
    let r = svg_rect(&page("svg{display:block}", "<svg viewBox='0 0 100 50'></svg>"));
    assert_eq!((r.2, r.3), (1000.0, 500.0));
}

#[test]
fn an_svg_with_a_percentage_width_resolves_it_against_the_containing_block() {
    let r = svg_rect(&page("", "<div style='width:400px'><svg width='50%' height=10></svg></div>"));
    assert_eq!((r.2, r.3), (200.0, 10.0));
}

#[test]
fn an_inline_svg_sits_in_a_line_and_a_block_svg_does_not() {
    // inline-level: the line holds the box (inline-block like), so the text and the svg share a line
    let result = layout(&page("", "<p style='margin:0;line-height:20px'>a<svg width=40 height=60></svg>b</p>"));
    let root = result.mRootElementFrame.unwrap();
    let p = rect(frames_of(root, "p")[0]);
    assert!(p.3 >= 60.0 && p.3 < 90.0, "{p:?}");
    let r = svg_rect(&page("", "<svg style='display:block' width=40 height=60></svg>"));
    assert_eq!((r.0, r.1, r.2, r.3), (0.0, 0.0, 40.0, 60.0));
}

#[test]
fn the_children_of_an_svg_get_no_frames() {
    let result = layout(&page("", "<svg width=40 height=30><g><rect width=10 height=10></rect></g><text>hello</text></svg>"));
    let root = result.mRootElementFrame.unwrap();
    let svgs = frames_of(root, "svg");
    assert_eq!(svgs.len(), 1);
    let mut lists = Vec::new();
    svgs[0].GetChildLists(&mut lists);
    assert!(lists.iter().all(|l| l.mList.FirstChild().is_none()), "no child frames");
    assert!(marker_texts(root).iter().all(|t| t != "hello"));
}

#[test]
fn an_svg_view_box_with_commas_and_a_zero_size_is_read_like_the_spec_says() {
    // viewBox accepts commas; a zero width is a degenerate ratio: no ratio, so 300 by 150.
    let r = svg_rect(&page("", "<svg viewBox='0,0,100,50' width=100></svg>"));
    assert_eq!((r.2, r.3), (100.0, 50.0));
    let r = svg_rect(&page("", "<svg viewBox='0 0 0 50'></svg>"));
    assert_eq!((r.2, r.3), (300.0, 150.0));
}

#[test]
fn an_svg_with_a_wrong_width_attribute_ignores_it() {
    let r = svg_rect(&page("", "<svg width='abc' height=10></svg>"));
    assert_eq!((r.2, r.3), (300.0, 10.0));
    let r = svg_rect(&page("", "<svg width='-5' height=10></svg>"));
    assert_eq!((r.2, r.3), (300.0, 10.0));
}
