//! Pattern fill layers in PSD: our 8- and 16-bit layered
//! export writes a pattern fill layer in Photoshop's form (the controller's
//! first ruling) — the rendered fill over its mask's rectangle (the canvas
//! without a mask), its `-2` mask, flags `0x18`, `PtFl`, `fxrp` = its reference and
//! its tile in `Patt`/`Pat2` — and our importer reads it back as the same
//! pattern fill layer — the description's `pattern_fill` equal, the pixels
//! the scale-1 closed form, the mask and opacity kept — for a linked fill on
//! a moved layer and an unlinked one. The same record without bit 4, and
//! pixel-less, is read too, each named in the report; a carried fill is
//! admitted as a canvas raster; a raster record whose table lost the tile
//! stays a raster with its marker losses, a pixel-less one is omitted with
//! its reason. A CMYK file's `PtFl` is `psd_pattern_fill_cmyk.rs`'s.
//!
//! The oracle is written here: at scale 1 and angle 0, pixel (x, y) of the
//! fill is tile[(x − Ox) mod w, (y − Oy) mod h] with O = phase + reference
//! when linked and O = phase when unlinked.

use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::{dr_artwork_render, dr_doc_artwork, dr_precision_doc_artwork};
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_doc_save_psd, dr_precision_doc_open_psd, dr_precision_doc_save_psd,
    dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, solid, take_err_string};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 96;
const H: u32 = 64;

// ---------------------------------------------------------------- plumbing --

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                T[((n >> (18 - 6 * i)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

/// take1's tile T (12 × 8, opaque), `pattern_overlay_take.texel`.
fn tile_t() -> Vec<u8> {
    let mut rgba = Vec::new();
    for y in 0..8u32 {
        for x in 0..12u32 {
            rgba.extend_from_slice(&[
                (20 * x + 10) as u8,
                (30 * y + 15) as u8,
                if (x + y) % 2 == 0 { 200 } else { 60 },
                255,
            ]);
        }
    }
    rgba
}

/// Tile T's texel at canvas pixel (x, y) with the tile's origin at `o`.
fn texel(o: (i32, i32), x: u32, y: u32) -> [u8; 4] {
    let tile = tile_t();
    let i = (x as i32 - o.0).rem_euclid(12) as usize;
    let j = (y as i32 - o.1).rem_euclid(8) as usize;
    let at = (j * 12 + i) * 4;
    tile[at..at + 4].try_into().unwrap()
}

/// One fill under test: linked or not, its phase and its layer's reference.
#[derive(Clone, Copy)]
struct Case {
    linked: bool,
    phase: (i32, i32),
    reference: (i32, i32),
}

impl Case {
    fn origin(&self) -> (i32, i32) {
        if self.linked {
            (
                self.phase.0 + self.reference.0,
                self.phase.1 + self.reference.1,
            )
        } else {
            self.phase
        }
    }

    /// The `pattern_fill` keys as a fill layer's description stores them.
    fn keys(&self) -> Value {
        json!({
            "pattern": {"name": "T", "width": 12, "height": 8, "rgba": base64(&tile_t())},
            "angle": 0, "scale": 1, "link_with_layer": self.linked,
            "phase": [self.phase.0, self.phase.1],
        })
    }

    fn description(&self) -> String {
        json!({"type": "fill", "version": 1, "fill": "pattern",
               "pattern_fill": self.keys(), "canvas": [W, H]})
        .to_string()
    }

    /// The host's drawing: the keys plus `anchor` = −reference when linked.
    fn components(&self) -> String {
        let mut fill = self.keys();
        if self.linked {
            fill["anchor"] = json!([-self.reference.0, -self.reference.1]);
        }
        json!([{"kind": "pattern", "fill": fill}]).to_string()
    }
}

/// Linked on a layer moved (+7, +3); unlinked with a phase, its reference
/// (4, 4) ignored.
const LINKED: Case = Case {
    linked: true,
    phase: (0, 0),
    reference: (7, 3),
};
const UNLINKED: Case = Case {
    linked: false,
    phase: (5, 2),
    reference: (4, 4),
};

pub(crate) fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

fn render(case: &Case) -> PrecisionImage {
    let components = CString::new(case.components()).unwrap();
    let profile = IccProfile::srgb();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_artwork_render(
            W,
            H,
            components.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    *unsafe { Box::from_raw(out) }
}

/// The masked fill's mask rectangle: (10, 6) to (70, 50), top-left-first.
const MASK_AT: (i32, i32) = (10, 6);
const MASK_SIZE: (u32, u32) = (60, 44);

/// A canvas-sized rectangle selection, (10, 6) to (70, 50).
fn rect_selection() -> Vec<u8> {
    let mut sel = vec![0u8; (W * H) as usize];
    for y in 6..50 {
        for x in 10..70 {
            sel[(y * W + x) as usize] = 255;
        }
    }
    sel
}

/// White, then LINKED (masked, opacity 0.6), then UNLINKED.
fn byte_doc() -> DrDocument {
    let mut doc = DrDocument::from_pixels(solid(W, H, [255; 4]));
    for (index, case) in [LINKED, UNLINKED].iter().enumerate() {
        let image = render(case);
        let name = CString::new(format!("Fill {index}")).unwrap();
        let text = CString::new(case.description()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_doc_artwork(
                &doc,
                index,
                false,
                &image,
                0,
                0,
                name.as_ptr(),
                text.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert_eq!(owned(err), None);
        doc = *unsafe { Box::from_raw(out) };
        doc.layers[index + 1].reference = case.reference;
    }
    let mut doc = doc
        .add_mask(1, MaskKind::FromSelection(&rect_selection()))
        .expect("the mask");
    // The mask as its own rectangle, so the record's rectangle is smaller
    // than the canvas.
    doc.layers[1].mask = Some(std::sync::Arc::new(image::GrayImage::from_pixel(
        MASK_SIZE.0,
        MASK_SIZE.1,
        image::Luma([255]),
    )));
    doc.layers[1].mask_properties = doctored_core::doc_mask::MaskProperties::at(MASK_AT);
    doc.with_layer_opacity(1, 0.6).expect("the opacity")
}

/// White, then LINKED (masked, opacity 0.6), then UNLINKED, at 16 bits.
fn typed_doc() -> PrecisionDocument {
    let white = PrecisionImage::from_rgba16(
        W,
        H,
        vec![[65535; 4]; (W * H) as usize],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let base = Layer::raster("Base", white, (0, 0), 1.0, true).unwrap();
    let mut doc = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![base],
    )
    .unwrap();
    for (index, case) in [LINKED, UNLINKED].iter().enumerate() {
        let image = render(case);
        let name = CString::new(format!("Fill {index}")).unwrap();
        let text = CString::new(case.description()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_artwork(
                &doc,
                index,
                false,
                &image,
                0,
                0,
                name.as_ptr(),
                text.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert_eq!(owned(err), None);
        doc = *unsafe { Box::from_raw(out) };
    }
    let mut layers: Vec<Layer> = (0..doc.layer_count())
        .map(|i| doc.layer(i).unwrap().clone())
        .collect();
    let plane = doctored_core::precision_mask::PrecisionPlane::from_u16(
        MASK_SIZE.0,
        MASK_SIZE.1,
        vec![65535; (MASK_SIZE.0 * MASK_SIZE.1) as usize],
        None,
    )
    .unwrap();
    let mask = doctored_core::precision_mask::PrecisionMask::new(
        plane,
        doctored_core::doc_mask::MaskProperties::at(MASK_AT),
        true,
    )
    .unwrap();
    layers[1] = layers[1]
        .clone()
        .with_reference(LINKED.reference)
        .with_mask(Some(mask));
    layers[2] = layers[2].clone().with_reference(UNLINKED.reference);
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap();
    doc.set_layer_opacity(1, 0.6).unwrap().expect("the opacity")
}

pub(crate) fn import_report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(raw)).expect("import report JSON")
}

pub(crate) fn open8(path: &Path) -> DrDocument {
    let report = import_report(path);
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_doc_open_psd(
            cpath(path).as_ptr(),
            0,
            token.as_ptr(),
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "open_psd: {}", take_err_string(error));
    *unsafe { Box::from_raw(raw) }
}

pub(crate) fn open16(path: &Path) -> PrecisionDocument {
    let report = import_report(path);
    let token = report["source_fingerprint"].as_str().unwrap().to_string();
    try_open16(path, &token, LIMIT).unwrap_or_else(|error| panic!("typed open: {error}"))
}

/// The typed layered open under the working limit `limit`.
pub(crate) fn try_open16(
    path: &Path,
    token: &str,
    limit: u64,
) -> Result<PrecisionDocument, String> {
    let token = CString::new(token).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            0,
            token.as_ptr(),
            limit,
            0,
            ptr::null::<DrJobControl>(),
            &mut error,
        )
    };
    if raw.is_null() {
        return Err(take_err_string(error));
    }
    let doc = unsafe { (*raw).clone() };
    unsafe { doctored_core::ffi_precision::dr_precision_doc_free(raw) };
    Ok(doc)
}

pub(crate) fn save8(doc: &DrDocument, path: &Path) {
    let mut error: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_doc_save_psd(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            0,
            0,
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(ok, "save: {}", take_err_string(error));
}

pub(crate) fn save16(doc: &PrecisionDocument, path: &Path) {
    let projection = doc.render(LIMIT, 0, None).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            &projection,
            cpath(path).as_ptr(),
            0,
            0,
            LIMIT,
            0,
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(ok, "save16: {}", take_err_string(error));
}

pub(crate) fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle)
}

fn be32(bytes: &[u8], at: usize) -> usize {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

/// One layer record as Adobe frames it.
pub(crate) struct Framed {
    pub(crate) bounds: [i32; 4],
    pub(crate) name: String,
    pub(crate) flags: u8,
    /// The flags byte's offset in the file.
    pub(crate) flags_at: usize,
    /// The mask record's rectangle and outside coverage, when it has one.
    pub(crate) mask: Option<([i32; 4], u8)>,
    pub(crate) blocks: Vec<([u8; 4], Vec<u8>)>,
    /// `(id, stored bytes)`: each channel's compression word and data.
    pub(crate) channels: Vec<(i16, Vec<u8>)>,
    /// The record's bytes from its blend signature to its end (everything
    /// after the channel table), verbatim.
    metadata: Vec<u8>,
}

/// A classic PSD's layer section as Adobe's "Layer and Mask Information"
/// frames it, written here so the test does not borrow the core's reader:
/// the offsets of the section's and the layer info's length words, the
/// records, and the end of the layer info.
struct Layout {
    section_at: usize,
    info_at: usize,
    records: Vec<Framed>,
}

fn layout(bytes: &[u8]) -> Layout {
    let mut at = 26;
    at += 4 + be32(bytes, at); // colour mode data
    at += 4 + be32(bytes, at); // image resources
    let section_at = at;
    at += 4;
    let info_at = at;
    at += 4;
    let count = i16::from_be_bytes(bytes[at..at + 2].try_into().unwrap()).unsigned_abs();
    at += 2;
    let mut records = Vec::new();
    let mut lengths = Vec::new();
    for _ in 0..count {
        let bounds: [i32; 4] = std::array::from_fn(|i| be32(bytes, at + 4 * i) as u32 as i32);
        at += 16;
        let channels = usize::from(u16::from_be_bytes(bytes[at..at + 2].try_into().unwrap()));
        at += 2;
        let mut table = Vec::new();
        for c in 0..channels {
            let id = i16::from_be_bytes(bytes[at + 6 * c..at + 6 * c + 2].try_into().unwrap());
            table.push((id, be32(bytes, at + 6 * c + 2)));
        }
        at += channels * 6;
        let metadata_at = at;
        let flags_at = at + 10;
        at += 12;
        let extra = be32(bytes, at);
        at += 4;
        let end = at + extra;
        let mut c = at;
        let mask_len = be32(bytes, c);
        let mask = (mask_len >= 20).then(|| {
            let rect = std::array::from_fn(|i| be32(bytes, c + 4 + 4 * i) as u32 as i32);
            (rect, bytes[c + 20])
        });
        c += 4 + mask_len;
        c += 4 + be32(bytes, c); // blending ranges
        let name_len = usize::from(bytes[c]);
        let name = String::from_utf8_lossy(&bytes[c + 1..c + 1 + name_len]).into_owned();
        c += (name_len + 4) & !3;
        let mut blocks = Vec::new();
        while c < end {
            let key: [u8; 4] = bytes[c + 4..c + 8].try_into().unwrap();
            let len = be32(bytes, c + 8);
            blocks.push((key, bytes[c + 12..c + 12 + len].to_vec()));
            c += 12 + len;
        }
        records.push(Framed {
            bounds,
            name,
            flags: bytes[flags_at],
            flags_at,
            mask,
            blocks,
            channels: Vec::new(),
            metadata: bytes[metadata_at..end].to_vec(),
        });
        lengths.push(table);
        at = end;
    }
    for (record, table) in records.iter_mut().zip(lengths) {
        for (id, length) in table {
            record.channels.push((id, bytes[at..at + length].to_vec()));
            at += length;
        }
    }
    Layout {
        section_at,
        info_at,
        records,
    }
}

pub(crate) fn framed_records(bytes: &[u8]) -> Vec<Framed> {
    layout(bytes).records
}

/// `bytes` with the record named `name` rewritten as a pixel-less record —
/// no rectangle, each colour channel its bare compression word, its mask
/// channel and every block kept — and the section lengths re-framed.
pub(crate) fn pixel_less(bytes: &[u8], name: &str) -> Vec<u8> {
    let Layout {
        section_at,
        info_at,
        records,
    } = layout(bytes);
    let section_end = section_at + 4 + be32(bytes, section_at);
    let info_end = info_at + 4 + be32(bytes, info_at);
    let mut info = (records.len() as i16).to_be_bytes().to_vec();
    let mut data = Vec::new();
    for record in &records {
        let target = record.name == name;
        let bounds = if target { [0; 4] } else { record.bounds };
        for v in bounds {
            info.extend(v.to_be_bytes());
        }
        info.extend((record.channels.len() as u16).to_be_bytes());
        for (id, stored) in &record.channels {
            let stored = if target && *id != -2 {
                vec![0, 0]
            } else {
                stored.clone()
            };
            info.extend(id.to_be_bytes());
            info.extend((stored.len() as u32).to_be_bytes());
            data.extend(stored);
        }
        info.extend(&record.metadata);
    }
    info.extend(data);
    if info.len() % 2 == 1 {
        info.push(0);
    }
    let rest = &bytes[info_end..section_end];
    let mut out = bytes[..section_at].to_vec();
    out.extend(((4 + info.len() + rest.len()) as u32).to_be_bytes());
    out.extend((info.len() as u32).to_be_bytes());
    out.extend(info);
    out.extend(rest);
    out.extend(&bytes[section_end..]);
    out
}

/// One stored channel's samples, `w × h` of `sample` bytes each: Raw or
/// PackBits rows (the two our writer uses).
pub(crate) fn plane(stored: &[u8], w: usize, h: usize, sample: usize) -> Vec<u8> {
    let row = w * sample;
    match u16::from_be_bytes([stored[0], stored[1]]) {
        0 => stored[2..2 + row * h].to_vec(),
        1 => {
            let mut at = 2 + 2 * h;
            let mut out = Vec::new();
            for y in 0..h {
                let len = usize::from(u16::from_be_bytes([stored[2 + 2 * y], stored[3 + 2 * y]]));
                let unpacked = crate::cmyk_psd_fixture::unpack(&stored[at..at + len]);
                assert_eq!(unpacked.len(), row, "row {y}");
                out.extend(unpacked);
                at += len;
            }
            out
        }
        other => panic!("compression {other}"),
    }
}

/// Every number as f64, so `0` and `0.0` compare equal.
pub(crate) fn norm(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().unwrap()),
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), norm(v))).collect()),
        other => other.clone(),
    }
}

/// The masked fill's record rectangle (top, left, bottom, right): its mask's.
const MASKED_RECT: [i32; 4] = [
    MASK_AT.1,
    MASK_AT.0,
    MASK_AT.1 + MASK_SIZE.1 as i32,
    MASK_AT.0 + MASK_SIZE.0 as i32,
];
/// The unmasked fill's: the canvas.
const CANVAS_RECT: [i32; 4] = [0, 0, H as i32, W as i32];

/// The written `PtFl` record named `name` is Photoshop's form: its rectangle
/// `rect`, flags bit 4 beside bit 3, `PtFl`, an `fxrp` equal to the case's
/// reference, a `-2` channel exactly when `masked`, and colour and alpha
/// channels holding the case's fill over `rect` (the oracle, at `sample`
/// bytes a sample).
fn assert_fill_record(
    bytes: &[u8],
    name: &str,
    case: &Case,
    rect: [i32; 4],
    masked: bool,
    sample: usize,
    what: &str,
) {
    let records = framed_records(bytes);
    let record = records
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("{what}: the record {name}"));
    assert_eq!(record.bounds, rect, "{what}: {name}'s rectangle");
    assert_eq!(record.flags & 0x18, 0x18, "{what}: {name}'s bit 4");
    assert!(
        record.blocks.iter().any(|(key, _)| key == b"PtFl"),
        "{what}: {name} carries PtFl"
    );
    let fxrp = record
        .blocks
        .iter()
        .find(|(key, _)| key == b"fxrp")
        .map(|(_, body)| body.clone())
        .unwrap_or_else(|| panic!("{what}: {name} carries fxrp"));
    let x = f64::from_be_bytes(fxrp[0..8].try_into().unwrap());
    let y = f64::from_be_bytes(fxrp[8..16].try_into().unwrap());
    assert_eq!(
        (x, y),
        (f64::from(case.reference.0), f64::from(case.reference.1)),
        "{what}: {name}'s fxrp"
    );
    assert_eq!(
        record.channels.iter().any(|(id, _)| *id == -2),
        masked,
        "{what}: {name}'s mask channel"
    );
    if masked {
        assert_eq!(
            record.mask,
            Some((rect, 0)),
            "{what}: {name}'s rectangle is its mask's"
        );
    }
    let (w, h) = ((rect[3] - rect[1]) as usize, (rect[2] - rect[0]) as usize);
    let o = case.origin();
    for (id, component) in [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)] {
        let stored = &record
            .channels
            .iter()
            .find(|(c, _)| *c == id)
            .unwrap_or_else(|| panic!("{what}: {name}'s channel {id}"))
            .1;
        let samples = plane(stored, w, h, sample);
        for j in 0..h {
            for i in 0..w {
                let (cx, cy) = (rect[1] as u32 + i as u32, rect[0] as u32 + j as u32);
                let want = texel(o, cx, cy)[component];
                let at = (j * w + i) * sample;
                let got = if sample == 1 {
                    u16::from(samples[at])
                } else {
                    u16::from_be_bytes([samples[at], samples[at + 1]])
                };
                let want = if sample == 1 {
                    u16::from(want)
                } else {
                    u16::from(want) * 257
                };
                assert_eq!(got, want, "{what}: {name} channel {id} at ({cx},{cy})");
            }
        }
    }
}

/// The reopened description is the fill layer's, with `case`'s keys.
fn assert_description(description: &str, case: &Case, what: &str) {
    let d: Value = serde_json::from_str(description).expect("description JSON");
    assert_eq!(d["type"], "fill", "{what}");
    assert_eq!(d["version"], 1, "{what}");
    assert_eq!(d["fill"], "pattern", "{what}");
    assert_eq!(d["canvas"], json!([W, H]), "{what}");
    assert!(
        d["pattern_fill"].get("anchor").is_none(),
        "{what}: no anchor"
    );
    assert_eq!(
        norm(&d["pattern_fill"]),
        norm(&case.keys()),
        "{what}: pattern_fill"
    );
}

/// The reopened 8-bit document holds both fills as `doc` does: each its
/// description, reference, canvas raster at the origin and the oracle's
/// pixels; the masked one its opacity and mask; the same flattened image.
fn assert_reopened8(reopened: &DrDocument, doc: &DrDocument, what: &str) {
    assert_eq!(reopened.layers.len(), 3, "{what}");
    for (index, case) in [(1, LINKED), (2, UNLINKED)] {
        let layer = &reopened.layers[index];
        let what = format!("{what} layer {index}");
        assert_description(layer.meta.as_deref().expect("a description"), &case, &what);
        assert_eq!(layer.reference, case.reference, "{what}: reference");
        assert_eq!(layer.offset, (0, 0), "{what}: offset");
        assert_eq!(layer.pixels.dimensions(), (W, H), "{what}: canvas raster");
        let pixels = layer.pixels.to_contiguous().unwrap();
        let o = case.origin();
        for y in 0..H {
            for x in 0..W {
                assert_eq!(
                    pixels.get_pixel(x, y).0,
                    texel(o, x, y),
                    "{what} ({x},{y}) O {o:?}"
                );
            }
        }
    }
    assert!(
        (reopened.layers[1].opacity - 0.6).abs() < 1e-3,
        "{what}: opacity"
    );
    assert_eq!(
        reopened.layers[1]
            .mask
            .as_deref()
            .map(|m| m.as_raw().clone()),
        doc.layers[1].mask.as_deref().map(|m| m.as_raw().clone()),
        "{what}: the mask"
    );
    assert_eq!(
        reopened.layers[1].mask_properties.offset, MASK_AT,
        "{what}: the mask's place"
    );
    assert_eq!(
        reopened.flattened().unwrap(),
        doc.flattened().unwrap(),
        "{what}: flattened"
    );
}

// ------------------------------------------------------------ round trips --

#[test]
fn an_8_bit_pattern_fill_layer_round_trips_in_photoshops_ptfl_form() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc();
    let path = dir.path().join("fill.psd");
    save8(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    assert!(contains(&bytes, b"8BIMPatt"), "the pattern table");
    assert_fill_record(&bytes, "Fill 0", &LINKED, MASKED_RECT, true, 1, "psd8");
    assert_fill_record(&bytes, "Fill 1", &UNLINKED, CANVAS_RECT, false, 1, "psd8");
    let report = import_report(&path);
    let observed = report["observations"].to_string();
    assert!(
        observed
            .contains("(“Fill 0”) is a Photoshop Pattern Fill layer (PtFl) in Photoshop's form"),
        "{observed}"
    );
    assert!(
        !report["verified_losses"].to_string().contains("Fill"),
        "{}",
        report["verified_losses"]
    );
    assert_reopened8(&open8(&path), &doc, "psd8");
}

#[test]
fn a_16_bit_pattern_fill_layer_round_trips_with_its_tile_in_pat2() {
    let dir = TempDir::new().unwrap();
    let doc = typed_doc();
    let path = dir.path().join("fill16.psd");
    save16(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    assert!(contains(&bytes, b"8BIMPat2"), "the 16-bit pattern table");
    assert_fill_record(&bytes, "Fill 0", &LINKED, MASKED_RECT, true, 2, "psd16");
    assert_fill_record(&bytes, "Fill 1", &UNLINKED, CANVAS_RECT, false, 2, "psd16");
    let report = import_report(&path);
    assert!(
        !report["verified_losses"].to_string().contains("Fill"),
        "{}",
        report["verified_losses"]
    );
    let reopened = open16(&path);
    assert_eq!(reopened.layer_count(), 3);
    for (index, case) in [(1, LINKED), (2, UNLINKED)] {
        let what = format!("psd16 layer {index}");
        assert_description(
            reopened.artwork_description(index).expect("a description"),
            &case,
            &what,
        );
        let layer = reopened.layer(index).unwrap();
        assert_eq!(layer.reference(), case.reference, "{what}: reference");
        assert_eq!(layer.offset(), (0, 0), "{what}: offset");
        let samples = layer.source().unwrap().to_rgba16().unwrap();
        let o = case.origin();
        for y in 0..H {
            for x in 0..W {
                let want = texel(o, x, y).map(|c| u16::from(c) * 257);
                assert_eq!(samples[(y * W + x) as usize], want, "{what} ({x},{y})");
            }
        }
    }
    let masked = reopened.layer(1).unwrap();
    assert!((masked.opacity() - 0.6).abs() < 1e-3, "opacity");
    let mask = masked.mask().expect("the mask");
    assert_eq!(mask.properties().offset, MASK_AT, "the mask's place");
    assert_eq!(
        mask.plane().samples().to_vec(),
        doc.layer(1)
            .unwrap()
            .mask()
            .map(|m| m.plane().samples().to_vec())
            .unwrap(),
        "the mask"
    );
    assert_eq!(
        reopened
            .render(LIMIT, 0, None)
            .unwrap()
            .to_rgba16()
            .unwrap(),
        doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
    );
}

// --------------------------------------------------------- crafted files --

#[test]
fn every_ptfl_form_is_read_and_named_in_the_report() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc();
    let path = dir.path().join("fill.psd");
    save8(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    // Without bit 4: both fills' flags 0x18 → 0x08.
    let mut unflagged = bytes.clone();
    for record in framed_records(&bytes) {
        if record.name.starts_with("Fill") {
            assert_eq!(record.flags, 0x18);
            unflagged[record.flags_at] = 0x08;
        }
    }
    let unflagged_path = dir.path().join("unflagged.psd");
    std::fs::write(&unflagged_path, &unflagged).unwrap();
    // Pixel-less: the masked fill with no rectangle and no colour channels.
    let less = pixel_less(&bytes, "Fill 0");
    let less_record = framed_records(&less)
        .into_iter()
        .find(|r| r.name == "Fill 0")
        .unwrap();
    assert_eq!(less_record.bounds, [0; 4], "the crafted record");
    let less_path = dir.path().join("pixel-less.psd");
    std::fs::write(&less_path, &less).unwrap();
    for (path, name, clause) in [
        (
            &unflagged_path,
            "Fill 0",
            "with pixels it does not mark irrelevant (no flags bit 4)",
        ),
        (&less_path, "Fill 0", "with no pixels;"),
        (&less_path, "Fill 1", "in Photoshop's form"),
    ] {
        let report = import_report(path);
        let observed = report["observations"].to_string();
        assert!(
            observed.contains(&format!(
                "(“{name}”) is a Photoshop Pattern Fill layer (PtFl) {clause}"
            )),
            "{name} {clause}: {observed}"
        );
        assert!(
            !report["verified_losses"].to_string().contains("Fill"),
            "{}",
            report["verified_losses"]
        );
        assert_reopened8(&open8(path), &doc, clause);
    }
}

#[test]
fn a_carried_fill_is_admitted_as_a_canvas_raster_not_its_rectangle() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("fill16.psd");
    save16(&typed_doc(), &path);
    let bytes = std::fs::read(&path).unwrap();
    // The masked fill's key renamed, so the same record is a plain raster of
    // its 60 × 44 rectangle.
    let mut raster = bytes.clone();
    let at = raster
        .windows(8)
        .position(|w| w == b"8BIMPtFl")
        .expect("the first PtFl");
    raster[at + 4..at + 8].copy_from_slice(b"XtFl");
    let raster_path = dir.path().join("raster16.psd");
    std::fs::write(&raster_path, &raster).unwrap();
    // The least working limit each typed open admits (both admit `LIMIT`).
    let least = |path: &Path| {
        let token = import_report(path)["source_fingerprint"]
            .as_str()
            .unwrap()
            .to_string();
        let (mut lo, mut hi) = (0u64, LIMIT);
        assert!(try_open16(path, &token, hi).is_ok());
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            if try_open16(path, &token, mid).is_ok() {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        hi
    };
    let (fill, plain) = (least(&path), least(&raster_path));
    // The canvas raster retains 8 bytes for every canvas pixel the
    // rectangle does not cover.
    let uncovered = u64::from(W * H - MASK_SIZE.0 * MASK_SIZE.1);
    assert!(
        fill >= plain + uncovered * 8,
        "fill {fill}, raster {plain}, uncovered {uncovered}"
    );
}

#[test]
fn a_ptfl_whose_tile_is_missing_stays_a_raster_or_is_omitted_with_its_reason() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("fill.psd");
    save8(&byte_doc(), &path);
    // Rename the `Patt` tail block, so the file carries no pattern table.
    let lose_table = |bytes: &mut Vec<u8>| {
        let at = bytes
            .windows(8)
            .position(|w| w == b"8BIMPatt")
            .expect("the table");
        bytes[at + 4..at + 8].copy_from_slice(b"Xatt");
    };
    // Photoshop's form: each record stays a raster of its own pixels, with
    // the marker losses.
    let mut bytes = std::fs::read(&path).unwrap();
    lose_table(&mut bytes);
    let crafted = dir.path().join("missing.psd");
    std::fs::write(&crafted, &bytes).unwrap();
    let losses = import_report(&crafted)["verified_losses"].to_string();
    for fragment in [
        "(“Fill 0”) has adjustment/fill metadata",
        "(“Fill 0”) declares its raster pixels irrelevant",
    ] {
        assert!(losses.contains(fragment), "{fragment}: {losses}");
    }
    let reopened = open8(&crafted);
    assert_eq!(reopened.layers.len(), 3, "both fills stay rasters");
    let masked = &reopened.layers[1];
    assert!(masked.meta.is_none(), "no description");
    assert_eq!(masked.offset, MASK_AT, "its own rectangle");
    assert_eq!(masked.pixels.dimensions(), MASK_SIZE);
    // Pixel-less: omitted with its reason.
    let mut bytes = pixel_less(&std::fs::read(&path).unwrap(), "Fill 0");
    lose_table(&mut bytes);
    let crafted = dir.path().join("missing-less.psd");
    std::fs::write(&crafted, &bytes).unwrap();
    let losses = import_report(&crafted)["verified_losses"].to_string();
    assert!(
        losses.contains("(“Fill 0”) is a Photoshop Pattern Fill layer (PtFl) whose pattern fill names a pattern the file does not carry; layered import omits it and its mask."),
        "{losses}"
    );
    let reopened = open8(&crafted);
    assert_eq!(reopened.layers.len(), 2, "the pixel-less fill is omitted");
}

// ------------------------------------------------- Photoshop's own files --

/// The take's chosen files (`fixtures/psd_photoshop/pattern_fill_layer`,
/// `scripts/photoshop-probe/pattern_fill_layer_take.py`): `photoshop.json`
/// names each case, what the take read from its `PtFl` and its PSD and TIFF
/// with their sha256.
fn photoshop_manifest() -> Value {
    let path = crate::common::psd_photoshop::fixture("pattern_fill_layer", "photoshop.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).expect("photoshop.json")
}

/// The largest channel difference, in 8-bit codes, between two RGB(A)8
/// rasters of the same size.
fn worst_code(ours: &[u8], theirs: &[u8]) -> i32 {
    assert_eq!(ours.len(), theirs.len());
    ours.chunks(4)
        .zip(theirs.chunks(4))
        .flat_map(|(a, b)| (0..3).map(move |c| (i32::from(a[c]) - i32::from(b[c])).abs()))
        .max()
        .unwrap()
}

#[test]
fn photoshops_own_pattern_fill_layers_read_as_their_fill_within_the_bound() {
    let manifest = photoshop_manifest();
    let bound = manifest["bound"].as_f64().unwrap() as i32;
    assert_eq!(manifest["link"], "Algn", "the take's LINK");
    assert_eq!(manifest["ft"], "KEEP", "the take's FT");
    let files = manifest["files"].as_object().unwrap();
    assert_eq!(files.len(), 9, "the take's nine fixture files");
    for (key, entry) in files {
        let case = &entry["case"];
        let path = |kind: &str| {
            crate::common::psd_photoshop::fixture(
                "pattern_fill_layer",
                entry[kind]["file"].as_str().unwrap(),
            )
        };
        for kind in ["psd", "tiff"] {
            assert_eq!(
                crate::common::sha256::sha256_hex(&std::fs::read(path(kind)).unwrap()),
                entry[kind]["sha256"].as_str().unwrap(),
                "{key}: the take's {kind}"
            );
        }
        let psd = path("psd");
        let report = import_report(&psd);
        // Photoshop writes its 8-bit fill with its own pixels (bit 4) and
        // its 16-bit one pixel-less; both are read, and named.
        let form = if entry["ptfl"]["bounds"] == json!([0, 0, 0, 0]) {
            "with no pixels"
        } else {
            "in Photoshop's form"
        };
        let observed = report["observations"].to_string();
        assert!(
            observed.contains(&format!(
                "(“PF”) is a Photoshop Pattern Fill layer (PtFl) {form}"
            )),
            "{key}: {observed}"
        );
        assert!(
            !report["verified_losses"].to_string().contains("PF"),
            "{key}: {}",
            report["verified_losses"]
        );
        let reference: Vec<i32> = entry["ptfl"]["fxrp"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap().round() as i32)
            .collect();
        let want = json!({
            "pattern": {"width": 12, "height": 8, "rgba": base64(&tile_t())},
            "scale": case["scale"].as_f64().unwrap() / 100.0,
            "angle": case["angle"].as_f64().unwrap(),
            "link_with_layer": case["link"].as_bool().unwrap(),
            "phase": case["phase"],
        });
        let check = |description: &str, what: &str| {
            let d: Value = serde_json::from_str(description).expect("description JSON");
            assert_eq!(d["fill"], "pattern", "{what}");
            let fill = &d["pattern_fill"];
            for k in ["scale", "angle", "link_with_layer", "phase"] {
                assert_eq!(norm(&fill[k]), norm(&want[k]), "{what}: {k}");
            }
            for k in ["width", "height", "rgba"] {
                assert_eq!(
                    fill["pattern"][k], want["pattern"][k],
                    "{what}: the tile T's {k}"
                );
            }
        };
        let theirs = DrDocument::open(path("tiff").to_str().unwrap())
            .unwrap()
            .flattened()
            .unwrap();
        if case["depth"] == 16 {
            let doc = open16(&psd);
            let index = (0..doc.layer_count())
                .find(|&i| doc.layer(i).unwrap().name() == "PF")
                .expect("the fill");
            check(doc.artwork_description(index).expect("a description"), key);
            let layer = doc.layer(index).unwrap();
            assert_eq!(
                layer.reference(),
                (reference[0], reference[1]),
                "{key}: fxrp"
            );
            assert_eq!(layer.offset(), (0, 0), "{key}: at the origin");
            let ours: Vec<u8> = doc
                .render(LIMIT, 0, None)
                .unwrap()
                .to_rgba16()
                .unwrap()
                .iter()
                .flat_map(|p| p.map(|v| ((u32::from(v) + 128) / 257) as u8))
                .collect();
            let worst = worst_code(&ours, theirs.as_raw());
            assert!(worst <= bound, "{key}: {worst} codes from Photoshop's TIFF");
            continue;
        }
        let doc = open8(&psd);
        let index = doc
            .layers
            .iter()
            .position(|l| l.name == "PF")
            .expect("the fill");
        let layer = &doc.layers[index];
        check(layer.meta.as_deref().expect("a description"), key);
        assert_eq!(layer.reference, (reference[0], reference[1]), "{key}: fxrp");
        assert_eq!(layer.offset, (0, 0), "{key}: at the origin");
        assert_eq!(layer.pixels.dimensions(), (W, H), "{key}: a canvas raster");
        if key == "PM" {
            // The anti-aliased diamond's `-2` mask is kept as the fill's.
            let mask = layer.mask.as_deref().expect("PM's mask");
            assert!(
                mask.as_raw().iter().any(|&v| v > 0 && v < 255),
                "PM: the mask keeps its anti-aliased edge"
            );
        }
        let worst = worst_code(doc.flattened().unwrap().as_raw(), theirs.as_raw());
        assert!(worst <= bound, "{key}: {worst} codes from Photoshop's TIFF");
    }
}
