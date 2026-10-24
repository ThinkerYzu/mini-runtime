// Tests of the form control frames (layout/forms): the sizes of the checkbox and radio, the text controls,
// the drop-down and list box selects, the buttons and the button-type inputs.
//
// Gecko has no gtest for these (its tests are reftests and mochitests that need a whole browser). They are
// written from the rules the Firefox code states (nsCheckboxRadioFrame::DefaultSize, nsTextControlFrame::
// CalcFixedSize, nsComboboxControlFrame::IntrinsicISize, nsListControlFrame::CalcIntrinsicBSize) and the
// user-agent sheet (forms.css: input padding 1px 2px and border 2px; button padding 1px 4px; select padding
// 1px 4px, border 2px, a 18px arrow). The text widths come from the same font metrics the frames use, so the
// numbers do not depend on the font installed; the last test pins numbers measured in Firefox (harness of
// bench/firefox_boxes.py, see bench/firefox_boxes) and runs only where fontconfig's sans-serif is Noto Sans,
// the font they were measured with.
//
// Not a translation of any Firefox file, so there is no `// Gecko:` marker.
#![cfg(test)]

use crate::driver::*;
use crate::nsLineLayoutAdapter::*;
use crate::*;
use mrt_style::mrt_dom::Dom;
use mrt_style::StyleDom;

const PX: i32 = 60;

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

// The first frame in the tree (document order of the principal lists) whose content is the HTML element `aName`.
fn find(aFrame: nsIFrame, aName: nsAtom) -> Option<nsIFrame> {
    if aFrame.GetContent().map_or(false, |c| c.IsHTMLElement(aName)) && aFrame.Type() != LayoutFrameType::Placeholder {
        return Some(aFrame);
    }
    let mut lists = Vec::new();
    aFrame.GetChildLists(&mut lists);
    for l in lists {
        let mut c = l.mList.FirstChild();
        while let Some(k) = c {
            if let Some(found) = find(k, aName) {
                return Some(found);
            }
            c = k.GetNextSibling();
        }
    }
    None
}

// The size of the first element `aName` of the page, in app units, and the frame.
fn size_of(html: &str, aName: nsAtom) -> (nsSize, nsIFrame) {
    // the layout (and its frames) lives as long as the thread, see "One page per thread" in CONVENTIONS.md
    let result = layout(html);
    let frame = find(result.mRootElementFrame.unwrap(), aName).unwrap_or_else(|| panic!("no frame for {aName}"));
    (frame.GetSize(), frame)
}

// The width of `aText` in the font of aFrame, in app units
fn text_width(aFrame: nsIFrame, aText: &str) -> nscoord {
    let fm = nsLayoutUtils::GetFontMetricsForFrame(aFrame, 1.0);
    AppUnitWidthOfString(aText, &fm)
}

// The font's character width as nsTextControlFrame::CalcFixedSize takes it
fn char_width(aFrame: nsIFrame) -> nscoord {
    let fm = nsLayoutUtils::GetFontMetricsForFrame(aFrame, 1.0);
    std::cmp::max(fm.ZeroOrAveCharWidth(), fm.AveCharWidth())
}

#[test]
fn a_checkbox_and_a_radio_are_14_by_14() {
    // nsCheckboxRadioFrame::DefaultSize: the theme's 14px; no font involved
    for ty in ["checkbox", "radio"] {
        let (size, frame) = size_of(&page("", &format!("<input type={ty} style='margin:0'>")), "input");
        assert_eq!(frame.Type(), LayoutFrameType::CheckboxRadio);
        assert_eq!(size, nsSize::new(14 * PX, 14 * PX), "{ty}");
    }
}

#[test]
fn a_checkbox_with_a_css_size_takes_it() {
    let (size, _) = size_of(&page("", "<input type=checkbox style='margin:0;width:30px;height:20px'>"), "input");
    assert_eq!(size, nsSize::new(30 * PX, 20 * PX));
}

#[test]
fn a_text_input_is_size_characters_wide_and_a_line_high() {
    // nsTextControlFrame::CalcFixedSize: cols * charWidth (cols = the `size` attribute, 20 by default) and
    // rows (1) * line height; a monospace font has no slop. The UA sheet adds 2px padding on the sides, 1px
    // above and below, and a 2px border: 8px wide and 6px high.
    let css = "input{font:16px monospace;line-height:20px;margin:0}";
    let (size, frame) = size_of(&page(css, "<input type=text size=7>"), "input");
    assert_eq!(frame.Type(), LayoutFrameType::TextInput);
    assert_eq!(size, nsSize::new(7 * char_width(frame) + 8 * PX, 26 * PX));
    let (size, frame) = size_of(&page(css, "<input type=text>"), "input");
    assert_eq!(size.width, 20 * char_width(frame) + 8 * PX);
    // a missing, zero or malformed size is the default
    for bad in ["size=0", "size=x", "size=-3"] {
        let (size, frame) = size_of(&page(css, &format!("<input type=text {bad}>")), "input");
        assert_eq!(size.width, 20 * char_width(frame) + 8 * PX, "{bad}");
    }
}

#[test]
fn every_text_type_of_input_is_a_text_control() {
    for ty in ["text", "email", "tel", "url", "password", "search", "number", "bogus", ""] {
        let (_, frame) = size_of(&page("", &format!("<input type='{ty}'>")), "input");
        assert_eq!(frame.Type(), LayoutFrameType::TextInput, "type {ty:?}");
    }
}

#[test]
fn a_text_input_with_a_css_width_keeps_the_height_of_a_line() {
    let css = "input{font:16px monospace;line-height:20px;margin:0}";
    let (size, _) = size_of(&page(css, "<input type=text style='width:100px'>"), "input");
    // content-box: 100px of content, then the padding and border
    assert_eq!(size, nsSize::new(108 * PX, 26 * PX));
    let (size, _) = size_of(&page(css, "<input type=text style='width:100px;box-sizing:border-box'>"), "input");
    assert_eq!(size.width, 100 * PX);
}

#[test]
fn a_textarea_is_cols_wide_and_rows_high() {
    // 2px padding and 2px border on every side: 8px each way
    let css = "textarea{font:16px monospace;line-height:20px;margin:0}";
    let (size, frame) = size_of(&page(css, "<textarea rows=3 cols=7></textarea>"), "textarea");
    assert_eq!(frame.Type(), LayoutFrameType::TextInput);
    assert_eq!(size, nsSize::new(7 * char_width(frame) + 8 * PX, 68 * PX));
    // the defaults are 20 columns and 2 rows
    let (size, frame) = size_of(&page(css, "<textarea></textarea>"), "textarea");
    assert_eq!(size, nsSize::new(20 * char_width(frame) + 8 * PX, 48 * PX));
    // the text in it is its default value, not a child frame
    let (size2, _) = size_of(&page(css, "<textarea>some\ntext\nhere\nand more</textarea>"), "textarea");
    assert_eq!(size2, size);
}

#[test]
fn a_button_has_the_padding_and_border_of_the_ua_sheet() {
    // forms.css: padding 1px 4px, border 2px: 12px wide and 6px high around the text
    let css = "button{font:16px monospace;line-height:20px;margin:0}";
    let (size, frame) = size_of(&page(css, "<button>OK</button>"), "button");
    let text = text_width(frame, "OK");
    assert_eq!(size, nsSize::new(text + 12 * PX, 26 * PX));
}

#[test]
fn a_button_type_input_is_as_wide_as_a_button_with_the_same_label() {
    let css = "button,input{font:16px monospace;line-height:20px;margin:0}";
    let (button, _) = size_of(&page(css, "<button>OK</button>"), "button");
    for ty in ["button", "submit", "reset"] {
        let (input, _) = size_of(&page(css, &format!("<input type={ty} value='OK'>")), "input");
        assert_eq!(input, button, "{ty}");
    }
    // without a value a submit button says "Submit Query" and a reset button "Reset"; a button says nothing
    // but still has a line (nsIFrame::ShouldHaveLineIfEmpty), so it is as high as one with a label
    let (submit, frame) = size_of(&page(css, "<input type=submit>"), "input");
    assert_eq!(submit.width, text_width(frame, "Submit Query") + 12 * PX);
    let (reset, frame) = size_of(&page(css, "<input type=reset>"), "input");
    assert_eq!(reset.width, text_width(frame, "Reset") + 12 * PX);
    let (empty, _) = size_of(&page(css, "<input type=button>"), "input");
    assert_eq!(empty, nsSize::new(12 * PX, 26 * PX));
}

#[test]
fn a_drop_down_select_is_the_widest_option_and_an_arrow_wide() {
    // nsComboboxControlFrame::IntrinsicISize: the longest option's text (and one app unit) plus the 18px of
    // the arrow button; the UA sheet adds 4px padding on each side and a 2px border: 30px in all, with the
    // text. The height is one line, plus 1px padding and 2px border above and below. The line is
    // `normal` whatever the page says: StyleAdjuster::adjust_for_appearance resets the line-height of a
    // <select> drawn as a menulist (Firefox does the same), so `line-height:20px` changes nothing.
    let css = "select{font:16px monospace;line-height:20px;margin:0}";
    let html = page(css, "<select><option>a<option>  twelve   chars <option>b</select>");
    let (size, frame) = size_of(&html, "select");
    assert_eq!(frame.Type(), LayoutFrameType::ComboboxControl);
    let (normal, _) = size_of(&page("select{font:16px monospace;margin:0}", "<select><option>a</select>"), "select");
    assert_ne!(normal.height, 26 * PX, "a 20px line would make it 26px");
    // (the whitespace of an option is stripped and collapsed)
    assert_eq!(size, nsSize::new(text_width(frame, "twelve chars") + 1 + 30 * PX, normal.height));
    // an empty select is just the arrow and the edges
    let (size, _) = size_of(&page(css, "<select></select>"), "select");
    assert_eq!(size, nsSize::new(30 * PX, normal.height));
    // the `label` attribute is what is shown
    let (size, frame) = size_of(&page(css, "<select><option label='long label here'>x</select>"), "select");
    assert_eq!(size.width, text_width(frame, "long label here") + 1 + 30 * PX);
}

#[test]
fn a_select_with_multiple_or_a_size_above_one_is_a_list_box() {
    let css = "select{font:16px monospace;margin:0}";
    for attrs in ["multiple", "size=2", "size=8", "multiple size=1"] {
        let (_, frame) = size_of(&page(css, &format!("<select {attrs}><option>a</select>")), "select");
        assert_eq!(frame.Type(), LayoutFrameType::ListControl, "{attrs}");
    }
    for attrs in ["", "size=1", "size=0"] {
        let (_, frame) = size_of(&page(css, &format!("<select {attrs}><option>a</select>")), "select");
        assert_eq!(frame.Type(), LayoutFrameType::ComboboxControl, "{attrs}");
    }
}

#[test]
fn a_list_box_shows_size_rows_four_by_default() {
    // nsListControlFrame::CalcIntrinsicBSize: the `size` rows (4 when there is none or it is 0) of the tallest option,
    // which is a line of `normal` height plus the option's 2px padding above and below, in a box with 1px padding
    // and a 2px border above and below. The select has no padding on the sides, so it is the widest option
    // (its text and 4px padding on each side) and the border.
    let css = "select{font:16px monospace;margin:0}";
    let html = |attrs: &str| page(css, &format!("<select {attrs}><option>a<option>wider<option>c<option>d<option>e<option>f</select>"));
    let (one, frame) = size_of(&html("size=3"), "select");
    let (two, _) = size_of(&html("size=6"), "select");
    let row = (two.height - 6 * PX) / 6;
    assert_eq!(one.height, 3 * row + 6 * PX);
    assert!(row > 4 * PX);
    assert_eq!(one.width, text_width(frame, "wider") + 8 * PX + 4 * PX);
    let (multiple, _) = size_of(&html("multiple"), "select");
    assert_eq!(multiple.height, 4 * row + 6 * PX);
    // more rows than options keeps the rows
    let (many, _) = size_of(&page(css, "<select size=9><option>a</select>"), "select");
    assert_eq!(many.height, 9 * row + 6 * PX);
}

#[test]
fn an_optgroup_label_and_its_options_are_rows_of_a_list_box() {
    let css = "select{font:16px monospace;margin:0}";
    let flat = page(css, "<select size=3><option>a<option>b<option>c</select>");
    let grouped = page(css, "<select size=3><optgroup label=g><option>a<option>b</optgroup></select>");
    assert_eq!(size_of(&flat, "select").0.height, size_of(&grouped, "select").0.height);
}

#[test]
fn display_none_options_are_not_measured() {
    let css = "select{font:16px monospace;margin:0} option.h{display:none}";
    let (shown, _) = size_of(&page(css, "<select><option>a<option>wide wide wide</select>"), "select");
    let (hidden, _) = size_of(&page(css, "<select><option>a<option class=h>wide wide wide</select>"), "select");
    assert!(hidden.width < shown.width);
}

#[test]
fn form_controls_are_atomic_inline_boxes_on_a_line() {
    // inline-block: they sit on the line with the text, next to each other
    let css = "body{font:16px monospace;line-height:20px} input,select,textarea,button{margin:0}";
    let html = page(css, "<input type=checkbox><input type=text size=3><select><option>a</select><button>b</button>");
    let result = layout(&html);
    let root = result.mRootElementFrame.unwrap();
    let mut xs = Vec::new();
    for tag in ["input", "select", "button"] {
        xs.push(find(root, tag).unwrap().GetRect().x);
    }
    // the checkbox is first, the select (after the text input) and the button after it
    assert_eq!(find(root, "input").unwrap().GetRect().x, 0);
    assert!(xs[1] > xs[0] && xs[2] > xs[1], "{xs:?}");
}

#[test]
fn a_block_level_text_input_is_laid_out_as_a_block() {
    let css = "input{font:16px monospace;line-height:20px;margin:0;display:block}";
    let result = layout(&page(css, "<input type=text size=4><input type=text size=4>"));
    let root = result.mRootElementFrame.unwrap();
    let first = find(root, "input").unwrap();
    // the second one is below it, not beside it
    let second = first.GetNextSibling().unwrap();
    assert_eq!(second.GetRect().x, 0);
    assert_eq!(second.GetRect().y, first.GetRect().y + first.GetSize().height);
}

// Numbers measured in Firefox (bench/firefox_boxes.py harness, 13.3333px Noto Sans for the controls), pinned
// where the same font is installed.
#[test]
fn numbers_measured_in_firefox_for_the_default_controls() {
    let sans = std::process::Command::new("fc-match").arg("sans-serif").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    if !sans.contains("Noto Sans") {
        eprintln!("skipped: fc-match sans-serif is {sans:?}, the numbers were measured with Noto Sans");
        return;
    }
    let css = "input,select,textarea,button{margin:0}";
    let px = |v: f64| (v * PX as f64).round() as i32;
    // <input type=text>: 180 by 25; size=17: 156 by 25
    assert_eq!(size_of(&page(css, "<input type=text>"), "input").0, nsSize::new(px(180.0), px(25.0)));
    assert_eq!(size_of(&page(css, "<input type=text size=17>"), "input").0, nsSize::new(px(156.0), px(25.0)));
    // <button>hide</button>: 39.38 by 25
    let (button, _) = size_of(&page(css, "<button>hide</button>"), "button");
    assert!((button.width - px(39.38)).abs() <= 2 && button.height == px(25.0), "{button:?}");
    // <input type=submit>: "Submit Query" 98.4 by 25; <input type=button value=Go>: 29.77 by 25
    let (submit, _) = size_of(&page(css, "<input type=submit>"), "input");
    assert!((submit.width - px(98.4)).abs() <= 2 && submit.height == px(25.0), "{submit:?}");
    let (go, _) = size_of(&page(css, "<input type=button value=Go>"), "input");
    assert!((go.width - px(29.77)).abs() <= 2 && go.height == px(25.0), "{go:?}");
    // <textarea>: 20 columns and 2 rows of the 12px monospace: 152.67 by 38
    let (ta, _) = size_of(&page(css, "<textarea></textarea>"), "textarea");
    assert!((ta.width - px(152.67)).abs() <= 2 && ta.height == px(38.0), "{ta:?}");
    // <select> with the options "a" and "b": 38.22 by 25; a list box with size=2: 20.2 by 52; empty: 30 by 25
    let (s, _) = size_of(&page(css, "<select><option>a<option>b</select>"), "select");
    assert!((s.width - px(38.22)).abs() <= 2 && s.height == px(25.0), "{s:?}");
    let (l, _) = size_of(&page(css, "<select size=2><option>a<option>b<option>c</select>"), "select");
    assert!((l.width - px(20.2)).abs() <= 2 && l.height == px(52.0), "{l:?}");
    let (e, _) = size_of(&page(css, "<select></select>"), "select");
    assert_eq!(e, nsSize::new(px(30.0), px(25.0)));
}

// The media elements (layout/generic/nsVideoFrame.cpp). The numbers are the fallback intrinsic size of 300 by
// 150 (kFallbackIntrinsicSize) and Firefox's measurements of the audio control bar (40px).
#[test]
fn a_video_without_a_size_is_300_by_150() {
    for attrs in ["", "controls", "poster=x.png", "src=x.mp4"] {
        let (size, frame) = size_of(&page("video{margin:0}", &format!("<video {attrs}></video>")), "video");
        assert_eq!(frame.Type(), LayoutFrameType::HTMLVideo);
        assert_eq!(size, nsSize::new(300 * PX, 150 * PX), "{attrs}");
    }
}

#[test]
fn a_video_with_one_dimension_keeps_the_2_to_1_ratio() {
    // CSS 2.2 10.3.2: the ratio fills in the other dimension (the width and height attributes are style)
    let (size, _) = size_of(&page("", "<video width=100></video>"), "video");
    assert_eq!(size, nsSize::new(100 * PX, 50 * PX));
    let (size, _) = size_of(&page("", "<video height=60></video>"), "video");
    assert_eq!(size, nsSize::new(120 * PX, 60 * PX));
    let (size, _) = size_of(&page("", "<video width=100 height=70></video>"), "video");
    assert_eq!(size, nsSize::new(100 * PX, 70 * PX));
    let (size, _) = size_of(&page("video{width:200px}", "<video></video>"), "video");
    assert_eq!(size, nsSize::new(200 * PX, 100 * PX));
}

#[test]
fn a_video_is_inline_level_and_sits_on_the_line() {
    let css = "body{font:16px monospace;line-height:20px} video{margin:0}";
    let result = layout(&page(css, "a<video></video>b"));
    let video = find(result.mRootElementFrame.unwrap(), "video").unwrap();
    assert!(video.GetRect().x > 0);
    // 150px tall, on the baseline: the line is at least that high
    assert!(result.RootElementHeightInCSSPixels().unwrap() >= 150.0);
}

#[test]
fn an_audio_with_controls_is_300_wide_and_the_height_of_the_control_bar() {
    let (size, frame) = size_of(&page("audio{margin:0}", "<audio controls></audio>"), "audio");
    assert_eq!(frame.Type(), LayoutFrameType::HTMLVideo);
    assert_eq!(size, nsSize::new(300 * PX, 40 * PX));
    // a CSS width is taken; the control bar's height stays
    let (size, _) = size_of(&page("audio{margin:0;width:250px}", "<audio controls></audio>"), "audio");
    assert_eq!(size, nsSize::new(250 * PX, 40 * PX));
    // html.css: `audio:not([controls]) { display: none !important }`
    let result = layout(&page("", "<audio></audio>"));
    assert!(find(result.mRootElementFrame.unwrap(), "audio").is_none());
}

