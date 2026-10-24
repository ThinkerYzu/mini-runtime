// Eager ::before / ::after styles and the lazy ::marker style, through StyleDom::pseudo_style.
use mrt_style::mrt_dom::{Dom, Kind, NodeId};
use mrt_style::style::selector_parser::PseudoElement;
use mrt_style::style::values::generics::counters::Content;
use mrt_style::StyleDom;
use style_traits::values::ToCss;

fn find(sd: &StyleDom, tag: &str) -> NodeId {
    (0..sd.tree.dom.nodes.len() as NodeId).find(|&i| sd.tree.dom.node(i).kind == Kind::Element && sd.tree.dom.tag(i) == tag).unwrap()
}

fn styled(body: &str, css: &str) -> StyleDom {
    let html = format!("<!DOCTYPE html><html><head><style>{css}</style></head><body>{body}</body></html>");
    let mut sd = StyleDom::new(Dom::parse(&html), (800.0, 600.0));
    sd.resolve();
    sd
}

#[test]
fn before_after_are_stored_and_found() {
    let sd = styled("<p>x</p><div>y</div>", "p::before{content:'a'} p::after{content:'b';display:block}");
    let p = find(&sd, "p");
    let before = sd.pseudo_style(p, PseudoElement::Before).expect("::before");
    assert!(matches!(before.get_counters().content, Content::Items(_)));
    let after = sd.pseudo_style(p, PseudoElement::After).expect("::after");
    assert_eq!(after.get_box().clone_display().to_css_string(), "block");
    assert!(sd.pseudo_style(find(&sd, "div"), PseudoElement::Before).is_none());
}

#[test]
fn content_none_and_display_none_do_not_exist() {
    let sd = styled("<p>x</p><div>y</div><span>z</span>", "p::before{content:none} div::before{content:'a';display:none} span::before{color:red}");
    assert!(sd.pseudo_style(find(&sd, "p"), PseudoElement::Before).is_none());
    assert!(sd.pseudo_style(find(&sd, "div"), PseudoElement::Before).is_none());
    assert!(sd.pseudo_style(find(&sd, "span"), PseudoElement::Before).is_none());
}

#[test]
fn marker_style_is_lazy_and_cached() {
    let sd = styled("<ol><li>q</li></ol>", "li::marker{color:red}");
    let li = find(&sd, "li");
    let a = sd.pseudo_style(li, PseudoElement::Marker).expect("::marker");
    let b = sd.pseudo_style(li, PseudoElement::Marker).unwrap();
    assert!(std::ptr::eq(&*a, &*b));
    assert_eq!(a.get_inherited_text().color.to_css_string(), "rgb(255, 0, 0)");
    // ua.css: ::marker { white-space: pre }
    assert_eq!(format!("{:?}", a.get_inherited_text().white_space_collapse), "Preserve");
}

#[test]
fn stats_count_elements_with_pseudos() {
    let sd = styled("<p>x</p><p>y</p><div>z</div>", "p::before{content:'a'} p::after{content:'b'}");
    assert_eq!(sd.eager_pseudo_stats(), (2, 2, 2));
}
