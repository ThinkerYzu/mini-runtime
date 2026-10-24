//! Native WebGL (GAPS B169): the runtime's WebGL batches (glue/webgl.js) run on real OpenGL ES 3
//! contexts in this process, so a page gets real answers (shader status, links, locations, limits,
//! pixels) with no browser attached. One EGL context per WebGL context, on Mesa's surfaceless
//! platform (llvmpipe by default: the renderer Firefox has under Xvfb).
//!
//! The request and reply are the browser executor's (bench/http-viewer/webgl.js):
//! {cmds: [[ctx id, extension or 0, method, arguments, id made or 0], ...], bin, sync} ->
//! {result, outs, throws, lost, lastThrew}. Arguments are its JSON forms ({$o} objects, {$v} views
//! of `bin`, {$out} arrays the call fills, {$img}, {$canvas}, {$blank}, {$imageData}).
//!
//! What WebGL adds over ES 3 is done here, as Gecko's WebGLContext does it: the default framebuffer
//! (a framebuffer object of the canvas's size with the context's attributes, multisampled and
//! resolved before reads when antialias), the `null` framebuffer meaning it, objects by the runtime's
//! ids (a deleted or foreign one is INVALID_OPERATION), uniform locations tied to their program's
//! link, the UNPACK_*_WEBGL parameters, WebGL 1's unsized float, half-float, sRGB and depth formats,
//! the identity strings, the extensions Firefox exposes on the same renderer, the feedback-loop
//! check, and the sizes of the arrays a call reads or fills (a short one is INVALID_OPERATION, and
//! GL never reads or writes past it). DOM sources are decoded here: images fetched through this
//! process's network and cookie jar with the page's origin (a cross-origin one without CORS is
//! tainted: SecurityError), another WebGL canvas's drawing buffer, ImageData.
//!
//! Not built: GLSL validation and translation (ANGLE's): shaders go to the driver as written, so
//! info logs are Mesa's and a shader WebGL forbids may compile; SVG images; video frames; OffscreenCanvas
//! and workers (B130).
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString, c_void};
use std::ptr;
use std::rc::Rc;

use base64::Engine;
use serde_json::{Map, Value, json};

use crate::gl::*;
use crate::{fetch, store::Stores};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;
/// the largest array a call fills (readPixels, getBufferSubData), as the bridge's batch limit
const MAX_OUTPUT: usize = 64 << 20;
/// Gecko's kMaxClientWaitSyncTimeoutNS: MAX_CLIENT_WAIT_TIMEOUT_WEBGL, the longest clientWaitSync
const MAX_CLIENT_WAIT_NS: u64 = 1_000_000_000;

const NO_ERROR: u32 = 0;
const INVALID_ENUM: u32 = 0x0500;
const INVALID_VALUE: u32 = 0x0501;
const INVALID_OPERATION: u32 = 0x0502;

const FRAMEBUFFER: u32 = 0x8D40;
const READ_FRAMEBUFFER: u32 = 0x8CA8;
const DRAW_FRAMEBUFFER: u32 = 0x8CA9;
const RENDERBUFFER: u32 = 0x8D41;
const COLOR_ATTACHMENT0: u32 = 0x8CE0;
const DEPTH_ATTACHMENT: u32 = 0x8D00;
const STENCIL_ATTACHMENT: u32 = 0x8D20;
const DEPTH_STENCIL_ATTACHMENT: u32 = 0x821A;
const BACK: u32 = 0x0405;
const SCISSOR_TEST: u32 = 0x0C11;
const RASTERIZER_DISCARD: u32 = 0x8C89;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Kind {
    Buffer,
    Framebuffer,
    Program,
    Renderbuffer,
    Shader,
    Texture,
    VertexArray,
    Query,
    Sampler,
    Sync,
    TransformFeedback,
}

impl Kind {
    fn iface(self) -> &'static str {
        match self {
            Kind::Buffer => "WebGLBuffer",
            Kind::Framebuffer => "WebGLFramebuffer",
            Kind::Program => "WebGLProgram",
            Kind::Renderbuffer => "WebGLRenderbuffer",
            Kind::Shader => "WebGLShader",
            Kind::Texture => "WebGLTexture",
            Kind::VertexArray => "WebGLVertexArrayObject",
            Kind::Query => "WebGLQuery",
            Kind::Sampler => "WebGLSampler",
            Kind::Sync => "WebGLSync",
            Kind::TransformFeedback => "WebGLTransformFeedback",
        }
    }
}

struct Obj {
    kind: Kind,
    name: u32,
    sync: GLsync,
    deleted: bool,
}

/// a uniform location: its program (runtime id), the link it came from, GL's location and type
struct Loc {
    program: String,
    link: u32,
    loc: i32,
    ty: u32,
}

#[derive(Default)]
struct Prog {
    link: u32,
    /// shaders in attachment order: (shader type, runtime id)
    attached: Vec<(u32, String)>,
    /// the sampler uniforms of the last successful link: (GL location, sampler type)
    samplers: Vec<(i32, u32)>,
}

/// An image source's pixels: straight alpha, rows top down
pub struct Pixels {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
    tainted: bool,
}

enum Fail {
    Throw(&'static str, String),
    Lost,
}

fn throw(name: &'static str, msg: impl Into<String>) -> Fail {
    Fail::Throw(name, msg.into())
}

struct Ctx {
    egl: EGLContext,
    webgl2: bool,
    attrs: Map<String, Value>,
    w: i32,
    h: i32,
    fbo: u32,
    color: u32,
    ds: u32,
    resolve: u32,
    resolve_color: u32,
    samples: i32,
    objs: HashMap<String, Obj>,
    /// (kind, GL name) -> the runtime id, for answers that name an object
    rev: HashMap<(Kind, u32), Value>,
    locs: HashMap<String, Loc>,
    progs: HashMap<u32, Prog>,
    /// a framebuffer's texture attachments: GL framebuffer -> attachment -> GL texture
    fb_tex: HashMap<u32, HashMap<u32, u32>>,
    /// a 2D or cube texture's level 0 size, for WebGL 1's rule on textures of other sizes than powers of two
    tex_size: HashMap<u32, (i32, i32)>,
    /// textures that sample as (0, 0, 0, 1), bound in place of incomplete ones: 2D, cube
    black: [u32; 2],
    /// a renderbuffer's format as the page gave it (WebGL 1's DEPTH_STENCIL is DEPTH24_STENCIL8 in GL)
    rb_format: HashMap<u32, u32>,
    error: u32,
    flip_y: bool,
    premul: bool,
    colorspace: i32,
    unpack: [i32; 6], // alignment, row length, image height, skip pixels, skip rows, skip images
    pack: [i32; 4],   // alignment, row length, skip pixels, skip rows
    exts: HashSet<String>,
    supported: Vec<String>,
    draw_fb: Option<String>,
    read_fb: Option<String>,
    program: Option<String>,
}

pub struct Gl {
    dpy: EGLDisplay,
    config: EGLConfig,
    ready: Option<bool>,
    ctxs: HashMap<i64, Ctx>,
    current: i64,
    images: HashMap<String, Option<Rc<Pixels>>>,
    gl_exts: HashSet<String>,
}

impl Default for Gl {
    fn default() -> Self {
        Gl { dpy: ptr::null_mut(), config: ptr::null_mut(), ready: None, ctxs: HashMap::new(), current: 0, images: HashMap::new(), gl_exts: HashSet::new() }
    }
}

/// Picks Mesa's software renderer unless MRT_GL=default (or glvnd is already told otherwise): the
/// renderer of Firefox under Xvfb, and the same answers on every machine. Called before any thread.
pub fn prefer_software() {
    if std::env::var("MRT_GL").as_deref() == Ok("default") {
        return;
    }
    let mesa = "/usr/share/glvnd/egl_vendor.d/50_mesa.json";
    // SAFETY: main calls this first, before the process has another thread
    unsafe {
        if std::env::var_os("__EGL_VENDOR_LIBRARY_FILENAMES").is_none() && std::path::Path::new(mesa).exists() {
            std::env::set_var("__EGL_VENDOR_LIBRARY_FILENAMES", mesa);
        }
        if std::env::var_os("LIBGL_ALWAYS_SOFTWARE").is_none() {
            std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");
        }
    }
}

// ---- arguments ----

struct View<'a> {
    ty: &'a str,
    bytes: &'a [u8],
}

fn elem_size(ty: &str) -> usize {
    match ty {
        "Int16Array" | "Uint16Array" | "Float16Array" => 2,
        "Int32Array" | "Uint32Array" | "Float32Array" => 4,
        "Float64Array" | "BigInt64Array" | "BigUint64Array" => 8,
        _ => 1,
    }
}

struct A<'a> {
    v: &'a [Value],
    bin: &'a [u8],
}

static NULL: Value = Value::Null;

impl<'a> A<'a> {
    fn len(&self) -> usize {
        self.v.len()
    }
    fn get(&self, i: usize) -> &'a Value {
        self.v.get(i).unwrap_or(&NULL)
    }
    fn num(&self, i: usize) -> f64 {
        num_of(self.get(i))
    }
    fn u(&self, i: usize) -> u32 {
        self.num(i) as i64 as u32
    }
    fn i(&self, i: usize) -> i32 {
        self.num(i) as i64 as i32
    }
    fn f(&self, i: usize) -> f32 {
        self.num(i) as f32
    }
    fn b(&self, i: usize) -> u8 {
        match self.get(i) {
            Value::Bool(b) => *b as u8,
            v => (num_of(v) != 0.0) as u8,
        }
    }
    fn isz(&self, i: usize) -> isize {
        self.num(i) as i64 as isize
    }
    fn is_null(&self, i: usize) -> bool {
        matches!(self.get(i), Value::Null) || self.get(i).get("$u").is_some()
    }
    fn is_number(&self, i: usize) -> bool {
        let v = self.get(i);
        v.is_number() || v.get("$n").is_some()
    }
    fn view(&self, i: usize) -> Option<View<'a>> {
        let v = self.get(i);
        let ty = v.get("$v")?.as_str()?;
        let o = v.get("o")?.as_u64()? as usize;
        let b = v.get("b")?.as_u64()? as usize;
        let bytes = self.bin.get(o..o.checked_add(b)?)?;
        Some(View { ty, bytes })
    }
    fn obj(&self, i: usize) -> Option<String> {
        self.get(i).get("$o").map(Value::to_string)
    }
    fn is_source(&self, i: usize) -> bool {
        let v = self.get(i);
        ["$img", "$canvas", "$blank", "$imageData"].iter().any(|k| v.get(*k).is_some())
    }
    /// a Float32List (or Int32List, Uint32List): an array of numbers or a view, from `offset`
    /// elements, `len` of them (0: the rest)
    fn list<T: Copy>(&self, i: usize, offset: usize, len: usize, from_num: fn(f64) -> T, from_le: fn([u8; 4]) -> T) -> Option<Vec<T>> {
        let all: Vec<T> = match self.get(i) {
            Value::Array(items) => items.iter().map(|v| from_num(num_of(v))).collect(),
            _ => {
                let view = self.view(i)?;
                view.bytes.chunks_exact(4).map(|c| from_le([c[0], c[1], c[2], c[3]])).collect()
            }
        };
        let end = if len == 0 { all.len() } else { offset.checked_add(len)? };
        all.get(offset..end).map(<[T]>::to_vec)
    }
    fn floats(&self, i: usize, offset: usize, len: usize) -> Option<Vec<f32>> {
        self.list(i, offset, len, |n| n as f32, f32::from_le_bytes)
    }
    fn ints(&self, i: usize, offset: usize, len: usize) -> Option<Vec<i32>> {
        self.list(i, offset, len, |n| n as i64 as i32, i32::from_le_bytes)
    }
    fn uints(&self, i: usize, offset: usize, len: usize) -> Option<Vec<u32>> {
        self.list(i, offset, len, |n| n as i64 as u32, u32::from_le_bytes)
    }
    fn strs(&self, i: usize) -> Vec<String> {
        self.get(i).as_array().map(|a| a.iter().map(str_of).collect()).unwrap_or_default()
    }
    fn s(&self, i: usize) -> String {
        str_of(self.get(i))
    }
}

fn num_of(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::Bool(b) => *b as u8 as f64,
        Value::String(s) => s.trim().parse().unwrap_or(f64::NAN),
        Value::Object(o) => match o.get("$n").and_then(Value::as_str) {
            Some("-0") => -0.0,
            Some("Infinity") => f64::INFINITY,
            Some("-Infinity") => f64::NEG_INFINITY,
            Some(s) => s.parse().unwrap_or(f64::NAN),
            None => f64::NAN,
        },
        _ => 0.0,
    }
}

fn str_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        Value::Object(_) => "undefined".into(),
        v => v.to_string(),
    }
}

fn cstring(s: &str) -> CString {
    // a name with a NUL is no GLSL identifier: the driver finds nothing for an empty one either
    CString::new(s).unwrap_or_default()
}

fn typed(ty: &str, bytes: Vec<u8>) -> Value {
    json!({"$v": ty, "b64": B64.encode(bytes)})
}

fn f32s(v: &[f32]) -> Value {
    typed("Float32Array", v.iter().flat_map(|x| x.to_le_bytes()).collect())
}

fn i32s(v: &[i32]) -> Value {
    typed("Int32Array", v.iter().flat_map(|x| x.to_le_bytes()).collect())
}

fn u32s(v: &[u32]) -> Value {
    typed("Uint32Array", v.iter().flat_map(|x| x.to_le_bytes()).collect())
}

/// a number as JSON can hold it ({$n} for what it cannot, as the runtime's dec reads it)
fn number(n: f64) -> Value {
    if n.is_finite() && !(n == 0.0 && n.is_sign_negative()) {
        json!(n)
    } else {
        json!({"$n": if n.is_nan() { "NaN".to_string() } else if n == 0.0 { "-0".into() } else if n > 0.0 { "Infinity".into() } else { "-Infinity".into() }})
    }
}

// ---- GL queries ----

fn geti(p: u32) -> i32 {
    let mut v = 0;
    unsafe { glGetIntegerv(p, &mut v) };
    v
}

fn getiv<const N: usize>(p: u32) -> [i32; N] {
    let mut v = [0; N];
    unsafe { glGetIntegerv(p, v.as_mut_ptr()) };
    v
}

fn getfv<const N: usize>(p: u32) -> [f32; N] {
    let mut v = [0.0; N];
    unsafe { glGetFloatv(p, v.as_mut_ptr()) };
    v
}

fn enabled(cap: u32) -> bool {
    unsafe { glIsEnabled(cap) != 0 }
}

fn set_enabled(cap: u32, on: bool) {
    unsafe {
        if on {
            glEnable(cap)
        } else {
            glDisable(cap)
        }
    }
}

fn gl_str(p: *const u8) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p as *const i8) }.to_string_lossy().into_owned()
}

/// a GL info log or source of `len` bytes, read by `f`
fn gl_text(len: i32, f: impl FnOnce(i32, *mut i32, *mut i8)) -> String {
    let mut buf = vec![0u8; len.max(1) as usize];
    let mut n = 0;
    f(buf.len() as i32, &mut n, buf.as_mut_ptr() as *mut i8);
    buf.truncate(n.max(0) as usize);
    String::from_utf8_lossy(&buf).into_owned()
}

/// Gecko's SanitizeRenderer for the renderers this backend can have; others pass as they are
fn sanitize_renderer(raw: &str) -> String {
    if raw.starts_with("llvmpipe") {
        return "llvmpipe, or similar".into();
    }
    if raw.starts_with("softpipe") {
        return "softpipe, or similar".into();
    }
    raw.to_string()
}

// ---- pixel layout ----

fn channels(format: u32) -> Option<usize> {
    Some(match format {
        0x1908 | 0x8D99 | 0x8C42 => 4,               // RGBA, RGBA_INTEGER, SRGB_ALPHA_EXT
        0x1907 | 0x8D98 | 0x8C40 => 3,               // RGB, RGB_INTEGER, SRGB_EXT
        0x8227 | 0x8228 | 0x190A => 2,               // RG, RG_INTEGER, LUMINANCE_ALPHA
        0x1903 | 0x8D94 | 0x1909 | 0x1906 => 1,      // RED, RED_INTEGER, LUMINANCE, ALPHA
        0x1902 | 0x84F9 => 1,                        // DEPTH_COMPONENT, DEPTH_STENCIL
        _ => return None,
    })
}

/// bytes per pixel of a format and type
fn pixel_bytes(format: u32, ty: u32) -> Option<usize> {
    Some(match ty {
        0x8033 | 0x8034 | 0x8363 => 2,                   // 4444, 5551, 565
        0x8368 | 0x8C3B | 0x8C3E | 0x84FA => 4,          // 2_10_10_10_REV, 10F_11F_11F_REV, 5_9_9_9_REV, 24_8
        0x8DAD => 8,                                     // FLOAT_32_UNSIGNED_INT_24_8_REV
        0x1400 | 0x1401 => channels(format)?,            // BYTE, UNSIGNED_BYTE
        0x1402 | 0x1403 | 0x140B | 0x8D61 => 2 * channels(format)?, // (UNSIGNED_)SHORT, HALF_FLOAT(_OES)
        0x1404 | 0x1405 | 0x1406 => 4 * channels(format)?, // (UNSIGNED_)INT, FLOAT
        _ => return None,
    })
}

/// the view type an array for `ty` must have (Gecko's ValidateArrayBufferView for pixels)
fn view_matches(view: &str, ty: u32) -> bool {
    match ty {
        0x1401 => matches!(view, "Uint8Array" | "Uint8ClampedArray"),
        0x1400 => view == "Int8Array",
        0x1402 => view == "Int16Array",
        0x1403 | 0x8033 | 0x8034 | 0x8363 | 0x140B | 0x8D61 => view == "Uint16Array",
        0x1404 => view == "Int32Array",
        0x1405 | 0x8368 | 0x8C3B | 0x8C3E | 0x84FA => view == "Uint32Array",
        0x1406 => view == "Float32Array",
        _ => true,
    }
}

/// the bytes GL reads (or writes) for a w x h x d block with these pixel-store parameters
fn block_bytes(w: i32, h: i32, d: i32, bpp: usize, align: i32, row_len: i32, img_h: i32, skips: [i32; 3]) -> Option<usize> {
    if w <= 0 || h <= 0 || d <= 0 {
        return Some(0);
    }
    let align = align.max(1) as usize;
    let row = (if row_len > 0 { row_len } else { w }) as usize * bpp;
    let stride = row.div_ceil(align) * align;
    let img = stride * (if img_h > 0 { img_h } else { h }) as usize;
    let [px, rows, images] = skips.map(|s| s.max(0) as usize);
    images.checked_mul(img)?
        .checked_add((d as usize - 1).checked_mul(img)?)?
        .checked_add((h as usize - 1 + rows).checked_mul(stride)?)?
        .checked_add(px * bpp + w as usize * bpp)
}

fn f16(x: f32) -> u16 {
    let b = x.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let man = b & 0x7f_ffff;
    if exp <= 0 {
        if exp < -10 {
            return sign;
        }
        let m = (man | 0x80_0000) >> (1 - exp);
        return sign | ((m + 0x1000) >> 13) as u16;
    }
    if exp >= 31 {
        return sign | 0x7c00;
    }
    sign | (((exp as u32) << 10) + ((man + 0x1000) >> 13)) as u16
}

/// A DOM source's pixels in `format` and `ty`, w x h (the source's top-left block, zero beyond),
/// rows packed (UNPACK_ALIGNMENT 1), flipped and premultiplied as asked
fn convert(src: &Pixels, w: u32, h: u32, format: u32, ty: u32, flip: bool, premul: bool) -> Option<Vec<u8>> {
    let pick: &[usize] = match format {
        0x1908 | 0x8D99 | 0x8C42 => &[0, 1, 2, 3],
        0x1907 | 0x8D98 | 0x8C40 => &[0, 1, 2],
        0x8227 | 0x8228 => &[0, 1],
        0x1903 | 0x8D94 | 0x1909 => &[0],
        0x1906 => &[3],
        0x190A => &[0, 3],
        _ => return None,
    };
    let mut out = Vec::with_capacity((w * h) as usize * 4 * pick.len());
    for y in 0..h {
        let sy = if flip { h - 1 - y } else { y };
        for x in 0..w {
            let mut c = [0f32; 4];
            if x < src.w && sy < src.h {
                let i = ((sy * src.w + x) * 4) as usize;
                for k in 0..4 {
                    c[k] = src.rgba[i + k] as f32;
                }
                if premul {
                    for k in 0..3 {
                        c[k] = (c[k] * c[3] / 255.0).round();
                    }
                }
            }
            let q = |k: usize, bits: u32| (c[k] / 255.0 * ((1 << bits) - 1) as f32).round() as u16;
            match ty {
                0x1401 => out.extend(pick.iter().map(|&k| c[k] as u8)),
                0x1406 => out.extend(pick.iter().flat_map(|&k| (c[k] / 255.0).to_le_bytes())),
                0x140B | 0x8D61 => out.extend(pick.iter().flat_map(|&k| f16(c[k] / 255.0).to_le_bytes())),
                0x8033 if pick.len() == 4 => out.extend((q(0, 4) << 12 | q(1, 4) << 8 | q(2, 4) << 4 | q(3, 4)).to_le_bytes()),
                0x8034 if pick.len() == 4 => out.extend((q(0, 5) << 11 | q(1, 5) << 6 | q(2, 5) << 1 | q(3, 1)).to_le_bytes()),
                0x8363 if pick.len() == 3 => out.extend((q(0, 5) << 11 | q(1, 6) << 5 | q(2, 5)).to_le_bytes()),
                _ => return None,
            }
        }
    }
    Some(out)
}

/// WebGL 1's formats as ES 3 takes them: unsized float, half-float, sRGB and depth texture formats
/// become sized ones, HALF_FLOAT_OES becomes HALF_FLOAT
fn fix_formats(webgl2: bool, ifmt: u32, format: u32, ty: u32) -> (u32, u32, u32) {
    let ty = if ty == 0x8D61 { 0x140B } else { ty };
    if webgl2 && ifmt != format {
        return (ifmt, format, ty);
    }
    match (ifmt, ty) {
        (0x8C42, _) => (0x8C43, 0x1908, ty), // SRGB_ALPHA_EXT -> SRGB8_ALPHA8
        (0x8C40, _) => (0x8C41, 0x1907, ty), // SRGB_EXT -> SRGB8
        (0x1908, 0x1406) => (0x8814, format, ty),
        (0x1907, 0x1406) => (0x8815, format, ty),
        (0x1908, 0x140B) => (0x881A, format, ty),
        (0x1907, 0x140B) => (0x881B, format, ty),
        (0x1902, 0x1403) => (0x81A5, format, ty), // DEPTH_COMPONENT16
        (0x1902, 0x1405) => (0x81A6, format, ty), // DEPTH_COMPONENT24
        (0x84F9, 0x84FA) => (0x88F0, format, ty), // DEPTH24_STENCIL8
        _ => (ifmt, format, ty),
    }
}

fn sub_format(format: u32) -> u32 {
    match format {
        0x8C42 => 0x1908,
        0x8C40 => 0x1907,
        f => f,
    }
}

/// the uniform's values: (component kind, count) of a GL type; 'b' bool, 'f' float, 'i' int, 'u' uint
fn uniform_shape(ty: u32) -> (char, usize) {
    match ty {
        0x1406 => ('f', 1),
        0x8B50 => ('f', 2),
        0x8B51 => ('f', 3),
        0x8B52 => ('f', 4),
        0x1404 => ('i', 1),
        0x8B53 => ('i', 2),
        0x8B54 => ('i', 3),
        0x8B55 => ('i', 4),
        0x8B56 => ('b', 1),
        0x8B57 => ('b', 2),
        0x8B58 => ('b', 3),
        0x8B59 => ('b', 4),
        0x8B5A => ('f', 4),
        0x8B5B => ('f', 9),
        0x8B5C => ('f', 16),
        0x8B65 | 0x8B67 => ('f', 6),
        0x8B66 | 0x8B69 => ('f', 8),
        0x8B68 | 0x8B6A => ('f', 12),
        0x1405 => ('u', 1),
        0x8DC6 => ('u', 2),
        0x8DC7 => ('u', 3),
        0x8DC8 => ('u', 4),
        _ => ('i', 1), // samplers
    }
}

/// the texture binding a sampler type reads
fn sampler_binding(ty: u32) -> Option<u32> {
    Some(match ty {
        0x8B5E | 0x8B62 | 0x8DCA | 0x8DD2 => 0x8069,          // 2D
        0x8B60 | 0x8DC5 | 0x8DCC | 0x8DD4 => 0x8514,          // CUBE
        0x8B5F | 0x8DCB | 0x8DD3 => 0x806A,                   // 3D
        0x8DC1 | 0x8DC4 | 0x8DCF | 0x8DD7 => 0x8C1D,          // 2D_ARRAY
        _ => return None,
    })
}

// getParameter's answers by kind (Gecko's ClientWebGLContext::GetParameter)
const P_BOOL: &[u32] = &[0x0BE2, 0x0B44, 0x0B71, 0x0B72, 0x0BD0, 0x8037, 0x809E, 0x80A0, 0x80AB, 0x0C11, 0x0B90, 0x8C89, 0x8E24, 0x8E23, 0x864F];
const P_FLOAT: &[u32] = &[0x0B73, 0x0B21, 0x8038, 0x2A00, 0x80AA, 0x84FD, 0x84FF];
const P_INT64: &[u32] = &[0x8D6B, 0x9111, 0x8A30, 0x8A31, 0x8A33];
const P_WEBGL2: &[u32] = &[
    0x8CAA, 0x8F36, 0x8F37, 0x88ED, 0x88EF, 0x8C8F, 0x8A28, 0x8E25, 0x8919, 0x806A, 0x8C1D, 0x8073, 0x88FF, 0x8D6B, 0x9111, 0x8A2F,
    0x8A34, 0x8A30, 0x8A31, 0x8A33, 0x8B49, 0x8B4A, 0x8B4B, 0x9122, 0x9125, 0x8904, 0x8905, 0x84FD, 0x8D57, 0x0D02, 0x0D03, 0x0D04,
    0x0CF2, 0x0CF3, 0x0CF4, 0x806E, 0x806D, 0x8C89, 0x8E24, 0x8E23, 0x8C8A, 0x8C80, 0x8C8B, 0x8A2B, 0x8A2D, 0x8A2E, 0x0C02, 0x9247,
    0x8CA9,
];

fn compressed_formats(ext: &str) -> &'static [u32] {
    match ext {
        "WEBGL_compressed_texture_s3tc" => &[0x83F0, 0x83F1, 0x83F2, 0x83F3],
        "WEBGL_compressed_texture_s3tc_srgb" => &[0x8C4C, 0x8C4D, 0x8C4E, 0x8C4F],
        "WEBGL_compressed_texture_etc" => &[0x9270, 0x9271, 0x9272, 0x9273, 0x9274, 0x9275, 0x9276, 0x9277, 0x9278, 0x9279],
        "EXT_texture_compression_bptc" => &[0x8E8C, 0x8E8D, 0x8E8E, 0x8E8F],
        "EXT_texture_compression_rgtc" => &[0x8DBB, 0x8DBC, 0x8DBD, 0x8DBE],
        "WEBGL_compressed_texture_astc" => &[
            0x93B0, 0x93B1, 0x93B2, 0x93B3, 0x93B4, 0x93B5, 0x93B6, 0x93B7, 0x93B8, 0x93B9, 0x93BA, 0x93BB, 0x93BC, 0x93BD, 0x93D0, 0x93D1,
            0x93D2, 0x93D3, 0x93D4, 0x93D5, 0x93D6, 0x93D7, 0x93D8, 0x93D9, 0x93DA, 0x93DB, 0x93DC, 0x93DD,
        ],
        _ => &[],
    }
}

impl Ctx {
    fn err(&mut self, e: u32) {
        if self.error == NO_ERROR {
            self.error = e;
        }
    }

    /// GL's pending error kept as the context's, so that a query's own error can be told apart
    fn keep_gl_error(&mut self) {
        let e = unsafe { glGetError() };
        if e != NO_ERROR {
            self.err(e);
        }
    }

    /// GL raised an error for the call just made (it is the context's now)
    fn gl_failed(&mut self) -> bool {
        let e = unsafe { glGetError() };
        if e != NO_ERROR {
            self.err(e);
        }
        e != NO_ERROR
    }

    /// An object argument's GL name: 0 for null; INVALID_OPERATION (and None) for a deleted one,
    /// another context's, or another kind's
    fn name(&mut self, a: &A, i: usize, kind: Kind) -> Option<u32> {
        let Some(id) = a.obj(i) else { return Some(0) };
        match self.objs.get(&id) {
            Some(o) if o.kind == kind && !o.deleted => Some(o.name),
            _ => {
                self.err(INVALID_OPERATION);
                None
            }
        }
    }

    /// an object of this context, live, for is*() (false otherwise, no error)
    fn live(&self, a: &A, i: usize, kind: Kind) -> Option<&Obj> {
        let id = a.obj(i)?;
        self.objs.get(&id).filter(|o| o.kind == kind && !o.deleted)
    }

    fn made(&mut self, made: &Value, kind: Kind, name: u32, sync: GLsync) -> Value {
        if made.is_null() || made == &json!(0) {
            return Value::Null;
        }
        self.objs.insert(made.to_string(), Obj { kind, name, sync, deleted: false });
        if kind != Kind::Sync {
            self.rev.insert((kind, name), made.clone());
        }
        json!({"$o": made, "$t": kind.iface()})
    }

    fn named(&self, kind: Kind, name: u32) -> Value {
        if name == 0 {
            return Value::Null;
        }
        match self.rev.get(&(kind, name)) {
            Some(id) => json!({"$o": id, "$t": kind.iface()}),
            None => Value::Null,
        }
    }

    fn fb_name(&self, id: &Option<String>) -> u32 {
        id.as_ref().and_then(|i| self.objs.get(i)).map(|o| o.name).unwrap_or(self.fbo)
    }

    fn rebind(&self) {
        unsafe {
            glBindFramebuffer(DRAW_FRAMEBUFFER, self.fb_name(&self.draw_fb));
            glBindFramebuffer(READ_FRAMEBUFFER, self.fb_name(&self.read_fb));
        }
    }

    /// Runs `f` with GL reading from the drawing buffer's pixels when the read framebuffer is the
    /// default one (its multisampled pixels resolved first); bindings and state are restored after
    fn with_read<T>(&mut self, f: impl FnOnce() -> T) -> T {
        if self.read_fb.is_some() || self.samples == 0 {
            return f();
        }
        let scissor = enabled(SCISSOR_TEST);
        let discard = enabled(RASTERIZER_DISCARD);
        unsafe {
            glDisable(SCISSOR_TEST);
            glDisable(RASTERIZER_DISCARD);
            glBindFramebuffer(READ_FRAMEBUFFER, self.fbo);
            glBindFramebuffer(DRAW_FRAMEBUFFER, self.resolve);
            glBlitFramebuffer(0, 0, self.w, self.h, 0, 0, self.w, self.h, 0x4000, 0x2600);
            glBindFramebuffer(READ_FRAMEBUFFER, self.resolve);
            glBindFramebuffer(DRAW_FRAMEBUFFER, self.fb_name(&self.draw_fb));
        }
        set_enabled(SCISSOR_TEST, scissor);
        set_enabled(RASTERIZER_DISCARD, discard);
        let r = f();
        self.rebind();
        r
    }

    /// The drawing buffer's RGBA pixels, rows bottom up (GL's order), premultiplied when the context is
    fn read_drawing_buffer(&mut self) -> Vec<u8> {
        let (w, h) = (self.w, self.h);
        let mut out = vec![0u8; (w * h * 4) as usize];
        let saved_read = self.read_fb.take();
        let pack_buffer = if self.webgl2 { geti(0x88ED) } else { 0 };
        unsafe {
            if self.webgl2 {
                glBindBuffer(0x88EB, 0);
                glPixelStorei(0x0D02, 0);
                glPixelStorei(0x0D04, 0);
                glPixelStorei(0x0D03, 0);
            }
            glPixelStorei(0x0D05, 4);
        }
        self.rebind();
        self.with_read(|| unsafe { glReadPixels(0, 0, w, h, 0x1908, 0x1401, out.as_mut_ptr() as *mut c_void) });
        self.read_fb = saved_read;
        self.rebind();
        unsafe {
            if self.webgl2 {
                glBindBuffer(0x88EB, pack_buffer as u32);
                glPixelStorei(0x0D02, self.pack[1]);
                glPixelStorei(0x0D04, self.pack[2]);
                glPixelStorei(0x0D03, self.pack[3]);
            }
            glPixelStorei(0x0D05, self.pack[0]);
        }
        out
    }

    /// the drawing buffer as an image source: straight alpha, rows top down
    fn as_source(&mut self) -> Pixels {
        let raw = self.read_drawing_buffer();
        let (w, h) = (self.w as u32, self.h as u32);
        let premultiplied = self.attr("premultipliedAlpha") && self.attr("alpha");
        let mut rgba = Vec::with_capacity(raw.len());
        for y in (0..h).rev() {
            let row = &raw[(y * w * 4) as usize..((y + 1) * w * 4) as usize];
            for p in row.chunks_exact(4) {
                if premultiplied && p[3] != 0 && p[3] != 255 {
                    let a = p[3] as f32;
                    rgba.extend([0, 1, 2].map(|k| ((p[k] as f32 * 255.0 / a).round()).min(255.0) as u8));
                    rgba.push(p[3]);
                } else {
                    rgba.extend_from_slice(p);
                }
            }
        }
        Pixels { w, h, rgba, tainted: false }
    }

    fn attr(&self, k: &str) -> bool {
        self.attrs.get(k).and_then(Value::as_bool).unwrap_or(false)
    }

    /// The drawing buffer at `w` x `h` (clamped to the renderbuffer limit), cleared: at creation and
    /// on every canvas resize
    fn alloc(&mut self, w: i32, h: i32) {
        let max = geti(0x84E8).max(1);
        self.w = w.clamp(1, max);
        self.h = h.clamp(1, max);
        let (depth, stencil) = (self.attr("depth"), self.attr("stencil"));
        let color_fmt = if self.attr("alpha") { 0x8058 } else { 0x8051 }; // RGBA8, RGB8
        let bound_rb = geti(0x8CA7) as u32;
        unsafe {
            for rb in [self.color, self.ds, self.resolve_color] {
                if rb != 0 {
                    glDeleteRenderbuffers(1, &rb);
                }
            }
            let storage = |rb: &mut u32, fmt: u32, samples: i32| {
                glGenRenderbuffers(1, rb);
                glBindRenderbuffer(RENDERBUFFER, *rb);
                if samples > 0 {
                    glRenderbufferStorageMultisample(RENDERBUFFER, samples, fmt, self.w, self.h);
                } else {
                    glRenderbufferStorage(RENDERBUFFER, fmt, self.w, self.h);
                }
            };
            let (mut color, mut ds, mut resolve_color) = (0, 0, 0);
            storage(&mut color, color_fmt, self.samples);
            glBindFramebuffer(FRAMEBUFFER, self.fbo);
            glFramebufferRenderbuffer(FRAMEBUFFER, COLOR_ATTACHMENT0, RENDERBUFFER, color);
            if depth || stencil {
                let (fmt, at) = match (depth, stencil) {
                    (true, true) => (0x88F0, DEPTH_STENCIL_ATTACHMENT),
                    (true, false) => (0x81A6, DEPTH_ATTACHMENT),
                    _ => (0x8D48, STENCIL_ATTACHMENT),
                };
                storage(&mut ds, fmt, self.samples);
                glFramebufferRenderbuffer(FRAMEBUFFER, at, RENDERBUFFER, ds);
            }
            if self.samples > 0 {
                storage(&mut resolve_color, color_fmt, 0);
                glBindFramebuffer(FRAMEBUFFER, self.resolve);
                glFramebufferRenderbuffer(FRAMEBUFFER, COLOR_ATTACHMENT0, RENDERBUFFER, resolve_color);
            }
            glBindRenderbuffer(RENDERBUFFER, bound_rb);
            self.color = color;
            self.ds = ds;
            self.resolve_color = resolve_color;
        }
        self.clear_drawing_buffer();
    }

    /// clears the drawing buffer to zero whatever the context's clear state, which is kept
    fn clear_drawing_buffer(&mut self) {
        let color = getfv::<4>(0x0C22);
        let depth = getfv::<1>(0x0B73)[0];
        let stencil = geti(0x0B91);
        let mut mask = [0u8; 4];
        let mut dmask = 0u8;
        unsafe {
            glGetBooleanv(0x0C23, mask.as_mut_ptr());
            glGetBooleanv(0x0B72, &mut dmask);
        }
        let (sfront, sback) = (geti(0x0B98), geti(0x8CA5));
        let scissor = enabled(SCISSOR_TEST);
        let discard = enabled(RASTERIZER_DISCARD);
        unsafe {
            glDisable(SCISSOR_TEST);
            glDisable(RASTERIZER_DISCARD);
            glBindFramebuffer(DRAW_FRAMEBUFFER, self.fbo);
            glClearColor(0.0, 0.0, 0.0, 0.0);
            glClearDepthf(1.0);
            glClearStencil(0);
            glColorMask(1, 1, 1, 1);
            glDepthMask(1);
            glStencilMask(0xffff_ffff);
            glClear(0x4000 | 0x100 | 0x400);
            glClearColor(color[0], color[1], color[2], color[3]);
            glClearDepthf(depth);
            glClearStencil(stencil);
            glColorMask(mask[0], mask[1], mask[2], mask[3]);
            glDepthMask(dmask);
            glStencilMaskSeparate(0x0404, sfront as u32);
            glStencilMaskSeparate(0x0405, sback as u32);
        }
        set_enabled(SCISSOR_TEST, scissor);
        set_enabled(RASTERIZER_DISCARD, discard);
        self.rebind();
    }

    /// What WebGL checks and substitutes before a draw, over the textures the program samples: a
    /// texture attached to the draw framebuffer is a feedback loop (INVALID_OPERATION, and None: no
    /// draw); in WebGL 1 a texture whose size is not a power of two samples as black unless it
    /// clamps to edge without mipmaps (Gecko binds a black texture for it). Some: the bindings to
    /// put back after the draw (`end_draw`).
    fn begin_draw(&mut self) -> Option<Vec<(u32, u32, u32)>> {
        // an enabled attribute with no buffer would make ES 3 read a client array: memory of this
        // process at the page's offset. WebGL: INVALID_OPERATION
        for i in 0..geti(0x8869).clamp(0, 64) as u32 {
            let (mut on, mut buffer) = ([0i32; 4], [0i32; 4]);
            unsafe {
                glGetVertexAttribiv(i, 0x8622, on.as_mut_ptr());
                glGetVertexAttribiv(i, 0x889F, buffer.as_mut_ptr());
            }
            if on[0] != 0 && buffer[0] == 0 {
                self.err(INVALID_OPERATION);
                return None;
            }
        }
        let Some(prog) = self.program.as_ref().and_then(|p| self.objs.get(p)).map(|o| o.name) else { return Some(vec![]) };
        let samplers = match self.progs.get(&prog) {
            Some(p) if !p.samplers.is_empty() => p.samplers.clone(),
            _ => return Some(vec![]),
        };
        let fb = self.fb_name(&self.draw_fb);
        let atts: Vec<u32> = if self.draw_fb.is_some() { self.fb_tex.get(&fb).map(|m| m.values().copied().collect()).unwrap_or_default() } else { vec![] };
        if atts.is_empty() && self.webgl2 {
            return Some(vec![]);
        }
        let active = geti(0x84E0) as u32;
        let mut restore = Vec::new();
        let mut looped = false;
        for (loc, ty) in samplers {
            let mut unit = 0;
            unsafe { glGetUniformiv(prog, loc, &mut unit) };
            let Some(binding) = sampler_binding(ty) else { continue };
            unsafe { glActiveTexture(0x84C0 + unit as u32) };
            let tex = geti(binding) as u32;
            if tex == 0 {
                continue;
            }
            if atts.contains(&tex) {
                looped = true;
                break;
            }
            if self.webgl2 || !matches!(binding, 0x8069 | 0x8514) {
                continue;
            }
            let pot = |n: i32| n > 0 && n & (n - 1) == 0;
            let Some(&(w, h)) = self.tex_size.get(&tex) else { continue };
            if pot(w) && pot(h) {
                continue;
            }
            let target = if binding == 0x8069 { 0x0DE1 } else { 0x8513 };
            let (mut min, mut ws, mut wt) = (0, 0, 0);
            unsafe {
                glGetTexParameteriv(target, 0x2801, &mut min);
                glGetTexParameteriv(target, 0x2802, &mut ws);
                glGetTexParameteriv(target, 0x2803, &mut wt);
            }
            if matches!(min, 0x2600 | 0x2601) && ws == 0x812F && wt == 0x812F {
                continue;
            }
            let black = self.black_texture(target);
            unsafe { glBindTexture(target, black) };
            restore.push((0x84C0 + unit as u32, target, tex));
        }
        unsafe { glActiveTexture(active) };
        if looped {
            self.end_draw(restore);
            self.err(INVALID_OPERATION);
            return None;
        }
        Some(restore)
    }

    fn end_draw(&mut self, restore: Vec<(u32, u32, u32)>) {
        if restore.is_empty() {
            return;
        }
        let active = geti(0x84E0) as u32;
        for (unit, target, tex) in restore {
            unsafe {
                glActiveTexture(unit);
                glBindTexture(target, tex);
            }
        }
        unsafe { glActiveTexture(active) };
    }

    /// a 1 x 1 (0, 0, 0, 1) texture of `target` (TEXTURE_2D or TEXTURE_CUBE_MAP), made once; the
    /// binding of the active unit is left as it was
    fn black_texture(&mut self, target: u32) -> u32 {
        let k = (target != 0x0DE1) as usize;
        if self.black[k] == 0 {
            let binding = if k == 0 { 0x8069 } else { 0x8514 };
            let was = geti(binding) as u32;
            let px = [0u8, 0, 0, 255];
            unsafe {
                glGenTextures(1, &mut self.black[k]);
                glBindTexture(target, self.black[k]);
                glPixelStorei(0x0CF5, 1);
                let faces: &[u32] = if k == 0 { &[0x0DE1] } else { &[0x8515, 0x8516, 0x8517, 0x8518, 0x8519, 0x851A] };
                for &f in faces {
                    glTexImage2D(f, 0, 0x1908, 1, 1, 0, 0x1908, 0x1401, px.as_ptr() as *const c_void);
                }
                glTexParameteri(target, 0x2801, 0x2600);
                glPixelStorei(0x0CF5, self.unpack[0]);
                glBindTexture(target, was);
            }
        }
        self.black[k]
    }

    /// the sampler uniforms of a program just linked (for the feedback-loop check)
    fn note_link(&mut self, prog: u32) {
        let mut ok = 0;
        unsafe { glGetProgramiv(prog, 0x8B82, &mut ok) };
        let p = self.progs.entry(prog).or_default();
        p.link += 1;
        p.samplers.clear();
        if ok == 0 {
            return;
        }
        let mut n = 0;
        unsafe { glGetProgramiv(prog, 0x8B86, &mut n) };
        for i in 0..n as u32 {
            let (mut size, mut ty) = (0, 0);
            let name = gl_text(256, |cap, len, out| unsafe { glGetActiveUniform(prog, i, cap, len, &mut size, &mut ty, out) });
            if sampler_binding(ty).is_none() {
                continue;
            }
            let base = name.strip_suffix("[0]").unwrap_or(&name).to_string();
            for k in 0..size.max(1) {
                let elem = if size > 1 { format!("{base}[{k}]") } else { name.clone() };
                let loc = unsafe { glGetUniformLocation(prog, cstring(&elem).as_ptr()) };
                if loc >= 0 {
                    p.samplers.push((loc, ty));
                }
            }
        }
    }

    /// the uniform location's GL location, when its program is current and not relinked since
    /// (else INVALID_OPERATION); None for null (a no-op)
    fn location(&mut self, a: &A, i: usize) -> Option<i32> {
        let id = a.get(i).get("$o")?.to_string();
        let ok = match self.locs.get(&id) {
            Some(l) => {
                let prog = self.objs.get(&l.program).map(|o| o.name).unwrap_or(0);
                Some(&l.program) == self.program.as_ref() && self.progs.get(&prog).map(|p| p.link) == Some(l.link)
            }
            None => false,
        };
        if !ok {
            self.err(INVALID_OPERATION);
            return None;
        }
        Some(self.locs[&id].loc)
    }
}

impl Gl {
    fn init(&mut self) -> bool {
        if let Some(r) = self.ready {
            return r;
        }
        let ok = unsafe {
            self.dpy = eglGetPlatformDisplay(EGL_PLATFORM_SURFACELESS_MESA, ptr::null_mut(), ptr::null());
            let (mut major, mut minor) = (0, 0);
            if self.dpy.is_null() || eglInitialize(self.dpy, &mut major, &mut minor) == 0 || eglBindAPI(EGL_OPENGL_ES_API) == 0 {
                false
            } else {
                // no surface: the default EGL_WINDOW_BIT would match no config of a surfaceless display
                let attribs = [EGL_RENDERABLE_TYPE, EGL_OPENGL_ES3_BIT, EGL_SURFACE_TYPE, 0, EGL_NONE];
                let mut n = 0;
                eglChooseConfig(self.dpy, attribs.as_ptr(), &mut self.config, 1, &mut n) != 0 && n > 0
            }
        };
        if !ok {
            eprintln!("mrt-api: webgl: no EGL surfaceless display with OpenGL ES 3 (display {:?}, EGL error {:#x})", self.dpy, unsafe { eglGetError() });
        }
        self.ready = Some(ok);
        ok
    }

    fn make_current(&mut self, cid: i64) {
        if self.current == cid {
            return;
        }
        if let Some(c) = self.ctxs.get(&cid) {
            unsafe { eglMakeCurrent(self.dpy, ptr::null_mut(), ptr::null_mut(), c.egl) };
            self.current = cid;
        }
    }

    fn has(&self, ext: &str) -> bool {
        self.gl_exts.contains(ext)
    }

    /// The WebGL extensions Firefox exposes on this renderer (WebGLContext::IsExtensionSupported),
    /// those whose GL extension the context has, in Firefox's order
    fn supported(&self, webgl2: bool) -> Vec<String> {
        let half = self.has("GL_EXT_color_buffer_half_float") || self.has("GL_EXT_color_buffer_float");
        let common = [
            ("EXT_depth_clamp", self.has("GL_EXT_depth_clamp")),
            ("EXT_float_blend", self.has("GL_EXT_float_blend")),
            ("EXT_texture_compression_bptc", self.has("GL_EXT_texture_compression_bptc")),
            ("EXT_texture_compression_rgtc", self.has("GL_EXT_texture_compression_rgtc")),
            ("EXT_texture_filter_anisotropic", self.has("GL_EXT_texture_filter_anisotropic")),
            ("OES_texture_float_linear", self.has("GL_OES_texture_float_linear")),
            ("WEBGL_compressed_texture_astc", self.has("GL_KHR_texture_compression_astc_ldr")),
            ("WEBGL_compressed_texture_etc", true),
            ("WEBGL_compressed_texture_s3tc", self.has("GL_EXT_texture_compression_s3tc")),
            ("WEBGL_compressed_texture_s3tc_srgb", self.has("GL_EXT_texture_compression_s3tc_srgb") || self.has("GL_EXT_texture_sRGB")),
            ("WEBGL_debug_renderer_info", true),
            ("WEBGL_debug_shaders", true),
            ("WEBGL_lose_context", true),
        ];
        let only: Vec<(&str, bool)> = if webgl2 {
            vec![
                ("EXT_color_buffer_float", self.has("GL_EXT_color_buffer_float")),
                ("OES_draw_buffers_indexed", self.has("GL_OES_draw_buffers_indexed") || self.has("GL_EXT_draw_buffers_indexed")),
            ]
        } else {
            vec![
                ("ANGLE_instanced_arrays", true),
                ("EXT_blend_minmax", true),
                ("EXT_color_buffer_half_float", half),
                ("EXT_frag_depth", true),
                ("EXT_shader_texture_lod", true),
                ("EXT_sRGB", true),
                ("OES_element_index_uint", true),
                ("OES_fbo_render_mipmap", true),
                ("OES_standard_derivatives", true),
                ("OES_texture_float", true),
                ("OES_texture_half_float", true),
                ("OES_texture_half_float_linear", true),
                ("OES_vertex_array_object", true),
                ("WEBGL_color_buffer_float", self.has("GL_EXT_color_buffer_float")),
                ("WEBGL_depth_texture", true),
                ("WEBGL_draw_buffers", true),
            ]
        };
        let mut out: Vec<String> = common.iter().chain(only.iter()).filter(|e| e.1).map(|e| e.0.to_string()).collect();
        out.sort_by_key(|s| s.to_lowercase());
        out
    }

    fn create(&mut self, cid: i64, a: &A) -> Value {
        if !self.init() {
            return Value::Null;
        }
        self.drop_ctx(cid);
        // robust buffer access when the driver has it (llvmpipe does): a draw past a buffer's end
        // reads zeros or the buffer, never other memory, as Gecko asks of ANGLE
        let robust = [EGL_CONTEXT_MAJOR_VERSION, 3, EGL_CONTEXT_MINOR_VERSION, 0, EGL_CONTEXT_OPENGL_ROBUST_ACCESS, 1, EGL_NONE];
        let plain = [EGL_CONTEXT_MAJOR_VERSION, 3, EGL_CONTEXT_MINOR_VERSION, 0, EGL_NONE];
        let mut egl = unsafe { eglCreateContext(self.dpy, self.config, ptr::null_mut(), robust.as_ptr()) };
        if egl.is_null() {
            egl = unsafe { eglCreateContext(self.dpy, self.config, ptr::null_mut(), plain.as_ptr()) };
        }
        if egl.is_null() || unsafe { eglMakeCurrent(self.dpy, ptr::null_mut(), ptr::null_mut(), egl) } == 0 {
            eprintln!("mrt-api: webgl: eglCreateContext failed");
            return Value::Null;
        }
        self.current = cid;
        if self.gl_exts.is_empty() {
            let n = geti(0x821D);
            self.gl_exts = (0..n as u32).map(|i| gl_str(unsafe { glGetStringi(0x1F03, i) })).collect();
        }
        let webgl2 = a.s(0) == "webgl2";
        let mut attrs = Map::new();
        for (k, v) in [("alpha", true), ("antialias", true), ("depth", true), ("failIfMajorPerformanceCaveat", false), ("premultipliedAlpha", true), ("preserveDrawingBuffer", false), ("stencil", false)] {
            attrs.insert(k.into(), json!(v));
        }
        attrs.insert("powerPreference".into(), json!("default"));
        if let Some(given) = a.get(1).as_object() {
            for (k, v) in given {
                if attrs.contains_key(k) {
                    attrs.insert(k.clone(), v.clone());
                }
            }
        }
        let antialias = attrs["antialias"].as_bool() == Some(true);
        let (mut fbo, mut resolve) = (0, 0);
        unsafe {
            glGenFramebuffers(1, &mut fbo);
            glGenFramebuffers(1, &mut resolve);
        }
        let supported = self.supported(webgl2);
        let mut c = Ctx {
            egl,
            webgl2,
            attrs,
            w: 1,
            h: 1,
            fbo,
            color: 0,
            ds: 0,
            resolve,
            resolve_color: 0,
            samples: if antialias { geti(0x8D57).min(4) } else { 0 },
            objs: HashMap::new(),
            rev: HashMap::new(),
            locs: HashMap::new(),
            progs: HashMap::new(),
            fb_tex: HashMap::new(),
            rb_format: HashMap::new(),
            tex_size: HashMap::new(),
            black: [0, 0],
            error: 0,
            flip_y: false,
            premul: false,
            colorspace: 0x9244,
            unpack: [4, 0, 0, 0, 0, 0],
            pack: [4, 0, 0, 0],
            exts: HashSet::new(),
            supported: supported.clone(),
            draw_fb: None,
            read_fb: None,
            program: None,
        };
        c.alloc(a.i(2), a.i(3));
        unsafe {
            glViewport(0, 0, c.w, c.h);
            glScissor(0, 0, c.w, c.h);
            glGetError();
        }
        let attrs = Value::Object(c.attrs.clone());
        self.ctxs.insert(cid, c);
        json!({"attrs": attrs, "exts": supported})
    }

    fn drop_ctx(&mut self, cid: i64) {
        if let Some(c) = self.ctxs.remove(&cid) {
            unsafe {
                eglMakeCurrent(self.dpy, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
                eglDestroyContext(self.dpy, c.egl);
            }
            self.current = 0;
        }
    }

    /// An image source argument's pixels (None: not a source). Images are fetched once per URL.
    fn source(&mut self, cid: i64, v: &Value, bin: &[u8], page: &Value, net: &mut fetch::Net, stores: &mut Stores) -> Option<Rc<Pixels>> {
        if let Some(url) = v.get("$img").and_then(Value::as_str) {
            let key = if v.get("bytes").is_some() { format!("bytes:{url}") } else { format!("{}|{url}", str_of(v.get("cors").unwrap_or(&NULL))) };
            if let Some(p) = self.images.get(&key) {
                return Some(p.clone().unwrap_or_else(|| Rc::new(Pixels { w: 0, h: 0, rgba: vec![], tainted: false })));
            }
            let p = load_image(url, v, page, net, stores).map(Rc::new);
            self.images.insert(key, p.clone());
            return Some(p.unwrap_or_else(|| Rc::new(Pixels { w: 0, h: 0, rgba: vec![], tainted: false })));
        }
        if let Some(other) = v.get("$canvas").and_then(Value::as_i64) {
            if !self.ctxs.contains_key(&other) {
                return Some(Rc::new(Pixels { w: 1, h: 1, rgba: vec![0; 4], tainted: false }));
            }
            self.make_current(other);
            let p = self.ctxs.get_mut(&other).map(Ctx::as_source);
            self.make_current(cid);
            return p.map(Rc::new);
        }
        if let Some(size) = v.get("$blank").and_then(Value::as_array) {
            let (w, h) = (num_of(&size[0]).max(0.0) as u32, num_of(size.get(1).unwrap_or(&NULL)).max(0.0) as u32);
            if (w as u64) * (h as u64) > (MAX_OUTPUT / 4) as u64 {
                return Some(Rc::new(Pixels { w: 0, h: 0, rgba: vec![], tainted: false }));
            }
            return Some(Rc::new(Pixels { w, h, rgba: vec![0; (w * h * 4) as usize], tainted: false }));
        }
        if let Some(size) = v.get("$imageData").and_then(Value::as_array) {
            let (w, h) = (num_of(&size[0]) as u32, num_of(size.get(1).unwrap_or(&NULL)) as u32);
            let d = v.get("data")?;
            let (o, b) = (d.get("o")?.as_u64()? as usize, d.get("b")?.as_u64()? as usize);
            let rgba = bin.get(o..o + b)?.to_vec();
            if rgba.len() != (w as usize) * (h as usize) * 4 {
                return None;
            }
            return Some(Rc::new(Pixels { w, h, rgba, tainted: false }));
        }
        None
    }

    fn command(&mut self, cid: i64, ext: Option<&str>, method: &str, args: &[Value], made: &Value, bin: &[u8], outs: &mut Vec<Value>, page: &Value, net: &mut fetch::Net, stores: &mut Stores) -> Result<Value, Fail> {
        let a = A { v: args, bin };
        if method == "$create" {
            return Ok(self.create(cid, &a));
        }
        if !self.ctxs.contains_key(&cid) {
            return Err(Fail::Lost);
        }
        self.make_current(cid);
        if method == "$drop" {
            self.drop_ctx(cid);
            return Ok(Value::Null);
        }
        let mut srcs = Vec::new();
        if method.starts_with("tex") && (method.contains("Image2D") || method.contains("Image3D")) {
            for v in args {
                srcs.push(self.source(cid, v, bin, page, net, stores));
            }
        }
        let c = self.ctxs.get_mut(&cid).unwrap();
        match method {
            "$size" => {
                c.alloc(a.i(0), a.i(1));
                Ok(Value::Null)
            }
            "$dbsize" => Ok(json!([c.w, c.h])),
            "$ext" => {
                let name = a.s(0);
                if c.supported.contains(&name) {
                    c.exts.insert(name);
                }
                Ok(Value::Null)
            }
            "$set" => Ok(Value::Null),
            "$dataURL" => Ok(json!(data_url(c, &a.s(0), a.get(1)))),
            _ => {
                let method = match ext {
                    Some(e) => {
                        if !c.exts.contains(e) {
                            return Ok(Value::Null);
                        }
                        ext_method(e, method)
                    }
                    None => method.to_string(),
                };
                c.call(&method, &a, made, outs, &srcs)
            }
        }
    }
}

/// an extension's method as the context's own (drawArraysInstancedANGLE: drawArraysInstanced)
fn ext_method(ext: &str, m: &str) -> String {
    if ext == "WEBGL_debug_shaders" && m == "getTranslatedShaderSource" {
        return "getShaderSource".into();
    }
    for suffix in ["ANGLE", "OES", "WEBGL", "EXT"] {
        if let Some(core) = m.strip_suffix(suffix) {
            return core.to_string();
        }
    }
    m.to_string()
}

fn data_url(c: &mut Ctx, ty: &str, quality: &Value) -> String {
    let raw = c.read_drawing_buffer();
    let (w, h) = (c.w as u32, c.h as u32);
    let premultiplied = c.attr("premultipliedAlpha");
    let mut img = image::RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let i = (((h - 1 - y) * w + x) * 4) as usize;
            let mut p = [raw[i], raw[i + 1], raw[i + 2], raw[i + 3]];
            if !c.attr("alpha") {
                p[3] = 255;
            } else if premultiplied && p[3] != 0 && p[3] != 255 {
                for k in 0..3 {
                    p[k] = ((p[k] as f32 * 255.0 / p[3] as f32).round()).min(255.0) as u8;
                }
            }
            img.put_pixel(x, y, image::Rgba(p));
        }
    }
    let mut bytes = Vec::new();
    let (mime, ok) = if ty.eq_ignore_ascii_case("image/jpeg") {
        let q = quality.as_f64().filter(|q| (0.0..=1.0).contains(q)).map(|q| (q * 100.0) as u8).unwrap_or(92);
        let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
        ("image/jpeg", image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, q).encode_image(&rgb).is_ok())
    } else {
        ("image/png", image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).is_ok())
    };
    if !ok {
        return "data:,".into();
    }
    format!("data:{mime};base64,{}", B64.encode(bytes))
}

/// An <img>'s pixels for a texture: its bytes from the runtime (a blob: URL), a data: URL, or
/// fetched as the element's request (its crossorigin attribute; the page's origin, referrer and
/// policies, the partition's cookies). An opaque response is tainted. None: not an image Firefox
/// decodes here (SVG, a failed load).
fn load_image(url: &str, v: &Value, page: &Value, net: &mut fetch::Net, stores: &mut Stores) -> Option<Pixels> {
    let (bytes, tainted) = if let Some(b) = v.get("bytes").and_then(Value::as_str) {
        (B64.decode(b).ok()?, false)
    } else if let Some(rest) = url.strip_prefix("data:") {
        let (meta, data) = rest.split_once(',')?;
        let raw = percent_decode(data);
        (if meta.ends_with(";base64") { B64.decode(raw.iter().filter(|b| !b.is_ascii_whitespace()).copied().collect::<Vec<u8>>()).ok()? } else { raw }, false)
    } else {
        let cors = v.get("cors").and_then(Value::as_str);
        let (mode, credentials) = match cors {
            None => ("no-cors", "include"),
            Some(c) if c.eq_ignore_ascii_case("use-credentials") => ("cors", "include"),
            Some(_) => ("cors", "same-origin"),
        };
        let mut req = json!({"url": url, "method": "GET", "mode": mode, "credentials": credentials, "bytes": true,
                             "headers": [["accept", "image/avif,image/webp,image/png,image/svg+xml,image/*;q=0.8,*/*;q=0.5"]]});
        for k in ["origin", "referrer", "metaReferrer", "doc", "site", "top", "ancestor"] {
            if let Some(x) = page.get(k) {
                req[k] = x.clone();
            }
        }
        let r = fetch::image(&req, net, stores).ok()?;
        let body = B64.decode(r.get("bodyBase64").and_then(Value::as_str)?).ok()?;
        (body, r.get("type").and_then(Value::as_str) == Some("opaque"))
    };
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    Some(Pixels { w: img.width(), h: img.height(), rgba: img.into_raw(), tainted })
}

fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// The `webgl` op: one batch, with the page's origin, referrer and document (for the images it loads)
pub fn handle(req: &Value, gl: &mut Gl, net: &mut fetch::Net, stores: &mut Stores) -> Result<Value, String> {
    let cmds = req.get("cmds").and_then(Value::as_array).ok_or("mrt-api: webgl: no cmds")?;
    let bin = match req.get("bin").and_then(Value::as_str) {
        Some(b) => B64.decode(b).map_err(|e| format!("mrt-api: webgl: bin: {e}"))?,
        None => Vec::new(),
    };
    let sync = req.get("sync").and_then(Value::as_bool).unwrap_or(false);
    let mut result = Value::Null;
    let mut outs = Vec::new();
    let mut throws = Vec::new();
    let mut lost = Vec::<i64>::new();
    let mut last_threw = Value::Null;
    for (i, cmd) in cmds.iter().enumerate() {
        let Some(cmd) = cmd.as_array().filter(|c| c.len() == 5) else { return Err("mrt-api: webgl: bad command".into()) };
        let cid = cmd[0].as_i64().ok_or("mrt-api: webgl: bad context id")?;
        let ext = cmd[1].as_str();
        let method = cmd[2].as_str().unwrap_or("");
        let args = cmd[3].as_array().map(Vec::as_slice).unwrap_or(&[]);
        outs.clear();
        match gl.command(cid, ext, method, args, &cmd[4], &bin, &mut outs, req, net, stores) {
            Ok(v) => result = v,
            Err(Fail::Lost) => {
                if !lost.contains(&cid) {
                    lost.push(cid);
                }
                result = Value::Null;
            }
            Err(Fail::Throw(name, msg)) => {
                if sync && i == cmds.len() - 1 {
                    last_threw = json!([name, msg]);
                }
                throws.push(json!([i, name, msg]));
                result = Value::Null;
            }
        }
    }
    Ok(json!({"result": if sync { result } else { Value::Null }, "outs": if sync { outs } else { vec![] },
              "throws": throws, "lost": lost, "lastThrew": last_threw}))
}

// ---- the calls ----

macro_rules! name {
    ($c:expr, $a:expr, $i:expr, $k:ident) => {
        match $c.name(&$a, $i, Kind::$k) {
            Some(n) => n,
            None => return Ok(Value::Null),
        }
    };
}

impl Ctx {
    #[allow(clippy::cognitive_complexity)]
    fn call(&mut self, m: &str, a: &A, made: &Value, outs: &mut Vec<Value>, srcs: &[Option<Rc<Pixels>>]) -> Result<Value, Fail> {
        let null = Ok(Value::Null);
        let w2 = self.webgl2;
        unsafe {
            match m {
                // -- objects --
                "createBuffer" | "createFramebuffer" | "createRenderbuffer" | "createTexture" | "createVertexArray" | "createQuery" | "createSampler" | "createTransformFeedback" => {
                    let mut n = 0;
                    let kind = match m {
                        "createBuffer" => { glGenBuffers(1, &mut n); Kind::Buffer }
                        "createFramebuffer" => { glGenFramebuffers(1, &mut n); Kind::Framebuffer }
                        "createRenderbuffer" => { glGenRenderbuffers(1, &mut n); Kind::Renderbuffer }
                        "createTexture" => { glGenTextures(1, &mut n); Kind::Texture }
                        "createVertexArray" => { glGenVertexArrays(1, &mut n); Kind::VertexArray }
                        "createQuery" => { glGenQueries(1, &mut n); Kind::Query }
                        "createSampler" => { glGenSamplers(1, &mut n); Kind::Sampler }
                        _ => { glGenTransformFeedbacks(1, &mut n); Kind::TransformFeedback }
                    };
                    Ok(self.made(made, kind, n, ptr::null()))
                }
                "createProgram" => {
                    let n = glCreateProgram();
                    self.progs.insert(n, Prog::default());
                    Ok(self.made(made, Kind::Program, n, ptr::null()))
                }
                "createShader" => {
                    let n = glCreateShader(a.u(0));
                    Ok(self.made(made, Kind::Shader, n, ptr::null()))
                }
                "fenceSync" => {
                    let s = glFenceSync(a.u(0), a.u(1));
                    if s.is_null() {
                        self.keep_gl_error();
                        return null;
                    }
                    Ok(self.made(made, Kind::Sync, 0, s))
                }
                m if m.starts_with("delete") => {
                    let kind = match m {
                        "deleteBuffer" => Kind::Buffer,
                        "deleteFramebuffer" => Kind::Framebuffer,
                        "deleteRenderbuffer" => Kind::Renderbuffer,
                        "deleteTexture" => Kind::Texture,
                        "deleteProgram" => Kind::Program,
                        "deleteShader" => Kind::Shader,
                        "deleteVertexArray" => Kind::VertexArray,
                        "deleteQuery" => Kind::Query,
                        "deleteSampler" => Kind::Sampler,
                        "deleteSync" => Kind::Sync,
                        "deleteTransformFeedback" => Kind::TransformFeedback,
                        _ => return Err(throw("TypeError", "Unknown WebGL method")),
                    };
                    let Some(id) = a.obj(0) else { return null };
                    let Some(o) = self.objs.get_mut(&id) else {
                        self.err(INVALID_OPERATION);
                        return null;
                    };
                    if o.kind != kind || o.deleted {
                        return null;
                    }
                    o.deleted = true;
                    let n = o.name;
                    match kind {
                        Kind::Buffer => glDeleteBuffers(1, &n),
                        Kind::Framebuffer => {
                            glDeleteFramebuffers(1, &n);
                            self.fb_tex.remove(&n);
                            if self.draw_fb.as_ref() == Some(&id) {
                                self.draw_fb = None;
                            }
                            if self.read_fb.as_ref() == Some(&id) {
                                self.read_fb = None;
                            }
                            self.rebind();
                        }
                        Kind::Renderbuffer => glDeleteRenderbuffers(1, &n),
                        Kind::Texture => {
                            glDeleteTextures(1, &n);
                            for atts in self.fb_tex.values_mut() {
                                atts.retain(|_, t| *t != n);
                            }
                        }
                        Kind::Program => glDeleteProgram(n),
                        Kind::Shader => glDeleteShader(n),
                        Kind::VertexArray => glDeleteVertexArrays(1, &n),
                        Kind::Query => glDeleteQueries(1, &n),
                        Kind::Sampler => glDeleteSamplers(1, &n),
                        Kind::Sync => glDeleteSync(self.objs[&id].sync),
                        Kind::TransformFeedback => glDeleteTransformFeedbacks(1, &n),
                    }
                    null
                }
                m if m.starts_with("is") && m != "isEnabled" => {
                    let kind = match m {
                        "isBuffer" => Kind::Buffer,
                        "isFramebuffer" => Kind::Framebuffer,
                        "isRenderbuffer" => Kind::Renderbuffer,
                        "isTexture" => Kind::Texture,
                        "isProgram" => Kind::Program,
                        "isShader" => Kind::Shader,
                        "isVertexArray" => Kind::VertexArray,
                        "isQuery" => Kind::Query,
                        "isSampler" => Kind::Sampler,
                        "isSync" => Kind::Sync,
                        "isTransformFeedback" => Kind::TransformFeedback,
                        _ => return Err(throw("TypeError", "Unknown WebGL method")),
                    };
                    let Some(o) = self.live(a, 0, kind) else { return Ok(json!(false)) };
                    let n = o.name;
                    let r = match kind {
                        Kind::Buffer => glIsBuffer(n),
                        Kind::Framebuffer => glIsFramebuffer(n),
                        Kind::Renderbuffer => glIsRenderbuffer(n),
                        Kind::Texture => glIsTexture(n),
                        Kind::Program => glIsProgram(n),
                        Kind::Shader => glIsShader(n),
                        Kind::VertexArray => glIsVertexArray(n),
                        Kind::Query => glIsQuery(n),
                        Kind::Sampler => glIsSampler(n),
                        Kind::Sync => glIsSync(o.sync),
                        Kind::TransformFeedback => glIsTransformFeedback(n),
                    };
                    Ok(json!(r != 0))
                }

                // -- bindings --
                "bindBuffer" => {
                    let t = a.u(0);
                    if !w2 && t != 0x8892 && t != 0x8893 {
                        self.err(INVALID_ENUM);
                        return null;
                    }
                    let b = name!(self, a, 1, Buffer);
                    glBindBuffer(t, b);
                    null
                }
                "bindBufferBase" => {
                    let b = name!(self, a, 2, Buffer);
                    glBindBufferBase(a.u(0), a.u(1), b);
                    null
                }
                "bindBufferRange" => {
                    let b = name!(self, a, 2, Buffer);
                    glBindBufferRange(a.u(0), a.u(1), b, a.isz(3), a.isz(4));
                    null
                }
                "bindFramebuffer" => {
                    let t = a.u(0);
                    if !matches!(t, FRAMEBUFFER) && !(w2 && matches!(t, READ_FRAMEBUFFER | DRAW_FRAMEBUFFER)) {
                        self.err(INVALID_ENUM);
                        return null;
                    }
                    let n = name!(self, a, 1, Framebuffer);
                    let id = a.obj(1);
                    glBindFramebuffer(t, if n == 0 { self.fbo } else { n });
                    if t != READ_FRAMEBUFFER {
                        self.draw_fb = id.clone();
                    }
                    if t != DRAW_FRAMEBUFFER {
                        self.read_fb = id;
                    }
                    null
                }
                "bindRenderbuffer" => {
                    let n = name!(self, a, 1, Renderbuffer);
                    glBindRenderbuffer(a.u(0), n);
                    null
                }
                "bindTexture" => {
                    let t = a.u(0);
                    if !w2 && t != 0x0DE1 && t != 0x8513 {
                        self.err(INVALID_ENUM);
                        return null;
                    }
                    let n = name!(self, a, 1, Texture);
                    glBindTexture(t, n);
                    null
                }
                "bindVertexArray" => {
                    let n = name!(self, a, 0, VertexArray);
                    glBindVertexArray(n);
                    null
                }
                "bindSampler" => {
                    let n = name!(self, a, 1, Sampler);
                    glBindSampler(a.u(0), n);
                    null
                }
                "bindTransformFeedback" => {
                    let n = name!(self, a, 1, TransformFeedback);
                    glBindTransformFeedback(a.u(0), n);
                    null
                }
                "useProgram" => {
                    let n = name!(self, a, 0, Program);
                    glUseProgram(n);
                    if !self.gl_failed() {
                        self.program = a.obj(0);
                    }
                    null
                }

                // -- shaders and programs --
                "shaderSource" => {
                    let s = name!(self, a, 0, Shader);
                    let src = cstring(&a.s(1));
                    let p = src.as_ptr();
                    glShaderSource(s, 1, &p, ptr::null());
                    null
                }
                "compileShader" => {
                    let s = name!(self, a, 0, Shader);
                    glCompileShader(s);
                    null
                }
                "attachShader" | "detachShader" => {
                    let p = name!(self, a, 0, Program);
                    let s = name!(self, a, 1, Shader);
                    if m == "attachShader" {
                        glAttachShader(p, s);
                    } else {
                        glDetachShader(p, s);
                    }
                    if !self.gl_failed() {
                        let mut ty = 0;
                        glGetShaderiv(s, 0x8B4F, &mut ty);
                        let id = a.obj(1).unwrap_or_default();
                        let prog = self.progs.entry(p).or_default();
                        prog.attached.retain(|(_, sid)| sid != &id);
                        if m == "attachShader" {
                            prog.attached.push((ty as u32, id));
                        }
                    }
                    null
                }
                "linkProgram" => {
                    let p = name!(self, a, 0, Program);
                    glLinkProgram(p);
                    self.note_link(p);
                    null
                }
                "validateProgram" => {
                    let p = name!(self, a, 0, Program);
                    glValidateProgram(p);
                    null
                }
                "bindAttribLocation" => {
                    let p = name!(self, a, 0, Program);
                    glBindAttribLocation(p, a.u(1), cstring(&a.s(2)).as_ptr());
                    null
                }
                "transformFeedbackVaryings" => {
                    let p = name!(self, a, 0, Program);
                    let names: Vec<CString> = a.strs(1).iter().map(|s| cstring(s)).collect();
                    let ptrs: Vec<*const i8> = names.iter().map(|s| s.as_ptr()).collect();
                    glTransformFeedbackVaryings(p, ptrs.len() as i32, ptrs.as_ptr(), a.u(2));
                    null
                }
                "getShaderParameter" => {
                    let s = name!(self, a, 0, Shader);
                    let pn = a.u(1);
                    self.keep_gl_error();
                    let mut v = [0; 16]; // (room for a name that answers several values)
                    glGetShaderiv(s, pn, v.as_mut_ptr());
                    let v = v[0];
                    if self.gl_failed() {
                        return null;
                    }
                    Ok(match pn {
                        0x8B80 | 0x8B81 => json!(v != 0), // DELETE_STATUS, COMPILE_STATUS
                        _ => json!(v),
                    })
                }
                "getProgramParameter" => {
                    let p = name!(self, a, 0, Program);
                    let pn = a.u(1);
                    self.keep_gl_error();
                    let mut v = [0; 16]; // (room for a name that answers several values)
                    glGetProgramiv(p, pn, v.as_mut_ptr());
                    let v = v[0];
                    if self.gl_failed() {
                        return null;
                    }
                    Ok(match pn {
                        0x8B80 | 0x8B82 | 0x8B83 => json!(v != 0), // DELETE, LINK, VALIDATE_STATUS
                        _ => json!(v),
                    })
                }
                "getShaderInfoLog" => {
                    let s = name!(self, a, 0, Shader);
                    let mut n = 0;
                    glGetShaderiv(s, 0x8B84, &mut n);
                    Ok(json!(gl_text(n, |cap, len, out| glGetShaderInfoLog(s, cap, len, out))))
                }
                "getProgramInfoLog" => {
                    let p = name!(self, a, 0, Program);
                    let mut n = 0;
                    glGetProgramiv(p, 0x8B84, &mut n);
                    Ok(json!(gl_text(n, |cap, len, out| glGetProgramInfoLog(p, cap, len, out))))
                }
                "getShaderSource" => {
                    let s = name!(self, a, 0, Shader);
                    let mut n = 0;
                    glGetShaderiv(s, 0x8B88, &mut n);
                    Ok(json!(gl_text(n, |cap, len, out| glGetShaderSource(s, cap, len, out))))
                }
                "getAttachedShaders" => {
                    let p = name!(self, a, 0, Program);
                    // Gecko keeps them in an unordered_map by type: the last attached first
                    let list: Vec<Value> = self.progs.get(&p).map(|pr| pr.attached.iter().rev().map(|(_, id)| json!({"$o": serde_json::from_str::<Value>(id).unwrap_or(Value::Null), "$t": "WebGLShader"})).collect()).unwrap_or_default();
                    Ok(Value::Array(list))
                }
                "getAttribLocation" => {
                    let p = name!(self, a, 0, Program);
                    Ok(json!(glGetAttribLocation(p, cstring(&a.s(1)).as_ptr())))
                }
                "getFragDataLocation" => {
                    let p = name!(self, a, 0, Program);
                    Ok(json!(glGetFragDataLocation(p, cstring(&a.s(1)).as_ptr())))
                }
                "getActiveAttrib" | "getActiveUniform" | "getTransformFeedbackVarying" => {
                    let p = name!(self, a, 0, Program);
                    self.keep_gl_error();
                    let (mut size, mut ty) = (0, 0u32);
                    let i = a.u(1);
                    let name = gl_text(1024, |cap, len, out| match m {
                        "getActiveAttrib" => glGetActiveAttrib(p, i, cap, len, &mut size, &mut ty, out),
                        "getActiveUniform" => glGetActiveUniform(p, i, cap, len, &mut size, &mut ty, out),
                        _ => glGetTransformFeedbackVarying(p, i, cap, len, &mut size, &mut ty, out),
                    });
                    if self.gl_failed() {
                        return null;
                    }
                    Ok(json!({"$ai": [name, size, ty]}))
                }
                "getUniformLocation" => {
                    let p = name!(self, a, 0, Program);
                    let want = a.s(1);
                    let loc = glGetUniformLocation(p, cstring(&want).as_ptr());
                    if loc < 0 {
                        return null;
                    }
                    // its type: the active uniform it names (an array's element names the array)
                    let mut n = 0;
                    glGetProgramiv(p, 0x8B86, &mut n);
                    let base = want.split('[').next().unwrap_or("").to_string();
                    let mut ty = 0;
                    for i in 0..n as u32 {
                        let (mut size, mut t) = (0, 0);
                        let name = gl_text(1024, |cap, len, out| glGetActiveUniform(p, i, cap, len, &mut size, &mut t, out));
                        if name == want || name.strip_suffix("[0]").is_some_and(|b| b == base) {
                            ty = t;
                            break;
                        }
                    }
                    let link = self.progs.get(&p).map(|pr| pr.link).unwrap_or(0);
                    let Some(program) = a.obj(0) else { return null };
                    self.locs.insert(made.to_string(), Loc { program, link, loc, ty });
                    Ok(json!({"$o": made, "$t": "WebGLUniformLocation"}))
                }
                "getUniform" => {
                    let p = name!(self, a, 0, Program);
                    let Some(id) = a.get(1).get("$o").map(Value::to_string) else { return null };
                    let Some(l) = self.locs.get(&id).filter(|l| Some(&l.program) == a.obj(0).as_ref()) else {
                        self.err(INVALID_OPERATION);
                        return null;
                    };
                    let (kind, n) = uniform_shape(l.ty);
                    let loc = l.loc;
                    Ok(match kind {
                        'f' => {
                            let mut v = [0f32; 16];
                            glGetUniformfv(p, loc, v.as_mut_ptr());
                            if n == 1 { number(v[0] as f64) } else { f32s(&v[..n]) }
                        }
                        'u' => {
                            let mut v = [0u32; 16];
                            glGetUniformuiv(p, loc, v.as_mut_ptr());
                            if n == 1 { json!(v[0]) } else { u32s(&v[..n]) }
                        }
                        'b' => {
                            let mut v = [0i32; 16];
                            glGetUniformiv(p, loc, v.as_mut_ptr());
                            if n == 1 { json!(v[0] != 0) } else { json!(v[..n].iter().map(|x| *x != 0).collect::<Vec<_>>()) }
                        }
                        _ => {
                            let mut v = [0i32; 16];
                            glGetUniformiv(p, loc, v.as_mut_ptr());
                            if n == 1 { json!(v[0]) } else { i32s(&v[..n]) }
                        }
                    })
                }
                "getUniformBlockIndex" => {
                    let p = name!(self, a, 0, Program);
                    Ok(json!(glGetUniformBlockIndex(p, cstring(&a.s(1)).as_ptr())))
                }
                "getActiveUniformBlockName" => {
                    let p = name!(self, a, 0, Program);
                    self.keep_gl_error();
                    let s = gl_text(1024, |cap, len, out| glGetActiveUniformBlockName(p, a.u(1), cap, len, out));
                    if self.gl_failed() {
                        return null;
                    }
                    Ok(json!(s))
                }
                "getActiveUniformBlockParameter" => {
                    let p = name!(self, a, 0, Program);
                    let (i, pn) = (a.u(1), a.u(2));
                    self.keep_gl_error();
                    if pn == 0x8A43 {
                        // UNIFORM_BLOCK_ACTIVE_UNIFORM_INDICES
                        let mut n = 0;
                        glGetActiveUniformBlockiv(p, i, 0x8A42, &mut n);
                        let mut v = vec![0i32; n.max(0) as usize];
                        if n > 0 {
                            glGetActiveUniformBlockiv(p, i, pn, v.as_mut_ptr());
                        }
                        if self.gl_failed() {
                            return null;
                        }
                        return Ok(u32s(&v.iter().map(|x| *x as u32).collect::<Vec<_>>()));
                    }
                    let mut v = [0; 16]; // (room for a name that answers several values)
                    glGetActiveUniformBlockiv(p, i, pn, v.as_mut_ptr());
                    let v = v[0];
                    if self.gl_failed() {
                        return null;
                    }
                    Ok(if matches!(pn, 0x8A44 | 0x8A46) { json!(v != 0) } else { json!(v) })
                }
                "uniformBlockBinding" => {
                    let p = name!(self, a, 0, Program);
                    glUniformBlockBinding(p, a.u(1), a.u(2));
                    null
                }
                "getUniformIndices" => {
                    let p = name!(self, a, 0, Program);
                    let names: Vec<CString> = a.strs(1).iter().map(|s| cstring(s)).collect();
                    let ptrs: Vec<*const i8> = names.iter().map(|s| s.as_ptr()).collect();
                    let mut out = vec![0u32; ptrs.len()];
                    if !ptrs.is_empty() {
                        glGetUniformIndices(p, ptrs.len() as i32, ptrs.as_ptr(), out.as_mut_ptr());
                    }
                    Ok(json!(out))
                }
                "getActiveUniforms" => {
                    let p = name!(self, a, 0, Program);
                    let idx = a.uints(1, 0, 0).unwrap_or_default();
                    let pn = a.u(2);
                    let mut out = vec![0i32; idx.len()];
                    self.keep_gl_error();
                    if !idx.is_empty() {
                        glGetActiveUniformsiv(p, idx.len() as i32, idx.as_ptr(), pn, out.as_mut_ptr());
                    }
                    if self.gl_failed() {
                        return null;
                    }
                    Ok(match pn {
                        0x8A3E => json!(out.iter().map(|x| *x != 0).collect::<Vec<_>>()), // UNIFORM_IS_ROW_MAJOR
                        0x8A37 => json!(out.iter().map(|x| *x as u32).collect::<Vec<_>>()), // UNIFORM_TYPE
                        _ => json!(out),
                    })
                }

                // -- uniforms and attributes --
                m if m.starts_with("uniform") && m != "uniformBlockBinding" => self.uniform(m, a),
                m if m.starts_with("vertexAttrib") && m != "vertexAttribPointer" && m != "vertexAttribIPointer" && m != "vertexAttribDivisor" => {
                    let i = a.u(0);
                    let int = m.starts_with("vertexAttribI4");
                    let n = m.as_bytes().get(if int { 13 } else { 12 }).map(|c| (c - b'0') as usize).unwrap_or(4);
                    let vector = m.ends_with('v');
                    if m.contains("I4u") {
                        let v = if vector { a.uints(1, 0, 0) } else { Some((0..4).map(|k| a.u(1 + k)).collect()) };
                        let Some(v) = v.filter(|v| v.len() >= 4) else { self.err(INVALID_VALUE); return null };
                        glVertexAttribI4uiv(i, v.as_ptr());
                    } else if int {
                        let v = if vector { a.ints(1, 0, 0) } else { Some((0..4).map(|k| a.i(1 + k)).collect()) };
                        let Some(v) = v.filter(|v| v.len() >= 4) else { self.err(INVALID_VALUE); return null };
                        glVertexAttribI4iv(i, v.as_ptr());
                    } else {
                        let given = if vector { a.floats(1, 0, 0) } else { Some((0..n).map(|k| a.f(1 + k)).collect()) };
                        let Some(given) = given.filter(|v| v.len() >= n) else { self.err(INVALID_VALUE); return null };
                        let mut v = [0.0, 0.0, 0.0, 1.0];
                        v[..n].copy_from_slice(&given[..n]);
                        glVertexAttrib4fv(i, v.as_ptr());
                    }
                    null
                }
                "vertexAttribPointer" | "vertexAttribIPointer" if geti(0x8894) == 0 && a.num(if m == "vertexAttribPointer" { 5 } else { 4 }) != 0.0 => {
                    // an offset with no buffer would be a client pointer
                    self.err(INVALID_OPERATION);
                    null
                }
                "vertexAttribPointer" => {
                    glVertexAttribPointer(a.u(0), a.i(1), a.u(2), a.b(3), a.i(4), a.isz(5) as *const c_void);
                    null
                }
                "vertexAttribIPointer" => {
                    glVertexAttribIPointer(a.u(0), a.i(1), a.u(2), a.i(3), a.isz(4) as *const c_void);
                    null
                }
                "vertexAttribDivisor" => {
                    glVertexAttribDivisor(a.u(0), a.u(1));
                    null
                }
                "enableVertexAttribArray" => {
                    glEnableVertexAttribArray(a.u(0));
                    null
                }
                "disableVertexAttribArray" => {
                    glDisableVertexAttribArray(a.u(0));
                    null
                }
                "getVertexAttrib" => {
                    let (i, pn) = (a.u(0), a.u(1));
                    self.keep_gl_error();
                    let r = match pn {
                        0x889F => {
                            let mut v = [0i32; 16];
                            glGetVertexAttribiv(i, pn, v.as_mut_ptr());
                            self.named(Kind::Buffer, v[0] as u32)
                        }
                        0x8626 => {
                            // CURRENT_VERTEX_ATTRIB, as the type it was last given
                            let mut v = [0f32; 4];
                            glGetVertexAttribfv(i, pn, v.as_mut_ptr());
                            f32s(&v)
                        }
                        0x8622 | 0x886A | 0x88FD => {
                            let mut v = [0i32; 16];
                            glGetVertexAttribiv(i, pn, v.as_mut_ptr());
                            json!(v[0] != 0)
                        }
                        _ => {
                            let mut v = [0i32; 16];
                            glGetVertexAttribiv(i, pn, v.as_mut_ptr());
                            json!(v[0])
                        }
                    };
                    if self.gl_failed() {
                        return null;
                    }
                    Ok(r)
                }
                "getVertexAttribOffset" => {
                    let mut p = ptr::null_mut();
                    glGetVertexAttribPointerv(a.u(0), a.u(1), &mut p);
                    Ok(json!(p as usize))
                }

                // -- state --
                "enable" | "disable" => {
                    let cap = a.u(0);
                    if !w2 && cap == RASTERIZER_DISCARD {
                        self.err(INVALID_ENUM);
                        return null;
                    }
                    if m == "enable" { glEnable(cap) } else { glDisable(cap) }
                    null
                }
                "enablei" | "disablei" => {
                    if m == "enablei" { glEnablei(a.u(0), a.u(1)) } else { glDisablei(a.u(0), a.u(1)) }
                    null
                }
                "isEnabled" => {
                    let cap = a.u(0);
                    if !w2 && cap == RASTERIZER_DISCARD {
                        self.err(INVALID_ENUM);
                        return Ok(json!(false));
                    }
                    Ok(json!(glIsEnabled(cap) != 0))
                }
                "activeTexture" => { glActiveTexture(a.u(0)); null }
                "blendColor" => { glBlendColor(a.f(0), a.f(1), a.f(2), a.f(3)); null }
                "blendEquation" => { glBlendEquation(a.u(0)); null }
                "blendEquationSeparate" => { glBlendEquationSeparate(a.u(0), a.u(1)); null }
                "blendFunc" => { glBlendFunc(a.u(0), a.u(1)); null }
                "blendFuncSeparate" => { glBlendFuncSeparate(a.u(0), a.u(1), a.u(2), a.u(3)); null }
                "blendEquationi" => { glBlendEquationi(a.u(0), a.u(1)); null }
                "blendEquationSeparatei" => { glBlendEquationSeparatei(a.u(0), a.u(1), a.u(2)); null }
                "blendFunci" => { glBlendFunci(a.u(0), a.u(1), a.u(2)); null }
                "blendFuncSeparatei" => { glBlendFuncSeparatei(a.u(0), a.u(1), a.u(2), a.u(3), a.u(4)); null }
                "colorMaski" => { glColorMaski(a.u(0), a.b(1), a.b(2), a.b(3), a.b(4)); null }
                "clearColor" => { glClearColor(a.f(0), a.f(1), a.f(2), a.f(3)); null }
                "clearDepth" => { glClearDepthf(a.f(0)); null }
                "clearStencil" => { glClearStencil(a.i(0)); null }
                "colorMask" => { glColorMask(a.b(0), a.b(1), a.b(2), a.b(3)); null }
                "cullFace" => { glCullFace(a.u(0)); null }
                "depthFunc" => { glDepthFunc(a.u(0)); null }
                "depthMask" => { glDepthMask(a.b(0)); null }
                "depthRange" => { glDepthRangef(a.f(0), a.f(1)); null }
                "frontFace" => { glFrontFace(a.u(0)); null }
                "hint" => { glHint(a.u(0), a.u(1)); null }
                "lineWidth" => { glLineWidth(a.f(0)); null }
                "polygonOffset" => { glPolygonOffset(a.f(0), a.f(1)); null }
                "sampleCoverage" => { glSampleCoverage(a.f(0), a.b(1)); null }
                "scissor" => { glScissor(a.i(0), a.i(1), a.i(2), a.i(3)); null }
                "viewport" => { glViewport(a.i(0), a.i(1), a.i(2), a.i(3)); null }
                "stencilFunc" => { glStencilFunc(a.u(0), a.i(1), a.u(2)); null }
                "stencilFuncSeparate" => { glStencilFuncSeparate(a.u(0), a.u(1), a.i(2), a.u(3)); null }
                "stencilMask" => { glStencilMask(a.u(0)); null }
                "stencilMaskSeparate" => { glStencilMaskSeparate(a.u(0), a.u(1)); null }
                "stencilOp" => { glStencilOp(a.u(0), a.u(1), a.u(2)); null }
                "stencilOpSeparate" => { glStencilOpSeparate(a.u(0), a.u(1), a.u(2), a.u(3)); null }
                "finish" => { glFinish(); null }
                "flush" => { glFlush(); null }
                "makeXRCompatible" => null,
                "pixelStorei" => {
                    let (pn, v) = (a.u(0), a.i(1));
                    match pn {
                        0x9240 => self.flip_y = v != 0,
                        0x9241 => self.premul = v != 0,
                        0x9243 => {
                            if v == 0 || v == 0x9244 { self.colorspace = v } else { self.err(INVALID_ENUM) }
                        }
                        _ => {
                            let slot = match pn {
                                0x0CF5 => Some((true, 0)),
                                0x0CF2 => Some((true, 1)),
                                0x806E => Some((true, 2)),
                                0x0CF4 => Some((true, 3)),
                                0x0CF3 => Some((true, 4)),
                                0x806D => Some((true, 5)),
                                0x0D05 => Some((false, 0)),
                                0x0D02 => Some((false, 1)),
                                0x0D04 => Some((false, 2)),
                                0x0D03 => Some((false, 3)),
                                _ => None,
                            };
                            if !w2 && !matches!(pn, 0x0CF5 | 0x0D05) {
                                self.err(INVALID_ENUM);
                                return null;
                            }
                            self.keep_gl_error();
                            glPixelStorei(pn, v);
                            if !self.gl_failed() {
                                match slot {
                                    Some((true, k)) => self.unpack[k] = v,
                                    Some((false, k)) => self.pack[k] = v,
                                    None => {}
                                }
                            }
                        }
                    }
                    null
                }
                "getError" => {
                    let e = if self.error != NO_ERROR { std::mem::replace(&mut self.error, NO_ERROR) } else { glGetError() };
                    Ok(json!(e))
                }
                "getParameter" => self.get_parameter(a.u(0)),
                "getIndexedParameter" => {
                    let (pn, i) = (a.u(0), a.u(1));
                    self.keep_gl_error();
                    let r = match pn {
                        0x8C8F | 0x8A28 => {
                            let mut v = [0; 16]; // (room for a name that answers several values)
                            glGetIntegeri_v(pn, i, v.as_mut_ptr());
                            let v = v[0];
                            self.named(Kind::Buffer, v as u32)
                        }
                        0x8C84 | 0x8C85 | 0x8A29 | 0x8A2A => {
                            let mut v = [0i64; 16]; // (room for a name that answers several values)
                            glGetInteger64i_v(pn, i, v.as_mut_ptr());
                            let v = v[0];
                            json!(v)
                        }
                        0x0BE2 => {
                            let mut v = [0; 16]; // (room for a name that answers several values)
                            glGetIntegeri_v(pn, i, v.as_mut_ptr());
                            let v = v[0];
                            json!(v != 0)
                        }
                        0x0C23 => {
                            let mut v = [0i32; 4];
                            glGetIntegeri_v(pn, i, v.as_mut_ptr());
                            json!(v.iter().map(|x| *x != 0).collect::<Vec<_>>())
                        }
                        _ => {
                            let mut v = [0; 16]; // (room for a name that answers several values)
                            glGetIntegeri_v(pn, i, v.as_mut_ptr());
                            let v = v[0];
                            json!(v)
                        }
                    };
                    if self.gl_failed() {
                        return null;
                    }
                    Ok(r)
                }
                "getBufferParameter" => {
                    let mut v = 0i64;
                    self.keep_gl_error();
                    glGetBufferParameteri64v(a.u(0), a.u(1), &mut v);
                    if self.gl_failed() {
                        return null;
                    }
                    Ok(json!(v))
                }
                "getTexParameter" => {
                    let (t, pn) = (a.u(0), a.u(1));
                    self.keep_gl_error();
                    let r = if matches!(pn, 0x84FE | 0x813A | 0x813B) {
                        let mut v = [0.0; 16]; // (room for a name that answers several values)
                        glGetTexParameterfv(t, pn, v.as_mut_ptr());
                        let v = v[0];
                        number(v as f64)
                    } else {
                        let mut v = [0; 16]; // (room for a name that answers several values)
                        glGetTexParameteriv(t, pn, v.as_mut_ptr());
                        let v = v[0];
                        if pn == 0x912F { json!(v != 0) } else { json!(v) }
                    };
                    if self.gl_failed() {
                        return null;
                    }
                    Ok(r)
                }
                "getRenderbufferParameter" => {
                    let mut v = 0;
                    self.keep_gl_error();
                    glGetRenderbufferParameteriv(a.u(0), a.u(1), &mut v);
                    if self.gl_failed() {
                        return null;
                    }
                    if a.u(1) == 0x8D44 {
                        // RENDERBUFFER_INTERNAL_FORMAT: the page's
                        if let Some(f) = self.rb_format.get(&(geti(0x8CA7) as u32)) {
                            return Ok(json!(f));
                        }
                    }
                    Ok(json!(v))
                }
                "getFramebufferAttachmentParameter" => {
                    let (t, mut at, pn) = (a.u(0), a.u(1), a.u(2));
                    let default = if t == READ_FRAMEBUFFER { self.read_fb.is_none() } else { self.draw_fb.is_none() };
                    if default {
                        at = match at {
                            BACK => COLOR_ATTACHMENT0,
                            0x1801 => DEPTH_ATTACHMENT,
                            0x1802 => STENCIL_ATTACHMENT,
                            _ => {
                                self.err(INVALID_ENUM);
                                return null;
                            }
                        };
                        if pn == 0x8CD0 {
                            return Ok(json!(0x8218)); // FRAMEBUFFER_DEFAULT
                        }
                        if pn == 0x8CD1 {
                            self.err(INVALID_ENUM);
                            return null;
                        }
                    }
                    self.keep_gl_error();
                    let mut v = [0; 16]; // (room for a name that answers several values)
                    glGetFramebufferAttachmentParameteriv(t, at, pn, v.as_mut_ptr());
                    let v = v[0];
                    if self.gl_failed() {
                        return null;
                    }
                    if pn == 0x8CD1 {
                        // FRAMEBUFFER_ATTACHMENT_OBJECT_NAME: the object, by its type
                        let mut kind = 0;
                        glGetFramebufferAttachmentParameteriv(t, at, 0x8CD0, &mut kind);
                        return Ok(self.named(if kind == 0x1702 { Kind::Texture } else { Kind::Renderbuffer }, v as u32));
                    }
                    Ok(json!(v))
                }
                "getShaderPrecisionFormat" => {
                    let (mut range, mut precision) = ([0i32; 2], 0);
                    self.keep_gl_error();
                    glGetShaderPrecisionFormat(a.u(0), a.u(1), range.as_mut_ptr(), &mut precision);
                    if self.gl_failed() {
                        return null;
                    }
                    Ok(json!({"$pf": [range[0], range[1], precision]}))
                }
                "getInternalformatParameter" => {
                    let (t, f, pn) = (a.u(0), a.u(1), a.u(2));
                    self.keep_gl_error();
                    let mut n = 0;
                    glGetInternalformativ(t, f, 0x9380, 1, &mut n); // NUM_SAMPLE_COUNTS
                    let mut v = vec![0i32; n.max(0) as usize];
                    if n > 0 {
                        glGetInternalformativ(t, f, pn, n, v.as_mut_ptr());
                    }
                    if self.gl_failed() {
                        return null;
                    }
                    Ok(i32s(&v))
                }
                "checkFramebufferStatus" => Ok(json!(glCheckFramebufferStatus(a.u(0)))),

                // -- framebuffers and renderbuffers --
                "framebufferTexture2D" | "framebufferTextureLayer" => {
                    let i = if m == "framebufferTexture2D" { 3 } else { 2 };
                    let t = a.u(0);
                    let at = a.u(1);
                    let tex = name!(self, a, i, Texture);
                    if (if t == READ_FRAMEBUFFER { &self.read_fb } else { &self.draw_fb }).is_none() {
                        self.err(INVALID_OPERATION);
                        return null;
                    }
                    self.keep_gl_error();
                    if m == "framebufferTexture2D" {
                        glFramebufferTexture2D(t, at, a.u(2), tex, a.i(4));
                    } else {
                        glFramebufferTextureLayer(t, at, tex, a.i(3), a.i(4));
                    }
                    if !self.gl_failed() {
                        let fb = self.fb_name(if t == READ_FRAMEBUFFER { &self.read_fb } else { &self.draw_fb });
                        let atts = self.fb_tex.entry(fb).or_default();
                        let points: &[u32] = if at == DEPTH_STENCIL_ATTACHMENT { &[DEPTH_ATTACHMENT, STENCIL_ATTACHMENT] } else { &[at] };
                        for &p in points {
                            if tex == 0 { atts.remove(&p); } else { atts.insert(p, tex); }
                        }
                    }
                    null
                }
                "framebufferRenderbuffer" => {
                    let (t, at) = (a.u(0), a.u(1));
                    let rb = name!(self, a, 3, Renderbuffer);
                    if (if t == READ_FRAMEBUFFER { &self.read_fb } else { &self.draw_fb }).is_none() {
                        self.err(INVALID_OPERATION);
                        return null;
                    }
                    glFramebufferRenderbuffer(t, at, a.u(2), rb);
                    let fb = self.fb_name(if t == READ_FRAMEBUFFER { &self.read_fb } else { &self.draw_fb });
                    if let Some(atts) = self.fb_tex.get_mut(&fb) {
                        atts.remove(&at);
                        if at == DEPTH_STENCIL_ATTACHMENT {
                            atts.remove(&DEPTH_ATTACHMENT);
                            atts.remove(&STENCIL_ATTACHMENT);
                        }
                    }
                    null
                }
                "renderbufferStorage" | "renderbufferStorageMultisample" => {
                    let k = if m == "renderbufferStorage" { 1 } else { 2 };
                    let given = a.u(k);
                    let f = if given == 0x84F9 { 0x88F0 } else { given };
                    self.keep_gl_error();
                    if k == 1 {
                        glRenderbufferStorage(a.u(0), f, a.i(2), a.i(3));
                    } else {
                        glRenderbufferStorageMultisample(a.u(0), a.i(1), f, a.i(3), a.i(4));
                    }
                    if !self.gl_failed() {
                        self.rb_format.insert(geti(0x8CA7) as u32, given);
                    }
                    null
                }
                "blitFramebuffer" => {
                    let r: [i32; 8] = std::array::from_fn(|k| a.i(k));
                    self.with_read(|| glBlitFramebuffer(r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7], a.u(8), a.u(9)));
                    null
                }
                "drawBuffers" => {
                    let mut list = a.uints(0, 0, 0).unwrap_or_default();
                    if self.draw_fb.is_none() {
                        if list.len() != 1 || !matches!(list[0], BACK | 0) {
                            self.err(INVALID_OPERATION);
                            return null;
                        }
                        list = vec![if list[0] == BACK { COLOR_ATTACHMENT0 } else { 0 }];
                    }
                    glDrawBuffers(list.len() as i32, list.as_ptr());
                    null
                }
                "readBuffer" => {
                    let mut b = a.u(0);
                    if self.read_fb.is_none() {
                        if !matches!(b, BACK | 0) {
                            self.err(INVALID_OPERATION);
                            return null;
                        }
                        b = if b == BACK { COLOR_ATTACHMENT0 } else { 0 };
                    }
                    glReadBuffer(b);
                    null
                }
                "invalidateFramebuffer" | "invalidateSubFramebuffer" => {
                    let t = a.u(0);
                    let default = if t == READ_FRAMEBUFFER { self.read_fb.is_none() } else { self.draw_fb.is_none() };
                    let list: Vec<u32> = a.uints(1, 0, 0).unwrap_or_default().into_iter().map(|x| if default { match x { 0x1800 => COLOR_ATTACHMENT0, 0x1801 => DEPTH_ATTACHMENT, 0x1802 => STENCIL_ATTACHMENT, x => x } } else { x }).collect();
                    if m == "invalidateFramebuffer" {
                        glInvalidateFramebuffer(t, list.len() as i32, list.as_ptr());
                    } else {
                        glInvalidateSubFramebuffer(t, list.len() as i32, list.as_ptr(), a.i(2), a.i(3), a.i(4), a.i(5));
                    }
                    null
                }

                // -- drawing --
                "clear" => { glClear(a.u(0)); null }
                "clearBufferfv" | "clearBufferiv" | "clearBufferuiv" => {
                    let (b, d) = (a.u(0), a.i(1));
                    let off = a.u(3) as usize;
                    let need = if b == 0x1800 { 4 } else { 1 };
                    match m {
                        "clearBufferfv" => {
                            let Some(v) = a.floats(2, off, 0).filter(|v| v.len() >= need) else { self.err(INVALID_VALUE); return null };
                            glClearBufferfv(b, d, v.as_ptr());
                        }
                        "clearBufferiv" => {
                            let Some(v) = a.ints(2, off, 0).filter(|v| v.len() >= need) else { self.err(INVALID_VALUE); return null };
                            glClearBufferiv(b, d, v.as_ptr());
                        }
                        _ => {
                            let Some(v) = a.uints(2, off, 0).filter(|v| v.len() >= need) else { self.err(INVALID_VALUE); return null };
                            glClearBufferuiv(b, d, v.as_ptr());
                        }
                    }
                    null
                }
                "clearBufferfi" => { glClearBufferfi(a.u(0), a.i(1), a.f(2), a.i(3)); null }
                "drawArrays" | "drawArraysInstanced" => {
                    let Some(restore) = self.begin_draw() else { return null };
                    if m == "drawArrays" { glDrawArrays(a.u(0), a.i(1), a.i(2)) } else { glDrawArraysInstanced(a.u(0), a.i(1), a.i(2), a.i(3)) }
                    self.end_draw(restore);
                    null
                }
                "drawElements" | "drawElementsInstanced" | "drawRangeElements" => {
                    let ty_at = if m == "drawRangeElements" { 4 } else { 2 };
                    if !w2 && a.u(ty_at) == 0x1405 && !self.exts.contains("OES_element_index_uint") {
                        self.err(INVALID_ENUM);
                        return null;
                    }
                    // no index buffer: ES 3 would read the indices from this process's memory
                    if geti(0x8895) == 0 {
                        self.err(INVALID_OPERATION);
                        return null;
                    }
                    let Some(restore) = self.begin_draw() else { return null };
                    match m {
                        "drawElements" => glDrawElements(a.u(0), a.i(1), a.u(2), a.isz(3) as *const c_void),
                        "drawElementsInstanced" => glDrawElementsInstanced(a.u(0), a.i(1), a.u(2), a.isz(3) as *const c_void, a.i(4)),
                        _ => glDrawRangeElements(a.u(0), a.u(1), a.u(2), a.i(3), a.u(4), a.isz(5) as *const c_void),
                    }
                    self.end_draw(restore);
                    null
                }
                "beginTransformFeedback" => { glBeginTransformFeedback(a.u(0)); null }
                "endTransformFeedback" => { glEndTransformFeedback(); null }
                "pauseTransformFeedback" => { glPauseTransformFeedback(); null }
                "resumeTransformFeedback" => { glResumeTransformFeedback(); null }

                // -- buffers --
                "bufferData" => {
                    let (t, usage) = (a.u(0), a.u(2));
                    if a.is_number(1) {
                        let n = a.num(1);
                        if !(0.0..=(1u64 << 31) as f64).contains(&n) {
                            self.err(INVALID_VALUE);
                            return null;
                        }
                        glBufferData(t, n as isize, ptr::null(), usage);
                        return null;
                    }
                    let Some(v) = a.view(1) else {
                        self.err(INVALID_VALUE);
                        return null;
                    };
                    let es = elem_size(v.ty);
                    let off = a.u(3) as usize * es;
                    let len = if a.len() > 4 && a.u(4) > 0 { a.u(4) as usize * es } else { v.bytes.len().saturating_sub(off) };
                    let Some(bytes) = v.bytes.get(off..off.saturating_add(len)) else {
                        self.err(INVALID_VALUE);
                        return null;
                    };
                    glBufferData(t, bytes.len() as isize, bytes.as_ptr() as *const c_void, usage);
                    null
                }
                "bufferSubData" => {
                    let Some(v) = a.view(2) else {
                        self.err(INVALID_VALUE);
                        return null;
                    };
                    let es = elem_size(v.ty);
                    let off = a.u(3) as usize * es;
                    let len = if a.len() > 4 && a.u(4) > 0 { a.u(4) as usize * es } else { v.bytes.len().saturating_sub(off) };
                    let Some(bytes) = v.bytes.get(off..off.saturating_add(len)) else {
                        self.err(INVALID_VALUE);
                        return null;
                    };
                    glBufferSubData(a.u(0), a.isz(1), bytes.len() as isize, bytes.as_ptr() as *const c_void);
                    null
                }
                "copyBufferSubData" => { glCopyBufferSubData(a.u(0), a.u(1), a.isz(2), a.isz(3), a.isz(4)); null }
                "getBufferSubData" => {
                    let (t, src) = (a.u(0), a.isz(1));
                    let out = a.get(2);
                    let ty = out.get("$out").and_then(Value::as_str).unwrap_or("Uint8Array");
                    let es = if ty == "DataView" { 1 } else { elem_size(ty) };
                    let n = out.get("n").and_then(Value::as_u64).unwrap_or(0) as usize * es;
                    if n > MAX_OUTPUT {
                        return Err(throw("RangeError", "WebGL output too large"));
                    }
                    let dst = a.u(3) as usize * es;
                    let len = if a.len() > 4 && a.u(4) > 0 { a.u(4) as usize * es } else { n.saturating_sub(dst) };
                    let mut buf = vec![0u8; n];
                    if dst + len > n {
                        self.err(INVALID_VALUE);
                        outs.push(Value::Null);
                        return null;
                    }
                    self.keep_gl_error();
                    if len > 0 {
                        let p = glMapBufferRange(t, src, len as isize, 0x0001);
                        if !p.is_null() {
                            ptr::copy_nonoverlapping(p as *const u8, buf.as_mut_ptr().add(dst), len);
                            glUnmapBuffer(t);
                        }
                    }
                    if self.gl_failed() {
                        outs.push(Value::Null);
                        return null;
                    }
                    outs.push(json!(B64.encode(buf)));
                    null
                }

                // -- textures --
                "texParameteri" => { glTexParameteri(a.u(0), a.u(1), a.i(2)); null }
                "texParameterf" => { glTexParameterf(a.u(0), a.u(1), a.f(2)); null }
                "samplerParameteri" | "samplerParameterf" => {
                    let s = name!(self, a, 0, Sampler);
                    if m == "samplerParameteri" { glSamplerParameteri(s, a.u(1), a.i(2)) } else { glSamplerParameterf(s, a.u(1), a.f(2)) }
                    null
                }
                "getSamplerParameter" => {
                    let s = name!(self, a, 0, Sampler);
                    let pn = a.u(1);
                    self.keep_gl_error();
                    let r = if matches!(pn, 0x813A | 0x813B | 0x84FE) {
                        let mut v = [0.0; 16]; // (room for a name that answers several values)
                        glGetSamplerParameterfv(s, pn, v.as_mut_ptr());
                        let v = v[0];
                        number(v as f64)
                    } else {
                        let mut v = [0; 16]; // (room for a name that answers several values)
                        glGetSamplerParameteriv(s, pn, v.as_mut_ptr());
                        let v = v[0];
                        json!(v)
                    };
                    if self.gl_failed() {
                        return null;
                    }
                    Ok(r)
                }
                "generateMipmap" => { glGenerateMipmap(a.u(0)); null }
                "texStorage2D" => { glTexStorage2D(a.u(0), a.i(1), a.u(2), a.i(3), a.i(4)); null }
                "texStorage3D" => { glTexStorage3D(a.u(0), a.i(1), a.u(2), a.i(3), a.i(4), a.i(5)); null }
                "texImage2D" | "texSubImage2D" | "texImage3D" | "texSubImage3D" => self.tex_image(m, a, srcs),
                "compressedTexImage2D" | "compressedTexSubImage2D" | "compressedTexImage3D" | "compressedTexSubImage3D" => {
                    let at = match m { "compressedTexImage2D" => 6, "compressedTexSubImage2D" => 7, "compressedTexImage3D" => 7, _ => 9 };
                    let (ptr_, size) = if a.is_number(at) {
                        // a PIXEL_UNPACK_BUFFER: (imageSize, offset)
                        (a.isz(at + 1) as *const c_void, a.i(at))
                    } else {
                        let Some(v) = a.view(at) else { self.err(INVALID_VALUE); return null };
                        let es = elem_size(v.ty);
                        let off = a.u(at + 1) as usize * es;
                        let len = if a.len() > at + 2 && a.u(at + 2) > 0 { a.u(at + 2) as usize * es } else { v.bytes.len().saturating_sub(off) };
                        let Some(b) = v.bytes.get(off..off.saturating_add(len)) else { self.err(INVALID_VALUE); return null };
                        (b.as_ptr() as *const c_void, b.len() as i32)
                    };
                    match m {
                        "compressedTexImage2D" => glCompressedTexImage2D(a.u(0), a.i(1), a.u(2), a.i(3), a.i(4), a.i(5), size, ptr_),
                        "compressedTexSubImage2D" => glCompressedTexSubImage2D(a.u(0), a.i(1), a.i(2), a.i(3), a.i(4), a.i(5), a.u(6), size, ptr_),
                        "compressedTexImage3D" => glCompressedTexImage3D(a.u(0), a.i(1), a.u(2), a.i(3), a.i(4), a.i(5), a.i(6), size, ptr_),
                        _ => glCompressedTexSubImage3D(a.u(0), a.i(1), a.i(2), a.i(3), a.i(4), a.i(5), a.i(6), a.i(7), a.u(8), size, ptr_),
                    }
                    null
                }
                "copyTexImage2D" => {
                    let r: [i32; 8] = std::array::from_fn(|k| a.i(k));
                    if r[1] == 0 && (r[0] == 0x0DE1 || (0x8515..=0x851A).contains(&r[0])) {
                        let tex = geti(if r[0] == 0x0DE1 { 0x8069 } else { 0x8514 }) as u32;
                        self.tex_size.insert(tex, (r[5], r[6]));
                    }
                    self.with_read(|| glCopyTexImage2D(r[0] as u32, r[1], r[2] as u32, r[3], r[4], r[5], r[6], r[7]));
                    null
                }
                "copyTexSubImage2D" => {
                    let r: [i32; 8] = std::array::from_fn(|k| a.i(k));
                    self.with_read(|| glCopyTexSubImage2D(r[0] as u32, r[1], r[2], r[3], r[4], r[5], r[6], r[7]));
                    null
                }
                "copyTexSubImage3D" => {
                    let r: [i32; 9] = std::array::from_fn(|k| a.i(k));
                    self.with_read(|| glCopyTexSubImage3D(r[0] as u32, r[1], r[2], r[3], r[4], r[5], r[6], r[7], r[8]));
                    null
                }
                "readPixels" => self.read_pixels(a, outs),

                // -- queries and syncs --
                "beginQuery" => {
                    let q = name!(self, a, 1, Query);
                    glBeginQuery(a.u(0), q);
                    null
                }
                "endQuery" => { glEndQuery(a.u(0)); null }
                "getQuery" => {
                    let mut v = 0;
                    self.keep_gl_error();
                    glGetQueryiv(a.u(0), a.u(1), &mut v);
                    if self.gl_failed() {
                        return null;
                    }
                    Ok(self.named(Kind::Query, v as u32))
                }
                "getQueryParameter" => {
                    let q = name!(self, a, 0, Query);
                    let pn = a.u(1);
                    let mut v = 0;
                    self.keep_gl_error();
                    glGetQueryObjectuiv(q, pn, &mut v);
                    if self.gl_failed() {
                        return null;
                    }
                    Ok(if pn == 0x8867 { json!(v != 0) } else { json!(v) })
                }
                "clientWaitSync" | "waitSync" | "getSyncParameter" => {
                    let Some(id) = a.obj(0) else { self.err(INVALID_VALUE); return null };
                    let Some(s) = self.objs.get(&id).filter(|o| o.kind == Kind::Sync && !o.deleted).map(|o| o.sync) else {
                        self.err(INVALID_OPERATION);
                        return null;
                    };
                    match m {
                        "clientWaitSync" => {
                            let timeout = a.num(2);
                            if !(0.0..=MAX_CLIENT_WAIT_NS as f64).contains(&timeout) {
                                self.err(INVALID_OPERATION);
                                return Ok(json!(0x911D)); // WAIT_FAILED
                            }
                            Ok(json!(glClientWaitSync(s, a.u(1), timeout as u64)))
                        }
                        "waitSync" => {
                            glWaitSync(s, a.u(1), u64::MAX);
                            null
                        }
                        _ => {
                            let mut v = [0; 16]; // (room for a name that answers several values)
                            glGetSynciv(s, a.u(1), 1, ptr::null_mut(), v.as_mut_ptr());
                            let v = v[0];
                            Ok(json!(v))
                        }
                    }
                }
                _ => Err(throw("TypeError", "Unknown WebGL method")),
            }
        }
    }

    fn uniform(&mut self, m: &str, a: &A) -> Result<Value, Fail> {
        let Some(loc) = self.location(a, 0) else { return Ok(Value::Null) };
        let rest = &m["uniform".len()..];
        unsafe {
            if let Some(dims) = rest.strip_prefix("Matrix") {
                let dims = dims.trim_end_matches("fv");
                let n = match dims { "2" => 4, "3" => 9, "4" => 16, "2x3" | "3x2" => 6, "2x4" | "4x2" => 8, _ => 12 };
                let Some(v) = a.floats(2, a.u(3) as usize, a.u(4) as usize).filter(|v| !v.is_empty() && v.len() % n == 0) else {
                    self.err(INVALID_VALUE);
                    return Ok(Value::Null);
                };
                let count = (v.len() / n) as i32;
                let t = a.b(1);
                let f = match dims {
                    "2" => glUniformMatrix2fv,
                    "3" => glUniformMatrix3fv,
                    "4" => glUniformMatrix4fv,
                    "2x3" => glUniformMatrix2x3fv,
                    "3x2" => glUniformMatrix3x2fv,
                    "2x4" => glUniformMatrix2x4fv,
                    "4x2" => glUniformMatrix4x2fv,
                    "3x4" => glUniformMatrix3x4fv,
                    _ => glUniformMatrix4x3fv,
                };
                f(loc, count, t, v.as_ptr());
                return Ok(Value::Null);
            }
            let n = (rest.as_bytes()[0] - b'0') as usize;
            let kind = &rest[1..];
            let vector = kind.ends_with('v');
            match kind.trim_end_matches('v') {
                "f" => {
                    let v = if vector { a.floats(1, a.u(2) as usize, a.u(3) as usize) } else { Some((0..n).map(|k| a.f(1 + k)).collect()) };
                    let Some(v) = v.filter(|v| !v.is_empty() && v.len() % n == 0) else { self.err(INVALID_VALUE); return Ok(Value::Null) };
                    let f = [glUniform1fv, glUniform2fv, glUniform3fv, glUniform4fv][n - 1];
                    f(loc, (v.len() / n) as i32, v.as_ptr());
                }
                "i" => {
                    let v = if vector { a.ints(1, a.u(2) as usize, a.u(3) as usize) } else { Some((0..n).map(|k| a.i(1 + k)).collect()) };
                    let Some(v) = v.filter(|v| !v.is_empty() && v.len() % n == 0) else { self.err(INVALID_VALUE); return Ok(Value::Null) };
                    let f = [glUniform1iv, glUniform2iv, glUniform3iv, glUniform4iv][n - 1];
                    f(loc, (v.len() / n) as i32, v.as_ptr());
                }
                "ui" => {
                    let v = if vector { a.uints(1, a.u(2) as usize, a.u(3) as usize) } else { Some((0..n).map(|k| a.u(1 + k)).collect()) };
                    let Some(v) = v.filter(|v| !v.is_empty() && v.len() % n == 0) else { self.err(INVALID_VALUE); return Ok(Value::Null) };
                    let f = [glUniform1uiv, glUniform2uiv, glUniform3uiv, glUniform4uiv][n - 1];
                    f(loc, (v.len() / n) as i32, v.as_ptr());
                }
                _ => return Err(throw("TypeError", "Unknown WebGL method")),
            }
        }
        Ok(Value::Null)
    }

    fn get_parameter(&mut self, pn: u32) -> Result<Value, Fail> {
        let w2 = self.webgl2;
        let has = |e: &str| self.exts.contains(e);
        if !w2 && P_WEBGL2.contains(&pn) {
            self.err(INVALID_ENUM);
            return Ok(Value::Null);
        }
        let gated = match pn {
            0x85B5 => w2 || has("OES_vertex_array_object"),
            0x9245 | 0x9246 => has("WEBGL_debug_renderer_info"),
            0x84FF => has("EXT_texture_filter_anisotropic"),
            0x8B8B => w2 || has("OES_standard_derivatives"),
            0x8824 | 0x8CDF => w2 || has("WEBGL_draw_buffers"),
            p if (0x8825..0x8835).contains(&p) => w2 || has("WEBGL_draw_buffers"),
            0x864F => has("EXT_depth_clamp"),
            _ => true,
        };
        if !gated {
            self.err(INVALID_ENUM);
            return Ok(Value::Null);
        }
        let vao_name = |n: i32| n as u32;
        Ok(match pn {
            0x1F00 => json!("Mozilla"),
            0x1F01 => json!(sanitize_renderer(&gl_str(unsafe { glGetString(0x1F01) }))),
            0x1F02 => json!(if w2 { "WebGL 2.0" } else { "WebGL 1.0" }),
            0x8B8C => json!(if w2 { "WebGL GLSL ES 3.00" } else { "WebGL GLSL ES 1.0" }),
            0x9245 => json!(gl_str(unsafe { glGetString(0x1F00) })),
            0x9246 => json!(sanitize_renderer(&gl_str(unsafe { glGetString(0x1F01) }))),
            0x9240 => json!(self.flip_y),
            0x9241 => json!(self.premul),
            0x9243 => json!(self.colorspace),
            0x9247 => json!(MAX_CLIENT_WAIT_NS),
            0x0CF5 => json!(self.unpack[0]),
            0x0D05 => json!(self.pack[0]),
            // the default framebuffer's own answers
            0x8CA6 => self.draw_fb.as_ref().map(|id| json!({"$o": serde_json::from_str::<Value>(id).unwrap_or(Value::Null), "$t": "WebGLFramebuffer"})).unwrap_or(Value::Null),
            0x8CAA => self.read_fb.as_ref().map(|id| json!({"$o": serde_json::from_str::<Value>(id).unwrap_or(Value::Null), "$t": "WebGLFramebuffer"})).unwrap_or(Value::Null),
            0x0C02 if self.read_fb.is_none() => json!(if geti(pn) == 0 { 0 } else { BACK }),
            p if (0x8825..0x8835).contains(&p) && self.draw_fb.is_none() => json!(if geti(p) == 0 { 0 } else { BACK }),
            0x846E => f32s(&[1.0, 1.0]), // a core profile's lines (Gecko leaves [1, 1])
            0x846D | 0x0B70 => f32s(&getfv::<2>(pn)),
            0x8005 | 0x0C22 => f32s(&getfv::<4>(pn)),
            0x0D3A => {
                let d = getiv::<2>(pn);
                i32s(&[d[0].min(d[1]), d[0].min(d[1])])
            }
            0x0BA2 | 0x0C10 => i32s(&getiv::<4>(pn)),
            0x86A3 => {
                let mut list: Vec<u32> = Vec::new();
                for e in &self.supported {
                    if self.exts.contains(e) {
                        list.extend_from_slice(compressed_formats(e));
                    }
                }
                u32s(&list)
            }
            0x0C23 => {
                let mut v = [0u8; 4];
                unsafe { glGetBooleanv(pn, v.as_mut_ptr()) };
                json!(v.iter().map(|x| *x != 0).collect::<Vec<_>>())
            }
            0x8B4D => json!(geti(pn).min(255)), // MAX_COMBINED_TEXTURE_IMAGE_UNITS (uint8_t in Gecko)
            0x8894 | 0x8895 | 0x8F36 | 0x8F37 | 0x88ED | 0x88EF | 0x8C8F | 0x8A28 => self.named(Kind::Buffer, geti(pn) as u32),
            0x8B8D => self.program.as_ref().map(|id| json!({"$o": serde_json::from_str::<Value>(id).unwrap_or(Value::Null), "$t": "WebGLProgram"})).unwrap_or(Value::Null),
            0x8CA7 => self.named(Kind::Renderbuffer, geti(pn) as u32),
            0x8069 | 0x8514 | 0x806A | 0x8C1D => self.named(Kind::Texture, geti(pn) as u32),
            0x85B5 => self.named(Kind::VertexArray, vao_name(geti(pn))),
            0x8E25 => self.named(Kind::TransformFeedback, geti(pn) as u32),
            0x8919 => self.named(Kind::Sampler, geti(pn) as u32),
            p if P_BOOL.contains(&p) => json!(enabled_or_bool(p)),
            p if P_FLOAT.contains(&p) => number(getfv::<1>(p)[0] as f64),
            p if P_INT64.contains(&p) => {
                let mut v = 0i64;
                unsafe { glGetInteger64v(p, &mut v) };
                json!(v as f64)
            }
            p => {
                self.keep_gl_error();
                // (room for a name that answers several values)
                let v = getiv::<64>(p)[0];
                if self.gl_failed() {
                    return Ok(Value::Null);
                }
                // the drawing buffer's bits and samples answer for the framebuffer bound, as GL does
                json!(v)
            }
        })
    }

    fn read_pixels(&mut self, a: &A, outs: &mut Vec<Value>) -> Result<Value, Fail> {
        let (x, y, w, h, format) = (a.i(0), a.i(1), a.i(2), a.i(3), a.u(4));
        let ty = if a.u(5) == 0x8D61 { 0x140B } else { a.u(5) };
        if a.is_number(6) {
            // into a PIXEL_PACK_BUFFER at an offset
            let off = a.isz(6);
            self.with_read(|| unsafe { glReadPixels(x, y, w, h, format, ty, off as *mut c_void) });
            return Ok(Value::Null);
        }
        let out = a.get(6);
        let Some(view_ty) = out.get("$out").and_then(Value::as_str) else {
            return Err(throw("TypeError", "WebGLRenderingContext.readPixels: Argument 7 is not an object."));
        };
        let es = if view_ty == "DataView" { 1 } else { elem_size(view_ty) };
        let n = out.get("n").and_then(Value::as_u64).unwrap_or(0) as usize * es;
        if n > MAX_OUTPUT {
            return Err(throw("RangeError", "WebGL output too large"));
        }
        let dst = a.u(7) as usize * es;
        if !view_matches(view_ty, ty) {
            self.err(INVALID_OPERATION);
            outs.push(Value::Null);
            return Ok(Value::Null);
        }
        let Some(bpp) = pixel_bytes(format, ty) else {
            self.err(INVALID_ENUM);
            outs.push(Value::Null);
            return Ok(Value::Null);
        };
        let need = block_bytes(w, h, 1, bpp, self.pack[0], self.pack[1], 0, [self.pack[2], self.pack[3], 0]);
        if w < 0 || h < 0 {
            self.err(INVALID_VALUE);
            outs.push(Value::Null);
            return Ok(Value::Null);
        }
        if need.is_none_or(|need| dst + need > n) {
            self.err(INVALID_OPERATION);
            outs.push(Value::Null);
            return Ok(Value::Null);
        }
        let mut buf = vec![0u8; n];
        self.keep_gl_error();
        if w > 0 && h > 0 {
            let pack_buffer = if self.webgl2 { geti(0x88ED) } else { 0 };
            if pack_buffer != 0 {
                // a pack buffer bound with an array given: Gecko's INVALID_OPERATION
                self.err(INVALID_OPERATION);
                outs.push(Value::Null);
                return Ok(Value::Null);
            }
            let p = unsafe { buf.as_mut_ptr().add(dst) } as *mut c_void;
            self.with_read(|| unsafe { glReadPixels(x, y, w, h, format, ty, p) });
        }
        if self.gl_failed() {
            outs.push(Value::Null);
            return Ok(Value::Null);
        }
        outs.push(json!(B64.encode(buf)));
        Ok(Value::Null)
    }

    /// texImage2D, texSubImage2D, texImage3D, texSubImage3D, every overload: a DOM source (decoded,
    /// converted, flipped and premultiplied here), an array (its size checked, flipped and
    /// premultiplied when asked), null, or an offset into a PIXEL_UNPACK_BUFFER
    fn tex_image(&mut self, m: &str, a: &A, srcs: &[Option<Rc<Pixels>>]) -> Result<Value, Fail> {
        let three = m.ends_with("3D");
        let sub = m.starts_with("texSub");
        let n = a.len();
        // the index of the source argument, and whether it is the DOM overload (no size given)
        let (dom_short, at) = match (sub, three) {
            (false, false) => (n == 6, if n == 6 { 5 } else { 8 }),
            (true, false) => (n == 7, if n == 7 { 6 } else { 8 }),
            (false, true) => (false, 9),
            (true, true) => (false, 10),
        };
        if n <= at {
            return Err(throw("TypeError", format!("WebGL2RenderingContext.{m}: Not enough arguments")));
        }
        let t = a.u(0);
        let level = a.i(1);
        // (internal format, x, y, z, w, h, d, border, format, type) from the overload
        let (ifmt, xo, yo, zo, mut w, mut h, d, border, format, ty) = match (sub, three, dom_short) {
            (false, false, true) => (a.u(2), 0, 0, 0, -1, -1, 1, 0, a.u(3), a.u(4)),
            (false, false, false) => (a.u(2), 0, 0, 0, a.i(3), a.i(4), 1, a.i(5), a.u(6), a.u(7)),
            (true, false, true) => (0, a.i(2), a.i(3), 0, -1, -1, 1, 0, a.u(4), a.u(5)),
            (true, false, false) => (0, a.i(2), a.i(3), 0, a.i(4), a.i(5), 1, 0, a.u(6), a.u(7)),
            (false, true, _) => (a.u(2), 0, 0, 0, a.i(3), a.i(4), a.i(5), a.i(6), a.u(7), a.u(8)),
            (true, true, _) => (0, a.i(2), a.i(3), a.i(4), a.i(5), a.i(6), a.i(7), 0, a.u(8), a.u(9)),
        };
        let (ifmt, format, ty) = if sub {
            let (_, f, ty) = fix_formats(self.webgl2, format, format, ty);
            (0, sub_format(f), ty)
        } else {
            fix_formats(self.webgl2, ifmt, format, ty)
        };
        let upload = |w: i32, h: i32, p: *const c_void| unsafe {
            match (sub, three) {
                (false, false) => glTexImage2D(t, level, ifmt as i32, w, h, border, format, ty, p),
                (true, false) => glTexSubImage2D(t, level, xo, yo, w, h, format, ty, p),
                (false, true) => glTexImage3D(t, level, ifmt as i32, w, h, d, border, format, ty, p),
                (true, true) => glTexSubImage3D(t, level, xo, yo, zo, w, h, d, format, ty, p),
            }
        };
        // level 0's size of a 2D texture or cube face (WebGL 1's power-of-two rule, begin_draw)
        if !sub && level == 0 && (t == 0x0DE1 || (0x8515..=0x851A).contains(&t)) {
            let binding = if t == 0x0DE1 { 0x8069 } else { 0x8514 };
            let tex = geti(binding) as u32;
            let size = if w < 0 { srcs.get(at).and_then(Option::as_ref).map(|p| (p.w as i32, p.h as i32)).unwrap_or((0, 0)) } else { (w, h) };
            self.tex_size.insert(tex, size);
        }
        let unpack_buffer = if self.webgl2 { geti(0x88EF) } else { 0 };
        if a.is_source(at) {
            let Some(src) = srcs.get(at).and_then(Option::as_ref) else { return Ok(Value::Null) };
            if src.tainted {
                return Err(throw("SecurityError", "The operation is insecure."));
            }
            if unpack_buffer != 0 {
                self.err(INVALID_OPERATION);
                return Ok(Value::Null);
            }
            if w < 0 {
                w = src.w as i32;
                h = src.h as i32;
            }
            let Some(pixels) = convert(src, w.max(0) as u32, (h.max(0) * d.max(1)) as u32, format, ty, self.flip_y, self.premul) else {
                self.err(INVALID_OPERATION);
                return Ok(Value::Null);
            };
            // a DOM source's pixels are tightly packed rows; the page's unpack state is restored after
            unsafe {
                glPixelStorei(0x0CF5, 1);
                if self.webgl2 {
                    for p in [0x0CF2, 0x806E, 0x0CF4, 0x0CF3, 0x806D] {
                        glPixelStorei(p, 0);
                    }
                }
            }
            upload(w, h, pixels.as_ptr() as *const c_void);
            unsafe {
                glPixelStorei(0x0CF5, self.unpack[0]);
                if self.webgl2 {
                    for (k, p) in [0x0CF2, 0x806E, 0x0CF4, 0x0CF3, 0x806D].into_iter().enumerate() {
                        glPixelStorei(p, self.unpack[k + 1]);
                    }
                }
            }
            return Ok(Value::Null);
        }
        if a.is_number(at) && !a.is_null(at) {
            // an offset into the bound PIXEL_UNPACK_BUFFER
            if unpack_buffer == 0 {
                self.err(INVALID_OPERATION);
                return Ok(Value::Null);
            }
            upload(w, h, a.isz(at) as *const c_void);
            return Ok(Value::Null);
        }
        if a.is_null(at) {
            if unpack_buffer != 0 {
                self.err(INVALID_OPERATION);
                return Ok(Value::Null);
            }
            if sub {
                return Err(throw("TypeError", format!("WebGL2RenderingContext.{m}: Argument {} is not an object.", at + 1)));
            }
            upload(w, h, ptr::null());
            return Ok(Value::Null);
        }
        let Some(view) = a.view(at) else {
            return Err(throw("TypeError", format!("WebGL2RenderingContext.{m}: Argument {} could not be converted to any of: ArrayBufferView.", at + 1)));
        };
        if !view_matches(view.ty, ty) {
            self.err(INVALID_OPERATION);
            return Ok(Value::Null);
        }
        let es = elem_size(view.ty);
        let off = if n > at + 1 { a.u(at + 1) as usize * es } else { 0 };
        let Some(bytes) = view.bytes.get(off..) else {
            self.err(INVALID_OPERATION);
            return Ok(Value::Null);
        };
        let Some(bpp) = pixel_bytes(format, ty) else {
            self.err(INVALID_ENUM);
            return Ok(Value::Null);
        };
        let u = self.unpack;
        let need = block_bytes(w, h, d, bpp, u[0], u[1], u[2], [u[3], u[4], u[5]]);
        if w < 0 || h < 0 || d < 0 {
            self.err(INVALID_VALUE);
            return Ok(Value::Null);
        }
        if need.is_none_or(|need| need > bytes.len()) {
            self.err(INVALID_OPERATION);
            return Ok(Value::Null);
        }
        let plain = u[1..].iter().all(|v| *v == 0) && !three;
        if (self.flip_y || self.premul) && plain && w > 0 && h > 0 {
            let row = w as usize * bpp;
            let stride = row.div_ceil(u[0].max(1) as usize) * u[0].max(1) as usize;
            let mut copy = bytes[..stride * (h as usize - 1) + row].to_vec();
            if self.premul && ty == 0x1401 && channels(format) == Some(4) {
                for r in 0..h as usize {
                    for p in copy[r * stride..r * stride + row].chunks_exact_mut(4) {
                        let al = p[3] as f32;
                        for k in 0..3 {
                            p[k] = (p[k] as f32 * al / 255.0).round() as u8;
                        }
                    }
                }
            }
            if self.flip_y {
                let src = copy.clone();
                for r in 0..h as usize {
                    let from = (h as usize - 1 - r) * stride;
                    copy[r * stride..r * stride + row].copy_from_slice(&src[from..from + row]);
                }
            }
            upload(w, h, copy.as_ptr() as *const c_void);
        } else {
            upload(w, h, bytes.as_ptr() as *const c_void);
        }
        Ok(Value::Null)
    }
}

fn enabled_or_bool(p: u32) -> bool {
    match p {
        0x0B72 => {
            let mut v = 0u8;
            unsafe { glGetBooleanv(p, &mut v) };
            v != 0
        }
        p => enabled(p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(gl: &mut Gl, cmds: Value, bin: Option<&[u8]>) -> Value {
        let mut net = fetch::Net::from_env();
        let mut stores = Stores::default();
        let mut req = json!({"cmds": cmds, "sync": true});
        if let Some(b) = bin {
            req["bin"] = json!(B64.encode(b));
        }
        handle(&req, gl, &mut net, &mut stores).unwrap()
    }

    #[test]
    fn draws_and_reads_back() {
        prefer_software();
        let mut gl = Gl::default();
        let r = run(&mut gl, json!([[1, 0, "$create", ["webgl", {"antialias": false}, 4, 4, 0], 0]]), None);
        // (MRT_GL_TEST=skip on a machine without EGL's surfaceless platform)
        if r["result"].is_null() && std::env::var("MRT_GL_TEST").as_deref() == Ok("skip") {
            return;
        }
        assert!(!r["result"].is_null(), "no WebGL context: EGL's surfaceless platform with OpenGL ES 3 is needed");
        assert!(r["result"]["exts"].as_array().unwrap().contains(&json!("WEBGL_lose_context")));
        let r = run(&mut gl, json!([[1, 0, "clearColor", [1, 0, 0, 1], 0], [1, 0, "clear", [16384], 0],
                                    [1, 0, "readPixels", [0, 0, 1, 1, 0x1908, 0x1401, {"$out": "Uint8Array", "n": 4}], 0]]), None);
        assert_eq!(r["outs"], json!([B64.encode([255, 0, 0, 255])]));
        // a too small array is INVALID_OPERATION, and nothing is written
        let r = run(&mut gl, json!([[1, 0, "readPixels", [0, 0, 2, 2, 0x1908, 0x1401, {"$out": "Uint8Array", "n": 4}], 0], [1, 0, "getError", [], 0]]), None);
        assert_eq!(r["result"], json!(INVALID_OPERATION));
        // a shader that does not compile, one that does
        let r = run(&mut gl, json!([[1, 0, "createShader", [0x8B30], 7], [1, 0, "shaderSource", [{"$o": 7}, "void main() { gl_FragColor = nope; }"], 0],
                                    [1, 0, "compileShader", [{"$o": 7}], 0], [1, 0, "getShaderParameter", [{"$o": 7}, 0x8B81], 0]]), None);
        assert_eq!(r["result"], json!(false));
        let r = run(&mut gl, json!([[1, 0, "getParameter", [0x1F02], 0]]), None);
        assert_eq!(r["result"], json!("WebGL 1.0"));
        // an enabled attribute without a buffer, an index draw without one: INVALID_OPERATION, no read
        let r = run(&mut gl, json!([[1, 0, "enableVertexAttribArray", [0], 0], [1, 0, "drawArrays", [4, 0, 3], 0], [1, 0, "getError", [], 0]]), None);
        assert_eq!(r["result"], json!(INVALID_OPERATION));
        let r = run(&mut gl, json!([[1, 0, "vertexAttribPointer", [0, 2, 0x1406, false, 0, 4096], 0], [1, 0, "getError", [], 0]]), None);
        assert_eq!(r["result"], json!(INVALID_OPERATION));
        let r = run(&mut gl, json!([[1, 0, "disableVertexAttribArray", [0], 0], [1, 0, "drawElements", [4, 3, 0x1403, 64], 0], [1, 0, "getError", [], 0]]), None);
        assert_eq!(r["result"], json!(INVALID_OPERATION));
        // an unknown context is lost
        let r = run(&mut gl, json!([[9, 0, "clear", [16384], 0]]), None);
        assert_eq!(r["lost"], json!([9]));
    }

    #[test]
    fn block_sizes() {
        assert_eq!(block_bytes(2, 2, 2, 1, 4, 0, 0, [0, 0, 0]), Some(4 * 3 + 2));
        assert_eq!(block_bytes(3, 2, 1, 4, 4, 0, 0, [0, 0, 0]), Some(24));
        assert_eq!(block_bytes(0, 2, 1, 4, 4, 0, 0, [0, 0, 0]), Some(0));
        assert_eq!(f16(1.0), 0x3c00);
        assert_eq!(f16(0.5), 0x3800);
    }
}
