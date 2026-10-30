//! The extended-range Fit preview of a 32-bit float development,
//! through `dr_precision_image_develop_preview_extended`.
//!
//! Oracle, written here and read off no implementation: the Fit cell of
//! output column `c` over a region starting at `x` and `w` wide at divisor
//! `d` is `x0 = x + c·d`, `x1 = min(x0 + d, x + w)`, centre
//! `x0 + floor((x1 − x0) / 2)` (rows the same), and each centre sample is
//! display_encoding's rule X at gain 1: RGB clamped to ±65504, times alpha,
//! and alpha, each rounded to nearest-even binary16 (`half`), written
//! little-endian. The peak is the largest RGB value of those bits. Bytes and
//! peak are compared exactly. Every refusal leaves the buffer as it was.
//! Clipping markers (#4): a cell any of whose alpha-positive samples has an
//! RGB channel ≤ 0.0 (flag 1) or ≥ 1.0 (flag 2) is opaque binary16 blue, red
//! or magenta; the marked count and planted endpoints are hand-counted.
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_develop_inspect::{
    dr_precision_image_develop_preview_extended, dr_precision_image_storage,
};
use doctored_core::float_view::EXTENDED_DISPLAY_BYTES_PER_PIXEL;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use std::ffi::{c_char, CStr};
use std::ptr;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// The core's per-operation bookkeeping allowance, restated (1 MiB).
const BOOKKEEPING: u64 = 1024 * 1024;
const SAMPLES: [f32; 7] = [0.0, 0.25, 1.0, 2.0, 8.0, -0.1, 65504.0];
const UNTOUCHED: u8 = 0xA5;

fn taken(error: *mut c_char) -> Option<String> {
    if error.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    Some(text)
}

/// A `w` × `h` float image whose every RGB channel takes each of `SAMPLES`
/// across the plane and whose alpha alternates 1 and 0.5.
fn float_image(w: u32, h: u32) -> PrecisionImage {
    let pixels = (0..w * h)
        .map(|i| {
            let i = i as usize;
            [
                SAMPLES[i % 7],
                SAMPLES[(i + 3) % 7],
                SAMPLES[(i * 5 + 1) % 7],
                if i.is_multiple_of(2) { 1.0 } else { 0.5 },
            ]
        })
        .collect();
    PrecisionImage::from_rgba32_extended(w, h, pixels, IccProfile::srgb(), None).unwrap()
}

fn centres(start: u32, extent: u32, divisor: u32) -> Vec<u32> {
    (0..extent.div_ceil(divisor))
        .map(|i| {
            let begin = start + i * divisor;
            let end = (begin + divisor).min(start + extent);
            begin + (end - begin) / 2
        })
        .collect()
}

fn rule_x(pixel: [f32; 4]) -> [u16; 4] {
    let alpha = pixel[3];
    let rgb = |v: f32| half::f16::from_f32(v.clamp(-65504.0, 65504.0) * alpha).to_bits();
    [
        rgb(pixel[0]),
        rgb(pixel[1]),
        rgb(pixel[2]),
        half::f16::from_f32(alpha).to_bits(),
    ]
}

/// The oracle's bytes and peak for `region` at `divisor`.
fn expected(image: &PrecisionImage, region: [u32; 4], divisor: u32) -> (Vec<u8>, f32) {
    let pixels = image.to_rgba32().unwrap();
    let width = image.dimensions().0;
    let mut bytes = Vec::new();
    let mut peak = f32::NEG_INFINITY;
    for y in centres(region[1], region[3], divisor) {
        for x in centres(region[0], region[2], divisor) {
            let bits = rule_x(pixels[(y * width + x) as usize]);
            for (c, value) in bits.iter().enumerate() {
                if c < 3 {
                    peak = peak.max(half::f16::from_bits(*value).to_f32());
                }
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    (bytes, peak)
}

/// One call into a buffer of `len` bytes prefilled with `UNTOUCHED`, no
/// clipping markers.
fn call(
    image: &PrecisionImage,
    region: [u32; 4],
    divisor: u32,
    len: usize,
    limit: u64,
    control: Option<&DrJobControl>,
) -> (Result<f32, String>, Vec<u8>) {
    let (result, out) = call_clipping(image, region, divisor, 0, len, limit, control);
    (
        result.map(|(peak, marked)| {
            assert_eq!(marked, 0, "clipping 0 marks nothing");
            peak
        }),
        out,
    )
}

/// One call with `clipping` flags; answers `(peak, marked)`.
fn call_clipping(
    image: &PrecisionImage,
    region: [u32; 4],
    divisor: u32,
    clipping: u32,
    len: usize,
    limit: u64,
    control: Option<&DrJobControl>,
) -> (Result<(f32, u64), String>, Vec<u8>) {
    let mut out = vec![UNTOUCHED; len];
    let mut peak = -1f32;
    let mut marked = u64::MAX;
    let mut error = ptr::null_mut();
    let ok = unsafe {
        dr_precision_image_develop_preview_extended(
            image,
            region[0],
            region[1],
            region[2],
            region[3],
            divisor,
            clipping,
            out.as_mut_ptr(),
            out.len(),
            &mut peak,
            &mut marked,
            limit,
            0,
            control.map_or(ptr::null(), |c| c as *const DrJobControl),
            &mut error,
        )
    };
    let message = taken(error);
    if ok {
        assert!(message.is_none());
        (Ok((peak, marked)), out)
    } else {
        assert_eq!(peak, -1.0, "a refusal writes no peak");
        assert_eq!(marked, u64::MAX, "a refusal writes no count");
        (Err(message.expect("a refusal names its reason")), out)
    }
}

/// Binary16 0.0 and 1.0 little-endian, the markers' components.
const H0: [u8; 2] = [0x00, 0x00];
const H1: [u8; 2] = [0x00, 0x3C];

/// The oracle marker of one cell: OR over every alpha-positive sample of
/// "some RGB channel ≤ 0.0" (bit 1) and "some RGB channel ≥ 1.0" (bit 2),
/// each tested only when its flag is set; `None` when unmarked.
fn marker(cell: &[[f32; 4]], clipping: u32) -> Option<[u8; 8]> {
    let lit = |p: &[f32; 4]| p[3] > 0.0;
    let low = clipping & 1 != 0
        && cell
            .iter()
            .any(|p| lit(p) && p[..3].iter().any(|v| *v <= 0.0));
    let high = clipping & 2 != 0
        && cell
            .iter()
            .any(|p| lit(p) && p[..3].iter().any(|v| *v >= 1.0));
    let colour = |r: [u8; 2], b: [u8; 2]| {
        let mut m = [0u8; 8];
        m[0..2].copy_from_slice(&r);
        m[2..4].copy_from_slice(&H0);
        m[4..6].copy_from_slice(&b);
        m[6..8].copy_from_slice(&H1);
        m
    };
    match (low, high) {
        (true, true) => Some(colour(H1, H1)),
        (true, false) => Some(colour(H0, H1)),
        (false, true) => Some(colour(H1, H0)),
        _ => None,
    }
}

/// A `w` × `h` image of mostly mid-grey with a few planted endpoint samples:
/// (1,1) exactly 1.0 in green, (5,2) 2.5 in red, (2,4) exactly 0.0 in blue,
/// (7,4) −0.2 in red and 3.0 in green (both), and (4,0) 4.0 at alpha 0
/// (never marked).
fn planted_image(w: u32, h: u32) -> PrecisionImage {
    let mut pixels = vec![[0.4f32, 0.5, 0.6, 1.0]; (w * h) as usize];
    let mut set = |x: u32, y: u32, p: [f32; 4]| pixels[(y * w + x) as usize] = p;
    set(1, 1, [0.4, 1.0, 0.6, 1.0]);
    set(5, 2, [2.5, 0.5, 0.6, 0.5]);
    set(2, 4, [0.4, 0.5, 0.0, 1.0]);
    set(7, 4, [-0.2, 3.0, 0.6, 1.0]);
    set(4, 0, [4.0, 4.0, 4.0, 0.0]);
    PrecisionImage::from_rgba32_extended(w, h, pixels, IccProfile::srgb(), None).unwrap()
}

/// The oracle bytes, peak and marked count with markers for `clipping`.
fn expected_clipping(
    image: &PrecisionImage,
    region: [u32; 4],
    divisor: u32,
    clipping: u32,
) -> (Vec<u8>, f32, u64) {
    let (plain, peak) = expected(image, region, divisor);
    let pixels = image.to_rgba32().unwrap();
    let width = image.dimensions().0;
    let columns = region[2].div_ceil(divisor);
    let mut bytes = plain.clone();
    let mut marked = 0;
    for row in 0..region[3].div_ceil(divisor) {
        for column in 0..columns {
            let x0 = region[0] + column * divisor;
            let x1 = (x0 + divisor).min(region[0] + region[2]);
            let y0 = region[1] + row * divisor;
            let y1 = (y0 + divisor).min(region[1] + region[3]);
            let cell: Vec<[f32; 4]> = (y0..y1)
                .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                .map(|(x, y)| pixels[(y * width + x) as usize])
                .collect();
            if let Some(m) = marker(&cell, clipping) {
                let at = ((row * columns + column) * 8) as usize;
                bytes[at..at + 8].copy_from_slice(&m);
                marked += 1;
            }
        }
    }
    (bytes, peak, marked)
}

#[test]
fn clipping_marks_every_cell_holding_an_endpoint_sample_and_the_peak_ignores_markers() {
    let image = planted_image(9, 6);
    for (region, divisor) in [([0, 0, 9, 6], 1), ([0, 0, 9, 6], 3), ([1, 1, 7, 4], 2)] {
        let (plain, plain_peak) = expected(&image, region, divisor);
        for clipping in 0..=3 {
            let (bytes, peak, marked) = expected_clipping(&image, region, divisor, clipping);
            let (result, out) =
                call_clipping(&image, region, divisor, clipping, bytes.len(), LIMIT, None);
            assert_eq!(
                result.unwrap(),
                (peak, marked),
                "{region:?} /{divisor} c{clipping}"
            );
            assert_eq!(out, bytes, "{region:?} /{divisor} c{clipping}");
            assert_eq!(peak, plain_peak, "markers never raise the peak");
            if clipping == 0 {
                assert_eq!(out, plain);
            }
        }
    }
    // Hand counts at divisor 1 over the whole image: highlights mark (1,1),
    // (5,2) and (7,4); shadows mark (2,4) and (7,4); the alpha-0 4.0 at
    // (4,0) is never marked. Both: four cells, (7,4) magenta.
    let len = 9 * 6 * 8;
    let count = |clipping| {
        call_clipping(&image, [0, 0, 9, 6], 1, clipping, len, LIMIT, None)
            .0
            .unwrap()
            .1
    };
    assert_eq!(count(2), 3);
    assert_eq!(count(1), 2);
    assert_eq!(count(3), 4);
    let (_, out) = call_clipping(&image, [0, 0, 9, 6], 1, 3, len, LIMIT, None);
    let at = ((4 * 9 + 7) * 8) as usize;
    assert_eq!(&out[at..at + 8], &[0, 0x3C, 0, 0, 0, 0x3C, 0, 0x3C]);
    // At divisor 3 the (5,2) highlight marks the whole middle top cell even
    // though its centre (4,1) is mid-grey.
    let (_, out) = call_clipping(&image, [0, 0, 9, 6], 3, 2, 3 * 2 * 8, LIMIT, None);
    assert_eq!(&out[8..16], &[0, 0x3C, 0, 0, 0, 0, 0, 0x3C]);
}

#[test]
fn clipping_past_three_is_refused_and_leaves_the_buffer_untouched() {
    let image = planted_image(9, 6);
    let (result, out) = call_clipping(&image, [0, 0, 9, 6], 1, 4, 9 * 6 * 8, LIMIT, None);
    assert!(result.unwrap_err().contains("clipping flags 0 to 3"));
    assert!(out.iter().all(|b| *b == UNTOUCHED));
}

fn refused(
    image: &PrecisionImage,
    region: [u32; 4],
    divisor: u32,
    len: usize,
    limit: u64,
) -> String {
    let (result, out) = call(image, region, divisor, len, limit, None);
    assert!(
        out.iter().all(|b| *b == UNTOUCHED),
        "a refusal leaves out untouched"
    );
    result.expect_err("refused")
}

#[test]
fn divisor_one_is_rule_x_of_every_pixel() {
    let image = float_image(9, 5);
    let region = [0, 0, 9, 5];
    let (bytes, peak) = expected(&image, region, 1);
    assert_eq!(bytes.len(), 9 * 5 * 8);
    let (result, out) = call(&image, region, 1, bytes.len(), LIMIT, None);
    assert_eq!(result.unwrap(), peak);
    assert_eq!(out, bytes);
    // 65504 at alpha 1 is the largest value rule X writes, −0.1 stays below 0.
    assert_eq!(peak, 65504.0);
    assert!(out
        .chunks_exact(2)
        .any(|b| half::f16::from_bits(u16::from_le_bytes([b[0], b[1]])).to_f32() < 0.0));
}

#[test]
fn a_larger_divisor_samples_the_cell_centres_of_an_offset_region_with_partial_edges() {
    let image = float_image(13, 11);
    // 8 × 7 from (2, 3) at divisor 3: 3 × 3 cells, the last column 2 wide and
    // the last row 1 tall.
    let region = [2, 3, 8, 7];
    assert_eq!(centres(2, 8, 3), vec![3, 6, 9]);
    assert_eq!(centres(3, 7, 3), vec![4, 7, 9]);
    let (bytes, peak) = expected(&image, region, 3);
    assert_eq!(bytes.len(), 3 * 3 * 8);
    let (result, out) = call(&image, region, 3, bytes.len(), LIMIT, None);
    assert_eq!(result.unwrap(), peak);
    assert_eq!(out, bytes);
    assert!(peak > 1.0);
}

#[test]
fn every_refusal_is_named_and_leaves_the_buffer_untouched() {
    let image = float_image(9, 5);
    let region = [0, 0, 9, 5];
    let len = 9 * 5 * 8;
    let sixteen = PrecisionImage::from_rgba16(
        9,
        5,
        vec![[1000, 2000, 3000, 65535]; 45],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    assert!(refused(&sixteen, region, 1, len, LIMIT).contains("rgba32_float"));
    assert!(refused(&image, region, 1, len - 8, LIMIT).contains("8 bytes"));
    assert!(refused(&image, region, 1, len + 1, LIMIT).contains("8 bytes"));
    assert!(refused(&image, region, 0, len, LIMIT).contains("positive divisor"));
    assert!(refused(&image, [1, 0, 9, 5], 1, len, LIMIT).contains("inside its source"));
    assert!(refused(&image, [0, 0, 0, 5], 1, 0, LIMIT).contains("inside its source"));
    let wide = float_image(1601, 1);
    assert!(refused(&wide, [0, 0, 1601, 1], 1, 1601 * 8, LIMIT).contains("1600 pixels per side"));
    // At divisor 2 the same strip fits.
    let (fits, _) = call(&wide, [0, 0, 1601, 1], 2, 801 * 8, LIMIT, None);
    assert!(fits.is_ok());

    // The plan is the extended view's at the output size, beside the
    // image's retained bytes: one byte short refuses, the plan itself fits.
    let plan = image.retained_bytes() + BOOKKEEPING + 45 * EXTENDED_DISPLAY_BYTES_PER_PIXEL;
    assert!(refused(&image, region, 1, len, plan - 1).contains("working limit"));
    let (admitted, _) = call(&image, region, 1, len, plan, None);
    assert!(admitted.is_ok());
    let (bytes, _) = expected(&image, [0, 0, 9, 5], 3);
    let small = image.retained_bytes() + BOOKKEEPING + 6 * EXTENDED_DISPLAY_BYTES_PER_PIXEL;
    let (fit, out) = call(&image, region, 3, bytes.len(), small, None);
    assert!(fit.is_ok());
    assert_eq!(out, bytes);
}

#[test]
fn a_cancelled_token_answers_operation_cancelled() {
    let image = float_image(9, 5);
    let token = DrJobControl::default();
    token.cancel();
    let (result, out) = call(&image, [0, 0, 9, 5], 1, 9 * 5 * 8, LIMIT, Some(&token));
    assert!(result.unwrap_err().contains("operation cancelled"));
    assert!(out.iter().all(|b| *b == UNTOUCHED));
    assert_eq!(token.scratch_bytes(), 0);
}

#[test]
fn the_storage_query_answers_the_bits_per_channel() {
    let float = float_image(3, 2);
    let sixteen =
        PrecisionImage::from_rgba16(3, 2, vec![[1, 2, 3, 65535]; 6], IccProfile::srgb(), None)
            .unwrap();
    unsafe {
        assert_eq!(dr_precision_image_storage(&float), 32);
        assert_eq!(dr_precision_image_storage(&sixteen), 16);
        assert_eq!(dr_precision_image_storage(ptr::null()), 0);
    }
}
