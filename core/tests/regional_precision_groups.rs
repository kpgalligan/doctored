//! Phase 13A-3a: the 16-bit and 32-bit float regional render over GROUPS —
//! isolated and pass-through, plain and gated, nested, as lone entries, clip
//! bases, clip members and baseless clipped entries, hidden and empty, with
//! typed Blend If on an isolated group — against the full-frame typed
//! projection. The oracle is INDEPENDENT of the regional code, exactly as in
//! `regional_precision_masks_clips.rs`: `dr_precision_doc_render`'s
//! projection read back through `dr_precision_image_pixels_rgba16` /
//! `to_rgba32`, cropped and top-left-cell sampled here, compared as u16 codes /
//! f32 bit patterns.

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
use doctored_core::precision_mask_edit::MaskEdit;
use doctored_core::style::{BlendIf, BlendIfChannel, LayerStyle};
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
) -> Result<(u32, u32, Vec<u32>), String> {
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe {
        dr_precision_doc_render_region(doc, x, y, w, h, d, LIMIT, 0, ptr::null(), &mut err)
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

fn assert_regions_match_with(doc: &PrecisionDocument, what: &str, extra: &[(u32, u32, u32, u32)]) {
    let reference = full(doc);
    let (cw, ch) = doc.dimensions();
    let mut windows = vec![
        (0, 0, cw, ch),
        (3, 2, 11, 7),
        (0, 0, 5, 4),
        (cw - 6, 0, 6, 5),
        (0, ch - 3, 7, 3),
        (cw - 4, ch - 5, 4, 5),
        (cw - 1, ch - 1, 1, 1),
        (0, 0, 1, 1),
    ];
    windows.extend_from_slice(extra);
    for window in windows {
        for d in [1, 2, 3, 7, u32::MAX] {
            let got =
                region(doc, window, d).unwrap_or_else(|e| panic!("{what}: {window:?}/{d}: {e}"));
            assert_eq!(
                got,
                expected(&reference, window, d),
                "{what}: window {window:?} divisor {d}"
            );
        }
    }
}

fn assert_regions_match(doc: &PrecisionDocument, what: &str) {
    assert_regions_match_with(doc, what, &[]);
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

fn adjustment(name: &str, params: Value) -> Layer {
    Layer::adjustment(name, &op(name, params), 1.0, true, &profile()).unwrap()
}

fn hue() -> Layer {
    adjustment("hue_saturation", json!({"hue":-60,"saturation":0.2}))
}

fn levels() -> Layer {
    adjustment("levels", json!({"black":0.1,"gamma":1.6}))
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

/// A u16 gradient coverage plane with a hard hole.
fn gradient(w: u32, h: u32) -> Vec<u16> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if x == w / 2 && y == h / 2 {
                0
            } else {
                ((x * 65535 / (w - 1).max(1)) ^ (y * 2311)) as u16
            }
        })
        .collect()
}

fn mask(w: u32, h: u32, offset: (i32, i32), density: f32, feather: f32) -> PrecisionMask {
    let mut properties = MaskProperties::at(offset);
    properties.density = density;
    properties.feather = feather;
    PrecisionMask::new(
        PrecisionPlane::from_u16(w, h, gradient(w, h), None).unwrap(),
        properties,
        true,
    )
    .unwrap()
}

fn around(offset: (i32, i32), w: u32, h: u32) -> Vec<(u32, u32, u32, u32)> {
    let clamp = |v: i64, hi: u32| v.clamp(0, i64::from(hi) - 1) as u32;
    let mut out = Vec::new();
    for (cx, cy) in [
        (i64::from(offset.0), i64::from(offset.1)),
        (i64::from(offset.0) + i64::from(w), i64::from(offset.1)),
        (i64::from(offset.0), i64::from(offset.1) + i64::from(h)),
        (
            i64::from(offset.0) + i64::from(w),
            i64::from(offset.1) + i64::from(h),
        ),
    ] {
        let x = clamp(cx - 3, W);
        let y = clamp(cy - 2, H);
        out.push((x, y, (W - x).min(6), (H - y).min(5)));
    }
    out
}

fn blend_if() -> BlendIf {
    BlendIf {
        channel: BlendIfChannel::Gray,
        this_layer: [10, 60, 180, 240],
        underlying: [0, 40, 200, 255],
        ..BlendIf::default()
    }
}

/// The four children every flat group case holds: raster A Multiply 0.85 at
/// (3, 2), raster B Screen 0.8 at (9, 5), a hue/saturation and a levels
/// adjustment.
fn children() -> Vec<Layer> {
    vec![
        raster("A", (3, 2), BlendMode::Multiply, 0.85),
        raster("B", (9, 5), BlendMode::Screen, 0.8),
        hue(),
        levels(),
    ]
}

fn column<'a>(p: &'a Value, key: &str) -> Vec<&'a Value> {
    p["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| &l[key])
        .collect()
}

fn rows(p: &Value) -> Vec<u64> {
    column(p, "index")
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect()
}

fn sample_at(image: &(u32, u32, Vec<u32>), x: u32, y: u32) -> &[u32] {
    let at = ((y * image.0 + x) * 4) as usize;
    &image.2[at..at + 4]
}

// ----------------------------------------------------- pass through --

#[test]
fn pass_through_groups_plain_and_gated_match_at_u16() {
    let pass = |opacity: f32| Layer::pass_through("Pass", children(), opacity, true).unwrap();
    let plain = doc(vec![base(), pass(1.0)]);
    // The adjustment children reach the backdrop BELOW the group: a pixel
    // outside both rasters moves, which isolating the group would stop.
    let isolated = doc(vec![
        base(),
        Layer::isolated_normal("Iso", children(), 1.0, true).unwrap(),
    ]);
    let untouched = full(&doc(vec![base()]));
    assert_ne!(
        sample_at(&full(&plain), 1, 14),
        sample_at(&untouched, 1, 14)
    );
    assert_eq!(
        sample_at(&full(&isolated), 1, 14),
        sample_at(&untouched, 1, 14),
        "an isolated group's adjustments reach only its own pixels"
    );
    assert_regions_match(&plain, "plain pass-through");
    assert_regions_match(&doc(vec![base(), pass(0.5)]), "pass-through at 0.5");
    let gated = pass(1.0).with_mask(Some(mask(20, 13, (2, 1), 0.5, 6.0)));
    let d = doc(vec![base(), gated]);
    assert_regions_match_with(&d, "pass-through masked", &around((2, 1), 20, 13));
    let both = pass(0.5).with_mask(Some(mask(20, 13, (2, 1), 0.5, 6.0)));
    let d = doc(vec![base(), both]);
    assert_regions_match_with(&d, "pass-through masked at 0.5", &around((2, 1), 20, 13));
    let p = plan(&d, (0, 0, W, H), 1).unwrap();
    assert_eq!(rows(&p), [0, 1, 2, 3, 4, 5]);
    let row = &p["layers"][5];
    assert_eq!(row["kind"], "group");
    assert_eq!(row["operator"], "pass_through");
    assert_eq!(row["mask"], true);
    assert_eq!(row["neighborhood"]["kind"], "spatial");
    assert_eq!(row["group"], Value::Null);
    for child in 1..5 {
        assert_eq!(p["layers"][child]["group"], 5, "child {child}");
    }
    assert_eq!(p["group_depth"], 1);
}

// ---------------------------------------------------------- isolated --

#[test]
fn isolated_groups_in_three_modes_match_at_u16() {
    for mode in [BlendMode::Normal, BlendMode::Multiply, BlendMode::Dissolve] {
        for opacity in [1.0, 0.7] {
            let g = Layer::isolated_normal("Iso", children(), opacity, true)
                .unwrap()
                .with_blend_mode(mode)
                .unwrap();
            let d = doc(vec![base(), g]);
            assert_regions_match(&d, &format!("isolated {mode:?} {opacity}"));
            assert_eq!(
                plan(&d, (0, 0, 4, 4), 1).unwrap()["layers"][5]["operator"],
                "isolated"
            );
        }
    }
    for feather in [0.0, 6.0] {
        let g = Layer::isolated_normal("Iso", children(), 0.7, true)
            .unwrap()
            .with_mask(Some(mask(18, 12, (4, 3), 0.8, feather)));
        assert_regions_match_with(
            &doc(vec![base(), g]),
            &format!("masked isolated feather {feather}"),
            &around((4, 3), 18, 12),
        );
    }
    // Blend If on the isolated group, lone and masked.
    let weighted = Layer::isolated_normal("Iso", children(), 0.9, true)
        .unwrap()
        .with_blend_if(Some(blend_if()))
        .unwrap();
    let d = doc(vec![base(), weighted.clone()]);
    assert_regions_match(&d, "isolated Blend If");
    assert_eq!(
        plan(&d, (0, 0, W, H), 1).unwrap()["layers"][5]["blend_if"],
        true
    );
    let d = doc(vec![
        base(),
        weighted.with_mask(Some(mask(18, 12, (4, 3), 0.8, 0.0))),
    ]);
    assert_regions_match_with(&d, "masked isolated Blend If", &around((4, 3), 18, 12));
    // A masked child.
    let mut kids = children();
    kids[1] = kids[1]
        .clone()
        .with_mask(Some(mask(13, 9, (9, 5), 0.6, 6.0)));
    let g = Layer::isolated_normal("Iso", kids, 1.0, true).unwrap();
    assert_regions_match_with(
        &doc(vec![base(), g]),
        "masked child",
        &around((9, 5), 13, 9),
    );
}

// ------------------------------------------------------------ nesting --

/// Inner (Multiply, isolated) ⊂ Middle (Pass Through, 0.6) ⊂ Outer (Normal,
/// isolated, 0.9, masked): postorder Base 0, A 1, B 2, Inner 3, hue 4,
/// Middle 5, levels 6, Outer 7.
fn nested() -> PrecisionDocument {
    let inner = Layer::isolated_normal(
        "Inner",
        vec![
            raster("A", (3, 2), BlendMode::Multiply, 0.85),
            raster("B", (9, 5), BlendMode::Screen, 0.8),
        ],
        1.0,
        true,
    )
    .unwrap()
    .with_blend_mode(BlendMode::Multiply)
    .unwrap();
    let middle = Layer::pass_through("Middle", vec![inner, hue()], 0.6, true).unwrap();
    let outer = Layer::isolated_normal("Outer", vec![middle, levels()], 0.9, true)
        .unwrap()
        .with_mask(Some(mask(18, 12, (4, 3), 0.8, 0.0)));
    doc(vec![base(), outer])
}

#[test]
fn groups_nested_three_deep_mixing_kinds_match_and_plan_their_rows_at_u16() {
    let d = nested();
    assert_regions_match_with(&d, "nested three deep", &around((4, 3), 18, 12));
    let window = (4, 3, 9, 7);
    let p = plan(&d, window, 1).unwrap();
    assert_eq!(rows(&p), [0, 1, 2, 3, 4, 5, 6, 7]);
    let groups: Vec<Option<u64>> = column(&p, "group").iter().map(|v| v.as_u64()).collect();
    assert_eq!(
        groups,
        [
            None,
            Some(3),
            Some(3),
            Some(5),
            Some(5),
            Some(7),
            Some(7),
            None
        ]
    );
    let operators: Vec<&str> = column(&p, "operator")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        operators,
        [
            "composite",
            "composite",
            "composite",
            "isolated",
            "hue_saturation",
            "pass_through",
            "levels",
            "isolated"
        ]
    );
    assert_eq!(p["group_depth"], 3);
    let pixels = 9u64 * 7;
    assert_eq!(p["work_units"], pixels * (8 + 1));
    assert_eq!(
        p["scratch_heap_bytes"],
        pixels * 16 + pixels * 8 + 1024 * 1024
    );
}

/// The scratch is Σ over EVERY contributing mask (all prepared before the
/// loop, since 13A-3a), not the largest unit's: a feathered group mask and a
/// feathered child mask are both charged.
#[test]
fn the_typed_plan_charges_every_contributing_mask() {
    let child = raster("Child", (4, 3), BlendMode::Normal, 1.0).with_mask(Some(mask(
        12,
        9,
        (7, 4),
        0.8,
        6.0,
    )));
    let g = Layer::pass_through("G", vec![child], 1.0, true)
        .unwrap()
        .with_mask(Some(mask(12, 9, (7, 4), 0.8, 6.0)));
    let d = doc(vec![base(), g]);
    assert_regions_match_with(&d, "two feathered masks", &around((7, 4), 12, 9));
    let window = (1, 2, 6, 4);
    let p = plan(&d, window, 1).unwrap();
    let halo = p["neighborhood"]["halo_pixels"].as_u64().unwrap();
    // `plane_blur::float_plan` restated as in `regional_precision_masks_clips`:
    // feather 6 → 13 taps over the padded extent.
    let plane = (12 + 2 * halo) * (9 + 2 * halo);
    let taps = 2 * 6 + 1;
    let pixels = 6u64 * 4;
    assert_eq!(
        p["scratch_heap_bytes"],
        pixels * 16 + pixels * 8 + 1024 * 1024 + 2 * (plane * 32 + taps * 4 + 4096)
    );
    assert_eq!(
        p["work_units"],
        pixels * (3 + 1) + 2 * (plane * 32 + plane * 2 * taps)
    );
}

// --------------------------------------------------------- clip units --

#[test]
fn a_group_clip_base_isolates_for_a_visible_member_at_u16() {
    let g = || {
        Layer::pass_through(
            "G",
            vec![raster("A", (3, 2), BlendMode::Normal, 0.9), hue()],
            1.0,
            true,
        )
        .unwrap()
    };
    let c = raster("C", (8, 6), BlendMode::Multiply, 0.8).with_clipped(true);
    let d = doc(vec![base(), g(), c]);
    assert_regions_match(&d, "group clip base");
    let p = plan(&d, (0, 0, W, H), 1).unwrap();
    assert_eq!(rows(&p), [0, 1, 2, 3, 4]);
    assert_eq!(p["layers"][3]["operator"], "isolated");
    assert_eq!(p["layers"][4]["clip_base"], 3);
    // A GROUP member, Pass Through: a member always composites isolated.
    let member = |visible: bool| {
        Layer::pass_through(
            "M",
            vec![raster("C", (8, 6), BlendMode::Normal, 1.0), levels()],
            1.0,
            visible,
        )
        .unwrap()
        .with_clipped(true)
    };
    let d = doc(vec![base(), g(), member(true)]);
    assert_regions_match(&d, "group member");
    let p = plan(&d, (0, 0, W, H), 1).unwrap();
    assert_eq!(rows(&p), [0, 1, 2, 3, 4, 5, 6]);
    assert_eq!(p["layers"][6]["operator"], "isolated");
    assert_eq!(p["layers"][6]["clip_base"], 3);
    assert_eq!(p["layers"][4]["group"], 6);
    // The member hidden: the base returns to Pass Through.
    let d = doc(vec![base(), g(), member(false)]);
    assert_regions_match(&d, "hidden group member");
    let p = plan(&d, (0, 0, W, H), 1).unwrap();
    assert_eq!(rows(&p), [0, 1, 2, 3]);
    assert_eq!(p["layers"][3]["operator"], "pass_through");
}

/// The objection's fixture at u16: an opaque backdrop, a Pass Through group
/// P holding an Invert adjustment, and a HIDDEN raster clipped to P. The
/// scene's `clipped_members` is empty, so P stays pass-through and the
/// backdrop IS inverted.
#[test]
fn a_pass_through_base_whose_only_member_is_hidden_keeps_pass_through_at_u16() {
    let p_group = |opacity: f32| {
        Layer::pass_through("P", vec![adjustment("invert", json!({}))], opacity, true).unwrap()
    };
    let hidden = Layer::raster("Hidden", image16(13, 9, 5, true), (8, 6), 1.0, false)
        .unwrap()
        .with_clipped(true);
    let backdrop = full(&doc(vec![base()]));
    let d = doc(vec![base(), p_group(1.0), hidden.clone()]);
    let projection = full(&d);
    for (got, was) in projection.2.chunks(4).zip(backdrop.2.chunks(4)) {
        for c in 0..3 {
            assert_eq!(got[c], 65535 - was[c], "inverted");
        }
        assert_eq!(got[3], 65535);
    }
    assert_regions_match(&d, "hidden-only member, opacity 1");
    let p = plan(&d, (0, 0, W, H), 1).unwrap();
    assert_eq!(rows(&p), [0, 1, 2]);
    assert_eq!(p["layers"][2]["operator"], "pass_through");
    let d = doc(vec![base(), p_group(0.5), hidden]);
    // The group's 50 % gates at its byte, 128 / 255 (#578): the
    // inverse mixed in at `q` is `x + (1 − 2x)·q`, no longer exactly a half.
    let q = crate::common::byte_unit(0.5);
    for (got, was) in full(&d).2.chunks(4).zip(backdrop.2.chunks(4)) {
        for c in 0..3 {
            let x = f64::from(was[c]) / 65535.0;
            let want = (x + (1.0 - 2.0 * x) * q) * 65535.0;
            assert!(
                (f64::from(got[c]) - want).abs() <= 1.0,
                "half inverted: {} vs {want}",
                got[c]
            );
        }
    }
    assert_regions_match(&d, "hidden-only member, opacity 0.5");
}

#[test]
fn baseless_clipped_groups_composite_as_if_unclipped_at_u16() {
    let bottom = || {
        Layer::pass_through("Bottom", vec![base(), hue()], 1.0, true)
            .unwrap()
            .with_clipped(true)
    };
    let top = || raster("Top", (8, 6), BlendMode::Screen, 0.8);
    let d = doc(vec![bottom(), top()]);
    assert_regions_match(&d, "baseless root group");
    let p = plan(&d, (0, 0, W, H), 1).unwrap();
    assert_eq!(p["layers"][2]["operator"], "pass_through");
    assert_eq!(p["layers"][2]["clip_base"], Value::Null);
    let outer = Layer::isolated_normal("Outer", vec![bottom(), top()], 1.0, true).unwrap();
    let d = doc(vec![outer]);
    assert_regions_match(&d, "baseless group inside a group");
    let p = plan(&d, (0, 0, W, H), 1).unwrap();
    assert_eq!(p["layers"][2]["operator"], "pass_through");
    assert_eq!(p["layers"][2]["group"], 4);
}

// ---------------------------------------------------- hidden, empty --

#[test]
fn hidden_and_empty_groups_match_at_u16() {
    let style = LayerStyle::from_json(
        "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}]}",
    )
    .unwrap();
    let mut kids = children();
    kids[0] = kids[0]
        .clone()
        .with_layer_style(Some(&style), (W, H))
        .unwrap();
    // Visible, the styled child is admitted since 13A-3b-i…
    let visible = Layer::isolated_normal("G", kids.clone(), 1.0, true).unwrap();
    assert_regions_match(&doc(vec![base(), visible]), "visible group, styled child");
    // …hidden, the subtree is skipped unchecked.
    let hidden = Layer::isolated_normal("G", kids, 1.0, false).unwrap();
    let d = doc(vec![base(), hidden, levels()]);
    assert_regions_match(&d, "hidden group, styled child");
    let p = plan(&d, (0, 0, W, H), 1).unwrap();
    assert_eq!(rows(&p), [0, 6]);
    assert_eq!(p["group_depth"], 0);
    // Empty groups, isolated and pass-through, lone, as a clip base with a
    // visible member and as a clip member.
    let empty_iso = || Layer::isolated_normal("Empty", vec![], 1.0, true).unwrap();
    let empty_pass = || Layer::pass_through("Empty", vec![], 1.0, true).unwrap();
    assert_regions_match(&doc(vec![base(), empty_iso()]), "empty isolated");
    assert_regions_match(&doc(vec![base(), empty_pass()]), "empty pass-through");
    let member = raster("C", (8, 6), BlendMode::Normal, 1.0).with_clipped(true);
    let d = doc(vec![base(), empty_pass(), member]);
    assert_regions_match(&d, "empty clip base");
    let d = doc(vec![
        base(),
        raster("B", (3, 2), BlendMode::Normal, 1.0),
        empty_pass().with_clipped(true),
    ]);
    assert_regions_match(&d, "empty clip member");
}

// ------------------------------------------------------- linear light --

#[test]
fn a_linear_light_u16_document_with_a_nested_multiply_group_matches() {
    let inner = Layer::isolated_normal(
        "Inner",
        vec![
            raster("A", (3, 2), BlendMode::Multiply, 0.85),
            raster("B", (9, 5), BlendMode::Screen, 0.8),
        ],
        1.0,
        true,
    )
    .unwrap()
    .with_blend_mode(BlendMode::Multiply)
    .unwrap();
    let outer = Layer::pass_through("Outer", vec![inner, levels()], 1.0, true).unwrap();
    let d = doc(vec![base(), outer]);
    let linear = d
        .with_evaluation(Evaluation::LinearLight)
        .unwrap()
        .expect("a policy change");
    assert_ne!(full(&linear), full(&d), "the policy changes the projection");
    assert_regions_match(&linear, "linear light groups");
}

// ------------------------------------------------------------ refusals --

#[test]
fn guided_children_and_styled_groups_are_admitted_at_u16() {
    // A fill-only style on a group is admitted since 13A-3b-i…
    let fill = LayerStyle::from_json("{\"fill_opacity\":0.5}").unwrap();
    let styled = Layer::isolated_normal("Group", children(), 1.0, true)
        .unwrap()
        .with_layer_style(Some(&fill), (W, H))
        .unwrap();
    assert_regions_match(&doc(vec![base(), styled]), "a fill-only group");
    // …and a group WITH an enabled effect since 13A-3b-ii (the materialized
    // group).
    let effects = LayerStyle::from_json(
        "{\"fill_opacity\":0.5,\"effects\":[{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}]}",
    )
    .unwrap();
    let styled = Layer::isolated_normal("Group", children(), 1.0, true)
        .unwrap()
        .with_layer_style(Some(&effects), (W, H))
        .unwrap();
    assert_regions_match(&doc(vec![base(), styled]), "a group with effects");
    // A guided Shadows/Highlights inside a Pass Through group: since
    // 13A-4b-ii-a staged by the root level.
    let guided = adjustment(
        "shadows_highlights",
        json!({"shadows":{"amount":0.6,"tone":0.4},"radius":12}),
    );
    let g = Layer::pass_through("Group", vec![guided], 1.0, true).unwrap();
    assert_regions_match(
        &doc(vec![base(), g]),
        "a guided child of a Pass Through group",
    );
}

// -------------------------------------------------------- cancellation --

#[test]
fn a_pre_cancelled_grouped_render_returns_no_output_at_u16() {
    let d = nested();
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

// ---------------------------------------------------------------- float --

#[test]
fn the_float_fixture_grouped_matches_bit_for_bit() {
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return;
    }
    let fixture = PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).unwrap();
    let added = edit(
        &fixture,
        json!({"op":"add_layer","name":"New","anchor":0,"position":"above"}),
    );
    let all: Vec<usize> = (0..added.layer_count()).collect();
    // A float document mints the Pass Through container the request asks
    // for; the Multiply step below isolates it.
    let grouped = edit(
        &added,
        json!({"op":"group_layers","layers":all,"name":"Group","pass_through":true}),
    );
    let top = grouped.layer_count() - 1;
    let p = plan(&grouped, (0, 0, 4, 4), 1).unwrap();
    assert_eq!(p["depth"], "rgba32f");
    assert_eq!(p["group_depth"], 1);
    assert_eq!(p["layers"][top]["kind"], "group");
    assert_eq!(p["layers"][top]["operator"], "pass_through");
    assert_regions_match(&grouped, "float pass-through group");
    // Multiply, then masked (feather 3, density 0.8).
    let multiply = edit(
        &grouped,
        json!({"op":"layer_blend","version":1,"layer":top,"blend_mode":BlendMode::Multiply.to_c()}),
    );
    assert_regions_match(&multiply, "float multiply group");
    let (fw, fh) = multiply.dimensions();
    let (mw, mh) = (fw / 2 + 1, fh / 2 + 1);
    let masked = multiply
        .mask_edit(
            &MaskEdit::parse(
                &json!({"op":"set_mask","layer":top,"width":mw,"height":mh,"x":2,"y":1,
                        "density":0.8,"feather":3.0})
                .to_string(),
            )
            .unwrap(),
            &gradient(mw, mh),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .expect("a mask");
    assert_regions_match(&masked, "float masked multiply group");
    // Nested once more.
    let nested = edit(
        &masked,
        json!({"op":"group_layers","layers":[top],"name":"Outer","pass_through":false}),
    );
    assert_eq!(plan(&nested, (0, 0, 4, 4), 1).unwrap()["group_depth"], 2);
    assert_regions_match(&nested, "float nested groups");
}
