//! A 32-bit float master (`SourcePixels::Rgba32F`, a placed
//! floating-point file with no contents) in an 8- or 16-bit document,
//! through the source-edit FFI. Photoshop 26.10.0 keeps 32-bit contents
//! there and renders them clipped: converted into the document profile in
//! linear light, clamped to [0, 1], encoded by the document's curve
//! (receipts `float-low-depth-600/take1`: P8 / P16 RENDER `clip`, PP3 GAMUT
//! `matrix_clip`, EC8 / EC16, NR16 / NR8, PS8, SR16; `take2`: SC16 DOMAIN
//! `H-enc`).
//!
//! Oracles, none read off the implementation: f64 through
//! `common::icc_chain` (the profiles' own colorant tags and `curv` tables),
//! the 16-bit code `round(enc · 65535)` and the byte `round(enc · 255)`,
//! each within 1 code; alpha `round(a · 65535)` / `round(a · 255)`. Every
//! refusal sentence is written out here.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::dr_doc_save_native;
use doctored_core::ffi_layer_clipboard::dr_precision_doc_paste_layers;
use doctored_core::ffi_retained_source::{
    dr_doc_mixed_source_edit, dr_doc_source_info, dr_precision_doc_mixed_source_edit,
    dr_precision_doc_source_edit, dr_precision_doc_source_info,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_quantize::PrecisionDither;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common;
use crate::float_fixture::float_doc;
use crate::live_filter_support::Doc;
use common::icc_chain::{apply, inverse, Chain};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const DEPTH_AUTHORIZATION: &str =
    "source and document depths differ; explicit convert_fallback authorization is required";
const DEPTH_FLAGS: &str = "unsupported embedded source depth or flags";
const MASTER_WORD: &str = "invalid native float master word";
const UNIFORM: [f32; 4] = [2.0, 0.5, -0.25, 1.0];
const E_LEFT: [f32; 4] = [0.05, 0.6, 0.9, 1.0];
const E_RIGHT: [f32; 4] = [0.8, 0.1, 0.3, 1.0];

// ------------------------------------------------------------- plumbing --

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

/// `dr_precision_doc_mixed_source_edit`: `Ok(None)` for a no-op.
fn mixed16(
    doc: &PrecisionDocument,
    source8: Option<&DrDocument>,
    source16: Option<&PrecisionDocument>,
    request: Value,
) -> Result<Option<PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_mixed_source_edit(
            doc,
            source8.map_or(ptr::null(), |s| s),
            source16.map_or(ptr::null(), |s| s),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    match owned(err) {
        Some(message) => Err(message),
        None => Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) })),
    }
}

/// `dr_doc_mixed_source_edit`: `Ok(None)` for a no-op.
fn mixed8(
    doc: &DrDocument,
    source8: Option<&DrDocument>,
    source16: Option<&PrecisionDocument>,
    request: Value,
) -> Result<Option<DrDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_mixed_source_edit(
            doc,
            source8.map_or(ptr::null(), |s| s),
            source16.map_or(ptr::null(), |s| s),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    match owned(err) {
        Some(message) => Err(message),
        None => Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) })),
    }
}

fn must16(
    doc: &PrecisionDocument,
    source8: Option<&DrDocument>,
    source16: Option<&PrecisionDocument>,
    request: Value,
) -> PrecisionDocument {
    mixed16(doc, source8, source16, request)
        .unwrap()
        .expect("the edit changes the document")
}

fn must8(
    doc: &DrDocument,
    source8: Option<&DrDocument>,
    source16: Option<&PrecisionDocument>,
    request: Value,
) -> DrDocument {
    mixed8(doc, source8, source16, request)
        .unwrap()
        .expect("the edit changes the document")
}

/// `dr_precision_doc_source_edit` with no input, which must change the
/// document.
fn edit16(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_source_edit(
            doc,
            ptr::null(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if let Some(message) = owned(err) {
        panic!("{message}");
    }
    assert!(!out.is_null(), "the edit changes the document");
    *unsafe { Box::from_raw(out) }
}

fn info16(doc: &PrecisionDocument, index: usize) -> Value {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_source_info(doc, index, &mut err) };
    assert!(owned(err).is_none());
    serde_json::from_str(&owned(out).expect("source info")).unwrap()
}

fn info8(doc: &DrDocument, index: usize) -> Value {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_source_info(doc, index, &mut err) };
    assert!(owned(err).is_none());
    serde_json::from_str(&owned(out).expect("source info")).unwrap()
}

fn place(id: &str, conversion: &str) -> Value {
    json!({"op":"place","source_id":id,"name":id,"x":1,"y":2,"depth_conversion":conversion})
}

fn convert(id: &str) -> Value {
    place(id, "convert_fallback")
}

fn transform(matrix: [f64; 6], filter: &str) -> Value {
    json!({"op":"transform","layer":1,"matrix":matrix,"filter":filter})
}

/// The f32 master: a one-record float document holding `pixels` exactly.
fn master(w: u32, h: u32, profile: &Arc<IccProfile>, pixels: Vec<[f32; 4]>) -> PrecisionDocument {
    float_doc(w, h, profile, &[pixels])
}

/// Above white, below 0 and partly covered, among ordinary values.
fn extended(w: u32, h: u32) -> Vec<[f32; 4]> {
    (0..w * h)
        .map(|i| match i % 4 {
            0 => [2.0, -0.25, 0.5, 0.5],
            1 => [0.125, 3.5, -0.0625, 1.0],
            2 => [0.2, 0.75, 1.25, 0.25],
            _ => [i as f32 / 64.0, 0.375, 0.875, 1.0],
        })
        .collect()
}

/// A 16-bit sRGB document of one opaque record.
fn typed(w: u32, h: u32) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            w,
            h,
            vec![[9000, 30000, 50000, 65535]; (w * h) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

fn byte(w: u32, h: u32) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_pixel(w, h, Rgba([9, 120, 200, 255])))
}

fn byte_master(w: u32, h: u32) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, y| {
        Rgba([(x * 41 + y * 7) as u8, 200, (x * y * 23 + 9) as u8, 255])
    }))
}

fn codes(doc: &PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    doc.layer_source(index).unwrap().to_rgba16().unwrap()
}

fn bytes8(doc: &DrDocument, index: usize) -> Vec<[u8; 4]> {
    doc.layers[index]
        .pixels
        .to_contiguous()
        .unwrap()
        .pixels()
        .map(|p| p.0)
        .collect()
}

/// The take's clip rule at `scale` (65535 or 255): `p`, already linear in
/// the document profile, clamped and encoded by `chain`'s curve.
fn clip(chain: &Chain, p: [f64; 4], scale: f64) -> [f64; 4] {
    let rgb: [f64; 3] = std::array::from_fn(|c| chain.encode(p[c].clamp(0.0, 1.0)) * scale);
    [rgb[0], rgb[1], rgb[2], p[3] * scale]
}

fn widened(p: [f32; 4]) -> [f64; 4] {
    p.map(f64::from)
}

fn assert_codes<T: Copy + Into<f64>>(got: &[[T; 4]], want: &[[f64; 4]], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: length");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        for c in 0..4 {
            let g: f64 = g[c].into();
            assert!(
                (g - w[c]).abs() <= 1.0,
                "{what}: pixel {i} channel {c}: {g} vs {}",
                w[c]
            );
        }
    }
}

fn srgb_chain() -> Chain {
    Chain::of(&IccProfile::srgb())
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn spliced(bytes: &[u8], at: usize, value: u32) -> Vec<u8> {
    let mut out = bytes[..at].to_vec();
    out.extend(value.to_le_bytes());
    out.extend(&bytes[at..]);
    out
}

/// Saves `doc` through `dr_doc_save_native` and returns the file's bytes.
fn save8(doc: &DrDocument, dir: &tempfile::TempDir, name: &str) -> Vec<u8> {
    let path = dir.path().join(name);
    let text = CString::new(path.to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let ok = unsafe { dr_doc_save_native(doc, text.as_ptr(), std::ptr::null(), &mut err) };
    if let Some(message) = owned(err) {
        panic!("save {name}: {message}");
    }
    assert!(ok);
    std::fs::read(&path).unwrap()
}

fn reopen8(bytes: &[u8]) -> Result<DrDocument, String> {
    let dir = tempfile::TempDir::new().unwrap();
    // The file open prefixes the reader's sentence with the path.
    let opened = Doc::open(bytes, false, &dir.path().join("reopen.rz"))
        .map_err(|error| error.rsplit(": ").next().unwrap_or_default().to_string())?;
    match opened {
        Doc::Byte(doc) => Ok(doc),
        Doc::Typed(_) => unreachable!("a byte open"),
    }
}

fn reopen16(bytes: &[u8]) -> Result<PrecisionDocument, String> {
    PrecisionDocument::parse_native(bytes, LIMIT, 0, None)
}

// --------------------------------------------------- place, transform --

#[test]
fn a_float_master_places_at_16_and_8_bits_as_the_clip_oracle() {
    let chain = srgb_chain();
    let pixels = extended(6, 5);
    let source = master(6, 5, &IccProfile::srgb(), pixels.clone());
    let want16: Vec<[f64; 4]> = pixels
        .iter()
        .map(|p| clip(&chain, widened(*p), 65535.0))
        .collect();
    let want8: Vec<[f64; 4]> = pixels
        .iter()
        .map(|p| clip(&chain, widened(*p), 255.0))
        .collect();

    let sixteen = must16(&typed(12, 10), None, Some(&source), convert("f"));
    assert_codes(&codes(&sixteen, 1), &want16, "16-bit place");
    assert_eq!(sixteen.layer(1).unwrap().offset(), (1, 2));
    let report = info16(&sixteen, 1);
    assert_eq!(report["valid"], true);
    assert_eq!(report["master_depth"], 32);
    assert_eq!(report["storage"], "rgba32_float");
    assert_eq!(report["depth_conversion"], "convert_fallback");

    let eight = must8(&byte(12, 10), None, Some(&source), convert("f"));
    assert_codes(&bytes8(&eight, 1), &want8, "8-bit place");
    let report = info8(&eight, 1);
    assert_eq!(report["valid"], true);
    assert_eq!(report["master_depth"], 32);
    assert_eq!(report["storage"], "rgba32_float");
    assert_eq!(report["depth_conversion"], "convert_fallback");

    // Without the authorization both refuse with the depth sentence.
    assert_eq!(
        mixed16(&typed(12, 10), None, Some(&source), place("f", "reject")).err(),
        Some(DEPTH_AUTHORIZATION.to_string())
    );
    assert_eq!(
        mixed8(&byte(12, 10), None, Some(&source), place("f", "reject")).err(),
        Some(DEPTH_AUTHORIZATION.to_string())
    );
}

/// take1 PP3 read `matrix_clip`: the unclamped linear values cross the
/// matrix, then clip.
#[test]
fn a_display_p3_master_converts_before_it_clips() {
    let p3 = IccProfile::display_p3();
    let (s, p) = (srgb_chain(), Chain::of(&p3));
    let m: [[f64; 3]; 3] = {
        let inv = inverse(s.to_pcs);
        std::array::from_fn(|r| {
            std::array::from_fn(|c| (0..3).map(|k| inv[r][k] * p.to_pcs[k][c]).sum())
        })
    };
    let mut pixels = extended(5, 4);
    pixels[0] = [2.0, 0.5, 0.5, 1.0];
    let source = master(5, 4, &p3, pixels.clone());
    let want: Vec<[f64; 4]> = pixels
        .iter()
        .map(|v| {
            let rgb = apply(m, [v[0], v[1], v[2]].map(f64::from));
            clip(&s, [rgb[0], rgb[1], rgb[2], f64::from(v[3])], 65535.0)
        })
        .collect();
    let doc = must16(&typed(8, 8), None, Some(&source), convert("p3"));
    assert_codes(&codes(&doc, 1), &want, "P3 master");
    // The clip-first candidate differs on take1's own value.
    let clipped = apply(m, [1.0, 0.5, 0.5]);
    let first = clip(&s, [clipped[0], clipped[1], clipped[2], 1.0], 65535.0);
    assert!((first[1] - want[0][1]).abs() > 100.0);
}

/// take2 SC16 read `H-enc`: a 16-bit document resamples the encoded render.
#[test]
fn a_bilinear_scale_at_16_bits_mixes_encoded_values() {
    let chain = srgb_chain();
    let pixels: Vec<[f32; 4]> = (0..8 * 2)
        .map(|i| if i % 8 < 4 { E_LEFT } else { E_RIGHT })
        .collect();
    let source = master(8, 2, &IccProfile::srgb(), pixels);
    let placed = must16(&typed(32, 8), None, Some(&source), convert("e"));
    let scaled = edit16(&placed, transform([3., 0., 0., 1., 0., 0.], "bilinear"));
    let image = scaled.layer_source(1).unwrap();
    assert_eq!(image.dimensions(), (24, 2));
    let rows = image.to_rgba16().unwrap();
    let a = clip(&chain, widened(E_LEFT), 65535.0);
    let b = clip(&chain, widened(E_RIGHT), 65535.0);
    let mut mixed = 0;
    for v in &rows[..24] {
        let w: [f64; 3] = std::array::from_fn(|c| (f64::from(v[c]) - a[c]) / (b[c] - a[c]));
        let mean = (w[0] + w[1] + w[2]) / 3.0;
        if !(0.05..0.95).contains(&mean) {
            continue;
        }
        mixed += 1;
        let spread =
            w.iter().cloned().fold(f64::MIN, f64::max) - w.iter().cloned().fold(f64::MAX, f64::min);
        assert!(spread <= 1e-3, "encoded weights {w:?}");
    }
    assert!(mixed >= 2, "{mixed} mixed samples");
}

#[test]
fn replace_moves_between_an_eight_bit_and_a_float_master_at_8_bits() {
    let chain = srgb_chain();
    let doc = must8(
        &byte(16, 16),
        Some(&byte_master(4, 4)),
        None,
        place("a", "reject"),
    );
    assert_eq!(info8(&doc, 1)["master_depth"], 8);
    let source = master(4, 4, &IccProfile::srgb(), extended(4, 4));
    let replace = |revision: &str, conversion: &str| json!({"op":"replace","layer":1,"revision_id":revision,"depth_conversion":conversion});
    assert_eq!(
        mixed8(&doc, None, Some(&source), replace("r2", "reject")).err(),
        Some(DEPTH_AUTHORIZATION.to_string())
    );
    let floated = must8(&doc, None, Some(&source), replace("r2", "convert_fallback"));
    assert_eq!(info8(&floated, 1)["master_depth"], 32);
    let want: Vec<[f64; 4]> = extended(4, 4)
        .iter()
        .map(|p| clip(&chain, widened(*p), 255.0))
        .collect();
    assert_codes(&bytes8(&floated, 1), &want, "replaced by f32");
    let back = must8(
        &floated,
        Some(&byte_master(4, 4)),
        None,
        replace("r3", "reject"),
    );
    let report = info8(&back, 1);
    assert_eq!(
        (
            report["master_depth"].clone(),
            report["depth_conversion"].clone()
        ),
        (json!(8), json!("none"))
    );
    assert!(back.layers[1]
        .pixels
        .equals_image(&byte_master(4, 4).flattened().unwrap()));
}

// ------------------------------------------------------ save and reopen --

#[test]
fn a_sixteen_bit_file_keeps_its_float_master_under_word_67() {
    let source = master(5, 4, &IccProfile::display_p3(), extended(5, 4));
    let doc = must16(&typed(16, 12), None, Some(&source), convert("f"));
    let doc = edit16(&doc, transform([1.5, 0., 0., 1.25, 0., 0.], "bicubic"));
    let bytes = doc.encode_native(LIMIT, 0, None).unwrap();
    assert_eq!((word(&bytes, 4), word(&bytes, 8)), (46, 67));
    let back = reopen16(&bytes).unwrap();
    assert!(!back.is_float());
    assert_eq!(info16(&back, 1), info16(&doc, 1));
    assert_eq!(info16(&back, 1)["valid"], true);
    assert_eq!(codes(&back, 1), codes(&doc, 1));
    assert_eq!(back.encode_native(LIMIT, 0, None).unwrap(), bytes);
}

#[test]
fn an_eight_bit_file_keeps_its_float_master_under_word_72() {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = must8(
        &byte(16, 12),
        Some(&byte_master(4, 4)),
        None,
        place("a", "reject"),
    );
    let plain = save8(&doc, &dir, "plain.rz");
    assert_eq!(word(&plain, 4), 45);
    assert_ne!(word(&plain, 8), 72, "no float master writes no 72");
    let source = master(5, 4, &IccProfile::srgb(), extended(5, 4));
    let doc = must8(&doc, None, Some(&source), convert("f"));
    let bytes = save8(&doc, &dir, "a.rz");
    assert_eq!((word(&bytes, 4), word(&bytes, 8)), (45, 72));
    let back = reopen8(&bytes).unwrap();
    for i in 1..3 {
        assert_eq!(info8(&back, i), info8(&doc, i), "layer {i}");
        assert_eq!(info8(&back, i)["valid"], true, "layer {i}");
        assert_eq!(bytes8(&back, i), bytes8(&doc, i), "layer {i}");
    }
    assert_eq!(info8(&back, 2)["master_depth"], 32);
    assert_eq!(save8(&back, &dir, "b.rz"), bytes, "reopen → resave");

    // Without word 72 the depth-32 asset refuses; 72 with none refuses.
    let mut without = bytes[..8].to_vec();
    without.extend(&bytes[12..]);
    assert_eq!(reopen8(&without).err(), Some(DEPTH_FLAGS.to_string()));
    assert_eq!(
        reopen8(&spliced(&plain, 8, 72)).err(),
        Some(MASTER_WORD.to_string())
    );
}

// ---------------------------------------------- narrowing and promotion --

/// A float document holding a uniform f32 master at record 1.
fn float_with_master() -> PrecisionDocument {
    let srgb = IccProfile::srgb();
    let base = float_doc(12, 12, &srgb, &[vec![[0.25, 0.5, 0.75, 1.0]; 144]]);
    must16(
        &base,
        None,
        Some(&master(4, 4, &srgb, vec![UNIFORM; 16])),
        place("u", "reject"),
    )
}

#[test]
fn narrowing_carries_a_float_master_and_re_renders_it_clipped() {
    let chain = srgb_chain();
    let doc = float_with_master();
    let scale = transform([2., 0., 0., 2., 0., 0.], "nearest");

    let sixteen = doc.narrow_structure_to_u16(LIMIT, 0, None).unwrap();
    let report = info16(&sixteen, 1);
    assert_eq!(report["valid"], true);
    assert_eq!(report["master_depth"], 32);
    assert_eq!(report["depth_conversion"], "convert_fallback");
    let scaled = edit16(&sixteen, scale.clone());
    let want16 = clip(&chain, widened(UNIFORM), 65535.0);
    assert_codes(&codes(&scaled, 1), &vec![want16; 64], "narrowed to 16");

    let eight = doc
        .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, None)
        .unwrap();
    assert_eq!(info8(&eight, 1)["valid"], true);
    assert_eq!(info8(&eight, 1)["master_depth"], 32);
    let scaled = must8(&eight, None, None, scale);
    let want8 = clip(&chain, widened(UNIFORM), 255.0);
    assert_codes(&bytes8(&scaled, 1), &vec![want8; 64], "narrowed to 8");
}

#[test]
fn a_narrowed_float_clip_pastes_its_float_master_into_a_16_bit_document() {
    let clip = float_with_master()
        .narrow_structure_to_u16(LIMIT, 0, None)
        .unwrap();
    let dest = typed(12, 12);
    let (mut report, mut err) = (ptr::null_mut(), ptr::null_mut());
    let out = unsafe {
        dr_precision_doc_paste_layers(
            &dest,
            &clip,
            0,
            0,
            0,
            0,
            false,
            LIMIT,
            0,
            &mut report,
            &mut err,
        )
    };
    if let Some(message) = owned(err) {
        panic!("paste: {message}");
    }
    owned(report);
    let out = *unsafe { Box::from_raw(out) };
    let report = info16(&out, 2);
    assert_eq!(report["valid"], true);
    assert_eq!(report["master_depth"], 32);
}

/// A 16-bit document whose profile has no matrix/TRC model (`A2B0`/`B2A0`
/// tables only) refuses a float master with a sentence about the master,
/// not about a float document it does not have.
#[test]
fn a_lut_profiled_16_bit_document_refuses_a_float_master() {
    use crate::icc_lut_builders::{build, tag_desc, tag_mft2, tag_wtpt};
    let bytes = build(
        [0x02, 0x20, 0, 0],
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("Synthetic LUT RGB")),
            (b"wtpt", tag_wtpt([0.9642, 1.0, 0.8249])),
            (b"A2B0", tag_mft2(3, 3, 2, &|c| c.to_vec())),
            (b"B2A0", tag_mft2(3, 3, 2, &|c| c.to_vec())),
        ],
    );
    let lut = Arc::new(IccProfile::parse(&bytes).unwrap());
    assert!(lut.model().is_none());
    let doc = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(4, 4, vec![[9000, 30000, 50000, 65535]; 16], lut, None)
            .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let source = master(2, 2, &IccProfile::srgb(), vec![UNIFORM; 4]);
    assert_eq!(
        mixed16(&doc, None, Some(&source), convert("f")).err(),
        Some(
            "“Synthetic LUT RGB” has no matrix/TRC model this build can decode, so it cannot \
             render a 32-bit float master"
                .to_string()
        )
    );
}

#[test]
fn an_eight_bit_float_master_promotes_to_16_bits_and_back() {
    let chain = srgb_chain();
    let source = master(4, 4, &IccProfile::srgb(), vec![UNIFORM; 16]);
    let scale = transform([2., 0., 0., 2., 0., 0.], "nearest");

    let eight = must8(&byte(12, 12), None, Some(&source), convert("u"));
    let promoted = eight.promote_structure(LIMIT, 0, None).unwrap();
    assert_eq!(info16(&promoted, 1)["valid"], true);
    assert_eq!(info16(&promoted, 1)["master_depth"], 32);
    let scaled = edit16(&promoted, scale.clone());
    let want16 = clip(&chain, widened(UNIFORM), 65535.0);
    assert_codes(&codes(&scaled, 1), &vec![want16; 64], "promoted to 16");

    let sixteen = must16(&typed(12, 12), None, Some(&source), convert("u"));
    let narrowed = sixteen
        .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, None)
        .unwrap();
    assert_eq!(info8(&narrowed, 1)["valid"], true);
    assert_eq!(info8(&narrowed, 1)["master_depth"], 32);
    let scaled = must8(&narrowed, None, None, scale);
    let want8 = clip(&chain, widened(UNIFORM), 255.0);
    assert_codes(&bytes8(&scaled, 1), &vec![want8; 64], "narrowed to 8");
}
