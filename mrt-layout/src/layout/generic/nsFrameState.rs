// ADAPTER (not a translation of layout/generic/nsFrameState.h): the frame state bits that
// translated layout code tests. In Gecko the enum is generated from nsFrameStateBits.h, with the
// bits of each frame class in its own group; the values here are only this crate's own bit numbers
// (nothing depends on them). Add a constant when translated code needs another bit, keeping
// Gecko's name.
use crate::*;

// Gecko: layout/generic/nsFrameState.h nsFrameState
pub type nsFrameState = u64;

pub const NS_FRAME_IN_REFLOW: nsFrameState = 1 << 0;
pub const NS_FRAME_FIRST_REFLOW: nsFrameState = 1 << 1;
pub const NS_FRAME_IS_DIRTY: nsFrameState = 1 << 2;
pub const NS_FRAME_HAS_DIRTY_CHILDREN: nsFrameState = 1 << 3;
pub const NS_FRAME_OUT_OF_FLOW: nsFrameState = 1 << 4;
pub const NS_FRAME_IN_CONSTRAINED_BSIZE: nsFrameState = 1 << 5;
pub const NS_FRAME_CONTAINS_RELATIVE_BSIZE: nsFrameState = 1 << 6;
pub const NS_FRAME_DYNAMIC_REFLOW_ROOT: nsFrameState = 1 << 7;
pub const NS_FRAME_FONT_INFLATION_FLOW_ROOT: nsFrameState = 1 << 8;
pub const NS_BLOCK_HAS_MARKER: nsFrameState = 1 << 9;
pub const NS_TABLE_CELL_HAD_SPECIAL_REFLOW: nsFrameState = 1 << 10;
// Gecko: layout/generic/nsFrameStateBits.h NS_FRAME_IS_PUSHED_OUT_OF_FLOW
pub const NS_FRAME_IS_PUSHED_OUT_OF_FLOW: nsFrameState = 1 << 11;
// Gecko: layout/generic/nsFrameStateBits.h NS_BLOCK_BFC
// (the block formatting context bit, set on nsBlockFrame; "block margin root" in BlockReflowState)
pub const NS_BLOCK_BFC: nsFrameState = 1 << 12;

// Gecko: layout/generic/nsFrameStateBits.h, group Block (nsBlockFrame). The bit numbers are
// Gecko's own bit numbers within the group (this crate's other bits are 0..12, so they do not clash).
// NS_BLOCK_BFC and NS_BLOCK_HAS_MARKER are defined above.
pub const NS_BLOCK_HAS_FLOATS: nsFrameState = 1 << 20;
pub const NS_BLOCK_HAS_PUSHED_FLOATS: nsFrameState = 1 << 21;
pub const NS_BLOCK_HAS_LINE_CURSOR: nsFrameState = 1 << 23;
pub const NS_BLOCK_HAS_OVERFLOW_LINES: nsFrameState = 1 << 24;
pub const NS_BLOCK_HAS_OVERFLOW_OUT_OF_FLOWS: nsFrameState = 1 << 25;
pub const NS_BLOCK_HAS_CLEAR_CHILDREN: nsFrameState = 1 << 26;
pub const NS_BLOCK_HAS_FIRST_LETTER_STYLE: nsFrameState = 1 << 27;
pub const NS_BLOCK_NEEDS_BIDI_RESOLUTION: nsFrameState = 1 << 29;
pub const NS_BLOCK_HAS_LINE_CLAMP_ELLIPSIS: nsFrameState = 1 << 30;
pub const NS_BLOCK_HAS_LINE_CLAMP_ELLIPSIS_DESCENDANT: nsFrameState = 1 << 31;
pub const NS_BLOCK_LOOK_FOR_DIRTY_FRAMES: nsFrameState = 1 << 60;
pub const NS_BLOCK_INTRINSICS_INFLATED: nsFrameState = 1 << 61;
pub const NS_BLOCK_HAS_FIRST_LETTER_CHILD: nsFrameState = 1 << 62;
pub const NS_BLOCK_HAS_INLINE_ABSPOS_DESCENDANT: nsFrameState = 1 << 63;

// Gecko: layout/generic/nsFrameStateBits.h, group nsIFrame (bits used by nsBlockFrame.rs; the number is
// this crate's own).
pub const NS_FRAME_DESCENDANT_INTRINSIC_ISIZE_DEPENDS_ON_BSIZE: nsFrameState = 1 << 44;
pub const NS_FRAME_IS_OVERFLOW_CONTAINER: nsFrameState = 1 << 45;
pub const NS_FRAME_HAS_ABSPOS_CHILDREN: nsFrameState = 1 << 46;
pub const NS_FRAME_NO_MOVE_VIEW: nsFrameState = 1 << 47;
pub const NS_FRAME_MOVED_BY_REFLOW: nsFrameState = 1 << 48;

// Gecko: layout/generic/nsFrameStateBits.h, group Generic (nsContainerFrame.rs and nsInlineFrame.rs
// use these). Bits 14..19 are this crate's own numbers (NS_FRAME_IS_OVERFLOW_CONTAINER is in the group above), not Gecko's.
pub const NS_FRAME_TOO_DEEP_IN_FRAME_TREE: nsFrameState = 1 << 14;
pub const NS_FRAME_PART_OF_IBSPLIT: nsFrameState = 1 << 15;
pub const NS_FRAME_OWNS_ANON_BOXES: nsFrameState = 1 << 16;
// Gecko: layout/generic/nsFrameStateBits.h, group Inline (nsInlineFrame).
pub const NS_INLINE_FRAME_BIDI_VISUAL_STATE_IS_SET: nsFrameState = 1 << 54;
pub const NS_INLINE_FRAME_BIDI_VISUAL_IS_FIRST: nsFrameState = 1 << 55;
pub const NS_INLINE_FRAME_BIDI_VISUAL_IS_LAST: nsFrameState = 1 << 56;

// Gecko: layout/generic/nsFrameStateBits.h, group Generic and the placeholder group
// (nsPlaceholderFrame, nsCSSFrameConstructor). The bit numbers are this crate's own.
pub const PLACEHOLDER_FOR_FLOAT: nsFrameState = 1 << 33;
pub const PLACEHOLDER_FOR_ABSPOS: nsFrameState = 1 << 34;
pub const PLACEHOLDER_FOR_FIXEDPOS: nsFrameState = 1 << 35;
pub const PLACEHOLDER_FOR_POPUP: nsFrameState = 1 << 36;
pub const PLACEHOLDER_FOR_TOPLAYER: nsFrameState = 1 << 37;
pub const PLACEHOLDER_HAVE_LINE_IS_EMPTY_SO_FAR: nsFrameState = 1 << 38;
pub const PLACEHOLDER_LINE_IS_EMPTY_SO_FAR: nsFrameState = 1 << 39;
pub const NS_FRAME_GENERATED_CONTENT: nsFrameState = 1 << 40;
pub const NS_FRAME_CAN_HAVE_ABSPOS_CHILDREN: nsFrameState = 1 << 41;
pub const NS_REPEATED_ROW_OR_ROWGROUP: nsFrameState = 1 << 50;

// Gecko: layout/generic/nsFrameStateBits.h, groups TableCell, TableRow and TableRowGroup (the bit
// numbers are Gecko's own numbers within each group; a frame is of one class only, so the groups
// share numbers). NS_TABLE_CELL_HAD_SPECIAL_REFLOW and NS_REPEATED_ROW_OR_ROWGROUP are above.
pub const NS_TABLE_CELL_CONTENT_EMPTY: nsFrameState = 1 << 21;
pub const NS_ROW_HAS_CELL_WITH_STYLE_BSIZE: nsFrameState = 1 << 29;
pub const NS_TABLE_ROW_HAS_UNPAGINATED_BSIZE: nsFrameState = 1 << 30;
pub const NS_ROWGROUP_HAS_ROW_CURSOR: nsFrameState = 1 << 27;
pub const NS_ROWGROUP_HAS_STYLE_BSIZE: nsFrameState = 1 << 30;
pub const NS_ROWGROUP_REPEATABLE: nsFrameState = 1 << 31;

// Gecko: layout/generic/nsFrameStateBits.h, group FlexContainer (nsFlexContainerFrame). The bit numbers
// are this crate's own (Gecko's are 20..25 of the group).
// NS_STATE_FLEX_NORMAL_FLOW_CHILDREN_IN_CSS_ORDER: set when the in-flow children are not in ascending CSS 'order'.
pub const NS_STATE_FLEX_NORMAL_FLOW_CHILDREN_IN_CSS_ORDER: nsFrameState = 1 << 51;
// NS_STATE_FLEX_IS_EMULATING_LEGACY_WEBKIT_BOX: display:-webkit-box emulation (NOT PORTED, never set).
pub const NS_STATE_FLEX_IS_EMULATING_LEGACY_WEBKIT_BOX: nsFrameState = 1 << 52;
// NS_STATE_FLEX_SYNTHESIZE_BASELINE: no baseline-aligned item; synthesize the container's baseline.
pub const NS_STATE_FLEX_SYNTHESIZE_BASELINE: nsFrameState = 1 << 53;
// NS_STATE_FLEX_DID_PUSH_ITEMS, NS_STATE_FLEX_HAS_CHILD_NIFS: fragmentation bits (NOT PORTED but defined).
pub const NS_STATE_FLEX_DID_PUSH_ITEMS: nsFrameState = 1 << 54;
pub const NS_STATE_FLEX_HAS_CHILD_NIFS: nsFrameState = 1 << 55;
// NS_STATE_FLEX_COMPUTED_INFO: devtools info was generated (NOT PORTED but defined).
pub const NS_STATE_FLEX_COMPUTED_INFO: nsFrameState = 1 << 56;

// (the NS_STATE_GRID_* bits of the GridContainer group are defined in nsGridContainerFrame.rs)

// Gecko: layout/generic/nsFrameStateBits.h PLACEHOLDER_STATICPOS_NEEDS_CSSALIGN (placeholder group; the bit
// number is this crate's own). Only flex/grid containers set it, in ReflowPlaceholders.
pub const PLACEHOLDER_STATICPOS_NEEDS_CSSALIGN: nsFrameState = 1 << 57;
