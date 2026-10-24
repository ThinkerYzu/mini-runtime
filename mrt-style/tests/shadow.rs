// Style in shadow trees: each shadow root's `<style>`, `:host`, `::slotted`, inheritance through the
// flattened tree. The expected values are release Firefox 157's getComputedStyle for the same page
// (bench/firefox_case.py).
use mrt_style::mrt_dom::{Dom, Kind, NodeId};
use mrt_style::StyleDom;
use style_traits::values::ToCss;

const BODY: &str = r#"<style>p{color:rgb(1,1,1)} span{color:rgb(2,2,2)} #h{color:rgb(3,3,3)} .x{font-size:20px} slot{color:rgb(4,4,4)}</style>
<div id=h><template shadowrootmode=open><style>:host{font-size:30px} p{color:rgb(10,10,10)} ::slotted(span){color:rgb(11,11,11)} ::slotted(*){font-weight:700} .x{font-style:italic} :host(#h){letter-spacing:2px} slot[name=n]{color:rgb(12,12,12)} :host(.nope){font-size:5px}</style><p id=sp class=x>in</p><slot id=sd></slot><i id=si>i</i><slot id=sn name=n><b id=fb>fallback</b></slot></template><span id=a>a</span><p id=lp class=x>light p</p><em id=un slot=none>unassigned</em></div>"#;

fn by_id(dom: &Dom, id: &str) -> NodeId {
    (0..dom.nodes.len() as NodeId).find(|&i| dom.node(i).kind == Kind::Element && dom.get_attribute(i, "id") == Some(id)).unwrap()
}

#[test]
fn shadow_tree_styles_match_firefox() {
    let html = format!("<!DOCTYPE html><html><head></head><body>{BODY}</body></html>");
    let mut sd = StyleDom::new(Dom::parse(&html), (800.0, 600.0));
    sd.resolve();
    let want = [
        ("h", "rgb(3, 3, 3) 30px 400 normal 2px block"),
        ("sp", "rgb(10, 10, 10) 30px 400 italic 2px block"),
        ("sd", "rgb(3, 3, 3) 30px 400 normal 2px contents"),
        ("a", "rgb(2, 2, 2) 30px 700 normal 2px inline"),
        ("lp", "rgb(1, 1, 1) 20px 700 normal 2px block"),
        ("si", "rgb(3, 3, 3) 30px 400 italic 2px inline"),
        ("sn", "rgb(12, 12, 12) 30px 400 normal 2px contents"),
        ("fb", "rgb(12, 12, 12) 30px 700 normal 2px inline"),
    ];
    let mut failures = Vec::new();
    for (id, expected) in want {
        let cs = sd.computed(by_id(&sd.tree.dom, id)).unwrap_or_else(|| panic!("{id} has no style"));
        let got = format!(
            "{} {}px {} {} {} {}",
            cs.get_inherited_text().clone_color().to_css_string(),
            cs.get_font().clone_font_size().computed_size().px(),
            cs.get_font().clone_font_weight().value(),
            cs.get_font().clone_font_style().to_css_string(),
            cs.get_inherited_text().clone_letter_spacing().to_css_string(),
            cs.get_box().clone_display().to_css_string(),
        );
        if got != expected {
            failures.push(format!("{id}: want {expected}, got {got}"));
        }
    }
    // a host's child that no slot takes is not in the flattened tree and gets no style, as in Gecko
    assert!(sd.computed(by_id(&sd.tree.dom, "un")).is_none());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// Adopted style sheets: after a tree's <style> sheets in its cascade, one sheet shared by two
// shadow roots and the document; release Firefox 157's computed values for the same page with the
// sheets adopted by script (bench/firefox_case.py).
const ADOPTED_BODY: &str = r#"<style>p{color:rgb(1,1,1)}</style>
<div id=h1><template shadowrootmode=open><style>p{color:rgb(10,10,10)} b{color:rgb(13,13,13)}</style><p id=p1>a</p><b id=b1>b</b></template></div>
<div id=h2><template shadowrootmode=open><p id=p2>c</p><i id=i2>d</i></template></div>
<p id=lp>light</p><i id=li>light i</i>"#;

#[test]
fn adopted_sheets_match_firefox() {
    let html = format!("<!DOCTYPE html><html><head></head><body>{ADOPTED_BODY}</body></html>");
    let mut dom = Dom::parse(&html);
    let shared = dom.new_constructed_sheet("p { color: rgb(20,20,20) } :host { font-size: 30px }");
    let other = dom.new_constructed_sheet("i { color: rgb(30,30,30) } p { color: rgb(31,31,31) }");
    let r1 = dom.shadow_roots[&by_id(&dom, "h1")];
    let r2 = dom.shadow_roots[&by_id(&dom, "h2")];
    dom.set_adopted_sheets(r1, &[shared]);
    dom.set_adopted_sheets(r2, &[other, shared]);
    dom.set_adopted_sheets(mrt_style::mrt_dom::DOCUMENT, &[other]);
    let mut sd = StyleDom::new(dom, (800.0, 600.0));
    sd.resolve();
    let want = [
        ("p1", "rgb(20, 20, 20) 30px"),
        ("b1", "rgb(13, 13, 13) 30px"),
        ("p2", "rgb(20, 20, 20) 30px"),
        ("i2", "rgb(30, 30, 30) 30px"),
        ("lp", "rgb(31, 31, 31) 16px"),
        ("li", "rgb(30, 30, 30) 16px"),
        ("h1", "rgb(0, 0, 0) 30px"),
    ];
    let mut failures = Vec::new();
    for (id, expected) in want {
        let cs = sd.computed(by_id(&sd.tree.dom, id)).unwrap_or_else(|| panic!("{id} has no style"));
        let got = format!("{} {}px", cs.get_inherited_text().clone_color().to_css_string(), cs.get_font().clone_font_size().computed_size().px());
        if got != expected {
            failures.push(format!("{id}: want {expected}, got {got}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
