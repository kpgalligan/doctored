//! Layer styles, Blend If and the global light in a 32-bit float
//! document, through the request API (`dr_precision_doc_edit` `layer_style`,
//! `layer_blend_if`, `global_light`, `merge_down`, `paint_blend`) and the
//! native round trip.
//!
//! The oracle is written here in binary64 from Photoshop 26.10.0's MEASURED
//! 32-bit statements (`scripts/photoshop-probe/float_styles.py`, receipts
//! `doctored-fixtures/photoshop/float-styles-473/take5` and `take6`), never
//! read off the implementation:
//!
//! - an effect's authored sRGB colour composites as its LINEAR light (the
//!   IEC 61966-2.1 decode, written here), by the W3C step in linear light
//!   with the effect's float mode — Linear Dodge unclamped, a Darken effect
//!   keeping the backdrop under a component of 1 and a Lighten one under a
//!   component of 0; a gradient interpolates between its decoded stops (on
//!   the Classic ramp);
//! - the order is Below effects, the pixels at `alpha · fill`, Interior
//!   effects, and a layer opacity below 1 mixes that result back over the
//!   backdrop (the package model), all unclamped;
//! - an inside Stroke REPLACES the layer under its band (take5's
//!   `hard_normal_inside_beneath_32`): with `s` the band's share of the shape
//!   the pixel is `X + (over(B0, stroke) − X)·s`, `B0` the backdrop after the
//!   Below effects;
//! - Blend If reads the STORED value, unrounded and unclamped, a full
//!   `[0, 0, 255, 255]` ramp weighting 1 and each ramp's zero end one 15-bit
//!   code (1/32768) further out than its knee.
//!
//! Each effect's COVERAGE comes from a 16-bit twin — the same shape, black,
//! over opaque black, with the effect recoloured white, Normal, opacity 1 —
//! as `round(out · 255) / 255`: the shared u8 plane every depth draws.
//! Tolerance `2e-5 · max(1, |want|)`.

use crate::common::classic_ramp;
use crate::float_fixture::float_doc;

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::dr_precision_doc_set_layer_opacity;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 48;
const H: u32 = 32;
const STRIPE: u32 = 8;
/// The styled record's shape: every column, rows 10..22.
const RECT_Y: (u32, u32) = (10, 22);
/// The backdrop's columns: zero, below and above white, a colour above
/// white and one with a negative component.
const BACK: [[f32; 3]; 6] = [
    [0.0, 0.0, 0.0],
    [0.18, 0.18, 0.18],
    [1.0, 1.0, 1.0],
    [4.0, 4.0, 4.0],
    [2.0, 0.5, 0.25],
    [-0.25, 0.5, 1.0],
];
const COLOR: &str = "#ff8000";
const STOPS: [&str; 2] = ["#ff8000", "#0040ff"];
const TOL: f64 = 2e-5;

// ------------------------------------------------------------- the oracle --

/// IEC 61966-2.1 decode of an 8-bit code.
fn decode(code: u8) -> f64 {
    let e = f64::from(code) / 255.0;
    if e <= 0.04045 {
        e / 12.92
    } else {
        ((e + 0.055) / 1.055).powf(2.4)
    }
}

fn hex(text: &str) -> [u8; 3] {
    let v = u32::from_str_radix(&text[1..], 16).unwrap();
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

fn linear(text: &str) -> [f64; 3] {
    hex(text).map(decode)
}

/// `B(cb, cs)` of an EFFECT's mode at 32 bits.
fn effect_blend(mode: &str, cb: f64, cs: f64) -> f64 {
    match mode {
        "normal" => cs,
        "multiply" => cb * cs,
        "linear_dodge" => cb + cs,
        "darken" if cs >= 1.0 => cb,
        "darken" => cb.min(cs),
        "lighten" if cs <= 0.0 => cb,
        "lighten" => cb.max(cs),
        // The W3C formula with no clamp.
        "linear_light" => cb + 2.0 * cs - 1.0,
        other => panic!("no oracle for {other}"),
    }
}

/// The W3C step over an opaque backdrop.
fn over(cb: [f64; 3], cs: [f64; 3], sa: f64, mode: &str) -> [f64; 3] {
    std::array::from_fn(|c| sa * effect_blend(mode, cb[c], cs[c]) + (1.0 - sa) * cb[c])
}

fn close(got: [f32; 4], want: [f64; 3]) -> bool {
    (0..3).all(|c| (f64::from(got[c]) - want[c]).abs() <= TOL * want[c].abs().max(1.0))
        && (got[3] - 1.0).abs() <= 1e-6
}

// ------------------------------------------------------------ fixtures --

fn column(x: u32) -> usize {
    (x / STRIPE) as usize
}

fn in_rect(y: u32) -> bool {
    (RECT_Y.0..RECT_Y.1).contains(&y)
}

fn backdrop_value(x: u32) -> [f32; 3] {
    BACK[column(x)]
}

/// The styled record's colour: the backdrop columns REVERSED, so it holds
/// 4.0, 2.0 and a negative component too.
fn source_value(x: u32) -> [f32; 3] {
    BACK[BACK.len() - 1 - column(x)]
}

fn backdrop_pixels() -> Vec<[f32; 4]> {
    (0..W * H)
        .map(|i| {
            let c = backdrop_value(i % W);
            [c[0], c[1], c[2], 1.0]
        })
        .collect()
}

fn source_pixels() -> Vec<[f32; 4]> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            if in_rect(y) {
                let c = source_value(x);
                [c[0], c[1], c[2], 1.0]
            } else {
                [0.0; 4]
            }
        })
        .collect()
}

fn styled_base(profile: &Arc<IccProfile>) -> PrecisionDocument {
    float_doc(W, H, profile, &[backdrop_pixels(), source_pixels()])
}

fn take(err: *mut c_char) -> String {
    let text = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    text
}

/// One request through `dr_precision_doc_edit`; `Err` is its refusal,
/// `Ok(None)` the no-op.
fn try_edit(doc: &PrecisionDocument, request: &Value) -> Result<Option<PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
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
    if !err.is_null() {
        assert!(out.is_null());
        return Err(take(err));
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

fn edit(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    try_edit(doc, &request)
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"))
}

fn styled(doc: &PrecisionDocument, layer: usize, style: Value) -> PrecisionDocument {
    edit(
        doc,
        json!({"op":"layer_style","version":1,"layer":layer,"style":style}),
    )
}

fn render(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    assert_eq!(doc.float_refusal(), None);
    doc.render(LIMIT, 0, None)
        .expect("the float scene renders")
        .to_rgba32()
        .unwrap()
}

/// The 16-bit twin: the styled record's shape in black over opaque black,
/// carrying `effect` (already recoloured white, Normal, opacity 1); its render
/// read as `round(out · 255) / 255` — the effect's coverage per pixel.
fn coverage(effect: Value) -> Vec<f64> {
    let srgb = IccProfile::srgb();
    let image = |alpha: &dyn Fn(u32) -> u16| {
        let pixels = (0..W * H).map(|i| [0, 0, 0, alpha(i / W)]).collect();
        PrecisionImage::from_rgba16(W, H, pixels, srgb.clone(), None).unwrap()
    };
    let black = Layer::raster("Black", image(&|_| 65535), (0, 0), 1.0, true).unwrap();
    let shape = Layer::raster(
        "Shape",
        image(&|y| if in_rect(y) { 65535 } else { 0 }),
        (0, 0),
        1.0,
        true,
    )
    .unwrap();
    let twin = PrecisionDocument::from_parts(
        W,
        H,
        srgb.clone(),
        Metadata::default(),
        Resolution::default(),
        vec![black, shape],
    )
    .unwrap();
    let twin = styled(&twin, 1, json!({"effects":[effect]}));
    let out = twin.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
    out.iter()
        .map(|p| (f64::from(p[0]) / 65535.0 * 255.0).round() / 255.0)
        .collect()
}

// --------------------------------------------------------------- effects --

#[derive(Clone, Copy, PartialEq)]
enum Placement {
    Below,
    Interior,
    /// The inside band: it replaces the layer under it.
    Band,
}

#[derive(Clone, Copy)]
enum Ink {
    Solid,
    /// `STOPS` along x, the box the shape's (every column).
    Gradient,
    /// `STOPS` along x over the shape's box grown by the 3 px outside band
    /// (a stroke's gradient spans its band, `style_stroke_fill`).
    BandGradient,
}

struct Case {
    name: &'static str,
    placement: Placement,
    ink: Ink,
    /// The 32-bit modes to drive: Normal and, where it differs, the effect's
    /// 32-bit default (Multiply, Linear Dodge).
    modes: &'static [&'static str],
    effect: fn(&str, &str, f32) -> Value,
}

fn gradient(stops: [&str; 2]) -> Value {
    json!({"stops":[{"position":0.0,"color":stops[0],"opacity":1.0},
                    {"position":1.0,"color":stops[1],"opacity":1.0}],
           "style":"linear","angle":0.0,"scale":1.0,"reverse":false,"align_with_layer":true})
}

const CASES: [Case; 13] = [
    Case {
        name: "drop_shadow",
        placement: Placement::Below,
        ink: Ink::Solid,
        modes: &["normal", "multiply"],
        effect: |color, mode, opacity| {
            json!({"type":"drop_shadow","blend":mode,"color":color,"opacity":opacity,
                   "angle":90,"use_global_light":false,"distance":3,"spread":0.5,"size":2})
        },
    },
    Case {
        name: "outer_glow",
        placement: Placement::Below,
        ink: Ink::Solid,
        modes: &["normal", "linear_dodge"],
        effect: |color, mode, opacity| {
            json!({"type":"outer_glow","blend":mode,"color":color,"opacity":opacity,
                   "spread":0.5,"size":4})
        },
    },
    Case {
        name: "stroke_outside",
        placement: Placement::Below,
        ink: Ink::Solid,
        modes: &["normal"],
        effect: |color, mode, opacity| {
            json!({"type":"stroke","blend":mode,"color":color,"opacity":opacity,"size":3,
                   "position":"outside","fill_type":"color"})
        },
    },
    Case {
        name: "stroke_gradient",
        placement: Placement::Below,
        ink: Ink::BandGradient,
        modes: &["normal"],
        effect: |color, mode, opacity| {
            let stops = if color == "#ffffff" {
                ["#ffffff", "#ffffff"]
            } else {
                STOPS
            };
            json!({"type":"stroke","blend":mode,"opacity":opacity,"size":3,
                   "position":"outside","fill_type":"gradient","gradient":gradient(stops)})
        },
    },
    Case {
        name: "inner_shadow",
        placement: Placement::Interior,
        ink: Ink::Solid,
        modes: &["normal", "multiply"],
        effect: |color, mode, opacity| {
            json!({"type":"inner_shadow","blend":mode,"color":color,"opacity":opacity,
                   "angle":90,"use_global_light":false,"distance":2,"choke":0.5,"size":2})
        },
    },
    Case {
        name: "inner_glow",
        placement: Placement::Interior,
        ink: Ink::Solid,
        modes: &["normal", "linear_dodge"],
        effect: |color, mode, opacity| {
            json!({"type":"inner_glow","blend":mode,"color":color,"opacity":opacity,
                   "choke":0.5,"size":3,"source":"edge"})
        },
    },
    Case {
        name: "inner_glow_hard",
        placement: Placement::Interior,
        ink: Ink::Solid,
        modes: &["normal"],
        effect: |color, mode, opacity| {
            json!({"type":"inner_glow","blend":mode,"color":color,"opacity":opacity,
                   "choke":1,"size":3,"source":"edge"})
        },
    },
    Case {
        name: "inner_glow_hard_center",
        placement: Placement::Interior,
        ink: Ink::Solid,
        modes: &["normal"],
        effect: |color, mode, opacity| {
            json!({"type":"inner_glow","blend":mode,"color":color,"opacity":opacity,
                   "choke":1,"size":3,"range":1,"source":"center"})
        },
    },
    Case {
        name: "inner_glow_center",
        placement: Placement::Interior,
        ink: Ink::Solid,
        modes: &["normal"],
        effect: |color, mode, opacity| {
            json!({"type":"inner_glow","blend":mode,"color":color,"opacity":opacity,
                   "choke":0,"size":3,"source":"center"})
        },
    },
    Case {
        name: "stroke_inside",
        placement: Placement::Band,
        ink: Ink::Solid,
        modes: &["normal"],
        effect: |color, mode, opacity| {
            json!({"type":"stroke","blend":mode,"color":color,"opacity":opacity,"size":3,
                   "position":"inside","fill_type":"color"})
        },
    },
    Case {
        name: "color_overlay",
        placement: Placement::Interior,
        ink: Ink::Solid,
        modes: &["normal"],
        effect: |color, mode, opacity| json!({"type":"color_overlay","blend":mode,"color":color,"opacity":opacity}),
    },
    Case {
        name: "gradient_overlay",
        placement: Placement::Interior,
        ink: Ink::Gradient,
        modes: &["normal"],
        effect: |color, mode, opacity| {
            let stops = if color == "#ffffff" {
                ["#ffffff", "#ffffff"]
            } else {
                STOPS
            };
            json!({"type":"gradient_overlay","blend":mode,"opacity":opacity,
                   "gradient":gradient(stops)})
        },
    },
    Case {
        name: "satin",
        placement: Placement::Interior,
        ink: Ink::Solid,
        modes: &["normal", "multiply"],
        effect: |color, mode, opacity| {
            json!({"type":"satin","blend":mode,"color":color,"opacity":opacity,
                   "angle":19,"distance":3,"size":2,"invert":true})
        },
    },
];

/// The effect's linear colour at column `x`: the decoded colour, or the
/// gradient between the decoded stops at `t = x / W` (a linear gradient at
/// angle 0 across the shape's box, every column, sampled at the pixel's
/// top-left corner), on the Classic ramp
/// (`common::classic_ramp`).
fn ink_at(ink: Ink, x: u32) -> [f64; 3] {
    let ramp = |t: f64| -> [f64; 3] {
        let (a, b) = (linear(STOPS[0]), linear(STOPS[1]));
        std::array::from_fn(|c| classic_ramp(&[(0.0, a[c]), (1.0, b[c])], t))
    };
    match ink {
        Ink::Solid => linear(COLOR),
        Ink::Gradient => ramp(f64::from(x) / f64::from(W)),
        Ink::BandGradient => ramp((f64::from(x) + 3.0) / f64::from(W + 6)),
    }
}

/// The f64 composite of one pixel (module doc): `below` / `interior` are
/// `(colour, sa, mode)` steps; the pixels at `s · fill`; `band` the inside
/// band's `(colour, opacity, share, mode)`, which replaces the result under
/// it by `over(B0, colour, opacity)` at its share (every pixel here is
/// opaque, so the premultiplied mix is the plain one); the package mix at
/// `opacity`.
fn oracle(
    x: u32,
    y: u32,
    below: &[([f64; 3], f64, &str)],
    interior: &[([f64; 3], f64, &str)],
    band: Option<([f64; 3], f64, f64, &str)>,
    fill: f64,
    opacity: f64,
) -> [f64; 3] {
    let cb0 = backdrop_value(x).map(f64::from);
    let mut acc = cb0;
    for (ec, sa, mode) in below {
        acc = over(acc, *ec, *sa, mode);
    }
    let b0 = acc;
    if in_rect(y) {
        acc = over(acc, source_value(x).map(f64::from), fill, "normal");
    }
    for (ec, sa, mode) in interior {
        acc = over(acc, *ec, *sa, mode);
    }
    if let Some((ec, sa, share, mode)) = band.filter(|_| in_rect(y)) {
        let replaced = over(b0, ec, sa, mode);
        acc = std::array::from_fn(|c| acc[c] + (replaced[c] - acc[c]) * share);
    }
    std::array::from_fn(|c| cb0[c] + (acc[c] - cb0[c]) * opacity)
}

fn assert_case(case: &Case, mode: &str, effect_opacity: f32, fill: f32, opacity: f32) {
    let a = coverage((case.effect)("#ffffff", "normal", 1.0));
    assert!(a.iter().any(|&v| v > 0.0), "{} draws something", case.name);
    let mut doc = styled(
        &styled_base(&IccProfile::srgb()),
        1,
        json!({"fill_opacity":fill,"effects":[(case.effect)(COLOR, mode, effect_opacity)]}),
    );
    if opacity < 1.0 {
        let mut err = ptr::null_mut();
        let out = unsafe { dr_precision_doc_set_layer_opacity(&doc, 1, opacity, &mut err) };
        assert!(err.is_null(), "{}", take(err));
        doc = *unsafe { Box::from_raw(out) };
    }
    let out = render(&doc);
    for (i, got) in out.iter().enumerate() {
        let (x, y) = (i as u32 % W, i as u32 / W);
        let step = [(ink_at(case.ink, x), f64::from(effect_opacity) * a[i], mode)];
        let (below, interior): (&[_], &[_]) = match case.placement {
            Placement::Below => (&step, &[]),
            Placement::Interior => (&[], &step),
            Placement::Band => (&[], &[]),
        };
        // The shape is opaque in the rect, so the band's share is its
        // coverage there.
        let band = (case.placement == Placement::Band)
            .then(|| (ink_at(case.ink, x), f64::from(effect_opacity), a[i], mode));
        let want = oracle(
            x,
            y,
            below,
            interior,
            band,
            f64::from(fill),
            f64::from(opacity),
        );
        assert!(
            close(*got, want),
            "{} {mode} effect opacity {effect_opacity} fill {fill} opacity {opacity} \
             at ({x}, {y}), coverage {}: got {got:?}, want {want:?}",
            case.name,
            a[i]
        );
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn every_effect_composites_by_the_measured_float_model() {
    for case in &CASES {
        for mode in case.modes {
            for effect_opacity in [1.0f32, 0.5] {
                assert_case(case, mode, effect_opacity, 1.0, 1.0);
            }
        }
    }
}

/// An effect takes the layer set, so a Color Overlay in
/// Linear Light composites `cb + 2·cs − 1` unclamped over the record's
/// columns at 4.0, 2.0 and −0.25 (`source_value`).
#[test]
fn a_linear_light_color_overlay_is_unclamped_over_4_and_below_zero() {
    let case = CASES
        .iter()
        .find(|case| case.name == "color_overlay")
        .expect("the colour overlay case");
    assert!(source_value(0)[0] == -0.25 && source_value(2 * STRIPE)[0] == 4.0);
    for effect_opacity in [1.0f32, 0.5] {
        assert_case(case, "linear_light", effect_opacity, 1.0, 1.0);
    }
}

#[test]
fn fill_opacity_and_the_package_model_hold_extended_range_values() {
    for case in &CASES {
        for (fill, opacity) in [(0.5f32, 1.0f32), (1.0, 0.5), (0.5, 0.5)] {
            assert_case(case, case.modes[case.modes.len() - 1], 0.5, fill, opacity);
        }
    }
}

#[test]
fn a_bevel_composites_each_lit_side_by_the_measured_model() {
    // The second base is overhead and softened: the Smooth law's NORMALIZED
    // shading (#2, `style_bevel_smooth`) shades every slope and lights none
    // at altitude 90. Emboss and Pillow Emboss run the law at half the Size
    // (#2) and draw a Below half outside the rect and an Interior half
    // inside it; the rect's edges are hard, so each pixel is in one band.
    for style in ["inner_bevel", "emboss", "pillow_emboss"] {
        for (altitude, soften) in [(30, 0), (90, 2)] {
            let bevel = |hl: (&str, &str, f32), sh: (&str, &str, f32)| {
                json!({"type":"bevel_emboss","style":style,"depth":1.0,"direction":"up",
                       "size":3,"soften":soften,"angle":120,"use_global_light":false,
                       "altitude":altitude,
                       "highlight_blend":hl.0,"highlight_color":hl.1,"highlight_opacity":hl.2,
                       "shadow_blend":sh.0,"shadow_color":sh.1,"shadow_opacity":sh.2})
            };
            let ah = coverage(bevel(
                ("normal", "#ffffff", 1.0),
                ("normal", "#ffffff", 0.0),
            ));
            let ash = coverage(bevel(
                ("normal", "#ffffff", 0.0),
                ("normal", "#ffffff", 1.0),
            ));
            if altitude == 90 {
                let peak = ash.iter().copied().fold(0.0, f64::max);
                // A Pillow's overhead shadow is shallow at Size 3: its height
                // dips only by 1/4 at the edge (W' = 2), so its slope is a
                // third of an Emboss's there.
                let floor = if style == "pillow_emboss" { 0.02 } else { 0.1 };
                assert!(
                    ah.iter().all(|&v| v == 0.0) && peak > floor,
                    "{style}: the overhead twin: a highlight, or a shadow peak of {peak}"
                );
            } else {
                assert!(
                    ah.iter().any(|&v| v > 0.0) && ash.iter().any(|&v| v > 0.0),
                    "{style}: both sides lit"
                );
            }
            for (hmode, smode) in [("normal", "normal"), ("linear_dodge", "multiply")] {
                let doc = styled(
                    &styled_base(&IccProfile::srgb()),
                    1,
                    json!({"effects":[bevel((hmode, STOPS[0], 0.5), (smode, STOPS[1], 0.75))]}),
                );
                for (i, got) in render(&doc).iter().enumerate() {
                    let (x, y) = (i as u32 % W, i as u32 / W);
                    let sides = [
                        (linear(STOPS[0]), 0.5 * ah[i], hmode),
                        (linear(STOPS[1]), 0.75 * ash[i], smode),
                    ];
                    let (below, interior): (&[_], &[_]) = if in_rect(y) {
                        (&[], &sides)
                    } else {
                        (&sides, &[])
                    };
                    let want = oracle(x, y, below, interior, None, 1.0, 1.0);
                    assert!(
                        close(*got, want),
                        "{style} bevel {hmode}/{smode} at altitude {altitude} ({x}, {y}): \
                         {got:?} {want:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_darken_or_lighten_effect_keeps_the_backdrop_at_its_neutral_component() {
    // Take 6: a white Darken effect over 4.0 reads 4.0; 254/255 takes the
    // minimum; a black Lighten effect over -0.25 reads -0.25.
    for (mode, color) in [
        ("darken", "#ffffff"),
        ("darken", "#fefefe"),
        ("lighten", "#000000"),
        ("lighten", "#010101"),
    ] {
        let doc = styled(
            &styled_base(&IccProfile::srgb()),
            1,
            json!({"effects":[{"type":"color_overlay","blend":mode,"color":color,
                               "opacity":1.0}]}),
        );
        for (i, got) in render(&doc).iter().enumerate() {
            let (x, y) = (i as u32 % W, i as u32 / W);
            let interior = [(linear(color), if in_rect(y) { 1.0 } else { 0.0 }, mode)];
            let want = oracle(x, y, &[], &interior, None, 1.0, 1.0);
            assert!(
                close(*got, want),
                "{mode} {color} ({x}, {y}): {got:?} {want:?}"
            );
        }
    }
}

#[test]
fn a_display_p3_document_keeps_the_backdrop_under_authored_white_darken_and_black_lighten() {
    // The extreme is the AUTHORED colour, not the converted one: authored
    // white is P3's white, exactly 1 per component, so a Darken effect keeps
    // 4.0 (and every other backdrop) untinted however the sRGB → P3 matrix
    // rounds; authored black likewise keeps a Lighten effect's backdrop.
    for (mode, color) in [("darken", "#ffffff"), ("lighten", "#000000")] {
        let doc = styled(
            &styled_base(&IccProfile::display_p3()),
            1,
            json!({"effects":[{"type":"color_overlay","blend":mode,"color":color,
                               "opacity":1.0}]}),
        );
        for (i, got) in render(&doc).iter().enumerate() {
            let (x, y) = (i as u32 % W, i as u32 / W);
            let want = if in_rect(y) {
                source_value(x)
            } else {
                backdrop_value(x)
            };
            assert_eq!(
                got.map(f32::to_bits),
                [want[0], want[1], want[2], 1.0].map(f32::to_bits),
                "P3 {mode} {color} ({x}, {y})"
            );
        }
    }
}

#[test]
fn a_non_srgb_document_composites_an_effect_colour_as_its_linear_fill_ink() {
    // The same authored sRGB colour as a colour overlay at opacity 1 and as a
    // Fill… in sRGB space over a select-all: both are the colour carried into
    // Display P3's LINEAR light, bit for bit.
    let (overlay, fill) = overlay_against_fill(&IccProfile::display_p3(), "srgb");
    assert_eq!(
        overlay
            .iter()
            .map(|p| p.map(f32::to_bits))
            .collect::<Vec<_>>(),
        fill.iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>()
    );
    // And it is NOT the sRGB numbers: P3's red primary is wider.
    assert!(f64::from(overlay[0][0]) < 1.0 - 1e-3);
}

/// A colour overlay at opacity 1 against a Fill… of the same authored bytes
/// in `space` over a select-all, on a float document in `profile`: the
/// shape's samples of each render.
fn overlay_against_fill(profile: &Arc<IccProfile>, space: &str) -> (Vec<[f32; 4]>, Vec<[f32; 4]>) {
    let base = styled_base(profile);
    let overlaid = styled(
        &base,
        1,
        json!({"effects":[{"type":"color_overlay","color":COLOR,"opacity":1.0}]}),
    );
    let ink = hex(COLOR).map(|v| f64::from(v) / 255.0);
    let fill = json!({"op":"paint_blend","layer":1,"color":[ink[0],ink[1],ink[2],1.0],
                      "color_space":space,"opacity":1.0,"blend_mode":0,"selection":false});
    let coverage_input = vec![65535u16; (W * H) as usize];
    let text = CString::new(fill.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let filled = unsafe {
        dr_precision_doc_edit(
            &base,
            text.as_ptr(),
            coverage_input.as_ptr(),
            coverage_input.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null(), "{}", take(err));
    let filled = *unsafe { Box::from_raw(filled) };
    let rows = |pixels: Vec<[f32; 4]>| {
        pixels
            .into_iter()
            .enumerate()
            .filter(|(i, _)| in_rect(*i as u32 / W))
            .map(|(_, p)| p)
            .collect::<Vec<_>>()
    };
    (rows(render(&overlaid)), rows(render(&filled)))
}

#[test]
fn a_second_spelling_of_srgb_keeps_the_authored_numbers() {
    // `PlaneColor::converted`'s rule: a document `MatrixTrc::approx_eq` calls
    // sRGB keeps an authored colour's numbers, so at float they decode by the
    // document's own curve — exactly Fill…'s document-space ink.
    let spelled = Arc::new(
        IccProfile::parse(&crate::icc_spelling::srgb_second_spelling()).expect("a profile"),
    );
    let (overlay, fill) = overlay_against_fill(&spelled, "document");
    assert_eq!(
        overlay
            .iter()
            .map(|p| p.map(f32::to_bits))
            .collect::<Vec<_>>(),
        fill.iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>()
    );
}

// -------------------------------------------------------------- Blend If --

/// The take's Blend If patches: neutrals straddling 60/100/140 in their
/// stored and encoded values, inside each split ramp, a negative, one above
/// white, and one chroma patch.
const BI: [[f32; 3]; 12] = [
    [-0.25, -0.25, -0.25],
    [0.0, 0.0, 0.0],
    [0.05, 0.05, 0.05],
    [0.1, 0.1, 0.1],
    [0.15, 0.15, 0.15],
    [0.2, 0.2, 0.2],
    [0.3, 0.3, 0.3],
    [0.42, 0.42, 0.42],
    [0.5, 0.5, 0.5],
    [1.0, 1.0, 1.0],
    [2.0, 2.0, 2.0],
    [0.5, 0.05, 0.05],
];
const P: u32 = 4;
const BI_SIZE: u32 = P * BI.len() as u32;
const RANGES: [([u8; 4], [u8; 4]); 4] = [
    ([0, 0, 100, 100], [0, 0, 255, 255]),
    ([0, 0, 60, 140], [0, 0, 255, 255]),
    ([0, 0, 255, 255], [100, 100, 255, 255]),
    ([0, 0, 255, 255], [60, 140, 255, 255]),
];

/// One measured ramp on a stored channel value (module doc).
fn ramp(knees: [u8; 4], v: f64) -> f64 {
    if knees == [0, 0, 255, 255] {
        return 1.0;
    }
    let e = 65535.0 / 32768.0;
    let k = knees.map(|k| f64::from(k) * 257.0);
    let (lo0, lo1, hi0, hi1) = (k[0] - e, k[1], k[2], k[3] + e);
    let v = v * 65535.0;
    if v < lo0 {
        0.0
    } else if v < lo1 {
        (v - lo0) / (lo1 - lo0)
    } else if v <= hi0 {
        1.0
    } else if v <= hi1 {
        (hi1 - v) / (hi1 - hi0)
    } else {
        0.0
    }
}

fn channel(name: &str, c: [f32; 3]) -> f64 {
    let c = c.map(f64::from);
    match name {
        "gray" => 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2],
        _ => c[0],
    }
}

fn bi_records() -> [Vec<[f32; 4]>; 2] {
    let at = |i: u32, row: bool| {
        let (x, y) = (i % BI_SIZE, i / BI_SIZE);
        let c = BI[((if row { y } else { x }) / P) as usize];
        [c[0], c[1], c[2], 1.0]
    };
    [
        (0..BI_SIZE * BI_SIZE).map(|i| at(i, true)).collect(),
        (0..BI_SIZE * BI_SIZE).map(|i| at(i, false)).collect(),
    ]
}

/// Every patch of `doc`'s render against the ramp oracle for the source
/// (columns) over the backdrop (rows); `skip` names patches a test leaves
/// out.
fn assert_blend_if(doc: &PrecisionDocument, name: &str, ranges: ([u8; 4], [u8; 4]), what: &str) {
    let out = render(doc);
    for (i, got) in out.iter().enumerate() {
        let (x, y) = (i as u32 % BI_SIZE, i as u32 / BI_SIZE);
        let (cs, cb) = (BI[(x / P) as usize], BI[(y / P) as usize]);
        let w = ramp(ranges.0, channel(name, cs)) * ramp(ranges.1, channel(name, cb));
        let want: [f64; 3] =
            std::array::from_fn(|c| f64::from(cb[c]) + w * (f64::from(cs[c]) - f64::from(cb[c])));
        assert!(
            close(*got, want),
            "{what} {name} {ranges:?} source {cs:?} backdrop {cb:?}: got {got:?}, w {w}"
        );
    }
}

fn blend_if(name: &str, ranges: ([u8; 4], [u8; 4])) -> Value {
    json!({"channel":name,"this_layer":ranges.0,"underlying":ranges.1})
}

#[test]
fn blend_if_reads_the_stored_value_on_a_raster_a_group_and_a_clip_base() {
    let [back, source] = bi_records();
    let srgb = IccProfile::srgb();
    let two = float_doc(BI_SIZE, BI_SIZE, &srgb, &[back.clone(), source.clone()]);
    let grouped = edit(
        &two,
        json!({"op":"group_layers","layers":[1],"name":"G","pass_through":false}),
    );
    // A clip base with one clipped member that is transparent everywhere.
    let empty = vec![[0.0f32; 4]; (BI_SIZE * BI_SIZE) as usize];
    let mut member = empty;
    member[0] = [0.5, 0.5, 0.5, 1.0];
    let clip = float_doc(BI_SIZE, BI_SIZE, &srgb, &[back, source, member]);
    let clip = edit(
        &clip,
        json!({"op":"layer_clipped","layer":2,"clipped":true}),
    );
    for name in ["gray", "red"] {
        for ranges in RANGES {
            let request = |layer: usize| {
                json!({"op":"layer_blend_if","version":1,"layer":layer,
                                      "blend_if":blend_if(name, ranges)})
            };
            assert_blend_if(&edit(&two, request(1)), name, ranges, "raster");
            // Postorder: the backdrop, the source inside, the group.
            assert_blend_if(&edit(&grouped, request(2)), name, ranges, "group");
            let based = edit(&clip, request(1));
            let out = render(&based);
            // The member's one opaque pixel aside, the base is the raster.
            let mut without = render(&edit(&two, request(1)));
            without[0] = out[0];
            assert_eq!(
                out.iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>(),
                without
                    .iter()
                    .map(|p| p.map(f32::to_bits))
                    .collect::<Vec<_>>(),
                "clip base {name} {ranges:?}"
            );
        }
    }
}

#[test]
fn a_style_carrying_blend_if_weights_its_pixels_by_the_same_ramp() {
    let [back, source] = bi_records();
    let two = float_doc(BI_SIZE, BI_SIZE, &IccProfile::srgb(), &[back, source]);
    let ranges = RANGES[1];
    let doc = styled(
        &two,
        1,
        json!({"fill_opacity":1.0,"blend_if":blend_if("red", ranges),
               "effects":[{"type":"drop_shadow","enabled":false}],"knockout":"none"}),
    );
    assert!(doc.layer(1).unwrap().reported_style().is_some());
    assert_blend_if(&doc, "red", ranges, "style");
}

// ------------------------------------------------------- the whole style --

fn full_style() -> Value {
    json!({"fill_opacity":0.75,
           "blend_if":{"channel":"red","this_layer":[0,0,60,140],"underlying":[0,0,255,255]},
           "effects":[
               {"type":"drop_shadow","blend":"multiply","color":COLOR,"opacity":0.5,
                "use_global_light":true,"distance":3,"spread":0.25,"size":2},
               {"type":"color_overlay","blend":"normal","color":"#0040ff","opacity":0.5},
               {"type":"outer_glow","blend":"linear_dodge","color":COLOR,"opacity":0.5,
                "size":3}]})
}

#[test]
fn merge_down_bakes_exactly_the_projection() {
    let doc = styled(&styled_base(&IccProfile::srgb()), 1, full_style());
    let projection = render(&doc);
    let merged = edit(&doc, json!({"op":"merge_down","layer":1}));
    assert_eq!(merged.layer_count(), 1);
    assert!(merged.layer(0).unwrap().reported_style().is_none());
    assert_eq!(
        render(&merged)
            .iter()
            .map(|p| p.map(f32::to_bits))
            .collect::<Vec<_>>(),
        projection
            .iter()
            .map(|p| p.map(f32::to_bits))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_styled_float_document_round_trips_natively_with_its_light() {
    let lit = edit(
        &styled_base(&IccProfile::srgb()),
        json!({"op":"global_light","angle":45.0,"altitude":60.0}),
    );
    let doc = styled(&lit, 1, full_style());
    let bytes = doc.encode_native(LIMIT, 0, None).unwrap();
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    assert!(reopened.is_float());
    assert_eq!(reopened.global_light(), doc.global_light());
    assert_eq!(
        reopened.layer(1).unwrap().reported_style(),
        doc.layer(1).unwrap().reported_style()
    );
    assert_eq!(
        render(&reopened)
            .iter()
            .map(|p| p.map(f32::to_bits))
            .collect::<Vec<_>>(),
        render(&doc)
            .iter()
            .map(|p| p.map(f32::to_bits))
            .collect::<Vec<_>>()
    );
    assert_eq!(reopened.encode_native(LIMIT, 0, None).unwrap(), bytes);
}

#[test]
fn a_global_light_effect_follows_the_float_documents_light() {
    let base = styled_base(&IccProfile::srgb());
    let lit = edit(
        &base,
        json!({"op":"global_light","angle":45.0,"altitude":60.0}),
    );
    let shadow = |global: bool, angle: f32| {
        json!({"effects":[{"type":"drop_shadow","blend":"normal","color":COLOR,"opacity":1.0,
                           "use_global_light":global,"angle":angle,"distance":4,"size":0}]})
    };
    let by_light = render(&styled(&lit, 1, shadow(true, 120.0)));
    let by_angle = render(&styled(&base, 1, shadow(false, 45.0)));
    assert_eq!(
        by_light
            .iter()
            .map(|p| p.map(f32::to_bits))
            .collect::<Vec<_>>(),
        by_angle
            .iter()
            .map(|p| p.map(f32::to_bits))
            .collect::<Vec<_>>()
    );
    assert_ne!(
        render(&styled(&base, 1, shadow(true, 120.0))),
        by_light,
        "the default light casts elsewhere"
    );
}

// ------------------------------------------------------------- refusals --

#[test]
fn an_effect_mode_outside_the_float_set_refuses_by_name_and_clearing_never_does() {
    let doc = styled_base(&IccProfile::srgb());
    for (style, feature) in [
        (
            json!({"effects":[{"type":"outer_glow"}]}),
            "the screen blend mode on the outer_glow layer effect",
        ),
        (
            json!({"effects":[{"type":"color_overlay","blend":"color_dodge"}]}),
            "the color_dodge blend mode on the color_overlay layer effect",
        ),
        (
            json!({"effects":[{"type":"bevel_emboss","highlight_blend":"linear_dodge",
                               "shadow_blend":"overlay"}]}),
            "the overlay blend mode on the bevel_emboss layer effect",
        ),
    ] {
        let refused = try_edit(
            &doc,
            &json!({"op":"layer_style","version":1,"layer":1,"style":style}),
        )
        .err()
        .expect("refused");
        assert_eq!(
            refused,
            format!("{feature} is unavailable in a 32-bit float document")
        );
    }
    let kept = styled(&doc, 1, full_style());
    assert!(try_edit(
        &kept,
        &json!({"op":"layer_style","version":1,"layer":1,"style":null})
    )
    .unwrap()
    .is_some());
}
