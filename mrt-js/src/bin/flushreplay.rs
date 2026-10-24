//! Runs again the layout flushes `realsite` recorded with `MRT_RECORD_FLUSHES=DIR`, without the
//! page, its scripts or the bridge (see flushrec.rs).
//!
//! usage: flushreplay DIR [FIRST [LAST]] [--ops]
//! With --ops it only prints each flush's restyle ops by kind (C cleared subtree, H hint, I inserted,
//! S snapshot, E everything).
//! With --ops-of LO-HI it only prints each flush's restyle and frame ops on nodes LO to HI, in full.
//! With --dom NODE it only prints the subtree of element NODE in each flush's DOM (tags, attributes, text).
//! With MRT_VERIFY_INCREMENTAL=1 each flush is checked against a layout from scratch; with
//! MRT_FLUSH_TIMES=1 each prints its phases.

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    let ops_only = args.iter().any(|a| a == "--ops");
    args.retain(|a| a != "--ops");
    let dom_node: Option<u32> = args.iter().position(|a| a == "--dom").map(|i| {
        let n = args[i + 1].parse().expect("--dom NODE");
        args.drain(i..=i + 1);
        n
    });
    let dir = std::path::PathBuf::from(args.get(1).expect("usage: flushreplay DIR [FIRST [LAST]]"));
    let first = args.get(2).map_or(1, |a| a.parse().unwrap());
    let last = args.get(3).map_or(usize::MAX, |a| a.parse().unwrap());
    let ops_of: Option<(u32, u32)> = args.iter().position(|a| a == "--ops-of").map(|i| {
        let (lo, hi) = args[i + 1].split_once('-').expect("--ops-of LO-HI");
        let range = (lo.parse().unwrap(), hi.parse().unwrap());
        args.drain(i..=i + 1);
        range
    });
    if let Some((lo, hi)) = ops_of {
        use mrt_js::flushrec::FrameOp;
        use mrt_js::mrt_style::restyle::RestyleOp;
        let dir = std::path::PathBuf::from(&args[1]);
        let first = args.get(2).map_or(1, |a| a.parse().unwrap());
        let last = args.get(3).map_or(usize::MAX, |a| a.parse().unwrap());
        let wanted = |n: u32| (lo..=hi).contains(&n);
        for flush in mrt_js::flushrec::recorded_flushes(&dir).into_iter().filter(|n| (first..=last).contains(n)) {
            let r = mrt_js::flushrec::read_flush(&dir, flush);
            for op in &r.ops {
                let node = match op {
                    RestyleOp::ClearSubtree(n) | RestyleOp::Hint(n, _) | RestyleOp::Inserted(n) | RestyleOp::Snapshot(n, _) => Some(*n),
                    RestyleOp::Everything => None,
                };
                if node.is_none_or(wanted) {
                    println!("{flush} restyle {op:?}");
                }
            }
            for op in &r.frame_ops {
                let node = match op {
                    FrameOp::Removed(rc) => rc.mChild,
                    FrameOp::Inserted(n) | FrameOp::TextChanged(n) | FrameOp::AttrChanged(n, _) | FrameOp::Reframe(n) => *n,
                };
                if wanted(node) {
                    println!("{flush} frame {op:?}");
                }
            }
        }
        return;
    }
    if let Some(node) = dom_node {
        for flush in mrt_js::flushrec::recorded_flushes(&dir).into_iter().filter(|n| (first..=last).contains(n)) {
            let r = mrt_js::flushrec::read_flush(&dir, flush);
            println!("flush {flush}:");
            print_subtree(&r.dom, node, 1);
        }
        return;
    }
    if ops_only {
        use mrt_js::mrt_style::restyle::RestyleOp;
        for flush in mrt_js::flushrec::recorded_flushes(&dir).into_iter().filter(|n| (first..=last).contains(n)) {
            let r = mrt_js::flushrec::read_flush(&dir, flush);
            let mut counts = [0usize; 5];
            for op in &r.ops {
                counts[match op {
                    RestyleOp::ClearSubtree(_) => 0,
                    RestyleOp::Hint(..) => 1,
                    RestyleOp::Inserted(_) => 2,
                    RestyleOp::Snapshot(..) => 3,
                    RestyleOp::Everything => 4,
                }] += 1;
            }
            println!("{flush}: C {} H {} I {} S {} E {}", counts[0], counts[1], counts[2], counts[3], counts[4]);
        }
        return;
    }
    let t = std::time::Instant::now();
    let panicked = mrt_js::replay_flushes(&dir, first..=last);
    eprintln!("REPLAY done in {:.1} s; flushes whose layout panicked: {:?}", t.elapsed().as_secs_f64(), panicked);
}

/// Prints `id` and its descendants, one node a line, indented by depth.
fn print_subtree(dom: &mrt_js::mrt_style::mrt_dom::Dom, id: u32, depth: usize) {
    use mrt_js::mrt_style::mrt_dom::Kind;
    if id as usize >= dom.nodes.len() {
        return;
    }
    let pad = "  ".repeat(depth);
    match dom.node(id).kind {
        Kind::Element => {
            let attrs: Vec<String> = dom.attrs_of(id).iter().map(|a| format!("{}={:?}", dom.attr_qualified_name(a), dom.attr_value(a))).collect();
            println!("{pad}<{}> #{id} {}", dom.tag(id), attrs.join(" "));
        }
        Kind::Text => println!("{pad}#{id} {:?}", dom.text_of(id)),
        _ => println!("{pad}#{id} {:?}", dom.node(id).kind),
    }
    for c in dom.children(id) {
        print_subtree(dom, c, depth + 1);
    }
}
