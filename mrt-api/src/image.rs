//! Native image loading (B164): bytes from the page's network/partition, sized as Gecko's
//! metadata decode sizes them (B165), with no pixels decoded; only load/error, natural size and
//! timing leave the API process.
use std::io::Cursor;
use std::time::Instant;

use base64::Engine;
use image::{ImageDecoder, ImageReader};
use serde_json::{Value, json};

use crate::{fetch, store::Stores};

/// The natural size of an image's bytes, or None where Gecko fires `error`. Like Gecko for an image
/// nobody paints, this is the metadata decode: a raster's header (no pixels are decoded, so bad pixel
/// data or a cut stream after the header still loads, and no size is too large), AVIF's container,
/// an SVG document's root.
fn dimensions(bytes: &[u8], mime: &str) -> Option<(u32, u32)> {
    let essence = mime.split(';').next()?.trim();
    if essence.eq_ignore_ascii_case("image/svg+xml") {
        return svg_size(bytes, mime);
    }
    if is_avif(bytes) {
        return avif_size(bytes);
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    reader.no_limits();
    let mut decoder = reader.into_decoder().ok()?;
    let (width, height) = decoder.dimensions();
    // EXIF orientations 5 to 8 turn the image a quarter.
    use image::metadata::Orientation as O;
    Some(match decoder.orientation().ok()? {
        O::Rotate90 | O::Rotate270 | O::Rotate90FlipH | O::Rotate270FlipH => (height, width),
        _ => (width, height),
    })
}

/// Gecko's sniffer (imgLoader::GetMimeTypeFromContent, nsMediaSniffer's ftyp table): an ftyp box
/// whose major or compatible brands include `avif`.
fn is_avif(bytes: &[u8]) -> bool {
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return false;
    }
    let size = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
    let ftyp = &bytes[8..size.clamp(12, bytes.len())];
    // major brand, minor version, then compatible brands
    ftyp.chunks_exact(4)
        .enumerate()
        .any(|(i, brand)| i != 1 && brand == b"avif")
}

/// nsAVIFDecoder's metadata decode: mp4parse at Gecko's strictness (image.avif.compliance_strictness
/// 1, Normal), the info Gecko reads (every part of it must parse), the primary item's data present,
/// then ispe's size turned by irot. A file without ispe makes Gecko decode the AV1 for its size,
/// which this does not (GAPS B165).
fn avif_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let context = mp4parse::read_avif(&mut Cursor::new(bytes), mp4parse::ParseStrictness::Normal).ok()?;
    let ispe = context.spatial_extents_ptr().ok()?;
    context.nclx_colour_information_ptr().transpose().ok()?;
    context.icc_colour_information().transpose().ok()?;
    let rotation = context.image_rotation().ok()?;
    context.image_mirror_ptr().ok()?;
    context.pixel_aspect_ratio_ptr().ok()?;
    context.primary_item_bits_per_channel().transpose().ok()?;
    context.alpha_item_bits_per_channel().transpose().ok()?;
    if context.primary_item_coded_data().is_none_or(<[u8]>::is_empty) || ispe.is_null() {
        return None;
    }
    // ImageSpatialExtentsProperty is #[repr(C)] { image_width: u32, image_height: u32 }, the struct
    // mp4parse_capi hands Gecko; its fields are private to the crate.
    let [width, height] = unsafe { *(ispe as *const [u32; 2]) };
    // Decoder::PostSize takes int32_t
    if width > i32::MAX as u32 || height > i32::MAX as u32 {
        return None;
    }
    Some(match rotation {
        mp4parse::ImageRotation::D90 | mp4parse::ImageRotation::D270 => (height, width),
        _ => (width, height),
    })
}

/// The XML's text, decoded as Gecko's parser picks its encoding: a byte order mark, then the
/// Content-Type's charset, then the XML declaration's, then UTF-8. Malformed bytes are an error.
fn svg_text(bytes: &[u8], mime: &str) -> Option<String> {
    use encoding_rs::Encoding;
    let declared = || -> Option<&'static Encoding> {
        let head = &bytes[..bytes.len().min(1024)];
        let head = std::str::from_utf8(head)
            .or_else(|e| std::str::from_utf8(&head[..e.valid_up_to()]))
            .ok()?;
        let decl = head.strip_prefix("<?xml")?;
        let decl = &decl[..decl.find("?>")?];
        let at = decl.find("encoding")?;
        let rest = decl[at + 8..].trim_start().strip_prefix('=')?.trim_start();
        let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let label = &rest[1..][..rest[1..].find(quote)?];
        Encoding::for_label(label.as_bytes())
    };
    let charset = || {
        mime.split(';').skip(1).find_map(|param| {
            let (name, value) = param.split_once('=')?;
            name.trim()
                .eq_ignore_ascii_case("charset")
                .then(|| Encoding::for_label(value.trim().trim_matches('"').as_bytes()))?
        })
    };
    let (encoding, body) = match Encoding::for_bom(bytes) {
        Some((encoding, bom)) => (encoding, &bytes[bom..]),
        None => (charset().or_else(declared).unwrap_or(encoding_rs::UTF_8), bytes),
    };
    encoding
        .decode_without_bom_handling_and_without_replacement(body)
        .map(|text| text.into_owned())
}

/// The default font's metrics, in ems: Gecko's gfxFont metrics of the first font for `serif`
/// (font.default, font.name.serif.x-western: fontconfig's match).
#[derive(Clone, Copy)]
struct FontMetrics {
    /// OS/2 sxHeight, else the `x` glyph's top; 0.5 when missing
    x_height: f64,
    /// OS/2 sCapHeight, else the `H` glyph's top; 1 when missing
    cap_height: f64,
    /// the advance of `0`; 0.5 when missing
    zero_width: f64,
    /// the advance of U+6C34 (水); 1 when missing
    ideographic_width: f64,
    /// hhea's (or with USE_TYPO_METRICS, OS/2's typo) ascent, descent and line gap
    ascent: f64,
    descent: f64,
    line_gap: f64,
}

impl FontMetrics {
    /// line-height: normal at `size` px (GetNormalLineHeight): FreeType's hinted ascent and
    /// descent, each up to a whole pixel, plus the line gap.
    fn normal_line_height(&self, size: f64) -> f64 {
        (self.ascent * size).ceil() + (self.descent * size).ceil() + (self.line_gap * size).round().max(0.0)
    }
}

fn default_font_metrics() -> FontMetrics {
    static METRICS: std::sync::OnceLock<FontMetrics> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let fallback = FontMetrics {
            x_height: 0.5,
            cap_height: 1.0,
            zero_width: 0.5,
            ideographic_width: 1.0,
            ascent: 0.8,
            descent: 0.2,
            line_gap: 0.0,
        };
        let measure = || -> Option<FontMetrics> {
            let out = std::process::Command::new("fc-match")
                .args(["-f", "%{file}\n%{index}", "serif:lang=en"])
                .output()
                .ok()?;
            let out = String::from_utf8(out.stdout).ok()?;
            let (file, index) = out.split_once('\n')?;
            let data = std::fs::read(file).ok()?;
            let face = ttf_parser::Face::parse(&data, index.trim().parse().unwrap_or(0)).ok()?;
            let em = face.units_per_em() as f64;
            let glyph_top = |c| Some(face.glyph_bounding_box(face.glyph_index(c)?)?.y_max as f64);
            let advance = |c| Some(face.glyph_hor_advance(face.glyph_index(c)?)? as f64);
            let height = |table: Option<i16>, c| table.filter(|h| *h > 0).map(f64::from).or_else(|| glyph_top(c));
            let typo = face.tables().os2.is_some_and(|os2| os2.use_typographic_metrics());
            let (ascent, descent, line_gap) = if typo {
                (face.typographic_ascender(), face.typographic_descender(), face.typographic_line_gap())
            } else {
                (Some(face.ascender()), Some(face.descender()), Some(face.line_gap()))
            };
            Some(FontMetrics {
                x_height: height(face.x_height(), 'x').map_or(fallback.x_height, |h| h / em),
                cap_height: height(face.capital_height(), 'H').map_or(fallback.cap_height, |h| h / em),
                zero_width: advance('0').map_or(fallback.zero_width, |a| a / em),
                ideographic_width: advance('\u{6C34}').map_or(fallback.ideographic_width, |a| a / em),
                ascent: ascent.map_or(fallback.ascent, |a| a as f64 / em),
                descent: descent.map_or(fallback.descent, |d| -(d as f64) / em),
                line_gap: line_gap.map_or(0.0, |g| g as f64 / em),
            })
        };
        measure().unwrap_or(fallback)
    })
}

/// px per unit of a font-relative unit, for a font of `size` px (the root's own font: `rem` and
/// the other root units are the same as their local ones on the root).
fn font_unit(unit: &str, size: f64) -> Option<f64> {
    let metrics = default_font_metrics();
    Some(match unit {
        "em" | "rem" => size,
        "ex" | "rex" => size * metrics.x_height,
        "cap" | "rcap" => size * metrics.cap_height,
        "ch" | "rch" => size * metrics.zero_width,
        "ic" | "ric" => size * metrics.ideographic_width,
        "lh" | "rlh" => metrics.normal_line_height(size),
        _ => return None,
    })
}

/// The root element, for its own style sheets' selectors.
#[derive(Clone, Copy)]
struct Root<'a, 'input>(roxmltree::Node<'a, 'input>);

impl simplecss::Element for Root<'_, '_> {
    fn parent_element(&self) -> Option<Self> {
        None
    }
    fn prev_sibling_element(&self) -> Option<Self> {
        None
    }
    fn has_local_name(&self, name: &str) -> bool {
        self.0.tag_name().name() == name
    }
    fn attribute_matches(&self, name: &str, operator: simplecss::AttributeOperator<'_>) -> bool {
        self.0.attribute(name).is_some_and(|v| operator.matches(v))
    }
    fn pseudo_class_matches(&self, class: simplecss::PseudoClass<'_>) -> bool {
        matches!(class, simplecss::PseudoClass::FirstChild)
    }
}

/// A CSS font-size of the root (the document's initial font is 16px): an absolute length, a size
/// relative to the initial font, a keyword (Gecko's sizes for medium 16px), a calc() of sums.
fn font_size(value: &str, presentation: bool) -> Option<f64> {
    let value = value.trim().to_ascii_lowercase();
    let keyword = match value.as_str() {
        "xx-small" => Some(9.0),
        "x-small" => Some(10.0),
        "small" => Some(13.0),
        "medium" => Some(16.0),
        "large" => Some(18.0),
        "x-large" => Some(24.0),
        "xx-large" => Some(32.0),
        "xxx-large" => Some(48.0),
        "smaller" => Some(16.0 / 1.2),
        "larger" => Some(16.0 * 1.2),
        _ => None,
    };
    if keyword.is_some() {
        return keyword;
    }
    if let Some(sum) = value.strip_prefix("calc(").and_then(|v| v.strip_suffix(')')) {
        // terms joined by + and - (which CSS requires spaces around); a negative total is 0
        let mut total = 0.0;
        let mut sign = 1.0;
        for (i, token) in sum.split_whitespace().enumerate() {
            match (i % 2, token) {
                (1, "+") => sign = 1.0,
                (1, "-") => sign = -1.0,
                (0, term) => total += sign * font_length(term)?,
                _ => return None,
            }
        }
        return Some(total.max(0.0));
    }
    let (number, unit) = split_number(&value)?;
    // a presentation attribute takes a bare number as px
    if unit.is_empty() && (presentation || number == 0.0) {
        return (number >= 0.0).then_some(number);
    }
    font_length(&value).filter(|px| *px >= 0.0)
}

/// A length or percentage in a font-size, against the initial 16px font.
fn font_length(value: &str) -> Option<f64> {
    let (number, unit) = split_number(value)?;
    Some(match unit {
        "%" => number * 16.0 / 100.0,
        "" => return None,
        unit => number * font_unit(unit, 16.0).or_else(|| absolute_unit(unit))?,
    })
}

/// The font-size a `font` shorthand sets: the token before the family (and before any
/// `/line-height`), after the style, variant, weight and stretch.
fn font_shorthand_size(value: &str) -> Option<f64> {
    let value = value.trim();
    let family_at = value.find(|c| c == '"' || c == '\'').unwrap_or(value.len());
    value[..family_at]
        .split_whitespace()
        .find_map(|token| font_size(token.split('/').next()?, false))
}

/// A leading number and what follows it (SVGContentUtils::ParseNumber): a sign, digits with an
/// optional fraction, an exponent only when a digit follows its `e` (so `1em` is 1 and `em`).
fn split_number(text: &str) -> Option<(f64, &str)> {
    let b = text.as_bytes();
    let digits = |mut i: usize| {
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        i
    };
    let start = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let mut end = digits(start);
    let whole = end > start;
    if b.get(end) == Some(&b'.') {
        let fraction = digits(end + 1);
        if fraction == end + 1 {
            return None;
        }
        end = fraction;
    } else if !whole {
        return None;
    }
    if matches!(b.get(end), Some(b'e' | b'E')) {
        let sign = end + 1 + usize::from(matches!(b.get(end + 1), Some(b'+' | b'-')));
        let exponent = digits(sign);
        if exponent > sign {
            end = exponent;
        }
    }
    let number: f64 = text[..end].parse().ok()?;
    number.is_finite().then_some((number, &text[end..]))
}

/// px per unit of the absolute units (SVGLength::GetAbsUnitsPerAbsUnit).
fn absolute_unit(unit: &str) -> Option<f64> {
    Some(match unit {
        "" | "px" => 1.0,
        "in" => 96.0,
        "cm" => 96.0 / 2.54,
        "mm" => 96.0 / 25.4,
        "q" => 96.0 / 101.6,
        "pt" => 96.0 / 72.0,
        "pc" => 16.0,
        _ => return None,
    })
}

/// The root's computed font-size: its font-size presentation attribute, then the document's
/// <style> sheets' rules for it by specificity and order, then its style attribute, !important
/// last; `font` sets it as font-size does. A value this does not read leaves the size before it.
fn root_font_size(doc: &roxmltree::Document, root: roxmltree::Node) -> f64 {
    let svg_ns = "http://www.w3.org/2000/svg";
    let mut size = root
        .attribute("font-size")
        .and_then(|v| font_size(v, true))
        .unwrap_or(16.0);
    let sheets: Vec<String> = doc
        .descendants()
        .filter(|n| n.has_tag_name((svg_ns, "style")))
        .filter(|n| n.attribute("type").is_none_or(|t| t.is_empty() || t.eq_ignore_ascii_case("text/css")))
        .map(|n| n.children().filter_map(|c| c.text()).collect())
        .collect();
    let mut found: Vec<(bool, [u8; 3], usize, simplecss::Declaration)> = Vec::new();
    let mut order = 0;
    for sheet in &sheets {
        for rule in simplecss::StyleSheet::parse(sheet).rules {
            if !rule.selector.matches(&Root(root)) {
                continue;
            }
            for decl in rule.declarations.iter().filter(|d| matches!(d.name, "font-size" | "font")) {
                order += 1;
                found.push((decl.important, rule.selector.specificity(), order, *decl));
            }
        }
    }
    // the style attribute beats every selector
    let inline: Vec<_> = root
        .attribute("style")
        .map(|s| {
            simplecss::DeclarationTokenizer::from(s)
                .filter(|d| matches!(d.name, "font-size" | "font"))
                .collect()
        })
        .unwrap_or_default();
    for decl in &inline {
        order += 1;
        found.push((decl.important, [u8::MAX; 3], order, *decl));
    }
    found.sort_by_key(|(important, specificity, order, _)| (*important, *specificity, *order));
    for (_, _, _, decl) in found {
        let px = match decl.name {
            "font" => font_shorthand_size(decl.value),
            _ => font_size(decl.value, false),
        };
        if let Some(px) = px {
            size = px;
        }
    }
    size
}

/// An SVG image's natural size: SVGSVGElement's intrinsic width and height from the root's width
/// and height attributes (not its CSS), with the viewBox's ratio for a missing one, and the
/// fallback 300 by 150 (VectorImage::GetWidth, HTMLImageElement's natural size).
fn svg_size(bytes: &[u8], mime: &str) -> Option<(u32, u32)> {
    let text = svg_text(bytes, mime)?;
    // Parsed as a non-executing image: no scripts, no external resources.
    let doc = roxmltree::Document::parse_with_options(
        &text,
        roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() },
    )
    .ok()?;
    let root = doc.root_element();
    if !root.has_tag_name(("http://www.w3.org/2000/svg", "svg")) {
        return None;
    }
    let font = root_font_size(&doc, root);
    // SVGLength::SetValueFromString: one token, a number, a unit in any case; Some(None) is a
    // percentage (no intrinsic size); negative is an error, as if not set.
    let length = |name| -> Option<Option<f64>> {
        let value = root
            .attribute(name)?
            .trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r'))
            .to_ascii_lowercase();
        let (number, unit) = split_number(&value)?;
        let px_per_unit = match unit {
            "%" => return (number >= 0.0).then_some(None),
            // an SVG image's document has no CSS viewport
            "vw" | "vh" | "vmin" | "vmax" => 0.0,
            unit => font_unit(unit, font).or_else(|| absolute_unit(unit))?,
        };
        (number >= 0.0).then_some(Some(number * px_per_unit))
    };
    let width = length("width");
    let height = length("height");
    let ratio = root
        .attribute("viewBox")
        .and_then(|v| v.parse::<svgtypes::ViewBox>().ok())
        .filter(|v| v.w > 0.0 && v.h > 0.0)
        .map(|v| v.w / v.h);
    let (w, h) = match (width.flatten(), height.flatten(), ratio) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some(r)) => (w, w / r),
        (None, Some(h), Some(r)) => (h * r, h),
        (None, None, Some(r)) => (300.0, 300.0 / r),
        (w, h, None) => (w.unwrap_or(300.0), h.unwrap_or(150.0)),
    };
    Some((w.round() as u32, h.round() as u32))
}

pub fn handle(req: &Value, net: &mut fetch::Net, stores: &mut Stores) -> Result<Value, String> {
    load(req, |request| fetch::image(request, net, stores))
}

/// `handle` for a live job (fetch_jobs, B218): the image loads on a worker while the page runs,
/// as Gecko's image loads do; the reply's `ms` is 0, since real time passed meanwhile.
pub fn live(req: &Value, net: &mut fetch::Net, stores: &mut impl fetch::CookieStores, timeout: std::time::Duration) -> Result<Value, String> {
    let mut reply = load(req, |request| fetch::image_live(request, net, stores, timeout))?;
    reply["ms"] = json!(0);
    Ok(reply)
}

/// An image request's reply ({ok, width, height, ms}, or the fetch's pending reply): its bytes from
/// the runtime (a blob: URL) or from `get` (the network), and the size their header gives.
fn load(req: &Value, get: impl FnOnce(&Value) -> Result<Value, String>) -> Result<Value, String> {
    let started = Instant::now();
    let mut ms = 0.0;
    let mime;
    let bytes;
    if let Some(encoded) = req.get("bodyBase64").and_then(Value::as_str) {
        // A blob URL belongs to the runtime; its registry sent the actual bytes.
        bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap_or_default();
        mime = req
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
    } else {
        let cross = req.get("crossOrigin").and_then(Value::as_str);
        let (mode, credentials) = match cross {
            None => ("no-cors", "include"),
            Some(value) if value.eq_ignore_ascii_case("use-credentials") => ("cors", "include"),
            _ => ("cors", "same-origin"),
        };
        let mut request = req.clone();
        request["method"] = json!("GET");
        request["mode"] = json!(mode);
        request["credentials"] = json!(credentials);
        request["bytes"] = json!(true);
        request["headers"] = json!([[
            "accept",
            "image/webp,image/png,image/svg+xml,image/*;q=0.8,*/*;q=0.5"
        ]]);
        let response = match get(&request) {
            Ok(response) => response,
            Err(_) => {
                return Ok(
                    json!({"ok": false, "width": 0, "height": 0, "ms": started.elapsed().as_secs_f64() * 1000.0}),
                );
            }
        };
        if response.get("pending").and_then(Value::as_bool) == Some(true) {
            return Ok(response);
        }
        ms = response.get("took").and_then(Value::as_f64).unwrap_or(0.0);
        // Gecko decodes valid image bodies even for non-success HTTP statuses.
        bytes = response
            .get("bodyBase64")
            .and_then(Value::as_str)
            .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
            .unwrap_or_default();
        mime = response
            .get("headers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|h| {
                h[0].as_str()
                    .is_some_and(|k| k.eq_ignore_ascii_case("content-type"))
            })
            .and_then(|h| h[1].as_str())
            .unwrap_or("")
            .to_string();
    }
    let decode_start = Instant::now();
    let size = dimensions(&bytes, &mime);
    ms += decode_start.elapsed().as_secs_f64() * 1000.0;
    let (width, height) = size.unwrap_or((0, 0));
    Ok(json!({"ok": size.is_some(), "width": width, "height": height, "ms": ms}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_each_format_and_rejects_a_cut_header() {
        for format in [
            image::ImageFormat::Png,
            image::ImageFormat::Jpeg,
            image::ImageFormat::Gif,
            image::ImageFormat::WebP,
            image::ImageFormat::Bmp,
            image::ImageFormat::Ico,
        ] {
            let img = if format == image::ImageFormat::Ico {
                image::DynamicImage::new_rgba8(7, 3)
            } else {
                image::DynamicImage::new_rgb8(7, 3)
            };
            let mut data = Cursor::new(Vec::new());
            img.write_to(&mut data, format).unwrap();
            assert_eq!(dimensions(data.get_ref(), ""), Some((7, 3)), "{format:?}");
            assert_eq!(dimensions(&data.get_ref()[..10], ""), None);
        }
    }

    #[test]
    #[ignore = "requires bench image fixtures; generate with python3 bench/make_images_formats.py, then run with --ignored"]
    fn sizes_from_headers_as_gecko_metadata_decode() {
        let case = |name: &str| std::fs::read(format!("{}/../bench/bridge-cases/images-formats-{name}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        // bad or missing pixel data after the header still has a size; no IDAT at all does not
        assert_eq!(dimensions(&case("idat.png"), ""), Some((7, 3)));
        assert_eq!(dimensions(&case("cut.png"), ""), Some((7, 3)));
        assert_eq!(dimensions(&case("header.png"), ""), None);
        assert_eq!(dimensions(&case("axis.png"), ""), Some((70000, 1)));
        assert_eq!(dimensions(&case("bad.avif"), ""), Some((7, 3)));
        assert_eq!(dimensions(&case("irot.avif"), ""), Some((3, 7)));
        assert_eq!(dimensions(&case("cut.avif"), ""), None);
    }

    #[test]
    fn svg_lengths_fonts_and_encodings() {
        let svg = |attrs: &str| format!(r#"<svg xmlns="http://www.w3.org/2000/svg" {attrs}/>"#);
        let size = |attrs: &str| dimensions(svg(attrs).as_bytes(), "image/svg+xml");
        // the attributes, not the root's CSS
        assert_eq!(size(r#"width="17" height="9" style="width:40px""#), Some((17, 9)));
        assert_eq!(size(r#"width=" 12PX " height="1IN""#), Some((12, 96)));
        assert_eq!(size(r#"width="-5" height="7""#), Some((300, 7)));
        assert_eq!(size(r#"width="10vw" height="50%""#), Some((0, 150)));
        assert_eq!(size(r#"width="2em" height="1em" style="font:bold 20px/2 serif""#), Some((40, 20)));
        assert_eq!(size(r#"width="2em" height="1em" font-size="20" style="font-size:calc(10px + 5px)""#), Some((30, 15)));
        let latin1 = |decl: &str| {
            let mut bytes = format!(r#"{decl}<svg xmlns="http://www.w3.org/2000/svg" width="17" height="9"><title>caf"#).into_bytes();
            bytes.extend(b"\xe9</title></svg>");
            bytes
        };
        assert_eq!(dimensions(&latin1(""), "image/svg+xml"), None);
        assert_eq!(dimensions(&latin1(r#"<?xml version="1.0" encoding="ISO-8859-1"?>"#), "image/svg+xml"), Some((17, 9)));
        assert_eq!(dimensions(&latin1(""), "image/svg+xml; charset=windows-1252"), Some((17, 9)));
        let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain(svg(r#"width="5" height="6""#).encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(dimensions(&utf16, "image/svg+xml"), Some((5, 6)));
    }

    #[test]
    fn svg_is_an_image_without_external_resources() {
        assert_eq!(dimensions(br#"<svg xmlns="http://www.w3.org/2000/svg" width="17" height="9"><image href="file:///no-such-file.png"/></svg>"#, "image/svg+xml"), Some((17, 9)));
        assert_eq!(dimensions(b"not SVG", "image/svg+xml"), None);
    }
}
