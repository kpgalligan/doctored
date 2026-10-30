//! #547 Bitmap phase 1b's methods through the FFI, each against a hand
//! oracle written from its declared rule (`bitmap_method`'s module doc), not
//! from the core: Pattern Dither against phase 2b's fitted table (PT8-MID,
//! written here from the fit's record) and each entry's own boundary, one Halftone
//! Screen cell worked by hand and a rotated screen of each of the six shapes
//! against a brute-force count of the spot grid (phase 1c), Custom Pattern
//! over a preset and an inline tile,
//! every new option refusal by sentence, and the to-Bitmap plan's resample
//! term (the larger of the geometry plan and the per-pixel shape, 1a's
//! review finding).

use std::ptr;

use doctored_core::ffi_precision_metadata::dr_precision_doc_set_resolution;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::bitmap_mode::{bits, gray_doc, plan, to_bitmap};
use crate::common::take_err_string;
use crate::indexed_fixture::{descriptor, edit_plan, free};

/// Phase 2b's Pattern Dither table, PT8-MID (`bitmap_take.py`'s fit1:
/// `⌊(lo + hi)/2⌋` of each position's interval in phase 1b's smoke, rows
/// `y mod 8`), PASS on take1: a pixel is white iff its gray is at least the
/// entry. Written here from the fit's record, not from the core.
const PT8_MID: [[u16; 8]; 8] = [
    [252, 124, 222, 95, 240, 121, 210, 83],
    [124, 221, 94, 247, 120, 209, 90, 59],
    [220, 93, 246, 119, 208, 89, 66, 187],
    [92, 245, 118, 215, 88, 65, 186, 35],
    [244, 117, 214, 87, 64, 185, 34, 155],
    [116, 213, 86, 63, 192, 33, 154, 51],
    [212, 85, 62, 191, 32, 153, 58, 179],
    [84, 61, 190, 31, 160, 57, 178, 27],
];

/// The output pixel's shape and bookkeeping (`bitmap_convert`'s
/// `TO_BITMAP_BYTES_PER_PIXEL`, `precision_image`'s `BOOKKEEPING_BYTES`).
const PER_PIXEL: u64 = 41;
const BOOKKEEPING: u64 = 1024 * 1024;

/// An 8-bit Grayscale document of `codes` at 72 × 72 ppi (the setter
/// answers NULL, no error, when the PNG already says 72).
pub(crate) fn doc72(width: u32, height: u32, codes: &[u8]) -> *mut PrecisionDocument {
    let gray = gray_doc(width, height, codes);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_set_resolution(gray, 72.0, 72.0, &mut err) };
    if doc.is_null() {
        assert!(err.is_null(), "{}", take_err_string(err));
        assert_eq!(
            descriptor(gray)["resolution"],
            json!({"x_ppi":72.0,"y_ppi":72.0})
        );
        return gray;
    }
    free(gray);
    doc
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                T[((n >> (18 - 6 * i)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

/// `options` converted, as bits.
fn converted(doc: *const PrecisionDocument, options: &Value) -> Vec<u8> {
    let bitmap = to_bitmap(doc, options).unwrap();
    let out = bits(bitmap);
    free(bitmap);
    out
}

#[test]
fn pattern_dither_is_the_fitted_threshold_table_on_a_ramp() {
    let codes: Vec<u8> = (0..256u32).map(|i| i as u8).collect();
    let doc = doc72(16, 16, &codes);
    let expected: Vec<u8> = (0..256usize)
        .map(|i| {
            let (x, y, g) = (i % 16, i / 16, u16::from(codes[i]));
            u8::from(g >= PT8_MID[y % 8][x % 8])
        })
        .collect();
    assert_eq!(converted(doc, &json!({"method":"pattern"})), expected);
    // Each entry is its own boundary: one gray below it is black there.
    let at: Vec<u8> = PT8_MID.iter().flatten().map(|&t| t as u8).collect();
    let below: Vec<u8> = at.iter().map(|&t| t - 1).collect();
    let on = doc72(8, 8, &at);
    let under = doc72(8, 8, &below);
    assert!(converted(on, &json!({"method":"pattern"}))
        .iter()
        .all(|&b| b == 1));
    assert!(converted(under, &json!({"method":"pattern"}))
        .iter()
        .all(|&b| b == 0));
    free(on);
    free(under);
    // The extremes: 0 is all black, 255 all white.
    let flat = doc72(8, 8, &[0; 64]);
    assert!(converted(flat, &json!({"method":"pattern"}))
        .iter()
        .all(|&b| b == 0));
    let white = doc72(8, 8, &[255; 64]);
    assert!(converted(white, &json!({"method":"pattern"}))
        .iter()
        .all(|&b| b == 1));
    for d in [white, flat, doc] {
        free(d);
    }
}

/// The shipped halftone rule on one flat 8 × 4 field at 18 lpi, 0°, Square,
/// 72 ppi: 4-pixel cells, |s|, |t| ∈ {0.75, 0.25, 0.25, 0.75} across each
/// (cells at the corner). Under AREA the threshold is `0.5 + 254·F`, F the
/// share of the 256² grid (|S| ≤ 0.25 holds 64 of 256 samples, ≤ 0.75 holds
/// 192) whose spot `−max(|s|,|t|)` is lower: at the cell's centre 2 × 2
/// (max 0.25) F = 1 − 64²/256² = 0.9375, T = 238.625; on the ring (max 0.75)
/// F = 1 − 192²/256² = 0.4375, T = 111.625.
#[test]
fn one_halftone_cell_by_hand() {
    let square = json!({"method":"halftone","frequency":18,"angle":0,"shape":"square"});
    let centre = |x: usize, y: usize| matches!(x % 4, 1 | 2) && matches!(y % 4, 1 | 2);
    for (g, centre_white, ring_white) in [
        (111u8, false, false),
        (112, false, true),
        (200, false, true),
        (238, false, true),
        (239, true, true),
    ] {
        let doc = doc72(8, 4, &[g; 32]);
        let expected: Vec<u8> = (0..32usize)
            .map(|i| {
                let (x, y) = (i % 8, i / 8);
                u8::from(if centre(x, y) {
                    centre_white
                } else {
                    ring_white
                })
            })
            .collect();
        assert_eq!(converted(doc, &square), expected, "gray {g}");
        free(doc);
    }
    // The screen is laid over the OUTPUT resolution: the same field at 144
    // ppi out and 36 lpi is the same 4-pixel cell on a 16 × 8 canvas.
    let doc = doc72(8, 4, &[200; 32]);
    let fine = json!({"method":"halftone","frequency":36,"angle":0,"shape":"square",
                      "resolution":144});
    let expected: Vec<u8> = (0..128usize)
        .map(|i| u8::from(!centre(i % 16, i / 16)))
        .collect();
    assert_eq!(converted(doc, &fine), expected);
    free(doc);
}

/// Adobe's spot functions as the module doc declares them (a = |s|, b = |t|),
/// each read off its PostScript procedure in ISO 32000-1:2008 Table 128
/// (operands x = s, y = t; `abs exch abs` leaves b below a):
///
/// * Round (SimpleDot inside the diamond):
///   `{abs exch abs 2 copy add 1 le {dup mul exch dup mul add 1 exch sub}
///   {1 sub dup mul exch 1 sub dup mul add 1 sub} ifelse}`.
/// * Ellipse (the 0.75-aspect ellipse, EllipseC's): `{abs exch abs 2 copy
///   3 mul exch 4 mul add 3 sub dup 0 lt {pop dup mul exch .75 div dup mul
///   add 4 div 1 exch sub} {dup 1 gt {pop 1 exch sub dup mul exch 1 exch
///   sub .75 div dup mul add 4 div 1 sub} {.5 exch sub exch pop exch pop}
///   ifelse} ifelse}`, so w = 3a + 4b − 3.
/// * Line: `{exch pop abs neg}`.
/// * Square: `{abs exch abs 2 copy lt {exch} if pop neg}`, −max(a, b).
/// * Cross: `{abs exch abs 2 copy gt {exch} if pop neg}`, −min(a, b).
/// * Diamond: `{abs exch abs 2 copy add .75 le {dup mul exch dup mul add 1
///   exch sub} {2 copy add 1.23 le {.85 mul add 1 exch sub} {1 sub dup mul
///   exch 1 sub dup mul add 1 sub} ifelse} ifelse}`.
fn spot(shape: &str, s: f64, t: f64) -> f64 {
    let (a, b) = (s.abs(), t.abs());
    let corner = (a - 1.0).powi(2) + (b - 1.0).powi(2) - 1.0;
    match shape {
        "round" if a + b <= 1.0 => 1.0 - (a * a + b * b),
        "round" => corner,
        "ellipse" => {
            let w = 3.0 * a + 4.0 * b - 3.0;
            if w < 0.0 {
                1.0 - (a.powi(2) + (b / 0.75).powi(2)) / 4.0
            } else if w > 1.0 {
                ((1.0 - a).powi(2) + ((1.0 - b) / 0.75).powi(2)) / 4.0 - 1.0
            } else {
                0.5 - w
            }
        }
        "line" => -b,
        "square" => -(if a < b { b } else { a }),
        "cross" => -(if a > b { b } else { a }),
        "diamond" if a + b <= 0.75 => 1.0 - (a * a + b * b),
        "diamond" if a + b <= 1.23 => 1.0 - (0.85 * a + b),
        "diamond" => corner,
        other => panic!("no oracle for {other}"),
    }
}

/// The shipped halftone rule (AREA, counterclockwise, cells at the corner)
/// at pixel (x, y), counting the grid by brute force rather than sorting.
fn halftone_white(g: u8, x: u32, y: u32, frequency: f64, angle: f64, shape: &str) -> bool {
    let phi = -angle * std::f64::consts::PI / 180.0;
    let (xx, yy) = ((f64::from(x) + 0.5) / 72.0, (f64::from(y) + 0.5) / 72.0);
    let u = frequency * (xx * phi.cos() + yy * phi.sin());
    let v = frequency * (-xx * phi.sin() + yy * phi.cos());
    let (s, t) = (2.0 * (u - u.floor()) - 1.0, 2.0 * (v - v.floor()) - 1.0);
    let here = spot(shape, s, t);
    let sample = |k: u32| (f64::from(k) + 0.5) / 128.0 - 1.0;
    let mut below = 0u32;
    for i in 0..256 {
        for j in 0..256 {
            below += u32::from(spot(shape, sample(i), sample(j)) < here);
        }
    }
    f64::from(g) > 0.5 + 254.0 * f64::from(below) / 65536.0
}

#[test]
fn a_rotated_screen_matches_the_declared_rule_on_a_ramp() {
    let (w, h) = (20u32, 12u32);
    let codes: Vec<u8> = (0..w * h)
        .map(|i| ((i % w) * 12 + (i / w) * 2) as u8)
        .collect();
    let doc = doc72(w, h, &codes);
    // Every shape, each at its own angle.
    for (frequency, angle, shape) in [
        (12.0, 45.0, "round"),
        (12.0, 30.0, "line"),
        (12.0, 15.0, "ellipse"),
        (12.0, 60.0, "square"),
        (12.0, -20.0, "cross"),
        (12.0, 75.0, "diamond"),
    ] {
        let expected: Vec<u8> = (0..w * h)
            .map(|i| {
                u8::from(halftone_white(
                    codes[i as usize],
                    i % w,
                    i / w,
                    frequency,
                    angle,
                    shape,
                ))
            })
            .collect();
        let options = json!({"method":"halftone","frequency":frequency,"angle":angle,
                             "shape":shape});
        assert_eq!(converted(doc, &options), expected, "{shape} {angle}°");
    }
    // The defaults: 53 lpi, 45°, Round.
    assert_eq!(
        converted(doc, &json!({"method":"halftone"})),
        converted(
            doc,
            &json!({"method":"halftone","frequency":53,"angle":45,"shape":"round"})
        )
    );
    free(doc);
}

#[test]
fn custom_pattern_tiles_the_patterns_gray_as_the_threshold() {
    // Stripes: 8 × 8, black for x < 4 (threshold 0), else white (255).
    let codes: Vec<u8> = (0..32u32)
        .map(|i| if i < 16 { i * 17 } else { 255 - (i - 16) * 17 } as u8)
        .collect();
    let doc = doc72(16, 2, &codes);
    let expected: Vec<u8> = (0..32usize)
        .map(|i| u8::from(i % 16 % 8 < 4 && codes[i] > 0))
        .collect();
    assert_eq!(
        converted(
            doc,
            &json!({"method":"custom","pattern":{"preset":"stripes"}})
        ),
        expected
    );
    free(doc);

    // An inline 2 × 2 tile of grays 64, 128 / 192, 255, tiled from the
    // origin: white iff g > its texel's gray.
    let tile = [64u8, 128, 192, 255];
    let rgba: Vec<u8> = tile.iter().flat_map(|&v| [v, v, v, 255]).collect();
    let codes: Vec<u8> = (0..32u32)
        .map(|i| ((i % 8) * 32 + (i / 8) * 8) as u8)
        .collect();
    let doc = doc72(8, 4, &codes);
    let expected: Vec<u8> = (0..32usize)
        .map(|i| {
            let (x, y) = (i % 8, i / 8);
            u8::from(codes[i] > tile[(y % 2) * 2 + x % 2])
        })
        .collect();
    let inline = json!({"method":"custom","pattern":{"name":"T","width":2,"height":2,
                                                     "rgba":base64(&rgba)}});
    assert_eq!(converted(doc, &inline), expected);
    // The default pattern is the checkerboard.
    assert_eq!(
        converted(doc, &json!({"method":"custom"})),
        converted(
            doc,
            &json!({"method":"custom","pattern":{"preset":"checkerboard"}})
        )
    );
    free(doc);
}

#[test]
fn every_new_option_refuses_by_sentence() {
    let doc = doc72(2, 2, &[0, 255, 255, 0]);
    let presets = "pattern.preset must be one of checkerboard, stripes, dots, grid";
    for (options, sentence) in [
        (
            json!({"method":"threshold","frequency":53}),
            "mode option frequency applies to method halftone",
        ),
        (
            json!({"angle":45}),
            "mode option angle applies to method halftone",
        ),
        (
            json!({"method":"custom","shape":"round"}),
            "mode option shape applies to method halftone",
        ),
        (
            json!({"method":"halftone","pattern":{"preset":"stripes"}}),
            "mode option pattern applies to method custom",
        ),
        (
            json!({"method":"halftone","frequency":0.5}),
            "mode option frequency must be 1 to 999.999 lines per inch",
        ),
        (
            json!({"method":"halftone","frequency":1000}),
            "mode option frequency must be 1 to 999.999 lines per inch",
        ),
        (
            json!({"method":"halftone","frequency":"53"}),
            "mode option frequency must be 1 to 999.999 lines per inch",
        ),
        (
            json!({"method":"halftone","angle":180.5}),
            "mode option angle must be -180 to 180 degrees",
        ),
        (
            json!({"method":"halftone","angle":-181}),
            "mode option angle must be -180 to 180 degrees",
        ),
        (
            json!({"method":"halftone","shape":"oval"}),
            "mode option shape must be round, ellipse, line, square, cross or diamond",
        ),
        (
            json!({"method":"halftone","shape":3}),
            "mode option shape must be round, ellipse, line, square, cross or diamond",
        ),
        (
            json!({"method":"custom","pattern":{"preset":"nope"}}),
            presets,
        ),
        (
            json!({"method":"custom","pattern":5}),
            "pattern must be an object",
        ),
        (
            json!({"method":"custom","pattern":{"width":2,"height":2,"rgba":"AAAA"}}),
            "pattern.rgba must be base64 of width x height x 4 bytes",
        ),
        (
            json!({"method":"dots"}),
            "mode option method must be threshold, diffusion, pattern, halftone or custom",
        ),
    ] {
        assert_eq!(to_bitmap(doc, &options).unwrap_err(), sentence, "{options}");
        assert_eq!(plan(doc, &options).unwrap_err(), sentence, "{options}");
    }
    // The bounds themselves are admitted.
    for options in [
        json!({"method":"halftone","frequency":1,"angle":-180}),
        json!({"method":"halftone","frequency":999.999,"angle":180}),
    ] {
        assert!(to_bitmap(doc, &options).is_ok(), "{options}");
    }
    free(doc);
}

#[test]
fn the_resampling_plan_charges_the_larger_of_the_resample_and_the_pixels() {
    let doc = doc72(40, 30, &[90; 1200]);
    let options = json!({"method":"threshold","resolution":144});
    let p = plan(doc, &options).unwrap();
    assert_eq!(
        (p["width"].clone(), p["height"].clone()),
        (json!(80), json!(60))
    );
    let geometry = edit_plan(
        doc,
        &json!({"op":"resize","width":80,"height":60,"filter":"bicubic"}),
    )
    .unwrap()["scratch_bytes"]
        .as_u64()
        .unwrap();
    let pixels = 80 * 60 * PER_PIXEL;
    assert_eq!(
        p["working_memory_plan_bytes"].as_u64().unwrap(),
        geometry.max(pixels) + BOOKKEEPING,
        "geometry {geometry}, pixels {pixels}"
    );
    // No resample: the pixels alone; Halftone adds its sorted 256² grid of
    // f64, Custom its tile (stripes, 8 × 8).
    for (options, method) in [
        (json!({"method":"threshold"}), 0u64),
        (json!({"method":"pattern"}), 0),
        (json!({"method":"halftone"}), 256 * 256 * 8),
        (
            json!({"method":"custom","pattern":{"preset":"stripes"}}),
            64,
        ),
    ] {
        let p = plan(doc, &options).unwrap();
        assert_eq!(
            p["working_memory_plan_bytes"].as_u64().unwrap(),
            40 * 30 * PER_PIXEL + method + BOOKKEEPING,
            "{options}"
        );
    }
    free(doc);
}
