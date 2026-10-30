//! The RAW local tone path through `dr_precision_doc_develop_locals`,
//! each control against the global operation's own result multiplied by the
//! local's coverage. The oracle for a control is its adjustment description,
//! written here from the stated mapping and run through
//! `dr_precision_doc_apply_adjustment` (Clarity and Texture: the 16-bit
//! `presence` selected edit), then `round(lerp(base, op(base), c))`.
//!
//! Bounds, declared before the first run: each control alone at coverage c
//! ≤ 1 u16 code; Exposure + Contrast + Saturation chained against the three
//! operations applied in turn ≤ 3 codes; two overlapping tone locals against
//! the operation applied twice ≤ 2 codes; zero coverage, an all-zero tone and
//! a disabled local bit-exact or NULL; a range multiplies the weight exactly
//! as it does a camera variant's (≤ 1 code against the variant path with the
//! operation's result as the variant); a white-balance variant with a tone
//! equals the tone of the variant ≤ 1 code; the overlap pin bit-exact.
//!
//! Sharpness and Noise Reduction, declared before the first
//! run: Sharpness +60 against the 16-bit selected `smart_sharpen {amount 90,
//! radius 1, noise 0, remove gaussian}`, −60 against `gaussian_blur {radius
//! 1.8}` and Noise Reduction 60 against `reduce_noise {radius 0.5, luminance
//! 0.06, detail 0, color 0}`, each times coverage c ≤ 1 code; Sharpness with
//! Clarity against the Presence result then the filter ≤ 1 code; a float base
//! refuses a Sharpness-only tone by name; Sharpness 101 refuses with the tone
//! sentence.
use crate::float_fixture::float_doc;
use crate::raw_local_numeric::{image, mask_edit, rgba, success, Mask};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_color::dr_precision_doc_apply_adjustment;
use doctored_core::ffi_raw_local::*;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::raw_local_mask::RawLocalMask;
use doctored_core::selected_adjustment::SelectedEdit;
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::ptr;

const W: u32 = 96;
const H: u32 = 64;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// A plate with a full tonal ramp across, fine texture on it, a colour cast
/// that varies by row and column, and a dark and a bright band, so every
/// control has something to move.
fn plate() -> Vec<[u16; 4]> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (f64::from(i % W), f64::from(i / W));
            let ramp = 0.03 + 0.94 * x / f64::from(W - 1);
            let texture = 0.04 * (x * 1.3).sin() * (y * 0.9).sin();
            let band = if y < 8.0 {
                -0.25
            } else if y >= 56.0 {
                0.25
            } else {
                0.0
            };
            let t = ramp + texture + band;
            let code = |v: f64| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
            [
                code(t + 0.05 * (y / 5.0).sin()),
                code(t),
                code(t - 0.06 * (x / 9.0).cos()),
                65535,
            ]
        })
        .collect()
}
fn base() -> Box<PrecisionDocument> {
    image(W, H, &plate(), &IccProfile::srgb())
}
fn full() -> Mask {
    mask_edit(None, W, H, json!({"op":"full"})).unwrap()
}
fn error_text(error: *mut std::ffi::c_char) -> Option<String> {
    (!error.is_null()).then(|| {
        let text = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(error) };
        text
    })
}
/// `dr_precision_doc_develop_locals` with optional variants.
pub(crate) fn try_develop(
    base: &PrecisionDocument,
    variants: &[Option<&PrecisionDocument>],
    masks: &[&Mask],
    settings: Value,
) -> Result<Option<Box<PrecisionDocument>>, String> {
    let variants: Vec<_> = variants
        .iter()
        .map(|v| v.map_or(ptr::null(), |d| d as *const PrecisionDocument))
        .collect();
    let masks: Vec<_> = masks.iter().map(|m| m.0 as *const RawLocalMask).collect();
    let settings = CString::new(settings.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let output = unsafe {
        dr_precision_doc_develop_locals(
            base,
            variants.as_ptr(),
            masks.as_ptr(),
            variants.len(),
            settings.as_ptr(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    match error_text(error) {
        Some(text) => Err(text),
        None => Ok((!output.is_null()).then(|| unsafe { Box::from_raw(output) })),
    }
}
/// The develop result's samples, the base's when it answered NULL.
fn developed(
    base: &PrecisionDocument,
    variants: &[Option<&PrecisionDocument>],
    masks: &[&Mask],
    settings: Value,
) -> Vec<[u16; 4]> {
    try_develop(base, variants, masks, settings)
        .unwrap_or_else(|e| panic!("develop refused: {e}"))
        .map_or_else(|| rgba(base), |d| rgba(&d))
}
fn local(tone: Value, density: f64) -> Value {
    json!({"enabled":true,"density":density,"feather":0,"tone":tone})
}
fn description(op: &str, params: Value) -> String {
    json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":params}).to_string()
}
/// The global operation on `document`'s only layer, or a copy of it for an
/// identity description.
fn applied(document: &PrecisionDocument, description: &str) -> Box<PrecisionDocument> {
    let text = CString::new(description).unwrap();
    let mut error = ptr::null_mut();
    let output = unsafe {
        dr_precision_doc_apply_adjustment(
            document,
            0,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    success(error);
    if output.is_null() {
        Box::new(document.clone())
    } else {
        unsafe { Box::from_raw(output) }
    }
}
/// The 16-bit Presence filter over the whole layer.
fn presence(document: &PrecisionDocument, params: Value) -> Box<PrecisionDocument> {
    let edit = SelectedEdit::parse(&json!({"layer":0,"op":"presence","params":params}).to_string())
        .unwrap();
    Box::new(
        document
            .selected_edit(&edit, &[], LIMIT, 0, None)
            .unwrap()
            .unwrap_or_else(|| document.clone()),
    )
}
/// The 16-bit selected filter `op` over the whole layer.
fn filtered(document: &PrecisionDocument, op: &str, params: Value) -> Box<PrecisionDocument> {
    let edit =
        SelectedEdit::parse(&json!({"layer":0,"op":op,"params":params}).to_string()).unwrap();
    Box::new(
        document
            .selected_edit(&edit, &[], LIMIT, 0, None)
            .unwrap()
            .unwrap_or_else(|| document.clone()),
    )
}
/// `presence_radii`'s coarse radius, from its stated rule: 40 px at a
/// 2048-pixel long side, scaled, rounded, kept above the mid (6) and fine (1)
/// radii, at most 192.
fn coarse_radius(w: u32, h: u32) -> u32 {
    let unit = f64::from(w.max(h)) / 2048.0;
    let scaled = |r: f64| (r * unit).round() as u32;
    let fine = scaled(1.0).max(1);
    let mid = scaled(6.0).max(fine + 1);
    scaled(40.0).max(mid + 1).min(192)
}
/// The global operation the stated mapping gives one control (module doc of
/// `raw_local_tone`), applied to `document`.
fn control_op(document: &PrecisionDocument, key: &str, value: f64) -> Box<PrecisionDocument> {
    match key {
        "exposure" => applied(
            document,
            &description(
                "exposure",
                json!({"exposure":value,"evaluation":"document_v2"}),
            ),
        ),
        "contrast" => applied(
            document,
            &description("bcs", json!({"contrast":value / 100.0,"legacy":false})),
        ),
        "highlights" | "shadows" => {
            let (s, h) = if key == "shadows" {
                (value, 0.0)
            } else {
                (0.0, value)
            };
            applied(
                document,
                &description(
                    "shadows_highlights",
                    json!({"shadows":{"amount":s / 100.0,"tone":0.5},
                           "highlights":{"amount":h / 100.0,"tone":0.5},
                           "radius":coarse_radius(W, H),"color":0.2,"midtone_contrast":0}),
                ),
            )
        }
        "whites" => applied(
            document,
            &description(
                "levels",
                if value > 0.0 {
                    json!({"white":1.0 - 0.25 * value / 100.0})
                } else {
                    json!({"output_white":1.0 + 0.25 * value / 100.0})
                },
            ),
        ),
        "blacks" => applied(
            document,
            &description(
                "levels",
                if value < 0.0 {
                    json!({"black":0.25 * -value / 100.0})
                } else {
                    json!({"output_black":0.25 * value / 100.0})
                },
            ),
        ),
        "saturation" => applied(
            document,
            &description("vibrance", json!({"saturation":value / 100.0})),
        ),
        "clarity" | "texture" => presence(document, json!({ key: value })),
        _ => unreachable!(),
    }
}
fn lerp(base: &[[u16; 4]], op: &[[u16; 4]], c: f64) -> Vec<[u16; 4]> {
    base.iter()
        .zip(op)
        .map(|(b, o)| {
            std::array::from_fn(|k| {
                (f64::from(b[k]) + (f64::from(o[k]) - f64::from(b[k])) * c).round() as u16
            })
        })
        .collect()
}
fn assert_within(actual: &[[u16; 4]], expected: &[[u16; 4]], bound: u16, label: &str) {
    assert_eq!(actual.len(), expected.len());
    for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
        for k in 0..4 {
            assert!(
                a[k].abs_diff(e[k]) <= bound,
                "{label} pixel{i} channel{k}: {} expected {} bound {bound}",
                a[k],
                e[k]
            );
        }
    }
}
fn moved(a: &[[u16; 4]], b: &[[u16; 4]]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

#[test]
fn each_control_alone_is_its_global_operation_times_coverage() {
    let base = base();
    let original = rgba(&base);
    let mask = full();
    let controls = [
        ("exposure", 0.7),
        ("exposure", -1.5),
        ("contrast", 40.0),
        ("contrast", -30.0),
        ("highlights", 60.0),
        ("shadows", 50.0),
        ("whites", 40.0),
        ("whites", -40.0),
        ("blacks", -50.0),
        ("blacks", 30.0),
        ("saturation", 50.0),
        ("saturation", -60.0),
        ("clarity", 60.0),
        ("clarity", -40.0),
        ("texture", 70.0),
        ("texture", -50.0),
    ];
    for (key, value) in controls {
        let op = rgba(&control_op(&base, key, value));
        assert!(
            moved(&op, &original) > 0,
            "{key} {value}: the oracle moved nothing"
        );
        for c in [1.0, 0.6, 0.25] {
            let out = developed(
                &base,
                &[None],
                &[&mask],
                json!([local(json!({ key: value }), c)]),
            );
            assert_within(
                &out,
                &lerp(&original, &op, c),
                1,
                &format!("{key} {value} at {c}"),
            );
        }
    }
}

#[test]
fn chained_controls_match_the_operations_in_turn() {
    let base = base();
    let mask = full();
    let tone = json!({"exposure":0.5,"contrast":35,"saturation":-40});
    let out = developed(&base, &[None], &[&mask], json!([local(tone, 1.0)]));
    let step = control_op(&base, "exposure", 0.5);
    let step = control_op(&step, "contrast", 35.0);
    let step = control_op(&step, "saturation", -40.0);
    assert_within(&out, &rgba(&step), 3, "exposure, contrast, saturation");
}

#[test]
fn overlapping_tone_locals_compound() {
    let base = base();
    let (a, b) = (full(), full());
    let tone = json!({"contrast":30});
    let out = developed(
        &base,
        &[None, None],
        &[&a, &b],
        json!([local(tone.clone(), 1.0), local(tone, 1.0)]),
    );
    let twice = control_op(&control_op(&base, "contrast", 30.0), "contrast", 30.0);
    assert_within(&out, &rgba(&twice), 2, "contrast applied twice");
}

#[test]
fn zero_coverage_zero_tone_and_disabled_change_nothing() {
    let base = base();
    let original = rgba(&base);
    let radial = mask_edit(
        None,
        W,
        H,
        json!({"op":"radial","center":[48,32],"radius":[20,14],"hardness":0.5}),
    )
    .unwrap();
    let mut count = 0;
    let mut error = ptr::null_mut();
    let bytes = unsafe { dr_raw_local_mask_bytes(radial.0, &mut count, &mut error) };
    success(error);
    let coverage: Vec<u16> = unsafe { std::slice::from_raw_parts(bytes, count) }[16..]
        .chunks_exact(2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .collect();
    let out = developed(
        &base,
        &[None],
        &[&radial],
        json!([local(json!({"contrast":60,"clarity":50,"shadows":40}), 1.0)]),
    );
    let outside = coverage.iter().filter(|c| **c == 0).count();
    assert!(outside > 1000 && moved(&out, &original) > 100);
    for (i, c) in coverage.iter().enumerate() {
        if *c == 0 {
            assert_eq!(out[i], original[i], "pixel{i} outside the coverage moved");
        }
    }
    let mask = full();
    for settings in [
        json!([local(json!({}), 1.0)]),
        json!([local(json!({"exposure":0,"contrast":0,"texture":0}), 1.0)]),
        json!([{"enabled":false,"density":1,"feather":0,"tone":{"contrast":80}}]),
        json!([local(json!({"contrast":80}), 0.0)]),
    ] {
        assert!(
            try_develop(&base, &[None], &[&mask], settings.clone())
                .unwrap()
                .is_none(),
            "{settings}: not the exact no-op"
        );
    }
}

#[test]
fn a_range_multiplies_the_tone_weight_as_it_does_a_variant() {
    let base = base();
    let mask = full();
    let op = control_op(&base, "contrast", 50.0);
    let range = json!({"luminance":{"low":0.35,"high":0.65,"low_falloff":0.2,
                                    "high_falloff":0.2,"invert":false}});
    let toned = developed(
        &base,
        &[None],
        &[&mask],
        json!([{"enabled":true,"density":1,"feather":0,"range":range,"tone":{"contrast":50}}]),
    );
    let variant = developed(
        &base,
        &[Some(&op)],
        &[&mask],
        json!([{"enabled":true,"density":1,"feather":0,"range":range}]),
    );
    assert_within(&toned, &variant, 1, "range over a tone");
    let original = rgba(&base);
    let full_op = rgba(&op);
    let partial = toned
        .iter()
        .zip(&original)
        .zip(&full_op)
        .filter(|((t, b), o)| t != b && t != o)
        .count();
    assert!(
        partial > 100,
        "the range left no partial weight ({partial})"
    );
}

#[test]
fn a_camera_variant_with_a_tone_is_the_tone_of_the_variant() {
    let base = base();
    let mask = full();
    // A white-balance stand-in: the base with red raised and blue lowered.
    let shifted: Vec<[u16; 4]> = plate()
        .iter()
        .map(|p| {
            [
                p[0].saturating_add(4000),
                p[1],
                p[2].saturating_sub(3000),
                p[3],
            ]
        })
        .collect();
    let variant = image(W, H, &shifted, &IccProfile::srgb());
    let out = developed(
        &base,
        &[Some(&variant)],
        &[&mask],
        json!([local(json!({"contrast":40}), 1.0)]),
    );
    let expected = control_op(&variant, "contrast", 40.0);
    assert_within(&out, &rgba(&expected), 1, "tone of the variant");
}

#[test]
fn a_float_base_refuses_the_tone_path_by_name() {
    let profile = IccProfile::srgb();
    let pixels: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            let v = (i % W) as f32 / W as f32;
            [v, v, v, 1.0]
        })
        .collect();
    let base = float_doc(W, H, &profile, &[pixels]);
    let mask = full();
    let error = try_develop(
        &base,
        &[None],
        &[&mask],
        json!([local(json!({"exposure":1}), 1.0)]),
    )
    .err()
    .expect("a float base refused");
    assert_eq!(
        error,
        "RAW local tone controls are not available for a 32-bit float development."
    );
}

#[test]
fn a_local_with_no_change_over_a_camera_local_keeps_both_contracts() {
    let base = base();
    let original = rgba(&base);
    let a = full();
    let b = mask_edit(
        None,
        W,
        H,
        json!({"op":"radial","center":[48,32],"radius":[30,20],"hardness":1}),
    )
    .unwrap();
    let gained = control_op(&base, "exposure", 1.0);
    let alone = developed(
        &base,
        &[Some(&gained)],
        &[&a],
        json!([{"enabled":true,"density":1,"feather":0}]),
    );
    assert!(moved(&alone, &original) > 1000);
    // Legacy: B develops a variant that is the base, so inside B the base is
    // restored bit for bit.
    let legacy = developed(
        &base,
        &[Some(&gained), Some(&base)],
        &[&a, &b],
        json!([{"enabled":true,"density":1,"feather":0},
               {"enabled":true,"density":1,"feather":0}]),
    );
    let mut count = 0;
    let mut error = ptr::null_mut();
    let bytes = unsafe { dr_raw_local_mask_bytes(b.0, &mut count, &mut error) };
    success(error);
    let coverage: Vec<u16> = unsafe { std::slice::from_raw_parts(bytes, count) }[16..]
        .chunks_exact(2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .collect();
    let inside = coverage.iter().filter(|c| **c == 65535).count();
    assert!(inside > 500);
    for (i, c) in coverage.iter().enumerate() {
        if *c == 65535 {
            assert_eq!(legacy[i], original[i], "legacy pixel{i}");
        }
    }
    // Version 3: B has no variant and an all-zero tone, so A stands.
    let current = developed(
        &base,
        &[Some(&gained), None],
        &[&a, &b],
        json!([{"enabled":true,"density":1,"feather":0},
               {"enabled":true,"density":1,"feather":0,"tone":{}}]),
    );
    assert_eq!(current, alone);
}

/// Each detail setting beside the global filter it is (`local_detail`'s
/// mapping).
fn detail_cases() -> [(Value, &'static str, Value); 3] {
    [
        (
            json!({"sharpness":60}),
            "smart_sharpen",
            json!({"amount":90,"radius":1,"noise":0,"remove":"gaussian"}),
        ),
        (
            json!({"sharpness":-60}),
            "gaussian_blur",
            json!({"radius":1.8}),
        ),
        (
            json!({"noise_reduction":60}),
            "reduce_noise",
            json!({"radius":0.5,"luminance":0.06,"detail":0,"color":0}),
        ),
    ]
}

#[test]
fn sharpness_and_noise_reduction_are_their_global_filters_times_coverage() {
    let base = base();
    let original = rgba(&base);
    let mask = full();
    for (tone, op, params) in detail_cases() {
        let filtered = rgba(&filtered(&base, op, params));
        assert!(
            moved(&filtered, &original) > 0,
            "{op}: the oracle moved nothing"
        );
        for c in [1.0, 0.6, 0.25] {
            let out = developed(&base, &[None], &[&mask], json!([local(tone.clone(), c)]));
            assert_within(
                &out,
                &lerp(&original, &filtered, c),
                1,
                &format!("{tone} at {c}"),
            );
        }
    }
}

#[test]
fn sharpness_with_clarity_sharpens_the_presence_result() {
    let base = base();
    let mask = full();
    let [(_, op, params), ..] = detail_cases();
    let out = developed(
        &base,
        &[None],
        &[&mask],
        json!([local(json!({"clarity":40,"sharpness":60}), 1.0)]),
    );
    let expected = filtered(&presence(&base, json!({"clarity":40})), op, params);
    assert_within(&out, &rgba(&expected), 1, "clarity, then sharpness");
}

#[test]
fn detail_on_a_float_base_and_out_of_range_refuses_by_name() {
    let profile = IccProfile::srgb();
    let pixels: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            let v = (i % W) as f32 / W as f32;
            [v, v, v, 1.0]
        })
        .collect();
    let float = float_doc(W, H, &profile, &[pixels]);
    let mask = full();
    let error = try_develop(
        &float,
        &[None],
        &[&mask],
        json!([local(json!({"sharpness":40}), 1.0)]),
    )
    .err()
    .expect("a float base refused");
    assert_eq!(
        error,
        "RAW local tone controls are not available for a 32-bit float development."
    );
    let base = base();
    for tone in [
        json!({"sharpness":101}),
        json!({"noise_reduction":-1}),
        json!({"noise_reduction":101}),
    ] {
        let error = try_develop(&base, &[None], &[&mask], json!([local(tone.clone(), 1.0)]))
            .err()
            .unwrap_or_else(|| panic!("{tone} accepted"));
        assert_eq!(
            error,
            "invalid local tone: exposure is -4 to 4 stops, highlights, shadows and noise \
             reduction 0 to 100, every other control -100 to 100",
            "{tone}"
        );
    }
}
