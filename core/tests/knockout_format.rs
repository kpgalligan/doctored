//! KNOCKOUT and "Blend Interior Effects as Group" — the FORMAT half of Phase
//! 12E-3: the JSON codec's two new keys and their conditional emission (D9),
//! the identity rule (D2), byte `.rz` word 49 (D10), the typed / float
//! refusals, and the PSD export's per-layer loss. The compositor's own
//! coverage and its tolerance are `knockout_composite.rs`.
//!
//! Every claim here is an EQUALITY, not a tolerance: the whole point of D9
//! and D10 is that a document without the keys is byte-for-byte the file the
//! writer before them produced, and that clearing the last key restores those
//! exact bytes again.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_psd::{dr_doc_psd_export_report, dr_doc_save_psd};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::{Knockout, LayerStyle};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use common::*;

use crate::identity;

const W: u32 = 6;
const H: u32 = 4;
/// D10's word, as the file must carry it — spelled out here rather than
/// imported, so a change to the constant cannot quietly change the claim.
const KNOCKOUT_WORD: u32 = 49;
const LAYERS_MODE: i32 = 0;

// ------------------------------------------------------------- the codec --

#[test]
fn the_canonical_json_omits_both_keys_at_their_defaults_and_round_trips_them_otherwise() {
    // D9: a style at the defaults writes neither key, so every stored style,
    // `.rz`, `LSTY` body, comps payload and committed PSD golden authored
    // before them is byte-identical.
    let plain = parse("{\"fill_opacity\":0.5}");
    let json = plain.to_json();
    assert!(
        !json.contains("knockout") && !json.contains("blend_interior_as_group"),
        "the defaults are not emitted: {json}"
    );
    assert_eq!(
        json,
        parse(
            "{\"fill_opacity\":0.5,\"knockout\":\"none\",\
               \"blend_interior_as_group\":false}"
        )
        .to_json(),
        "spelling the defaults out changes nothing"
    );

    for name in ["shallow", "deep"] {
        let style = parse(&format!("{{\"knockout\":\"{name}\"}}"));
        let echo = style.to_json();
        assert!(echo.contains(&format!("\"knockout\":\"{name}\"")), "{echo}");
        assert_eq!(parse(&echo), style, "{name} round-trips");
    }
    let flagged = parse("{\"blend_interior_as_group\":true,\"fill_opacity\":0.5}");
    let echo = flagged.to_json();
    assert!(echo.contains("\"blend_interior_as_group\":true"), "{echo}");
    assert_eq!(parse(&echo), flagged);
}

#[test]
fn strict_parse_errors_name_the_offending_key_and_lenient_parse_defaults_it() {
    for (json, needle) in [
        ("{\"knockout\":\"sideways\"}", "knockout"),
        ("{\"knockout\":7}", "knockout"),
        (
            "{\"blend_interior_as_group\":\"yes\"}",
            "blend_interior_as_group",
        ),
    ] {
        let message = LayerStyle::from_json(json).expect_err(json);
        assert!(message.contains(needle), "{json}: {message}");
    }
    // LENIENT (the DRDC reader): an unknown knockout string takes `none`, so
    // a file from a newer build keeps everything this build understands.
    let lenient = LayerStyle::from_json_lenient("{\"knockout\":\"sideways\",\"fill_opacity\":0.5}")
        .expect("lenient");
    assert_eq!(lenient.knockout, Knockout::None);
    assert_eq!(lenient.fill_opacity, 0.5);
    // A wrong TYPE stays strict in both modes: it is not an unknown value.
    assert!(LayerStyle::from_json_lenient("{\"blend_interior_as_group\":\"yes\"}").is_err());
}

#[test]
fn d2_knockout_makes_a_style_non_identity_and_the_grouped_flag_alone_does_not() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "a.png", &solid(W, H, WHITE));

    // A knockout renders something, so the style is STORED.
    let knocked = set_style(doc, 0, "{\"knockout\":\"shallow\"}");
    assert!(!parse("{\"knockout\":\"shallow\"}").is_identity());
    assert!(
        ffi_style(knocked, 0).is_some_and(|s| s.contains("\"knockout\":\"shallow\"")),
        "a knockout-only style is stored"
    );

    // The grouped flag alone renders nothing — there is no grouped effect to
    // re-route — so it is an IDENTITY and clears.
    assert!(parse("{\"blend_interior_as_group\":true}").is_identity());
    let cleared = set_style(knocked, 0, "{\"blend_interior_as_group\":true}");
    assert_eq!(
        ffi_style(cleared, 0),
        None,
        "the lone flag clears, like every other identity"
    );
    unsafe { dr_doc_free(cleared) };
}

// -------------------------------------------------------------- byte .rz --

/// A two-layer document; `style` goes on the top entry when given.
fn native_fixture(dir: &TempDir, style: Option<&str>) -> *mut DrDocument {
    let mut doc = doc_from(dir, "bg.png", &solid(W, H, WHITE));
    doc = add_layer(dir, "top.png", doc, 0, &solid(W, H, RED), "Top");
    match style {
        Some(json) => set_style(doc, 1, json),
        None => doc,
    }
}

fn save(doc: *const DrDocument, path: &std::path::Path) -> Vec<u8> {
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe { dr_doc_save_native(doc, c.as_ptr(), std::ptr::null(), &mut err) };
    assert!(ok, "save_native: {}", take_err_string(err));
    std::fs::read(path).expect("the saved file")
}

fn open(path: &std::path::Path) -> Result<*mut DrDocument, String> {
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_open(c.as_ptr(), std::ptr::null(), &mut err) };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

#[test]
fn word_49_sits_inside_45_only_when_a_style_carries_a_key_and_clearing_restores_the_prior_bytes() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("doc.rz");

    // No key: the file is exactly what the prior writer made — 45 first, and
    // the next word is NOT 49.
    let plain = native_fixture(&dir, Some("{\"fill_opacity\":0.5}"));
    let plain_bytes = save(plain, &path);
    assert_eq!(
        identity::drdc_word(&plain_bytes, 0),
        identity::IDENTITY_BYTE
    );
    assert_ne!(
        identity::drdc_word(&plain_bytes, 1),
        KNOCKOUT_WORD,
        "no 49 without a key"
    );

    // With a key: 45 then 49, and nothing else moves.
    let knocked = set_style(plain, 1, "{\"fill_opacity\":0.5,\"knockout\":\"deep\"}");
    let knocked_bytes = save(knocked, &path);
    assert_eq!(
        identity::drdc_word(&knocked_bytes, 0),
        identity::IDENTITY_BYTE
    );
    assert_eq!(
        identity::drdc_word(&knocked_bytes, 1),
        KNOCKOUT_WORD,
        "45 -> 49 -> the prior words"
    );
    // NO TABLE: the file still CLOSES with the identity tail, exactly as the
    // prior writer's did, so 49 added a version word and nothing else.
    let tail = identity::identity_tail(2);
    assert_eq!(
        &knocked_bytes[knocked_bytes.len() - tail..knocked_bytes.len() - tail + 4],
        b"LIDS",
        "word 49 appends no tail of its own"
    );
    assert_eq!(identity::identity_records(&knocked_bytes), 2);

    // The GROUPED flag alone is not enough to keep a style, so pair it with a
    // fill opacity: the word still appears.
    let flagged = set_style(
        knocked,
        1,
        "{\"fill_opacity\":0.5,\"blend_interior_as_group\":true}",
    );
    let flagged_bytes = save(flagged, &path);
    assert_eq!(identity::drdc_word(&flagged_bytes, 1), KNOCKOUT_WORD);

    // Clearing the last key restores the prior writer's EXACT bytes.
    let restored = set_style(flagged, 1, "{\"fill_opacity\":0.5}");
    assert_eq!(
        save(restored, &path),
        plain_bytes,
        "clearing the last key restores the prior bytes"
    );
    unsafe { dr_doc_free(restored) };
}

#[test]
fn a_knockout_document_reopens_with_its_keys_and_resaves_byte_identically() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("doc.rz");
    let doc = native_fixture(
        &dir,
        Some("{\"knockout\":\"shallow\",\"blend_interior_as_group\":true,\"fill_opacity\":0.25}"),
    );
    let first = save(doc, &path);
    let before = ffi_style(doc, 1);
    let flat = flat_pixels(doc);
    unsafe { dr_doc_free(doc) };

    let reopened = open(&path).expect("reopen");
    assert_eq!(ffi_style(reopened, 1), before, "both keys survive the file");
    assert_eq!(flat_pixels(reopened), flat, "and so does the picture");
    let again = dir.path().join("again.rz");
    assert_eq!(
        save(reopened, &again),
        first,
        "reopen -> resave is identical"
    );
    unsafe { dr_doc_free(reopened) };
}

#[test]
fn a_hand_patched_file_refuses_with_each_of_d10s_two_sentences() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("doc.rz");

    // 49 with no such style anywhere.
    let plain = native_fixture(&dir, Some("{\"fill_opacity\":0.5}"));
    let plain_bytes = save(plain, &path);
    unsafe { dr_doc_free(plain) };
    let mut forged = plain_bytes[..8].to_vec();
    forged.extend_from_slice(&KNOCKOUT_WORD.to_le_bytes());
    forged.extend_from_slice(&plain_bytes[8..]);
    let forged_path = dir.path().join("forged.rz");
    std::fs::write(&forged_path, &forged).unwrap();
    let message = open(&forged_path).expect_err("49 with no key must refuse");
    assert!(
        message.ends_with("invalid native knockout word"),
        "{message}"
    );

    // Such a style with no 49.
    let knocked = native_fixture(&dir, Some("{\"knockout\":\"deep\"}"));
    let knocked_bytes = save(knocked, &path);
    unsafe { dr_doc_free(knocked) };
    assert_eq!(identity::drdc_word(&knocked_bytes, 1), KNOCKOUT_WORD);
    let mut stripped = knocked_bytes[..8].to_vec();
    stripped.extend_from_slice(&knocked_bytes[12..]);
    let stripped_path = dir.path().join("stripped.rz");
    std::fs::write(&stripped_path, &stripped).unwrap();
    let message = open(&stripped_path).expect_err("a key with no 49 must refuse");
    assert!(
        message.ends_with("native knockout styles require DRDC 49"),
        "{message}"
    );

    // A BARE 49 — not nested inside 45 — is refused by name, which is also
    // what every older build does with the nested one.
    let mut bare = knocked_bytes[..4].to_vec();
    bare.extend_from_slice(&KNOCKOUT_WORD.to_le_bytes());
    bare.extend_from_slice(&knocked_bytes[8..]);
    let bare_path = dir.path().join("bare.rz");
    std::fs::write(&bare_path, &bare).unwrap();
    let message = open(&bare_path).expect_err("a bare 49 must refuse");
    assert!(
        message.contains("unsupported DRDC version 49"),
        "older builds refuse it by name: {message}"
    );
}

// -------------------------------------------------- the depth refusals --

fn typed_document() -> PrecisionDocument {
    let pixels = vec![[40000u16, 20000, 10000, 65535]; (W * H) as usize];
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Top", image, (0, 0), 1.0, true).unwrap();
    PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Default::default(),
        Default::default(),
        vec![layer],
    )
    .unwrap()
}

/// The edit, applied: the new document on success, the message on refusal.
fn typed_apply(doc: &PrecisionDocument, request: Value) -> Result<Box<PrecisionDocument>, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        doctored_core::ffi_precision_edit::dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if err.is_null() {
        // An exact no-op answers NULL with no message; the caller decides
        // whether that is what it expected.
        return Ok(if out.is_null() {
            Box::new(doc.clone())
        } else {
            unsafe { Box::from_raw(out) }
        });
    }
    assert!(out.is_null());
    let text = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    Err(text)
}

fn typed_edit(doc: &PrecisionDocument, request: Value) -> Result<(), String> {
    typed_apply(doc, request).map(|_| ())
}

/// 12E-3b: the typed depth ACCEPTS both keys and echoes them back verbatim
/// (decision iv's storage rule), where 12E-3 refused them by name.
#[test]
fn a_typed_document_accepts_both_keys_and_echoes_them() {
    let doc = typed_document();
    for (style, key, value) in [
        (json!({"knockout": "deep"}), "\"knockout\":\"deep\"", true),
        (
            json!({"knockout": "shallow", "fill_opacity": 0.5}),
            "\"knockout\":\"shallow\"",
            true,
        ),
        (
            json!({"blend_interior_as_group": true, "fill_opacity": 0.5}),
            "\"blend_interior_as_group\":true",
            true,
        ),
        // Decision (iv)'s stated DROP: a lone flag with nothing to group and
        // nothing else non-default renders nothing at either depth.
        (
            json!({"blend_interior_as_group": true}),
            "blend_interior_as_group",
            false,
        ),
    ] {
        let next = typed_apply(
            &doc,
            json!({"op":"layer_style","version":1,"layer":0,"style":style}),
        )
        .expect("the typed depth accepts both keys");
        let reported = next.layer(0).unwrap().reported_style();
        match value {
            true => assert!(
                reported.as_deref().is_some_and(|s| s.contains(key)),
                "{key} survives: {reported:?}"
            ),
            false => assert!(
                reported.is_none() || !reported.as_deref().unwrap().contains(key),
                "a lone flag is dropped: {reported:?}"
            ),
        }
    }
    // A style with neither key is still accepted, unchanged.
    typed_edit(
        &doc,
        json!({"op":"layer_style","version":1,"layer":0,"style":{"fill_opacity":0.5}}),
    )
    .expect("an ordinary typed style still works");

    // And the model-level rule the edit rests on, asked directly: a
    // knockout-ONLY style is STORED, where 12E-3's filter dropped it.
    let style = LayerStyle::from_json("{\"knockout\":\"deep\"}").unwrap();
    let layer = PrecisionDocumentLayer::raster(
        "Top",
        PrecisionImage::from_rgba16(
            W,
            H,
            vec![[0u16, 0, 0, 65535]; (W * H) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        (0, 0),
        1.0,
        true,
    )
    .unwrap()
    .with_layer_style(Some(&style), (W, H))
    .expect("with_layer_style stores a knockout-only style");
    assert_eq!(layer.style().map(|s| s.knockout), Some(Knockout::Deep));
}

/// A float document holds a knockout style (Photoshop's
/// 32-bit knockout matched the typed rule); what it still cannot hold — an
/// effect blend mode outside its float set — refuses BY NAME with the 32-bit
/// float sentence, not the removed knockout one (decision viii).
#[test]
fn a_float_document_holds_a_knockout_style_and_refuses_by_a_named_sentence() {
    let path = std::path::Path::new("tests/fixtures/float_11f/scene.tiff");
    if !path.exists() {
        // The fixture is committed; a missing one is a real failure, not a
        // skip, so say so rather than passing quietly.
        panic!("the float fixture is missing: {}", path.display());
    }
    let doc = PrecisionDocument::open_preserving(
        path.to_str().expect("a UTF-8 path"),
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .expect("the float fixture opens");
    assert!(typed_edit(
        &doc,
        json!({"op":"layer_style","version":1,"layer":0,"style":{"knockout":"deep"}}),
    )
    .is_ok());
    let message = typed_edit(
        &doc,
        json!({"op":"layer_style","version":1,"layer":0,"style":{"knockout":"deep",
               "effects":[{"type":"outer_glow","blend":"screen"}]}}),
    )
    .expect_err("a float document must refuse");
    assert!(
        message.contains("32-bit float"),
        "the refusal names the DEPTH: {message}"
    );
}

// ----------------------------------------------------------- PSD export --

#[test]
fn the_psd_export_writes_the_knockout_blocks_and_the_old_loss_sentence_is_gone() {
    let dir = TempDir::new().unwrap();
    let report = |doc: *const DrDocument| -> Value {
        let mut err: *mut c_char = ptr::null_mut();
        let json = unsafe { dr_doc_psd_export_report(doc, 0, LAYERS_MODE, &mut err) };
        assert!(!json.is_null(), "report: {}", take_err_string(err));
        let text = unsafe { CStr::from_ptr(json) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(json) };
        serde_json::from_str(&text).expect("report JSON")
    };
    let write = |doc: *const DrDocument, name: &str| -> Vec<u8> {
        let path = dir.path().join(name);
        let c = cpath(&path);
        let mut err: *mut c_char = ptr::null_mut();
        let ok = unsafe {
            dr_doc_save_psd(
                doc,
                ptr::null(),
                c.as_ptr(),
                0,
                LAYERS_MODE,
                std::ptr::null(),
                &mut err,
            )
        };
        assert!(ok, "save_psd: {}", take_err_string(err));
        std::fs::read(&path).unwrap()
    };
    // The sentence 12E-3 shipped, which 12E-3c DELETED: the keys are written
    // now, so a report that still says this would be a lie.
    const OLD: &str = "uses knockout or Blend Interior Effects as Group";
    const WRITTEN: &str =
        "Layer “Top” uses knockout (deep); written as Photoshop's own knko record.";

    let plain = native_fixture(&dir, Some("{\"fill_opacity\":0.5}"));
    let plain_report = report(plain);
    assert!(
        !plain_report.to_string().contains(OLD),
        "no knockout sentence without the keys"
    );
    let plain_bytes = write(plain, "plain.psd");
    assert!(
        !plain_bytes.windows(4).any(|w| w == b"knko"),
        "and no block either"
    );

    let knocked = set_style(plain, 1, "{\"fill_opacity\":0.5,\"knockout\":\"deep\"}");
    let knocked_report = report(knocked);
    assert!(
        !knocked_report.to_string().contains(OLD),
        "the 12E-3 sentence is gone: {knocked_report}"
    );
    let observations = knocked_report["observations"].as_array().unwrap().clone();
    assert!(
        observations.iter().any(|o| o == WRITTEN),
        "the written-style observation: {observations:?}"
    );
    // The block IS in the file now, in Photoshop's own framing.
    let knocked_bytes = write(knocked, "knocked.psd");
    assert_ne!(
        knocked_bytes, plain_bytes,
        "a written key changes the bytes"
    );
    let mut block = b"8BIMknko".to_vec();
    block.extend_from_slice(&4u32.to_be_bytes());
    block.extend_from_slice(&[2, 0, 0, 0]);
    assert!(
        knocked_bytes
            .windows(block.len())
            .any(|window| window == block),
        "the exact `knko` block for Deep"
    );
    unsafe { dr_doc_free(knocked) };
}

/// The safe-API twin of the writer check, so the loss cannot be an artifact
/// of one particular FFI path: a directly built style carrying the keys makes
/// the same `to_json` the setter would.
#[test]
fn a_directly_constructed_style_agrees_with_the_parser_on_both_keys() {
    let mut style = LayerStyle::default();
    style.knockout = Knockout::Deep;
    style.blend_interior_as_group = true;
    style.fill_opacity = 0.5;
    let echo = style.to_json();
    let reparsed = LayerStyle::from_json(&echo).unwrap();
    assert_eq!(reparsed.knockout, Knockout::Deep);
    assert!(reparsed.blend_interior_as_group);
    // Scale Effects carries both, unscaled: neither is a pixel quantity.
    let scaled = Arc::new(style.scaled(2.0));
    assert_eq!(scaled.knockout, Knockout::Deep);
    assert!(scaled.blend_interior_as_group);
}
