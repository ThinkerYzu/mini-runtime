//! A record of what a page's layout thread was asked to do, flush by flush, so that the flushes can
//! be run again offline (`flushreplay`): the DOM each flush got, its stylesheet links' text and the
//! restyle ops. Written when `MRT_RECORD_FLUSHES=DIR` is set (see `host.rs`, `layout_flush`).
//!
//! Files in DIR, by flush number N (1, 2, ...): `N.dom` (`Dom::to_bytes`), `N.ops` (the restyle ops,
//! below), `N.frameops` (the frame ops),
//! and `N.sheets` when the links' sheets differ from the last recorded ones (a flush uses the latest
//! `.sheets` file at or before its number). Every number is a little-endian u32, every string a u32
//! length then its UTF-8 bytes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mrt_style::mrt_dom::NodeId;
use mrt_style::restyle::{ElementSnapshotData, RestyleOp};
use mrt_style::style::invalidation::element::restyle_hints::RestyleHint;

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    put_u32(out, s.len() as u32);
    out.extend_from_slice(s.as_bytes());
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u32(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.b[self.at..self.at + 4].try_into().unwrap());
        self.at += 4;
        v
    }
    fn u64(&mut self) -> u64 {
        (self.u32() as u64) | ((self.u32() as u64) << 32)
    }
    fn string(&mut self) -> String {
        let len = self.u32() as usize;
        let s = String::from_utf8(self.b[self.at..self.at + len].to_vec()).unwrap();
        self.at += len;
        s
    }
    fn done(&self) -> bool {
        self.at >= self.b.len()
    }
}

/// A change of the DOM as the frames see it (DESIGN, "Incremental style and layout", step 3), recorded
/// by the runtime's DOM funnels next to the restyle ops: the layout thread applies the removals to its
/// kept frames before restyling (with the DOM as it was at the removal), the rest after.
#[derive(Clone, Debug)]
pub enum FrameOp {
    /// A node left its parent (Gecko: PresShell::ContentWillBeRemoved).
    Removed(mrt_layout::RemovedContent),
    /// A node was inserted (its frames are built in the flush, as Gecko builds lazily inserted content).
    Inserted(NodeId),
    /// A text node's text changed (PresShell::CharacterDataChanged).
    TextChanged(NodeId),
    /// An attribute of an element changed (PresShell::AttributeChanged; most of what it does to frames
    /// comes through the restyle, the rest is decided by the layout thread).
    AttrChanged(NodeId, String),
    /// The element's frames are to be rebuilt (a shadow root attached: PresShell::DestroyFramesForAndRestyle).
    Reframe(NodeId),
}

/// The frame ops as bytes: per op a tag (0 Removed, 1 Inserted, 2 TextChanged, 3 AttrChanged, 4 Reframe)
/// and its fields.
pub fn encode_frame_ops(ops: &[FrameOp]) -> Vec<u8> {
    let mut out = Vec::new();
    for op in ops {
        match op {
            FrameOp::Removed(r) => {
                put_u32(&mut out, 0);
                for v in [r.mChild, r.mContainer, r.mPrevSibling, r.mNextSibling] {
                    put_u32(&mut out, v);
                }
                put_u32(&mut out, r.mPrevSiblingHasPrevSibling as u32 | (r.mNextSiblingHasNextSibling as u32) << 1);
            }
            FrameOp::Inserted(n) => {
                put_u32(&mut out, 1);
                put_u32(&mut out, *n);
            }
            FrameOp::TextChanged(n) => {
                put_u32(&mut out, 2);
                put_u32(&mut out, *n);
            }
            FrameOp::AttrChanged(n, name) => {
                put_u32(&mut out, 3);
                put_u32(&mut out, *n);
                put_str(&mut out, name);
            }
            FrameOp::Reframe(n) => {
                put_u32(&mut out, 4);
                put_u32(&mut out, *n);
            }
        }
    }
    out
}

/// What `encode_frame_ops` wrote.
pub fn decode_frame_ops(b: &[u8]) -> Vec<FrameOp> {
    let mut r = Reader { b, at: 0 };
    let mut ops = Vec::new();
    while !r.done() {
        let op = match r.u32() {
            0 => {
                let (child, container, prev, next) = (r.u32(), r.u32(), r.u32(), r.u32());
                let bits = r.u32();
                FrameOp::Removed(mrt_layout::RemovedContent { mChild: child, mContainer: container, mPrevSibling: prev, mNextSibling: next, mPrevSiblingHasPrevSibling: bits & 1 != 0, mNextSiblingHasNextSibling: bits & 2 != 0 })
            }
            1 => FrameOp::Inserted(r.u32()),
            2 => FrameOp::TextChanged(r.u32()),
            3 => {
                let n = r.u32();
                FrameOp::AttrChanged(n, r.string())
            }
            4 => FrameOp::Reframe(r.u32()),
            tag => panic!("flush record: unknown frame op {tag}"),
        };
        ops.push(op);
    }
    ops
}

/// The ops as bytes: per op a tag (0 ClearSubtree, 1 Hint, 2 Inserted, 3 Snapshot, 4 Everything),
/// then its fields.
pub fn encode_ops(ops: &[RestyleOp]) -> Vec<u8> {
    let mut out = Vec::new();
    for op in ops {
        match op {
            RestyleOp::ClearSubtree(n) => {
                put_u32(&mut out, 0);
                put_u32(&mut out, *n);
            }
            RestyleOp::Hint(n, hint) => {
                put_u32(&mut out, 1);
                put_u32(&mut out, *n);
                put_u32(&mut out, hint.bits() as u32);
            }
            RestyleOp::Inserted(n) => {
                put_u32(&mut out, 2);
                put_u32(&mut out, *n);
            }
            RestyleOp::Snapshot(n, s) => {
                put_u32(&mut out, 3);
                put_u32(&mut out, *n);
                match &s.attrs {
                    Some(attrs) => {
                        put_u32(&mut out, attrs.len() as u32);
                        for (name, value, ns) in attrs {
                            put_str(&mut out, name);
                            put_str(&mut out, value);
                            put_u32(&mut out, *ns as u32);
                        }
                    }
                    None => put_u32(&mut out, u32::MAX),
                }
                match s.state {
                    Some(state) => {
                        put_u32(&mut out, 1);
                        put_u32(&mut out, state as u32);
                        put_u32(&mut out, (state >> 32) as u32);
                    }
                    None => put_u32(&mut out, 0),
                }
                put_u32(&mut out, s.changed_attrs.len() as u32);
                for a in &s.changed_attrs {
                    put_str(&mut out, a);
                }
                put_u32(&mut out, s.id_changed as u32 | (s.class_changed as u32) << 1 | (s.other_attributes_changed as u32) << 2);
            }
            RestyleOp::Everything => put_u32(&mut out, 4),
        }
    }
    out
}

/// What `encode_ops` wrote.
pub fn decode_ops(b: &[u8]) -> Vec<RestyleOp> {
    let mut r = Reader { b, at: 0 };
    let mut ops = Vec::new();
    while !r.done() {
        let op = match r.u32() {
            0 => RestyleOp::ClearSubtree(r.u32()),
            1 => {
                let n = r.u32();
                RestyleOp::Hint(n, RestyleHint::from_bits_retain(r.u32() as u16))
            }
            2 => RestyleOp::Inserted(r.u32()),
            3 => {
                let n = r.u32();
                let count = r.u32();
                let attrs = (count != u32::MAX).then(|| (0..count).map(|_| (r.string(), r.string(), r.u32() as u8)).collect());
                let state = (r.u32() == 1).then(|| r.u64());
                let changed_attrs = (0..r.u32()).map(|_| r.string()).collect();
                let bits = r.u32();
                RestyleOp::Snapshot(
                    n,
                    ElementSnapshotData { attrs, state, changed_attrs, id_changed: bits & 1 != 0, class_changed: bits & 2 != 0, other_attributes_changed: bits & 4 != 0 },
                )
            }
            4 => RestyleOp::Everything,
            tag => panic!("flush record: unknown op {tag}"),
        };
        ops.push(op);
    }
    ops
}

fn encode_sheets(sheets: &HashMap<NodeId, Arc<str>>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut list: Vec<_> = sheets.iter().collect();
    list.sort_by_key(|e| *e.0);
    for (node, text) in list {
        put_u32(&mut out, *node);
        put_str(&mut out, text);
    }
    out
}

fn decode_sheets(b: &[u8]) -> HashMap<NodeId, Arc<str>> {
    let mut r = Reader { b, at: 0 };
    let mut sheets = HashMap::new();
    while !r.done() {
        let node = r.u32();
        sheets.insert(node, Arc::from(r.string()));
    }
    sheets
}

/// Writes the flushes of one page (on its layout thread).
pub struct FlushRecorder {
    dir: PathBuf,
    /// The sheets last written, as (node, address of the text): the cache shares one `Arc` per URL.
    last_sheets: Vec<(NodeId, usize)>,
}

impl FlushRecorder {
    /// A recorder if `MRT_RECORD_FLUSHES` names a directory (made if missing).
    pub fn from_env() -> Option<FlushRecorder> {
        let dir = PathBuf::from(std::env::var_os("MRT_RECORD_FLUSHES")?);
        std::fs::create_dir_all(&dir).ok()?;
        Some(FlushRecorder { dir, last_sheets: Vec::new() })
    }

    pub fn record(&mut self, flush: usize, dom: &mrt_style::mrt_dom::Dom, sheets: &HashMap<NodeId, Arc<str>>, ops: &[RestyleOp], frame_ops: &[FrameOp]) {
        let path = |ext: &str| self.dir.join(format!("{flush}.{ext}"));
        std::fs::write(path("dom"), dom.to_bytes()).unwrap();
        std::fs::write(path("ops"), encode_ops(ops)).unwrap();
        std::fs::write(path("frameops"), encode_frame_ops(frame_ops)).unwrap();
        let mut key: Vec<(NodeId, usize)> = sheets.iter().map(|(n, t)| (*n, t.as_ptr() as usize)).collect();
        key.sort();
        if key != self.last_sheets || flush == 1 {
            std::fs::write(path("sheets"), encode_sheets(sheets)).unwrap();
            self.last_sheets = key;
        }
    }
}

/// One recorded flush, read back.
pub struct RecordedFlush {
    pub flush: usize,
    pub dom: mrt_style::mrt_dom::Dom,
    pub sheets: HashMap<NodeId, Arc<str>>,
    pub ops: Vec<RestyleOp>,
    /// Empty for a record made before the frame ops were recorded.
    pub frame_ops: Vec<FrameOp>,
}

/// The numbers of the flushes recorded in `dir`, in order.
pub fn recorded_flushes(dir: &Path) -> Vec<usize> {
    let mut list: Vec<usize> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| {
            let name = e.ok()?.file_name().into_string().ok()?;
            name.strip_suffix(".dom")?.parse().ok()
        })
        .collect();
    list.sort();
    list
}

/// Flush `flush` of `dir`, with the sheets of the latest `.sheets` file at or before it.
pub fn read_flush(dir: &Path, flush: usize) -> RecordedFlush {
    let dom = mrt_style::mrt_dom::Dom::from_bytes(&std::fs::read(dir.join(format!("{flush}.dom"))).unwrap()).expect("not a saved DOM");
    let ops = decode_ops(&std::fs::read(dir.join(format!("{flush}.ops"))).unwrap());
    let sheets = (1..=flush)
        .rev()
        .find_map(|n| std::fs::read(dir.join(format!("{n}.sheets"))).ok())
        .map(|b| decode_sheets(&b))
        .unwrap_or_default();
    let frame_ops = std::fs::read(dir.join(format!("{flush}.frameops"))).map(|b| decode_frame_ops(&b)).unwrap_or_default();
    RecordedFlush { flush, dom, sheets, ops, frame_ops }
}
