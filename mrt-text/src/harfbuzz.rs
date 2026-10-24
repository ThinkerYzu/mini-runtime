//! The few HarfBuzz and shim entry points the Rust side calls (declared by hand, no bindgen).
use std::os::raw::{c_char, c_int, c_void};

pub enum hb_blob_t {}
pub enum hb_face_t {}
pub enum hb_font_t {}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct mrt_glyph {
    pub glyph: u32,
    pub cluster: u32,
    pub x_advance: i32,
    pub y_advance: i32,
    pub x_offset: i32,
    pub y_offset: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct hb_glyph_extents_t {
    pub x_bearing: i32,
    pub y_bearing: i32,
    pub width: i32,
    pub height: i32,
}

unsafe extern "C" {
    pub fn hb_blob_create_from_file_or_fail(file_name: *const c_char) -> *mut hb_blob_t;
    pub fn hb_blob_destroy(blob: *mut hb_blob_t);
    pub fn hb_blob_get_data(blob: *mut hb_blob_t, length: *mut u32) -> *const c_char;
    pub fn hb_face_create(blob: *mut hb_blob_t, index: u32) -> *mut hb_face_t;
    pub fn hb_face_destroy(face: *mut hb_face_t);
    pub fn hb_face_get_upem(face: *const hb_face_t) -> u32;
    pub fn hb_face_reference_table(face: *const hb_face_t, tag: u32) -> *mut hb_blob_t;
    pub fn hb_font_destroy(font: *mut hb_font_t);
    pub fn hb_font_get_nominal_glyph(font: *mut hb_font_t, unicode: u32, glyph: *mut u32) -> c_int;
    pub fn hb_font_get_glyph_h_advance(font: *mut hb_font_t, glyph: u32) -> i32;
    pub fn hb_font_get_glyph_extents(font: *mut hb_font_t, glyph: u32, extents: *mut hb_glyph_extents_t) -> c_int;

    pub fn mrt_create_hb_font(face: *mut hb_face_t, size: f64) -> *mut hb_font_t;
    pub fn mrt_shape_utf16(
        font: *mut hb_font_t, text: *const u16, length: u32, rtl: c_int, language: *const c_char,
        out: *mut mrt_glyph, capacity: u32,
    ) -> i32;
    pub fn mrt_hb_version() -> *const c_char;
}

pub fn version() -> String {
    unsafe { std::ffi::CStr::from_ptr(mrt_hb_version()).to_string_lossy().into_owned() }
}

pub const fn HB_TAG(a: u8, b: u8, c: u8, d: u8) -> u32 {
    ((a as u32) << 24) | ((b as u32) << 16) | ((c as u32) << 8) | d as u32
}

// Keep the compiler from warning about the unused import on c_void.
const _: *const c_void = std::ptr::null();
