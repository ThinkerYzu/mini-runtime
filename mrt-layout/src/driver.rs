// ADAPTER (not a translation of a Firefox file): lays out a styled document, the way the first
// PresShell::Initialize and reflow of a page do. The pieces are Gecko's (nsCSSFrameConstructor,
// PresShell::DoReflow, the viewport and canvas frames); this function is the glue for a program
// that has a `StyleDom` (mrt-dom parsed, mrt-style resolved) and wants the frames and their sizes.
//
// Crate dependency, decided: `mrt-layout` depends on `mrt-style` (which depends on `mrt-dom`), as it
// did for the style adapters before. The frame constructor reads the arena DOM and the computed
// styles through a `&StyleDom`; mrt-style and mrt-dom know nothing of layout, so there is no cycle.
// The binary `layoutcount` (src/bin/layoutcount.rs) and the tests use this function.
use crate::*;
use mrt_style::mrt_dom::{Kind, NodeId, DOCUMENT, NONE};
use mrt_style::StyleDom;
use std::collections::BTreeMap;

/// What laying out a document made.
pub struct PageLayout {
    /// The primary frame of each element that has an `id` (only when asked for, see ConstructFramesTracking).
    pub mFramesOfElementsWithId: Vec<(NodeId, nsIFrame)>,
    pub mViewport: nsIFrame,
    pub mCanvas: nsIFrame,
    /// The frame of the document element; None when it is display:none.
    pub mRootElementFrame: Option<nsIFrame>,
    /// The size the viewport wanted, in app units.
    pub mViewportSize: nsSize,
    /// The root scroll offset, in app units (PresShellFrames::SetRootScrollPosition).
    pub mRootScrollPosition: nsPoint,
}

impl PageLayout {
    /// The height of the document in CSS px, as the canvas frame reports it (the root element's
    /// margin box rounded up to a whole pixel).
    pub fn DocumentHeightInCSSPixels(&self) -> f64 {
        self.mCanvas.GetSize().height as f64 / AppUnitsPerCSSPixel() as f64
    }

    /// The border-box height of the document element's frame in CSS px.
    pub fn RootElementHeightInCSSPixels(&self) -> Option<f64> {
        self.mRootElementFrame.map(|f| f.GetSize().height as f64 / AppUnitsPerCSSPixel() as f64)
    }
}

/// ADAPTER: makes `aStyleDom` last as long as the thread, for a program that lays a document out and
/// drops it before it is done with the frames: the frames read their content from the DOM (see
/// nsIContent in nsIFrame.rs). The tests use it in their helpers; `layoutcount` just keeps its
/// StyleDom.
pub fn KeepDocumentAlive(aStyleDom: StyleDom) -> &'static StyleDom {
    Box::leak(Box::new(aStyleDom))
}

/// The document element of the document: the first element child of the document node.
pub fn DocumentElement(aStyleDom: &StyleDom) -> Option<NodeId> {
    let dom = &aStyleDom.tree.dom;
    dom.children(aStyleDom.tree.doc).find(|&c| dom.node(c).kind == Kind::Element)
}

/// Builds the frame tree of `aStyleDom` (the styles must be resolved), for a viewport of
/// `aWidthPx` x `aHeightPx` CSS px. The frame tree is the one of this thread (`WithFrameTree`):
/// call it once per thread. `ReflowFrames` lays it out.
pub fn ConstructFrames(aStyleDom: &StyleDom, aWidthPx: i32, aHeightPx: i32) -> PageLayout {
    ConstructFramesTracking(aStyleDom, aWidthPx, aHeightPx, false)
}

/// ConstructFrames; with `aTrack` the layout also keeps the frames of the elements that have an `id`.
pub fn ConstructFramesTracking(aStyleDom: &StyleDom, aWidthPx: i32, aHeightPx: i32, aTrack: bool) -> PageLayout {
    ConstructFramesTrackingMode(aStyleDom, aWidthPx, aHeightPx, aTrack, false)
}

/// ConstructFramesTracking; with `aTrackAll` the layout keeps the primary frame of every element
/// (for `layoutcount --boxes`), in `mFramesOfElementsWithId` too.
pub fn ConstructFramesTrackingMode(aStyleDom: &StyleDom, aWidthPx: i32, aHeightPx: i32, aTrack: bool, aTrackAll: bool) -> PageLayout {
    let (layout, _frameConstructor) = ConstructFramesKeeping(aStyleDom, aWidthPx, aHeightPx, aTrack, aTrackAll);
    layout
}

/// ConstructFramesTrackingMode, which also gives the frame constructor back (PresShellFrames keeps it
/// to apply the DOM's changes later).
fn ConstructFramesKeeping(aStyleDom: &StyleDom, aWidthPx: i32, aHeightPx: i32, aTrack: bool, aTrackAll: bool) -> (PageLayout, nsCSSFrameConstructor) {
    let mut presContext = nsPresContext::default();
    presContext.SetVisibleArea(nsRect::new(0, 0, aWidthPx * AppUnitsPerCSSPixel(), aHeightPx * AppUnitsPerCSSPixel()));
    presContext.SetCompatibilityMode(match aStyleDom.tree.mode() {
        2 => eCompatibility_NavQuirks,
        1 => eCompatibility_AlmostStandards,
        _ => eCompatibility_FullStandards,
    });
    nsPresContext::SetCurrent(presContext);

    // Gecko: CharacterData::SetTextInternal and BindToTree call Document::SetBidiEnabled for a text
    // node whose buffer has RTL characters (the DOM is built before this runs, so it is done here).
    nsPresContext::ResetBidiEnabled();
    {
        let dom = &aStyleDom.tree.dom;
        for n in 0..dom.nodes.len() as u32 {
            if dom.node(n).kind == mrt_style::mrt_dom::Kind::Text && TextHasRTLChars(dom.text_of(n)) {
                presContext.SetBidiEnabled();
                break;
            }
        }
    }

    let docElement = DocumentElement(aStyleDom).expect("document has no root element");
    let mut frameConstructor = nsCSSFrameConstructor::new(aStyleDom);
    if aTrack {
        frameConstructor.TrackPrimaryFramesOfElementsWithId();
    }
    if aTrackAll {
        frameConstructor.TrackPrimaryFramesOfAllElements();
    }
    PresShell::Initialize(&mut frameConstructor, docElement);
    let viewport = frameConstructor.GetRootFrame().unwrap();
    let canvas = frameConstructor.GetCanvasFrame().unwrap();
    let rootElementFrame = frameConstructor.GetRootElementFrame();
    let mut framesOfElementsWithId: Vec<(NodeId, nsIFrame)> = frameConstructor.TrackedPrimaryFrames();
    framesOfElementsWithId.sort_by_key(|e| e.0);
    (PageLayout { mFramesOfElementsWithId: framesOfElementsWithId, mViewport: viewport, mCanvas: canvas, mRootElementFrame: rootElementFrame, mViewportSize: nsSize::new(0, 0), mRootScrollPosition: nsPoint::new(0, 0) }, frameConstructor)
}

/// Gecko: CharacterData::SetTextInternal's test for Document::SetBidiEnabled: the text has a
/// right-to-left character (HasRTLChars on the text's UTF-16 units).
///
/// ADAPTER: the UTF-8 text is scanned in place. Each frame tree built from scratch runs this over every
/// text node, inline scripts included, and Gmail's page holds 4.6 MB of non-ASCII script text: copying
/// it to UTF-16 cost about 12 ms a build (GAPS B80). The first RTL character is U+0590, whose UTF-8
/// lead byte is 0xD6; continuation bytes are 0x80 to 0xBF, so a byte of 0xD6 or more always starts a
/// character, and only those characters are decoded.
fn TextHasRTLChars(aText: &str) -> bool {
    let bytes = aText.as_bytes();
    let mut i = 0;
    while let Some(offset) = bytes[i..].iter().position(|&b| b >= 0xD6) {
        let start = i + offset;
        let c = aText[start..].chars().next().unwrap();
        if CodePointIsRTL(c as u32) {
            return true;
        }
        i = start + c.len_utf8();
    }
    false
}

/// HasRTLChars's test for one code point: a supplementary character is RTL when its lead surrogate is
/// (0xD802 and 0xD803 lead U+10800 to U+10FFF, 0xD83A and 0xD83B lead U+1E800 to U+1EFFF).
fn CodePointIsRTL(aCodePoint: u32) -> bool {
    if aCodePoint < 0x10000 {
        HasRTLChars(&[aCodePoint as u16])
    } else {
        (0x10800..=0x10FFF).contains(&aCodePoint) || (0x1E800..=0x1EFFF).contains(&aCodePoint)
    }
}

/// ADAPTER: a page's frames kept from one layout flush to the next, with the frame constructor that
/// applies the DOM's changes to them (DESIGN, "Incremental style and layout", step 3); Gecko's
/// PresShell keeps the same two. The frames are those of this thread, over a StyleDom that must
/// outlive them and stay at the same address (its DOM is put back in place before each flush).
pub struct PresShellFrames {
    pub mFrameConstructor: nsCSSFrameConstructor,
    pub mLayout: PageLayout,
    /// The scroll offset of each element scroll container that is scrolled, in app units (bridge.js's
    /// element scroll state, task 0bec4): applied by ReflowDirty, after the reflow.
    pub mElementScrollPositions: Vec<(NodeId, nsPoint)>,
    /// What ApplyElementScroll moved: each container's frame and the offset its children are moved by.
    mAppliedElementScroll: Vec<(NodeId, nsIFrame, nsPoint)>,
    /// Each scrolled container's ClientAreaAndScrollSize, read before its children were moved.
    pub mElementScrollSizes: BTreeMap<NodeId, (f64, f64, f64, f64)>,
}

impl PresShellFrames {
    /// Builds the frames of `aStyleDom` (styled) for a viewport of `aWidthPx` x `aHeightPx` CSS px,
    /// as ConstructFrames; ReflowDirty lays them out.
    pub fn new(aStyleDom: &StyleDom, aWidthPx: i32, aHeightPx: i32) -> PresShellFrames {
        let (layout, frameConstructor) = ConstructFramesKeeping(aStyleDom, aWidthPx, aHeightPx, false, false);
        PresShellFrames {
            mFrameConstructor: frameConstructor,
            mLayout: layout,
            mElementScrollPositions: Vec::new(),
            mAppliedElementScroll: Vec::new(),
            mElementScrollSizes: BTreeMap::new(),
        }
    }

    /// Scrolls the element scroll containers to mElementScrollPositions, after a reflow: the children
    /// of each container's frame (all its child lists) move by minus its offset, as Gecko's scrolled
    /// frame moves inside its ScrollContainerFrame (ScrollContainerFrame::ScrollToImpl), so the boxes,
    /// hit testing and the intersection walks below it read scrolled.
    /// ADAPTER: the port has no scroll container frames (GAPS B155): the container's own frame stands
    /// for both the scroll container and its scrolled frame. The offset is the page's, not clamped
    /// here; UndoElementScroll puts the children back before the frames change again.
    pub fn ApplyElementScroll(&mut self) {
        self.mElementScrollSizes.clear();
        for &(node, offset) in &self.mElementScrollPositions {
            let Some(frame) = nsIContent::FromNodeId(node).GetPrimaryFrame() else { continue };
            // (the scroll size is that of the content where the reflow put it)
            self.mElementScrollSizes.insert(node, ClientAreaAndScrollSize(frame));
            ShiftChildFrames(frame, nsPoint::new(-offset.x, -offset.y));
            self.mAppliedElementScroll.push((node, frame, offset));
        }
    }

    /// Puts back the children ApplyElementScroll moved: called before the frames change (removals,
    /// reframes, reflow), while every frame it moved is still the primary frame of its element.
    pub fn UndoElementScroll(&mut self) {
        for (node, frame, offset) in std::mem::take(&mut self.mAppliedElementScroll) {
            if nsIContent::FromNodeId(node).GetPrimaryFrame() == Some(frame) {
                ShiftChildFrames(frame, offset);
            }
        }
    }

    /// Before the changes of a flush: what was read from the DOM before it changed is forgotten (text
    /// buffers, languages), and the style caches keyed by address are emptied.
    pub fn BeginChanges(&mut self) {
        nsIContent::ForgetDocumentReads();
        self.mFrameConstructor.ClearStyleCaches();
    }

    /// A node left the DOM (Gecko: PresShell::ContentWillBeRemoved), with the DOM as it was then.
    pub fn ContentRemoved(&mut self, aRemoved: RemovedContent) {
        self.mFrameConstructor.ContentRemoved(aRemoved);
    }

    /// Gecko: RestyleManager::ProcessRestyledFrames for a ReconstructFrame change hint:
    /// nsCSSFrameConstructor::RecreateFramesForContent(aContent, InsertionKind::Sync). A node that has
    /// no frames yet gets them (Gecko builds lazily inserted content this way).
    pub fn RecreateFramesForContent(&mut self, aContent: NodeId) {
        self.NoteNewText(aContent);
        self.mFrameConstructor.RecreateFramesForContent(aContent, InsertionKind::Sync);
        self.mFrameConstructor.ReleaseDestroyedFrames();
    }

    /// The text of a text node changed (Gecko: PresShell::CharacterDataChanged).
    pub fn CharacterDataChanged(&mut self, aContent: NodeId) {
        self.NoteNewText(aContent);
        self.mFrameConstructor.CharacterDataChanged(aContent);
        self.mFrameConstructor.ReleaseDestroyedFrames();
    }

    /// The rebuilds the changes asked for (InsertionKind::Async), until there are none, then the counter
    /// texts (Gecko: the RestyleManager's loop over posted restyles, and RecalcQuotesAndCounters).
    pub fn ProcessPendingReconstructs(&mut self) {
        loop {
            let mut pending = self.mFrameConstructor.TakePendingReconstructs();
            if pending.is_empty() {
                self.mFrameConstructor.RecalcQuotesAndCounters();
                // Taking the queue drains it: process this batch rather than
                // discarding the counter rebuilds in an emptiness check.
                pending = self.mFrameConstructor.TakePendingReconstructs();
                if pending.is_empty() {
                    break;
                }
            }
            for content in pending {
                self.RecreateFramesForContent(content);
            }
        }
        // the root element's frames may have been rebuilt
        let fc = &self.mFrameConstructor;
        self.mLayout.mCanvas = fc.GetCanvasFrame().unwrap_or(self.mLayout.mCanvas);
        self.mLayout.mRootElementFrame = fc.GetRootElementFrame();
    }

    /// What the dynamic paths did since the last call.
    pub fn TakeDynamicChangeCounts(&mut self) -> DynamicChangeCounts {
        self.mFrameConstructor.TakeDynamicChangeCounts()
    }

    /// Gecko: Document::SetBidiEnabled when text with right-to-left characters enters the document.
    fn NoteNewText(&self, aContent: NodeId) {
        if nsPresContext::Current().BidiEnabled() {
            return;
        }
        let dom = &self.mFrameConstructor.StyleDomOf().tree.dom;
        let mut stack = vec![aContent];
        while let Some(n) = stack.pop() {
            if dom.node(n).kind == mrt_style::mrt_dom::Kind::Text {
                if TextHasRTLChars(dom.text_of(n)) {
                    nsPresContext::Current().SetBidiEnabled();
                    return;
                }
            } else {
                stack.extend(dom.children(n));
                let root = dom.shadow_root(n);
                if root != NONE {
                    stack.push(root);
                }
            }
        }
    }

    /// Reflows what is dirty, from the viewport (Gecko: PresShell::ProcessReflowCommands with the
    /// viewport as the only reflow root). With `aEverything` every frame is marked dirty first, intrinsic
    /// sizes included (PresShell::FrameNeedsReflow(root, FrameAncestorsAndDescendants, NS_FRAME_IS_DIRTY)
    /// and nsIFrame::MarkSubtreeDirty on the root: a dirty frame alone reflows only what it must).
    pub fn ReflowDirty(&mut self, aEverything: bool) {
        let viewport = self.mLayout.mViewport;
        // (the children reflow where they are not scrolled)
        self.UndoElementScroll();
        if aEverything {
            // (MarkSubtreeDirty first: it stops at a frame that is dirty already)
            viewport.MarkSubtreeDirty();
            PresShell_FrameNeedsReflow(viewport, IntrinsicDirty::FrameAncestorsAndDescendants, NS_FRAME_IS_DIRTY);
        }
        if viewport.HasAnyStateBits(NS_FRAME_IS_DIRTY | NS_FRAME_HAS_DIRTY_CHILDREN) {
            // (reflowed where the viewport puts it, then scrolled again)
            self.mLayout.mCanvas.SetPosition__nsPoint(nsPoint::new(0, 0));
            let desired = PresShell::DoReflow(viewport);
            self.mLayout.mViewportSize = desired.PhysicalSize();
        }
        self.SetRootScrollPosition(self.mLayout.mRootScrollPosition);
        self.ApplyElementScroll();
        // the text buffers made from the DOM while reflowing are not needed any more (see nsIContent)
        nsIContent::ClearContentCaches();
    }

    /// Scrolls the document to `aPosition` (app units, from the top left of the scrolled area): the
    /// canvas frame, which stands for the root scroll frame's scrolled frame, is placed at minus it,
    /// as Gecko places the scrolled frame (ScrollContainerFrame::ScrollToImpl). Everything below the
    /// canvas then reads in client coordinates (BoxInDocument, NodesFromPoint, the intersection
    /// walks), while the fixed-position frames, the viewport's own children, stay where they are.
    /// ADAPTER: the offset is the bridge tab's (realsite --interactive); it is not clamped here, and
    /// sticky positioning does not follow it (GAPS B155).
    pub fn SetRootScrollPosition(&mut self, aPosition: nsPoint) {
        self.mLayout.mRootScrollPosition = aPosition;
        self.mLayout.mCanvas.SetPosition__nsPoint(nsPoint::new(-aPosition.x, -aPosition.y));
    }

    /// The primary frame of every element that has one.
    pub fn ElementFrames(&self) -> Vec<(NodeId, nsIFrame)> {
        let dom = &self.mFrameConstructor.StyleDomOf().tree.dom;
        (1..dom.nodes.len() as NodeId)
            .filter(|&n| dom.node(n).kind == mrt_style::mrt_dom::Kind::Element)
            .filter_map(|n| Some((n, nsIContent::FromNodeId(n).GetPrimaryFrame()?)))
            .collect()
    }

    /// `offsetParent` and `offsetLeft`, `offsetTop`, `offsetWidth`, `offsetHeight` of an element, from
    /// the frames of the last flush: the offset parent (NONE for none) and the rectangle in CSS px,
    /// rounded; None when the element has no frame (then null and zeros).
    ///
    /// Gecko: dom/base/Element.cpp Element::GetOffsetRect, after the layout flush.
    /// ADAPTER: no zoom (the Servo build has no `zoom`), so the rectangle is not unzoomed.
    pub fn GetOffsetRect(&self, aElement: NodeId) -> Option<(NodeId, [i32; 4])> {
        let dom = &self.mFrameConstructor.StyleDomOf().tree.dom;
        let doc = self.mFrameConstructor.StyleDomOf().tree.doc;
        nsIContent::FromNodeId(aElement).GetPrimaryFrame()?;
        let thisResult = GetUnretargetedOffsetsFor(dom, doc, aElement);
        let mut rect = thisResult.1;
        let mut parent = thisResult.0;
        while parent != NONE && ShouldBeRetargeted(dom, aElement, parent) {
            let result = GetUnretargetedOffsetsFor(dom, doc, parent);
            rect.x += result.1.x;
            rect.y += result.1.y;
            parent = result.0;
        }
        // CSSIntRect::FromAppUnitsRounded: each of the four rounded (NSToIntRound, half away from zero)
        let px = |v: nscoord| {
            let f = v as f32 / AppUnitsPerCSSPixel() as f32;
            (if f >= 0.0 { f + 0.5 } else { f - 0.5 }) as i32
        };
        Some((parent, [px(rect.x), px(rect.y), px(rect.width), px(rect.height)]))
    }

    /// What a ResizeObserverEntry for an element holds, from the frames of the last flush: [has a
    /// frame (0 or 1), border-box inline and block size, content-box inline and block size,
    /// device-pixel content-box inline and block size, contentRect x, y, width, height]. Sizes are
    /// logical (inline first) and in CSS px as floats; contentRect is physical, its origin the
    /// padding's top-left. Without a frame, zeros.
    ///
    /// Gecko: dom/base/ResizeObserver.cpp ResizeObserver::CalculateBoxSize, for each of the three
    /// boxes, and ResizeObserverEntry::SetContentRectAndSize.
    /// ADAPTER: no zoom (the Servo build has none); one fragment (dom.resize_observer.support_fragments
    /// is off in release); one device pixel per CSS px. The port has no scroll container frames, so
    /// GetContentRectSize is always the content rect (as with overlay scrollbars). NOT PORTED: SVG
    /// layout (the elements inside an outer <svg> have no frame here, so they get zeros, not their
    /// bounding box).
    pub fn ResizeObserverBoxes(&self, aElement: NodeId) -> [f64; 11] {
        let Some(frame) = nsIContent::FromNodeId(aElement).GetPrimaryFrame() else {
            return [0.0; 11];
        };
        let perPixel = AppUnitsPerCSSPixel();
        // CSSPixel::FromAppUnits: a float
        let css = |v: nscoord| v as f32 / perPixel as f32;
        let wm = frame.GetWritingMode();
        // LogicalPixelSize(wm, size): (inline, block)
        let logical = |w: f32, h: f32| if wm.IsVertical() { (h, w) } else { (w, h) };
        let padding = frame.GetUsedPadding();
        // non-replaced inline boxes: an empty size for every box (LogicalPixelSize(), horizontal)
        let (border, content, devicePixels) = if !frame.IsReplaced() && frame.IsLineParticipant() {
            ((0.0, 0.0), (0.0, 0.0), (0.0, 0.0))
        } else {
            let size = frame.GetSize();
            let contentSize = frame.GetContentRectRelativeToSelf().Size();
            // Device_pixel_content_box: the content size placed at the frame's offset to the
            // reference frame (the root here: no transforms), each corner rounded (floor(v + 0.5))
            let mut offset = frame.GetPosition();
            let mut f = frame.GetParent();
            while let Some(p) = f {
                offset = offset + p.GetPosition();
                f = p.GetParent();
            }
            let perDevPixel = perPixel as f64;
            let rect = [offset.x as f64 / perDevPixel, offset.y as f64 / perDevPixel, contentSize.width as f64 / perDevPixel,
                contentSize.height as f64 / perDevPixel].map(|v| v as f32);
            let round = |v: f32| (v + 0.5).floor();
            let dw = round(rect[0] + rect[2]) - round(rect[0]);
            let dh = round(rect[1] + rect[3]) - round(rect[1]);
            (logical(css(size.width), css(size.height)), logical(css(contentSize.width), css(contentSize.height)), logical(dw, dh))
        };
        // contentRect: the padding's top-left, and the content box's physical size back in app units
        // (CSSPixel::ToAppUnits rounds), then DOMRect::SetLayoutRect's 1/65536 px steps
        let (cw, ch) = if wm.IsVertical() { (content.1, content.0) } else { content };
        let appUnits = |v: f32| (v * perPixel as f32 + 0.5).floor() as i64;
        let (x, y, xMost, yMost) = (padding.left as i64, padding.top as i64, padding.left as i64 + appUnits(cw), padding.top as i64 + appUnits(ch));
        let toPx = |v: i64| (v as f64 * (65536.0 / perPixel as f64) + 0.5).floor() / 65536.0;
        let (rx, ry) = (toPx(x), toPx(y));
        [1.0, border.0 as f64, border.1 as f64, content.0 as f64, content.1 as f64, devicePixels.0 as f64, devicePixels.1 as f64,
            rx, ry, toPx(xMost) - rx, toPx(yMost) - ry]
    }

    /// The intersection root rectangle of an element root, relative to the viewport in app units:
    /// its scroll port when Gecko makes it a scroll container (here its padding box: no scrollbars
    /// are drawn and nothing is scrolled), else the union of its border boxes. None without a frame.
    ///
    /// Gecko: dom/base/DOMIntersectionObserver.cpp DOMIntersectionObserver::ComputeInput, the
    /// element case (through the transforms of the root and its ancestors).
    pub fn IntersectionRootRect(&self, aRoot: NodeId) -> Option<nsRect> {
        let rootFrame = nsIContent::FromNodeId(aRoot).GetPrimaryFrame()?;
        let viewport = self.mLayout.mViewport;
        Some(if IsScrollContainerStandIn(rootFrame) {
            // rootRect should be the content rect of rootFrame, not including the
            // scrollbars.
            nsLayoutUtils::TransformFrameRectToAncestor(rootFrame, rootFrame.GetPaddingRectRelativeToSelf(), viewport)
        } else {
            // rootRect should be the border rect of rootFrame.
            GetAllInFlowRectsUnion(rootFrame, viewport, true)
        })
    }

    /// What IntersectionObserver computes of a target in this document, relative to the viewport in
    /// app units: None when the target has no frame or is not a descendant of the root's frame
    /// (`aRoot` an element of this document, or NONE for the viewport: Intersect's early returns);
    /// else the target's rectangle (the union of its border boxes) and that rectangle clipped by the
    /// ancestors between the target and the root (None when a clip leaves nothing); a scroll
    /// container's port is inflated by `aScrollMargin`.
    ///
    /// Gecko: dom/base/DOMIntersectionObserver.cpp DOMIntersectionObserver::Intersect (steps 2.3 and
    /// 2.4: the target's box relative to itself, then through the transforms to the viewport) and
    /// ComputeTheIntersection (steps 1 to 4).
    /// ADAPTER: content-visibility is not read (layout ignores it, GAPS B77).
    pub fn IntersectionTarget(&self, aTarget: NodeId, aRoot: NodeId, aScrollMargin: &IntersectionMargin) -> Option<(nsRect, Option<nsRect>)> {
        let targetFrame = nsIContent::FromNodeId(aTarget).GetPrimaryFrame()?;
        let viewport = self.mLayout.mViewport;
        let rootFrame = if aRoot == NONE { viewport } else { nsIContent::FromNodeId(aRoot).GetPrimaryFrame()? };
        // 2.3: the root's frame (or a continuation of it) is an ancestor of the target's
        if rootFrame == targetFrame || !IsAncestorFrameConsideringContinuations(rootFrame, targetFrame) {
            return None;
        }
        // 2.4. Set targetRect to the DOMRectReadOnly obtained by running the
        // getBoundingClientRect() algorithm on target. We compute the box relative to
        // self first, then transform.
        let targetRectRelativeToTarget = GetAllInFlowRectsUnion(targetFrame, targetFrame, true);
        let targetRect = nsLayoutUtils::TransformFrameRectToAncestor(targetFrame, targetRectRelativeToTarget, viewport);
        let clipped = ComputeTheIntersection(targetFrame, targetRectRelativeToTarget, rootFrame, viewport, aScrollMargin);
        Some((targetRect, clipped))
    }

    /// A rectangle relative to the viewport clipped by `aElement`'s frame and its ancestors up to the
    /// viewport: the walk of ComputeTheIntersection carried on from a frame element into its parent
    /// document. None when a clip leaves nothing, or the element has no frame.
    /// ADAPTER: the rectangle is taken back to the element's frame by its offset, without the
    /// transforms of the frame element's ancestors (SubdocumentOrigin has none either).
    pub fn IntersectionClipFrom(&self, aElement: NodeId, aRect: nsRect, aScrollMargin: &IntersectionMargin) -> Option<nsRect> {
        let frame = nsIContent::FromNodeId(aElement).GetPrimaryFrame()?;
        let viewport = self.mLayout.mViewport;
        let offset = frame.GetOffsetTo(viewport);
        let rect = nsRect::new(aRect.x - offset.x, aRect.y - offset.y, aRect.width, aRect.height);
        // (the frame element's own clip first: Gecko's walk reaches it as the subdocument's container)
        let axes = OverflowClipAxes(frame);
        let rect = if axes.isEmpty() {
            rect
        } else {
            let clip = OverflowAreas::GetOverflowClipRect(rect, frame.GetRectRelativeToSelf(), axes, OverflowClipMarginOf(frame, axes));
            rect.EdgeInclusiveIntersection(clip)?
        };
        ComputeTheIntersection(frame, rect, viewport, viewport, aScrollMargin)
    }

    /// The content box of a frame element relative to the viewport, in app units: where its
    /// document's viewport is (nsSubDocumentFrame's inner area). None without a frame.
    pub fn SubdocumentOrigin(&self, aElement: NodeId) -> Option<nsPoint> {
        let frame = nsIContent::FromNodeId(aElement).GetPrimaryFrame()?;
        let offset = frame.GetOffsetTo(self.mLayout.mViewport);
        let bp = frame.GetUsedBorderAndPadding();
        Some(nsPoint::new(offset.x + bp.left, offset.y + bp.top))
    }

    /// The nodes under a point of the viewport, in app units, the topmost first: the content of each
    /// frame nsLayoutUtils::GetFramesForArea finds (Text nodes included), for QueryNodesFromRect.
    /// `aScrollPort` is the viewport's visible area (the root scroll frame's port).
    ///
    /// Gecko: dom/base/DocumentOrShadowRoot.cpp QueryNodesFromRect, its first part:
    /// Document::GetContentInThisDocument and the walk out of native anonymous content.
    /// ADAPTER: a frame without content of the DOM (generated content, the port's synthetic content,
    /// anonymous boxes without content) is answered by the nearest frame up through placeholders
    /// that has some, as Gecko answers native anonymous content by its subtree root's parent; the
    /// canvas and the viewport (standing for the root scroll frame, see
    /// ViewportFrame_BuildDisplayList) are the root element's, as in Gecko.
    pub fn NodesFromPoint(&self, aPoint: nsPoint, aScrollPort: nsRect) -> Vec<NodeId> {
        let frames = nsLayoutUtils::GetFramesForArea(self.mLayout.mViewport, aScrollPort, nsRect::new(aPoint.x, aPoint.y, 1, 1));
        let docElement = DocumentElement(self.mFrameConstructor.StyleDomOf());
        frames
            .into_iter()
            .filter_map(|frame| {
                let mut f = Some(frame);
                while let Some(fr) = f {
                    if matches!(fr.Type(), LayoutFrameType::Viewport | LayoutFrameType::Canvas) {
                        return docElement;
                    }
                    if let Some(content) = fr.GetContent() {
                        let id = content.Id();
                        if id != NONE && id < kSyntheticContentBase {
                            return Some(id);
                        }
                    }
                    f = nsLayoutUtils::GetParentOrPlaceholderFor(fr);
                }
                None
            })
            .collect()
    }

    /// `aElement`'s innerText from the last flush's frames, as UTF-16; None when the element is not
    /// rendered (no primary frame and not `display: contents`), for which innerText is textContent.
    ///
    /// Gecko: dom/html/nsGenericHTMLElement.cpp nsGenericHTMLElement::GetInnerText, its end
    /// (`IsRendered()`), and dom/base/nsRange.cpp nsRange::GetInnerTextNoFlush: the light tree's
    /// nodes in order, each with its primary frame.
    pub fn InnerText(&self, aElement: NodeId) -> Option<Vec<u16>> {
        let sd = self.mFrameConstructor.StyleDomOf();
        let dom = &sd.tree.dom;
        let content = nsIContent::FromNodeId(aElement);
        // Element::IsRendered
        let isDisplayContents = || sd.computed(aElement).is_some_and(|s| nsStyleDisplay::new(s).mDisplay() == mrt_style::style::values::specified::box_::Display::Contents);
        if content.GetPrimaryFrame().is_none() && !isDisplayContents() {
            return None;
        }

        let mut aValue = Vec::new();
        let mut result = InnerTextAccumulator { mString: &mut aValue, mRequiredLineBreakCount: 0 };

        if dom.node(aElement).kind == Kind::Text {
            AppendTransformedText(&mut result, content);
            return Some(aValue);
        }

        let mut currentNode = aElement;
        let mut currentState = TreeTraversalState::AFTER_NODE;

        let endNode = aElement;
        let endState = TreeTraversalState::AFTER_NODE;

        let firstChild = dom.node(aElement).first_child;
        if firstChild != NONE {
            currentNode = firstChild;
            currentState = TreeTraversalState::AT_NODE;
        }

        while currentNode != endNode || currentState != endState {
            let current = nsIContent::FromNodeId(currentNode);
            let f = current.GetPrimaryFrame();
            let isVisibleAndNotReplaced = IsVisibleAndNotInReplacedElement(f);
            if currentState == TreeTraversalState::AT_NODE {
                let isText = dom.node(currentNode).kind == Kind::Text;
                if isVisibleAndNotReplaced {
                    let f = f.unwrap();
                    result.AddRequiredLineBreakCount(GetRequiredInnerTextLineBreakCount(f));
                    if isText {
                        let text = nsTextFrame::from_frame(f).GetRenderedText(0, u32::MAX, TextOffsetType::OffsetsInContentText, TrailingWhitespace::Trim);
                        result.Append(&text.mString);
                    }
                }
                let child = dom.node(currentNode).first_child;
                if child != NONE {
                    currentNode = child;
                    continue;
                }
                currentState = TreeTraversalState::AFTER_NODE;
            }
            if currentNode == endNode && currentState == endState {
                break;
            }
            if isVisibleAndNotReplaced {
                let f = f.unwrap();
                if current.IsHTMLElement(nsGkAtoms::br) {
                    result.Append(&[b'\n' as u16]);
                }
                match f.StyleDisplay().DisplayInside() {
                    StyleDisplayInside::TableCell => {
                        if !IsLastCellOfRow(f) {
                            result.Append(&[b'\t' as u16]);
                        }
                    }
                    StyleDisplayInside::TableRow => {
                        if !IsLastRowOfRowGroup(f) || !f.GetParent().map_or(true, IsLastNonemptyRowGroupOfTable) {
                            result.Append(&[b'\n' as u16]);
                        }
                    }
                    _ => {} // Do nothing
                }
                result.AddRequiredLineBreakCount(GetRequiredInnerTextLineBreakCount(f));
            }
            let next = dom.node(currentNode).next_sibling;
            if next != NONE {
                currentNode = next;
                currentState = TreeTraversalState::AT_NODE;
            } else {
                currentNode = dom.node(currentNode).parent;
            }
        }

        // Do not flush trailing line breaks! Required breaks at the end of the text
        // are suppressed.
        Some(aValue)
    }

    /// The size each frame element gives its document, in CSS px (nsSubDocumentFrame::GetSubdocumentSize):
    /// the elements with a subdocument frame.
    pub fn SubdocumentSizes(&self) -> Vec<(NodeId, (i32, i32))> {
        self.ElementFrames()
            .into_iter()
            .filter(|(_, frame)| frame.Type() == LayoutFrameType::SubDocument)
            .map(|(node, frame)| (node, nsSubDocumentFrame::from_frame(frame).GetSubdocumentSize()))
            .collect()
    }

    /// Destroys the frames (DestroyFrameTree).
    pub fn Destroy(self) {
        DestroyFrameTree(self.mLayout);
    }
}

/// Reflows the frame tree with an inline-size of the viewport width and an unconstrained
/// block-size; afterwards the frames have their sizes and positions.
pub fn ReflowFrames(aLayout: &mut PageLayout) {
    let desired = PresShell::DoReflow(aLayout.mViewport);
    aLayout.mViewportSize = desired.PhysicalSize();
    // the text buffers made from the DOM while reflowing are not needed any more (see nsIContent)
    nsIContent::ClearContentCaches();
    ShrinkFrameStorage();
}

/// ADAPTER: gives back the spare capacity of the arrays that hold the frames and their members. They
/// grow by doubling while the frames are made, so up to half of each is unused when layout is done,
/// and the frames are never added to again (there are no dynamic changes). Gecko's arena has no such
/// slack to give back. ReflowFrames calls it; no reference into a frame's data may be held across it
/// (the last chunk of frames moves).
pub fn ShrinkFrameStorage() {
    WithFrameTree(|t| t.mFrames.shrink_to_fit());
    nsTextFrame::ShrinkTextFrameTable();
}

/// ADAPTER: frees the slots of `aFrames`, frames whose Destroy ran (FrameTree::TakeDestroyedFrames),
/// with what the side tables of this thread hold for them (the text frames' members, the line layout
/// stubs, the form controls' data), so a new frame in the slot starts clean. The frame constructor
/// calls it once a change is done (nsCSSFrameConstructor::ReleaseDestroyedFrames).
pub fn ReleaseFrameSlots(aFrames: &[nsIFrame]) {
    nsTextFrame::ReleaseTextFrameData(aFrames);
    crate::nsLineLayoutAdapter::ReleaseLineLayoutStubs(aFrames);
    ReleaseFormControlData(aFrames);
    WithFrameTree(|t| t.FreeSlots(aFrames));
}

/// ADAPTER (stage 4): destroys the frame tree of `aLayout`, as Gecko's PresShell::Destroy does: the
/// viewport frame's Destroy runs down the whole tree (each class frees what it owns: line boxes, frame
/// lists, cell maps, member data, and every frame its deletable properties), then the stores this
/// thread keeps per frame or per document are emptied, as the pres arena is freed: the frames, the
/// text-frame table, the content caches, the line-layout side tables, form-control data and the table
/// anonymous-box style resolver. Process-wide caches stay (fonts, font metrics, atoms, the line
/// segmenter), as they do in Gecko. Afterwards `ConstructFrames` can build the tree again.
pub fn DestroyFrameTree(aLayout: PageLayout) {
    let mut context = FrameDestroyContext { mDestroyedFrames: Vec::new() };
    aLayout.mViewport.Destroy(&mut context);
    drop(aLayout);
    ResetFrameTree();
    nsTextFrame::ResetTextFrameTable();
    nsIContent::ResetContentContext();
    crate::nsLineLayoutAdapter::ResetLineLayoutStubs();
    ResetFormControlData();
    SetTableAnonBoxStyleResolver(None);
    nsPresContext::ResetBidiEnabled();
}

/// ConstructFrames and ReflowFrames.
pub fn LayoutDocument(aStyleDom: &StyleDom, aWidthPx: i32, aHeightPx: i32) -> PageLayout {
    let mut layout = ConstructFrames(aStyleDom, aWidthPx, aHeightPx);
    ReflowFrames(&mut layout);
    layout
}

/// How many frames of each type the frame tree of this thread holds. Text frames that are
/// continuations (a text node wrapped over several lines) are counted as "TextContinuation".
pub fn FrameCountsByType() -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    WithFrameTree(|t| {
        for f in &t.mFrames {
            // (a freed slot has no style)
            if f.mStyle.is_none() {
                continue;
            }
            let name = match f.mType {
                LayoutFrameType::Text if f.mPrevContinuation.is_some() => "TextContinuation".to_string(),
                ty => format!("{:?}", ty),
            };
            *counts.entry(name).or_insert(0) += 1;
        }
    });
    counts
}

/// The frame tree of this thread as text, one frame per line (type, rect in CSS px, state), down to
/// `aMaxDepth` levels, for debugging.
pub fn DumpFrameTree(aRoot: nsIFrame, aMaxDepth: usize) -> String {
    use std::fmt::Write;
    fn px(v: nscoord) -> f64 {
        v as f64 / AppUnitsPerCSSPixel() as f64
    }
    fn dump(f: nsIFrame, depth: usize, max: usize, out: &mut String) {
        let r = f.GetRect();
        let name = f.GetContent().and_then(|c| c.mHTMLLocalName()).unwrap_or_default();
        let text = f.GetContent().filter(|c| c.IsText()).map(|c| {
            let mut s = Vec::new();
            if let Some(b) = &c.mCharacterDataBuffer() {
                b.AppendTo__nsAString_uint32_t_uint32_t(&mut s, 0, b.GetLength().min(30));
            }
            format!(" {:?}", String::from_utf16_lossy(&s))
        });
        let _ = writeln!(out, "{}{:?} {} [{} {} {} {}]{}", "  ".repeat(depth), f.Type(), name, px(r.x), px(r.y), px(r.width), px(r.height), text.unwrap_or_default());
        if depth >= max {
            return;
        }
        let mut lists = Vec::new();
        f.GetChildLists(&mut lists);
        for l in lists {
            let mut c = l.mList.FirstChild();
            while let Some(k) = c {
                dump(k, depth + 1, max, out);
                c = k.GetNextSibling();
            }
        }
    }
    let mut out = String::new();
    dump(aRoot, 0, aMaxDepth, &mut out);
    out
}

// Gecko: dom/base/Element.cpp IsOffsetParent
fn IsOffsetParent(aFrame: nsIFrame) -> bool {
    let frameType = aFrame.Type();

    if frameType == LayoutFrameType::TableCell || frameType == LayoutFrameType::TableWrapper {
        // Per the IDL for Element, only td, th, and table are acceptable
        // offsetParents apart from body or positioned elements; we need to check
        // the content type as well as the frame type so we ignore anonymous tables
        // created by an element with display: table-cell with no actual table
        let content = aFrame.GetContent();

        return content.map_or(false, |c| c.IsAnyOfHTMLElements(&["table", "td", "th"] /* nsGkAtoms::table, td, th */));
    }
    false
}

// The element of a frame's content (None for content with no DOM node, such as an anonymous box's).
fn ElementOf(aContent: Option<nsIContent>) -> NodeId {
    aContent.map(|c| c.Id()).filter(|&id| id != 0 && id < kSyntheticContentBase).unwrap_or(NONE)
}

// Gecko: dom/base/Element.cpp GetUnretargetedOffsetsFor
// ADAPTER: (the offset parent or NONE, the rectangle in app units). Elements are node ids; the
// document's root and body elements are found in the arena (Document::GetRootElement,
// Document::GetBodyElement). GetPositionIgnoringScrolling is GetPosition (no scroll frames). No zoom.
fn GetUnretargetedOffsetsFor(aDom: &mrt_style::mrt_dom::Dom, aDoc: NodeId, aElement: NodeId) -> (NodeId, nsRect) {
    let Some(frame) = nsIContent::FromNodeId(aElement).GetPrimaryFrame() else {
        return (NONE, nsRect::default());
    };

    let styleFrame = nsLayoutUtils::GetStyleFrame(frame);

    let mut parent = frame.GetParent();
    let mut origin = nsPoint::new(0, 0);

    let mut offsetParent = NONE;
    let docElement = aDom.children(aDoc).find(|&n| aDom.node(n).kind == Kind::Element).unwrap_or(NONE);
    let content = ElementOf(frame.GetContent());

    if content != NONE && (nsIContent::FromNodeId(content).IsHTMLElement(nsGkAtoms::body) || content == docElement) {
        parent = Some(frame);
    } else {
        let isPositioned = styleFrame.IsAbsPosContainingBlock();
        let isAbsolutelyPositioned = frame.IsAbsolutelyPositioned(None);
        origin += frame.GetPosition();

        while let Some(p) = parent {
            let content = ElementOf(p.GetContent());

            // Stop at the first ancestor that is positioned.
            if p.IsAbsPosContainingBlock() {
                offsetParent = content;
                break;
            }

            // (NOT PORTED: the zoom check; there is no zoom)

            // Add the parent's origin to our own to get to the
            // right coordinate system.
            let isOffsetParent = !isPositioned && IsOffsetParent(p);
            if !isOffsetParent {
                origin += p.GetPosition();
            }

            if content != NONE {
                // If we've hit the document element, break here.
                if content == docElement {
                    break;
                }

                // Break if the ancestor frame type makes it suitable as offset parent
                // and this element is *not* positioned or if we found the body element.
                if isOffsetParent || nsIContent::FromNodeId(content).IsHTMLElement(nsGkAtoms::body) {
                    offsetParent = content;
                    break;
                }
            }
            parent = p.GetParent();
        }

        if isAbsolutelyPositioned && offsetParent == NONE && !frame.GetParent().map_or(false, |p| p.IsViewportFrame()) {
            // If this element is absolutely positioned, but we don't have
            // an offset parent it means this element is an absolutely
            // positioned child that's not nested inside another positioned
            // element, in this case the element's frame's parent is the
            // frame for the HTML element so we fail to find the body in the
            // parent chain. We want the offset parent in this case to be
            // the body, so we just get the body element from the document.
            //
            // We use GetBodyElement() here, not GetBody(), because we don't want to
            // end up with framesets here.
            offsetParent = if docElement != NONE && nsIContent::FromNodeId(docElement).IsHTMLElement(nsGkAtoms::html) {
                aDom.children(docElement)
                    .find(|&n| aDom.node(n).kind == Kind::Element && nsIContent::FromNodeId(n).IsHTMLElement(nsGkAtoms::body))
                    .unwrap_or(NONE)
            } else {
                NONE
            };
        }
    }

    // Make the position relative to the padding edge.
    if let Some(p) = parent {
        let border = p.StyleBorder();
        origin.x -= border.GetComputedBorderWidth(eSideLeft);
        origin.y -= border.GetComputedBorderWidth(eSideTop);
    }

    // Get the union of all rectangles in this and continuation frames.
    // It doesn't really matter what we use as aRelativeTo here, since
    // we only care about the size. We just have to use something non-null.
    let mut rcFrame = GetAllInFlowRectsUnion(frame, frame, false);
    rcFrame.x = origin.x;
    rcFrame.y = origin.y;
    (offsetParent, rcFrame)
}

// Gecko: dom/base/Element.cpp ShouldBeRetargeted
fn ShouldBeRetargeted(aDom: &mrt_style::mrt_dom::Dom, aReferenceElement: NodeId, aElementToMaybeRetarget: NodeId) -> bool {
    let shadow = aDom.containing_shadow(aElementToMaybeRetarget);
    if shadow == NONE {
        return false;
    }
    let mut scope = aDom.containing_shadow(aReferenceElement);
    while scope != NONE {
        if scope == shadow {
            return false;
        }
        scope = aDom.containing_shadow(aDom.host(scope));
    }

    true
}

// Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::AddBoxesForFrame
// ADAPTER: the callback is a closure; BoxCallback::mIncludeCaptionBoxForTable is true.
fn AddBoxesForFrame(aFrame: nsIFrame, aCallback: &mut dyn FnMut(nsIFrame)) {
    let pseudoType = aFrame.StylePseudoType();

    if pseudoType == PseudoStyleType::MozTableWrapper {
        for kid in aFrame.PrincipalChildList() {
            AddBoxesForFrame(kid, aCallback);
        }
    } else if pseudoType == PseudoStyleType::MozBlockInsideInlineWrapper {
        for kid in aFrame.PrincipalChildList() {
            AddBoxesForFrame(kid, aCallback);
        }
    } else {
        aCallback(aFrame);
    }
}

// Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::GetAllInFlowRectsUnion, with GetAllInFlowRects,
// GetAllInFlowBoxes, BoxToRect::AddBox (border boxes; `aAccountForTransforms` is the flag
// GetAllInFlowRectsFlag::AccountForTransforms) and RectAccumulator::AddRect.
// ADAPTER: no SVG outer frames.
pub fn GetAllInFlowRectsUnion(aFrame: nsIFrame, aRelativeTo: nsIFrame, aAccountForTransforms: bool) -> nsRect {
    let mut mResultRect = nsRect::default();
    let mut mFirstRect: Option<nsRect> = None;
    // If the frame we're measuring relative to is the root, we know all frames
    // are descendants of it, so we don't need to compute the common ancestor
    // between a frame and mRelativeTo.
    let mRelativeToIsRoot = aRelativeTo.GetParent().is_none();
    // For the same reason, if the frame we're measuring relative to is the target
    // (this is useful for IntersectionObserver), we know all frames are
    // descendants of it except if we're in a continuation or ib-split-sibling of
    // it.
    let mRelativeToIsTarget = aRelativeTo == aFrame;
    let mut mInTargetContinuation = false;
    let mut addBox = |f: nsIFrame, aInTargetContinuation: bool| {
        // Use the border-box.
        let mut r = nsRect::new(0, 0, f.GetRect().width, f.GetRect().height);
        if f != aRelativeTo {
            if aAccountForTransforms {
                let isAncestorKnown = mRelativeToIsRoot || (mRelativeToIsTarget && !aInTargetContinuation);
                if isAncestorKnown {
                    r = nsLayoutUtils::TransformFrameRectToAncestor(f, r, aRelativeTo);
                } else {
                    nsLayoutUtils::TransformRect(f, aRelativeTo, &mut r);
                }
            } else {
                let offset = f.GetOffsetTo(aRelativeTo);
                r.x += offset.x;
                r.y += offset.y;
            }
        }
        mResultRect.UnionRect(mResultRect, r);
        mFirstRect.get_or_insert(r);
    };
    let mut f = Some(aFrame);
    while let Some(frame) = f {
        AddBoxesForFrame(frame, &mut |b| addBox(b, mInTargetContinuation));
        f = nsLayoutUtils::GetNextContinuationOrIBSplitSibling(frame);
        mInTargetContinuation = true;
    }
    if mResultRect.IsEmpty() { mFirstRect.unwrap_or_default() } else { mResultRect }
}

/// What `getBoundingClientRect` gives of an element whose primary frame is `aFrame`, in app units
/// relative to the viewport: the union of the border boxes of its continuations (and of an inline's
/// split parts), each through the transforms of it and its ancestors.
///
/// Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetBoundingClientRect (the containing block for
/// client rects is the root frame, the viewport).
pub fn GetBoundingClientRect(aFrame: nsIFrame) -> nsRect {
    let mut root = aFrame;
    while let Some(p) = root.GetParent() {
        root = p;
    }
    GetAllInFlowRectsUnion(aFrame, root, true)
}

/// GetBoundingClientRect in CSS px as the DOMRect has it: (x, y, width, height).
///
/// Gecko: dom/base/DOMRect.cpp DOMRect::SetLayoutRect: the edges rounded to 1/65536 px, then
/// DOMRect::SetRect, whose parameters are floats.
pub fn BoundingClientRectInCSSPixels(aFrame: nsIFrame) -> (f64, f64, f64, f64) {
    let aLayoutRect = GetBoundingClientRect(aFrame);
    let scale = 65536.0;
    // Round to the nearest 1/scale units. We choose scale so it can be represented
    // exactly by machine floating point.
    let scaleInv = 1.0 / scale;
    let t2pScaled = scale / AppUnitsPerCSSPixel() as f64;
    // RoundFloat
    let round = |v: f64| (v + 0.5).floor();
    let x = round(aLayoutRect.x as f64 * t2pScaled) * scaleInv;
    let y = round(aLayoutRect.y as f64 * t2pScaled) * scaleInv;
    let float = |v: f64| v as f32 as f64;
    (float(x), float(y), float(round(aLayoutRect.XMost() as f64 * t2pScaled) * scaleInv - x), float(round(aLayoutRect.YMost() as f64 * t2pScaled) * scaleInv - y))
}

// ADAPTER: would Gecko have made this frame a ScrollContainerFrame? A block, flex or grid container
// whose `overflow` is not visible or clip, other than the root element and a body whose overflow goes
// to the viewport (nsCSSFrameConstructor::FindDisplayData's scroll frames; the port builds the plain
// container, whose padding box is the scroll port Gecko would have, nothing being scrolled).
pub(crate) fn IsScrollContainerStandIn(aFrame: nsIFrame) -> bool {
    let display = aFrame.StyleDisplay();
    if !display.IsScrollableOverflow() || display.IsInlineFlow() || aFrame.IsReplaced() {
        return false;
    }
    if !aFrame.IsBlockFrameOrSubclass() && !aFrame.IsFlexContainerFrame() && !aFrame.IsGridContainerFrame() {
        return false;
    }
    !ElementWouldPropagateScrollStyles(aFrame)
}

// Gecko: nsPresContext::ElementWouldPropagateScrollStyles: the root element, and the body when the
// root element's overflow is visible.
fn ElementWouldPropagateScrollStyles(aFrame: nsIFrame) -> bool {
    if aFrame.IsRootElementStyle() {
        return true;
    }
    if aFrame.GetContent().map_or(false, |c| c.IsHTMLElement(nsGkAtoms::body)) {
        if let Some(parent) = aFrame.GetParent() {
            if parent.IsRootElementStyle() && parent.StyleDisplay().mOverflowX() == StyleOverflow::Visible {
                return true;
            }
        }
    }
    false
}

// Gecko: layout/generic/nsIFrame.cpp nsIFrame::ShouldApplyOverflowClipping
// ADAPTER: what the port's own ShouldApplyOverflowClipping does not do (it clips nothing, as layout
// does not clip); used for IntersectionObserver only. The scroll containers Gecko would make are
// left out (IsScrollContainerStandIn), as Gecko's return nothing here. No printing.
pub(crate) fn OverflowClipAxes(aFrame: nsIFrame) -> PhysicalAxes {
    if IsScrollContainerStandIn(aFrame) {
        return PhysicalAxes::new();
    }
    let disp = aFrame.StyleDisplay();
    if disp.IsContainPaint() && aFrame.SupportsContainLayoutAndPaint() {
        return kPhysicalAxesBoth;
    }
    if disp.IsScrollableOverflow() {
        match aFrame.Type() {
            LayoutFrameType::CheckboxRadio
            | LayoutFrameType::ComboboxControl
            | LayoutFrameType::Progress
            | LayoutFrameType::Range
            | LayoutFrameType::SubDocument
            | LayoutFrameType::SVGForeignObject
            | LayoutFrameType::SVGOuterSVG
            | LayoutFrameType::Image
            | LayoutFrameType::HTMLVideo
            | LayoutFrameType::TableCell => return kPhysicalAxesBoth,
            LayoutFrameType::Table => {
                return if disp.mOverflowX() == StyleOverflow::Hidden && disp.mOverflowY() == StyleOverflow::Hidden {
                    kPhysicalAxesBoth
                } else {
                    PhysicalAxes::new()
                };
            }
            LayoutFrameType::TextInput => return PhysicalAxes::new(),
            _ => {}
        }
    }
    let mut axes = PhysicalAxes::new();
    if (disp.mOverflowX() == StyleOverflow::Clip || disp.mOverflowY() == StyleOverflow::Clip) && !ElementWouldPropagateScrollStyles(aFrame) {
        if disp.mOverflowX() == StyleOverflow::Clip {
            axes += PhysicalAxis::Horizontal;
        }
        if disp.mOverflowY() == StyleOverflow::Clip {
            axes += PhysicalAxis::Vertical;
        }
    }
    axes
}

// Gecko: layout/generic/nsIFrame.cpp nsIFrame::OverflowClipMargin (aAllowNegative false)
fn OverflowClipMarginOf(aFrame: nsIFrame, aClipAxes: PhysicalAxes) -> nsMargin {
    let mut result = nsMargin::default();
    if aClipAxes.isEmpty() {
        return result;
    }
    let style = aFrame.Style();
    let margin = &style.get_margin().overflow_clip_margin;
    let offset = margin.offset.0.px();
    if offset == 0.0 {
        return result;
    }
    use mrt_style::style::values::generics::box_::OverflowClipMarginBox;
    let negate = |m: nsMargin| nsMargin::new(-m.top, -m.right, -m.bottom, -m.left);
    match margin.visual_box {
        OverflowClipMarginBox::BorderBox => {}
        OverflowClipMarginBox::PaddingBox => result = negate(aFrame.GetUsedBorder()),
        OverflowClipMarginBox::ContentBox => result = negate(aFrame.GetUsedBorderAndPadding()),
    }
    let au = (offset * AppUnitsPerCSSPixel() as f32).round() as nscoord;
    result = nsMargin::new(result.top + au, result.right + au, result.bottom + au, result.left + au);
    result.EnsureAtLeast(nsMargin::default());
    result
}

// Gecko: nsLayoutUtils::IsAncestorFrameCrossDocInProcessConsideringContinuations, in one document
fn IsAncestorFrameConsideringContinuations(aAncestor: nsIFrame, aFrame: nsIFrame) -> bool {
    let ancestor = aAncestor.FirstContinuation();
    let mut f = Some(aFrame);
    while let Some(frame) = f {
        if frame.FirstContinuation() == ancestor {
            return true;
        }
        f = frame.GetParent();
    }
    false
}

// Gecko: dom/base/DOMIntersectionObserver.cpp ComputeTheIntersection, steps 1 to 4: the target's
// rectangle (`aTargetRectRelativeToTarget`, in `aTarget`'s coordinates), clipped by the scroll port of
// each scroll container and the overflow clip of each frame from the target's parent up to (not
// including) `aRoot`, each time mapped into the clipping frame's coordinates through the transforms
// between them, then mapped to `aViewport`'s (the containing block for client rects). None when a
// clip leaves nothing. Step 5, the root bounds, is the caller's.
// ADAPTER: the root scroll frame is the viewport (its scroll port is in the root bounds); the
// scroll containers Gecko would make are their stand-ins (IsScrollContainerStandIn), nothing scrolled
// in them. No popups, no remote documents, no clip-path (as Gecko).
fn ComputeTheIntersection(aTarget: nsIFrame, aTargetRectRelativeToTarget: nsRect, aRoot: nsIFrame, aViewport: nsIFrame, aScrollMargin: &IntersectionMargin) -> Option<nsRect> {
    let mut target = aTarget;
    // 1. Let intersectionRect be the result of running the
    // getBoundingClientRect() algorithm on the target.
    //
    // `intersectionRect` is kept relative to `target` during the loop.
    let mut intersectionRect = aTargetRectRelativeToTarget;

    // 2. Let container be the containing block of the target.
    // (We go through the parent chain and only look at scroll frames)
    //
    // 3. While container is not the intersection root:
    let mut containerFrame = aTarget.GetParent();
    while let Some(container) = containerFrame {
        if container == aRoot {
            break;
        }
        if IsScrollContainerStandIn(container) {
            let mut subFrameRect = container.GetPaddingRectRelativeToSelf();

            // 3.2 Map intersectionRect to the coordinate space of container.
            let intersectionRectRelativeToContainer = nsLayoutUtils::TransformFrameRectToAncestor(target, intersectionRect, container);

            // 3.3 If container is a scroll container, apply the
            // IntersectionObserver’s [[scrollMargin]] to the container’s clip rect as
            // described in apply scroll margin to a scrollport.
            subFrameRect.Inflate__nsMargin(ResolveIntersectionMargin(aScrollMargin, subFrameRect.Size()));

            intersectionRect = intersectionRectRelativeToContainer.EdgeInclusiveIntersection(subFrameRect)?;
            target = container;
        } else {
            let clipAxes = OverflowClipAxes(container);
            // 3.4 If container has a content clip or a css clip-path property, update
            // intersectionRect by applying container’s clip.

            // 3.4 TODO: Apply clip-path.
            if !clipAxes.isEmpty() {
                // 3.2 Map intersectionRect to the coordinate space of container.
                let intersectionRectRelativeToContainer = nsLayoutUtils::TransformFrameRectToAncestor(target, intersectionRect, container);
                let clipRect = OverflowAreas::GetOverflowClipRect(intersectionRectRelativeToContainer, container.GetRectRelativeToSelf(), clipAxes, OverflowClipMarginOf(container, clipAxes));
                intersectionRect = intersectionRectRelativeToContainer.EdgeInclusiveIntersection(clipRect)?;
                target = container;
            }
        }
        // 3.5 If container is the root element of a browsing context, update
        // container to be the browsing context’s document; otherwise, update
        // container to be the containing block of container.
        containerFrame = container.GetParent();
    }

    // 4. Map intersectionRect to the coordinate space of the intersection root.
    Some(nsLayoutUtils::TransformFrameRectToAncestor(target, intersectionRect, aViewport))
}

/// An IntersectionObserver's rootMargin or scrollMargin: top, right, bottom, left, each a length in
/// CSS px or (true) a percentage of the root's size.
pub type IntersectionMargin = [(f32, bool); 4];

// Gecko: dom/base/DOMIntersectionObserver.cpp ResolveMargin: lengths and percentages (of the height
// for top and bottom, of the width for the sides) rounded to app units (NSToCoordRoundWithClamp)
pub fn ResolveIntersectionMargin(aMargin: &IntersectionMargin, aPercentBasis: nsSize) -> nsMargin {
    let side = |i: usize, basis: nscoord| {
        let (v, percent) = aMargin[i];
        let au = if percent { v / 100.0 * basis as f32 } else { v * AppUnitsPerCSSPixel() as f32 };
        au.round() as nscoord
    };
    nsMargin::new(side(0, aPercentBasis.height), side(1, aPercentBasis.width), side(2, aPercentBasis.height), side(3, aPercentBasis.width))
}

/// The box of a frame relative to the document, in CSS px: (x, y, width, height). The offsets of the
/// ancestors are added up (relative positioning is not applied).
pub fn BoxInDocument(aFrame: nsIFrame) -> (f64, f64, f64, f64) {
    let (mut x, mut y) = (0, 0);
    let mut f = aFrame.GetParent();
    while let Some(p) = f {
        x += p.GetPosition().x;
        y += p.GetPosition().y;
        f = p.GetParent();
    }
    let r = aFrame.GetRect();
    let px = |v: nscoord| v as f64 / AppUnitsPerCSSPixel() as f64;
    (px(r.x + x), px(r.y + y), px(r.width), px(r.height))
}

// Gecko: layout/generic/ScrollContainerFrame.cpp ScrollContainerFrame::ComputePerAxisScrollDirections
// ADAPTER: (to the right, to the bottom). GetScrolledFrameDir's first-line rule for
// `unicode-bidi: plaintext` is not ported: the frame's own direction is used.
fn ComputePerAxisScrollDirections(aScrolledFrame: nsIFrame) -> (bool, bool) {
    let wm = aScrolledFrame.GetWritingMode();
    let mut scrollToRight = wm.IsPhysicalLTR();
    let mut scrollToBottom = !wm.IsVertical() || wm.GetInlineDir() == WritingMode_InlineDir::TTB;
    if aScrolledFrame.IsFlexContainerFrame() {
        let info = FlexboxAxisInfo::new(aScrolledFrame);
        let isMainAxisVertical = info.mIsRowOriented == wm.IsVertical();
        if info.mIsMainAxisReversed {
            if isMainAxisVertical {
                scrollToBottom = !scrollToBottom;
            } else {
                scrollToRight = !scrollToRight;
            }
        }
        if info.mIsCrossAxisReversed {
            if isMainAxisVertical {
                scrollToRight = !scrollToRight;
            } else {
                scrollToBottom = !scrollToBottom;
            }
        }
    }
    (scrollToRight, scrollToBottom)
}

// nsIFrame::UnionChildOverflow is virtual in Gecko: blocks and flex containers have their own.
fn UnionChildOverflow(aFrame: nsIFrame, aOverflowAreas: &mut OverflowAreas, aAsIfScrolled: bool) {
    if aFrame.IsBlockFrameOrSubclass() {
        nsBlockFrame::from_frame(aFrame).UnionChildOverflow(aOverflowAreas, aAsIfScrolled);
    } else if let Some(flex) = nsFlexContainerFrame::do_QueryFrame(aFrame) {
        flex.UnionChildOverflow(aOverflowAreas, aAsIfScrolled);
    } else {
        nsLayoutUtils::UnionChildOverflow(aFrame, aOverflowAreas, &[]);
    }
}

/// Adds to `aOverflow` the scrollable overflow `aFrame`'s children give the root scroll range, in the
/// coordinates where `aFrame` is at `aOrigin`: each child's border box and, unless the child stands
/// for a scroll container, what its own children stick out. ADAPTER: the stored overflow of a block
/// Gecko would make a scroll container holds its scrolled content (the port has no scroll container
/// frames), which Gecko's ScrollContainerFrame keeps to itself, so the frames are walked instead of
/// reading the canvas's overflow. A child whose stored overflow is within its box is not entered, and
/// a transformed one counts with its stored (transformed) overflow.
fn UnionScrolledOverflow(aFrame: nsIFrame, aOrigin: nsPoint, aOverflow: &mut nsRect) {
    let mut lists = Vec::new();
    aFrame.GetChildLists(&mut lists);
    aFrame.nsIFrame_GetChildLists(&mut lists);
    let mut seen = std::collections::HashSet::<nsIFrame>::new();
    for list in &lists {
        for child in &list.mList {
            if !seen.insert(child) || child.IsPlaceholderFrame() {
                continue;
            }
            let origin = aOrigin + child.GetPosition();
            let bounds = child.GetRect() - child.GetPosition() + origin;
            *aOverflow = aOverflow.UnionEdges(bounds);
            let stored = child.ScrollableOverflowRect() + origin;
            if bounds.Contains__nsRect(stored) || StandsForScrollContainer(child, aFrame) {
                continue;
            }
            if child.IsTransformed() {
                *aOverflow = aOverflow.UnionEdges(stored);
            } else {
                UnionScrolledOverflow(child, origin, aOverflow);
            }
        }
    }
}

/// Whether `aFrame` (a child of `aParent`) is a block Gecko would make a scroll container. The root
/// element's overflow, or else the body's, is the viewport's (Gecko's PropagateScrollToViewport,
/// not ported: those blocks scroll nothing).
fn StandsForScrollContainer(aFrame: nsIFrame, aParent: nsIFrame) -> bool {
    let isRoot = |f: nsIFrame| f.GetParent().is_some_and(|p| p.IsCanvasFrame() && p.PrincipalChildList_FirstChild() == Some(f));
    if !aFrame.StyleDisplay().IsScrollableOverflow() || !aFrame.IsBlockFrameOrSubclass() || isRoot(aFrame) {
        return false;
    }
    let isBody = aFrame.GetContent().is_some_and(|c| c.IsHTMLElement(nsGkAtoms::body));
    let rootVisible = isRoot(aParent) && !aParent.StyleDisplay().IsScrollableOverflow();
    !(isBody && rootVisible)
}

/// Moves every child frame of `aFrame`, in each of its child lists, by `aDelta` (app units).
fn ShiftChildFrames(aFrame: nsIFrame, aDelta: nsPoint) {
    let mut lists = Vec::new();
    aFrame.GetChildLists(&mut lists);
    aFrame.nsIFrame_GetChildLists(&mut lists);
    let mut seen = std::collections::HashSet::new();
    for list in &lists {
        for child in &list.mList {
            if seen.insert(child) {
                child.SetPosition__nsPoint(child.GetPosition() + aDelta);
            }
        }
    }
}

/// An element's client area and scroll size from its primary frame, in CSS px: (clientWidth,
/// clientHeight, scrollWidth, scrollHeight) before Gecko rounds them to integers.
///
/// Gecko: dom/base/Element.cpp Element::GetClientAreaRect and Element::GetScrollSize.
/// ADAPTER: the port has no scroll container frames: an element with `overflow` other than `visible`
/// is a block like any other. Its scroll port is then its padding box (no classic scrollbars, as in
/// the Firefox it is compared with); a block's scroll size is its scrolled content's overflow
/// (nsBlockFrame::ScrolledContentOverflow), and that of any other frame is computed as for an
/// element that is not a scroll container (GetScrollRectSizeForOverflowVisibleFrame): grid
/// containers miss the grid area and end padding Gecko adds in reflow (GAPS B39). The scrolling
/// element's client area is the caller's (the viewport); the root element's scroll size is the
/// root scroll range's content (see below), which the caller takes as the scrolling element's.
pub fn ClientAreaAndScrollSize(aFrame: nsIFrame) -> (f64, f64, f64, f64) {
    let px = |v: nscoord| v as f64 / AppUnitsPerCSSPixel() as f64;
    // The root element's frame, a child of the canvas: the canvas's scrollable overflow, which is what
    // Gecko's root scroll frame scrolls (ScrollContainerFrame::GetScrolledRect, from the origin): the
    // root's margin box, its descendants' overflow and the boxes absolutely positioned against the
    // initial containing block; fixed boxes are the viewport's (GAPS B215, task 9a0d7).
    // ADAPTER: in quirks mode Gecko's <html> keeps its own scroll size; here it has this one too.
    // (the canvas's principal child; its absolutely positioned children have it as parent too)
    if let Some(canvas) = aFrame.GetParent().filter(|p| p.IsCanvasFrame() && p.PrincipalChildList_FirstChild() == Some(aFrame)) {
        let mut overflow = canvas.GetRect() - canvas.GetPosition();
        UnionScrolledOverflow(canvas, nsPoint::new(0, 0), &mut overflow);
        let paddingRect = aFrame.GetPaddingRectRelativeToSelf();
        return (px(paddingRect.width), px(paddingRect.height), px(overflow.XMost().max(0)), px(overflow.YMost().max(0)));
    }
    let paddingRect = aFrame.GetPaddingRectRelativeToSelf();
    // GetClientAreaRect: the padding box, except for inline boxes that are not replaced
    let display = aFrame.StyleDisplay();
    let (clientWidth, clientHeight) =
        if !display.IsInlineFlow() || aFrame.IsReplaced() { (px(paddingRect.width), px(paddingRect.height)) } else { (0.0, 0.0) };
    // A block Gecko would make a scroll container: its scrolled content's overflow (GetScrollRange plus
    // the port, from the overflow reflow stored). Otherwise GetScrollRectSizeForOverflowVisibleFrame.
    let overflowRect = if display.IsScrollableOverflow() && aFrame.IsBlockFrameOrSubclass() {
        nsBlockFrame::from_frame(aFrame).ScrolledContentOverflow().UnionEdges(paddingRect)
    } else {
        let mut overflowAreas = OverflowAreas::new(paddingRect, paddingRect);
        UnionChildOverflow(aFrame, &mut overflowAreas, true);
        overflowAreas.ScrollableOverflow().UnionEdges(paddingRect)
    };
    let (toRight, toBottom) = ComputePerAxisScrollDirections(aFrame);
    let height = if toBottom { overflowRect.YMost() - paddingRect.y } else { paddingRect.YMost() - overflowRect.y };
    let width = if toRight { overflowRect.XMost() - paddingRect.x } else { paddingRect.XMost() - overflowRect.x };
    (clientWidth, clientHeight, px(width), px(height))
}

/// The id attribute of an element.
pub fn IdOf(aStyleDom: &StyleDom, aNode: NodeId) -> String {
    let dom = &aStyleDom.tree.dom;
    dom.attrs_of(aNode).iter().find(|a| dom.names.get(a.name) == "id").map(|a| dom.attr_value(a).to_string()).unwrap_or_default()
}

/// One element of the page for the per-element comparison with Firefox (`layoutcount --boxes`).
pub struct ElementBox {
    /// Position in document order among the elements (what `querySelectorAll('*')` gives in Firefox).
    pub mIndex: usize,
    /// Index of the parent element; None for the document element.
    pub mParentIndex: Option<usize>,
    pub mTag: String,
    /// The border box in document CSS px (x, y, width, height); None when the element has no frame.
    pub mBox: Option<(f64, f64, f64, f64)>,
}

/// An element that Firefox does not list in `querySelectorAll('*')`: a declarative shadow root
/// template (`<template shadowrootmode>`), which Firefox turns into a shadow root and the arena DOM keeps
/// as an element.
fn IsShadowRootTemplate(aStyleDom: &StyleDom, aNode: NodeId) -> bool {
    let dom = &aStyleDom.tree.dom;
    dom.tag(aNode) == "template" && dom.attrs_of(aNode).iter().any(|a| dom.names.get(a.name) == "shadowrootmode")
}

/// The border box of an element that has the primary frame `aPrimary`, in document CSS px: the union
/// of the frames of its continuations (what `getBoundingClientRect` gives), and for an inline that was
/// split around blocks, of the boxes of the children of the anonymous blocks in between (Gecko:
/// nsLayoutUtils::GetAllInFlowRects, AddBoxesForFrame).
pub fn BoxOfElementInDocument(aPrimary: nsIFrame) -> (f64, f64, f64, f64) {
    fn add_chain(aFirst: nsIFrame, aRects: &mut Vec<(f64, f64, f64, f64)>) {
        let mut f = Some(aFirst.FirstContinuation());
        while let Some(k) = f {
            aRects.push(BoxInDocument(k));
            f = k.GetNextContinuation();
        }
    }
    let mut rects = Vec::new();
    add_chain(aPrimary, &mut rects);
    // block-in-inline: inline, anonymous block, inline, ...
    let mut sib = aPrimary.FirstContinuation().GetProperty(nsIFrame::IBSplitSibling());
    while let Some(s) = sib {
        if s.IsBlockFrameOrSubclass() {
            // the anonymous block (the one in the middle of the split shares the inline's content)
            let mut lists = Vec::new();
            s.GetChildLists(&mut lists);
            for l in lists {
                let mut c = l.mList.FirstChild();
                while let Some(k) = c {
                    add_chain(k, &mut rects);
                    c = k.GetNextSibling();
                }
            }
        } else {
            add_chain(s, &mut rects);
        }
        sib = s.FirstContinuation().GetProperty(nsIFrame::IBSplitSibling());
    }
    // (nsRect::UnionRect leaves out an empty rect; when all are empty the first one stands)
    let first = rects[0];
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    if rects.iter().all(|r| r.2 <= 0.0 || r.3 <= 0.0) {
        return first;
    }
    for (x, y, w, h) in rects.into_iter().filter(|r| r.2 > 0.0 && r.3 > 0.0) {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x + w);
        y1 = y1.max(y + h);
    }
    (x0, y0, x1 - x0, y1 - y0)
}

/// Every element of the document in document order with its border box (None: no frame). The layout
/// must have been made with `ConstructFramesTrackingMode(.., aTrackAll = true)`.
pub fn ElementBoxes(aStyleDom: &StyleDom, aLayout: &PageLayout) -> Vec<ElementBox> {
    let dom = &aStyleDom.tree.dom;
    let mut frames: std::collections::HashMap<NodeId, nsIFrame> = aLayout.mFramesOfElementsWithId.iter().copied().collect();
    // (the constructor does not set the primary frame of the document element, see ConstructDocElementFrame)
    if let (Some(root), Some(frame)) = (DocumentElement(aStyleDom), aLayout.mRootElementFrame) {
        frames.entry(root).or_insert(frame);
    }
    let mut out: Vec<ElementBox> = Vec::new();
    // iterative pre-order walk: (node, index of the parent element)
    let mut stack: Vec<(NodeId, Option<usize>)> = dom.children(aStyleDom.tree.doc).collect::<Vec<_>>().into_iter().rev().map(|c| (c, None)).collect();
    while let Some((n, parent)) = stack.pop() {
        if dom.node(n).kind != Kind::Element || IsShadowRootTemplate(aStyleDom, n) {
            continue;
        }
        let index = out.len();
        out.push(ElementBox { mIndex: index, mParentIndex: parent, mTag: dom.tag(n).to_string(), mBox: frames.get(&n).map(|&f| BoxOfElementInDocument(f)) });
        let kids: Vec<NodeId> = dom.children(n).collect();
        for &c in kids.iter().rev() {
            stack.push((c, Some(index)));
        }
    }
    out
}

// Gecko: dom/base/nsRange.cpp InnerTextAccumulator (the string as UTF-16)
struct InnerTextAccumulator<'a> {
    mString: &'a mut Vec<u16>,
    mRequiredLineBreakCount: i8,
}

impl InnerTextAccumulator<'_> {
    fn FlushLineBreaks(&mut self) {
        while self.mRequiredLineBreakCount > 0 {
            // Required line breaks at the start of the text are suppressed.
            if !self.mString.is_empty() {
                self.mString.push(b'\n' as u16);
            }
            self.mRequiredLineBreakCount -= 1;
        }
    }
    fn Append(&mut self, aString: &[u16]) {
        if aString.is_empty() {
            return;
        }
        self.FlushLineBreaks();
        self.mString.extend_from_slice(aString);
    }
    fn AddRequiredLineBreakCount(&mut self, aCount: i8) {
        self.mRequiredLineBreakCount = std::cmp::max(self.mRequiredLineBreakCount, aCount);
    }
}

// Gecko: dom/base/nsRange.cpp IsVisibleAndNotInReplacedElement
// ADAPTER: NS_FRAME_IS_NONDISPLAY is never set (no SVG children) and HidesContent is false
// (`content-visibility` is ignored, GAPS B77).
fn IsVisibleAndNotInReplacedElement(aFrame: Option<nsIFrame>) -> bool {
    let Some(aFrame) = aFrame else { return false };
    if !aFrame.StyleVisibility().IsVisible() {
        return false;
    }
    let mut f = aFrame.GetParent();
    while let Some(p) = f {
        if p.IsReplaced() && !p.GetContent().is_some_and(|c| c.IsAnyOfHTMLElements(&[nsGkAtoms::button, nsGkAtoms::select]) || c.IsSVGElement()) {
            return false;
        }
        f = p.GetParent();
    }
    true
}

// Gecko: dom/base/nsRange.cpp AppendTransformedText
fn AppendTransformedText(aResult: &mut InnerTextAccumulator, aContainer: nsIContent) {
    let frame = aContainer.GetPrimaryFrame();
    if !IsVisibleAndNotInReplacedElement(frame) {
        return;
    }
    let text = nsTextFrame::from_frame(frame.unwrap()).GetRenderedText(0, aContainer.TextLength(), TextOffsetType::OffsetsInContentText, TrailingWhitespace::Trim);
    aResult.Append(&text.mString);
}

// Gecko: dom/base/nsRange.cpp TreeTraversalState
#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
enum TreeTraversalState {
    AT_NODE,
    AFTER_NODE,
}

// Gecko: dom/base/nsRange.cpp GetRequiredInnerTextLineBreakCount
fn GetRequiredInnerTextLineBreakCount(aFrame: nsIFrame) -> i8 {
    if aFrame.GetContent().is_some_and(|c| c.IsHTMLElement(nsGkAtoms::p)) {
        return 2;
    }
    let styleDisplay = aFrame.StyleDisplay();
    if styleDisplay.IsBlockOutside(aFrame) || styleDisplay.DisplayOutside() == StyleDisplayOutside::TableCaption {
        return 1;
    }
    0
}

// Gecko: dom/base/nsRange.cpp IsLastCellOfRow
fn IsLastCellOfRow(aFrame: nsIFrame) -> bool {
    if aFrame.Type() != LayoutFrameType::TableCell {
        return true;
    }
    let mut c = Some(aFrame);
    while let Some(f) = c {
        if f.GetNextSibling().is_some() {
            return false;
        }
        c = f.GetNextContinuation();
    }
    true
}

// Gecko: dom/base/nsRange.cpp IsLastRowOfRowGroup
fn IsLastRowOfRowGroup(aFrame: nsIFrame) -> bool {
    if !aFrame.IsTableRowFrame() {
        return true;
    }
    let mut c = Some(aFrame);
    while let Some(f) = c {
        if f.GetNextSibling().is_some() {
            return false;
        }
        c = f.GetNextContinuation();
    }
    true
}

// Gecko: dom/base/nsRange.cpp IsLastNonemptyRowGroupOfTable
fn IsLastNonemptyRowGroupOfTable(aFrame: nsIFrame) -> bool {
    if !aFrame.IsTableRowGroupFrame() {
        return true;
    }
    let mut c = Some(aFrame);
    while let Some(f) = c {
        let mut next = f.GetNextSibling();
        while let Some(n) = next {
            if n.IsTableRowGroupFrame() && n.PrincipalChildList_FirstChild().is_some() {
                return false;
            }
            next = n.GetNextSibling();
        }
        c = f.GetNextContinuation();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    // The in-place UTF-8 scan gives HasRTLChars's answer on the UTF-16 units for every code point.
    #[test]
    fn TextHasRTLCharsMatchesUTF16() {
        for cp in (0u32..=0x10FFFF).filter(|c| !(0xD800..=0xDFFF).contains(c)) {
            let c = char::from_u32(cp).unwrap();
            let text = format!("ab{c}cd");
            let units: Vec<u16> = text.encode_utf16().collect();
            assert_eq!(TextHasRTLChars(&text), HasRTLChars(&units), "U+{cp:04X}");
        }
        assert!(!TextHasRTLChars("plain ascii, é, 日本語"));
        assert!(TextHasRTLChars("日本語 and עברית"));
    }
}
