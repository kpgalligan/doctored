//! Reduce Noise's guided filter keeps its binary64 window
//! statistics as streamed row rings (`sampling_box::BoxRows`) instead of whole
//! n×8 planes, bit-identical to before, and a 6000×4000 layer's plans at 8
//! and 16 bits charge the rings and cover the measured peaks; Box Blur's
//! whole-plane box sum is now that ring's wrapper.
//! Every expectation is black-box through the public API and the C boundary.
use crate::common;
use crate::gaussian_reference;
use crate::live_filter_support;
use common::probe::{cancelled_parked_at, reports};
use common::sha256;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba};
use doctored_core::ffi_filters::dr_image_filter_op;
use doctored_core::ffi_selected_adjustment::{
    dr_precision_doc_selected_edit, dr_precision_doc_selected_edit_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use live_filter_support::*;
use serde_json::{json, Value};
use std::ffi::{c_char, CString};
use std::ptr;

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Splitmix noise colour, opaque, with a centred 50%-alpha disc of radius 40
/// and a 60×60 fully transparent block at (5,5) holding saturated hidden RGB:
/// the transparent block drives the window's summed alpha to zero (the
/// `s0 ≤ 0` / `c0 ≤ 0` branches).
fn fixture(w: u32, h: u32) -> Vec<[u16; 4]> {
    let mut state = 0x5EED_u64;
    (0..w * h)
        .map(|i| {
            let v = splitmix(&mut state);
            let (x, y) = (i % w, i / w);
            if (5..65).contains(&x) && (5..65).contains(&y) {
                return [65535, 0, 65535, 0];
            }
            let (dx, dy) = (
                f64::from(x) + 0.5 - f64::from(w) / 2.0,
                f64::from(y) + 0.5 - f64::from(h) / 2.0,
            );
            let alpha = if dx * dx + dy * dy <= 40.0 * 40.0 {
                32768
            } else {
                65535
            };
            [v as u16, (v >> 16) as u16, (v >> 32) as u16, alpha]
        })
        .collect()
}

/// Opaque splitmix noise, the calibration and admission plate.
fn opaque_noise(w: u32, h: u32) -> Vec<[u16; 4]> {
    let mut state = 0x5EED_u64;
    (0..u64::from(w) * u64::from(h))
        .map(|_| {
            let v = splitmix(&mut state);
            [v as u16, (v >> 16) as u16, (v >> 32) as u16, 65535]
        })
        .collect()
}

fn to_bytes(pixels: &[[u16; 4]]) -> Vec<u8> {
    pixels
        .iter()
        .flat_map(|p| p.map(|v| ((u32::from(v) + 128) / 257) as u8))
        .collect()
}

fn le16(pixels: &[[u16; 4]]) -> Vec<u8> {
    pixels
        .iter()
        .flat_map(|p| p.iter().flat_map(|v| v.to_le_bytes()))
        .collect()
}

/// Byte `dr_image_filter_op` over byte samples: the output bytes, or the
/// error (a NULL with no error is a bypass, answered as `Ok(None)`).
fn byte_op(
    w: u32,
    h: u32,
    bytes: &[u8],
    op: &str,
    params: &Value,
    control: Option<&DrJobControl>,
) -> Result<Option<Vec<u8>>, String> {
    let img = unsafe { dr_image_from_rgba(bytes.as_ptr(), w, h) };
    assert!(!img.is_null());
    let op = CString::new(op).unwrap();
    let params = CString::new(params.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_image_filter_op(
            img,
            op.as_ptr(),
            params.as_ptr(),
            control.map_or(ptr::null(), |c| c as *const DrJobControl),
            &mut err,
        )
    };
    let error = unsafe { text(err) };
    let result = if out.is_null() {
        if error.is_empty() {
            Ok(None)
        } else {
            Err(error)
        }
    } else {
        assert!(error.is_empty(), "{error}");
        let len = bytes.len();
        let v = unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(out), len) }.to_vec();
        unsafe { dr_image_free(out) };
        Ok(Some(v))
    };
    unsafe { dr_image_free(img) };
    result
}

fn doc16(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

fn request(op: &str, params: &Value) -> CString {
    CString::new(json!({"layer": 0, "op": op, "params": params}).to_string()).unwrap()
}

/// The 16-bit selected edit at full coverage through the C boundary.
fn selected(
    d: &PrecisionDocument,
    op: &str,
    params: &Value,
    control: Option<&DrJobControl>,
) -> Result<Option<Vec<[u16; 4]>>, String> {
    let request = request(op, params);
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            d,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c as *const DrJobControl),
            &mut err,
        )
    };
    let error = unsafe { text(err) };
    if out.is_null() {
        return if error.is_empty() {
            Ok(None)
        } else {
            Err(error)
        };
    }
    assert!(error.is_empty(), "{error}");
    let out = unsafe { Box::from_raw(out) };
    Ok(Some(out.layer_source(0).unwrap().to_rgba16().unwrap()))
}

/// The 16-bit selected plan: `(retained_bytes, scratch_bytes)`.
fn selected_plan(d: &PrecisionDocument, op: &str, params: &Value) -> Result<(u64, u64), String> {
    let request = request(op, params);
    let mut err: *mut c_char = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_selected_edit_plan(d, request.as_ptr(), LIMIT, 0, &mut err) };
    let info = unsafe { json_result(out, err) }?;
    Ok((
        info["retained_bytes"].as_u64().unwrap(),
        info["scratch_bytes"].as_u64().unwrap(),
    ))
}

const FIXTURES: [(&str, u32, u32); 5] = [
    ("F_A", 131, 300),
    ("F_B", 300, 131),
    ("F_C", 7, 5),
    ("F_D", 1, 97),
    ("F_E", 97, 1),
];

/// One parameter set: its name and its filter-op params.
type ParamSet = (&'static str, Value);

/// P1–P5.
fn params() -> Vec<ParamSet> {
    vec![
        ("P1", json!({})),
        (
            "P2",
            json!({"radius": 4, "luminance": 0.05, "detail": 0, "color": 1}),
        ),
        ("P3", json!({"radius": 2, "color": 0})),
        ("P4", json!({"radius": 2, "luminance": 0})),
        ("P5", json!({"radius": 16})),
    ]
}

/// One path's digest: SHA-256 over its cases' outputs concatenated in order,
/// with the per-case digests for a mismatch message.
fn path_digest(cases: Vec<(String, Vec<u8>)>) -> (String, String) {
    let mut all = Vec::new();
    let mut lines = String::new();
    for (name, bytes) in &cases {
        lines.push_str(&format!("  {name}: {}\n", sha256::sha256_hex(bytes)));
        all.extend_from_slice(bytes);
    }
    (sha256::sha256_hex(&all), lines)
}

fn some_bytes(answer: Result<Option<Vec<u8>>, String>) -> Vec<u8> {
    answer.unwrap().unwrap_or_else(|| b"none".to_vec())
}

/// (a) Every Reduce Noise output and every Box Blur output is unchanged:
/// SHA-256 pins captured on the tree before the row rings (HEAD a4a5859,
/// whole-plane `box_sum`), one per path.
#[test]
fn reduce_noise_and_box_blur_are_bit_identical_to_the_whole_plane_sums() {
    let mut paths: Vec<(&str, (String, String))> = Vec::new();

    let mut cases = Vec::new();
    for (f, w, h) in FIXTURES {
        let bytes = to_bytes(&fixture(w, h));
        for (p, value) in params() {
            let out = byte_op(w, h, &bytes, "reduce_noise", &value, None);
            cases.push((format!("{f} {p}"), some_bytes(out)));
        }
    }
    paths.push(("byte reduce_noise", path_digest(cases)));

    let mut cases = Vec::new();
    for (f, w, h) in [FIXTURES[0], FIXTURES[2]] {
        let d = doc16(w, h, fixture(w, h));
        for (p, value) in params() {
            if !["P1", "P2", "P5"].contains(&p) {
                continue;
            }
            let out = selected(&d, "reduce_noise", &value, None).unwrap();
            let bytes = out.map_or_else(|| b"none".to_vec(), |v| le16(&v));
            cases.push((format!("{f} {p}"), bytes));
        }
    }
    paths.push(("selected16 reduce_noise", path_digest(cases)));

    let mut cases = Vec::new();
    for radius in [3, 200] {
        for (f, w, h) in [FIXTURES[0], FIXTURES[3]] {
            let bytes = to_bytes(&fixture(w, h));
            let out = byte_op(w, h, &bytes, "box_blur", &json!({ "radius": radius }), None);
            cases.push((format!("{f} r{radius}"), some_bytes(out)));
        }
    }
    paths.push(("byte box_blur", path_digest(cases)));

    // Captured at HEAD a4a5859, before the row rings.
    let pins: [&str; 3] = [
        "d5b4b467bc6af9d0822c15aa3e68dbe9af88e038d285d0c7ece7a0cba802faf4",
        "ad50c631d53e7c98f888442ecc4054c7f7ace45a6e97acc7ce19a27cf5e922ef",
        "8538b8cd2b2d7aef983831f80a46553b70f2f578c09e9dd06a7b22ac313f4082",
    ];
    let mut failures = String::new();
    for ((name, (digest, lines)), pin) in paths.iter().zip(pins) {
        if digest != pin {
            failures.push_str(&format!("{name}: {digest} (pin {pin})\n{lines}"));
        }
    }
    assert!(failures.is_empty(), "{failures}");
}

/// The 16-bit selected `reduce_noise` `{radius:64}` plan scratch at
/// 2000×1500, captured at HEAD a4a5859 (whole-plane statistics).
const HEAD_R64_SCRATCH: u64 = 280_485_760;

/// ⌈max E⌉ of S16-N (16-bit selected `reduce_noise` defaults on 6000×4000
/// noise; the record in `designs/phase-9f-selected-tone-status.md`).
const S16_FLOOR: u64 = 753_877_544;
/// (b) A 6000×4000 16-bit layer's Reduce Noise plans at the sheet's
/// defaults, at the slider's top (16) and at the core's top (64); the
/// defaults' plan covers its measured peak.
#[test]
fn a_24_mp_16_bit_reduce_noise_plans_cover_the_peak() {
    let (w, h) = (6000_u32, 4000_u32);
    let d = doc16(w, h, opaque_noise(w, h));
    for params in [json!({}), json!({"radius": 16}), json!({"radius": 64})] {
        selected_plan(&d, "reduce_noise", &params).unwrap_or_else(|e| panic!("{params}: {e}"));
    }
    let (_, scratch) = selected_plan(&d, "reduce_noise", &json!({})).unwrap();
    assert!(scratch >= S16_FLOOR, "{scratch} < {S16_FLOOR}");
}

/// The largest quarter-pixel Dust & Scratches radius a 6000×4000 plan admits
/// under the calibrated rank charge (`sampling_rank::RANK_NEIGHBOUR_WORK` 16,
/// `RANK_PIXEL_WORK` 364): r3.5, as under the former 24 / 24.
const DUST_CEILING: f64 = 3.5;

/// (d) Dust & Scratches on a 6000×4000 16-bit layer: the sheet's default r2
/// is admitted, and so is the recorded 24 MP ceiling, while the next
/// quarter-pixel step refuses by work when uncontrolled. The plan carries
/// no work refusal (a controlled run has no ceiling), so that step
/// plans and only the uncontrolled run refuses.
#[test]
fn the_24_mp_dust_ceiling_is_the_calibrated_one() {
    let (w, h) = (6000_u32, 4000_u32);
    let d = doc16(w, h, vec![[20001, 30002, 40003, 65535]; (w * h) as usize]);
    for radius in [2.0, DUST_CEILING] {
        selected_plan(
            &d,
            "dust_and_scratches",
            &json!({"radius": radius, "threshold": 0}),
        )
        .unwrap_or_else(|e| panic!("r{radius}: {e}"));
    }
    let over = json!({"radius": DUST_CEILING + 0.25, "threshold": 0});
    selected_plan(&d, "dust_and_scratches", &over)
        .unwrap_or_else(|e| panic!("r{}: {e}", DUST_CEILING + 0.25));
    let error = selected(&d, "dust_and_scratches", &over, None).unwrap_err();
    assert_eq!(
        error, "rank source filter exceeds the bounded pixel-work limit",
        "{error}"
    );
}

/// (e) Cancellation: a precancelled byte op and 16-bit selected edit answer
/// only the cancel; a cancel parked in the byte op's chroma half answers the
/// cancel; an uncancelled run's reports are monotone and end at 1.0.
#[test]
fn a_cancelled_reduce_noise_answers_only_the_cancel() {
    let (_, w, h) = FIXTURES[0];
    let pixels = fixture(w, h);
    let bytes = to_bytes(&pixels);
    let control = DrJobControl::default();
    control.cancel();
    let error = byte_op(w, h, &bytes, "reduce_noise", &json!({}), Some(&control)).unwrap_err();
    assert_eq!(error, "operation cancelled");
    let d = doc16(w, h, pixels.clone());
    let control = DrJobControl::default();
    control.cancel();
    let error = selected(&d, "reduce_noise", &json!({}), Some(&control)).unwrap_err();
    assert_eq!(error, "operation cancelled");
    assert_eq!(control.scratch_bytes(), 0);
    assert_eq!(d.layer_source(0).unwrap().to_rgba16().unwrap(), pixels);

    let bytes = &bytes;
    let render = move |token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match byte_op(w, h, bytes, "reduce_noise", &json!({}), Some(control)) {
            Ok(out) => (out.is_some(), String::new()),
            Err(error) => (false, error),
        }
    };
    let log = reports(render);
    let progress: Vec<f64> = log.iter().map(|r| r.progress).collect();
    assert!(
        progress.windows(2).all(|p| p[0] <= p[1]),
        "progress is not monotone: {progress:?}"
    );
    assert_eq!(progress.last(), Some(&1.0));
    // Both halves run, so the chroma half reports in (0.5, 1].
    let chroma: Vec<u64> = log
        .iter()
        .filter(|r| r.stage == 6 && r.progress > 0.5)
        .map(|r| r.checkpoints)
        .collect();
    assert!(chroma.len() > 2, "the chroma half reports per row");
    let count = chroma[chroma.len() / 2];
    let parked = cancelled_parked_at(
        count,
        &|progress, stage| stage == 6 && progress > 0.5,
        render,
    );
    assert_eq!(parked.count, count);
}

/// (f) The plan's statistics term is the streamed half's rings, not whole
/// planes: at 2000×1500 `{radius:64}` (luminance radius 192, chroma 256) the
/// chroma half exceeds the whole-plane arm's floor, so the plan shows it.
/// HEAD charged 8 n×8 planes (64n); the chroma half now holds 11 rings of
/// min(2·256+2, 1500) + 1 rows, its 257-row summed-alpha delay ring and 6
/// regression rows.
#[test]
fn the_plan_charges_the_streamed_rings() {
    let (w, h) = (2000_u32, 1500_u32);
    let n = u64::from(w) * u64::from(h);
    let d = doc16(w, h, opaque_noise(w, h));
    let (_, scratch) = selected_plan(&d, "reduce_noise", &json!({"radius": 64})).unwrap();
    let chroma_half = (11 * (2 * 256 + 2 + 1) + 257 + 6) * 2000 * 8;
    assert_eq!(chroma_half, 94_848_000);
    assert_eq!(scratch, HEAD_R64_SCRATCH - 64 * n + chroma_half);
}

// ------------------------------------------------------------ calibration --

/// The explicit margin every work constant carries over its measured figure
/// (the Surface Blur and fast-Gaussian probes' factor).
const SAFETY_FACTOR: f64 = 1.15;
/// Batch 149's recorded R3.9 (`gaussian_reference`'s comment): byte High Pass
/// σ3.9 on 3000×2000 noise, two runs, units/s at n·(8·25+12).
const R39_RECORDED: [f64; 2] = [3.4035e9, 3.3502e9];
/// `sampling_gaussian::MAX_DESTRUCTIVE_WORK`, the one-shot ceiling.
const DESTRUCTIVE_WORK: u64 = 24_000_000_000;

fn min_of_3(mut run: impl FnMut()) -> f64 {
    (0..3)
        .map(|_| {
            let start = std::time::Instant::now();
            run();
            start.elapsed().as_secs_f64()
        })
        .fold(f64::INFINITY, f64::min)
}

/// Least squares `U = a·k + b`; refit through the origin when `b < 0`.
fn fit(points: &[(f64, f64)]) -> (f64, f64) {
    let count = points.len() as f64;
    let (sk, su) = points
        .iter()
        .fold((0.0, 0.0), |(k, u), p| (k + p.0, u + p.1));
    let skk: f64 = points.iter().map(|p| p.0 * p.0).sum();
    let sku: f64 = points.iter().map(|p| p.0 * p.1).sum();
    let a = (count * sku - sk * su) / (count * skk - sk * sk);
    let b = (su - a * sk) / count;
    if b < 0.0 {
        (sku / skk, 0.0)
    } else {
        (a, b)
    }
}

/// The largest quarter-pixel radius ≤ 16 a 6000×4000 dust plan admits at
/// `(neighbour, pixel)` units.
fn dust_ceiling(neighbour: u64, pixel: u64) -> f64 {
    let n = 6000 * 4000_u64;
    (4..=64)
        .map(|q| f64::from(q) / 4.0)
        .take_while(|&r| {
            let k = doctored_core::sampling_rank::disc_offsets(r).len() as u64;
            n * (neighbour * k + pixel) <= DESTRUCTIVE_WORK
        })
        .last()
        .unwrap_or(0.0)
}

/// The calibration record. Times batch 149's reference (byte High
/// Pass σ3.9 on 3000×2000 noise, charged n·(8·25+12)) and states it in the
/// radius-16 unit every other work constant uses
/// (`gaussian_reference::R39_PER_R16`), then:
///
/// * Dust & Scratches at threshold 0, r ∈ {1,2,3,4,6,8} (r8 on the plate's
///   first 1500 rows, which the pre-calibration charge admits), byte
///   `dr_image_filter_op` and the 16-bit selected edit: each path's implied
///   units per pixel U = t·rate/n are fitted to U = a·k + b (k the disc's
///   size; b < 0 refits through the origin), a and b are the larger of the
///   two paths', f the largest U/(a·k+b) over every point, and the constants
///   are ⌈1.15·f·a⌉ / ⌈1.15·f·b⌉;
/// * Reduce Noise luminance-only (color 0) and chroma-only (luminance 0) at
///   r1 and r16, both paths: 1.15·t·rate against the run's charge
///   n·const + seeding, and the smallest const that covers it.
///
/// Every timing is the minimum of three. Prints the 24 MP dust ceiling
/// before and after.
#[test]
#[ignore = "calibration probe: make t F=banded_reduce_noise A='--ignored --nocapture --test-threads=1'"]
fn rank_and_denoise_calibration_probe() {
    let (w, h) = (3000_u32, 2000_u32);
    let n = u64::from(w) * u64::from(h);
    let mut state = 0x5EED_u64;
    let bytes: Vec<u8> = (0..n)
        .flat_map(|_| {
            let v = splitmix(&mut state);
            [v as u8, (v >> 8) as u8, (v >> 16) as u8, 255]
        })
        .collect();
    let t_reference = min_of_3(|| {
        byte_op(w, h, &bytes, "high_pass", &json!({"radius": 3.9}), None)
            .unwrap()
            .unwrap();
    });
    let r39 = (n * (8 * 25 + 12)) as f64 / t_reference;
    let recorded = (R39_RECORDED[0] + R39_RECORDED[1]) / 2.0;
    let drift = (r39 / recorded - 1.0).abs();
    println!(
        "reference: high_pass σ3.9 on {w}x{h}: {t_reference:.3} s, R3.9 {r39:.4e} units/s \
         ({:.1}% from 149's {recorded:.4e}): {}",
        drift * 100.0,
        if drift > 0.10 { "STOP" } else { "PASS" }
    );
    let rate = r39 / gaussian_reference::R39_PER_R16;
    println!("r16-unit rate {rate:.4e} units/s");
    let codes: Vec<[u16; 4]> = bytes
        .chunks_exact(4)
        .map(|p| [0, 1, 2, 3].map(|c| u16::from(p[c]) * 257))
        .collect();
    // r8 is timed on the plate's first 1500 rows: at 3000×2000 the current
    // charge refuses it, and U is per pixel.
    let short = 1500_u32;
    let short_bytes = &bytes[..(w * short * 4) as usize];
    let short_doc = doc16(w, short, codes[..(w * short) as usize].to_vec());
    let d = doc16(w, h, codes);
    let time_on = |rows: u32, bytes: &[u8], d: &PrecisionDocument, op: &str, params: &Value| {
        let t8 = min_of_3(|| {
            byte_op(w, rows, bytes, op, params, None).unwrap().unwrap();
        });
        let t16 = min_of_3(|| {
            selected(d, op, params, None).unwrap().unwrap();
        });
        (t8, t16)
    };
    let time = |op: &str, params: &Value| time_on(h, &bytes, &d, op, params);

    let mut paths: [Vec<(f64, f64)>; 2] = [Vec::new(), Vec::new()];
    for r in [1.0, 2.0, 3.0, 4.0, 6.0, 8.0] {
        let k = doctored_core::sampling_rank::disc_offsets(r).len() as f64;
        let params = json!({"radius": r, "threshold": 0});
        let (rows, (t8, t16)) = if r == 8.0 {
            (
                short,
                time_on(
                    short,
                    short_bytes,
                    &short_doc,
                    "dust_and_scratches",
                    &params,
                ),
            )
        } else {
            (h, time("dust_and_scratches", &params))
        };
        let pixels = f64::from(w * rows);
        let (u8_, u16_) = (t8 * rate / pixels, t16 * rate / pixels);
        paths[0].push((k, u8_));
        paths[1].push((k, u16_));
        println!(
            "dust r{r} (k {k}) on {w}x{rows}: byte {t8:.3} s → U {u8_:.1}; 16-bit edit {t16:.3} s → U {u16_:.1}"
        );
    }
    let fits = [fit(&paths[0]), fit(&paths[1])];
    println!(
        "fit byte U = {:.3}·k + {:.3}; 16-bit U = {:.3}·k + {:.3}",
        fits[0].0, fits[0].1, fits[1].0, fits[1].1
    );
    let (a, b) = (fits[0].0.max(fits[1].0), fits[0].1.max(fits[1].1));
    let f = paths
        .iter()
        .flatten()
        .map(|&(k, u)| u / (a * k + b))
        .fold(0.0_f64, f64::max);
    let neighbour = (SAFETY_FACTOR * f * a).ceil() as u64;
    let pixel = (SAFETY_FACTOR * f * b).ceil() as u64;
    println!(
        "a {a:.3}, b {b:.3}, f {f:.4}: RANK_NEIGHBOUR_WORK = {neighbour}, RANK_PIXEL_WORK = {pixel}"
    );
    let r2 = doctored_core::sampling_rank::disc_offsets(2.0).len() as u64;
    println!(
        "24 MP dust: r2 charge {} ({}); ceiling r{} at 24/24 (before calibration) → r{} at {neighbour}/{pixel}",
        6000 * 4000 * (neighbour * r2 + pixel),
        if 6000 * 4000 * (neighbour * r2 + pixel) <= DESTRUCTIVE_WORK {
            "admitted"
        } else {
            "REFUSED: STOP"
        },
        dust_ceiling(24, 24),
        dust_ceiling(neighbour, pixel)
    );

    for (half, constant, passes, base) in [
        // `sampling_denoise`'s DENOISE_LUMA_WORK / DENOISE_CHROMA_WORK (44 / 92
        // before this calibration) and each half's box passes.
        ("luminance", 139_u64, 5_u64, json!({"color": 0})),
        ("chroma", 326, 11, json!({"luminance": 0})),
    ] {
        let mut needed = constant;
        for radius in [1.0, 16.0] {
            let mut params = base.clone();
            params["radius"] = json!(radius);
            let r = if half == "luminance" {
                ((3.0_f64 * radius).ceil() as u64).max(1)
            } else {
                (((3.0_f64 * radius).ceil() as u64).max(1) * 4).min(256)
            };
            let seeding = passes * (2 * r + 1) * u64::from(w + h);
            let charge = n * constant + seeding;
            let (t8, t16) = time("reduce_noise", &params);
            for (path, t) in [("byte", t8), ("16-bit edit", t16)] {
                let spent = SAFETY_FACTOR * t * rate;
                let covering = ((spent - seeding as f64) / n as f64).ceil().max(0.0) as u64;
                needed = needed.max(covering);
                println!(
                    "reduce_noise {half} r{radius} {path}: {t:.3} s → U {:.1}; 1.15·U·n {spent:.4e} vs charge {charge} ({})",
                    t * rate / n as f64,
                    if spent > charge as f64 { "EXCEEDS" } else { "covered" }
                );
            }
        }
        println!("reduce_noise {half}: const {constant} → {needed}");
    }
}
