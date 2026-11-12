//! A STORED 8-bit document holding a calibrated (`document_v2`)
//! Exposure layer under "Synthetic Bad Gamma", a matrix/TRC profile whose
//! tone curves fail the precision check. Since Assign refuses such a
//! profile over a calibrated layer and a new or edited `document_v2`
//! layer in such a document refuses, no in-app route can create this
//! state any more — only an older file can carry it. This file pins how
//! the core keeps such a file working: it reopens and resaves unchanged,
//! renders, exports and merges down with the calibrated numbers, and
//! switches to `legacy_srgb` with the legacy numbers.
//!
//! # The stored file
//!
//! `tests/fixtures/rz_byte_bad_gamma/calibrated_exposure.rz` is a FORMAT
//! ORACLE, as `psd_10c/*.psd` is — not a golden image. Nothing in this tree
//! regenerates it:
//!
//! - commit: 667f0dc7a9f04fd9a97624a6a4c2920e6c26b715 (667f0dc), the
//!   current writer — see the writer history below for why an older
//!   writer would have produced the same bytes;
//! - builder: a throwaway, never-committed `core/tests/gen_bad_gamma_fixture.rs`
//!   holding `encode_refusals.rs`'s `bad_profile`, its
//!   `common/icc_lut_builders.rs` `#[path]` import, `text`, `byte_document`
//!   and the setup lines of
//!   `an_existing_calibrated_layer_in_a_failing_own_profile_keeps_rendering_at_8_bits`
//!   verbatim (the two-pixel `[200,40,90,255, 10,220,130,255]` document,
//!   "Synthetic Bad Gamma" assigned, an empty layer "Exposure" added at 1
//!   and given the meta
//!   `{"type":"adjust","op":"exposure","params":{"exposure":0.7,"evaluation":"document_v2"}}`),
//!   and one ignored test calling `dr_doc_save_native(doc, $DR_OUT)`;
//! - command: `cd core && DR_OUT=$PWD/tests/fixtures/rz_byte_bad_gamma/calibrated_exposure.rz
//!   cargo test --release --test gen_bad_gamma_fixture -- --ignored write_fixture`;
//! - bytes: 802, sha256
//!   `0bdff81dd527bb65203d2471083ef65eba0d0731f3dfbbf7d99192e033fa6555`.
//!
//! # Writer history
//!
//! For this document a writer from before the layer refusal and the current
//! one write the same bytes, so no scratch worktree was needed:
//!
//! - 95704c3 changed only `adjust_calibrated.rs` and
//!   `ffi_calibrated.rs` in `core/src`. Neither is on `drdc::encode_native`'s
//!   path, and layer `meta` is written verbatim through `put_opt_string`.
//! - From `95704c3^` to 667f0dc, `drdc.rs` (last changed in 295a75b, before
//!   the layer refusal), `drdc_capacity.rs`, `identity_native`,
//!   `mask_outside_native`, `label_native`, `comps_native`, `warp_native`,
//!   `source_transform_native`, `source_refusal_native` and `style_knockout`
//!   are untouched.
//! - The writer changes in that range miss this document:
//!   `native_sources.rs` and `native_sources_assets.rs` changed only inside
//!   the per-asset loop, and this document has no assets;
//!   `evaluation_native` and `float_native` changed only the typed plan's
//!   `header_bytes`; `doc.rs`, `comps_geometry` and `doc_structure*` are
//!   merge and structure code, and the rest are typed, export, clipboard or
//!   new-document code, not byte writer code.
//!
//! The current writer must reproduce the file byte for byte. The planes
//! inside it are encoded by the PNG crate, so a dependency bump that moves
//! the PNG encoder's bytes fails that test ON PURPOSE: the stored file is
//! what an earlier build wrote, and a writer that no longer reproduces it
//! has changed the format.

use crate::common::sha256 as sha;
use crate::identity;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{
    dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width, dr_string_free,
};
use doctored_core::ffi_calibrated::dr_doc_calibrated_refusal;
use doctored_core::ffi_doc::{
    dr_doc_flattened, dr_doc_free, dr_doc_layer_count, dr_doc_layer_image, dr_doc_layer_meta,
    dr_doc_merging_down, dr_doc_open, dr_doc_save_native, dr_doc_with_layer_meta,
};
use doctored_core::ffi_export::{
    dr_doc_export_encode, dr_export_result_bytes, dr_export_result_free,
};
use doctored_core::DrImage;
use identity::drdc_word;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

/// The stored file (see the module doc).
const STORED: &[u8] = include_bytes!("fixtures/rz_byte_bad_gamma/calibrated_exposure.rz");
const STORED_LEN: usize = 802;
const STORED_SHA256: &str = "0bdff81dd527bb65203d2471083ef65eba0d0731f3dfbbf7d99192e033fa6555";

/// The own-profile sentence, written independently of the core.
const OWN_PROFILE_SENTENCE: &str = "Doctored cannot encode colour into \u{201c}Synthetic Bad \
     Gamma\u{201d} \u{2014} its matrix/TRC tone curves fail the precision check (they must be \
     finite and monotone). Convert the document to another profile first (Image \u{25b8} Mode \
     \u{25b8} Convert to Profile\u{2026}).";

/// The `document_v2` render. Oracle, derived by hand: the gamma-0 TRC parses
/// as `Identity`, so Exposure's per-channel gamma is 1 (rule Γ, #2) and
/// each channel is linear as stored; the gain is 2^0.7 ≈ 1.6245, clamped to
/// [0, 1] and rounded to the 8-bit grid. 200·1.6245 = 324.9 → 255, 40 →
/// 64.98 → 65, 90 → 146.2 → 146, 10 → 16.2 → 16, 220 → 357.4 → 255, 130 →
/// 211.2 → 211; alpha untouched.
const V2: [u8; 8] = [255, 65, 146, 255, 16, 255, 211, 255];

/// The `legacy_srgb` render. Oracle, derived by hand: each channel goes
/// through the sRGB EOTF, ×2^0.7 (1.62450), the sRGB OETF, then ×255
/// rounded. 200 → 247.95 → 248, 40 → 52.10 → 52, 90 → 113.31 → 113;
/// 10 decodes on the linear segment (10/255/12.92 = 0.003035), and
/// 0.003035·1.6245 = 0.004931 > 0.0031308 encodes on the power segment →
/// 15.39 → 15; 220 → 1.1627 linear, clamped → 255; 130 → 162.27 → 162;
/// alpha untouched.
const LEGACY: [u8; 8] = [248, 52, 113, 255, 15, 255, 162, 255];

/// The conditional `.rz` words a writer adds inside identity 45 only for
/// state this document does not hold.
const CONDITIONAL_WORDS: [u32; 8] = [56, 53, 51, 49, 43, 41, 38, 28];

unsafe fn text(pointer: *mut c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let string = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    Some(string)
}

/// The RGBA8 bytes of `image`, which is freed here.
fn image_bytes(image: *mut DrImage) -> Vec<u8> {
    assert!(!image.is_null());
    let length = unsafe { dr_image_width(image) * dr_image_height(image) } as usize * 4;
    let bytes = unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(image), length) }.to_vec();
    unsafe { dr_image_free(image) };
    bytes
}

fn flattened(doc: *const DrDocument) -> Vec<u8> {
    let mut err = ptr::null_mut();
    let image = unsafe { dr_doc_flattened(doc, 0, std::ptr::null(), &mut err) };
    assert!(!image.is_null(), "{:?}", unsafe { text(err) });
    image_bytes(image)
}

/// The stored file, copied into `dir` and opened through the FFI.
fn open_stored(dir: &tempfile::TempDir) -> *mut DrDocument {
    let path = dir.path().join("stored.rz");
    std::fs::write(&path, STORED).expect("write the stored copy");
    let path = CString::new(path.to_str().expect("utf8")).expect("path");
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_doc_open(path.as_ptr(), std::ptr::null(), &mut err) };
    assert!(!doc.is_null(), "{:?}", unsafe { text(err) });
    doc
}

#[test]
fn the_stored_file_reopens_and_resaves_byte_for_byte() {
    assert_eq!(STORED.len(), STORED_LEN);
    assert_eq!(sha::sha256_hex(STORED), STORED_SHA256);
    assert_eq!(&STORED[..4], b"RZDC");
    assert_eq!(drdc_word(STORED, 0), 45, "the byte identity wrapper");
    let inner = drdc_word(STORED, 1);
    assert!(
        !CONDITIONAL_WORDS.contains(&inner),
        "no conditional word: {inner}"
    );

    let dir = tempfile::TempDir::new().expect("tempdir");
    let doc = open_stored(&dir);
    let resaved = dir.path().join("resaved.rz");
    let path = CString::new(resaved.to_str().expect("utf8")).expect("path");
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(doc, path.as_ptr(), std::ptr::null(), &mut err) },
        "{:?}",
        unsafe { text(err) }
    );
    unsafe { dr_doc_free(doc) };
    assert!(
        std::fs::read(&resaved).expect("read the resave")
            == [b"DRDC".as_slice(), &STORED[4..]].concat(),
        "the current writer reproduces the stored file under its own magic"
    );
}

#[test]
fn the_stored_calibrated_layer_keeps_rendering_exporting_and_merging() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let doc = open_stored(&dir);
    assert_eq!(unsafe { dr_doc_layer_count(doc) }, 2);
    let meta = unsafe { text(dr_doc_layer_meta(doc, 1)) }.expect("layer 1 meta");
    let meta: Value = serde_json::from_str(&meta).expect("meta JSON");
    assert_eq!(
        meta,
        json!({
            "type": "adjust",
            "op": "exposure",
            "params": {"exposure": 0.7, "evaluation": "document_v2"},
        })
    );
    // Gamma 0 makes the curves repaired (`precision_compatible` false), so a
    // NEW Exposure is refused too, not only a White Balance (#2).
    let exposure = CString::new("exposure").expect("op");
    assert_eq!(
        unsafe { text(dr_doc_calibrated_refusal(doc, exposure.as_ptr())) }.as_deref(),
        Some(OWN_PROFILE_SENTENCE)
    );

    assert_eq!(flattened(doc), V2, "render");

    let request = CString::new(r#"{"format":"png"}"#).expect("request");
    let mut err = ptr::null_mut();
    let result =
        unsafe { dr_doc_export_encode(doc, ptr::null(), request.as_ptr(), ptr::null(), &mut err) };
    assert!(!result.is_null(), "{:?}", unsafe { text(err) });
    let mut length = 0usize;
    let pointer = unsafe { dr_export_result_bytes(result, &mut length) };
    assert!(!pointer.is_null());
    let png = unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec();
    unsafe { dr_export_result_free(result) };
    let decoded = image::load_from_memory(&png).expect("PNG").to_rgba8();
    assert_eq!(decoded.into_raw(), V2, "export");

    let merged = unsafe { dr_doc_merging_down(doc, 1) };
    assert!(!merged.is_null());
    assert_eq!(unsafe { dr_doc_layer_count(merged) }, 1);
    assert_eq!(
        image_bytes(unsafe { dr_doc_layer_image(merged, 0) }),
        V2,
        "merged"
    );
    unsafe { dr_doc_free(merged) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn switching_the_stored_layer_to_legacy_srgb_renders_the_legacy_numbers() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let doc = open_stored(&dir);
    let meta = json!({
        "type": "adjust",
        "op": "exposure",
        "params": {"exposure": 0.7, "evaluation": "legacy_srgb"},
    })
    .to_string();
    let meta = CString::new(meta).expect("meta");
    let switched = unsafe { dr_doc_with_layer_meta(doc, 1, meta.as_ptr()) };
    assert!(!switched.is_null());
    assert_eq!(flattened(switched), LEGACY, "legacy render");
    assert_eq!(flattened(doc), V2, "the source is unchanged");
    unsafe { dr_doc_free(switched) };
    unsafe { dr_doc_free(doc) };
}
