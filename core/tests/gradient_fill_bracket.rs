//! A Gradient Fill LAYER's Classic term is held to each
//! segment's bracketing stops (`style_gradient::fill_bound`, the FILL CLAMP
//! of `gradient_map_678_take.py`'s confirm take), deliberately re-rendering a
//! fill whose spline overshot a stop, while a Gradient Overlay keeps the
//! unbounded spline (no take measured it).
//!
//! Each fill is the `gradient` artwork component rendered by
//! `dr_artwork_render` over a 256 × 256 sRGB canvas, Linear at 0°, scale 1,
//! and published by `dr_doc_artwork` (8 bits) or `dr_precision_doc_artwork`
//! (16 bits); it is read on row 128, where the ramp parameter of column `x`
//! is `t = x / 256` (the pixel's top-left corner, the identified Linear
//! sample). The oracle is `common::bracket_ramp` per channel, written from
//! the runner's declared BRACKET law; the unbounded one is
//! `common::smooth_ramp`.

use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::{dr_artwork_render, dr_doc_artwork, dr_precision_doc_artwork};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common::{bracket_ramp, layer_pixels, smooth_ramp};
use crate::identify_578::render8;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const SIZE: u32 = 256;
const ROW: u32 = 128;

/// One stop: position, sRGB colour in 0..1, colour midpoint.
type Stop = (f64, [f64; 3], f64);

/// The confirm take's N4 stops at their sent locations (whole 1/4096ths).
fn n4(midpoints: [f64; 4]) -> Vec<Stop> {
    let c = |r: u8, g: u8, b: u8| [r, g, b].map(|v| f64::from(v) / 255.0);
    let at = |loc: u32| f64::from(loc) / 4096.0;
    vec![
        (at(0), c(40, 200, 220), midpoints[0]),
        (at(1229), c(120, 80, 30), midpoints[1]),
        (at(2458), c(60, 150, 90), midpoints[2]),
        (at(4096), c(250, 20, 60), midpoints[3]),
    ]
}

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

/// The `GradientFill` object: opaque stops, Linear 0°, `smoothness` only
/// when not 1, a `midpoint` only when not 0.5.
fn gradient_json(stops: &[Stop], smoothness: f64) -> Value {
    let list: Vec<Value> = stops
        .iter()
        .map(|&(position, color, midpoint)| {
            let v = color.map(|x| (x * 255.0).round() as u8);
            let mut stop = json!({"position": position, "opacity": 1.0,
                "color": format!("#{:02x}{:02x}{:02x}", v[0], v[1], v[2])});
            if midpoint != 0.5 {
                stop["midpoint"] = json!(midpoint);
            }
            stop
        })
        .collect();
    let mut out = json!({"stops": list, "style": "linear", "angle": 0.0, "scale": 1.0,
        "reverse": false, "align_with_layer": false});
    if smoothness != 1.0 {
        out["smoothness"] = json!(smoothness);
    }
    out
}

fn render_fill(stops: &[Stop], smoothness: f64) -> PrecisionImage {
    let colors: Vec<[f64; 3]> = stops.iter().map(|s| s.1).collect();
    let components = json!([{"kind": "gradient",
        "gradient": gradient_json(stops, smoothness), "colors": colors}]);
    let description = CString::new(components.to_string()).unwrap();
    let profile = IccProfile::srgb();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_artwork_render(
            SIZE,
            SIZE,
            description.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    assert!(!out.is_null());
    *unsafe { Box::from_raw(out) }
}

fn fill_description(stops: &[Stop], smoothness: f64) -> CString {
    let text = json!({"type": "fill", "version": 1, "canvas": [SIZE, SIZE],
        "fill": "gradient", "gradient": gradient_json(stops, smoothness)});
    CString::new(text.to_string()).unwrap()
}

/// Row 128 of the fill layer published at `depth`, R, G, B in 8-bit codes
/// (16-bit samples / 257).
fn fill_row(stops: &[Stop], smoothness: f64, depth: u32) -> Vec<[f64; 3]> {
    let image = render_fill(stops, smoothness);
    let name = CString::new("Fill").unwrap();
    let text = fill_description(stops, smoothness);
    let mut err = ptr::null_mut();
    let start = (ROW * SIZE) as usize;
    if depth == 8 {
        let doc = DrDocument::from_pixels(RgbaImage::from_pixel(SIZE, SIZE, Rgba([0, 0, 0, 255])));
        let out = unsafe {
            dr_doc_artwork(
                &doc,
                0,
                false,
                &image,
                0,
                0,
                name.as_ptr(),
                text.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert_eq!(owned(err), None);
        assert!(!out.is_null());
        let published = unsafe { Box::from_raw(out) };
        let px = layer_pixels(&*published, 1);
        return px[start * 4..(start + SIZE as usize) * 4]
            .chunks_exact(4)
            .map(|p| [0, 1, 2].map(|c| f64::from(p[c])))
            .collect();
    }
    let doc = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            SIZE,
            SIZE,
            vec![[0, 0, 0, 65535]; (SIZE * SIZE) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Default::default(),
        Default::default(),
    )
    .unwrap();
    let out = unsafe {
        dr_precision_doc_artwork(
            &doc,
            0,
            false,
            &image,
            0,
            0,
            name.as_ptr(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    assert!(!out.is_null());
    let published = *unsafe { Box::from_raw(out) };
    let samples = published.layer_source(1).unwrap().to_rgba16().ok().unwrap();
    samples[start..start + SIZE as usize]
        .iter()
        .map(|p| [0, 1, 2].map(|c| f64::from(p[c]) / 257.0))
        .collect()
}

/// Channel `c` of `stops` as the (position, value) list the ramps take.
fn channel(stops: &[Stop], c: usize) -> Vec<(f64, f64)> {
    stops.iter().map(|s| (s.0, s.1[c])).collect()
}

#[test]
fn a_fill_layers_classic_term_is_held_to_its_bracketing_stops() {
    let stops = n4([0.5; 4]);
    for smoothness in [1.0, 0.5] {
        for depth in [8, 16] {
            let row = fill_row(&stops, smoothness, depth);
            let (mut apart, mut worst) = (0.0f64, 0.0f64);
            for (x, px) in row.iter().enumerate() {
                let t = x as f64 / 256.0;
                for (c, &got) in px.iter().enumerate() {
                    let ramp = channel(&stops, c);
                    let want = bracket_ramp(&ramp, t, smoothness) * 255.0;
                    let unbounded = smooth_ramp(&ramp, t, smoothness).clamp(0.0, 1.0) * 255.0;
                    worst = worst.max((got - want).abs());
                    apart = apart.max((got - unbounded).abs());
                }
            }
            // One code at 8 bits; at 16, 0.05 codes (the chromatic stops'
            // conversion through the document profile and the rounding), far
            // inside the 3 codes the bound moves the spline.
            let bound = if depth == 8 { 1.0 } else { 0.05 };
            eprintln!("smoothness {smoothness} depth {depth}: worst {worst:.6}, {apart:.3} apart");
            assert!(
                worst <= bound,
                "smoothness {smoothness} depth {depth}: {worst} codes from the oracle"
            );
            // The deliberate re-render: the unbounded spline is gone.
            if smoothness == 1.0 {
                assert!(
                    apart >= 3.0,
                    "depth {depth}: {apart} codes from the unbounded spline"
                );
            }
        }
    }
}

/// The confirm take's four Gradient Fill rows (`map_678_take1.json`,
/// `f_n4_i100_*` and `f_seg_n4_i0_*`) at the runner's declared 2 / 0.5 codes.
/// The SEG rows' midpoints are shifted as `psd_layer_effects_fx`'s `Grdn`
/// reads them: our stop `i` carries Photoshop stop `i + 1`'s `Mdpn`.
#[test]
fn the_confirm_takes_fill_rows_meet_the_bound() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/gradient_identify_673/map_678_take1.json");
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(fixture["verdicts"]["fill_clamp"], "CLAMPED");
    let mut seen = 0;
    for row in fixture["rows"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        if !name.starts_with("f_") {
            continue;
        }
        seen += 1;
        let mdpn: Vec<f64> = row["midpoints"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m.as_f64().unwrap() / 100.0)
            .collect();
        let shifted = std::array::from_fn(|i| mdpn.get(i + 1).copied().unwrap_or(0.5));
        let stops = n4(shifted);
        let smoothness = row["smoothness"].as_f64().unwrap();
        let depth = row["depth"].as_u64().unwrap() as u32;
        let ours = fill_row(&stops, smoothness, depth);
        let theirs = row["codes"].as_array().unwrap();
        assert_eq!(ours.len(), theirs.len(), "{name}");
        let (mut max, mut sum) = (0.0f64, 0.0f64);
        for (o, t) in ours.iter().zip(theirs) {
            for c in 0..3 {
                let e = (o[c] - t[c].as_f64().unwrap()).abs();
                max = max.max(e);
                sum += e;
            }
        }
        let mean = sum / (3 * ours.len()) as f64;
        eprintln!("{name}: max {max:.6} mean {mean:.6}");
        assert!(
            max <= 2.0 && mean <= 0.5,
            "{name}: {max} / {mean} misses 2 / 0.5"
        );
    }
    assert_eq!(seen, 4, "the four f_* rows");
}

/// A Gradient Overlay over the same stops keeps the unbounded spline: no
/// take measured the overlay's bound.
#[test]
fn a_gradient_overlay_keeps_the_unbounded_spline() {
    let stops = n4([0.5; 4]);
    let mut gradient = gradient_json(&stops, 1.0);
    gradient["align_with_layer"] = json!(true);
    let style = json!({"effects": [{"type": "gradient_overlay", "blend": "normal",
        "opacity": 1.0, "gradient": gradient}]});
    let red = render8(SIZE, SIZE, &vec![true; (SIZE * SIZE) as usize], &style);
    let ramp = channel(&stops, 0);
    let mut apart = 0.0f64;
    for x in 0..SIZE as usize {
        let t = x as f64 / 256.0;
        let got = red[ROW as usize * SIZE as usize + x];
        let want = smooth_ramp(&ramp, t, 1.0).clamp(0.0, 1.0) * 255.0;
        assert!(
            (got - want).abs() <= 1.0,
            "x {x}: {got} vs the unbounded {want}"
        );
        apart = apart.max((bracket_ramp(&ramp, t, 1.0) - smooth_ramp(&ramp, t, 1.0)).abs() * 255.0);
    }
    assert!(
        apart >= 1.5,
        "the scene separates the bound on red by {apart} codes"
    );
}
