// Gecko: layout/painting/nsDisplayList.h and nsDisplayList.cpp, reduced to hit testing
//
// ADAPTER: what nsLayoutUtils::GetFramesForArea builds and walks for elementFromPoint and
// elementsFromPoint: a display list in nsDisplayListBuilderMode::EventDelivery, then
// nsDisplayList::HitTest. Only that mode is here, so only the items event delivery makes:
// nsDisplayEventReceiver (nsIFrame::DisplayBackgroundUnconditional), the table backgrounds
// (nsDisplayTableBackgroundColor, which hit as their cell), nsDisplayText, and nsDisplayWrapList for
// stacking contexts, positioned boxes and floats, and nsDisplayTransform (task 33620). Painting items (borders, outlines, shadows, the
// canvas background color) hit nothing and are left out. The display list building of each frame
// class (nsIFrame::BuildDisplayListForChild and BuildDisplayListForStackingContext, and the
// BuildDisplayList methods of the frames the port has) is gathered here, each marked with its Gecko
// file, since those files are translated for reflow only.
//
// Coordinates are app units relative to the item's reference frame: the viewport, or the nearest
// transformed ancestor frame (each transformed frame is a reference frame, as in Gecko).
// NOT PORTED: 3D contexts (preserve-3d; every transformed frame is flattened, backface-visibility
// is not read); the `clip` property; clip-path and masks as clips;
// SVG children of an outer <svg> (no SVG layout); the top layer (dialogs, fullscreen, popovers);
// view transitions; fragmentation (continuations of blocks and their overflow containers); the
// `inert` attribute (-moz-inert is a Gecko-only property); the dynamic-toolbar and display-port cases
// of DescendIntoChild; hit testing for visibility (OnlyVisible, opacity thresholds).
use crate::*;
use std::collections::{HashMap, HashSet};

// Gecko: layout/painting/DisplayItemClip.h DisplayItemClip: a clip rectangle and rounded rectangles,
// all of which an item is drawn inside.
#[derive(Clone, Default, Debug)]
pub struct DisplayItemClip {
    mClipRect: Option<nsRect>,
    mRoundedClipRects: Vec<(nsRect, nsRectCornerRadii)>,
}

impl DisplayItemClip {
    // Gecko: layout/painting/DisplayListClipState.cpp ClipContainingBlockDescendants (a clip
    // intersected with the current one; the rounded rectangle kept when there are radii)
    fn IntersectWith(&mut self, aRect: nsRect, aRadii: Option<nsRectCornerRadii>) {
        self.mClipRect = Some(match self.mClipRect {
            Some(r) => r.Intersect(aRect),
            None => aRect,
        });
        if let Some(radii) = aRadii {
            self.mRoundedClipRects.push((aRect, radii));
        }
    }

    // Gecko: layout/painting/DisplayItemClip.cpp DisplayItemClip::MayIntersect
    fn MayIntersect(&self, aRect: nsRect) -> bool {
        let Some(clipRect) = self.mClipRect else {
            return !aRect.IsEmpty();
        };
        let r = aRect.Intersect(clipRect);
        if r.IsEmpty() {
            return false;
        }
        for (rect, radii) in &self.mRoundedClipRects {
            if !nsLayoutUtils::RoundedRectIntersectsRect(*rect, radii, r) {
                return false;
            }
        }
        true
    }
}

// Gecko: layout/painting/nsDisplayList.h nsDisplayItem and the subclasses event delivery makes
#[derive(Debug)]
pub enum nsDisplayItem {
    // nsDisplayEventReceiver: the frame's border box (with its rounded corners)
    EventReceiver { mFrame: nsIFrame, mToReferenceFrame: nsPoint, mClip: DisplayItemClip },
    // nsDisplayBackgroundColor and nsDisplayTableBackgroundColor (a row's, row group's, column's or
    // column group's background drawn in a cell, whose frame is the cell): bounded by
    // mBackgroundRect, hit as mFrame's border box
    BackgroundColor { mFrame: nsIFrame, mToReferenceFrame: nsPoint, mBackgroundRect: nsRect, mClip: DisplayItemClip },
    // nsDisplayText: bounded by the frame's ink overflow, hit as its rectangle
    Text { mFrame: nsIFrame, mToReferenceFrame: nsPoint, mBounds: nsRect, mClip: DisplayItemClip },
    // ADAPTER: the root ScrollContainerFrame's nsDisplayEventReceiver (ViewportFrame_BuildDisplayList):
    // its frame's rectangle is the scroll port
    RootScrollFrameEventReceiver { mFrame: nsIFrame, mScrollPort: nsRect, mClip: DisplayItemClip },
    // nsDisplayWrapList: the items of a stacking context, a positioned box or a float, with the
    // frame's ZIndex
    WrapList { mFrame: nsIFrame, mList: nsDisplayList },
    // nsDisplayTransform: the items of a transformed frame, relative to the frame (a reference
    // frame); mMatrix maps them to the outer reference frame (GetTransform: the frame's transform,
    // then its offset to the outer reference frame), in device pixels. mClip is the clip of the
    // ancestors, in the outer reference frame's coordinates.
    Transform { mFrame: nsIFrame, mList: nsDisplayList, mMatrix: Matrix4x4, mClip: DisplayItemClip },
}

// Gecko: layout/painting/nsDisplayList.h nsDisplayList
pub type nsDisplayList = Vec<nsDisplayItem>;

impl nsDisplayItem {
    // Gecko: layout/painting/nsDisplayList.cpp nsDisplayItem::ZIndex
    fn ZIndex(&self) -> i32 {
        let frame = match self {
            nsDisplayItem::EventReceiver { mFrame, .. }
            | nsDisplayItem::BackgroundColor { mFrame, .. }
            | nsDisplayItem::Text { mFrame, .. }
            | nsDisplayItem::RootScrollFrameEventReceiver { mFrame, .. }
            | nsDisplayItem::WrapList { mFrame, .. }
            | nsDisplayItem::Transform { mFrame, .. } => *mFrame,
        };
        ZIndex(frame).unwrap_or(0)
    }
}

// Gecko: layout/painting/nsDisplayList.h nsDisplayListCollection: the six lists of CSS 2.1
// appendix E, in the order of nsDisplayListSet's members.
#[derive(Default)]
pub struct nsDisplayListCollection {
    mLists: [nsDisplayList; 6],
}

const kBorderBackground: usize = 0;
const kBlockBorderBackgrounds: usize = 1;
const kFloats: usize = 2;
const kContent: usize = 3;
const kPositionedDescendants: usize = 4;
const kOutlines: usize = 5;

// Gecko: layout/painting/nsDisplayList.h nsDisplayListSet: six lists, some of which may be the same
// list (a child's BorderBackground() is its parent's BlockBorderBackgrounds() or Content()).
// ADAPTER: raw pointers, as Gecko's nsDisplayList* members; the lists live in collections on the
// stack of the build, which outlive every set made from them.
#[derive(Clone, Copy)]
pub struct nsDisplayListSet {
    mLists: [*mut nsDisplayList; 6],
}

impl nsDisplayListSet {
    // Gecko: nsDisplayListSet(nsDisplayListCollection&) (the collection is a set of its own lists)
    fn new(aCollection: &mut nsDisplayListCollection) -> nsDisplayListSet {
        let p = aCollection.mLists.as_mut_ptr();
        nsDisplayListSet { mLists: std::array::from_fn(|i| unsafe { p.add(i) }) }
    }
    // Gecko: nsDisplayListSet(const nsDisplayListSet& aLists, nsDisplayList* aBorderBackground)
    fn WithBorderBackground(aLists: nsDisplayListSet, aBorderBackground: *mut nsDisplayList) -> nsDisplayListSet {
        let mut set = aLists;
        set.mLists[kBorderBackground] = aBorderBackground;
        set
    }
    fn list(&self, aIndex: usize) -> &mut nsDisplayList {
        // SAFETY: see the struct
        unsafe { &mut *self.mLists[aIndex] }
    }
    fn BorderBackground(&self) -> &mut nsDisplayList {
        self.list(kBorderBackground)
    }
    fn BlockBorderBackgrounds(&self) -> &mut nsDisplayList {
        self.list(kBlockBorderBackgrounds)
    }
    fn Floats(&self) -> &mut nsDisplayList {
        self.list(kFloats)
    }
    fn Content(&self) -> &mut nsDisplayList {
        self.list(kContent)
    }
    fn PositionedDescendants(&self) -> &mut nsDisplayList {
        self.list(kPositionedDescendants)
    }
    fn Outlines(&self) -> &mut nsDisplayList {
        self.list(kOutlines)
    }
}

impl nsDisplayListCollection {
    // Gecko: layout/painting/nsDisplayList.cpp nsDisplayListSet::MoveTo
    fn MoveTo(&mut self, aDestination: nsDisplayListSet) {
        for i in 0..6 {
            aDestination.list(i).append(&mut self.mLists[i]);
        }
    }

    // Gecko: layout/painting/nsDisplayList.cpp nsDisplayListCollection::SerializeWithCorrectZOrder
    // (the outlines are not sorted by content order: event delivery makes none)
    fn SerializeWithCorrectZOrder(&mut self, aOutResultList: &mut nsDisplayList) {
        // Sort PositionedDescendants() in CSS 'z-order' order.  The list is already
        // in content document order and SortByZOrder is a stable sort which
        // guarantees that boxes produced by the same element are placed together
        // in the sort.
        self.mLists[kPositionedDescendants].sort_by_key(|item| item.ZIndex());

        // Now follow the rules of http://www.w3.org/TR/CSS21/zindex.html
        // 1,2: backgrounds and borders
        aOutResultList.append(&mut self.mLists[kBorderBackground]);
        // 3: negative z-index children.
        let positioned = std::mem::take(&mut self.mLists[kPositionedDescendants]);
        for item in positioned {
            if item.ZIndex() < 0 {
                aOutResultList.push(item);
            } else {
                self.mLists[kPositionedDescendants].push(item);
            }
        }
        // 4: block backgrounds
        aOutResultList.append(&mut self.mLists[kBlockBorderBackgrounds]);
        // 5: floats
        aOutResultList.append(&mut self.mLists[kFloats]);
        // 7: general content
        aOutResultList.append(&mut self.mLists[kContent]);
        // 7.5: outlines
        aOutResultList.append(&mut self.mLists[kOutlines]);
        // 8, 9: non-negative z-index children
        aOutResultList.append(&mut self.mLists[kPositionedDescendants]);
    }
}

// Gecko: layout/generic/nsIFrame.h nsIFrame::DisplayChildFlag
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct DisplayChildFlags {
    mInline: bool,
    mForceStackingContext: bool,
    mForcePseudoStackingContext: bool,
}

// Gecko: layout/painting/nsDisplayList.h nsDisplayListBuilder, in EventDelivery mode
// ADAPTER: the builder's state is what hit testing reads: the area (aRect), whether the dirty rect is
// that area or empty (an out-of-flow frame not marked for display), the current frame's offset to
// the reference frame (ToReferenceFrame; the viewport, or the transformed frame being built), the clip
// of the items made now (DisplayListClipState), the clip
// each marked out-of-flow frame starts from (OutOfFlowDisplayData::mContainingBlockClipChain) and the
// frames marked NS_FRAME_FORCE_DISPLAY_LIST_DESCEND_INTO (a set here: the frames' state bits stay
// untouched).
pub struct nsDisplayListBuilder {
    mRect: nsRect,
    mViewport: nsIFrame,
    mScrollPort: nsRect,
    mDirtyIsArea: bool,
    mCurrentFrame: nsIFrame,
    mCurrentOffset: nsPoint,
    mCurrentReferenceFrame: nsIFrame,
    mClip: DisplayItemClip,
    mOutOfFlowData: HashMap<nsIFrame, DisplayItemClip>,
    mForceDescend: HashSet<nsIFrame>,
}

impl nsDisplayListBuilder {
    // Gecko: nsDisplayListBuilder::ToReferenceFrame, for the current frame or a child of it
    fn ToReferenceFrame(&self, aFrame: nsIFrame) -> nsPoint {
        if aFrame == self.mCurrentFrame {
            return self.mCurrentOffset;
        }
        if aFrame.GetParent() == Some(self.mCurrentFrame) {
            return self.mCurrentOffset + aFrame.GetPosition();
        }
        aFrame.GetOffsetTo(self.mCurrentReferenceFrame)
    }

    // Gecko: nsDisplayListBuilder::GetDirtyRect, relative to the viewport
    fn DirtyRect(&self) -> nsRect {
        if self.mDirtyIsArea { self.mRect } else { nsRect::default() }
    }

    // Gecko: layout/painting/nsDisplayList.cpp nsDisplayListBuilder::MarkFramesForDisplayList and
    // MarkOutOfFlowFrameForDisplay: the out-of-flow frames of `aDirtyFrame` (the current frame) whose
    // ink overflow meets the dirty rect keep the clip of now, and they and the frames from them to
    // `aDirtyFrame` through their placeholders are marked to be descended into.
    fn MarkFramesForDisplayList(&mut self, aDirtyFrame: nsIFrame, aFrames: &nsFrameList) {
        let dirty = self.DirtyRect();
        if dirty.IsEmpty() {
            return;
        }
        for e in aFrames {
            let offset = self.ToReferenceFrame(e);
            if !(e.InkOverflowRect() + offset).Intersects(dirty) {
                continue;
            }
            self.mOutOfFlowData.insert(e, self.mClip.clone());
            // MarkFrameForDisplay
            let mut f = Some(e);
            while let Some(frame) = f {
                if !self.mForceDescend.insert(frame) {
                    break;
                }
                if frame == aDirtyFrame {
                    break;
                }
                f = nsLayoutUtils::GetParentOrPlaceholderFor(frame);
            }
        }
    }
}

// Gecko: layout/generic/nsIFrame.cpp nsIFrame::ZIndexApplies
fn ZIndexApplies(aFrame: nsIFrame) -> bool {
    aFrame.StyleDisplay().IsPositionedStyle() || aFrame.IsFlexOrGridItem()
}

// Gecko: layout/generic/nsIFrame.cpp nsIFrame::ZIndex
fn ZIndex(aFrame: nsIFrame) -> Option<i32> {
    if !ZIndexApplies(aFrame) {
        return None;
    }
    let zIndex = aFrame.Style().get_position().z_index;
    if zIndex.is_auto() {
        return None;
    }
    Some(zIndex.integer_or(0))
}

// Gecko: layout/generic/nsIFrame.cpp nsIFrame::IsStackingContext
// ADAPTER: no opacity animations, no view transitions; UsingEffectsForFrame is filter, clip-path and
// mask-image.
pub fn IsStackingContext(aFrame: nsIFrame) -> bool {
    let disp = aFrame.StyleDisplay();
    let style = aFrame.Style();
    let effects = style.get_effects();
    let willChange = disp.mWillChangeBits();
    // HasOpacity
    if effects.opacity < 1.0 || willChange.contains(StyleWillChangeBits::OPACITY) {
        return true;
    }
    if aFrame.IsTransformed() {
        return true;
    }
    if disp.IsContainPaint() || disp.IsContainLayout() || willChange.contains(StyleWillChangeBits::CONTAIN) {
        if aFrame.SupportsContainLayoutAndPaint() {
            return true;
        }
    }
    // strictly speaking, 'perspective' doesn't require visual atomicity,
    // but the spec says it acts like the rest of these
    if disp.HasPerspectiveStyle() || willChange.contains(StyleWillChangeBits::PERSPECTIVE) {
        if aFrame.SupportsCSSTransforms() {
            return true;
        }
    }
    if !style.get_position().z_index.is_auto() || willChange.contains(StyleWillChangeBits::Z_INDEX) {
        if ZIndexApplies(aFrame) {
            return true;
        }
    }
    use mrt_style::style::computed_values::{isolation, mix_blend_mode};
    use mrt_style::style::values::generics::basic_shape::GenericClipPath;
    use mrt_style::style::values::generics::image::GenericImage;
    let svg = style.get_svg();
    let usingEffects = !effects.filter.0.is_empty()
        || !matches!(svg.clip_path, GenericClipPath::None)
        || svg.mask_image.0.iter().any(|image| !matches!(image, GenericImage::None));
    effects.mix_blend_mode != mix_blend_mode::T::Normal
        || usingEffects
        // IsPositionForcingStackingContext
        || matches!(disp.mPosition(), StylePositionProperty::Sticky | StylePositionProperty::Fixed)
        || style.get_box().isolation != isolation::T::Auto
        || willChange.contains(StyleWillChangeBits::STACKING_CONTEXT_UNCONDITIONAL)
}

// Gecko: layout/generic/nsIFrame.cpp nsIFrame::IsVisibleForPainting
fn IsVisibleForPainting(aFrame: nsIFrame) -> bool {
    aFrame.StyleVisibility().IsVisible()
}

// Gecko: layout/painting/nsDisplayList.cpp IsFrameReceivingPointerEvents (ComputedStyle::PointerEvents
// without -moz-inert)
fn IsFrameReceivingPointerEvents(aFrame: nsIFrame) -> bool {
    use mrt_style::style::computed_values::pointer_events;
    aFrame.Style().get_inherited_ui().pointer_events != pointer_events::T::None
}

// Gecko: layout/generic/nsIFrame.cpp nsIFrame::ComputeBorderRadii and GetBorderRadii(nsRectCornerRadii&)
// ADAPTER: no skip sides (the port's frames are not fragmented, but a wrapped inline's boxes keep
// every corner), no corner-shape (not in Stylo's Servo build), not themed.
fn GetBorderRadii(aFrame: nsIFrame) -> Option<nsRectCornerRadii> {
    let size = aFrame.GetSize();
    let style = aFrame.Style();
    let border = style.get_border();
    let corners = [&border.border_top_left_radius, &border.border_top_right_radius, &border.border_bottom_right_radius, &border.border_bottom_left_radius];
    let mut radii = nsRectCornerRadii::default();
    for (i, c) in corners.iter().enumerate() {
        // Percentages are relative to whichever side they're on.
        radii.radii[i] = nsSize::new(std::cmp::max(0, c.0.width.0.Resolve(size.width)), std::cmp::max(0, c.0.height.0.Resolve(size.height)));
    }
    // css3-background specifies this algorithm for reducing
    // corner radii when they are too big.
    let mut haveRadius = false;
    let mut ratio = 1.0f64;
    // (top: TL.x + TR.x of the width, right: TR.y + BR.y of the height, bottom, left)
    let sides = [(0, 1, true), (1, 2, false), (3, 2, true), (0, 3, false)];
    for (a, b, horizontal) in sides {
        let (length, sum) = if horizontal {
            (size.width, radii.radii[a].width + radii.radii[b].width)
        } else {
            (size.height, radii.radii[a].height + radii.radii[b].height)
        };
        if sum != 0 {
            haveRadius = true;
            // avoid floating point division in the normal case
            if length < sum {
                ratio = ratio.min(length as f64 / sum as f64);
            }
        }
    }
    if ratio < 1.0 {
        for r in radii.radii.iter_mut() {
            r.width = (r.width as f64 * ratio) as nscoord;
            r.height = (r.height as f64 * ratio) as nscoord;
        }
    }
    haveRadius.then_some(radii)
}

impl nsLayoutUtils {
    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::RoundedRectIntersectsRect
    pub fn RoundedRectIntersectsRect(aRoundedRect: nsRect, aRadii: &nsRectCornerRadii, aTestRect: nsRect) -> bool {
        if !aTestRect.Intersects(aRoundedRect) {
            return false;
        }

        // distances from this edge of aRoundedRect to opposite edge of aTestRect,
        // which we know are positive due to the Intersects check above.
        let top = aTestRect.YMost() - aRoundedRect.y;
        let right = aRoundedRect.XMost() - aTestRect.x;
        let bottom = aRoundedRect.YMost() - aTestRect.y;
        let left = aTestRect.XMost() - aRoundedRect.x;

        // Check whether the bottom-right corner of aTestRect is inside the
        // top left corner of aBounds when rounded by aRadii, etc.  If any
        // corner is not, then fail; otherwise succeed.
        let r = &aRadii.radii;
        CheckCorner(left, top, r[0].width, r[0].height)
            && CheckCorner(right, top, r[1].width, r[1].height)
            && CheckCorner(right, bottom, r[2].width, r[2].height)
            && CheckCorner(left, bottom, r[3].width, r[3].height)
    }
}

// Gecko: layout/base/nsLayoutUtils.cpp CheckCorner
fn CheckCorner(aXOffset: nscoord, aYOffset: nscoord, aXRadius: nscoord, aYRadius: nscoord) -> bool {
    // Avoid floating point math unless we're either (1) within the
    // quarter-ellipse area at the rounded corner or (2) outside the
    // rounding.
    if aXOffset >= aXRadius || aYOffset >= aYRadius {
        return true;
    }

    // Convert coordinates to a unit circle with (0,0) as the center of
    // curvature, and see if we're inside the circle or outside.
    let scaledX = (aXRadius - aXOffset) as f32 / aXRadius as f32;
    let scaledY = (aYRadius - aYOffset) as f32 / aYRadius as f32;
    scaledX * scaledX + scaledY * scaledY < 1.0
}

// Gecko: layout/painting/nsDisplayList.cpp RoundedBorderIntersectsRect
fn RoundedBorderIntersectsRect(aFrame: nsIFrame, aFrameToReferenceFrame: nsPoint, aTestRect: nsRect) -> bool {
    let rect = nsRect::new(aFrameToReferenceFrame.x, aFrameToReferenceFrame.y, aFrame.GetSize().width, aFrame.GetSize().height);
    if !rect.Intersects(aTestRect) {
        return false;
    }
    match GetBorderRadii(aFrame) {
        None => true,
        Some(radii) => nsLayoutUtils::RoundedRectIntersectsRect(rect, &radii, aTestRect),
    }
}

// Gecko: layout/generic/nsIFrame.cpp nsIFrame::OverflowClipMargin (aAllowNegative true)
fn OverflowClipMarginAllowingNegative(aFrame: nsIFrame) -> nsMargin {
    use mrt_style::style::values::generics::box_::OverflowClipMarginBox;
    let style = aFrame.Style();
    let margin = &style.get_margin().overflow_clip_margin;
    let mut result = match margin.visual_box {
        OverflowClipMarginBox::BorderBox => nsMargin::default(),
        OverflowClipMarginBox::PaddingBox => {
            let b = aFrame.GetUsedBorder();
            nsMargin::new(-b.top, -b.right, -b.bottom, -b.left)
        }
        OverflowClipMarginBox::ContentBox => {
            let b = aFrame.GetUsedBorderAndPadding();
            nsMargin::new(-b.top, -b.right, -b.bottom, -b.left)
        }
    };
    let offset = margin.offset.0.px();
    if offset != 0.0 {
        let au = (offset * AppUnitsPerCSSPixel() as f32).round() as nscoord;
        result = nsMargin::new(result.top + au, result.right + au, result.bottom + au, result.left + au);
    }
    result
}

// Gecko: layout/generic/nsIFrame.cpp ApplyOverflowClipping and
// nsIFrame::ComputeOverflowClipRectRelativeToSelf, and the scroll port that
// ScrollContainerFrame::BuildDisplayList clips its scrolled frame to: the clip a frame puts on its
// children (aFrame's offset to the viewport is `aOffset`), None for none.
// ADAPTER: the scroll containers Gecko would make (driver.rs IsScrollContainerStandIn) clip to their
// padding box with its rounded corners (the scroll port; nothing is scrolled, no scrollbars).
fn ClipForChildren(aFrame: nsIFrame, aOffset: nsPoint) -> Option<(nsRect, Option<nsRectCornerRadii>)> {
    if crate::driver::IsScrollContainerStandIn(aFrame) {
        let port = aFrame.GetPaddingRectRelativeToSelf() + aOffset;
        let radii = GetBorderRadii(aFrame).map(|mut r| {
            r.AdjustInwards(aFrame.GetUsedBorder());
            r
        });
        return Some((port, radii));
    }
    let axes = crate::driver::OverflowClipAxes(aFrame);
    if axes.isEmpty() {
        return None;
    }
    let mut boxMargin = OverflowClipMarginAllowingNegative(aFrame);
    // ApplySkipSides(ToSkipSides(aClipAxes))
    if !axes.contains(PhysicalAxis::Vertical) {
        boxMargin.top = 0;
        boxMargin.bottom = 0;
    }
    if !axes.contains(PhysicalAxis::Horizontal) {
        boxMargin.left = 0;
        boxMargin.right = 0;
    }
    let size = aFrame.GetSize();
    let mut rect = nsRect::new(0, 0, size.width, size.height);
    rect.Inflate__nsMargin(boxMargin);
    if !axes.contains(PhysicalAxis::Horizontal) {
        // NOTE(mats) We shouldn't be clipping at all in this dimension really,
        // but clipping in just one axis isn't supported by our GFX APIs so we
        // clip to our visual overflow rect instead.
        let o = aFrame.InkOverflowRect();
        rect.x = o.x;
        rect.width = o.width;
    }
    if !axes.contains(PhysicalAxis::Vertical) {
        // See the note above.
        let o = aFrame.InkOverflowRect();
        rect.y = o.y;
        rect.height = o.height;
    }
    let radii = GetBorderRadii(aFrame).map(|mut r| {
        r.AdjustOutwards(boxMargin);
        r
    });
    Some((rect + aOffset, radii))
}

// Gecko: layout/generic/nsIFrame.cpp DescendIntoChild
// ADAPTER: `aChildOffset` is the child's offset to the viewport; the table parts' normal-position
// rule is not needed (rows and row groups hit as their cells, laid out where they are).
fn DescendIntoChild(aBuilder: &nsDisplayListBuilder, aChild: nsIFrame, aChildOffset: nsPoint, aDirty: nsRect) -> bool {
    if aBuilder.mForceDescend.contains(&aChild) {
        return true;
    }
    let overflow = aChild.InkOverflowRect() + aChildOffset;
    aDirty.Intersects(overflow)
}

// Gecko: layout/generic/nsIFrame.cpp nsIFrame::DisplayBorderBackgroundOutline, for event delivery:
// DisplayBackgroundUnconditional's nsDisplayEventReceiver (borders, shadows and outlines hit nothing)
fn DisplayBorderBackgroundOutline(aBuilder: &nsDisplayListBuilder, aFrame: nsIFrame, aLists: nsDisplayListSet) {
    // The visibility check belongs here since child elements have the
    // opportunity to override the visibility property and display even if
    // their parent is hidden.
    if !IsVisibleForPainting(aFrame) {
        return;
    }
    aLists.BorderBackground().push(nsDisplayItem::EventReceiver { mFrame: aFrame, mToReferenceFrame: aBuilder.ToReferenceFrame(aFrame), mClip: aBuilder.mClip.clone() });
}

// Gecko: layout/generic/nsIFrame.cpp nsIFrame::MarkAbsoluteFramesForDisplayList
fn MarkAbsoluteFramesForDisplayList(aBuilder: &mut nsDisplayListBuilder, aFrame: nsIFrame) {
    if let Some(absCB) = aFrame.GetAbsoluteContainingBlock() {
        aBuilder.MarkFramesForDisplayList(aFrame, absCB.GetChildList());
    }
}

// Gecko: layout/generic/nsIFrame.cpp nsIFrame::BuildDisplayListForStackingContext
// (`aFrame` is the builder's current frame.) A transformed frame is built as a reference frame: the
// area is mapped into its coordinates (nsDisplayTransform::UntransformRect), and the ancestors' clip
// is cleared inside and kept on the nsDisplayTransform wrapping its items.
fn BuildDisplayListForStackingContext(aBuilder: &mut nsDisplayListBuilder, aFrame: nsIFrame, aList: &mut nsDisplayList) {
    let isTransformed = aFrame.IsTransformed();
    let saved = (aBuilder.mRect, aBuilder.mCurrentOffset, aBuilder.mCurrentReferenceFrame, aBuilder.mDirtyIsArea, aBuilder.mClip.clone());
    let mut matrix = Matrix4x4::default();
    if isTransformed {
        // nsDisplayTransform::GetTransform: the transform, then the offset to the outer reference frame
        let factor = AppUnitsPerCSSPixel() as f32;
        matrix = nsDisplayTransform::GetResultingTransformMatrix(aFrame, aBuilder.mCurrentOffset, factor, nsDisplayTransform::INCLUDE_PERSPECTIVE | nsDisplayTransform::OFFSET_BY_ORIGIN);
        match nsDisplayTransform::UntransformRect(aBuilder.mRect, &matrix, factor) {
            Some(r) => aBuilder.mRect = r,
            None => aBuilder.mDirtyIsArea = false,
        }
        aBuilder.mCurrentOffset = nsPoint::new(0, 0);
        aBuilder.mCurrentReferenceFrame = aFrame;
        // We don't need to pass ancestor clipping down to our children;
        // everything goes inside a display item's child list, and the display
        // item itself will be clipped.
        // For transforms we also need to clear ancestor clipping because it's
        // relative to the wrong display item reference frame anyway.
        aBuilder.mClip = DisplayItemClip::default();
    }
    let mut set = nsDisplayListCollection::default();
    MarkAbsoluteFramesForDisplayList(aBuilder, aFrame);
    BuildDisplayList(aBuilder, aFrame, nsDisplayListSet::new(&mut set));
    if !isTransformed {
        set.SerializeWithCorrectZOrder(aList);
        return;
    }
    let mut list = nsDisplayList::new();
    set.SerializeWithCorrectZOrder(&mut list);
    (aBuilder.mRect, aBuilder.mCurrentOffset, aBuilder.mCurrentReferenceFrame, aBuilder.mDirtyIsArea, aBuilder.mClip) = saved;
    aList.push(nsDisplayItem::Transform { mFrame: aFrame, mList: list, mMatrix: matrix, mClip: aBuilder.mClip.clone() });
}

// Gecko: layout/generic/nsIFrame.cpp nsIFrame::BuildDisplayListForChild
// (`aFrame` is the current frame, the builder's mCurrentFrame)
fn BuildDisplayListForChild(aBuilder: &mut nsDisplayListBuilder, aFrame: nsIFrame, aChild: nsIFrame, aLists: nsDisplayListSet, aFlags: DisplayChildFlags) {
    let placeholder = aChild.IsPlaceholderFrame();
    let childOrOutOfFlow = if placeholder {
        match nsPlaceholderFrame::from_frame(aChild).GetOutOfFlowFrame() {
            Some(f) => f,
            None => return,
        }
    } else {
        aChild
    };
    // ShouldSkipFrame: NS_FRAME_IS_NONDISPLAY and NS_FRAME_TOO_DEEP_IN_FRAME_TREE are never set here
    let Some(parent) = childOrOutOfFlow.GetParent() else { return };

    let mut child = aChild;
    let mut dirty = aBuilder.DirtyRect();
    let mut savedOutOfFlowData = None;
    if placeholder {
        child = childOrOutOfFlow;
        // If 'child' is a pushed out-of-flow then it's owned by a block that's not
        // an ancestor of the placeholder, and it will be painted by that block and
        // should not be painted through the placeholder.
        if child.HasAnyStateBits(NS_FRAME_IS_PUSHED_OUT_OF_FLOW) {
            return;
        }
        // NOT PORTED: the top layer (PLACEHOLDER_FOR_TOPLAYER)
        savedOutOfFlowData = aBuilder.mOutOfFlowData.get(&child).cloned();
        if savedOutOfFlowData.is_none() {
            // The out-of-flow frame did not intersect the dirty area. We may still
            // need to traverse into it, since it may contain placeholders we need
            // to enter to reach other out-of-flow frames that are visible.
            dirty = nsRect::default();
        }
    }

    let childOffset = aBuilder.ToReferenceFrame(child);
    if !DescendIntoChild(aBuilder, child, childOffset, dirty) {
        return;
    }

    // true if this is a real or pseudo stacking context
    let mut pseudoStackingContext = aFlags.mForcePseudoStackingContext;
    if !pseudoStackingContext && aFlags.mInline && !child.IsLineParticipant() {
        // child is a non-inline frame in an inline context, i.e.,
        // it acts like inline-block or inline-table. Therefore it is a
        // pseudo-stacking-context.
        pseudoStackingContext = true;
    }

    let disp = child.StyleDisplay();
    let isPositioned = disp.IsPositionedStyle();
    let isStackingContext = aFlags.mForceStackingContext || IsStackingContext(child);
    let isFloating = disp.IsFloating(child);
    if pseudoStackingContext || isStackingContext || isPositioned || placeholder || isFloating {
        pseudoStackingContext = true;
    }

    // AutoBuildingDisplayList and AutoClipMultiple: the builder's state for the child
    let saved = (aBuilder.mCurrentFrame, aBuilder.mCurrentOffset, aBuilder.mDirtyIsArea, aBuilder.mClip.clone());
    if let Some(clip) = savedOutOfFlowData {
        aBuilder.mClip = clip;
    }
    // Setup clipping for the parent's overflow:clip,
    // or overflow:hidden on elements that don't support scrolling (and therefore
    // don't create nsHTML/XULScrollFrame). This clipping needs to not clip
    // anything directly rendered by the parent, only the rendering of its
    // children.
    // (ADAPTER: and the scroll port of a scroll container Gecko would make, see ClipForChildren)
    let parentOffset = aBuilder.ToReferenceFrame(parent);
    if let Some((rect, radii)) = ClipForChildren(parent, parentOffset) {
        aBuilder.mClip.IntersectWith(rect, radii);
    }
    aBuilder.mCurrentFrame = child;
    aBuilder.mCurrentOffset = childOffset;
    aBuilder.mDirtyIsArea = !dirty.IsEmpty();

    let mut list = nsDisplayList::new();
    let mut extraPositionedDescendants = nsDisplayList::new();
    if isStackingContext {
        // True stacking context.
        BuildDisplayListForStackingContext(aBuilder, child, &mut list);
    } else {
        // NOT PORTED: the `clip` property (GetClipPropClipRect)
        MarkAbsoluteFramesForDisplayList(aBuilder, child);

        if !pseudoStackingContext {
            // THIS IS THE COMMON CASE.
            // Not a pseudo or real stacking context. Do the simple thing and
            // return early.
            BuildDisplayList(aBuilder, child, aLists);
            (aBuilder.mCurrentFrame, aBuilder.mCurrentOffset, aBuilder.mDirtyIsArea, aBuilder.mClip) = saved;
            return;
        }

        // A pseudo-stacking context (e.g., a positioned element with z-index auto).
        // We allow positioned descendants of the child to escape to our parent
        // stacking context's positioned descendant list, because they might be
        // z-index:non-auto
        let mut pseudoStack = nsDisplayListCollection::default();
        BuildDisplayList(aBuilder, child, nsDisplayListSet::new(&mut pseudoStack));
        for i in [kBorderBackground, kBlockBorderBackgrounds, kFloats, kContent, kOutlines] {
            list.append(&mut pseudoStack.mLists[i]);
        }
        extraPositionedDescendants.append(&mut pseudoStack.mLists[kPositionedDescendants]);
    }
    (aBuilder.mCurrentFrame, aBuilder.mCurrentOffset, aBuilder.mDirtyIsArea, aBuilder.mClip) = saved;

    if !list.is_empty() {
        if isPositioned || isStackingContext {
            // Genuine stacking contexts, and positioned pseudo-stacking-contexts,
            // go in this level.
            aLists.PositionedDescendants().push(nsDisplayItem::WrapList { mFrame: child, mList: list });
        } else if isFloating {
            aLists.Floats().push(nsDisplayItem::WrapList { mFrame: child, mList: list });
        } else {
            aLists.Content().append(&mut list);
        }
    }
    // We delay placing the positioned descendants of positioned frames to here,
    // because in the absence of z-index this is the correct order for them.
    aLists.PositionedDescendants().append(&mut extraPositionedDescendants);
}

// The frames' BuildDisplayList methods (virtual in Gecko: a match on the frame type here).
fn BuildDisplayList(aBuilder: &mut nsDisplayListBuilder, aFrame: nsIFrame, aLists: nsDisplayListSet) {
    use LayoutFrameType as T;
    if aFrame.IsBlockFrameOrSubclass() {
        return nsBlockFrame_BuildDisplayList(aBuilder, aFrame, aLists);
    }
    match aFrame.Type() {
        T::Viewport => ViewportFrame_BuildDisplayList(aBuilder, aFrame, aLists),
        T::Canvas => nsCanvasFrame_BuildDisplayList(aBuilder, aFrame, aLists),
        T::Text => nsTextFrame_BuildDisplayList(aBuilder, aFrame, aLists),
        // nsIFrame::BuildDisplayList: nothing (BRFrame, WBRFrame; nsPlaceholderFrame's out-of-flow is
        // displayed by BuildDisplayListForChild)
        T::Br | T::Wbr | T::Placeholder | T::None => {}
        // nsInlineFrame (nsFirstLineFrame), nsFirstLetterFrame and the ruby frames:
        // BuildDisplayListForInline
        T::Inline | T::Line | T::Letter | T::Ruby | T::RubyBase | T::RubyBaseContainer | T::RubyText | T::RubyTextContainer => {
            BuildDisplayListForInline(aBuilder, aFrame, aLists)
        }
        T::FlexContainer => nsFlexContainerFrame_BuildDisplayList(aBuilder, aFrame, aLists),
        T::GridContainer => nsGridContainerFrame_BuildDisplayList(aBuilder, aFrame, aLists),
        T::TableWrapper => nsTableWrapperFrame_BuildDisplayList(aBuilder, aFrame, aLists),
        T::Table => nsTableFrame_BuildDisplayList(aBuilder, aFrame, aLists),
        T::TableRowGroup => nsTableRowGroupFrame_BuildDisplayList(aBuilder, aFrame, aLists),
        T::TableRow => nsTableRowFrame_BuildDisplayList(aBuilder, aFrame, aLists),
        T::TableCell => nsTableCellFrame_BuildDisplayList(aBuilder, aFrame, aLists),
        // "All css properties of table-column and table-column-group boxes are ignored": the cells
        // draw their backgrounds (nsTableCellFrame_BuildDisplayList)
        T::TableCol | T::TableColGroup => {}
        // Frames whose children are native anonymous content (which QueryNodesFromRect answers as the
        // element) or not laid out here (SVG): the border and background.
        // NOT PORTED: the SVG children of an outer <svg> (SVG layout).
        T::SubDocument
        | T::Image
        | T::HTMLCanvas
        | T::HTMLVideo
        | T::CheckboxRadio
        | T::TextInput
        | T::ComboboxControl
        | T::ImageControl
        | T::Progress
        | T::Range
        | T::ColorControl
        | T::FileControl
        | T::DateTimeControl
        | T::SVGOuterSVG => DisplayBorderBackgroundOutline(aBuilder, aFrame, aLists),
        // nsContainerFrame::BuildDisplayList
        _ => {
            DisplayBorderBackgroundOutline(aBuilder, aFrame, aLists);
            BuildDisplayListForNonBlockChildren(aBuilder, aFrame, aLists, DisplayChildFlags::default());
        }
    }
}

// Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::BuildDisplayListForNonBlockChildren
fn BuildDisplayListForNonBlockChildren(aBuilder: &mut nsDisplayListBuilder, aFrame: nsIFrame, aLists: nsDisplayListSet, aFlags: DisplayChildFlags) {
    // Put each child's background directly onto the content list
    let set = nsDisplayListSet::WithBorderBackground(aLists, aLists.Content());
    // The children should be in content order
    for kid in aFrame.mFrames().as_static() {
        BuildDisplayListForChild(aBuilder, aFrame, kid, set, aFlags);
    }
}

// Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::BuildDisplayListForInline
fn BuildDisplayListForInline(aBuilder: &mut nsDisplayListBuilder, aFrame: nsIFrame, aLists: nsDisplayListSet) {
    DisplayBorderBackgroundOutline(aBuilder, aFrame, aLists);
    BuildDisplayListForNonBlockChildren(aBuilder, aFrame, aLists, DisplayChildFlags { mInline: true, ..Default::default() });
}

// Gecko: layout/generic/ViewportFrame.cpp ViewportFrame::BuildDisplayList
// ADAPTER: Gecko's viewport holds the root ScrollContainerFrame, whose content is the root element
// and which holds the canvas; the port's viewport holds the canvas. The scroll frame's part is done
// here: ScrollContainerFrame::BuildDisplayList's DisplayBorderBackgroundOutline (an event receiver
// over the scroll port; its style is the viewport's anonymous box, visible and receiving pointer
// events whatever the root element's are, so the viewport frame stands for it), then the canvas
// clipped to the scroll port. NOT PORTED: the top layer.
fn ViewportFrame_BuildDisplayList(aBuilder: &mut nsDisplayListBuilder, aFrame: nsIFrame, aLists: nsDisplayListSet) {
    let Some(kid) = aFrame.mFrames().as_static().FirstChild() else { return };
    let mut set = nsDisplayListCollection::default();
    let port = aBuilder.mScrollPort;
    let saved = aBuilder.mClip.clone();
    aBuilder.mClip.IntersectWith(port, None);
    set.mLists[kBorderBackground].push(nsDisplayItem::RootScrollFrameEventReceiver { mFrame: aFrame, mScrollPort: port, mClip: aBuilder.mClip.clone() });
    BuildDisplayListForChild(aBuilder, aFrame, kid, nsDisplayListSet::new(&mut set), DisplayChildFlags::default());
    aBuilder.mClip = saved;
    set.MoveTo(aLists);
}

// Gecko: layout/generic/nsCanvasFrame.cpp nsCanvasFrame::BuildDisplayList
// ADAPTER: the canvas background's items (a solid color, which hits nothing, and an image layer,
// which hits the canvas, answered as the root element as the root scroll frame below it is) are left
// out: they answer what the scroll frame's event receiver answers.
fn nsCanvasFrame_BuildDisplayList(aBuilder: &mut nsDisplayListBuilder, aFrame: nsIFrame, aLists: nsDisplayListSet) {
    for kid in aFrame.mFrames().as_static() {
        // Put our child into its own pseudo-stack.
        BuildDisplayListForChild(aBuilder, aFrame, kid, aLists, DisplayChildFlags::default());
    }
}

// Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::BuildDisplayList
// (TEXT_NO_RENDERED_GLYPHS and transparent text skip the item only when painting)
fn nsTextFrame_BuildDisplayList(aBuilder: &mut nsDisplayListBuilder, aFrame: nsIFrame, aLists: nsDisplayListSet) {
    if !IsVisibleForPainting(aFrame) {
        return;
    }
    let toReferenceFrame = aBuilder.ToReferenceFrame(aFrame);
    aLists.Content().push(nsDisplayItem::Text {
        mFrame: aFrame,
        mToReferenceFrame: toReferenceFrame,
        mBounds: aFrame.InkOverflowRect() + toReferenceFrame,
        mClip: aBuilder.mClip.clone(),
    });
}

// Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::BuildDisplayList and DisplayLine
// NOT PORTED: continuations (overflow containers, pushed floats), text-overflow, line-clamp, the
// forced-colors backplate; the line cursor (every line is tested, as without one).
fn nsBlockFrame_BuildDisplayList(aBuilder: &mut nsDisplayListBuilder, aFrame: nsIFrame, aLists: nsDisplayListSet) {
    DisplayBorderBackgroundOutline(aBuilder, aFrame, aLists);

    let block = nsBlockFrame::from_frame(aFrame);
    aBuilder.MarkFramesForDisplayList(aFrame, block.GetChildList(FrameChildListID::Float));

    if let Some(outsideMarker) = block.GetOutsideMarker() {
        // Display outside ::marker manually.
        BuildDisplayListForChild(aBuilder, aFrame, outsideMarker, aLists, DisplayChildFlags::default());
    }

    let offset = aBuilder.ToReferenceFrame(aFrame);
    let descendAlways = aBuilder.mForceDescend.contains(&aFrame);
    for line in block.Lines() {
        let lineArea = line.InkOverflowRect() + offset;
        // ShouldDescendIntoLine
        if !(descendAlways || lineArea.Intersects(aBuilder.DirtyRect())) {
            continue;
        }
        // DisplayLine: block-level child backgrounds go on the blockBorderBackgrounds list,
        // inline-level child backgrounds go on the regular child content list.
        let lineInLine = !line.IsBlock();
        let mut collection = nsDisplayListCollection::default();
        let collectionSet = nsDisplayListSet::new(&mut collection);
        let childLists = nsDisplayListSet::WithBorderBackground(
            collectionSet,
            if lineInLine { collectionSet.Content() } else { collectionSet.BlockBorderBackgrounds() },
        );
        let flags = DisplayChildFlags { mInline: lineInLine, ..Default::default() };
        for kid in line.ChildFrames() {
            BuildDisplayListForChild(aBuilder, aFrame, kid, childLists, flags);
        }
        collection.MoveTo(aLists);
    }
}

// Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::BuildDisplayList
fn nsFlexContainerFrame_BuildDisplayList(aBuilder: &mut nsDisplayListBuilder, aFrame: nsIFrame, aLists: nsDisplayListSet) {
    let mut tempLists = nsDisplayListCollection::default();
    let tempSet = nsDisplayListSet::new(&mut tempLists);
    DisplayBorderBackgroundOutline(aBuilder, aFrame, tempSet);

    // Our children are all block-level, so their borders/backgrounds all go on
    // the BlockBorderBackgrounds list.
    let childLists = nsDisplayListSet::WithBorderBackground(tempSet, tempSet.BlockBorderBackgrounds());
    let flex = nsFlexContainerFrame::from_frame(aFrame);
    let mut iter = CSSOrderAwareFrameIterator::new(
        aFrame,
        FrameChildListID::Principal,
        CSSOrderAwareFrameIterator_ChildFilter::IncludeAll,
        OrderStateForIter(flex),
        OrderingPropertyForIter(flex),
    );
    let flags = DisplayChildFlags { mForcePseudoStackingContext: true, ..Default::default() };
    while !iter.AtEnd() {
        BuildDisplayListForChild(aBuilder, aFrame, iter.get(), childLists, flags);
        iter.Next();
    }
    tempLists.MoveTo(aLists);
}

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::BuildDisplayList
fn nsGridContainerFrame_BuildDisplayList(aBuilder: &mut nsDisplayListBuilder, aFrame: nsIFrame, aLists: nsDisplayListSet) {
    DisplayBorderBackgroundOutline(aBuilder, aFrame, aLists);

    // Our children are all grid-level boxes, which behave the same as
    // inline-blocks in painting, so their borders/backgrounds all go on
    // the BlockBorderBackgrounds list.
    let order = if aFrame.HasAnyStateBits(NS_STATE_GRID_NORMAL_FLOW_CHILDREN_IN_CSS_ORDER) {
        CSSOrderAwareFrameIterator_OrderState::Ordered
    } else {
        CSSOrderAwareFrameIterator_OrderState::Unordered
    };
    let mut iter = CSSOrderAwareFrameIterator::new(
        aFrame,
        FrameChildListID::Principal,
        CSSOrderAwareFrameIterator_ChildFilter::IncludeAll,
        order,
        CSSOrderAwareFrameIterator_OrderingProperty::Order,
    );
    let flags = DisplayChildFlags { mForcePseudoStackingContext: true, ..Default::default() };
    while !iter.AtEnd() {
        BuildDisplayListForChild(aBuilder, aFrame, iter.get(), aLists, flags);
        iter.Next();
    }
}

// Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::BuildDisplayList
fn nsTableWrapperFrame_BuildDisplayList(aBuilder: &mut nsDisplayListBuilder, aFrame: nsIFrame, aLists: nsDisplayListSet) {
    // No border or background is painted because they belong to the inner table.
    let frames = aFrame.mFrames().as_static();
    let first = frames.FirstChild();
    if first.is_some() && first.and_then(|f| f.GetNextSibling()).is_none() {
        BuildDisplayListForChild(aBuilder, aFrame, first.unwrap(), aLists, DisplayChildFlags::default());
        return;
    }

    let mut set = nsDisplayListCollection::default();
    let setLists = nsDisplayListSet::new(&mut set);
    let captionSet = nsDisplayListSet::WithBorderBackground(setLists, setLists.BlockBorderBackgrounds());
    for frame in frames {
        let isTable = frame.IsTableFrame();
        BuildDisplayListForChild(aBuilder, aFrame, frame, if isTable { setLists } else { captionSet }, DisplayChildFlags::default());
        if !isTable {
            // FIXME(emilio, bug 144517): Historically we haven't displayed / laid
            // out multiple captions. This preserves that behavior.
            break;
        }
    }
    // Now we have to sort everything by content order, since the caption
    // may be somewhere inside the table.
    // NOT PORTED: SortByContentOrder of the floats, content, positioned and outline lists (a
    // caption's items come before or after the table's by the frames' order here).
    set.MoveTo(aLists);
}

// Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::BuildDisplayList
// (nsDisplayTableBackgroundSet: the columns' and column groups' backgrounds are drawn per cell by
// the cell, see nsTableCellFrame_BuildDisplayList; the table's border hits nothing)
fn nsTableFrame_BuildDisplayList(aBuilder: &mut nsDisplayListBuilder, aFrame: nsIFrame, aLists: nsDisplayListSet) {
    DisplayBorderBackgroundOutline(aBuilder, aFrame, aLists);

    let mut lists = nsDisplayListCollection::default();
    // This is similar to what
    // nsContainerFrame::BuildDisplayListForNonBlockChildren does, except that we
    // allow the children's background and borders to go in our BorderBackground
    // list.
    for kid in aFrame.mFrames().as_static() {
        BuildDisplayListForChild(aBuilder, aFrame, kid, nsDisplayListSet::new(&mut lists), DisplayChildFlags::default());
    }
    lists.MoveTo(aLists);
}

// Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::BuildDisplayList and DisplayRows
// (PaintCellBackgroundsForFrame's items are the cells': see nsTableCellFrame_BuildDisplayList)
fn nsTableRowGroupFrame_BuildDisplayList(aBuilder: &mut nsDisplayListBuilder, aFrame: nsIFrame, aLists: nsDisplayListSet) {
    for kid in aFrame.mFrames().as_static() {
        BuildDisplayListForChild(aBuilder, aFrame, kid, aLists, DisplayChildFlags::default());
    }
}

// Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::BuildDisplayList
fn nsTableRowFrame_BuildDisplayList(aBuilder: &mut nsDisplayListBuilder, aFrame: nsIFrame, aLists: nsDisplayListSet) {
    for kid in aFrame.mFrames().as_static() {
        BuildDisplayListForChild(aBuilder, aFrame, kid, aLists, DisplayChildFlags::default());
    }
}

// Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::BuildDisplayList and
// ShouldPaintBordersAndBackgrounds
// ADAPTER: for event delivery a cell's background and the backgrounds its row, row group, column and
// column group draw in it (PaintCellBackgroundsForFrame, the nsDisplayTableBackgroundSet) are all
// nsDisplayBackgroundColor items whose frame is the cell, bounded by the cell's rectangle: one item
// stands for them. They are made only when the cell paints its background.
fn nsTableCellFrame_BuildDisplayList(aBuilder: &mut nsDisplayListBuilder, aFrame: nsIFrame, aLists: nsDisplayListSet) {
    let cell = nsTableCellFrame::from_frame(aFrame);
    // If we're not visible, we don't paint. Consider 'empty-cells', but only in separated borders mode.
    let shouldPaint = IsVisibleForPainting(aFrame)
        && (!cell.GetContentEmpty()
            || cell.GetTableFrame().IsBorderCollapse()
            || aFrame.StyleTableBorder().mEmptyCells() == StyleEmptyCells::Show);
    if shouldPaint {
        let toReferenceFrame = aBuilder.ToReferenceFrame(aFrame);
        let size = aFrame.GetSize();
        aLists.BorderBackground().push(nsDisplayItem::BackgroundColor {
            mFrame: aFrame,
            mToReferenceFrame: toReferenceFrame,
            mBackgroundRect: nsRect::new(toReferenceFrame.x, toReferenceFrame.y, size.width, size.height),
            mClip: aBuilder.mClip.clone(),
        });
    }

    // The child's background will go in our BorderBackground() list.
    BuildDisplayListForChild(aBuilder, aFrame, cell.Inner(), aLists, DisplayChildFlags::default());
}

// Gecko: layout/painting/nsDisplayList.cpp nsDisplayList::HitTest and the items' HitTest
// (no 3D contexts; the pointer-events filter on each frame found)
fn HitTest(aList: &nsDisplayList, aRect: nsRect, aOutFrames: &mut Vec<nsIFrame>) {
    for item in aList.iter().rev() {
        let (frame, hit) = match item {
            nsDisplayItem::WrapList { mList, .. } => {
                // nsDisplayWrapList::HitTest (the wrapper's clip is its items')
                HitTest(mList, aRect, aOutFrames);
                continue;
            }
            nsDisplayItem::Transform { mFrame, mList, mMatrix, mClip } => {
                // (nsDisplayList::HitTest tests the item's clip first)
                if mClip.MayIntersect(aRect) {
                    nsDisplayTransform::HitTest(*mFrame, mList, mMatrix, aRect, aOutFrames);
                }
                continue;
            }
            nsDisplayItem::EventReceiver { mFrame, mToReferenceFrame, mClip } => {
                let bounds = nsRect::new(mToReferenceFrame.x, mToReferenceFrame.y, mFrame.GetSize().width, mFrame.GetSize().height);
                (*mFrame, mClip.MayIntersect(bounds.Intersect(aRect)) && RoundedBorderIntersectsRect(*mFrame, *mToReferenceFrame, aRect))
            }
            nsDisplayItem::RootScrollFrameEventReceiver { mFrame, mScrollPort, mClip } => {
                (*mFrame, mClip.MayIntersect(mScrollPort.Intersect(aRect)) && mScrollPort.Intersects(aRect))
            }
            nsDisplayItem::BackgroundColor { mFrame, mToReferenceFrame, mBackgroundRect, mClip } => {
                (*mFrame, mClip.MayIntersect(mBackgroundRect.Intersect(aRect)) && RoundedBorderIntersectsRect(*mFrame, *mToReferenceFrame, aRect))
            }
            nsDisplayItem::Text { mFrame, mToReferenceFrame, mBounds, mClip } => {
                let rect = nsRect::new(mToReferenceFrame.x, mToReferenceFrame.y, mFrame.GetSize().width, mFrame.GetSize().height);
                (*mFrame, mClip.MayIntersect(mBounds.Intersect(aRect)) && rect.Intersects(aRect))
            }
        };
        // Filter out some frames depending on the type of hittest
        // we are doing. For pointer tests, only pass through frames that are styled
        // to receive pointer events.
        if hit && IsFrameReceivingPointerEvents(frame) {
            aOutFrames.push(frame);
        }
    }
}

impl nsLayoutUtils {
    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::GetFramesForArea, with the options
    // QueryNodesFromRect gives (IgnorePaintSuppression, IgnoreCrossDoc: a frame element's document is
    // not entered): the frames under `aRect` (app units, relative to the viewport `aViewport` whose
    // scroll port is `aScrollPort`), the topmost first.
    pub fn GetFramesForArea(aViewport: nsIFrame, aScrollPort: nsRect, aRect: nsRect) -> Vec<nsIFrame> {
        let mut builder = nsDisplayListBuilder {
            mRect: aRect,
            mViewport: aViewport,
            mScrollPort: aScrollPort,
            mDirtyIsArea: true,
            mCurrentFrame: aViewport,
            mCurrentOffset: nsPoint::new(0, 0),
            mCurrentReferenceFrame: aViewport,
            mClip: DisplayItemClip::default(),
            mOutOfFlowData: HashMap::new(),
            mForceDescend: HashSet::new(),
        };
        let mut list = nsDisplayList::new();
        BuildDisplayListForStackingContext(&mut builder, aViewport, &mut list);
        let mut outFrames = Vec::new();
        HitTest(&list, aRect, &mut outFrames);
        outFrames
    }
}

// Gecko: layout/painting/nsDisplayList.h and nsDisplayList.cpp nsDisplayTransform: the static
// functions that compute a transformed frame's matrix, and its HitTest (task 33620).
// ADAPTER: one device pixel per CSS px (aAppUnitsPerPixel is AppUnitsPerCSSPixel); no SVG transforms
// (GetParentSVGTransforms); no 3D contexts (INCLUDE_PRESERVE3D_ANCESTORS is never given, see
// nsIFrame::Combines3DTransformWithAncestors); no snapping (ShouldSnapToGrid is for SVG).
pub struct nsDisplayTransform;

impl nsDisplayTransform {
    // Gecko: nsDisplayTransform's flags for GetResultingTransformMatrix
    pub const OFFSET_BY_ORIGIN: u32 = 1 << 0;
    pub const INCLUDE_PRESERVE3D_ANCESTORS: u32 = 1 << 1;
    pub const INCLUDE_PERSPECTIVE: u32 = 1 << 2;

    // Gecko: nsDisplayTransform::GetDeltaToTransformOrigin: the transform origin in the frame's
    // coordinates, in pixels of `aAppUnitsPerPixel`.
    pub fn GetDeltaToTransformOrigin(aFrame: nsIFrame, aRefBox: &TransformReferenceBox, aAppUnitsPerPixel: f32) -> (f32, f32, f32) {
        if !aFrame.IsTransformed() {
            return (0.0, 0.0, 0.0);
        }

        /* For both of the coordinates, if the value of transform is a
         * percentage, it's relative to the size of the frame.  Otherwise, if it's
         * a distance, it's already computed for us!
         */
        let style = aFrame.Style();
        let transformOrigin = &style.get_box().transform_origin;
        let (mut x, mut y) = nsStyleTransformMatrix::Convert2DPosition(&transformOrigin.horizontal, &transformOrigin.vertical, aRefBox);

        // Note:
        // 1. SVG frames have a reference box that can be (and typically is) offset
        //    from the TopLeft() of the frame. We need to account for that here.
        // 2. If we are using transform-box:content-box in CSS layout, we have the
        //    offset from TopLeft() of the frame as well.
        x += aRefBox.X() as f32 / AppUnitsPerCSSPixel() as f32;
        y += aRefBox.Y() as f32 / AppUnitsPerCSSPixel() as f32;

        let scale = AppUnitsPerCSSPixel() as f32 / aAppUnitsPerPixel;
        let z = transformOrigin.depth.px();
        (x * scale, y * scale, z * scale)
    }

    // Gecko: nsDisplayTransform::ComputePerspectiveMatrix: the perspective `aFrame`'s parent element
    // gives it, if any.
    // ADAPTER: the frame of the closest flattened-tree ancestor element
    // (GetClosestFlattenedTreeAncestorPrimaryFrame) is found up the frame tree: the first ancestor
    // frame that is the primary frame of an element other than `aFrame`'s.
    pub fn ComputePerspectiveMatrix(aFrame: nsIFrame, aAppUnitsPerPixel: f32, aOutMatrix: &mut Matrix4x4) -> bool {
        MOZ_ASSERT!(aOutMatrix.IsIdentity());

        if !aFrame.IsTransformed() {
            return false;
        }

        let content = aFrame.GetContent().map(|c| c.Id());
        let mut perspectiveFrame = aFrame.GetParent();
        while let Some(f) = perspectiveFrame {
            let c = f.GetContent();
            if c.is_some_and(|c| Some(c.Id()) != content && c.GetPrimaryFrame() == Some(f)) {
                break;
            }
            perspectiveFrame = f.GetParent();
        }
        let Some(perspectiveFrame) = perspectiveFrame else {
            return false;
        };

        /* Grab the values for perspective and perspective-origin (if present) */
        let style = perspectiveFrame.Style();
        let b = style.get_box();
        use mrt_style::style::values::generics::box_::Perspective;
        let Perspective::Length(length) = &b.perspective else {
            return false;
        };
        let perspective = length.px().max(1.0);
        if perspective < f32::EPSILON {
            return true;
        }

        let refBox = TransformReferenceBox::new(perspectiveFrame);

        let (ox, oy) = nsStyleTransformMatrix::Convert2DPosition(&b.perspective_origin.horizontal, &b.perspective_origin.vertical, &refBox);
        let scale = AppUnitsPerCSSPixel() as f32 / aAppUnitsPerPixel;
        let (mut ox, mut oy) = (ox * scale, oy * scale);

        /* GetOffsetTo computes the offset required to move from 0,0 in
         * perspectiveFrame to 0,0 in aFrame. Although we actually want the inverse of
         * this, it's faster to compute this way.
         */
        let frameToPerspectiveOffset = aFrame.GetOffsetTo(perspectiveFrame);

        /* Move the perspective origin to be relative to aFrame, instead of relative
         * to the containing block which is how it was specified in the style system.
         */
        ox -= NSAppUnitsToFloatPixels(frameToPerspectiveOffset.x, aAppUnitsPerPixel);
        oy -= NSAppUnitsToFloatPixels(frameToPerspectiveOffset.y, aAppUnitsPerPixel);

        aOutMatrix._34 = -1.0 / NSAppUnitsToFloatPixels(NSToCoordRound__float(perspective * AppUnitsPerCSSPixel() as f32), aAppUnitsPerPixel);

        aOutMatrix.ChangeBasis(ox, oy, 0.0);
        true
    }

    // Gecko: nsDisplayTransform::GetResultingTransformMatrix(const nsIFrame*, const nsPoint&, float,
    // uint32_t) and GetResultingTransformMatrixInternal: the frame's transform about its transform
    // origin, then (INCLUDE_PERSPECTIVE) its parent's perspective, then (OFFSET_BY_ORIGIN) a
    // translation by `aOrigin`, in pixels of `aAppUnitsPerPixel`.
    pub fn GetResultingTransformMatrix(aFrame: nsIFrame, aOrigin: nsPoint, aAppUnitsPerPixel: f32, aFlags: u32) -> Matrix4x4 {
        let refBox = TransformReferenceBox::new(aFrame);
        nsDisplayTransform::GetResultingTransformMatrixInternal(aFrame, &refBox, aOrigin, aAppUnitsPerPixel, aFlags)
    }

    // Gecko: nsDisplayTransform::GetResultingTransformMatrixInternal (FrameTransformProperties are
    // read from the frame)
    fn GetResultingTransformMatrixInternal(aFrame: nsIFrame, aRefBox: &TransformReferenceBox, aOrigin: nsPoint, aAppUnitsPerPixel: f32, aFlags: u32) -> Matrix4x4 {
        /* Get the matrix, then change its basis to factor in the origin. */
        // (FrameTransformProperties::HasTransform: a transformed frame with no transform functions
        // has the identity, as one with only `will-change: transform` or preserve-3d)
        let mut result = nsStyleTransformMatrix::ReadTransforms(aFrame, aRefBox);

        // Apply any translation due to 'transform-origin' and/or 'transform-box':
        let toTransformOrigin = nsDisplayTransform::GetDeltaToTransformOrigin(aFrame, aRefBox, aAppUnitsPerPixel);
        if toTransformOrigin != (0.0, 0.0, 0.0) {
            result.ChangeBasis(toTransformOrigin.0, toTransformOrigin.1, toTransformOrigin.2);
        }

        let hasPerspective = aFlags & nsDisplayTransform::INCLUDE_PERSPECTIVE != 0;
        if hasPerspective {
            let mut perspectiveMatrix = Matrix4x4::default();
            if nsDisplayTransform::ComputePerspectiveMatrix(aFrame, aAppUnitsPerPixel, &mut perspectiveMatrix) {
                result = result * perspectiveMatrix;
            }
        }

        if aFlags & nsDisplayTransform::OFFSET_BY_ORIGIN != 0 && aOrigin != nsPoint::new(0, 0) {
            // nsLayoutUtils::PostTranslate (not rounded)
            result.PostTranslate(NSAppUnitsToFloatPixels(aOrigin.x, aAppUnitsPerPixel), NSAppUnitsToFloatPixels(aOrigin.y, aAppUnitsPerPixel), 0.0);
        }

        result
    }

    // Gecko: nsDisplayTransform::TransformRect(const nsRect&, const nsIFrame*,
    // TransformReferenceBox&): `aUntransformedBounds`, in the frame's coordinates, as the bounds of
    // its image under the frame's transform (kTransformRectFlags without OFFSET_BY_ORIGIN:
    // INCLUDE_PERSPECTIVE).
    pub fn TransformRect(aUntransformedBounds: nsRect, aFrame: nsIFrame, aRefBox: &TransformReferenceBox) -> nsRect {
        let factor = AppUnitsPerCSSPixel() as f32;
        let matrix = nsDisplayTransform::GetResultingTransformMatrixInternal(aFrame, aRefBox, nsPoint::new(0, 0), factor, nsDisplayTransform::INCLUDE_PERSPECTIVE);
        nsLayoutUtils::MatrixTransformRect(aUntransformedBounds, &matrix, factor)
    }

    // Gecko: nsDisplayTransform::UntransformRect(const nsRect&, const nsRect&, const Matrix4x4&,
    // float, nsRect*): the bounds of the area of `aTransformedBounds` before `aMatrix`; None when the
    // matrix has no inverse.
    // ADAPTER: the child bounds the result is clamped to are the whole coordinate space.
    pub fn UntransformRect(aTransformedBounds: nsRect, aMatrix: &Matrix4x4, aAppUnitsPerPixel: f32) -> Option<nsRect> {
        let inverse = aMatrix.MaybeInverse()?;
        let factor = aAppUnitsPerPixel as f64;
        let result = gfxRectTyped::new(
            NSAppUnitsToDoublePixels(aTransformedBounds.x, factor),
            NSAppUnitsToDoublePixels(aTransformedBounds.y, factor),
            NSAppUnitsToDoublePixels(aTransformedBounds.width, factor),
            NSAppUnitsToDoublePixels(aTransformedBounds.height, factor),
        );
        let childGfxBounds = gfxRectTyped::new(nscoord_MIN as f64 / factor, nscoord_MIN as f64 / factor, nscoord_MAX as f64 / factor * 2.0, nscoord_MAX as f64 / factor * 2.0);
        let result = inverse.ProjectRectBounds(result, childGfxBounds);
        Some(nsLayoutUtils::RoundGfxRectToAppRect(result, aAppUnitsPerPixel))
    }

    // Gecko: nsDisplayTransform::HitTest: `aRect`, in the outer reference frame, mapped into the
    // frame's coordinates with the inverse of `aMatrix` (a point as a point, a rectangle as the
    // bounds of its preimage), then tested against the items.
    // NOT PORTED: backface-visibility (IsFrameVisible), 3D contexts, hit testing for visibility.
    fn HitTest(aFrame: nsIFrame, aList: &nsDisplayList, aMatrix: &Matrix4x4, aRect: nsRect, aOutFrames: &mut Vec<nsIFrame>) {
        /* Here's how this works:
         * 1. Get the matrix.  If it's singular, abort (clearly we didn't hit
         *    anything).
         * 2. Invert the matrix.
         * 3. Use it to transform the rect into the correct space.
         * 4. Pass that rect down through to the list's version of HitTest.
         */
        // GetTransform always operates in dev pixels.
        let factor = AppUnitsPerCSSPixel() as f32;

        /* We want to go from transformed-space to regular space.
         * Thus we have to invert the matrix, which normally does
         * the reverse operation (e.g. regular->transformed)
         */
        let Some(matrix) = aMatrix.MaybeInverse() else {
            return;
        };

        /* Now, apply the transform and pass it down the channel. */
        // Magic width/height indicating we're hit testing a point, not a rect
        let testingPoint = aRect.width == 1 && aRect.height == 1;
        let resultingRect = if testingPoint {
            let point = matrix.ProjectPoint((NSAppUnitsToFloatPixels(aRect.x, factor), NSAppUnitsToFloatPixels(aRect.y, factor)));
            if !point.HasPositiveWCoord() {
                return;
            }

            let point2d = point.As2DPoint();

            nsRect::new(NSFloatPixelsToAppUnits(point2d.0, factor), NSFloatPixelsToAppUnits(point2d.1, factor), 1, 1)
        } else {
            let originalRect = gfxRectTyped::new(
                NSAppUnitsToFloatPixels(aRect.x, factor),
                NSAppUnitsToFloatPixels(aRect.y, factor),
                NSAppUnitsToFloatPixels(aRect.width, factor),
                NSAppUnitsToFloatPixels(aRect.height, factor),
            );

            // (ADAPTER: the children's bounds are the whole coordinate space)
            let childGfxBounds = gfxRectTyped::new(nscoord_MIN as f32 / factor, nscoord_MIN as f32 / factor, nscoord_MAX as f32 / factor * 2.0, nscoord_MAX as f32 / factor * 2.0);

            let rect = matrix.ProjectRectBounds(originalRect, childGfxBounds);

            nsRect::new(
                NSFloatPixelsToAppUnits(rect.X(), factor),
                NSFloatPixelsToAppUnits(rect.Y(), factor),
                NSFloatPixelsToAppUnits(rect.width, factor),
                NSFloatPixelsToAppUnits(rect.height, factor),
            )
        };

        if resultingRect.IsEmpty() {
            return;
        }

        HitTest(aList, resultingRect, aOutFrames);
    }
}
