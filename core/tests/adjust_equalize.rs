//! Image ▸ Adjustments ▸ Equalize (#547) at 8, 16 and 32 bits on RGB,
//! through the FFI, against an independent f64 histogram equalization
//! written here: one table from the summed R, G and B histograms, each
//! sample weighted by selection coverage × alpha, `lut[v] = round(max ·
//! (cdf(v) − cdf(v_min)) / (N − cdf(v_min)))`.
//!
//! * 8 bits (`dr_image_equalize`): the table over every pixel, a
//!   partial-coverage mask weighting the histogram only (no lerp), a mask of
//!   another length refused, an identity answered NULL.
//! * 16 bits (the selected edit `equalize`): no selection, `selected` (the
//!   write lerped by coverage, outside untouched) and `entire` (outside
//!   rewritten), on an offset layer.
//! * 32 bits: the table on the display encoding E (here the sRGB curve,
//!   written here, within a few codes of the document's own E), samples
//!   above white and below zero counted at the ends and kept bit for bit.
//! * A group and an adjustment layer refuse by name.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba};
use doctored_core::ffi_equalize::dr_image_equalize;
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::float_fixture::float_doc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// The oracle table over `levels` levels from `(level, weight)` samples.
fn table(samples: impl Iterator<Item = (usize, f64)>, levels: usize) -> Option<Vec<f64>> {
    let mut bins = vec![0.0f64; levels];
    for (level, weight) in samples {
        if weight > 0.0 {
            bins[level.min(levels - 1)] += weight;
        }
    }
    let first = bins.iter().position(|&w| w > 0.0)?;
    let last = bins.iter().rposition(|&w| w > 0.0)?;
    if first == last {
        return None;
    }
    let total: f64 = bins.iter().sum();
    let floor = bins[first];
    let max = (levels - 1) as f64;
    let mut cdf = 0.0;
    Some(
        bins.iter()
            .map(|&w| {
                cdf += w;
                (max * ((cdf - floor) / (total - floor)).clamp(0.0, 1.0)).round()
            })
            .collect(),
    )
}

/// The refusal sentence of a call that must refuse.
fn refusal<T>(result: Result<T, String>) -> String {
    match result {
        Err(sentence) => sentence,
        Ok(_) => panic!("the call was not refused"),
    }
}

fn hash(i: u32, salt: u32) -> u32 {
    i.wrapping_mul(2_654_435_761)
        .wrapping_add(salt.wrapping_mul(40_503))
        .rotate_left(7)
}

// ------------------------------------------------------------------ 8 bits --

fn byte_image(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let j = hash(i, 3);
            // A low-key picture, so the table spreads it.
            let v = |s: u32| (40 + (j >> s) % 90) as u8;
            let alpha = match i % 4 {
                0 => 0,
                1 => 128,
                _ => 255,
            };
            [v(0), v(8), v(16), alpha]
        })
        .collect()
}

fn equalize(
    bytes: &[u8],
    (w, h): (u32, u32),
    mask: Option<&[u8]>,
) -> Result<Option<Vec<u8>>, String> {
    let image = unsafe { dr_image_from_rgba(bytes.as_ptr(), w, h) };
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_image_equalize(
            image,
            mask.map_or(ptr::null(), |m| m.as_ptr()),
            mask.map_or(0, |m| m.len()),
            &mut err,
        )
    };
    unsafe { dr_image_free(image) };
    if !err.is_null() {
        return Err(take_err_string(err));
    }
    if out.is_null() {
        return Ok(None);
    }
    let pixels =
        unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(out), (w * h * 4) as usize) }
            .to_vec();
    unsafe { dr_image_free(out) };
    Ok(Some(pixels))
}

#[test]
fn eight_bit_equalize_is_the_summed_table_over_every_pixel() {
    let (w, h) = (23u32, 17u32);
    let bytes = byte_image(w, h);
    let bytes = &bytes[..];
    let n = (w * h) as usize;
    let mask: Vec<u8> = (0..n).map(|i| (hash(i as u32, 9) % 256) as u8).collect();
    for weight in [None, Some(&mask[..])] {
        let lut = table(
            (0..n).flat_map(|i| {
                let k = f64::from(weight.map_or(255, |m| m[i])) / 255.0
                    * f64::from(bytes[i * 4 + 3])
                    / 255.0;
                (0..3).map(move |c| (usize::from(bytes[i * 4 + c]), k))
            }),
            256,
        )
        .unwrap();
        let got = equalize(bytes, (w, h), weight).unwrap().unwrap();
        for i in 0..n {
            for c in 0..4 {
                let want = if c == 3 || bytes[i * 4 + 3] == 0 {
                    bytes[i * 4 + c]
                } else {
                    lut[usize::from(bytes[i * 4 + c])] as u8
                };
                assert_eq!(
                    got[i * 4 + c],
                    want,
                    "pixel {i} channel {c} (mask {})",
                    weight.is_some()
                );
            }
        }
    }
    // The mask weighs only: a pixel at mask 0 is rewritten by the table too.
    let zero = mask.iter().position(|&m| m == 0).unwrap_or(0);
    let masked = equalize(bytes, (w, h), Some(&mask)).unwrap().unwrap();
    assert!(
        bytes[zero * 4 + 3] == 0 || masked[zero * 4..zero * 4 + 3] != bytes[zero * 4..zero * 4 + 3]
    );
    // Two levels only: the table maps them to the ends, which they are.
    let flat: Vec<u8> = (0..n)
        .flat_map(|i| {
            if i % 2 == 0 {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            }
        })
        .collect();
    assert_eq!(equalize(&flat, (w, h), None).unwrap(), None);
    assert_eq!(
        refusal(equalize(bytes, (w, h), Some(&mask[..n - 1]))),
        "equalize mask length differs from the image"
    );
    let mut err = ptr::null_mut();
    assert!(unsafe { dr_image_equalize(ptr::null(), ptr::null(), 0, &mut err) }.is_null());
    assert!(!err.is_null());
    let _ = take_err_string(err);
}

// ------------------------------------------------------------ 16 and 32 --

const CW: u32 = 20;
const CH: u32 = 15;

fn selected(
    doc: &PrecisionDocument,
    request: &Value,
    coverage: &[u16],
) -> Result<Option<PrecisionDocument>, String> {
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
    if !err.is_null() {
        return Err(take_err_string(err));
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

/// A left-half selection with a ramp edge.
fn selection() -> Vec<u16> {
    (0..CW * CH)
        .map(|i| match i % CW {
            x if x < 8 => 65535,
            x if x < 12 => (65535 * (12 - x) / 5) as u16,
            _ => 0,
        })
        .collect()
}

/// Canvas coverage at a layer pixel (the placement rule).
fn coverage_at(coverage: &[u16], (w, _): (u32, u32), (ox, oy): (i32, i32), i: usize) -> f64 {
    if coverage.is_empty() {
        return 1.0;
    }
    let (x, y) = (i as i32 % w as i32 + ox, i as i32 / w as i32 + oy);
    if x < 0 || y < 0 || x >= CW as i32 || y >= CH as i32 {
        return 0.0;
    }
    f64::from(coverage[(y as u32 * CW + x as u32) as usize]) / 65535.0
}

#[test]
fn sixteen_bit_equalize_matches_the_oracle_in_each_scope() {
    let (w, h, offset) = (14u32, 11u32, (4i32, 3i32));
    let pixels: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let j = hash(i, 5);
            let v = |s: u32| (20000 + (j >> s) % 9000) as u16;
            [
                v(0),
                v(5),
                v(11),
                if i % 5 == 0 {
                    0
                } else if i % 5 == 1 {
                    30000
                } else {
                    65535
                },
            ]
        })
        .collect();
    let image =
        PrecisionImage::from_rgba16(w, h, pixels.clone(), IccProfile::srgb(), None).unwrap();
    let doc = PrecisionDocument::from_parts(
        CW,
        CH,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![PrecisionDocumentLayer::raster("L", image, offset, 1.0, true).unwrap()],
    )
    .unwrap();
    let sel = selection();
    for (coverage, scope) in [
        (Vec::new(), "selected"),
        (sel.clone(), "selected"),
        (sel.clone(), "entire"),
    ] {
        let request = json!({"layer":0,"op":"equalize","params":{"scope":scope}});
        let out = selected(&doc, &request, &coverage).unwrap().unwrap();
        let got = out.layer(0).unwrap().source().unwrap().to_rgba16().unwrap();
        let lut = table(
            pixels.iter().enumerate().flat_map(|(i, p)| {
                let k = coverage_at(&coverage, (w, h), offset, i) * f64::from(p[3]) / 65535.0;
                (0..3).map(move |c| (usize::from(p[c]), k))
            }),
            65536,
        )
        .unwrap();
        for (i, p) in pixels.iter().enumerate() {
            let t = if scope == "entire" {
                1.0
            } else {
                coverage_at(&coverage, (w, h), offset, i)
            };
            for c in 0..4 {
                let want = if c == 3 || p[3] == 0 || t == 0.0 {
                    f64::from(p[c])
                } else {
                    (f64::from(p[c]) + t * (lut[usize::from(p[c])] - f64::from(p[c]))).round()
                };
                assert!(
                    (f64::from(got[i][c]) - want).abs() <= 1.0,
                    "{scope} pixel {i} channel {c}: {} vs {want}",
                    got[i][c]
                );
            }
        }
    }
    // Identity: two levels at the ends.
    let ends: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            if i % 2 == 0 {
                [0, 0, 0, 65535]
            } else {
                [65535; 4]
            }
        })
        .collect();
    let flat = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, ends, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    assert!(selected(&flat, &json!({"layer":0,"op":"equalize"}), &[])
        .unwrap()
        .is_none());
    assert_eq!(
        refusal(selected(
            &flat,
            &json!({"layer":0,"op":"equalize","params":{"scope":"all"}}),
            &[]
        )),
        "equalize scope must be selected or entire"
    );
}

/// The sRGB curve and its inverse in f64 (IEC 61966-2-1).
fn encode(v: f64) -> f64 {
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn decode(e: f64) -> f64 {
    if e <= 0.040_45 {
        e / 12.92
    } else {
        ((e + 0.055) / 1.055).powf(2.4)
    }
}

#[test]
fn float_equalize_works_on_the_display_encoding_and_keeps_the_extended_range() {
    let n = (CW * CH) as usize;
    // In-range samples sit on 16-bit display codes (a level's centre), so
    // the oracle's levels are the document's; one pixel holds a 4.0
    // highlight and one a −0.02 undershoot.
    let records: Vec<[f32; 4]> = (0..n)
        .map(|i| {
            let j = hash(i as u32, 13);
            let code = |s: u32| decode(f64::from(20000 + (j >> s) % 38000) / 65535.0) as f32;
            match i {
                3 => [4.0, code(3), code(9), 1.0],
                7 => [-0.02, code(3), 0.25, 1.0],
                _ => [
                    code(0),
                    code(6),
                    code(12),
                    if i % 6 == 0 { 0.5 } else { 1.0 },
                ],
            }
        })
        .collect();
    let doc = float_doc(CW, CH, &IccProfile::srgb(), std::slice::from_ref(&records));
    let level = |v: f32| -> usize {
        if v > 1.0 {
            65535
        } else if v < 0.0 {
            0
        } else {
            (encode(f64::from(v)) * 65535.0).round() as usize
        }
    };
    let lut = table(
        records.iter().flat_map(|p| {
            let k = f64::from(p[3]);
            (0..3).map(move |c| (level(p[c]), k))
        }),
        65536,
    )
    .unwrap();
    let out = selected(&doc, &json!({"layer":0,"op":"equalize"}), &[])
        .unwrap()
        .unwrap();
    let layer = out.layer(0).unwrap();
    let image = layer.source().unwrap();
    for (i, p) in records.iter().enumerate() {
        let (x, y) = (i as u32 % CW, i as u32 / CW);
        let got = image.sample_at(x, y).unwrap();
        assert_eq!(got[3].to_bits(), p[3].to_bits(), "alpha is never written");
        for c in 0..3 {
            if p[c] > 1.0 || p[c] < 0.0 {
                assert_eq!(
                    got[c].to_bits(),
                    p[c].to_bits(),
                    "pixel {i}: out of range kept"
                );
                continue;
            }
            // The document's own curve (the built-in profile's) differs from
            // this oracle's IEC formula by up to 0.3 % in the toe, so the
            // table is compared in E within 8 of 65535 codes rather than bit
            // for bit.
            let from = level(p[c]);
            let got_code = encode(f64::from(got[c])) * 65535.0;
            assert!(
                (got_code - lut[from]).abs() <= 8.0,
                "pixel {i} channel {c}: E {got_code} vs table {}",
                lut[from]
            );
        }
    }
}

#[test]
fn equalize_refuses_a_group_and_an_adjustment_layer_by_name() {
    let image = PrecisionImage::from_rgba16(
        4,
        4,
        vec![[1000, 2000, 3000, 65535]; 16],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let leaf = PrecisionDocumentLayer::raster("L", image, (0, 0), 1.0, true).unwrap();
    let group = PrecisionDocumentLayer::isolated_normal("G", vec![leaf], 1.0, true).unwrap();
    let doc = PrecisionDocument::from_parts(
        4,
        4,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![group],
    )
    .unwrap();
    assert_eq!(
        refusal(selected(&doc, &json!({"layer":1,"op":"equalize"}), &[])),
        "Equalize needs a raster layer: a group has no pixels of its own"
    );
    let adjusted = {
        let text = CString::new(
            r#"{"type":"adjust","evaluation":"precision_v1","op":"levels","params":{"output_black":0.1,"output_white":0.8}}"#,
        )
        .unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment(
                &doc,
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
        *unsafe { Box::from_raw(out) }
    };
    let top = adjusted.layer_count() - 1;
    assert_eq!(
        refusal(selected(
            &adjusted,
            &json!({"layer":top,"op":"equalize"}),
            &[]
        )),
        "Equalize needs a raster layer: an adjustment layer has no pixels of its own"
    );
}
