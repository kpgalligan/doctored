//! Phase 11E (C): the linear-light projection against an INDEPENDENT f64
//! oracle, at byte depth, plus the policy's refusals and its no-ops.
//!
//! The oracle decodes with the analytic sRGB curve written here, composites
//! with the W3C formula over blend functions written here, replicates the byte
//! adjustment transcode's code rounding, and re-encodes with the same forward
//! threshold rule the crate uses (`curve((k + 0.5) / 255)`, counted). Nothing
//! in it calls the crate's own colour or blend code.
//!
//! GATE: 1 code per channel. The one non-lattice adjustment fixture — a curves
//! layer over a 50 % layer, where the backdrop the kernel sees is itself the
//! result of a compositing step and the byte transcode quantizes it a second
//! time — is gated at 2 codes and says so at its assertion.
//!
//! The last section is the TYPED twin of three of these measurements — the
//! separable-mode oracle, the adjustment transcode and the post-switch policy
//! of `render` / `render_plan` — through `dr_precision_doc_*`, so the typed
//! decode and the typed continuous transcode have their own oracle rather
//! than inheriting the byte one's result. Its gate is derived at
//! `TYPED_GATE`.

use std::ffi::{c_char, c_int, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument, MaskKind};
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_evaluation::*;
use doctored_core::ffi_precision::{
    dr_precision_doc_render, dr_precision_image_free, dr_precision_image_height,
    dr_precision_image_pixels_rgba16, dr_precision_image_width,
};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{
    PrecisionDocument, PrecisionDocumentLayer as PrecisionLayer,
};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use image::{Rgba, RgbaImage};

const ENCODED: c_int = 0;
const LINEAR: c_int = 1;

// ------------------------------------------------------------- the oracle --

fn decode(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// The crate's forward-threshold encode, written independently: the output
/// code is the number of thresholds `curve((k + 0.5) / 255)` strictly below
/// the linear value.
fn encode(linear: f64) -> u8 {
    let linear = linear.clamp(0.0, 1.0);
    (0..255)
        .filter(|k| decode((f64::from(*k) + 0.5) / 255.0) < linear)
        .count() as u8
}

fn code(v: u8) -> f64 {
    f64::from(v) / 255.0
}

/// The analytic inverse of [`decode`] — the CONTINUOUS encode the crate
/// deliberately does not have (there is no curve inverter in it), written
/// here so the oracle can state what a transcode without the byte lattice
/// would have produced.
fn srgb_encode(linear: f64) -> f64 {
    let linear = linear.clamp(0.0, 1.0);
    if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

fn color_burn(cb: f64, cs: f64) -> f64 {
    if cb >= 1.0 {
        1.0
    } else if cs <= 0.0 {
        0.0
    } else {
        1.0 - ((1.0 - cb) / cs).min(1.0)
    }
}

fn color_dodge(cb: f64, cs: f64) -> f64 {
    if cb <= 0.0 {
        0.0
    } else if cs >= 1.0 {
        1.0
    } else {
        (cb / (1.0 - cs)).min(1.0)
    }
}

fn hard_light(cb: f64, cs: f64) -> f64 {
    if cs <= 0.5 {
        2.0 * cb * cs
    } else {
        1.0 - 2.0 * (1.0 - cb) * (1.0 - cs)
    }
}

/// The twenty separable blend functions the policy evaluates, from the W3C
/// compositing specification and Photoshop's published definitions.
fn blend(mode: BlendMode, cb: f64, cs: f64) -> f64 {
    match mode {
        BlendMode::Normal => cs,
        BlendMode::Multiply => cb * cs,
        BlendMode::Screen => cb + cs - cb * cs,
        BlendMode::Overlay => hard_light(cs, cb),
        BlendMode::SoftLight => {
            let d = if cb <= 0.25 {
                ((16.0 * cb - 12.0) * cb + 4.0) * cb
            } else {
                cb.sqrt()
            };
            if cs <= 0.5 {
                cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
            } else {
                cb + (2.0 * cs - 1.0) * (d - cb)
            }
        }
        BlendMode::HardLight => hard_light(cb, cs),
        BlendMode::Darken => cb.min(cs),
        BlendMode::Lighten => cb.max(cs),
        BlendMode::Difference => (cb - cs).abs(),
        BlendMode::Exclusion => cb + cs - 2.0 * cb * cs,
        BlendMode::ColorDodge => color_dodge(cb, cs),
        BlendMode::ColorBurn => color_burn(cb, cs),
        BlendMode::Addition => (cb + cs).min(1.0),
        BlendMode::Subtract => (cb - cs).max(0.0),
        BlendMode::LinearBurn => (cb + cs - 1.0).max(0.0),
        BlendMode::VividLight => {
            if cs <= 0.5 {
                color_burn(cb, 2.0 * cs)
            } else {
                color_dodge(cb, 2.0 * cs - 1.0)
            }
        }
        BlendMode::LinearLight => (cb + 2.0 * cs - 1.0).clamp(0.0, 1.0),
        BlendMode::PinLight => {
            if cs <= 0.5 {
                cb.min(2.0 * cs)
            } else {
                cb.max(2.0 * cs - 1.0)
            }
        }
        BlendMode::HardMix => {
            if cb + cs >= 1.0 {
                1.0
            } else {
                0.0
            }
        }
        BlendMode::Divide => {
            if cs == 0.0 {
                1.0
            } else {
                (cb / cs).min(1.0)
            }
        }
        other => panic!("{other:?} is not a separable mode this phase evaluates"),
    }
}

/// W3C source-over of one straight-alpha source over one straight-alpha
/// backdrop, in whatever domain the caller's components are in.
fn over(bg: [f64; 4], src: [f64; 3], sa: f64, mode: BlendMode) -> [f64; 4] {
    if sa <= 0.0 {
        return bg;
    }
    let ab = bg[3];
    let ao = sa + ab * (1.0 - sa);
    let mut out = [0.0f64; 4];
    for c in 0..3 {
        out[c] = (sa * (1.0 - ab) * src[c]
            + sa * ab * blend(mode, bg[c], src[c])
            + (1.0 - sa) * ab * bg[c])
            / ao;
    }
    out[3] = ao;
    out
}

// ----------------------------------------------------------------- the FFI --

fn take_err(err: *mut c_char) -> String {
    assert!(!err.is_null(), "a refusal must carry a sentence");
    let text = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    text
}

fn handle(doc: &DrDocument) -> *mut DrDocument {
    Box::into_raw(Box::new(doc.clone()))
}

/// `dr_doc_set_evaluation`: `Ok(None)` is the unchanged-policy NULL, `Err` a
/// refusal sentence.
fn switch(doc: &DrDocument, policy: c_int) -> Result<Option<DrDocument>, String> {
    let h = handle(doc);
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_set_evaluation(h, policy, &mut err) };
    unsafe { dr_doc_free(h) };
    if out.is_null() {
        if err.is_null() {
            Ok(None)
        } else {
            Err(take_err(err))
        }
    } else {
        assert!(err.is_null(), "err_out set on success");
        Ok(Some(*unsafe { Box::from_raw(out) }))
    }
}

fn linear(doc: &DrDocument) -> DrDocument {
    switch(doc, LINEAR)
        .unwrap_or_else(|e| panic!("switch refused: {e}"))
        .expect("an encoded document changes policy")
}

/// `dr_doc_flattened`, the projection as the host sees it.
fn flatten(doc: &DrDocument) -> Result<Vec<u8>, String> {
    let h = handle(doc);
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_flattened(h, u64::MAX, std::ptr::null(), &mut err) };
    unsafe { dr_doc_free(h) };
    if out.is_null() {
        return Err(take_err(err));
    }
    let (w, h2) = unsafe { (dr_image_width(out), dr_image_height(out)) };
    let ptr = unsafe { dr_image_pixels_rgba(out) };
    let bytes = unsafe { std::slice::from_raw_parts(ptr, (w * h2 * 4) as usize) }.to_vec();
    unsafe { dr_image_free(out) };
    Ok(bytes)
}

fn refusal(doc: &DrDocument) -> Option<String> {
    let h = handle(doc);
    let out = unsafe { dr_doc_linear_light_refusal(h) };
    unsafe { dr_doc_free(h) };
    if out.is_null() {
        None
    } else {
        Some(take_err(out))
    }
}

fn policy_of(doc: &DrDocument) -> c_int {
    let h = handle(doc);
    let value = unsafe { dr_doc_evaluation(h) };
    unsafe { dr_doc_free(h) };
    value
}

/// Layer `idx`'s adjustment meta, through the FFI setter.
fn with_meta(doc: &DrDocument, idx: usize, meta: &str) -> DrDocument {
    let h = handle(doc);
    let c = CString::new(meta).expect("no interior NUL");
    let out = unsafe { dr_doc_with_layer_meta(h, idx, c.as_ptr()) };
    unsafe { dr_doc_free(h) };
    assert!(!out.is_null(), "the meta was refused: {meta}");
    *unsafe { Box::from_raw(out) }
}

fn with_style(doc: &DrDocument, idx: usize, json: &str) -> DrDocument {
    let style = LayerStyle::from_json(json).unwrap_or_else(|e| panic!("{json}: {e}"));
    doc.set_layer_style(idx, Some(Arc::new(style)))
        .expect("a non-identity style lands")
}

// -------------------------------------------------------------- fixtures --

/// Eight opaque backdrop codes, chosen across the toe, the knee and the
/// shoulder and away from every blend-function seam the gate is sensitive to
/// (`HardMix`'s `cb + cs == 1` threshold and `Divide`'s `cs == 0`).
const BACKDROP: [[u8; 3]; 8] = [
    [16, 239, 128],
    [64, 191, 100],
    [96, 159, 60],
    [128, 127, 200],
    [160, 95, 30],
    [200, 55, 210],
    [230, 25, 90],
    [250, 5, 150],
];

const SOURCE: [[u8; 3]; 8] = [
    [200, 70, 20],
    [150, 120, 240],
    [90, 180, 44],
    [40, 220, 165],
    [220, 35, 75],
    [70, 200, 130],
    [180, 90, 250],
    [20, 235, 10],
];

fn row(colors: &[[u8; 3]; 8], alpha: u8) -> RgbaImage {
    RgbaImage::from_fn(8, 1, |x, _| {
        let c = colors[x as usize];
        Rgba([c[0], c[1], c[2], alpha])
    })
}

/// An opaque backdrop with one source layer over it.
fn two_layer(source_alpha: u8) -> DrDocument {
    let doc = DrDocument::from_pixels(row(&BACKDROP, 255));
    doc.adding_image_layer(0, row(&SOURCE, source_alpha), "Top")
        .expect("the fixture's second layer")
}

fn pixel(bytes: &[u8], x: usize) -> [u8; 4] {
    [
        bytes[x * 4],
        bytes[x * 4 + 1],
        bytes[x * 4 + 2],
        bytes[x * 4 + 3],
    ]
}

fn assert_within(actual: [u8; 4], expected: [u8; 4], gate: i32, what: &str) {
    for c in 0..4 {
        let delta = i32::from(actual[c]) - i32::from(expected[c]);
        assert!(
            delta.abs() <= gate,
            "{what}: channel {c} got {} want {} (gate {gate} codes)",
            actual[c],
            expected[c]
        );
    }
}

// ------------------------------------------------------- the measurements --

#[test]
fn every_separable_mode_matches_the_linear_oracle_over_an_opaque_backdrop() {
    let modes = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::SoftLight,
        BlendMode::HardLight,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::Addition,
        BlendMode::Subtract,
        BlendMode::LinearBurn,
        BlendMode::VividLight,
        BlendMode::LinearLight,
        BlendMode::PinLight,
        BlendMode::HardMix,
        BlendMode::Divide,
    ];
    assert_eq!(modes.len(), 20, "Normal plus the 19 separable modes");
    let base = two_layer(255);
    for mode in modes {
        let doc = base.with_layer_blend_mode(1, mode).unwrap_or(base.clone());
        let bytes = flatten(&linear(&doc)).expect("a migrated stack projects");
        for x in 0..8usize {
            // A guard on the fixture, not on the code: HardMix's threshold and
            // Divide's pole would turn a 1e-9 disagreement into a 255-code
            // one, so the fixture is asserted to stay away from both.
            let cb = BACKDROP[x].map(|v| decode(code(v)));
            let cs = SOURCE[x].map(|v| decode(code(v)));
            for c in 0..3 {
                assert!(
                    (cb[c] + cs[c] - 1.0).abs() > 1e-3 && cs[c] > 1e-3,
                    "fixture pixel {x} channel {c} sits on a blend seam"
                );
            }
            let out = over([cb[0], cb[1], cb[2], 1.0], cs, 1.0, mode);
            let expected = [
                encode(out[0]),
                encode(out[1]),
                encode(out[2]),
                (out[3] * 255.0).round() as u8,
            ];
            assert_within(pixel(&bytes, x), expected, 1, &format!("{mode:?} at {x}"));
        }
    }
}

#[test]
fn opacity_and_masks_lerp_in_linear() {
    let base = two_layer(160);
    let doc = base.with_layer_opacity(1, 0.5).expect("a new opacity");
    // A canvas-sized mask with a ramp, so coverage is neither 0 nor 1.
    let coverage: Vec<u8> = (0..8u32).map(|x| (x * 36) as u8).collect();
    let doc = doc
        .add_mask(1, MaskKind::FromSelection(&coverage))
        .expect("the fixture's mask");
    let bytes = flatten(&linear(&doc)).expect("a migrated stack projects");
    for x in 0..8usize {
        let cb = BACKDROP[x].map(|v| decode(code(v)));
        let cs = SOURCE[x].map(|v| decode(code(v)));
        let sa = (160.0 / 255.0) * 0.5 * (f64::from(coverage[x]) / 255.0);
        let out = over([cb[0], cb[1], cb[2], 1.0], cs, sa, BlendMode::Normal);
        let expected = [
            encode(out[0]),
            encode(out[1]),
            encode(out[2]),
            (out[3] * 255.0).round() as u8,
        ];
        assert_within(pixel(&bytes, x), expected, 1, &format!("masked at {x}"));
    }
}

#[test]
fn a_transparent_backdrop_keeps_the_sources_own_codes() {
    // Nothing underneath: the projection is the source, decoded and re-encoded
    // once, which the transfer's round trip makes exact.
    let doc = DrDocument::from_pixels(row(&SOURCE, 255));
    let encoded = flatten(&doc).expect("the encoded projection");
    let switched = flatten(&linear(&doc)).expect("the linear projection");
    assert_eq!(encoded, switched, "a lone opaque layer is a round trip");
}

#[test]
fn an_isolated_group_boundary_re_encodes_without_moving_the_picture() {
    let doc = two_layer(255);
    let grouped = doc
        .group_layers(&[1], "Group")
        .expect("a group around the top layer")
        .0;
    // `group_layers` makes a PASS-THROUGH group, which allocates no buffer and
    // never enters `doc_group::rendered_group`. Giving the group its own blend
    // mode is what makes `is_isolated` true, so this fixture actually crosses
    // the one boundary that re-encodes a private accumulator.
    let grouped = grouped
        .with_layer_blend_mode(2, BlendMode::Normal)
        .expect("the group isolates");
    let plain = flatten(&linear(&doc)).expect("ungrouped");
    let inside = flatten(&linear(&grouped)).expect("grouped");
    assert_eq!(
        plain, inside,
        "an isolated group of one opaque Normal layer is the layer"
    );
    // And the same identity at the group's own half opacity, where the
    // boundary's re-encoded codes are what the enclosing level then decodes.
    let faded = grouped
        .with_layer_opacity(2, 0.5)
        .expect("a group opacity below 1");
    let bytes = flatten(&linear(&faded)).expect("a faded group projects");
    for x in 0..8usize {
        let cb = BACKDROP[x].map(|v| decode(code(v)));
        let cs = SOURCE[x].map(|v| decode(code(v)));
        let out = over([cb[0], cb[1], cb[2], 1.0], cs, 0.5, BlendMode::Normal);
        let expected = [
            encode(out[0]),
            encode(out[1]),
            encode(out[2]),
            (out[3] * 255.0).round() as u8,
        ];
        assert_within(
            pixel(&bytes, x),
            expected,
            1,
            &format!("faded group at {x}"),
        );
    }
}

#[test]
fn a_clip_group_confines_the_member_to_the_bases_footprint() {
    // A half-transparent base: the clipped member must vanish where it is.
    let footprint = RgbaImage::from_fn(8, 1, |x, _| {
        let c = BACKDROP[x as usize];
        Rgba([c[0], c[1], c[2], if x < 4 { 255 } else { 0 }])
    });
    let base = DrDocument::from_pixels(footprint);
    let doc = base
        .adding_image_layer(0, row(&SOURCE, 255), "Clipped")
        .expect("the member");
    let doc = doc.with_layer_clipped(1, true).expect("clipped");
    let bytes = flatten(&linear(&doc)).expect("a migrated stack projects");
    for x in 4..8usize {
        assert_eq!(pixel(&bytes, x)[3], 0, "outside the footprint at {x}");
    }
    for (x, source) in SOURCE.iter().enumerate().take(4) {
        let expected = [source[0], source[1], source[2], 255];
        assert_within(pixel(&bytes, x), expected, 1, &format!("clipped at {x}"));
    }
}

/// An adjustment layer directly above an OPAQUE LATTICE backdrop at full
/// opacity: the transcode re-encodes the backdrop to exactly the codes the
/// encoded path shows the kernel, so the two policies must agree.
#[test]
fn a_full_opacity_adjustment_over_a_lattice_backdrop_matches_the_encoded_path() {
    for meta in [
        r#"{"type":"adjust","op":"exposure","params":{"exposure":0.75,"evaluation":"document_v2"}}"#,
        r#"{"type":"adjust","op":"curves","params":{"rgb":[[0,12],[128,158],[255,255]]}}"#,
    ] {
        let doc = DrDocument::from_pixels(row(&BACKDROP, 255));
        let doc = doc
            .adding_image_layer(0, RgbaImage::new(1, 1), "Adjustment")
            .expect("the adjustment entry");
        let doc = with_meta(&doc, 1, meta);
        let encoded = flatten(&doc).expect("the encoded projection");
        let switched = flatten(&linear(&doc)).expect("the linear projection");
        for x in 0..8usize {
            assert_within(
                pixel(&switched, x),
                pixel(&encoded, x),
                1,
                &format!("{meta} at {x}"),
            );
        }
    }
}

/// The same adjustment at HALF coverage: the two policies must now differ by
/// the lerp domain, and the linear one must follow the oracle.
#[test]
fn a_masked_adjustment_lerps_in_linear() {
    let meta =
        r#"{"type":"adjust","op":"exposure","params":{"exposure":1.0,"evaluation":"document_v2"}}"#;
    let doc = DrDocument::from_pixels(row(&BACKDROP, 255));
    let doc = doc
        .adding_image_layer(0, RgbaImage::new(1, 1), "Adjustment")
        .expect("the adjustment entry");
    let doc = with_meta(&doc, 1, meta);
    let doc = doc.with_layer_opacity(1, 0.5).expect("half opacity");
    let full = flatten(&doc.with_layer_opacity(1, 1.0).expect("full opacity"))
        .expect("the full-opacity encoded projection");
    let switched = flatten(&linear(&doc)).expect("the linear projection");
    for (x, base) in BACKDROP.iter().enumerate() {
        let adjusted = pixel(&full, x);
        let mut expected = [0u8; 4];
        for c in 0..3 {
            let backdrop = decode(code(base[c]));
            let target = decode(code(adjusted[c]));
            expected[c] = encode(backdrop + (target - backdrop) * 0.5);
        }
        expected[3] = 255;
        assert_within(
            pixel(&switched, x),
            expected,
            1,
            &format!("masked adjustment at {x}"),
        );
    }
}

/// The DISCLOSED double quantization. The backdrop under this adjustment is
/// itself a composite, so it is NOT a lattice value, and the byte transcode
/// rounds it to one before the kernel sees it. The oracle here is the IDEAL
/// transcode — the same kernel applied to the backdrop with no intermediate
/// rounding at all — so what the gate measures is exactly that extra step.
///
/// The kernel is `invert` rather than `curves`: the byte `curves` op is a
/// 256-entry lookup table, so its own input is quantized under BOTH policies
/// and an oracle for it would have to duplicate `adjust_curves`' natural
/// spline. `invert` is `1 - v` in one line, which keeps the oracle genuinely
/// independent and leaves the transcode as the only thing being measured.
///
/// GATE: 2 codes, fixed before measuring, and recorded in the status doc.
#[test]
fn a_pointwise_adjustment_over_a_half_opacity_layer_stays_within_two_codes() {
    let base = two_layer(255);
    let doc = base.with_layer_opacity(1, 0.5).expect("half opacity");
    let doc = doc
        .adding_image_layer(1, RgbaImage::new(1, 1), "Invert")
        .expect("the adjustment entry");
    let doc = with_meta(&doc, 2, r#"{"type":"adjust","op":"invert"}"#);
    let switched = flatten(&linear(&doc)).expect("the linear projection");
    for x in 0..8usize {
        let cb = BACKDROP[x].map(|v| decode(code(v)));
        let cs = SOURCE[x].map(|v| decode(code(v)));
        let composited = over([cb[0], cb[1], cb[2], 1.0], cs, 0.5, BlendMode::Normal);
        let mut expected = [0u8; 4];
        for c in 0..3 {
            // The ideal transcode: encode continuously, invert, decode.
            expected[c] = encode(decode(1.0 - srgb_encode(composited[c])));
        }
        expected[3] = 255;
        assert_within(
            pixel(&switched, x),
            expected,
            2,
            &format!("non-lattice adjustment at {x}"),
        );
    }
}

// ------------------------------------------------------------- the policy --

/// Every operator phase 1 refused — the seven modes, a style, Blend If and a
/// guided Shadows/Highlights — switches, refuses nothing and projects. Their
/// numbers are `evaluation_nonseparable`, `evaluation_styles` and
/// `evaluation_guide`'s.
#[test]
fn every_formerly_refused_operator_is_admitted_at_the_switch() {
    let base = two_layer(255);
    let mut docs = Vec::new();
    for mode in [
        BlendMode::DarkerColor,
        BlendMode::LighterColor,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
        BlendMode::Dissolve,
    ] {
        docs.push(base.with_layer_blend_mode(1, mode).expect("a new mode"));
    }
    docs.push(with_style(
        &base,
        1,
        "{\"effects\":[{\"type\":\"color_overlay\"}]}",
    ));
    docs.push(with_style(
        &base,
        1,
        "{\"blend_if\":{\"channel\":\"gray\",\"this_layer\":[0,0,200,255],\
         \"underlying\":[0,0,255,255]}}",
    ));
    docs.push(with_meta(
        &base,
        1,
        r#"{"type":"adjust","op":"shadows_highlights","params":{"shadows":{"amount":0.5},"highlights":{"amount":0.2}}}"#,
    ));
    for doc in docs {
        let switched = linear(&doc);
        assert!(refusal(&switched).is_none(), "nothing is refused");
        assert!(flatten(&switched).is_ok(), "and it projects");
    }
}

/// The one refusal left is the staleness net: a profile replaced without
/// its rebind refuses every projection with the walk's own sentence.
#[test]
fn a_stale_transfer_refuses_every_projection() {
    let switched = linear(&two_layer(255));
    assert_eq!(policy_of(&switched), LINEAR);
    assert!(
        refusal(&switched).is_none(),
        "a current transfer refuses nothing"
    );

    let mut stale = switched.clone();
    stale.profile = IccProfile::display_p3();
    let reason = refusal(&stale).expect("the net names it");
    assert!(
        reason.contains("does not belong to its colour profile"),
        "{reason}"
    );
    assert_eq!(
        flatten(&stale).expect_err("the projection refuses"),
        reason,
        "the render's sentence IS the net's"
    );
}

#[test]
fn an_unchanged_policy_returns_null_without_an_error() {
    let doc = two_layer(255);
    assert!(
        switch(&doc, ENCODED).expect("no error").is_none(),
        "an encoded document asked for encoded"
    );
    let switched = linear(&doc);
    assert!(
        switch(&switched, LINEAR).expect("no error").is_none(),
        "a linear document asked for linear"
    );
    assert!(
        switch(&switched, ENCODED)
            .expect("no error")
            .is_some_and(|d| policy_of(&d) == ENCODED),
        "and back again"
    );
}

#[test]
fn an_encoded_projection_is_untouched_by_the_policy_existing() {
    let doc = two_layer(200);
    let before = flatten(&doc).expect("the encoded projection");
    let round_trip = switch(&linear(&doc), ENCODED)
        .expect("no error")
        .expect("back to encoded");
    let after = flatten(&round_trip).expect("the encoded projection again");
    assert_eq!(before, after, "encoded documents are byte-identical");
    assert!(
        refusal(&round_trip).is_none(),
        "an encoded document is never walked"
    );
}

#[test]
fn a_profile_assign_rebuilds_the_transfer_and_a_lut_profile_refuses_it() {
    let doc = two_layer(255);
    let p3 = IccProfile::display_p3();
    let assigned = linear(&doc)
        .assign_profile(&p3)
        .expect("assign onto linear")
        .expect("a different profile");
    let fresh = linear(
        &doc.assign_profile(&p3)
            .expect("assign onto encoded")
            .expect("a different profile"),
    );
    let a = flatten(&assigned).expect("the rebound projection");
    let b = flatten(&fresh).expect("the fresh projection");
    for x in 0..8usize {
        assert_within(pixel(&a, x), pixel(&b, x), 1, &format!("rebound at {x}"));
    }

    // A profile the policy cannot decode is refused by the ASSIGN rather than
    // silently leaving a stale transfer behind.
    let lut = Arc::new(IccProfile::parse(&lut_profile_bytes()).expect("the LUT fixture parses"));
    assert!(
        linear(&doc).assign_profile(&lut).is_err(),
        "assigning a LUT profile onto a linear document refuses"
    );
    assert!(
        matches!(doc.assign_profile(&lut), Ok(Some(_))),
        "but an encoded document still accepts it"
    );
}

/// An RGB profile with an A2B0 table and no matrix tags.
fn lut_profile_bytes() -> Vec<u8> {
    let tags: Vec<([u8; 4], Vec<u8>)> = vec![(
        *b"A2B0",
        vec![b'm', b'f', b't', b'2', 0, 0, 0, 0, 3, 3, 0, 0],
    )];
    let start = 132 + 12 * tags.len();
    let mut table = Vec::new();
    let mut payload: Vec<u8> = Vec::new();
    for (sig, data) in &tags {
        let offset = (start + payload.len()) as u32;
        payload.extend_from_slice(data);
        while !payload.len().is_multiple_of(4) {
            payload.push(0);
        }
        table.extend_from_slice(sig);
        table.extend_from_slice(&offset.to_be_bytes());
        table.extend_from_slice(&(data.len() as u32).to_be_bytes());
    }
    let mut out = vec![0u8; 128];
    out[0..4].copy_from_slice(&((start + payload.len()) as u32).to_be_bytes());
    out[8..12].copy_from_slice(&[0x02, 0x10, 0x00, 0x00]);
    out[12..16].copy_from_slice(b"mntr");
    out[16..20].copy_from_slice(b"RGB ");
    out[20..24].copy_from_slice(b"XYZ ");
    out[36..40].copy_from_slice(b"acsp");
    out.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    out.extend_from_slice(&table);
    out.extend_from_slice(&payload);
    out
}

#[test]
fn the_operator_table_names_what_is_migrated_and_what_is_not() {
    let raw = unsafe { dr_evaluation_operators() };
    assert!(!raw.is_null());
    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(raw) };
    let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    let migrated = value["migrated"].as_array().expect("migrated");
    assert!(migrated.iter().any(|v| v == "multiply"));
    assert!(migrated.iter().any(|v| v == "pointwise_adjustment_layer"));
    let refused = value["refused"].as_array().expect("refused");
    assert!(refused
        .iter()
        .all(|entry| matches!(entry["phase"].as_u64(), Some(2) | Some(3))));
    assert!(refused.iter().any(|entry| entry["operator"]
        .as_str()
        .is_some_and(|s| s.contains("Image Size"))));
    assert!(migrated.iter().any(|v| v == "hue"));

    for (name, expected) in [("encoded", 0), ("linear_light", 1)] {
        let c = CString::new(name).unwrap();
        assert_eq!(unsafe { dr_evaluation_from_name(c.as_ptr()) }, expected);
    }
    let bogus = CString::new("srgb").unwrap();
    assert_eq!(unsafe { dr_evaluation_from_name(bogus.as_ptr()) }, -1);
    assert_eq!(unsafe { dr_evaluation_from_name(ptr::null()) }, -1);
}

// ------------------------------------------------------ the typed twins (C) --
//
// The same three measurements at 16 bits, through the typed FFI, so the typed
// decode, the typed continuous transcode and the typed post-switch refusal have
// their own oracle rather than inheriting the byte one's result.
//
// The typed encode is `PrecisionToneEncoder`, a linear interpolation of the
// forward curve over 65536 intervals, so it is continuous where the byte
// encode is a lattice — the oracle below re-encodes with the analytic inverse
// `srgb_encode` and rounds once, exactly as
// `PrecisionImage::from_float_samples` does.
//
// GATE: 24 codes of 65535, DERIVED rather than measured, and wider than the
// spec's typed figure of 2 for one stated reason. The built-in sRGB profile's
// TRC is not the analytic curve: it is 1024 samples of it rounded to 16 bits
// (`icc_builtin`), which `evaluation_transfer` bounds at 1e-5 linear. This
// oracle is analytic, so that 1e-5 is inside every number it produces, and the
// typed encode's steepest slope — the toe, `65535 · 12.92` codes per unit
// linear — turns it into 8.5 codes on the way out; decode and encode each
// contribute one such term, so 17 codes is the bound and 24 is the round
// number above it. That is still 0.09 of ONE BYTE code, so the typed gate
// remains three thousand times tighter than the byte file's own 1-code gate;
// the spec's 2-code figure belongs to `evaluation_transfer`, whose oracle
// measures the crate's tables directly instead of through a profile.
const TYPED_GATE: i64 = 24;

/// An 8-bit fixture code as the 16-bit LATTICE code it promotes to: `257 · k`
/// is an exact entry of the typed decode table, so both depths' oracles decode
/// the same number.
fn typed_code(v: u8) -> u16 {
    u16::from(v) * 257
}

fn typed_row(rows: &[[u8; 3]; 8], alpha: u16) -> Vec<[u16; 4]> {
    rows.iter()
        .map(|c| [typed_code(c[0]), typed_code(c[1]), typed_code(c[2]), alpha])
        .collect()
}

fn typed_image(pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(pixels.len() as u32, 1, pixels, IccProfile::srgb(), None)
        .expect("the fixture image")
}

fn typed_raster(name: &str, pixels: Vec<[u16; 4]>, opacity: f32) -> PrecisionLayer {
    PrecisionLayer::raster(name, typed_image(pixels), (0, 0), opacity, true).expect("a raster")
}

fn typed_document(layers: Vec<PrecisionLayer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        8,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .expect("the fixture document")
}

fn typed_two_layer(alpha: u16) -> PrecisionDocument {
    typed_document(vec![
        typed_raster("Base", typed_row(&BACKDROP, 65535), 1.0),
        typed_raster("Top", typed_row(&SOURCE, alpha), 1.0),
    ])
}

/// `dr_precision_doc_set_evaluation`, the typed twin of [`switch`].
fn typed_switch(
    doc: &PrecisionDocument,
    policy: c_int,
) -> Result<Option<PrecisionDocument>, String> {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_evaluation(doc, policy, &mut err) };
    if out.is_null() {
        if err.is_null() {
            Ok(None)
        } else {
            Err(take_err(err))
        }
    } else {
        assert!(err.is_null(), "err_out set on success");
        Ok(Some(*unsafe { Box::from_raw(out) }))
    }
}

fn typed_linear(doc: &PrecisionDocument) -> PrecisionDocument {
    typed_switch(doc, LINEAR)
        .unwrap_or_else(|e| panic!("typed switch refused: {e}"))
        .expect("an encoded typed document changes policy")
}

/// `dr_precision_doc_render`, the authoritative u16 projection.
fn typed_render(doc: &PrecisionDocument) -> Result<Vec<[u16; 4]>, String> {
    let mut err: *mut c_char = ptr::null_mut();
    let image =
        unsafe { dr_precision_doc_render(doc, REFERENCE_WORKING_BYTES, 0, ptr::null(), &mut err) };
    if image.is_null() {
        return Err(take_err(err));
    }
    let count =
        unsafe { dr_precision_image_width(image) * dr_precision_image_height(image) } as usize;
    let ptr = unsafe { dr_precision_image_pixels_rgba16(image) };
    assert!(!ptr.is_null(), "a u16 projection");
    let samples = unsafe { std::slice::from_raw_parts(ptr, count * 4) }.to_vec();
    unsafe { dr_precision_image_free(image) };
    Ok(samples
        .chunks_exact(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect())
}

/// `dr_precision_doc_edit`; `None` is the exact no-op.
fn typed_edit(doc: &PrecisionDocument, request: &str) -> Option<PrecisionDocument> {
    let request = CString::new(request).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        assert!(err.is_null(), "the edit refused: {}", take_err(err));
        return None;
    }
    Some(*unsafe { Box::from_raw(out) })
}

fn typed_blend(doc: &PrecisionDocument, layer: usize, mode: BlendMode) -> PrecisionDocument {
    let request = format!(
        r#"{{"op":"layer_blend","version":1,"layer":{layer},"blend_mode":{}}}"#,
        mode.to_c()
    );
    typed_edit(doc, &request).unwrap_or_else(|| doc.clone())
}

/// The typed re-encode: continuous, then the ONE `round(v · 65535)` the store
/// makes.
fn typed_encode(linear: f64) -> u16 {
    (srgb_encode(linear) * 65535.0).round() as u16
}

fn typed_within(actual: [u16; 4], expected: [u16; 4], what: &str) {
    for c in 0..4 {
        let delta = i64::from(actual[c]) - i64::from(expected[c]);
        assert!(
            delta.abs() <= TYPED_GATE,
            "{what}: channel {c} got {} want {} (gate {TYPED_GATE} codes of 65535)",
            actual[c],
            expected[c]
        );
    }
}

#[test]
fn every_separable_mode_matches_the_linear_oracle_at_typed_depth() {
    let modes = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::SoftLight,
        BlendMode::HardLight,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::Addition,
        BlendMode::Subtract,
        BlendMode::LinearBurn,
        BlendMode::VividLight,
        BlendMode::LinearLight,
        BlendMode::PinLight,
        BlendMode::HardMix,
        BlendMode::Divide,
    ];
    assert_eq!(modes.len(), 20, "Normal plus the 19 separable modes");
    let base = typed_two_layer(65535);
    for mode in modes {
        let doc = typed_blend(&base, 1, mode);
        let samples = typed_render(&typed_linear(&doc)).expect("a migrated typed stack renders");
        for x in 0..8usize {
            let cb = BACKDROP[x].map(|v| decode(code(v)));
            let cs = SOURCE[x].map(|v| decode(code(v)));
            for c in 0..3 {
                assert!(
                    (cb[c] + cs[c] - 1.0).abs() > 1e-3 && cs[c] > 1e-3,
                    "fixture pixel {x} channel {c} sits on a blend seam"
                );
            }
            let out = over([cb[0], cb[1], cb[2], 1.0], cs, 1.0, mode);
            let expected = [
                typed_encode(out[0]),
                typed_encode(out[1]),
                typed_encode(out[2]),
                (out[3] * 65535.0).round() as u16,
            ];
            typed_within(samples[x], expected, &format!("typed {mode:?} at {x}"));
        }
    }
}

/// The typed TRANSCODE: `decode(kernel(encode(backdrop)))`, continuously, so a
/// full-opacity adjustment over an opaque backdrop must agree with the encoded
/// typed render — and at half opacity the `k` lerp must happen in LINEAR.
#[test]
fn a_typed_adjustment_transcodes_and_lerps_in_linear() {
    let meta = r#"{"type":"adjust","op":"curves","params":{"rgb":[[0,12],[128,158],[255,255]]},"evaluation":"precision_v1"}"#;
    let base = || typed_raster("Base", typed_row(&BACKDROP, 65535), 1.0);
    let adjustment = |opacity: f32| {
        PrecisionLayer::adjustment("Adjustment", meta, opacity, true, &IccProfile::srgb())
            .expect("the typed adjustment entry")
    };
    let full = typed_document(vec![base(), adjustment(1.0)]);
    let encoded = typed_render(&full).expect("the encoded typed projection");
    let switched = typed_render(&typed_linear(&full)).expect("the linear typed projection");
    for x in 0..8usize {
        typed_within(switched[x], encoded[x], &format!("typed transcode at {x}"));
    }

    // Half opacity: the two policies now differ by the lerp's domain.
    let half = typed_document(vec![base(), adjustment(0.5)]);
    let faded = typed_render(&typed_linear(&half)).expect("the faded linear typed projection");
    for (x, base_pixel) in BACKDROP.iter().enumerate() {
        let mut expected = [0u16; 4];
        for c in 0..3 {
            let backdrop = decode(code(base_pixel[c]));
            let target = decode(f64::from(encoded[x][c]) / 65535.0);
            expected[c] = typed_encode(backdrop + (target - backdrop) * 0.5);
        }
        expected[3] = 65535;
        typed_within(faded[x], expected, &format!("typed half adjustment at {x}"));
    }
}

/// The typed policy after the switch: an operator phase 1 refused — a style,
/// Blend If, Hue — added AFTER the switch renders and plans.
#[test]
fn a_typed_formerly_refused_operator_after_the_switch_renders_and_plans() {
    let switched = typed_linear(&typed_two_layer(65535));
    assert!(
        typed_render(&switched).is_ok(),
        "the switched stack renders"
    );
    let styled = typed_edit(
        &switched,
        r#"{"op":"layer_style","version":1,"layer":1,"style":{"effects":[{"type":"color_overlay"}]}}"#,
    )
    .expect("the style lands");
    let blend_if = typed_edit(
        &switched,
        r#"{"op":"layer_blend_if","version":1,"layer":1,"blend_if":{"channel":"gray","this_layer":[0,0,200,255]}}"#,
    )
    .expect("the Blend If lands");
    for doc in [styled, blend_if, typed_blend(&switched, 1, BlendMode::Hue)] {
        assert!(doc.linear_light_refusal().is_none(), "nothing is refused");
        assert!(typed_render(&doc).is_ok(), "it renders");
        assert!(doc.render_plan().is_ok(), "and plans");
    }
}
