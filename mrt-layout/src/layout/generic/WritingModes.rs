// Gecko: layout/generic/WritingModes.h (whole file)
//
// ACCESSOR SCHEME (read this first; it applies to every Logical* class below)
// ---------------------------------------------------------------------------
// C++ overloads one name on constness (`nscoord ISize(wm) const` and
// `nscoord& ISize(wm)`), on parameters (`Size(wm)` and `Size(axis, wm)`), and on
// public-versus-private (`IStart(wm)` public, `IStart()` private). Rust has none of these,
// so, on top of the CONVENTIONS.md overload rule:
//
//  1. The const overload keeps the plain name and returns the value:
//         region.ISize(aWM)                  // nscoord ISize(WritingMode) const
//     The non-const overload, which returns `nscoord&` to a stored member, gets the suffix
//     `_mut` (appended after any `__types` suffix) and returns `&mut nscoord`:
//         *region.ISize_mut(aWM) = 0;        // region.ISize(aWM) = 0;
//         *region.ISize_mut(aWM) += 5;       // region.ISize(aWM) += 5;
//     Where the C++ non-const overload returns a reference to a stored member, the Rust one
//     returns a reference to that member. (In every class here the references are to stored
//     members; there is no accessor whose reference is to a computed value.)
//
//  2. A name that has several PUBLIC overloads with different parameter types gets the
//     `__<types>` suffix on each of them (LogicalRect::Size__WritingMode and
//     LogicalRect::Size__LogicalAxis_WritingMode, plus `_mut` on the latter). A name whose
//     public overloads differ only in constness keeps the plain name (LogicalPoint::I,
//     LogicalSize::Size, LogicalMargin::Side).
//
//  3. The PRIVATE no-argument helpers (`I()`, `B()`, `ISize()`, `IStart()`, `IStartEnd()`,
//     ...) that C++ keeps next to the public `I(wm)`: the public WritingMode-taking one keeps
//     the plain name (rule 2 would otherwise force `I__WritingMode`, which would make every
//     caller in layout code unlike the C++), and the private one gets `__` (the suffix for an
//     empty parameter list): `I__()` and `I___mut()`; likewise `IStart__()`, `ISize___mut()`.
//     They are private and only used inside this file.
//
//  4. Constructors: the all-members one is `new`; the others are `new__<param types>`
//     (`new__WritingMode`, `new__WritingMode_nsRect_nsSize`, ...). The deleted default
//     constructors of the Logical* classes are not implemented (so no `Default`).
//
//  5. Operators are std traits: `==`/`!=` are `PartialEq` (with the writing-mode check in
//     debug builds), `+ - += -=` are `Add Sub AddAssign SubAssign`. `operator<<` is
//     `std::fmt::Display` with the same output. C++ `LogicalMargin::operator+=` returns a
//     copy by value; the Rust `AddAssign` returns nothing (nobody uses that return value).
//
//  6. `CHECK_WRITING_MODE(param)` takes the object explicitly because Rust macro hygiene
//     does not let a macro see the caller's `self`: `CHECK_WRITING_MODE!(self, param)`.
//
//  7. Nested C++ types (`WritingMode::InlineDir`, `WritingMode::BlockDir`) are top-level
//     `WritingMode_InlineDir` and `WritingMode_BlockDir`. Static C++ constants and
//     functions are associated consts and associated functions (`WritingMode::kUnknownWritingMode`,
//     `WritingMode::PhysicalSideForBlockAxis(..)`).
//
//  8. C++ `friend` access (LogicalRect reads `aOrigin.mPoint`) becomes plain private field
//     access: all the classes are in this one module.
//
// Style-system mapping: `StyleWritingMode` (Gecko's cbindgen of Stylo's `WritingMode` bit
// flags) is `style::logical_geometry::WritingMode` of the standalone `stylo` crate, whose bit
// values are the same as Gecko's (VERTICAL 0x01, INLINE_REVERSED 0x02, VERTICAL_LR 0x04,
// LINE_INVERTED 0x08, RTL 0x10, VERTICAL_SIDEWAYS 0x20, TEXT_SIDEWAYS 0x40, UPRIGHT 0x80).
// `._0` (cbindgen's name for the bits) is `.bits()`; `StyleWritingMode(x)` is
// `StyleWritingMode::from_bits_retain(x)`. See `WritingMode::new__ComputedStyle`.

use crate::*;
use std::fmt;
use std::marker::PhantomData;
use std::ops::{Add, AddAssign, Sub, SubAssign};

use mrt_style::style::logical_geometry::LogicalAxis as StyleLogicalAxis;
use mrt_style::style::logical_geometry::WritingMode as StyleWritingMode;
use mrt_style::style::properties::ComputedValues;

// ADAPTER: Gecko's `ComputedStyle` is stylo's `ComputedValues`.
type ComputedStyle = ComputedValues;

// ---------------------------------------------------------------------------
// TEMPORARY STAND-INS (headers not translated yet; replace with the real files when they exist)
// ---------------------------------------------------------------------------

// Gecko: mfbt/EnumSet.h mozilla::EnumSet (TEMPORARY STAND-IN: only what WritingModes.h uses.
// The bit for a value is `1 << (value as u32)`, as in C++ for enums without gaps.)
pub trait EnumSetValue: Copy {
    fn BitFor(self) -> u32;
}

pub struct EnumSet<T: EnumSetValue> {
    mBitField: u32,
    _marker: PhantomData<T>,
}

impl<T: EnumSetValue> Clone for EnumSet<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: EnumSetValue> Copy for EnumSet<T> {}
impl<T: EnumSetValue> PartialEq for EnumSet<T> {
    fn eq(&self, aOther: &Self) -> bool {
        self.mBitField == aOther.mBitField
    }
}
impl<T: EnumSetValue> Eq for EnumSet<T> {}
impl<T: EnumSetValue> Default for EnumSet<T> {
    fn default() -> Self {
        Self::from_mask(0)
    }
}
impl<T: EnumSetValue> fmt::Debug for EnumSet<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "EnumSet({:#b})", self.mBitField)
    }
}

impl<T: EnumSetValue> EnumSet<T> {
    // Rust-only: build the set from a raw bit mask (what C++ does with the constexpr
    // initializer-list constructor), usable in `const` items.
    pub const fn from_mask(aMask: u32) -> Self {
        EnumSet {
            mBitField: aMask,
            _marker: PhantomData,
        }
    }
    pub fn new() -> Self {
        Self::from_mask(0)
    }
    pub fn contains(&self, aEnum: T) -> bool {
        self.mBitField & aEnum.BitFor() != 0
    }
    pub fn isEmpty(&self) -> bool {
        self.mBitField == 0
    }
}
impl<T: EnumSetValue> AddAssign<T> for EnumSet<T> {
    fn add_assign(&mut self, aEnum: T) {
        self.mBitField |= aEnum.BitFor();
    }
}
impl<T: EnumSetValue> AddAssign<EnumSet<T>> for EnumSet<T> {
    fn add_assign(&mut self, aOther: EnumSet<T>) {
        self.mBitField |= aOther.mBitField;
    }
}
impl<T: EnumSetValue> SubAssign<T> for EnumSet<T> {
    fn sub_assign(&mut self, aEnum: T) {
        self.mBitField &= !aEnum.BitFor();
    }
}

// Gecko: layout/style/nsStyleConsts.h kWritingModeSidewaysMask (the header is not translated;
// private on purpose so the real one, once it exists, does not clash).
const kWritingModeSidewaysMask: u8 = 4;

// It is the caller's responsibility to operate on logical-coordinate objects
// with matched writing modes. Failure to do so will be a runtime bug; the
// compiler can't catch it, but in debug mode, we'll throw an assertion.
// NOTE that in non-debug builds, a writing mode mismatch error will NOT be
// detected, yet the results will be nonsense (and may lead to further layout
// failures). Therefore, it is important to test (and fuzz-test) writing-mode
// support using debug builds.

// Methods in logical-coordinate classes that take another logical-coordinate
// object as a parameter should call CHECK_WRITING_MODE on it to verify that
// the writing modes match.
// (In some cases, there are internal (private) methods that don't do this;
// such methods should only be used by other methods that have already checked
// the writing modes.)
// The check ignores the StyleWritingMode::VERTICAL_SIDEWAYS and
// StyleWritingMode::TEXT_SIDEWAYS bit of writing mode, because
// this does not affect the interpretation of logical coordinates.

// Gecko: layout/generic/WritingModes.h CHECK_WRITING_MODE
// `$obj` is the object whose GetWritingMode() the C++ macro reaches through `this`.
// The assertion is debug-only (the macro body is compiled out otherwise, as NS_ASSERTION is
// in release C++ builds, and IgnoreSideways() only exists in debug builds).
macro_rules! CHECK_WRITING_MODE {
    ($obj:expr, $param:expr) => {
        #[cfg(debug_assertions)]
        NS_ASSERTION!(
            $param.IgnoreSideways() == $obj.GetWritingMode().IgnoreSideways(),
            "writing-mode mismatch"
        );
    };
}

// Logical axis, edge, side and corner constants for use in various places.
// Gecko: layout/generic/WritingModes.h mozilla::LogicalAxis
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum LogicalAxis {
    Block,
    Inline,
}
// Gecko: layout/generic/WritingModes.h mozilla::LogicalEdge
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum LogicalEdge {
    Start,
    End,
}

// Gecko: layout/generic/WritingModes.h mozilla::LogicalSide
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum LogicalSide {
    BStart,
    BEnd,
    IStart,
    IEnd,
}
impl EnumSetValue for LogicalSide {
    fn BitFor(self) -> u32 {
        1 << (self as u32)
    }
}

// Gecko: layout/generic/WritingModes.h mozilla::LogicalCorner
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum LogicalCorner {
    BStartIStart,
    BStartIEnd,
    BEndIEnd,
    BEndIStart,
}

// Physical axis constants.
// Gecko: layout/generic/WritingModes.h mozilla::PhysicalAxis
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum PhysicalAxis {
    Vertical,
    Horizontal,
}
impl EnumSetValue for PhysicalAxis {
    fn BitFor(self) -> u32 {
        1 << (self as u32)
    }
}

// Gecko: layout/generic/WritingModes.h mozilla::PhysicalAxes
pub type PhysicalAxes = EnumSet<PhysicalAxis>;
// Gecko: layout/generic/WritingModes.h mozilla::kPhysicalAxesBoth
// (PhysicalAxis::Vertical and PhysicalAxis::Horizontal)
pub const kPhysicalAxesBoth: PhysicalAxes = EnumSet::from_mask(
    (1 << (PhysicalAxis::Vertical as u32)) | (1 << (PhysicalAxis::Horizontal as u32)),
);

// Gecko: layout/generic/WritingModes.h mozilla::ToStyleLogicalAxis
// (StyleLogicalAxis is Stylo's `logical_geometry::LogicalAxis`.)
pub fn ToStyleLogicalAxis(aLogicalAxis: LogicalAxis) -> StyleLogicalAxis {
    if aLogicalAxis == LogicalAxis::Block {
        StyleLogicalAxis::Block
    } else {
        StyleLogicalAxis::Inline
    }
}

// Gecko: layout/generic/WritingModes.h mozilla::GetOrthogonalAxis
pub fn GetOrthogonalAxis(aAxis: LogicalAxis) -> LogicalAxis {
    if aAxis == LogicalAxis::Block {
        LogicalAxis::Inline
    } else {
        LogicalAxis::Block
    }
}

// Gecko: layout/generic/WritingModes.h mozilla::IsInline
pub fn IsInline(aSide: LogicalSide) -> bool {
    (aSide == LogicalSide::IStart) || (aSide == LogicalSide::IEnd)
}

// Gecko: layout/generic/WritingModes.h mozilla::IsBlock
pub fn IsBlock(aSide: LogicalSide) -> bool {
    !IsInline(aSide)
}

// Gecko: layout/generic/WritingModes.h mozilla::IsEnd
pub fn IsEnd(aSide: LogicalSide) -> bool {
    (aSide == LogicalSide::BEnd) || (aSide == LogicalSide::IEnd)
}

// Gecko: layout/generic/WritingModes.h mozilla::IsStart
pub fn IsStart(aSide: LogicalSide) -> bool {
    !IsEnd(aSide)
}

// Gecko: layout/generic/WritingModes.h mozilla::GetAxis
pub fn GetAxis(aSide: LogicalSide) -> LogicalAxis {
    if IsInline(aSide) {
        LogicalAxis::Inline
    } else {
        LogicalAxis::Block
    }
}

// Gecko: layout/generic/WritingModes.h mozilla::GetEdge
pub fn GetEdge(aSide: LogicalSide) -> LogicalEdge {
    if IsEnd(aSide) {
        LogicalEdge::End
    } else {
        LogicalEdge::Start
    }
}

// Gecko: layout/generic/WritingModes.h mozilla::GetOppositeEdge
pub fn GetOppositeEdge(aEdge: LogicalEdge) -> LogicalEdge {
    if aEdge == LogicalEdge::Start {
        LogicalEdge::End
    } else {
        LogicalEdge::Start
    }
}

// Gecko: layout/generic/WritingModes.h mozilla::MakeLogicalSide
pub fn MakeLogicalSide(aAxis: LogicalAxis, aEdge: LogicalEdge) -> LogicalSide {
    if aAxis == LogicalAxis::Inline {
        return if aEdge == LogicalEdge::Start {
            LogicalSide::IStart
        } else {
            LogicalSide::IEnd
        };
    }
    if aEdge == LogicalEdge::Start {
        LogicalSide::BStart
    } else {
        LogicalSide::BEnd
    }
}

// Gecko: layout/generic/WritingModes.h mozilla::GetOppositeSide
pub fn GetOppositeSide(aSide: LogicalSide) -> LogicalSide {
    MakeLogicalSide(GetAxis(aSide), GetOppositeEdge(GetEdge(aSide)))
}

// Gecko: layout/generic/WritingModes.h mozilla::LineRelativeDir
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum LineRelativeDir {
    Over = LogicalSide::BStart as u8,
    Under = LogicalSide::BEnd as u8,
    Left = LogicalSide::IStart as u8,
    Right = LogicalSide::IEnd as u8,
}

// Gecko: layout/generic/WritingModes.h mozilla::WritingMode::InlineDir
// Absolute inline flow direction
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum WritingMode_InlineDir {
    LTR, // text flows horizontally left to right
    RTL, // text flows horizontally right to left
    TTB, // text flows vertically top to bottom
    BTT, // text flows vertically bottom to top
}

// Gecko: layout/generic/WritingModes.h mozilla::WritingMode::BlockDir
// Absolute block flow direction
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum WritingMode_BlockDir {
    TB, // horizontal lines stack top to bottom
    RL, // vertical lines stack right to left
    LR, // vertical lines stack left to right
}

/**
 * mozilla::WritingMode is an immutable class representing a
 * writing mode.
 *
 * It efficiently stores the writing mode and can rapidly compute
 * interesting things about it for use in layout.
 *
 * Writing modes are computed from the CSS 'direction',
 * 'writing-mode', and 'text-orientation' properties.
 * See CSS3 Writing Modes for more information
 *   http://www.w3.org/TR/css3-writing-modes/
 */
// Gecko: layout/generic/WritingModes.h mozilla::WritingMode
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WritingMode {
    mWritingMode: StyleWritingMode,
}

// `constexpr WritingMode() : mWritingMode{0}`: see WritingMode::new() below.
impl Default for WritingMode {
    fn default() -> Self {
        WritingMode::new()
    }
}

impl WritingMode {
    /**
     * Return the absolute inline flow direction as an InlineDir
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::GetInlineDir
    pub fn GetInlineDir(&self) -> WritingMode_InlineDir {
        if self.IsVertical() {
            return if self.IsInlineReversed() {
                WritingMode_InlineDir::BTT
            } else {
                WritingMode_InlineDir::TTB
            };
        }
        if self.IsInlineReversed() {
            WritingMode_InlineDir::RTL
        } else {
            WritingMode_InlineDir::LTR
        }
    }

    /**
     * Return the absolute block flow direction as a BlockDir
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::GetBlockDir
    pub fn GetBlockDir(&self) -> WritingMode_BlockDir {
        if self.IsVertical() {
            return if !(self.mWritingMode & StyleWritingMode::VERTICAL_LR).is_empty() {
                WritingMode_BlockDir::LR
            } else {
                WritingMode_BlockDir::RL
            };
        }
        WritingMode_BlockDir::TB
    }

    /**
     * Return true if the inline flow direction is against physical direction
     * (i.e. right-to-left or bottom-to-top).
     * This occurs when writing-mode is sideways-lr OR direction is rtl (but not
     * if both of those are true).
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::IsInlineReversed
    pub fn IsInlineReversed(&self) -> bool {
        !(self.mWritingMode & StyleWritingMode::INLINE_REVERSED).is_empty()
    }

    /**
     * Return true if bidi direction is LTR.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::IsBidiLTR
    pub fn IsBidiLTR(&self) -> bool {
        !self.IsBidiRTL()
    }

    /**
     * Return true if bidi direction is RTL.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::IsBidiRTL
    pub fn IsBidiRTL(&self) -> bool {
        !(self.mWritingMode & StyleWritingMode::RTL).is_empty()
    }

    /**
     * True if it is vertical and vertical-lr, or is horizontal and bidi LTR.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::IsPhysicalLTR
    pub fn IsPhysicalLTR(&self) -> bool {
        if self.IsVertical() {
            self.IsVerticalLR()
        } else {
            self.IsBidiLTR()
        }
    }

    /**
     * True if it is vertical and vertical-rl, or is horizontal and bidi RTL.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::IsPhysicalRTL
    pub fn IsPhysicalRTL(&self) -> bool {
        if self.IsVertical() {
            self.IsVerticalRL()
        } else {
            self.IsBidiRTL()
        }
    }

    /**
     * True if vertical-mode block direction is LR (convenience method).
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::IsVerticalLR
    pub fn IsVerticalLR(&self) -> bool {
        self.GetBlockDir() == WritingMode_BlockDir::LR
    }

    /**
     * True if vertical-mode block direction is RL (convenience method).
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::IsVerticalRL
    pub fn IsVerticalRL(&self) -> bool {
        self.GetBlockDir() == WritingMode_BlockDir::RL
    }

    /**
     * True if vertical writing mode, i.e. when
     * writing-mode: vertical-lr | vertical-rl | sideways-lr | sideways-rl.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::IsVertical
    pub fn IsVertical(&self) -> bool {
        !(self.mWritingMode & StyleWritingMode::VERTICAL).is_empty()
    }

    /**
     * True if line-over/line-under are inverted from block-start/block-end.
     * This is true only when writing-mode is vertical-lr.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::IsLineInverted
    pub fn IsLineInverted(&self) -> bool {
        !(self.mWritingMode & StyleWritingMode::LINE_INVERTED).is_empty()
    }

    /**
     * Block-axis flow-relative to line-relative factor.
     * May be used as a multiplication factor for block-axis coordinates
     * to convert between flow- and line-relative coordinate systems (e.g.
     * positioning an over- or under-line decoration).
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::FlowRelativeToLineRelativeFactor
    pub fn FlowRelativeToLineRelativeFactor(&self) -> i32 {
        if self.IsLineInverted() { -1 } else { 1 }
    }

    /**
     * True if vertical sideways writing mode, i.e. when
     * writing-mode: sideways-lr | sideways-rl.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::IsVerticalSideways
    pub fn IsVerticalSideways(&self) -> bool {
        !(self.mWritingMode & StyleWritingMode::VERTICAL_SIDEWAYS).is_empty()
    }

    // Gecko: layout/generic/WritingModes.h WritingMode::IsUpright
    pub fn IsUpright(&self) -> bool {
        !(self.mWritingMode & StyleWritingMode::UPRIGHT).is_empty()
    }

    /**
     * True if this is writing-mode: sideways-rl (convenience method).
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::IsSidewaysRL
    pub fn IsSidewaysRL(&self) -> bool {
        self.IsVerticalRL() && self.IsVerticalSideways()
    }

    /**
     * True if this is writing-mode: sideways-lr (convenience method).
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::IsSidewaysLR
    pub fn IsSidewaysLR(&self) -> bool {
        self.IsVerticalLR() && self.IsVerticalSideways()
    }

    /**
     * True if either text-orientation or writing-mode will force all text to be
     * rendered sideways in vertical lines, in which case we should prefer an
     * alphabetic baseline; otherwise, the default is centered.
     *
     * Note that some glyph runs may be rendered sideways even if this is false,
     * due to text-orientation:mixed resolution, but in that case the dominant
     * baseline remains centered.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::IsSideways
    pub fn IsSideways(&self) -> bool {
        !(self.mWritingMode
            & (StyleWritingMode::VERTICAL_SIDEWAYS | StyleWritingMode::TEXT_SIDEWAYS))
            .is_empty()
    }

    // Used by CHECK_WRITING_MODE to compare modes without regard for the
    // StyleWritingMode::VERTICAL_SIDEWAYS or StyleWritingMode::TEXT_SIDEWAYS
    // flags.
    // Gecko: layout/generic/WritingModes.h WritingMode::IgnoreSideways
    #[cfg(debug_assertions)]
    pub fn IgnoreSideways(&self) -> WritingMode {
        WritingMode::new__uint8_t(
            self.mWritingMode.bits()
                & !(StyleWritingMode::VERTICAL_SIDEWAYS | StyleWritingMode::TEXT_SIDEWAYS)
                    .bits(),
        )
    }

    /**
     * Return true if boxes with this writing mode should use central baselines.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::IsCentralBaseline
    pub fn IsCentralBaseline(&self) -> bool {
        self.IsVertical() && !self.IsSideways()
    }

    /**
     * Return true if boxes with this writing mode should use alphabetical
     * baselines.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::IsAlphabeticalBaseline
    pub fn IsAlphabeticalBaseline(&self) -> bool {
        !self.IsCentralBaseline()
    }

    /**
     * Convert LogicalAxis to PhysicalAxis given the current writing mode.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::PhysicalAxis
    pub fn PhysicalAxis(&self, aAxis: LogicalAxis) -> PhysicalAxis {
        let isInline = aAxis == LogicalAxis::Inline;
        if isInline == self.IsVertical() {
            PhysicalAxis::Vertical
        } else {
            PhysicalAxis::Horizontal
        }
    }

    // Gecko: layout/generic/WritingModes.h WritingMode::PhysicalSideForBlockAxis
    pub fn PhysicalSideForBlockAxis(mut aWritingModeValue: u8, aEdge: LogicalEdge) -> Side {
        // indexes are StyleWritingModeProperty values, which are the same as these
        // two-bit values:
        //   bit 0 = the StyleWritingMode::VERTICAL value
        //   bit 1 = the StyleWritingMode::VERTICAL_LR value
        const kLogicalBlockSides: [[Side; 2]; 4] = [
            [eSideTop, eSideBottom], // horizontal-tb
            [eSideRight, eSideLeft], // vertical-rl
            [eSideBottom, eSideTop], // (horizontal-bt)
            [eSideLeft, eSideRight], // vertical-lr
        ];

        // Ignore the SidewaysMask bit of the writing-mode value, as this has no
        // effect on the side mappings.
        aWritingModeValue &= !kWritingModeSidewaysMask;

        // What's left of the writing-mode should be in the range 0-3:
        NS_ASSERTION!(aWritingModeValue < 4, "invalid aWritingModeValue value");

        kLogicalBlockSides[aWritingModeValue as usize][aEdge as u8 as usize]
    }

    // Gecko: layout/generic/WritingModes.h WritingMode::PhysicalSideForInlineAxis
    pub fn PhysicalSideForInlineAxis(&self, aEdge: LogicalEdge) -> Side {
        // indexes are four-bit values:
        //   bit 0 = the StyleWritingMode::VERTICAL value
        //   bit 1 = the StyleWritingMode::INLINE_REVERSED value
        //   bit 2 = the StyleWritingMode::VERTICAL_LR value
        //   bit 3 = the StyleWritingMode::LINE_INVERTED value
        // Not all of these combinations can actually be specified via CSS: there
        // is no horizontal-bt writing-mode, and no text-orientation value that
        // produces "inverted" text. (The former 'sideways-left' value, no longer
        // in the spec, would have produced this in vertical-rl mode.)
        const kLogicalInlineSides: [[Side; 2]; 16] = [
            [eSideLeft, eSideRight],  // horizontal-tb               ltr
            [eSideTop, eSideBottom],  // vertical-rl                 ltr
            [eSideRight, eSideLeft],  // horizontal-tb               rtl
            [eSideBottom, eSideTop],  // vertical-rl                 rtl
            [eSideRight, eSideLeft],  // (horizontal-bt)  (inverted) ltr
            [eSideTop, eSideBottom],  // sideways-lr                 rtl
            [eSideLeft, eSideRight],  // (horizontal-bt)  (inverted) rtl
            [eSideBottom, eSideTop],  // sideways-lr                 ltr
            [eSideLeft, eSideRight],  // horizontal-tb    (inverted) rtl
            [eSideTop, eSideBottom],  // vertical-rl      (inverted) rtl
            [eSideRight, eSideLeft],  // horizontal-tb    (inverted) ltr
            [eSideBottom, eSideTop],  // vertical-rl      (inverted) ltr
            [eSideLeft, eSideRight],  // (horizontal-bt)             ltr
            [eSideTop, eSideBottom],  // vertical-lr                 ltr
            [eSideRight, eSideLeft],  // (horizontal-bt)             rtl
            [eSideBottom, eSideTop],  // vertical-lr                 rtl
        ];

        // Inline axis sides depend on all three of writing-mode, text-orientation
        // and direction, which are encoded in the StyleWritingMode::VERTICAL,
        // StyleWritingMode::INLINE_REVERSED, StyleWritingMode::VERTICAL_LR and
        // StyleWritingMode::LINE_INVERTED bits.  Use these four bits to index into
        // kLogicalInlineSides.
        const _: () = assert!(
            StyleWritingMode::VERTICAL.bits() == 0x01
                && StyleWritingMode::INLINE_REVERSED.bits() == 0x02
                && StyleWritingMode::VERTICAL_LR.bits() == 0x04
                && StyleWritingMode::LINE_INVERTED.bits() == 0x08,
            "Unexpected values for StyleWritingMode constants!"
        );
        let index: u8 = self.mWritingMode.bits() & 0x0F;
        kLogicalInlineSides[index as usize][aEdge as u8 as usize]
    }

    /**
     * Returns the physical side corresponding to the specified logical side,
     * given the current writing mode.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::PhysicalSide
    pub fn PhysicalSide(&self, aSide: LogicalSide) -> Side {
        if IsBlock(aSide) {
            const _: () = assert!(
                StyleWritingMode::VERTICAL.bits() == 0x01
                    && StyleWritingMode::VERTICAL_LR.bits() == 0x04,
                "Unexpected values for StyleWritingMode constants!"
            );
            let wm: u8 = ((self.mWritingMode & StyleWritingMode::VERTICAL_LR).bits() >> 1)
                | (self.mWritingMode & StyleWritingMode::VERTICAL).bits();
            return WritingMode::PhysicalSideForBlockAxis(wm, GetEdge(aSide));
        }

        self.PhysicalSideForInlineAxis(GetEdge(aSide))
    }

    /**
     * Returns the logical side corresponding to the specified physical side,
     * given the current writing mode.
     * (This is the inverse of the PhysicalSide() method above.)
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::LogicalSideForPhysicalSide
    pub fn LogicalSideForPhysicalSide(&self, aSide: Side) -> LogicalSide {
        use LogicalSide::*;
        // rustfmt::skip
        // indexes are four-bit values:
        //   bit 0 = the StyleWritingMode::VERTICAL value
        //   bit 1 = the StyleWritingMode::INLINE_REVERSED value
        //   bit 2 = the StyleWritingMode::VERTICAL_LR value
        //   bit 3 = the StyleWritingMode::LINE_INVERTED value
        const kPhysicalToLogicalSides: [[LogicalSide; 4]; 16] = [
            // top                right
            // bottom             left
            [BStart, IEnd, BEnd, IStart], // horizontal-tb         ltr
            [IStart, BStart, IEnd, BEnd], // vertical-rl           ltr
            [BStart, IStart, BEnd, IEnd], // horizontal-tb         rtl
            [IEnd, BStart, IStart, BEnd], // vertical-rl           rtl
            [BEnd, IStart, BStart, IEnd], // (horizontal-bt) (inv) ltr
            [IStart, BEnd, IEnd, BStart], // vertical-lr   sw-left rtl
            [BEnd, IEnd, BStart, IStart], // (horizontal-bt) (inv) rtl
            [IEnd, BEnd, IStart, BStart], // vertical-lr   sw-left ltr
            [BStart, IEnd, BEnd, IStart], // horizontal-tb   (inv) rtl
            [IStart, BStart, IEnd, BEnd], // vertical-rl   sw-left rtl
            [BStart, IStart, BEnd, IEnd], // horizontal-tb   (inv) ltr
            [IEnd, BStart, IStart, BEnd], // vertical-rl   sw-left ltr
            [BEnd, IEnd, BStart, IStart], // (horizontal-bt)       ltr
            [IStart, BEnd, IEnd, BStart], // vertical-lr           ltr
            [BEnd, IStart, BStart, IEnd], // (horizontal-bt)       rtl
            [IEnd, BEnd, IStart, BStart], // vertical-lr           rtl
        ];

        const _: () = assert!(
            StyleWritingMode::VERTICAL.bits() == 0x01
                && StyleWritingMode::INLINE_REVERSED.bits() == 0x02
                && StyleWritingMode::VERTICAL_LR.bits() == 0x04
                && StyleWritingMode::LINE_INVERTED.bits() == 0x08,
            "Unexpected values for StyleWritingMode constants!"
        );
        let index: u8 = self.mWritingMode.bits() & 0x0F;
        kPhysicalToLogicalSides[index as usize][aSide as usize]
    }

    /**
     * Returns the logical side corresponding to the specified
     * line-relative direction, given the current writing mode.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::LogicalSideForLineRelativeDir
    pub fn LogicalSideForLineRelativeDir(&self, aDir: LineRelativeDir) -> LogicalSide {
        // static_cast<LogicalSide>(aDir): LineRelativeDir's values are LogicalSide's.
        let side = match aDir {
            LineRelativeDir::Over => LogicalSide::BStart,
            LineRelativeDir::Under => LogicalSide::BEnd,
            LineRelativeDir::Left => LogicalSide::IStart,
            LineRelativeDir::Right => LogicalSide::IEnd,
        };
        if IsInline(side) {
            return if self.IsBidiLTR() {
                side
            } else {
                GetOppositeSide(side)
            };
        }
        if !self.IsLineInverted() {
            side
        } else {
            GetOppositeSide(side)
        }
    }

    /**
     * Construct a default WritingMode, equivalent to specifying
     * 'writing-mode: horizontal-tb' and 'direction: ltr' in CSS.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::WritingMode()
    pub const fn new() -> WritingMode {
        WritingMode {
            mWritingMode: StyleWritingMode::empty(),
        }
    }

    /**
     * Construct writing mode based on a ComputedStyle.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::WritingMode(const ComputedStyle*)
    //
    // Mapping to Stylo: Gecko's `ComputedStyle::WritingMode()` returns the `writing_mode` field
    // of the Rust `ComputedValues`. In the standalone stylo crate that field is the same
    // `logical_geometry::WritingMode` bit set (computed by `WritingMode::new(&InheritedBox)`
    // from `get_inherited_box().clone_direction()`, `.clone_writing_mode()` and, with stylo's
    // `gecko` feature only, `.clone_text_orientation()`), so we read the field instead of
    // recomputing it. Gecko's StyleDirection / StyleWritingModeProperty / StyleTextOrientation
    // are stylo's `direction::computed_value::T`, `logical_geometry::WritingModeProperty`
    // and `text_orientation::computed_value::T`. The mrt-style build does not enable stylo's
    // `gecko` feature, so sideways-*, text-orientation and therefore the UPRIGHT and
    // TEXT_SIDEWAYS / VERTICAL_SIDEWAYS bits are never set by styles (vertical-rl/lr, rtl and
    // the line-inverted bit are).
    pub fn new__ComputedStyle(aComputedStyle: &ComputedStyle) -> WritingMode {
        // NS_ASSERTION(aComputedStyle, "we need an ComputedStyle here"): a Rust reference is
        // never null.
        WritingMode {
            mWritingMode: aComputedStyle.writing_mode,
        }
    }

    // Gecko: layout/generic/WritingModes.h WritingMode::ToStyleWritingMode
    pub fn ToStyleWritingMode(&self) -> StyleWritingMode {
        StyleWritingMode::from_bits_retain(self.GetBits())
    }

    /**
     * This function performs fixup for elements with 'unicode-bidi: plaintext',
     * where inline directionality is derived from the Unicode bidi categories
     * of the element's content, and not the CSS 'direction' property.
     *
     * The WritingMode constructor will have already incorporated the 'direction'
     * property into our flag bits, so such elements need to use this method
     * (after resolving the bidi level of their content) to update the direction
     * bits as needed.
     *
     * If it turns out that our bidi direction already matches what plaintext
     * resolution determined, there's nothing to do here. If it didn't (i.e. if
     * the rtl-ness doesn't match), then we correct the direction by flipping the
     * same bits that get flipped in the constructor's CSS 'direction'-based
     * chunk.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::SetDirectionFromBidiLevel
    pub fn SetDirectionFromBidiLevel(&mut self, level: BidiEmbeddingLevel) {
        if level.IsRTL() == self.IsBidiLTR() {
            self.mWritingMode ^= StyleWritingMode::RTL | StyleWritingMode::INLINE_REVERSED;
        }
    }

    // Compare two WritingModes for equality.
    // (operator== / operator!= = default: `#[derive(PartialEq)]` on the struct.)

    /**
     * Check whether two modes are orthogonal to each other.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::IsOrthogonalTo
    pub fn IsOrthogonalTo(&self, aOther: &WritingMode) -> bool {
        self.IsVertical() != aOther.IsVertical()
    }

    /**
     * Convert aAxis in current writing mode to the axis in aToMode.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::ConvertAxisTo
    pub fn ConvertAxisTo(&self, aAxis: LogicalAxis, aToMode: WritingMode) -> LogicalAxis {
        if self.IsOrthogonalTo(&aToMode) {
            GetOrthogonalAxis(aAxis)
        } else {
            aAxis
        }
    }

    /**
     * Returns true if this WritingMode's aLogicalAxis has the same physical
     * start side as the parallel axis of WritingMode |aOther|.
     *
     * @param aLogicalAxis The axis to compare from this WritingMode.
     * @param aOther The other WritingMode (from which we'll choose the axis
     *               that's parallel to this WritingMode's aLogicalAxis, for
     *               comparison).
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::ParallelAxisStartsOnSameSide
    pub fn ParallelAxisStartsOnSameSide(&self, aLogicalAxis: LogicalAxis, aOther: &WritingMode) -> bool {
        if *self == *aOther {
            // Dedicated short circuit for the common case.
            return true;
        }

        let myStartSide: Side =
            self.PhysicalSide(MakeLogicalSide(aLogicalAxis, LogicalEdge::Start));

        // Figure out which of aOther's axes is parallel to |this| WritingMode's
        // aLogicalAxis, and get its physical start side as well.
        let otherWMAxis: LogicalAxis = self.ConvertAxisTo(aLogicalAxis, *aOther);
        let otherWMStartSide: Side =
            aOther.PhysicalSide(MakeLogicalSide(otherWMAxis, LogicalEdge::Start));

        NS_ASSERTION!(
            (myStartSide as u8) % 2 == (otherWMStartSide as u8) % 2,
            "Should end up with sides in the same physical axis"
        );
        myStartSide as u8 == otherWMStartSide as u8
    }

    /**
     * Determine a writing mode for determining the line-over/line-under side of a
     * box to use for baseline alignment, based on the container and item's
     * writing modes and the alignment axis.
     *
     * See: https://drafts.csswg.org/css-align-3/#baseline-export
     *
     * @param aContainerWM writing mode of the container (alignment context).
     * @param aItemWM writing mode of the item being aligned.
     * @param aAlignmentAxis axis of alignment, in the container’s writing mode.
     *        LogicalAxis::Inline (rows) for align-{items,self}:[last] baseline.
     *        LogicalAxis::Block (columns) for justify-{items,self}:[last]
     *        baseline.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::DetermineWritingModeForBaselineSynthesis
    pub fn DetermineWritingModeForBaselineSynthesis(
        aContainerWM: &WritingMode,
        aItemWM: &WritingMode,
        aAlignmentAxis: LogicalAxis,
    ) -> WritingMode {
        // Use the physical alignment axis for comparison:
        let physicalAlignmentAxis = aContainerWM.PhysicalAxis(aAlignmentAxis);

        // If the item's block flow direction is orthogonal to the alignment axis,
        // use its writing mode.
        let itemAxis = aItemWM.PhysicalAxis(LogicalAxis::Block);
        if itemAxis != physicalAlignmentAxis {
            return *aItemWM;
        }

        // If the container's block flow direction is orthogonal to the alignment
        // axis, use its writing mode.
        let containerAxis = aContainerWM.PhysicalAxis(LogicalAxis::Block);
        if containerAxis != physicalAlignmentAxis {
            return *aContainerWM;
        }

        // If not using the item or container writing mode, synthesize an
        // axis-compatible writing mode based on the container's writing mode.
        //
        // From https://drafts.csswg.org/css-align-3/#baseline-export:
        //
        // - If the box’s own writing mode is vertical, assume `horizontal-tb`.
        // - If the box’s own writing mode is horizontal, assume `vertical-lr` if
        //   direction is `ltr` and `vertical-rl` if direction is `rtl`.

        if aContainerWM.IsVertical() {
            return WritingMode::new__uint8_t(StyleWritingMode::WRITING_MODE_HORIZONTAL_TB.bits());
        }

        if aContainerWM.IsBidiLTR() {
            WritingMode::new__uint8_t(StyleWritingMode::WRITING_MODE_VERTICAL_LR.bits())
        } else {
            WritingMode::new__uint8_t(StyleWritingMode::WRITING_MODE_VERTICAL_RL.bits())
        }
    }

    // Gecko: layout/generic/WritingModes.h WritingMode::GetBits
    pub fn GetBits(&self) -> u8 {
        self.mWritingMode.bits()
    }

    // (private section; `friend` declarations: see accessor scheme point 8.)
    // NOT PORTED: `friend struct IPC::ParamTraits<WritingMode>` and
    // `friend struct widget::IMENotification` (IPC and IME are not in this runtime).

    /**
     * Unknown writing mode (should never actually be stored or used anywhere).
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::kUnknownWritingMode
    const kUnknownWritingMode: u8 = 0xff;

    /**
     * Return a WritingMode representing an unknown value.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::Unknown
    fn Unknown() -> WritingMode {
        WritingMode::new__uint8_t(WritingMode::kUnknownWritingMode)
    }

    /**
     * Constructing a WritingMode with an arbitrary value is a private operation.
     * This is currently only used by the Unknown() and IgnoreSideways() methods,
     * and a friend struct IMENotification.
     */
    // Gecko: layout/generic/WritingModes.h WritingMode::WritingMode(uint8_t)
    fn new__uint8_t(aValue: u8) -> WritingMode {
        WritingMode {
            mWritingMode: StyleWritingMode::from_bits_retain(aValue),
        }
    }
}

// Gecko: layout/generic/WritingModes.h mozilla::operator<<(std::ostream&, const WritingMode&)
impl fmt::Display for WritingMode {
    fn fmt(&self, aStream: &mut fmt::Formatter) -> fmt::Result {
        let aWM = self;
        aStream.write_str(if aWM.IsVertical() {
            if aWM.IsVerticalLR() {
                if aWM.IsBidiLTR() {
                    if aWM.IsSideways() { "sw-lr-ltr" } else { "v-lr-ltr" }
                } else if aWM.IsSideways() {
                    "sw-lr-rtl"
                } else {
                    "v-lr-rtl"
                }
            } else if aWM.IsBidiLTR() {
                if aWM.IsSideways() { "sw-rl-ltr" } else { "v-rl-ltr" }
            } else if aWM.IsSideways() {
                "sw-rl-rtl"
            } else {
                "v-rl-rtl"
            }
        } else if aWM.IsBidiLTR() {
            "h-ltr"
        } else {
            "h-rtl"
        })
    }
}

/**
 * Logical-coordinate classes:
 *
 * There are three sets of coordinate space:
 *   - physical (top, left, bottom, right)
 *       relative to graphics coord system
 *   - flow-relative (block-start, inline-start, block-end, inline-end)
 *       relative to block/inline flow directions
 *   - line-relative (line-over, line-left, line-under, line-right)
 *       relative to glyph orientation / inline bidi directions
 * See CSS3 Writing Modes for more information
 *   http://www.w3.org/TR/css3-writing-modes/#abstract-box
 *
 * For shorthand, B represents the block-axis
 *                I represents the inline-axis
 *
 * The flow-relative geometric classes store coords in flow-relative space.
 * They use a private ns{Point,Size,Rect,Margin} member to store the actual
 * coordinate values, but reinterpret them as logical instead of physical.
 * This allows us to easily perform calculations in logical space (provided
 * writing modes of the operands match), by simply mapping to nsPoint (etc)
 * methods.
 *
 * Physical-coordinate accessors/setters are responsible to translate these
 * internal logical values as necessary.
 *
 * In DEBUG builds, the logical types store their WritingMode and check
 * that the same WritingMode is passed whenever callers ask them to do a
 * writing-mode-dependent operation. Non-DEBUG builds do NOT check this,
 * to avoid the overhead of storing WritingMode fields.
 *
 * Open question: do we need a different set optimized for line-relative
 * math, for use in nsLineLayout and the like? Or is multiplying values
 * by FlowRelativeToLineRelativeFactor() enough?
 */

/**
 * Flow-relative point
 */
// Gecko: layout/generic/WritingModes.h mozilla::LogicalPoint
#[derive(Clone, Copy, Debug)]
pub struct LogicalPoint {
    #[cfg(debug_assertions)]
    mWritingMode: WritingMode,

    // We use an nsPoint to hold the coordinates, but reinterpret its .x and .y
    // fields as the inline and block directions. Hence, this is not exposed
    // directly, but only through accessors that will map them according to the
    // writing mode.
    mPoint: nsPoint,
}

impl LogicalPoint {
    // Gecko: layout/generic/WritingModes.h LogicalPoint::LogicalPoint(WritingMode)
    pub fn new__WritingMode(aWritingMode: WritingMode) -> LogicalPoint {
        LogicalPoint {
            #[cfg(debug_assertions)]
            mWritingMode: aWritingMode,
            mPoint: nsPoint::new(0, 0),
        }
    }

    // Construct from a writing mode and individual coordinates (which MUST be
    // values in that writing mode, NOT physical coordinates!)
    // Gecko: layout/generic/WritingModes.h LogicalPoint::LogicalPoint(WritingMode, nscoord, nscoord)
    pub fn new(aWritingMode: WritingMode, aI: nscoord, aB: nscoord) -> LogicalPoint {
        LogicalPoint {
            #[cfg(debug_assertions)]
            mWritingMode: aWritingMode,
            mPoint: nsPoint::new(aI, aB),
        }
    }

    // Construct from a writing mode and a physical point, within a given
    // containing rectangle's size (defining the conversion between LTR
    // and RTL coordinates, and between TTB and BTT coordinates).
    // Gecko: layout/generic/WritingModes.h LogicalPoint::LogicalPoint(WritingMode, const nsPoint&, const nsSize&)
    pub fn new__WritingMode_nsPoint_nsSize(
        aWritingMode: WritingMode,
        aPoint: nsPoint,
        aContainerSize: nsSize,
    ) -> LogicalPoint {
        let mut result = LogicalPoint {
            #[cfg(debug_assertions)]
            mWritingMode: aWritingMode,
            mPoint: nsPoint::new(0, 0),
        };
        if aWritingMode.IsVertical() {
            *result.I___mut() = if aWritingMode.IsInlineReversed() {
                aContainerSize.height - aPoint.y
            } else {
                aPoint.y
            };
            *result.B___mut() = if aWritingMode.IsVerticalLR() {
                aPoint.x
            } else {
                aContainerSize.width - aPoint.x
            };
        } else {
            *result.I___mut() = if aWritingMode.IsInlineReversed() {
                aContainerSize.width - aPoint.x
            } else {
                aPoint.x
            };
            *result.B___mut() = aPoint.y;
        }
        result
    }

    /**
     * Read-only (const) access to the logical coordinates.
     */
    // Gecko: layout/generic/WritingModes.h LogicalPoint::I
    pub fn I(&self, aWritingMode: WritingMode) -> nscoord // inline-axis
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        self.mPoint.x
    }
    // Gecko: layout/generic/WritingModes.h LogicalPoint::B
    pub fn B(&self, aWritingMode: WritingMode) -> nscoord // block-axis
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        self.mPoint.y
    }
    // Gecko: layout/generic/WritingModes.h LogicalPoint::Pos
    pub fn Pos(&self, aAxis: LogicalAxis, aWM: WritingMode) -> nscoord {
        if aAxis == LogicalAxis::Inline { self.I(aWM) } else { self.B(aWM) }
    }
    // Gecko: layout/generic/WritingModes.h LogicalPoint::LineRelative
    pub fn LineRelative(&self, aWritingMode: WritingMode, aContainerSize: nsSize) -> nscoord // line-axis
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsBidiLTR() {
            return self.I__();
        }
        (if aWritingMode.IsVertical() {
            aContainerSize.height
        } else {
            aContainerSize.width
        }) - self.I__()
    }

    /**
     * These non-const accessors return a reference (lvalue) that can be
     * assigned to by callers.
     */
    // Gecko: layout/generic/WritingModes.h LogicalPoint::I (non-const)
    pub fn I_mut(&mut self, aWritingMode: WritingMode) -> &mut nscoord // inline-axis
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        &mut self.mPoint.x
    }
    // Gecko: layout/generic/WritingModes.h LogicalPoint::B (non-const)
    pub fn B_mut(&mut self, aWritingMode: WritingMode) -> &mut nscoord // block-axis
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        &mut self.mPoint.y
    }
    // Gecko: layout/generic/WritingModes.h LogicalPoint::Pos (non-const)
    pub fn Pos_mut(&mut self, aAxis: LogicalAxis, aWM: WritingMode) -> &mut nscoord {
        if aAxis == LogicalAxis::Inline { self.I_mut(aWM) } else { self.B_mut(aWM) }
    }

    /**
     * Return a physical point corresponding to our logical coordinates,
     * converted according to our writing mode.
     */
    // Gecko: layout/generic/WritingModes.h LogicalPoint::GetPhysicalPoint
    pub fn GetPhysicalPoint(&self, aWritingMode: WritingMode, aContainerSize: nsSize) -> nsPoint {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsVertical() {
            nsPoint::new(
                if aWritingMode.IsVerticalLR() {
                    self.B__()
                } else {
                    aContainerSize.width - self.B__()
                },
                if aWritingMode.IsInlineReversed() {
                    aContainerSize.height - self.I__()
                } else {
                    self.I__()
                },
            )
        } else {
            nsPoint::new(
                if aWritingMode.IsInlineReversed() {
                    aContainerSize.width - self.I__()
                } else {
                    self.I__()
                },
                self.B__(),
            )
        }
    }

    /**
     * Return the equivalent point in a different writing mode.
     */
    // Gecko: layout/generic/WritingModes.h LogicalPoint::ConvertTo
    pub fn ConvertTo(
        &self,
        aToMode: WritingMode,
        aFromMode: WritingMode,
        aContainerSize: nsSize,
    ) -> LogicalPoint {
        CHECK_WRITING_MODE!(self, aFromMode);
        if aToMode == aFromMode {
            *self
        } else {
            LogicalPoint::new__WritingMode_nsPoint_nsSize(
                aToMode,
                self.GetPhysicalPoint(aFromMode, aContainerSize),
                aContainerSize,
            )
        }
    }

    /**
     * Considering 'this' LogicalPoint as the origin of some rect, as expressed
     * in writing mode aFromMode: this method returns the origin of that same
     * rect, as expressed in writing mode aToMode.
     *
     * The two points ('this' and the return value) may correspond to *different*
     * physical corners of the rect.  Each point is using its own writing-mode
     * to determine which corner is the origin.
     *
     * @param aToMode The writing mode to use for the return value.
     * @param aFromMode The writing mode for 'this' LogicalPoint.
     * @param aRectSize The physical size of the rectangle whose origin
     *                  is being requested.
     * @param aContainerSize The physical size of the container that defines the
     *                       local coordinate space.  (Our points' coordinates are
     *                       relative to the bounds of this container.)
     */
    // Gecko: layout/generic/WritingModes.h LogicalPoint::ConvertRectOriginTo
    pub fn ConvertRectOriginTo(
        &self,
        aToMode: WritingMode,
        aFromMode: WritingMode,
        aRectSize: nsSize,
        aContainerSize: nsSize,
    ) -> LogicalPoint {
        CHECK_WRITING_MODE!(self, aFromMode);
        if aFromMode == aToMode {
            return *self;
        }

        // Note: this might *look* like it's abusing ConvertTo(), since the last
        // param to ConvertTo() is named as if it's just a container-size rather
        // than the difference-in-sizes that we're passing here.  But this calling
        // pattern is actually correct for what we're trying to do here.
        //
        // Conceptually, ConvertTo()'s aContainerSize param just represents how
        // much extra space the container has *around* the converted thing in each
        // physical axis. When we're just converting a LogicalPoint with zero
        // thickness -- the way ConvertTo() expects to be called -- the extra space
        // around it in its container is simply the container size. But here, we're
        // converting the origin of a *rect*, and the rect has some
        // potentially-nonzero-thickness; so here, the extra space is the
        // container's size *minus* the rect's size.
        self.ConvertTo(aToMode, aFromMode, aContainerSize - aRectSize)
    }

    // (operator== / operator!=: PartialEq below; operator+ += - -=: std ops below.)

    // We don't allow construction of a LogicalPoint with no writing mode.
    // LogicalPoint() = delete;   (NOT IMPLEMENTED on purpose: no Default)

    /**
     * NOTE that in non-DEBUG builds, GetWritingMode() always returns
     * WritingMode::Unknown(), as the current mode is not stored in the logical-
     * geometry classes. Therefore, this method is private; it is used ONLY
     * by the DEBUG-mode checking macros in this class and its friends;
     * other code is not allowed to ask a logical point for its writing mode,
     * as this info will simply not be available in non-DEBUG builds.
     *
     * Also, in non-DEBUG builds, CHECK_WRITING_MODE does nothing, and the
     * WritingMode parameter to logical methods will generally be optimized
     * away altogether.
     */
    // Gecko: layout/generic/WritingModes.h LogicalPoint::GetWritingMode
    #[cfg(debug_assertions)]
    fn GetWritingMode(&self) -> WritingMode {
        self.mWritingMode
    }
    #[cfg(not(debug_assertions))]
    fn GetWritingMode(&self) -> WritingMode {
        WritingMode::Unknown()
    }

    // Accessors that don't take or check a WritingMode value.
    // These are for internal use only; they are called by methods that have
    // themselves already checked the WritingMode passed by the caller.
    // Gecko: layout/generic/WritingModes.h LogicalPoint::I() const
    fn I__(&self) -> nscoord // inline-axis
    {
        self.mPoint.x
    }
    // Gecko: layout/generic/WritingModes.h LogicalPoint::B() const
    fn B__(&self) -> nscoord // block-axis
    {
        self.mPoint.y
    }

    // Gecko: layout/generic/WritingModes.h LogicalPoint::I()
    fn I___mut(&mut self) -> &mut nscoord // inline-axis
    {
        &mut self.mPoint.x
    }
    // Gecko: layout/generic/WritingModes.h LogicalPoint::B()
    fn B___mut(&mut self) -> &mut nscoord // block-axis
    {
        &mut self.mPoint.y
    }
}

// Gecko: layout/generic/WritingModes.h LogicalPoint::operator==
impl PartialEq for LogicalPoint {
    fn eq(&self, aOther: &LogicalPoint) -> bool {
        CHECK_WRITING_MODE!(self, aOther.GetWritingMode());
        self.mPoint == aOther.mPoint
    }
}

// Gecko: layout/generic/WritingModes.h LogicalPoint::operator+
impl Add for LogicalPoint {
    type Output = LogicalPoint;
    fn add(self, aOther: LogicalPoint) -> LogicalPoint {
        CHECK_WRITING_MODE!(self, aOther.GetWritingMode());
        // In non-debug builds, LogicalPoint does not store the WritingMode,
        // so the first parameter here (which will always be WritingMode::Unknown())
        // is ignored.
        LogicalPoint::new(
            self.GetWritingMode(),
            self.mPoint.x + aOther.mPoint.x,
            self.mPoint.y + aOther.mPoint.y,
        )
    }
}

// Gecko: layout/generic/WritingModes.h LogicalPoint::operator+=
impl AddAssign for LogicalPoint {
    fn add_assign(&mut self, aOther: LogicalPoint) {
        CHECK_WRITING_MODE!(self, aOther.GetWritingMode());
        *self.I___mut() += aOther.I__();
        *self.B___mut() += aOther.B__();
    }
}

// Gecko: layout/generic/WritingModes.h LogicalPoint::operator-
impl Sub for LogicalPoint {
    type Output = LogicalPoint;
    fn sub(self, aOther: LogicalPoint) -> LogicalPoint {
        CHECK_WRITING_MODE!(self, aOther.GetWritingMode());
        // In non-debug builds, LogicalPoint does not store the WritingMode,
        // so the first parameter here (which will always be WritingMode::Unknown())
        // is ignored.
        LogicalPoint::new(
            self.GetWritingMode(),
            self.mPoint.x - aOther.mPoint.x,
            self.mPoint.y - aOther.mPoint.y,
        )
    }
}

// Gecko: layout/generic/WritingModes.h LogicalPoint::operator-=
impl SubAssign for LogicalPoint {
    fn sub_assign(&mut self, aOther: LogicalPoint) {
        CHECK_WRITING_MODE!(self, aOther.GetWritingMode());
        *self.I___mut() -= aOther.I__();
        *self.B___mut() -= aOther.B__();
    }
}

// Gecko: layout/generic/WritingModes.h operator<<(std::ostream&, const LogicalPoint&)
// (prints mPoint the way BasePoint's operator<< does: "(x,y)")
impl fmt::Display for LogicalPoint {
    fn fmt(&self, aStream: &mut fmt::Formatter) -> fmt::Result {
        write!(aStream, "({},{})", self.mPoint.x, self.mPoint.y)
    }
}

/**
 * Flow-relative size
 */
// Gecko: layout/generic/WritingModes.h mozilla::LogicalSize
#[derive(Clone, Copy, Debug)]
pub struct LogicalSize {
    #[cfg(debug_assertions)]
    mWritingMode: WritingMode,
    mSize: nsSize,
}

impl LogicalSize {
    // Gecko: layout/generic/WritingModes.h LogicalSize::LogicalSize(WritingMode)
    pub fn new__WritingMode(aWritingMode: WritingMode) -> LogicalSize {
        LogicalSize {
            #[cfg(debug_assertions)]
            mWritingMode: aWritingMode,
            mSize: nsSize::new(0, 0),
        }
    }

    // Gecko: layout/generic/WritingModes.h LogicalSize::LogicalSize(WritingMode, nscoord, nscoord)
    pub fn new(aWritingMode: WritingMode, aISize: nscoord, aBSize: nscoord) -> LogicalSize {
        LogicalSize {
            #[cfg(debug_assertions)]
            mWritingMode: aWritingMode,
            mSize: nsSize::new(aISize, aBSize),
        }
    }

    // Gecko: layout/generic/WritingModes.h LogicalSize::LogicalSize(WritingMode, const nsSize&)
    pub fn new__WritingMode_nsSize(aWritingMode: WritingMode, aPhysicalSize: nsSize) -> LogicalSize {
        let mut result = LogicalSize {
            #[cfg(debug_assertions)]
            mWritingMode: aWritingMode,
            mSize: nsSize::new(0, 0),
        };
        if aWritingMode.IsVertical() {
            *result.ISize___mut() = aPhysicalSize.height;
            *result.BSize___mut() = aPhysicalSize.width;
        } else {
            *result.ISize___mut() = aPhysicalSize.width;
            *result.BSize___mut() = aPhysicalSize.height;
        }
        result
    }

    // Gecko: layout/generic/WritingModes.h LogicalSize::SizeTo
    pub fn SizeTo(&mut self, aWritingMode: WritingMode, aISize: nscoord, aBSize: nscoord) {
        CHECK_WRITING_MODE!(self, aWritingMode);
        self.mSize.SizeTo(aISize, aBSize);
    }

    /**
     * Dimensions in logical and physical terms
     */
    // Gecko: layout/generic/WritingModes.h LogicalSize::ISize
    pub fn ISize(&self, aWritingMode: WritingMode) -> nscoord // inline-size
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        self.mSize.width
    }
    // Gecko: layout/generic/WritingModes.h LogicalSize::BSize
    pub fn BSize(&self, aWritingMode: WritingMode) -> nscoord // block-size
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        self.mSize.height
    }
    // Gecko: layout/generic/WritingModes.h LogicalSize::Size
    pub fn Size(&self, aAxis: LogicalAxis, aWM: WritingMode) -> nscoord {
        if aAxis == LogicalAxis::Inline { self.ISize(aWM) } else { self.BSize(aWM) }
    }

    // Gecko: layout/generic/WritingModes.h LogicalSize::Width
    pub fn Width(&self, aWritingMode: WritingMode) -> nscoord {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsVertical() { self.BSize__() } else { self.ISize__() }
    }
    // Gecko: layout/generic/WritingModes.h LogicalSize::Height
    pub fn Height(&self, aWritingMode: WritingMode) -> nscoord {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsVertical() { self.ISize__() } else { self.BSize__() }
    }

    /**
     * Writable references to the logical dimensions
     */
    // Gecko: layout/generic/WritingModes.h LogicalSize::ISize (non-const)
    pub fn ISize_mut(&mut self, aWritingMode: WritingMode) -> &mut nscoord // inline-size
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        &mut self.mSize.width
    }
    // Gecko: layout/generic/WritingModes.h LogicalSize::BSize (non-const)
    pub fn BSize_mut(&mut self, aWritingMode: WritingMode) -> &mut nscoord // block-size
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        &mut self.mSize.height
    }
    // Gecko: layout/generic/WritingModes.h LogicalSize::Size (non-const)
    pub fn Size_mut(&mut self, aAxis: LogicalAxis, aWM: WritingMode) -> &mut nscoord {
        if aAxis == LogicalAxis::Inline { self.ISize_mut(aWM) } else { self.BSize_mut(aWM) }
    }

    /**
     * Return an nsSize containing our physical dimensions
     */
    // Gecko: layout/generic/WritingModes.h LogicalSize::GetPhysicalSize
    pub fn GetPhysicalSize(&self, aWritingMode: WritingMode) -> nsSize {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsVertical() {
            nsSize::new(self.BSize__(), self.ISize__())
        } else {
            nsSize::new(self.ISize__(), self.BSize__())
        }
    }

    /**
     * Return a LogicalSize representing this size in a different writing mode
     */
    // Gecko: layout/generic/WritingModes.h LogicalSize::ConvertTo
    pub fn ConvertTo(&self, aToMode: WritingMode, aFromMode: WritingMode) -> LogicalSize {
        #[cfg(debug_assertions)]
        {
            // In DEBUG builds make sure to return a LogicalSize with the
            // expected writing mode
            CHECK_WRITING_MODE!(self, aFromMode);
            if aToMode == aFromMode {
                *self
            } else {
                LogicalSize::new__WritingMode_nsSize(aToMode, self.GetPhysicalSize(aFromMode))
            }
        }
        #[cfg(not(debug_assertions))]
        {
            // optimization for non-DEBUG builds where LogicalSize doesn't store
            // the writing mode
            if aToMode == aFromMode || !aToMode.IsOrthogonalTo(&aFromMode) {
                *self
            } else {
                LogicalSize::new(aToMode, self.BSize__(), self.ISize__())
            }
        }
    }

    /**
     * Test if a size is (0, 0).
     */
    // Gecko: layout/generic/WritingModes.h LogicalSize::IsAllZero
    pub fn IsAllZero(&self) -> bool {
        self.IsAllValues(0)
    }
    // Gecko: layout/generic/WritingModes.h LogicalSize::IsAllValues
    pub fn IsAllValues(&self, aValue: nscoord) -> bool {
        self.ISize__() == aValue && self.BSize__() == aValue
    }

    // Various binary operators on LogicalSize. These are valid ONLY for operands
    // that share the same writing mode.
    // (operator== / != / + / += / - / -=: PartialEq and std ops below.)

    // LogicalSize() = delete;   (NOT IMPLEMENTED on purpose: no Default)

    // Gecko: layout/generic/WritingModes.h LogicalSize::GetWritingMode
    #[cfg(debug_assertions)]
    fn GetWritingMode(&self) -> WritingMode {
        self.mWritingMode
    }
    #[cfg(not(debug_assertions))]
    fn GetWritingMode(&self) -> WritingMode {
        WritingMode::Unknown()
    }

    // Gecko: layout/generic/WritingModes.h LogicalSize::ISize() const
    fn ISize__(&self) -> nscoord // inline-size
    {
        self.mSize.width
    }
    // Gecko: layout/generic/WritingModes.h LogicalSize::BSize() const
    fn BSize__(&self) -> nscoord // block-size
    {
        self.mSize.height
    }

    // Gecko: layout/generic/WritingModes.h LogicalSize::ISize()
    fn ISize___mut(&mut self) -> &mut nscoord // inline-size
    {
        &mut self.mSize.width
    }
    // Gecko: layout/generic/WritingModes.h LogicalSize::BSize()
    fn BSize___mut(&mut self) -> &mut nscoord // block-size
    {
        &mut self.mSize.height
    }
}

// Gecko: layout/generic/WritingModes.h LogicalSize::operator==
impl PartialEq for LogicalSize {
    fn eq(&self, aOther: &LogicalSize) -> bool {
        CHECK_WRITING_MODE!(self, aOther.GetWritingMode());
        self.mSize == aOther.mSize
    }
}

// Gecko: layout/generic/WritingModes.h LogicalSize::operator+
impl Add for LogicalSize {
    type Output = LogicalSize;
    fn add(self, aOther: LogicalSize) -> LogicalSize {
        CHECK_WRITING_MODE!(self, aOther.GetWritingMode());
        LogicalSize::new(
            self.GetWritingMode(),
            self.ISize__() + aOther.ISize__(),
            self.BSize__() + aOther.BSize__(),
        )
    }
}
// Gecko: layout/generic/WritingModes.h LogicalSize::operator+=
impl AddAssign for LogicalSize {
    fn add_assign(&mut self, aOther: LogicalSize) {
        CHECK_WRITING_MODE!(self, aOther.GetWritingMode());
        *self.ISize___mut() += aOther.ISize__();
        *self.BSize___mut() += aOther.BSize__();
    }
}

// Gecko: layout/generic/WritingModes.h LogicalSize::operator-
impl Sub for LogicalSize {
    type Output = LogicalSize;
    fn sub(self, aOther: LogicalSize) -> LogicalSize {
        CHECK_WRITING_MODE!(self, aOther.GetWritingMode());
        LogicalSize::new(
            self.GetWritingMode(),
            self.ISize__() - aOther.ISize__(),
            self.BSize__() - aOther.BSize__(),
        )
    }
}
// Gecko: layout/generic/WritingModes.h LogicalSize::operator-=
impl SubAssign for LogicalSize {
    fn sub_assign(&mut self, aOther: LogicalSize) {
        CHECK_WRITING_MODE!(self, aOther.GetWritingMode());
        *self.ISize___mut() -= aOther.ISize__();
        *self.BSize___mut() -= aOther.BSize__();
    }
}

// Gecko: layout/generic/WritingModes.h operator<<(std::ostream&, const LogicalSize&)
// (prints mSize the way BaseSize's operator<< does: "(w x h)")
impl fmt::Display for LogicalSize {
    fn fmt(&self, aStream: &mut fmt::Formatter) -> fmt::Result {
        write!(aStream, "({} x {})", self.mSize.width, self.mSize.height)
    }
}

/**
 * LogicalSides represents a set of logical sides.
 */
// Gecko: layout/generic/WritingModes.h mozilla::LogicalSides
#[derive(Clone, Copy, Debug)]
pub struct LogicalSides {
    #[cfg(debug_assertions)]
    mWritingMode: WritingMode,
    mSides: EnumSet<LogicalSide>,
}

impl LogicalSides {
    // Gecko: layout/generic/WritingModes.h LogicalSides::BBoth
    pub const BBoth: EnumSet<LogicalSide> = EnumSet::from_mask(
        (1 << (LogicalSide::BStart as u32)) | (1 << (LogicalSide::BEnd as u32)),
    );
    // Gecko: layout/generic/WritingModes.h LogicalSides::IBoth
    pub const IBoth: EnumSet<LogicalSide> = EnumSet::from_mask(
        (1 << (LogicalSide::IStart as u32)) | (1 << (LogicalSide::IEnd as u32)),
    );
    // Gecko: layout/generic/WritingModes.h LogicalSides::All
    pub const All: EnumSet<LogicalSide> = EnumSet::from_mask(
        (1 << (LogicalSide::BStart as u32))
            | (1 << (LogicalSide::BEnd as u32))
            | (1 << (LogicalSide::IStart as u32))
            | (1 << (LogicalSide::IEnd as u32)),
    );

    // Gecko: layout/generic/WritingModes.h LogicalSides::LogicalSides(WritingMode)
    pub fn new__WritingMode(aWritingMode: WritingMode) -> LogicalSides {
        LogicalSides {
            #[cfg(debug_assertions)]
            mWritingMode: aWritingMode,
            mSides: EnumSet::new(),
        }
    }
    // Gecko: layout/generic/WritingModes.h LogicalSides::LogicalSides(WritingMode, LogicalSides)
    pub fn new__WritingMode_LogicalSides(aWritingMode: WritingMode, aSides: LogicalSides) -> LogicalSides {
        LogicalSides {
            #[cfg(debug_assertions)]
            mWritingMode: aWritingMode,
            mSides: aSides.mSides,
        }
    }
    // Gecko: layout/generic/WritingModes.h LogicalSides::LogicalSides(WritingMode, EnumSet<LogicalSide>)
    pub fn new__WritingMode_EnumSet(aWritingMode: WritingMode, aSides: EnumSet<LogicalSide>) -> LogicalSides {
        LogicalSides {
            #[cfg(debug_assertions)]
            mWritingMode: aWritingMode,
            mSides: aSides,
        }
    }
    // Gecko: layout/generic/WritingModes.h LogicalSides::IsEmpty
    pub fn IsEmpty(&self) -> bool {
        self.mSides.isEmpty()
    }
    // Gecko: layout/generic/WritingModes.h LogicalSides::BStart
    pub fn BStart(&self) -> bool {
        self.mSides.contains(LogicalSide::BStart)
    }
    // Gecko: layout/generic/WritingModes.h LogicalSides::BEnd
    pub fn BEnd(&self) -> bool {
        self.mSides.contains(LogicalSide::BEnd)
    }
    // Gecko: layout/generic/WritingModes.h LogicalSides::IStart
    pub fn IStart(&self) -> bool {
        self.mSides.contains(LogicalSide::IStart)
    }
    // Gecko: layout/generic/WritingModes.h LogicalSides::IEnd
    pub fn IEnd(&self) -> bool {
        self.mSides.contains(LogicalSide::IEnd)
    }
    // Gecko: layout/generic/WritingModes.h LogicalSides::Contains
    pub fn Contains(&self, aSide: LogicalSide) -> bool {
        self.mSides.contains(aSide)
    }
    // (operator+=(LogicalSides) and operator+=(LogicalSide): AddAssign impls below;
    //  operator== / !=: PartialEq below.)

    // Gecko: layout/generic/WritingModes.h LogicalSides::GetWritingMode
    #[cfg(debug_assertions)]
    pub fn GetWritingMode(&self) -> WritingMode {
        self.mWritingMode
    }
    #[cfg(not(debug_assertions))]
    pub fn GetWritingMode(&self) -> WritingMode {
        WritingMode::Unknown()
    }
}

// Gecko: layout/generic/WritingModes.h LogicalSides::operator+=(LogicalSides)
impl AddAssign<LogicalSides> for LogicalSides {
    fn add_assign(&mut self, aOther: LogicalSides) {
        self.mSides += aOther.mSides;
    }
}
// Gecko: layout/generic/WritingModes.h LogicalSides::operator+=(LogicalSide)
impl AddAssign<LogicalSide> for LogicalSides {
    fn add_assign(&mut self, aOther: LogicalSide) {
        self.mSides += aOther;
    }
}
// Gecko: layout/generic/WritingModes.h LogicalSides::operator==
impl PartialEq for LogicalSides {
    fn eq(&self, aOther: &LogicalSides) -> bool {
        CHECK_WRITING_MODE!(self, aOther.GetWritingMode());
        self.mSides == aOther.mSides
    }
}

/**
 * Flow-relative margin
 */
// Gecko: layout/generic/WritingModes.h mozilla::LogicalMargin
#[derive(Clone, Copy, Debug)]
pub struct LogicalMargin {
    #[cfg(debug_assertions)]
    mWritingMode: WritingMode,
    mMargin: nsMargin,
}

impl LogicalMargin {
    // Gecko: layout/generic/WritingModes.h LogicalMargin::LogicalMargin(WritingMode)
    pub fn new__WritingMode(aWritingMode: WritingMode) -> LogicalMargin {
        LogicalMargin {
            #[cfg(debug_assertions)]
            mWritingMode: aWritingMode,
            mMargin: nsMargin::new(0, 0, 0, 0),
        }
    }

    // Gecko: layout/generic/WritingModes.h LogicalMargin::LogicalMargin(WritingMode, nscoord x4)
    pub fn new(
        aWritingMode: WritingMode,
        aBStart: nscoord,
        aIEnd: nscoord,
        aBEnd: nscoord,
        aIStart: nscoord,
    ) -> LogicalMargin {
        LogicalMargin {
            #[cfg(debug_assertions)]
            mWritingMode: aWritingMode,
            mMargin: nsMargin::new(aBStart, aIEnd, aBEnd, aIStart),
        }
    }

    // Gecko: layout/generic/WritingModes.h LogicalMargin::LogicalMargin(WritingMode, const nsMargin&)
    pub fn new__WritingMode_nsMargin(aWritingMode: WritingMode, aPhysicalMargin: nsMargin) -> LogicalMargin {
        let mut result = LogicalMargin {
            #[cfg(debug_assertions)]
            mWritingMode: aWritingMode,
            mMargin: nsMargin::new(0, 0, 0, 0),
        };
        if aWritingMode.IsVertical() {
            if aWritingMode.IsVerticalLR() {
                result.mMargin.top = aPhysicalMargin.left;
                result.mMargin.bottom = aPhysicalMargin.right;
            } else {
                result.mMargin.top = aPhysicalMargin.right;
                result.mMargin.bottom = aPhysicalMargin.left;
            }
            if aWritingMode.IsInlineReversed() {
                result.mMargin.left = aPhysicalMargin.bottom;
                result.mMargin.right = aPhysicalMargin.top;
            } else {
                result.mMargin.left = aPhysicalMargin.top;
                result.mMargin.right = aPhysicalMargin.bottom;
            }
        } else {
            result.mMargin.top = aPhysicalMargin.top;
            result.mMargin.bottom = aPhysicalMargin.bottom;
            if aWritingMode.IsInlineReversed() {
                result.mMargin.left = aPhysicalMargin.right;
                result.mMargin.right = aPhysicalMargin.left;
            } else {
                result.mMargin.left = aPhysicalMargin.left;
                result.mMargin.right = aPhysicalMargin.right;
            }
        }
        result
    }

    // Gecko: layout/generic/WritingModes.h LogicalMargin::IStart
    pub fn IStart(&self, aWritingMode: WritingMode) -> nscoord // inline-start margin
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        self.mMargin.left
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::IEnd
    pub fn IEnd(&self, aWritingMode: WritingMode) -> nscoord // inline-end margin
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        self.mMargin.right
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::BStart
    pub fn BStart(&self, aWritingMode: WritingMode) -> nscoord // block-start margin
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        self.mMargin.top
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::BEnd
    pub fn BEnd(&self, aWritingMode: WritingMode) -> nscoord // block-end margin
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        self.mMargin.bottom
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::Start
    pub fn Start(&self, aAxis: LogicalAxis, aWM: WritingMode) -> nscoord {
        if aAxis == LogicalAxis::Inline { self.IStart(aWM) } else { self.BStart(aWM) }
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::End
    pub fn End(&self, aAxis: LogicalAxis, aWM: WritingMode) -> nscoord {
        if aAxis == LogicalAxis::Inline { self.IEnd(aWM) } else { self.BEnd(aWM) }
    }

    // Gecko: layout/generic/WritingModes.h LogicalMargin::IStart (non-const)
    pub fn IStart_mut(&mut self, aWritingMode: WritingMode) -> &mut nscoord // inline-start margin
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        &mut self.mMargin.left
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::IEnd (non-const)
    pub fn IEnd_mut(&mut self, aWritingMode: WritingMode) -> &mut nscoord // inline-end margin
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        &mut self.mMargin.right
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::BStart (non-const)
    pub fn BStart_mut(&mut self, aWritingMode: WritingMode) -> &mut nscoord // block-start margin
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        &mut self.mMargin.top
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::BEnd (non-const)
    pub fn BEnd_mut(&mut self, aWritingMode: WritingMode) -> &mut nscoord // block-end margin
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        &mut self.mMargin.bottom
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::Start (non-const)
    pub fn Start_mut(&mut self, aAxis: LogicalAxis, aWM: WritingMode) -> &mut nscoord {
        if aAxis == LogicalAxis::Inline { self.IStart_mut(aWM) } else { self.BStart_mut(aWM) }
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::End (non-const)
    pub fn End_mut(&mut self, aAxis: LogicalAxis, aWM: WritingMode) -> &mut nscoord {
        if aAxis == LogicalAxis::Inline { self.IEnd_mut(aWM) } else { self.BEnd_mut(aWM) }
    }

    // Gecko: layout/generic/WritingModes.h LogicalMargin::IStartEnd
    pub fn IStartEnd(&self, aWritingMode: WritingMode) -> nscoord // inline margins
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        self.mMargin.LeftRight()
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::BStartEnd
    pub fn BStartEnd(&self, aWritingMode: WritingMode) -> nscoord // block margins
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        self.mMargin.TopBottom()
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::StartEnd
    pub fn StartEnd(&self, aAxis: LogicalAxis, aWM: WritingMode) -> nscoord {
        if aAxis == LogicalAxis::Inline { self.IStartEnd(aWM) } else { self.BStartEnd(aWM) }
    }

    // Gecko: layout/generic/WritingModes.h LogicalMargin::Side
    pub fn Side(&self, aSide: LogicalSide, aWM: WritingMode) -> nscoord {
        match aSide {
            LogicalSide::BStart => self.BStart(aWM),
            LogicalSide::BEnd => self.BEnd(aWM),
            LogicalSide::IStart => self.IStart(aWM),
            LogicalSide::IEnd => self.IEnd(aWM),
        }

        // MOZ_ASSERT_UNREACHABLE("We should handle all sides!");
        // return BStart(aWM);
        // (Rust: the match above is exhaustive, so this code cannot be reached.)
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::Side (non-const)
    pub fn Side_mut(&mut self, aSide: LogicalSide, aWM: WritingMode) -> &mut nscoord {
        match aSide {
            LogicalSide::BStart => self.BStart_mut(aWM),
            LogicalSide::BEnd => self.BEnd_mut(aWM),
            LogicalSide::IStart => self.IStart_mut(aWM),
            LogicalSide::IEnd => self.IEnd_mut(aWM),
        }

        // MOZ_ASSERT_UNREACHABLE("We should handle all sides!");
        // return BStart(aWM);
        // (Rust: the match above is exhaustive, so this code cannot be reached.)
    }

    /*
     * Return margin values for line-relative sides, as defined in
     * http://www.w3.org/TR/css-writing-modes-3/#line-directions:
     *
     * line-left
     *     Nominally the side from which LTR text would start.
     * line-right
     *     Nominally the side from which RTL text would start. (Opposite of
     *     line-left.)
     */
    // Gecko: layout/generic/WritingModes.h LogicalMargin::LineLeft
    pub fn LineLeft(&self, aWritingMode: WritingMode) -> nscoord {
        // We don't need to CHECK_WRITING_MODE here because the IStart or IEnd
        // accessor that we call will do it.
        if aWritingMode.IsBidiLTR() {
            self.IStart(aWritingMode)
        } else {
            self.IEnd(aWritingMode)
        }
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::LineRight
    pub fn LineRight(&self, aWritingMode: WritingMode) -> nscoord {
        if aWritingMode.IsBidiLTR() {
            self.IEnd(aWritingMode)
        } else {
            self.IStart(aWritingMode)
        }
    }

    /**
     * Return a LogicalSize representing the total size of the inline-
     * and block-dimension margins.
     */
    // Gecko: layout/generic/WritingModes.h LogicalMargin::Size
    pub fn Size(&self, aWritingMode: WritingMode) -> LogicalSize {
        CHECK_WRITING_MODE!(self, aWritingMode);
        LogicalSize::new(aWritingMode, self.IStartEnd__(), self.BStartEnd__())
    }

    /**
     * Return a LogicalPoint representing an offset to the start-sides, i.e.
     * inline-start and block-start.
     */
    // Gecko: layout/generic/WritingModes.h LogicalMargin::StartOffset
    pub fn StartOffset(&self, aWritingMode: WritingMode) -> LogicalPoint {
        CHECK_WRITING_MODE!(self, aWritingMode);
        LogicalPoint::new(aWritingMode, self.IStart__(), self.BStart__())
    }

    /**
     * Accessors for physical margins, using our writing mode to convert from
     * logical values.
     */
    // Gecko: layout/generic/WritingModes.h LogicalMargin::Top
    pub fn Top(&self, aWritingMode: WritingMode) -> nscoord {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsVertical() {
            if aWritingMode.IsInlineReversed() { self.IEnd__() } else { self.IStart__() }
        } else {
            self.BStart__()
        }
    }

    // Gecko: layout/generic/WritingModes.h LogicalMargin::Bottom
    pub fn Bottom(&self, aWritingMode: WritingMode) -> nscoord {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsVertical() {
            if aWritingMode.IsInlineReversed() { self.IStart__() } else { self.IEnd__() }
        } else {
            self.BEnd__()
        }
    }

    // Gecko: layout/generic/WritingModes.h LogicalMargin::Left
    pub fn Left(&self, aWritingMode: WritingMode) -> nscoord {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsVertical() {
            if aWritingMode.IsVerticalLR() { self.BStart__() } else { self.BEnd__() }
        } else if aWritingMode.IsInlineReversed() {
            self.IEnd__()
        } else {
            self.IStart__()
        }
    }

    // Gecko: layout/generic/WritingModes.h LogicalMargin::Right
    pub fn Right(&self, aWritingMode: WritingMode) -> nscoord {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsVertical() {
            if aWritingMode.IsVerticalLR() { self.BEnd__() } else { self.BStart__() }
        } else if aWritingMode.IsInlineReversed() {
            self.IStart__()
        } else {
            self.IEnd__()
        }
    }

    // Gecko: layout/generic/WritingModes.h LogicalMargin::LeftRight
    pub fn LeftRight(&self, aWritingMode: WritingMode) -> nscoord {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsVertical() { self.BStartEnd__() } else { self.IStartEnd__() }
    }

    // Gecko: layout/generic/WritingModes.h LogicalMargin::TopBottom
    pub fn TopBottom(&self, aWritingMode: WritingMode) -> nscoord {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsVertical() { self.IStartEnd__() } else { self.BStartEnd__() }
    }

    // Gecko: layout/generic/WritingModes.h LogicalMargin::SizeTo
    pub fn SizeTo(
        &mut self,
        aWritingMode: WritingMode,
        aBStart: nscoord,
        aIEnd: nscoord,
        aBEnd: nscoord,
        aIStart: nscoord,
    ) {
        CHECK_WRITING_MODE!(self, aWritingMode);
        self.mMargin.SizeTo(aBStart, aIEnd, aBEnd, aIStart);
    }

    /**
     * Return an nsMargin containing our physical coordinates
     */
    // Gecko: layout/generic/WritingModes.h LogicalMargin::GetPhysicalMargin
    pub fn GetPhysicalMargin(&self, aWritingMode: WritingMode) -> nsMargin {
        CHECK_WRITING_MODE!(self, aWritingMode);
        let (IStart, IEnd, BStart, BEnd) =
            (self.IStart__(), self.IEnd__(), self.BStart__(), self.BEnd__());
        if aWritingMode.IsVertical() {
            if aWritingMode.IsVerticalLR() {
                if aWritingMode.IsInlineReversed() {
                    nsMargin::new(IEnd, BEnd, IStart, BStart)
                } else {
                    nsMargin::new(IStart, BEnd, IEnd, BStart)
                }
            } else if aWritingMode.IsInlineReversed() {
                nsMargin::new(IEnd, BStart, IStart, BEnd)
            } else {
                nsMargin::new(IStart, BStart, IEnd, BEnd)
            }
        } else if aWritingMode.IsInlineReversed() {
            nsMargin::new(BStart, IStart, BEnd, IEnd)
        } else {
            nsMargin::new(BStart, IEnd, BEnd, IStart)
        }
    }

    /**
     * Return a LogicalMargin representing this margin in a different
     * writing mode
     */
    // Gecko: layout/generic/WritingModes.h LogicalMargin::ConvertTo
    pub fn ConvertTo(&self, aToMode: WritingMode, aFromMode: WritingMode) -> LogicalMargin {
        CHECK_WRITING_MODE!(self, aFromMode);
        if aToMode == aFromMode {
            *self
        } else {
            LogicalMargin::new__WritingMode_nsMargin(aToMode, self.GetPhysicalMargin(aFromMode))
        }
    }

    // Gecko: layout/generic/WritingModes.h LogicalMargin::ApplySkipSides
    pub fn ApplySkipSides(&mut self, aSkipSides: LogicalSides) -> &mut LogicalMargin {
        CHECK_WRITING_MODE!(self, aSkipSides.GetWritingMode());
        if aSkipSides.BStart() {
            *self.BStart___mut() = 0;
        }
        if aSkipSides.BEnd() {
            *self.BEnd___mut() = 0;
        }
        if aSkipSides.IStart() {
            *self.IStart___mut() = 0;
        }
        if aSkipSides.IEnd() {
            *self.IEnd___mut() = 0;
        }
        self
    }

    // Gecko: layout/generic/WritingModes.h LogicalMargin::IsAllZero
    pub fn IsAllZero(&self) -> bool {
        self.mMargin.IsAllZero()
    }

    // (operator== / != / + / += / -: PartialEq and std ops below.)

    // LogicalMargin() = delete;   (NOT IMPLEMENTED on purpose: no Default)

    // Gecko: layout/generic/WritingModes.h LogicalMargin::GetWritingMode
    #[cfg(debug_assertions)]
    fn GetWritingMode(&self) -> WritingMode {
        self.mWritingMode
    }
    #[cfg(not(debug_assertions))]
    fn GetWritingMode(&self) -> WritingMode {
        WritingMode::Unknown()
    }

    // Gecko: layout/generic/WritingModes.h LogicalMargin::IStart() const
    fn IStart__(&self) -> nscoord // inline-start margin
    {
        self.mMargin.left
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::IEnd() const
    fn IEnd__(&self) -> nscoord // inline-end margin
    {
        self.mMargin.right
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::BStart() const
    fn BStart__(&self) -> nscoord // block-start margin
    {
        self.mMargin.top
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::BEnd() const
    fn BEnd__(&self) -> nscoord // block-end margin
    {
        self.mMargin.bottom
    }

    // Gecko: layout/generic/WritingModes.h LogicalMargin::IStart()
    fn IStart___mut(&mut self) -> &mut nscoord // inline-start margin
    {
        &mut self.mMargin.left
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::IEnd()
    fn IEnd___mut(&mut self) -> &mut nscoord // inline-end margin
    {
        &mut self.mMargin.right
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::BStart()
    fn BStart___mut(&mut self) -> &mut nscoord // block-start margin
    {
        &mut self.mMargin.top
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::BEnd()
    fn BEnd___mut(&mut self) -> &mut nscoord // block-end margin
    {
        &mut self.mMargin.bottom
    }

    // Gecko: layout/generic/WritingModes.h LogicalMargin::IStartEnd() const
    fn IStartEnd__(&self) -> nscoord // inline margins
    {
        self.mMargin.LeftRight()
    }
    // Gecko: layout/generic/WritingModes.h LogicalMargin::BStartEnd() const
    fn BStartEnd__(&self) -> nscoord // block margins
    {
        self.mMargin.TopBottom()
    }
}

// Gecko: layout/generic/WritingModes.h LogicalMargin::operator==
impl PartialEq for LogicalMargin {
    fn eq(&self, aMargin: &LogicalMargin) -> bool {
        CHECK_WRITING_MODE!(self, aMargin.GetWritingMode());
        self.mMargin == aMargin.mMargin
    }
}

// Gecko: layout/generic/WritingModes.h LogicalMargin::operator+
impl Add for LogicalMargin {
    type Output = LogicalMargin;
    fn add(self, aMargin: LogicalMargin) -> LogicalMargin {
        CHECK_WRITING_MODE!(self, aMargin.GetWritingMode());
        LogicalMargin::new(
            self.GetWritingMode(),
            self.BStart__() + aMargin.BStart__(),
            self.IEnd__() + aMargin.IEnd__(),
            self.BEnd__() + aMargin.BEnd__(),
            self.IStart__() + aMargin.IStart__(),
        )
    }
}

// Gecko: layout/generic/WritingModes.h LogicalMargin::operator+=
impl AddAssign for LogicalMargin {
    fn add_assign(&mut self, aMargin: LogicalMargin) {
        CHECK_WRITING_MODE!(self, aMargin.GetWritingMode());
        self.mMargin += aMargin.mMargin;
    }
}

// Gecko: layout/generic/WritingModes.h LogicalMargin::operator-
impl Sub for LogicalMargin {
    type Output = LogicalMargin;
    fn sub(self, aMargin: LogicalMargin) -> LogicalMargin {
        CHECK_WRITING_MODE!(self, aMargin.GetWritingMode());
        LogicalMargin::new(
            self.GetWritingMode(),
            self.BStart__() - aMargin.BStart__(),
            self.IEnd__() - aMargin.IEnd__(),
            self.BEnd__() - aMargin.BEnd__(),
            self.IStart__() - aMargin.IStart__(),
        )
    }
}

// Gecko: layout/generic/WritingModes.h operator<<(std::ostream&, const LogicalMargin&)
// (prints mMargin the way BaseMargin's operator<< does)
impl fmt::Display for LogicalMargin {
    fn fmt(&self, aStream: &mut fmt::Formatter) -> fmt::Result {
        write!(
            aStream,
            "(t={}, r={}, b={}, l={})",
            self.mMargin.top, self.mMargin.right, self.mMargin.bottom, self.mMargin.left
        )
    }
}

/**
 * Flow-relative rectangle
 */
// Gecko: layout/generic/WritingModes.h mozilla::LogicalRect
#[derive(Clone, Copy, Debug)]
pub struct LogicalRect {
    #[cfg(debug_assertions)]
    mWritingMode: WritingMode,
    // Inline- and block-geometry dimension
    mIStart: nscoord, // inline-start edge
    mBStart: nscoord, // block-start edge
    mISize: nscoord,  // inline-size
    mBSize: nscoord,  // block-size
}

impl LogicalRect {
    // Gecko: layout/generic/WritingModes.h LogicalRect::LogicalRect(WritingMode)
    pub fn new__WritingMode(aWritingMode: WritingMode) -> LogicalRect {
        LogicalRect {
            #[cfg(debug_assertions)]
            mWritingMode: aWritingMode,
            mIStart: 0,
            mBStart: 0,
            mISize: 0,
            mBSize: 0,
        }
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::LogicalRect(WritingMode, nscoord x4)
    pub fn new(
        aWritingMode: WritingMode,
        aIStart: nscoord,
        aBStart: nscoord,
        aISize: nscoord,
        aBSize: nscoord,
    ) -> LogicalRect {
        LogicalRect {
            #[cfg(debug_assertions)]
            mWritingMode: aWritingMode,
            mIStart: aIStart,
            mBStart: aBStart,
            mISize: aISize,
            mBSize: aBSize,
        }
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::LogicalRect(WritingMode, const LogicalPoint&, const LogicalSize&)
    pub fn new__WritingMode_LogicalPoint_LogicalSize(
        aWritingMode: WritingMode,
        aOrigin: LogicalPoint,
        aSize: LogicalSize,
    ) -> LogicalRect {
        let result = LogicalRect {
            #[cfg(debug_assertions)]
            mWritingMode: aWritingMode,
            mIStart: aOrigin.mPoint.x,
            mBStart: aOrigin.mPoint.y,
            mISize: aSize.mSize.width,
            mBSize: aSize.mSize.height,
        };
        CHECK_WRITING_MODE!(result, aOrigin.GetWritingMode());
        CHECK_WRITING_MODE!(result, aSize.GetWritingMode());
        result
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::LogicalRect(WritingMode, const nsRect&, const nsSize&)
    pub fn new__WritingMode_nsRect_nsSize(
        aWritingMode: WritingMode,
        aRect: nsRect,
        aContainerSize: nsSize,
    ) -> LogicalRect {
        let mut result = LogicalRect {
            #[cfg(debug_assertions)]
            mWritingMode: aWritingMode,
            mIStart: 0,
            mBStart: 0,
            mISize: 0,
            mBSize: 0,
        };
        if aWritingMode.IsVertical() {
            result.mBStart = if aWritingMode.IsVerticalLR() {
                aRect.X()
            } else {
                aContainerSize.width - aRect.XMost()
            };
            result.mIStart = if aWritingMode.IsInlineReversed() {
                aContainerSize.height - aRect.YMost()
            } else {
                aRect.Y()
            };
            result.mBSize = aRect.Width();
            result.mISize = aRect.Height();
        } else {
            result.mIStart = if aWritingMode.IsInlineReversed() {
                aContainerSize.width - aRect.XMost()
            } else {
                aRect.X()
            };
            result.mBStart = aRect.Y();
            result.mISize = aRect.Width();
            result.mBSize = aRect.Height();
        }
        result
    }

    /**
     * Inline- and block-dimension geometry.
     */
    // Gecko: layout/generic/WritingModes.h LogicalRect::IStart
    pub fn IStart(&self, aWritingMode: WritingMode) -> nscoord // inline-start edge
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        self.mIStart
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::IEnd
    pub fn IEnd(&self, aWritingMode: WritingMode) -> nscoord // inline-end edge
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        self.mIStart + self.mISize
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::ISize
    pub fn ISize(&self, aWritingMode: WritingMode) -> nscoord // inline-size
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        self.mISize
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::BStart
    pub fn BStart(&self, aWritingMode: WritingMode) -> nscoord // block-start edge
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        self.mBStart
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::BEnd
    pub fn BEnd(&self, aWritingMode: WritingMode) -> nscoord // block-end edge
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        self.mBStart + self.mBSize
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::BSize
    pub fn BSize(&self, aWritingMode: WritingMode) -> nscoord // block-size
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        self.mBSize
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::Start
    pub fn Start(&self, aAxis: LogicalAxis, aWM: WritingMode) -> nscoord {
        if aAxis == LogicalAxis::Inline { self.IStart(aWM) } else { self.BStart(aWM) }
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::End
    pub fn End(&self, aAxis: LogicalAxis, aWM: WritingMode) -> nscoord {
        if aAxis == LogicalAxis::Inline { self.IEnd(aWM) } else { self.BEnd(aWM) }
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::Size(LogicalAxis, WritingMode) const
    pub fn Size__LogicalAxis_WritingMode(&self, aAxis: LogicalAxis, aWM: WritingMode) -> nscoord {
        if aAxis == LogicalAxis::Inline { self.ISize(aWM) } else { self.BSize(aWM) }
    }

    /**
     * Writable (reference) accessors are only available for the basic logical
     * fields (Start and Size), not derivatives like End.
     */
    // Gecko: layout/generic/WritingModes.h LogicalRect::IStart (non-const)
    pub fn IStart_mut(&mut self, aWritingMode: WritingMode) -> &mut nscoord // inline-start edge
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        &mut self.mIStart
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::ISize (non-const)
    pub fn ISize_mut(&mut self, aWritingMode: WritingMode) -> &mut nscoord // inline-size
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        &mut self.mISize
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::BStart (non-const)
    pub fn BStart_mut(&mut self, aWritingMode: WritingMode) -> &mut nscoord // block-start edge
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        &mut self.mBStart
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::BSize (non-const)
    pub fn BSize_mut(&mut self, aWritingMode: WritingMode) -> &mut nscoord // block-size
    {
        CHECK_WRITING_MODE!(self, aWritingMode);
        &mut self.mBSize
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::Start (non-const)
    pub fn Start_mut(&mut self, aAxis: LogicalAxis, aWM: WritingMode) -> &mut nscoord {
        if aAxis == LogicalAxis::Inline { self.IStart_mut(aWM) } else { self.BStart_mut(aWM) }
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::Size(LogicalAxis, WritingMode) (non-const)
    pub fn Size__LogicalAxis_WritingMode_mut(&mut self, aAxis: LogicalAxis, aWM: WritingMode) -> &mut nscoord {
        if aAxis == LogicalAxis::Inline { self.ISize_mut(aWM) } else { self.BSize_mut(aWM) }
    }

    /**
     * Accessors for line-relative coordinates
     */
    // Gecko: layout/generic/WritingModes.h LogicalRect::LineLeft
    pub fn LineLeft(&self, aWritingMode: WritingMode, aContainerSize: nsSize) -> nscoord {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsBidiLTR() {
            return self.IStart__();
        }
        let containerISize: nscoord = if aWritingMode.IsVertical() {
            aContainerSize.height
        } else {
            aContainerSize.width
        };
        containerISize - self.IEnd__()
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::LineRight
    pub fn LineRight(&self, aWritingMode: WritingMode, aContainerSize: nsSize) -> nscoord {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsBidiLTR() {
            return self.IEnd__();
        }
        let containerISize: nscoord = if aWritingMode.IsVertical() {
            aContainerSize.height
        } else {
            aContainerSize.width
        };
        containerISize - self.IStart__()
    }

    /**
     * Physical coordinates of the rect.
     */
    // Gecko: layout/generic/WritingModes.h LogicalRect::X
    pub fn X(&self, aWritingMode: WritingMode, aContainerWidth: nscoord) -> nscoord {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsVertical() {
            return if aWritingMode.IsVerticalLR() {
                self.mBStart
            } else {
                aContainerWidth - self.BEnd__()
            };
        }
        if aWritingMode.IsInlineReversed() {
            aContainerWidth - self.IEnd__()
        } else {
            self.mIStart
        }
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::Y
    pub fn Y(&self, aWritingMode: WritingMode, aContainerHeight: nscoord) -> nscoord {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsVertical() {
            return if aWritingMode.IsInlineReversed() {
                aContainerHeight - self.IEnd__()
            } else {
                self.mIStart
            };
        }
        self.mBStart
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::Width
    pub fn Width(&self, aWritingMode: WritingMode) -> nscoord {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsVertical() { self.mBSize } else { self.mISize }
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::Height
    pub fn Height(&self, aWritingMode: WritingMode) -> nscoord {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsVertical() { self.mISize } else { self.mBSize }
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::XMost
    pub fn XMost(&self, aWritingMode: WritingMode, aContainerWidth: nscoord) -> nscoord {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsVertical() {
            return if aWritingMode.IsVerticalLR() {
                self.BEnd__()
            } else {
                aContainerWidth - self.mBStart
            };
        }
        if aWritingMode.IsInlineReversed() {
            aContainerWidth - self.mIStart
        } else {
            self.IEnd__()
        }
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::YMost
    pub fn YMost(&self, aWritingMode: WritingMode, aContainerHeight: nscoord) -> nscoord {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsVertical() {
            return if aWritingMode.IsInlineReversed() {
                aContainerHeight - self.mIStart
            } else {
                self.IEnd__()
            };
        }
        self.BEnd__()
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::IsEmpty
    pub fn IsEmpty(&self) -> bool {
        self.mISize <= 0 || self.mBSize <= 0
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::IsAllZero
    pub fn IsAllZero(&self) -> bool {
        self.mIStart == 0 && self.mBStart == 0 && self.mISize == 0 && self.mBSize == 0
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::Size
    pub fn Size(&self, aWM: WritingMode) -> LogicalSize {
        CHECK_WRITING_MODE!(self, aWM);
        LogicalSize::new(aWM, self.mISize, self.mBSize)
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::IsZeroSize
    pub fn IsZeroSize(&self) -> bool {
        self.mISize == 0 && self.mBSize == 0
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::SetEmpty
    pub fn SetEmpty(&mut self) {
        self.mISize = 0;
        self.mBSize = 0;
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::IsEqualEdges
    pub fn IsEqualEdges(&self, aOther: LogicalRect) -> bool {
        CHECK_WRITING_MODE!(self, aOther.GetWritingMode());
        let result = self.mIStart == aOther.mIStart
            && self.mBStart == aOther.mBStart
            && self.mISize == aOther.mISize
            && self.mBSize == aOther.mBSize;

        // We want the same result as nsRect, so assert we get it.
        MOZ_ASSERT!(
            result
                == nsRect::new(self.mIStart, self.mBStart, self.mISize, self.mBSize).IsEqualEdges(
                    nsRect::new(aOther.mIStart, aOther.mBStart, aOther.mISize, aOther.mBSize)
                )
        );
        result
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::Origin
    pub fn Origin(&self, aWritingMode: WritingMode) -> LogicalPoint {
        CHECK_WRITING_MODE!(self, aWritingMode);
        LogicalPoint::new(aWritingMode, self.IStart__(), self.BStart__())
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::SetOrigin
    pub fn SetOrigin(&mut self, aWritingMode: WritingMode, aPoint: LogicalPoint) {
        *self.IStart_mut(aWritingMode) = aPoint.I(aWritingMode);
        *self.BStart_mut(aWritingMode) = aPoint.B(aWritingMode);
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::Size(WritingMode) const
    pub fn Size__WritingMode(&self, aWritingMode: WritingMode) -> LogicalSize {
        CHECK_WRITING_MODE!(self, aWritingMode);
        LogicalSize::new(aWritingMode, self.ISize__(), self.BSize__())
    }

    // (operator+ / += / - / -= with a LogicalPoint: std ops below.)

    // Gecko: layout/generic/WritingModes.h LogicalRect::MoveBy
    pub fn MoveBy(&mut self, aWritingMode: WritingMode, aDelta: LogicalPoint) {
        CHECK_WRITING_MODE!(self, aWritingMode);
        CHECK_WRITING_MODE!(self, aDelta.GetWritingMode());
        *self.IStart___mut() += aDelta.I__();
        *self.BStart___mut() += aDelta.B__();
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::Inflate(nscoord)
    pub fn Inflate__nscoord(&mut self, aD: nscoord) {
        #[cfg(debug_assertions)]
        // Compute using nsRect and assert the results match
        let rectDebug = {
            let mut rectDebug = nsRect::new(self.mIStart, self.mBStart, self.mISize, self.mBSize);
            rectDebug.Inflate__nscoord(aD);
            rectDebug
        };
        self.mIStart -= aD;
        self.mBStart -= aD;
        self.mISize += 2 * aD;
        self.mBSize += 2 * aD;
        #[cfg(debug_assertions)]
        MOZ_ASSERT!(rectDebug.IsEqualEdges(nsRect::new(
            self.mIStart,
            self.mBStart,
            self.mISize,
            self.mBSize
        )));
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::Inflate(nscoord, nscoord)
    pub fn Inflate__nscoord_nscoord(&mut self, aDI: nscoord, aDB: nscoord) {
        #[cfg(debug_assertions)]
        // Compute using nsRect and assert the results match
        let rectDebug = {
            let mut rectDebug = nsRect::new(self.mIStart, self.mBStart, self.mISize, self.mBSize);
            rectDebug.Inflate__nscoord_nscoord(aDI, aDB);
            rectDebug
        };
        self.mIStart -= aDI;
        self.mBStart -= aDB;
        self.mISize += 2 * aDI;
        self.mBSize += 2 * aDB;
        #[cfg(debug_assertions)]
        MOZ_ASSERT!(rectDebug.IsEqualEdges(nsRect::new(
            self.mIStart,
            self.mBStart,
            self.mISize,
            self.mBSize
        )));
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::Inflate(WritingMode, const LogicalMargin&)
    pub fn Inflate__WritingMode_LogicalMargin(&mut self, aWritingMode: WritingMode, aMargin: LogicalMargin) {
        CHECK_WRITING_MODE!(self, aWritingMode);
        CHECK_WRITING_MODE!(self, aMargin.GetWritingMode());
        #[cfg(debug_assertions)]
        // Compute using nsRect and assert the results match
        let rectDebug = {
            let mut rectDebug = nsRect::new(self.mIStart, self.mBStart, self.mISize, self.mBSize);
            rectDebug.Inflate__nsMargin(aMargin.mMargin);
            rectDebug
        };
        self.mIStart -= aMargin.mMargin.left;
        self.mBStart -= aMargin.mMargin.top;
        self.mISize += aMargin.mMargin.LeftRight();
        self.mBSize += aMargin.mMargin.TopBottom();
        #[cfg(debug_assertions)]
        MOZ_ASSERT!(rectDebug.IsEqualEdges(nsRect::new(
            self.mIStart,
            self.mBStart,
            self.mISize,
            self.mBSize
        )));
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::Deflate(nscoord)
    pub fn Deflate__nscoord(&mut self, aD: nscoord) {
        #[cfg(debug_assertions)]
        // Compute using nsRect and assert the results match
        let rectDebug = {
            let mut rectDebug = nsRect::new(self.mIStart, self.mBStart, self.mISize, self.mBSize);
            rectDebug.Deflate__nscoord(aD);
            rectDebug
        };
        self.mIStart += aD;
        self.mBStart += aD;
        self.mISize = std::cmp::max(0, self.mISize - 2 * aD);
        self.mBSize = std::cmp::max(0, self.mBSize - 2 * aD);
        #[cfg(debug_assertions)]
        MOZ_ASSERT!(rectDebug.IsEqualEdges(nsRect::new(
            self.mIStart,
            self.mBStart,
            self.mISize,
            self.mBSize
        )));
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::Deflate(nscoord, nscoord)
    pub fn Deflate__nscoord_nscoord(&mut self, aDI: nscoord, aDB: nscoord) {
        #[cfg(debug_assertions)]
        // Compute using nsRect and assert the results match
        let rectDebug = {
            let mut rectDebug = nsRect::new(self.mIStart, self.mBStart, self.mISize, self.mBSize);
            rectDebug.Deflate__nscoord_nscoord(aDI, aDB);
            rectDebug
        };
        self.mIStart += aDI;
        self.mBStart += aDB;
        self.mISize = std::cmp::max(0, self.mISize - 2 * aDI);
        self.mBSize = std::cmp::max(0, self.mBSize - 2 * aDB);
        #[cfg(debug_assertions)]
        MOZ_ASSERT!(rectDebug.IsEqualEdges(nsRect::new(
            self.mIStart,
            self.mBStart,
            self.mISize,
            self.mBSize
        )));
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::Deflate(WritingMode, const LogicalMargin&)
    pub fn Deflate__WritingMode_LogicalMargin(&mut self, aWritingMode: WritingMode, aMargin: LogicalMargin) {
        CHECK_WRITING_MODE!(self, aWritingMode);
        CHECK_WRITING_MODE!(self, aMargin.GetWritingMode());
        #[cfg(debug_assertions)]
        // Compute using nsRect and assert the results match
        let rectDebug = {
            let mut rectDebug = nsRect::new(self.mIStart, self.mBStart, self.mISize, self.mBSize);
            rectDebug.Deflate__nsMargin(aMargin.mMargin);
            rectDebug
        };
        self.mIStart += aMargin.mMargin.left;
        self.mBStart += aMargin.mMargin.top;
        self.mISize = std::cmp::max(0, self.mISize - aMargin.mMargin.LeftRight());
        self.mBSize = std::cmp::max(0, self.mBSize - aMargin.mMargin.TopBottom());
        #[cfg(debug_assertions)]
        MOZ_ASSERT!(rectDebug.IsEqualEdges(nsRect::new(
            self.mIStart,
            self.mBStart,
            self.mISize,
            self.mBSize
        )));
    }

    /**
     * Return an nsRect containing our physical coordinates within the given
     * container size.
     */
    // Gecko: layout/generic/WritingModes.h LogicalRect::GetPhysicalRect
    pub fn GetPhysicalRect(&self, aWritingMode: WritingMode, aContainerSize: nsSize) -> nsRect {
        CHECK_WRITING_MODE!(self, aWritingMode);
        if aWritingMode.IsVertical() {
            nsRect::new(
                if aWritingMode.IsVerticalLR() {
                    self.BStart__()
                } else {
                    aContainerSize.width - self.BEnd__()
                },
                if aWritingMode.IsInlineReversed() {
                    aContainerSize.height - self.IEnd__()
                } else {
                    self.IStart__()
                },
                self.BSize__(),
                self.ISize__(),
            )
        } else {
            nsRect::new(
                if aWritingMode.IsInlineReversed() {
                    aContainerSize.width - self.IEnd__()
                } else {
                    self.IStart__()
                },
                self.BStart__(),
                self.ISize__(),
                self.BSize__(),
            )
        }
    }

    /**
     * Return a LogicalRect representing this rect in a different writing mode
     */
    // Gecko: layout/generic/WritingModes.h LogicalRect::ConvertTo
    pub fn ConvertTo(&self, aToMode: WritingMode, aFromMode: WritingMode, aContainerSize: nsSize) -> LogicalRect {
        CHECK_WRITING_MODE!(self, aFromMode);
        if aToMode == aFromMode {
            *self
        } else {
            LogicalRect::new__WritingMode_nsRect_nsSize(
                aToMode,
                self.GetPhysicalRect(aFromMode, aContainerSize),
                aContainerSize,
            )
        }
    }

    /**
     * Set *this to be the rectangle containing the intersection of aRect1
     * and aRect2, return whether the intersection is non-empty.
     */
    // Gecko: layout/generic/WritingModes.h LogicalRect::IntersectRect
    pub fn IntersectRect(&mut self, aRect1: LogicalRect, aRect2: LogicalRect) -> bool {
        CHECK_WRITING_MODE!(self, aRect1.mWritingMode);
        CHECK_WRITING_MODE!(self, aRect2.mWritingMode);
        #[cfg(debug_assertions)]
        // Compute using nsRect and assert the results match
        let rectDebug = {
            let mut rectDebug = nsRect::default();
            rectDebug.IntersectRect(
                nsRect::new(aRect1.mIStart, aRect1.mBStart, aRect1.mISize, aRect1.mBSize),
                nsRect::new(aRect2.mIStart, aRect2.mBStart, aRect2.mISize, aRect2.mBSize),
            );
            rectDebug
        };

        let iEnd: nscoord = std::cmp::min(aRect1.IEnd__(), aRect2.IEnd__());
        self.mIStart = std::cmp::max(aRect1.mIStart, aRect2.mIStart);
        self.mISize = iEnd - self.mIStart;

        let bEnd: nscoord = std::cmp::min(aRect1.BEnd__(), aRect2.BEnd__());
        self.mBStart = std::cmp::max(aRect1.mBStart, aRect2.mBStart);
        self.mBSize = bEnd - self.mBStart;

        if self.mISize < 0 || self.mBSize < 0 {
            self.mISize = 0;
            self.mBSize = 0;
        }

        #[cfg(debug_assertions)]
        MOZ_ASSERT!(
            (rectDebug.IsEmpty() && (self.mISize == 0 || self.mBSize == 0))
                || rectDebug.IsEqualEdges(nsRect::new(
                    self.mIStart,
                    self.mBStart,
                    self.mISize,
                    self.mBSize
                ))
        );
        self.mISize > 0 && self.mBSize > 0
    }

    // LogicalRect() = delete;   (NOT IMPLEMENTED on purpose: no Default)

    // Gecko: layout/generic/WritingModes.h LogicalRect::GetWritingMode
    #[cfg(debug_assertions)]
    fn GetWritingMode(&self) -> WritingMode {
        self.mWritingMode
    }
    #[cfg(not(debug_assertions))]
    fn GetWritingMode(&self) -> WritingMode {
        WritingMode::Unknown()
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::IStart() const
    fn IStart__(&self) -> nscoord // inline-start edge
    {
        self.mIStart
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::IEnd() const
    fn IEnd__(&self) -> nscoord // inline-end edge
    {
        self.mIStart + self.mISize
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::ISize() const
    fn ISize__(&self) -> nscoord // inline-size
    {
        self.mISize
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::BStart() const
    fn BStart__(&self) -> nscoord // block-start edge
    {
        self.mBStart
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::BEnd() const
    fn BEnd__(&self) -> nscoord // block-end edge
    {
        self.mBStart + self.mBSize
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::BSize() const
    fn BSize__(&self) -> nscoord // block-size
    {
        self.mBSize
    }

    // Gecko: layout/generic/WritingModes.h LogicalRect::IStart()
    fn IStart___mut(&mut self) -> &mut nscoord // inline-start edge
    {
        &mut self.mIStart
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::ISize()
    fn ISize___mut(&mut self) -> &mut nscoord // inline-size
    {
        &mut self.mISize
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::BStart()
    fn BStart___mut(&mut self) -> &mut nscoord // block-start edge
    {
        &mut self.mBStart
    }
    // Gecko: layout/generic/WritingModes.h LogicalRect::BSize()
    fn BSize___mut(&mut self) -> &mut nscoord // block-size
    {
        &mut self.mBSize
    }
}

// Gecko: layout/generic/WritingModes.h LogicalRect::operator+(const LogicalPoint&)
impl Add<LogicalPoint> for LogicalRect {
    type Output = LogicalRect;
    fn add(self, aPoint: LogicalPoint) -> LogicalRect {
        CHECK_WRITING_MODE!(self, aPoint.GetWritingMode());
        LogicalRect::new(
            self.GetWritingMode(),
            self.IStart__() + aPoint.I__(),
            self.BStart__() + aPoint.B__(),
            self.ISize__(),
            self.BSize__(),
        )
    }
}

// Gecko: layout/generic/WritingModes.h LogicalRect::operator+=(const LogicalPoint&)
impl AddAssign<LogicalPoint> for LogicalRect {
    fn add_assign(&mut self, aPoint: LogicalPoint) {
        CHECK_WRITING_MODE!(self, aPoint.GetWritingMode());
        self.mIStart += aPoint.mPoint.x;
        self.mBStart += aPoint.mPoint.y;
    }
}

// Gecko: layout/generic/WritingModes.h LogicalRect::operator-(const LogicalPoint&)
impl Sub<LogicalPoint> for LogicalRect {
    type Output = LogicalRect;
    fn sub(self, aPoint: LogicalPoint) -> LogicalRect {
        CHECK_WRITING_MODE!(self, aPoint.GetWritingMode());
        LogicalRect::new(
            self.GetWritingMode(),
            self.IStart__() - aPoint.I__(),
            self.BStart__() - aPoint.B__(),
            self.ISize__(),
            self.BSize__(),
        )
    }
}

// Gecko: layout/generic/WritingModes.h LogicalRect::operator-=(const LogicalPoint&)
impl SubAssign<LogicalPoint> for LogicalRect {
    fn sub_assign(&mut self, aPoint: LogicalPoint) {
        CHECK_WRITING_MODE!(self, aPoint.GetWritingMode());
        self.mIStart -= aPoint.mPoint.x;
        self.mBStart -= aPoint.mPoint.y;
    }
}

// Gecko: layout/generic/WritingModes.h operator<<(std::ostream&, const LogicalRect&)
impl fmt::Display for LogicalRect {
    fn fmt(&self, aStream: &mut fmt::Formatter) -> fmt::Result {
        write!(
            aStream,
            "({},{},{},{})",
            self.IStart__(),
            self.BStart__(),
            self.ISize__(),
            self.BSize__()
        )
    }
}

// Gecko: layout/generic/WritingModes.h StyleRect<T>::Get(LogicalSide, WritingMode) and friends.
// ADAPTER: `StyleRect<T>` is stylo's `style::values::generics::rect::Rect<T>` (a tuple struct
// in top, right, bottom, left order). C++ adds these methods to the cbindgen'd type from this
// header; Rust cannot add inherent methods to a foreign type, so they are a local extension
// trait implemented for `Rect<T>`. The physical `Get(Side)` they call is in Gecko's
// ServoStyleConstsInlines.h (not translated); its two overloads (const, non-const) are
// `Get__Side` and `Get__Side_mut` here. The const/non-const pairs follow the accessor scheme
// (`_mut` for the non-const one).
pub trait StyleRect_WritingModes<T> {
    fn Get__Side(&self, aSide: Side) -> &T;
    fn Get__Side_mut(&mut self, aSide: Side) -> &mut T;

    fn Get__LogicalSide_WritingMode(&self, aSide: LogicalSide, aWM: WritingMode) -> &T;
    fn GetIStart(&self, aWM: WritingMode) -> &T;
    fn GetBStart(&self, aWM: WritingMode) -> &T;
    fn GetIEnd(&self, aWM: WritingMode) -> &T;
    fn GetBEnd(&self, aWM: WritingMode) -> &T;

    fn Get__LogicalSide_WritingMode_mut(&mut self, aSide: LogicalSide, aWM: WritingMode) -> &mut T;
    fn GetIStart_mut(&mut self, aWM: WritingMode) -> &mut T;
    fn GetBStart_mut(&mut self, aWM: WritingMode) -> &mut T;
    fn GetIEnd_mut(&mut self, aWM: WritingMode) -> &mut T;
    fn GetBEnd_mut(&mut self, aWM: WritingMode) -> &mut T;

    fn Start(&self, aAxis: LogicalAxis, aWM: WritingMode) -> &T;
    fn End(&self, aAxis: LogicalAxis, aWM: WritingMode) -> &T;
}

impl<T> StyleRect_WritingModes<T> for mrt_style::style::values::generics::rect::Rect<T> {
    // Gecko: servo/ServoStyleConstsInlines.h StyleRect<T>::Get(Side) const
    fn Get__Side(&self, aSide: Side) -> &T {
        match aSide as u8 {
            0 => &self.0, // eSideTop
            1 => &self.1, // eSideRight
            2 => &self.2, // eSideBottom
            _ => &self.3, // eSideLeft
        }
    }
    // Gecko: servo/ServoStyleConstsInlines.h StyleRect<T>::Get(Side)
    fn Get__Side_mut(&mut self, aSide: Side) -> &mut T {
        match aSide as u8 {
            0 => &mut self.0,
            1 => &mut self.1,
            2 => &mut self.2,
            _ => &mut self.3,
        }
    }

    // Gecko: layout/generic/WritingModes.h StyleRect<T>::Get(LogicalSide, WritingMode) const
    fn Get__LogicalSide_WritingMode(&self, aSide: LogicalSide, aWM: WritingMode) -> &T {
        self.Get__Side(aWM.PhysicalSide(aSide))
    }

    // Gecko: layout/generic/WritingModes.h StyleRect<T>::GetIStart
    fn GetIStart(&self, aWM: WritingMode) -> &T {
        self.Get__LogicalSide_WritingMode(LogicalSide::IStart, aWM)
    }

    // Gecko: layout/generic/WritingModes.h StyleRect<T>::GetBStart
    fn GetBStart(&self, aWM: WritingMode) -> &T {
        self.Get__LogicalSide_WritingMode(LogicalSide::BStart, aWM)
    }

    // Gecko: layout/generic/WritingModes.h StyleRect<T>::GetIEnd
    fn GetIEnd(&self, aWM: WritingMode) -> &T {
        self.Get__LogicalSide_WritingMode(LogicalSide::IEnd, aWM)
    }

    // Gecko: layout/generic/WritingModes.h StyleRect<T>::GetBEnd
    fn GetBEnd(&self, aWM: WritingMode) -> &T {
        self.Get__LogicalSide_WritingMode(LogicalSide::BEnd, aWM)
    }

    // Gecko: layout/generic/WritingModes.h StyleRect<T>::Get(LogicalSide, WritingMode)
    fn Get__LogicalSide_WritingMode_mut(&mut self, aSide: LogicalSide, aWM: WritingMode) -> &mut T {
        self.Get__Side_mut(aWM.PhysicalSide(aSide))
    }

    // Gecko: layout/generic/WritingModes.h StyleRect<T>::GetIStart (non-const)
    fn GetIStart_mut(&mut self, aWM: WritingMode) -> &mut T {
        self.Get__LogicalSide_WritingMode_mut(LogicalSide::IStart, aWM)
    }

    // Gecko: layout/generic/WritingModes.h StyleRect<T>::GetBStart (non-const)
    fn GetBStart_mut(&mut self, aWM: WritingMode) -> &mut T {
        self.Get__LogicalSide_WritingMode_mut(LogicalSide::BStart, aWM)
    }

    // Gecko: layout/generic/WritingModes.h StyleRect<T>::GetIEnd (non-const)
    fn GetIEnd_mut(&mut self, aWM: WritingMode) -> &mut T {
        self.Get__LogicalSide_WritingMode_mut(LogicalSide::IEnd, aWM)
    }

    // Gecko: layout/generic/WritingModes.h StyleRect<T>::GetBEnd (non-const)
    fn GetBEnd_mut(&mut self, aWM: WritingMode) -> &mut T {
        self.Get__LogicalSide_WritingMode_mut(LogicalSide::BEnd, aWM)
    }

    // Gecko: layout/generic/WritingModes.h StyleRect<T>::Start
    fn Start(&self, aAxis: LogicalAxis, aWM: WritingMode) -> &T {
        if aAxis == LogicalAxis::Inline { self.GetIStart(aWM) } else { self.GetBStart(aWM) }
    }

    // Gecko: layout/generic/WritingModes.h StyleRect<T>::End
    fn End(&self, aAxis: LogicalAxis, aWM: WritingMode) -> &T {
        if aAxis == LogicalAxis::Inline { self.GetIEnd(aWM) } else { self.GetBEnd(aWM) }
    }
}

// NOT PORTED: everything from here to the end of the header is an inline definition of a member
// of a type that lives in a header this runtime does not translate as C++ classes (AspectRatio,
// StyleSize / StyleMaxSize (cbindgen'd Stylo types), nsStylePosition, nsStyleMargin) or is in
// another file of this crate:
//  - AspectRatio::ConvertToWritingMode, StyleSize::BehavesLikeInitialValue(LogicalAxis),
//    StyleMaxSize::BehavesLikeInitialValue(LogicalAxis), nsStylePosition::*, nsStyleMargin::*:
//    NOT PORTED: those types are not in this crate yet; the definitions are kept below as
//    comments, in order, to be moved into their files when those are translated.
//  - nsStyleDisplay::UsedFloat / UsedClear: NOT PORTED here: translated in
//    layout/style/nsStyleStruct.rs (nsStyleDisplay::UsedFloat / UsedClear).
//
// The C++ text, verbatim:
//
// inline AspectRatio AspectRatio::ConvertToWritingMode(
//     const WritingMode& aWM) const {
//   return aWM.IsVertical() ? Inverted() : *this;
// }
//
// template <>
// inline bool StyleSize::BehavesLikeInitialValue(LogicalAxis aAxis) const {
//   return aAxis == LogicalAxis::Inline ? IsAuto()
//                                       : BehavesLikeInitialValueOnBlockAxis();
// }
//
// template <>
// inline bool StyleMaxSize::BehavesLikeInitialValue(LogicalAxis aAxis) const {
//   return aAxis == LogicalAxis::Inline ? IsNone()
//                                       : BehavesLikeInitialValueOnBlockAxis();
// }
//
// }  // namespace mozilla
//
// // Definitions of inline methods for nsStylePosition, declared in
// // nsStyleStruct.h but not defined there because they need WritingMode.
// inline AnchorResolvedInset nsStylePosition::GetAnchorResolvedInset(
//     mozilla::LogicalSide aSide, WritingMode aWM,
//     const AnchorPosOffsetResolutionParams& aParams) const {
//   return GetAnchorResolvedInset(aWM.PhysicalSide(aSide), aParams);
// }
//
// inline mozilla::Maybe<mozilla::Side> nsStylePosition::GetSingleAutoInsetInAxis(
//     LogicalAxis aAxis, WritingMode aWM,
//     const AnchorPosOffsetResolutionParams& aParams) const {
//   const bool isInlineAxis = (aAxis == LogicalAxis::Inline);
//   const mozilla::StylePhysicalAxis physicalAxis =
//       (isInlineAxis == aWM.IsVertical())
//           ? mozilla::StylePhysicalAxis::Vertical
//           : mozilla::StylePhysicalAxis::Horizontal;
//   return GetSingleAutoInsetInAxis(physicalAxis, aParams);
// }
//
// inline AnchorResolvedSize nsStylePosition::ISize(
//     WritingMode aWM, const AnchorPosResolutionParams& aParams) const {
//   return aWM.IsVertical() ? GetHeight(aParams) : GetWidth(aParams);
// }
//
// inline AnchorResolvedSize nsStylePosition::MinISize(
//     WritingMode aWM, const AnchorPosResolutionParams& aParams) const {
//   return aWM.IsVertical() ? GetMinHeight(aParams) : GetMinWidth(aParams);
// }
//
// inline AnchorResolvedMaxSize nsStylePosition::MaxISize(
//     WritingMode aWM, const AnchorPosResolutionParams& aParams) const {
//   return aWM.IsVertical() ? GetMaxHeight(aParams) : GetMaxWidth(aParams);
// }
//
// inline AnchorResolvedSize nsStylePosition::BSize(
//     WritingMode aWM, const AnchorPosResolutionParams& aParams) const {
//   return aWM.IsVertical() ? GetWidth(aParams) : GetHeight(aParams);
// }
//
// inline AnchorResolvedSize nsStylePosition::MinBSize(
//     WritingMode aWM, const AnchorPosResolutionParams& aParams) const {
//   return aWM.IsVertical() ? GetMinWidth(aParams) : GetMinHeight(aParams);
// }
//
// inline AnchorResolvedMaxSize nsStylePosition::MaxBSize(
//     WritingMode aWM, const AnchorPosResolutionParams& aParams) const {
//   return aWM.IsVertical() ? GetMaxWidth(aParams) : GetMaxHeight(aParams);
// }
//
// inline AnchorResolvedSize nsStylePosition::Size(
//     mozilla::LogicalAxis aAxis, WritingMode aWM,
//     const AnchorPosResolutionParams& aParams) const {
//   return aAxis == mozilla::LogicalAxis::Inline ? ISize(aWM, aParams)
//                                                : BSize(aWM, aParams);
// }
//
// inline AnchorResolvedSize nsStylePosition::MinSize(
//     mozilla::LogicalAxis aAxis, WritingMode aWM,
//     const AnchorPosResolutionParams& aParams) const {
//   return aAxis == mozilla::LogicalAxis::Inline ? MinISize(aWM, aParams)
//                                                : MinBSize(aWM, aParams);
// }
//
// inline AnchorResolvedMaxSize nsStylePosition::MaxSize(
//     mozilla::LogicalAxis aAxis, mozilla::WritingMode aWM,
//     const AnchorPosResolutionParams& aParams) const {
//   return aAxis == mozilla::LogicalAxis::Inline ? MaxISize(aWM, aParams)
//                                                : MaxBSize(aWM, aParams);
// }
//
// inline bool nsStylePosition::ISizeDependsOnContainer(
//     const AnchorResolvedSize& aSize) {
//   return aSize->IsAuto() || ISizeCoordDependsOnContainer(*aSize);
// }
//
// inline bool nsStylePosition::MinISizeDependsOnContainer(
//     const AnchorResolvedSize& aSize) {
//   // NOTE: For a flex item, "min-inline-size:auto" is supposed to behave like
//   // "min-content", which does depend on the container, so you might think we'd
//   // need a special case for "flex item && min-inline-size:auto" here. However,
//   // we don't actually need that special-case code, because flex items are
//   // explicitly supposed to *ignore* their min-inline-size (i.e. behave like
//   // it's 0) until the flex container explicitly considers it. So -- since the
//   // flex container doesn't rely on this method, we don't need to worry about
//   // special behavior for flex items' "min-inline-size:auto" values here.
//   return ISizeCoordDependsOnContainer(*aSize);
// }
//
// inline bool nsStylePosition::MaxISizeDependsOnContainer(
//     const AnchorResolvedMaxSize& aSize) {
//   // NOTE: The comment above MinISizeDependsOnContainer about flex items
//   // applies here, too.
//   return ISizeCoordDependsOnContainer(*aSize);
// }
//
// // Note that these functions count `auto` as depending on the container
// // since that's the case for absolutely positioned elements.
// // However, some callers do not care about this case and should check
// // for it, since it is the most common case.
// // FIXME: We should probably change the assumption to be the other way
// // around.
// inline bool nsStylePosition::BSizeDependsOnContainer(
//     const AnchorResolvedSize& aSize) {
//   return aSize->BehavesLikeInitialValueOnBlockAxis() ||
//          BSizeCoordDependsOnContainer(*aSize);
// }
//
// inline bool nsStylePosition::MinBSizeDependsOnContainer(
//     const AnchorResolvedSize& aSize) {
//   return BSizeCoordDependsOnContainer(*aSize);
// }
//
// inline bool nsStylePosition::MaxBSizeDependsOnContainer(
//     const AnchorResolvedMaxSize& aSize) {
//   return BSizeCoordDependsOnContainer(*aSize);
// }
//
// inline bool nsStyleMargin::HasBlockAxisAuto(
//     mozilla::WritingMode aWM, const AnchorPosResolutionParams& aParams) const {
//   return GetMargin(mozilla::LogicalSide::BStart, aWM, aParams)->IsAuto() ||
//          GetMargin(mozilla::LogicalSide::BEnd, aWM, aParams)->IsAuto();
// }
//
// inline bool nsStyleMargin::HasInlineAxisAuto(
//     mozilla::WritingMode aWM, const AnchorPosResolutionParams& aParams) const {
//   return GetMargin(mozilla::LogicalSide::IStart, aWM, aParams)->IsAuto() ||
//          GetMargin(mozilla::LogicalSide::IEnd, aWM, aParams)->IsAuto();
// }
//
// inline bool nsStyleMargin::HasAuto(
//     mozilla::LogicalAxis aAxis, mozilla::WritingMode aWM,
//     const AnchorPosResolutionParams& aParams) const {
//   return aAxis == mozilla::LogicalAxis::Inline ? HasInlineAxisAuto(aWM, aParams)
//                                                : HasBlockAxisAuto(aWM, aParams);
// }
//
// inline AnchorResolvedMargin nsStyleMargin::GetMargin(
//     mozilla::LogicalSide aSide, mozilla::WritingMode aWM,
//     const AnchorPosResolutionParams& aParams) const {
//   return GetMargin(aWM.PhysicalSide(aSide), aParams);
// }
//
// inline mozilla::StyleAlignFlags nsStylePosition::UsedSelfAlignment(
//     LogicalAxis aAlignContainerAxis,
//     const ComputedStyle* aAlignContainerStyle) const {
//   return aAlignContainerAxis == LogicalAxis::Block
//              ? UsedAlignSelf(aAlignContainerStyle)._0
//              : UsedJustifySelf(aAlignContainerStyle)._0;
// }
//
// inline mozilla::StyleAlignFlags nsStylePosition::UsedSelfAlignment(
//     WritingMode aAlignSubjectWM, LogicalAxis aAlignSubjectAxis,
//     WritingMode aAlignContainerWM,
//     const ComputedStyle* aAlignContainerStyle) const {
//   const auto alignContainerAxis =
//       aAlignSubjectWM.ConvertAxisTo(aAlignSubjectAxis, aAlignContainerWM);
//   return UsedSelfAlignment(alignContainerAxis, aAlignContainerStyle);
// }
//
// inline mozilla::StyleContentDistribution nsStylePosition::UsedContentAlignment(
//     mozilla::LogicalAxis aAxis) const {
//   return aAxis == mozilla::LogicalAxis::Block ? mAlignContent : mJustifyContent;
// }
//
// inline mozilla::UsedFloat nsStyleDisplay::UsedFloat(
//     mozilla::WritingMode aCBWM) const {
//   switch (mFloat) {
//     case mozilla::StyleFloat::None:
//       return mozilla::UsedFloat::None;
//     case mozilla::StyleFloat::Left:
//       return mozilla::UsedFloat::Left;
//     case mozilla::StyleFloat::Right:
//       return mozilla::UsedFloat::Right;
//     case mozilla::StyleFloat::InlineStart:
//       return aCBWM.IsBidiLTR() ? mozilla::UsedFloat::Left
//                                : mozilla::UsedFloat::Right;
//     case mozilla::StyleFloat::InlineEnd:
//       return aCBWM.IsBidiLTR() ? mozilla::UsedFloat::Right
//                                : mozilla::UsedFloat::Left;
//   }
//   MOZ_ASSERT_UNREACHABLE("all cases are handled above!");
//   return mozilla::UsedFloat::None;
// }
//
// inline mozilla::UsedClear nsStyleDisplay::UsedClear(
//     mozilla::WritingMode aCBWM) const {
//   switch (mClear) {
//     case mozilla::StyleClear::None:
//       return mozilla::UsedClear::None;
//     case mozilla::StyleClear::Left:
//       return mozilla::UsedClear::Left;
//     case mozilla::StyleClear::Right:
//       return mozilla::UsedClear::Right;
//     case mozilla::StyleClear::Both:
//       return mozilla::UsedClear::Both;
//     case mozilla::StyleClear::InlineStart:
//       return aCBWM.IsBidiLTR() ? mozilla::UsedClear::Left
//                                : mozilla::UsedClear::Right;
//     case mozilla::StyleClear::InlineEnd:
//       return aCBWM.IsBidiLTR() ? mozilla::UsedClear::Right
//                                : mozilla::UsedClear::Left;
//   }
//   MOZ_ASSERT_UNREACHABLE("all cases are handled above!");
//   return mozilla::UsedClear::None;
// }
//

// Tests: WritingModes.h has no gtest in the Firefox tree, so these are written from the
// documented behaviour of the classes (conversions between logical and physical, round trips,
// line-relative accessors, the side tables).
#[cfg(test)]
mod tests {
    use super::*;

    // Gecko's computed bit values for the writing modes CSS can produce (see the table in
    // `WritingMode::PhysicalSideForInlineAxis`).
    fn wm(aBits: u8) -> WritingMode {
        WritingMode::new__uint8_t(aBits)
    }
    fn horizontal_ltr() -> WritingMode {
        WritingMode::new()
    }
    fn horizontal_rtl() -> WritingMode {
        // direction: rtl sets RTL and INLINE_REVERSED
        wm((StyleWritingMode::RTL | StyleWritingMode::INLINE_REVERSED).bits())
    }
    fn vertical_rl() -> WritingMode {
        wm(StyleWritingMode::WRITING_MODE_VERTICAL_RL.bits())
    }
    fn vertical_lr() -> WritingMode {
        wm(StyleWritingMode::WRITING_MODE_VERTICAL_LR.bits())
    }
    fn vertical_rl_rtl() -> WritingMode {
        wm((StyleWritingMode::WRITING_MODE_VERTICAL_RL
            | StyleWritingMode::RTL
            | StyleWritingMode::INLINE_REVERSED)
            .bits())
    }
    fn vertical_lr_rtl() -> WritingMode {
        wm((StyleWritingMode::WRITING_MODE_VERTICAL_LR
            | StyleWritingMode::RTL
            | StyleWritingMode::INLINE_REVERSED)
            .bits())
    }
    fn sideways_lr() -> WritingMode {
        // sideways-lr with direction: ltr has INLINE_REVERSED set (see WritingMode::new in Stylo)
        wm((StyleWritingMode::WRITING_MODE_SIDEWAYS_LR | StyleWritingMode::INLINE_REVERSED).bits())
    }
    fn all_modes() -> Vec<WritingMode> {
        vec![
            horizontal_ltr(),
            horizontal_rtl(),
            vertical_rl(),
            vertical_lr(),
            vertical_rl_rtl(),
            vertical_lr_rtl(),
            sideways_lr(),
        ]
    }
    fn container() -> nsSize {
        nsSize::new(100, 60)
    }

    #[test]
    fn WritingMode_Flags() {
        assert!(horizontal_ltr().IsBidiLTR());
        assert!(!horizontal_ltr().IsVertical());
        assert!(!horizontal_ltr().IsInlineReversed());
        assert!(horizontal_rtl().IsBidiRTL());
        assert!(horizontal_rtl().IsInlineReversed());
        assert!(horizontal_rtl().IsPhysicalRTL());
        assert!(vertical_rl().IsVertical() && vertical_rl().IsVerticalRL());
        assert!(vertical_lr().IsVertical() && vertical_lr().IsVerticalLR());
        assert!(vertical_lr().IsLineInverted());
        assert_eq!(vertical_lr().FlowRelativeToLineRelativeFactor(), -1);
        assert_eq!(vertical_rl().FlowRelativeToLineRelativeFactor(), 1);
        assert!(vertical_lr().IsPhysicalLTR());
        assert!(vertical_rl().IsPhysicalRTL());
        assert!(sideways_lr().IsSidewaysLR());
        assert!(sideways_lr().IsSideways());
        assert!(!sideways_lr().IsCentralBaseline());
        assert!(vertical_rl().IsCentralBaseline());
        assert!(horizontal_ltr().IsAlphabeticalBaseline());
        assert_eq!(WritingMode::default(), horizontal_ltr());
        assert_eq!(horizontal_ltr().GetBits(), 0);
    }

    #[test]
    fn WritingMode_Dirs() {
        assert_eq!(horizontal_ltr().GetInlineDir(), WritingMode_InlineDir::LTR);
        assert_eq!(horizontal_rtl().GetInlineDir(), WritingMode_InlineDir::RTL);
        assert_eq!(vertical_rl().GetInlineDir(), WritingMode_InlineDir::TTB);
        assert_eq!(vertical_rl_rtl().GetInlineDir(), WritingMode_InlineDir::BTT);
        assert_eq!(horizontal_ltr().GetBlockDir(), WritingMode_BlockDir::TB);
        assert_eq!(vertical_rl().GetBlockDir(), WritingMode_BlockDir::RL);
        assert_eq!(vertical_lr().GetBlockDir(), WritingMode_BlockDir::LR);
    }

    #[test]
    fn WritingMode_Display() {
        assert_eq!(horizontal_ltr().to_string(), "h-ltr");
        assert_eq!(horizontal_rtl().to_string(), "h-rtl");
        assert_eq!(vertical_rl().to_string(), "v-rl-ltr");
        assert_eq!(vertical_lr_rtl().to_string(), "v-lr-rtl");
        assert_eq!(sideways_lr().to_string(), "sw-lr-ltr");
    }

    #[test]
    fn WritingMode_PhysicalSides() {
        // horizontal-tb ltr: inline runs left to right, block top to bottom.
        let h = horizontal_ltr();
        assert_eq!(h.PhysicalSide(LogicalSide::IStart), eSideLeft);
        assert_eq!(h.PhysicalSide(LogicalSide::IEnd), eSideRight);
        assert_eq!(h.PhysicalSide(LogicalSide::BStart), eSideTop);
        assert_eq!(h.PhysicalSide(LogicalSide::BEnd), eSideBottom);
        // rtl swaps the inline sides.
        let r = horizontal_rtl();
        assert_eq!(r.PhysicalSide(LogicalSide::IStart), eSideRight);
        assert_eq!(r.PhysicalSide(LogicalSide::IEnd), eSideLeft);
        // vertical-rl: lines stack right to left, inline runs top to bottom.
        let v = vertical_rl();
        assert_eq!(v.PhysicalSide(LogicalSide::BStart), eSideRight);
        assert_eq!(v.PhysicalSide(LogicalSide::BEnd), eSideLeft);
        assert_eq!(v.PhysicalSide(LogicalSide::IStart), eSideTop);
        // vertical-lr: lines stack left to right.
        let l = vertical_lr();
        assert_eq!(l.PhysicalSide(LogicalSide::BStart), eSideLeft);
        assert_eq!(l.PhysicalSide(LogicalSide::IStart), eSideTop);
        assert_eq!(vertical_lr_rtl().PhysicalSide(LogicalSide::IStart), eSideBottom);
        // Sideways bit does not change the block sides.
        assert_eq!(
            WritingMode::PhysicalSideForBlockAxis(1 | kWritingModeSidewaysMask, LogicalEdge::Start),
            eSideRight
        );
    }

    #[test]
    fn WritingMode_LogicalSideForPhysicalSide_IsInverse() {
        for mode in all_modes() {
            for side in [
                LogicalSide::BStart,
                LogicalSide::BEnd,
                LogicalSide::IStart,
                LogicalSide::IEnd,
            ] {
                assert_eq!(
                    mode.LogicalSideForPhysicalSide(mode.PhysicalSide(side)),
                    side,
                    "mode {}, side {:?}",
                    mode,
                    side
                );
            }
        }
    }

    #[test]
    fn WritingMode_LineRelativeDir() {
        assert_eq!(
            horizontal_ltr().LogicalSideForLineRelativeDir(LineRelativeDir::Left),
            LogicalSide::IStart
        );
        assert_eq!(
            horizontal_rtl().LogicalSideForLineRelativeDir(LineRelativeDir::Left),
            LogicalSide::IEnd
        );
        assert_eq!(
            vertical_rl().LogicalSideForLineRelativeDir(LineRelativeDir::Over),
            LogicalSide::BStart
        );
        // vertical-lr is line-inverted: over is block-end.
        assert_eq!(
            vertical_lr().LogicalSideForLineRelativeDir(LineRelativeDir::Over),
            LogicalSide::BEnd
        );
    }

    #[test]
    fn WritingMode_Orthogonal_Axes() {
        assert!(horizontal_ltr().IsOrthogonalTo(&vertical_rl()));
        assert!(!vertical_lr().IsOrthogonalTo(&vertical_rl()));
        assert_eq!(
            horizontal_ltr().ConvertAxisTo(LogicalAxis::Inline, vertical_rl()),
            LogicalAxis::Block
        );
        assert_eq!(
            horizontal_ltr().ConvertAxisTo(LogicalAxis::Inline, horizontal_rtl()),
            LogicalAxis::Inline
        );
        assert_eq!(horizontal_ltr().PhysicalAxis(LogicalAxis::Inline), PhysicalAxis::Horizontal);
        assert_eq!(vertical_rl().PhysicalAxis(LogicalAxis::Inline), PhysicalAxis::Vertical);
        assert_eq!(vertical_rl().PhysicalAxis(LogicalAxis::Block), PhysicalAxis::Horizontal);
        assert_eq!(GetOrthogonalAxis(LogicalAxis::Block), LogicalAxis::Inline);
        assert_eq!(GetOppositeSide(LogicalSide::IStart), LogicalSide::IEnd);
        assert_eq!(GetOppositeSide(LogicalSide::BEnd), LogicalSide::BStart);
        assert_eq!(MakeLogicalSide(LogicalAxis::Block, LogicalEdge::End), LogicalSide::BEnd);
        assert!(IsInline(LogicalSide::IEnd) && IsBlock(LogicalSide::BStart));
        assert!(IsEnd(LogicalSide::BEnd) && IsStart(LogicalSide::IStart));
        assert_eq!(GetAxis(LogicalSide::IStart), LogicalAxis::Inline);
        assert_eq!(GetEdge(LogicalSide::BEnd), LogicalEdge::End);
    }

    #[test]
    fn WritingMode_ParallelAxisStartsOnSameSide() {
        // ltr and rtl horizontal: inline starts on opposite sides, block on the same side.
        assert!(!horizontal_ltr()
            .ParallelAxisStartsOnSameSide(LogicalAxis::Inline, &horizontal_rtl()));
        assert!(horizontal_ltr().ParallelAxisStartsOnSameSide(LogicalAxis::Block, &horizontal_rtl()));
        // horizontal ltr inline starts left; vertical-lr's block (parallel axis) starts left.
        assert!(horizontal_ltr().ParallelAxisStartsOnSameSide(LogicalAxis::Inline, &vertical_lr()));
        assert!(!horizontal_ltr().ParallelAxisStartsOnSameSide(LogicalAxis::Inline, &vertical_rl()));
    }

    #[test]
    fn WritingMode_BaselineSynthesis() {
        // Alignment along the inline axis (physically horizontal) of a horizontal container with
        // a vertical item: the item's block axis is the alignment axis, the container's is not,
        // so the container's writing mode is used.
        assert_eq!(
            WritingMode::DetermineWritingModeForBaselineSynthesis(
                &horizontal_ltr(),
                &vertical_rl(),
                LogicalAxis::Inline
            ),
            horizontal_ltr()
        );
        // Both horizontal, aligning along the block axis (physically vertical): the item's
        // block axis is the alignment axis, so synthesise vertical-lr for an ltr container and
        // vertical-rl for an rtl one.
        assert_eq!(
            WritingMode::DetermineWritingModeForBaselineSynthesis(
                &horizontal_ltr(),
                &horizontal_ltr(),
                LogicalAxis::Block
            ),
            vertical_lr()
        );
        assert_eq!(
            WritingMode::DetermineWritingModeForBaselineSynthesis(
                &horizontal_rtl(),
                &horizontal_rtl(),
                LogicalAxis::Block
            ),
            vertical_rl()
        );
        // Both vertical, aligning along the block axis (physically horizontal): horizontal-tb.
        assert_eq!(
            WritingMode::DetermineWritingModeForBaselineSynthesis(
                &vertical_rl(),
                &vertical_lr(),
                LogicalAxis::Block
            ),
            horizontal_ltr()
        );
        // Both horizontal, aligning along the inline axis: the item's block axis is orthogonal
        // to it, so the item's own writing mode is used.
        assert_eq!(
            WritingMode::DetermineWritingModeForBaselineSynthesis(
                &horizontal_ltr(),
                &horizontal_rtl(),
                LogicalAxis::Inline
            ),
            horizontal_rtl()
        );
    }

    #[test]
    fn WritingMode_SetDirectionFromBidiLevel() {
        let mut m = horizontal_ltr();
        m.SetDirectionFromBidiLevel(BidiEmbeddingLevel::new(1));
        assert_eq!(m, horizontal_rtl());
        m.SetDirectionFromBidiLevel(BidiEmbeddingLevel::new(1));
        assert_eq!(m, horizontal_rtl());
        m.SetDirectionFromBidiLevel(BidiEmbeddingLevel::new(0));
        assert_eq!(m, horizontal_ltr());
    }

    // The physical rect (10, 20, 30, 40) in a 100x60 container, as logical (I, B, ISize, BSize):
    //   horizontal-tb ltr : (10, 20, 30, 40)
    //   horizontal-tb rtl : (100-40=60, 20, 30, 40)
    //   vertical-rl       : I = y = 20, B = 100-(10+30) = 60, ISize = height = 40, BSize = width = 30
    //   vertical-lr       : I = 20, B = x = 10, ISize = 40, BSize = 30
    //   vertical-rl rtl   : I = 60-(20+40) = 0, B = 60
    #[test]
    fn LogicalRect_FromPhysical() {
        let phys = nsRect::new(10, 20, 30, 40);
        let c = container();
        let expect = |m: WritingMode, i: nscoord, b: nscoord, isz: nscoord, bsz: nscoord| {
            let r = LogicalRect::new__WritingMode_nsRect_nsSize(m, phys, c);
            assert_eq!(
                (r.IStart(m), r.BStart(m), r.ISize(m), r.BSize(m)),
                (i, b, isz, bsz),
                "mode {}",
                m
            );
            assert_eq!(r.IEnd(m), i + isz);
            assert_eq!(r.BEnd(m), b + bsz);
        };
        expect(horizontal_ltr(), 10, 20, 30, 40);
        expect(horizontal_rtl(), 60, 20, 30, 40);
        expect(vertical_rl(), 20, 60, 40, 30);
        expect(vertical_lr(), 20, 10, 40, 30);
        expect(vertical_rl_rtl(), 0, 60, 40, 30);
        expect(vertical_lr_rtl(), 0, 10, 40, 30);
    }

    #[test]
    fn LogicalRect_ToPhysical() {
        let c = container();
        // Logical (I=10, B=20, 30 x 40) in each mode, back to physical.
        let l = |m: WritingMode| LogicalRect::new(m, 10, 20, 30, 40);
        assert_eq!(l(horizontal_ltr()).GetPhysicalRect(horizontal_ltr(), c), nsRect::new(10, 20, 30, 40));
        // rtl: x = 100 - IEnd(40) = 60
        assert_eq!(l(horizontal_rtl()).GetPhysicalRect(horizontal_rtl(), c), nsRect::new(60, 20, 30, 40));
        // vertical-rl: x = 100 - BEnd(60) = 40, y = I = 10, w = BSize = 40, h = ISize = 30
        assert_eq!(l(vertical_rl()).GetPhysicalRect(vertical_rl(), c), nsRect::new(40, 10, 40, 30));
        // vertical-lr: x = B = 20
        assert_eq!(l(vertical_lr()).GetPhysicalRect(vertical_lr(), c), nsRect::new(20, 10, 40, 30));
        // vertical-lr rtl: y = 60 - IEnd(40) = 20
        assert_eq!(
            l(vertical_lr_rtl()).GetPhysicalRect(vertical_lr_rtl(), c),
            nsRect::new(20, 20, 40, 30)
        );
    }

    #[test]
    fn LogicalRect_RoundTrips() {
        let c = container();
        let phys = nsRect::new(7, 3, 25, 11);
        for m in all_modes() {
            let l = LogicalRect::new__WritingMode_nsRect_nsSize(m, phys, c);
            assert_eq!(l.GetPhysicalRect(m, c), phys, "mode {}", m);
            // The per-component physical accessors agree with GetPhysicalRect.
            assert_eq!(l.X(m, c.width), phys.X());
            assert_eq!(l.Y(m, c.height), phys.Y());
            assert_eq!(l.XMost(m, c.width), phys.XMost());
            assert_eq!(l.YMost(m, c.height), phys.YMost());
            assert_eq!(l.Width(m), phys.Width());
            assert_eq!(l.Height(m), phys.Height());
            // Converting to every other mode and back gives the same logical rect.
            for to in all_modes() {
                let converted = l.ConvertTo(to, m, c);
                assert_eq!(converted.GetPhysicalRect(to, c), phys);
                let back = converted.ConvertTo(m, to, c);
                assert!(back.IsEqualEdges(l));
            }
        }
    }

    #[test]
    fn LogicalPoint_RoundTrips() {
        let c = container();
        let p = nsPoint::new(13, 42);
        for m in all_modes() {
            let l = LogicalPoint::new__WritingMode_nsPoint_nsSize(m, p, c);
            assert_eq!(l.GetPhysicalPoint(m, c), p, "mode {}", m);
            for to in all_modes() {
                assert_eq!(l.ConvertTo(to, m, c).GetPhysicalPoint(to, c), p);
            }
        }
        // horizontal-tb ltr: logical == physical; vertical-rl: I = y, B = width - x.
        let h = LogicalPoint::new__WritingMode_nsPoint_nsSize(horizontal_ltr(), p, c);
        assert_eq!((h.I(horizontal_ltr()), h.B(horizontal_ltr())), (13, 42));
        let v = LogicalPoint::new__WritingMode_nsPoint_nsSize(vertical_rl(), p, c);
        assert_eq!((v.I(vertical_rl()), v.B(vertical_rl())), (42, 87));
        let vl = LogicalPoint::new__WritingMode_nsPoint_nsSize(vertical_lr(), p, c);
        assert_eq!((vl.I(vertical_lr()), vl.B(vertical_lr())), (42, 13));
        assert_eq!(v.Pos(LogicalAxis::Inline, vertical_rl()), 42);
        assert_eq!(v.Pos(LogicalAxis::Block, vertical_rl()), 87);
    }

    #[test]
    fn LogicalPoint_LineRelative() {
        let c = container();
        let m = horizontal_rtl();
        let p = LogicalPoint::new(m, 10, 5);
        assert_eq!(p.LineRelative(m, c), 100 - 10);
        let ltr = horizontal_ltr();
        assert_eq!(LogicalPoint::new(ltr, 10, 5).LineRelative(ltr, c), 10);
        let v = vertical_rl_rtl();
        assert_eq!(LogicalPoint::new(v, 10, 5).LineRelative(v, c), 60 - 10);
    }

    #[test]
    fn LogicalPoint_ConvertRectOriginTo() {
        // The origin of a rect, as seen in another mode, is the matching physical corner.
        let c = container();
        let phys = nsRect::new(10, 20, 30, 40);
        let from = horizontal_ltr();
        let to = vertical_rl();
        let origin = LogicalRect::new__WritingMode_nsRect_nsSize(from, phys, c).Origin(from);
        let converted = origin.ConvertRectOriginTo(to, from, phys.Size(), c);
        let expected = LogicalRect::new__WritingMode_nsRect_nsSize(to, phys, c).Origin(to);
        assert_eq!(converted, expected);
        // Same mode: unchanged.
        assert_eq!(origin.ConvertRectOriginTo(from, from, phys.Size(), c), origin);
    }

    #[test]
    fn LogicalPoint_Operators() {
        let m = vertical_rl();
        let a = LogicalPoint::new(m, 1, 2);
        let b = LogicalPoint::new(m, 10, 20);
        assert_eq!(a + b, LogicalPoint::new(m, 11, 22));
        assert_eq!(b - a, LogicalPoint::new(m, 9, 18));
        let mut c = a;
        c += b;
        assert_eq!(c, LogicalPoint::new(m, 11, 22));
        c -= a;
        assert_eq!(c, b);
        assert!(a != b);
        *c.I_mut(m) = 100;
        *c.Pos_mut(LogicalAxis::Block, m) = 200;
        assert_eq!((c.I(m), c.B(m)), (100, 200));
        assert_eq!(format!("{}", a), "(1,2)");
    }

    #[test]
    fn LogicalSize_Conversions() {
        let c_phys = nsSize::new(30, 40);
        for m in all_modes() {
            let l = LogicalSize::new__WritingMode_nsSize(m, c_phys);
            assert_eq!(l.GetPhysicalSize(m), c_phys);
            assert_eq!((l.Width(m), l.Height(m)), (30, 40));
            if m.IsVertical() {
                assert_eq!((l.ISize(m), l.BSize(m)), (40, 30));
            } else {
                assert_eq!((l.ISize(m), l.BSize(m)), (30, 40));
            }
            for to in all_modes() {
                assert_eq!(l.ConvertTo(to, m).GetPhysicalSize(to), c_phys);
            }
        }
    }

    #[test]
    fn LogicalSize_AccessorsAndOperators() {
        let m = horizontal_ltr();
        let mut s = LogicalSize::new(m, 5, 7);
        assert!(!s.IsAllZero());
        *s.ISize_mut(m) = 0;
        *s.BSize_mut(m) = 0;
        assert!(s.IsAllZero());
        assert!(s.IsAllValues(0));
        s.SizeTo(m, 3, 4);
        assert_eq!((s.ISize(m), s.BSize(m)), (3, 4));
        assert_eq!(s.Size(LogicalAxis::Inline, m), 3);
        *s.Size_mut(LogicalAxis::Block, m) += 6;
        assert_eq!(s.BSize(m), 10);
        let t = LogicalSize::new(m, 1, 1);
        assert_eq!(s + t, LogicalSize::new(m, 4, 11));
        assert_eq!(s - t, LogicalSize::new(m, 2, 9));
        let mut u = s;
        u += t;
        u -= t;
        assert_eq!(u, s);
        assert_eq!(format!("{}", s), "(3 x 10)");
    }

    #[test]
    fn LogicalSides_Basics() {
        let m = horizontal_ltr();
        let mut s = LogicalSides::new__WritingMode(m);
        assert!(s.IsEmpty());
        s += LogicalSide::BStart;
        assert!(s.BStart() && !s.BEnd() && !s.IStart() && !s.IEnd());
        s += LogicalSides::new__WritingMode_EnumSet(m, LogicalSides::IBoth);
        assert!(s.IStart() && s.IEnd() && s.Contains(LogicalSide::BStart));
        let all = LogicalSides::new__WritingMode_EnumSet(m, LogicalSides::All);
        let bboth = LogicalSides::new__WritingMode_EnumSet(m, LogicalSides::BBoth);
        s += bboth;
        assert_eq!(s, all);
        assert_eq!(LogicalSides::new__WritingMode_LogicalSides(m, all), all);
        assert!(kPhysicalAxesBoth.contains(PhysicalAxis::Vertical));
        assert!(kPhysicalAxesBoth.contains(PhysicalAxis::Horizontal));
    }

    #[test]
    fn LogicalMargin_Conversions() {
        let phys = nsMargin::new(1, 2, 3, 4); // top, right, bottom, left
        for m in all_modes() {
            let l = LogicalMargin::new__WritingMode_nsMargin(m, phys);
            assert_eq!(l.GetPhysicalMargin(m), phys, "mode {}", m);
            assert_eq!(
                (l.Top(m), l.Right(m), l.Bottom(m), l.Left(m)),
                (1, 2, 3, 4),
                "mode {}",
                m
            );
            assert_eq!(l.LeftRight(m), 6);
            assert_eq!(l.TopBottom(m), 4);
            for to in all_modes() {
                assert_eq!(l.ConvertTo(to, m).GetPhysicalMargin(to), phys);
            }
        }
        // horizontal ltr: BStart = top, IEnd = right, BEnd = bottom, IStart = left.
        let h = horizontal_ltr();
        let l = LogicalMargin::new__WritingMode_nsMargin(h, phys);
        assert_eq!((l.BStart(h), l.IEnd(h), l.BEnd(h), l.IStart(h)), (1, 2, 3, 4));
        // vertical-rl: BStart = right, BEnd = left, IStart = top, IEnd = bottom.
        let v = vertical_rl();
        let l = LogicalMargin::new__WritingMode_nsMargin(v, phys);
        assert_eq!((l.BStart(v), l.BEnd(v), l.IStart(v), l.IEnd(v)), (2, 4, 1, 3));
        // vertical-lr: BStart = left.
        let v = vertical_lr();
        let l = LogicalMargin::new__WritingMode_nsMargin(v, phys);
        assert_eq!((l.BStart(v), l.BEnd(v)), (4, 2));
    }

    #[test]
    fn LogicalMargin_LineLeftRight_Rtl() {
        let ltr = horizontal_ltr();
        let l = LogicalMargin::new(ltr, 1, 2, 3, 4); // BStart, IEnd, BEnd, IStart
        assert_eq!(l.LineLeft(ltr), 4);
        assert_eq!(l.LineRight(ltr), 2);
        let rtl = horizontal_rtl();
        let r = LogicalMargin::new(rtl, 1, 2, 3, 4);
        assert_eq!(r.LineLeft(rtl), 2); // IEnd
        assert_eq!(r.LineRight(rtl), 4); // IStart
    }

    #[test]
    fn LogicalMargin_AccessorsAndOperators() {
        let m = vertical_lr();
        let mut l = LogicalMargin::new(m, 1, 2, 3, 4);
        assert_eq!((l.BStart(m), l.IEnd(m), l.BEnd(m), l.IStart(m)), (1, 2, 3, 4));
        assert_eq!(l.Start(LogicalAxis::Inline, m), 4);
        assert_eq!(l.End(LogicalAxis::Block, m), 3);
        assert_eq!(l.StartEnd(LogicalAxis::Inline, m), 6);
        assert_eq!(l.IStartEnd(m), 6);
        assert_eq!(l.BStartEnd(m), 4);
        assert_eq!(l.Side(LogicalSide::IEnd, m), 2);
        *l.Side_mut(LogicalSide::IEnd, m) = 20;
        *l.Start_mut(LogicalAxis::Inline, m) = 40;
        *l.End_mut(LogicalAxis::Block, m) = 30;
        *l.BStart_mut(m) = 10;
        assert_eq!((l.BStart(m), l.IEnd(m), l.BEnd(m), l.IStart(m)), (10, 20, 30, 40));
        let sz = l.Size(m);
        assert_eq!((sz.ISize(m), sz.BSize(m)), (60, 40));
        let off = l.StartOffset(m);
        assert_eq!((off.I(m), off.B(m)), (40, 10));
        assert!(!l.IsAllZero());
        let t = LogicalMargin::new(m, 1, 1, 1, 1);
        assert_eq!(l + t, LogicalMargin::new(m, 11, 21, 31, 41));
        assert_eq!(l - t, LogicalMargin::new(m, 9, 19, 29, 39));
        let mut u = l;
        u += t;
        assert_eq!(u, l + t);
        // ApplySkipSides zeroes the skipped sides.
        let mut skipped = l;
        let mut sides = LogicalSides::new__WritingMode(m);
        sides += LogicalSide::BStart;
        sides += LogicalSide::IEnd;
        skipped.ApplySkipSides(sides);
        assert_eq!(skipped, LogicalMargin::new(m, 0, 0, 30, 40));
        skipped.SizeTo(m, 0, 0, 0, 0);
        assert!(skipped.IsAllZero());
        assert_eq!(format!("{}", t), "(t=1, r=1, b=1, l=1)");
    }

    #[test]
    fn LogicalRect_AccessorsAndLineRelative() {
        let c = container();
        let m = horizontal_ltr();
        let mut r = LogicalRect::new(m, 10, 20, 30, 40);
        assert_eq!((r.IStart(m), r.IEnd(m), r.ISize(m)), (10, 40, 30));
        assert_eq!((r.BStart(m), r.BEnd(m), r.BSize(m)), (20, 60, 40));
        assert_eq!(r.Start(LogicalAxis::Inline, m), 10);
        assert_eq!(r.End(LogicalAxis::Block, m), 60);
        assert_eq!(r.Size__LogicalAxis_WritingMode(LogicalAxis::Inline, m), 30);
        assert_eq!(r.LineLeft(m, c), 10);
        assert_eq!(r.LineRight(m, c), 40);
        // rtl: line-left is measured from the container's left edge to the rect's inline-end.
        let rtl = horizontal_rtl();
        let rr = LogicalRect::new(rtl, 10, 20, 30, 40);
        assert_eq!(rr.LineLeft(rtl, c), 100 - 40);
        assert_eq!(rr.LineRight(rtl, c), 100 - 10);
        // vertical rtl: the container's inline size is its height.
        let v = vertical_rl_rtl();
        let rv = LogicalRect::new(v, 10, 20, 30, 40);
        assert_eq!(rv.LineLeft(v, c), 60 - 40);
        assert_eq!(rv.LineRight(v, c), 60 - 10);

        *r.ISize_mut(m) = 0;
        assert!(r.IsEmpty() && r.IsZeroSize() == false);
        *r.BSize_mut(m) = 0;
        assert!(r.IsZeroSize() && !r.IsAllZero());
        *r.IStart_mut(m) = 0;
        *r.BStart_mut(m) = 0;
        assert!(r.IsAllZero());
        *r.Start_mut(LogicalAxis::Inline, m) = 5;
        *r.Size__LogicalAxis_WritingMode_mut(LogicalAxis::Block, m) = 6;
        assert_eq!((r.IStart(m), r.BSize(m)), (5, 6));
        r.SetEmpty();
        assert!(r.IsEmpty());

        let origin = LogicalPoint::new(m, 3, 4);
        let size = LogicalSize::new(m, 7, 8);
        let q = LogicalRect::new__WritingMode_LogicalPoint_LogicalSize(m, origin, size);
        assert!(q.IsEqualEdges(LogicalRect::new(m, 3, 4, 7, 8)));
        assert_eq!(q.Origin(m), origin);
        assert_eq!(q.Size__WritingMode(m), size);
        let mut w = LogicalRect::new__WritingMode(m);
        assert!(w.IsAllZero());
        w.SetOrigin(m, origin);
        assert_eq!((w.IStart(m), w.BStart(m)), (3, 4));
        assert_eq!(format!("{}", q), "(3,4,7,8)");
    }

    #[test]
    fn LogicalRect_PointOperatorsAndMoveBy() {
        let m = vertical_rl();
        let r = LogicalRect::new(m, 10, 20, 30, 40);
        let d = LogicalPoint::new(m, 1, 2);
        let a = r + d;
        assert!(a.IsEqualEdges(LogicalRect::new(m, 11, 22, 30, 40)));
        let b = r - d;
        assert!(b.IsEqualEdges(LogicalRect::new(m, 9, 18, 30, 40)));
        let mut c = r;
        c += d;
        assert!(c.IsEqualEdges(a));
        c -= d;
        assert!(c.IsEqualEdges(r));
        c.MoveBy(m, d);
        assert!(c.IsEqualEdges(a));
    }

    #[test]
    fn LogicalRect_InflateDeflate() {
        let m = vertical_lr();
        let mut r = LogicalRect::new(m, 10, 20, 30, 40);
        r.Inflate__nscoord(2);
        assert!(r.IsEqualEdges(LogicalRect::new(m, 8, 18, 34, 44)));
        r.Deflate__nscoord(2);
        assert!(r.IsEqualEdges(LogicalRect::new(m, 10, 20, 30, 40)));
        r.Inflate__nscoord_nscoord(1, 3);
        assert!(r.IsEqualEdges(LogicalRect::new(m, 9, 17, 32, 46)));
        r.Deflate__nscoord_nscoord(1, 3);
        assert!(r.IsEqualEdges(LogicalRect::new(m, 10, 20, 30, 40)));
        // BStart, IEnd, BEnd, IStart
        let margin = LogicalMargin::new(m, 1, 2, 3, 4);
        r.Inflate__WritingMode_LogicalMargin(m, margin);
        assert!(r.IsEqualEdges(LogicalRect::new(m, 6, 19, 36, 44)));
        r.Deflate__WritingMode_LogicalMargin(m, margin);
        assert!(r.IsEqualEdges(LogicalRect::new(m, 10, 20, 30, 40)));
        // Deflating by more than the size clamps to zero.
        r.Deflate__nscoord(100);
        assert_eq!((r.ISize(m), r.BSize(m)), (0, 0));
    }

    #[test]
    fn LogicalRect_IntersectRect() {
        let m = horizontal_ltr();
        let a = LogicalRect::new(m, 0, 0, 10, 10);
        let b = LogicalRect::new(m, 5, 5, 10, 10);
        let mut out = LogicalRect::new__WritingMode(m);
        assert!(out.IntersectRect(a, b));
        assert!(out.IsEqualEdges(LogicalRect::new(m, 5, 5, 5, 5)));
        let far = LogicalRect::new(m, 50, 50, 5, 5);
        assert!(!out.IntersectRect(a, far));
        assert!(out.IsZeroSize());
    }

    #[test]
    fn StyleRect_LogicalAccessors() {
        use mrt_style::style::values::generics::rect::Rect;
        // top, right, bottom, left
        let mut r = Rect::new(1, 2, 3, 4);
        assert_eq!(*r.Get__Side(eSideLeft), 4);
        let h = horizontal_ltr();
        assert_eq!(*r.GetIStart(h), 4);
        assert_eq!(*r.GetIEnd(h), 2);
        assert_eq!(*r.GetBStart(h), 1);
        assert_eq!(*r.GetBEnd(h), 3);
        let v = vertical_rl();
        assert_eq!(*r.GetBStart(v), 2); // right
        assert_eq!(*r.GetIStart(v), 1); // top
        assert_eq!(*r.Start(LogicalAxis::Block, v), 2);
        assert_eq!(*r.End(LogicalAxis::Inline, v), 3);
        *r.GetIStart_mut(h) = 40;
        *r.Get__LogicalSide_WritingMode_mut(LogicalSide::BEnd, h) = 30;
        assert_eq!((r.0, r.1, r.2, r.3), (1, 2, 30, 40));
    }

    #[test]
    fn Unknown_And_IgnoreSideways() {
        assert_eq!(WritingMode::Unknown().GetBits(), 0xff);
        #[cfg(debug_assertions)]
        {
            // Sideways flags do not make two modes different for CHECK_WRITING_MODE.
            assert_eq!(sideways_lr().IgnoreSideways(), vertical_lr_not_inverted());
        }
    }

    #[cfg(debug_assertions)]
    fn vertical_lr_not_inverted() -> WritingMode {
        wm((StyleWritingMode::VERTICAL | StyleWritingMode::VERTICAL_LR | StyleWritingMode::INLINE_REVERSED).bits())
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "writing-mode mismatch")]
    fn CheckWritingMode_Mismatch_Asserts() {
        let r = LogicalRect::new(horizontal_ltr(), 0, 0, 1, 1);
        let _ = r.IStart(vertical_rl());
    }
}
