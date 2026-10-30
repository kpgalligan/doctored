//! Edit ▸ Fill… / Stroke… on ONE colour channel of a 32-bit float layer
//! and above-white Fill/Stroke ink, black-box through the
//! request API.
//!
//! `paint_blend` with a `plane` writes the 30/59/11 luma of the ink's
//! components into that one component, against an independent f64 oracle
//! `b + (B(b, V) − b)·t` with `t = ink alpha × opacity × coverage`: every
//! built mode over an extended backdrop with negatives, the other components
//! and every uncovered pixel bit for bit, a trimmed layer that never grows,
//! pixels of stored alpha 0, 0.5 and 1 (Photoshop's measured `written` row:
//! each takes the write at `t`), a Stroke band, a Display P3 document, the
//! refusals, the locks, the targets that are no raster, and the cleared
//! description. Linear ink is used as is, up to 65504.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_lock::{LOCK_PIXELS, LOCK_TRANSPARENCY};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::{
    dr_precision_doc_artwork_description, dr_precision_doc_artwork_metadata,
};
use doctored_core::ffi_precision_edit::*;
use doctored_core::ffi_retained_source::dr_precision_doc_mixed_source_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::Metadata;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::tiff_float::FloatTiff;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::float_fixture::float_doc;
use crate::native_scalar as native;
use native::*;

const W: u32 = 11;
const H: u32 = 7;
const PLANES: [&str; 4] = ["red", "green", "blue", "alpha"];
/// Normal, Multiply, Darken, Lighten, Difference, Linear Dodge, Subtract.
const SCALAR_MODES: [u32; 7] = [0, 1, 6, 7, 8, 12, 13];
const DISSOLVE: u32 = 14;
const UNAVAILABLE: &str = "is unavailable in a 32-bit float document";
const LINEAR_RANGE: &str =
    "linear ink must be finite with red, green and blue in [0, 65504] and alpha in [0,1]";
const LINEAR_ONLY: &str = "linear ink is a 32-bit float document's paint and gradient colour";
const INK: [f64; 4] = [0.9, 0.2, 0.1, 0.8];

// ------------------------------------------------------------- fixtures --

/// Extended-range straight samples: zeros, negatives, above-white RGB,
/// alpha 0 with hidden RGB, partial alpha and alpha 1.
fn extended(width: u32, height: u32) -> Vec<[f32; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let v = ((x * 37 + y * 101) % 1000) as f32 / 1000.0;
            let r = if (x + y) % 9 == 0 { 0.0 } else { v * 6.0 - 1.0 };
            let g = if (x * y) % 7 == 3 { -0.25 } else { 1.0 - v };
            let b = ((x ^ y) % 97) as f32 / 4.0 - 2.0;
            let a = match (x + 2 * y) % 5 {
                0 => 0.0,
                1 | 2 => 1.0,
                k => k as f32 / 5.0,
            };
            [r, g, b, a]
        })
        .collect()
}

/// Columns of stored alpha 0, 0.5 and 1 over extended RGB.
fn alphas(width: u32, height: u32) -> Vec<[f32; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let a = [0.0, 0.5, 1.0][(x % 3) as usize];
            [x as f32 * 0.75 - 1.5, 2.5 - y as f32 * 0.5, -0.25, a]
        })
        .collect()
}

/// An sRGB float document holding one canvas-sized raster of `samples`.
fn float_document(width: u32, height: u32, samples: &[[f32; 4]]) -> PrecisionDocument {
    let tiff = FloatTiff {
        width,
        height,
        channels: 4,
        alpha: Some(false),
        samples: samples.iter().flatten().copied().collect(),
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let bytes = doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("float TIFF");
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("plane.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    assert_eq!(bits(&layer32(&doc, 0)), bits(samples));
    doc
}

fn layer32(d: &PrecisionDocument, i: usize) -> Vec<[f32; 4]> {
    d.layer_source(i).unwrap().to_rgba32().unwrap()
}

fn bits(pixels: &[[f32; 4]]) -> Vec<[u32; 4]> {
    pixels.iter().map(|p| p.map(f32::to_bits)).collect()
}

fn geometry(d: &PrecisionDocument, i: usize) -> ((u32, u32), (i32, i32)) {
    let layer = d.layer(i).unwrap();
    (layer.source().unwrap().dimensions(), layer.offset())
}

/// Fractional coverage holding 0, 65535 and intermediate weights.
fn coverage(n: u32, salt: u32) -> Vec<u16> {
    (0..n)
        .map(|i| match (i + salt) % 5 {
            0 => 0,
            1 => 65535,
            k => (k * 9001 + i * 7) as u16,
        })
        .collect()
}

fn plane_fill(plane: &str, color: [f64; 4], space: &str, opacity: f64, mode: u32) -> Value {
    json!({"op":"paint_blend","layer":0,"color":color,"color_space":space,
        "opacity":opacity,"blend_mode":mode,"plane":plane})
}

fn layer_fill(color: [f64; 4], space: &str, opacity: f64, mode: u32) -> Value {
    json!({"op":"paint_blend","layer":0,"color":color,"color_space":space,
        "opacity":opacity,"blend_mode":mode})
}

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

/// The plan's refusal for the request `text`, which must refuse.
fn plan_error_text(d: &PrecisionDocument, text: &str) -> String {
    let t = CString::new(text).unwrap();
    let mut e = ptr::null_mut();
    let p = unsafe { dr_precision_doc_edit_plan(d, t.as_ptr(), LIMIT, 0, &mut e) };
    assert!(
        owned(p).is_none(),
        "the plan was expected to refuse: {text}"
    );
    owned(e).expect("a refusal")
}

fn plan_error(d: &PrecisionDocument, v: Value) -> String {
    plan_error_text(d, &v.to_string())
}

/// The refusal of the request `text` from `dr_precision_doc_edit` asked on its
/// own, with no plan call before it and a whole-canvas coverage as input, and
/// then from the plan: the same sentence from both.
fn refusal_text(d: &PrecisionDocument, text: &str) -> String {
    let (w, h) = d.dimensions();
    let input = vec![65535u16; (w * h) as usize];
    let t = CString::new(text).unwrap();
    let mut e = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            d,
            t.as_ptr(),
            input.as_ptr(),
            input.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    assert!(out.is_null(), "the edit was expected to refuse: {text}");
    let message = owned(e).expect("an edit refusal");
    assert_eq!(plan_error_text(d, text), message, "{text}");
    message
}

fn refusal(d: &PrecisionDocument, v: Value) -> String {
    refusal_text(d, &v.to_string())
}

/// The edit's refusal for `v`, which must refuse after a successful plan.
fn edit_error(d: &PrecisionDocument, v: Value, input: &[u16]) -> String {
    plan(d, v.clone());
    let t = text(v);
    let mut e = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            d,
            t.as_ptr(),
            input.as_ptr(),
            input.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    assert!(out.is_null(), "the edit was expected to refuse");
    owned(e).expect("a refusal")
}

// --------------------------------------------------------------- oracle --

fn component(plane: &str) -> usize {
    PLANES.iter().position(|p| *p == plane).unwrap()
}

/// The value a colour-plane fill writes: 0.30 r + 0.59 g + 0.11 b of the
/// ink's components as given.
fn luma(color: [f64; 4]) -> f64 {
    0.30 * color[0] + 0.59 * color[1] + 0.11 * color[2]
}

/// `B(b, v)` of the scalar modes, unclamped.
fn blend(mode: u32, b: f64, v: f64) -> f64 {
    match mode {
        0 => v,
        1 => b * v,
        6 => b.min(v),
        7 => b.max(v),
        8 => (b - v).abs(),
        12 => b + v,
        13 => b - v,
        _ => unreachable!("not a scalar mode"),
    }
}

/// Component `k` after a plane fill of value `v` at strength `t`.
fn written(mode: u32, k: usize, b: f32, v: f64, t: f64) -> f64 {
    let (b, v) = (f64::from(b), if k == 3 { v.clamp(0.0, 1.0) } else { v });
    let out = t * blend(mode, b, v) + (1.0 - t) * b;
    if k == 3 {
        out.clamp(0.0, 1.0)
    } else {
        out
    }
}

fn close(actual: f32, expected: f64) -> bool {
    (f64::from(actual) - expected).abs() <= 1e-5 * expected.abs().max(1.0)
}

/// Every pixel of `after` against the oracle: component `k` where `t(i)` is
/// above 0, and every other sample bit for bit.
fn check(
    label: &str,
    before: &[[f32; 4]],
    after: &[[f32; 4]],
    mode: u32,
    k: usize,
    v: f64,
    t: impl Fn(usize) -> f64,
) {
    assert_eq!(before.len(), after.len(), "{label}");
    for (i, (was, got)) in before.iter().zip(after).enumerate() {
        for c in 0..4 {
            if c != k || t(i) <= 0.0 {
                assert_eq!(
                    got[c].to_bits(),
                    was[c].to_bits(),
                    "{label} at {i} component {c} is bitwise kept"
                );
                continue;
            }
            let expected = written(mode, k, was[k], v, t(i));
            assert!(
                close(got[k], expected),
                "{label} at {i}: {got:?} vs {expected} over {was:?}"
            );
        }
    }
}

/// One W3C source-over step of straight `cs` at alpha `sa` onto `bg`.
fn composite(mode: u32, bg: [f32; 4], cs: [f64; 3], sa: f64) -> [f64; 4] {
    let ab = f64::from(bg[3]);
    let ao = sa + ab * (1.0 - sa);
    let mut out = [0.0; 4];
    for c in 0..3 {
        let cb = f64::from(bg[c]);
        out[c] =
            (sa * (1.0 - ab) * cs[c] + sa * ab * blend(mode, cb, cs[c]) + (1.0 - sa) * ab * cb)
                / ao;
    }
    out[3] = ao;
    out
}

// --------------------------------------------------------------- planes --

#[test]
fn each_plane_takes_the_luma_under_every_scalar_mode() {
    let samples = extended(W, H);
    let d = float_document(W, H, &samples);
    let n = W * H;
    let v = luma(INK);
    for plane in PLANES {
        let k = component(plane);
        for mode in SCALAR_MODES {
            for selected in [false, true] {
                let mut input = coverage(n, 0);
                let mut request = plane_fill(plane, INK, "document", 0.7, mode);
                if selected {
                    input.extend(coverage(n, 2));
                    request["selection"] = json!(true);
                }
                let out = edit(&d, request, &input).expect("the fill changes the plane");
                assert_eq!(geometry(&out, 0), ((W, H), (0, 0)));
                let weight = |i: usize| {
                    let mut weight = f64::from(input[i]) / 65535.0;
                    if selected {
                        weight *= f64::from(input[n as usize + i]) / 65535.0;
                    }
                    INK[3] * 0.7 * weight
                };
                check(
                    &format!("{plane} mode {mode} selection {selected}"),
                    &samples,
                    &layer32(&out, 0),
                    mode,
                    k,
                    v,
                    weight,
                );
            }
        }
    }
}

#[test]
fn dissolve_writes_the_value_or_keeps_the_backdrop() {
    let (w, h) = (32, 32);
    let samples = extended(w, h);
    let d = float_document(w, h, &samples);
    let whole = vec![65535u16; (w * h) as usize];
    let ink = [INK[0], INK[1], INK[2], 1.0];
    let v = luma(ink);
    let half = edit(
        &d,
        plane_fill("red", ink, "document", 0.5, DISSOLVE),
        &whole,
    )
    .expect("a dither");
    let after = layer32(&half, 0);
    let mut wrote = 0;
    for (was, got) in samples.iter().zip(&after) {
        assert_eq!(
            bits(&[[0.0, got[1], got[2], got[3]]]),
            bits(&[[0.0, was[1], was[2], was[3]]])
        );
        if got[0].to_bits() != was[0].to_bits() {
            assert!(close(got[0], v), "{got:?}");
            wrote += 1;
        }
    }
    let share = f64::from(wrote) / f64::from(w * h);
    assert!(
        (0.40..=0.60).contains(&share),
        "the dither's share is {share}"
    );
    let full = edit(
        &d,
        plane_fill("red", ink, "document", 1.0, DISSOLVE),
        &whole,
    )
    .expect("a full dissolve");
    check(
        "dissolve at 1",
        &samples,
        &layer32(&full, 0),
        0,
        0,
        v,
        |_| 1.0,
    );
}

/// `d`'s canvas grown by `(8, 6)` with the content at `(4, 3)`.
fn trimmed(d: &PrecisionDocument) -> PrecisionDocument {
    let edit = GeometryEdit::parse(
        &json!({"op":"canvas_size","width":W + 8,"height":H + 6,"x":4,"y":3}).to_string(),
    )
    .unwrap();
    let out = d.geometry(&edit, LIMIT, 0, None).unwrap().expect("resized");
    assert_eq!(geometry(&out, 0), ((W, H), (4, 3)));
    out
}

#[test]
fn a_trimmed_layer_never_grows() {
    let samples = extended(W, H);
    let d = trimmed(&float_document(W, H, &samples));
    let whole = vec![65535u16; ((W + 8) * (H + 6)) as usize];
    let ink = [INK[0], INK[1], INK[2], 1.0];
    for plane in PLANES {
        let out = edit(&d, plane_fill(plane, ink, "document", 1.0, 0), &whole)
            .expect("the plane changes");
        assert_eq!(geometry(&out, 0), ((W, H), (4, 3)), "{plane}");
        check(
            plane,
            &samples,
            &layer32(&out, 0),
            0,
            component(plane),
            luma(ink),
            |_| 1.0,
        );
    }
    // Coverage that misses the layer changes nothing.
    let mut outside = vec![0u16; whole.len()];
    outside[..(W + 8) as usize].fill(65535);
    assert!(edit(&d, plane_fill("red", ink, "document", 1.0, 0), &outside).is_none());
}

#[test]
fn stored_alpha_does_not_scale_the_write() {
    // Photoshop's measured row (`plane-fill-431/take1`, SOFT `written`): a
    // pixel takes the write at t whatever its stored alpha.
    let (w, h) = (12, 10);
    let samples = alphas(w, h);
    let d = float_document(w, h, &samples);
    let n = (w * h) as usize;
    let ink = [INK[0], INK[1], INK[2], 1.0];
    let v = luma(ink);
    for plane in PLANES {
        let out = edit(
            &d,
            plane_fill(plane, ink, "document", 0.6, 0),
            &vec![65535; n],
        )
        .expect("every pixel changes");
        check(
            plane,
            &samples,
            &layer32(&out, 0),
            0,
            component(plane),
            v,
            |_| 0.6,
        );
    }
    // A fill over stored-alpha-0 pixels only writes them too.
    let transparent: Vec<u16> = (0..n as u32)
        .map(|i| if (i % w) % 3 == 0 { 65535 } else { 0 })
        .collect();
    let out = edit(
        &d,
        plane_fill("blue", ink, "document", 1.0, 0),
        &transparent,
    )
    .expect("alpha-0 pixels take the write");
    check("alpha 0 only", &samples, &layer32(&out, 0), 0, 2, v, |i| {
        f64::from(transparent[i]) / 65535.0
    });
    // A Stroke band is the same walk over the band's coverage.
    let rect: Vec<u16> = (0..n as u32)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if (2..10).contains(&x) && (1..9).contains(&y) {
                65535
            } else {
                0
            }
        })
        .collect();
    let band = selection(
        w,
        h,
        json!({"kind":"stroke","width":2.0,"location":"inside"}),
        &rect,
    );
    assert!(band.contains(&65535) && band.contains(&0));
    for plane in ["green", "alpha"] {
        let out = edit(&d, plane_fill(plane, ink, "document", 1.0, 0), &band)
            .expect("the band changes the plane");
        check(
            &format!("{plane} band"),
            &samples,
            &layer32(&out, 0),
            0,
            component(plane),
            v,
            |i| f64::from(band[i]) / 65535.0,
        );
    }
}

#[test]
fn a_display_p3_document_writes_the_same_raw_value() {
    let pixels: Vec<[f32; 4]> = extended(W, H)
        .into_iter()
        .map(|p| [p[0], p[1], p[2], 1.0])
        .collect();
    let d = float_doc(
        W,
        H,
        &IccProfile::display_p3(),
        std::slice::from_ref(&pixels),
    );
    assert_eq!(geometry(&d, 0), ((W, H), (0, 0)));
    let ink = [INK[0], INK[1], INK[2], 1.0];
    let whole = vec![65535u16; (W * H) as usize];
    let out = edit(&d, plane_fill("red", ink, "document", 1.0, 0), &whole).expect("a fill");
    let after = layer32(&out, 0);
    check("p3", &pixels, &after, 0, 0, luma(ink), |_| 1.0);
    // The same bits an sRGB document holds: no profile decode.
    let srgb = edit(
        &float_document(W, H, &pixels),
        plane_fill("red", ink, "document", 1.0, 0),
        &whole,
    )
    .expect("a fill");
    assert_eq!(bits(&after), bits(&layer32(&srgb, 0)));
}

#[test]
fn plane_refusals_name_their_rule() {
    let d = float_document(W, H, &extended(W, H));
    let request = |mode| plane_fill("red", INK, "document", 1.0, mode);
    let typed = doc(W, H, vec![[1000, 2000, 3000, 65535]; (W * H) as usize]);
    // Each sentence comes from the edit export asked alone and from the plan
    // (`refusal`).
    assert_eq!(
        refusal(&typed, request(0)),
        "paint_blend's plane is a 32-bit float document's target; a 16-bit colour plane is a scalar_edit target"
    );
    let mut preserving = request(0);
    preserving["preserve_alpha"] = json!(true);
    assert_eq!(
        refusal(&d, preserving),
        "a colour-plane fill takes no preserve_alpha"
    );
    assert_eq!(
        refusal(&d, plane_fill("green", INK, "srgb", 1.0, 0)),
        "a colour-plane fill takes document ink"
    );
    for mode in 23..=26 {
        assert_eq!(
            refusal(&d, request(mode)),
            "plane blend needs one scalar or three RGB planes; HSL requires RGB and Pass Through is unavailable",
            "mode {mode}"
        );
    }
    // Divide is admitted (decision B); Vivid Light stands in.
    for (mode, name) in [(2, "screen"), (3, "overlay"), (18, "vivid_light")] {
        let message = refusal(&d, request(mode));
        assert!(
            message.ends_with(UNAVAILABLE) && message.contains(name),
            "{message}"
        );
        assert!(!message.contains("one colour channel"), "{message}");
    }
    for (mode, name) in [(16, "darker_color"), (17, "lighter_color")] {
        assert_eq!(
            refusal(&d, request(mode)),
            format!("the {name} blend mode on one colour channel {UNAVAILABLE}")
        );
        // The layer fill keeps both modes.
        assert!(edit(
            &d,
            layer_fill(INK, "document", 1.0, mode),
            &vec![65535; (W * H) as usize]
        )
        .is_some());
    }
    assert_eq!(
        refusal(
            &d,
            plane_fill("blue", [1.5, 0.2, 0.1, 1.0], "document", 1.0, 0)
        ),
        "native authored colors must be finite in [0,1]"
    );
    assert!(refusal(&d, plane_fill("gray", INK, "document", 1.0, 0))
        .starts_with("invalid native paint tool"));
    assert_eq!(
        refusal(&d, request(40)),
        "native paint opacity or blend mode is invalid"
    );
}

#[test]
fn locks_follow_the_pixels_kind() {
    let samples = extended(W, H);
    let d = float_document(W, H, &samples);
    let whole = vec![65535u16; (W * H) as usize];
    let ink = [INK[0], INK[1], INK[2], 1.0];
    let locked = d.with_layer_locks(0, LOCK_PIXELS).unwrap().unwrap();
    for plane in PLANES {
        let message = edit_error(&locked, plane_fill(plane, ink, "document", 1.0, 0), &whole);
        assert!(message.contains("locked"), "{plane}: {message}");
    }
    let frozen = d.with_layer_locks(0, LOCK_TRANSPARENCY).unwrap().unwrap();
    // The lock's own restore (`restoring_alpha`) gives a stored-alpha-0 pixel
    // its colour back, as for every Pixels edit under it.
    let kept = |i: usize| if samples[i][3] == 0.0 { 0.0 } else { 1.0 };
    for plane in ["red", "green", "blue"] {
        let out = edit(&frozen, plane_fill(plane, ink, "document", 1.0, 0), &whole)
            .expect("a colour channel is no alpha edit");
        check(
            plane,
            &samples,
            &layer32(&out, 0),
            0,
            component(plane),
            luma(ink),
            kept,
        );
    }
    assert!(edit(
        &frozen,
        plane_fill("alpha", ink, "document", 1.0, 0),
        &whole
    )
    .is_none());
}

fn mixed(doc: &PrecisionDocument, source: &DrDocument, request: Value) -> PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_mixed_source_edit(
            doc,
            source,
            ptr::null(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    assert!(!out.is_null(), "the place changes the document");
    *unsafe { Box::from_raw(out) }
}

#[test]
fn a_retained_source_a_group_and_an_adjustment_refuse() {
    let d = float_document(W, H, &extended(W, H));
    let mut request = plane_fill("red", INK, "document", 1.0, 0);
    request["layer"] = json!(1);
    let master = DrDocument::from_pixels(RgbaImage::from_pixel(4, 4, Rgba([9, 99, 199, 255])));
    let placed = mixed(
        &d,
        &master,
        json!({"op":"place","source_id":"m","name":"m","x":1,"y":1,"depth_conversion":"reject"}),
    );
    assert_eq!(
        plan_error(&placed, request.clone()),
        "Rasterize the retained source before painting its pixels"
    );
    let grouped = edit(
        &d,
        json!({"op":"group_layers","layers":[0],"name":"G","pass_through":false}),
        &[],
    )
    .expect("grouped");
    assert_eq!(
        plan_error(&grouped, request.clone()),
        "native paint requires a raster target"
    );
    let levels = json!({"type":"adjust","evaluation":"precision_v1","op":"levels",
        "params":{"black":0.1,"white":0.9,"gamma":1.2}})
    .to_string();
    let adjusted = d
        .add_adjustment("Adjust", &levels, 1.0, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        plan_error(&adjusted, request),
        "native paint requires a raster target"
    );
}

#[test]
fn a_plane_fill_clears_the_description() {
    let d = float_document(W, H, &extended(W, H));
    let description =
        CString::new(r#"{"type":"shape","version":4,"artwork":{"ink":"linear_srgb"}}"#).unwrap();
    let mut err = ptr::null_mut();
    let described = unsafe {
        dr_precision_doc_artwork_metadata(&d, 0, description.as_ptr(), LIMIT, 0, &mut err)
    };
    assert_eq!(owned(err), None);
    assert!(!described.is_null());
    let described = *unsafe { Box::from_raw(described) };
    assert!(owned(unsafe { dr_precision_doc_artwork_description(&described, 0) }).is_some());
    let out = edit(
        &described,
        plane_fill("green", INK, "document", 1.0, 0),
        &vec![65535; (W * H) as usize],
    )
    .expect("a fill");
    assert_eq!(
        owned(unsafe { dr_precision_doc_artwork_description(&out, 0) }),
        None
    );
}

// ------------------------------------------------------------ linear ink --

const HDR: [f64; 4] = [4.0, 0.5, 0.25, 1.0];

#[test]
fn linear_ink_is_written_as_given_in_srgb_and_p3_documents() {
    let opaque: Vec<[f32; 4]> = extended(W, H)
        .into_iter()
        .map(|p| [p[0], p[1], p[2], 1.0])
        .collect();
    let whole = vec![65535u16; (W * H) as usize];
    for d in [
        float_document(W, H, &extended(W, H)),
        float_doc(
            W,
            H,
            &IccProfile::display_p3(),
            std::slice::from_ref(&opaque),
        ),
    ] {
        let out = edit(&d, layer_fill(HDR, "linear", 1.0, 0), &whole).expect("a fill");
        for p in layer32(&out, 0) {
            assert_eq!(p, [4.0, 0.5, 0.25, 1.0]);
        }
        // On a colour plane the value is the luma of the linear RGB, unclipped
        // (the take's `hdr` verdict `lin`).
        let before = layer32(&d, 0);
        let red = edit(&d, plane_fill("red", HDR, "linear", 1.0, 0), &whole).expect("a fill");
        check(
            "linear plane",
            &before,
            &layer32(&red, 0),
            0,
            0,
            1.5225,
            |_| 1.0,
        );
    }
}

#[test]
fn above_white_ink_blends_by_the_layer_formulas() {
    let samples = extended(W, H);
    let d = float_document(W, H, &samples);
    let input = coverage(W * H, 1);
    let ink = [HDR[0], HDR[1], HDR[2], 0.8];
    for mode in [1, 12] {
        let out = edit(&d, layer_fill(ink, "linear", 0.7, mode), &input).expect("a fill");
        for (i, (bg, got)) in samples.iter().zip(layer32(&out, 0)).enumerate() {
            let sa = ink[3] * 0.7 * f64::from(input[i]) / 65535.0;
            if sa <= 0.0 {
                assert_eq!(got.map(f32::to_bits), bg.map(f32::to_bits));
                continue;
            }
            let expected = composite(mode, *bg, [HDR[0], HDR[1], HDR[2]], sa);
            for c in 0..4 {
                assert!(
                    close(got[c], expected[c]),
                    "mode {mode} at {i} channel {c}: {got:?} vs {expected:?} over {bg:?}"
                );
            }
        }
    }
}

#[test]
fn linear_ink_is_admitted_up_to_the_extended_range() {
    let d = float_document(2, 2, &[[0.25, 0.5, 0.75, 1.0]; 4]);
    let top = edit(
        &d,
        layer_fill([65504.0, 0.0, 1.0, 1.0], "linear", 1.0, 0),
        &[65535; 4],
    )
    .expect("65504 is admitted");
    assert_eq!(layer32(&top, 0)[0], [65504.0, 0.0, 1.0, 1.0]);
    for color in [
        [65505.0, 0.0, 0.0, 1.0],
        [0.5, -0.001, 0.5, 1.0],
        [0.5, 0.5, 0.5, 1.5],
        [0.5, 0.5, 1e39, 1.0],
        [0.5, 0.5, 0.5, -0.1],
    ] {
        // From the edit export asked alone and from the plan (`refusal`).
        assert_eq!(
            refusal(&d, layer_fill(color, "linear", 1.0, 0)),
            LINEAR_RANGE,
            "{color:?}"
        );
    }
    // JSON has no NaN: the request does not parse.
    let nan = r#"{"op":"paint_blend","layer":0,"color":[NaN,0.5,0.5,1.0],"color_space":"linear","opacity":1.0,"blend_mode":0}"#;
    assert!(!refusal_text(&d, nan).is_empty());
    // Document and sRGB ink keep their [0, 1] range.
    for space in ["document", "srgb"] {
        assert_eq!(
            refusal(&d, layer_fill(HDR, space, 1.0, 0)),
            "native authored colors must be finite in [0,1]"
        );
    }
}

#[test]
fn a_layer_edge_stroke_takes_linear_ink() {
    let samples: Vec<[f32; 4]> = (0..24 * 20)
        .map(|i| {
            let (x, y) = (i % 24, i / 24);
            if (6..16).contains(&x) && (5..13).contains(&y) {
                [x as f32 / 2.0 - 4.0, -0.5, 7.0, 1.0]
            } else {
                [0.0; 4]
            }
        })
        .collect();
    let d = float_document(24, 20, &samples);
    let request = json!({"op":"stroke_layer_edge","layer":0,"width":2.0,"location":"outside",
        "color":HDR,"color_space":"linear","opacity":1.0,"blend_mode":0});
    let out = edit(&d, request.clone(), &[]).expect("a stroke");
    let after = layer32(&out, 0);
    let mut inked = 0;
    for (i, (was, got)) in samples.iter().zip(&after).enumerate() {
        let (x, y) = (i as u32 % 24, i as u32 / 24);
        if was[3] == 1.0 {
            assert_eq!(
                got.map(f32::to_bits),
                was.map(f32::to_bits),
                "inside ({x}, {y})"
            );
        } else if (5..17).contains(&x) && (4..14).contains(&y) {
            // The ring one pixel out is fully inside the 2 px band.
            assert_eq!(*got, [4.0, 0.5, 0.25, 1.0], "band ({x}, {y})");
            inked += 1;
        }
    }
    assert_eq!(inked, 12 * 10 - 10 * 8);
    let typed = doc(24, 20, vec![[1000, 2000, 3000, 65535]; 24 * 20]);
    assert_eq!(plan_error(&typed, request), LINEAR_ONLY);
}

#[test]
fn linear_ink_is_a_float_fill_and_stroke_colour_only() {
    let float = float_document(W, H, &extended(W, H));
    let typed = doc(W, H, vec![[1000, 2000, 3000, 65535]; (W * H) as usize]);
    let ink = [0.5, 0.5, 0.5, 1.0];
    assert_eq!(
        plan_error(&typed, layer_fill(ink, "linear", 1.0, 0)),
        LINEAR_ONLY
    );
    let bucket = json!({"op":"bucket_fill","layer":0,"color":ink,"color_space":"linear",
        "opacity":1.0,"x":1,"y":1,"tolerance":0,"contiguous":true});
    let gradient = json!({"op":"gradient","layer":0,"start":ink,"end":[1.0,1.0,1.0,1.0],
        "color_space":"linear","opacity":1.0,"from":[0.0,0.0],"to":[5.0,5.0],"radial":false});
    let paint = json!({"op":"paint","layer":0,"color":ink,"color_space":"linear","opacity":1.0});
    let rasterize = json!({"op":"rasterize","name":"N","width":2,"height":2,"x":0,"y":0,
        "colors":[ink],"color_space":"linear"});
    // The bucket (float tools) and the gradient take linear ink
    // in a float document.
    assert_eq!(plan_error(&typed, bucket.clone()), LINEAR_ONLY, "{bucket}");
    plan(&float, bucket);
    assert_eq!(plan_error(&typed, gradient.clone()), LINEAR_ONLY);
    plan(&float, gradient);
    for request in [&paint, &rasterize] {
        assert_eq!(
            plan_error(&typed, request.clone()),
            LINEAR_ONLY,
            "{request}"
        );
    }
}
