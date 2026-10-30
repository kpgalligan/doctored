//! #1 Indexed Color phases 1a, 1b and 1c's shared fixtures: an 8-bit RGB
//! document from its codes, the conversion and plan through the FFI, the
//! index plane read back point by point, palette PNG and GIF files written by
//! the `png` and `gif` encoders, a palette TIFF built by hand, the typed
//! export with its preview, the colour table read and replaced, and (phase
//! 2b) an edit request with its input samples, its plan and an RGB file
//! opened as a typed document (no fixture on disk), and (phases 2c and 2d)
//! the expansion oracle: Image ▸ Mode ▸ RGB Color, the canvas window of a
//! result and the tests' own re-index, written from the rule's definition.

use std::borrow::Cow;
use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::ffi::{
    dr_image_free, dr_image_height, dr_image_open, dr_image_pixels_rgba, dr_image_width,
    dr_string_free,
};
use doctored_core::ffi_export::{
    dr_export_result_bytes, dr_export_result_free, dr_export_result_info, dr_export_result_preview,
    dr_precision_doc_export_encode, dr_precision_doc_export_plan,
};
use doctored_core::ffi_indexed::{
    dr_precision_doc_convert_to_indexed, dr_precision_doc_convert_to_indexed_plan,
    dr_precision_doc_indexed_color_table, dr_precision_doc_indexed_index_at,
    dr_precision_doc_open_indexed, dr_precision_doc_set_color_table,
};
use doctored_core::ffi_precision::{
    dr_precision_doc_descriptor, dr_precision_doc_free, dr_precision_doc_open,
};
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::Value;

use crate::common::{cpath, take_err_string};

pub const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// An 8-bit RGB document (one Background layer, sRGB) of `pixels`, row-major
/// RGBA codes, each ×257 into u16 storage.
pub fn rgb_doc(width: u32, height: u32, pixels: &[[u8; 4]]) -> *mut PrecisionDocument {
    let wide: Vec<[u16; 4]> = pixels
        .iter()
        .map(|p| p.map(|c| u16::from(c) * 257))
        .collect();
    let image = PrecisionImage::from_rgba16(width, height, wide, IccProfile::srgb(), None).unwrap();
    Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ))
}

/// Opaque RGB codes as RGBA.
pub fn opaque(colors: &[[u8; 3]]) -> Vec<[u8; 4]> {
    colors.iter().map(|c| [c[0], c[1], c[2], 255]).collect()
}

pub fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
}

fn json_of(text: *mut std::ffi::c_char, err: *mut std::ffi::c_char) -> Value {
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

pub fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    json_of(unsafe { dr_precision_doc_descriptor(doc, &mut err) }, err)
}

/// Image ▸ Mode ▸ Indexed Color's plan of `doc` under `options`.
pub fn plan(doc: *const PrecisionDocument, options: &Value) -> Value {
    let text = CString::new(options.to_string()).unwrap();
    let mut err = ptr::null_mut();
    json_of(
        unsafe { dr_precision_doc_convert_to_indexed_plan(doc, text.as_ptr(), &mut err) },
        err,
    )
}

/// Image ▸ Mode ▸ Indexed Color of `doc` under `options`, or its error.
pub fn to_indexed(
    doc: *const PrecisionDocument,
    options: &Value,
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(options.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_indexed(doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

/// Image ▸ Mode ▸ Color Table's table of `doc`
/// (`dr_precision_doc_indexed_color_table`).
pub fn color_table(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    json_of(
        unsafe { dr_precision_doc_indexed_color_table(doc, &mut err) },
        err,
    )
}

/// `doc`'s palette as RGB triples and its transparent index (`color_table`).
pub fn palette_of(doc: *const PrecisionDocument) -> (Vec<[u8; 3]>, Option<u8>) {
    let table = color_table(doc);
    let colors = table["colors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| [0, 1, 2].map(|i| c[i].as_u64().unwrap() as u8))
        .collect();
    (colors, table["transparent_index"].as_u64().map(|t| t as u8))
}

/// `dr_precision_doc_set_color_table` of `doc` with `table`: the new
/// document, `None` when it changes nothing (NULL with no error), or the
/// error.
pub fn set_table(
    doc: *const PrecisionDocument,
    table: &Value,
) -> Result<Option<*mut PrecisionDocument>, String> {
    let text = CString::new(table.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_set_color_table(doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    if !out.is_null() {
        return Ok(Some(out));
    }
    if err.is_null() {
        Ok(None)
    } else {
        Err(take_err_string(err))
    }
}

/// Every index of `doc`'s plane, row-major, through `dr_precision_doc_indexed_index_at`.
pub fn indices(doc: *const PrecisionDocument) -> Vec<i32> {
    let (w, h) = unsafe { &*doc }.dimensions();
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| unsafe { dr_precision_doc_indexed_index_at(doc, x, y) })
        .collect()
}

/// The composite as RGBA16, row-major.
pub fn render16(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

/// The one layer's stored samples as RGBA16, row-major (a clear pixel keeps
/// its colour there; the composite drops it).
pub fn layer16(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    let doc = unsafe { &*doc };
    doc.layer(0).unwrap().source().unwrap().to_rgba16().unwrap()
}

/// `dr_precision_doc_open_indexed` of `path`, or its error.
pub fn open_indexed(path: &Path) -> Result<*mut PrecisionDocument, String> {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_open_indexed(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

/// A palette PNG: `indices` one per pixel, packed at `depth` (1, 2, 4 or 8)
/// by the `png` encoder, with `trns` as its tRNS when given.
pub fn write_png(
    path: &Path,
    (width, height): (u32, u32),
    depth: u8,
    palette: &[[u8; 3]],
    trns: Option<&[u8]>,
    indices: &[u8],
) {
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Indexed);
    encoder.set_depth(match depth {
        1 => png::BitDepth::One,
        2 => png::BitDepth::Two,
        4 => png::BitDepth::Four,
        _ => png::BitDepth::Eight,
    });
    encoder.set_palette(palette.concat());
    if let Some(trns) = trns {
        encoder.set_trns(trns.to_vec());
    }
    let mut writer = encoder.write_header().unwrap();
    let per_byte = 8 / usize::from(depth);
    let line = (width as usize).div_ceil(per_byte);
    let mut data = vec![0u8; line * height as usize];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let shift = 8 - usize::from(depth) * (x % per_byte + 1);
            data[y * line + x / per_byte] |= indices[y * width as usize + x] << shift;
        }
    }
    writer.write_image_data(&data).unwrap();
    writer.finish().unwrap();
}

/// A GIF of `frames` frames, each of `indices` over the whole screen, its
/// global table `palette` (a power of two entries) and `transparent`.
pub fn write_gif(
    path: &Path,
    (width, height): (u16, u16),
    palette: &[[u8; 3]],
    transparent: Option<u8>,
    indices: &[u8],
    frames: usize,
) {
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = gif::Encoder::new(file, width, height, &palette.concat()).unwrap();
    for _ in 0..frames {
        let frame = gif::Frame {
            width,
            height,
            transparent,
            buffer: Cow::Borrowed(indices),
            ..Default::default()
        };
        encoder.write_frame(&frame).unwrap();
    }
}

/// An export's encoded bytes, its info and its preview's RGBA8 pixels.
pub type Exported = (Vec<u8>, Value, Vec<[u8; 4]>);

/// `dr_precision_doc_export_encode` of `doc` (no projection) under
/// `request`, or the error.
pub fn export(doc: *const PrecisionDocument, request: &str) -> Result<Exported, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_export_encode(
            doc,
            ptr::null(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if result.is_null() {
        return Err(take_err_string(err));
    }
    let mut length = 0usize;
    let pointer = unsafe { dr_export_result_bytes(result, &mut length) };
    let bytes = unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec();
    let mut err = ptr::null_mut();
    let info = json_of(unsafe { dr_export_result_info(result, &mut err) }, err);
    let mut err = ptr::null_mut();
    let image = unsafe { dr_export_result_preview(result, &mut err) };
    assert!(!image.is_null(), "preview: {}", take_err_string(err));
    let (w, h) = unsafe { (dr_image_width(image), dr_image_height(image)) };
    let raw =
        unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(image), (w * h * 4) as usize) };
    let preview = raw
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect();
    unsafe { dr_image_free(image) };
    unsafe { dr_export_result_free(result) };
    Ok((bytes, info, preview))
}

/// `dr_precision_doc_export_plan` of `doc` (no projection), or its error.
pub fn export_plan(doc: *const PrecisionDocument, request: &str) -> Result<Value, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_export_plan(doc, ptr::null(), text.as_ptr(), &mut err) };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    Ok(json_of(out, err))
}

/// A hand-built little-endian classic TIFF, uncompressed, one strip:
/// `photometric` (3 for a palette), one `bits`-bit sample a pixel (rows
/// packed most significant bits first), or two 8-bit samples with `alpha`
/// (ExtraSamples [2]), and `color_map` as its ColorMap when not empty.
pub fn palette_tiff(
    (width, height): (u32, u32),
    bits: u16,
    photometric: u16,
    color_map: &[u16],
    alpha: Option<&[u8]>,
    indices: &[u8],
) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let data: Vec<u8> = match alpha {
        Some(alpha) => indices
            .iter()
            .zip(alpha)
            .flat_map(|(&i, &a)| [i, a])
            .collect(),
        None => {
            let line = (w * usize::from(bits)).div_ceil(8);
            let mut out = vec![0u8; line * h];
            for y in 0..h {
                for x in 0..w {
                    let bit = x * usize::from(bits);
                    out[y * line + bit / 8] |=
                        indices[y * w + x] << (8 - usize::from(bits) - bit % 8);
                }
            }
            out
        }
    };
    let samples: u16 = if alpha.is_some() { 2 } else { 1 };
    // (tag, type, count, value or offset); out-of-line arrays follow the IFD.
    let mut entries: Vec<(u16, u16, u32, u32)> = Vec::new();
    let count = 10 + u32::from(!color_map.is_empty()) + u32::from(alpha.is_some());
    let ifd_len = 2 + count * 12 + 4;
    let map_at = 8 + ifd_len;
    let data_at = map_at + color_map.len() as u32 * 2;
    entries.push((256, 4, 1, width));
    entries.push((257, 4, 1, height));
    // One SHORT, or two inline: [bits, bits].
    let bits_value = if samples == 2 {
        u32::from(bits) | (u32::from(bits) << 16)
    } else {
        u32::from(bits)
    };
    entries.push((258, 3, u32::from(samples), bits_value));
    entries.push((259, 3, 1, 1));
    entries.push((262, 3, 1, u32::from(photometric)));
    entries.push((273, 4, 1, data_at));
    entries.push((277, 3, 1, u32::from(samples)));
    entries.push((278, 4, 1, height));
    entries.push((279, 4, 1, data.len() as u32));
    entries.push((284, 3, 1, 1));
    if !color_map.is_empty() {
        entries.push((320, 3, color_map.len() as u32, map_at));
    }
    if alpha.is_some() {
        entries.push((338, 3, 1, 2));
    }
    let mut out = b"II*\0".to_vec();
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, kind, n, value) in entries {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    for v in color_map {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&data);
    out
}

/// `dr_precision_doc_edit` of `doc` with `request` and its `input` samples:
/// the new document, `None` when it changes nothing (NULL with no error),
/// or the error.
pub fn paint(
    doc: *const PrecisionDocument,
    request: &Value,
    input: &[u16],
) -> Result<Option<*mut PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            if input.is_empty() {
                ptr::null()
            } else {
                input.as_ptr()
            },
            input.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if !out.is_null() {
        return Ok(Some(out));
    }
    if err.is_null() {
        Ok(None)
    } else {
        Err(take_err_string(err))
    }
}

/// `dr_precision_doc_edit_plan` of `doc` with `request`, or its error.
pub fn edit_plan(doc: *const PrecisionDocument, request: &Value) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    Ok(json_of(out, err))
}

/// `dr_precision_doc_open` of `path` (an RGB file as a typed document), or
/// its error.
pub fn open_rgb(path: &Path) -> Result<*mut PrecisionDocument, String> {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

/// Image ▸ Mode ▸ RGB Color of `doc` (`dr_precision_doc_convert_indexed_to_rgb`).
pub fn to_rgb(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe {
        doctored_core::ffi_indexed::dr_precision_doc_convert_indexed_to_rgb(doc, &mut err)
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    out
}

fn code8(v: u16) -> u8 {
    ((u32::from(v) + 128) / 257) as u8
}

/// The canvas window of `doc`'s one layer as RGBA8 codes, read through its
/// offset; 0 outside the layer.
pub fn window(doc: *const PrecisionDocument) -> Vec<[u8; 4]> {
    let doc = unsafe { &*doc };
    let (w, h) = doc.dimensions();
    let layer = doc.layer(0).unwrap();
    let image = layer.source().unwrap();
    let (iw, ih) = image.dimensions();
    let pixels = image.to_rgba16().unwrap();
    let (ox, oy) = layer.offset();
    let mut out = Vec::new();
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let (lx, ly) = (x - i64::from(ox), y - i64::from(oy));
            out.push(
                if lx >= 0 && ly >= 0 && lx < i64::from(iw) && ly < i64::from(ih) {
                    pixels[(ly * i64::from(iw) + lx) as usize].map(code8)
                } else {
                    [0; 4]
                },
            );
        }
    }
    out
}

/// THRESH's k, from the 2b rule's definition.
const K: i128 = 131;

/// The oracle's ink, written from the rule's definition.
#[derive(Clone, Copy)]
pub enum Ink {
    Fixed([u8; 3]),
    /// Phase 3b: an ink in a blend mode but Normal; the function is the
    /// test's own full-strength colour of the ink over backdrop s
    /// (`blended(s, ink)`, 8-bit codes).
    Blended([u8; 3], fn([u8; 3], [u8; 3]) -> [u8; 3]),
    Own,
    Clear,
}

fn distance(a: [u8; 3], b: [u8; 3]) -> i128 {
    (0..3)
        .map(|c| (i128::from(a[c]) - i128::from(b[c])).pow(2))
        .sum()
}

/// The nearest non-transparent entry, ties to the lowest index.
pub fn nearest(palette: &[[u8; 3]], t: Option<u8>, c: [u8; 3]) -> i32 {
    (0..palette.len())
        .filter(|&i| Some(i as u8) != t)
        .min_by_key(|&i| (distance(palette[i], c), i))
        .unwrap() as i32
}

/// The index a clear pixel takes (phase 3a's background rule, from its
/// definition): t, else the entry nearest white, the default background
/// colour.
pub fn background(palette: &[[u8; 3]], t: Option<u8>) -> i32 {
    background_of(palette, t, [255, 255, 255])
}

/// The index a clear pixel takes with the background colour `codes` (phase
/// 3b, from the rule's definition): t, else the entry nearest `codes`.
pub fn background_of(palette: &[[u8; 3]], t: Option<u8>, codes: [u8; 3]) -> i32 {
    t.map_or_else(|| nearest(palette, None, codes), i32::from)
}

/// Whether a changed pixel `own` over backdrop `s` takes `target` (THRESH,
/// from its definition): clamp(num / den, 0, 1) >= K / 255, den 0 taking it.
fn thresh(own: [u8; 3], s: [u8; 3], target: [u8; 3]) -> bool {
    let num: i128 = (0..3)
        .map(|k| {
            (i128::from(own[k]) - i128::from(s[k])) * (i128::from(target[k]) - i128::from(s[k]))
        })
        .sum();
    let den = distance(target, s);
    den == 0 || 255 * num.clamp(0, den) >= K * den
}

/// [`oracle_with`] with white as the background colour.
pub fn oracle(
    palette: &[[u8; 3]],
    t: Option<u8>,
    before: Option<&[i32]>,
    after: &[[u8; 4]],
    ink: Ink,
) -> Result<Vec<i32>, &'static str> {
    oracle_with(palette, t, before, after, ink, [255, 255, 255])
}

/// The tests' own re-index: unchanged keeps; alpha < 128 takes
/// [`background_of`] the `background` colour; `Clear` keeps; a t backdrop
/// or none takes the nearest of the ink or the pixel's own colour; else
/// `Fixed` takes its nearest when clamp(num / den, 0, 1) >= K / 255,
/// `Blended` the nearest of its full-strength colour T over the backdrop
/// when THRESH against T holds, and `Own` its own nearest.
pub fn oracle_with(
    palette: &[[u8; 3]],
    t: Option<u8>,
    before: Option<&[i32]>,
    after: &[[u8; 4]],
    ink: Ink,
    background: [u8; 3],
) -> Result<Vec<i32>, &'static str> {
    let mut out = Vec::new();
    for (i, px) in after.iter().enumerate() {
        let old = before.map(|b| b[i]);
        if let Some(old) = old {
            let s = palette[old as usize];
            let clear = Some(old as u8) == t;
            if *px == [s[0], s[1], s[2], if clear { 0 } else { 255 }] {
                out.push(old);
                continue;
            }
        }
        if px[3] < 128 {
            out.push(background_of(palette, t, background));
            continue;
        }
        let own = [px[0], px[1], px[2]];
        let backdrop = old.filter(|&o| Some(o as u8) != t);
        out.push(match (ink, backdrop) {
            (Ink::Clear, _) => old.ok_or("an erase needs a backdrop")?,
            (Ink::Fixed(c) | Ink::Blended(c, _), None) => nearest(palette, t, c),
            (Ink::Own, _) => nearest(palette, t, own),
            (Ink::Fixed(c), Some(old)) => {
                if thresh(own, palette[old as usize], c) {
                    nearest(palette, t, c)
                } else {
                    old
                }
            }
            (Ink::Blended(c, blended), Some(old)) => {
                let s = palette[old as usize];
                let target = blended(s, c);
                if thresh(own, s, target) {
                    nearest(palette, t, target)
                } else {
                    old
                }
            }
        });
    }
    Ok(out)
}

/// `dr_image_open` of `path` (an 8-bit file as straight RGBA8): its width,
/// height and pixels, row-major.
pub fn open_rgba8(path: &Path) -> (u32, u32, Vec<[u8; 4]>) {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let image = unsafe { dr_image_open(c.as_ptr(), &mut err) };
    assert!(
        !image.is_null(),
        "{}: {}",
        path.display(),
        take_err_string(err)
    );
    let (w, h) = unsafe { (dr_image_width(image), dr_image_height(image)) };
    let raw =
        unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(image), (w * h * 4) as usize) };
    let pixels = raw
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect();
    unsafe { dr_image_free(image) };
    (w, h, pixels)
}
