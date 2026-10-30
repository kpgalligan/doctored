//! The retouch brushes on a CMYK document (#547), through the FFI, each
//! against an oracle written here:
//!
//! * Dodge/Burn, Blur, Sharpen, Smudge and the History Brush equal, exactly,
//!   the shipped RGB16 walk on the two `L = 1 − ink` twin documents
//!   (`[L_c, L_m, L_y, α]` and `[L_k; 3, α]`, built here from the record's
//!   own samples at its offset on the canvas), mapped back — on an offset
//!   layer inside a group, under a stroke and a selection;
//! * the Sponge runs the C/M/Y twin alone: the K plate is bit for bit the
//!   source's;
//! * Dodge/Burn with and without Protect Tones against an f64 statement of
//!   the curve on `L` and the group rule (`[L_c, L_m, L_y]` scaled by
//!   `curve(V) / V`, K the plain curve);
//! * the History Brush refuses a state that is not a CMYK raster, by name.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_retouch_brush::{dr_precision_doc_history_brush, dr_precision_doc_smudge};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::cmyk_structure_fixture::{cmyk_layers, free, then, varied, RgbLayer, LIMIT};
use crate::common::take_err_string;

const W: u32 = 22;
const H: u32 = 18;
const TARGET: usize = 1;
const HARD: &str =
    r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#;

/// A 22 × 18 CMYK document: an opaque background and a 13 × 10 layer of
/// every alpha class at (6, 5), grouped (record 1 inside group record 2).
fn document(seed: u32) -> *mut PrecisionDocument {
    let doc = cmyk_layers(
        W,
        H,
        vec![
            RgbLayer {
                name: "Background",
                w: W,
                h: H,
                offset: (0, 0),
                pixels: varied(W, H, seed)
                    .into_iter()
                    .map(|p| [p[0], p[1], p[2], 65535])
                    .collect(),
            },
            RgbLayer {
                name: "Offset",
                w: 13,
                h: 10,
                offset: (6, 5),
                pixels: varied(13, 10, seed + 7),
            },
        ],
    );
    then(
        doc,
        r#"{"op":"group_layers","layers":[1],"name":"Group","pass_through":false}"#,
        &[],
    )
}

/// A soft stroke (a ramp over the canvas) and a selection of the left
/// two-thirds, as one input.
fn input(selection: bool) -> Vec<u16> {
    let n = (W * H) as usize;
    let mut out: Vec<u16> = (0..n)
        .map(|i| {
            let (x, y) = ((i as u32 % W) as f64, (i as u32 / W) as f64);
            let d = ((x - 11.0).powi(2) + (y - 9.0).powi(2)).sqrt();
            ((1.0 - d / 10.0).clamp(0.0, 1.0) * 65535.0).round() as u16
        })
        .collect();
    if selection {
        out.extend((0..n).map(|i| if (i as u32 % W) < 15 { 65535 } else { 20000 }));
    }
    out
}

fn edit(doc: *const PrecisionDocument, request: &Value, samples: &[u16]) -> *mut PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null(), "{request}: {}", take_err_string(err));
    out
}

/// A record's samples, size and offset.
type Record = (Vec<[u16; 5]>, (u32, u32), (i32, i32));

/// The record's samples, its size and its offset.
fn record(doc: *const PrecisionDocument, index: usize) -> Record {
    let layer = unsafe { &*doc }.layer(index).unwrap();
    let dims = layer.cmyk_source().unwrap().dimensions();
    (
        unsafe { &*doc }.cmyk_layer_samples(index).unwrap(),
        dims,
        layer.offset(),
    )
}

/// The `L` twin of a record (pass `k`: `[L_k; 3]`), canvas sized, the layer
/// at its offset.
fn twin(doc: *const PrecisionDocument, index: usize, k: bool) -> *mut PrecisionDocument {
    let (samples, (w, h), offset) = record(doc, index);
    let pixels = samples
        .iter()
        .map(|s| {
            if k {
                [65535 - s[3], 65535 - s[3], 65535 - s[3], s[4]]
            } else {
                [65535 - s[0], 65535 - s[1], 65535 - s[2], s[4]]
            }
        })
        .collect();
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("twin", image, offset, 1.0, true).unwrap();
    Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            W,
            H,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            vec![layer],
        )
        .unwrap(),
    ))
}

/// A twin's layer after `out` (or the twin itself when `out` is NULL).
fn twin_pixels(twin: *mut PrecisionDocument, out: *mut PrecisionDocument) -> Vec<[u16; 4]> {
    let from = if out.is_null() { twin } else { out };
    let px = unsafe { &*from }
        .layer(0)
        .unwrap()
        .source()
        .unwrap()
        .to_rgba16()
        .unwrap();
    if !out.is_null() {
        free(out);
    }
    free(twin);
    px
}

/// The module doc's join: α = A's; α 0 keeps the source ink; K = 65535 −
/// B's G, or the source's K without a B.
fn joined(source: &[[u16; 5]], a: &[[u16; 4]], b: Option<&[[u16; 4]]>) -> Vec<[u16; 5]> {
    source
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let pa = a[i];
            if pa[3] == 0 {
                [s[0], s[1], s[2], s[3], 0]
            } else {
                let k = b.map_or(s[3], |b| 65535 - b[i][1]);
                [65535 - pa[0], 65535 - pa[1], 65535 - pa[2], k, pa[3]]
            }
        })
        .collect()
}

/// `run` on both twins (only A when `k_pass` is false), mapped back.
fn twin_oracle(
    doc: *const PrecisionDocument,
    k_pass: bool,
    run: impl Fn(*mut PrecisionDocument, bool) -> *mut PrecisionDocument,
) -> Vec<[u16; 5]> {
    let (source, ..) = record(doc, TARGET);
    let a = {
        let t = twin(doc, TARGET, false);
        twin_pixels(t, run(t, false))
    };
    let b = k_pass.then(|| {
        let t = twin(doc, TARGET, true);
        twin_pixels(t, run(t, true))
    });
    joined(&source, &a, b.as_deref())
}

/// `request` with `layer` replaced.
fn at(request: &Value, layer: usize) -> Value {
    let mut out = request.clone();
    out["layer"] = json!(layer);
    out
}

fn assert_twin_equal(request: Value, k_pass: bool, selection: bool) {
    let doc = document(3);
    let samples = input(selection);
    let out = edit(doc, &at(&request, TARGET), &samples);
    assert!(!out.is_null(), "{request} changed the record");
    let want = twin_oracle(doc, k_pass, |twin, _| {
        edit(twin, &at(&request, 0), &samples)
    });
    let (got, ..) = record(out, TARGET);
    assert_eq!(got, want, "{request}");
    if !k_pass {
        let (source, ..) = record(doc, TARGET);
        for (g, s) in got.iter().zip(&source) {
            assert_eq!(g[3], s[3], "{request}: the K plate is held exactly");
        }
    }
    // The record stays a CMYK raster inside its group; the background is
    // untouched.
    assert_eq!(record(out, 0).0, record(doc, 0).0);
    free(out);
    free(doc);
}

#[test]
fn dodge_and_burn_equal_their_two_rgb16_twins_with_and_without_protect_tones() {
    for (range, burn, protect) in [(0, false, false), (1, true, true), (2, false, true)] {
        for selection in [false, true] {
            assert_twin_equal(
                json!({"op":"dodge_burn","layer":0,"exposure":0.6,"range":range,"burn":burn,
                    "protect_tones":protect,"selection":selection}),
                true,
                selection,
            );
        }
    }
}

#[test]
fn blur_and_sharpen_equal_their_twins_and_the_sponge_holds_k() {
    for selection in [false, true] {
        for (kind, k_pass) in [
            ("blur", true),
            ("sharpen", true),
            ("saturate", false),
            ("desaturate", false),
        ] {
            assert_twin_equal(
                json!({"op":"retouch_brush","layer":0,"kind":kind,"strength":0.7,
                    "selection":selection}),
                k_pass,
                selection,
            );
        }
    }
}

fn smudge(
    doc: *const PrecisionDocument,
    layer: usize,
    dabs: &[f64],
    selection: &[u16],
) -> Result<*mut PrecisionDocument, String> {
    let brush = CString::new(HARD).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_smudge(
            doc,
            layer,
            brush.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            0.8,
            if selection.is_empty() {
                ptr::null()
            } else {
                selection.as_ptr()
            },
            selection.len(),
            false,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if !err.is_null() {
        return Err(take_err_string(err));
    }
    Ok(out)
}

#[test]
fn smudge_equals_its_two_rgb16_twins() {
    let dabs: Vec<f64> = (0..12)
        .flat_map(|i| [5.0 + f64::from(i), 7.0 + f64::from(i) * 0.5, 6.0, 1.0])
        .collect();
    let n = (W * H) as usize;
    for selection in [Vec::new(), input(true)[n..].to_vec()] {
        let doc = document(5);
        let out = smudge(doc, TARGET, &dabs, &selection).unwrap();
        assert!(!out.is_null());
        let want = twin_oracle(doc, true, |twin, _| {
            smudge(twin, 0, &dabs, &selection).unwrap()
        });
        assert_eq!(record(out, TARGET).0, want);
        free(out);
        free(doc);
    }
}

fn history(
    doc: *const PrecisionDocument,
    layer: usize,
    source: *const PrecisionDocument,
    input: &[u16],
) -> Result<*mut PrecisionDocument, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_history_brush(
            doc,
            layer,
            source,
            layer,
            0.75,
            input.as_ptr(),
            input.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if !err.is_null() {
        return Err(take_err_string(err));
    }
    Ok(out)
}

#[test]
fn the_history_brush_paints_from_the_states_two_passes() {
    for selection in [false, true] {
        let doc = document(3);
        let state = document(9);
        let samples = input(selection);
        let out = history(doc, TARGET, state, &samples).unwrap();
        assert!(!out.is_null());
        let want = twin_oracle(doc, true, |twin, k| {
            let from = self::twin(state, TARGET, k);
            let result = history(twin, 0, from, &samples).unwrap();
            free(from);
            result
        });
        assert_eq!(record(out, TARGET).0, want);
        free(out);
        free(doc);
        free(state);
    }
}

#[test]
fn the_history_brush_refuses_a_state_that_is_not_a_cmyk_raster() {
    let doc = document(3);
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_image(
            PrecisionImage::from_rgba16(
                W,
                H,
                vec![[0; 4]; (W * H) as usize],
                IccProfile::srgb(),
                None,
            )
            .unwrap(),
            Metadata::default(),
            Resolution::default(),
        )
        .unwrap(),
    ));
    let samples = input(false);
    assert_eq!(
        history(doc, TARGET, rgb, &samples).unwrap_err(),
        "The History Brush's source state is not a CMYK raster"
    );
    // The group record has no pixels of its own.
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_history_brush(
            doc,
            TARGET,
            doc,
            2,
            0.5,
            samples.as_ptr(),
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(
        unsafe { CStr::from_ptr(err) }.to_str().unwrap(),
        "The History Brush's source state is not a CMYK raster"
    );
    unsafe { doctored_core::ffi::dr_string_free(err) };
    free(rgb);
    free(doc);
}

/// `doc_retouch::shaped` restated in f64 (its doc comment's curves).
fn shaped(v: f64, e: f64, range: u8, burn: bool) -> f64 {
    let k = e / 3.0;
    match (range, burn) {
        (0, false) => k + v * (1.0 - k),
        (0, true) => ((v - k) / (1.0 - k)).max(0.0),
        (1, false) => v.powf(1.0 / (1.0 + e)),
        (1, true) => v.powf(1.0 + e),
        (2, false) => (v / (1.0 - k)).min(1.0),
        _ => v * (1.0 - k),
    }
}

/// The group rule: every value × `shaped(V) / V`, `V` the largest.
fn protected(values: &[f64], e: f64, range: u8, burn: bool) -> Vec<f64> {
    let v = values.iter().copied().fold(0.0, f64::max);
    if v <= 0.0 {
        return vec![shaped(0.0, e, range, burn); values.len()];
    }
    let scale = shaped(v, e, range, burn) / v;
    values.iter().map(|x| x * scale).collect()
}

#[test]
fn dodge_and_burn_on_ink_match_the_f64_curve_and_group_rule() {
    let e = 0.6;
    for (range, burn) in [(0, false), (1, true), (2, false), (1, false)] {
        for protect in [false, true] {
            let doc = document(4);
            let samples = input(false);
            let request = json!({"op":"dodge_burn","layer":TARGET,"exposure":e,"range":range,
                "burn":burn,"protect_tones":protect});
            let out = edit(doc, &request, &samples);
            let (before, (w, _), (ox, oy)) = record(doc, TARGET);
            let (after, ..) = record(out, TARGET);
            for (i, (b, a)) in before.iter().zip(&after).enumerate() {
                let (x, y) = (i as i32 % w as i32 + ox, i as i32 / w as i32 + oy);
                let c = f64::from(samples[(y as u32 * W + x as u32) as usize]) / 65535.0;
                if b[4] == 0 || c == 0.0 {
                    assert_eq!(a, b, "pixel {i} is untouched");
                    continue;
                }
                let l: Vec<f64> = (0..4).map(|p| f64::from(65535 - b[p]) / 65535.0).collect();
                let cmy = if protect {
                    protected(&l[..3], e, range, burn)
                } else {
                    l[..3].iter().map(|&v| shaped(v, e, range, burn)).collect()
                };
                let target = [cmy[0], cmy[1], cmy[2], shaped(l[3], e, range, burn)];
                for p in 0..4 {
                    let lo = l[p] * (1.0 - c) + target[p] * c;
                    let want = 65535.0 - (lo * 65535.0).round();
                    assert!(
                        (f64::from(a[p]) - want).abs() <= 1.0,
                        "range {range} burn {burn} protect {protect} pixel {i} plate {p}: {} vs {want}",
                        a[p]
                    );
                }
                assert_eq!(a[4], b[4], "alpha is never written");
            }
            free(out);
            free(doc);
        }
    }
}
