//! A Photoshop smart-object record (`PlLd` / `SoLd`) imports as its layer
//! pixels: Import Layers keeps the record's own channel data, binds no
//! retained source, and reports the discarded record as a loss (smart objects
//! were removed 2026-10-10; this is the behaviour before phase 8a).
//!
//! The files are Photoshop 26.10.0's own
//! (`fixtures/psd_photoshop/smart_objects`): an 8-bit embedded smart object,
//! the same with a Gaussian Blur smart filter, a 16-bit smart object and a
//! 16-bit one whose contents are 32-bit. The oracle is the file's own merged
//! composite, which Photoshop rendered: the imported layers, flattened over
//! white, equal it within one byte code (257 of 65535 at 16 bits), the
//! rounding of one alpha-over step.

use std::ffi::CString;
use std::path::PathBuf;
use std::ptr;

use doctored_core::ffi_psd::dr_precision_doc_open_psd;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::Value;

use crate::common::psd_photoshop::{fixture, import_report, open_psd, strings, COMPOSITE, LAYERS};
use crate::common::{cpath, flat_pixels, take_err_string};

const LOSS: &str = "has placed/smart-object or linked-layer metadata; layered import discards these records and does not retain editable object contents or links";

fn family(name: &str) -> PathBuf {
    fixture("smart_objects", name)
}

/// Straight RGBA over white, in f64 codes of `max`.
fn over_white(rgba: [f64; 4], max: f64) -> [f64; 3] {
    let a = rgba[3] / max;
    [0, 1, 2].map(|c| rgba[c] * a + max * (1.0 - a))
}

fn assert_close(what: &str, got: &[[f64; 4]], want: &[[f64; 4]], max: f64, bound: f64) {
    assert_eq!(got.len(), want.len(), "{what}: sample count");
    let mut worst = 0.0_f64;
    for (g, w) in got.iter().zip(want) {
        let (g, w) = (over_white(*g, max), over_white(*w, max));
        for c in 0..3 {
            worst = worst.max((g[c] - w[c]).abs());
        }
    }
    println!("{what}: worst difference {worst} of {max}");
    assert!(worst <= bound, "{what}: {worst} > {bound}");
}

fn assert_loss(name: &str, report: &Value) {
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    let losses = strings(report, "verified_losses");
    assert!(
        losses.iter().any(|loss| loss.contains(LOSS)),
        "{name}: {losses:?}"
    );
}

#[test]
fn an_eight_bit_smart_object_imports_as_its_pixels() {
    for name in ["p8_smart_object.psd", "p8_smart_filter.psd"] {
        let path = family(name);
        let report = import_report(&path);
        assert_loss(name, &report);
        let layered = open_psd(&path, &report, LAYERS);
        for (i, layer) in layered.layers.iter().enumerate() {
            assert!(
                layer.retained_source.is_none() && !layered.has_retained_source(i),
                "{name}: layer {i} “{}” is bound",
                layer.name
            );
        }
        let composite = open_psd(&path, &report, COMPOSITE);
        let samples = |doc: &doctored_core::doc::DrDocument| -> Vec<[f64; 4]> {
            flat_pixels(doc)
                .chunks_exact(4)
                .map(|p| [p[0], p[1], p[2], p[3]].map(f64::from))
                .collect()
        };
        assert_close(name, &samples(&layered), &samples(&composite), 255.0, 1.0);
    }
}

fn open16(name: &str, report: &Value, mode: i32) -> PrecisionDocument {
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_open_psd(
            cpath(&family(name)).as_ptr(),
            mode,
            token.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null::<DrJobControl>(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "{name}: {}", take_err_string(error));
    *unsafe { Box::from_raw(raw) }
}

#[test]
fn a_sixteen_bit_smart_object_imports_as_its_pixels() {
    for name in ["t7_C16.psd", "f3_SRC16.psd"] {
        let report = import_report(&family(name));
        assert_loss(name, &report);
        let layered = open16(name, &report, LAYERS);
        for i in 0..layered.layer_count() {
            assert!(
                !layered.has_retained_source(i),
                "{name}: layer {i} is bound"
            );
        }
        let composite = open16(name, &report, COMPOSITE);
        let samples = |doc: &PrecisionDocument| -> Vec<[f64; 4]> {
            doc.render(REFERENCE_WORKING_BYTES, 0, None)
                .unwrap()
                .to_rgba16()
                .unwrap()
                .iter()
                .map(|p| p.map(f64::from))
                .collect()
        };
        assert_close(
            name,
            &samples(&layered),
            &samples(&composite),
            65535.0,
            257.0,
        );
    }
}
