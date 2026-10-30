//! #2 part 1: the take record of `adjust-parity-578/take1`
//! (`scripts/photoshop-probe/adjust_parity_578_take.py`), Photoshop 26.10.0
//! on a fresh scene: Levels (`R8(h,h,x)` at 8 bits, Color Balance's Hermite
//! toe H-CB at 16) and Posterize (`floor(c·n/256)` bins at 8 bits, the 15-bit
//! grid at 16) CONFIRMED inside max 2 / mean 0.5 codes per sample; Black &
//! White's `h8a-LE` / `pred16` REFUTED at 8 bits (D1 3 codes, D2 2) and kept;
//! Vibrance NOT RUN (neither fitted candidate met the gate offline), V3t-P
//! kept.
//!
//! Each judged row carries the runner's prediction of the shipped kernel
//! (`shipped.codes`) and that kernel's residual against Photoshop. The core
//! is held, row by row, within the core-vs-oracle gap the parity tests
//! already assert (1 code at 8 bits, 255·1e-4 codes at 16) of that
//! prediction, and its own Photoshop residual to the judged one plus the gap
//! it measured on the row (plus 0.01 codes of readback slack at 16 bits).
//! 8-bit rows run through `dr_doc_adjust_image` on a document with the row's
//! profile, 16-bit rows through `PrecisionAdjustment`. Both tests skip when
//! the receipt is absent.

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Arc;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba};
use doctored_core::ffi_calibrated::dr_doc_adjust_image;
use doctored_core::icc::IccProfile;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common::take_err_string;

const ROMM: &str = "/System/Library/ColorSync/Profiles/ROMM RGB.icc";

/// The core's allowed distance from the runner's prediction, in codes: the
/// `CORE_GAP` that `scripts/photoshop-probe/adjust_parity_578_take.py`
/// declares (1 code at 8 bits, 0.0255 at 16).
const CORE_GAP_8: f64 = 1.0;
const CORE_GAP_16: f64 = 0.0255;

fn receipt_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        Path::new(&home)
            .join("devel/doctored-fixtures/photoshop/adjust-parity-578/take1/judged.json"),
    )
}

/// The judged receipt, or `None` (with a line on stderr) when it is absent.
fn judged() -> Option<Value> {
    let Some(path) = receipt_path() else {
        eprintln!("skipping: no HOME");
        return None;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("skipping: {} is absent", path.display());
        return None;
    };
    Some(serde_json::from_str(&text).expect("judged.json"))
}

fn profile_of(name: &str) -> Option<Arc<IccProfile>> {
    match name {
        "srgb" => Some(IccProfile::srgb()),
        _ => {
            let bytes = std::fs::read(ROMM).ok()?;
            Some(Arc::new(
                IccProfile::parse(&bytes).expect("ROMM RGB parses"),
            ))
        }
    }
}

fn triples(value: &Value) -> Vec<[f64; 3]> {
    value
        .as_array()
        .expect("triples")
        .iter()
        .map(|t| {
            let t = t.as_array().expect("a triple");
            [0, 1, 2].map(|c| t[c].as_f64().expect("a number"))
        })
        .collect()
}

/// The core's output codes for one row: integers at 8 bits, value × 65535
/// at 16.
fn ours(row: &Value, profile: &Arc<IccProfile>) -> Vec<[f64; 3]> {
    let input = triples(&row["input"]);
    let op = row["op"].as_str().unwrap();
    let params = &row["params"];
    if row["bits"].as_u64() == Some(8) {
        let width = input.len() as u32;
        let src = RgbaImage::from_fn(width, 1, |x, _| {
            let c = input[x as usize];
            Rgba([c[0] as u8, c[1] as u8, c[2] as u8, 255])
        });
        let mut doc = DrDocument::from_pixels(src.clone());
        doc.profile = profile.clone();
        let image = unsafe { dr_image_from_rgba(src.as_raw().as_ptr(), width, 1) };
        assert!(!image.is_null());
        let op_c = CString::new(op).unwrap();
        let params_c = CString::new(params.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let out =
            unsafe { dr_doc_adjust_image(&doc, image, op_c.as_ptr(), params_c.as_ptr(), &mut err) };
        unsafe { dr_image_free(image) };
        assert!(!out.is_null(), "{}", take_err_string(err));
        let bytes =
            unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(out), src.as_raw().len()) }
                .to_vec();
        unsafe { dr_image_free(out) };
        return bytes
            .chunks_exact(4)
            .map(|p| [0, 1, 2].map(|c| f64::from(p[c])))
            .collect();
    }
    let meta = json!({
        "type": "adjust", "evaluation": "precision_v1", "op": op, "params": params
    })
    .to_string();
    let adjustment = PrecisionAdjustment::parse(&meta, profile).expect("parses");
    input
        .iter()
        .map(|c| {
            let got = adjustment.apply_at(c.map(|v| (v / 65535.0) as f32), 0.0, (0, 0));
            got.map(|v| f64::from(v) * 65535.0)
        })
        .collect()
}

/// Per-sample error in 8-bit codes between the core's codes and a
/// reference, `a1.errs`'s units: at 8 bits the reference rounded half up.
fn sample_error(bits: u64, core: &[f64; 3], reference: &[f64; 3]) -> f64 {
    (0..3)
        .map(|c| {
            if bits == 8 {
                (core[c] - (reference[c] + 0.5).floor()).abs()
            } else {
                255.0 * (core[c] - reference[c]).abs() / 65535.0
            }
        })
        .fold(0.0, f64::max)
}

fn max_mean(errors: &[f64]) -> (f64, f64) {
    (
        errors.iter().cloned().fold(0.0, f64::max),
        errors.iter().sum::<f64>() / errors.len() as f64,
    )
}

#[test]
fn every_take_row_holds_the_shipped_kernel_to_its_judged_residual() {
    let Some(judged) = judged() else { return };
    let rows = judged["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 40, "12 Levels, 16 Posterize and 12 B&W rows");
    for row in rows {
        let name = row["name"].as_str().unwrap();
        let bits = row["bits"].as_u64().unwrap();
        let Some(profile) = profile_of(row["profile"].as_str().unwrap()) else {
            eprintln!("skipping {name}: {ROMM} is absent");
            continue;
        };
        let core = ours(row, &profile);
        let shipped = triples(&row["shipped"]["codes"]);
        let photoshop = triples(&row["photoshop"]);
        assert_eq!(core.len(), shipped.len(), "{name}");
        let gap: Vec<f64> = core
            .iter()
            .zip(&shipped)
            .map(|(c, s)| sample_error(bits, c, s))
            .collect();
        let (gap_max, gap_mean) = max_mean(&gap);
        let core_gap = if bits == 8 { CORE_GAP_8 } else { CORE_GAP_16 };
        assert!(
            gap_max <= core_gap + 1e-9,
            "{name}: the core is {gap_max} codes from the runner's prediction"
        );
        // Photoshop's codes are exact at 8 bits and readback × 65535 at 16.
        let residual: Vec<f64> = core
            .iter()
            .zip(&photoshop)
            .map(|(c, p)| {
                (0..3)
                    .map(|k| {
                        if bits == 8 {
                            (c[k] - p[k]).abs()
                        } else {
                            255.0 * (c[k] - p[k]).abs() / 65535.0
                        }
                    })
                    .fold(0.0, f64::max)
            })
            .collect();
        // In the judge's own rounding (`a1.stats`: 4 decimals), the form the
        // judged residual is stored in.
        let (max, mean) = max_mean(&residual);
        let (max, mean) = ((max * 1e4).round() / 1e4, (mean * 1e4).round() / 1e4);
        let slack = if bits == 8 { 1e-9 } else { 0.01 };
        let judged_max = row["shipped"]["max"].as_f64().unwrap();
        let judged_mean = row["shipped"]["mean"].as_f64().unwrap();
        eprintln!("{name}: max {max:.4} mean {mean:.4} (judged {judged_max} / {judged_mean})");
        assert!(
            max <= judged_max + gap_max + slack,
            "{name}: max {max} over the judged {judged_max} + gap {gap_max}"
        );
        assert!(
            mean <= judged_mean + gap_mean + slack,
            "{name}: mean {mean} over the judged {judged_mean} + gap {gap_mean}"
        );
    }
}

#[test]
fn the_take_verdicts_are_what_ships() {
    let Some(judged) = judged() else { return };
    let verdicts = &judged["verdicts"];
    for (item, verdict, ships) in [
        ("vibrance", "NOT RUN", "V3t-P"),
        ("levels", "CONFIRMED", "R8(h,h,x)|H-CB"),
        ("posterize", "CONFIRMED", "P256.Ofloor|Q15a.Ufrac"),
        ("bw", "REFUTED", "h8a-LE|pred16"),
    ] {
        assert_eq!(verdicts[item]["verdict"], verdict, "{item}");
        assert_eq!(verdicts[item]["ships"], ships, "{item}");
    }
    assert_eq!(judged["fits"]["VH"], false);
    assert_eq!(judged["fits"]["VS"], false);
}
