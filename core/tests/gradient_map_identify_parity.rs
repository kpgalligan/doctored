//! The RGB Gradient Map adjustment against Photoshop's own
//! takes, at 8 and 16 bits. Two fixtures (`tests/fixtures/psd_photoshop/
//! gradient_identify_673/`, README), each written by its runner's `--judge`:
//!
//! - `map_take1.json` (`scripts/photoshop-probe/gradient_map_673_take.py`,
//!   gradient-map-673 take1): GRID ({G2, G3, C3} × every stop's Mdpn 20 / 50
//!   / 80 × Smoothness 0 / 50 / 100 %), SEG (G3 with Mdpn 20, 80, 50 at
//!   Smoothness 0 and 100 %) and METHOD (G3 at Perceptual, Linear, Classic
//!   and Smooth);
//! - `map_678_take1.json` (`scripts/photoshop-probe/gradient_map_678_take.py`,
//!   gradient-map-678 take1, the confirm take): SMOOTH (N2 and N4 at
//!   Smoothness 0 / 50 / 100 %), SEG (N4 with Mdpn 30, 70, 20, 60) and
//!   METHOD (Perceptual, Linear and Smooth on N2 and N4). Its two Gradient
//!   Fill rows per depth are recorded for the fills, not judged here.
//!
//! Dither off, Reverse off. The map ships `method: "classic"` (the
//! confirm take's verdicts SMOOTH SHIP with the bracket bound and MIDPOINT
//! SHIP), so every row whose method is Classic (sent or not) is rebuilt as
//! `method: "classic"` with the row's Smoothness and its midpoints SHIFTED
//! as `psd_adjustment_grdm` reads them: our stop `i`'s midpoint is
//! Photoshop stop `i + 1`'s `Mdpn`. Phase 2 ships `method:
//! "perceptual"` (the confirm take's METHOD Perceptual SHIP, PERC-OK), so a
//! `Perc` row is rebuilt the same way as `method: "perceptual"`. A row of a
//! method refused for good (Linear, Smooth: phase 2's bounded attempt fitted
//! none, `gradient_map_678_take.py` RECORDED-2) is rendered as the legacy
//! map, today's linear one. Each row is a gray ramp (8 bits: 256 wide, gray x at column
//! x; 16 bits: 1024 wide, gray round(65535 x / 1023)) through
//! `gradient_map`, by `dr_image_adjust_op` at 8 bits and an adjustment layer
//! added by `dr_precision_doc_add_adjustment` over a 16-bit sRGB layer at
//! 16. A row's error is the max and the mean of `|ours − Photoshop|` over
//! every column and R, G, B, in 8-bit codes (16-bit samples / 257).
//!
//! The bound, declared in each runner before its take, is max <= 2 and mean
//! <= 0.5. A row that misses it asserts its own measured number (`PINNED`,
//! rounded up to 0.01), never a wider one, and that it still misses: the
//! Linear and Smooth METHOD rows.

use std::collections::BTreeMap;
use std::ffi::{c_char, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba};
use doctored_core::ffi_adjust::dr_image_adjust_op;
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

use crate::common::take_err_string;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const MAX: f64 = 2.0;
const MEAN: f64 = 0.5;

// Measured on the legacy (linear) map, max and mean in codes rounded up to
// 0.01, for the Linear and Smooth rows that miss 2 / 0.5.
const PINNED: &[(&str, f64, f64)] = &[
    ("m_Lnr_16", 71.7, 34.96),
    ("m_Lnr_8", 72.0, 34.89),
    ("m_Smoo_16", 34.65, 11.4),
    ("m_Smoo_8", 35.0, 11.36),
];

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/gradient_identify_673")
        .join(name)
}

fn fixture(name: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(fixture_path(name)).expect(name)).expect("json")
}

/// The written gray codes per column (the runners' `written`).
fn ramp(depth: u64) -> Vec<u16> {
    if depth == 8 {
        (0..256).collect()
    } else {
        (0..1024u32)
            .map(|x| (65535.0 * f64::from(x) / 1023.0 + 0.5).floor() as u16)
            .collect()
    }
}

/// The built method a row renders as (module doc): Classic, sent or not, and
/// Perceptual; `None` for Linear and Smooth.
fn built(row: &Value) -> Option<&'static str> {
    match row["method"].as_str() {
        None | Some("Gcls") => Some("classic"),
        Some("Perc") => Some("perceptual"),
        Some(_) => None,
    }
}

/// The row's `gradient_map` params (module doc): Dither off; a built row
/// takes its method with its Smoothness and shifted midpoints, any other is
/// legacy.
fn params(row: &Value) -> Value {
    let mids = row["midpoints"].as_array().expect("midpoints");
    let stops: Vec<Value> = row["stops"]
        .as_array()
        .expect("stops")
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let c = s["color"].as_array().expect("color");
            let hex = format!(
                "#{:02x}{:02x}{:02x}",
                c[0].as_u64().unwrap(),
                c[1].as_u64().unwrap(),
                c[2].as_u64().unwrap()
            );
            // The location Photoshop was sent, whole 1/4096ths.
            let position = (s["position"].as_f64().unwrap() * 4096.0).round() / 4096.0;
            let mut stop = json!({"position": position, "color": hex});
            if let Some(next) = mids.get(i + 1).and_then(Value::as_f64) {
                stop["midpoint"] = json!(next / 100.0);
            }
            stop
        })
        .collect();
    if let Some(method) = built(row) {
        json!({"dither": false, "method": method,
               "gradient": {"stops": stops, "smoothness": row["smoothness"]}})
    } else {
        json!({"dither": false, "gradient": {"stops": stops}})
    }
}

/// Our 8-bit codes per column and channel.
fn ours8(row: &Value) -> Vec<[f64; 3]> {
    let gray = ramp(8);
    let rgba: Vec<u8> = gray
        .iter()
        .flat_map(|&g| [g as u8, g as u8, g as u8, 255])
        .collect();
    let op = CString::new("gradient_map").unwrap();
    let text = CString::new(params(row).to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    unsafe {
        let img = dr_image_from_rgba(rgba.as_ptr(), gray.len() as u32, 1);
        assert!(!img.is_null());
        let out = dr_image_adjust_op(img, op.as_ptr(), text.as_ptr(), &mut err);
        assert!(!out.is_null(), "{}", take_err_string(err));
        let px = std::slice::from_raw_parts(dr_image_pixels_rgba(out), gray.len() * 4);
        let codes = px
            .chunks_exact(4)
            .map(|p| [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])])
            .collect();
        dr_image_free(out);
        dr_image_free(img);
        codes
    }
}

/// Our 16-bit samples / 257 per column and channel.
fn ours16(row: &Value) -> Vec<[f64; 3]> {
    let gray = ramp(16);
    let w = gray.len() as u32;
    let pixels: Vec<[u16; 4]> = gray.iter().map(|&g| [g, g, g, 65535]).collect();
    let image = PrecisionImage::from_rgba16(w, 1, pixels, IccProfile::srgb(), None).unwrap();
    let doc = PrecisionDocument::from_parts(
        w,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![PrecisionDocumentLayer::raster("L", image, (0, 0), 1.0, true).unwrap()],
    )
    .unwrap();
    let description = json!({"type": "adjust", "evaluation": "precision_v1",
                             "op": "gradient_map", "params": params(row)});
    let text = CString::new(description.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            &doc,
            c"Gradient Map".as_ptr(),
            text.as_ptr(),
            1.0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let mapped = *unsafe { Box::from_raw(out) };
    let rendered = mapped.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
    rendered
        .iter()
        .map(|p| [0, 1, 2].map(|c| f64::from(p[c]) / 257.0))
        .collect()
}

/// (max, mean) of |ours − Photoshop| over every column and channel.
fn measure(row: &Value) -> (f64, f64) {
    let ours = if row["depth"].as_u64() == Some(8) {
        ours8(row)
    } else {
        ours16(row)
    };
    let theirs = row["codes"].as_array().expect("codes");
    assert_eq!(ours.len(), theirs.len(), "one entry per column");
    let (mut max, mut sum) = (0.0f64, 0.0f64);
    for (o, t) in ours.iter().zip(theirs) {
        for (c, value) in o.iter().enumerate() {
            let e = (value - t[c].as_f64().expect("code")).abs();
            max = max.max(e);
            sum += e;
        }
    }
    (max, sum / (3 * ours.len()) as f64)
}

/// The map rows of a fixture by name (the Gradient Fill rows left out).
fn map_rows(fixture: &Value) -> BTreeMap<String, Value> {
    fixture["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .filter(|r| !r["family"].as_str().unwrap_or("").starts_with("fill"))
        .map(|r| (r["name"].as_str().expect("name").to_string(), r.clone()))
        .collect()
}

/// Every row meets 2 / 0.5 or, pinned, its pin while still missing.
fn judge(rows: &BTreeMap<String, Value>, pinned: &[(&str, f64, f64)]) {
    for (name, ..) in pinned {
        assert!(
            rows.contains_key(*name),
            "{name}: pinned but not in the fixture"
        );
    }
    let mut failures = Vec::new();
    for (name, row) in rows {
        let (max, mean) = measure(row);
        eprintln!("{name}: max {max:.6} mean {mean:.6}");
        match pinned.iter().find(|p| p.0 == name.as_str()) {
            None => {
                if !(max <= MAX && mean <= MEAN) {
                    failures.push(format!(
                        "{name}: max {max:.6} mean {mean:.6} misses 2 / 0.5"
                    ));
                }
            }
            Some((_, max_pin, mean_pin)) => {
                if !(max <= *max_pin && mean <= *mean_pin) {
                    failures.push(format!(
                        "{name}: max {max:.6} (pinned {max_pin}), mean {mean:.6} (pinned \
                         {mean_pin})"
                    ));
                }
                if max <= MAX && mean <= MEAN {
                    failures.push(format!(
                        "{name}: max {max:.6} mean {mean:.6} now meets 2 / 0.5: unpin it"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn every_gradient_map_take_row_meets_the_bound_or_its_pin() {
    let fixture = fixture("map_take1.json");
    assert_eq!(fixture["verdicts"]["smooth"], "KEEP");
    let rows = map_rows(&fixture);
    assert_eq!(rows.len(), 66, "54 GRID, 4 SEG and 8 METHOD rows");
    judge(&rows, PINNED);
}

// Measured on the legacy (linear) map, max and mean in codes rounded up to
// 0.01, for the confirm take's Linear and Smooth rows.
const PINNED_678: &[(&str, f64, f64)] = &[
    ("m_Lnr_N2_i4096_16", 42.98, 19.05),
    ("m_Lnr_N2_i4096_8", 43.0, 18.99),
    ("m_Lnr_N4_i0_16", 43.19, 11.98),
    ("m_Lnr_N4_i0_8", 44.0, 11.95),
    ("m_Lnr_N4_i4096_16", 44.74, 13.65),
    ("m_Lnr_N4_i4096_8", 45.0, 13.63),
    ("m_Smoo_N2_i4096_16", 37.78, 10.86),
    ("m_Smoo_N2_i4096_8", 38.0, 10.82),
    ("m_Smoo_N4_i0_16", 33.28, 9.37),
    ("m_Smoo_N4_i0_8", 34.0, 9.34),
    ("m_Smoo_N4_i4096_16", 33.28, 9.37),
    ("m_Smoo_N4_i4096_8", 34.0, 9.34),
];

#[test]
fn every_confirm_take_map_row_meets_the_bound_or_its_pin() {
    let fixture = fixture("map_678_take1.json");
    assert_eq!(fixture["verdicts"]["smooth"], "SHIP");
    assert_eq!(fixture["verdicts"]["r_star"], "CLAMP");
    assert_eq!(fixture["verdicts"]["midpoint"], "SHIP");
    let rows = map_rows(&fixture);
    assert_eq!(rows.len(), 36, "12 SMOOTH, 6 SEG and 18 METHOD rows");
    judge(&rows, PINNED_678);
}

/// The confirm fixture's codes are the receipt's (`gradient-map-678/take1/
/// profiles.json`) within the fixture's 2-decimal rounding; skipped when the
/// receipt is absent.
#[test]
fn the_confirm_fixture_holds_the_receipts_codes() {
    let Some(home) = std::env::var_os("HOME") else {
        eprintln!("skipping: no HOME");
        return;
    };
    let receipt = Path::new(&home)
        .join("devel/doctored-fixtures/photoshop/gradient-map-678/take1/profiles.json");
    let Ok(text) = std::fs::read_to_string(&receipt) else {
        eprintln!("skipping: {} is absent", receipt.display());
        return;
    };
    let receipt: Value = serde_json::from_str(&text).expect("profiles.json");
    let fixture = fixture("map_678_take1.json");
    let rows = fixture["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 40);
    for row in rows {
        let name = row["name"].as_str().unwrap();
        let theirs = receipt["cases"][name]["codes"].as_array().expect(name);
        let ours = row["codes"].as_array().unwrap();
        assert_eq!(ours.len(), theirs.len(), "{name}");
        for (o, t) in ours.iter().zip(theirs) {
            for c in 0..3 {
                let d = (o[c].as_f64().unwrap() - t[c].as_f64().unwrap()).abs();
                assert!(d <= 0.005 + 1e-9, "{name}: {o} vs {t}");
            }
        }
    }
}
