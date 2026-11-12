//! Independent public-FFI references for platform buffer layout and alpha.
//! Expected orientation uses labelled row matrices, never production transforms.
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_host_pixels::{dr_doc_from_host_pixels, dr_precision_doc_from_host_pixels};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use crate::common;
use common::probe::{cancelled_parked_at, entry_count, parked_line};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
// Source rows are ABC / DEF. Expected output is written explicitly from
// TIFF/EXIF's row/column directions, independent of core coordinate code.
const ORIENTED: [[usize; 6]; 8] = [
    [0, 1, 2, 3, 4, 5],
    [2, 1, 0, 5, 4, 3],
    [5, 4, 3, 2, 1, 0],
    [3, 4, 5, 0, 1, 2],
    [0, 3, 1, 4, 2, 5],
    [3, 0, 4, 1, 5, 2],
    [5, 2, 4, 1, 3, 0],
    [2, 5, 1, 4, 0, 3],
];

struct Request<'a> {
    bytes: &'a [u8],
    width: u32,
    height: u32,
    row: usize,
    layout: &'a str,
    orientation: u32,
    profile: &'a [u8],
    typed: bool,
    limit: u64,
    other: u64,
    control: Option<&'a DrJobControl>,
}
#[derive(Debug, PartialEq)]
struct ResultPixels {
    dimensions: (u32, u32),
    samples: Vec<[u16; 4]>,
    profile: Vec<u8>,
}
unsafe fn error_string(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let value = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    value
}
fn capture(request: &Request<'_>) -> Result<ResultPixels, String> {
    let name = CString::new(request.layout).unwrap();
    let mut error = ptr::null_mut();
    let control = request.control.map_or(ptr::null(), |value| value);
    let result = if request.typed {
        let pointer = unsafe {
            dr_precision_doc_from_host_pixels(
                request.bytes.as_ptr(),
                request.bytes.len(),
                request.width,
                request.height,
                request.row,
                name.as_ptr(),
                request.orientation,
                request.profile.as_ptr(),
                request.profile.len(),
                request.limit,
                request.other,
                control,
                &mut error,
            )
        };
        if pointer.is_null() {
            None
        } else {
            let doc = unsafe { Box::from_raw(pointer) };
            Some(ResultPixels {
                dimensions: doc.dimensions(),
                samples: doc.layer(0).unwrap().source().unwrap().to_rgba16().unwrap(),
                profile: doc.profile().bytes().to_vec(),
            })
        }
    } else {
        let pointer = unsafe {
            dr_doc_from_host_pixels(
                request.bytes.as_ptr(),
                request.bytes.len(),
                request.width,
                request.height,
                request.row,
                name.as_ptr(),
                request.orientation,
                request.profile.as_ptr(),
                request.profile.len(),
                request.limit,
                request.other,
                control,
                &mut error,
            )
        };
        if pointer.is_null() {
            None
        } else {
            let doc = unsafe { Box::from_raw(pointer) };
            Some(ResultPixels {
                dimensions: (doc.width, doc.height),
                samples: doc.layers[0]
                    .pixels
                    .contiguous_image()
                    .pixels()
                    .map(|pixel| pixel.0.map(u16::from))
                    .collect(),
                profile: doc.profile.bytes().to_vec(),
            })
        }
    };
    let message = unsafe { error_string(error) };
    if let Some(value) = result {
        assert!(message.is_empty(), "{message}");
        Ok(value)
    } else {
        assert!(!message.is_empty(), "refusal lacked a diagnostic");
        Err(message)
    }
}
fn request<'a>(
    bytes: &'a [u8],
    layout: &'a str,
    profile: &'a [u8],
    width: u32,
    height: u32,
    row: usize,
) -> Request<'a> {
    Request {
        bytes,
        width,
        height,
        row,
        layout,
        orientation: 1,
        profile,
        typed: layout.contains("16"),
        limit: LIMIT,
        other: 0,
        control: None,
    }
}
fn encode(pixels: &[[u16; 4]], layout: &str, width: usize, padding: usize) -> (Vec<u8>, usize) {
    // Independently encode a named platform memory representation. This helper
    // never calculates expected alpha or orientation; tests supply those values.
    let sixteen = layout.contains("16");
    let rgb = layout == "rgb8" || layout == "rgb16_le";
    let stride = width * if rgb { 3 } else { 4 } * if sixteen { 2 } else { 1 } + padding;
    let mut result = Vec::new();
    for row in pixels.chunks(width) {
        for p in row {
            let components = if layout.starts_with("argb") {
                [p[3], p[0], p[1], p[2]]
            } else if layout.starts_with("bgra") {
                [p[2], p[1], p[0], p[3]]
            } else {
                *p
            };
            for value in components.into_iter().take(if rgb { 3 } else { 4 }) {
                if sixteen {
                    result.extend(if layout.contains("_be") {
                        value.to_be_bytes()
                    } else {
                        value.to_le_bytes()
                    });
                } else {
                    result.push(value as u8);
                }
            }
        }
        result.extend(std::iter::repeat_n(0xd7, padding));
    }
    (result, stride)
}

#[test]
fn all_straight_layouts_preserve_exact_samples_original_profile_and_hidden_rgb() {
    let profile = IccProfile::display_p3();
    for layout in [
        "rgba8",
        "rgb8",
        "bgra8",
        "bgra8_opaque",
        "rgba8_opaque",
        "rgba16_le",
        "rgb16_le",
        "rgba16_le_opaque",
        "argb16_be",
        "argb16_be_opaque",
    ] {
        let maximum = if layout.contains("16") { 65535 } else { 255 };
        let pixels = [
            [1, maximum - 1, 31, 0],
            [maximum, 17, 3, 1],
            [37, 53, maximum / 2, maximum],
        ];
        let (bytes, row) = encode(&pixels, layout, 3, 13);
        let before = bytes.clone();
        let output = capture(&request(&bytes, layout, profile.bytes(), 3, 1, row)).unwrap();
        // The stored 0/1/maximum alphas act as junk X bytes for the RGBX
        // layouts, which must ignore them.
        let opaque = layout == "rgb8"
            || layout == "rgb16_le"
            || layout == "bgra8_opaque"
            || layout == "rgba8_opaque"
            || layout == "rgba16_le_opaque"
            || layout == "argb16_be_opaque";
        let expected = pixels.map(|mut pixel| {
            if opaque {
                pixel[3] = maximum;
            }
            pixel
        });
        assert_eq!(output.samples, expected, "{layout}");
        assert_eq!(output.dimensions, (3, 1));
        assert_eq!(output.profile, profile.bytes().as_ref());
        assert_eq!(bytes, before);
    }
}

#[test]
fn premultiplied_layouts_use_independent_rational_rounding_with_zero_and_overrange() {
    let profile = IccProfile::srgb();
    for layout in [
        "rgba8_premultiplied",
        "rgba16_le_premultiplied",
        "argb16_be_premultiplied",
    ] {
        let maximum = if layout.contains("16") { 65535 } else { 255 };
        let pixels = [
            [9, maximum, 1, 0],
            [1, 0, 1, 1],
            [1, 2, 3, 6],
            [maximum, 31, 17, maximum - 1],
        ];
        let (bytes, row) = encode(&pixels, layout, 2, 5);
        let output = capture(&request(&bytes, layout, profile.bytes(), 2, 2, row)).unwrap();
        let expected: Vec<[u16; 4]> = pixels
            .into_iter()
            .map(|pixel| {
                let mut p = pixel;
                for c in 0..3 {
                    p[c] = if p[3] == 0 {
                        0
                    } else {
                        (f64::from(pixel[c]) * f64::from(maximum) / f64::from(pixel[3]))
                            .round()
                            .min(f64::from(maximum)) as u16
                    };
                }
                p
            })
            .collect();
        assert_eq!(output.samples, expected, "{layout}");
    }
}

#[test]
fn every_exif_orientation_matches_labelled_row_matrix_at_both_native_depths() {
    let profile = IccProfile::display_p3();
    let expected = ORIENTED;
    for layout in ["rgba8", "bgra8", "rgba16_le", "argb16_be"] {
        let multiplier = if layout.contains("16") { 251 } else { 1 };
        let pixels: Vec<_> = (0..6)
            .map(|i| {
                [
                    multiplier * (11 + i),
                    multiplier * (51 + i),
                    multiplier * (101 + i),
                    i,
                ]
            })
            .collect();
        let (bytes, row) = encode(&pixels, layout, 3, 9);
        for orientation in 1..=8 {
            let mut input = request(&bytes, layout, profile.bytes(), 3, 2, row);
            input.orientation = orientation;
            let result = capture(&input).unwrap();
            assert_eq!(
                result.dimensions,
                if orientation <= 4 { (3, 2) } else { (2, 3) }
            );
            assert_eq!(
                result.samples,
                expected[orientation as usize - 1]
                    .iter()
                    .map(|index| pixels[*index])
                    .collect::<Vec<_>>(),
                "{layout} orientation{orientation}"
            );
        }
    }
}

#[test]
fn native_ten_and_sixteen_bit_distinctions_do_not_collapse_to_a_byte_grid() {
    let profile = IccProfile::srgb();
    let pixels: Vec<_> = (0..1024u16)
        .map(|value| {
            [
                value,
                10001 + value,
                30003 + value,
                (value << 6) | (value >> 4),
            ]
        })
        .collect();
    let (bytes, row) = encode(&pixels, "argb16_be", 1024, 0);
    let result = capture(&request(&bytes, "argb16_be", profile.bytes(), 1024, 1, row)).unwrap();
    assert_eq!(result.samples, pixels);
    assert_eq!(
        result
            .samples
            .iter()
            .map(|pixel| pixel[3])
            .collect::<std::collections::HashSet<_>>()
            .len(),
        1024
    );
}

fn pack10(
    codes: &[[u32; 3]],
    top: &dyn Fn(usize) -> u32,
    width: usize,
    padding: usize,
) -> (Vec<u8>, usize) {
    // Independently pack one little-endian u32 per pixel: bits 31..30 carry
    // `top` (padding the reader must ignore), R 29..20, G 19..10, B 9..0.
    let mut result = Vec::new();
    for (row, chunk) in codes.chunks(width).enumerate() {
        for (column, [r, g, b]) in chunk.iter().enumerate() {
            let word = (top(row * width + column) << 30) | (r << 20) | (g << 10) | b;
            result.extend(word.to_le_bytes());
        }
        result.extend(std::iter::repeat_n(0xd7, padding));
    }
    (result, width * 4 + padding)
}
fn expected10(code: u32) -> u16 {
    (f64::from(code) * 65535.0 / 1023.0).round() as u16
}
fn packed_request<'a>(
    bytes: &'a [u8],
    profile: &'a [u8],
    width: u32,
    height: u32,
    row: usize,
) -> Request<'a> {
    let mut value = request(bytes, "rgb10_packed_le", profile, width, height, row);
    // `request` infers the owner from "16"; this layout's output depth is 16.
    value.typed = true;
    value
}

#[test]
fn packed_ten_bit_expands_every_code_on_every_channel() {
    let profile = IccProfile::display_p3();
    let codes: Vec<[u32; 3]> = (0..1024u32)
        .map(|v| [v, 1023 - v, (v + 512) % 1024])
        .collect();
    let (bytes, row) = pack10(&codes, &|_| 0, 1024, 0);
    let output = capture(&packed_request(&bytes, profile.bytes(), 1024, 1, row)).unwrap();
    let expected: Vec<[u16; 4]> = codes
        .iter()
        .map(|[r, g, b]| [expected10(*r), expected10(*g), expected10(*b), 65535])
        .collect();
    assert_eq!(output.samples, expected);
    assert_eq!(output.dimensions, (1024, 1));
    assert_eq!(output.profile, profile.bytes().as_ref());
}

#[test]
fn packed_ten_bit_ignores_the_top_two_bits() {
    let profile = IccProfile::srgb();
    let codes: Vec<[u32; 3]> = (0..12u32)
        .map(|i| [i * 85, 1023 - i * 31, (i * 397) % 1024])
        .collect();
    let (plain, row) = pack10(&codes, &|_| 0, 4, 0);
    let reference = capture(&packed_request(&plain, profile.bytes(), 4, 3, row)).unwrap();
    for top in 0..4u32 {
        let (bytes, row) = pack10(&codes, &|_| top, 4, 0);
        let output = capture(&packed_request(&bytes, profile.bytes(), 4, 3, row)).unwrap();
        assert_eq!(output, reference, "top bits {top:#04b}");
    }
    let (mixed, row) = pack10(&codes, &|i| (i as u32 * 7) % 4, 4, 3);
    let output = capture(&packed_request(&mixed, profile.bytes(), 4, 3, row)).unwrap();
    assert_eq!(output, reference);
}

#[test]
fn packed_ten_bit_row_padding_and_trimmed_final_row_are_accepted() {
    let profile = IccProfile::srgb();
    let codes: Vec<[u32; 3]> = (0..6u32).map(|i| [i * 171, i * 3, 1023 - i]).collect();
    let (plain, plain_row) = pack10(&codes, &|_| 0, 3, 0);
    let reference = capture(&packed_request(&plain, profile.bytes(), 3, 2, plain_row)).unwrap();
    let (bytes, row) = pack10(&codes, &|_| 0, 3, 5);
    for trim in 0..=5 {
        let data = &bytes[..bytes.len() - trim];
        assert_eq!(
            capture(&packed_request(data, profile.bytes(), 3, 2, row)).unwrap(),
            reference,
            "trim {trim}"
        );
    }
}

#[test]
fn packed_ten_bit_every_exif_orientation_matches_labelled_row_matrix() {
    let profile = IccProfile::display_p3();
    let codes: Vec<[u32; 3]> = (0..6u32).map(|i| [11 + i, 511 + i, 1017 + i - 6]).collect();
    let pixels: Vec<[u16; 4]> = codes
        .iter()
        .map(|[r, g, b]| [expected10(*r), expected10(*g), expected10(*b), 65535])
        .collect();
    let (bytes, row) = pack10(&codes, &|_| 3, 3, 9);
    for orientation in 1..=8 {
        let mut input = packed_request(&bytes, profile.bytes(), 3, 2, row);
        input.orientation = orientation;
        let result = capture(&input).unwrap();
        assert_eq!(
            result.dimensions,
            if orientation <= 4 { (3, 2) } else { (2, 3) }
        );
        assert_eq!(
            result.samples,
            ORIENTED[orientation as usize - 1]
                .iter()
                .map(|index| pixels[*index])
                .collect::<Vec<_>>(),
            "orientation{orientation}"
        );
    }
}

#[test]
fn packed_ten_bit_refuses_short_rows_short_buffers_extra_bytes_and_the_byte_owner() {
    let profile = IccProfile::srgb();
    let codes = [[17u32, 31, 47]; 6];
    let (bytes, row) = pack10(&codes, &|_| 0, 3, 0);
    assert_eq!(row, 12);
    assert!(capture(&packed_request(&bytes, profile.bytes(), 3, 2, row)).is_ok());
    // A row narrower than width * 4 bytes.
    let short_row = vec![0u8; 11 * 2];
    assert!(capture(&packed_request(&short_row, profile.bytes(), 3, 2, 11)).is_err());
    // One byte short of the minimum (full rows, the last one unpadded).
    assert!(capture(&packed_request(
        &bytes[..bytes.len() - 1],
        profile.bytes(),
        3,
        2,
        row
    ))
    .is_err());
    // One byte over the bound of height padded rows.
    let (padded, padded_row) = pack10(&codes, &|_| 0, 3, 4);
    assert!(capture(&packed_request(&padded, profile.bytes(), 3, 2, padded_row)).is_ok());
    let mut extra = padded.clone();
    extra.push(0);
    assert!(capture(&packed_request(&extra, profile.bytes(), 3, 2, padded_row)).is_err());
    // The byte owner accepts only genuine byte layouts.
    let mut byte_owner = packed_request(&bytes, profile.bytes(), 3, 2, row);
    byte_owner.typed = false;
    assert!(capture(&byte_owner).is_err());
}

/// Bit replication written as arithmetic: `16·c + floor(c / 256)`.
fn replicated12(code: u16) -> u16 {
    code * 16 + code / 256
}

#[test]
fn left_aligned_twelve_bit_rgb_widens_by_bit_replication_and_alpha_is_never_expanded() {
    let profile = IccProfile::display_p3();
    // Every 12-bit code on every channel, left-aligned in u16; straight alpha
    // carries low bits it must keep exactly.
    let codes: Vec<u16> = (0..4096).collect();
    let pixels: Vec<[u16; 4]> = codes
        .iter()
        .map(|&c| {
            [
                c << 4,
                (4095 - c) << 4,
                ((c + 2048) % 4096) << 4,
                c * 13 + 7,
            ]
        })
        .collect();
    for layout in ["rgba16_le_left12", "rgba16_le_opaque_left12"] {
        let (bytes, row) = encode(&pixels, layout, 256, 6);
        let output = capture(&request(&bytes, layout, profile.bytes(), 256, 16, row)).unwrap();
        let expected: Vec<[u16; 4]> = codes
            .iter()
            .map(|&c| {
                [
                    replicated12(c),
                    replicated12(4095 - c),
                    replicated12((c + 2048) % 4096),
                    if layout.contains("opaque") {
                        65535
                    } else {
                        c * 13 + 7
                    },
                ]
            })
            .collect();
        assert_eq!(output.samples, expected, "{layout}");
        assert_eq!(output.dimensions, (256, 16));
        assert_eq!(output.profile, profile.bytes().as_ref());
        assert_eq!(output.samples[4095][0], 65535, "white is full scale");
    }
}

#[test]
fn left_aligned_twelve_bit_follows_exif_orientation() {
    let profile = IccProfile::srgb();
    let pixels: Vec<[u16; 4]> = (0..6u16)
        .map(|i| {
            [
                (11 + i) << 4,
                (2047 + i) << 4,
                (4090 + i) << 4,
                1000 * i + 3,
            ]
        })
        .collect();
    let expanded: Vec<[u16; 4]> = pixels
        .iter()
        .map(|p| {
            [
                replicated12(p[0] >> 4),
                replicated12(p[1] >> 4),
                replicated12(p[2] >> 4),
                p[3],
            ]
        })
        .collect();
    let (bytes, row) = encode(&pixels, "rgba16_le_left12", 3, 9);
    for orientation in 1..=8 {
        let mut input = request(&bytes, "rgba16_le_left12", profile.bytes(), 3, 2, row);
        input.orientation = orientation;
        let result = capture(&input).unwrap();
        assert_eq!(
            result.samples,
            ORIENTED[orientation as usize - 1]
                .iter()
                .map(|index| expanded[*index])
                .collect::<Vec<_>>(),
            "orientation{orientation}"
        );
    }
}

#[test]
fn left_aligned_twelve_bit_refuses_rgb_low_bits_and_the_byte_owner() {
    let profile = IccProfile::srgb();
    let sentence = "12-bit left-aligned host samples must have their low 4 bits clear";
    for layout in ["rgba16_le_left12", "rgba16_le_opaque_left12"] {
        let clean = [[16, 32, 48, 5], [65520, 0, 4096, 65535]];
        let (bytes, row) = encode(&clean, layout, 2, 0);
        assert!(capture(&request(&bytes, layout, profile.bytes(), 2, 1, row)).is_ok());
        // Any one RGB sample of any pixel with a low bit set refuses.
        for pixel in 0..2 {
            for channel in 0..3 {
                for bit in [1u16, 8] {
                    let mut stray = clean;
                    stray[pixel][channel] |= bit;
                    let (bytes, row) = encode(&stray, layout, 2, 0);
                    let error =
                        capture(&request(&bytes, layout, profile.bytes(), 2, 1, row)).unwrap_err();
                    assert_eq!(error, sentence, "{layout} {pixel} {channel} {bit}");
                }
            }
        }
        // The byte owner accepts only genuine byte layouts.
        let mut byte_owner = request(&bytes, layout, profile.bytes(), 2, 1, row);
        byte_owner.typed = false;
        assert!(capture(&byte_owner).is_err());
    }
}

#[test]
fn premultiplied_left_aligned_twelve_bit_widens_first_and_divides_after() {
    // A premultiplied 12-bit identity-matrix AVIF, as ImageIO returns it
    // once its `prem` reference is detached. RGB is a left-aligned 12-bit
    // PREMULTIPLIED code; alpha is already full scale and keeps its low bits.
    let profile = IccProfile::display_p3();
    let layout = "rgba16_le_premultiplied_left12";
    let mut cases: Vec<([u16; 3], u16)> = vec![
        ([4095, 0, 2048], 65535),
        ([9, 4095, 1], 0),
        ([1, 2, 3], 97),
        ([4095, 31, 17], 65519),
        ([2047, 2048, 1000], 32768),
        ([4095, 4095, 4095], 1),
    ];
    for code in (0..4096u16).step_by(37) {
        cases.push(([code, 4095 - code, code / 2], code * 16 + 7));
    }
    while !cases.len().is_multiple_of(4) {
        cases.push(([0, 0, 0], 0));
    }
    let pixels: Vec<[u16; 4]> = cases
        .iter()
        .map(|(codes, alpha)| [codes[0] << 4, codes[1] << 4, codes[2] << 4, *alpha])
        .collect();
    let (bytes, row) = encode(&pixels, layout, 4, 6);
    let height = (pixels.len() / 4) as u32;
    let output = capture(&request(&bytes, layout, profile.bytes(), 4, height, row)).unwrap();
    // Independent rational form: widen the code to full scale, then one
    // rounded division by alpha, clamped; alpha 0 hides the colour.
    let expected: Vec<[u16; 4]> = cases
        .iter()
        .map(|(codes, alpha)| {
            let straight = |code: u16| {
                if *alpha == 0 {
                    0
                } else {
                    (f64::from(replicated12(code)) * 65535.0 / f64::from(*alpha))
                        .round()
                        .min(65535.0) as u16
                }
            };
            [
                straight(codes[0]),
                straight(codes[1]),
                straight(codes[2]),
                *alpha,
            ]
        })
        .collect();
    assert_eq!(output.samples, expected);
    assert_eq!(output.samples[0], [65535, 0, replicated12(2048), 65535]);
    assert_eq!(output.samples[1], [0, 0, 0, 0]);
    assert_eq!(output.profile, profile.bytes().as_ref());

    // The low-bit check comes before the division, whatever the alpha.
    let sentence = "12-bit left-aligned host samples must have their low 4 bits clear";
    for alpha in [0u16, 97, 65535] {
        for channel in 0..3 {
            let mut stray = [[16u16, 32, 48, alpha]];
            stray[0][channel] |= 4;
            let (bytes, row) = encode(&stray, layout, 1, 0);
            let error = capture(&request(&bytes, layout, profile.bytes(), 1, 1, row)).unwrap_err();
            assert_eq!(error, sentence, "{alpha} {channel}");
        }
    }
    // The byte owner accepts only genuine byte layouts.
    let mut byte_owner = request(&bytes, layout, profile.bytes(), 4, height, row);
    byte_owner.typed = false;
    assert!(capture(&byte_owner).is_err());
}

#[test]
fn final_row_padding_is_optional_but_missing_pixels_or_extra_bytes_refuse() {
    let profile = IccProfile::srgb();
    for layout in ["rgba8", "rgba16_le"] {
        let pixels = [[17, 31, 47, 53]; 6];
        let (bytes, row) = encode(&pixels, layout, 3, 11);
        let complete = capture(&request(&bytes, layout, profile.bytes(), 3, 2, row)).unwrap();
        for trim in 0..=11 {
            let data = &bytes[..bytes.len() - trim];
            assert_eq!(
                capture(&request(data, layout, profile.bytes(), 3, 2, row)).unwrap(),
                complete
            );
        }
        assert!(capture(&request(
            &bytes[..bytes.len() - 12],
            layout,
            profile.bytes(),
            3,
            2,
            row
        ))
        .is_err());
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(capture(&request(&extra, layout, profile.bytes(), 3, 2, row)).is_err());
    }
}

#[test]
fn invalid_dimensions_orientation_layout_profile_and_wrong_depth_refuse() {
    let profile = IccProfile::srgb();
    let bytes = [17u8; 32];
    for (width, height, row) in [
        (0, 2, 16),
        (2, 0, 16),
        (2, 2, 7),
        (u32::MAX, 2, usize::MAX),
        (2, 2, usize::MAX),
    ] {
        assert!(capture(&request(
            &bytes,
            "rgba8",
            profile.bytes(),
            width,
            height,
            row
        ))
        .is_err());
    }
    for layout in [
        "",
        "RGBA8",
        "rgba16_be",
        "rgba32_float",
        "yuv420",
        "unknown",
    ] {
        assert!(capture(&request(&bytes, layout, profile.bytes(), 2, 2, 16)).is_err());
    }
    for orientation in [0, 9, u32::MAX] {
        let mut value = request(&bytes, "rgba8", profile.bytes(), 2, 2, 16);
        value.orientation = orientation;
        assert!(capture(&value).is_err());
    }
    for invalid in [
        &[][..],
        &[0u8; 128][..],
        &profile.bytes()[..profile.bytes().len() - 1],
    ] {
        assert!(capture(&request(&bytes, "rgba8", invalid, 2, 2, 16)).is_err());
    }
    for layout in ["rgba8", "rgba16_le"] {
        let mut value = request(&bytes, layout, profile.bytes(), 2, 2, 16);
        value.typed = !value.typed;
        assert!(capture(&value).is_err());
    }
}

/// One 16-bit image holds the document's pixel limit, so the 8193² source
/// the old per-image cap refused ("…67 megapixels…") is
/// no longer refused for its size — this request fails on its 32-byte buffer
/// instead — and a source past the pixel limit refuses in the pixel-limit
/// sentence, never one that names the decoder.
#[test]
fn a_67_mp_source_is_not_too_large_and_one_past_the_pixel_limit_refuses() {
    let profile = IccProfile::srgb();
    let error = capture(&request(
        &[0u8; 32],
        "rgba16_le",
        profile.bytes(),
        8193,
        8193,
        8193 * 8,
    ))
    .unwrap_err();
    assert!(
        error.contains("host row padding and buffer length disagree"),
        "{error}"
    );
    assert!(!error.contains("too large"), "{error}");
    let error = capture(&request(
        &[0u8; 32],
        "rgba16_le",
        profile.bytes(),
        10001,
        10000,
        10001 * 8,
    ))
    .unwrap_err();
    assert!(
        error.contains("dimensions exceed the supported pixel limit"),
        "{error}"
    );
    assert!(!error.contains("decoder"), "{error}");
}

#[test]
fn admission_and_precancel_refuse_without_changing_caller_data_or_leaking_scratch() {
    let profile = IccProfile::display_p3();
    let bytes = [31u8; 32];
    for layout in ["rgba8", "rgba16_le"] {
        for (limit, other) in [(1, 0), (LIMIT, LIMIT), (LIMIT, u64::MAX)] {
            let mut value = request(&bytes, layout, profile.bytes(), 2, 2, 16);
            value.limit = limit;
            value.other = other;
            assert!(capture(&value).is_err());
        }
        let control = DrJobControl::default();
        control.cancel();
        let mut value = request(&bytes, layout, profile.bytes(), 2, 2, 16);
        value.control = Some(&control);
        assert!(capture(&value).unwrap_err().contains("cancel"));
        assert_eq!(control.scratch_bytes(), 0);
        assert_eq!(bytes, [31u8; 32]);
    }
}

#[test]
fn successful_tokens_complete_release_scratch_and_cannot_be_reused() {
    let profile = IccProfile::srgb();
    let bytes = [31u8; 32];
    for layout in ["rgba8", "rgba16_le"] {
        let token = DrJobControl::default();
        let mut value = request(&bytes, layout, profile.bytes(), 2, 2, 16);
        value.control = Some(&token);
        capture(&value).unwrap();
        assert_eq!(token.progress(), 1.0);
        assert_eq!(token.scratch_bytes(), 0);
        assert!(capture(&value).is_err());
    }
}

#[test]
fn cancellation_after_admission_never_returns_a_partially_packed_document() {
    let profile = IccProfile::srgb();
    let (width, height) = (4096, 2048);
    let bytes = vec![42u8; width * height * 8];
    let render = || {
        let (bytes, profile) = (&bytes, &profile);
        move |token: usize| {
            let mut value = request(
                bytes,
                "argb16_be",
                profile.bytes(),
                width as u32,
                height as u32,
                width * 8,
            );
            value.orientation = 6;
            value.control = Some(unsafe { &*(token as *const DrJobControl) });
            match capture(&value) {
                Ok(_) => (true, String::new()),
                Err(e) => (false, e),
            }
        }
    };
    // Park at the packing's last checkpoint before its closing report (a
    // fresh twin's log), with the reservation live.
    let f = entry_count(&|_, stage, _| stage == 5, render());
    let parked = cancelled_parked_at(f - 1, &|_, stage| stage == 1, render());
    eprintln!("{}", parked_line(&parked));
    assert!(
        parked.scratch > 0,
        "packing completed before the live cancellation checkpoint"
    );
    assert!(bytes.iter().all(|byte| *byte == 42));
}

#[test]
fn both_exports_tolerate_each_required_null_argument_and_null_error_output() {
    let bytes = [17u8; 32];
    let profile = IccProfile::srgb();
    for typed in [false, true] {
        let layout = CString::new(if typed { "rgba16_le" } else { "rgba8" }).unwrap();
        for missing in 0..3 {
            let mut error = ptr::null_mut();
            let data = if missing == 0 {
                ptr::null()
            } else {
                bytes.as_ptr()
            };
            let name = if missing == 1 {
                ptr::null()
            } else {
                layout.as_ptr()
            };
            let icc = if missing == 2 {
                ptr::null()
            } else {
                profile.bytes().as_ptr()
            };
            let refused = unsafe {
                if typed {
                    dr_precision_doc_from_host_pixels(
                        data,
                        bytes.len(),
                        2,
                        2,
                        16,
                        name,
                        1,
                        icc,
                        profile.bytes().len(),
                        LIMIT,
                        0,
                        ptr::null(),
                        &mut error,
                    )
                    .is_null()
                } else {
                    dr_doc_from_host_pixels(
                        data,
                        bytes.len(),
                        2,
                        2,
                        16,
                        name,
                        1,
                        icc,
                        profile.bytes().len(),
                        LIMIT,
                        0,
                        ptr::null(),
                        &mut error,
                    )
                    .is_null()
                }
            };
            assert!(refused);
            assert!(!unsafe { error_string(error) }.is_empty());
        }
        unsafe {
            if typed {
                assert!(dr_precision_doc_from_host_pixels(
                    ptr::null(),
                    0,
                    0,
                    0,
                    0,
                    ptr::null(),
                    0,
                    ptr::null(),
                    0,
                    LIMIT,
                    0,
                    ptr::null(),
                    ptr::null_mut()
                )
                .is_null());
            } else {
                assert!(dr_doc_from_host_pixels(
                    ptr::null(),
                    0,
                    0,
                    0,
                    0,
                    ptr::null(),
                    0,
                    ptr::null(),
                    0,
                    LIMIT,
                    0,
                    ptr::null(),
                    ptr::null_mut()
                )
                .is_null());
            }
        }
    }
}
