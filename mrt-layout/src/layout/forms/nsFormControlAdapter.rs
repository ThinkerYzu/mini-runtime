// ADAPTER (not a Firefox file): what the form control frames read from the DOM.
//
// Gecko's text control, combobox and list control frames ask their element (HTMLInputElement::GetCols,
// HTMLTextAreaElement::GetRows, HTMLSelectElement::Size, the options of the select) and get their
// children (the anonymous editor, the option frames) from the frame constructor. This runtime builds
// none of that: the frame constructor reads the attributes and the options when it makes the frame and
// stores them here, keyed by the frame, for the Reflow and IntrinsicISize of the frame classes in this
// directory.
use crate::*;
use std::cell::RefCell;
use std::collections::HashMap;

// ADAPTER: one <option> of a <select> (or the label of an <optgroup>).
#[derive(Clone, Debug, Default)]
pub struct FormControlOption {
    // HTMLOptionElement::GetRenderedLabel: the `label` attribute if it is not empty, else the text
    // with the whitespace stripped and collapsed. For an <optgroup>, its `label` attribute.
    pub mLabel: String,
    pub mIsOptGroupLabel: bool,
    // padding-inline-start + padding-inline-end of the option, in app units (the UA sheet gives 4px +
    // 4px, and 20px + 4px inside an <optgroup>)
    pub mPaddingISize: nscoord,
    // padding-block-start + padding-block-end, in app units (2px + 2px)
    pub mPaddingBSize: nscoord,
}

// ADAPTER: the attributes and the options that the form control frames read.
#[derive(Clone, Debug, Default)]
pub struct FormControlData {
    // HTMLInputElement::GetCols / HTMLTextAreaElement::GetCols (`size` of an <input>, `cols` of a
    // <textarea>): None when the attribute is missing or not a positive integer (a text area
    // keeps a non-positive `cols` as Some, as Gecko does; CalcFixedSize clamps nothing).
    pub mCols: Option<i32>,
    // HTMLInputElement::GetRows (DEFAULT_ROWS, 1) / HTMLTextAreaElement::GetRows (`rows`, default 2)
    pub mRows: i32,
    pub mIsTextArea: bool,
    // HTMLSelectElement::Size (the `size` attribute; 0 when absent) and Multiple
    pub mSelectSize: i32,
    pub mSelectMultiple: bool,
    pub mOptions: Vec<FormControlOption>,
}

thread_local! {
    static FORM_CONTROL_DATA: RefCell<HashMap<u32, FormControlData>> = RefCell::new(HashMap::new());
}

// ADAPTER: drops the data of every form control frame (stage 4; see DestroyFrameTree in driver.rs).
pub fn ResetFormControlData() {
    FORM_CONTROL_DATA.with(|m| *m.borrow_mut() = HashMap::new());
}

// ADAPTER: drops the data of these frames, whose slots are about to be reused (see ReleaseFrameSlots
// in driver.rs).
pub fn ReleaseFormControlData(aFrames: &[nsIFrame]) {
    FORM_CONTROL_DATA.with(|m| {
        let mut m = m.borrow_mut();
        for f in aFrames {
            m.remove(&f.0);
        }
    });
}

impl nsIFrame {
    // ADAPTER: stores what the frame constructor read for this form control frame.
    pub fn SetFormControlData(&self, aData: FormControlData) {
        FORM_CONTROL_DATA.with(|m| m.borrow_mut().insert(self.0, aData));
    }

    // ADAPTER: what the frame constructor read for this form control frame (the defaults if it set none).
    pub fn FormControlData(&self) -> FormControlData {
        FORM_CONTROL_DATA.with(|m| m.borrow().get(&self.0).cloned().unwrap_or_default())
    }
}
