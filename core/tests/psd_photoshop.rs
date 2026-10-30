//! The committed Adobe Photoshop 2025 (26.10.0) fixture, through the C API.
//!
//! `core/tests/fixtures/psd_photoshop/adjustments/` is a document Photoshop
//! itself authored (`scripts/photoshop-probe/`, whose README holds the
//! provenance and the pin): a 64 x 64 base raster with one adjustment layer
//! per carried kind, saved at 8 bit (`source.psd`) and 16 bit
//! (`source16.psd`). Every other PSD fixture in this crate is either
//! hand-built from Adobe's tables or written by Doctored itself, so this is
//! the ONE file here whose framing no Doctored code chose.
//!
//! What it pins is therefore the real forms: a `phfl` in colour space 7 (Lab,
//! which is what Photoshop writes for EVERY photo filter), a 20-byte mask
//! body with an EMPTY rectangle on every adjustment record, a
//! layer-information section rounded to a multiple of 4, and a 16-bit file
//! whose records live in an `Lr16` tail block with ZIP-with-prediction
//! channels, which this build reads (`psd_container::layer_info`,
//! `psd_zip`) — so both depths of this file now import layered.
//!
//! The per-kind table below is a MEASUREMENT of what this file yields, taken
//! on 2026-09-16: all eleven kinds carry, with those metas, and the file
//! reports no loss at all. A kind Photoshop wrote in a form this codec does
//! not read would sit here as a pinned decline instead; the table is shaped
//! to say either.

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::ffi_psd::{dr_doc_open_psd, dr_precision_doc_open_psd};
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::{json, Value};

use crate::common;
use common::psd_photoshop::{import_report, open_psd, strings, COMPOSITE, LAYERS};
use common::{cpath, take_err_string};

const CANVAS: (u32, u32) = (64, 64);

fn fixture(name: &str) -> PathBuf {
    common::psd_photoshop::fixture("adjustments", name)
}

/// A typed document handle freed through the C API on drop.
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

fn open_typed(path: &Path, report: &Value, mode: i32) -> Owned {
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            mode,
            token.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null::<DrJobControl>(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "typed open: {}", take_err_string(error));
    Owned(raw)
}

// ----------------------------------------------------- the per-kind table --

/// One record of the fixture: the layer's name (which IS its file key), the
/// report's name for the kind, the Doctored op, whether this file's body is
/// CARRIED, and either the exact meta params or the exact decline reason.
struct Kind {
    key: &'static str,
    name: &'static str,
    op: &'static str,
    carried: bool,
    params: Value,
    decline_reason: Option<&'static str>,
}

fn carried(key: &'static str, name: &'static str, op: &'static str, params: Value) -> Kind {
    Kind {
        key,
        name,
        op,
        carried: true,
        params,
        decline_reason: None,
    }
}

/// The band block Doctored reads out of Photoshop's factory hue wheel: six
/// symmetric bands, 15 deg inner and 30 deg falloff, every control at zero.
fn factory_bands() -> Value {
    let band = |center: f64| {
        json!({"center": center, "falloff": 30.0, "hue": 0.0, "inner": 15.0,
               "lightness": 0.0, "saturation": 0.0})
    };
    json!({
        "reds": band(0.0), "yellows": band(60.0), "greens": band(120.0),
        "cyans": band(180.0), "blues": band(240.0), "magentas": band(300.0),
    })
}

fn kinds() -> Vec<Kind> {
    let neutral = json!({"cyan_red": 0.0, "magenta_green": 0.0, "yellow_blue": 0.0});
    vec![
        carried("nvrt", "Invert", "invert", json!({})),
        // 153 / 255.
        carried("thrs", "Threshold", "threshold", json!({"level": 0.6})),
        carried("post", "Posterize", "posterize", json!({"levels": 4})),
        carried(
            "levl",
            "Levels",
            "levels",
            json!({"black": 0.2, "gamma": 1.5, "white": 0.8}),
        ),
        carried(
            "curv",
            "Curves",
            "curves",
            json!({"rgb": [[0, 0], [64, 80], [255, 255]]}),
        ),
        carried(
            "blnc",
            "Color Balance",
            "color_balance",
            json!({
                "shadows": neutral, "highlights": neutral,
                "midtones": {"cyan_red": 0.25, "magenta_green": -0.1, "yellow_blue": 0.3},
                "preserve_luminosity": false,
            }),
        ),
        carried(
            "hue2",
            "Hue/Saturation",
            "hue_saturation",
            json!({
                "hue": 20.0, "saturation": 0.25, "lightness": -0.1,
                "colorize": false, "colorize_hue": 0.0,
                "colorize_saturation": 0.25, "colorize_lightness": 0.0,
                "bands": factory_bands(),
            }),
        ),
        // Photoshop wrote this as Lab (67.19, 33.14, 72.79) in colour space
        // 7, NOT as the RGB the picker was given; `#ec8a00` is that Lab
        // decoded through the built-in sRGB model.
        carried(
            "phfl",
            "Photo Filter",
            "photo_filter",
            json!({"color": "#ec8a00", "density": 0.4, "preserve_luminosity": true}),
        ),
        carried(
            "expA",
            "Exposure",
            "exposure",
            json!({"evaluation": "document_v2", "exposure": 1.5, "gamma": 2.0,
                   "offset": -0.25}),
        ),
        carried(
            "vibA",
            "Vibrance",
            "vibrance",
            json!({"saturation": -0.2, "vibrance": 0.4}),
        ),
        carried(
            "blwh",
            "Black & White",
            "black_and_white",
            json!({"reds": 0.3, "yellows": -0.5, "greens": 1.2, "cyans": 0.6,
                   "blues": 2.5, "magentas": 0.0, "tint": true,
                   "tint_color": "#998a66"}),
        ),
    ]
}

fn meta(op: &str, params: &Value) -> String {
    json!({"type": "adjust", "op": op, "params": params}).to_string()
}

/// The same meta as a 16-bit document holds it: `psd_adjustment`'s typed
/// description adds the `evaluation` key.
fn typed_meta(op: &str, params: &Value) -> String {
    json!({"type": "adjust", "evaluation": "precision_v1", "op": op, "params": params}).to_string()
}

// ------------------------------------------------------------------ 8 bit --

#[test]
fn the_photoshop_file_imports_layered_with_twelve_rows_and_no_masks() {
    let report = import_report(&fixture("source.psd"));
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    // Before this run BOTH of these refused the whole layered choice: the two
    // trailing pad bytes of Photoshop's multiple-of-4 layer-information
    // section, and the empty mask rectangle on every adjustment record.
    assert_eq!(report["layered_refusal"], Value::Null, "{report}");
    assert_eq!(report["import_precision"], Value::from("rgba8"));
    assert_eq!(report["layer_count"], Value::from(12));
    assert_eq!(report["group_count"], Value::from(0));
    // The whole loss list, not a sample of it: this file costs nothing.
    assert_eq!(
        strings(&report, "verified_losses"),
        Vec::<String>::new(),
        "{report}"
    );

    let doc = open_psd(&fixture("source.psd"), &report, LAYERS);
    assert_eq!(doc.layers.len(), 12);
    assert_eq!(doc.layers[0].name, "Base");
    assert_eq!(doc.layers[0].pixels.dimensions(), CANVAS);
    for layer in &doc.layers {
        assert!(layer.mask.is_none(), "{} has no mask", layer.name);
        assert!(!layer.clipped, "{}", layer.name);
    }
}

#[test]
fn every_kind_the_file_carries_has_its_measured_meta_and_sentence() {
    let report = import_report(&fixture("source.psd"));
    let observations = strings(&report, "observations");
    let losses = strings(&report, "verified_losses");
    let doc = open_psd(&fixture("source.psd"), &report, LAYERS);
    let mut carried_count = 0;
    for kind in kinds() {
        let layer = doc
            .layers
            .iter()
            .find(|l| l.name == kind.key)
            .unwrap_or_else(|| panic!("layer {}", kind.key));
        if kind.carried {
            carried_count += 1;
            assert_eq!(
                layer.meta.as_deref(),
                Some(meta(kind.op, &kind.params).as_str()),
                "{} meta",
                kind.key
            );
            assert!(
                observations.iter().any(|o| o.contains(&format!(
                    "(“{}”) is a Photoshop {} adjustment layer ({}); layered import carries it as a Doctored {} adjustment layer",
                    kind.key, kind.name, kind.key, kind.op))),
                "the {} carry sentence: {observations:?}",
                kind.key
            );
        } else {
            let reason = kind.decline_reason.expect("a decline names its reason");
            assert_eq!(layer.meta, None, "{} is declined", kind.key);
            assert!(
                losses
                    .iter()
                    .any(|l| l.contains(&format!("(“{}”)", kind.key))
                        && l.contains(&format!("with no pixels whose {reason};"))),
                "the {} decline sentence: {losses:?}",
                kind.key
            );
        }
    }
    assert_eq!(carried_count, 11, "every kind in this file carries");
    assert_eq!(
        observations
            .iter()
            .filter(|o| o.contains("layered import carries it"))
            .count(),
        11
    );
}

// ----------------------------------------------------------------- 16 bit --

#[test]
fn the_sixteen_bit_file_imports_layered_with_the_same_twelve_rows() {
    let report = import_report(&fixture("source16.psd"));
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    assert_eq!(report["import_precision"], Value::from("rgba16"));
    // Photoshop's 16-bit records live in an `Lr16` TAIL block with
    // ZIP-with-prediction channels and an EMPTY layer-information section.
    // Before this run that framed no layer at all and the layered choice
    // resolved to the stored composite; now it is the same twelve rows the
    // 8-bit twin has.
    assert_eq!(report["layer_count"], Value::from(12), "{report}");
    assert_eq!(report["group_count"], Value::from(0));
    assert_eq!(
        strings(&report, "verified_losses"),
        Vec::<String>::new(),
        "{report}"
    );

    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_doc_open_psd(
            cpath(&fixture("source16.psd")).as_ptr(),
            LAYERS,
            token.as_ptr(),
            &mut error,
        )
    };
    assert!(raw.is_null(), "the byte entry point must refuse depth 16");
    assert!(
        take_err_string(error).contains("16-bit PSD opens only as a 16-bit document"),
        "the depth refusal names the typed path"
    );

    let composite = open_typed(&fixture("source16.psd"), &report, COMPOSITE);
    assert_eq!(composite.dimensions(), CANVAS);
    assert_eq!(composite.layer_count(), 1);

    let layered = open_typed(&fixture("source16.psd"), &report, LAYERS);
    assert_eq!(layered.layer_count(), 12);
    let names: Vec<String> = (0..12)
        .map(|index| layered.layer(index).expect("layer").name().to_string())
        .collect();
    let mut expected = vec!["Base".to_string()];
    expected.extend(kinds().iter().map(|k| k.key.to_string()));
    assert_eq!(
        names, expected,
        "bottom-first, as the typed stack holds them"
    );
    // Every kind carries the SAME parameters as the 8-bit twin, with the
    // typed `evaluation` key the 16-bit document adds.
    for kind in kinds() {
        let index = names
            .iter()
            .position(|name| name == kind.key)
            .unwrap_or_else(|| panic!("layer {}", kind.key));
        assert_eq!(
            layered.adjustment_description(index),
            Some(typed_meta(kind.op, &kind.params).as_str()),
            "{} typed description",
            kind.key
        );
    }
    // Photoshop's 16-bit adjustment records carry a real canvas-sized mask
    // (its 8-bit twin writes a ZERO rectangle instead), so each one arrives
    // masked; the base raster has none.
    assert!(layered.layer(0).and_then(|l| l.mask()).is_none(), "Base");
    for (index, name) in names.iter().enumerate().skip(1) {
        let mask = layered
            .layer(index)
            .and_then(|layer| layer.mask())
            .unwrap_or_else(|| panic!("mask on {name}"));
        assert_eq!(mask.plane().dimensions(), CANVAS);
        assert_eq!(mask.properties().offset, (0, 0));
        assert!(mask.plane().samples().iter().all(|s| *s == 65535));
    }
}
