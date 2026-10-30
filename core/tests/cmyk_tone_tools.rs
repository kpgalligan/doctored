//! Levels and Curves' tools on a CMYK document (#1 phase 5), through the
//! FFI: the per-plate histograms of `L = 1 − ink` against hand-built
//! distributions (selection coverage and the backdrop source included);
//! Auto's endpoints, both algorithms with asymmetric clips, against an
//! oracle written here from `ops_auto`'s documented rule (the black's count
//! rule, the white walk and its MIX3 placement), K left at (0, 1) by
//! per_channel and given the summed pair by monochromatic; the eyedroppers'
//! sample and targets against the synthetic press's own tables; the ⌥
//! clipping display's masks; every refusal by its sentence.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_string_free};
use doctored_core::ffi_cmyk_colour::*;
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_doc_open};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::Value;
use tempfile::TempDir;

use crate::cmyk_fixture::{b2a1_codes, encode_lab, ink_lab, press_profile, srgb_to_ink_bpc};
use crate::cmyk_tiff_builder::cmyk_tiff;
use crate::common::{cpath, img_pixels, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

fn cmyk_doc(dir: &TempDir, w: u32, h: u32, samples: &[[u16; 4]]) -> *mut PrecisionDocument {
    let data: Vec<u8> = samples
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let path = dir.path().join(format!("tone{w}x{h}.tif"));
    std::fs::write(
        &path,
        cmyk_tiff(w, h, 16, 4, 0, &data, Some(&press_profile("Press A"))),
    )
    .unwrap();
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

fn analysis(
    doc: *const PrecisionDocument,
    request: &str,
    coverage: &[u16],
) -> Result<Value, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_cmyk_tone_analysis(
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
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    Ok(value)
}

/// 1000 pixels (40 × 25) whose plates hold the distributions the tests
/// reason about, as L16 (`65535 − ink`):
/// C: 3 at 2570, 992 at 50000, 5 at 64250; M: a ramp 1000 + 60·i;
/// Y: no ink anywhere (a flat plane); K: 10 at 0, 990 at 30000.
fn distribution() -> Vec<[u16; 4]> {
    (0..1000u32)
        .map(|i| {
            let c = match i {
                0..=2 => 2570,
                3..=994 => 50000,
                _ => 64250,
            };
            let m = 1000 + 60 * i;
            let k = if i < 10 { 0 } else { 30000 };
            [65535 - c, (65535 - m) as u16, 0, 65535 - k]
        })
        .collect()
}

fn l_plane(samples: &[[u16; 4]], c: usize) -> Vec<u64> {
    let mut plane = vec![0u64; 65536];
    for s in samples {
        plane[usize::from(65535 - s[c])] += 65535;
    }
    plane
}

/// `ops_auto`'s black: the lowest code whose running mass exceeds `cut`.
fn black_code(plane: &[u64], cut: f64) -> Option<usize> {
    let mut run = 0f64;
    for (v, n) in plane.iter().enumerate() {
        run += *n as f64;
        if run > cut {
            return Some(v);
        }
    }
    None
}

/// `ops_auto`'s white at clip > 0: the walk over 256 bins
/// (round(v/257), half up), T = cut + 3·unit, θ 0.7, MIX3 placement.
fn white_code(plane: &[u64], cut: f64, unit: f64) -> Option<usize> {
    let mut bins = [0f64; 256];
    for (v, n) in plane.iter().enumerate() {
        bins[(2 * v + 257) / 514] += *n as f64;
    }
    let t = cut + 3.0 * unit;
    let lowest = (0..256).find(|&v| bins[v] > 0.0)?;
    let mut run = 0f64;
    let mut last = None;
    for v in (lowest..256).rev() {
        let b = bins[v];
        if b == 0.0 {
            continue;
        }
        if v != lowest && t - run >= 0.7 * b {
            run += b;
            last = Some(v);
            continue;
        }
        return Some(match last {
            Some(code) if b < 3.0 * unit => code - 1,
            _ => v,
        });
    }
    None
}

/// The endpoint pair of one plane, as L fractions, or (0, 1).
fn endpoints(plane: &[u64], shadows: f64, highlights: f64) -> (f64, f64) {
    let unit = 65535.0;
    let Some(lo) = black_code(plane, shadows) else {
        return (0.0, 1.0);
    };
    let lo = lo as f64 / 65535.0;
    let hi = if highlights <= 0.0 {
        (0..65536).rev().find(|&v| plane[v] > 0).unwrap() as f64 / 65535.0
    } else {
        white_code(plane, highlights, unit).unwrap() as f64 / 255.0
    };
    if lo < hi {
        (lo, hi)
    } else {
        (0.0, 1.0)
    }
}

fn floats(value: &Value) -> Vec<f64> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect()
}

#[test]
fn the_histograms_count_each_plate_of_l() {
    let dir = TempDir::new().unwrap();
    let samples = distribution();
    let doc = cmyk_doc(&dir, 40, 25, &samples);
    let answer = analysis(doc, r#"{"source":"composite"}"#, &[]).unwrap();
    let c = floats(&answer["histogram"]["c"]);
    assert_eq!(c.len(), 256);
    assert_eq!((c[10], c[195], c[250]), (3.0, 992.0, 5.0));
    assert_eq!(c.iter().sum::<f64>(), 1000.0);
    assert_eq!(floats(&answer["histogram"]["y"])[255], 1000.0);
    let k = floats(&answer["histogram"]["k"]);
    assert_eq!((k[0], k[(2 * 30000 + 257) / 514]), (10.0, 990.0));
    // The composite is the four plates summed.
    let cmyk = floats(&answer["histogram"]["cmyk"]);
    let plates: Vec<Vec<f64>> = ["c", "m", "y", "k"]
        .iter()
        .map(|p| floats(&answer["histogram"][p]))
        .collect();
    for (i, total) in cmyk.iter().enumerate() {
        let sum: f64 = plates.iter().map(|p| p[i]).sum();
        assert!((total - sum).abs() < 1e-9);
    }
    // A layer's own pixels and the backdrop below record 1 are the same
    // single opaque raster here; nothing lies below record 0.
    let layer = analysis(doc, r#"{"source":"layer","layer":0}"#, &[]).unwrap();
    assert_eq!(layer["histogram"], answer["histogram"]);
    let backdrop = analysis(doc, r#"{"source":"backdrop","below":1}"#, &[]).unwrap();
    assert_eq!(backdrop["histogram"], answer["histogram"]);
    let empty = analysis(
        doc,
        r#"{"source":"backdrop","below":0,"auto":{"algorithm":"per_channel"}}"#,
        &[],
    )
    .unwrap();
    assert_eq!(empty["auto"], serde_json::json!({"empty":true}));
    // A selection: the left half (x < 20) at full coverage, the rest at a
    // quarter.
    let coverage: Vec<u16> = (0..1000u32)
        .map(|i| if i % 40 < 20 { 65535 } else { 16384 })
        .collect();
    let selected = analysis(doc, "{}", &coverage).unwrap();
    let total: f64 = floats(&selected["histogram"]["c"]).iter().sum();
    let expected = 500.0 + 500.0 * ((65535u64 * 16384 + 32767) / 65535) as f64 / 65535.0;
    assert!((total - expected).abs() < 1e-6, "{total} vs {expected}");
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn auto_takes_each_algorithm_with_its_own_clips() {
    let dir = TempDir::new().unwrap();
    let samples = distribution();
    let doc = cmyk_doc(&dir, 40, 25, &samples);
    let mass = 1000.0 * 65535.0;
    for (shadows, highlights) in [(0.5, 1.0), (0.2, 0.0), (0.0, 0.6), (0.1, 0.1)] {
        let (cut_s, cut_h) = (shadows / 100.0 * mass, highlights / 100.0 * mass);
        let answer = analysis(
            doc,
            &format!(
                r#"{{"auto":{{"algorithm":"per_channel","shadows":{shadows},"highlights":{highlights}}}}}"#
            ),
            &[],
        )
        .unwrap();
        let (black, white) = (
            floats(&answer["auto"]["black"]),
            floats(&answer["auto"]["white"]),
        );
        // K (whose plane would stretch: 10 pixels at 0, 990 at 30000) is
        // held at (0, 1).
        assert_eq!((black[3], white[3]), (0.0, 1.0), "per_channel holds K");
        for c in 0..3 {
            let (lo, hi) = endpoints(&l_plane(&samples, c), cut_s, cut_h);
            assert!(
                (black[c] - lo).abs() < 1e-6 && (white[c] - hi).abs() < 1e-6,
                "per_channel {shadows}/{highlights} plate {c}: ({}, {}) vs ({lo}, {hi})",
                black[c],
                white[c]
            );
        }
        // Monochromatic: one pair from the four plates summed, on all four.
        let answer = analysis(
            doc,
            &format!(
                r#"{{"auto":{{"algorithm":"monochromatic","shadows":{shadows},"highlights":{highlights}}}}}"#
            ),
            &[],
        )
        .unwrap();
        let mut summed = vec![0u64; 65536];
        for c in 0..4 {
            for (slot, n) in summed.iter_mut().zip(l_plane(&samples, c)) {
                *slot += n;
            }
        }
        let (lo, hi) = endpoints(&summed, 4.0 * cut_s, 4.0 * cut_h);
        assert_eq!(answer["auto"]["algorithm"], "monochromatic");
        for c in 0..4 {
            assert!((floats(&answer["auto"]["black"])[c] - lo).abs() < 1e-6);
            assert!((floats(&answer["auto"]["white"])[c] - hi).abs() < 1e-6);
        }
    }
    // The cyan plate's numbers, stated: with 0.5 % / 1 % the 3 dark pixels
    // are clipped (black = 50000) and the 5 light ones are (white = bin 195).
    let answer = analysis(
        doc,
        r#"{"auto":{"algorithm":"per_channel","shadows":0.5,"highlights":1.0}}"#,
        &[],
    )
    .unwrap();
    let black = floats(&answer["auto"]["black"])[0];
    assert!((black - 50000.0 / 65535.0).abs() < 1e-6, "{black}");
    assert!((floats(&answer["auto"]["white"])[0] - 195.0 / 255.0).abs() < 1e-6);
    // The flat yellow plate keeps (0, 1).
    assert_eq!(floats(&answer["auto"]["black"])[2], 0.0);
    assert_eq!(floats(&answer["auto"]["white"])[2], 1.0);
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn the_eyedroppers_sample_and_target_through_the_profile() {
    let dir = TempDir::new().unwrap();
    let ink = [12000u16, 30000, 5000, 20000];
    let doc = cmyk_doc(&dir, 3, 3, &[ink; 9]);
    let pick = |kind: &str| {
        analysis(
            doc,
            &format!(r#"{{"eyedropper":{{"kind":"{kind}","x":1,"y":1,"reach":1}}}}"#),
            &[],
        )
        .unwrap()["eyedropper"]
            .clone()
    };
    let unit = ink.map(|v| f64::from(v) / 65535.0);
    let white = pick("white");
    let sample = floats(&white["sample"]);
    for c in 0..4 {
        assert!((sample[c] - unit[c]).abs() < 1e-6);
    }
    assert_eq!(floats(&white["target"]), vec![0.0; 4]);
    // Black: sRGB black separated through the press (BPC on).
    let black = floats(&pick("black")["target"]);
    let expected = srgb_to_ink_bpc([0.0; 3], true);
    for c in 0..4 {
        assert!(
            (black[c] - expected[c].clamp(0.0, 1.0)).abs() < 2.0 / 65535.0,
            "black {c}"
        );
    }
    // Grey: the sample's own L* with a* = b* = 0, back through B2A1.
    let grey = floats(&pick("gray")["target"]);
    let lab = ink_lab(unit);
    let expected =
        b2a1_codes(encode_lab([lab[0], 0.0, 0.0])).map(|v| (v / 65535.0).clamp(0.0, 1.0));
    for c in 0..4 {
        assert!(
            (grey[c] - expected[c]).abs() < 2.0 / 65535.0,
            "grey {c}: {} vs {}",
            grey[c],
            expected[c]
        );
    }
    assert_eq!(
        analysis(doc, r#"{"eyedropper":{"kind":"red","x":0,"y":0}}"#, &[]).unwrap_err(),
        "eyedropper kind is black, gray or white"
    );
    assert_eq!(
        analysis(doc, r#"{"source":"layer","layer":4}"#, &[]).unwrap_err(),
        "record 4 is not a CMYK raster layer"
    );
    assert_eq!(
        analysis(doc, r#"{"auto":{"shadows":12}}"#, &[]).unwrap_err(),
        "Auto Options shadows is a percentage from 0 to 9.99"
    );
    assert_eq!(
        analysis(doc, r#"{"histogram":true}"#, &[]).unwrap_err(),
        "the CMYK tone analysis request has an unknown key \"histogram\""
    );
    let mut err = ptr::null_mut();
    let wrong = unsafe {
        dr_precision_doc_cmyk_tone_analysis(
            doc,
            c"{}".as_ptr(),
            [1u16; 4].as_ptr(),
            4,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(wrong.is_null());
    assert_eq!(
        take_err_string(err),
        "selection coverage must be empty or canvas-sized u16 samples"
    );
    unsafe { dr_precision_doc_free(doc) };
}

fn clip(doc: *const PrecisionDocument, request: &str) -> Result<Vec<u8>, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let image = unsafe {
        dr_precision_doc_cmyk_clip_display(doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    if image.is_null() {
        return Err(take_err_string(err));
    }
    let pixels = img_pixels(image);
    unsafe { dr_image_free(image) };
    Ok(pixels.chunks(4).map(|p| p[0]).collect())
}

#[test]
fn the_clipping_display_marks_paper_and_full_ink() {
    let dir = TempDir::new().unwrap();
    let doc = cmyk_doc(
        &dir,
        2,
        2,
        &[
            [0, 0, 0, 0],
            [65535, 0, 0, 0],
            [0, 0, 0, 65535],
            [100, 0, 0, 0],
        ],
    );
    assert_eq!(
        clip(doc, r#"{"mode":"highlights"}"#).unwrap(),
        vec![255, 0, 0, 0]
    );
    assert_eq!(
        clip(doc, r#"{"mode":"shadows"}"#).unwrap(),
        vec![255, 0, 0, 255]
    );
    assert_eq!(
        clip(doc, r#"{"mode":"shadows","plates":["k"]}"#).unwrap(),
        vec![255, 255, 0, 255]
    );
    assert_eq!(
        clip(doc, r#"{"mode":"highlights","plates":["m","y"]}"#).unwrap(),
        vec![255; 4]
    );
    assert_eq!(
        clip(doc, r#"{"mode":"midtones"}"#).unwrap_err(),
        "the clipping display mode is shadows or highlights"
    );
    assert_eq!(
        clip(doc, r#"{"mode":"shadows","plates":[]}"#).unwrap_err(),
        "the clipping display needs at least one plate"
    );
    unsafe { dr_precision_doc_free(doc) };
}

fn decode(bytes: &[u8], extension: &str) -> Result<Value, String> {
    let ext = CString::new(extension).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_adjustment_preset_file_decode_cmyk(bytes.as_ptr(), bytes.len(), ext.as_ptr(), &mut err)
    };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    Ok(value)
}

fn be(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_be_bytes()).collect()
}

#[test]
fn cmyk_presets_decode_onto_ink_channels() {
    // An `.alv`: version 2, 29 records; the composite and cyan move.
    let mut words = vec![2u16];
    let mut records = vec![[0u16, 255, 0, 255, 100]; 29];
    records[0] = [10, 240, 0, 255, 100];
    records[1] = [0, 255, 20, 235, 150];
    for record in &records {
        words.extend_from_slice(record);
    }
    let levels = decode(&be(&words), "alv").unwrap();
    assert_eq!(levels["op"], "levels");
    let params = &levels["params"];
    assert!((params["black"].as_f64().unwrap() - 10.0 / 255.0).abs() < 1e-12);
    assert!((params["white"].as_f64().unwrap() - 240.0 / 255.0).abs() < 1e-12);
    assert_eq!(params["gamma"], 1.0);
    assert_eq!(params["c"]["gamma"], 1.5);
    assert!((params["c"]["output_black"].as_f64().unwrap() - 20.0 / 255.0).abs() < 1e-12);
    assert!(params.get("m").is_none() && params.get("k").is_none());
    // A bad record names itself.
    records[3] = [100, 90, 0, 255, 100];
    let mut bad = vec![2u16];
    for record in &records {
        bad.extend_from_slice(record);
    }
    assert_eq!(
        decode(&be(&bad), "alv").unwrap_err(),
        "this Photoshop Levels preset cannot be used: its input ceiling does not exceed its floor"
    );
    // An `.acv`: version 4, five curves of (output, input) pairs; cyan
    // bends, the rest are the identity.
    let identity = [2u16, 0, 0, 255, 255];
    let mut curves = vec![4u16, 5];
    curves.extend_from_slice(&identity);
    curves.extend_from_slice(&[3, 0, 0, 140, 128, 255, 255]);
    for _ in 0..3 {
        curves.extend_from_slice(&identity);
    }
    let decoded = decode(&be(&curves), "acv").unwrap();
    assert_eq!(decoded["op"], "curves");
    // The point reflection (255 − in, 255 − out) onto the ink axes.
    assert_eq!(
        decoded["params"],
        serde_json::json!({"c":[[0,0],[127,115],[255,255]]})
    );
    // An RGB preset (four curves) and every other extension refuse.
    let mut rgb = vec![4u16, 4];
    for _ in 0..4 {
        rgb.extend_from_slice(&identity);
    }
    assert_eq!(
        decode(&be(&rgb), "acv").unwrap_err(),
        "this curves preset holds 4 curves, an RGB preset; a CMYK document needs a CMYK preset of five curves"
    );
    assert_eq!(
        decode(&[0, 2], "ahu").unwrap_err(),
        "A .ahu preset is unavailable in a CMYK document"
    );
}
