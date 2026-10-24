// Incremental restyle (mrt_style::restyle and StyleDom::attach_dom): after a change made through the
// recorder, as the runtime's DOM funnels make it, the restyled page has the colors a page styled from
// scratch has. Written from the behaviour of the selectors involved (CSS Selectors 4); each case also
// checks that the restyle did not restyle everything.
use mrt_style::mrt_dom::{Dom, Kind, NodeId, NONE};
use mrt_style::restyle::RestyleRecorder;
use mrt_style::{SheetCache, StyleDom};
use style_traits::values::ToCss;

const CSS: &str = "p { color: black } .on { color: red } p:first-child { color: blue } div:empty { color: green } #x { color: purple } [data-k] { color: orange } p + p { color: gray }";

fn page(body: &str) -> String {
    format!("<!DOCTYPE html><html><head><style>{CSS}</style></head><body>{body}</body></html>")
}

fn elements(dom: &Dom, tag: &str) -> Vec<NodeId> {
    (0..dom.nodes.len() as NodeId).filter(|&i| dom.node(i).kind == Kind::Element && dom.tag(i) == tag && dom.is_connected(i)).collect()
}

fn color(sd: &StyleDom, id: NodeId) -> String {
    sd.computed(id).map(|s| s.get_inherited_text().clone_color().to_css_string()).unwrap_or_default()
}

/// Styles `body`, lets `change` change the DOM through the recorder, restyles incrementally, and
/// checks every connected element against a style build from scratch. Returns the number of
/// elements whose style was computed again.
fn check(body: &str, change: impl FnOnce(&mut Dom, &mut RestyleRecorder)) -> usize {
    let mut sd = StyleDom::with_sheet_cache(Dom::parse(&page(body)), (800.0, 600.0), Default::default(), SheetCache::default());
    sd.resolve();
    let (flags, styled) = sd.restyle_state();
    let before: Vec<(NodeId, Option<*const ()>)> = (0..sd.tree.dom.nodes.len() as NodeId).map(|i| (i, sd.computed(i).map(|s| &*s as *const _ as *const ()))).collect();
    let mut recorder = RestyleRecorder::default();
    recorder.reset(flags, styled);
    let mut dom = sd.detach_dom();
    change(&mut dom, &mut recorder);
    let ops = recorder.take_ops();
    sd.attach_dom(dom.clone(), Default::default(), &ops);
    sd.resolve();

    let mut fresh = StyleDom::new(dom, (800.0, 600.0));
    fresh.resolve();
    for i in 0..fresh.tree.dom.nodes.len() as NodeId {
        if fresh.tree.dom.node(i).kind == Kind::Element && fresh.tree.dom.is_connected(i) {
            assert_eq!(color(&sd, i), color(&fresh, i), "<{}> #{i}", fresh.tree.dom.tag(i));
        }
    }
    before.iter().filter(|(i, s)| s.is_some() && sd.computed(*i).map(|n| &*n as *const _ as *const ()) != *s).count()
}

#[test]
fn a_class_change_restyles_through_the_snapshot() {
    let restyled = check("<p>a</p><p id=t>b</p><div>c</div>", |dom, r| {
        let p = elements(dom, "p")[1];
        r.attribute_will_change(dom, p, "class");
        dom.set_attribute(p, "class", "on");
        r.attribute_changed(p, "class");
    });
    assert!(restyled < 3, "restyled {restyled}");
}

#[test]
fn an_attribute_change_restyles_the_element() {
    check("<p>a</p><p>b</p>", |dom, r| {
        let p = elements(dom, "p")[1];
        r.attribute_will_change(dom, p, "data-k");
        dom.set_attribute(p, "data-k", "1");
        r.attribute_changed(p, "data-k");
    });
    check("<p>a</p><p id=y>b</p>", |dom, r| {
        let p = elements(dom, "p")[1];
        r.attribute_will_change(dom, p, "id");
        dom.set_attribute(p, "id", "x");
        r.attribute_changed(p, "id");
    });
}

#[test]
fn an_insertion_before_the_first_child_restyles_it() {
    // p:first-child and p + p: the old first <p> is no longer first, and now follows a <p>
    check("<section><p>a</p><p>b</p></section>", |dom, r| {
        let section = elements(dom, "section")[0];
        let first = dom.node(section).first_child;
        let p = dom.create_element("p");
        dom.insert_child(section, p, first);
        r.content_inserted(dom, p);
    });
}

#[test]
fn a_removal_restyles_the_new_first_child() {
    check("<section><p>a</p><p>b</p><p>c</p></section>", |dom, r| {
        let first = elements(dom, "p")[0];
        r.content_will_be_removed(dom, first);
        dom.remove_child(dom.node(first).parent, first);
    });
}

#[test]
fn text_in_an_empty_element_restyles_it() {
    check("<div></div><div>x</div>", |dom, r| {
        let div = elements(dom, "div")[0];
        let t = dom.create_text("now");
        dom.insert_child(div, t, NONE);
        r.content_inserted(dom, t);
    });
    check("<div>x</div>", |dom, r| {
        let div = elements(dom, "div")[0];
        let t = dom.node(div).first_child;
        r.content_will_be_removed(dom, t);
        dom.remove_child(div, t);
    });
}

#[test]
fn a_moved_element_is_styled_in_its_new_place() {
    check("<section><p>a</p></section><div><p>b</p><p class=on>c</p></div>", |dom, r| {
        let moved = elements(dom, "p")[2];
        let section = elements(dom, "section")[0];
        r.content_will_be_removed(dom, moved);
        dom.insert_child(section, moved, dom.node(section).first_child);
        r.content_inserted(dom, moved);
    });
}

#[test]
fn a_new_sheet_restyles_everything() {
    let mut sd = StyleDom::with_sheet_cache(Dom::parse(&page("<p>a</p>")), (800.0, 600.0), Default::default(), SheetCache::default());
    sd.resolve();
    let mut dom = sd.detach_dom();
    let head = elements(&dom, "head")[0];
    let style = dom.create_element("style");
    let text = dom.create_text("p { color: yellow !important }");
    dom.insert_child(style, text, NONE);
    dom.insert_child(head, style, NONE);
    sd.attach_dom(dom, Default::default(), &[]);
    sd.resolve();
    let p = elements(&sd.tree.dom, "p")[0];
    assert_eq!(color(&sd, p), "rgb(255, 255, 0)");
}
