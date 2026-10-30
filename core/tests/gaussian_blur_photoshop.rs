//! Filter ▸ Blur ▸ Gaussian Blur runs one kernel at every
//! depth — 8-bit `dr_image_filter_op` `blur`, the 16-bit `blur` selected
//! edit and the 32-bit float route — a K-pass extended box at σ = s·r by the
//! law fitted on one Photoshop 26.10.0 take and confirmed on a second
//! (`scripts/photoshop-probe/blur_wand_take.py`). Every expectation is
//! black-box through the C boundary against `extended_box_reference`, an
//! independent binary64 oracle written from Gwosdek et al. 2011, run on
//! premultiplied samples; that helper restates the fitted law (from radius 7
//! up, 4 boxes at σ = 0.99·r). Below 7 Gaussian Blur keeps its old kernel
//! (`byte_gaussian_blur_op.rs`, `gaussian_radius_independent.rs`). The last test
//! holds the 8-bit output to Photoshop's own confirming-take renders
//! (`fixtures/photoshop_blur_wand/`) at the bound declared before that take.
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba};
use doctored_core::ffi_filters::dr_image_filter_op;
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::json;
use std::ffi::CString;
use std::ptr;

use crate::extended_box_reference::{fitted_blur_rgba, FITTED_MIN_RADIUS};
use crate::float_fixture::float_doc;

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Straight samples in [0, 1]: a quadrant step, splitmix noise (both
/// opaque) and a 50%-alpha disc whose transparent outside hides saturated RGB.
fn fixture(kind: &str, w: u32, h: u32) -> Vec<[f64; 4]> {
    let mut state = 0x443_u64;
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            match kind {
                "quadrant" => {
                    let (right, low) = (x >= w / 2, y >= h / 2);
                    [
                        f64::from(u8::from(right)),
                        f64::from(u8::from(low)),
                        f64::from(u8::from(right != low)),
                        1.0,
                    ]
                }
                "noise" => {
                    let v = splitmix(&mut state);
                    [
                        (v & 0xFFFF) as f64 / 65535.0,
                        ((v >> 16) & 0xFFFF) as f64 / 65535.0,
                        ((v >> 32) & 0xFFFF) as f64 / 65535.0,
                        1.0,
                    ]
                }
                _ => {
                    let dx = f64::from(x) + 0.5 - f64::from(w) / 2.0;
                    let dy = f64::from(y) + 0.5 - f64::from(h) / 2.0;
                    if dx * dx + dy * dy <= 40.0 * 40.0 {
                        [
                            f64::from(x) / f64::from(w),
                            f64::from(y) / f64::from(h),
                            0.3,
                            0.5,
                        ]
                    } else {
                        [1.0, 0.0, 1.0, 0.0]
                    }
                }
            }
        })
        .collect()
}

fn premultiply(p: [f64; 4]) -> [f64; 4] {
    [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]]
}

/// `dr_image_filter_op` `blur` at `radius`, or None for NULL.
fn blur8(w: u32, h: u32, pixels: &[u8], radius: f64) -> Option<Vec<u8>> {
    let img = unsafe { dr_image_from_rgba(pixels.as_ptr(), w, h) };
    assert!(!img.is_null());
    let op = CString::new("blur").unwrap();
    let params = CString::new(json!({ "sigma": radius }).to_string()).unwrap();
    let out = unsafe {
        dr_image_filter_op(
            img,
            op.as_ptr(),
            params.as_ptr(),
            ptr::null(),
            ptr::null_mut(),
        )
    };
    unsafe { dr_image_free(img) };
    if out.is_null() {
        return None;
    }
    let bytes =
        unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(out), pixels.len()) }.to_vec();
    unsafe { dr_image_free(out) };
    Some(bytes)
}

/// The 16-bit or float `blur` selected edit on layer 0.
fn edit(d: &PrecisionDocument, radius: f64) -> PrecisionDocument {
    let request =
        CString::new(json!({"layer": 0, "op": "blur", "params": {"sigma": radius}}).to_string())
            .unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            d,
            request.as_ptr(),
            ptr::null(),
            0,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null() && !out.is_null(), "blur r{radius}");
    *unsafe { Box::from_raw(out) }
}

fn doc16(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

/// The largest |premultiplied output − oracle| in codes of `full`, from
/// straight output codes.
fn worst(got: impl Iterator<Item = [f64; 4]>, want: &[[f64; 4]], full: f64) -> f64 {
    let mut worst = 0.0_f64;
    for (got, want) in got.zip(want) {
        let a = got[3];
        for c in 0..4 {
            let value = if c == 3 { a } else { got[c] * a / full };
            worst = worst.max((value - want[c] * full).abs());
        }
    }
    worst
}

/// Radii from the law's first radius to the 16-bit slider's top.
const RADII: [f64; 5] = [7.0, 9.5, 12.5, 40.0, 64.0];

/// 8 and 16 bits equal the oracle to within each output's own quantization
/// (half a code of colour plus half a code of alpha, so 1 code premultiplied).
#[test]
fn eight_and_sixteen_bit_blur_equal_the_extended_box_oracle() {
    let (w, h) = (131_u32, 300_u32);
    for kind in ["quadrant", "noise", "disc"] {
        let straight = fixture(kind, w, h);
        let codes16: Vec<[u16; 4]> = straight
            .iter()
            .map(|p| p.map(|v| (v * 65535.0).round() as u16))
            .collect();
        let bytes: Vec<u8> = straight
            .iter()
            .flat_map(|p| p.map(|v| (v * 255.0).round() as u8))
            .collect();
        let in16: Vec<[f64; 4]> = codes16
            .iter()
            .map(|p| premultiply(p.map(|v| f64::from(v) / 65535.0)))
            .collect();
        let in8: Vec<[f64; 4]> = bytes
            .chunks_exact(4)
            .map(|p| premultiply([0, 1, 2, 3].map(|c| f64::from(p[c]) / 255.0)))
            .collect();
        let d = doc16(w, h, codes16);
        assert!(RADII.iter().all(|&r| r >= FITTED_MIN_RADIUS));
        for radius in RADII {
            let want8 = fitted_blur_rgba(&in8, w as usize, h as usize, radius);
            let got8 = blur8(w, h, &bytes, radius).unwrap();
            let e8 = worst(
                got8.chunks_exact(4)
                    .map(|p| [0, 1, 2, 3].map(|c| f64::from(p[c]))),
                &want8,
                255.0,
            );
            assert!(e8 <= 1.0, "8-bit {kind} r{radius}: {e8} codes");
            let want16 = fitted_blur_rgba(&in16, w as usize, h as usize, radius);
            let out = edit(&d, radius);
            let got16 = out.layer_source(0).unwrap().to_rgba16().unwrap();
            let e16 = worst(got16.iter().map(|p| p.map(f64::from)), &want16, 65535.0);
            assert!(e16 <= 1.0, "16-bit {kind} r{radius}: {e16} codes");
        }
    }
}

/// 32 bits: the same kernel on the linear values, nothing clamped — a value
/// far above white spreads its energy, and the result is the oracle's to
/// f32 storage (relative 1e-5).
#[test]
fn float_blur_is_the_same_kernel_on_linear_values() {
    let (w, h) = (64_u32, 48_u32);
    let mut state = 0xF10A7_u64;
    let pixels: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            let v = splitmix(&mut state);
            let hot = if i == 24 * 64 + 32 { 4000.0 } else { 0.0 };
            [
                (v & 0xFFF) as f32 / 4095.0 + hot,
                ((v >> 12) & 0xFFF) as f32 / 4095.0,
                ((v >> 24) & 0xFFF) as f32 / 4095.0 * 2.0,
                if (v >> 40).is_multiple_of(5) {
                    0.25
                } else {
                    1.0
                },
            ]
        })
        .collect();
    let d = float_doc(w, h, &IccProfile::srgb(), std::slice::from_ref(&pixels));
    let input: Vec<[f64; 4]> = pixels
        .iter()
        .map(|p| premultiply(p.map(f64::from)))
        .collect();
    for radius in [7.0, 12.5] {
        let want = fitted_blur_rgba(&input, w as usize, h as usize, radius);
        let out = edit(&d, radius);
        let got = out.layer_source(0).unwrap().to_rgba32().unwrap();
        let mut over_white = false;
        for (g, p) in got.iter().zip(&want) {
            let a = f64::from(g[3]);
            assert!((a - p[3]).abs() <= 1e-5, "r{radius} alpha {a} vs {}", p[3]);
            for c in 0..3 {
                let value = f64::from(g[c]) * a;
                assert!(
                    (value - p[c]).abs() <= 1e-5 * p[c].abs().max(1.0),
                    "r{radius}: {value} vs {}",
                    p[c]
                );
            }
            over_white |= g[0] > 1.0;
        }
        assert!(over_white, "r{radius}: the hot pixel's light was clamped");
    }
}

/// σ1000, the largest radius `blur` runs, on an 8001-pixel step whose edges
/// are each 4σ from it: the oracle to 1 code.
#[test]
fn radius_1000_equals_the_oracle() {
    let (w, h) = (8001_u32, 1_u32);
    let straight = fixture("quadrant", w, h);
    let bytes: Vec<u8> = straight
        .iter()
        .flat_map(|p| p.map(|v| (v * 255.0).round() as u8))
        .collect();
    let input: Vec<[f64; 4]> = bytes
        .chunks_exact(4)
        .map(|p| premultiply([0, 1, 2, 3].map(|c| f64::from(p[c]) / 255.0)))
        .collect();
    let want = fitted_blur_rgba(&input, w as usize, h as usize, 1000.0);
    let got = blur8(w, h, &bytes, 1000.0).expect("r1000 is admitted");
    let e = worst(
        got.chunks_exact(4)
            .map(|p| [0, 1, 2, 3].map(|c| f64::from(p[c]))),
        &want,
        255.0,
    );
    assert!(e <= 1.0, "r1000: {e} codes");
}

/// Photoshop's own renders from the confirming take (take2), committed with
/// their manifest: the 8-bit `blur` of the take's 30° edge and zone plate is
/// within 1 code of Photoshop's at every channel of every pixel — the bound
/// declared before the take, which every take2 radius from 9 up met.
#[test]
fn eight_bit_blur_is_within_one_code_of_photoshops_renders() {
    let folder =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/photoshop_blur_wand");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(folder.join("manifest.json")).unwrap()).unwrap();
    let rows = manifest["gaussian"].as_array().unwrap();
    assert_eq!(rows.len(), 5);
    for row in rows {
        let input = image::open(folder.join(row["input"].as_str().unwrap()))
            .unwrap()
            .to_rgba8();
        let photoshop = image::open(folder.join(row["photoshop"].as_str().unwrap()))
            .unwrap()
            .to_rgba8();
        let radius = row["radius"].as_f64().unwrap();
        assert!(radius >= FITTED_MIN_RADIUS);
        let (w, h) = input.dimensions();
        let got = blur8(w, h, input.as_raw(), radius).unwrap();
        let worst = got
            .chunks_exact(4)
            .zip(photoshop.as_raw().chunks_exact(4))
            .map(|(g, p)| (0..3).map(|c| g[c].abs_diff(p[c])).max().unwrap())
            .max()
            .unwrap();
        assert!(
            worst <= 1,
            "{} r{radius}: {worst} codes from Photoshop",
            row["input"]
        );
    }
}

/// The 6000×4000 plans `scripts/gaussian-blur-admission/measure.py` holds
/// against the Release app's measured footprints (the record is on
/// `gaussian_blur_filter::plan`): 8-bit `dr_image_filter_op_scratch_bytes`
/// and the 16-bit selected edit plan's `scratch_bytes`, at radius 25 and 64.
/// The float plans are `float_adjust_apply_24mp.rs`'s.
const PLANS_24MP: [(u32, f64, u64); 4] = [
    (8, 25.0, 184_357_760),
    (8, 64.0, 218_412_160),
    (16, 25.0, 485_583_360),
    (16, 64.0, 512_826_880),
];

#[test]
fn camera_size_plans_are_the_admission_drives() {
    use doctored_core::ffi_filters::dr_image_filter_op_scratch_bytes;
    use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit_plan;
    let (w, h) = (6000_u32, 4000_u32);
    let bytes = vec![128_u8; (w * h * 4) as usize];
    let img = unsafe { dr_image_from_rgba(bytes.as_ptr(), w, h) };
    drop(bytes);
    let d = doc16(w, h, vec![[20001, 30002, 40003, 65535]; (w * h) as usize]);
    let mut got = Vec::new();
    for (depth, radius, _) in PLANS_24MP {
        let params = json!({ "sigma": radius }).to_string();
        let plan = if depth == 8 {
            let op = CString::new("blur").unwrap();
            let params = CString::new(params).unwrap();
            unsafe {
                dr_image_filter_op_scratch_bytes(img, op.as_ptr(), params.as_ptr(), ptr::null_mut())
            }
        } else {
            let request = CString::new(
                json!({"layer": 0, "op": "blur", "params": {"sigma": radius}}).to_string(),
            )
            .unwrap();
            let mut error = ptr::null_mut();
            let out = unsafe {
                dr_precision_doc_selected_edit_plan(
                    &d,
                    request.as_ptr(),
                    REFERENCE_WORKING_BYTES,
                    0,
                    &mut error,
                )
            };
            assert!(error.is_null() && !out.is_null());
            let info: serde_json::Value =
                serde_json::from_slice(unsafe { std::ffi::CStr::from_ptr(out) }.to_bytes())
                    .unwrap();
            unsafe { doctored_core::ffi::dr_string_free(out) };
            info["scratch_bytes"].as_u64().unwrap()
        };
        got.push((depth, radius, plan));
    }
    unsafe { dr_image_free(img) };
    assert_eq!(got, PLANS_24MP);
}

#[repr(C)]
struct RusageInfoV4 {
    uuid: [u8; 16],
    fields: [u64; 35],
}
const PHYS_FOOTPRINT: usize = 7;
const INTERVAL_MAX_PHYS_FOOTPRINT: usize = 33;

extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut RusageInfoV4) -> i32;
    fn proc_reset_footprint_interval(pid: i32) -> i32;
}

fn rusage() -> RusageInfoV4 {
    let mut info = RusageInfoV4 {
        uuid: [0; 16],
        fields: [0; 35],
    };
    let rc = unsafe { proc_pid_rusage(std::process::id() as i32, 4, &mut info) };
    assert_eq!(rc, 0, "proc_pid_rusage");
    info
}

/// The 16-bit `blur` selected edit's own footprint on an opaque 6000×4000
/// layer, the operation [`PLANS_24MP`]'s 16-bit rows plan: one case per
/// process (`DR_GB_CASE` = `base`, or a radius such as `25`, `64`, or `6` for
/// the old kernel below 7), under a job control as the app runs it, its
/// result held until E = interval peak − pre is read. Release,
/// `MallocLargeCache=0`. Prints `GB_MEASURE`.
#[test]
#[ignore = "measurement: DR_GB_CASE=base|6|25|64, Release, MallocLargeCache=0"]
fn measure_sixteen_bit_blur() {
    use doctored_core::ffi_jobs::{dr_job_control_create, dr_job_control_free};
    use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit_plan;
    let case = std::env::var("DR_GB_CASE").expect("DR_GB_CASE");
    let (w, h) = (6000_u32, 4000_u32);
    let mut state = 0x16_u64;
    let pixels: Vec<[u16; 4]> = (0..w * h)
        .map(|_| {
            let v = splitmix(&mut state);
            [v as u16, (v >> 16) as u16, (v >> 32) as u16, 65535]
        })
        .collect();
    let d = doc16(w, h, pixels);
    let radius: Option<f64> = (case != "base").then(|| case.parse().expect("a radius"));
    let request = radius.map(|r| {
        CString::new(json!({"layer": 0, "op": "blur", "params": {"sigma": r}}).to_string()).unwrap()
    });
    let plan = request.as_ref().map_or(0, |request| {
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_selected_edit_plan(
                &d,
                request.as_ptr(),
                REFERENCE_WORKING_BYTES,
                0,
                &mut error,
            )
        };
        assert!(error.is_null() && !out.is_null());
        let info: serde_json::Value =
            serde_json::from_slice(unsafe { std::ffi::CStr::from_ptr(out) }.to_bytes()).unwrap();
        unsafe { doctored_core::ffi::dr_string_free(out) };
        info["scratch_bytes"].as_u64().unwrap()
    });
    let pre = rusage().fields[PHYS_FOOTPRINT];
    unsafe { proc_reset_footprint_interval(std::process::id() as i32) };
    let held = request.map(|request| {
        let control = unsafe { dr_job_control_create() };
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_selected_edit(
                &d,
                request.as_ptr(),
                ptr::null(),
                0,
                REFERENCE_WORKING_BYTES,
                0,
                control,
                &mut error,
            )
        };
        unsafe { dr_job_control_free(control) };
        assert!(error.is_null() && !out.is_null());
        unsafe { Box::from_raw(out) }
    });
    let peak = rusage().fields[INTERVAL_MAX_PHYS_FOOTPRINT];
    drop(held);
    let e = peak.saturating_sub(pre);
    println!(
        "GB_MEASURE case={case} plan={plan} pre={pre} peak={peak} E={e} floor={}",
        (e * 5).div_ceil(4)
    );
}

/// The 8-bit `dr_image_filter_op` `blur` footprint on opaque 6000×4000
/// splitmix noise, the operation [`PLANS_24MP`]'s 8-bit rows plan
/// (uncontrolled): one case per process (`DR_GB8_CASE` = `base` or a
/// radius), its output held until E = interval peak − pre is read. Release,
/// `MallocLargeCache=0`. Prints `GB8_MEASURE`.
#[test]
#[ignore = "measurement: DR_GB8_CASE=base|25|64, Release, MallocLargeCache=0"]
fn measure_eight_bit_blur() {
    use doctored_core::ffi_filters::dr_image_filter_op_scratch_bytes;
    let case = std::env::var("DR_GB8_CASE").expect("DR_GB8_CASE");
    let (w, h) = (6000_u32, 4000_u32);
    let mut state = 0x488_u64;
    let bytes: Vec<u8> = (0..w * h)
        .flat_map(|_| {
            let v = splitmix(&mut state);
            [v as u8, (v >> 8) as u8, (v >> 16) as u8, 255]
        })
        .collect();
    let img = unsafe { dr_image_from_rgba(bytes.as_ptr(), w, h) };
    drop(bytes);
    let op = CString::new("blur").unwrap();
    let params = (case != "base").then(|| {
        let radius: f64 = case.parse().expect("a radius");
        CString::new(json!({ "sigma": radius }).to_string()).unwrap()
    });
    let plan = params.as_ref().map_or(0, |p| unsafe {
        dr_image_filter_op_scratch_bytes(img, op.as_ptr(), p.as_ptr(), ptr::null_mut())
    });
    let pre = rusage().fields[PHYS_FOOTPRINT];
    unsafe { proc_reset_footprint_interval(std::process::id() as i32) };
    let out = params.as_ref().map(|p| unsafe {
        dr_image_filter_op(img, op.as_ptr(), p.as_ptr(), ptr::null(), ptr::null_mut())
    });
    let peak = rusage().fields[INTERVAL_MAX_PHYS_FOOTPRINT];
    if let Some(out) = out {
        assert!(!out.is_null());
        unsafe { dr_image_free(out) };
    }
    unsafe { dr_image_free(img) };
    let e = peak.saturating_sub(pre);
    println!(
        "GB8_MEASURE case={case} plan={plan} pre={pre} peak={peak} E={e} floor={}",
        (e * 5).div_ceil(4)
    );
}
