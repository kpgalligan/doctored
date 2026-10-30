//! Every spatial filter at both depths: the 24 op names of the app's
//! `SelectedAdjustment.spatial` set, each run at 8 bits (through
//! `dr_image_filter_op`, or the op's own byte export where it has one) and at
//! 16 bits (the `SelectedEdit` of the same name and parameters) on the same
//! plate widened by ×257.
//!
//! Neither depth is the other's implementation of record: the claim is that
//! a filter means the same picture at both depths. Each row states its own
//! bound in 8-bit levels, on the 16-bit result rounded to 8 bits. No row
//! needs a statistical bound: the three seeded filters (`add_noise`,
//! `add_grain`, the clouds pair) draw the same stream at both depths, so they
//! are compared pixel by pixel like the rest. The plate is opaque, so the
//! comparison is of each filter's colour math; alpha handling is each
//! filter's own test file's.

use doctored_core::ffi::{
    dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba, dr_image_sharpen, dr_string_free,
};
use doctored_core::ffi_filters::{
    dr_image_edge_detect, dr_image_emboss, dr_image_filter_op, dr_image_noise, dr_image_pixelate,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::selected_adjustment::SelectedEdit;
use doctored_core::DrImage;
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::ptr;

const W: u32 = 32;
const H: u32 = 24;

/// One level: the 8-bit result is rounded once from its own arithmetic and
/// the 16-bit result once more on the way down to 8 bits, so two correct
/// implementations of one filter can sit either side of a rounding boundary.
const ONE_LEVEL: f64 = 1.0;
/// Offset moves samples and computes nothing, so its depths agree exactly.
const EXACT: f64 = 0.0;

struct Row {
    op: &'static str,
    params: Value,
    /// Every channel of every pixel within this many 8-bit levels.
    bound: f64,
}

/// The table: one row per name in `SelectedAdjustment.spatial`.
fn rows() -> Vec<Row> {
    let row = |op, params, bound| Row { op, params, bound };
    vec![
        row("blur", json!({"sigma": 2.5}), ONE_LEVEL),
        row("sharpen", json!({"amount": 1.5}), ONE_LEVEL),
        row(
            "unsharp_mask",
            json!({"radius": 2.0, "amount": 1.2, "threshold": 0.02}),
            ONE_LEVEL,
        ),
        row("high_pass", json!({"radius": 3.0}), ONE_LEVEL),
        row(
            "reduce_noise",
            json!({"radius": 2.0, "luminance": 0.05, "detail": 0.4, "color": 0.6}),
            ONE_LEVEL,
        ),
        row(
            "dust_and_scratches",
            json!({"radius": 2.0, "threshold": 0.05}),
            ONE_LEVEL,
        ),
        row("edge_detect", json!({}), ONE_LEVEL),
        row("emboss", json!({}), ONE_LEVEL),
        row("pixelate", json!({"block": 4}), ONE_LEVEL),
        row("add_noise", json!({"amount": 0.2, "seed": 7}), ONE_LEVEL),
        row("box_blur", json!({"radius": 3}), ONE_LEVEL),
        row(
            "surface_blur",
            json!({"radius": 3, "threshold": 40.0}),
            ONE_LEVEL,
        ),
        row(
            "add_grain",
            json!({"amount": 60, "size": 1.5, "roughness": 30, "seed": 7}),
            ONE_LEVEL,
        ),
        row(
            "radial_blur",
            json!({"amount": 12, "method": "spin", "quality": "good"}),
            ONE_LEVEL,
        ),
        row("clouds", json!({"seed": 5}), ONE_LEVEL),
        row("difference_clouds", json!({"seed": 5}), ONE_LEVEL),
        row(
            "offset",
            json!({"horizontal": 5, "vertical": -3, "undefined_areas": "wrap"}),
            EXACT,
        ),
        row(
            "motion_blur",
            json!({"angle": 30.0, "distance": 6}),
            ONE_LEVEL,
        ),
        row(
            "lens_blur",
            json!({"radius": 3, "brightness": 20.0, "threshold": 200.0}),
            ONE_LEVEL,
        ),
        row(
            "tilt_shift",
            json!({"radius": 4, "focus": 0.2, "transition": 0.3}),
            ONE_LEVEL,
        ),
        row(
            "lens_correction",
            json!({"distortion": 40, "chromatic_red_cyan": 60, "vignette_amount": -30,
                "angle": 5.0, "scale": 105}),
            ONE_LEVEL,
        ),
        row(
            "presence",
            json!({"texture": 60, "clarity": -40, "dehaze": 30}),
            ONE_LEVEL,
        ),
        row(
            "develop_filter",
            json!({"temperature": 30, "tint": -20, "exposure": 0.4, "contrast": 25,
                "highlights": -40, "shadows": 35, "whites": 15, "blacks": -10,
                "texture": 30, "clarity": 20, "dehaze": 15, "vibrance": 25,
                "saturation": -10}),
            ONE_LEVEL,
        ),
        row(
            "smart_sharpen",
            json!({"amount": 150, "radius": 1.5, "noise": 20, "remove": "lens",
                "shadow_fade": 40, "highlight_fade": 30, "shadow_radius": 2}),
            ONE_LEVEL,
        ),
    ]
}

/// The 24 names the app admits as spatial filters at 16 bits
/// (`SelectedAdjustment.spatial`), restated by hand: nothing reads the Swift
/// set from here, so a name added there must be added to this list and given
/// a row in the same change. The test below only holds the table to this list.
const SPATIAL: [&str; 24] = [
    "blur",
    "sharpen",
    "unsharp_mask",
    "high_pass",
    "reduce_noise",
    "dust_and_scratches",
    "edge_detect",
    "emboss",
    "pixelate",
    "add_noise",
    "box_blur",
    "surface_blur",
    "add_grain",
    "radial_blur",
    "clouds",
    "difference_clouds",
    "offset",
    "motion_blur",
    "lens_blur",
    "tilt_shift",
    "lens_correction",
    "presence",
    "develop_filter",
    "smart_sharpen",
];

/// An opaque photographic plate: smooth gradients, a hard edge, texture and
/// isolated specks, so every kernel has something to act on.
fn plate() -> Vec<u8> {
    (0..W * H)
        .flat_map(|i| {
            let (x, y) = (i % W, i / W);
            let ramp = 30 + x * 5 + y * 3;
            let edge = if x > W / 2 { 60 } else { 0 };
            let texture = (x * 7 + y * 13) % 17;
            let speck = if i % 37 == 0 { 90 } else { 0 };
            let value = |offset: u32| ((ramp + edge + texture + speck + offset) % 256) as u8;
            [value(0), value(45), value(110), 255]
        })
        .collect()
}

fn widen(pixels: &[u8]) -> Vec<[u16; 4]> {
    pixels
        .chunks_exact(4)
        .map(|p| [0, 1, 2, 3].map(|c| u16::from(p[c]) * 257))
        .collect()
}

fn take(out: *mut DrImage, img: *mut DrImage) -> Option<Vec<u8>> {
    let result = (!out.is_null()).then(|| {
        let p = unsafe { dr_image_pixels_rgba(out) };
        assert!(!p.is_null());
        unsafe { std::slice::from_raw_parts(p, (W * H * 4) as usize) }.to_vec()
    });
    unsafe {
        dr_image_free(out);
        dr_image_free(img);
    }
    result
}

/// The 8-bit result: the op's own byte export for the five that have one,
/// `dr_image_filter_op` for the nineteen it dispatches.
fn byte_filter(input: &[u8], op: &str, params: &Value) -> Vec<u8> {
    let img = unsafe { dr_image_from_rgba(input.as_ptr(), W, H) };
    assert!(!img.is_null());
    let number = |key: &str| params[key].as_f64().expect("a numeric parameter");
    let out = match op {
        "sharpen" => unsafe { dr_image_sharpen(img, number("amount") as f32) },
        "edge_detect" => unsafe { dr_image_edge_detect(img) },
        "emboss" => unsafe { dr_image_emboss(img) },
        "pixelate" => unsafe { dr_image_pixelate(img, number("block") as u32) },
        "add_noise" => unsafe {
            dr_image_noise(
                img,
                number("amount") as f32,
                params["seed"].as_u64().expect("a seed"),
            )
        },
        _ => {
            let name = CString::new(op).unwrap();
            let text = CString::new(params.to_string()).unwrap();
            let mut err = ptr::null_mut();
            let out = unsafe {
                dr_image_filter_op(img, name.as_ptr(), text.as_ptr(), ptr::null(), &mut err)
            };
            if !err.is_null() {
                let message = unsafe { CStr::from_ptr(err) }
                    .to_string_lossy()
                    .into_owned();
                unsafe { dr_string_free(err) };
                panic!("{op} at 8 bits: {message}");
            }
            out
        }
    };
    take(out, img).unwrap_or_else(|| panic!("{op} at 8 bits answered no result"))
}

/// The 16-bit result on the ×257 plate, through the selected-edit path.
fn wide_filter(input: &[u8], op: &str, params: &Value) -> Vec<[u16; 4]> {
    let d = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(W, H, widen(input), IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let e = SelectedEdit::parse(&json!({"layer": 0, "op": op, "params": params}).to_string())
        .unwrap_or_else(|e| panic!("{op} at 16 bits: {e}"));
    let out = d
        .selected_edit(&e, &[], REFERENCE_WORKING_BYTES, 0, None)
        .unwrap_or_else(|e| panic!("{op} at 16 bits: {e}"))
        .unwrap_or_else(|| panic!("{op} at 16 bits answered no change"));
    out.layer_source(0).unwrap().to_rgba16().unwrap()
}

/// The 16-bit code as an 8-bit level, rounded.
fn narrowed(code: u16) -> f64 {
    (f64::from(code) / 257.0).round()
}

#[test]
fn the_table_holds_every_spatial_name_once() {
    let table: Vec<&str> = rows().iter().map(|r| r.op).collect();
    assert_eq!(table, SPATIAL);
}

#[test]
fn every_spatial_filter_agrees_across_depths() {
    let input = plate();
    let mut failures = Vec::new();
    for row in rows() {
        let byte = byte_filter(&input, row.op, &row.params);
        let wide = wide_filter(&input, row.op, &row.params);
        assert_ne!(
            byte, input,
            "{}: the 8-bit filter changed the plate",
            row.op
        );
        assert_ne!(
            wide,
            widen(&input),
            "{}: the 16-bit filter changed the plate",
            row.op
        );
        let mut worst = 0.0f64;
        for (i, p) in wide.iter().enumerate() {
            for c in 0..4 {
                worst = worst.max((narrowed(p[c]) - f64::from(byte[i * 4 + c])).abs());
            }
        }
        if worst > row.bound {
            failures.push(format!(
                "{}: {worst} levels apart, bound {}",
                row.op, row.bound
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
