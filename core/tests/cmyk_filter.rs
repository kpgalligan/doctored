//! The Filter menu and Edit ▸ Fade on a CMYK document (#547), through the
//! FFI, each against an oracle written here:
//!
//! * every filter equals, exactly, the RGB16 selected-edit results of the two
//!   `L = 1 − ink` twin documents (`[L_c, L_m, L_y, α]` and `[L_k; 3, α]`)
//!   built here from the layer's own samples, mapped back — on an offset
//!   layer, with and without a selection;
//! * a transparency-locked disc keeps every alpha code under Gaussian Blur
//!   while its ink changes;
//! * Add Noise's K plate is not C's (another seed); Clouds is one field over
//!   every plate it reaches;
//! * Fade against an f64 statement of `fade::fade_pixel` on ink (Normal,
//!   Multiply on `L`, frozen alpha, a stroke's changed samples only);
//! * identity filters answer NULL; refusals by sentence; the descriptor's
//!   plans.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_fade::dr_precision_doc_fade;
use doctored_core::ffi_precision::{dr_precision_doc_descriptor, dr_precision_doc_free};
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Map, Value};

use crate::cmyk_structure_fixture::{cmyk_layers, then, varied, RgbLayer};
use crate::common::take_err_string;

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

fn free(document: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(document) };
}

fn samples(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 5]> {
    unsafe { &*doc }
        .cmyk_layer_samples(index)
        .expect("a CMYK record")
}

fn selected(
    doc: *const PrecisionDocument,
    request: &Value,
    coverage: &[u16],
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            doc,
            text.as_ptr(),
            if coverage.is_empty() {
                ptr::null()
            } else {
                coverage.as_ptr()
            },
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() && !err.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

/// A 20 × 16 CMYK document: a canvas-sized background and an offset
/// 12 × 9 layer (index 1) of every alpha class.
fn document() -> *mut PrecisionDocument {
    cmyk_layers(
        20,
        16,
        vec![
            RgbLayer {
                name: "Background",
                w: 20,
                h: 16,
                offset: (0, 0),
                pixels: varied(20, 16, 3)
                    .into_iter()
                    .map(|p| [p[0], p[1], p[2], 65535])
                    .collect(),
            },
            RgbLayer {
                name: "Offset",
                w: 12,
                h: 9,
                offset: (5, 4),
                pixels: varied(12, 9, 11),
            },
        ],
    )
}

/// The layer's canvas offset (the public record API).
fn offset(doc: *const PrecisionDocument, index: usize) -> (i32, i32) {
    unsafe { &*doc }.layer(index).unwrap().offset()
}

/// `params` as pass `k` (false: C/M/Y, true: K) runs them: the clouds inks
/// as `#rrggbb` of their `L`, Add Noise's K seed salted.
fn pass_params(op: &str, params: &Map<String, Value>, k: bool) -> Map<String, Value> {
    let mut out = params.clone();
    if op == "clouds" || op == "difference_clouds" {
        let ink = |key: &str, default: [f64; 4]| -> [f64; 4] {
            out.get(key).map_or(default, |v| {
                let a = v.as_array().unwrap();
                std::array::from_fn(|i| a[i].as_f64().unwrap())
            })
        };
        let fg = ink("foreground_ink", [0.0, 0.0, 0.0, 1.0]);
        let bg = ink("background_ink", [0.0; 4]);
        out.remove("foreground_ink");
        out.remove("background_ink");
        let hex = |i: [f64; 4]| {
            let l = |v: f64| (255.0 * (1.0 - v)).round() as u8;
            let rgb = if k {
                [l(i[3]); 3]
            } else {
                [l(i[0]), l(i[1]), l(i[2])]
            };
            format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
        };
        out.insert("foreground_color".into(), json!(hex(fg)));
        out.insert("background_color".into(), json!(hex(bg)));
    }
    if op == "add_noise" && k {
        let seed = out.get("seed").and_then(Value::as_u64).unwrap_or(1);
        out.insert("seed".into(), json!(seed ^ 0x9E37_79B9_7F4A_7C15));
    }
    out
}

/// The oracle: the two twin documents' RGB16 selected edits, mapped back.
fn twin_result(
    source: &[[u16; 5]],
    (w, h): (u32, u32),
    layer_offset: (i32, i32),
    canvas: (u32, u32),
    op: &str,
    params: &Map<String, Value>,
    coverage: &[u16],
) -> Vec<[u16; 5]> {
    let local: Vec<u16> = if coverage.is_empty() {
        Vec::new()
    } else {
        (0..(w * h) as usize)
            .map(|i| {
                let x = (i % w as usize) as i64 + i64::from(layer_offset.0);
                let y = (i / w as usize) as i64 + i64::from(layer_offset.1);
                if x < 0 || y < 0 || x >= i64::from(canvas.0) || y >= i64::from(canvas.1) {
                    0
                } else {
                    coverage[y as usize * canvas.0 as usize + x as usize]
                }
            })
            .collect()
    };
    let run = |pixels: Vec<[u16; 4]>, k: bool| -> Vec<[u16; 4]> {
        let image =
            PrecisionImage::from_rgba16(w, h, pixels.clone(), IccProfile::srgb(), None).unwrap();
        let doc = Box::into_raw(Box::new(
            PrecisionDocument::from_image(image, Metadata::default(), Resolution::default())
                .unwrap(),
        ));
        let request = json!({"layer":0,"op":op,"params":pass_params(op, params, k)});
        let out = selected(doc, &request, &local).unwrap_or_else(|e| panic!("{request}: {e}"));
        let result = if out.is_null() {
            pixels
        } else {
            let px = unsafe { &*out }
                .layer(0)
                .unwrap()
                .source()
                .unwrap()
                .to_rgba16()
                .unwrap();
            free(out);
            px
        };
        free(doc);
        result
    };
    let a = run(
        source
            .iter()
            .map(|s| [65535 - s[0], 65535 - s[1], 65535 - s[2], s[4]])
            .collect(),
        false,
    );
    let b = run(
        source
            .iter()
            .map(|s| [65535 - s[3], 65535 - s[3], 65535 - s[3], s[4]])
            .collect(),
        true,
    );
    source
        .iter()
        .zip(a.iter().zip(&b))
        .map(|(s, (a, b))| {
            if a[3] == 0 {
                [s[0], s[1], s[2], s[3], 0]
            } else {
                [65535 - a[0], 65535 - a[1], 65535 - a[2], 65535 - b[1], a[3]]
            }
        })
        .collect()
}

fn filters() -> Vec<(&'static str, Value)> {
    vec![
        ("blur", json!({"sigma":2})),
        ("gaussian_blur", json!({"radius":3})),
        ("box_blur", json!({"radius":2})),
        ("surface_blur", json!({"radius":3,"threshold":15})),
        ("motion_blur", json!({"angle":30,"distance":5})),
        (
            "radial_blur",
            json!({"amount":10,"method":"spin","quality":"draft"}),
        ),
        (
            "radial_blur",
            json!({"amount":20,"method":"zoom","quality":"good"}),
        ),
        (
            "unsharp_mask",
            json!({"radius":1.5,"amount":1.2,"threshold":0.0157}),
        ),
        ("sharpen", json!({})),
        ("smart_sharpen", json!({})),
        ("high_pass", json!({"radius":3})),
        ("dust_and_scratches", json!({"radius":2,"threshold":0})),
        ("reduce_noise", json!({})),
        ("pixelate", json!({"block":3})),
        ("emboss", json!({})),
        ("edge_detect", json!({})),
        (
            "offset",
            json!({"horizontal":3,"vertical":-2,"undefined_areas":"wrap"}),
        ),
        (
            "offset",
            json!({"horizontal":2,"vertical":1,"undefined_areas":"transparent"}),
        ),
        ("lens_blur", json!({"radius":3})),
        ("tilt_shift", json!({})),
        ("add_noise", json!({"amount":0.2,"seed":7})),
        (
            "clouds",
            json!({"seed":3,"foreground_ink":[0.2,0.6,0.1,0.9],"background_ink":[0,0,0.3,0]}),
        ),
        ("difference_clouds", json!({"seed":5})),
        // Liquify is phase 4b's (`cmyk_warp`, `cmyk_warp.rs`), routed before
        // `cmyk_filter`, so it is not a two-pass twin.
    ]
}

#[test]
fn each_filter_equals_its_two_rgb_twin_passes() {
    let doc = document();
    let source = samples(doc, 1);
    let offset1 = offset(doc, 1);
    assert_eq!(offset1, (5, 4), "the layer is offset");
    let canvas = (20u32, 16u32);
    let selection: Vec<u16> = (0..(canvas.0 * canvas.1) as usize)
        .map(|i| {
            let (x, y) = (i % 20, i / 20);
            if (7..16).contains(&x) && (5..12).contains(&y) {
                [65535, 40000, 65535][i % 3]
            } else {
                0
            }
        })
        .collect();
    for (op, params) in filters() {
        let params = params.as_object().unwrap().clone();
        for coverage in [&[][..], &selection[..]] {
            let request = json!({"layer":1,"op":op,"params":params});
            let out =
                selected(doc, &request, coverage).unwrap_or_else(|e| panic!("{request}: {e}"));
            let want = twin_result(&source, (12, 9), offset1, canvas, op, &params, coverage);
            let got = if out.is_null() {
                source.clone()
            } else {
                samples(out, 1)
            };
            for (i, (g, w)) in got.iter().zip(&want).enumerate() {
                assert_eq!(
                    g,
                    w,
                    "{op} {params:?} (selection {}): pixel {i}",
                    !coverage.is_empty()
                );
            }
            if !out.is_null() {
                assert_eq!(
                    samples(out, 0),
                    samples(doc, 0),
                    "{op}: other records untouched"
                );
                free(out);
            }
        }
    }
    free(doc);
}

#[test]
fn a_transparency_locked_disc_keeps_every_alpha_code() {
    let (w, h) = (16u32, 16u32);
    let disc: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let (x, y) = ((i % w) as f64 - 7.5, (i / w) as f64 - 7.5);
            let inside = x * x + y * y <= 25.0;
            [
                (i * 4099 % 65536) as u16,
                (i * 911 % 65536) as u16,
                (i * 2711 % 65536) as u16,
                if inside { 65535 } else { 0 },
            ]
        })
        .collect();
    let doc = cmyk_layers(
        w,
        h,
        vec![RgbLayer {
            name: "Disc",
            w,
            h,
            offset: (0, 0),
            pixels: disc,
        }],
    );
    let source = samples(doc, 0);
    let locked = then(doc, r#"{"op":"layer_lock","layer":0,"locks":1}"#, &[]);
    let out = selected(
        locked,
        &json!({"layer":0,"op":"gaussian_blur","params":{"radius":3}}),
        &[],
    )
    .unwrap();
    assert!(!out.is_null(), "the blur changes the ink");
    let got = samples(out, 0);
    let mut moved = 0;
    for (i, (g, s)) in got.iter().zip(&source).enumerate() {
        assert_eq!(g[4], s[4], "pixel {i}: the alpha code is the source's");
        moved += usize::from(g[..4] != s[..4]);
    }
    assert!(moved > 10, "the ink changed ({moved} pixels)");
    free(out);
    free(locked);
}

#[test]
fn add_noise_salts_k_and_clouds_is_one_field() {
    let (w, h) = (24u32, 20u32);
    let doc = cmyk_layers(
        w,
        h,
        vec![RgbLayer {
            name: "Grey",
            w,
            h,
            offset: (0, 0),
            pixels: vec![[30000, 30000, 30000, 65535]; (w * h) as usize],
        }],
    );
    let source = samples(doc, 0);
    let noisy = selected(
        doc,
        &json!({"layer":0,"op":"add_noise","params":{"amount":0.3,"seed":9}}),
        &[],
    )
    .unwrap();
    let got = samples(noisy, 0);
    let delta = |plate: usize| -> Vec<i64> {
        got.iter()
            .zip(&source)
            .map(|(g, s)| i64::from(g[plate]) - i64::from(s[plate]))
            .collect()
    };
    assert_ne!(delta(0), delta(3), "K's noise is not C's");
    assert!(delta(3).iter().any(|d| *d != 0), "K is noised");
    // Clouds with an ink that is equal on every plate: one field everywhere.
    let clouds = selected(
        doc,
        &json!({"layer":0,"op":"clouds","params":{"seed":4,"foreground_ink":[1,1,1,1],"background_ink":[0,0,0,0]}}),
        &[],
    )
    .unwrap();
    let field = samples(clouds, 0);
    assert!(field.iter().any(|s| s[0] != field[0][0]), "the clouds vary");
    for (i, s) in field.iter().enumerate() {
        assert!(
            s[0] == s[1] && s[1] == s[2] && s[2] == s[3],
            "pixel {i}: one field over every plate: {s:?}"
        );
    }
    // The default inks: K over paper, C/M/Y left at none.
    let k_only = selected(
        doc,
        &json!({"layer":0,"op":"clouds","params":{"seed":4}}),
        &[],
    )
    .unwrap();
    for s in samples(k_only, 0) {
        assert_eq!(&s[..3], &[0, 0, 0], "the default clouds are K over paper");
    }
    for d in [noisy, clouds, k_only, doc] {
        free(d);
    }
}

// -------------------------------------------------------------- fade --

fn unit(v: u16) -> f64 {
    f64::from(v) / 65535.0
}

/// `blend`'s premultiplied gate: `dst` toward `src` by `t`.
fn gate(dst: [f64; 5], src: [f64; 5], t: f64) -> [f64; 5] {
    let a = dst[4] + (src[4] - dst[4]) * t;
    let mut out = [0.0; 5];
    out[4] = a;
    if a > 0.0 {
        for c in 0..4 {
            out[c] = (dst[c] * dst[4] * (1.0 - t) + src[c] * src[4] * t) / a;
        }
    }
    out
}

/// The W3C step with Multiply on `L = 1 − ink`, straight in and out.
fn multiply_over(backdrop: [f64; 5], source: [f64; 5]) -> [f64; 5] {
    let (ab, as_) = (backdrop[4], source[4]);
    let ao = as_ + ab * (1.0 - as_);
    let mut out = [0.0; 5];
    out[4] = ao;
    for c in 0..4 {
        let blended = 1.0 - (1.0 - backdrop[c]) * (1.0 - source[c]);
        out[c] =
            ((1.0 - ab) * as_ * source[c] + as_ * ab * blended + (1.0 - as_) * ab * backdrop[c])
                / ao;
    }
    out
}

fn fade_oracle(
    before: [f64; 5],
    after: [f64; 5],
    multiply: bool,
    weight: f64,
    opacity: f64,
    frozen: bool,
) -> [f64; 5] {
    if before[4] <= 0.0 && after[4] <= 0.0 {
        return after;
    }
    if weight <= 0.0 && before == after {
        return after;
    }
    let target = if !multiply {
        after
    } else if after[4] <= 0.0 {
        before
    } else {
        multiply_over(before, after)
    };
    let weighted = gate(after, target, weight);
    let mut out = gate(before, weighted, opacity);
    if frozen {
        out[4] = after[4];
    }
    out
}

fn fade(
    after: *const PrecisionDocument,
    before: *const PrecisionDocument,
    mode: i32,
    opacity: f64,
    coverage: &[u16],
    changed_only: bool,
) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_fade(
            after,
            1,
            before,
            mode,
            opacity,
            if coverage.is_empty() {
                ptr::null()
            } else {
                coverage.as_ptr()
            },
            coverage.len(),
            changed_only,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "fade: {}", take_err_string(err));
    out
}

#[test]
fn fade_is_fade_pixel_on_ink() {
    let before = document();
    let canvas = 20 * 16;
    let coverage: Vec<u16> = (0..canvas).map(|i| (i * 2399 % 65536) as u16).collect();
    let after = selected(
        before,
        &json!({"layer":1,"op":"gaussian_blur","params":{"radius":2}}),
        &[],
    )
    .unwrap();
    let (b, a) = (samples(before, 1), samples(after, 1));
    let off = offset(before, 1);
    let weight_at = |i: usize, region: bool| -> f64 {
        if !region {
            return 1.0;
        }
        let (x, y) = (i % 12 + off.0 as usize, i / 12 + off.1 as usize);
        unit(coverage[y * 20 + x])
    };
    for (mode, multiply, opacity, region, changed_only) in [
        (0, false, 0.5, false, false),
        (1, true, 0.7, true, false),
        (0, false, 0.35, false, true),
    ] {
        let cover: &[u16] = if region { &coverage } else { &[] };
        let out = fade(after, before, mode, opacity, cover, changed_only);
        for (i, ((g, x), y)) in samples(out, 1).iter().zip(&b).zip(&a).enumerate() {
            let changed = x != y;
            let weight = if changed_only {
                f64::from(u8::from(changed))
            } else {
                weight_at(i, region)
            };
            let want = fade_oracle(x.map(unit), y.map(unit), multiply, weight, opacity, false);
            for c in 0..5 {
                let want = (want[c].clamp(0.0, 1.0) * 65535.0).round() as u16;
                assert!(
                    g[c].abs_diff(want) <= 2,
                    "mode {mode} opacity {opacity}: pixel {i} component {c}: {} vs {want}",
                    g[c]
                );
            }
        }
        free(out);
    }
    // Frozen alpha: a transparency-locked layer keeps the result's alpha.
    let locked_before = then(
        document(),
        r#"{"op":"layer_lock","layer":1,"locks":1}"#,
        &[],
    );
    let locked_after = selected(
        locked_before,
        &json!({"layer":1,"op":"gaussian_blur","params":{"radius":2}}),
        &[],
    )
    .unwrap();
    let out = fade(locked_after, locked_before, 1, 0.6, &[], false);
    for (g, y) in samples(out, 1).iter().zip(samples(locked_after, 1)) {
        assert_eq!(g[4], y[4], "a locked layer keeps the faded step's alpha");
    }
    for d in [out, locked_after, locked_before, after, before] {
        free(d);
    }
}

// --------------------------------------------- identity and refusals --

#[test]
fn identity_filters_answer_null_and_refusals_name_themselves() {
    let doc = document();
    for request in [
        json!({"layer":1,"op":"offset","params":{"horizontal":0,"vertical":0}}),
        json!({"layer":1,"op":"dust_and_scratches","params":{"radius":0.5}}),
        json!({"layer":1,"op":"unsharp_mask","params":{"amount":0}}),
    ] {
        let out = selected(doc, &request, &[]).unwrap_or_else(|e| panic!("{request}: {e}"));
        assert!(out.is_null(), "{request} is an identity");
    }
    let refusals = [
        (
            json!({"layer":1,"op":"lens_correction","params":{"distortion":0.2}}),
            "Lens Correction is unavailable in a CMYK document",
        ),
        (
            json!({"layer":1,"op":"develop_filter","params":{"exposure":0.5}}),
            "Develop Filter is unavailable in a CMYK document",
        ),
        (
            json!({"layer":1,"op":"presence","params":{"clarity":20}}),
            "Presence is unavailable in a CMYK document",
        ),
        (
            json!({"layer":1,"op":"add_grain","params":{"amount":30}}),
            "Add Grain is unavailable in a CMYK document",
        ),
        (
            json!({"layer":1,"op":"clouds","params":{"foreground_color":"#000000"}}),
            "The RGB \"foreground_color\" key (give Clouds' ink) is unavailable in a CMYK document",
        ),
    ];
    for (request, sentence) in &refusals {
        assert_eq!(
            selected(doc, request, &[]).unwrap_err(),
            *sentence,
            "{request}"
        );
    }
    // An adjustment record is not a filter's target.
    let toned = {
        let text = CString::new(
            r#"{"type":"adjust","evaluation":"precision_v1","op":"levels","params":{"gamma":2}}"#,
        )
        .unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment(
                doc,
                c"Levels".as_ptr(),
                text.as_ptr(),
                1.0,
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!out.is_null(), "{}", take_err_string(err));
        out
    };
    assert_eq!(
        selected(
            toned,
            &json!({"layer":2,"op":"blur","params":{"sigma":2}}),
            &[]
        )
        .unwrap_err(),
        "a CMYK filter requires a CMYK raster layer"
    );
    free(toned);
    free(doc);
}

#[test]
fn the_descriptor_reports_the_filter_shadows_and_fade_plans() {
    let doc = document();
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_descriptor(doc, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let descriptor: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    for key in [
        "cmyk_filter_gaussian_scratch_bytes",
        "cmyk_filter_surface_scratch_bytes",
        "cmyk_filter_dust_scratch_bytes",
        "cmyk_shadows_apply_scratch_bytes",
        "cmyk_fade_scratch_bytes",
    ] {
        assert!(
            descriptor[key].as_u64().is_some_and(|v| v > 0),
            "{key}: {}",
            descriptor[key]
        );
    }
    free(doc);
}
