//! HDR Toning against Photoshop: the take's record held to its
//! measured residuals.
//!
//! `fixtures/photoshop_hdr_toning/rows.json` is written by
//! `scripts/photoshop-probe/hdr_toning_564_take.py --export-record` from ONE
//! judged take (`take` in the file, under
//! `~/devel/doctored-fixtures/photoshop/`): Photoshop 26.10.0 toned the
//! 32-bit chart and the merged bracket to 16 bits by the descriptor form the
//! live selftest chose (`form`, `members`), and the runner's judge scored
//! Doctored's `hdr_toning` (through the app) against each output as the
//! max and mean |Δ| / 257 over RGB (codes at 8-bit scale). A row is PASS
//! when it meets the declared gate (max 2, mean 0.5); a MISS row ships as
//! declared with its numbers disclosed.
//!
//! The test tones each source again through `dr_precision_doc_hdr_toning`
//! with the row's core request and requires the residual against
//! Photoshop's own file to be no worse than recorded (1e-4 for the record's
//! four-decimal rounding), so a change that moves a method away from
//! Photoshop fails here. The take's TIFFs stay outside the repo; without
//! the take folder the measured half skips.

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::ffi_hdr_toning::dr_precision_doc_hdr_toning;
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_doc_open};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::Value;

use crate::common::{cpath, take_err_string};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// The record's numbers are rounded to four decimals.
const ROUNDING: f64 = 1e-4;

fn record() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/photoshop_hdr_toning/rows.json");
    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap()
}

/// The take folder named by the record, when it is on this machine.
fn take_folder(record: &Value) -> Option<PathBuf> {
    let root: PathBuf = std::env::var_os("HOME")?.into();
    let folder = root
        .join("devel/doctored-fixtures/photoshop")
        .join(record["take"].as_str().unwrap());
    folder.is_dir().then_some(folder)
}

fn open(path: &Path) -> *mut PrecisionDocument {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(
        !doc.is_null(),
        "open {}: {}",
        path.display(),
        take_err_string(err)
    );
    doc
}

/// The one raster's RGBA16 codes.
fn codes(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    assert_eq!(doc.layer_count(), 1, "one raster");
    let layer = doc.layer(0).unwrap();
    layer.source().unwrap().to_rgba16().unwrap()
}

/// (max, mean) of |a − b| / 257 over every RGB sample.
fn residual(got: &[[u16; 4]], want: &[[u16; 4]]) -> (f64, f64) {
    assert_eq!(got.len(), want.len(), "the same pixel count");
    let (mut worst, mut total) = (0.0f64, 0.0f64);
    for (a, b) in got.iter().zip(want) {
        for c in 0..3 {
            let d = (f64::from(a[c]) - f64::from(b[c])).abs() / 257.0;
            worst = worst.max(d);
            total += d;
        }
    }
    (worst, total / (got.len() * 3) as f64)
}

#[test]
fn each_take_row_is_within_its_recorded_residual() {
    let record = record();
    let gate_max = record["gate"]["max"].as_f64().unwrap();
    let gate_mean = record["gate"]["mean"].as_f64().unwrap();
    let rows = record["rows"].as_array().unwrap();
    for method in ["LA", "EG", "HC", "EH"] {
        for document in ["chart", "merged"] {
            assert!(
                rows.iter()
                    .any(|r| r["method"] == method && r["document"] == document),
                "{method} has a judged row on {document}"
            );
        }
    }
    for row in rows {
        let (max, mean) = (row["max"].as_f64().unwrap(), row["mean"].as_f64().unwrap());
        let meets = max <= gate_max && mean <= gate_mean;
        assert_eq!(
            row["verdict"] == "PASS",
            meets,
            "{}: the verdict is the gate's",
            row["name"]
        );
    }
    let Some(take) = take_folder(&record) else {
        eprintln!("SKIP: the take folder {} is absent", record["take"]);
        return;
    };
    for row in rows {
        let document = row["document"].as_str().unwrap();
        let name = row["name"].as_str().unwrap();
        let source = open(&take.join("documents").join(format!("{document}.tif")));
        let photoshop = open(
            &take
                .join(format!("job-{document}"))
                .join(format!("{document}_{name}.tif")),
        );
        let request = CString::new(row["request"].to_string()).unwrap();
        let mut err = ptr::null_mut();
        let toned = unsafe {
            dr_precision_doc_hdr_toning(source, request.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        };
        assert!(!toned.is_null(), "{name}: {}", take_err_string(err));
        let (max, mean) = residual(unsafe { &codes(&*toned) }, unsafe { &codes(&*photoshop) });
        unsafe {
            dr_precision_doc_free(toned);
            dr_precision_doc_free(photoshop);
            dr_precision_doc_free(source);
        }
        let (want_max, want_mean) = (row["max"].as_f64().unwrap(), row["mean"].as_f64().unwrap());
        assert!(
            max <= want_max + ROUNDING && mean <= want_mean + ROUNDING,
            "{document} {name}: {max:.4} / {mean:.4} against the recorded {want_max} / {want_mean}"
        );
        if row["verdict"] == "PASS" {
            assert!(
                max <= gate_max && mean <= gate_mean,
                "{document} {name} meets the gate"
            );
        }
    }
}
