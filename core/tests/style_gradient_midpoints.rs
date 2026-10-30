//! Gradient stop midpoints in the layer-style and fill-layer gradient.
//!
//! The style JSON's per-stop `midpoint` and `opacity_midpoint` (fractions,
//! Photoshop's 5..95 %) are clamped into 0.05..=0.95, rounded to 0.01 and
//! written only when not 0.5; after the sort the last stop's are reset to
//! 0.5. A glow's gradient and a legacy Gradient Map (no `method`) ignore
//! them; a `method: "classic"` map reads them
//! (`gradient_map_classic.rs`).
//!
//! The renders are held, through the FFI, to `common::midpoint_t` (the take-2
//! description's piecewise-linear move of `t` inside its bracket, written in
//! the test helpers and never read off the core) under `common::smooth_ramp`
//! for the colour and the linear ramp for the opacity: a Gradient Overlay at
//! 8 and 16 bits (at Smoothness 100 % and 50 %), a Stroke Gradient, a
//! Gradient Fill layer (`dr_artwork_render`, published by `dr_doc_artwork`
//! and `dr_precision_doc_artwork`), a 32-bit float overlay and a CMYK
//! artwork's ink; the two fill layers' Classic term is held to its
//! bracketing stops (`common::bracket_ramp`, the FILL CLAMP). Linear at 0°
//! over a full-canvas `w`-wide box samples the
//! pixel's top-left corner, so pixel `x` reads `t = x / w`.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_adjust::dr_image_adjust_op;
use doctored_core::ffi_artwork::{dr_artwork_render, dr_doc_artwork, dr_precision_doc_artwork};
use doctored_core::ffi_cmyk_artwork::dr_precision_doc_cmyk_artwork;
use doctored_core::ffi_gradient::dr_gradient_midpoint_fraction;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_structure_fixture::{cmyk_layers, free as free_doc, varied, RgbLayer};
use crate::common::{
    bracket_ramp, ffi_style, free, layer_pixels, midpoint_t, open_pattern, pixels, rect_layer_doc,
    smooth_ramp, styled, take_err_string,
};
use crate::identify_578::{render16, render32_alpha, render8};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 64;
const H: u32 = 4;

/// An overlay's opacity as a byte coverage (`style_fx_gradient_overlay`):
/// half a code, plus the deeper render's own rounding.
const OPACITY_TOL: f64 = 0.5 + 2.0 / 257.0;

/// Three gray stops (position, value).
const THREE: [(f64, f64); 3] = [(0.0, 0.0), (0.3, 1.0), (1.0, 64.0 / 255.0)];

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

fn hex(v: f64) -> String {
    let code = (v * 255.0).round() as u8;
    format!("#{code:02x}{code:02x}{code:02x}")
}

/// Gray opaque stops with colour midpoints `mids` (written only off 0.5).
fn gray_stops(stops: &[(f64, f64)], mids: &[f64]) -> Value {
    Value::from(
        stops
            .iter()
            .zip(mids)
            .map(|(&(p, v), &m)| {
                let mut s = json!({"position": p, "color": hex(v), "opacity": 1});
                if m != 0.5 {
                    s["midpoint"] = json!(m);
                }
                s
            })
            .collect::<Vec<_>>(),
    )
}

/// White stops whose opacities are `stops`' values, with opacity midpoints.
fn opacity_stops(stops: &[(f64, f64)], mids: &[f64]) -> Value {
    Value::from(
        stops
            .iter()
            .zip(mids)
            .map(|(&(p, a), &m)| {
                let mut s = json!({"position": p, "color": "#ffffff", "opacity": a});
                if m != 0.5 {
                    s["opacity_midpoint"] = json!(m);
                }
                s
            })
            .collect::<Vec<_>>(),
    )
}

fn linear_gradient(stops: Value, smoothness: f64) -> Value {
    let mut g = json!({"stops": stops, "style": "linear", "angle": 0, "scale": 1,
        "reverse": false, "align_with_layer": true});
    if smoothness != 1.0 {
        g["smoothness"] = json!(smoothness);
    }
    g
}

fn overlay(gradient: Value) -> Value {
    json!({"effects": [{"type": "gradient_overlay", "blend": "normal", "opacity": 1,
        "gradient": gradient}]})
}

/// Pixel `x`'s `t` (module doc).
fn t_at(x: u32, w: u32) -> f64 {
    f64::from(x) / f64::from(w)
}

/// The colour oracle at `t`: `smooth_ramp` at the moved `t`.
fn want_color(stops: &[(f64, f64)], mids: &[f64], t: f64, k: f64) -> f64 {
    smooth_ramp(stops, midpoint_t(stops, mids, t), k).clamp(0.0, 1.0)
}

/// A Gradient Fill LAYER's colour oracle at `t`: `bracket_ramp` at the moved
/// `t` (its Classic term held to the bracketing stops).
fn want_fill_color(stops: &[(f64, f64)], mids: &[f64], t: f64, k: f64) -> f64 {
    bracket_ramp(stops, midpoint_t(stops, mids, t), k)
}

/// The opacity oracle at `t`: the linear ramp at the moved `t`.
fn want_opacity(stops: &[(f64, f64)], mids: &[f64], t: f64) -> f64 {
    smooth_ramp(stops, midpoint_t(stops, mids, t), 0.0)
}

fn check_rows(got: &[f64], want: impl Fn(u32) -> f64, w: u32, h: u32, tol: f64, what: &str) {
    for y in 0..h {
        for x in 0..w {
            let (g, e) = (got[(y * w + x) as usize], want(x) * 255.0);
            assert!((g - e).abs() <= tol, "{what} ({x},{y}): {g} against {e}");
        }
    }
}

// ------------------------------------------------------------------- JSON --

fn effect0(doc: &DrDocument) -> Value {
    let text = ffi_style(doc as *const DrDocument, 1).expect("a style");
    serde_json::from_str::<Value>(&text).unwrap()["effects"][0].clone()
}

#[test]
fn midpoints_are_written_off_50_clamped_rounded_and_the_last_reset() {
    let doc = rect_layer_doc((24, 16), (4, 4, 12, 8), [200, 40, 40, 255]);
    let read = |stops: Value| {
        effect0(&styled(
            &doc,
            1,
            &overlay(json!({"stops": stops})).to_string(),
        ))["gradient"]["stops"]
            .clone()
    };
    let two = |m: Value, o: Value| {
        json!([{"position": 0, "color": "#000000", "opacity": 1, "midpoint": m,
            "opacity_midpoint": o},
            {"position": 1, "color": "#ffffff", "opacity": 1, "midpoint": 0.3,
            "opacity_midpoint": 0.7}])
    };
    let s = read(two(json!(0.5), json!(0.5)));
    for stop in s.as_array().unwrap() {
        assert!(stop.get("midpoint").is_none(), "{s}");
        assert!(stop.get("opacity_midpoint").is_none(), "{s}");
    }
    let s = read(two(json!(0.25), json!(0.7)));
    assert_eq!(s[0]["midpoint"], json!(0.25), "{s}");
    assert_eq!(s[0]["opacity_midpoint"], json!(0.7), "{s}");
    assert!(
        s[1].get("midpoint").is_none(),
        "the last stop's resets: {s}"
    );
    assert!(s[1].get("opacity_midpoint").is_none(), "{s}");
    let s = read(two(json!(0.01), json!(0.99)));
    assert_eq!(s[0]["midpoint"], json!(0.05), "clamped: {s}");
    assert_eq!(s[0]["opacity_midpoint"], json!(0.95), "clamped: {s}");
    let s = read(two(json!(0.333), json!(0.6789)));
    assert_eq!(s[0]["midpoint"], json!(0.33), "whole percents: {s}");
    assert_eq!(s[0]["opacity_midpoint"], json!(0.68), "{s}");
    // The last stop AFTER the sort: a stop listed first but placed last.
    let s = read(json!([
        {"position": 1, "color": "#ffffff", "opacity": 1, "midpoint": 0.2},
        {"position": 0, "color": "#000000", "opacity": 1, "midpoint": 0.4}]));
    assert_eq!(s[0]["midpoint"], json!(0.4), "{s}");
    assert!(s[1].get("midpoint").is_none(), "{s}");
    // Not a number: refused by name.
    let bad = overlay(json!({"stops": [
        {"position": 0, "color": "#000000", "midpoint": "half"},
        {"position": 1, "color": "#ffffff"}]}));
    let refused = doctored_core::style::LayerStyle::from_json(&bad.to_string());
    assert_eq!(
        refused.err().as_deref(),
        Some("effects[0].gradient.stops[0].midpoint must be a number")
    );
    // A stroke's gradient carries them, whatever its fill type.
    let stroke = json!({"effects": [{"type": "stroke", "size": 3, "fill_type": "color",
        "gradient": {"stops": two(json!(0.4), json!(0.6))}}]});
    let e = effect0(&styled(&doc, 1, &stroke.to_string()));
    assert_eq!(e["gradient"]["stops"][0]["midpoint"], json!(0.4), "{e}");
    assert_eq!(
        e["gradient"]["stops"][0]["opacity_midpoint"],
        json!(0.6),
        "{e}"
    );
}

#[test]
fn explicit_50_keys_render_and_write_as_none() {
    let inside = vec![true; (W * H) as usize];
    let plain = overlay(linear_gradient(gray_stops(&THREE, &[0.5; 3]), 1.0));
    let mut explicit = plain.clone();
    for stop in explicit["effects"][0]["gradient"]["stops"]
        .as_array_mut()
        .unwrap()
    {
        stop["midpoint"] = json!(0.5);
        stop["opacity_midpoint"] = json!(0.5);
    }
    assert_eq!(
        render8(W, H, &inside, &plain),
        render8(W, H, &inside, &explicit)
    );
    assert_eq!(
        render16(W, H, &inside, &plain),
        render16(W, H, &inside, &explicit)
    );
    let doc = rect_layer_doc((24, 16), (4, 4, 12, 8), [200, 40, 40, 255]);
    assert_eq!(
        ffi_style(&styled(&doc, 1, &plain.to_string()) as *const DrDocument, 1),
        ffi_style(
            &styled(&doc, 1, &explicit.to_string()) as *const DrDocument,
            1
        ),
        "the same canonical JSON"
    );
}

#[test]
fn glows_and_gradient_maps_ignore_midpoints() {
    let doc = rect_layer_doc((40, 32), (10, 8, 20, 16), [200, 40, 40, 255]);
    let glow = |mids: &[f64]| {
        json!({"effects": [{"type": "outer_glow", "fill_type": "gradient", "size": 8,
            "gradient": {"stops": gray_stops(&THREE, mids)}}]})
        .to_string()
    };
    let with = styled(&doc, 1, &glow(&[0.2, 0.8, 0.5]));
    let without = styled(&doc, 1, &glow(&[0.5; 3]));
    assert_eq!(
        ffi_style(&with as *const DrDocument, 1),
        ffi_style(&without as *const DrDocument, 1),
        "a glow writes none"
    );
    assert_eq!(with.flattened().unwrap(), without.flattened().unwrap());

    let dir = TempDir::new().unwrap();
    let src = RgbaImage::from_fn(32, 8, |x, y| {
        let v = (x * 8 + y) as u8;
        Rgba([v, 255 - v, v / 2, 255])
    });
    let map = |mids: &[f64]| {
        let img = open_pattern(&dir, &format!("map{}.png", mids[0]), &src);
        let params =
            json!({"gradient": {"stops": gray_stops(&THREE, mids)}, "dither": false}).to_string();
        let (op, params) = (
            CString::new("gradient_map").unwrap(),
            CString::new(params).unwrap(),
        );
        let mut err = ptr::null_mut();
        let out = unsafe { dr_image_adjust_op(img, op.as_ptr(), params.as_ptr(), &mut err) };
        assert!(!out.is_null(), "{}", take_err_string(err));
        let bytes = pixels(out);
        free(out);
        free(img);
        bytes
    };
    assert_eq!(
        map(&[0.2, 0.8, 0.5]),
        map(&[0.5; 3]),
        "a legacy map ignores them"
    );
}

// ---------------------------------------------------------------- overlay --

#[test]
fn an_overlay_moves_its_colour_and_opacity_through_the_midpoints() {
    let inside = vec![true; (W * H) as usize];
    for (mids, k) in [
        ([0.25, 0.75, 0.5], 1.0),
        ([0.75, 0.25, 0.5], 1.0),
        ([0.25, 0.75, 0.5], 0.5),
        ([0.1, 0.9, 0.5], 0.0),
    ] {
        let style = overlay(linear_gradient(gray_stops(&THREE, &mids), k));
        let want = |x: u32| want_color(&THREE, &mids, t_at(x, W), k);
        let what = format!("colour {mids:?} k {k}");
        check_rows(&render8(W, H, &inside, &style), want, W, H, 1.0, &what);
        check_rows(
            &render16(W, H, &inside, &style),
            want,
            W,
            H,
            2.0 / 257.0,
            &what,
        );
    }
    // Opacity: white stops over the black layer read the opacity alone. An
    // overlay's opacity is a byte coverage at every depth
    // (`style_fx_gradient_overlay`), so 16 bits is within half a code.
    let ops = [(0.0, 1.0), (0.6, 0.2), (1.0, 0.9)];
    for mids in [[0.3, 0.8, 0.5], [0.8, 0.3, 0.5]] {
        let style = overlay(linear_gradient(opacity_stops(&ops, &mids), 1.0));
        let want = |x: u32| want_opacity(&ops, &mids, t_at(x, W));
        let what = format!("opacity {mids:?}");
        check_rows(&render8(W, H, &inside, &style), want, W, H, 1.0, &what);
        check_rows(
            &render16(W, H, &inside, &style),
            want,
            W,
            H,
            OPACITY_TOL,
            &what,
        );
    }
}

#[test]
fn a_stroke_gradient_moves_through_its_midpoints() {
    // An Inside stroke wider than the layer covers all of it, so its colour
    // is the gradient at every pixel of the canvas box.
    let inside = vec![true; (W * H) as usize];
    let mids = [0.25, 0.75, 0.5];
    let style = json!({"effects": [{"type": "stroke", "size": 100, "position": "inside",
        "fill_type": "gradient", "gradient": linear_gradient(gray_stops(&THREE, &mids), 1.0)}]});
    let want = |x: u32| want_color(&THREE, &mids, t_at(x, W), 1.0);
    check_rows(&render8(W, H, &inside, &style), want, W, H, 1.0, "stroke 8");
    check_rows(
        &render16(W, H, &inside, &style),
        want,
        W,
        H,
        2.0 / 257.0,
        "stroke 16",
    );
}

// ------------------------------------------------------------ fill layers --

fn fill_gradient(mids: &[f64], k: f64) -> Value {
    let mut g = linear_gradient(gray_stops(&THREE, mids), k);
    g["align_with_layer"] = json!(false);
    g
}

fn render_fill(gradient: &Value) -> PrecisionImage {
    let colors: Vec<[f64; 3]> = THREE.iter().map(|s| [s.1; 3]).collect();
    let components = CString::new(
        json!([{"kind": "gradient", "gradient": gradient, "colors": colors}]).to_string(),
    )
    .unwrap();
    let profile = IccProfile::srgb();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_artwork_render(
            W,
            H,
            components.as_ptr(),
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

#[test]
fn a_gradient_fill_layer_moves_through_its_midpoints_at_8_and_16_bits() {
    for (mids, k) in [([0.25, 0.75, 0.5], 1.0), ([0.75, 0.25, 0.5], 0.5)] {
        let gradient = fill_gradient(&mids, k);
        let image = render_fill(&gradient);
        let want = |x: u32| want_fill_color(&THREE, &mids, t_at(x, W), k);
        let red16: Vec<f64> = image
            .to_rgba16()
            .unwrap()
            .iter()
            .map(|p| f64::from(p[0]) / 257.0)
            .collect();
        check_rows(&red16, want, W, H, 2.0 / 257.0, "fill render");

        let description = CString::new(
            json!({"type": "fill", "version": 1, "fill": "gradient", "gradient": gradient,
                "canvas": [W, H]})
            .to_string(),
        )
        .unwrap();
        let name = CString::new("Gradient Fill").unwrap();
        let doc = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([0, 0, 0, 255])));
        let mut err = ptr::null_mut();
        let byte = unsafe {
            dr_doc_artwork(
                &doc,
                0,
                false,
                &image,
                0,
                0,
                name.as_ptr(),
                description.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert_eq!(owned(err), None);
        assert!(!byte.is_null());
        let byte = unsafe { Box::from_raw(byte) };
        let red8: Vec<f64> = layer_pixels(&*byte, 1)
            .chunks_exact(4)
            .map(|p| f64::from(p[0]))
            .collect();
        check_rows(&red8, want, W, H, 1.0, "fill published at 8 bits");

        let typed = PrecisionDocument::from_image(
            PrecisionImage::from_rgba16(
                W,
                H,
                vec![[0, 0, 0, 65535]; (W * H) as usize],
                IccProfile::srgb(),
                None,
            )
            .unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_artwork(
                &typed,
                0,
                false,
                &image,
                0,
                0,
                name.as_ptr(),
                description.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert_eq!(owned(err), None);
        assert!(!out.is_null());
        let published = unsafe { Box::from_raw(out) };
        let red16: Vec<f64> = published
            .layer_source(1)
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap()
            .into_iter()
            .map(|p| f64::from(p[0]) / 257.0)
            .collect();
        check_rows(&red16, want, W, H, 2.0 / 257.0, "fill published at 16 bits");
    }
}

// --------------------------------------------------------- float and CMYK --

#[test]
fn a_32_bit_overlay_moves_through_its_midpoints() {
    // Stops of 0 and 1 are the same in linear light and encoded.
    let stops = [(0.0, 0.0), (0.4, 1.0), (1.0, 0.0)];
    let mids = [0.25, 0.75, 0.5];
    let style = overlay(linear_gradient(gray_stops(&stops, &mids), 1.0));
    let alpha = vec![1.0f32; (W * H) as usize];
    let want = |x: u32| want_color(&stops, &mids, t_at(x, W), 1.0);
    check_rows(
        &render32_alpha(W, H, &alpha, &style),
        want,
        W,
        H,
        0.01,
        "32-bit colour",
    );
    let ops = [(0.0, 1.0), (0.6, 0.2), (1.0, 0.9)];
    let omids = [0.3, 0.8, 0.5];
    let style = overlay(linear_gradient(opacity_stops(&ops, &omids), 1.0));
    let want = |x: u32| want_opacity(&ops, &omids, t_at(x, W));
    check_rows(
        &render32_alpha(W, H, &alpha, &style),
        want,
        W,
        H,
        OPACITY_TOL,
        "32-bit opacity",
    );
}

const INKS: [[f64; 4]; 3] = [
    [0.7, 0.2, 0.1, 0.05],
    [0.1, 0.55, 0.9, 0.0],
    [0.0, 0.3, 0.6, 0.2],
];

#[test]
fn a_cmyk_fill_moves_its_ink_through_the_midpoints() {
    let mids = [0.25, 0.75, 0.5];
    let mut gradient = fill_gradient(&mids, 1.0);
    // Opacity stops too: 1, 0.4, 1 with an opacity midpoint on the first.
    let ops = [(0.0, 1.0), (0.3, 0.4), (1.0, 1.0)];
    let omids = [0.7, 0.5, 0.5];
    for (k, stop) in gradient["stops"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        stop["opacity"] = json!(ops[k].1);
        if omids[k] != 0.5 {
            stop["opacity_midpoint"] = json!(omids[k]);
        }
    }
    let base = cmyk_layers(
        W,
        H,
        vec![RgbLayer {
            name: "Bottom",
            w: W,
            h: H,
            offset: (0, 0),
            pixels: varied(W, H, 3),
        }],
    );
    let components =
        CString::new(json!([{"kind": "gradient", "gradient": gradient, "inks": INKS}]).to_string())
            .unwrap();
    let description = CString::new(
        json!({"type": "fill", "version": 2, "ink": "cmyk", "fill": "gradient",
            "gradient": gradient, "stop_inks": INKS, "canvas": [W, H]})
        .to_string(),
    )
    .unwrap();
    let name = CString::new("G").unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_cmyk_artwork(
            base,
            0,
            false,
            0,
            W,
            H,
            components.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            0,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    free_doc(base);
    assert!(!out.is_null(), "{}", take_err_string(err));
    let samples = unsafe { (*out).cmyk_layer_samples(1).unwrap() };
    for y in 0..H {
        for x in 0..W {
            let s = samples[(y * W + x) as usize];
            let t = t_at(x, W);
            for c in 0..4 {
                let plate: Vec<(f64, f64)> =
                    THREE.iter().zip(&INKS).map(|(p, i)| (p.0, i[c])).collect();
                let want = want_fill_color(&plate, &mids, t, 1.0) * 65535.0;
                assert!(
                    (f64::from(s[c]) - want).abs() <= 2.0,
                    "plate {c} ({x},{y}): {} against {want}",
                    s[c]
                );
            }
            let want = want_opacity(&ops, &omids, t) * 65535.0;
            assert!(
                (f64::from(s[4]) - want).abs() <= 2.0,
                "alpha ({x},{y}): {} against {want}",
                s[4]
            );
        }
    }
    free_doc(out);
}

/// The gradient editor's strip draws through `dr_gradient_midpoint_fraction`,
/// so the export must be the described remap: held to
/// `common::midpoint_t` on one unit segment at three midpoints, 0.5 answering
/// `f` bit for bit, and the documented clamps.
#[test]
fn the_exported_remap_is_the_described_one_at_three_midpoints() {
    for m in [0.25_f64, 0.5, 0.75] {
        for k in 0..=100 {
            let f = k as f64 / 100.0;
            let want = midpoint_t(&[(0.0, 0.0), (1.0, 1.0)], &[m, 0.5], f);
            let got = dr_gradient_midpoint_fraction(f as f32, m as f32) as f64;
            assert!(
                (got - want).abs() <= 1e-6,
                "m {m} f {f}: export {got}, described {want}"
            );
        }
    }
    for k in 0..=100 {
        let f = k as f32 / 100.0;
        assert_eq!(dr_gradient_midpoint_fraction(f, 0.5).to_bits(), f.to_bits());
        assert_eq!(
            dr_gradient_midpoint_fraction(f, 0.01),
            dr_gradient_midpoint_fraction(f, 0.05),
            "a midpoint under 5 % clamps to 5 %"
        );
        assert_eq!(
            dr_gradient_midpoint_fraction(f, 0.99),
            dr_gradient_midpoint_fraction(f, 0.95),
            "a midpoint over 95 % clamps to 95 %"
        );
        assert_eq!(
            dr_gradient_midpoint_fraction(f, f32::NAN).to_bits(),
            f.to_bits(),
            "a NaN midpoint is 50 %"
        );
    }
    assert_eq!(dr_gradient_midpoint_fraction(-1.0, 0.3), 0.0);
    assert_eq!(dr_gradient_midpoint_fraction(2.0, 0.3), 1.0);
    assert_eq!(dr_gradient_midpoint_fraction(f32::NAN, 0.3), 0.0);
}
