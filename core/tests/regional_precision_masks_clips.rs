//! Phase 13A-2a: the 16-bit and 32-bit float regional render over LAYER
//! MASKS, CLIP UNITS and typed BLEND IF, against the full-frame typed
//! projection. The oracle is INDEPENDENT of the regional code, exactly as in
//! `regional_precision.rs`: `dr_precision_doc_render`'s projection read back
//! through `dr_precision_image_pixels_rgba16` / `to_rgba32`, cropped and
//! top-left-cell sampled here, compared as u16 codes / f32 bit patterns.

use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

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

use crate::common;
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

fn adjustment(name: &str, params: Value, mode: BlendMode, opacity: f32) -> Layer {
    Layer::adjustment(name, &op(name, params), opacity, true, &profile())
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

#[allow(clippy::too_many_arguments)]
fn mask(
    w: u32,
    h: u32,
    values: Vec<u16>,
    offset: (i32, i32),
    density: f32,
    feather: f32,
    outside: u8,
    enabled: bool,
) -> PrecisionMask {
    let mut properties = MaskProperties::at(offset);
    properties.density = density;
    properties.feather = feather;
    properties.outside_coverage = outside;
    PrecisionMask::new(
        PrecisionPlane::from_u16(w, h, values, None).unwrap(),
        properties,
        enabled,
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

// ------------------------------------------------------------- masks --

/// A case label, its mask, the mask's offset and its dimensions.
type MaskCase = (String, PrecisionMask, (i32, i32), (u32, u32));

#[test]
fn reveal_hide_gradient_offset_density_and_feather_masks_match_at_u16() {
    let cases: Vec<MaskCase> = vec![
        (
            "reveal".into(),
            mask(13, 9, vec![65535; 117], (4, 3), 1.0, 0.0, 0, true),
            (4, 3),
            (13, 9),
        ),
        (
            "hide".into(),
            mask(13, 9, vec![0; 117], (4, 3), 1.0, 0.0, 0, true),
            (4, 3),
            (13, 9),
        ),
        (
            "gradient".into(),
            mask(13, 9, gradient(13, 9), (4, 3), 1.0, 0.0, 0, true),
            (4, 3),
            (13, 9),
        ),
    ];
    let mut cases = cases;
    for outside in [0u8, 255] {
        for offset in [(-4, -3), (12, 8), (16, -2)] {
            cases.push((
                format!("offset {offset:?} outside {outside}"),
                mask(11, 9, gradient(11, 9), offset, 1.0, 0.0, outside, true),
                offset,
                (11, 9),
            ));
        }
        cases.push((
            format!("density 0.5 outside {outside}"),
            mask(11, 9, gradient(11, 9), (6, 2), 0.5, 0.0, outside, true),
            (6, 2),
            (11, 9),
        ));
        for feather in [6.0, 40.0] {
            cases.push((
                format!("feather {feather} outside {outside}"),
                mask(12, 9, gradient(12, 9), (7, 4), 0.8, feather, outside, true),
                (7, 4),
                (12, 9),
            ));
        }
    }
    for (what, m, offset, (mw, mh)) in cases {
        let layer = raster("Masked", (4, 3), BlendMode::Multiply, 0.8).with_mask(Some(m));
        let d = doc(vec![base(), layer]);
        assert_regions_match_with(&d, &what, &around(offset, mw, mh));
        assert_eq!(
            plan(&d, (0, 0, 4, 4), 1).unwrap()["layers"][1]["mask"],
            true
        );
    }
}

#[test]
fn a_disabled_mask_and_a_masked_adjustment_match_at_u16() {
    let disabled = raster("Masked", (4, 3), BlendMode::Normal, 1.0).with_mask(Some(mask(
        12,
        9,
        gradient(12, 9),
        (7, 4),
        0.5,
        6.0,
        0,
        false,
    )));
    let d = doc(vec![base(), disabled]);
    assert_regions_match(&d, "disabled");
    let p = plan(&d, (0, 0, W, H), 1).unwrap();
    assert_eq!(p["layers"][1]["mask"], false);
    assert_eq!(p["neighborhood"]["kind"], "pointwise");
    for mode in [BlendMode::Normal, BlendMode::Multiply, BlendMode::Dissolve] {
        for feather in [0.0, 6.0] {
            let adjusted = adjustment(
                "hue_saturation",
                json!({"hue":50,"saturation":0.4}),
                mode,
                0.8,
            )
            .with_mask(Some(mask(
                11,
                8,
                gradient(11, 8),
                (4, 6),
                0.7,
                feather,
                0,
                true,
            )));
            let d = doc(vec![
                base(),
                raster("Top", (5, 3), BlendMode::Overlay, 0.8),
                adjusted,
            ]);
            assert_regions_match_with(
                &d,
                &format!("masked adjustment {mode:?} {feather}"),
                &around((4, 6), 11, 8),
            );
        }
    }
}

// ------------------------------------------------------------ clip units --

/// Base, a Multiply base at (3, 2), a raster Multiply 0.6 member and a levels
/// adjustment member.
fn clip_layers() -> Vec<Layer> {
    vec![
        base(),
        raster("ClipBase", (3, 2), BlendMode::Multiply, 0.85),
        raster("Member", (8, 5), BlendMode::Multiply, 0.6).with_clipped(true),
        adjustment(
            "levels",
            json!({"black":0.1,"gamma":1.6}),
            BlendMode::Normal,
            0.8,
        )
        .with_clipped(true),
    ]
}

#[test]
fn clip_units_match_at_u16() {
    let d = doc(clip_layers());
    assert_regions_match(&d, "clip unit");
    let p = plan(&d, (0, 0, W, H), 1).unwrap();
    let bases: Vec<_> = p["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| (l["index"].clone(), l["clip_base"].clone()))
        .collect();
    assert_eq!(
        bases,
        [
            (json!(0), Value::Null),
            (json!(1), Value::Null),
            (json!(2), json!(1)),
            (json!(3), json!(1)),
        ]
    );

    let mut hidden = clip_layers();
    hidden[2] = Layer::raster("Member", image16(13, 9, 5, true), (8, 5), 0.6, false)
        .unwrap()
        .with_blend_mode(BlendMode::Multiply)
        .unwrap()
        .with_clipped(true);
    assert_regions_match(&doc(hidden), "hidden member");

    let mut zero = clip_layers();
    zero[2] = raster("Member", (8, 5), BlendMode::Multiply, 0.0).with_clipped(true);
    let zero = doc(zero);
    assert_regions_match(&zero, "zero-opacity member");
    // The typed scene keeps a zero-opacity record.
    assert_eq!(
        plan(&zero, (0, 0, W, H), 1).unwrap()["layers"]
            .as_array()
            .unwrap()
            .len(),
        4
    );

    let mut hidden_base = clip_layers();
    hidden_base[1] = Layer::raster("ClipBase", image16(13, 9, 5, true), (3, 2), 0.85, false)
        .unwrap()
        .with_blend_mode(BlendMode::Multiply)
        .unwrap();
    let hidden_base = doc(hidden_base);
    assert_regions_match(&hidden_base, "hidden base");
    assert_eq!(
        plan(&hidden_base, (0, 0, W, H), 1).unwrap()["layers"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let mut dissolve = clip_layers();
    dissolve[1] = raster("ClipBase", (3, 2), BlendMode::Dissolve, 0.55);
    assert_regions_match(&doc(dissolve), "dissolve base");

    let mut masked = clip_layers();
    masked[1] = masked[1].clone().with_mask(Some(mask(
        13,
        9,
        gradient(13, 9),
        (3, 2),
        1.0,
        0.0,
        0,
        true,
    )));
    masked[2] = masked[2].clone().with_mask(Some(mask(
        10,
        12,
        gradient(10, 12),
        (10, 4),
        0.6,
        6.0,
        255,
        true,
    )));
    masked[3] =
        masked[3]
            .clone()
            .with_mask(Some(mask(8, 8, gradient(8, 8), (1, 1), 1.0, 0.0, 0, true)));
    assert_regions_match_with(
        &doc(masked),
        "masked base + members",
        &around((10, 4), 10, 12),
    );
}

#[test]
fn an_adjustment_base_with_visible_members_contributes_nothing_at_u16() {
    let d = doc(vec![
        base(),
        adjustment("invert", json!({}), BlendMode::Normal, 1.0),
        raster("Member", (5, 3), BlendMode::Normal, 1.0).with_clipped(true),
    ]);
    assert_eq!(full(&d), full(&doc(vec![base()])));
    assert_regions_match(&d, "adjustment base");
}

#[test]
fn an_adjustment_base_whose_only_member_is_hidden_applies_at_u16() {
    // The typed builder's `clipped_members` holds only VISIBLE records, so
    // this run is EMPTY and the Invert takes the lone arm and APPLIES — the
    // stated depth difference from 8 bits, where it contributes nothing.
    let member = Layer::raster("Member", image16(13, 9, 5, true), (5, 3), 1.0, false)
        .unwrap()
        .with_clipped(true);
    let d = doc(vec![
        base(),
        adjustment("invert", json!({}), BlendMode::Normal, 1.0),
        member,
    ]);
    assert_ne!(full(&d), full(&doc(vec![base()])), "the Invert applies");
    assert_regions_match(&d, "hidden-only run");
    let p = plan(&d, (0, 0, W, H), 1).unwrap();
    let rows = p["layers"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "the base and no member rows");
    assert_eq!(rows[1]["kind"], "adjustment");
    assert_eq!(rows[1]["clip_base"], Value::Null);
}

#[test]
fn baseless_clipped_entries_are_each_their_own_unit_at_u16() {
    let small = Layer::raster("Small", image16(9, 7, 3, false), (0, 0), 1.0, true)
        .unwrap()
        .with_clipped(true);
    let wide = Layer::raster("Wide", image16(20, 14, 4, true), (4, 3), 0.9, true)
        .unwrap()
        .with_blend_mode(BlendMode::Screen)
        .unwrap()
        .with_clipped(true);
    let d = doc(vec![
        small,
        wide,
        raster("Top", (2, 2), BlendMode::Normal, 0.7).with_clipped(true),
    ]);
    assert_regions_match(&d, "baseless");
    let p = plan(&d, (0, 0, W, H), 1).unwrap();
    for row in p["layers"].as_array().unwrap() {
        assert_eq!(row["clip_base"], Value::Null, "{row}");
    }
    let (_, _, px) = region(&d, (15, 10, 1, 1), 1).unwrap();
    assert!(px[3] > 0, "entry 1 is visible outside entry 0's footprint");
}

// ------------------------------------------------------------- Blend If --

#[test]
fn typed_blend_if_on_a_raster_a_clip_base_and_a_member_matches() {
    let lone = raster("Weighted", (3, 2), BlendMode::Screen, 0.9)
        .with_blend_if(Some(blend_if()))
        .unwrap();
    let d = doc(vec![base(), lone]);
    assert_regions_match(&d, "lone Blend If");
    assert_eq!(
        plan(&d, (0, 0, 4, 4), 1).unwrap()["layers"][1]["blend_if"],
        true
    );

    let mut on_base = clip_layers();
    on_base[1] = on_base[1].clone().with_blend_if(Some(blend_if())).unwrap();
    let on_base = doc(on_base);
    assert_regions_match(&on_base, "Blend If on a clip base");
    assert_eq!(
        plan(&on_base, (0, 0, 4, 4), 1).unwrap()["layers"][1]["blend_if"],
        true
    );

    let mut on_member = clip_layers();
    on_member[2] = on_member[2]
        .clone()
        .with_blend_if(Some(BlendIf {
            channel: BlendIfChannel::Red,
            ..blend_if()
        }))
        .unwrap();
    assert_regions_match(&doc(on_member), "Blend If on a member");

    let mut masked_base = clip_layers();
    masked_base[1] = masked_base[1]
        .clone()
        .with_blend_if(Some(blend_if()))
        .unwrap()
        .with_mask(Some(mask(
            13,
            9,
            gradient(13, 9),
            (3, 2),
            0.7,
            6.0,
            0,
            true,
        )));
    assert_regions_match(&doc(masked_base), "Blend If + mask on a clip base");
}

#[test]
fn a_linear_light_u16_document_with_masks_and_clips_matches() {
    let mut layers = clip_layers();
    layers[2] = layers[2].clone().with_mask(Some(mask(
        10,
        12,
        gradient(10, 12),
        (10, 4),
        0.6,
        6.0,
        0,
        true,
    )));
    // (Blend If is refused by the 11E policy under linear light.)
    let d = doc(layers);
    let linear = d
        .with_evaluation(Evaluation::LinearLight)
        .unwrap()
        .expect("a policy change");
    assert_ne!(full(&linear), full(&d));
    assert_regions_match(&linear, "linear light");
}

// ----------------------------------------------------------------- plan --

#[test]
fn the_typed_plan_declares_masks_halos_and_formulas() {
    let (mw, mh) = (12u32, 9u32);
    let mut layers = clip_layers();
    layers[2] = layers[2].clone().with_mask(Some(mask(
        mw,
        mh,
        gradient(mw, mh),
        (7, 4),
        1.0,
        6.0,
        0,
        true,
    )));
    let d = doc(layers);
    let window = (0, H - 4, 6, 4);
    let p = plan(&d, window, 1).unwrap();
    let rows = p["layers"].as_array().unwrap();
    let halo = rows[2]["neighborhood"]["halo_pixels"].as_u64().unwrap() as u32;
    assert!(halo > 0);
    assert_eq!(rows[2]["mask"], true);
    assert_eq!(rows[2]["clip_base"], 1);
    assert_eq!(
        p["neighborhood"],
        json!({"kind":"spatial","halo_pixels":halo})
    );
    let y0 = (H - 4).saturating_sub(halo);
    let x1 = W.min(6 + halo);
    assert_eq!(
        p["required_input_region"],
        json!({"x":0,"y":y0,"width":x1,"height":H - y0})
    );
    // work = pixels·(rows + 1) + the mask's preparation work; scratch adds the
    // prepared plane of EVERY contributing mask (Σ since 13A-3a — one mask
    // here, so the value is unchanged), both over and above the unmasked plan.
    let q = plan(&doc(clip_layers()), window, 1).unwrap();
    let pixels = 6u64 * 4;
    assert_eq!(q["work_units"], pixels * 5);
    assert_eq!(
        q["scratch_heap_bytes"],
        pixels * 16 + pixels * 8 + 1024 * 1024
    );
    // The feathered mask's prepared plane is its padded extent; feather 6 →
    // sigma 2.3 → downsample factor 1 → `feather_radius` 6.0 → 13 taps
    // (`plane_blur::float_plan`'s formula, restated independently).
    let plane = u64::from(mw + 2 * halo) * u64::from(mh + 2 * halo);
    let taps = 2 * 6 + 1;
    assert_eq!(p["work_units"], pixels * 5 + plane * 32 + plane * 2 * taps);
    assert_eq!(
        p["scratch_heap_bytes"],
        q["scratch_heap_bytes"].as_u64().unwrap() + plane * 32 + taps * 4 + 4096
    );
}

// ------------------------------------------------------------ refusals --

#[test]
fn styled_group_and_knockout_members_are_admitted_at_u16() {
    let style = LayerStyle::from_json(
        "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}]}",
    )
    .unwrap();
    let mut layers = clip_layers();
    layers[2] = layers[2]
        .clone()
        .with_layer_style(Some(&style), (W, H))
        .unwrap();
    // Since 13A-3b-i a styled member with an enabled effect is admitted,
    // and since 13A-3c a knockout member (it punches to the base-only
    // sample).
    assert_regions_match(&doc(layers), "a drop-shadow member");
    let knockout = LayerStyle::from_json("{\"knockout\":\"shallow\"}").unwrap();
    let mut layers = clip_layers();
    layers[2] = layers[2]
        .clone()
        .with_layer_style(Some(&knockout), (W, H))
        .unwrap();
    assert_regions_match(&doc(layers.clone()), "a knockout member");
    // Since 13A-4b-ii-b-1 the unit's guided Shadows/Highlights member beside
    // the knockout member (an unstyled base) is staged too.
    layers[3] = adjustment(
        "shadows_highlights",
        json!({"shadows":{"amount":0.6,"tone":0.4},"radius":12}),
        BlendMode::Normal,
        0.8,
    )
    .with_clipped(true);
    assert_regions_match(&doc(layers), "a guided member beside a knockout member");
    let group = Layer::isolated_normal(
        "Group",
        vec![raster("Child", (0, 0), BlendMode::Normal, 1.0)],
        1.0,
        false,
    )
    .unwrap()
    .with_clipped(true);
    let mut layers = clip_layers();
    layers.insert(2, group);
    // Base 0, ClipBase 1, Child 2, Group 3: since 13A-3a the hidden group
    // member is admitted and equals the reference.
    assert_regions_match(&doc(layers), "a hidden group member");
}

// -------------------------------------------------------- cancellation --

#[test]
fn a_pre_cancelled_masked_clip_render_returns_no_output_at_u16() {
    let mut layers = clip_layers();
    layers[2] = layers[2].clone().with_mask(Some(mask(
        10,
        12,
        gradient(10, 12),
        (10, 4),
        0.6,
        6.0,
        0,
        true,
    )));
    let d = doc(layers);
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

/// The u16 twin of `regional_masks_clips.rs`'s concurrent cancel. With
/// `bench`, a wall-clock record: DECLARED BOUND: a cancel returns within 4096
/// samples (one checkpoint of the unit loop). Measured: the cancel-to-return
/// latency and, from an uncancelled render of the same document, the time
/// per sample; their quotient estimates the samples processed after the
/// cancel, printed for the record (`--nocapture`). Without it, the
/// load-independent default: the first report's checkpoint count comes from
/// a fresh twin's own report log and the worker is cancelled PARKED at the
/// next checkpoint (`cancelled_past_each`), no poller.
fn cancel_masked_clip_render_at_u16(bench: bool) {
    let (w, h) = (3000u32, 2000u32);
    let build = || {
        let mut layers =
            vec![Layer::raster("Base", image16(w, h, 1, false), (0, 0), 1.0, true).unwrap()];
        for i in 0..3u32 {
            layers.push(
                Layer::raster(
                    "Raster",
                    image16(w, h, i + 2, true),
                    (i as i32, 0),
                    0.9,
                    true,
                )
                .unwrap()
                .with_blend_mode(BlendMode::SoftLight)
                .unwrap(),
            );
        }
        layers[2] = layers[2].clone().with_clipped(true);
        layers[3] = layers[3].clone().with_clipped(true);
        // Unfeathered masks at density 0.5: no blur precedes the loop.
        let plane: Vec<u16> = (0..w * h)
            .map(|i| (((i % w) + (i / w)) * 257 % 65536) as u16)
            .collect();
        for idx in [1, 2] {
            layers[idx] = layers[idx].clone().with_mask(Some(mask(
                w,
                h,
                plane.clone(),
                (0, 0),
                0.5,
                0.0,
                0,
                true,
            )));
        }
        PrecisionDocument::from_parts(
            w,
            h,
            profile(),
            Metadata::default(),
            Resolution::default(),
            layers,
        )
        .unwrap()
    };
    let render_at = move |address: usize| {
        move |token: usize| {
            let mut err: *mut c_char = ptr::null_mut();
            let out = unsafe {
                dr_precision_doc_render_region(
                    address as *const PrecisionDocument,
                    0,
                    0,
                    w,
                    h,
                    1,
                    LIMIT,
                    0,
                    token as *const _,
                    &mut err,
                )
            };
            let ok = !out.is_null();
            if ok {
                unsafe { dr_precision_image_free(out) };
            }
            (ok, take(err))
        }
    };
    let d = build();
    let render = render_at(&d as *const PrecisionDocument as usize);
    if bench {
        let started = Instant::now();
        let img = unsafe {
            dr_precision_doc_render_region(
                &d,
                0,
                0,
                w,
                h,
                1,
                LIMIT,
                0,
                ptr::null(),
                ptr::null_mut(),
            )
        };
        let uncancelled = started.elapsed();
        assert!(!img.is_null());
        unsafe { dr_precision_image_free(img) };
        // Unit-samples: one per canvas pixel per unit (background + clip unit).
        let per_sample = uncancelled.as_secs_f64() / (2.0 * f64::from(w) * f64::from(h));
        let control = unsafe { dr_job_control_create() };
        let token = control as usize;
        let latency = thread::scope(|scope| {
            let worker = scope.spawn(move || {
                let (ok, message) = render(token);
                (ok, message, Instant::now())
            });
            let deadline = Instant::now() + Duration::from_secs(60);
            while unsafe { dr_job_control_progress(control) } <= 0.0 && !worker.is_finished() {
                assert!(Instant::now() < deadline, "the render never started");
                thread::sleep(Duration::from_millis(1));
            }
            assert!(
                !worker.is_finished(),
                "render completed before cancellation"
            );
            let cancelled_at = Instant::now();
            unsafe { dr_job_control_cancel(control) };
            let (ok, message, returned_at) = worker.join().unwrap();
            assert!(!ok, "a cancelled render returns no image");
            assert_eq!(message, "operation cancelled");
            returned_at.saturating_duration_since(cancelled_at)
        });
        eprintln!(
            "13A-2b u16 cancel: {:.3} ms cancel-to-return; {:.2} ns per unit-sample \
             (uncancelled {:.1} ms); ≈{:.0} samples after the cancel (declared bound 4096)",
            latency.as_secs_f64() * 1e3,
            per_sample * 1e9,
            uncancelled.as_secs_f64() * 1e3,
            latency.as_secs_f64() / per_sample,
        );
        assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
        unsafe { dr_job_control_free(control) };
    } else {
        let twin = |token: usize| {
            let twin = build();
            render_at(&twin as *const PrecisionDocument as usize)(token)
        };
        let parked = common::probe::cancelled_past_each(&[0.0], twin, render);
        eprintln!(
            "u16 cancel once the render reports: {}",
            common::probe::parked_line(&parked[0])
        );
    }
}

#[test]
fn a_concurrently_cancelled_masked_clip_render_returns_operation_cancelled_at_u16() {
    cancel_masked_clip_render_at_u16(false);
}

#[test]
#[ignore = "make cancel-bench: a wall-clock record"]
fn a_concurrently_cancelled_masked_clip_render_is_measured_at_u16() {
    cancel_masked_clip_render_at_u16(true);
}

// ---------------------------------------------------------------- float --

#[test]
fn the_float_fixture_with_a_mask_and_a_clip_unit_matches_bit_for_bit() {
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return;
    }
    let fixture = PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).unwrap();
    let added = edit(
        &fixture,
        json!({"op":"add_layer","name":"New","anchor":0,"position":"above"}),
    );
    let top = added.layer_count() - 1;
    let (fw, fh) = added.dimensions();
    let (mw, mh) = (fw / 2 + 1, fh / 2 + 1);
    let masked = added
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
    assert_regions_match(&masked, "float + mask");
    let clipped = edit(
        &masked,
        json!({"op":"layer_clipped","layer":top,"clipped":true}),
    );
    let p = plan(&clipped, (0, 0, 4, 4), 1).unwrap();
    assert_eq!(p["depth"], "rgba32f");
    assert_regions_match(&clipped, "float + mask + clip");
}
