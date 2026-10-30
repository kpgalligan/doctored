//! Photoshop 26.10.0's own 32-bit RGB PSD (take1,
//! `scripts/photoshop-probe/psd32_653_take.py`), through the C API.
//!
//! `core/tests/fixtures/psd_photoshop/depth32/source32.psd` is the take's
//! `ps32.psd`: Photoshop's 32-bit form — an EMPTY layer-information section,
//! an empty `Mt32` block declaring the merged alpha, the records in `Lr32`
//! with a positive count, every layer and mask channel ZIP with Photoshop's
//! byte-plane prediction (f32 masks included), a Raw composite. The scene and
//! the bound are the runner's, restated here as the oracle: per component
//! `|got − declared| ≤ 1e-3·|declared| + 1e-6`.
//!
//! The stored composite is matted over white (candidate H1, `c·a + (1 − a)`):
//! region A (nothing shows) stores RGB 1.0 at alpha 0, which H0 cannot. The
//! take's declared region B (Inner alone, Normal 75 %) used alpha 0.75; the
//! file stores Inner's opacity as its 8-bit code 191 (the record's own byte),
//! so B's alpha is 191/255 and its stored RGB `1 − 0.5·191/255`: the judged
//! 0.75 missed the bound by that quantization, recorded in the runner. This
//! test asserts the composite against the code the file holds.

use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi_psd::{dr_precision_doc_open_psd, dr_psd_import_report};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::Value;

use crate::common;
use common::{cpath, take_err_string};

const W: u32 = 32;
const H: u32 = 24;
const BASE_VALUES: [[f32; 3]; 9] = [
    [0.0; 3],
    [1e-3; 3],
    [0.18; 3],
    [0.5; 3],
    [1.0; 3],
    [2.5; 3],
    [8.0; 3],
    [16.0; 3],
    [2.0, 0.25, 0.5],
];
const BRIGHT: [f32; 3] = [4.0, 1.5, 0.25];
const MASKED: [f32; 3] = [0.25, 2.0, 8.0];
const INNER: [f32; 3] = [0.5, 0.5, 0.5];
const HIDDEN: [f32; 3] = [1.0, 0.0, 0.0];

fn fixture(name: &str) -> PathBuf {
    common::psd_photoshop::fixture("depth32", name)
}

struct Owned(*mut PrecisionDocument);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { doctored_core::ffi_precision::dr_precision_doc_free(self.0) };
    }
}
impl std::ops::Deref for Owned {
    type Target = PrecisionDocument;
    fn deref(&self) -> &PrecisionDocument {
        unsafe { &*self.0 }
    }
}

fn report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("report JSON")
}

fn open(path: &Path, mode: i32) -> Owned {
    let token =
        std::ffi::CString::new(report(path)["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            mode,
            token.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    Owned(raw)
}

fn within(got: f32, want: f32) -> bool {
    (f64::from(got) - f64::from(want)).abs() <= 1e-3 * f64::from(want).abs() + 1e-6
}

fn inside(rects: &[[u32; 4]], x: u32, y: u32) -> bool {
    rects
        .iter()
        .any(|[rx, ry, rw, rh]| x >= *rx && x < rx + rw && y >= *ry && y < ry + rh)
}

/// A layer's own straight (r, g, b, a) at canvas (x, y), mask off.
fn declared(layer: &str, x: u32, y: u32) -> [f32; 4] {
    let with = |c: [f32; 3], a: f32| [c[0], c[1], c[2], a];
    match layer {
        "Base" => {
            let k = ((y / 4) * 8 + x / 4) as usize;
            let opaque = !inside(&[[4, 4, 28, 4], [0, 8, 32, 16]], x, y);
            with(BASE_VALUES[k.min(8)], if opaque { 1.0 } else { 0.0 })
        }
        "Bright" => with(BRIGHT, if x < 16 { 1.0 } else { 0.0 }),
        "Masked" => with(MASKED, 1.0),
        "Inner" => with(
            INNER,
            if (8..24).contains(&x) && y >= 16 {
                1.0
            } else {
                0.0
            },
        ),
        "Hidden" => with(HIDDEN, 1.0),
        _ => unreachable!(),
    }
}

#[test]
fn photoshops_32_bit_psd_reports_rgba32_with_both_choices() {
    let report = report(&fixture("source32.psd"));
    assert_eq!(report["depth"], 32);
    assert_eq!(report["import_precision"], "rgba32");
    assert_eq!(report["layered_available"], true, "{report}");
    assert_eq!(report["stored_composite_available"], true, "{report}");
    assert_eq!(report["layer_count"], 5);
    assert_eq!(report["group_count"], 1);
}

#[test]
fn photoshops_32_bit_layers_open_with_their_rows_and_samples() {
    let document = open(&fixture("source32.psd"), 0);
    assert!(document.is_float());
    assert_eq!(document.dimensions(), (W, H));
    let names: Vec<&str> = (0..document.layer_count())
        .map(|i| document.layer(i).unwrap().name())
        .collect();
    assert_eq!(
        names,
        ["Base", "Bright", "Masked", "Inner", "Set", "Hidden"]
    );
    let layer = |i: usize| document.layer(i).unwrap();
    assert_eq!(layer(1).blend_mode(), BlendMode::Addition);
    assert_eq!(layer(1).opacity(), 128.0 / 255.0);
    assert_eq!(layer(2).blend_mode(), BlendMode::Multiply);
    assert_eq!(layer(3).opacity(), 191.0 / 255.0);
    assert!(layer(4).is_pass_through());
    assert!(!layer(5).visible());
    // Masked's mask: f32 coverage 1 over x < 16, outside 0.
    let mask = layer(2).mask().expect("Masked's mask");
    assert!(mask.enabled());
    assert_eq!(mask.properties().outside_coverage, 0);
    assert_eq!(mask.properties().offset, (0, 0));
    assert_eq!(mask.plane().dimensions(), (16, 24));
    assert!(mask.plane().samples().iter().all(|v| *v == 65535));
    // Every layer's samples on the compared domain: alpha everywhere (0
    // outside the record), RGB where the declared alpha is above 0.
    for (index, name) in [
        (0, "Base"),
        (1, "Bright"),
        (2, "Masked"),
        (3, "Inner"),
        (5, "Hidden"),
    ] {
        let image = layer(index).source().expect("raster");
        let (ox, oy) = layer(index).offset();
        let (w, h) = image.dimensions();
        for y in 0..H {
            for x in 0..W {
                let want = declared(name, x, y);
                let (lx, ly) = (i64::from(x) - i64::from(ox), i64::from(y) - i64::from(oy));
                let got = if lx >= 0 && ly >= 0 && lx < i64::from(w) && ly < i64::from(h) {
                    image.sample_at(lx as u32, ly as u32).unwrap()
                } else {
                    [0.0; 4]
                };
                assert!(
                    within(got[3], want[3]),
                    "{name} alpha at ({x}, {y}): {got:?}"
                );
                if want[3] > 0.0 {
                    for c in 0..3 {
                        assert!(within(got[c], want[c]), "{name} at ({x}, {y}): {got:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn photoshops_32_bit_stored_composite_is_matted_over_white() {
    let document = open(&fixture("source32.psd"), 1);
    assert!(document.is_float());
    let image = document.layer(0).unwrap().source().unwrap();
    let at = |x: u32, y: u32| image.sample_at(x, y).unwrap();
    // A: nothing shows; the un-matted colour is the canonical 0.
    for y in 8..24 {
        for x in 24..32 {
            assert_eq!(at(x, y), [0.0; 4], "A at ({x}, {y})");
        }
    }
    // B: Inner alone at its stored opacity code.
    let alpha = 191.0f32 / 255.0;
    for y in 16..24 {
        for x in 16..24 {
            let got = at(x, y);
            assert!(within(got[3], alpha), "B alpha at ({x}, {y}): {got:?}");
            for c in 0..3 {
                assert!(within(got[c], 0.5), "B at ({x}, {y}): {got:?}");
            }
        }
    }
    // D: Base patches 4 to 7 alone, opaque, exact.
    for y in 0..4 {
        for x in 16..32 {
            let want = BASE_VALUES[(x / 4) as usize][0];
            assert_eq!(at(x, y), [want, want, want, 1.0], "D at ({x}, {y})");
        }
    }
}
