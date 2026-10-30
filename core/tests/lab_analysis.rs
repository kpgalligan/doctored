//! #1 Lab phase 6b-i on a Lab Color document through the FFI: the
//! Histogram, Apply Image's and Calculations' stages and Stack Modes, each
//! the existing kernel run on the L\*a\*b\* codes per channel.
//!
//! # The oracles
//!
//! Each is this file's own arithmetic on the stored codes (`lab_geometry`'s
//! `ramp`, opaque, at 8 and 16 bits):
//!
//! * (a) the Histogram of the composite and of the layer: each channel's
//!   row (Lightness, a, b) holds every opaque pixel's weight in bin
//!   `round(code · 255 / 65535)`, and the descriptor lists `histogram` and
//!   `layer_stack_mode`;
//! * (b) Apply Image and Calculations: `dr_precision_doc_select`'s `color`
//!   red, green and blue read a layer's and the composite's L\*, a\* and b\*
//!   codes exactly; `dr_selection_u16`'s `blend` on one plane and on three
//!   equals the W3C separable formula per channel on the unit codes
//!   (Multiply, Screen, Overlay, Linear Light), `b + (B(b, s) − b)·opacity`,
//!   within 1 u16 code; `scalar_edit` `replace_canvas` onto red, green or
//!   blue writes that component exactly and leaves the others and alpha
//!   unchanged; the blend into a new channel (`set_channel`) is read back
//!   exactly;
//! * (c) Stack Modes median, mean and maximum over three Lab layers equal
//!   the per-channel integer oracle exactly (`stack_stat`'s rounding: the
//!   mean `(2·Σ + n) / (2·n)`, the middle of three, the largest), the result
//!   a Lab document of the same depth.
//!
//! # Bounds, declared before the first run
//!
//! As listed: exact, except (b)'s blends within 1 u16 code.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_precision_edit::{
    dr_precision_doc_edit_plan, dr_precision_doc_read_plane, dr_precision_doc_select,
};
use doctored_core::ffi_scoped_statistics::dr_precision_doc_scoped_analysis;
use doctored_core::ffi_selection_u16::dr_selection_u16;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::gray_ops::{edit, free, layer, plan_json, LIMIT};
use crate::lab_geometry::{descriptor, document, ramp, twin_profile, DEPTHS, H, W};

const N: usize = (W * H) as usize;

/// A Lab document of `count` canvas-sized opaque ramp layers at `bits`.
fn stack(bits: u32, count: u32) -> *mut PrecisionDocument {
    let twin = twin_profile();
    let records: Vec<Layer> = (0..count)
        .map(|k| {
            let pixels = ramp(W, H, bits, k * 17)
                .into_iter()
                .map(|[l, a, b, _]| [l, a, b, 65535])
                .collect();
            let image = PrecisionImage::from_rgba16(W, H, pixels, twin.clone(), None).unwrap();
            Layer::raster(&format!("Layer {k}"), image, (0, 0), 1.0, true).unwrap()
        })
        .collect();
    document(&records, bits, true)
}

fn analysis(doc: *const PrecisionDocument, request: &Value) -> Value {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_scoped_analysis(
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
    plan_json(out, err).unwrap()
}

#[test]
fn the_histogram_of_lab_bins_each_channels_codes() {
    for bits in DEPTHS {
        let doc = stack(bits, 1);
        let codes = layer(doc, 0, bits);
        for request in [
            json!({"operation":"histogram","source":"composite"}),
            json!({"operation":"histogram","source":"layer","layer":0}),
        ] {
            let what = format!("{request} at {bits} bits");
            let report = analysis(doc, &request);
            let total = report["sampled_rgb_weight_units"].as_u64().unwrap();
            assert_eq!(total % N as u64, 0, "{what}: {total}");
            let unit = total / N as u64;
            let rows = report["bin_weight_units"].as_array().unwrap();
            for c in 0..3 {
                let mut want = vec![0u64; 256];
                for p in &codes {
                    want[(f64::from(p[c]) * 255.0 / 65535.0).round() as usize] += unit;
                }
                let got: Vec<u64> = rows[c]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap())
                    .collect();
                assert_eq!(got, want, "{what}: channel {c}");
            }
        }
        let operations = descriptor(doc)["supported_operations"].clone();
        for family in ["histogram", "layer_stack_mode"] {
            assert!(
                operations.as_array().unwrap().contains(&family.into()),
                "{family}: {operations}"
            );
        }
        free(doc);
    }
}

/// One canvas plane through `dr_precision_doc_select` (the host's
/// `NativeSelectionWork`): `operation` of the composite, or of `layer`.
fn plane(doc: *const PrecisionDocument, layer: Option<usize>, operation: Value) -> Vec<u16> {
    let mut request = json!({"op":"selection","source":"composite","operation":operation});
    if let Some(layer) = layer {
        request["source"] = json!("layer");
        request["layer"] = json!(layer);
    }
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let planned = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    let planned = plan_json(planned, err).unwrap();
    let mut out = vec![999u16; planned["output_samples"].as_u64().unwrap() as usize];
    assert_eq!(out.len(), N);
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_select(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{request}: {}", take_err_string(err));
    out
}

/// The host's `NativePlaneReader.blend`: `base` planes then `source` planes
/// through `dr_selection_u16`'s `blend`.
fn blend(base: &[Vec<u16>], source: &[Vec<u16>], mode: i32, opacity: f64) -> Vec<Vec<u16>> {
    let channels = base.len();
    let request = CString::new(
        json!({"kind":"blend","channels":channels,"mode":mode,"opacity":opacity}).to_string(),
    )
    .unwrap();
    let input: Vec<u16> = base.iter().chain(source).flatten().copied().collect();
    let mut out = vec![0u16; N * channels];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_selection_u16(
            W,
            H,
            request.as_ptr(),
            input.as_ptr(),
            input.len(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
    out.chunks(N).map(<[u16]>::to_vec).collect()
}

/// The W3C separable formula of `mode` (DrBlendMode) on unit values.
fn separable(mode: i32, b: f64, s: f64) -> f64 {
    match mode {
        1 => b * s,
        2 => b + s - b * s,
        3 => {
            if b <= 0.5 {
                2.0 * b * s
            } else {
                1.0 - 2.0 * (1.0 - b) * (1.0 - s)
            }
        }
        19 => (b + 2.0 * s - 1.0).clamp(0.0, 1.0),
        _ => unreachable!("mode {mode}"),
    }
}

/// (b)'s oracle of one plane pair.
fn oracle(base: &[u16], source: &[u16], mode: i32, opacity: f64) -> Vec<f64> {
    base.iter()
        .zip(source)
        .map(|(&b, &s)| {
            let (b, s) = (f64::from(b) / 65535.0, f64::from(s) / 65535.0);
            let v = b + (separable(mode, b, s) - b) * opacity;
            v.clamp(0.0, 1.0) * 65535.0
        })
        .collect()
}

fn assert_near(got: &[u16], want: &[f64], what: &str) {
    for (i, (&g, &w)) in got.iter().zip(want).enumerate() {
        assert!(
            (f64::from(g) - w).abs() <= 1.0,
            "{what}: pixel {i}: {g} against {w}"
        );
    }
}

/// `scalar_edit` `replace_canvas` of `target` on layer `index`; frees `doc`
/// when it changed.
fn write(
    doc: *mut PrecisionDocument,
    target: &str,
    index: usize,
    samples: &[u16],
) -> *mut PrecisionDocument {
    let request = json!({"op":"scalar_edit","target":target,"index":index,
        "operation":{"kind":"replace_canvas"}});
    match edit(doc, &request, samples).unwrap_or_else(|e| panic!("{target}: {e}")) {
        Some(next) => {
            free(doc);
            next
        }
        None => doc,
    }
}

fn channel(doc: *const PrecisionDocument, index: usize) -> Vec<u16> {
    let text =
        CString::new(json!({"op":"read_plane","target":"channel","index":index}).to_string())
            .unwrap();
    let mut out = vec![0u16; N];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_read_plane(
            doc,
            text.as_ptr(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
    out
}

const NAMES: [&str; 3] = ["red", "green", "blue"];

#[test]
fn apply_image_and_calculations_blend_each_lab_channel_alone() {
    for bits in DEPTHS {
        let mut doc = stack(bits, 2);
        let colour =
            |doc, layer, c: usize| plane(doc, layer, json!({"kind":"color","channel":NAMES[c]}));
        // The reads are the codes.
        for index in 0..2 {
            let codes = layer(doc, index, bits);
            for c in 0..3 {
                let want: Vec<u16> = codes.iter().map(|p| p[c] as u16).collect();
                assert_eq!(
                    colour(doc, Some(index), c),
                    want,
                    "layer {index} {c} at {bits}"
                );
            }
        }
        let top = layer(doc, 1, bits);
        for c in 0..3 {
            let want: Vec<u16> = top.iter().map(|p| p[c] as u16).collect();
            assert_eq!(colour(doc, None, c), want, "composite {c} at {bits}");
        }
        for (mode, opacity) in [(1, 1.0), (2, 0.6), (3, 1.0), (19, 1.0)] {
            let what = format!("mode {mode} at {bits} bits");
            // One plane: the layer's Lightness from the composite's a.
            let base = colour(doc, Some(0), 0);
            let source = colour(doc, None, 1);
            let got = blend(
                std::slice::from_ref(&base),
                std::slice::from_ref(&source),
                mode,
                opacity,
            );
            assert_near(&got[0], &oracle(&base, &source, mode, opacity), &what);
            // Three planes: each channel alone.
            let bases: Vec<_> = (0..3).map(|c| colour(doc, Some(0), c)).collect();
            let sources: Vec<_> = (0..3).map(|c| colour(doc, Some(1), c)).collect();
            let got = blend(&bases, &sources, mode, opacity);
            for c in 0..3 {
                let want = oracle(&bases[c], &sources[c], mode, opacity);
                assert_near(&got[c], &want, &format!("{what} channel {c}"));
            }
        }
        // The plane writes: one component each, exactly.
        let bases: Vec<_> = (0..3).map(|c| colour(doc, Some(0), c)).collect();
        let sources: Vec<_> = (0..3).map(|c| colour(doc, Some(1), c)).collect();
        let blended = blend(&bases, &sources, 2, 1.0);
        for c in 0..3 {
            let before = layer(doc, 0, bits);
            doc = write(doc, NAMES[c], 0, &blended[c]);
            let after = layer(doc, 0, bits);
            for (i, (a, b)) in after.iter().zip(&before).enumerate() {
                for k in 0..4 {
                    let want = if k == c {
                        u32::from(blended[c][i])
                    } else {
                        b[k]
                    };
                    assert_eq!(
                        a[k], want,
                        "write {} pixel {i} component {k} at {bits}",
                        NAMES[c]
                    );
                }
            }
            assert_eq!(descriptor(doc)["color_mode"], "lab");
        }
        // Calculations: the result into a new channel.
        let base = colour(doc, Some(0), 0);
        let source = colour(doc, Some(1), 2);
        let result = blend(&[base], &[source], 1, 1.0);
        let request = json!({"op":"set_channel","index":null,"name":"Calc"});
        let next = edit(doc, &request, &result[0]).unwrap().unwrap();
        assert_eq!(channel(next, 0), result[0], "Calculations at {bits}");
        free(next);
        free(doc);
    }
}

/// `stack_stat`'s integer statistic of three opaque samples.
fn statistic(mode: &str, v: [u32; 3]) -> u32 {
    let mut sorted = v;
    sorted.sort_unstable();
    match mode {
        "mean" => {
            let sum: u32 = v.iter().sum();
            (2 * sum + 3) / 6
        }
        "median" => sorted[1],
        "maximum" => sorted[2],
        _ => unreachable!(),
    }
}

#[test]
fn stack_modes_on_lab_are_per_channel_on_the_codes() {
    for bits in DEPTHS {
        let doc = stack(bits, 3);
        let members: Vec<_> = (0..3).map(|k| layer(doc, k, bits)).collect();
        for mode in ["median", "mean", "maximum"] {
            let what = format!("{mode} at {bits} bits");
            let request = json!({"op":"stack_mode","layers":[0,1,2],"above":2,"mode":mode,
                "name":"Stat"});
            let out = edit(doc, &request, &[])
                .unwrap_or_else(|e| panic!("{what}: {e}"))
                .unwrap();
            let d = descriptor(out);
            assert_eq!(d["color_mode"], "lab", "{what}");
            assert_eq!(d["lab"]["depth"], bits, "{what}");
            assert_eq!(unsafe { &*out }.layer_count(), 4, "{what}");
            let got = layer(out, 3, bits);
            for (i, p) in got.iter().enumerate() {
                for c in 0..3 {
                    let v = [members[0][i][c], members[1][i][c], members[2][i][c]];
                    assert_eq!(p[c], statistic(mode, v), "{what}: pixel {i} channel {c}");
                }
                assert_eq!(p[3], 65535, "{what}: pixel {i} alpha");
            }
            free(out);
        }
        free(doc);
    }
}
