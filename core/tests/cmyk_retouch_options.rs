//! The retouch brushes' two options on a CMYK
//! document, through the FFI, against oracles written here in f64:
//!
//! * the Sponge's Vibrance on a uniform patch runs the Vibrance stage
//!   (`common/vibrance_reference.rs`) on `[L_c, L_m, L_y] = 1 − [C, M, Y]`
//!   through the sRGB curve and holds the K plate bit for bit;
//! * Sample All Layers' Blur and Smudge fill an empty target record from the
//!   ink picture of every visible layer (`SAMPLE_ALL_LAYERS` is `All`),
//!   flattened by hand from the records' own ink: Blur as the alpha-weighted
//!   Gaussian of each `L` plate of that picture, with its alpha, Smudge
//!   inside each plate's range of it; a visible layer above is read and a
//!   hidden one is not.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_retouch_brush::dr_precision_doc_smudge;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::retouch_sampling::RetouchSampling;
use doctored_core::SAMPLE_ALL_LAYERS;
use serde_json::json;

use crate::cmyk_structure_fixture::{
    cmyk_layers, composite, free, refusal, then, try_edit, RgbLayer, LIMIT,
};
use crate::common::take_err_string;
use crate::vibrance_reference;

const W: u32 = 20;
const H: u32 = 14;
const HARD: &str =
    r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#;

/// The Vibrance stage at slider `v` (a fraction) on one encoded triple
/// through the sRGB curve (`retouch_brush`'s `Encoded`): decoded, the stage in
/// linear light, encoded.
fn vibrance(rgb: [f64; 3], v: f64) -> [f64; 3] {
    let decode = |c: f64| {
        let c = c.clamp(0.0, 1.0);
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let encode = |u: f64| {
        if u <= 0.0031308 {
            12.92 * u
        } else {
            1.055 * u.powf(1.0 / 2.4) - 0.055
        }
    };
    vibrance_reference::stage(rgb.map(decode), v).map(encode)
}

fn taps(sigma: f64) -> Vec<f64> {
    let half = (3.0 * sigma).ceil() as i64;
    let raw: Vec<f64> = (-half..=half)
        .map(|k| (-0.5 * (k as f64 / sigma).powi(2)).exp())
        .collect();
    let sum: f64 = raw.iter().sum();
    raw.iter().map(|v| v / sum).collect()
}

fn samples(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 5]> {
    unsafe { &*doc }.cmyk_layer_samples(index).unwrap()
}

fn within(got: u16, want: f64, bound: f64, what: &str) {
    assert!(
        (f64::from(got) - want).abs() <= bound,
        "{what}: {got} against {want}"
    );
}

#[test]
fn cmyk_vibrance_runs_the_stage_on_the_cmy_pass_and_holds_k() {
    for rgb in [[52000_u16, 30000, 21000], [9000, 41000, 60000], [30000; 3]] {
        let doc = cmyk_layers(
            W,
            H,
            vec![RgbLayer {
                name: "Patch",
                w: W,
                h: H,
                offset: (0, 0),
                pixels: vec![[rgb[0], rgb[1], rgb[2], 65535]; (W * H) as usize],
            }],
        );
        let source = samples(doc, 0);
        for (kind, slider) in [("saturate", 1.0), ("desaturate", -1.0)] {
            let request = json!({"op":"retouch_brush","layer":0,"kind":kind,"strength":1.0,
                "vibrance":true})
            .to_string();
            let out = try_edit(doc, &request, &vec![65535; (W * H) as usize]).expect(kind);
            let got = if out.is_null() {
                source.clone()
            } else {
                let got = samples(out, 0);
                free(out);
                got
            };
            for (g, s) in got.iter().zip(&source) {
                let l = [s[0], s[1], s[2]].map(|v| 1.0 - f64::from(v) / 65535.0);
                let target = vibrance(l, slider);
                for c in 0..3 {
                    within(g[c], (1.0 - target[c]) * 65535.0, 3.0, kind);
                }
                assert_eq!(g[3], s[3], "{kind}: the K plate is held bit for bit");
                assert_eq!(g[4], s[4], "{kind}: alpha");
            }
        }
        free(doc);
    }
}

/// An opaque varied background, an EMPTY target (record 1) and, when
/// `above`, an opaque patch over the right half (record 2).
fn stack(above: bool) -> *mut PrecisionDocument {
    let n = (W * H) as usize;
    let background = (0..n)
        .map(|i| {
            let (x, y) = (i as u32 % W, i as u32 / W);
            [
                ((x * 3001 + y * 977) % 65536) as u16,
                ((x * 811 + y * 4409) % 65536) as u16,
                if (x / 3 + y / 3) % 2 == 0 {
                    8000
                } else {
                    58000
                },
                65535,
            ]
        })
        .collect();
    let mut layers = vec![
        RgbLayer {
            name: "Background",
            w: W,
            h: H,
            offset: (0, 0),
            pixels: background,
        },
        RgbLayer {
            name: "Target",
            w: W,
            h: H,
            offset: (0, 0),
            pixels: vec![[0; 4]; n],
        },
    ];
    if above {
        layers.push(RgbLayer {
            name: "Above",
            w: W,
            h: H,
            offset: (0, 0),
            pixels: (0..n)
                .map(|i| {
                    if i as u32 % W >= W / 2 {
                        [65535, 0, 0, 65535]
                    } else {
                        [0; 4]
                    }
                })
                .collect(),
        });
    }
    cmyk_layers(W, H, layers)
}

/// The document's own composite of [`stack`], straight, as
/// `[L_c, L_m, L_y, L_k, α]` in unit range.
fn picture(above: bool) -> Vec<[f64; 5]> {
    let source = stack(above);
    let out = (0..(W * H) as i32)
        .map(|i| {
            let s = composite(source, i % W as i32, i / W as i32);
            [
                1.0 - f64::from(s[0]) / 65535.0,
                1.0 - f64::from(s[1]) / 65535.0,
                1.0 - f64::from(s[2]) / 65535.0,
                1.0 - f64::from(s[3]) / 65535.0,
                f64::from(s[4]) / 65535.0,
            ]
        })
        .collect();
    free(source);
    out
}

/// The ink picture of every visible layer of a [`stack`] document `doc`,
/// flattened by hand from the records' own ink: record 2 where it is opaque
/// (when `above`), else the background record, as `[L_c, L_m, L_y, L_k, α]`
/// in unit range.
fn hand_picture(doc: *const PrecisionDocument, above: bool) -> Vec<[f64; 5]> {
    let background = samples(doc, 0);
    let top = above.then(|| samples(doc, 2));
    (0..background.len())
        .map(|i| {
            let s = match &top {
                Some(t) if t[i][4] == 65535 => t[i],
                _ => background[i],
            };
            let l = |v: u16| 1.0 - f64::from(v) / 65535.0;
            [
                l(s[0]),
                l(s[1]),
                l(s[2]),
                l(s[3]),
                f64::from(s[4]) / 65535.0,
            ]
        })
        .collect()
}

/// Plate `plate`'s alpha-weighted Gaussian (sigma 2) at (x, y), reads
/// clamped to the canvas.
fn blur_plate(picture: &[[f64; 5]], plate: usize, x: u32, y: u32) -> f64 {
    let k = taps(2.0);
    let half = (k.len() / 2) as i64;
    let (mut acc, mut total) = (0.0, 0.0);
    for (j, ky) in k.iter().enumerate() {
        for (i, kx) in k.iter().enumerate() {
            let cx = (i64::from(x) + i as i64 - half).clamp(0, i64::from(W) - 1);
            let cy = (i64::from(y) + j as i64 - half).clamp(0, i64::from(H) - 1);
            let p = picture[(cy * i64::from(W) + cx) as usize];
            acc += kx * ky * p[4] * p[plate];
            total += kx * ky * p[4];
        }
    }
    acc / total
}

#[test]
fn cmyk_sampled_blur_fills_an_empty_record_from_the_ink_picture() {
    assert_eq!(SAMPLE_ALL_LAYERS, RetouchSampling::All);
    let doc = stack(true);
    let full = vec![65535_u16; (W * H) as usize];
    let request = json!({"op":"retouch_brush","layer":1,"kind":"blur","strength":1.0,
        "sample_all_layers":true})
    .to_string();
    let out = try_edit(doc, &request, &full).expect("no refusal");
    assert!(!out.is_null(), "an empty record fills");
    let got = samples(out, 1);
    let ink = hand_picture(doc, true);
    assert_eq!(
        ink,
        picture(true),
        "the hand-flattened picture is the composite"
    );
    for (i, g) in got.iter().enumerate() {
        let (x, y) = (i as u32 % W, i as u32 / W);
        within(g[4], ink[i][4] * 65535.0, 2.0, "alpha");
        for (plate, value) in g.iter().take(4).enumerate() {
            within(
                *value,
                (1.0 - blur_plate(&ink, plate, x, y)) * 65535.0,
                6.0,
                &format!("plate {plate} at ({x}, {y})"),
            );
        }
    }
    // The background and the layer above are untouched.
    assert_eq!(samples(out, 0), samples(doc, 0));
    assert_eq!(samples(out, 2), samples(doc, 2));
    let without = stack(false);
    let lone = try_edit(without, &request, &full).expect("no refusal");
    assert_ne!(samples(lone, 1), got, "the visible layer above is read");
    let hidden = then(
        stack(true),
        r#"{"op":"layer_visible","layer":2,"visible":false}"#,
        &[],
    );
    let unseen = try_edit(hidden, &request, &full).expect("no refusal");
    assert_eq!(
        samples(unseen, 1),
        samples(lone, 1),
        "a hidden layer above is not read"
    );
    for d in [out, lone, without, unseen, hidden, doc] {
        free(d);
    }
}

#[test]
fn cmyk_sampled_smudge_smears_the_ink_picture_into_an_empty_record() {
    assert_eq!(SAMPLE_ALL_LAYERS, RetouchSampling::All);
    let doc = stack(true);
    let got = smear(doc);
    let ink = hand_picture(doc, true);
    let mut written = 0;
    for (i, g) in got.iter().enumerate() {
        if g[4] == 0 {
            continue;
        }
        written += 1;
        // A written sample is a smear of the picture: inside each plate's
        // range of it.
        for plate in 0..4 {
            let (lo, hi) = ink.iter().fold((1.0_f64, 0.0_f64), |(lo, hi), p| {
                (lo.min(p[plate]), hi.max(p[plate]))
            });
            let l = 1.0 - f64::from(g[plate]) / 65535.0;
            assert!(
                l >= lo - 1e-3 && l <= hi + 1e-3,
                "sample {i} plate {plate}: {l} outside [{lo}, {hi}]"
            );
        }
    }
    assert!(written > 0);
    let without = stack(false);
    assert_ne!(smear(without), got, "the visible layer above is read");
    free(without);
    free(doc);
}

/// Record 1 of `doc` after the sampled Smudge's three dabs, strength 0.6.
fn smear(doc: *const PrecisionDocument) -> Vec<[u16; 5]> {
    let brush = CString::new(HARD).unwrap();
    let dabs = [5.0, 7.0, 8.0, 1.0, 10.0, 7.0, 8.0, 1.0, 15.0, 7.0, 8.0, 1.0];
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_smudge(
            doc,
            1,
            brush.as_ptr(),
            dabs.as_ptr(),
            3,
            0.6,
            ptr::null(),
            0,
            true,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null(), "{}", take_err_string(err));
    assert!(!out.is_null(), "the smear writes into the empty record");
    let got = samples(out, 1);
    free(out);
    got
}

#[test]
fn cmyk_options_refuse_by_name() {
    let doc = stack(false);
    let full = vec![65535_u16; (W * H) as usize];
    assert_eq!(
        refusal(
            doc,
            &json!({"op":"retouch_brush","layer":0,"kind":"blur","strength":1.0,
                "vibrance":true})
            .to_string(),
            &full
        ),
        "vibrance applies to the sponge only"
    );
    assert_eq!(
        refusal(
            doc,
            &json!({"op":"retouch_brush","layer":0,"kind":"desaturate","strength":1.0,
                "sample_all_layers":true})
            .to_string(),
            &full
        ),
        "sample_all_layers applies to blur, sharpen and smudge"
    );
    free(doc);
}
