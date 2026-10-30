//! Move and Free Transform of a 24 MP float layer's selected pixels are
//! admitted under their measured plan. An opaque 6000×4000 float layer in an
//! sRGB document and a selection of rect (300, 200, 5400, 3600); M moves it
//! by (37, 23) and T turns it 7° about the canvas centre, bilinear, both
//! carrying that rect as `bounds`. One 24 MP document at a time: one `#[test]`.
//!
//! Each scratch is at least ⌈1.25 × the largest measured footprint E⌉ in
//! `precision_selected_pixels`'s module doc (`# Admission`): one case
//! per process, three runs, `MallocLargeCache=0`, a baseline twin.
use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::precision_clipboard::ClipboardPayload;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::{json, Value};

const W: u32 = 6000;
const H: u32 = 4000;
/// ⌈1.25 × max E⌉ for M.
const FLOOR_M: u64 = 834_663_030;
/// ⌈1.25 × max E⌉ for T.
const FLOOR_T: u64 = 1_323_869_120;
/// The measurement's limit: T's plan plus the document's retained bytes
/// passes the 2 GiB test pin, so the plan is read where it was measured.
const MEASURED_LIMIT: u64 = 8_589_934_592;

/// The measured document: RZCB v3 f32, opaque, ramps and a hashed blue.
fn document() -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let mut bytes = b"RZCB".to_vec();
    for word in [3u32, 2, W, H, 0, 0] {
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend(72f32.to_le_bytes());
    bytes.extend(72f32.to_le_bytes());
    bytes.extend((profile.bytes().len() as u32).to_le_bytes());
    bytes.extend(profile.bytes().iter());
    bytes.push(0);
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(4f32.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.reserve(W as usize * H as usize * 16);
    for i in 0..u64::from(W) * u64::from(H) {
        let (x, y) = ((i % u64::from(W)) as f32, (i / u64::from(W)) as f32);
        let pixel = [
            -0.25 + 4.0 * x / W as f32,
            0.02 + 2.0 * y / H as f32,
            ((i * 2654435761) % 1000) as f32 / 500.0,
            1.0,
        ];
        pixel.iter().for_each(|v| bytes.extend(v.to_le_bytes()));
    }
    let payload = ClipboardPayload::decode(&bytes, REFERENCE_WORKING_BYTES, 0).unwrap();
    drop(bytes);
    let doc =
        PrecisionDocument::from_clipboard(&payload, REFERENCE_WORKING_BYTES, 0, None).unwrap();
    assert!(doc.is_float());
    doc
}

fn bounds() -> Value {
    json!([300, 200, 5400, 3600])
}

fn moved() -> String {
    json!({"op":"move_selected","layer":0,"dx":37,"dy":23,"bounds":bounds()}).to_string()
}

fn turned() -> String {
    let t = 7f64.to_radians();
    let (c, s) = (t.cos(), t.sin());
    let (cx, cy) = (f64::from(W) / 2.0, f64::from(H) / 2.0);
    let m = [c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy];
    json!({"op":"transform_selected","layer":0,"matrix":m,"filter":"bilinear","bounds":bounds()})
        .to_string()
}

fn scratch(doc: &PrecisionDocument, text: &str) -> u64 {
    let text = CString::new(text).unwrap();
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), MEASURED_LIMIT, 0, &mut err) };
    if !err.is_null() {
        let message = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        panic!("the plan refuses: {message}");
    }
    assert!(!out.is_null(), "the plan answers");
    let value: Value =
        serde_json::from_str(&unsafe { CStr::from_ptr(out) }.to_string_lossy()).unwrap();
    unsafe { dr_string_free(out) };
    value["scratch_bytes"].as_u64().unwrap()
}

#[test]
fn a_float_selected_pixels_edit_is_admitted_at_24mp() {
    let doc = document();
    let m = scratch(&doc, &moved());
    println!("M scratch {m}");
    assert!(m >= FLOOR_M, "M: {m} < floor {FLOOR_M}");
    let t = scratch(&doc, &turned());
    println!("T scratch {t}");
    assert!(t >= FLOOR_T, "T: {t} < floor {FLOOR_T}");
    let coverage: Vec<u16> = (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            if (300..5700).contains(&x) && (200..3800).contains(&y) {
                u16::MAX
            } else {
                0
            }
        })
        .collect();
    let text = CString::new(moved()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            &doc,
            text.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null(), "the move is admitted");
    assert!(!out.is_null(), "the move changes the layer");
    drop(unsafe { Box::from_raw(out) });
}
