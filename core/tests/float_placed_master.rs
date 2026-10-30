//! A 32-bit float file as a placed image's master in a 32-bit
//! float document (`source_asset::SourcePixels::Rgba32F`,
//! `retained_source_float`, `float_native`'s word 67), through the
//! source-edit FFI. Photoshop 26.10.0 keeps 32-bit contents for a placed
//! 32-bit TIFF and reads its samples back exact (receipt
//! `float-placed-473/take1`, PL32).
//!
//! Oracles, none read off the implementation: a master in the document's
//! own profile lands bit for bit; one in another profile is the f64 matrix
//! `inv(dst.to_pcs) · src.to_pcs` over the profiles' own colorant tags
//! (`common::icc_chain`; the samples are already linear, so no curve), never
//! clamped, within 1e-6 · max(1, |want|), alpha kept; the resample domain is
//! the per-channel weight of a two-colour edge; every refusal sentence is
//! written out here.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_layer_clipboard::dr_precision_doc_paste_layers;
use doctored_core::ffi_retained_source::{
    dr_doc_mixed_source_edit, dr_precision_doc_mixed_source_edit, dr_precision_doc_source_edit,
    dr_precision_doc_source_info,
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
use common::icc_chain::{apply, inverse, Chain};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const TOLERANCE: f64 = 1e-6;
const DEPTH_AUTHORIZATION: &str =
    "source and document depths differ; explicit convert_fallback authorization is required";
const RANGE: &str = "the placed image's colours convert to a value outside the 32-bit float range (±65504) in the document's profile; the place is refused rather than clipped";
const DEPTH_FLAGS: &str = "unsupported embedded source depth or flags";
const MASTER_WORD: &str = "invalid native float master word";
const EMPTY_REPORT: &str = r#"{"blend_reset":[],"effect_blend_reset":[],"rasterized":[]}"#;

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
fn mixed(
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

/// `dr_doc_mixed_source_edit` (a byte document), its refusal as `Err`.
fn mixed_byte(
    doc: &DrDocument,
    source16: &PrecisionDocument,
    request: Value,
) -> Result<(), String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_mixed_source_edit(
            doc,
            ptr::null(),
            source16,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if !out.is_null() {
        drop(unsafe { Box::from_raw(out) });
    }
    owned(err).map_or(Ok(()), Err)
}

/// A placed result that must exist.
fn placed(
    doc: &PrecisionDocument,
    source8: Option<&DrDocument>,
    source16: Option<&PrecisionDocument>,
    request: Value,
) -> PrecisionDocument {
    mixed(doc, source8, source16, request)
        .unwrap()
        .expect("the edit changes the document")
}

/// `dr_precision_doc_source_edit` with no input, which must change the
/// document.
fn edit(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
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

fn info(doc: &PrecisionDocument, index: usize) -> Value {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_source_info(doc, index, &mut err) };
    assert!(owned(err).is_none());
    serde_json::from_str(&owned(out).expect("source info")).unwrap()
}

fn place(id: &str, x: i32, y: i32) -> Value {
    json!({"op":"place","source_id":id,"name":id,"x":x,"y":y,"depth_conversion":"reject"})
}

fn transform(layer: usize, matrix: [f64; 6], filter: &str) -> Value {
    json!({"op":"transform","layer":layer,"matrix":matrix,"filter":filter})
}

/// A `w × h` float document in `profile` of one opaque record.
fn base(w: u32, h: u32, profile: &Arc<IccProfile>) -> PrecisionDocument {
    float_doc(
        w,
        h,
        profile,
        &[vec![[0.25, 0.5, 0.75, 1.0]; (w * h) as usize]],
    )
}

/// The f32 master: a one-record float document holding `pixels` exactly.
fn master(w: u32, h: u32, profile: &Arc<IccProfile>, pixels: Vec<[f32; 4]>) -> PrecisionDocument {
    float_doc(w, h, profile, &[pixels])
}

/// Above white, below 0 and half covered, among ordinary values.
fn extended(w: u32, h: u32) -> Vec<[f32; 4]> {
    (0..w * h)
        .map(|i| match i % 4 {
            0 => [2.0, -0.25, 0.5, 0.5],
            1 => [0.125, 3.5, -0.0625, 1.0],
            2 => [-0.5, 0.75, 1.25, 0.25],
            _ => [i as f32 / 64.0, 0.375, 0.875, 1.0],
        })
        .collect()
}

fn byte_master(w: u32, h: u32) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, y| {
        Rgba([(x * 41 + y * 7) as u8, 200, (x * y * 23 + 9) as u8, 255])
    }))
}

fn typed_master(w: u32, h: u32) -> PrecisionDocument {
    let codes = (0..w * h)
        .map(|i| [(i * 2311) as u16, (i * 9001) as u16, 4000, 65535])
        .collect();
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, codes, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

fn floats(doc: &PrecisionDocument, index: usize) -> Vec<[f32; 4]> {
    doc.layer_source(index)
        .expect("a raster record")
        .to_rgba32()
        .expect("float samples")
}

fn bits(pixels: &[[f32; 4]]) -> Vec<[u32; 4]> {
    pixels.iter().map(|p| p.map(f32::to_bits)).collect()
}

fn combined(src: &Chain, dst: &Chain) -> [[f64; 3]; 3] {
    let inv = inverse(dst.to_pcs);
    std::array::from_fn(|r| {
        std::array::from_fn(|c| (0..3).map(|k| inv[r][k] * src.to_pcs[k][c]).sum())
    })
}

fn assert_close(got: f32, want: f64, what: &str) {
    let bound = TOLERANCE * want.abs().max(1.0);
    assert!(
        (f64::from(got) - want).abs() <= bound,
        "{what}: {got} vs {want}"
    );
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

fn reopen(bytes: &[u8]) -> Result<PrecisionDocument, String> {
    PrecisionDocument::parse_native(bytes, LIMIT, 0, None)
}

// ------------------------------------------------------- place and info --

#[test]
fn an_srgb_float_master_places_bit_exact_with_no_authorization() {
    let srgb = IccProfile::srgb();
    let pixels = extended(6, 5);
    let source = master(6, 5, &srgb, pixels.clone());
    let doc = placed(&base(16, 12, &srgb), None, Some(&source), place("f", 2, 3));
    assert_eq!(doc.float_refusal(), None);
    assert_eq!(doc.layer(1).unwrap().offset(), (2, 3));
    assert_eq!(bits(&floats(&doc, 1)), bits(&pixels));
    let report = info(&doc, 1);
    assert_eq!(report["valid"], true);
    assert_eq!(report["depth"], 32);
    assert_eq!(report["master_depth"], 32);
    assert_eq!(report["storage"], "rgba32_float");
    assert_eq!(report["fallback_depth"], 32);
    assert_eq!(report["depth_conversion"], "none");
    assert_eq!(
        (report["width"].clone(), report["height"].clone()),
        (json!(6), json!(5))
    );
}

#[test]
fn a_display_p3_float_master_converts_by_the_matrix_unclamped() {
    let (srgb, p3) = (IccProfile::srgb(), IccProfile::display_p3());
    let m = combined(&Chain::of(&p3), &Chain::of(&srgb));
    let mut pixels = extended(5, 4);
    pixels[3] = [1.0, 0.0, 0.0, 1.0];
    let source = master(5, 4, &p3, pixels.clone());
    let doc = placed(&base(8, 8, &srgb), None, Some(&source), place("p3", 0, 0));
    let got = floats(&doc, 1);
    assert_eq!(got.len(), pixels.len());
    for (i, (g, p)) in got.iter().zip(&pixels).enumerate() {
        let want = apply(m, [p[0], p[1], p[2]].map(f64::from));
        for c in 0..3 {
            assert_close(g[c], want[c], &format!("pixel {i} channel {c}"));
        }
        assert_eq!(g[3].to_bits(), p[3].to_bits(), "pixel {i}: alpha");
    }
    // P3 red leaves sRGB: above 1 in red, negative in green and blue.
    assert!(got[3][0] > 1.2 && got[3][1] < 0.0 && got[3][2] < 0.0);
    assert_eq!(info(&doc, 1)["depth_conversion"], "none");
}

#[test]
fn a_converted_sample_past_the_float_range_refuses_the_place() {
    let p3 = IccProfile::display_p3();
    let source = master(2, 2, &p3, vec![[60000.0, 0.0, 0.0, 1.0]; 4]);
    assert_eq!(
        mixed(
            &base(4, 4, &IccProfile::srgb()),
            None,
            Some(&source),
            place("hot", 0, 0)
        )
        .err(),
        Some(RANGE.to_string())
    );
}

/// An 8- or 16-bit document takes a float master under
/// `convert_fallback` (`float_master_low_depth.rs`); `reject` refuses it as
/// any other master of a different depth.
#[test]
fn eight_and_sixteen_bit_documents_refuse_a_float_master_without_convert_fallback() {
    let source = master(4, 4, &IccProfile::srgb(), extended(4, 4));
    let reject = place("f", 0, 0);
    let byte = DrDocument::from_pixels(RgbaImage::from_pixel(8, 8, Rgba([9, 9, 9, 255])));
    let typed = typed_master(8, 8);
    assert_eq!(
        mixed_byte(&byte, &source, reject.clone()).err(),
        Some(DEPTH_AUTHORIZATION.to_string())
    );
    assert_eq!(
        mixed(&typed, None, Some(&source), reject).err(),
        Some(DEPTH_AUTHORIZATION.to_string())
    );
}

// ----------------------------------------------------------- re-render --

/// The two-colour f32 edge: left (0.5, 2.0, 0.1), right (−0.25, 0.75, 4.0).
const LEFT: [f32; 4] = [0.5, 2.0, 0.1, 1.0];
const RIGHT: [f32; 4] = [-0.25, 0.75, 4.0, 1.0];

fn edge(w: u32, h: u32) -> Vec<[f32; 4]> {
    (0..w * h)
        .map(|i| if i % w < w / 2 { LEFT } else { RIGHT })
        .collect()
}

#[test]
fn a_bilinear_scale_mixes_the_float_master_in_linear_light() {
    let srgb = IccProfile::srgb();
    let source = master(16, 4, &srgb, edge(16, 4));
    let doc = placed(
        &base(64, 8, &srgb),
        None,
        Some(&source),
        place("edge", 0, 0),
    );
    let scaled = edit(&doc, transform(1, [3., 0., 0., 1., 0., 0.], "bilinear"));
    let image = scaled.layer_source(1).unwrap();
    assert_eq!(image.dimensions(), (48, 4));
    let mut mixed_samples = 0;
    for x in 0..48 {
        let v = image.sample_at(x, 1).unwrap();
        let w: [f64; 3] = std::array::from_fn(|c| {
            (f64::from(v[c]) - f64::from(LEFT[c])) / (f64::from(RIGHT[c]) - f64::from(LEFT[c]))
        });
        let mean = (w[0] + w[1] + w[2]) / 3.0;
        if !(0.05..0.95).contains(&mean) {
            continue;
        }
        mixed_samples += 1;
        let spread =
            w.iter().cloned().fold(f64::MIN, f64::max) - w.iter().cloned().fold(f64::MAX, f64::min);
        assert!(spread <= 1e-5, "column {x}: weights {w:?}");
    }
    assert!(mixed_samples >= 2, "{mixed_samples} mixed samples");
}

#[test]
fn transforms_re_render_from_the_float_master() {
    let srgb = IccProfile::srgb();
    let source = master(8, 6, &srgb, extended(8, 6));
    let doc = placed(&base(32, 32, &srgb), None, Some(&source), place("m", 0, 0));
    let original = bits(&floats(&doc, 1));
    let half = edit(&doc, transform(1, [0.5, 0., 0., 0.5, 0., 0.], "bilinear"));
    // 0.5 then 2 composes to identity: the place, bit for bit.
    let back = edit(&half, transform(1, [2., 0., 0., 2., 0., 0.], "bilinear"));
    assert_eq!(bits(&floats(&back, 1)), original);
    assert_eq!(back.layer(1).unwrap().offset(), (0, 0));
    // 0.5 then 3 equals one transform by the composed 1.5.
    let twice = edit(&half, transform(1, [3., 0., 0., 3., 0., 0.], "bilinear"));
    let once = edit(&doc, transform(1, [1.5, 0., 0., 1.5, 0., 0.], "bilinear"));
    assert_eq!(bits(&floats(&twice, 1)), bits(&floats(&once, 1)));
    // An exact translate re-identifies the plane: the samples move whole.
    let moved = edit(&doc, transform(1, [1., 0., 0., 1., 3., 2.], "bilinear"));
    assert_eq!(bits(&floats(&moved, 1)), original);
    assert_eq!(moved.layer(1).unwrap().offset(), (3, 2));
    assert_eq!(info(&moved, 1)["valid"], true);
}

#[test]
fn replace_moves_between_an_eight_bit_and_a_float_master() {
    let srgb = IccProfile::srgb();
    let pixels = extended(6, 4);
    let source = master(6, 4, &srgb, pixels.clone());
    let eight = byte_master(6, 4);
    let doc = placed(&base(16, 16, &srgb), Some(&eight), None, place("m", 1, 1));
    let first = bits(&floats(&doc, 1));
    let replace = |revision: &str| json!({"op":"replace","layer":1,"revision_id":revision,"depth_conversion":"reject"});
    let float = placed(&doc, None, Some(&source), replace("float"));
    assert_eq!(bits(&floats(&float, 1)), bits(&pixels));
    let report = info(&float, 1);
    assert_eq!(report["master_depth"], 32);
    assert_eq!(report["revision_id"], "float");
    let back = placed(&float, Some(&eight), None, replace("back"));
    assert_eq!(bits(&floats(&back, 1)), first);
    assert_eq!(info(&back, 1)["master_depth"], 8);

    let flat = edit(&float, json!({"op":"rasterize","layer":1}));
    assert_eq!(bits(&floats(&flat, 1)), bits(&pixels));
    assert_eq!(info(&flat, 1)["valid"], false);
}

// ---------------------------------------------------------- persistence --

fn with_masters() -> PrecisionDocument {
    let (srgb, p3) = (IccProfile::srgb(), IccProfile::display_p3());
    let doc = placed(
        &base(24, 20, &srgb),
        Some(&byte_master(8, 6)),
        None,
        place("a", 2, 1),
    );
    let doc = placed(
        &doc,
        None,
        Some(&master(5, 4, &p3, extended(5, 4))),
        place("b", 6, 7),
    );
    edit(&doc, transform(2, [1.25, 0., 0., 1.5, 0., 0.], "bicubic"))
}

#[test]
fn a_float_master_reopens_bound_and_bit_exact_under_word_67() {
    let doc = with_masters();
    let bytes = doc.encode_native(LIMIT, 0, None).unwrap();
    assert_eq!((word(&bytes, 4), word(&bytes, 8)), (46, 67));
    let reopened = reopen(&bytes).unwrap();
    assert!(reopened.is_float());
    for i in 1..3 {
        assert_eq!(info(&reopened, i)["valid"], true, "layer {i}");
        assert_eq!(info(&reopened, i), info(&doc, i));
        assert_eq!(bits(&floats(&reopened, i)), bits(&floats(&doc, i)));
    }
    assert_eq!(info(&reopened, 2)["storage"], "rgba32_float");
    assert_eq!(reopened.encode_native(LIMIT, 0, None).unwrap(), bytes);
    let request = transform(2, [0.75, 0.25, -0.25, 0.75, 1., 0.], "bilinear");
    let before = edit(&doc, request.clone());
    let after = edit(&reopened, request);
    assert_eq!(bits(&floats(&after, 2)), bits(&floats(&before, 2)));
    assert_eq!(
        after.layer(2).unwrap().offset(),
        before.layer(2).unwrap().offset()
    );

    // 8- and 16-bit masters in a float document write no word 67.
    let srgb = IccProfile::srgb();
    let plain = placed(
        &base(16, 16, &srgb),
        Some(&byte_master(4, 4)),
        None,
        place("a", 0, 0),
    );
    let plain = placed(&plain, None, Some(&typed_master(4, 4)), place("b", 1, 1));
    let bytes = plain.encode_native(LIMIT, 0, None).unwrap();
    assert_eq!(word(&bytes, 4), 46);
    assert_ne!(word(&bytes, 8), 67);
    assert!(reopen(&bytes).is_ok());
}

#[test]
fn the_reader_checks_word_67_before_any_payload_decodes() {
    let bytes = with_masters().encode_native(LIMIT, 0, None).unwrap();
    let mut without = bytes[..8].to_vec();
    without.extend(&bytes[12..]);
    assert_eq!(reopen(&without).err(), Some(DEPTH_FLAGS.to_string()));

    let srgb = IccProfile::srgb();
    let eight = placed(
        &base(8, 8, &srgb),
        Some(&byte_master(4, 4)),
        None,
        place("a", 0, 0),
    )
    .encode_native(LIMIT, 0, None)
    .unwrap();
    assert!(reopen(&eight).is_ok());
    assert_eq!(
        reopen(&spliced(&eight, 8, 67)).err(),
        Some(MASTER_WORD.to_string())
    );

    // A 16-bit file may hold word 67: with a float
    // master it opens; with none the claim refuses as anywhere else.
    let sixteen = typed_master(4, 4).encode_native(LIMIT, 0, None).unwrap();
    assert_eq!(word(&sixteen, 4), 46);
    assert!(reopen(&sixteen).is_ok());
    assert_eq!(
        reopen(&spliced(&sixteen, 8, 67)).err(),
        Some(MASTER_WORD.to_string())
    );
    let mut convert = place("f", 0, 0);
    convert["depth_conversion"] = json!("convert_fallback");
    let held = placed(
        &typed_master(8, 8),
        None,
        Some(&master(4, 4, &srgb, extended(4, 4))),
        convert,
    )
    .encode_native(LIMIT, 0, None)
    .unwrap();
    assert_eq!((word(&held, 4), word(&held, 8)), (46, 67));
    let reopened = reopen(&held).unwrap();
    assert!(!reopened.is_float());
    assert_eq!(info(&reopened, 1)["master_depth"], 32);
}

// ------------------------------------------------------------ narrowing --

/// Narrowing carries a float master's binding as it carries
/// an 8-bit one (receipt `float-low-depth-600/take1`, NR16 / NR8).
#[test]
fn narrowing_carries_a_float_master_binding_and_an_eight_bit_one() {
    let doc = with_masters();
    let sixteen = doc.narrow_structure_to_u16(LIMIT, 0, None).unwrap();
    assert_eq!(info(&sixteen, 1)["valid"], true);
    assert_eq!(info(&sixteen, 2)["valid"], true);
    assert_eq!(info(&sixteen, 2)["master_depth"], 32);
    let eight = doc
        .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, None)
        .unwrap();
    assert!(eight.layers[1].effective_source(&eight.profile).is_some());
    assert!(eight.layers[2].effective_source(&eight.profile).is_some());
}

// ---------------------------------------------------------- layers paste --

/// `dr_precision_doc_paste_layers` of all of `clip` above record 0, Convert
/// or Preserve.
fn paste(
    doc: &PrecisionDocument,
    clip: &PrecisionDocument,
    preserve: bool,
) -> (PrecisionDocument, String) {
    let (mut report, mut err) = (ptr::null_mut(), ptr::null_mut());
    let out = unsafe {
        dr_precision_doc_paste_layers(
            doc,
            clip,
            0,
            0,
            0,
            0,
            preserve,
            LIMIT,
            0,
            &mut report,
            &mut err,
        )
    };
    if let Some(message) = owned(err) {
        panic!("paste: {message}");
    }
    assert!(!out.is_null());
    let report: Value = serde_json::from_str(&owned(report).expect("a report")).unwrap();
    (*unsafe { Box::from_raw(out) }, report.to_string())
}

#[test]
fn a_float_master_pasted_as_layers_re_renders_or_keeps_its_binding() {
    let (srgb, p3) = (IccProfile::srgb(), IccProfile::display_p3());
    let pixels = extended(6, 5);
    let source = master(6, 5, &srgb, pixels.clone());
    let clip = placed(&base(6, 5, &srgb), None, Some(&source), place("m", 0, 0));
    assert!(clip.layer(1).unwrap().effective_source().is_some());

    // Into a Display P3 document under Convert: a fresh place's render.
    let dest = base(6, 5, &p3);
    let (out, report) = paste(&dest, &clip, false);
    assert_eq!(report, EMPTY_REPORT);
    let landed = out.layer(2).unwrap();
    assert!(landed.effective_source().is_some());
    assert_eq!(info(&out, 2)["master_depth"], 32);
    let fresh = placed(&dest, None, Some(&source), place("fresh", 0, 0));
    assert_eq!(bits(&floats(&out, 2)), bits(&floats(&fresh, 1)));
    assert_eq!(landed.offset(), fresh.layer(1).unwrap().offset());

    // Into the same profile: the binding is kept over the same samples.
    let (same, report) = paste(&base(6, 5, &srgb), &clip, false);
    assert_eq!(report, EMPTY_REPORT);
    assert!(same.layer(2).unwrap().effective_source().is_some());
    assert_eq!(info(&same, 2)["master_depth"], 32);
    assert_eq!(bits(&floats(&same, 2)), bits(&pixels));
}
