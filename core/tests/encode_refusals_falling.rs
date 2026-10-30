//! The falling-boundary refusal split (#578), through the FFI: a matrix/TRC
//! profile whose `para` steps DOWN at its branch boundary but whose
//! parameters are usable ("Synthetic Falling Boundary") admits a NEW
//! calibrated Exposure at both depths — the asker, the destructive apply, a
//! typed layer, the promotion plan and Assign — and refuses White Balance with
//! the own-profile and Assign sentences (literal oracles).
//!
//! The profile is a type-3 `para` (g 2.2, a 0.9, b 0.1, c 1, d 0.5: the toe
//! ends at 0.5, the power branch starts at 0.55^2.2 = 0.268), built byte by
//! byte (`common/icc_lut_builders`). The documents and outcome helpers are
//! `encode_refusals.rs`'s, whose module this split out of.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_calibrated::{
    dr_doc_calibrated_refusal, dr_precision_doc_calibrated_refusal,
};
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_with_layer_meta, dr_doc_with_layer_name};
use doctored_core::ffi_insert::dr_doc_adding_layer_at;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::ffi_precision_conversion::dr_doc_promote_structure_plan;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::Value;

use crate::encode_refusals::{
    byte_adjust, byte_assigns, byte_document, text, typed_assigns, typed_document, typed_outcome,
};
use crate::icc_lut_builders as builders;
use builders::{build, tag_desc, tag_wtpt};

/// The own-profile sentence for the falling-boundary profile, written
/// independently of the core.
const FALLING_OWN_SENTENCE: &str = "Doctored cannot encode colour into \u{201c}Synthetic Falling \
     Boundary\u{201d} \u{2014} its matrix/TRC tone curves fail the precision check (they must be \
     finite and monotone). Convert the document to another profile first (Image \u{25b8} Mode \
     \u{25b8} Convert to Profile\u{2026}).";

/// The calibrated-layer Assign sentence for the falling-boundary
/// profile and the layer "White Balance", written independently of the core.
const FALLING_ASSIGN_SENTENCE: &str = "Doctored cannot assign \u{201c}Synthetic Falling \
     Boundary\u{201d} to a document whose layer \u{201c}White Balance\u{201d} uses calibrated \
     Exposure/White Balance (document_v2) \u{2014} its matrix/TRC tone curves fail the precision \
     check (they must be finite and monotone), and that layer would have to evaluate through \
     them. Merge it down or delete it first, or choose another profile.";

/// A `para` tag of function `kind` with `params` as s15Fixed16.
fn tag_para(kind: u16, params: &[f64]) -> Vec<u8> {
    let mut tag = b"para\0\0\0\0".to_vec();
    tag.extend_from_slice(&kind.to_be_bytes());
    tag.extend_from_slice(&[0, 0]);
    for p in params {
        tag.extend_from_slice(&((p * 65536.0).round() as i32).to_be_bytes());
    }
    tag
}

/// An RGB matrix/TRC profile whose three TRCs are a type-3 `para` (g 2.2,
/// a 0.9, b 0.1, c 1, d 0.5) whose linear toe ends ABOVE the power branch's
/// start: every parameter is usable, but the curve steps down at d, which
/// fails the precision check.
fn falling_profile() -> Vec<u8> {
    let curve = tag_para(3, &[2.2, 0.9, 0.1, 1.0, 0.5]);
    let xyz = |v: [f64; 3]| tag_wtpt(v);
    build(
        [0x02, 0x10, 0, 0],
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("Synthetic Falling Boundary")),
            (b"rXYZ", xyz([0.4361, 0.2225, 0.0139])),
            (b"gXYZ", xyz([0.3851, 0.7169, 0.0971])),
            (b"bXYZ", xyz([0.1431, 0.0606, 0.7141])),
            (b"rTRC", curve.clone()),
            (b"gTRC", curve.clone()),
            (b"bTRC", curve),
        ],
    )
}

/// A typed `document_v2` adjustment description of `op`.
fn typed_calibrated(op: &str) -> CString {
    let params = if op == "exposure" {
        serde_json::json!({"exposure": 0.7, "evaluation": "document_v2"})
    } else {
        serde_json::json!({"temperature": 5200, "evaluation": "document_v2"})
    };
    CString::new(
        serde_json::json!({
            "type": "adjust", "evaluation": "precision_v1", "op": op, "params": params,
        })
        .to_string(),
    )
    .expect("description")
}

/// A byte `document_v2` adjustment meta of `op`.
fn byte_calibrated(op: &str) -> CString {
    let params = if op == "exposure" {
        serde_json::json!({"exposure": 0.7, "evaluation": "document_v2"})
    } else {
        serde_json::json!({"temperature": 5200, "evaluation": "document_v2"})
    };
    CString::new(serde_json::json!({"type": "adjust", "op": op, "params": params}).to_string())
        .expect("meta")
}

#[test]
fn a_falling_boundary_profile_admits_exposure_and_refuses_white_balance_at_both_depths() {
    let falling = falling_profile();
    let exposure = CString::new("exposure").expect("op");
    let white_balance = CString::new("white_balance").expect("op");

    // The asker, at both depths: Exposure admitted, White Balance (and NULL,
    // which answers as White Balance) refused with the own-profile sentence.
    let byte = byte_document(Some(&falling));
    let typed = typed_document(Some(&falling));
    assert_eq!(
        unsafe { text(dr_doc_calibrated_refusal(byte, exposure.as_ptr())) },
        None
    );
    assert_eq!(
        unsafe {
            text(dr_precision_doc_calibrated_refusal(
                typed,
                exposure.as_ptr(),
            ))
        },
        None
    );
    for op in [white_balance.as_ptr(), ptr::null()] {
        assert_eq!(
            unsafe { text(dr_doc_calibrated_refusal(byte, op)) }.as_deref(),
            Some(FALLING_OWN_SENTENCE)
        );
        assert_eq!(
            unsafe { text(dr_precision_doc_calibrated_refusal(typed, op)) }.as_deref(),
            Some(FALLING_OWN_SENTENCE)
        );
    }

    // The destructive apply.
    assert_eq!(
        byte_adjust(
            byte,
            "exposure",
            r#"{"exposure":0.7,"evaluation":"document_v2"}"#
        ),
        Ok(()),
        "byte exposure"
    );
    assert_eq!(
        byte_adjust(
            byte,
            "white_balance",
            r#"{"temperature":5200,"evaluation":"document_v2"}"#
        ),
        Err(FALLING_OWN_SENTENCE.to_string()),
        "byte white balance"
    );

    // A new typed adjustment layer.
    for (op, want) in [
        ("exposure", Ok(())),
        ("white_balance", Err(FALLING_OWN_SENTENCE.to_string())),
    ] {
        let name = CString::new(op).expect("name");
        let description = typed_calibrated(op);
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_add_adjustment(
                typed,
                name.as_ptr(),
                description.as_ptr(),
                1.0,
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert_eq!(typed_outcome(out, err), want, "typed {op} layer");
    }
    unsafe { dr_precision_doc_free(typed) };

    // The 16-bit promotion plan of a byte document holding each layer:
    // the Exposure carries, the White Balance is blocked by name.
    for op in ["exposure", "white_balance"] {
        let name = CString::new(op).expect("name");
        let added = unsafe { dr_doc_adding_layer_at(byte, 0, 0, name.as_ptr()) };
        assert!(!added.is_null());
        let meta = byte_calibrated(op);
        let doc = unsafe { dr_doc_with_layer_meta(added, 1, meta.as_ptr()) };
        assert!(!doc.is_null());
        unsafe { dr_doc_free(added) };
        let mut err = ptr::null_mut();
        let plan = unsafe { text(dr_doc_promote_structure_plan(doc, &mut err)) }
            .unwrap_or_else(|| panic!("{:?}", unsafe { text(err) }));
        let plan: Value = serde_json::from_str(&plan).expect("plan JSON");
        let blocked = plan["blocked"].as_array().expect("blocked rows");
        if op == "exposure" {
            assert!(blocked.is_empty(), "{plan}");
        } else {
            assert_eq!(blocked[0]["index"], 1, "{plan}");
            assert!(
                blocked[0]["reason"]
                    .as_str()
                    .is_some_and(|reason| reason.contains("Synthetic Falling Boundary")),
                "{plan}"
            );
        }
        unsafe { dr_doc_free(doc) };
    }
    unsafe { dr_doc_free(byte) };

    // Assign into the profile from sRGB, at both depths: a calibrated
    // Exposure layer is admitted, a White Balance one refused by name.
    let refused = || Err(FALLING_ASSIGN_SENTENCE.to_string());
    for op in ["exposure", "white_balance"] {
        let layer = if op == "exposure" {
            "Exposure"
        } else {
            "White Balance"
        };
        let name = CString::new(layer).expect("name");
        let plain = byte_document(None);
        let meta = byte_calibrated(op);
        let adjusted = unsafe { dr_doc_with_layer_meta(plain, 0, meta.as_ptr()) };
        assert!(!adjusted.is_null());
        unsafe { dr_doc_free(plain) };
        let doc = unsafe { dr_doc_with_layer_name(adjusted, 0, name.as_ptr()) };
        assert!(!doc.is_null());
        unsafe { dr_doc_free(adjusted) };
        let want = if op == "exposure" {
            [Ok(()), Ok(()), Ok(())]
        } else {
            [refused(), refused(), refused()]
        };
        assert_eq!(byte_assigns(doc, &falling), want, "byte {op}");
        unsafe { dr_doc_free(doc) };

        let plain = typed_document(None);
        let description = typed_calibrated(op);
        let mut err = ptr::null_mut();
        let doc = unsafe {
            dr_precision_doc_add_adjustment(
                plain,
                name.as_ptr(),
                description.as_ptr(),
                1.0,
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!doc.is_null(), "{:?}", unsafe { text(err) });
        unsafe { dr_precision_doc_free(plain) };
        assert_eq!(typed_assigns(doc, &falling), want, "typed {op}");
        unsafe { dr_precision_doc_free(doc) };
    }
}
