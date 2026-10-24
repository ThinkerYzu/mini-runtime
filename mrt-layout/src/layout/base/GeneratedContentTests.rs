// Tests of generated content: ::before, ::after, ::marker, the `content` property, quotes, counters and
// the numbers of list markers. Gecko has no gtest for this (its tests are reftests and mochitests that
// need a whole browser); these are written from CSS 2.2 chapter 12 (generated content, automatic
// numbering, lists), chapter 12.5 and CSS Lists 3, and HTML's rendering section for `<ol start>`,
// `<ol reversed>` and `<li value>`. Sizes are set in the snippets, so the heights do not depend on a font;
// the text of the generated frames is read back from the frame tree.
//
// Not a translation of any Firefox file, so there is no `// Gecko:` marker.
#![cfg(test)]

use crate::driver::*;
use crate::*;
use mrt_style::mrt_dom::Dom;
use mrt_style::StyleDom;

const BODY: &str = "<!DOCTYPE html><html><head><style>body{margin:0}</style></head><body>";

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

fn height(html: &str) -> f64 {
    layout(html).RootElementHeightInCSSPixels().unwrap()
}

// The text of every text frame (not continuations) in tree order.
fn texts(aFrame: nsIFrame) -> Vec<String> {
    fn walk(f: nsIFrame, out: &mut Vec<String>) {
        if f.Type() == LayoutFrameType::Text {
            if let Some(content) = f.GetContent() {
                if let Some(buffer) = &content.mCharacterDataBuffer() {
                    let mut units = Vec::new();
                    buffer.AppendTo__nsAString(&mut units);
                    out.push(String::from_utf16_lossy(&units));
                }
            }
        }
        let mut lists = Vec::new();
        f.GetChildLists(&mut lists);
        // the outside marker is in a list of its own, after the principal list; it is the first
        // child in tree order
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

fn texts_of(html: &str) -> Vec<String> {
    texts(layout(html).mRootElementFrame.unwrap())
}

// The marker texts of the page: the texts that end the way a marker does.
fn markers(html: &str) -> Vec<String> {
    texts_of(html).into_iter().filter(|t| t.ends_with(". ") || t.ends_with(' ') && t.chars().count() == 2 || t == " ").collect()
}

// The width of the shrink-to-fit float with id `f`, in CSS px.
fn float_width(html: &str) -> f64 {
    let result = layout(html);
    let mut found = None;
    fn walk(f: nsIFrame, found: &mut Option<f64>) {
        if f.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW) {
            found.get_or_insert(f.GetSize().width as f64 / AppUnitsPerCSSPixel() as f64);
        }
        let mut lists = Vec::new();
        f.GetChildLists(&mut lists);
        for l in lists {
            let mut c = l.mList.FirstChild();
            while let Some(k) = c {
                walk(k, found);
                c = k.GetNextSibling();
            }
        }
    }
    walk(result.mRootElementFrame.unwrap(), &mut found);
    found.expect("a float")
}

// ---- ::before and ::after (CSS 2.2 12.1) -----------------------------------------------------------------

#[test]
fn before_text_comes_before_the_content_and_is_as_wide_as_the_same_text() {
    // CSS 2.2 12.1: ::before inserts generated content before the content of the element. Its text is
    // laid out inline, so the shrink-to-fit width of the float is the width of "abcdx".
    let font = "div{float:left;font:20px sans-serif} ";
    let with = float_width(&page(&format!("{font} div::before{{content:'abcd'}}"), "<div>x</div>"));
    let literal = float_width(&page(font, "<div>abcdx</div>"));
    let without = float_width(&page(font, "<div>x</div>"));
    assert!(with > without + 10.0, "{with} {without}");
    assert!((with - literal).abs() < 0.01, "{with} {literal}");
}

#[test]
fn after_text_comes_after_the_content_and_is_as_wide_as_the_same_text() {
    let font = "div{float:left;font:20px sans-serif} ";
    let with = float_width(&page(&format!("{font} div::after{{content:'abcd'}}"), "<div>x</div>"));
    let literal = float_width(&page(font, "<div>xabcd</div>"));
    assert!((with - literal).abs() < 0.01, "{with} {literal}");
}

#[test]
fn before_is_the_first_child_and_after_the_last() {
    let t = texts_of(&page("p::before{content:'B'} p::after{content:'A'}", "<p>x<b>y</b></p>"));
    assert_eq!(t, ["B", "x", "y", "A"]);
}

#[test]
fn generated_text_is_the_text_of_the_property_value_with_all_its_items() {
    let t = texts_of(&page("p::before{content:'a' 'b' \"c\"}", "<p>x</p>"));
    assert_eq!(t, ["a", "b", "c", "x"]);
}

#[test]
fn a_block_pseudo_element_adds_its_own_height() {
    // CSS 2.2 12.1: the pseudo-element is a box; display:block makes it a block box above the
    // content of the element, and its height adds to the element's.
    let base = page("p{margin:0;line-height:20px} ", "<p>x</p>");
    let before = page("p{margin:0;line-height:20px} p::before{content:'';display:block;height:30px}", "<p>x</p>");
    let after = page("p{margin:0;line-height:20px} p::after{content:'a';display:block;height:30px}", "<p>x</p>");
    assert_eq!(height(&base), 20.0);
    assert_eq!(height(&before), 50.0);
    assert_eq!(height(&after), 50.0);
}

#[test]
fn an_inline_pseudo_element_with_text_adds_no_height_to_a_line() {
    let html = page("p{margin:0;line-height:20px} p::before{content:'abc'} p::after{content:'def'}", "<p>x</p>");
    assert_eq!(height(&html), 20.0);
}

#[test]
fn content_none_and_normal_make_no_pseudo_element() {
    // CSS 2.2 12.2: `normal` computes to `none` for ::before and ::after, which then does not
    // generate a box.
    assert_eq!(texts_of(&page("p::before{content:none} p::after{content:normal}", "<p>x</p>")), ["x"]);
    assert_eq!(texts_of(&page("p::before{color:red}", "<p>x</p>")), ["x"]);
}

#[test]
fn display_none_pseudo_elements_are_skipped() {
    let html = page("p{margin:0;line-height:20px} p::before{content:'abc';display:none} p::after{content:'';display:none}", "<p>x</p>");
    assert_eq!(texts_of(&html), ["x"]);
    assert_eq!(height(&html), 20.0);
}

#[test]
fn an_empty_string_makes_a_pseudo_element_with_no_text() {
    // The box exists (it can have a height, as a clearfix does) but has no text frame.
    let html = page("div{margin:0} div::after{content:'';display:block;clear:both;height:7px}", "<div><span style='float:left;height:30px;width:5px'></span></div>");
    assert_eq!(texts_of(&html), Vec::<String>::new());
    assert_eq!(height(&html), 37.0);
}

#[test]
fn attr_is_the_value_of_the_attribute_of_the_originating_element() {
    // CSS 2.2 12.2: attr(X) returns the value of attribute X of the subject of the selector; the
    // empty string when there is none.
    let t = texts_of(&page("a::after{content:' (' attr(href) ')'}", "<a href='http://x.test/'>l</a>"));
    assert_eq!(t, ["l", " (", "http://x.test/", ")"]);
    let t = texts_of(&page("a::after{content:'[' attr(title) ']'}", "<a>l</a>"));
    assert_eq!(t, ["l", "[", "]"]);
}

#[test]
fn a_pseudo_element_of_an_inline_element_is_inside_it() {
    let t = texts_of(&page("b::before{content:'<'} b::after{content:'>'}", "<p>a<b>x</b>c</p>"));
    assert_eq!(t, ["a", "<", "x", ">", "c"]);
}

#[test]
fn a_block_pseudo_element_inside_an_inline_splits_it() {
    // CSS 2.2 9.2.1.1: a block box inside an inline splits the inline; the line boxes before and after
    // it are anonymous blocks, and the block adds its height.
    let html = page("p{margin:0;line-height:20px} b::after{content:'';display:block;height:30px}", "<p>a<b>x</b>c</p>");
    assert_eq!(height(&html), 20.0 + 30.0 + 20.0);
}

#[test]
fn pseudo_elements_of_a_list_item_follow_its_marker() {
    let t = texts_of(&page("li::before{content:'B'}", "<ul><li>x</li></ul>"));
    assert_eq!(t.len(), 3);
    assert!(t.contains(&"B".to_string()) && t.contains(&"x".to_string()));
}

// ---- quotes (CSS 2.2 12.3) -------------------------------------------------------------------------------

#[test]
fn q_gets_quotes_from_the_user_agent_sheet() {
    // html.css: q:before { content: open-quote } q:after { content: close-quote }
    let t = texts_of(&page("", "<p>a <q>x</q> b</p>"));
    assert!(t.contains(&"\u{201C}".to_string()) && t.contains(&"\u{201D}".to_string()), "{t:?}");
}

#[test]
fn nested_quotes_use_the_next_pair() {
    // CSS 2.2 12.3.1: the depth of nesting chooses the pair; the English default is “” then ‘’.
    let t = texts_of(&page("", "<q>a<q>b</q>c</q>"));
    let quotes: Vec<&str> = t.iter().map(|s| s.as_str()).filter(|s| matches!(*s, "\u{201C}" | "\u{201D}" | "\u{2018}" | "\u{2019}")).collect();
    assert_eq!(quotes, ["\u{201C}", "\u{2018}", "\u{2019}", "\u{201D}"]);
}

#[test]
fn the_quotes_property_gives_the_pairs() {
    let t = texts_of(&page("q{quotes:'<' '>' '[' ']'}", "<q>a<q>b</q>c</q>"));
    let q: Vec<&str> = t.iter().map(|s| s.as_str()).filter(|s| matches!(*s, "<" | ">" | "[" | "]")).collect();
    assert_eq!(q, ["<", "[", "]", ">"]);
    // the last pair serves every deeper level
    let t = texts_of(&page("q{quotes:'<' '>'}", "<q><q>b</q></q>"));
    let q: Vec<&str> = t.iter().map(|s| s.as_str()).filter(|s| matches!(*s, "<" | ">")).collect();
    assert_eq!(q, ["<", "<", ">", ">"]);
}

#[test]
fn no_open_quote_and_no_close_quote_only_change_the_depth() {
    let t = texts_of(&page("q::before{content:no-open-quote} q::after{content:no-close-quote}", "<q>a</q>"));
    assert_eq!(t, ["a"]);
    // an unbalanced close-quote at depth 0 is empty
    let t = texts_of(&page("b::after{content:close-quote}", "<b>a</b>"));
    assert_eq!(t, ["a"]);
}

#[test]
fn quotes_none_gives_no_text() {
    let t = texts_of(&page("q{quotes:none}", "<q>a</q>"));
    assert_eq!(t, ["a"]);
}

// ---- lists (CSS 2.2 12.5, CSS Lists 3) -------------------------------------------------------------------

fn ol(items: &str) -> String {
    page("", &format!("<ol>{items}</ol>"))
}

#[test]
fn an_ordered_list_numbers_its_items_from_one() {
    assert_eq!(markers(&ol("<li>a<li>b<li>c")), ["1. ", "2. ", "3. "]);
}

#[test]
fn an_unordered_list_has_bullets() {
    assert_eq!(markers(&page("", "<ul><li>a<li>b</ul>")), ["\u{2022} ", "\u{2022} "]);
}

#[test]
fn nested_unordered_lists_alternate_the_bullet_shape() {
    // html.css: ul ul is circle, ul ul ul is square.
    let m = markers(&page("", "<ul><li>a<ul><li>b<ul><li>c</ul></ul></ul>"));
    assert_eq!(m, ["\u{2022} ", "\u{25E6} ", "\u{25AA} "]);
}

#[test]
fn the_start_attribute_sets_the_first_number() {
    assert_eq!(markers(&page("", "<ol start=5><li>a<li>b</ol>")), ["5. ", "6. "]);
    assert_eq!(markers(&page("", "<ol start=-1><li>a<li>b<li>c</ol>")), ["-1. ", "0. ", "1. "]);
}

#[test]
fn the_reversed_attribute_counts_down_from_the_number_of_items() {
    assert_eq!(markers(&page("", "<ol reversed><li>a<li>b<li>c</ol>")), ["3. ", "2. ", "1. "]);
    assert_eq!(markers(&page("", "<ol reversed start=10><li>a<li>b</ol>")), ["10. ", "9. "]);
}

#[test]
fn the_value_attribute_of_an_item_sets_its_number_and_the_next_ones_continue() {
    assert_eq!(markers(&page("", "<ol><li>a<li value=10>b<li>c</ol>")), ["1. ", "10. ", "11. "]);
}

#[test]
fn a_nested_list_has_its_own_counter() {
    // CSS Lists 3 4.4: ol resets `list-item`, so the inner list starts again and the outer one carries
    // on after it.
    let m = markers(&page("", "<ol><li>a<ol><li>b<li>c</ol><li>d</ol>"));
    assert_eq!(m, ["1. ", "1. ", "2. ", "2. "]);
}

#[test]
fn sibling_lists_each_start_at_one() {
    let m = markers(&page("", "<ol><li>a<li>b</ol><ol><li>c</ol>"));
    assert_eq!(m, ["1. ", "2. ", "1. "]);
}

#[test]
fn list_style_type_chooses_the_number_system() {
    let m = markers(&page("ol{list-style-type:lower-alpha}", "<ol><li>a<li>b<li>c</ol>"));
    assert_eq!(m, ["a. ", "b. ", "c. "]);
    let m = markers(&page("ol{list-style-type:upper-alpha}", "<ol start=27><li>a<li>b</ol>"));
    assert_eq!(m, ["AA. ", "AB. "]);
    let m = markers(&page("ol{list-style-type:lower-greek}", "<ol><li>a<li>b</ol>"));
    assert_eq!(m, ["\u{3b1}. ", "\u{3b2}. "]);
    let m = markers(&page("ol{list-style-type:square}", "<ol><li>a</ol>"));
    assert_eq!(m, ["\u{25AA} "]);
}

#[test]
fn list_style_type_none_has_no_marker_text() {
    assert_eq!(texts_of(&page("ol{list-style-type:none}", "<ol><li>a<li>b</ol>")), ["a", "b"]);
}

#[test]
fn a_list_item_that_is_not_in_a_list_is_numbered_from_one() {
    // `li` is display:list-item and increments `list-item`, which nothing reset: a counter is made on
    // the element itself (CSS Lists 3, 4.5).
    let m = markers(&page("li{list-style-type:decimal}", "<li>a</li><li>b</li>"));
    // the second item follows the first, so it sees the counter the first one made
    assert_eq!(m, ["1. ", "2. "]);
}

#[test]
fn display_none_items_do_not_count() {
    let m = markers(&page("", "<ol><li>a<li style='display:none'>b<li>c</ol>"));
    assert_eq!(m, ["1. ", "2. "]);
}

#[test]
fn other_elements_made_list_items_are_numbered_too() {
    let m = markers(&page("div{display:list-item;list-style-type:decimal;margin-left:20px}", "<div>a</div><div>b</div>"));
    assert_eq!(m, ["1. ", "2. "]);
}

#[test]
fn the_marker_of_an_ordered_list_does_not_change_the_height() {
    // CSS Lists 3: an outside ::marker is out of the line of the item.
    let html = page("ol{margin:0;padding:0 0 0 40px} li{line-height:20px}", "<ol><li>a<li>b<li>c</ol>");
    assert_eq!(height(&html), 60.0);
}

#[test]
fn the_marker_of_a_multi_digit_number_is_one_text() {
    let m = markers(&page("", "<ol start=99><li>a<li>b</ol>"));
    assert_eq!(m, ["99. ", "100. "]);
}

#[test]
fn a_marker_pseudo_element_can_replace_the_text() {
    let t = texts_of(&page("li::marker{content:'>> '}", "<ul><li>a</ul>"));
    assert_eq!(t, [">> ", "a"]);
}

// ---- counters (CSS 2.2 12.4) -----------------------------------------------------------------------------

#[test]
fn counter_increment_and_counter_reset_number_headings() {
    // CSS 2.2 12.4.1: counter-reset makes the counter, counter-increment adds to it, counter() shows it.
    let css = "body{counter-reset:sec} h2{counter-increment:sec} h2::before{content:counter(sec) '. '}";
    let t = texts_of(&page(css, "<h2>a</h2><h2>b</h2><h2>c</h2>"));
    assert_eq!(t, ["1", ". ", "a", "2", ". ", "b", "3", ". ", "c"]);
}

#[test]
fn counter_values_can_be_changed_by_a_pseudo_element_itself() {
    let css = "body{counter-reset:n} p::before{counter-increment:n 2;content:counter(n)}";
    let t = texts_of(&page(css, "<p>a</p><p>b</p>"));
    assert_eq!(t, ["2", "a", "4", "b"]);
}

#[test]
fn counter_increment_defaults_to_one_and_reset_to_zero() {
    let css = "body{counter-reset:n} p{counter-increment:n} p::after{content:counter(n)}";
    assert_eq!(texts_of(&page(css, "<p>a</p><p>b</p>")), ["a", "1", "b", "2"]);
    // a counter that no reset made is made by the first increment, at 0 plus the increment
    let css = "p{counter-increment:m} p::after{content:counter(m)}";
    assert_eq!(texts_of(&page(css, "<p>a</p><p>b</p>")), ["a", "1", "b", "2"]);
}

#[test]
fn counter_with_a_style_formats_the_value() {
    let css = "body{counter-reset:n} p{counter-increment:n} p::before{content:counter(n, lower-alpha)}";
    assert_eq!(texts_of(&page(css, "<p>a</p><p>b</p><p>c</p>")), ["a", "a", "b", "b", "c", "c"]);
}

#[test]
fn counters_with_a_separator_show_every_level_of_nesting() {
    // CSS 2.2 12.4.1: nested counters of one name are a stack, counters() joins them.
    let css = "ol{counter-reset:item;list-style:none} li{counter-increment:item} li::before{content:counters(item, '.') ' '}";
    let t = texts_of(&page(css, "<ol><li>a<ol><li>b<li>c</ol><li>d</ol>"));
    let numbers: Vec<&str> = t.iter().map(|s| s.as_str()).filter(|s| s.chars().next().map_or(false, |c| c.is_ascii_digit())).collect();
    assert_eq!(numbers, ["1", "1.1", "1.2", "2"]);
}

#[test]
fn a_counter_made_by_a_sibling_is_replaced_not_nested() {
    // CSS Lists 3 4.4: a reset of a counter made by a sibling takes its place.
    let css = "h1{counter-reset:s} h1::after{content:counter(s)} p{counter-increment:s} p::after{content:counter(s)}";
    let t = texts_of(&page(css, "<h1>a</h1><p>b</p><p>c</p><h1>d</h1><p>e</p>"));
    assert_eq!(t, ["a", "0", "b", "1", "c", "2", "d", "0", "e", "1"]);
}

#[test]
fn counter_set_is_not_ported_but_display_none_does_not_count() {
    let css = "body{counter-reset:n} p{counter-increment:n} p::after{content:counter(n)}";
    assert_eq!(texts_of(&page(css, "<p>a</p><p style='display:none'>b</p><p>c</p>")), ["a", "1", "c", "2"]);
}
