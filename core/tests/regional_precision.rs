//! Phase 13A-1 (and 13A-2a's admissions): the 16-bit and 32-bit float regional render against the
//! full-frame typed projection. The oracle is INDEPENDENT of the regional
//! code: `dr_precision_doc_render`'s projection is read back through
//! `dr_precision_image_pixels_rgba16` / `to_rgba32`, cropped and top-left-cell
//! sampled here in plain index arithmetic, and every admitted stack must
//! match it sample for sample (u16 codes, f32 bit patterns).

use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::BlendMode;
use doctored_core::doc_evaluation::Evaluation;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_jobs::*;
use doctored_core::ffi_precision::*;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_regional::*;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::style::{BlendIf, LayerStyle};
use serde_json::{json, Value};

use crate::region_plan;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 23;
const H: u32 = 15;
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

// ------------------------------------------------------------- plumbing --

fn take(p: *mut c_char) -> String {
    if p.is_null() {
        return "<no error message>".into();
    }
    let text = unsafe { std::ffi::CStr::from_ptr(p) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(p) };
    text
}

/// Samples of a typed image as `u32` bit patterns: u16 codes widened, f32
/// through `to_bits`, so one equality covers both storages exactly.
fn samples(image: *mut PrecisionImage) -> (u32, u32, Vec<u32>) {
    let (w, h) = unsafe {
        (
            dr_precision_image_width(image),
            dr_precision_image_height(image),
        )
    };
    let n = (w * h * 4) as usize;
    let p16 = unsafe { dr_precision_image_pixels_rgba16(image) };
    let out = if !p16.is_null() {
        unsafe { std::slice::from_raw_parts(p16, n) }
            .iter()
            .map(|&v| u32::from(v))
            .collect()
    } else {
        unsafe { &*image }
            .to_rgba32()
            .expect("an image with samples")
            .as_flattened()
            .iter()
            .map(|v| v.to_bits())
            .collect()
    };
    unsafe { dr_precision_image_free(image) };
    (w, h, out)
}

fn full(doc: &PrecisionDocument) -> (u32, u32, Vec<u32>) {
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "render: {}", take(err));
    samples(image)
}

fn region(
    doc: &PrecisionDocument,
    (x, y, w, h): (u32, u32, u32, u32),
    d: u32,
    limit: u64,
) -> Result<(u32, u32, Vec<u32>), String> {
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe {
        dr_precision_doc_render_region(doc, x, y, w, h, d, limit, 0, ptr::null(), &mut err)
    };
    if image.is_null() {
        return Err(take(err));
    }
    assert!(err.is_null(), "err_out set on success");
    Ok(samples(image))
}

fn plan(
    doc: &PrecisionDocument,
    (x, y, w, h): (u32, u32, u32, u32),
    d: u32,
) -> Result<Value, String> {
    region_plan::typed(doc, (x, y, w, h), d)
}

/// The independent oracle: sample (x + i·d, y + j·d) of the full render.
fn expected(
    (cw, _, reference): &(u32, u32, Vec<u32>),
    (x, y, w, h): (u32, u32, u32, u32),
    d: u32,
) -> (u32, u32, Vec<u32>) {
    let ow = w.div_ceil(d);
    let oh = h.div_ceil(d);
    let mut out = Vec::new();
    for j in 0..oh {
        for i in 0..ow {
            let cx = u64::from(x) + u64::from(i) * u64::from(d);
            let cy = u64::from(y) + u64::from(j) * u64::from(d);
            let at = ((cy * u64::from(*cw) + cx) * 4) as usize;
            out.extend_from_slice(&reference[at..at + 4]);
        }
    }
    (ow, oh, out)
}

fn assert_regions_match(doc: &PrecisionDocument, what: &str) {
    let reference = full(doc);
    let (cw, ch) = doc.dimensions();
    let windows = [
        (0, 0, cw, ch),
        (3, 2, 11, 7),
        (0, 0, 5, 4),
        (cw - 6, 0, 6, 5),
        (0, ch - 3, 7, 3),
        (cw - 4, ch - 5, 4, 5),
        (cw - 1, ch - 1, 1, 1),
        (0, 0, 1, 1),
    ];
    for window in windows {
        for d in [1, 2, 3, 7, u32::MAX] {
            let got = region(doc, window, d, LIMIT)
                .unwrap_or_else(|e| panic!("{what}: {window:?}/{d}: {e}"));
            assert_eq!(
                got,
                expected(&reference, window, d),
                "{what}: window {window:?} divisor {d}"
            );
        }
    }
}

fn profile() -> Arc<IccProfile> {
    IccProfile::srgb()
}

fn image16(w: u32, h: u32, seed: u32, translucent: bool) -> PrecisionImage {
    let pixels = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let a = if translucent {
                (9000 + (x * 3001 + y * 1777 + seed * 97) % 56000) as u16
            } else {
                65535
            };
            [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                ((x ^ y) * 1999 % 65536 + seed) as u16,
                a,
            ]
        })
        .collect();
    PrecisionImage::from_rgba16(w, h, pixels, profile(), None).unwrap()
}

fn base() -> Layer {
    Layer::raster("Base", image16(W, H, 1, false), (0, 0), 1.0, true).unwrap()
}

fn raster(name: &str, offset: (i32, i32), mode: BlendMode, opacity: f32) -> Layer {
    Layer::raster(name, image16(13, 9, 5, true), offset, opacity, true)
        .unwrap()
        .with_blend_mode(mode)
        .unwrap()
}

fn op(name: &str, params: Value) -> String {
    json!({"type":"adjust","op":name,"params":params,"evaluation":"precision_v1"}).to_string()
}

fn adjustment(name: &str, params: Value, mode: BlendMode) -> Layer {
    Layer::adjustment(name, &op(name, params), 0.8, true, &profile())
        .unwrap()
        .with_blend_mode(mode)
        .unwrap()
}

fn doc(layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        W,
        H,
        profile(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

fn edit(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take(err));
    *unsafe { Box::from_raw(out) }
}

// ---------------------------------------------------------------- tests --

#[test]
fn every_typed_raster_blend_mode_matches_the_u16_projection() {
    for code in 0..27 {
        let mode = BlendMode::from_c(code).unwrap();
        let d = doc(vec![base(), raster("Top", (5, 3), mode, 0.7)]);
        assert_regions_match(&d, &format!("mode {code}"));
    }
}

#[test]
fn offsets_and_hidden_entries_match_the_u16_projection() {
    for offset in [(-6, -4), (15, 10), (W as i32 + 2, 0), (-50, 3)] {
        let hidden = Layer::raster("Hidden", image16(7, 7, 9, true), (1, 1), 1.0, false)
            .unwrap()
            .with_mask(Some(mask()));
        let zero = raster("Zero", (2, 2), BlendMode::Multiply, 0.0);
        let d = doc(vec![
            base(),
            raster("Top", offset, BlendMode::Screen, 0.9),
            hidden,
            zero,
        ]);
        assert_regions_match(&d, &format!("offset {offset:?}"));
        let listed = plan(&d, (0, 0, W, H), 1).unwrap();
        let indices: Vec<_> = listed["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["index"].as_u64().unwrap())
            .collect();
        // The hidden record is skipped; the zero-opacity one is a record of
        // the scene (the typed walk skips visibility alone).
        assert_eq!(indices, [0, 1, 3]);
    }
}

#[test]
fn pointwise_adjustments_match_the_u16_projection() {
    for (name, params) in [
        ("invert", json!({})),
        ("levels", json!({"black":0.1,"white":0.9,"gamma":1.8})),
        ("hue_saturation", json!({"hue":40,"saturation":0.3})),
        (
            "exposure",
            json!({"evaluation":"document_v2","exposure":0.7,"offset":0.017,"gamma":1.2}),
        ),
        ("posterize", json!({"levels":4})),
        (
            "shadows_highlights",
            json!({"shadows":{"amount":0},"highlights":{"amount":0},"midtone_contrast":0.3}),
        ),
    ] {
        for mode in [BlendMode::Normal, BlendMode::Multiply, BlendMode::Dissolve] {
            let d = doc(vec![
                base(),
                raster("Top", (5, 3), BlendMode::Overlay, 0.8),
                adjustment(name, params.clone(), mode),
            ]);
            assert_regions_match(&d, &format!("{name} {mode:?}"));
            let top = &plan(&d, (0, 0, 4, 4), 1).unwrap()["layers"][2];
            assert_eq!(top["kind"], "adjustment");
            assert_eq!(top["operator"], name);
        }
    }
}

#[test]
fn a_linear_light_u16_document_matches_with_a_non_separable_mode() {
    let d = doc(vec![
        base(),
        raster("Top", (-2, 4), BlendMode::Multiply, 0.75),
        adjustment("levels", json!({"gamma":0.7}), BlendMode::Normal),
    ]);
    let linear = d
        .with_evaluation(Evaluation::LinearLight)
        .unwrap()
        .expect("a policy change");
    assert_ne!(full(&linear), full(&d), "the policy changes the projection");
    assert_regions_match(&linear, "linear light");
    // A non-separable mode set after the switch evaluates in linear light,
    // and the regional render follows it sample for sample.
    let hue = edit(
        &linear,
        json!({"op":"layer_blend","version":1,"layer":1,"blend_mode":BlendMode::Hue.to_c()}),
    );
    assert_ne!(full(&hue), full(&linear), "Hue changes the projection");
    assert_regions_match(&hue, "linear light, Hue");
}

fn mask() -> PrecisionMask {
    PrecisionMask::new(
        PrecisionPlane::from_u16(7, 7, vec![40000; 49], None).unwrap(),
        MaskProperties::at((1, 1)),
        true,
    )
    .unwrap()
}

#[test]
fn every_unsupported_typed_construct_refuses_by_name() {
    // Since 13A-3a a group is admitted and equals the reference, hidden or
    // not, isolated or pass-through.
    let group = Layer::isolated_normal(
        "Group",
        vec![raster("Child", (0, 0), BlendMode::Normal, 1.0)],
        1.0,
        false,
    )
    .unwrap();
    assert_regions_match(&doc(vec![base(), group]), "a hidden group");
    let pass = Layer::pass_through(
        "Pass",
        vec![raster("Child", (0, 0), BlendMode::Normal, 1.0)],
        1.0,
        true,
    )
    .unwrap();
    assert_regions_match(&doc(vec![base(), pass]), "a pass-through group");

    // Since 13A-2a clip members, Blend If and masks are admitted and equal
    // the reference.
    let clipped = raster("Clip", (1, 1), BlendMode::Normal, 1.0).with_clipped(true);
    assert_regions_match(&doc(vec![base(), clipped]), "a clip member is admitted");

    let style = LayerStyle::from_json(
        "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}]}",
    )
    .unwrap();
    let styled = raster("Styled", (1, 1), BlendMode::Normal, 1.0)
        .with_layer_style(Some(&style), (W, H))
        .unwrap();
    // Since 13A-3b-i every typed raster style is admitted.
    assert_regions_match(&doc(vec![base(), styled]), "a drop shadow is admitted");

    let blend_if = BlendIf {
        this_layer: [0, 0, 200, 255],
        ..BlendIf::default()
    };
    let weighted = raster("Weighted", (1, 1), BlendMode::Normal, 1.0)
        .with_blend_if(Some(blend_if))
        .unwrap();
    assert_regions_match(&doc(vec![base(), weighted]), "Blend If is admitted");

    let masked = raster("Masked", (1, 1), BlendMode::Normal, 1.0).with_mask(Some(mask()));
    assert_regions_match(&doc(vec![base(), masked]), "a mask is admitted");

    // Since 13A-4a a guided Shadows/Highlights at the root is admitted
    // (staged) and equals the reference.
    let guided = adjustment(
        "shadows_highlights",
        json!({"shadows":{"amount":0.6,"tone":0.4},"radius":12}),
        BlendMode::Normal,
    );
    assert_regions_match(
        &doc(vec![base(), guided]),
        "a guided Shadows/Highlights at the root",
    );
}

#[test]
fn a_hidden_clipped_member_is_admitted_at_u16() {
    let hue = adjustment(
        "hue_saturation",
        json!({"hue":-60,"saturation":0.2}),
        BlendMode::Normal,
    );
    let member = Layer::raster("Member", image16(13, 9, 5, true), (5, 3), 1.0, false).unwrap();
    let clipped = doc(vec![base(), hue.clone(), member.clone().with_clipped(true)]);
    assert_regions_match(&clipped, "hidden, clipped");
    assert_regions_match(&doc(vec![base(), hue, member]), "hidden, unclipped");
}

#[test]
fn the_typed_plan_declares_depth_reference_and_no_output_budget() {
    let d = doc(vec![base(), raster("Top", (5, 3), BlendMode::Screen, 0.5)]);
    let p = plan(&d, (3, 2, 11, 7), 3).unwrap();
    assert_eq!(p["depth"], "rgba16");
    assert_eq!(p["reference"], "dr_precision_doc_render");
    assert_eq!(p["operation"], "composite");
    assert_eq!(p["preview_only"], true);
    assert_eq!(p["output"], json!({"width":4,"height":3,"bytes":96}));
    assert_eq!(p["scratch_heap_bytes"], 12 * 16 + 96 + 1024 * 1024);
    assert_eq!(p["work_units"], 12 * 3);
    assert_eq!(p["cancellation_checkpoint_pixels"], 4096);
    assert!(p.get("max_output_bytes").is_none());
    assert_eq!(p["layers"][1]["operator"], "composite");
    for layer in p["layers"].as_array().unwrap() {
        assert_eq!(layer["clip_base"], Value::Null);
        assert_eq!(layer["mask"], false);
        assert_eq!(layer["blend_if"], false);
    }
    for (window, divisor, message) in [
        (
            (0, 0, 0, 1),
            1,
            "render region must have nonzero dimensions",
        ),
        ((W, 0, 1, 1), 1, "render region is outside the source image"),
        ((0, 0, 1, 1), 0, "render preview divisor must be at least 1"),
    ] {
        assert_eq!(plan(&d, window, divisor).unwrap_err(), message);
        assert_eq!(region(&d, window, divisor, LIMIT).unwrap_err(), message);
    }
}

#[test]
fn a_tiny_working_limit_refuses_admission() {
    let d = doc(vec![base(), raster("Top", (5, 3), BlendMode::Normal, 1.0)]);
    let message = region(&d, (0, 0, W, H), 1, 1000).unwrap_err();
    assert!(message.contains("working limit is 1000 bytes"), "{message}");
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let mut err: *mut c_char = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_render_region(&d, 0, 0, W, H, 1, LIMIT, 0, control, &mut err) };
    assert!(out.is_null());
    assert_eq!(take(err), "operation cancelled");
    assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
    unsafe { dr_job_control_free(control) };
}

#[test]
fn the_float_fixture_matches_its_projection_bit_for_bit() {
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return;
    }
    let fixture = PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).unwrap();
    let added = edit(
        &fixture,
        json!({"op":"add_layer","name":"New","anchor":0,"position":"above"}),
    );
    let exposed = added
        .add_adjustment(
            "Exposure",
            &op("exposure", json!({"exposure":0.4,"offset":0.0,"gamma":1.0})),
            0.7,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .expect("an adjustment");
    assert_ne!(
        full(&exposed),
        full(&added),
        "the exposure changes the projection"
    );
    for d in [&fixture, &added, &exposed] {
        let p = plan(d, (0, 0, 4, 4), 1).unwrap();
        assert_eq!(p["depth"], "rgba32f");
        assert_regions_match(d, "float");
    }
}

#[test]
fn null_inputs_return_null_without_an_error() {
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe {
        dr_precision_doc_render_region(ptr::null(), 0, 0, 1, 1, 1, LIMIT, 0, ptr::null(), &mut err)
    }
    .is_null());
    assert!(err.is_null());
}
