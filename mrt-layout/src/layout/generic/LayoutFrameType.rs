// Gecko: layout/generic/nsIFrame.h mozilla::LayoutFrameType
// The enum is generated in Gecko from layout/generic/FrameClasses.py (GenerateFrameLists.py writes
// FrameTypeList.h, one FRAME_TYPE(type, first class, last class) line per group, largest group first). This
// file lists the same types in the same order (generated with GenerateFrameLists.grouped_frame_classes() at
// GECKO_REV); when FrameClasses.py gains a type, add it here.
use crate::*;

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum LayoutFrameType {
    None, // nsIFrame, nsHTMLFramesetBlankFrame, nsHTMLFramesetBorderFrame, ...
    Block, // nsBlockFrame, ComboboxLabelFrame, FileControlLabelFrame, ...
    SimpleXULLeaf, // SimpleXULLeafFrame, nsScrollbarButtonFrame, nsSplitterFrame, ...
    TableCell, // nsBCTableCellFrame, nsMathMLmtdFrame, nsTableCellFrame
    HTMLVideo, // nsVideoFrame, nsAudioFrame
    Inline, // nsInlineFrame, nsMathMLmathInlineFrame
    Letter, // nsFirstLetterFrame, nsFloatingFirstLetterFrame
    Table, // nsMathMLmtableFrame, nsTableFrame
    TableRow, // nsMathMLmtrFrame, nsTableRowFrame
    TableWrapper, // nsMathMLmtableWrapperFrame, nsTableWrapperFrame
    Text, // nsContinuingTextFrame, nsTextFrame
    Br, // BRFrame
    ColumnSetWrapper, // ColumnSetWrapperFrame
    InputButtonControl, // InputButtonControlFrame
    MiddleCroppingLabel, // MiddleCroppingLabelFrame
    PrintedSheet, // PrintedSheetFrame
    SVGA, // SVGAFrame
    SVGClipPath, // SVGClipPathFrame
    SVGFEContainer, // SVGFEContainerFrame
    SVGFEImage, // SVGFEImageFrame
    SVGFELeaf, // SVGFELeafFrame
    SVGFEUnstyledLeaf, // SVGFEUnstyledLeafFrame
    SVGFilter, // SVGFilterFrame
    SVGForeignObject, // SVGForeignObjectFrame
    SVGG, // SVGGFrame
    SVGGeometry, // SVGGeometryFrame
    SVGImage, // SVGImageFrame
    SVGInnerSVG, // SVGInnerSVGFrame
    SVGLinearGradient, // SVGLinearGradientFrame
    SVGMarkerAnonChild, // SVGMarkerAnonChildFrame
    SVGMarker, // SVGMarkerFrame
    SVGMask, // SVGMaskFrame
    SVGOuterSVGAnonChild, // SVGOuterSVGAnonChildFrame
    SVGOuterSVG, // SVGOuterSVGFrame
    SVGPattern, // SVGPatternFrame
    SVGRadialGradient, // SVGRadialGradientFrame
    SVGStop, // SVGStopFrame
    SVGSwitch, // SVGSwitchFrame
    SVGSymbol, // SVGSymbolFrame
    SVGText, // SVGTextFrame
    SVGUse, // SVGUseFrame
    SVGView, // SVGViewFrame
    ScrollContainer, // ScrollContainerFrame
    Viewport, // ViewportFrame
    Wbr, // WBRFrame
    Canvas, // nsCanvasFrame
    CheckboxRadio, // nsCheckboxRadioFrame
    ColorControl, // nsColorControlFrame
    ColumnSet, // nsColumnSetFrame
    ComboboxControl, // nsComboboxControlFrame
    DateTimeControl, // nsDateTimeControlFrame
    FieldSet, // nsFieldSetFrame
    FileControl, // nsFileControlFrame
    Line, // nsFirstLineFrame
    FlexContainer, // nsFlexContainerFrame
    GridContainer, // nsGridContainerFrame
    HTMLCanvas, // nsHTMLCanvasFrame
    FrameSet, // nsHTMLFramesetFrame
    ImageControl, // nsImageControlFrame
    Image, // nsImageFrame
    ListControl, // nsListControlFrame
    MenuPopup, // nsMenuPopupFrame
    PageBreak, // nsPageBreakFrame
    PageContent, // nsPageContentFrame
    Page, // nsPageFrame
    PageSequence, // nsPageSequenceFrame
    Placeholder, // nsPlaceholderFrame
    Progress, // nsProgressFrame
    Range, // nsRangeFrame
    RubyBaseContainer, // nsRubyBaseContainerFrame
    RubyBase, // nsRubyBaseFrame
    Ruby, // nsRubyFrame
    RubyTextContainer, // nsRubyTextContainerFrame
    RubyText, // nsRubyTextFrame
    Scrollbar, // nsScrollbarFrame
    Slider, // nsSliderFrame
    SubDocument, // nsSubDocumentFrame
    TableCol, // nsTableColFrame
    TableColGroup, // nsTableColGroupFrame
    TableRowGroup, // nsTableRowGroupFrame
    TextInput, // nsTextControlFrame
}
