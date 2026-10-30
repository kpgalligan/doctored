//! 8-bit Gaussian Blur as `dr_image_filter_op` `blur`, a planned,
//! cancellable job run one 128-row band at a time. Every expectation is
//! black-box through the C boundary. Below σ4 the oracle is the image crate's
//! kernel over the WHOLE image, which the banded run must equal byte for
//! byte: on opaque input directly, and on partial alpha as the whole-image
//! premultiplied f32 blur divided back out by the rule restated here. From σ4
//! to 7 the recursive filter runs (`gaussian_radius_independent.rs` holds it to
//! its bound through the legacy Sharpen), and a radius from 7 up
//! runs the extended-box kernel (`gaussian_blur_photoshop.rs`); here the
//! controlled run reports one stage-6 step per band and equals the
//! uncontrolled one. The plan is checked against its formula.
use crate::common;
use common::probe::reports;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_filters::{dr_image_filter_op, dr_image_filter_op_scratch_bytes};
use doctored_core::ffi_jobs::{dr_job_control_create, dr_job_control_free};
use doctored_core::job_control::DrJobControl;
use image::{Rgba, Rgba32FImage, RgbaImage};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

/// The core's per-allocation bookkeeping allowance (1 MiB).
const BK: u64 = 1 << 20;
/// `selected_filter::BAND_ROWS`.
const BAND_ROWS: u64 = 128;

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Opaque splitmix noise.
fn opaque(w: u32, h: u32) -> RgbaImage {
    let mut state = 0x488_u64;
    RgbaImage::from_fn(w, h, |_, _| {
        let v = splitmix(&mut state);
        Rgba([v as u8, (v >> 8) as u8, (v >> 16) as u8, 255])
    })
}

/// Splitmix colour whose alpha is 0 for a quarter of the pixels, a partial
/// value for another quarter and opaque otherwise.
fn partial(w: u32, h: u32) -> RgbaImage {
    let mut state = 0x4880_u64;
    RgbaImage::from_fn(w, h, |_, _| {
        let v = splitmix(&mut state);
        let alpha = match (v >> 40) % 4 {
            0 => 0,
            1 => 1 + ((v >> 48) as u8 % 254),
            _ => 255,
        };
        Rgba([v as u8, (v >> 8) as u8, (v >> 16) as u8, alpha])
    })
}

unsafe fn text(p: *mut c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    s
}

/// `dr_image_filter_op` `blur` with `params` under `control`: the output
/// bytes, or the error (empty for a NULL with no message).
fn blur(src: &RgbaImage, params: &str, control: *const DrJobControl) -> Result<Vec<u8>, String> {
    let img = unsafe { dr_image_from_rgba(src.as_ptr(), src.width(), src.height()) };
    assert!(!img.is_null());
    let op = CString::new("blur").unwrap();
    let params = CString::new(params).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_image_filter_op(img, op.as_ptr(), params.as_ptr(), control, &mut err) };
    let error = unsafe { text(err) };
    let result = if out.is_null() {
        Err(error)
    } else {
        assert!(error.is_empty(), "{error}");
        let v =
            unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(out), src.as_raw().len()) }
                .to_vec();
        unsafe { dr_image_free(out) };
        Ok(v)
    };
    unsafe { dr_image_free(img) };
    result
}

fn sigma(value: f32) -> String {
    format!(r#"{{"sigma":{}}}"#, f64::from(value))
}

/// `dr_image_filter_op_scratch_bytes` for `blur` on a `w`×`h` image.
fn plan(w: u32, h: u32, params: &str) -> Result<u64, String> {
    let bytes = vec![0_u8; w as usize * h as usize * 4];
    let img = unsafe { dr_image_from_rgba(bytes.as_ptr(), w, h) };
    assert!(!img.is_null());
    drop(bytes);
    let op = CString::new("blur").unwrap();
    let params = CString::new(params).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let scratch =
        unsafe { dr_image_filter_op_scratch_bytes(img, op.as_ptr(), params.as_ptr(), &mut err) };
    unsafe { dr_image_free(img) };
    let error = unsafe { text(err) };
    if error.is_empty() {
        Ok(scratch)
    } else {
        assert_eq!(scratch, 0, "a refusal answers 0");
        Err(error)
    }
}

/// The byte quantization of the core's `sampling::quantize_coverage`.
fn quantize(value: f32) -> u8 {
    value.clamp(0.0, 255.0).round() as u8
}

/// `sampling::accumulate_unorm` at weight 1 into a zeroed accumulator, in its
/// operation order: `pm = 1 · a · (1/255)`, colour `0 + pm · c`, alpha `0 + 1 · a`.
fn premultiplied(pixel: [u8; 4]) -> [f32; 4] {
    let a = f32::from(pixel[3]);
    let pm = 1.0_f32 * a * (1.0 / 255.0);
    let mut acc = [0.0_f32; 4];
    for c in 0..3 {
        acc[c] += pm * f32::from(pixel[c]);
    }
    acc[3] += 1.0 * a;
    acc
}

/// `sampling::straight_unorm` for bytes: the alpha is quantized first, a zero
/// alpha is transparent black, and colour divides by the RAW alpha.
fn straight(acc: [f32; 4]) -> [u8; 4] {
    let alpha = quantize(acc[3]);
    if alpha == 0 {
        return [0; 4];
    }
    let scale = 255.0 / acc[3];
    [
        quantize(acc[0] * scale),
        quantize(acc[1] * scale),
        quantize(acc[2] * scale),
        alpha,
    ]
}

/// Below the switch an opaque image is the image crate's kernel over the
/// whole image, byte for byte, whatever the band count (300 rows are three
/// bands; the halo rows are clamped at both image edges).
#[test]
fn opaque_blur_below_the_switch_equals_the_whole_image_kernel() {
    for (w, h) in [(131_u32, 300_u32), (97, 300)] {
        let src = opaque(w, h);
        for s in [0.5_f32, 2.0, 3.9] {
            let want = image::imageops::blur(&src, s).into_raw();
            let got = blur(&src, &sigma(s), ptr::null()).unwrap_or_else(|e| panic!("σ{s}: {e}"));
            assert!(
                got == want,
                "σ{s} at {w}x{h} differs from the whole-image kernel"
            );
            assert!(got != *src.as_raw(), "σ{s} at {w}x{h} changed nothing");
        }
    }
}

/// Partial alpha: the whole-image premultiplied f32 blur, divided back out.
#[test]
fn partial_alpha_blur_below_the_switch_equals_the_whole_image_premultiplied_kernel() {
    let (w, h) = (131_u32, 300_u32);
    let src = partial(w, h);
    let float = Rgba32FImage::from_fn(w, h, |x, y| Rgba(premultiplied(src.get_pixel(x, y).0)));
    for s in [0.5_f32, 2.0, 3.9] {
        let low = image::imageops::blur(&float, s);
        let want: Vec<u8> = low.pixels().flat_map(|p| straight(p.0)).collect();
        let got = blur(&src, &sigma(s), ptr::null()).unwrap_or_else(|e| panic!("σ{s}: {e}"));
        assert!(
            got == want,
            "σ{s} differs from the whole-image premultiplied kernel"
        );
    }
}

/// A controlled run reports one stage-6 step per band, strictly increasing,
/// in both arms, and its bytes are the uncontrolled run's.
#[test]
fn a_controlled_blur_reports_each_band_and_equals_the_uncontrolled_run() {
    let (w, h) = (240_u32, 400_u32);
    let src = partial(w, h);
    for s in [6.0_f32, 2.0] {
        let params = sigma(s);
        let plain = blur(&src, &params, ptr::null()).unwrap_or_else(|e| panic!("σ{s}: {e}"));
        let control = unsafe { dr_job_control_create() };
        let controlled = blur(&src, &params, control).unwrap_or_else(|e| panic!("σ{s}: {e}"));
        unsafe { dr_job_control_free(control) };
        assert!(plain == controlled, "σ{s}: controlled bytes differ");

        let log = reports(
            |token| match blur(&src, &params, token as *const DrJobControl) {
                Ok(_) => (true, String::new()),
                Err(e) => (false, e),
            },
        );
        let steps: Vec<f64> = log
            .iter()
            .filter(|r| r.stage == 6)
            .map(|r| r.progress)
            .collect();
        // 400 rows are four bands: 128, 128, 128 and 16.
        assert_eq!(steps.len(), 4, "σ{s}: {steps:?}");
        assert!(
            steps.windows(2).all(|pair| pair[0] < pair[1]),
            "σ{s}: stage-6 progress is not strictly increasing: {steps:?}"
        );
        assert!(steps[0] > 0.0 && steps[3] == 1.0, "σ{s}: {steps:?}");
        let last = log.last().expect("a report");
        assert!(last.progress == 1.0 && last.stage == 5, "σ{s}: {last:?}");
    }
}

/// The default σ is 4, and `blur`'s three refusals are the plan's too, in the
/// same words.
#[test]
fn blur_defaults_to_sigma_4_and_refuses_in_its_own_words() {
    let src = opaque(37, 29);
    assert_eq!(
        blur(&src, "{}", ptr::null()),
        blur(&src, r#"{"sigma":4}"#, ptr::null())
    );
    assert!(blur(&src, "{}", ptr::null()).is_ok());
    // σ1000 is admitted; nothing above it is run as σ1000.
    assert!(blur(&src, r#"{"sigma":1000}"#, ptr::null()).is_ok());
    for (params, refusal) in [
        (r#"{"radius":2}"#, "unexpected blur parameter radius"),
        (r#"{"sigma":"2"}"#, "sigma must be numeric"),
        (r#"{"sigma":null}"#, "sigma must be numeric"),
        (r#"{"sigma":0}"#, "sigma must be finite in (0,1000]"),
        (r#"{"sigma":-1}"#, "sigma must be finite in (0,1000]"),
        (r#"{"sigma":1000.5}"#, "sigma must be finite in (0,1000]"),
        (r#"{"sigma":1e-60}"#, "sigma must be finite in (0,1000]"),
    ] {
        assert_eq!(
            blur(&src, params, ptr::null()),
            Err(refusal.to_string()),
            "{params}"
        );
        assert_eq!(plan(37, 29, params), Err(refusal.to_string()), "{params}");
    }
}

/// The plan is the n×4 output plus one band, plus 4 BK, plus a quarter: no
/// 20n combine stage, because `blur` holds no float plane beside its output.
#[test]
fn the_plan_is_the_output_plus_one_band() {
    let (w, h) = (6000_u64, 4000_u64);
    let quarter = |base: u64| base + base.div_ceil(4);
    // σ2, the image crate's kernel: 48 B per pixel of a band and its
    // ceil(4σ + 4) halo rows either side, plus 2 BK.
    let halo = 12;
    let legacy = w * h.min(BAND_ROWS + 2 * halo) * 48 + 2 * BK;
    let expected = quarter(4 * w * h + legacy + 4 * BK);
    assert_eq!(expected, 182_584_320);
    assert_eq!(plan(6000, 4000, r#"{"sigma":2}"#), Ok(expected));
    // r25 is the extended-box blur (`gaussian_blur_filter::plan`
    // for one band; it was the recursive filter's 188,916,480): the band's f32
    // output, an f32 window of the band and the kernel's support either side
    // (4 boxes of radius l = 20 at σ = 0.99·25, so 4·21 = 84 rows), the two
    // binary64 extended rows, the two binary64 extended 32-column blocks,
    // plus 5 BK.
    let pad = 84;
    let rows = BAND_ROWS.min(h);
    let window = h.min(BAND_ROWS + 2 * pad);
    let boxes = rows * w * 16
        + window * w * 16
        + 2 * (w + 2 * pad) * 32
        + 2 * (window + 2 * pad) * 32 * 32
        + 5 * BK;
    let expected = quarter(4 * w * h + boxes + 4 * BK);
    assert_eq!(expected, 184_357_760);
    assert_eq!(plan(6000, 4000, r#"{"sigma":25}"#), Ok(expected));
    // A plan never bypasses, and it is far below the 20n the other eleven
    // ops charge for their combine stage.
    assert!(expected < 20 * w * h);
}
