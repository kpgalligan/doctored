//! Filters ▸ Develop Filter: the `develop_filter` `SpatialKernel` arm
//! behind `dr_image_filter_op` at 8 bits and `selected_edit` at 16. Every
//! expectation is a property or the reference in `common/develop_filter_reference.rs`
//! (each stage restated from its own op's test oracle); there are no golden
//! images and no Photoshop numbers.
//!
//! The bounds, declared before the first run and never widened:
//!
//! - POINT. On a 7³ colour grid at 8 bits, every setting without Highlights,
//!   Shadows or Presence is within 1 level of the reference chain.
//! - FLAT. A picture of one colour with all thirteen set is within 1 level of
//!   the reference chain with the guide at the colour's own stage-3 luma (the
//!   guide of a flat picture; Presence leaves it unchanged), at both depths.
//! - PRESENCE. Texture, Clarity and Dehaze alone are byte-identical to
//!   Filters ▸ Presence at 8 bits and code-identical at 16.
//! - CHAIN. On a 16-bit textured plate, all thirteen are within 8 codes of
//!   three edits run one after another: `develop_filter` with stages 1–5, then
//!   `presence`, then `develop_filter` with Vibrance and Saturation.
//! - ZERO. Each control alone at 0, `{}`, and all thirteen at 0 answer no
//!   result and no error at 8 bits and no edit at 16, and plan 0 bytes; a
//!   control at 0 beside another changes nothing that the other alone does
//!   not.
//! - ORDER. On a grey ramp, Exposure +1 with Contrast +50 is within 1 level
//!   of Contrast after Exposure, and Exposure after Contrast differs from it
//!   by at least 8 levels somewhere.
//! - Parameters refuse by name, a float document runs Exposure and refuses
//!   Highlights by name, the run
//!   reports progress and a parked cancel returns "operation cancelled", and
//!   the 24 MP plans at both depths are pinned to their derivations.
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_filters::{dr_image_filter_op, dr_image_filter_op_scratch_bytes};
use doctored_core::ffi_jobs::{dr_job_control_create, dr_job_control_free};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::selected_adjustment::SelectedEdit;
use doctored_core::DrImage;
use serde_json::{json, Map, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use crate::common;
use crate::develop_filter_reference::{self as reference, Controls};
use common::probe::{cancelled_parked_at, reports};

const KEYS: [&str; 13] = [
    "temperature",
    "tint",
    "exposure",
    "contrast",
    "highlights",
    "shadows",
    "whites",
    "blacks",
    "texture",
    "clarity",
    "dehaze",
    "vibrance",
    "saturation",
];

fn image(w: u32, h: u32, pixels: &[u8]) -> *mut DrImage {
    let p = unsafe { dr_image_from_rgba(pixels.as_ptr(), w, h) };
    assert!(!p.is_null());
    p
}

/// `(pixels, message)` of `op`: `pixels` is None for a NULL result, `message`
/// empty when no error was written.
fn run_op(
    img: *const DrImage,
    op: &str,
    params: &str,
    control: *const DrJobControl,
    count: usize,
) -> (Option<Vec<u8>>, String) {
    let op = CString::new(op).unwrap();
    let params = CString::new(params).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_image_filter_op(img, op.as_ptr(), params.as_ptr(), control, &mut err) };
    let pixels = (!out.is_null()).then(|| {
        let v =
            unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(out), count * 4) }.to_vec();
        unsafe { dr_image_free(out) };
        v
    });
    let message = if err.is_null() {
        String::new()
    } else {
        let m = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        m
    };
    (pixels, message)
}

fn run(
    img: *const DrImage,
    params: &str,
    control: *const DrJobControl,
    count: usize,
) -> (Option<Vec<u8>>, String) {
    run_op(img, "develop_filter", params, control, count)
}

/// The 8-bit result of `op`, or None for the bypass.
fn filter8_op(w: u32, h: u32, pixels: &[u8], op: &str, params: &Value) -> Option<Vec<u8>> {
    let img = image(w, h, pixels);
    let (out, message) = run_op(img, op, &params.to_string(), ptr::null(), (w * h) as usize);
    unsafe { dr_image_free(img) };
    assert!(message.is_empty(), "{op} {params}: {message}");
    out
}

fn filter8(w: u32, h: u32, pixels: &[u8], params: &Value) -> Option<Vec<u8>> {
    filter8_op(w, h, pixels, "develop_filter", params)
}

fn doc16(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

/// The 16-bit result of `op` through the selected-edit path, or None for no
/// edit.
fn filter16_op(
    w: u32,
    h: u32,
    pixels: &[[u16; 4]],
    op: &str,
    params: &Value,
) -> Option<Vec<[u16; 4]>> {
    let edit = SelectedEdit::parse(&json!({"layer":0,"op":op,"params":params}).to_string())
        .unwrap_or_else(|e| panic!("{op} {params}: {e}"));
    doc16(w, h, pixels.to_vec())
        .selected_edit(&edit, &[], REFERENCE_WORKING_BYTES, 0, None)
        .unwrap_or_else(|e| panic!("{op} {params}: {e}"))
        .map(|d| d.layer_source(0).unwrap().to_rgba16().unwrap())
}

fn filter16(w: u32, h: u32, pixels: &[[u16; 4]], params: &Value) -> Option<Vec<[u16; 4]>> {
    filter16_op(w, h, pixels, "develop_filter", params)
}

fn widen(pixels: &[u8]) -> Vec<[u16; 4]> {
    pixels
        .chunks_exact(4)
        .map(|p| [0, 1, 2, 3].map(|c| u16::from(p[c]) * 257))
        .collect()
}

/// Structure and mixed alpha (opaque, partial, a transparent pixel with RGB 0).
fn plate(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            let value = |offset: u32| ((x * 23 + y * 41 + offset + (i % 3) * 17) % 256) as u8;
            if i % 13 == 5 {
                return [0, 0, 0, 0];
            }
            [
                value(0),
                value(80),
                value(160),
                if i % 4 == 1 {
                    128 + (i % 64) as u8
                } else {
                    255
                },
            ]
        })
        .collect()
}

/// The reference's 8-bit answer for one 0…1 value.
fn level(v: f64) -> f64 {
    (v.clamp(0.0, 1.0) * 255.0).round()
}

// ---- Point stages --------------------------------------------------------------

#[test]
fn the_point_stages_follow_the_reference_chain() {
    let steps = [0_u8, 37, 90, 128, 180, 230, 255];
    let mut colours: Vec<[u8; 3]> = Vec::new();
    for r in steps {
        for g in steps {
            for b in steps {
                colours.push([r, g, b]);
            }
        }
    }
    let (w, h) = (49_u32, 7_u32);
    assert_eq!(colours.len(), (w * h) as usize);
    let src: Vec<u8> = colours
        .iter()
        .flat_map(|c| [c[0], c[1], c[2], 255])
        .collect();
    let mut failures = Vec::new();
    for params in [
        json!({"temperature": 40}),
        json!({"temperature": -70}),
        json!({"tint": 50}),
        json!({"tint": -30}),
        json!({"exposure": 0.7}),
        json!({"exposure": -1.3}),
        json!({"contrast": 60}),
        json!({"contrast": -45}),
        json!({"whites": 50}),
        json!({"whites": -60}),
        json!({"blacks": -40}),
        json!({"blacks": 30}),
        json!({"vibrance": 60}),
        json!({"vibrance": -50}),
        json!({"saturation": 40}),
        json!({"saturation": -70}),
        json!({"temperature": 25, "tint": -15, "exposure": 0.5, "contrast": 30,
            "whites": 20, "blacks": -15, "vibrance": 35, "saturation": 20}),
    ] {
        let controls = Controls::of(&params);
        let out = filter8(w, h, &src, &params).unwrap_or_else(|| panic!("{params}: a bypass"));
        let mut worst = 0.0_f64;
        for (i, c) in colours.iter().enumerate() {
            let want = reference::chain(c.map(|v| f64::from(v) / 255.0), 0.0, &controls);
            for k in 0..3 {
                worst = worst.max((level(want[k]) - f64::from(out[i * 4 + k])).abs());
            }
            assert_eq!(out[i * 4 + 3], 255, "{params}: alpha moved");
        }
        if worst > 1.0 {
            failures.push(format!("{params}: {worst} levels from the reference"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---- Flat pictures, all thirteen -------------------------------------------------

#[test]
fn a_flat_picture_follows_the_reference_with_every_control_set() {
    let (w, h) = (64_u32, 48_u32);
    let settings = [
        json!({"temperature": 30, "tint": -20, "exposure": 0.6, "contrast": 40,
            "highlights": -50, "shadows": 45, "whites": 25, "blacks": -20,
            "texture": 60, "clarity": 50, "dehaze": 40, "vibrance": 30, "saturation": 15}),
        json!({"temperature": -55, "tint": 35, "exposure": -0.8, "contrast": -30,
            "highlights": 60, "shadows": -70, "whites": -40, "blacks": 35,
            "texture": -50, "clarity": -60, "dehaze": -45, "vibrance": -40, "saturation": -25}),
    ];
    for colour in [
        [128_u8, 128, 128],
        [40, 30, 25],
        [220, 140, 110],
        [60, 120, 200],
        [240, 235, 225],
    ] {
        let flat8: Vec<u8> = (0..w * h)
            .flat_map(|_| [colour[0], colour[1], colour[2], 255])
            .collect();
        let flat16 = widen(&flat8);
        for params in &settings {
            let want = reference::flat(colour.map(|v| f64::from(v) / 255.0), &Controls::of(params));
            let out8 = filter8(w, h, &flat8, params).expect("an 8-bit result");
            let out16 = filter16(w, h, &flat16, params).expect("a 16-bit edit");
            for i in 0..(w * h) as usize {
                for c in 0..3 {
                    let got8 = f64::from(out8[i * 4 + c]);
                    let got16 = f64::from(out16[i][c]) / 257.0;
                    assert!(
                        (got8 - level(want[c])).abs() <= 1.0
                            && (got16 - want[c] * 255.0).abs() <= 1.0,
                        "{colour:?} {params} channel {c}: 8-bit {got8}, 16-bit {got16}, \
                         reference {}",
                        want[c] * 255.0
                    );
                }
            }
        }
    }
}

// ---- Presence inside the filter ---------------------------------------------------

#[test]
fn the_presence_controls_equal_filters_presence() {
    let (w, h) = (36_u32, 28_u32);
    let src = plate(w, h);
    let wide = widen(&src);
    for params in [
        json!({"texture": 60, "clarity": -40, "dehaze": 30}),
        json!({"texture": -80, "dehaze": -50}),
        json!({"clarity": 100}),
    ] {
        let ours8 = filter8(w, h, &src, &params).expect("a result");
        let theirs8 = filter8_op(w, h, &src, "presence", &params).expect("a result");
        assert!(
            ours8 == theirs8,
            "{params}: 8-bit bytes differ from presence"
        );
        let ours16 = filter16(w, h, &wide, &params).expect("an edit");
        let theirs16 = filter16_op(w, h, &wide, "presence", &params).expect("an edit");
        assert!(
            ours16 == theirs16,
            "{params}: 16-bit codes differ from presence"
        );
    }
}

/// A smooth 16-bit plate with texture on it, opaque: every stage has tones
/// and detail to act on.
fn textured16(w: u32, h: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (f64::from(i % w), f64::from(i / w));
            let ramp = 0.15 + 0.7 * x / f64::from(w);
            let detail = 0.06 * (x * 0.9).sin() * (y * 0.7).cos() + 0.03 * ((x + y) * 0.21).sin();
            let value = |tint: f64| {
                ((ramp + detail + tint + 0.1 * y / f64::from(h)).clamp(0.0, 1.0) * 65535.0).round()
                    as u16
            };
            [value(0.05), value(0.0), value(-0.08), 65535]
        })
        .collect()
}

#[test]
fn all_thirteen_equal_the_three_edits_in_order() {
    let (w, h) = (96_u32, 64_u32);
    let src = textured16(w, h);
    let all = json!({"temperature": 30, "tint": -20, "exposure": 0.4, "contrast": 25,
        "highlights": -40, "shadows": 35, "whites": 15, "blacks": -10,
        "texture": 30, "clarity": 20, "dehaze": 15, "vibrance": 25, "saturation": -10});
    let ours = filter16(w, h, &src, &all).expect("an edit");
    let pick = |keys: &[&str]| -> Value {
        let mut map = Map::new();
        for key in keys {
            map.insert((*key).into(), all[*key].clone());
        }
        Value::Object(map)
    };
    let toned = filter16(w, h, &src, &pick(&KEYS[..8])).expect("stages 1-5");
    let present =
        filter16_op(w, h, &toned, "presence", &pick(&KEYS[8..11])).expect("the presence edit");
    let chained = filter16(w, h, &present, &pick(&KEYS[11..])).expect("stage 7");
    let mut worst = 0_u16;
    for (a, b) in ours.iter().zip(&chained) {
        for c in 0..4 {
            worst = worst.max(a[c].abs_diff(b[c]));
        }
    }
    assert!(worst <= 8, "{worst} codes from the chained edits");
}

// ---- Zero --------------------------------------------------------------------------

fn plan(img: *const DrImage, params: &str) -> u64 {
    let (name, text) = (
        CString::new("develop_filter").unwrap(),
        CString::new(params).unwrap(),
    );
    let mut err: *mut c_char = ptr::null_mut();
    let bytes =
        unsafe { dr_image_filter_op_scratch_bytes(img, name.as_ptr(), text.as_ptr(), &mut err) };
    assert!(err.is_null(), "{params}");
    bytes
}

/// A value each control moves a picture with.
fn moving(key: &str) -> Value {
    match key {
        "exposure" => json!(0.5),
        "highlights" | "blacks" | "tint" => json!(-35),
        // The plate's bands are a few pixels wide, so Clarity needs a full
        // amount to move a level.
        "clarity" => json!(-100),
        _ => json!(40),
    }
}

#[test]
fn a_zero_control_changes_nothing() {
    let (w, h) = (24_u32, 18_u32);
    let src = plate(w, h);
    let img = image(w, h, &src);
    let mut zeros: Vec<Value> = vec![json!({})];
    zeros.extend(KEYS.iter().map(|k| json!({ *k: 0 })));
    zeros.push(Value::Object(
        KEYS.iter().map(|k| ((*k).to_string(), json!(0))).collect(),
    ));
    for params in &zeros {
        let (out, message) = run(img, &params.to_string(), ptr::null(), (w * h) as usize);
        assert!(out.is_none() && message.is_empty(), "{params}: {message}");
        assert_eq!(plan(img, &params.to_string()), 0, "{params}");
        assert!(
            filter16(w, h, &widen(&src), params).is_none(),
            "{params}: a 16-bit edit"
        );
    }
    for (index, key) in KEYS.iter().enumerate() {
        let other = KEYS[(index + 1) % KEYS.len()];
        let alone = json!({ other: moving(other) });
        let beside = json!({ *key: 0, other: moving(other) });
        let a = filter8(w, h, &src, &alone).unwrap_or_else(|| panic!("{alone}: a bypass"));
        let b = filter8(w, h, &src, &beside).unwrap_or_else(|| panic!("{beside}: a bypass"));
        assert!(a == b, "{beside} differs from {alone}");
        // Every control moves the plate on its own.
        assert!(a != src, "{alone} changed nothing");
        assert!(
            plan(img, &alone.to_string()) >= u64::from(w * h) * 16,
            "{alone}"
        );
    }
    unsafe { dr_image_free(img) };
}

// ---- Order -----------------------------------------------------------------------

#[test]
fn exposure_then_contrast_is_the_order_and_the_order_matters() {
    let (w, h) = (256_u32, 2_u32);
    let src: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let v = (i % w) as u8;
            [v, v, v, 255]
        })
        .collect();
    let params = json!({"exposure": 1, "contrast": 50});
    let out = filter8(w, h, &src, &params).expect("a result");
    let mut apart = 0.0_f64;
    for x in 0..w as usize {
        let v = x as f64 / 255.0;
        let ours = reference::contrast([reference::exposure(v, 1.0); 3], 0.5)[0];
        let theirs = reference::exposure(reference::contrast([v; 3], 0.5)[0], 1.0);
        let got = f64::from(out[x * 4]);
        assert!(
            (got - level(ours)).abs() <= 1.0,
            "x {x}: {got} against contrast after exposure {}",
            level(ours)
        );
        apart = apart.max((level(ours) - level(theirs)).abs());
    }
    assert!(
        apart >= 8.0,
        "exposure after contrast is only {apart} levels from contrast after exposure"
    );
}

// ---- Refusals, progress and cancel ------------------------------------------------

#[test]
fn parameters_and_a_float_document_refuse_by_name() {
    let (w, h) = (4_u32, 3_u32);
    let img = image(w, h, &[128; 48]);
    for (params, wanted) in [
        (
            json!({"exposure": 4.5}),
            "exposure must be finite in [-4,4]",
        ),
        (
            json!({"temperature": 101}),
            "temperature must be finite in [-100,100]",
        ),
        (
            json!({"blacks": -100.5}),
            "blacks must be finite in [-100,100]",
        ),
        (json!({"saturation": "a"}), "saturation must be numeric"),
        (
            json!({"radius": 3}),
            "unexpected develop_filter parameter radius",
        ),
    ] {
        let (out, message) = run(img, &params.to_string(), ptr::null(), 12);
        assert!(out.is_none(), "{params} was accepted");
        assert_eq!(message, wanted, "{params}");
    }
    unsafe { dr_image_free(img) };

    let float = crate::float_fixture::float_doc(
        w,
        h,
        &IccProfile::srgb(),
        &[vec![[0.4, 0.5, 0.6, 1.0]; (w * h) as usize]],
    );
    // A float document runs Exposure on its linear samples (`v·2^e`) and
    // Highlights on its display-encoded copy (rule H); `float_filters_develop.rs`
    // has the rules.
    let edit = |params: Value| {
        SelectedEdit::parse(&json!({"layer":0,"op":"develop_filter","params":params}).to_string())
            .unwrap()
    };
    let exposed = float
        .selected_edit(
            &edit(json!({"exposure": 1})),
            &[],
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .expect("a float document runs develop_filter exposure")
        .expect("exposure changes the layer");
    let doubled = exposed.layer_source(0).unwrap().to_rgba32().unwrap();
    assert!(doubled.iter().all(|p| *p == [0.8, 1.0, 1.2, 1.0]));
    let toned = float
        .selected_edit(
            &edit(json!({"highlights": 20})),
            &[],
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .expect("a float document runs develop_filter highlights")
        .expect("highlights changes the layer");
    let toned = toned.layer_source(0).unwrap().to_rgba32().unwrap();
    assert!(toned
        .iter()
        .all(|p| p.iter().all(|v| v.is_finite()) && *p != [0.4, 0.5, 0.6, 1.0]));
}

#[test]
fn it_reports_progress_and_cancels_mid_run() {
    let (w, h) = (120_u32, 80_u32);
    let n = (w * h) as usize;
    let img = image(w, h, &plate(w, h)) as usize;
    for params in [
        r#"{"temperature":20,"exposure":0.3,"highlights":-30,"shadows":40,"texture":50,"clarity":-30,"dehaze":40,"vibrance":20}"#,
        r#"{"contrast":40,"shadows":60}"#,
        r#"{"saturation":-50}"#,
    ] {
        let (plain, message) = run(img as *const DrImage, params, ptr::null(), n);
        assert!(message.is_empty(), "{params}: {message}");
        assert!(plain.is_some(), "{params}: a bypass");
        let control = unsafe { dr_job_control_create() };
        let (controlled, message) = run(img as *const DrImage, params, control, n);
        unsafe { dr_job_control_free(control) };
        assert!(message.is_empty(), "{params}: {message}");
        assert!(plain == controlled, "{params}: controlled bytes differ");

        let render = |token: usize| {
            let (out, message) = run(
                img as *const DrImage,
                params,
                token as *const DrJobControl,
                n,
            );
            (out.is_some(), message)
        };
        let log = reports(render);
        assert!(
            log.iter().all(|r| (0.0..=1.0).contains(&r.progress)),
            "{params}: a progress outside [0, 1]"
        );
        assert!(
            log.windows(2)
                .all(|pair| pair[0].progress <= pair[1].progress),
            "{params}: progress went backwards"
        );
        let last = log.last().expect("a report");
        assert!(
            last.progress == 1.0 && last.stage == 5,
            "{params}: the log ends at ({}, stage {})",
            last.progress,
            last.stage
        );
        let inside: Vec<u64> = log
            .iter()
            .filter(|r| r.stage == 6 && r.progress > 0.0 && r.progress < 1.0)
            .map(|r| r.checkpoints)
            .collect();
        assert!(
            inside.len() >= 2,
            "{params}: fewer than two stage-6 reports inside (0, 1)"
        );
        // `checkpoint` parks before `report` stores its stage, so the second
        // stage-6 report parks with stage 6 already stored by the first.
        let count = inside[1];
        let parked = cancelled_parked_at(count, &|_, stage| stage == 6, render);
        assert_eq!(parked.count, count, "{params}");
    }
    unsafe { dr_image_free(img as *mut DrImage) };
}

// ---- The plan ----------------------------------------------------------------------

/// The 8-bit plan of a 6000×4000 image with all thirteen controls on, derived
/// from the stages the header documents. n = 24,000,000. The toned plane is
/// n×16. Beside it, the larger of the guide and Presence's working set: the
/// guide at radius 30·6000/2048 = 87.89 is sigma 43.95, held at factor 8 on
/// a 750×500 grid with a 35-tap kernel (grid sigma 5.46), 375,000·24 + 35·4 +
/// 4096 = 9,004,236; Presence with all three amounts is 836,931,456
/// (`filter_presence.rs`'s derivation), so Presence's. Two bookkeeping MiB:
/// 1,223,028,608, above the 20n combine stage. `filter_op_scratch_bytes` adds
/// four bookkeeping MiB and a quarter.
const PLAN_24MP: u64 = 1_534_028_640;
/// The 16-bit plan of the same edit, `selected_edit_plan`'s `scratch_bytes`:
/// six bookkeeping MiB, the n×8 output codes, the kernel's 1,223,028,608 and
/// the canvas's n×2 coverage.
const PLAN_24MP_WIDE: u64 = 1_469_320_064;
/// The tone-only plan (Presence off): n×16 plus the guide plus two
/// bookkeeping MiB, 395,101,388, under the 20n combine stage, so the 8-bit
/// plan is 20n plus four MiB, plus a quarter.
const PLAN_24MP_TONE: u64 = 605_242_880;
/// The tone-only 16-bit plan: the kernel's 395,101,388 is under the n×16
/// plane held beside the n×8 copy, so six MiB, n×8, 24n and n×2.
const PLAN_24MP_TONE_WIDE: u64 = 822_291_456;

#[test]
fn the_24_megapixel_plans_are_pinned() {
    let (w, h) = (6000_u32, 4000_u32);
    let bytes = vec![0_u8; w as usize * h as usize * 4];
    let img = image(w, h, &bytes);
    drop(bytes);
    let all = json!({"temperature": 30, "tint": -20, "exposure": 0.4, "contrast": 25,
        "highlights": -40, "shadows": 35, "whites": 15, "blacks": -10,
        "texture": 50, "clarity": 50, "dehaze": 50, "vibrance": 25, "saturation": -10});
    let tone = json!({"temperature": 30, "tint": -20, "exposure": 0.4, "contrast": 25,
        "highlights": -40, "shadows": 35, "whites": 15, "blacks": -10, "vibrance": 25,
        "saturation": -10});
    let planned = plan(img, &all.to_string());
    let planned_tone = plan(img, &tone.to_string());
    unsafe { dr_image_free(img) };
    let n = 24_000_000_u64;
    let mib = 1024 * 1024_u64;
    let guide = 375_000 * 24 + 35 * 4 + 4096;
    let presence = 836_931_456_u64;
    let kernel = 16 * n + guide.max(presence) + 2 * mib;
    let base = kernel + 4 * mib;
    assert_eq!(base + base.div_ceil(4), PLAN_24MP, "the derivation");
    assert_eq!(planned, PLAN_24MP);
    let tone_kernel = 16 * n + guide + 2 * mib;
    assert!(tone_kernel < 20 * n);
    let tone_base = 20 * n + 4 * mib;
    assert_eq!(
        tone_base + tone_base.div_ceil(4),
        PLAN_24MP_TONE,
        "the derivation"
    );
    assert_eq!(planned_tone, PLAN_24MP_TONE);

    let doc = doc16(w, h, vec![[0_u16; 4]; (w * h) as usize]);
    let wide = |params: &Value| {
        let edit = SelectedEdit::parse(
            &json!({"layer":0,"op":"develop_filter","params":params}).to_string(),
        )
        .unwrap();
        doc.selected_edit_plan(&edit).unwrap()["scratch_bytes"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(
        6 * mib + 8 * n + kernel + 2 * n,
        PLAN_24MP_WIDE,
        "the derivation"
    );
    assert_eq!(wide(&all), PLAN_24MP_WIDE);
    assert!(tone_kernel < 24 * n);
    assert_eq!(
        6 * mib + 8 * n + 24 * n + 2 * n,
        PLAN_24MP_TONE_WIDE,
        "the derivation"
    );
    assert_eq!(wide(&tone), PLAN_24MP_TONE_WIDE);
}
