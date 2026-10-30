//! v1.x HDR photos: the 32-bit HDR HEIC / AVIF export's core half —
//! `hdr_encoding` parsing and validation, the two pending intermediates, and
//! the `info_json` keys the host reads.
//!
//! The gain-map intermediate is checked as an EQUALITY with the `"tiff"`
//! 32-bit request's bytes; the PQ intermediate is decoded by the `tiff`
//! crate (not the core) and every code compared with the oracle written
//! here: BT.2087's published BT.709 → BT.2020 matrix, ST 2084's inverse EOTF
//! in its own spelling, 1.0 = 203 cd/m², within two 16-bit codes (the
//! matrix's four published decimals). The host's encode is the app's half
//! and its round trip is the drive's.

use std::ffi::{c_char, CStr, CString};
use std::io::Cursor;
use std::ptr;

use crate::common::take_err_string;
use crate::float_fixture::float_doc;
use doctored_core::doc_export::{ExportRequest, ExportResult};
use doctored_core::ffi_export::{
    dr_export_result_free, dr_export_result_info, dr_precision_doc_export_encode,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::Value;
use tempfile::TempDir;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

fn scene() -> Vec<[f32; 4]> {
    vec![
        [0.18, 0.18, 0.18, 1.0],
        [1.0, 1.0, 1.0, 1.0],
        [8.0, 2.0, 0.5, 1.0],
        [0.9, 0.05, 0.02, 0.5],
        [4.926, 4.926, 4.926, 1.0],
        [-0.01, 0.3, 0.6, 1.0],
    ]
}

fn export(document: &PrecisionDocument, json: &str) -> Result<ExportResult, String> {
    let request = ExportRequest::parse(json)?;
    document.export_encoded(None, &request, LIMIT, 0, None)
}

fn pq_signal(nits: f64) -> f64 {
    let y = (nits / 10000.0).clamp(0.0, 1.0);
    let p = y.powf(0.159_301_757_812_5);
    ((0.835_937_5 + 18.851_562_5 * p) / (1.0 + 18.687_5 * p)).powf(78.843_75)
}

/// ITU-R BT.2087's BT.709 → BT.2020 matrix.
const TO_2020: [[f64; 3]; 3] = [
    [0.6274, 0.3293, 0.0433],
    [0.0691, 0.9195, 0.0114],
    [0.0164, 0.0880, 0.8956],
];

#[test]
fn a_gain_map_intermediate_is_the_32_bit_tiff_and_reports_its_headroom() {
    let document = float_doc(3, 2, &IccProfile::srgb(), &[scene()]);
    let tiff = export(&document, r#"{"format":"tiff","bit_depth":32}"#).unwrap();
    for format in ["heic", "avif"] {
        let result = export(
            &document,
            &format!(r#"{{"format":"{format}","bit_depth":32,"hdr_encoding":"gain_map"}}"#),
        )
        .unwrap();
        assert_eq!(result.bytes(), tiff.bytes(), "{format}");
        assert!(result.is_host_pending());
        let info = result.info_json();
        assert_eq!(info["hdr_encoding"], "gain_map");
        assert_eq!(info["content_headroom"], 8.0);
        assert_eq!(info["host_encoded"], true);
        assert_eq!(info["intermediate"], "tiff");
        assert_eq!(info["bit_depth"], 32);
        assert_eq!(info["format"], format);
        // Pending: no disk, no preview.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("out.heic");
        assert!(result
            .write(path.to_str().unwrap(), false)
            .unwrap_err()
            .contains("must adopt the host encoder's bytes"));
        assert!(result.decode_preview().is_err());
        // Adoption keeps the HDR report and clears the intermediate.
        let adopted = result.adopt_host_bytes(vec![1, 2, 3], 10, 0).unwrap();
        let info = adopted.info_json();
        assert_eq!(info["hdr_encoding"], "gain_map");
        assert_eq!(info["content_headroom"], 8.0);
        assert_eq!(info["intermediate"], Value::Null);
        assert_eq!(info["bit_depth"], 10);
    }
    // A document that never passes white still reports headroom 1.
    let dim = float_doc(1, 1, &IccProfile::srgb(), &[vec![[0.5, 0.2, 0.1, 1.0]]]);
    let result = export(
        &dim,
        r#"{"format":"heic","bit_depth":32,"hdr_encoding":"gain_map"}"#,
    )
    .unwrap();
    assert_eq!(result.info_json()["content_headroom"], 1.0);
    // An ordinary result carries neither key.
    assert!(tiff.info_json().get("hdr_encoding").is_none());
    assert!(tiff.info_json().get("content_headroom").is_none());
}

#[test]
fn a_pq_intermediate_holds_bt2100_pq_codes_in_bt2020() {
    let document = float_doc(3, 2, &IccProfile::srgb(), &[scene()]);
    let source = document
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba32()
        .unwrap();
    let result = export(
        &document,
        r#"{"format":"avif","bit_depth":32,"hdr_encoding":"pq"}"#,
    )
    .unwrap();
    assert!(result.is_host_pending());
    let info = result.info_json();
    assert_eq!(info["hdr_encoding"], "pq");
    assert_eq!(info["host_encoded"], true);

    let mut decoder = tiff::decoder::Decoder::new(Cursor::new(result.bytes().to_vec())).unwrap();
    assert_eq!(decoder.dimensions().unwrap(), (3, 2));
    assert!(
        decoder.get_tag(tiff::tags::Tag::IccProfile).is_err(),
        "a PQ intermediate carries no ICC"
    );
    let tiff::decoder::DecodingResult::U16(codes) = decoder.read_image().unwrap() else {
        panic!("a 16-bit TIFF");
    };
    assert_eq!(codes.len(), 6 * 4);
    let mut headroom = 1.0_f64;
    for (i, pixel) in source.iter().enumerate() {
        let rgb = [pixel[0], pixel[1], pixel[2]].map(f64::from);
        let wide: [f64; 3] =
            std::array::from_fn(|r| (0..3).map(|k| TO_2020[r][k] * rgb[k]).sum::<f64>());
        for c in 0..3 {
            headroom = headroom.max(wide[c].min(10000.0 / 203.0));
            let expected = (65535.0 * pq_signal(wide[c] * 203.0)).round();
            let got = f64::from(codes[i * 4 + c]);
            assert!(
                (got - expected).abs() <= 2.0,
                "pixel {i}:{c} code {got} vs {expected}"
            );
        }
        assert_eq!(
            codes[i * 4 + 3],
            (f64::from(pixel[3]) * 65535.0).round() as u16
        );
    }
    let reported = info["content_headroom"].as_f64().unwrap();
    assert!(
        (reported - headroom).abs() < 1e-3,
        "{reported} vs {headroom}"
    );
    // 203 cd/m² white is PQ's 58 %.
    assert!((f64::from(codes[4]) / 65535.0 - 0.5806).abs() < 0.001);

    // A highlight past 10000 cd/m² is held at PQ's ceiling, and so is the
    // headroom it reports.
    let bright = float_doc(
        1,
        1,
        &IccProfile::srgb(),
        &[vec![[100.0, 100.0, 100.0, 1.0]]],
    );
    let result = export(
        &bright,
        r#"{"format":"heic","bit_depth":32,"hdr_encoding":"pq"}"#,
    )
    .unwrap();
    let headroom = result.info_json()["content_headroom"].as_f64().unwrap();
    assert!((headroom - 10000.0 / 203.0).abs() < 1e-9, "{headroom}");
}

#[test]
fn the_ffi_encodes_an_hdr_request_and_reports_it() {
    let document = float_doc(3, 2, &IccProfile::srgb(), &[scene()]);
    let json = CString::new(r#"{"format":"heic","bit_depth":32,"hdr_encoding":"pq"}"#).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_export_encode(
            &document,
            ptr::null(),
            json.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!result.is_null(), "{}", take_err_string(err));
    let info = unsafe { dr_export_result_info(result, ptr::null_mut()) };
    let text = unsafe { CStr::from_ptr(info) }
        .to_str()
        .unwrap()
        .to_string();
    unsafe { doctored_core::ffi::dr_string_free(info) };
    unsafe { dr_export_result_free(result) };
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["hdr_encoding"], "pq");
    assert!(value["content_headroom"].as_f64().unwrap() > 4.0);
}

#[test]
fn hdr_encoding_refusals_name_their_reason() {
    let document = float_doc(1, 1, &IccProfile::srgb(), &[vec![[2.0, 1.0, 0.5, 1.0]]]);
    for json in [
        r#"{"format":"tiff","bit_depth":32,"hdr_encoding":"pq"}"#,
        r#"{"format":"exr","bit_depth":32,"hdr_encoding":"gain_map"}"#,
        r#"{"format":"heic","bit_depth":16,"hdr_encoding":"pq"}"#,
        r#"{"format":"avif","hdr_encoding":"gain_map"}"#,
        r#"{"format":"jpeg","hdr_encoding":"gain_map"}"#,
    ] {
        assert_eq!(
            export(&document, json).err().unwrap(),
            "export hdr_encoding requires bit_depth 32 and heic or avif",
            "{json}"
        );
    }
    assert_eq!(
        export(
            &document,
            r#"{"format":"heic","bit_depth":32,"hdr_encoding":"hlg"}"#
        )
        .err()
        .unwrap(),
        "export hdr_encoding \"hlg\" is not one of gain_map or pq"
    );
    assert_eq!(
        export(
            &document,
            r#"{"format":"heic","bit_depth":32,"hdr_encoding":7}"#
        )
        .err()
        .unwrap(),
        "export request hdr_encoding must be a string"
    );
    // Without hdr_encoding a 32-bit HEIC keeps today's refusal.
    assert_eq!(
        export(&document, r#"{"format":"heic","bit_depth":32}"#)
            .err()
            .unwrap(),
        "a 32-bit float export requires tiff, exr or hdr"
    );
    assert_eq!(
        export(
            &document,
            r#"{"format":"avif","bit_depth":32,"hdr_encoding":"pq","profile":"srgb"}"#
        )
        .err()
        .unwrap(),
        "a 32-bit float export writes the document profile; use profile document"
    );
    // A 16-bit document has no HDR deliverable.
    let image = PrecisionImage::from_rgba16(1, 1, vec![[1, 2, 3, 65535]], IccProfile::srgb(), None)
        .unwrap();
    let plain =
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap();
    assert_eq!(
        export(
            &plain,
            r#"{"format":"heic","bit_depth":32,"hdr_encoding":"gain_map"}"#
        )
        .err()
        .unwrap(),
        "a 32-bit float export requires a 32-bit float document"
    );
}
