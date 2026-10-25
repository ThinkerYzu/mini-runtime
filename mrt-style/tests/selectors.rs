//! Selectors over a growing arena after page replacement, and across shadow/frame documents.
use mrt_style::{match_selector, query_selector, mrt_dom::{Dom, Kind, NodeId, DOCUMENT, NONE}};

fn by_id(dom: &Dom, value: &str) -> NodeId {
    (0..dom.nodes.len() as NodeId).find(|&n| dom.node(n).kind == Kind::Element && dom.get_attribute(n, "id") == Some(value)).unwrap()
}

fn matches(dom: &mut Dom, node: NodeId, selector: &str, closest: bool) -> Option<NodeId> {
    let (next, result) = match_selector(std::mem::take(dom), node, selector, closest);
    *dom = next;
    result
}

fn query(dom: &mut Dom, root: NodeId, selector: &str, max: usize) -> Option<Vec<NodeId>> {
    let (next, result) = query_selector(std::mem::take(dom), root, selector, max);
    *dom = next;
    result
}

#[test]
fn selectors_after_page_replacement_and_mutation() {
    let mut dom = Dom::parse("<!doctype html><section id=group><b id=old class=pick></b><b></b></section>");
    let parent = by_id(&dom, "group");
    let child = by_id(&dom, "old");
    for i in 0..12_000 {
        let detached = dom.create_element("div");
        dom.set_attribute(detached, "style", &format!("width:{i}px;color:red;--unused:{i}"));
    }
    assert_eq!(matches(&mut dom, child, "#group > b.pick:first-child", false), Some(child));
    assert_eq!(matches(&mut dom, child, "section#group", true), Some(parent));
    dom.set_attribute(child, "id", "current");
    dom.set_attribute(child, "class", "chosen");
    dom.set_attribute(child, "data-state", "READY");
    assert_eq!(matches(&mut dom, child, "#old, .pick", false), Some(NONE));
    assert_eq!(query(&mut dom, DOCUMENT, "#group > #current.chosen[data-state=ready i]", 0), Some(vec![child]));
    assert_eq!(query(&mut dom, parent, ":scope > b + b", 1).unwrap().len(), 1);
    assert_eq!(query(&mut dom, DOCUMENT, "!!", 0), None);
    assert_eq!(matches(&mut dom, child, "!!", false), None);
    assert!(dom.remove_child(parent, child));
    assert_eq!(query(&mut dom, DOCUMENT, "#current", 0), Some(vec![]));
    assert_eq!(matches(&mut dom, child, "#current.chosen", true), Some(child));
    assert_eq!(matches(&mut dom, child, "section", true), Some(NONE));
}

#[test]
fn matching_cost_does_not_scale_with_unrelated_styled_nodes() {
    use std::time::{Duration, Instant};
    let mut dom = Dom::parse("<!doctype html><section id=group><b id=target class=pick></b></section>");
    let child = by_id(&dom, "target");
    let parent = by_id(&dom, "group");
    let measure = |dom: &mut Dom| {
        let start = Instant::now();
        for _ in 0..1000 {
            assert_eq!(matches(dom, child, "#group > b.pick", false), Some(child));
            assert_eq!(matches(dom, child, "#group", true), Some(parent));
            assert_eq!(query(dom, parent, ".pick", 1), Some(vec![child]));
        }
        start.elapsed()
    };
    measure(&mut dom); // Warm up Stylo and the atom interner before measuring.
    let small = measure(&mut dom);
    for i in 0..12_000 {
        let detached = dom.create_element("div");
        dom.set_attribute(detached, "style", &format!("width:{i}px;color:red;--unused:{i}"));
    }
    let large = measure(&mut dom);
    eprintln!("3000 selector calls: small {small:?}, with 12000 unrelated styled nodes {large:?}");
    // A generous ratio plus scheduling allowance tolerates noisy hosts, but catches rebuilding
    // and parsing the whole arena on every call (the YouTube transition regression).
    assert!(large < small * 20 + Duration::from_millis(250), "unrelated nodes increased selector cost: {small:?} -> {large:?}");
}

#[test]
fn selectors_in_shadow_and_frame_documents() {
    let mut dom = Dom::parse("<!doctype html><div id=host><template shadowrootmode=open><b id=inside></b></template><b id=light></b></div>");
    let host = by_id(&dom, "host");
    let shadow = dom.shadow_root(host);
    let inside = by_id(&dom, "inside");
    assert_eq!(query(&mut dom, shadow, "#inside", 1), Some(vec![inside]));
    assert_eq!(query(&mut dom, DOCUMENT, "#inside", 0), Some(vec![]));
    assert_eq!(matches(&mut dom, inside, "#host", true), Some(NONE));
    let frame = dom.parse_frame_document("<!doctype html><html><body id=Frame><b id=Mixed class=Case></b></body></html>");
    let child = by_id(&dom, "Mixed");
    assert_eq!(query(&mut dom, frame, "body > #Mixed.Case", 1), Some(vec![child]));
    assert_eq!(query(&mut dom, frame, "#mixed", 0), Some(vec![]));
    assert_eq!(matches(&mut dom, child, "#Mixed.Case", false), Some(child));
    assert_eq!(matches(&mut dom, child, "#mixed", false), Some(NONE));
    assert_eq!(matches(&mut dom, child, "body", true), Some(by_id(&dom, "Frame")));
    let quirks = dom.parse_frame_document("<html><body><b id=Upper class=Case></b></body></html>");
    let upper = by_id(&dom, "Upper");
    assert_eq!(query(&mut dom, quirks, "#upper.case", 1), Some(vec![upper]));
    assert_eq!(matches(&mut dom, upper, "#upper.case", false), Some(upper));
}
