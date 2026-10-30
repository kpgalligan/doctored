//! The 16-bit selected-edit path evaluates the legacy Gaussian (blur,
//! sharpen) and Surface Blur one 128-row band at a time. Every expectation is
//! an independent oracle over the WHOLE image — a direct 2-D tent sum, one
//! unbroken `image::imageops::blur` call — so a band seam that changed one
//! sample would show; the plan is checked at 24 MP against the one-band
//! budget, and the whole-plane retile term and the plan's `output_bytes` are
//! checked too. The legacy Gaussian runs the radius-independent filter from
//! σ4 up (`gaussian_radius_independent.rs`), so these library-blur cases
//! stay below that switch and σ12 is admitted at 24 MP.
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_selected_adjustment::{
    dr_precision_doc_selected_edit, dr_precision_doc_selected_edit_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::ptr;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// The core's per-allocation bookkeeping allowance (`precision_image`'s
/// crate-private `BOOKKEEPING_BYTES`, 1 MiB).
const BK: u64 = 1 << 20;

fn doc(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

fn text(op: &str, params: Value) -> CString {
    CString::new(json!({"layer": 0, "op": op, "params": params}).to_string()).unwrap()
}

fn take_error(error: *mut std::ffi::c_char) -> String {
    assert!(!error.is_null());
    let message = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    message
}

/// The selected edit through the C boundary; `Err` carries its message.
fn edit(
    d: &PrecisionDocument,
    op: &str,
    params: Value,
    coverage: &[u16],
    control: Option<&DrJobControl>,
) -> Result<Vec<[u16; 4]>, String> {
    let request = text(op, params);
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            d,
            request.as_ptr(),
            if coverage.is_empty() {
                ptr::null()
            } else {
                coverage.as_ptr()
            },
            coverage.len(),
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c as *const DrJobControl),
            &mut error,
        )
    };
    if !error.is_null() {
        assert!(out.is_null());
        return Err(take_error(error));
    }
    assert!(!out.is_null(), "{op} changed nothing");
    let out = unsafe { Box::from_raw(out) };
    Ok(out.layer_source(0).unwrap().to_rgba16().unwrap().to_vec())
}

/// The plan's `(scratch_bytes, output_bytes)` through the C boundary.
fn plan(d: &PrecisionDocument, op: &str, params: Value) -> Result<(u64, u64), String> {
    plan_request(d, &text(op, params))
}

fn plan_request(d: &PrecisionDocument, request: &CString) -> Result<(u64, u64), String> {
    let mut error = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_selected_edit_plan(d, request.as_ptr(), LIMIT, 0, &mut error) };
    if !error.is_null() {
        assert!(out.is_null());
        return Err(take_error(error));
    }
    let info: Value = serde_json::from_slice(unsafe { CStr::from_ptr(out) }.to_bytes()).unwrap();
    unsafe { dr_string_free(out) };
    Ok((
        info["scratch_bytes"].as_u64().unwrap(),
        info["output_bytes"].as_u64().unwrap(),
    ))
}

/// Byte-origin 16-bit samples (every code `v·257`): structure, speckle and
/// alpha 0, 1, 255 and partials, varying down the rows so bands differ.
fn byte_origin(w: u32, h: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let base = 40 + (x % 7) * 9 + (y % 37) * 5;
            let speck = if i % 11 == 0 { 120 } else { 0 };
            let level = |offset: u32| ((base + speck + offset) % 256) as u16 * 257;
            let alpha = match i % 5 {
                0 => 0,
                1 => 1,
                2 => 255,
                _ => (i * 53 + 17) % 256,
            } as u16
                * 257;
            [level(0), level(37), level(91), alpha]
        })
        .collect()
}

/// The direct tent filter at one pixel and channel, in 16-bit codes:
/// `spatial_filters.rs`'s law `w(d) = max(0, 1 − |d|/(2.5·T))` with `d` and
/// `T` scaled from levels to codes, alpha-weighted, over the clamped window.
#[allow(clippy::too_many_arguments)]
fn tent(input: &[[u16; 4]], w: u32, h: u32, x: i64, y: i64, c: usize, r: i64, t: f64) -> f64 {
    let at = |x: i64, y: i64| {
        let x = x.clamp(0, i64::from(w) - 1) as usize;
        let y = y.clamp(0, i64::from(h) - 1) as usize;
        input[y * w as usize + x].map(f64::from)
    };
    let centre = at(x, y)[c];
    let reach = 2.5 * t * 257.0;
    let (mut num, mut den) = (0.0, 0.0);
    for dy in -r..=r {
        for dx in -r..=r {
            let p = at(x + dx, y + dy);
            let weight = (1.0 - (p[c] - centre).abs() / reach).max(0.0);
            num += p[3] * weight * p[c];
            den += p[3] * weight;
        }
    }
    if den > 0.0 {
        num / den
    } else {
        centre
    }
}

/// `selected_adjustment::mix_codes`' alpha-preserving law, in codes: the
/// before code moved toward the filtered value by the coverage fraction.
fn mixed(before: u16, after: f64, weight: u16) -> f64 {
    let t = f64::from(weight) / 65535.0;
    let old = f64::from(before);
    (old + t * (after.clamp(0.0, 65535.0) - old)).round()
}

#[test]
fn banded_surface_blur_is_the_direct_tent_filter_across_band_edges() {
    // 300 rows are three bands (128, 128, 44); radius 40 reaches across both
    // band edges from rows on either side.
    let (w, h) = (97_u32, 300_u32);
    let input = byte_origin(w, h);
    let d = doc(w, h, input.clone());
    let partial: Vec<u16> = (0..w * h)
        .map(|i| match i % 4 {
            0 => 0,
            1 => 65535,
            _ => ((i * 7919) % 65534 + 1) as u16,
        })
        .collect();
    for radius in [1_i64, 7, 40] {
        for threshold in [2.0, 15.0, 255.0] {
            let params = json!({"radius": radius, "threshold": threshold});
            let oracle: Vec<[f64; 3]> = (0..i64::from(w * h))
                .map(|i| {
                    let (x, y) = (i % i64::from(w), i / i64::from(w));
                    [0, 1, 2].map(|c| tent(&input, w, h, x, y, c, radius, threshold))
                })
                .collect();
            for coverage in [&[][..], &partial[..]] {
                let out = edit(&d, "surface_blur", params.clone(), coverage, None).unwrap();
                for (i, (got, before)) in out.iter().zip(&input).enumerate() {
                    let tag = format!(
                        "r{radius} T{threshold} ({}, {}) selection {}",
                        i % w as usize,
                        i / w as usize,
                        !coverage.is_empty()
                    );
                    let weight = coverage.get(i).copied().unwrap_or(65535);
                    assert_eq!(got[3], before[3], "{tag}: alpha is exact");
                    if before[3] == 0 || weight == 0 {
                        assert_eq!(got, before, "{tag}: untouched");
                        continue;
                    }
                    for c in 0..3 {
                        let expected = oracle[i][c];
                        let value = f64::from(got[c]);
                        if weight < 65535 {
                            let law = mixed(before[c], expected, weight);
                            assert!((value - law).abs() <= 1.0, "{tag} c{c}: {value} vs {law}");
                            continue;
                        }
                        // Exact, except that a value within 0.01 of a half code
                        // is where the f32 plane and this binary64 sum may
                        // round apart.
                        let tie = ((expected - expected.floor()) - 0.5).abs() < 0.01;
                        if tie {
                            assert!((value - expected.round()).abs() <= 1.0, "{tag} c{c}");
                        } else {
                            assert_eq!(value, expected.round(), "{tag} c{c}");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn banded_legacy_gaussian_matches_one_unbroken_library_blur() {
    let (w, h) = (131_u32, 300_u32);
    let input: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            [
                (i * 109 % 65536) as u16,
                (i * 97 % 65536) as u16,
                (i * 47 % 65536) as u16,
                ((i * 131) % 65535 + 1) as u16,
            ]
        })
        .collect();
    let d = doc(w, h, input.clone());
    let float = image::Rgba32FImage::from_fn(w, h, |x, y| {
        let p = input[(y * w + x) as usize].map(|v| f32::from(v) / 65535.);
        image::Rgba([p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]])
    });
    for (op, params, sigma) in [
        ("blur", json!({"sigma": 1.5}), 1.5_f32),
        // The largest half-integer below the fast filter's σ4 switch: the
        // widest library-blur halo the bands still run.
        ("blur", json!({"sigma": 3.5}), 3.5),
        ("sharpen", json!({"amount": 1.5}), 1.5),
    ] {
        let low = image::imageops::blur(&float, sigma);
        let out = edit(&d, op, params, &[], None).unwrap();
        for (i, p) in low.pixels().enumerate() {
            let mut expected = [0; 4];
            for c in 0..4 {
                let value = if c == 3 { p[c] } else { p[c] / p[3] };
                let q = (value * 65535.).clamp(0., 65535.).round() as u16;
                expected[c] = if op == "blur" {
                    q
                } else {
                    (2 * i32::from(input[i][c]) - i32::from(q)).clamp(0, 65535) as u16
                };
            }
            if expected[3] == 0 {
                expected = [0; 4];
            }
            for c in 0..4 {
                assert!(
                    out[i][c].abs_diff(expected[c]) <= 1,
                    "{op} σ{sigma} sample {i} channel {c}: {} vs {}",
                    out[i][c],
                    expected[c]
                );
            }
        }
    }
}

/// The fast arm keeps the banded contract: the 16-bit layer edit
/// runs it one band at a time, the scalar red-plane edit runs it once over
/// the whole plane (`legacy_blur`), and on an opaque gray layer the two agree
/// code for code at σ12, across the 300-row image's three bands.
#[test]
fn banded_fast_gaussian_equals_its_whole_plane() {
    let (w, h) = (97_u32, 300_u32);
    let mut state = 0x5EED_u64;
    let input: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let edge = if (i / w) % 64 < 32 { 9000 } else { 52000 };
            let g = (edge + (state >> 52) as u32) as u16;
            [g, g, g, 65535]
        })
        .collect();
    let d = doc(w, h, input);
    let banded = edit(&d, "blur", json!({"sigma": 12}), &[], None).unwrap();
    let request = CString::new(
        json!({"op": "scalar_edit", "target": "red", "index": 0, "selection": false,
               "operation": {"kind": "filter",
                             "edit": {"layer": 0, "op": "blur", "params": {"sigma": 12}}}})
        .to_string(),
    )
    .unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            &d,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "{}", take_error(error));
    assert!(!out.is_null());
    let whole = unsafe { Box::from_raw(out) }
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .unwrap()
        .to_vec();
    for (i, (b, p)) in banded.iter().zip(&whole).enumerate() {
        assert_eq!(b[0], p[0], "sample {i} (row {})", i as u32 / w);
    }
}

#[test]
fn banded_plans_fit_one_band_at_24_mp() {
    let (w, h) = (6000_u32, 4000_u32);
    let n = u64::from(w) * u64::from(h);
    let d = doc(w, h, vec![[20001, 30002, 40003, 65535]; n as usize]);
    // The output codes and their tiled copy, the canvas coverage lease,
    // bookkeeping and at most 64 MiB for one band of the filter's plane and
    // its working buffers — never the whole n×16 plane.
    let bound = 6 * BK + n * 8 + n * 8 + n * 2 + 64 * 1024 * 1024;
    for (op, params) in [
        ("surface_blur", json!({"radius": 10, "threshold": 15})),
        ("blur", json!({"sigma": 2})),
    ] {
        let (scratch, _) = plan(&d, op, params).unwrap();
        assert!(scratch <= bound, "{op}: {scratch} > {bound}");
    }
    // σ12 runs the radius-independent filter and is admitted
    // within the same one-band bound.
    let (scratch, _) = plan(&d, "blur", json!({"sigma": 12})).unwrap();
    assert!(scratch <= bound, "blur σ12: {scratch} > {bound}");
    // A plan carries no work refusal: T255 at r10, over the
    // uncontrolled ceiling, plans within the same one-band bound.
    let (scratch, _) = plan(&d, "surface_blur", json!({"radius": 10, "threshold": 255})).unwrap();
    assert!(scratch <= bound, "surface_blur T255: {scratch} > {bound}");
}

#[test]
fn whole_plane_plans_charge_the_retile_copy_and_report_the_output() {
    // Every branch rebuilds the layer through `rebuilt_rgba16`,
    // whose n×8 tiled copy of the output codes is live beside them. A tone
    // adjustment charges it on top of its held state, as the banded ones do;
    // and every plan reports the n×8 codes the result keeps.
    let (w, h) = (6000_u32, 4000_u32);
    let n = u64::from(w) * u64::from(h);
    let d = doc(w, h, vec![[20001, 30002, 40003, 65535]; n as usize]);
    let description =
        json!({"type": "adjust", "evaluation": "precision_v1", "op": "invert", "params": {}});
    let invert = CString::new(
        json!({"layer": 0, "op": "adjustment", "description": description.to_string()}).to_string(),
    )
    .unwrap();
    let (scratch, output) = plan_request(&d, &invert).unwrap();
    // Bookkeeping, the output codes, their tiled copy and the coverage lease.
    let floor = 6 * BK + 16 * n + 2 * n;
    assert!(scratch >= floor, "invert: {scratch} < {floor}");
    assert_eq!(output, 8 * n, "invert");
    for (op, params) in [
        ("box_blur", json!({"radius": 10})),
        ("blur", json!({"sigma": 2})),
    ] {
        let (_, output) = plan(&d, op, params).unwrap();
        assert_eq!(output, 8 * n, "{op}");
    }
    // A whole-plane kernel frees its transient scratch before the copy is
    // made, so Box Blur's plan stays the kernel's own figure. From
    // `selected_adjustment.rs`'s `selected_edit_plan`: 6 BK + the n×8 codes +
    // max(kernel, held n×16 + output n×8) + the n×2 coverage, where the
    // kernel is 24n + the `BoxRows` ring ((min(2r+2, h) + 1)·w·8
    // at r = 10) + 2 BK, above the held 16n + 8n: 34n + ring + 8 BK.
    let ring = (22_u64.min(u64::from(h)) + 1) * u64::from(w) * 8;
    let (scratch, _) = plan(&d, "box_blur", json!({"radius": 10})).unwrap();
    assert_eq!(scratch, 34 * n + ring + 8 * BK, "box_blur");
    // A kernel with no plane-sized intermediate holds its plane, the codes and
    // the copy at once.
    for (op, params) in [
        ("edge_detect", json!({})),
        ("pixelate", json!({"block": 8})),
    ] {
        let (scratch, _) = plan(&d, op, params).unwrap();
        let floor = 6 * BK + 8 * n + 16 * n + 8 * n + 2 * n;
        assert!(scratch >= floor, "{op}: {scratch} < {floor}");
    }
    let (scratch, _) = plan(&d, "add_noise", json!({"amount": 0.1})).unwrap();
    let floor = 6 * BK + 8 * n + 8 * n + 2 * n;
    assert!(scratch >= floor, "add_noise: {scratch} < {floor}");
}

#[test]
fn precancelled_banded_edits_answer_only_the_cancel() {
    let (w, h) = (97_u32, 300_u32);
    let d = doc(w, h, byte_origin(w, h));
    for (op, params) in [
        ("surface_blur", json!({"radius": 7, "threshold": 15})),
        ("blur", json!({"sigma": 2})),
        ("sharpen", json!({"amount": 1.5})),
    ] {
        let control = DrJobControl::default();
        control.cancel();
        let error = edit(&d, op, params, &[], Some(&control)).unwrap_err();
        assert_eq!(error, "operation cancelled", "{op}");
        assert_eq!(control.scratch_bytes(), 0, "{op}");
    }
}
