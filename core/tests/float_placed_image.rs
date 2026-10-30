//! Phase 187a: a placed image in a 32-bit float document
//! (`retained_source_float`), through the source-edit FFI.
//!
//! Oracles, none read off the implementation: a rendered sample is the f64
//! chain over the profiles' own tag data (`common::icc_chain`: the master's
//! `curv` table, then `inv(dst.to_pcs) · src.to_pcs` when the profiles
//! differ, never clamped — Photoshop 26.10.0's measured H-ext), within
//! 1e-6 · max(1, |want|), alpha exactly `code / 65535`; the resample domain
//! is the per-channel weight of a two-colour edge, which is one number across
//! R, G and B only when the mix ran in linear light (measured H-lin); every
//! refusal sentence is written out here. A LUT-profile master is the
//! probe's linear profile rebuilt here: its oracle is the written corners,
//! `Σ (code / 255) · corner / 32768` as PCS XYZ, then `inv(sRGB.to_pcs)`,
//! unclamped (Photoshop's measured H-ext, `float-lut-placed-601/take1`).

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_evaluation::{dr_doc_evaluation, dr_precision_doc_evaluation};
use doctored_core::ffi_layer_clipboard::dr_precision_doc_paste_layers;
use doctored_core::ffi_retained_source::{
    dr_precision_doc_mixed_source_edit, dr_precision_doc_mixed_source_plan,
    dr_precision_doc_source_edit, dr_precision_doc_source_info,
};
use doctored_core::float_document::float_operators_json;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_quantize::PrecisionDither;
use doctored_core::retained_source::SourceEdit;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common;
use crate::float_fixture::float_doc;
use crate::icc_lut_builders::{build, tag_desc, tag_mft2, tag_wtpt};
use common::icc_chain::{apply, inverse, Chain};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const TOLERANCE: f64 = 1e-6;
// ------------------------------------------------------------- plumbing --

pub(crate) fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

/// `dr_precision_doc_mixed_source_edit`: `Ok(None)` for a no-op.
pub(crate) fn mixed(
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

/// `dr_precision_doc_source_edit` with no input, which must change the
/// document.
pub(crate) fn edit(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
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

pub(crate) fn info(doc: &PrecisionDocument, index: usize) -> Value {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_source_info(doc, index, &mut err) };
    assert!(owned(err).is_none());
    serde_json::from_str(&owned(out).expect("source info")).unwrap()
}

pub(crate) fn place(id: &str, x: i32, y: i32) -> Value {
    json!({"op":"place","source_id":id,"name":id,"x":x,"y":y,"depth_conversion":"reject"})
}

pub(crate) fn transform(layer: usize, matrix: [f64; 6], filter: &str) -> Value {
    json!({"op":"transform","layer":layer,"matrix":matrix,"filter":filter})
}

/// A `w × h` float sRGB document of one opaque record.
pub(crate) fn base(w: u32, h: u32) -> PrecisionDocument {
    float_doc(
        w,
        h,
        &IccProfile::srgb(),
        &[vec![[0.25, 0.5, 0.75, 1.0]; (w * h) as usize]],
    )
}

pub(crate) fn byte_master(w: u32, h: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, y| Rgba(pixel(x, y))))
}

fn pattern8(x: u32, y: u32) -> [u8; 4] {
    [
        (x * 41 + y * 7) as u8,
        255 - (y * 37 + x * 3) as u8,
        (x * y * 23 + 9) as u8,
        if (x + y).is_multiple_of(4) { 120 } else { 255 },
    ]
}

pub(crate) fn typed_master(
    w: u32,
    h: u32,
    codes: Vec<[u16; 4]>,
    profile: Arc<IccProfile>,
) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, codes, profile, None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

/// Saturated P3 primaries and a ramp: the primaries leave sRGB's gamut.
pub(crate) fn p3_codes(w: u32, h: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| match i % 5 {
            0 => [65535, 0, 0, 65535],
            1 => [0, 65535, 0, 65535],
            2 => [0, 0, 65535, 40000],
            _ => [
                (i * 2311) as u16,
                (i * 9001) as u16,
                (i * 433) as u16,
                65535,
            ],
        })
        .collect()
}

pub(crate) fn floats(doc: &PrecisionDocument, index: usize) -> Vec<[f32; 4]> {
    doc.layer_source(index)
        .expect("a raster record")
        .to_rgba32()
        .expect("float samples")
}

pub(crate) fn bits(pixels: &[[f32; 4]]) -> Vec<[u32; 4]> {
    pixels.iter().map(|p| p.map(f32::to_bits)).collect()
}

pub(crate) fn combined(src: &Chain, dst: &Chain) -> [[f64; 3]; 3] {
    let inv = inverse(dst.to_pcs);
    std::array::from_fn(|r| {
        std::array::from_fn(|c| (0..3).map(|k| inv[r][k] * src.to_pcs[k][c]).sum())
    })
}

/// The f64 chain for one master code: its curve, then `matrix`, unclamped.
pub(crate) fn oracle(decode: &Chain, matrix: Option<[[f64; 3]; 3]>, code: [u16; 4]) -> [f64; 4] {
    let linear: [f64; 3] = std::array::from_fn(|c| decode.decode(f64::from(code[c]) / 65535.0));
    let rgb = matrix.map_or(linear, |m| apply(m, linear));
    [rgb[0], rgb[1], rgb[2], f64::from(code[3]) / 65535.0]
}

pub(crate) fn assert_rendered(
    got: &[[f32; 4]],
    codes: &[[u16; 4]],
    want: impl Fn([u16; 4]) -> [f64; 4],
) {
    assert_eq!(got.len(), codes.len());
    for (i, (g, code)) in got.iter().zip(codes).enumerate() {
        let w = want(*code);
        for c in 0..3 {
            let bound = TOLERANCE * w[c].abs().max(1.0);
            let d = (f64::from(g[c]) - w[c]).abs();
            assert!(d <= bound, "pixel {i} channel {c}: {} vs {}", g[c], w[c]);
        }
        assert_eq!(g[3], f32::from(code[3]) / 65535.0, "pixel {i}: alpha");
    }
}

fn widened(master: &DrDocument) -> Vec<[u16; 4]> {
    master.layers[0]
        .pixels
        .contiguous_image()
        .pixels()
        .map(|p| p.0.map(|v| u16::from(v) * 257))
        .collect()
}

// ------------------------------------------------------- place and info --

#[test]
fn eight_and_sixteen_bit_masters_place_without_authorization_as_linear_light() {
    let (srgb, p3) = (IccProfile::srgb(), IccProfile::display_p3());
    let (s, d) = (Chain::of(&srgb), Chain::of(&p3));
    let dest = base(16, 12);
    let master8 = byte_master(6, 5, pattern8);
    let placed = mixed(&dest, Some(&master8), None, place("eight", 2, 3))
        .unwrap()
        .unwrap();
    assert_eq!(placed.float_refusal(), None);
    let layer = placed.layer(1).unwrap();
    assert_eq!(layer.offset(), (2, 3));
    assert_eq!(placed.layer_source(1).unwrap().dimensions(), (6, 5));
    assert_rendered(&floats(&placed, 1), &widened(&master8), |c| {
        oracle(&s, None, c)
    });
    let report = info(&placed, 1);
    assert_eq!(report["valid"], true);
    assert_eq!(report["master_depth"], 8);
    assert_eq!(report["fallback_depth"], 32);
    assert_eq!(report["depth_conversion"], "convert_fallback");

    // A 16-bit Display P3 master: P3 decode, then P3 → sRGB, negatives kept.
    let codes = p3_codes(5, 4);
    let master16 = typed_master(5, 4, codes.clone(), p3.clone());
    let both = mixed(&placed, None, Some(&master16), place("sixteen", 0, 0))
        .unwrap()
        .unwrap();
    let m = combined(&d, &s);
    let landed = floats(&both, 2);
    assert_rendered(&landed, &codes, |c| oracle(&d, Some(m), c));
    assert!(landed.iter().any(|p| p[..3].iter().any(|v| *v < 0.0)));
    assert!(landed.iter().any(|p| p[..3].iter().any(|v| *v > 1.0)));
    let report = info(&both, 2);
    assert_eq!(
        (
            report["master_depth"].clone(),
            report["fallback_depth"].clone()
        ),
        (json!(16), json!(32))
    );
    assert_eq!(report["depth_conversion"], "convert_fallback");

    // Replace with `reject`: the new revision renders into the same layer.
    let replaced = mixed(
        &both,
        None,
        Some(&master16),
        json!({"op":"replace","layer":1,"revision_id":"second","depth_conversion":"reject"}),
    )
    .unwrap()
    .unwrap();
    assert_rendered(&floats(&replaced, 1), &codes, |c| oracle(&d, Some(m), c));
    assert_eq!(info(&replaced, 1)["revision_id"], "second");
    assert_eq!(info(&replaced, 1)["fallback_depth"], 32);

    // CaptureRaster with `reject`: the record's own pixels become the
    // master's render.
    let master = byte_master(16, 12, pattern8);
    let captured = mixed(
        &dest,
        Some(&master),
        None,
        json!({"op":"capture_raster","layer":0,"source_id":"captured",
            "source_transform":[1,0,0,1,0,0],"depth_conversion":"reject"}),
    )
    .unwrap()
    .unwrap();
    assert_rendered(&floats(&captured, 0), &widened(&master), |c| {
        oracle(&s, None, c)
    });
    assert_eq!(info(&captured, 0)["fallback_depth"], 32);
}

// ------------------------------------------------ a LUT-profile master --

/// The `A2B0` corners of the probe's linear profile, in u16 PCS-XYZ
/// codes (XYZ = code / 32768): device red, green and blue.
const LUT_CORNERS: [[f64; 3]; 3] = [
    [19980.0, 10194.0, 638.0],
    [6727.0, 20502.0, 1995.0],
    [4889.0, 2072.0, 24398.0],
];

/// The probe's patches: black, white, the three primaries, grey and a half
/// covered colour.
const LUT_PATCHES: [[u8; 4]; 7] = [
    [0, 0, 0, 255],
    [255, 255, 255, 255],
    [255, 0, 0, 255],
    [0, 255, 0, 255],
    [0, 0, 255, 255],
    [128, 128, 128, 255],
    [128, 64, 32, 128],
];

/// An RGB display profile with lut16 `A2B0` / `B2A0` and no matrix/TRC tags:
/// identity 2-entry tables around a 2-point grid whose corner (r, g, b) is
/// `r · cR + g · cG + b · cB`, so the stored map is exactly linear.
fn lut_profile() -> Arc<IccProfile> {
    let corners = |c: &[f64]| -> Vec<f64> {
        (0..3)
            .map(|k| (0..3).map(|i| c[i] * LUT_CORNERS[i][k]).sum::<f64>() / 65535.0)
            .collect()
    };
    let built = build(
        [0x02, 0x20, 0, 0],
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("rz601 LUT RGB")),
            (b"wtpt", tag_wtpt([0.9642, 1.0, 0.8249])),
            (b"A2B0", tag_mft2(3, 3, 2, &corners)),
            (b"B2A0", tag_mft2(3, 3, 2, &|c| c.to_vec())),
        ],
    );
    let profile = Arc::new(IccProfile::parse(&built).unwrap());
    assert!(profile.model().is_none(), "no matrix/TRC model");
    profile
}

fn lut_master(w: u32, h: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> DrDocument {
    let mut master = byte_master(w, h, pixel);
    master.profile = lut_profile();
    master
}

fn patch_master() -> DrDocument {
    lut_master(7, 2, |x, _| LUT_PATCHES[x as usize])
}

/// The written corners over one widened master code, then the sRGB
/// profile's own colorants inverted, unclamped.
fn lut_oracle(code: [u16; 4]) -> [f64; 4] {
    let device: [f64; 3] = std::array::from_fn(|c| f64::from(code[c] / 257) / 255.0);
    let xyz: [f64; 3] = std::array::from_fn(|r| {
        (0..3)
            .map(|c| device[c] * LUT_CORNERS[c][r] / 32768.0)
            .sum()
    });
    let rgb = apply(inverse(Chain::of(&IccProfile::srgb()).to_pcs), xyz);
    [rgb[0], rgb[1], rgb[2], f64::from(code[3]) / 65535.0]
}

#[test]
fn a_lut_profile_master_places_as_linear_light() {
    let master = patch_master();
    let codes = widened(&master);
    let wants: Vec<[f64; 4]> = codes.iter().map(|c| lut_oracle(*c)).collect();
    assert!(wants.iter().any(|w| w[..3].iter().any(|v| *v > 1.0)));
    assert!(wants.iter().any(|w| w[..3].iter().any(|v| *v < 0.0)));
    let placed = mixed(&base(16, 12), Some(&master), None, place("lut", 2, 3))
        .unwrap()
        .unwrap();
    assert_eq!(placed.float_refusal(), None);
    assert_eq!(placed.layer(1).unwrap().offset(), (2, 3));
    assert_rendered(&floats(&placed, 1), &codes, lut_oracle);
    let report = info(&placed, 1);
    assert_eq!(report["valid"], true);
    assert_eq!(report["master_depth"], 8);
    assert_eq!(report["fallback_depth"], 32);
}

#[test]
fn a_lut_master_re_renders_through_a_transform() {
    let placed = mixed(
        &base(32, 32),
        Some(&lut_master(8, 6, pattern8)),
        None,
        place("lut", 0, 0),
    )
    .unwrap()
    .unwrap();
    let original = bits(&floats(&placed, 1));
    let half = edit(
        &placed,
        transform(1, [0.5, 0., 0., 0.5, 0., 0.], "bilinear"),
    );
    assert_eq!(half.layer_source(1).unwrap().dimensions(), (4, 3));
    assert_eq!(info(&half, 1)["valid"], true);
    assert_eq!(info(&half, 1)["fallback_depth"], 32);
    // 0.5 then 2 re-renders the master at identity: the place, bit for bit.
    let back = edit(&half, transform(1, [2., 0., 0., 2., 0., 0.], "bilinear"));
    assert_eq!(bits(&floats(&back, 1)), original);
    assert_eq!(back.layer(1).unwrap().offset(), (0, 0));
}

/// `dr_precision_doc_paste_layers` of all of `clip` above record 0 under
/// Convert, with its report.
pub(crate) fn paste(
    doc: &PrecisionDocument,
    clip: &PrecisionDocument,
) -> (PrecisionDocument, Value) {
    let (mut report, mut err) = (ptr::null_mut(), ptr::null_mut());
    let out = unsafe {
        dr_precision_doc_paste_layers(
            doc,
            clip,
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
    assert!(!out.is_null());
    let report = serde_json::from_str(&owned(report).expect("a report")).unwrap();
    (*unsafe { Box::from_raw(out) }, report)
}

#[test]
fn a_lut_profile_record_pasted_as_layers_re_renders() {
    let master = patch_master();
    let codes = widened(&master);
    // The clip: a 16-bit sRGB document holding the LUT master placed.
    let sixteen = typed_master(8, 6, vec![[65535; 4]; 48], IccProfile::srgb());
    let clip = mixed(
        &sixteen,
        Some(&master),
        None,
        json!({"op":"place","source_id":"lut","name":"lut","x":0,"y":0,
            "depth_conversion":"convert_fallback"}),
    )
    .unwrap()
    .unwrap();
    assert!(!clip.is_float());
    assert_eq!(info(&clip, 1)["fallback_depth"], 16);

    let (out, report) = paste(&base(8, 6), &clip);
    assert_eq!(
        report,
        json!({"blend_reset":[],"effect_blend_reset":[],"rasterized":[]})
    );
    // Record 0 is the destination's, 1 the clip's base, 2 the placed image:
    // re-rendered from its master, unclamped, its binding kept.
    assert!(out.layer(2).unwrap().effective_source().is_some());
    assert_rendered(&floats(&out, 2), &codes, lut_oracle);
    let report = info(&out, 2);
    assert_eq!(report["valid"], true);
    assert_eq!(report["master_depth"], 8);
    assert_eq!(report["fallback_depth"], 32);
}

fn plan(doc: &PrecisionDocument, source8: &DrDocument, request: Value) -> Value {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_mixed_source_plan(doc, source8, ptr::null(), text.as_ptr(), &mut err)
    };
    if let Some(message) = owned(err) {
        panic!("plan: {message}");
    }
    serde_json::from_str(&owned(out).expect("a plan")).unwrap()
}

/// The plan charges the LUT conversion: `LUT_COLOR_WORK_FACTOR` (11) times
/// the 4 units a pixel's colour visit costs, so 40 more per master pixel
/// than the sRGB-tagged twin (whose profile is the document's: no
/// conversion, no tables), and the prepared pipeline's tables as scratch.
#[test]
fn a_float_plan_charges_a_lut_master_its_pipeline() {
    let (w, h) = (8u64, 6u64);
    let dest = base(16, 12);
    let lut = plan(&dest, &lut_master(8, 6, pattern8), place("lut", 0, 0));
    let twin = plan(&dest, &byte_master(8, 6, pattern8), place("twin", 0, 0));
    let field = |plan: &Value, key: &str| plan[key].as_u64().unwrap();
    assert!(
        field(&lut, "work_units") >= field(&twin, "work_units") + w * h * 40,
        "{lut} vs {twin}"
    );
    assert!(
        field(&lut, "scratch_bytes") > field(&twin, "scratch_bytes"),
        "{lut} vs {twin}"
    );
}

#[test]
fn the_operator_table_admits_placed_images_and_has_no_live_filter_row() {
    let table: Value = serde_json::from_str(&float_operators_json()).unwrap();
    let admitted = table["admitted"].as_array().unwrap();
    assert!(admitted.contains(&json!("placed_image")));
    // Phase 187b: text and shape layers are admitted, and their row is gone.
    assert!(admitted.contains(&json!("text_layer")));
    assert!(admitted.contains(&json!("shape_layer")));
    let refused: Vec<&str> = table["refused"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["operator"].as_str().unwrap())
        .collect();
    // Live filters were removed (2026-10-10): no row names one.
    assert!(!admitted
        .iter()
        .any(|row| row.as_str().is_some_and(|s| s.contains("live_filter"))));
    assert!(!refused.iter().any(|row| row.contains("live filter")));
    assert!(!refused.iter().any(|row| row.contains("live-filter")));

    assert!(!refused.iter().any(|row| row.contains("unmeasured")));
    assert!(!refused
        .iter()
        .any(|row| row.contains("artwork description")));
    assert!(!refused.iter().any(|row| row.contains("retained source")));
}

// ----------------------------------------------------------- re-render --

/// The two-colour edge: left (16, 200, 100), right (240, 40, 180).
fn edge() -> DrDocument {
    byte_master(16, 4, |x, _| {
        if x < 8 {
            [16, 200, 100, 255]
        } else {
            [240, 40, 180, 255]
        }
    })
}

#[test]
fn a_bilinear_scale_mixes_in_linear_light() {
    let s = Chain::of(&IccProfile::srgb());
    let placed = mixed(&base(64, 8), Some(&edge()), None, place("edge", 0, 0))
        .unwrap()
        .unwrap();
    let scaled = edit(&placed, transform(1, [3., 0., 0., 1., 0., 0.], "bilinear"));
    let image = scaled.layer_source(1).unwrap();
    assert_eq!(image.dimensions(), (48, 4));
    let lin = |rgb: [u8; 3]| rgb.map(|v| s.decode(f64::from(v) / 255.0));
    let (a, b) = (lin([16, 200, 100]), lin([240, 40, 180]));
    let mut mixed_samples = 0;
    let mut encoded_spread: f64 = 0.0;
    for x in 0..48 {
        let v = image.sample_at(x, 1).unwrap();
        let w: [f64; 3] = std::array::from_fn(|c| (f64::from(v[c]) - a[c]) / (b[c] - a[c]));
        let mean = (w[0] + w[1] + w[2]) / 3.0;
        if !(0.05..0.95).contains(&mean) {
            continue;
        }
        mixed_samples += 1;
        let spread =
            w.iter().cloned().fold(f64::MIN, f64::max) - w.iter().cloned().fold(f64::MAX, f64::min);
        assert!(spread <= 1e-5, "column {x}: linear weights {w:?}");
        let e: [f64; 3] = std::array::from_fn(|c| {
            let enc = |y: f64| s.encode(y);
            (enc(f64::from(v[c])) - enc(a[c])) / (enc(b[c]) - enc(a[c]))
        });
        encoded_spread = encoded_spread.max(
            e.iter().cloned().fold(f64::MIN, f64::max) - e.iter().cloned().fold(f64::MAX, f64::min),
        );
    }
    assert!(mixed_samples >= 2, "{mixed_samples} mixed samples");
    assert!(encoded_spread > 0.05, "the encoded domain would disagree");
}

#[test]
fn a_projective_transform_renders_from_the_master() {
    let placed = mixed(
        &base(16, 16),
        Some(&byte_master(8, 8, pattern8)),
        None,
        place("p", 0, 0),
    )
    .unwrap()
    .unwrap();
    let perspective = GeometryEdit::parse(
        &json!({"op":"perspective_set","layers":[1],"source_rect":[0,0,8,8],
            "quad":[0,0,7,0,6,7,0,8],"filter":"bilinear"})
        .to_string(),
    )
    .unwrap();
    let warped = placed
        .geometry(&perspective, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let report = info(&warped, 1);
    assert_eq!(report["valid"], true);
    assert_eq!(report["transform_kind"], "projective");
    assert_eq!(report["fallback_depth"], 32);
    assert!(floats(&warped, 1)
        .iter()
        .all(|p| p.iter().all(|v| v.is_finite())));
    assert_ne!(bits(&floats(&warped, 1)), bits(&floats(&placed, 1)));
}

#[test]
fn transforms_re_render_from_the_master_and_a_translate_moves_the_samples() {
    let placed = mixed(
        &base(32, 32),
        Some(&byte_master(8, 6, pattern8)),
        None,
        place("p", 0, 0),
    )
    .unwrap()
    .unwrap();
    let original = bits(&floats(&placed, 1));
    let half = edit(
        &placed,
        transform(1, [0.5, 0., 0., 0.5, 0., 0.], "bilinear"),
    );
    // 0.5 then 2 re-renders the master at identity: the place, bit for bit.
    let back = edit(&half, transform(1, [2., 0., 0., 2., 0., 0.], "bilinear"));
    assert_eq!(bits(&floats(&back, 1)), original);
    assert_eq!(back.layer(1).unwrap().offset(), (0, 0));
    // 0.5 then 3 equals one transform by the composed 1.5.
    let twice = edit(&half, transform(1, [3., 0., 0., 3., 0., 0.], "bilinear"));
    let once = edit(
        &placed,
        transform(1, [1.5, 0., 0., 1.5, 0., 0.], "bilinear"),
    );
    assert_eq!(bits(&floats(&twice, 1)), bits(&floats(&once, 1)));
    assert_eq!(
        twice.layer(1).unwrap().offset(),
        once.layer(1).unwrap().offset()
    );
    // An exact translate resamples nothing.
    let moved = edit(&placed, transform(1, [1., 0., 0., 1., 3., 2.], "bilinear"));
    assert_eq!(bits(&floats(&moved, 1)), original);
    assert_eq!(moved.layer(1).unwrap().offset(), (3, 2));
    assert_eq!(info(&moved, 1)["valid"], true);
}

#[test]
fn rasterize_keeps_the_pixels_and_drops_the_binding() {
    let placed = mixed(
        &base(16, 16),
        Some(&byte_master(8, 6, pattern8)),
        None,
        place("p", 1, 1),
    )
    .unwrap()
    .unwrap();
    let flat = edit(&placed, json!({"op":"rasterize","layer":1}));
    assert_eq!(bits(&floats(&flat, 1)), bits(&floats(&placed, 1)));
    assert_eq!(info(&flat, 1)["valid"], false);
    assert_eq!(flat.float_refusal(), None);
}

// ------------------------------------------------- persistence and depth --

fn two_placed() -> PrecisionDocument {
    let placed = mixed(
        &base(24, 20),
        Some(&byte_master(8, 6, pattern8)),
        None,
        place("a", 2, 1),
    )
    .unwrap()
    .unwrap();
    let placed = mixed(
        &placed,
        None,
        Some(&typed_master(
            5,
            4,
            p3_codes(5, 4),
            IccProfile::display_p3(),
        )),
        place("b", 6, 7),
    )
    .unwrap()
    .unwrap();
    edit(
        &placed,
        transform(1, [1.25, 0., 0., 1.5, 0., 0.], "bicubic"),
    )
}

#[test]
fn a_placed_float_document_reopens_bound_and_bit_exact() {
    let doc = two_placed();
    let bytes = doc.encode_native(LIMIT, 0, None).unwrap();
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    assert!(reopened.is_float());
    for i in 1..3 {
        let report = info(&reopened, i);
        assert_eq!(report["valid"], true, "layer {i}");
        assert_eq!(report["fallback_depth"], 32);
        assert_eq!(report, info(&doc, i));
        assert_eq!(bits(&floats(&reopened, i)), bits(&floats(&doc, i)));
        assert_eq!(
            reopened.layer(i).unwrap().offset(),
            doc.layer(i).unwrap().offset()
        );
    }
    assert_eq!(reopened.encode_native(LIMIT, 0, None).unwrap(), bytes);
    let request = transform(2, [0.75, 0.25, -0.25, 0.75, 1., 0.], "bilinear");
    let before = edit(&doc, request.clone());
    let after = edit(&reopened, request);
    assert_eq!(bits(&floats(&after, 2)), bits(&floats(&before, 2)));
    assert_eq!(
        after.layer(2).unwrap().offset(),
        before.layer(2).unwrap().offset()
    );
}

/// The float plan charges each fallback raster raw, `w · h · 16`, and each
/// embedded master at its own depth, `w · h · 4 · depth / 8`: a master is a
/// PNG at every document depth, and the reader decodes it as one.
#[test]
fn a_float_plan_charges_a_master_at_its_own_depth() {
    let doc = two_placed();
    let area = |(w, h): (u32, u32)| u64::from(w) * u64::from(h);
    let rasters: u64 = (0..3)
        .map(|i| area(doc.layer_source(i).unwrap().dimensions()))
        .sum();
    let plan = doc.native_plan().unwrap();
    assert_eq!(
        plan.serialized_pixel_bytes,
        16 * rasters + 4 * (8 * 6) + 8 * (5 * 4)
    );
}

/// Two 45 MP 8-bit masters beside small fallbacks: charged as raw f32
/// rasters they need 2 × 16 B/px decoded and the 9 B/px float reopen margin
/// (about 2.25 GB; the streamed reader holds no file), past the 2 GiB
/// reference working limit; charged as the PNGs they are (4 B/px decoded, one
/// 27 B/px whole decode, about 1.58 GB) the plan fits.
#[test]
fn a_float_plan_admits_masters_a_raw_f32_charge_would_refuse() {
    use doctored_core::precision_native::native_color_plan;
    let canvas = (100, 100);
    let master = (9000, 5000);
    let plan = native_color_plan(
        canvas,
        &[canvas, (120, 80)],
        &[(master, 8), (master, 8)],
        true,
    )
    .expect("the true charge admits");
    assert_eq!(
        plan.serialized_pixel_bytes,
        16 * (100 * 100 + 120 * 80) + 2 * 4 * 9000 * 5000
    );
    let raw = native_color_plan(canvas, &[canvas, (120, 80), master, master], &[], true)
        .expect_err("a 16 B/px charge refuses");
    assert!(
        raw.starts_with("At 32 bits the result needs more than the 2 GB working limit"),
        "{raw}"
    );
}

#[test]
fn narrowing_to_sixteen_bits_carries_an_effective_binding() {
    let doc = two_placed();
    let sixteen = doc.narrow_structure_to_u16(LIMIT, 0, None).unwrap();
    assert!(!sixteen.is_float());
    for i in 1..3 {
        let report = info(&sixteen, i);
        assert_eq!(report["valid"], true, "layer {i}");
        assert_eq!(report["fallback_depth"], 16);
    }
    // Linear light at 16 bits holds a placed image (phase 187a0), and
    // the carried binding re-renders on the next edit.
    assert_eq!(unsafe { dr_precision_doc_evaluation(&sixteen) }, 1);
    assert_eq!(sixteen.linear_light_refusal(), None);
    let moved = edit(
        &sixteen,
        transform(1, [1.5, 0., 0., 1.5, 0., 0.], "bilinear"),
    );
    assert_eq!(info(&moved, 1)["valid"], true);
    assert_eq!(moved.linear_light_refusal(), None);
}

#[test]
fn narrowing_to_eight_bits_carries_an_effective_binding() {
    let doc = two_placed();
    let eight = doc
        .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, None)
        .unwrap();
    for i in 1..3 {
        assert!(
            eight.layers[i].effective_source(&eight.profile).is_some(),
            "layer {i}"
        );
    }
    assert_eq!(unsafe { dr_doc_evaluation(&eight) }, 1);
    assert_eq!(eight.linear_light_refusal(), None);
    let moved = eight
        .source_edit(
            None,
            &SourceEdit::parse(&transform(1, [1.5, 0., 0., 1.5, 0., 0.], "bilinear").to_string())
                .unwrap(),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .expect("the transform changes the layer");
    assert!(moved.layers[1].effective_source(&moved.profile).is_some());
    assert_eq!(moved.linear_light_refusal(), None);
}
