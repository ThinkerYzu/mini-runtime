// Translated from layout/generic/CSSAlignUtils.h and CSSAlignUtils.cpp (GECKO_REV), stage 2b.
//
// Rust shape (see CONVENTIONS.md): the nested enum CSSAlignUtils::AlignJustifyFlag is the top-level
// `CSSAlignUtils_AlignJustifyFlag` with the aliases `AlignJustifyFlag` and `AlignJustifyFlags` (the C++
// `using AlignJustifyFlags = EnumSet<AlignJustifyFlag>`), which is what the grid code uses. The nested struct
// AnchorAlignInfo is `CSSAlignUtils_AnchorAlignInfo` with the alias `AnchorAlignInfo`. `Maybe<AnchorAlignInfo>`
// is `Option<AnchorAlignInfo>`; the C++ default argument `Nothing()` is not available in Rust, so callers
// pass `None`.
use crate::*;

// Gecko: layout/generic/CSSAlignUtils.h mozilla::CSSAlignUtils
pub struct CSSAlignUtils;

/**
 * Flags to customize the behavior of AlignJustifySelf:
 */
// Gecko: layout/generic/CSSAlignUtils.h mozilla::CSSAlignUtils::AlignJustifyFlag
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum CSSAlignUtils_AlignJustifyFlag {
    // Indicates that we have <overflow-position> = safe.
    OverflowSafe,

    // Indicates that the container's start side in aAxis is the same
    // as the child's start side in the child's parallel axis.
    SameSide,

    // Indicates that AlignJustifySelf() shouldn't expand "auto" margins.
    // (By default, AlignJustifySelf() *will* expand such margins, to fill the
    // available space before any alignment is done.)
    IgnoreAutoMargins,

    // We're aligning a margin box - the margin is already included in the
    // size. Implies `IgnoreAutoMargins`.
    AligningMarginBox,

    // If the item is baseline-aligned, this flag indicates that the item is in
    // the last baseline sharing group of the container, otherwise the item is
    // in the container's first baseline sharing group.
    LastBaselineSharingGroup,
}
impl EnumSetValue for CSSAlignUtils_AlignJustifyFlag {
    fn BitFor(self) -> u32 {
        1 << (self as u32)
    }
}
pub type AlignJustifyFlag = CSSAlignUtils_AlignJustifyFlag;
// Gecko: layout/generic/CSSAlignUtils.h mozilla::CSSAlignUtils::AlignJustifyFlags
pub type AlignJustifyFlags = EnumSet<AlignJustifyFlag>;

/**
 * Additional information required to resolve anchor-center on a particular
 * axis.
 */
// Gecko: layout/generic/CSSAlignUtils.h mozilla::CSSAlignUtils::AnchorAlignInfo
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CSSAlignUtils_AnchorAlignInfo {
    pub mAnchorStart: nscoord,
    pub mAnchorSize: nscoord,
}
pub type AnchorAlignInfo = CSSAlignUtils_AnchorAlignInfo;

impl CSSAlignUtils {
    /**
     * Map a raw StyleAlignFlags value to the used one.
     */
    // Gecko: layout/generic/CSSAlignUtils.cpp CSSAlignUtils::UsedAlignmentForAbsPos
    pub fn UsedAlignmentForAbsPos(aFrame: nsIFrame, mut aFlags: StyleAlignFlags, aLogicalAxis: LogicalAxis, aCBWM: WritingMode) -> StyleAlignFlags {
        MOZ_ASSERT!(aFrame.IsAbsolutelyPositioned(None));

        // Extract and strip the flag bits
        let alignmentFlags = aFlags & StyleAlignFlags::FLAG_BITS;
        aFlags &= !StyleAlignFlags::FLAG_BITS;

        if aFlags == StyleAlignFlags::NORMAL {
            // "the 'normal' keyword behaves as 'start' on replaced
            // absolutely-positioned boxes, and behaves as 'stretch' on all other
            // absolutely-positioned boxes."
            // https://drafts.csswg.org/css-align/#align-abspos
            // https://drafts.csswg.org/css-align/#justify-abspos
            aFlags = if aFrame.HasReplacedSizing() { StyleAlignFlags::START } else { StyleAlignFlags::STRETCH };
        } else if aFlags == StyleAlignFlags::FLEX_START {
            aFlags = StyleAlignFlags::START;
        } else if aFlags == StyleAlignFlags::FLEX_END {
            aFlags = StyleAlignFlags::END;
        } else if aFlags == StyleAlignFlags::LEFT || aFlags == StyleAlignFlags::RIGHT {
            if aLogicalAxis == LogicalAxis::Inline {
                let isLeft = aFlags == StyleAlignFlags::LEFT;
                aFlags = if isLeft == aCBWM.IsBidiLTR() { StyleAlignFlags::START } else { StyleAlignFlags::END };
            } else {
                aFlags = StyleAlignFlags::START;
            }
        } else if aFlags == StyleAlignFlags::BASELINE {
            aFlags = StyleAlignFlags::START;
        } else if aFlags == StyleAlignFlags::LAST_BASELINE {
            aFlags = StyleAlignFlags::END;
        }

        aFlags | alignmentFlags
    }
}

// Gecko: layout/generic/CSSAlignUtils.cpp SpaceToFill
fn SpaceToFill(aWM: WritingMode, aSize: &LogicalSize, aMargin: nscoord, aAxis: LogicalAxis, aCBSize: nscoord) -> nscoord {
    let size = aSize.Size(aAxis, aWM);
    aCBSize - (size + aMargin)
}

impl CSSAlignUtils {
    /**
     * This computes the aligned offset of a CSS-aligned child within its
     * alignment container. The returned offset is distance between the
     * logical "start" edge of the alignment container & the logical "start" edge
     * of the aligned child (in terms of the alignment container's writing mode).
     *
     * @param aAlignment An enumerated value representing a keyword for
     *                   "align-self" or "justify-self". The values
     *                   StyleAlignFlags::{AUTO,LEFT,RIGHT} must *not* be
     *                   passed here; this method expects the caller to have
     *                   already resolved those to 'start', 'end', or 'stretch'.
     * @param aAxis The container's axis in which we're doing alignment.
     * @param aBaselineAdjust The amount to offset baseline-aligned children.
     * @param aCBSize The size of the alignment container, in its aAxis.
     * @param aRI A ReflowInput for the child.
     * @param aChildSize The child's LogicalSize (in its own writing mode).
     * @param aAnchorInfo When specified, an inset-modified anchor start and size
     * (in the child's writing mode) to use for anchor-center alignment.
     */
    // Gecko: layout/generic/CSSAlignUtils.cpp CSSAlignUtils::AlignJustifySelf
    // ADAPTER: the C++ default argument `aAnchorRect = Nothing()` becomes two functions: this one passes no
    // anchor, `AlignJustifySelf_WithAnchor` takes it. (StyleAlignFlags is a small Copy value.)
    pub fn AlignJustifySelf(aAlignment: StyleAlignFlags, aAxis: LogicalAxis, aFlags: AlignJustifyFlags, aBaselineAdjust: nscoord, aCBSize: nscoord, aRI: &ReflowInput, aChildSize: &LogicalSize) -> nscoord {
        CSSAlignUtils::AlignJustifySelf_WithAnchor(&aAlignment, aAxis, aFlags, aBaselineAdjust, aCBSize, aRI, aChildSize, &None)
    }

    // Gecko: layout/generic/CSSAlignUtils.cpp CSSAlignUtils::AlignJustifySelf (with aAnchorInfo)
    pub fn AlignJustifySelf_WithAnchor(
        aAlignment: &StyleAlignFlags,
        aAxis: LogicalAxis,
        aFlags: AlignJustifyFlags,
        aBaselineAdjust: nscoord,
        aCBSize: nscoord,
        aRI: &ReflowInput,
        aChildSize: &LogicalSize,
        aAnchorInfo: &Option<AnchorAlignInfo>,
    ) -> nscoord {
        MOZ_ASSERT!(*aAlignment != StyleAlignFlags::AUTO, "auto values should have resolved already");
        MOZ_ASSERT!(*aAlignment != StyleAlignFlags::LEFT && *aAlignment != StyleAlignFlags::RIGHT, "caller should map that to the corresponding START/END");

        let isSameSide = aFlags.contains(AlignJustifyFlag::SameSide);

        let mut alignment = *aAlignment;
        // Map some alignment values to 'start' / 'end'.
        if alignment == StyleAlignFlags::SELF_START {
            // align/justify-self: self-start
            alignment = if isSameSide { StyleAlignFlags::START } else { StyleAlignFlags::END };
        } else if alignment == StyleAlignFlags::SELF_END {
            alignment = if isSameSide { StyleAlignFlags::END } else { StyleAlignFlags::START };
            // flex-start/flex-end are the same as start/end, in most contexts.
            // (They have special behavior in flex containers, so flex containers
            // should map them to some other value before calling this method.)
        } else if alignment == StyleAlignFlags::FLEX_START {
            alignment = StyleAlignFlags::START;
        } else if alignment == StyleAlignFlags::FLEX_END {
            alignment = StyleAlignFlags::END;
        }

        // Get the item's margin corresponding to the container's start/end side.
        let wm = aRI.GetWritingMode();
        // If we're handling the margin box, it's already included in the incoming
        // size.
        let margin = if aFlags.contains(AlignJustifyFlag::AligningMarginBox) { LogicalMargin::new__WritingMode(wm) } else { aRI.ComputedLogicalMargin(wm) };
        let startSide = MakeLogicalSide(aAxis, if isSameSide { LogicalEdge::Start } else { LogicalEdge::End });
        let marginStart = margin.Side(startSide, wm);
        let endSide = GetOppositeSide(startSide);
        let marginEnd = margin.Side(endSide, wm);

        let hasAutoMarginStart;
        let hasAutoMarginEnd;
        let styleMargin = &aRI.mStyleMargin;
        let anchorResolutionParams = AnchorPosResolutionParams::From__SizeComputationInput_bool(aRI, false);
        if aFlags.contains(AlignJustifyFlag::IgnoreAutoMargins) || aFlags.contains(AlignJustifyFlag::AligningMarginBox) {
            // (Note: ReflowInput will have treated "auto" margins as 0, so we
            // don't need to do anything special to avoid expanding them.)
            hasAutoMarginStart = false;
            hasAutoMarginEnd = false;
        } else if aAxis == LogicalAxis::Block {
            hasAutoMarginStart = styleMargin.GetMargin__LogicalSide_WritingMode_AnchorPosResolutionParams(LogicalSide::BStart, wm, &anchorResolutionParams).IsAuto();
            hasAutoMarginEnd = styleMargin.GetMargin__LogicalSide_WritingMode_AnchorPosResolutionParams(LogicalSide::BEnd, wm, &anchorResolutionParams).IsAuto();
        } else {
            /* aAxis == LogicalAxis::Inline */
            hasAutoMarginStart = styleMargin.GetMargin__LogicalSide_WritingMode_AnchorPosResolutionParams(LogicalSide::IStart, wm, &anchorResolutionParams).IsAuto();
            hasAutoMarginEnd = styleMargin.GetMargin__LogicalSide_WritingMode_AnchorPosResolutionParams(LogicalSide::IEnd, wm, &anchorResolutionParams).IsAuto();
        }

        // https://drafts.csswg.org/css-align-3/#overflow-values
        // This implements <overflow-position> = 'safe'.
        // And auto-margins: https://drafts.csswg.org/css-grid/#auto-margins
        if (aFlags.contains(AlignJustifyFlag::OverflowSafe) && alignment != StyleAlignFlags::START) || hasAutoMarginStart || hasAutoMarginEnd {
            let space = SpaceToFill(wm, aChildSize, marginStart + marginEnd, aAxis, aCBSize);
            // XXX we might want to include == 0 here as an optimization -
            // I need to see what the baseline/last baseline code looks like first.
            if space < 0 {
                // "Overflowing elements ignore their auto margins and overflow
                // in the end directions"
                alignment = StyleAlignFlags::START;
            } else if hasAutoMarginEnd {
                alignment = if hasAutoMarginStart {
                    StyleAlignFlags::CENTER
                } else if isSameSide {
                    StyleAlignFlags::START
                } else {
                    StyleAlignFlags::END
                };
            } else if hasAutoMarginStart {
                alignment = if isSameSide { StyleAlignFlags::END } else { StyleAlignFlags::START };
            }
        }

        // Determine the offset for the child frame (its border-box) which will
        // achieve the requested alignment.
        let offset: nscoord;
        if alignment == StyleAlignFlags::BASELINE || alignment == StyleAlignFlags::LAST_BASELINE {
            let isFirstBaselineSharingGroup = !aFlags.contains(AlignJustifyFlag::LastBaselineSharingGroup);
            if isFirstBaselineSharingGroup {
                offset = marginStart + aBaselineAdjust;
            } else {
                let size = aChildSize.Size(aAxis, wm);
                offset = aCBSize - (size + marginEnd) - aBaselineAdjust;
            }
        } else if alignment == StyleAlignFlags::STRETCH || alignment == StyleAlignFlags::START {
            // ComputeSize() deals with stretch
            offset = marginStart;
        } else if alignment == StyleAlignFlags::END {
            let size = aChildSize.Size(aAxis, wm);
            offset = aCBSize - (size + marginEnd);
        } else if alignment == StyleAlignFlags::ANCHOR_CENTER && aAnchorInfo.is_some() {
            let aAnchorInfo = aAnchorInfo.as_ref().unwrap();
            let anchorSize = aAnchorInfo.mAnchorSize;
            let anchorStart = aAnchorInfo.mAnchorStart;
            let size = aChildSize.Size(aAxis, wm);

            // Offset relative to the anchors center, accounting for margins
            offset = anchorStart + (anchorSize - size + marginStart - marginEnd) / 2;
        } else {
            // ANCHOR_CENTER with no Anchor is treated like CENTER.
            MOZ_ASSERT!(alignment == StyleAlignFlags::CENTER || alignment == StyleAlignFlags::ANCHOR_CENTER, "unknown align-/justify-self value");
            let size = aChildSize.Size(aAxis, wm);
            offset = (aCBSize - size + marginStart - marginEnd) / 2;
        }

        offset
    }
}

// Tests written from CSS Box Alignment 3 (6.2 self-alignment, auto margins): the offset of the
// child's border box from the container's start edge, for a 100px container and a 40px child.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::*;
    use mrt_style::mrt_dom::Dom;
    use mrt_style::StyleDom;

    // Lays out one block child with the given style, and returns the offset AlignJustifySelf gives it in
    // a 100px-wide container (inline axis) with the alignment and flags.
    fn offset_of(aStyle: &str, aAlignment: StyleAlignFlags, aFlags: AlignJustifyFlags) -> nscoord {
        let html = format!("<!DOCTYPE html><html><head><style>body{{margin:0}}</style></head><body><div style='display:grid;width:100px'><div id=c style='{aStyle}'></div></div></body></html>");
        let mut sd = StyleDom::new(Dom::parse(&html), (1000.0, 800.0));
        sd.resolve();
        // the frames read their content from the DOM (see nsIContent)
        let sd = KeepDocumentAlive(sd);
        let result = LayoutDocument(&sd, 1000, 800);
        // html > body > grid container > item
        let child = result.mRootElementFrame.unwrap().GetChildList(FrameChildListID::Principal).FirstChild().unwrap().GetChildList(FrameChildListID::Principal).FirstChild().unwrap().GetChildList(FrameChildListID::Principal).FirstChild().unwrap();
        let wm = child.GetWritingMode();
        let px = |v: i32| v * AppUnitsPerCSSPixel();
        // The child's reflow input needs a parent: without one Gecko computes no margins (a root frame has
        // none), so the margins of the child would be 0. The parent is a grid container so that, as for a
        // real grid item, an over-constrained inline-size does not change the end margin.
        let parent = child.GetParent().unwrap();
        let parentRI = ReflowInput::new__nsPresContext_nsIFrame_gfxContext_LogicalSize_InitFlags(&nsPresContext::Current(), parent, gfxContext, LogicalSize::new(wm, px(100), NS_UNCONSTRAINEDSIZE), EnumSet::default());
        let ri = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(&nsPresContext::Current(), &parentRI, child, LogicalSize::new(wm, px(100), NS_UNCONSTRAINEDSIZE), None, EnumSet::default(), StyleSizeOverrides::default(), ComputeSizeFlags::default(), std::ptr::null_mut());
        let size = LogicalSize::new(wm, px(40), px(10));
        CSSAlignUtils::AlignJustifySelf(aAlignment, LogicalAxis::Inline, aFlags, 0, px(100), &ri, &size) / AppUnitsPerCSSPixel()
    }

    fn same_side() -> AlignJustifyFlags {
        let mut f = AlignJustifyFlags::default();
        f += AlignJustifyFlag::SameSide;
        f
    }

    #[test]
    fn start_end_center_place_the_child_at_the_matching_edge() {
        assert_eq!(offset_of("width:40px", StyleAlignFlags::START, same_side()), 0);
        assert_eq!(offset_of("width:40px", StyleAlignFlags::END, same_side()), 60);
        assert_eq!(offset_of("width:40px", StyleAlignFlags::CENTER, same_side()), 30);
    }

    #[test]
    fn margins_move_the_offset() {
        // start adds the start margin; end subtracts the end margin; center splits the difference
        assert_eq!(offset_of("width:40px;margin-left:10px;margin-right:4px", StyleAlignFlags::START, same_side()), 10);
        assert_eq!(offset_of("width:40px;margin-left:10px;margin-right:4px", StyleAlignFlags::END, same_side()), 56);
        assert_eq!(offset_of("width:40px;margin-left:10px;margin-right:4px", StyleAlignFlags::CENTER, same_side()), 33);
    }

    #[test]
    fn auto_margins_absorb_the_free_space_unless_ignored() {
        // both auto: centered; end auto only: start side; start auto only: end side
        assert_eq!(offset_of("width:40px;margin-left:auto;margin-right:auto", StyleAlignFlags::START, same_side()), 30);
        assert_eq!(offset_of("width:40px;margin-right:auto", StyleAlignFlags::END, same_side()), 0);
        assert_eq!(offset_of("width:40px;margin-left:auto", StyleAlignFlags::START, same_side()), 60);
        let mut f = same_side();
        f += AlignJustifyFlag::IgnoreAutoMargins;
        assert_eq!(offset_of("width:40px;margin-left:auto;margin-right:auto", StyleAlignFlags::START, f), 0);
    }

    #[test]
    fn self_start_depends_on_the_side() {
        assert_eq!(offset_of("width:40px", StyleAlignFlags::SELF_START, same_side()), 0);
        assert_eq!(offset_of("width:40px", StyleAlignFlags::SELF_START, AlignJustifyFlags::default()), 60);
    }
}
