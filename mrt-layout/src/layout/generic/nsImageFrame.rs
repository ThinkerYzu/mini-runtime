/* rendering object for replaced elements with image data */
// Translated from layout/generic/nsImageFrame.h and nsImageFrame.cpp (GECKO_REV), only the choice of
// frame class for an <img> (ImageFrameTypeFor and what it reads).
//
// ADAPTER: the mini runtime loads no images (the Firefox harness it is compared with blocks them:
// `permissions.default.image` = 2), so every <img> element is in the BROKEN state (what
// nsImageLoadingContent::UpdateImageState sets when there is no current request). `ImageOk` is
// therefore always false for an <img>.
//
// NOT PORTED: the frame class itself (nsImageFrame: loading, decoding, sizing from the image's
// intrinsic size, painting, the broken-image icon, the alt-text feedback, image maps, selection), and
// the `content: url()` case (ShouldCreateImageFrameForContentProperty: the servo configuration of
// Stylo has no image `content` on elements; the generated-image content is not ported).
use crate::*;
use mrt_style::style::servo_arc::Arc as ServoArc;

// Gecko: layout/generic/nsImageFrame.h nsImageFrame::ImageFrameType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum nsImageFrame_ImageFrameType {
    // Image frame for an element with a `content: <image>` property.
    ForContentProperty,
    // Image frame for the image request of the element (src, srcset, ...).
    ForElementRequest,
    // No image frame at all: the element is made into an inline (and its alt text shows).
    None,
}

// Gecko: layout/generic/nsImageFrame.cpp HaveSpecifiedSize
// test if the width and height are fixed, looking at the style data
// This is used by nsImageFrame::ImageFrameTypeFor and should not be used for
// layout decisions.
fn HaveSpecifiedSize(aStylePosition: &nsStylePosition, aParams: &AnchorPosResolutionParams) -> bool {
    // check the width and height values in the reflow input's style struct
    // - if width and height are specified as either coord or percentage, then
    //   the size of the image frame is constrained
    aStylePosition.GetWidth(aParams).IsLengthPercentage() && aStylePosition.GetHeight(aParams).IsLengthPercentage()
}

// Gecko: layout/generic/nsImageFrame.cpp ImageOk
// ADAPTER: no image is ever loaded (see the top of this file), so the element is always broken.
fn ImageOk(aBroken: bool) -> bool {
    !aBroken
}

// Gecko: layout/generic/nsImageFrame.cpp HasAltText
// ADAPTER: the element is an <img> (an <input type=image> is not given alt text by this runtime, see
// nsCSSFrameConstructor::FindInputData), read through the frame constructor.
fn HasAltText(aFrameConstructor: &nsCSSFrameConstructor, aElement: ContentId) -> bool {
    // We always return some alternate text for <input>, see
    // nsCSSFrameConstructor::GetAlternateTextFor.
    if aFrameConstructor.IsHTMLElement(aElement, "input") {
        return true;
    }

    MOZ_ASSERT!(aFrameConstructor.IsHTMLElement(aElement, "img"));
    // aElement.HasNonEmptyAttr(nsGkAtoms::alt)
    aFrameConstructor.GetAttr(aElement, "alt").map_or(false, |alt| !alt.is_empty())
}

impl nsImageFrame {
    // Gecko: layout/generic/nsImageFrame.cpp nsImageFrame::ImageFrameTypeFor
    // Check if we want to use an image frame or just let the frame constructor make
    // us into an inline, and if so, which kind of image frame should we create.
    // ADAPTER: takes the frame constructor, which is how the DOM is read here.
    pub fn ImageFrameTypeFor(aFrameConstructor: &nsCSSFrameConstructor, aElement: ContentId, aStyle: &ServoArc<ComputedStyle>) -> nsImageFrame_ImageFrameType {
        // ShouldCreateImageFrameForContentProperty: NOT PORTED (see the top of this file).

        // ADAPTER: an <img> never loads, so it is broken (a lazily loaded one is not, until it is
        // scrolled to: the harness never scrolls, but a page can set loading=lazy; those stay
        // broken here too, which matches what the harness shows once the lazy load fails).
        let broken = true;
        if ImageOk(broken) {
            // Image is fine or loading; do the image frame thing
            return nsImageFrame_ImageFrameType::ForElementRequest;
        }

        // aStyle.StyleUIReset()->mMozForceBrokenImageIcon: NOT PORTED (the servo configuration of
        // Stylo has no -moz-force-broken-image-icon; no sheet of Firefox sets it either).

        if !HasAltText(aFrameConstructor, aElement) {
            return nsImageFrame_ImageFrameType::ForElementRequest;
        }

        // FIXME(emilio, bug 1788767): We definitely don't reframe when
        // HaveSpecifiedSize changes...
        if nsPresContext::Current().CompatibilityMode() == eCompatibility_NavQuirks && HaveSpecifiedSize(&nsStylePosition::new(aStyle.clone()), &AnchorPosResolutionParams) {
            return nsImageFrame_ImageFrameType::ForElementRequest;
        }

        nsImageFrame_ImageFrameType::None
    }
}

// Gecko: layout/generic/nsImageFrame.h nsImageFrame
// ADAPTER: only the static functions are ported, so the class is an empty marker.
pub struct nsImageFrame;
