//! Layer ▸ Warp at 32-bit float.
//!
//! The yardstick is the independent binary64 warp oracle the persisted warp
//! suite uses (`common/warp_oracle.rs`: lattice nodes, the bilinear
//! displacement, a Newton inverse, the reference taps and the destination
//! bounds), with the float finish restated here: PREMULTIPLIED accumulation,
//! the unpremultiply by the RAW accumulated alpha, D2 (an accumulated alpha
//! below 2^-24 is transparent), D3 (each RGB component clamped to the source
//! layer's own visible range, per channel over its alpha > 0 pixels) and an
//! overshooting alpha clamped to 1.
//!
//! # The tolerance
//!
//! The shipped walk accumulates in binary64, rounds the four sums to f32 once
//! and divides in f32, so each component is compared with
//! `|Δ| ≤ 8·ε₃₂·Σ|wᵢaᵢvᵢ|/a_ref + ε₃₂·|ref| + 1e-8·Σ|aᵢvᵢ|/a_ref` and alpha
//! with `|Δ| ≤ 8·ε₃₂·Σ|wᵢaᵢ| + 1e-8·Σaᵢ`, where `a_ref` is the reference's raw
//! accumulated alpha. The 1e-8 terms cover the shipped inverse's 1e-10 px
//! residual against the oracle's (a tap weight moves by at most a few times
//! the position error). A pixel whose `a_ref` lies within the alpha
//! tolerance of 2^-24 is skipped: either side of D2 is correct there.
//!
//! The fixture is a 23×17 32-bit float TIFF opened through the shipped
//! reader: negatives, ±65504, partial alpha, alpha-0 pixels whose HIDDEN RGB
//! lies outside the visible range, and one alpha of 1e-31.
#![allow(clippy::needless_range_loop)]

use crate::warp_fixture;
use crate::warp_oracle;

use std::ffi::CString;
use std::ptr;

use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_warp::dr_precision_doc_warp_edit;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::tiff_float::{encode_to_vec, FloatTiff};
use doctored_core::warp::WarpEdit;
use serde_json::{json, Value};
use warp_fixture::field;
use warp_oracle::{bounds, inverse, nodes, taps};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const EPS: f64 = f32::EPSILON as f64;
/// D2's output floor, 2^-24, `sampling::EXTENDED_ALPHA_FLOOR` restated.
const FLOOR: f64 = 1.0 / 16_777_216.0;
/// The fixed f32 rounding multiplier; see the module doc.
const K: f64 = 8.0;
/// The inverse-residual allowance; see the module doc.
const RESIDUAL: f64 = 1e-8;
const W: u32 = 23;
const H: u32 = 17;

// ---------------------------------------------------------------- fixtures --

fn hash(i: u32) -> u32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^ (x >> 12)
}

/// Straight RGBA. Columns 0..3 are alpha 0 with LOUD hidden RGB, one of them
/// carries an alpha of 1e-31, column 3 is partial alpha and the rest is
/// opaque or partial with values in [-3, 30]; `extremes` puts ±65504 into
/// the visible set.
fn scene(extremes: bool) -> Vec<[f32; 4]> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            let r = hash(i);
            let v = |shift: u32| ((r >> shift) & 0xff) as f32 / 255.0 * 33.0 - 3.0;
            if x < 3 {
                if x == 1 && y == H / 2 {
                    return [5.0, 5.0, 5.0, 1e-31];
                }
                let hidden = if r & 1 == 0 { 60000.0 } else { -60000.0 };
                return [hidden, -hidden, hidden, 0.0];
            }
            let alpha = if x == 3 || r.is_multiple_of(7) {
                ((r >> 4) % 1000) as f32 / 1000.0 * 0.9 + 0.05
            } else {
                1.0
            };
            let mut p = [v(8), v(16), v(24), alpha];
            if extremes && (x, y) == (W / 2, H / 2) {
                p = [65504.0, -65504.0, 65504.0, 1.0];
            }
            if extremes && (x, y) == (W / 2 + 3, H / 2 - 2) {
                p = [-65504.0, 65504.0, -1.0, 1.0];
            }
            p
        })
        .collect()
}

fn float_doc(pixels: &[[f32; 4]]) -> PrecisionDocument {
    let tiff = FloatTiff {
        width: W,
        height: H,
        channels: 4,
        alpha: Some(false),
        samples: pixels.iter().flatten().copied().collect(),
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("warp.tiff");
    std::fs::write(&path, encode_to_vec(&tiff, false).unwrap()).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    assert!(doc.is_float());
    assert_eq!(
        bits(&source(&doc)),
        bits(pixels),
        "the fixture opens bit-exact"
    );
    doc
}

fn u16_doc() -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            W,
            H,
            vec![[1000, 2000, 3000, 65535]; (W * H) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

/// A placed (non-canvas) mask, linked or not.
fn mask(linked: bool) -> PrecisionMask {
    let (w, h) = (19u32, 13u32);
    let samples: Vec<u16> = (0..w * h).map(|i| (hash(i + 7) % 65_536) as u16).collect();
    let mut properties = MaskProperties::at((1, -2));
    properties.linked = linked;
    PrecisionMask::new(
        PrecisionPlane::from_u16(w, h, samples, None).unwrap(),
        properties,
        true,
    )
    .unwrap()
}

// ----------------------------------------------------------------- helpers --

fn bits(pixels: &[[f32; 4]]) -> Vec<u32> {
    pixels.iter().flatten().map(|v| v.to_bits()).collect()
}

fn source(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    let image = doc.layer_source(0).expect("a raster record");
    assert_eq!(image.storage(), PrecisionStorage::Rgba32Float);
    image.to_rgba32().expect("float storage")
}

fn extent(doc: &PrecisionDocument) -> (i32, i32, u32, u32) {
    let (x, y) = doc.layer(0).unwrap().offset();
    let (w, h) = doc.layer_source(0).unwrap().dimensions();
    (x, y, w, h)
}

fn warp(doc: &PrecisionDocument, request: &Value) -> PrecisionDocument {
    let edit = WarpEdit::parse(&request.to_string()).expect("a warp request");
    doc.warp_edit(&edit, LIMIT, 0, None)
        .unwrap_or_else(|e| panic!("{request} runs: {e}"))
        .unwrap_or_else(|| panic!("{request} changes something"))
}

fn ffi_warp(doc: &PrecisionDocument, request: &Value) -> PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_warp_edit(doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut error)
    };
    if !error.is_null() {
        let text = unsafe { std::ffi::CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { doctored_core::ffi::dr_string_free(error) };
        panic!("{request} reported: {text}");
    }
    assert!(!out.is_null(), "{request} returned nothing");
    *unsafe { Box::from_raw(out) }
}

fn ffi_edit(doc: &PrecisionDocument, value: &Value) -> PrecisionDocument {
    let text = CString::new(value.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if !error.is_null() {
        let text = unsafe { std::ffi::CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { doctored_core::ffi::dr_string_free(error) };
        panic!("{value} reported: {text}");
    }
    assert!(!out.is_null(), "{value} returned nothing");
    *unsafe { Box::from_raw(out) }
}

/// A set request whose every node is `displacement`.
fn uniform(displacement: [f64; 2], filter: &str) -> Value {
    json!({"op":"set","layer":0,"target":"pixels","columns":3,"rows":3,
        "displacements":vec![displacement; 9],"filter":filter})
}

/// D3's range, restated: per-channel min/max over alpha > 0.
fn visible_range(pixels: &[[f32; 4]]) -> ([f64; 3], [f64; 3]) {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for p in pixels.iter().filter(|p| p[3] > 0.0) {
        for c in 0..3 {
            min[c] = min[c].min(f64::from(p[c]));
            max[c] = max[c].max(f64::from(p[c]));
        }
    }
    if min[0].is_infinite() {
        return ([0.0; 3], [0.0; 3]);
    }
    (min, max)
}

// --------------------------------------------------------------- reference --

/// One reference pixel: the signed premultiplied sums, the sums of their
/// magnitudes (`Σ|wᵢaᵢvᵢ|`, `Σ|wᵢaᵢ|`) and the unweighted magnitudes
/// (`Σ|aᵢvᵢ|`, `Σaᵢ`) over the taps that contribute.
struct Acc {
    sum: [f64; 4],
    abs: [f64; 4],
    mag: [f64; 4],
}

fn reference(
    request: &Value,
    source: &[[f32; 4]],
    size: (u32, u32),
    offset: (i32, i32),
) -> ((i32, i32, u32, u32), Vec<Acc>) {
    let (c, r, n, bicubic) = nodes(request);
    let rect = (offset.0, offset.1, size.0, size.1);
    let dest = bounds(c, r, &n, rect);
    let mut out = Vec::new();
    for y in 0..dest.3 {
        for x in 0..dest.2 {
            let q = [
                f64::from(dest.0) + f64::from(x) + 0.5,
                f64::from(dest.1) + f64::from(y) + 0.5,
            ];
            let p = inverse(c, r, &n, rect, q);
            let xt = taps(p[0] - f64::from(offset.0), bicubic);
            let yt = taps(p[1] - f64::from(offset.1), bicubic);
            let mut acc = Acc {
                sum: [0.0; 4],
                abs: [0.0; 4],
                mag: [0.0; 4],
            };
            for (sy, wy) in &yt {
                for (sx, wx) in &xt {
                    if *sx < 0 || *sy < 0 || *sx >= i64::from(size.0) || *sy >= i64::from(size.1) {
                        continue;
                    }
                    let px = source[*sy as usize * size.0 as usize + *sx as usize];
                    let w = wy * wx;
                    let a = f64::from(px[3]);
                    for k in 0..3 {
                        let v = f64::from(px[k]);
                        acc.sum[k] += w * a * v;
                        acc.abs[k] += (w * a * v).abs();
                        acc.mag[k] += (a * v).abs();
                    }
                    acc.sum[3] += w * a;
                    acc.abs[3] += (w * a).abs();
                    acc.mag[3] += a;
                }
            }
            out.push(acc);
        }
    }
    (dest, out)
}

/// (b) on every result, then (a) against the reference. Returns how many
/// visible pixels were compared component by component.
fn assert_matches(got: &[[f32; 4]], want: &[Acc], source: &[[f32; 4]], what: &str) -> usize {
    assert_eq!(got.len(), want.len(), "{what}: sample count");
    let (min, max) = visible_range(source);
    let mut compared = 0;
    for (index, (g, r)) in got.iter().zip(want).enumerate() {
        let a = f64::from(g[3]);
        assert!(
            a == 0.0 || a >= FLOOR,
            "{what}: alpha {a:e} at {index} is inside (0, 2^-24)"
        );
        if a > 0.0 {
            for c in 0..3 {
                let v = f64::from(g[c]);
                assert!(
                    v >= min[c] && v <= max[c],
                    "{what}: channel {c} = {v} at {index} leaves the visible range [{}, {}]",
                    min[c],
                    max[c]
                );
            }
        }
        let alpha_tolerance = K * EPS * r.abs[3] + RESIDUAL * r.mag[3];
        if (r.sum[3] - FLOOR).abs() <= alpha_tolerance {
            continue;
        }
        if r.sum[3] < FLOOR {
            assert_eq!(
                g.map(f32::to_bits),
                [0u32; 4],
                "{what}: {index} is below 2^-24 in the reference and stores zeros"
            );
            continue;
        }
        let want_alpha = r.sum[3].min(1.0);
        assert!(
            (a - want_alpha).abs() <= alpha_tolerance,
            "{what}: alpha at {index} is {a} against {want_alpha}"
        );
        for c in 0..3 {
            let want_c = (r.sum[c] / r.sum[3]).clamp(min[c], max[c]);
            let tolerance =
                K * EPS * r.abs[c] / r.sum[3] + EPS * want_c.abs() + RESIDUAL * r.mag[c] / r.sum[3];
            let v = f64::from(g[c]);
            assert!(
                (v - want_c).abs() <= tolerance,
                "{what}: channel {c} at {index} is {v} against {want_c} (tolerance {tolerance:e})"
            );
        }
        compared += 1;
    }
    compared
}

fn assert_warp_matches(base: &PrecisionDocument, out: &PrecisionDocument, request: &Value) {
    let pixels = source(base);
    let (x, y, w, h) = extent(base);
    let (dest, want) = reference(request, &pixels, (w, h), (x, y));
    assert_eq!(extent(out), dest, "{request}: the destination extent");
    let compared = assert_matches(&source(out), &want, &pixels, &request.to_string());
    assert!(compared > 0, "{request}: some visible pixel is compared");
}

// ------------------------------------------------------------------- tests --

/// (a) + (b): bilinear and bicubic over two lattices match the f64 oracle,
/// one of them through the FFI.
#[test]
fn a_float_warp_matches_the_reference() {
    for extremes in [false, true] {
        let base = float_doc(&scene(extremes));
        for (columns, rows) in [(3, 3), (4, 2)] {
            for filter in ["bilinear", "bicubic"] {
                let request = field(columns, rows, filter);
                let out = if extremes && (columns, rows, filter) == (3, 3, "bicubic") {
                    ffi_warp(&base, &request)
                } else {
                    warp(&base, &request)
                };
                assert!(out.is_float());
                assert_warp_matches(&base, &out, &request);
            }
        }
    }
}

/// (c): an integer translation shares the samples and moves the offset; a
/// fractional warp at that non-zero origin still matches the oracle.
#[test]
fn an_integer_translation_shares_samples_and_a_later_warp_matches() {
    let base = float_doc(&scene(true));
    let moved = warp(&base, &uniform([2.0, -3.0], "bicubic"));
    assert!(
        PrecisionImage::ptr_eq(
            base.layer_source(0).unwrap(),
            moved.layer_source(0).unwrap()
        ),
        "an integer translation shares the allocation"
    );
    assert_eq!(bits(&source(&moved)), bits(&source(&base)));
    assert_eq!(moved.layer(0).unwrap().offset(), (2, -3));
    for filter in ["bilinear", "bicubic"] {
        let request = field(3, 3, filter);
        let out = warp(&moved, &request);
        assert_warp_matches(&moved, &out, &request);
    }
}

/// (d): a linked mask rides a pixel warp, and a mask-only warp leaves the
/// f32 pixels alone; either way the mask equals the same request on a u16
/// document carrying the same mask.
#[test]
fn a_float_mask_warp_equals_the_same_mask_on_a_u16_document() {
    let float = float_doc(&scene(false))
        .set_layer_mask(0, Some(mask(true)), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let typed = u16_doc()
        .set_layer_mask(0, Some(mask(true)), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    for target in ["pixels", "mask"] {
        for filter in ["bilinear", "bicubic"] {
            let mut request = field(3, 3, filter);
            request["target"] = Value::String(target.into());
            let a = warp(&float, &request);
            let b = warp(&typed, &request);
            let (ma, mb) = (
                a.layer(0).unwrap().mask().unwrap(),
                b.layer(0).unwrap().mask().unwrap(),
            );
            assert_ne!(ma.properties().offset, (1, -2), "{request}: the mask moved");
            assert_eq!(ma.properties().offset, mb.properties().offset, "{request}");
            assert_eq!(
                ma.plane().dimensions(),
                mb.plane().dimensions(),
                "{request}"
            );
            assert_eq!(ma.plane().samples(), mb.plane().samples(), "{request}");
            if target == "mask" {
                assert!(
                    PrecisionImage::ptr_eq(
                        a.layer_source(0).unwrap(),
                        float.layer_source(0).unwrap()
                    ),
                    "{request}: a mask-only warp leaves the f32 pixels untouched"
                );
            } else {
                assert_warp_matches(&float, &a, &request);
            }
        }
    }
    // An unlinked mask stays where it is under a pixel warp.
    let unlinked = float_doc(&scene(false))
        .set_layer_mask(0, Some(mask(false)), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let out = warp(&unlinked, &field(3, 3, "bicubic"));
    let (before, after) = (
        unlinked.layer(0).unwrap().mask().unwrap(),
        out.layer(0).unwrap().mask().unwrap(),
    );
    assert_eq!(after.properties().offset, before.properties().offset);
    assert_eq!(after.plane().samples(), before.plane().samples());
}

/// (e): a float set keeps no field; clear, bake and identity change nothing,
/// and a second identical set warps the warped pixels again.
#[test]
fn a_float_warp_keeps_no_field_and_sets_compound() {
    let base = float_doc(&scene(false));
    assert_eq!(
        base.warp_info(0).unwrap(),
        json!({"schema_version":1,"field":null})
    );
    let request = field(3, 3, "bicubic");
    let once = warp(&base, &request);
    assert_eq!(
        once.warp_info(0).unwrap(),
        json!({"schema_version":1,"field":null}),
        "a float warp keeps no re-editable field"
    );
    for (what, edit) in [
        ("clear", json!({"op":"clear","layer":0})),
        ("bake", json!({"op":"bake","layer":0})),
        ("identity", uniform([0.0, 0.0], "bicubic")),
    ] {
        let edit = WarpEdit::parse(&edit.to_string()).unwrap();
        assert!(
            once.warp_edit(&edit, LIMIT, 0, None)
                .unwrap_or_else(|e| panic!("{what}: {e}"))
                .is_none(),
            "{what} changes nothing at float"
        );
        assert_eq!(
            once.warp_plan(&edit).unwrap().scratch_bytes,
            0,
            "{what} plans nothing"
        );
    }
    let twice = warp(&once, &request);
    assert_ne!(
        bits(&source(&twice)),
        bits(&source(&once)),
        "a second identical set warps the pixels again"
    );
    assert_warp_matches(&once, &twice, &request);
}

/// The plan charges 16 B/px of output plus the source copy and the output's
/// tiling copy.
#[test]
fn the_float_plan_charges_the_f32_output_and_copies() {
    let base = float_doc(&scene(false));
    let request = field(3, 3, "bicubic");
    let (c, r, n, _) = nodes(&request);
    let dest = bounds(c, r, &n, (0, 0, W, H));
    let plan = base
        .warp_plan(&WarpEdit::parse(&request.to_string()).unwrap())
        .unwrap();
    let out = u64::from(dest.2) * u64::from(dest.3);
    assert!(
        plan.scratch_bytes >= out * 16 + u64::from(W * H) * 16 + out * 16,
        "scratch {} covers the f32 output, the source copy and the adoption",
        plan.scratch_bytes
    );
    assert!(plan.retained_bytes >= out * 16);
}

/// (f): a warped float layer survives the v40 round trip bit-exact.
#[test]
fn a_float_warp_survives_the_v40_round_trip_bit_exact() {
    let base = float_doc(&scene(true));
    for filter in ["bilinear", "bicubic"] {
        let out = warp(&base, &field(4, 2, filter));
        let bytes = out.encode_native(LIMIT, 0, None).expect("v40 bytes");
        let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
        assert!(reopened.is_float(), "{filter}");
        assert_eq!(extent(&reopened), extent(&out), "{filter}");
        assert_eq!(
            bits(&source(&reopened)),
            bits(&source(&out)),
            "{filter}: every f32 bit survives"
        );
        assert_eq!(
            reopened.encode_native(LIMIT, 0, None).expect("resave"),
            bytes,
            "{filter}: reopen then resave is identical"
        );
    }
}

/// (g): a precancelled job errs and keeps nothing reserved.
#[test]
fn a_cancelled_float_warp_errs_with_no_result() {
    let base = float_doc(&scene(true));
    let control = DrJobControl::default();
    control.cancel();
    let edit = WarpEdit::parse(&field(3, 3, "bicubic").to_string()).unwrap();
    let message = match base.warp_edit(&edit, LIMIT, 0, Some(&control)) {
        Err(message) => message,
        Ok(_) => panic!("a cancelled job must not produce a result"),
    };
    assert!(message.contains("cancel"), "{message}");
    assert_eq!(control.scratch_bytes(), 0, "nothing stays reserved");
}

/// (h): a bicubic warp of an HDR edge, under Exposure +2, renders and
/// merges: D3 keeps the overshoot inside the layer's range, so ×4 stays
/// inside ±65504, and D2 keeps every alpha well above the 2^-96 floor.
#[test]
fn a_bicubic_hdr_edge_warp_then_exposure_renders_and_merges() {
    let mut pixels = scene(false);
    for p in &mut pixels {
        if p[3] > 0.0 && p[3] < 1e-20 {
            *p = [0.0; 4];
        }
    }
    for y in 0..H {
        pixels[(y * W + 3) as usize] = [16000.0, 0.25, -2.0, 1.0];
        pixels[(y * W + 4) as usize] = [-16000.0, 15000.0, 3.0, 0.5];
    }
    let base = float_doc(&pixels);
    let out = warp(&base, &field(3, 3, "bicubic"));
    let exposed = out
        .add_adjustment(
            "Exposure",
            "{\"type\":\"adjust\",\"evaluation\":\"precision_v1\",\"op\":\"exposure\",\"params\":{\"exposure\":2.0}}",
            1.0,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(exposed.float_refusal(), None);
    let rendered = exposed.render(LIMIT, 0, None).expect("the render succeeds");
    assert_eq!(rendered.storage(), PrecisionStorage::Rgba32Float);
    let merged = ffi_edit(&exposed, &json!({"op": "merge_down", "layer": 1}));
    assert!(merged.is_float());
    merged
        .render(LIMIT, 0, None)
        .expect("the merged render succeeds");
}
