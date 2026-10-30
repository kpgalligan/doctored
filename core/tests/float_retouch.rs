//! The typed retouch in a 32-bit float document, black-box through
//! the request API (`dr_precision_doc_edit`, its plan and
//! `dr_precision_doc_retouch_from`).
//!
//! - Clone: `retouch_clone` copies in linear light. The oracle maps each
//!   destination centre through the matrix, takes the premultiplied bilinear
//!   sample there and one W3C step in binary64, over above-white, negative
//!   and partly transparent pixels.
//! - Heal: `retouch_heal` solves its membrane in the working encoding W. The
//!   oracle writes W out again in binary64, classifies the box by
//!   `poisson::Region`'s rule and relaxes the membrane by Gauss-Seidel.
//! - Spot Heal and Content-Aware Fill: a hole in a known surround has a known
//!   continuation (a constant, a periodic stripe), and a soft-covered fill
//!   mixes in W.
//! - The refusals, cancellation, the capability rows and the operator table.
//!
//! Float documents are authored as float TIFFs and opened with
//! `open_preserving` (`tests/float_brush_tools.rs`), so a raster holds
//! exactly the samples the oracle reads. No expected value comes from the
//! code under test.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc_lock::LOCK_TRANSPARENCY;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_float::dr_float_operators;
use doctored_core::ffi_precision_conversion::dr_precision_capability;
use doctored_core::ffi_precision_edit::*;
use doctored_core::ffi_retouch::{
    dr_precision_doc_retouch_from, dr_precision_image_transform_retouch_source,
};
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::Metadata;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::tiff_float::FloatTiff;
use serde_json::{json, Value};

use crate::native_scalar as native;
use native::*;

const UNAVAILABLE: &str = "is unavailable in a 32-bit float document";
/// The largest stored magnitude (`float_document::FLOAT_RANGE_MAX`).
const RANGE: f64 = 65504.0;

// ------------------------------------------------------------- fixtures --

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
    let path = directory.path().join("retouch.tiff");
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
fn fractional(n: u32, salt: u32) -> Vec<u16> {
    (0..n)
        .map(|i| match (i + salt) % 5 {
            0 => 0,
            1 => 65535,
            k => (k * 9001 + i * 7) as u16,
        })
        .collect()
}

/// The plan's refusal for `v`, which must refuse.
fn plan_error(d: &PrecisionDocument, v: Value) -> String {
    let t = text(v);
    let mut e = ptr::null_mut();
    let p = unsafe { dr_precision_doc_edit_plan(d, t.as_ptr(), LIMIT, 0, &mut e) };
    assert!(p.is_null(), "the plan was expected to refuse");
    unsafe { error(e) }
}

/// The edit's refusal for `v` under `control`, after a successful plan.
fn edit_error(
    d: &PrecisionDocument,
    v: Value,
    input: &[u16],
    control: Option<&DrJobControl>,
) -> String {
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
            control.map_or(ptr::null(), |c| c as *const DrJobControl),
            &mut e,
        )
    };
    assert!(out.is_null(), "the edit was expected to refuse");
    unsafe { error(e) }
}

/// `dr_precision_doc_retouch_from`: the document, or the refusal.
fn from_plates(
    d: &PrecisionDocument,
    plates: &PrecisionDocument,
    v: Value,
    input: &[u16],
) -> Result<Option<PrecisionDocument>, String> {
    let t = text(v);
    let mut e = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_retouch_from(
            d,
            plates,
            t.as_ptr(),
            input.as_ptr(),
            input.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    if !e.is_null() {
        assert!(out.is_null());
        return Err(unsafe { error(e) });
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

/// `d`'s canvas grown by `(8, 6)` with the content at `(4, 3)`: the raster
/// keeps its size, so it is trimmed against the canvas.
fn trimmed(d: &PrecisionDocument, (w, h): (u32, u32)) -> PrecisionDocument {
    let edit = GeometryEdit::parse(
        &json!({"op":"canvas_size","width":w + 8,"height":h + 6,"x":4,"y":3}).to_string(),
    )
    .unwrap();
    let out = d.geometry(&edit, LIMIT, 0, None).unwrap().expect("resized");
    assert_eq!(geometry(&out, 0), ((w, h), (4, 3)));
    out
}

fn close(actual: f32, expected: f64, tolerance: f64) -> bool {
    (f64::from(actual) - expected).abs() <= tolerance * expected.abs().max(1.0)
}

/// Every sample is finite and inside the stored range.
fn assert_stored(label: &str, pixels: &[[f32; 4]]) {
    for (i, p) in pixels.iter().enumerate() {
        for v in &p[..3] {
            assert!(
                v.is_finite() && f64::from(v.abs()) <= RANGE,
                "{label} at {i}: {p:?}"
            );
        }
        assert!(
            p[3].is_finite() && (0.0..=1.0).contains(&p[3]),
            "{label} at {i}"
        );
    }
}

// ---------------------------------------------------------- clone oracle --

const CW: u32 = 20;
const CH: u32 = 14;
/// Normal, Multiply, Lighten.
const CLONE_MODES: [u32; 3] = [0, 1, 7];
/// A whole-pixel translation and a 2× source matrix whose centres fall
/// between four source pixels.
const MATRICES: [[f64; 6]; 2] = [
    [1.0, 0.0, 0.0, 1.0, 3.0, -2.0],
    [2.0, 0.0, 0.0, 2.0, -10.0, -7.0],
];

/// Extended-range straight samples: (8, 4, 2) above white, negatives, zeros,
/// alpha 0 with hidden RGB, half alpha and alpha 1.
fn extended(width: u32, height: u32) -> Vec<[f32; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let v = ((x * 37 + y * 101) % 1000) as f32 / 1000.0;
            let a = match (x + 2 * y) % 5 {
                0 => 0.0,
                1 | 2 => 1.0,
                3 => 0.5,
                _ => 0.75,
            };
            if (x + y) % 6 == 0 {
                return [8.0, 4.0, 2.0, a];
            }
            let r = v * 6.0 - 1.0;
            let g = if (x * y) % 7 == 3 { -0.25 } else { 1.0 - v };
            let b = ((x ^ y) % 97) as f32 / 4.0 - 2.0;
            [r, g, b, a]
        })
        .collect()
}

/// The W3C separable blend `B(cb, cs)` for the tested modes.
fn blend(mode: u32, cb: f64, cs: f64) -> f64 {
    match mode {
        0 => cs,
        1 => cb * cs,
        7 => cb.max(cs),
        _ => unreachable!("not a tested mode"),
    }
}

/// One W3C source-over step of straight `cs` at effective alpha `sa` onto
/// the straight backdrop `bg`, in binary64. `frozen` is the transparency
/// lock's rule: an alpha-0 pixel is kept, a partly transparent one takes
/// `lerp(Cb, B(Cb, Cs), sa)` at its own alpha.
fn composite(mode: u32, bg: [f32; 4], cs: [f64; 4], sa: f64, frozen: bool) -> [f64; 4] {
    let ab = f64::from(bg[3]);
    if frozen && ab <= 0.0 {
        return bg.map(f64::from);
    }
    if frozen && ab < 1.0 {
        let mut out = [0.0, 0.0, 0.0, ab];
        for c in 0..3 {
            let cb = f64::from(bg[c]);
            out[c] = sa * blend(mode, cb, cs[c]) + (1.0 - sa) * cb;
        }
        return out;
    }
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

/// The plate under destination pixel `(x, y)`: the matrix maps the pixel's
/// centre to a source centre, and the sample there is the premultiplied
/// bilinear mix of the four pixels around it, straightened. `None` when a
/// weighted tap lies off the canvas (an edge pixel, which the caller does not
/// judge).
fn mapped(
    plate: &[[f32; 4]],
    (w, h): (u32, u32),
    matrix: [f64; 6],
    x: u32,
    y: u32,
) -> Option<[f64; 4]> {
    let (cx, cy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
    let u = matrix[0] * cx + matrix[2] * cy + matrix[4];
    let v = matrix[1] * cx + matrix[3] * cy + matrix[5];
    let (bx, by) = ((u - 0.5).floor(), (v - 0.5).floor());
    let (fx, fy) = (u - 0.5 - bx, v - 0.5 - by);
    let mut acc = [0.0f64; 4];
    for (dy, wy) in [(0.0, 1.0 - fy), (1.0, fy)] {
        for (dx, wx) in [(0.0, 1.0 - fx), (1.0, fx)] {
            let weight = wx * wy;
            if weight == 0.0 {
                continue;
            }
            let (sx, sy) = (bx + dx, by + dy);
            if sx < 0.0 || sy < 0.0 || sx >= f64::from(w) || sy >= f64::from(h) {
                return None;
            }
            let p = plate[sy as usize * w as usize + sx as usize];
            let a = f64::from(p[3]);
            for c in 0..3 {
                acc[c] += weight * a * f64::from(p[c]);
            }
            acc[3] += weight * a;
        }
    }
    if acc[3] <= 0.0 {
        return Some([0.0; 4]);
    }
    Some([
        acc[0] / acc[3],
        acc[1] / acc[3],
        acc[2] / acc[3],
        acc[3].min(1.0),
    ])
}

/// What one clone is checked against.
struct CloneCase<'a> {
    label: String,
    canvas: (u32, u32),
    /// The destination before the clone and the plate it samples, both in
    /// canvas space.
    backdrop: &'a [[f32; 4]],
    plate: &'a [[f32; 4]],
    matrix: [f64; 6],
    mode: u32,
    opacity: f64,
    /// Coverage, then the optional selection plane.
    input: &'a [u16],
    frozen: bool,
}

/// Every pixel of `after` against the oracle: a pixel with no coverage, no
/// selection or no source is bitwise kept, and every other interior pixel is
/// the composite of its mapped source at `alpha × opacity × weights`.
/// Returns how many pixels were judged against a composite.
fn check_clone(case: &CloneCase<'_>, after: &[[f32; 4]]) -> usize {
    let (w, h) = case.canvas;
    let n = (w * h) as usize;
    assert_eq!(after.len(), n, "{}", case.label);
    let mut judged = 0;
    for (i, &got) in after.iter().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        let bg = case.backdrop[i];
        let mut weight = f64::from(case.input[i]) / 65535.0;
        if case.input.len() > n {
            weight *= f64::from(case.input[n + i]) / 65535.0;
        }
        let kept = |why: &str| {
            assert_eq!(
                got.map(f32::to_bits),
                bg.map(f32::to_bits),
                "{} at ({x}, {y}): {why}",
                case.label
            );
        };
        if weight <= 0.0 {
            kept("an uncovered pixel is bitwise kept");
            continue;
        }
        let Some(source) = mapped(case.plate, case.canvas, case.matrix, x, y) else {
            continue;
        };
        if source[3] <= 0.0 {
            kept("a pixel with no source is bitwise kept");
            continue;
        }
        let sa = source[3] * case.opacity * weight;
        let expected = composite(case.mode, bg, source, sa, case.frozen);
        for c in 0..4 {
            assert!(
                close(got[c], expected[c], 1e-5),
                "{} at ({x}, {y}) channel {c}: {got:?} vs {expected:?} over {bg:?}",
                case.label
            );
        }
        judged += 1;
    }
    judged
}

fn clone_request(matrix: [f64; 6], opacity: f64, mode: u32, selection: bool) -> Value {
    json!({"op":"retouch_clone","layer":0,"matrix":matrix,"opacity":opacity,
        "blend_mode":mode,"selection":selection})
}

// ---------------------------------------------------------------- clone --

#[test]
fn a_clone_copies_linear_light_through_the_w3c_oracle() {
    let samples = extended(CW, CH);
    let d = float_document(CW, CH, &samples);
    let n = CW * CH;
    for (m, matrix) in MATRICES.into_iter().enumerate() {
        for opacity in [1.0, 0.6] {
            for mode in CLONE_MODES {
                for selected in [false, true] {
                    let mut input = fractional(n, 1);
                    if selected {
                        input.extend(fractional(n, 3));
                    }
                    let out = edit(&d, clone_request(matrix, opacity, mode, selected), &input)
                        .expect("the clone changes pixels");
                    assert_eq!(geometry(&out, 0), ((CW, CH), (0, 0)));
                    let after = layer32(&out, 0);
                    assert_stored("clone", &after);
                    let judged = check_clone(
                        &CloneCase {
                            label: format!(
                                "matrix {m} opacity {opacity} mode {mode} selection {selected}"
                            ),
                            canvas: (CW, CH),
                            backdrop: &samples,
                            plate: &samples,
                            matrix,
                            mode,
                            opacity,
                            input: &input,
                            frozen: false,
                        },
                        &after,
                    );
                    assert!(judged > 12, "matrix {m}: {judged} pixels judged");
                }
            }
        }
    }
}

#[test]
fn a_cloned_value_above_white_reads_itself_and_nothing_clamps() {
    let d = float_document(2, 1, &[[8.0, 4.0, 2.0, 1.0], [0.25, 0.25, 0.25, 1.0]]);
    // Pixel 1 reads pixel 0: its centre 1.5 maps to 0.5.
    let left = [1.0, 0.0, 0.0, 1.0, -1.0, 0.0];
    let out = edit(&d, clone_request(left, 1.0, 0, false), &[0, 65535])
        .expect("the clone changes the pixel");
    let pixels = layer32(&out, 0);
    assert_eq!(pixels[1], [8.0, 4.0, 2.0, 1.0]);
    assert_eq!(
        pixels[0].map(f32::to_bits),
        [8.0f32, 4.0, 2.0, 1.0].map(f32::to_bits)
    );
    // Multiply keeps the unclamped product: 0.25 × (8, 4, 2).
    let out = edit(&d, clone_request(left, 1.0, 1, false), &[0, 65535])
        .expect("the clone changes the pixel");
    assert_eq!(layer32(&out, 0)[1], [2.0, 1.0, 0.5, 1.0]);
}

#[test]
fn a_clone_mode_outside_the_float_set_and_a_result_past_the_range_refuse() {
    let d = float_document(CW, CH, &extended(CW, CH));
    // Screen inverts about white, so a float document refuses it.
    let message = plan_error(&d, clone_request(MATRICES[0], 1.0, 2, false));
    assert!(message.contains(UNAVAILABLE), "{message}");
    assert!(message.to_lowercase().contains("screen"), "{message}");
    // 300 × 300 is past the stored range under Multiply; the same request
    // over tame values is admitted.
    let left = [1.0, 0.0, 0.0, 1.0, -1.0, 0.0];
    let hot = float_document(2, 1, &[[300.0, 1.0, 1.0, 1.0], [300.0, 1.0, 1.0, 1.0]]);
    let message = edit_error(&hot, clone_request(left, 1.0, 1, false), &[0, 65535], None);
    assert!(message.contains("65504"), "{message}");
    let tame = float_document(2, 1, &[[3.0, 1.0, 1.0, 1.0], [3.0, 1.0, 1.0, 1.0]]);
    assert!(edit(&tame, clone_request(left, 1.0, 1, false), &[0, 65535]).is_some());
}

#[test]
fn a_clone_lands_on_a_new_layer_a_grown_extent_and_under_a_transparency_lock() {
    let samples = extended(CW, CH);
    let d = float_document(CW, CH, &samples);
    let n = CW * CH;
    let matrix = MATRICES[0];
    let input = fractional(n, 1);

    // A new layer: the clone over transparency, the target untouched.
    let mut request = clone_request(matrix, 0.6, 0, false);
    request["output"] = json!("new_layer");
    let out = edit(&d, request, &input).expect("the clone adds a layer");
    assert_eq!(out.layer_count(), 2);
    assert_eq!(bits(&layer32(&out, 0)), bits(&samples));
    assert_eq!(geometry(&out, 1), ((CW, CH), (0, 0)));
    let blank = vec![[0.0f32; 4]; n as usize];
    let judged = check_clone(
        &CloneCase {
            label: "new layer".into(),
            canvas: (CW, CH),
            backdrop: &blank,
            plate: &samples,
            matrix,
            mode: 0,
            opacity: 0.6,
            input: &input,
            frozen: false,
        },
        &layer32(&out, 1),
    );
    assert!(judged > 12);

    // A trimmed layer grows to the canvas, its old samples in place.
    let small = trimmed(&d, (CW, CH));
    let (gw, gh) = (CW + 8, CH + 6);
    let placed: Vec<[f32; 4]> = (0..gw * gh)
        .map(|i| {
            let (x, y) = (i % gw, i / gw);
            if (4..4 + CW).contains(&x) && (3..3 + CH).contains(&y) {
                samples[((y - 3) * CW + x - 4) as usize]
            } else {
                [0.0; 4]
            }
        })
        .collect();
    let whole = vec![65535u16; (gw * gh) as usize];
    let mut request = clone_request(matrix, 1.0, 0, false);
    request["expand_destination"] = json!(true);
    let grown = edit(&small, request, &whole).expect("the clone grows the layer");
    assert_eq!(geometry(&grown, 0), ((gw, gh), (0, 0)));
    let judged = check_clone(
        &CloneCase {
            label: "grown".into(),
            canvas: (gw, gh),
            backdrop: &placed,
            plate: &placed,
            matrix,
            mode: 0,
            opacity: 1.0,
            input: &whole,
            frozen: false,
        },
        &layer32(&grown, 0),
    );
    assert!(judged > 12);

    // A transparency lock keeps every alpha, and an alpha-0 pixel whole.
    let locked = d.with_layer_locks(0, LOCK_TRANSPARENCY).unwrap().unwrap();
    let out = edit(&locked, clone_request(matrix, 0.6, 0, false), &input)
        .expect("the clone recolours the layer");
    let after = layer32(&out, 0);
    for (was, got) in samples.iter().zip(&after) {
        assert_eq!(got[3].to_bits(), was[3].to_bits(), "alpha is bitwise kept");
        if was[3] == 0.0 {
            assert_eq!(got.map(f32::to_bits), was.map(f32::to_bits));
        }
    }
    let judged = check_clone(
        &CloneCase {
            label: "locked".into(),
            canvas: (CW, CH),
            backdrop: &samples,
            plate: &samples,
            matrix,
            mode: 0,
            opacity: 0.6,
            input: &input,
            frozen: true,
        },
        &after,
    );
    assert!(judged > 12);
}

#[test]
fn a_clone_reads_the_named_source_layer_of_a_float_document() {
    let samples = extended(CW, CH);
    let d = float_document(CW, CH, &samples);
    let n = CW * CH;
    // A second raster holding another picture: the 2× clone on a new layer.
    let mut request = clone_request(MATRICES[1], 1.0, 0, false);
    request["output"] = json!("new_layer");
    let two = edit(&d, request, &vec![65535u16; n as usize]).expect("the clone adds a layer");
    assert_eq!(two.layer_count(), 2);
    assert_eq!(geometry(&two, 1), ((CW, CH), (0, 0)));
    let layers = [samples.clone(), layer32(&two, 1)];
    assert_eq!(bits(&layer32(&two, 0)), bits(&layers[0]));
    assert_ne!(bits(&layers[1]), bits(&layers[0]));

    let matrix = MATRICES[0];
    let input = fractional(n, 1);
    // Each layer in turn takes the other as its plate.
    for (target, source) in [(0usize, 1usize), (1, 0)] {
        let named = json!({"op":"retouch_clone","layer":target,"source_layer":source,
            "sample":"current","matrix":matrix,"opacity":0.6});
        let out = edit(&two, named, &input).expect("the clone changes pixels");
        assert_eq!(out.layer_count(), 2);
        assert_eq!(geometry(&out, target), ((CW, CH), (0, 0)));
        // The source layer is read, never written.
        assert_eq!(bits(&layer32(&out, source)), bits(&layers[source]));
        let after = layer32(&out, target);
        assert_stored("source layer", &after);
        let judged = check_clone(
            &CloneCase {
                label: format!("layer {target} from source layer {source}"),
                canvas: (CW, CH),
                backdrop: &layers[target],
                plate: &layers[source],
                matrix,
                mode: 0,
                opacity: 0.6,
                input: &input,
                frozen: false,
            },
            &after,
        );
        assert!(judged > 12, "source layer {source}: {judged} pixels judged");
        // Without the name the plate is the target itself: another result.
        let own = json!({"op":"retouch_clone","layer":target,"sample":"current",
            "matrix":matrix,"opacity":0.6});
        let own = edit(&two, own, &input).expect("the clone changes pixels");
        assert_ne!(bits(&layer32(&own, target)), bits(&after));
    }
    // A composite sampling mode refuses the name, as at 16 bits.
    let message = plan_error(
        &two,
        json!({"op":"retouch_clone","layer":0,"source_layer":1,"sample":"all","matrix":matrix}),
    );
    assert_eq!(message, "source_layer requires sample=current");
}

#[test]
fn the_source_transform_export_maps_a_float_plate_through_the_bilinear_oracle() {
    let samples = extended(CW, CH);
    let d = float_document(CW, CH, &samples);
    let plate = d.layer_source(0).unwrap();
    for (m, matrix) in MATRICES.into_iter().enumerate() {
        let mut e = ptr::null_mut();
        let out = unsafe {
            dr_precision_image_transform_retouch_source(
                plate,
                matrix.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut e,
            )
        };
        assert!(!out.is_null(), "{}", unsafe { error(e) });
        assert!(e.is_null());
        let out = unsafe { Box::from_raw(out) };
        assert_eq!(out.dimensions(), (CW, CH));
        let mapped_plate = out.to_rgba32().expect("the mapped plate is 32-bit float");
        assert_stored("mapped plate", &mapped_plate);
        let mut judged = 0;
        for (i, got) in mapped_plate.iter().enumerate() {
            let (x, y) = (i as u32 % CW, i as u32 / CW);
            let Some(expected) = mapped(&samples, (CW, CH), matrix, x, y) else {
                continue;
            };
            for c in 0..4 {
                assert!(
                    close(got[c], expected[c], 1e-5),
                    "matrix {m} at ({x}, {y}) channel {c}: {got:?} vs {expected:?}"
                );
            }
            judged += 1;
        }
        assert!(judged > 12, "matrix {m}: {judged} pixels judged");
    }
    // The plate itself is not written.
    assert_eq!(bits(&layer32(&d, 0)), bits(&samples));
}

// ------------------------------------------------- the working encoding --

/// The sRGB OETF, written out.
fn srgb_oetf(u: f64) -> f64 {
    if u <= 0.0031308 {
        12.92 * u
    } else {
        1.055 * u.powf(1.0 / 2.4) - 0.055
    }
}

/// The sRGB EOTF, written out.
fn srgb_eotf(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

const SHOULDER: f64 = 1.055 / 2.4;

/// W's scale: the largest stored magnitude lands on 65535.
fn w_scale() -> f64 {
    65535.0 / (1.0 + SHOULDER * RANGE.ln())
}

/// The working encoding, written out again in binary64: the sRGB curve up to
/// white, a logarithm with the curve's own slope above it, odd in `v`.
fn w(v: f64) -> f64 {
    let u = v.abs();
    let f = if u <= 1.0 {
        srgb_oetf(u)
    } else {
        1.0 + SHOULDER * u.ln()
    };
    (w_scale() * f).copysign(v)
}

/// W's inverse.
fn w_inverse(code: f64) -> f64 {
    let f = code.abs() / w_scale();
    let u = if f <= 1.0 {
        srgb_eotf(f)
    } else {
        ((f - 1.0) / SHOULDER).exp()
    };
    u.copysign(code)
}

#[test]
fn the_written_out_encoding_has_the_stated_shape() {
    assert!((w(1.0) - 11_155.0).abs() < 1.0, "{}", w(1.0));
    assert!((w(RANGE) - 65535.0).abs() < 1e-6);
    assert_eq!(w(-3.5), -w(3.5));
    // C1 at white: the slopes either side agree.
    let (below, above) = (
        (w(1.0) - w(1.0 - 1e-6)) / 1e-6,
        (w(1.0 + 1e-6) - w(1.0)) / 1e-6,
    );
    assert!((below - above).abs() < 1e-3 * above, "{below} vs {above}");
    for v in [0.001, 0.2, 1.0, 5.0, 10_000.0, -0.25] {
        assert!((w_inverse(w(v)) - v).abs() <= 1e-9 * v.abs().max(1.0));
    }
}

// ----------------------------------------------------------------- heal --

const HW: u32 = 32;
const HH: u32 = 24;
/// The healed box: x, y, width, height.
const BOX: (u32, u32, u32, u32) = (4, 3, 12, 10);
/// The source lies this far right of and below its destination.
const SHIFT: (u32, u32) = (14, 9);

fn in_box(x: u32, y: u32) -> bool {
    (BOX.0..BOX.0 + BOX.2).contains(&x) && (BOX.1..BOX.1 + BOX.3).contains(&y)
}

/// A box pixel with a neighbour outside the box: `poisson::Region`'s
/// Dirichlet boundary (every pixel here is usable and the box is clear of
/// the canvas edge).
fn on_ring(x: u32, y: u32) -> bool {
    in_box(x, y)
        && (!in_box(x - 1, y) || !in_box(x + 1, y) || !in_box(x, y - 1) || !in_box(x, y + 1))
}

fn box_coverage() -> Vec<u16> {
    (0..HW * HH)
        .map(|i| if in_box(i % HW, i / HW) { 65535 } else { 0 })
        .collect()
}

fn heal_request() -> Value {
    json!({"op":"retouch_heal","layer":0,
        "matrix":[1.0, 0.0, 0.0, 1.0, f64::from(SHIFT.0), f64::from(SHIFT.1)],
        "x":BOX.0,"y":BOX.1,"width":BOX.2,"height":BOX.3})
}

/// A picture with values above white, a channel that crosses zero, grain and
/// some partly transparent pixels; every pixel has alpha above 0.
fn heal_scene() -> Vec<[f32; 4]> {
    (0..HW * HH)
        .map(|i| {
            let (x, y) = (i % HW, i / HW);
            let (fx, fy) = (x as f32 / (HW - 1) as f32, y as f32 / (HH - 1) as f32);
            let grain = ((x * 37 + y * 101) % 17) as f32 / 17.0;
            [
                0.2 + 3.0 * fx + 2.0 * fy + 0.3 * grain,
                -0.3 + 0.9 * fx + 0.1 * grain,
                6.0 - 4.0 * fy + 0.5 * grain * fx,
                if (x + y) % 7 == 0 { 0.75 } else { 1.0 },
            ]
        })
        .collect()
}

/// The membrane heal of `BOX` in W, in binary64: the expected W code of each
/// channel of each box pixel (`None` outside the box).
///
/// `poisson::Region`'s rule on this geometry: a box pixel next to a pixel
/// outside the box is a Dirichlet boundary and holds
/// `u = W(destination) − W(source)`; every other box pixel is an unknown
/// whose `u` is the mean of its four neighbours (all inside the box), relaxed
/// here by Gauss-Seidel to convergence. The healed code is
/// `clamp(W(source) + u, ±65535)`.
fn heal_oracle(destination: &[[f32; 4]], source: &[[f32; 4]]) -> Vec<Option<[f64; 3]>> {
    let at = |x: u32, y: u32| (y * HW + x) as usize;
    let mut expected: Vec<Option<[f64; 3]>> = vec![None; (HW * HH) as usize];
    for c in 0..3 {
        let code = |plane: &[[f32; 4]], x: u32, y: u32| w(f64::from(plane[at(x, y)][c]));
        let from = |x: u32, y: u32| code(source, x + SHIFT.0, y + SHIFT.1);
        let mut u = vec![0.0f64; (HW * HH) as usize];
        for y in BOX.1..BOX.1 + BOX.3 {
            for x in BOX.0..BOX.0 + BOX.2 {
                if on_ring(x, y) {
                    u[at(x, y)] = code(destination, x, y) - from(x, y);
                }
            }
        }
        for _ in 0..20_000 {
            let mut moved = 0.0f64;
            for y in BOX.1..BOX.1 + BOX.3 {
                for x in BOX.0..BOX.0 + BOX.2 {
                    if on_ring(x, y) {
                        continue;
                    }
                    let mean =
                        (u[at(x - 1, y)] + u[at(x + 1, y)] + u[at(x, y - 1)] + u[at(x, y + 1)])
                            / 4.0;
                    moved = moved.max((mean - u[at(x, y)]).abs());
                    u[at(x, y)] = mean;
                }
            }
            if moved < 1e-10 {
                break;
            }
        }
        for y in BOX.1..BOX.1 + BOX.3 {
            for x in BOX.0..BOX.0 + BOX.2 {
                let healed = (from(x, y) + u[at(x, y)]).clamp(-65535.0, 65535.0);
                expected[at(x, y)].get_or_insert([0.0; 3])[c] = healed;
            }
        }
    }
    expected
}

/// `after` against the heal oracle: every box sample within 2 W codes of its
/// expected code, every alpha and every pixel outside the box bitwise kept.
fn check_heal(label: &str, before: &[[f32; 4]], source: &[[f32; 4]], after: &[[f32; 4]]) {
    let expected = heal_oracle(before, source);
    let mut worst = 0.0f64;
    for (i, (was, got)) in before.iter().zip(after).enumerate() {
        assert_eq!(got[3].to_bits(), was[3].to_bits(), "{label}: alpha at {i}");
        let Some(codes) = expected[i] else {
            assert_eq!(
                got.map(f32::to_bits),
                was.map(f32::to_bits),
                "{label} at {i}: outside the box is bitwise kept"
            );
            continue;
        };
        for c in 0..3 {
            let off = (w(f64::from(got[c])) - codes[c]).abs();
            assert!(
                off <= 2.0,
                "{label} at {i} channel {c}: {} is {off} W codes from {}",
                got[c],
                w_inverse(codes[c])
            );
            worst = worst.max(off);
        }
    }
    eprintln!("{label}: worst membrane error {worst:.4} W codes");
}

#[test]
fn a_heal_solves_its_membrane_in_the_working_encoding() {
    let scene = heal_scene();
    let d = float_document(HW, HH, &scene);
    let coverage = box_coverage();
    let out = edit(&d, heal_request(), &coverage).expect("the heal changes pixels");
    assert_eq!(geometry(&out, 0), ((HW, HH), (0, 0)));
    let after = layer32(&out, 0);
    assert_stored("heal", &after);
    assert_ne!(bits(&after), bits(&scene));
    check_heal("heal", &scene, &scene, &after);
}

#[test]
fn a_heal_from_another_documents_plates_reads_them_and_a_16_bit_plate_refuses() {
    let scene = heal_scene();
    let plates = float_document(HW, HH, &scene);
    // The document's own source area holds something else entirely: only
    // the plates' picture may reach the box.
    let mut filled = scene.clone();
    for (i, p) in filled.iter_mut().enumerate() {
        let (x, y) = (i as u32 % HW, i as u32 / HW);
        if in_box(x.wrapping_sub(SHIFT.0), y.wrapping_sub(SHIFT.1)) {
            *p = [0.5, 0.5, 0.5, 1.0];
        }
    }
    let d = float_document(HW, HH, &filled);
    let coverage = box_coverage();
    let out = from_plates(&d, &plates, heal_request(), &coverage)
        .expect("the heal is admitted")
        .expect("the heal changes pixels");
    let after = layer32(&out, 0);
    assert_stored("heal from plates", &after);
    check_heal("heal from plates", &filled, &scene, &after);
    // The same request against itself is the plain heal, to the bit.
    let plain = edit(&plates, heal_request(), &coverage).unwrap();
    let same = from_plates(&plates, &plates, heal_request(), &coverage)
        .unwrap()
        .unwrap();
    assert_eq!(bits(&layer32(&same, 0)), bits(&layer32(&plain, 0)));
    // Plates of another depth are refused by name.
    let typed = doc(HW, HH, vec![[1000, 2000, 3000, 65535]; (HW * HH) as usize]);
    let message = from_plates(&d, &typed, heal_request(), &coverage).err();
    assert_eq!(
        message.as_deref(),
        Some("retouch plates must have the document's bit depth")
    );
    let message = from_plates(&typed, &d, heal_request(), &coverage).err();
    assert_eq!(
        message.as_deref(),
        Some("retouch plates must have the document's bit depth")
    );
}

/// The bound case: the destination is `level` everywhere, and the source
/// under the box reads `rim` on the box's Dirichlet ring and `core` inside
/// it, so `W(core) + (W(level) − W(rim))` is past ±65535.
fn bound_scene(level: f32, rim: f32, core: f32) -> Vec<[f32; 4]> {
    (0..HW * HH)
        .map(|i| {
            let (x, y) = (i % HW, i / HW);
            let (sx, sy) = (x.wrapping_sub(SHIFT.0), y.wrapping_sub(SHIFT.1));
            let v = if !in_box(sx, sy) {
                level
            } else if on_ring(sx, sy) {
                rim
            } else {
                core
            };
            [v, v, v, 1.0]
        })
        .collect()
}

#[test]
fn a_heal_whose_membrane_overshoots_lands_at_the_stored_bound() {
    for sign in [1.0f32, -1.0] {
        let scene = bound_scene(sign * 10_000.0, sign * 0.01, sign * 5.0);
        // The overshoot the case is built for.
        let raw = w(5.0) + (w(10_000.0) - w(0.01));
        assert!(raw > 65535.0 && (raw - 74_252.0).abs() < 5.0, "{raw}");
        let d = float_document(HW, HH, &scene);
        let out = edit(&d, heal_request(), &box_coverage())
            .expect("the heal returns a document, with no error");
        let after = layer32(&out, 0);
        assert_stored("heal at the bound", &after);
        check_heal("heal at the bound", &scene, &scene, &after);
        let bound = f64::from(sign) * RANGE;
        let mut landed = 0;
        for (i, (was, got)) in scene.iter().zip(&after).enumerate() {
            let (x, y) = (i as u32 % HW, i as u32 / HW);
            if !in_box(x, y) {
                assert_eq!(got.map(f32::to_bits), was.map(f32::to_bits), "at {i}");
                continue;
            }
            assert_eq!(got[3].to_bits(), was[3].to_bits(), "alpha at {i}");
            for value in &got[..3] {
                assert!(value.is_finite() && f64::from(value.abs()) <= RANGE);
                if on_ring(x, y) {
                    // The ring is the membrane's boundary: it keeps the
                    // destination.
                    assert!(
                        close(*value, f64::from(was[0]), 1e-4),
                        "ring at {i}: {value}"
                    );
                } else {
                    assert!(close(*value, bound, 1e-4), "at {i}: {value} vs {bound}");
                    landed += 1;
                }
            }
        }
        assert_eq!(landed, 3 * ((BOX.2 - 2) * (BOX.3 - 2)) as usize);
    }
}

// --------------------------------------------- spot heal and fill --

const SW: u32 = 32;
const SH: u32 = 32;
/// The hole: x, y, width, height.
const HOLE: (u32, u32, u32, u32) = (12, 12, 8, 8);
const SYNTHESIS: [&str; 2] = ["spot_heal", "content_aware_fill"];

fn in_hole(x: u32, y: u32) -> bool {
    (HOLE.0..HOLE.0 + HOLE.2).contains(&x) && (HOLE.1..HOLE.1 + HOLE.3).contains(&y)
}

/// `surround` everywhere but the hole, which holds `inside`; and the hole's
/// coverage at `level`.
fn hole_scene(
    surround: impl Fn(u32, u32) -> [f32; 4],
    inside: [f32; 4],
    level: u16,
) -> (Vec<[f32; 4]>, Vec<u16>) {
    let mut pixels = Vec::new();
    let mut coverage = Vec::new();
    for i in 0..SW * SH {
        let (x, y) = (i % SW, i / SW);
        pixels.push(if in_hole(x, y) {
            inside
        } else {
            surround(x, y)
        });
        coverage.push(if in_hole(x, y) { level } else { 0 });
    }
    (pixels, coverage)
}

fn synthesis(op: &str) -> Value {
    json!({"op":op,"layer":0,"x":HOLE.0,"y":HOLE.1,"width":HOLE.2,"height":HOLE.3,
        "ring":0,"seed":7})
}

/// `after` against a hole whose every pixel should read `want(x, y)` within
/// `tolerance`: alpha and everything outside the hole bitwise kept.
fn check_hole(
    label: &str,
    before: &[[f32; 4]],
    after: &[[f32; 4]],
    want: impl Fn(u32, u32) -> [f64; 3],
    tolerance: f64,
) {
    assert_stored(label, after);
    for (i, (was, got)) in before.iter().zip(after).enumerate() {
        let (x, y) = (i as u32 % SW, i as u32 / SW);
        assert_eq!(got[3].to_bits(), was[3].to_bits(), "{label}: alpha at {i}");
        if !in_hole(x, y) {
            assert_eq!(
                got.map(f32::to_bits),
                was.map(f32::to_bits),
                "{label} at ({x}, {y}): outside the hole is bitwise kept"
            );
            continue;
        }
        let expected = want(x, y);
        for c in 0..3 {
            assert!(
                close(got[c], expected[c], tolerance),
                "{label} at ({x}, {y}) channel {c}: {got:?} vs {expected:?}"
            );
        }
    }
}

#[test]
fn a_hole_in_a_constant_surround_fills_to_it_above_white_and_below_zero() {
    for surround in [[6.0f32, 3.0, 1.5, 1.0], [-0.25, 0.5, 2.0, 1.0]] {
        let (scene, coverage) = hole_scene(|_, _| surround, [0.1, 9.0, -1.0, 1.0], 65535);
        let d = float_document(SW, SH, &scene);
        let want = [surround[0], surround[1], surround[2]].map(f64::from);
        for op in SYNTHESIS {
            let label = format!("{op} in {surround:?}");
            let out = edit(&d, synthesis(op), &coverage).expect("the hole is filled");
            assert_eq!(geometry(&out, 0), ((SW, SH), (0, 0)));
            let after = layer32(&out, 0);
            check_hole(&label, &scene, &after, |_, _| want, 1e-4);
            // One seed twice is the same picture, bit for bit.
            let again = edit(&d, synthesis(op), &coverage).unwrap();
            assert_eq!(bits(&layer32(&again, 0)), bits(&after), "{label}");
            // A new layer holds the filled pixels alone; the target is kept.
            let mut request = synthesis(op);
            request["output"] = json!("new_layer");
            let layered = edit(&d, request, &coverage).expect("the fill adds a layer");
            assert_eq!(layered.layer_count(), 2, "{label}");
            assert_eq!(bits(&layer32(&layered, 0)), bits(&scene), "{label}");
            assert_eq!(geometry(&layered, 1), ((SW, SH), (0, 0)));
            let above = layer32(&layered, 1);
            assert_stored(&label, &above);
            for (i, got) in above.iter().enumerate() {
                if !in_hole(i as u32 % SW, i as u32 / SW) {
                    assert_eq!(got.map(f32::to_bits), [0u32; 4], "{label}: blank at {i}");
                    continue;
                }
                assert_eq!(got[3], 1.0, "{label}: the new layer's pixel is opaque");
                for c in 0..3 {
                    assert!(close(got[c], want[c], 1e-4), "{label} at {i}: {got:?}");
                }
            }
        }
        // The fill's source mask is admitted; the spot heal's parse refuses it.
        let mut masked = synthesis("content_aware_fill");
        masked["source_mask"] = json!({"version":1,"initial":"all","operations":[]});
        let out = edit(&d, masked, &coverage).expect("the masked fill runs");
        check_hole("masked fill", &scene, &layer32(&out, 0), |_, _| want, 1e-4);
        let mut refused = synthesis("spot_heal");
        refused["source_mask"] = json!({"version":1,"initial":"all","operations":[]});
        assert_eq!(
            plan_error(&d, refused),
            "source_mask is not an argument of this retouch operation"
        );
    }
}

#[test]
fn a_hole_in_stripes_continues_each_columns_stripe() {
    // Period-4 stripes of 0.2 and 7.0: two columns each.
    let stripe = |x: u32| {
        if (x / 2).is_multiple_of(2) {
            0.2f32
        } else {
            7.0
        }
    };
    let (scene, coverage) = hole_scene(
        |x, _| [stripe(x), stripe(x), stripe(x), 1.0],
        [1.0, 1.0, 1.0, 1.0],
        65535,
    );
    let d = float_document(SW, SH, &scene);
    for op in SYNTHESIS {
        let out = edit(&d, synthesis(op), &coverage).expect("the hole is filled");
        check_hole(
            &format!("{op} in stripes"),
            &scene,
            &layer32(&out, 0),
            |x, _| [f64::from(stripe(x)); 3],
            1e-3,
        );
    }
}

#[test]
fn a_soft_covered_fill_mixes_in_the_working_encoding() {
    let surround = [6.0f32, 3.0, 1.5, 1.0];
    let (scene, coverage) = hole_scene(|_, _| surround, [1.0, 1.0, 1.0, 1.0], 32768);
    let d = float_document(SW, SH, &scene);
    let out = edit(&d, synthesis("content_aware_fill"), &coverage).expect("the fill mixes");
    // Half-covered: the old value moves `32768 / 65535` of the way to the
    // fill in W, not in linear light.
    let weight = 32768.0 / 65535.0;
    let mixed: [f64; 3] =
        std::array::from_fn(|c| w_inverse(w(1.0) + weight * (w(f64::from(surround[c])) - w(1.0))));
    let linear = 1.0 + weight * (f64::from(surround[0]) - 1.0);
    assert!((mixed[0] - linear).abs() > 0.5, "the two mixes differ");
    check_hole("soft fill", &scene, &layer32(&out, 0), |_, _| mixed, 1e-3);
}

// ------------------------------------------------------------- refusals --

#[test]
fn red_eye_runs_in_place_and_changes_only_its_rect() {
    // A red disc on grey: the float Red Eye corrects it in
    // place and writes nothing outside the rect (`float_filters_develop.rs`
    // holds its 16-bit twin).
    let mut samples = vec![[0.27f32, 0.27, 0.27, 1.0]; (SW * SH) as usize];
    for y in 0..SH {
        for x in 0..SW {
            if (i64::from(x) - 8).pow(2) + (i64::from(y) - 8).pow(2) <= 9 {
                samples[(y * SW + x) as usize] = [0.63, 0.009, 0.009, 1.0];
            }
        }
    }
    let d = float_document(SW, SH, &samples);
    let request = json!({"op":"red_eye","layer":0,"x":3,"y":3,"width":11,"height":11});
    let out = edit(&d, request, &vec![65535; (SW * SH) as usize]).expect("the pupil changes");
    let after = layer32(&out, 0);
    let mut corrected = 0;
    for (i, (a, b)) in after.iter().zip(&samples).enumerate() {
        let (x, y) = (i as u32 % SW, i as u32 / SW);
        if !(3..14).contains(&x) || !(3..14).contains(&y) {
            assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits), "{i}");
        } else if a[0] < b[0] {
            corrected += 1;
        }
    }
    assert!(corrected > 10, "{corrected}");
    assert_stored("red eye", &after);
}

#[test]
fn a_cancelled_control_stops_each_op_before_it_writes() {
    let scene = heal_scene();
    let healed = float_document(HW, HH, &scene);
    let (holed, hole) = hole_scene(|_, _| [6.0, 3.0, 1.5, 1.0], [0.1, 9.0, -1.0, 1.0], 65535);
    let synthesized = float_document(SW, SH, &holed);
    let cases = [
        (
            &healed,
            clone_request(
                [1.0, 0.0, 0.0, 1.0, f64::from(SHIFT.0), f64::from(SHIFT.1)],
                1.0,
                0,
                false,
            ),
            box_coverage(),
        ),
        (&healed, heal_request(), box_coverage()),
        (&synthesized, synthesis("spot_heal"), hole.clone()),
        (&synthesized, synthesis("content_aware_fill"), hole),
    ];
    for (d, request, coverage) in cases {
        let control = DrJobControl::default();
        control.cancel();
        assert_eq!(
            edit_error(d, request.clone(), &coverage, Some(&control)),
            "operation cancelled",
            "{request}"
        );
        // The same request with no control is admitted.
        assert!(edit(d, request, &coverage).is_some());
    }
}

unsafe fn owned_json(value: *mut c_char) -> Value {
    assert!(!value.is_null());
    let parsed = serde_json::from_slice(unsafe { CStr::from_ptr(value) }.to_bytes()).unwrap();
    unsafe { dr_string_free(value) };
    parsed
}

#[test]
fn capability_rows_and_the_operator_table_admit_the_retouch_family() {
    for (operation, target, depth, supported) in [
        ("clone", "raster", 32, true),
        ("heal", "raster", 32, true),
        ("clone", "mask", 32, false),
        ("heal", "mask", 32, false),
        ("clone", "raster", 16, true),
        ("heal", "raster", 16, true),
    ] {
        let name = CString::new(operation).unwrap();
        let plane = CString::new(target).unwrap();
        let mut err = ptr::null_mut();
        let answer = unsafe {
            owned_json(dr_precision_capability(
                name.as_ptr(),
                plane.as_ptr(),
                depth,
                &mut err,
            ))
        };
        assert!(err.is_null());
        assert_eq!(
            answer["supported"], supported,
            "{operation} on {target} at {depth}"
        );
    }
    let table = unsafe { owned_json(dr_float_operators()) };
    let admitted = table["admitted"].as_array().unwrap();
    for row in [
        "clone",
        "heal",
        "spot_heal",
        "content_aware_fill",
        "red_eye",
    ] {
        assert!(admitted.iter().any(|v| v == row), "{row} is admitted");
    }
    let refused = table["refused"].as_array().unwrap();
    // No retouch tool is refused at float any more — the Sponge
    // (lane A, R5) and Red Eye (phase 2, `display_encoding`) both run.
    for tool in ["the Sponge", "Red Eye"] {
        assert!(!refused.iter().any(|row| row["operator"] == tool), "{tool}");
    }
    assert!(!refused
        .iter()
        .any(|row| row["operator"].as_str().unwrap().contains("healing")));
}
