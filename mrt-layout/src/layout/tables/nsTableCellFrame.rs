// Translated from layout/tables/nsTableCellFrame.h and nsTableCellFrame.cpp (GECKO_REV), stage 2b.
//
// Rust shape (see CONVENTIONS.md):
//  - `nsTableCellFrame` is a `Copy` handle over the frame (an nsIFrame of type TableCell) that
//    derefs to nsIFrame, as the C++ class derives from nsContainerFrame. `nsTableCellFrame*` that
//    may be null is `Option<nsTableCellFrame>`. The data members (`mColIndex`, `mPriorAvailISize`,
//    `mDesiredSize`) live in one heap block per frame behind a frame property, as in nsBlockFrame.rs
//    (see nsTableCellFrame_Members).
//  - A cell is `display: table-cell`; its single child is the anonymous cell-content block
//    (`Inner()`), which applies the padding. The cell itself applies the border.
//  - What is virtual in C++ is a `match` on the frame type in nsIFrame.rs / nsLineLayoutAdapter.rs
//    (`Reflow`, `IntrinsicISize`, `IntrinsicISizeOffsets`, `GetUsedMargin`, `GetLogicalSkipSides`).
//  - nsIPercentBSizeObserver is the trait of that name (nsIPercentBSizeObserver.rs), implemented by
//    the handle.
//
// NOT PORTED: collapsed borders (nsBCTableCellFrame: a table is never border-collapsed here, so
// `NS_NewTableCellFrame` always makes a plain cell), painting (BuildDisplayList, ProcessBorders,
// DecorateForSelection, ShouldPaintBackground), invalidation (InvalidateFrame*), accessibility,
// attribute and style-change notifications (AttributeChanged, DidSetComputedStyle), the
// nsITableCellLayout editor interface (kept: GetCellIndexes only), scroll containers in a cell
// (ScrollContainerFrame), and pagination (CalcUnpaginatedBSize is kept, it is never called without
// a paginated pres context). Each is marked where it occurs. Refcount logging (MOZ_COUNT_CTOR) is
// not ported.
//
// ADAPTER: Gecko reads the `rowspan` and `colspan` attributes of the content in GetRowSpan and
// GetColSpan. The reduced nsIContent has no attributes, so the frame constructor (or a test) stores
// the already-parsed values with SetSpans(); a cell without them spans one row and one column,
// which is also what Gecko answers for an anonymous cell.
use crate::nsLineLayoutAdapter::*;
use crate::*;
use std::ops::Deref;

// Gecko: layout/tables/nsTableCellFrame.h TableCellAlignment
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TableCellAlignment {
    Top,
    Middle,
    Bottom,
    Baseline,
}

// Gecko: layout/tables/nsTableCellFrame.h nsTableCellFrame
// ADAPTER: a handle (see the notes at the top of the file).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct nsTableCellFrame(pub nsIFrame);

impl Deref for nsTableCellFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// ADAPTER: the members of the C++ class nsTableCellFrame that are plain data, in one heap block per
// frame reached through a frame property (see nsBlockFrame_Members for the scheme). The block lives
// as long as the frame (frames are never freed).
pub struct nsTableCellFrame_Members {
    // The starting column for this cell
    // Gecko: nsTableCellFrame::mColIndex
    pub mColIndex: u32,
    // The avail isize during the last reflow
    // Gecko: nsTableCellFrame::mPriorAvailISize
    pub mPriorAvailISize: nscoord,
    // The last desired inline and block size
    // Gecko: nsTableCellFrame::mDesiredSize
    pub mDesiredSize: LogicalSize,
    // ADAPTER: the parsed `rowspan` and `colspan` attributes (see the top of the file).
    pub mRowSpan: i32,
    pub mColSpan: i32,
}

// (ids: this crate's own numbers, 7000..7019 belong to the table cell, row and row group frames.)
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(CellMembersProperty, usize, 7000);

// Gecko: layout/tables/nsTableCellFrame.cpp NS_NewTableCellFrame
// ADAPTER: frames are made by FrameTree::AddFrame; this gives one the class and what the C++
// constructor sets (`mDesiredSize(aTableFrame->GetWritingMode())`, `SetContentEmpty(false)`).
// NOT PORTED: `if (aTableFrame->IsBorderCollapse()) return nsBCTableCellFrame` (collapsed borders).
pub fn NS_NewTableCellFrame(aFrame: nsIFrame, aTableFrame: nsTableFrame) -> nsTableCellFrame {
    aFrame.SetType(LayoutFrameType::TableCell);
    let cell = nsTableCellFrame(aFrame);
    cell.Construct(aTableFrame);
    cell
}

impl nsTableCellFrame {
    // static_cast<nsTableCellFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsTableCellFrame {
        nsTableCellFrame(aFrame)
    }

    // do_QueryFrame<nsTableCellFrame>(frame)
    // (NS_QUERYFRAME_HEAD(nsTableCellFrame): the class itself, nsITableCellLayout,
    // nsIPercentBSizeObserver, and the base classes. nsBCTableCellFrame has no type of its own.)
    pub fn do_QueryFrame(aFrame: nsIFrame) -> Option<nsTableCellFrame> {
        if aFrame.Type() == LayoutFrameType::TableCell {
            Some(nsTableCellFrame(aFrame))
        } else {
            None
        }
    }

    // ADAPTER: this frame's data members (see nsTableCellFrame_Members). Created on first use.
    fn Members(&self) -> &'static mut nsTableCellFrame_Members {
        let p = match self.0.GetProperty(CellMembersProperty()) {
            Some(p) => p,
            None => {
                let p = Box::into_raw(Box::new(nsTableCellFrame_Members {
                    mColIndex: 0,
                    mPriorAvailISize: 0,
                    mDesiredSize: LogicalSize::new__WritingMode(self.0.GetWritingMode()),
                    mRowSpan: 1,
                    mColSpan: 1,
                })) as usize;
                self.0.SetOwnedPointerProperty::<nsTableCellFrame_Members>(CellMembersProperty(), p);
                p
            }
        };
        unsafe { &mut *(p as *mut nsTableCellFrame_Members) }
    }

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::nsTableCellFrame
    fn Construct(&self, aTableFrame: nsTableFrame) {
        // mDesiredSize(aTableFrame->GetWritingMode())
        self.Members().mDesiredSize = LogicalSize::new__WritingMode(aTableFrame.GetWritingMode());
        self.SetContentEmpty(false);
    }

    // ADAPTER: stores the parsed `rowspan` and `colspan` of the cell's element (see the top of the
    // file). Gecko clamps them when it parses the attributes (rowspan 0 to 65534, colspan 1 to 1000);
    // the caller passes the clamped values.
    pub fn SetSpans(&self, aRowSpan: i32, aColSpan: i32) {
        let members = self.Members();
        members.mRowSpan = aRowSpan;
        members.mColSpan = aColSpan;
    }

    // Gecko: layout/tables/nsTableCellFrame.h nsTableCellFrame::GetTableRowFrame
    pub fn GetTableRowFrame(&self) -> nsTableRowFrame {
        let parent = self.GetParent();
        MOZ_ASSERT!(parent.is_some() && parent.unwrap().IsTableRowFrame());
        nsTableRowFrame::from_frame(parent.unwrap())
    }

    // Gecko: layout/tables/nsTableCellFrame.h nsTableCellFrame::GetTableFrame
    pub fn GetTableFrame(&self) -> nsTableFrame {
        self.GetTableRowFrame().GetTableFrame()
    }

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::Init
    pub fn Init(&self, aContent: Option<nsIContent>, aParent: nsIFrame, aPrevInFlow: Option<nsIFrame>) {
        // Let the base class do its initialization
        self.nsContainerFrame_Init(aContent, aParent, aPrevInFlow);

        if self.HasAnyStateBits(NS_FRAME_FONT_INFLATION_CONTAINER) {
            self.AddStateBits(NS_FRAME_FONT_INFLATION_FLOW_ROOT);
        }

        if let Some(prevInFlow) = aPrevInFlow {
            // Set the column index
            let cellFrame = nsTableCellFrame(prevInFlow);
            let colIndex = cellFrame.ColIndex();
            self.SetColIndex(colIndex as i32);
        } else {
            // Although the spec doesn't say that writing-mode is not applied to
            // table-cells, we still override style value here because we want to
            // make effective writing mode of table structure frames consistent
            // within a table. The content inside table cells is reflowed by an
            // anonymous block, hence their writing mode is not affected.
            self.SetWritingMode(self.GetTableFrame().GetWritingMode());
        }
    }

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::Destroy
    pub fn Destroy(&self, aContext: &mut FrameDestroyContext) {
        nsTableFrame::MaybeUnregisterPositionedTablePart(self.0);
        self.nsContainerFrame_Destroy(aContext);
    }

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::AttributeChanged
    // NOT PORTED: attribute changes (a page is laid out once, there are no incremental updates).
    //
    // nsresult nsTableCellFrame::AttributeChanged(int32_t aNameSpaceID, nsAtom* aAttribute, AttrModType) {
    //   // We need to recalculate in this case because of the nowrap quirk in
    //   // BasicTableLayoutStrategy
    //   if (aNameSpaceID == kNameSpaceID_None && aAttribute == nsGkAtoms::nowrap &&
    //       PresContext()->CompatibilityMode() == eCompatibility_NavQuirks) {
    //     PresShell()->FrameNeedsReflow(this, IntrinsicDirty::FrameAndAncestors, NS_FRAME_IS_DIRTY);
    //   }
    //   const nsAtom* colSpanAttribute = MOZ_UNLIKELY(mContent->AsElement()->IsMathMLElement())
    //           ? nsGkAtoms::columnspan : nsGkAtoms::colspan;
    //   if (aAttribute == nsGkAtoms::rowspan || aAttribute == colSpanAttribute) {
    //     nsLayoutUtils::PostRestyleEvent(mContent->AsElement(), RestyleHint{0}, nsChangeHint_UpdateTableCellSpans);
    //   }
    //   return NS_OK;
    // }

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::DidSetComputedStyle
    // NOT PORTED: style changes (no restyling after the first style resolution), accessibility
    // notifications and border-collapse damage areas (nsTableCellFrame.cpp lines 169-209 at GECKO_REV).

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::AppendFrames, InsertFrames,
    // RemoveFrame (#ifdef DEBUG: they only MOZ_CRASH("unsupported operation"), because the
    // anonymous inner block is the content insertion frame)
    #[cfg(debug_assertions)]
    pub fn AppendFrames(&self, aListID: FrameChildListID, aFrameList: nsFrameList) {
        MOZ_CRASH!("unsupported operation");
    }
    #[cfg(debug_assertions)]
    pub fn InsertFrames(&self, aListID: FrameChildListID, aPrevFrame: Option<nsIFrame>, aPrevFrameLine: Option<&nsLineList_iterator>, aFrameList: nsFrameList) {
        MOZ_CRASH!("unsupported operation");
    }
    #[cfg(debug_assertions)]
    pub fn RemoveFrame(&self, aContext: &mut FrameDestroyContext, aListID: FrameChildListID, aOldFrame: nsIFrame) {
        MOZ_CRASH!("unsupported operation");
    }

    // Gecko: layout/tables/nsTableCellFrame.h nsTableCellFrame::GetContentInsertionFrame
    pub fn GetContentInsertionFrame(&self) -> Option<nsIFrame> {
        self.Inner().GetContentInsertionFrame()
    }

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::SetColIndex
    pub fn SetColIndex(&self, aColIndex: i32) {
        self.Members().mColIndex = aColIndex as u32;
    }

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::GetUsedMargin
    pub fn GetUsedMargin(&self) -> nsMargin {
        nsMargin::new(0, 0, 0, 0)
    }

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::NotifyPercentBSize
    // (nsIPercentBSizeObserver, below)

    // ASSURE DIFFERENT COLORS for selection
    // Gecko: layout/tables/nsTableCellFrame.cpp EnsureDifferentColors
    // NOT PORTED: selection painting (DecorateForSelection).

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::DecorateForSelection
    // NOT PORTED: painting (nsTableCellFrame.cpp lines 253-312 at GECKO_REV).

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::ProcessBorders
    // NOT PORTED: painting.

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::InvalidateFrame,
    // InvalidateFrameWithRect
    // NOT PORTED: invalidation (there are no display items to invalidate).

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::ShouldPaintBordersAndBackgrounds
    // NOT PORTED: painting. (It only reads `visibility`, `empty-cells` and GetContentEmpty().)

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::ShouldPaintBackground
    // NOT PORTED: painting.

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::GetLogicalSkipSides
    pub fn GetLogicalSkipSides(&self) -> LogicalSides {
        let mWritingMode = self.GetWritingMode();
        let mut skip = LogicalSides::new__WritingMode(mWritingMode);
        if self.StyleBorder().mBoxDecorationBreak() == StyleBoxDecorationBreak::Clone {
            return skip;
        }

        if self.GetPrevInFlow().is_some() {
            skip += LogicalSide::BStart;
        }
        if self.GetNextInFlow().is_some() {
            skip += LogicalSide::BEnd;
        }
        skip
    }

    // GetBorderOverflow says how far the cell's own borders extend
    // outside its own bounds.  In the separated borders model this should
    // just be zero (as it is for most frames), but in the collapsed
    // borders model (for which nsBCTableCellFrame overrides this virtual
    // method), it considers the extents of the collapsed border.
    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::GetBorderOverflow
    pub fn GetBorderOverflow(&self) -> nsMargin {
        nsMargin::new(0, 0, 0, 0)
    }

    // Align the cell's anonymous-block child within the cell. This applies the
    // CSS `vertical-align` property to position the child frame appropriately
    // (in terms of the writing mode of the cell contents, which may be different
    // from the table's WM).
    // This also resets the child's inline position, which in the case of an
    // orthogonal child may have been based on an unknown container size when
    // it was initially reflowed.
    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::AlignChildWithinCell
    pub fn AlignChildWithinCell(&self, aMaxAscent: nscoord, aForceAlignTop: ForceAlignTopForTableCell) {
        MOZ_ASSERT!(aForceAlignTop != ForceAlignTopForTableCell::Yes || self.PresContext().IsPaginated(), "We shouldn't force table-cells to do top alignment if we're not in printing!");

        let inner = self.Inner();
        let tableWM = self.GetWritingMode();
        let innerWM = inner.GetWritingMode();

        // The anonymous block child is to be placed within the cell's padding rect.
        // Get it in the inner frame's writing mode for alignment calculation.
        let containerSize = self.GetRect().Size();
        let paddingRect = LogicalRect::new__WritingMode_nsRect_nsSize(innerWM, self.GetPaddingRectRelativeToSelf(), containerSize);

        let kidRect = inner.GetLogicalRect__WritingMode_nsSize(innerWM, containerSize);

        // Calculate the position for the inner frame, initializing to the origin.
        let mut kidPosition = paddingRect.Origin(innerWM);

        // Apply table cell alignment to the block coordinate.
        let alignment = if aForceAlignTop == ForceAlignTopForTableCell::Yes { TableCellAlignment::Top } else { self.GetTableCellAlignment() };
        match alignment {
            TableCellAlignment::Baseline => {
                // [[fallthrough]] to Top when there is no baseline
                if let Some(baseline) = self.GetCellBaseline() {
                    // Align the baseline of the child frame with the baselines of other
                    // children in the same row which have baseline alignment
                    *kidPosition.B_mut(innerWM) = paddingRect.BStart(innerWM) + aMaxAscent - baseline;
                }
                // fallback to start alignment: leave kidPosition at the origin.
            }
            TableCellAlignment::Top => {
                // Leave kidPosition at the origin: the child frame will be aligned
                // with the padding rect's block-start.
            }

            TableCellAlignment::Bottom => {
                // Align the block-end of the child frame with the block-end of the
                // padding rect.
                *kidPosition.B_mut(innerWM) = paddingRect.BEnd(innerWM) - kidRect.BSize(innerWM);
            }

            TableCellAlignment::Middle => {
                // Align the middle of the child frame with the middle of the cell's
                // padding rect.
                *kidPosition.B_mut(innerWM) = paddingRect.BStart(innerWM) + (paddingRect.BSize(innerWM) - kidRect.BSize(innerWM)) / 2;
            }
        }

        // If the content is larger than the cell bSize, align from the padding-rect's
        // bStart edge.
        *kidPosition.B_mut(innerWM) = std::cmp::max(paddingRect.BStart(innerWM), kidPosition.B(innerWM));

        if kidPosition != kidRect.Origin(innerWM) {
            // If we're moving the inner frame, invalidate at the old position first.
            // inner->InvalidateFrameSubtree();
            // NOT PORTED: invalidation.
        }

        inner.SetPosition__WritingMode_LogicalPoint_nsSize(innerWM, kidPosition, containerSize);

        let mut reflowOutput = ReflowOutput::new__WritingMode(tableWM);
        reflowOutput.SetSize(tableWM, &self.GetLogicalSize__WritingMode(tableWM));

        let mut overflow = nsRect::new(0, 0, self.GetSize().width, self.GetSize().height);
        overflow.Inflate__nsMargin(self.GetBorderOverflow());
        reflowOutput.mOverflowAreas.SetAllTo(overflow);
        self.ConsiderChildOverflow(&mut reflowOutput.mOverflowAreas, inner, OverflowAreaUnionFlags::None);
        self.FinishAndStoreOverflow__ReflowOutput_nsStyleDisplay(&mut reflowOutput, None);

        if kidPosition != kidRect.Origin(innerWM) {
            // Invalidate new overflow rect.
            // inner->InvalidateFrameSubtree();
            // NOT PORTED: invalidation.
        }
    }

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::ComputeCustomOverflow
    pub fn ComputeCustomOverflow(&self, aOverflowAreas: &mut OverflowAreas) -> bool {
        let mut bounds = nsRect::new(0, 0, self.GetSize().width, self.GetSize().height);
        bounds.Inflate__nsMargin(self.GetBorderOverflow());

        aOverflowAreas.UnionAllWith(bounds);
        // return nsContainerFrame::ComputeCustomOverflow(aOverflowAreas);
        // NOT PORTED: nsIFrame::ComputeCustomOverflow (the base class only reads the overflow of
        // text decorations and the like; it returns true).
        true
    }

    // Per CSS 2.1, we map 'sub', 'super', 'text-top', 'text-bottom',
    // length, percentage, and calc() values to 'baseline'.
    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::GetTableCellAlignment
    pub fn GetTableCellAlignment(&self) -> TableCellAlignment {
        // ADAPTER: StyleDisplay()->mBaselineShift and mAlignmentBaseline come from
        // `vertical-align` here (nsIFrame::BaselineShift and AlignmentBaseline in nsLineLayoutAdapter.rs).
        let baselineShift = self.BaselineShift();
        if baselineShift.IsKeyword() {
            let value = baselineShift.AsKeyword();
            match value {
                StyleBaselineShiftKeyword::Top => return TableCellAlignment::Top,
                StyleBaselineShiftKeyword::Bottom => return TableCellAlignment::Bottom,
                _ => {}
            }
        }

        let alignmentBaseline = self.AlignmentBaseline();
        if alignmentBaseline == StyleAlignmentBaseline::Middle {
            return TableCellAlignment::Middle;
        }

        TableCellAlignment::Baseline
    }

    // Gecko: layout/tables/nsTableCellFrame.h nsTableCellFrame::HasTableCellAlignmentBaseline
    pub fn HasTableCellAlignmentBaseline(&self) -> bool {
        self.GetTableCellAlignment() == TableCellAlignment::Baseline && !self.GetContentEmpty()
    }

    // Return our anonymous inner frame.
    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::Inner
    pub fn Inner(&self) -> nsIFrame {
        let mFrames = self.mFrames();
        MOZ_ASSERT!(mFrames.OnlyChild().is_some(), "A table cell should have exactly one child!");
        mFrames.FirstChild().unwrap()
    }

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::CellContentFrame
    pub fn CellContentFrame(&self) -> nsIFrame {
        let inner = self.Inner();
        // if (ScrollContainerFrame* sf = do_QueryFrame(inner)) { return sf->GetScrolledFrame(); }
        // NOT PORTED: scroll containers (a cell's content is never wrapped in one).
        inner
    }

    // Get the first-line baseline of the cell relative to its block-start border
    // edge, as if the cell were vertically aligned to the top of the row.
    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::GetCellBaseline
    pub fn GetCellBaseline(&self) -> Option<nscoord> {
        // Empty cells don't participate in baseline alignment - fallback to
        // start alignment.
        if self.GetContentEmpty() {
            return None;
        }
        // Ignore the position of the inner frame relative to the cell frame
        // since we want the position as though the inner were top-aligned.
        let wm = self.GetWritingMode();
        let mut result: nscoord = 0;
        if self.StyleDisplay().IsContainLayout() || !nsLayoutUtils::GetFirstLineBaseline(wm, self.Inner(), &mut result) {
            // Synthesize a baseline from our content box, see bug 1591219.
            return Some(self.CellContentFrame().ContentBSize(wm) + self.GetLogicalUsedBorderAndPadding(wm).BStart(wm));
        }
        // `result` already includes the padding-start from the inner frame.
        Some(result + self.GetLogicalUsedBorder(wm).BStart(wm))
    }

    /**
     * return the cell's specified row span. this is what was specified in the
     * content model or in the style info, and is always >= 0.
     * to get the effective row span (the actual value that applies), use
     * GetEffectiveRowSpan()
     * @see nsTableFrame::GetEffectiveRowSpan()
     */
    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::GetRowSpan
    pub fn GetRowSpan(&self) -> i32 {
        // int32_t rowSpan = 1;
        //
        // // Don't look at the content's rowspan if we're a pseudo cell
        // if (!Style()->IsPseudoOrAnonBox()) {
        //   dom::Element* elem = mContent->AsElement();
        //   const nsAttrValue* attr = elem->GetParsedAttr(nsGkAtoms::rowspan);
        //   // Note that we don't need to check the tag name, because only table cells
        //   // (including MathML <mtd>) and table headers parse the "rowspan" attribute
        //   // into an integer.
        //   if (attr && attr->Type() == nsAttrValue::eInteger) {
        //     rowSpan = attr->GetIntegerValue();
        //   }
        // }
        // return rowSpan;
        // ADAPTER: the parsed attribute is stored with SetSpans (see the top of the file).
        self.Members().mRowSpan
    }

    // there is no set row index because row index depends on the cell's parent
    // row only

    // Return our cell content frame.
    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::AppendDirectlyOwnedAnonBoxes
    // NOT PORTED: restyling of anonymous boxes (the list is only read when styles change).

    /*---------------- nsITableCellLayout methods ------------------------*/

    /**
     * return the cell's starting row index (starting at 0 for the first row).
     * for continued cell frames the row index is that of the cell's first-in-flow
     * and the column index (starting at 0 for the first column
     */
    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::GetCellIndexes
    pub fn GetCellIndexes(&self, aRowIndex: &mut i32, aColIndex: &mut i32) -> nsresult {
        *aRowIndex = self.RowIndex() as i32;
        *aColIndex = self.Members().mColIndex as i32;
        NS_OK
    }

    /** return the mapped cell's row index (starting at 0 for the first row) */
    // Gecko: layout/tables/nsTableCellFrame.h nsTableCellFrame::RowIndex
    pub fn RowIndex(&self) -> u32 {
        nsTableRowFrame::from_frame(self.GetParent().unwrap()).GetRowIndex() as u32
    }

    /**
     * return the cell's specified col span. this is what was specified in the
     * content model or in the style info, and is always >= 1.
     * to get the effective col span (the actual value that applies), use
     * GetEffectiveColSpan()
     * @see nsTableFrame::GetEffectiveColSpan()
     */
    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::GetColSpan
    pub fn GetColSpan(&self) -> i32 {
        // int32_t colSpan = 1;
        //
        // // Don't look at the content's colspan if we're a pseudo cell
        // if (!Style()->IsPseudoOrAnonBox()) {
        //   dom::Element* elem = mContent->AsElement();
        //   const nsAttrValue* attr = elem->GetParsedAttr(
        //       MOZ_UNLIKELY(elem->IsMathMLElement()) ? nsGkAtoms::columnspan : nsGkAtoms::colspan);
        //   // Note that we don't need to check the tag name, because only table cells
        //   // (including MathML <mtd>) and table headers parse the "colspan" attribute
        //   // into an integer.
        //   if (attr && attr->Type() == nsAttrValue::eInteger) {
        //     colSpan = attr->GetIntegerValue();
        //   }
        // }
        // return colSpan;
        // ADAPTER: the parsed attribute is stored with SetSpans (see the top of the file).
        self.Members().mColSpan
    }

    /** return the cell's column index (starting at 0 for the first column) */
    // Gecko: layout/tables/nsTableCellFrame.h nsTableCellFrame::ColIndex
    pub fn ColIndex(&self) -> u32 {
        // NOTE: We copy this from previous continuations, and we don't ever have
        // dynamic updates when tables split, so our mColIndex always matches our
        // first continuation's.
        MOZ_ASSERT!(nsTableCellFrame(self.FirstContinuation()).Members().mColIndex == self.Members().mColIndex, "mColIndex out of sync with first continuation");
        self.Members().mColIndex
    }

    // Get or set the available isize given to this frame during its last reflow.
    // Gecko: layout/tables/nsTableCellFrame.h nsTableCellFrame::GetPriorAvailISize
    pub fn GetPriorAvailISize(&self) -> nscoord {
        self.Members().mPriorAvailISize
    }
    // Gecko: layout/tables/nsTableCellFrame.h nsTableCellFrame::SetPriorAvailISize
    pub fn SetPriorAvailISize(&self, aPriorAvailISize: nscoord) {
        self.Members().mPriorAvailISize = aPriorAvailISize;
    }

    // Get or set the desired size returned by this frame during its last reflow.
    // Gecko: layout/tables/nsTableCellFrame.h nsTableCellFrame::GetDesiredSize
    pub fn GetDesiredSize(&self) -> LogicalSize {
        self.Members().mDesiredSize
    }
    // Gecko: layout/tables/nsTableCellFrame.h nsTableCellFrame::SetDesiredSize
    pub fn SetDesiredSize(&self, aDesiredSize: &ReflowOutput) {
        self.Members().mDesiredSize = aDesiredSize.Size(self.GetWritingMode());
    }

    // Gecko: layout/tables/nsTableCellFrame.h nsTableCellFrame::GetContentEmpty
    pub fn GetContentEmpty(&self) -> bool {
        self.HasAnyStateBits(NS_TABLE_CELL_CONTENT_EMPTY)
    }
    // Gecko: layout/tables/nsTableCellFrame.h nsTableCellFrame::SetContentEmpty
    pub fn SetContentEmpty(&self, aContentEmpty: bool) {
        self.AddOrRemoveStateBits(NS_TABLE_CELL_CONTENT_EMPTY, aContentEmpty);
    }

    // Gecko: layout/tables/nsTableCellFrame.h nsTableCellFrame::GetNextCell
    pub fn GetNextCell(&self) -> Option<nsTableCellFrame> {
        let sibling = self.GetNextSibling();
        MOZ_ASSERT!(sibling.map_or(true, |s| nsTableCellFrame::do_QueryFrame(s).is_some()), "How do we have a non-cell sibling?");
        sibling.map(nsTableCellFrame)
    }

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::GetBorderWidth
    pub fn GetBorderWidth(&self, aWM: WritingMode) -> LogicalMargin {
        LogicalMargin::new__WritingMode_nsMargin(aWM, self.StyleBorder().GetComputedBorder())
    }

    // Gecko: layout/tables/nsTableCellFrame.h nsTableCellFrame::BCBordersChanged
    // (nsBCTableCellFrame overrides it; NOT PORTED, so a plain cell answers false)
    pub fn BCBordersChanged(&self) -> bool {
        false
    }

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::GetScrollTargetFrame
    // NOT PORTED: scroll containers.

    // Gecko: layout/tables/nsTableCellFrame.cpp CellHasVisibleContent
    fn CellHasVisibleContent(aTableFrame: nsTableFrame, aCell: nsTableCellFrame) -> bool {
        // see  http://www.w3.org/TR/CSS21/tables.html#empty-cells
        let content = aCell.CellContentFrame();
        if content.GetContentRect().height > 0 {
            return true;
        }
        if aTableFrame.IsBorderCollapse() {
            return true;
        }
        for innerFrame in content.mFrames().begin() {
            let frameType = innerFrame.Type();
            if LayoutFrameType::Text == frameType {
                let textFrame = nsTextFrame::from_frame(innerFrame);
                if textFrame.HasNoncollapsedCharacters() {
                    return true;
                }
            } else if LayoutFrameType::Placeholder != frameType {
                return true;
            } else if nsLayoutUtils::GetFloatFromPlaceholder(innerFrame).is_some() {
                return true;
            }
        }
        false
    }

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::IntrinsicISize
    pub fn IntrinsicISize(&self, aInput: &IntrinsicSizeInput, aType: IntrinsicISizeType) -> nscoord {
        // Note: a table cell has the same writing mode as its table ancestor, which
        // may differ from its inner frame that derives its writing mode from the
        // style of the <td> element. See nsTableCellFrame::Init().
        let innerInput = IntrinsicSizeInput::new__IntrinsicSizeInput_WritingMode_WritingMode(aInput, self.Inner().GetWritingMode(), self.GetWritingMode());
        nsLayoutUtils::IntrinsicForContainer(innerInput.mContext, self.Inner(), aType, innerInput.mPercentageBasisForChildren, nsLayoutUtils::IGNORE_PADDING, &StyleSizeOverrides::default())
    }

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::IntrinsicISizeOffsets
    pub fn IntrinsicISizeOffsets(&self, aPercentageBasis: nscoord) -> nsIFrame_IntrinsicSizeOffsetData {
        // nsContainerFrame::IntrinsicISizeOffsets is nsIFrame's
        let mut result = self.nsIFrame_IntrinsicISizeOffsets(aPercentageBasis);

        result.margin = 0;

        let wm = self.GetWritingMode();
        result.border = self.GetBorderWidth(wm).IStartEnd(wm);

        result
    }
}

// Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::NotifyPercentBSize,
// NeedsToObserve (nsIPercentBSizeObserver)
impl nsIPercentBSizeObserver for nsTableCellFrame {
    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::NotifyPercentBSize
    fn NotifyPercentBSize(&self, aReflowInput: &ReflowInput) {
        // ReflowInput ensures the mCBReflowInput of blocks inside a
        // cell is the cell frame, not the inner-cell block, and that the
        // containing block of an inner table is the containing block of its
        // table wrapper.
        // XXXldb Given the now-stricter |NeedsToObserve|, many if not all of
        // these tests are probably unnecessary.

        // Maybe the cell reflow input; we sure if we're inside the |if|.
        let cellRI = aReflowInput.mCBReflowInput;

        if let Some(cellRI) = cellRI {
            if cellRI.mFrame == self.0 && (cellRI.ComputedBSize() == NS_UNCONSTRAINEDSIZE || cellRI.ComputedBSize() == 0) {
                // XXXldb Why 0?
                // This is a percentage bsize on a frame whose percentage bsizes
                // are based on the bsize of the cell, since its containing block
                // is the inner cell frame.

                // We'll only honor the percent bsize if sibling-cells/ancestors
                // have specified/pct bsize. (Also, siblings only count for this if
                // both this cell and the sibling cell span exactly 1 row.)

                if nsTableFrame::AncestorsHaveStyleBSize(cellRI) || (self.GetTableFrame().GetEffectiveRowSpan__nsTableCellFrame_nsCellMap(&*self, None) == 1 && cellRI.mParentReflowInput.unwrap().mFrame.HasAnyStateBits(NS_ROW_HAS_CELL_WITH_STYLE_BSIZE)) {
                    let mut rs = aReflowInput.mParentReflowInput;
                    // for (rs = aReflowInput.mParentReflowInput; rs != cellRI; rs = rs->mParentReflowInput)
                    while let Some(r) = rs {
                        if std::ptr::eq(r, cellRI) {
                            break;
                        }
                        r.mFrame.AddStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE);
                        rs = r.mParentReflowInput;
                    }

                    nsTableFrame::RequestSpecialBSizeReflow(cellRI);
                }
            }
        }
    }

    // The cell needs to observe its block and things inside its block but nothing
    // below that
    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::NeedsToObserve
    fn NeedsToObserve(&self, aReflowInput: &ReflowInput) -> bool {
        let mut rs = aReflowInput.mParentReflowInput;
        let Some(r) = rs else {
            return false;
        };
        if r.mFrame == self.0 {
            // We always observe the child block.  It will never send any
            // notifications, but we need this so that the observer gets
            // propagated to its kids.
            return true;
        }
        rs = r.mParentReflowInput;
        let Some(r) = rs else {
            return false;
        };

        // We always need to let the percent bsize observer be propagated
        // from a table wrapper frame to an inner table frame.
        let fType = aReflowInput.mFrame.Type();
        if fType == LayoutFrameType::Table {
            return true;
        }

        // We need the observer to be propagated to all children of the cell
        // (i.e., children of the child block) in quirks mode, but only to
        // tables in standards mode.
        // XXX This may not be true in the case of orthogonal flows within
        // the cell (bug 1174711 comment 8); we may need to observe isizes
        // instead of bsizes for orthogonal children.
        r.mFrame == self.0 && (self.PresContext().CompatibilityMode() == eCompatibility_NavQuirks || fType == LayoutFrameType::TableWrapper)
    }
}

impl nsTableCellFrame {
    // the computed bsize for the cell, which descendants use for percent bsize
    // calculations it is the bsize (minus border, padding) of the cell's first in
    // flow during its final reflow without an unconstrained bsize.
    // Gecko: layout/tables/nsTableCellFrame.cpp CalcUnpaginatedBSize
    fn CalcUnpaginatedBSize(aCellFrame: nsTableCellFrame, aTableFrame: nsTableFrame, aBlockDirBorderPadding: nscoord) -> nscoord {
        let firstCellInFlow = nsTableCellFrame(aCellFrame.FirstInFlow());
        let firstTableInFlow = nsTableFrame::from_frame(aTableFrame.FirstInFlow());
        let mut row = nsTableRowFrame::from_frame(firstCellInFlow.GetParent().unwrap());
        let firstRGInFlow = nsTableRowGroupFrame::from_frame(row.GetParent().unwrap());

        let rowIndex = firstCellInFlow.RowIndex() as i32;
        let rowSpan = aTableFrame.GetEffectiveRowSpan__nsTableCellFrame_nsCellMap(&firstCellInFlow, None);

        let mut computedBSize = firstTableInFlow.GetRowSpacing__int32_t_int32_t(rowIndex, rowIndex + rowSpan - 1);
        computedBSize -= aBlockDirBorderPadding;
        let mut rowX: u32;
        // for (row = firstRGInFlow->GetFirstRow(), rowX = 0; row; row = row->GetNextRow(), rowX++)
        let mut currentRow = firstRGInFlow.GetFirstRow();
        rowX = 0;
        while let Some(row) = currentRow {
            if rowX as i32 > rowIndex + rowSpan - 1 {
                break;
            } else if rowX as i32 >= rowIndex {
                computedBSize += row.GetUnpaginatedBSize();
            }
            currentRow = row.GetNextRow();
            rowX += 1;
        }
        computedBSize
    }

    // Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::Reflow
    #[allow(invalid_reference_casting)]
    pub fn Reflow(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.MarkInReflow();
        // DO_GLOBAL_REFLOW_COUNT("nsTableCellFrame");  NOT PORTED: reflow counts.
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");
        // ADAPTER: Gecko's `const ReflowInput&` is const_cast to a mutable one below (SetComputedBSize).
        let aReflowInputMut: *mut ReflowInput = aReflowInput as *const ReflowInput as *mut ReflowInput;

        if aReflowInput.mFlags.mSpecialBSizeReflow {
            self.FirstInFlow().AddStateBits(NS_TABLE_CELL_HAD_SPECIAL_REFLOW);
        }

        // see if a special bsize reflow needs to occur due to having a pct height
        nsTableFrame::CheckRequestSpecialBSizeReflow(aReflowInput);

        let wm = aReflowInput.GetWritingMode();
        let mut availSize = aReflowInput.AvailableSize__();

        // @note |this| frame applies borders but not any padding.  Our anonymous
        // inner frame applies the padding (but not borders).
        let mut border = self.GetBorderWidth(wm);

        let mut kidSize = ReflowOutput::new__WritingMode(wm);
        self.SetPriorAvailISize(aReflowInput.AvailableISize());
        let inner = self.Inner();
        let tableFrame = self.GetTableFrame();

        if aReflowInput.mFlags.mSpecialBSizeReflow || aPresContext.IsPaginated() {
            // Here, we're changing our own reflow input, so we need to account for our
            // padding, even though we don't apply it anywhere else, to get the correct
            // percentage resolution on children.
            let bp = border + aReflowInput.ComputedLogicalPadding(wm);
            if aReflowInput.mFlags.mSpecialBSizeReflow {
                unsafe { &mut *aReflowInputMut }.SetComputedBSize(self.BSize(wm) - bp.BStartEnd(wm), ReflowInput_ResetResizeFlags::Yes);
            } else {
                let computedUnpaginatedBSize = nsTableCellFrame::CalcUnpaginatedBSize(*self, tableFrame, bp.BStartEnd(wm));
                if computedUnpaginatedBSize > 0 {
                    unsafe { &mut *aReflowInputMut }.SetComputedBSize(computedUnpaginatedBSize, ReflowInput_ResetResizeFlags::Yes);
                }
            }
        }

        // We need to apply the skip sides for current fragmentainer's border after
        // we finish calculating the special block-size or unpaginated block-size to
        // prevent the skip sides from affecting the results.
        //
        // We assume we are the last fragment by using
        // PreReflowBlockLevelLogicalSkipSides(), i.e. the block-end border and
        // padding is not skipped.
        border.ApplySkipSides(self.PreReflowBlockLevelLogicalSkipSides());

        *availSize.ISize_mut(wm) -= border.IStartEnd(wm);

        // If we have a constrained available block-size, shrink it by subtracting our
        // block-direction border and padding for our children.
        if NS_UNCONSTRAINEDSIZE != availSize.BSize(wm) {
            *availSize.BSize_mut(wm) -= border.BStart(wm);

            if aReflowInput.mStyleBorder.mBoxDecorationBreak() == StyleBoxDecorationBreak::Clone {
                // We have box-decoration-break:clone. Subtract block-end border from the
                // available block-size as well.
                *availSize.BSize_mut(wm) -= border.BEnd(wm);
            }
        }

        // Available block-size can became negative after subtracting block-direction
        // border and padding. Per spec, to guarantee progress, fragmentainers are
        // assumed to have a minimum block size of 1px regardless of their used size.
        // https://drafts.csswg.org/css-break/#breaking-rules
        *availSize.BSize_mut(wm) = std::cmp::max(availSize.BSize(wm), nsPresContext::CSSPixelsToAppUnits__int32_t(1));

        let kidWM = inner.GetWritingMode();
        let mut initFlags = ReflowInput_InitFlags::new();
        initFlags += ReflowInput_InitFlag::CallerWillInit;
        let mut kidReflowInput = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
            aPresContext,
            aReflowInput,
            inner,
            availSize.ConvertTo(kidWM, wm),
            None,
            initFlags,
            StyleSizeOverrides::default(),
            ComputeSizeFlags::default(),
            std::ptr::null_mut(),
        );
        // Override computed padding, in case it's percentage padding
        {
            let padding = aReflowInput.ComputedLogicalPadding(kidWM);
            kidReflowInput.Init(aPresContext, None, None, Some(padding));
            if inner.IsScrollContainerFrame() {
                // Propagate explicit block sizes to our inner frame, if it's a scroll
                // frame. Note that in table layout, explicit heights act as a minimum
                // height, see nsTableRowFrame::CalcCellActualBSize.
                //
                // Table cells don't respect box-sizing, so we need to remove the
                // padding, so that the scroll-frame sizes properly (since the
                // scrollbars also add to the padding area).
                let ToScrolledBSize = |aBSize: nscoord| -> nscoord { std::cmp::max(0, aBSize - padding.BStartEnd(kidWM)) };
                let mut minBSize = aReflowInput.ComputedMinBSize();
                if aReflowInput.ComputedBSize() != NS_UNCONSTRAINEDSIZE {
                    minBSize = std::cmp::max(minBSize, aReflowInput.ComputedBSize());
                }
                if minBSize > 0 {
                    kidReflowInput.SetComputedMinBSize(ToScrolledBSize(minBSize));
                }
            }
        }

        // Don't be a percent height observer if we're in the middle of
        // special-bsize reflow, in case we get an accidental NotifyPercentBSize()
        // call (which we shouldn't honor during special-bsize reflow)
        if !aReflowInput.mFlags.mSpecialBSizeReflow {
            // mPercentBSizeObserver is for children of cells in quirks mode,
            // but only those than are tables in standards mode.  NeedsToObserve
            // will determine how far this is propagated to descendants.
            kidReflowInput.mPercentBSizeObserver = Some(self);
        }
        // Don't propagate special bsize reflow input to our kids
        kidReflowInput.mFlags.mSpecialBSizeReflow = false;

        if aReflowInput.mFlags.mSpecialBSizeReflow || self.FirstInFlow().HasAnyStateBits(NS_TABLE_CELL_HAD_SPECIAL_REFLOW) {
            // We need to force the kid to have mBResize set if we've had a
            // special reflow in the past, since the non-special reflow needs to
            // resize back to what it was without the special bsize reflow.
            kidReflowInput.SetBResize(true);
        }

        let containerSize = aReflowInput.ComputedSizeAsContainerIfConstrained();

        let kidOrigin = border.StartOffset(wm);
        let origRect = inner.GetRect();
        // const nsRect origInkOverflow = inner->InkOverflowRect();
        // const bool firstReflow = inner->HasAnyStateBits(NS_FRAME_FIRST_REFLOW);
        // NOT PORTED: invalidation (origInkOverflow and firstReflow are only for InvalidateTableFrame).

        self.ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
            inner,
            aPresContext,
            &mut kidSize,
            &kidReflowInput,
            wm,
            kidOrigin,
            containerSize,
            nsIFrame_ReflowChildFlags::Default,
            aStatus,
            None,
        );
        if aStatus.IsOverflowIncomplete() {
            // Don't pass OVERFLOW_INCOMPLETE through tables until they can actually
            // handle it
            // XXX should paginate overflow as overflow, but not in this patch (bug
            // 379349)
            aStatus.SetIncomplete();
            NS_WARNING!("Set table cell incomplete");
        }

        // XXXbz is this invalidate actually needed, really?
        if self.HasAnyStateBits(NS_FRAME_IS_DIRTY) {
            // InvalidateFrameSubtree();
            // NOT PORTED: invalidation.
        }

        #[cfg(debug_assertions)]
        nsTableCellFrame::DebugCheckChildSize(inner, &kidSize);

        // Place the child
        nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(
            inner,
            aPresContext,
            &kidSize,
            Some(&kidReflowInput),
            wm,
            kidOrigin,
            containerSize,
            nsIFrame_ReflowChildFlags::Default,
        );

        {
            let prevInFlow = self.GetPrevInFlow();
            let isEmpty = match prevInFlow {
                Some(p) => nsTableCellFrame(p).GetContentEmpty(),
                None => !nsTableCellFrame::CellHasVisibleContent(tableFrame, *self),
            };
            self.SetContentEmpty(isEmpty);
        }

        if tableFrame.IsBorderCollapse() {
            // nsTableFrame::InvalidateTableFrame(inner, origRect, origInkOverflow, firstReflow);
            // NOT PORTED: invalidation.
        }
        // first, compute the bsize which can be set w/o being restricted by
        // available bsize
        let mut cellSize = LogicalSize::new__WritingMode(wm);
        *cellSize.BSize_mut(wm) = kidSize.BSize(wm);

        if NS_UNCONSTRAINEDSIZE != cellSize.BSize(wm) {
            *cellSize.BSize_mut(wm) += border.BStart(wm);

            if aStatus.IsComplete() || aReflowInput.mStyleBorder.mBoxDecorationBreak() == StyleBoxDecorationBreak::Clone {
                *cellSize.BSize_mut(wm) += border.BEnd(wm);
            }
        }

        // next determine the cell's isize. At this point, we've factored in the
        // cell's style attributes.
        *cellSize.ISize_mut(wm) = kidSize.ISize(wm);

        // factor in border (and disregard padding, which is handled by our child).
        if NS_UNCONSTRAINEDSIZE != cellSize.ISize(wm) {
            *cellSize.ISize_mut(wm) += border.IStartEnd(wm);
        }

        // set the cell's desired size and max element size
        aDesiredSize.SetSize(wm, &cellSize);

        // the overflow area will be computed when BlockDirAlignChild() gets called

        if aReflowInput.mFlags.mSpecialBSizeReflow && NS_UNCONSTRAINEDSIZE == aReflowInput.AvailableBSize() {
            *aDesiredSize.BSize_mut(wm) = self.BSize(wm);
        }

        // If our parent is in initial reflow, it'll handle invalidating our
        // entire overflow rect.
        if !self.GetParent().unwrap().HasAnyStateBits(NS_FRAME_FIRST_REFLOW) && nsSize::new(aDesiredSize.Width(), aDesiredSize.Height()) != self.GetRect().Size() {
            // InvalidateFrame();
            // NOT PORTED: invalidation.
        }

        // remember the desired size for this reflow
        self.SetDesiredSize(aDesiredSize);

        // Any absolutely-positioned children will get reflowed in
        // nsIFrame::FixupPositionedTableParts in another pass, so propagate our
        // dirtiness to them before our parent clears our dirty bits.
        self.PushDirtyBitToAbsoluteFrames();
    }

    // Gecko: layout/tables/nsTableCellFrame.cpp DebugCheckChildSize (#ifdef DEBUG)
    #[cfg(debug_assertions)]
    fn DebugCheckChildSize(aChild: nsIFrame, aMet: &ReflowOutput) {
        const PROBABLY_TOO_LARGE: nscoord = 1000000;
        let wm = aMet.GetWritingMode();
        if (aMet.ISize(wm) < 0) || (aMet.ISize(wm) > PROBABLY_TOO_LARGE) {
            eprintln!("WARNING: cell content {:?} has large inline size {} ", aChild, aMet.ISize(wm));
        }
    }
}

// Gecko: layout/tables/nsTableCellFrame.cpp nsBCTableCellFrame::Reflow, GetUsedBorder,
// GetBorderWidth, SetBorderWidth, GetBorderOverflow and the rest of nsBCTableCellFrame
// NOT PORTED: collapsed borders (a table is never border-collapsed here).

// Gecko: layout/tables/nsTableCellFrame.cpp nsTableCellFrame::BuildDisplayList and
// nsDisplayTableCellSelection
// NOT PORTED: painting (nsTableCellFrame.cpp lines 1042-1174 at GECKO_REV).

// Implemented here because that's a sane-ish way to make the includes work out.
// Gecko: layout/tables/nsTableCellFrame.h nsTableRowFrame::GetFirstCell
impl nsTableRowFrame {
    pub fn GetFirstCell(&self) -> Option<nsTableCellFrame> {
        let firstChild = self.mFrames().FirstChild();
        MOZ_ASSERT!(firstChild.map_or(true, |c| nsTableCellFrame::do_QueryFrame(c).is_some()), "How do we have a non-cell child?");
        firstChild.map(nsTableCellFrame)
    }
}

//////////////////////////////////////////////////////////////////////////////////////////
// TEMP stubs
//
// Everything below is NOT part of nsTableCellFrame.h/.cpp: members of nsIFrame and friends that the
// cell calls and no other translated file has yet. When the real member lands in its own file,
// delete the item here (a duplicate definition fails to compile).
//////////////////////////////////////////////////////////////////////////////////////////

impl nsIFrame {
    // Gecko: layout/generic/nsIFrame.h nsIFrame::SetWritingMode (the C++ code assigns mWritingMode)
    // ADAPTER: `mWritingMode = GetTableFrame()->GetWritingMode();` of the table part frames' Init.
    pub fn SetWritingMode(&self, aWritingMode: WritingMode) {
        WithFrameTree(|t| t.mFrames[self.0 as usize].mWritingMode = aWritingMode);
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetPaddingRectRelativeToSelf
    pub fn GetPaddingRectRelativeToSelf(&self) -> nsRect {
        let mut border = self.GetUsedBorder();
        border.ApplySkipSides(self.GetSkipSides());
        let mut r = nsRect::new(0, 0, self.GetSize().width, self.GetSize().height);
        r.Deflate__nsMargin(border);
        r
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetContentRectRelativeToSelf
    pub fn GetContentRectRelativeToSelf(&self) -> nsRect {
        let mut bp = self.GetUsedBorderAndPadding();
        bp.ApplySkipSides(self.GetSkipSides());
        let mut r = nsRect::new(0, 0, self.GetSize().width, self.GetSize().height);
        r.Deflate__nsMargin(bp);
        r
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetContentRect
    pub fn GetContentRect(&self) -> nsRect {
        let mut r = self.GetPaddingRectRelativeToSelf();
        r.Deflate__nsMargin(self.GetUsedPadding());
        r + self.GetPosition()
    }
}

// Tests: written from the documented behaviour (CSS 2.2 17.5.3 table height algorithm and
// vertical-align in cells, 17.6 borders and padding of cells), see nsTableWrapperFrame_test_support.
#[cfg(test)]
mod tests {
    use crate::nsTableWrapperFrame_test_support::*;
    use crate::BlockReflowState_test_support::*;
    use crate::*;

    // The block-start of the cell's inner block relative to the cell, in px.
    fn inner_y(t: &TTable, r: usize, c: usize) -> i32 {
        t.inners[r][c].GetPosition().y / PX
    }

    #[test]
    fn a_cell_is_as_big_as_its_content_plus_padding_and_border() {
        // CSS 2.2 17.5.3 / 17.6: the cell box is its content plus its padding and border; the
        // anonymous inner block takes the padding, the cell itself takes the border.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10).css("padding:5px;border:2px solid")]]);
        layout_root(&t, 500);
        let cell = t.cells[0][0];
        assert_eq!(cell.GetSize(), nsSize::new((20 + 10 + 4) * PX, (10 + 10 + 4) * PX));
        // the inner block is inside the border, and holds the padding
        assert_eq!(t.inners[0][0].GetPosition(), nsPoint::new(2 * PX, 2 * PX));
        assert_eq!(t.inners[0][0].GetSize(), nsSize::new(30 * PX, 20 * PX));
        assert_eq!(t.contents[0][0].GetPosition(), nsPoint::new(5 * PX, 5 * PX));
    }

    #[test]
    fn the_cell_has_no_margin() {
        // CSS 2.2 17.5: table cells have no margins.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10).css("margin:7px")]]);
        layout_root(&t, 500);
        assert_eq!(t.cells[0][0].GetUsedMargin(), nsMargin::new(0, 0, 0, 0));
        assert_eq!(t.cells[0][0].GetPosition(), nsPoint::new(0, 0));
        assert_eq!(t.cells[0][0].GetSize(), nsSize::new(20 * PX, 10 * PX));
    }

    #[test]
    fn cells_of_a_row_are_as_tall_as_the_row() {
        // CSS 2.2 17.5.3: the height of a row is the maximum of the heights of its cells; every cell
        // of the row is stretched to it.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10), tcell(20, 40)]]);
        layout_root(&t, 500);
        assert_eq!(t.cells[0][0].GetSize().height, 40 * PX);
        assert_eq!(t.cells[0][1].GetSize().height, 40 * PX);
        assert_eq!(t.rows[0].GetSize().height, 40 * PX);
    }

    #[test]
    fn vertical_align_top_keeps_the_content_at_the_top() {
        // CSS 2.2 17.5.3: 'vertical-align: top' aligns the top of the cell box's content with the top
        // of the row.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10).css("vertical-align:top"), tcell(20, 40)]]);
        layout_root(&t, 500);
        assert_eq!(inner_y(&t, 0, 0), 0);
        assert_eq!(t.cells[0][0].GetTableCellAlignment(), TableCellAlignment::Top);
    }

    #[test]
    fn vertical_align_middle_centers_the_content() {
        // CSS 2.2 17.5.3: 'middle' centers the cell box's content in the row.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10).css("vertical-align:middle"), tcell(20, 40)]]);
        layout_root(&t, 500);
        assert_eq!(inner_y(&t, 0, 0), 15);
        assert_eq!(t.cells[0][0].GetTableCellAlignment(), TableCellAlignment::Middle);
    }

    #[test]
    fn vertical_align_bottom_puts_the_content_at_the_bottom() {
        // CSS 2.2 17.5.3: 'bottom' aligns the bottom of the cell box's content with the bottom of
        // the row.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10).css("vertical-align:bottom"), tcell(20, 40)]]);
        layout_root(&t, 500);
        assert_eq!(inner_y(&t, 0, 0), 30);
        assert_eq!(t.cells[0][0].GetTableCellAlignment(), TableCellAlignment::Bottom);
    }

    #[test]
    fn vertical_align_baseline_is_the_default_and_aligns_the_baselines() {
        // CSS 2.2 17.5.3: with 'baseline' the baselines of the cells of a row are aligned; a cell
        // without a line box has its baseline at the bottom of its content, so the shorter cell
        // sits at the bottom of the taller one.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10), tcell(20, 40)]]);
        layout_root(&t, 500);
        assert_eq!(t.cells[0][0].GetTableCellAlignment(), TableCellAlignment::Baseline);
        assert_eq!(t.cells[0][0].GetCellBaseline(), Some(10 * PX));
        assert_eq!(t.cells[0][1].GetCellBaseline(), Some(40 * PX));
        assert_eq!(t.rows[0].GetMaxCellAscent(), 40 * PX);
        assert_eq!(inner_y(&t, 0, 0), 30);
        assert_eq!(inner_y(&t, 0, 1), 0);
    }

    #[test]
    fn sub_and_super_and_length_values_map_to_baseline() {
        // The comment of nsTableCellFrame::GetTableCellAlignment (CSS 2.1 17.5.3): 'sub', 'super',
        // 'text-top', 'text-bottom', lengths and percentages are treated as 'baseline'.
        for css in ["vertical-align:sub", "vertical-align:super", "vertical-align:text-top", "vertical-align:text-bottom", "vertical-align:5px", "vertical-align:20%"] {
            let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10).css(css)]]);
            assert_eq!(t.cells[0][0].GetTableCellAlignment(), TableCellAlignment::Baseline, "{css}");
        }
    }

    #[test]
    fn the_baseline_includes_the_border_and_padding_above_the_content() {
        // GetCellBaseline: the baseline is measured from the cell's border-box top; a cell without
        // a line box synthesizes it at the bottom of the content box.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10).css("padding:3px;border:2px solid")]]);
        layout_root(&t, 500);
        assert_eq!(t.cells[0][0].GetCellBaseline(), Some((10 + 3 + 2) * PX));
    }

    #[test]
    fn a_cell_height_is_a_minimum_for_the_row() {
        // CSS 2.2 17.5.3: the 'height' of a cell is a minimum; the row is at least that tall.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10).css("height:50px"), tcell(20, 20)]]);
        layout_root(&t, 500);
        assert_eq!(t.rows[0].GetSize().height, 50 * PX);
        assert_eq!(t.cells[0][1].GetSize().height, 50 * PX);
    }

    #[test]
    fn a_cell_height_smaller_than_its_content_is_ignored() {
        // CSS 2.2 17.5.3: the row is never shorter than the tallest cell content.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 40).css("height:10px")]]);
        layout_root(&t, 500);
        assert_eq!(t.rows[0].GetSize().height, 40 * PX);
    }

    #[test]
    fn the_cell_remembers_its_column_and_row_and_spans() {
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10).span(2, 1), tcell(20, 10)], vec![tcell(20, 10), tcell(20, 10), tcell(20, 10)]]);
        assert_eq!(t.cells[0][0].ColIndex(), 0);
        assert_eq!(t.cells[0][0].GetColSpan(), 2);
        assert_eq!(t.cells[0][0].GetRowSpan(), 1);
        assert_eq!(t.cells[0][1].ColIndex(), 2);
        assert_eq!(t.cells[1][2].ColIndex(), 2);
        assert_eq!(t.cells[1][2].RowIndex(), 1);
        let mut row = -1;
        let mut col = -1;
        assert_eq!(t.cells[1][1].GetCellIndexes(&mut row, &mut col), NS_OK);
        assert_eq!((row, col), (1, 1));
        assert_eq!(t.cells[0][0].GetNextCell(), Some(t.cells[0][1]));
        assert_eq!(t.cells[0][1].GetNextCell(), None);
        assert_eq!(t.cells[0][0].GetTableRowFrame(), t.rows[0]);
        assert_eq!(t.cells[0][0].GetTableFrame(), t.table);
        assert_eq!(t.rows[1].GetFirstCell(), Some(t.cells[1][0]));
        assert_eq!(t.cells[0][0].Inner(), t.inners[0][0]);
    }

    #[test]
    fn a_cell_without_children_is_empty_and_one_with_a_child_box_is_not() {
        // The cell's empty flag drives 'empty-cells' (CSS 2.2 17.6.1.1): a cell whose content has no
        // height and no child that is not a collapsed text or placeholder is empty. Any child box
        // (even a zero-height one) is content, as in CellHasVisibleContent.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 0), tcell(20, 10)]]);
        t.inners[0][0].mFrames().RemoveFrame(t.contents[0][0]);
        layout_root(&t, 500);
        assert!(t.cells[0][0].GetContentEmpty());
        assert!(!t.cells[0][1].GetContentEmpty());
        let u = build_table(500, "", "", "", None, &[vec![tcell(20, 0)]]);
        layout_root(&u, 500);
        assert!(!u.cells[0][0].GetContentEmpty());
    }

    #[test]
    fn the_prior_available_inline_size_and_desired_size_are_remembered_by_the_reflow() {
        let t = build_table(500, "", "", "", None, &[vec![tcell(30, 10), tcell(20, 25)]]);
        layout_root(&t, 500);
        let wm = t.cells[0][0].GetWritingMode();
        assert_eq!(t.cells[0][0].GetPriorAvailISize(), 30 * PX);
        assert_eq!(t.cells[0][1].GetPriorAvailISize(), 20 * PX);
        // the desired size is the size of the last reflow, before the row stretched the cell
        assert_eq!(t.cells[0][0].GetDesiredSize(), LogicalSize::new(wm, 30 * PX, 10 * PX));
        assert_eq!(t.cells[0][1].GetDesiredSize(), LogicalSize::new(wm, 20 * PX, 25 * PX));
    }

    #[test]
    fn the_intrinsic_inline_sizes_of_a_cell_are_those_of_its_content_without_padding() {
        // nsTableCellFrame::IntrinsicISize passes IGNORE_PADDING: the padding is in
        // IntrinsicISizeOffsets, with the border and without the margin.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10).flex(12, 40).css("padding:3px;border:2px solid;margin:9px")]]);
        layout_root(&t, 500);
        let input = IntrinsicSizeInput::new(Some(gfxContext), None, None);
        let cell = t.cells[0][0];
        assert_eq!(cell.GetMinISize(&input), 12 * PX);
        assert_eq!(cell.GetPrefISize(&input), 40 * PX);
        let offsets = cell.IntrinsicISizeOffsets(0);
        assert_eq!(offsets.padding, 6 * PX);
        assert_eq!(offsets.border, 4 * PX);
        assert_eq!(offsets.margin, 0);
    }

    #[test]
    fn a_cell_has_the_writing_mode_of_the_table() {
        // nsTableCellFrame::Init: the cell takes the writing mode of its table, whatever its style says.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10).css("writing-mode:vertical-rl")]]);
        assert_eq!(t.cells[0][0].GetWritingMode(), t.table.GetWritingMode());
    }

    #[test]
    fn do_query_frame_finds_only_cells() {
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10)]]);
        assert!(nsTableCellFrame::do_QueryFrame(t.cells[0][0].0).is_some());
        assert!(nsTableCellFrame::do_QueryFrame(t.rows[0].0).is_none());
    }
}
