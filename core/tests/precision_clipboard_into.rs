//! Paste Into / Paste Outside at high depth: the typed paste's
//! `into` placement centred on a caller frame, and the typed `set_mask`
//! edit's `outside_coverage`.
//!
//! Oracles are written here: the offset is `frame origin + floor((frame −
//! payload) / 2)` per axis, computed with `i64::div_euclid` independently of
//! the implementation's expression; mask properties are read back from the
//! document info JSON.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::dr_precision_doc_info;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::{ClipboardPayload, PasteRequest};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 20;
const H: u32 = 16;

fn take_error(error: *mut c_char) -> Option<String> {
    if error.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    Some(text)
}

fn document() -> PrecisionDocument {
    let srgb = IccProfile::srgb();
    let pixels = vec![[1000u16, 2000, 3000, 65535]; (W * H) as usize];
    let image = PrecisionImage::from_rgba16(W, H, pixels, srgb.clone(), None).unwrap();
    PrecisionDocument::from_parts(
        W,
        H,
        srgb,
        Metadata::default(),
        Resolution::default(),
        vec![Layer::raster("Background", image, (0, 0), 1.0, true).unwrap()],
    )
    .unwrap()
}

fn payload(w: u32, h: u32) -> ClipboardPayload {
    let srgb: Arc<IccProfile> = IccProfile::srgb();
    let pixels: Vec<u8> = (0..w * h * 4).map(|i| (i * 7 % 251) as u8).collect();
    ClipboardPayload::from_rgba8(
        &pixels,
        w,
        h,
        srgb.bytes(),
        (3, 4),
        Resolution::default(),
        LIMIT,
        0,
    )
    .unwrap()
}

fn request(value: Value) -> PasteRequest {
    serde_json::from_value(value).unwrap()
}

fn paste(
    doc: &PrecisionDocument,
    payload: &ClipboardPayload,
    value: Value,
) -> Result<PrecisionDocument, String> {
    doc.clipboard_paste(payload, &request(value), LIMIT, 0, None)
}

fn into(frame: [i64; 4]) -> Value {
    json!({"name":"P","anchor":0,"position":"above","placement":"into","frame":frame})
}

/// The independent oracle: frame origin plus the floor of half the slack.
fn centred(frame: [i64; 4], (w, h): (u32, u32)) -> (i32, i32) {
    let x = frame[0] + (frame[2] - i64::from(w)).div_euclid(2);
    let y = frame[1] + (frame[3] - i64::from(h)).div_euclid(2);
    (x as i32, y as i32)
}

#[test]
fn into_centres_the_payload_on_the_frame() {
    let doc = document();
    // Even and odd slack on each axis, and a frame reaching past the canvas.
    for (frame, size) in [
        ([5, 2, 9, 7], (4, 3)),
        ([3, 3, 6, 6], (4, 3)),
        ([0, 0, 8, 5], (4, 3)),
        ([-3, 10, 11, 12], (5, 2)),
    ] {
        let clip = payload(size.0, size.1);
        let placed = paste(&doc, &clip, into(frame)).unwrap();
        assert_eq!(placed.layer_count(), 2);
        assert_eq!(
            placed.layer(1).unwrap().offset(),
            centred(frame, size),
            "frame {frame:?} payload {size:?}"
        );
    }
    // Hand-checked: floor((9 − 4)/2) = 2 → 7, floor((7 − 3)/2) = 2 → 4.
    let placed = paste(&doc, &payload(4, 3), into([5, 2, 9, 7])).unwrap();
    assert_eq!(placed.layer(1).unwrap().offset(), (7, 4));
}

#[test]
fn a_payload_larger_than_the_frame_floors_to_a_negative_slack() {
    let doc = document();
    let clip = payload(12, 10);
    let placed = paste(&doc, &clip, into([1, 1, 5, 4])).unwrap();
    // floor((5 − 12)/2) = −4 → −3; floor((4 − 10)/2) = −3 → −2.
    assert_eq!(placed.layer(1).unwrap().offset(), (-3, -2));
    assert_eq!(
        placed.layer(1).unwrap().offset(),
        centred([1, 1, 5, 4], (12, 10))
    );
}

#[test]
fn into_refuses_a_missing_or_empty_frame() {
    let doc = document();
    let clip = payload(4, 3);
    let missing = json!({"name":"P","anchor":0,"position":"above","placement":"into"});
    let sentence = "placement into requires a frame [x, y, width, height] with a positive size";
    assert_eq!(paste(&doc, &clip, missing).err().as_deref(), Some(sentence));
    for frame in [[0, 0, 0, 5], [0, 0, 5, 0], [2, 2, -1, 4]] {
        assert_eq!(
            paste(&doc, &clip, into(frame)).err().as_deref(),
            Some(sentence),
            "{frame:?}"
        );
    }
}

#[test]
fn a_frame_is_refused_with_any_other_placement() {
    let doc = document();
    let clip = payload(4, 3);
    for placement in ["centered", "in_place"] {
        let value = json!({"name":"P","anchor":0,"position":"above",
            "placement":placement,"frame":[0,0,4,3]});
        assert_eq!(
            paste(&doc, &clip, value).err().as_deref(),
            Some("a paste frame is only valid with placement into"),
            "{placement}"
        );
    }
    // Without a frame the two placements are unchanged.
    let placed = paste(
        &doc,
        &clip,
        json!({"name":"P","anchor":0,"position":"above","placement":"in_place"}),
    )
    .unwrap();
    assert_eq!(placed.layer(1).unwrap().offset(), (3, 4));
}

fn edit(
    doc: &PrecisionDocument,
    value: Value,
    samples: &[u16],
) -> Result<PrecisionDocument, String> {
    let text = CString::new(value.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if let Some(message) = take_error(error) {
        assert!(result.is_null());
        return Err(message);
    }
    assert!(!result.is_null());
    Ok(unsafe { *Box::from_raw(result) })
}

fn mask_info(doc: &PrecisionDocument, layer: usize) -> Value {
    let mut error = ptr::null_mut();
    let info = unsafe { dr_precision_doc_info(doc, &mut error) };
    assert!(take_error(error).is_none());
    let text = unsafe { CStr::from_ptr(info) }.to_str().unwrap().to_owned();
    unsafe { dr_string_free(info) };
    let value: Value = serde_json::from_str(&text).unwrap();
    value["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["index"] == layer)
        .map(|row| row["mask"].clone())
        .unwrap()
}

#[test]
fn typed_set_mask_carries_outside_coverage() {
    let doc = document();
    let samples = vec![0u16, 65535, 32768, 1, 65534, 0];
    let outside = edit(
        &doc,
        json!({"op":"set_mask","layer":0,"width":W,"height":H,"linked":false,
            "outside_coverage":255}),
        &vec![40000u16; (W * H) as usize],
    )
    .unwrap();
    let mask = mask_info(&outside, 0);
    assert_eq!(mask["outside_coverage"], 255);
    assert_eq!(mask["linked"], false);
    assert_eq!(mask["offset"], json!([0, 0]));
    assert_eq!(mask["dimensions"], json!([W, H]));

    // Omitted: the default 0.
    let plain = edit(
        &doc,
        json!({"op":"set_mask","layer":0,"width":3,"height":2,"x":1,"y":1}),
        &samples,
    )
    .unwrap();
    assert_eq!(mask_info(&plain, 0)["outside_coverage"], 0);

    // Only the PSD endpoints are admitted.
    let refused = edit(
        &doc,
        json!({"op":"set_mask","layer":0,"width":3,"height":2,"outside_coverage":7}),
        &samples,
    );
    assert_eq!(
        refused.err().as_deref(),
        Some("invalid typed mask properties or dimensions")
    );
}
