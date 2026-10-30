//! A 32-bit float document's PSD export (superseding its earlier
//! refusal): the report is available at depth 32 for a PSD, a PSB and
//! a layered TIFF in both modes, a gradient or pattern fill layer refuses the
//! layered mode by name, and the operator table no longer carries a PSD row. The
//! 16-bit and 8-bit reports keep their own first sentences. Every oracle is
//! restated literally below; the bytes themselves are `psd_depth32.rs`'s.

use std::ffi::{c_char, c_int, CStr, CString};
use std::fs;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_float::dr_float_operators;
use doctored_core::ffi_psd::{
    dr_doc_psd_export_report, dr_precision_doc_psd_export_report, dr_precision_doc_save_psd,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::RgbaImage;
use serde_json::Value;

use crate::common;
use common::take_err_string;

const FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";
const LAYERS: c_int = 0;
const FLATTENED: c_int = 1;

const FLOAT_PSD_FIRST: &str =
    "Only 32-bit RGB classic PSD is written for a 32-bit float document: \
linear-light float samples in the document's own primaries under its profile's linear (gamma 1.0) twin, uncompressed (Raw) channels and layer \
records in Photoshop's Lr32 block. A style or adjustment layer that 8 and 16 bits would rasterize \
refuses the layered mode instead, and a 32-bit document is never written at a lower depth.";
const FLOAT_PSB_FIRST: &str = "Only 32-bit RGB large-document PSB is written for a 32-bit float \
document: linear-light float samples in the document's own primaries under its profile's linear (gamma 1.0) twin, uncompressed (Raw) channels \
and layer records in Photoshop's Lr32 block. A style or adjustment layer that 8 and 16 bits would \
rasterize refuses the layered mode instead, and a 32-bit document is never written at a lower depth.";
const FLOAT_TIFF_FIRST: &str = "Only a 32-bit RGB TIFF is written for a 32-bit float document: \
a flat image of linear-light float samples with Photoshop's layer block beside it, layer records \
in Photoshop's Lr32 block, uncompressed (Raw) channels, Macintosh byte order, under the profile's \
linear (gamma 1.0) twin. A style or adjustment layer that 8 and 16 bits would rasterize refuses \
the layered mode instead, and a 32-bit document is never written at a lower depth.";
const SIXTEEN_FIRST: &str = "Only 16-bit RGB classic PSD is written for a 16-bit document, with \
PackBits (RLE) channel compression. ZIP, grayscale and CMYK output are not available, and \
a 16-bit document is never written as 8-bit.";
const EIGHT_FIRST: &str = "Only 8-bit RGB classic PSD is written, with PackBits (RLE) channel \
compression. 16-bit, ZIP, grayscale and CMYK output are not available.";

fn float_document() -> PrecisionDocument {
    PrecisionDocument::open_preserving(FIXTURE, REFERENCE_WORKING_BYTES, 0, None)
        .expect("the float fixture opens")
}

fn sixteen_bit_document() -> PrecisionDocument {
    let pixels = vec![[12345u16, 23456, 34567, 65535]; 64];
    let image = PrecisionImage::from_rgba16(8, 8, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}

fn take_json(json: *mut c_char, err: *mut c_char) -> Value {
    assert!(!json.is_null(), "{}", take_err_string(err));
    assert!(err.is_null());
    let value = serde_json::from_str(unsafe { CStr::from_ptr(json) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(json) };
    value
}

fn typed_report(document: &PrecisionDocument, container: c_int, mode: c_int) -> Value {
    let mut err = ptr::null_mut();
    take_json(
        unsafe { dr_precision_doc_psd_export_report(document, container, mode, &mut err) },
        err,
    )
}

fn first_limitation(report: &Value) -> &str {
    report["general_limitations"][0].as_str().unwrap()
}

#[test]
fn float_report_is_available_at_32_bits_for_psd_and_psb() {
    let document = float_document();
    for (container, format, first) in [(0, "PSD", FLOAT_PSD_FIRST), (1, "PSB", FLOAT_PSB_FIRST)] {
        for (mode, name) in [(LAYERS, "layers"), (FLATTENED, "flattened")] {
            let report = typed_report(&document, container, mode);
            assert_eq!(report["format"], format);
            assert_eq!(report["mode"], name);
            assert_eq!(report["depth"], 32);
            assert_eq!(report["available"], true, "{report}");
            assert_eq!(report["refusal"], Value::Null);
            assert!(report["worst_case_bytes"].as_u64().unwrap() > 0);
            assert_eq!(first_limitation(&report), first);
        }
    }
}

#[test]
fn float_layered_tiff_is_available_with_its_own_sentence() {
    let document = float_document();
    for mode in [LAYERS, FLATTENED] {
        let report = typed_report(&document, 2, mode);
        assert_eq!(report["depth"], 32);
        assert_eq!(report["available"], true, "{report}");
        assert_eq!(report["refusal"], Value::Null);
        assert_eq!(first_limitation(&report), FLOAT_TIFF_FIRST);
    }
}

#[test]
fn float_save_writes_a_psd_and_a_32_bit_layered_tiff() {
    let document = float_document();
    let projection = document.render(REFERENCE_WORKING_BYTES, 0, None).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("existing.tif");
    let before = b"not a tiff, and it must survive".to_vec();
    fs::write(&destination, &before).unwrap();
    let path = CString::new(destination.to_str().unwrap()).unwrap();
    let save = |container: c_int, mode: c_int, path: &CString| {
        let mut err = ptr::null_mut();
        let saved = unsafe {
            dr_precision_doc_save_psd(
                &document,
                &projection,
                path.as_ptr(),
                container,
                mode,
                REFERENCE_WORKING_BYTES,
                0,
                &mut err,
            )
        };
        (
            saved,
            if saved {
                String::new()
            } else {
                take_err_string(err)
            },
        )
    };
    for mode in [LAYERS, FLATTENED] {
        assert_eq!(save(2, mode, &path), (true, String::new()));
        let bytes = fs::read(&destination).unwrap();
        assert_ne!(bytes, before);
        assert_eq!(&bytes[..4], b"MM\0*", "Macintosh byte order");
    }
    let psd = CString::new(directory.path().join("float.psd").to_str().unwrap()).unwrap();
    for mode in [LAYERS, FLATTENED] {
        assert_eq!(save(0, mode, &psd), (true, String::new()));
        let bytes = fs::read(directory.path().join("float.psd")).unwrap();
        assert_eq!(&bytes[..4], b"8BPS");
        assert_eq!(u16::from_be_bytes([bytes[22], bytes[23]]), 32);
    }
}

#[test]
fn sixteen_bit_report_keeps_its_depth_and_sentence() {
    let document = sixteen_bit_document();
    for mode in [LAYERS, FLATTENED] {
        let report = typed_report(&document, 0, mode);
        assert_eq!(report["depth"], 16);
        assert_eq!(report["available"], true);
        assert_eq!(report["refusal"], Value::Null);
        assert_eq!(first_limitation(&report), SIXTEEN_FIRST);
    }
}

#[test]
fn eight_bit_report_keeps_its_sentence() {
    let document = DrDocument::from_pixels(RgbaImage::new(8, 8));
    for mode in [LAYERS, FLATTENED] {
        let mut err = ptr::null_mut();
        let report = take_json(
            unsafe { dr_doc_psd_export_report(&document, 0, mode, &mut err) },
            err,
        );
        assert_eq!(report["depth"], 8);
        assert_eq!(first_limitation(&report), EIGHT_FIRST);
    }
}

#[test]
fn operator_table_no_longer_names_a_psd_row() {
    let raw = unsafe { dr_float_operators() };
    let table = take_json(raw, ptr::null_mut());
    let refused = table["refused"].as_array().unwrap();
    assert!(refused.iter().all(|row| {
        let operator = row["operator"].as_str().unwrap();
        !operator.contains("PSD") && !operator.contains("Photoshop")
    }));
}
