//! Smart Sharpen as the export workspace's output sharpening: the
//! request's `sharpen` object with `"method": "smart_sharpen"`
//! (`export_sharpen`). The checks, declared before the first run:
//!
//! - At size none, an 8-bit export's decoded pixels equal
//!   `dr_image_filter_op("smart_sharpen")` of the flattened image, and a
//!   16-bit PNG's bytes equal the document after the `smart_sharpen` selected
//!   edit exported without sharpening.
//! - `"method": "unsharp_mask"` gives the method-less request's bytes at both
//!   depths.
//! - The typed plan grows by at least the kernel's n×16 plane, its 4n stage
//!   margin and two bookkeeping MiB, plus the u16 output's 8n.
//! - A 32-bit export refuses it ("a 32-bit float export cannot sharpen"); an
//!   unknown method and a Smart key refuse by name; the Unsharp Mask
//!   refusals keep their words.
use crate::common;
use common::take_err_string;

use doctored_core::doc::DrDocument;
use doctored_core::doc_export::{ExportRequest, ExportResult};
use doctored_core::ffi::{
    dr_image_free, dr_image_from_rgba, dr_image_height, dr_image_pixels_rgba, dr_image_width,
};
use doctored_core::ffi_doc::{dr_doc_flattened, dr_doc_free, dr_doc_from_image};
use doctored_core::ffi_export::{
    dr_doc_export_encode, dr_export_result_bytes, dr_export_result_free, dr_export_result_preview,
};
use doctored_core::ffi_filters::dr_image_filter_op;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::selected_adjustment::SelectedEdit;
use doctored_core::DrImage;
use serde_json::{json, Value};
use std::ffi::{c_char, CString};
use std::ptr;

const W: u32 = 30;
const H: u32 = 22;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;

fn plate() -> Vec<u8> {
    (0..W * H)
        .flat_map(|i| {
            let (x, y) = (i % W, i / W);
            let ramp = 30 + x * 5 + y * 4;
            let edge = if x > W / 2 { 80 } else { 0 };
            let texture = (x * 7 + y * 13) % 11 * 3;
            let value = |offset: u32| ((ramp + edge + texture + offset) % 200 + 25) as u8;
            [
                value(0),
                value(60),
                value(130),
                if i % 6 == 1 { 120 } else { 255 },
            ]
        })
        .collect()
}

fn smart() -> Value {
    json!({"amount": 160, "radius": 1.5, "noise": 20, "remove": "lens",
        "shadow_fade": 30, "highlight_fade": 20})
}

fn image_bytes(image: *const DrImage) -> Vec<u8> {
    let w = unsafe { dr_image_width(image) } as usize;
    let h = unsafe { dr_image_height(image) } as usize;
    let p = unsafe { dr_image_pixels_rgba(image) };
    assert!(!p.is_null());
    unsafe { std::slice::from_raw_parts(p, w * h * 4) }.to_vec()
}

fn byte_export(doc: *const DrDocument, flat: *const DrImage, request: &Value) -> *mut ExportResult {
    let json = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let result = unsafe { dr_doc_export_encode(doc, flat, json.as_ptr(), ptr::null(), &mut err) };
    assert!(!result.is_null(), "{request}: {}", take_err_string(err));
    result
}

fn result_bytes(result: *const ExportResult) -> Vec<u8> {
    let mut length = 0usize;
    let pointer = unsafe { dr_export_result_bytes(result, &mut length) };
    assert!(!pointer.is_null());
    unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec()
}

fn typed_doc() -> PrecisionDocument {
    let codes = plate()
        .chunks_exact(4)
        .map(|p| [0, 1, 2, 3].map(|c| u16::from(p[c]) * 257))
        .collect();
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(W, H, codes, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

fn typed_bytes(doc: &PrecisionDocument, request: &Value) -> Vec<u8> {
    let request = ExportRequest::parse(&request.to_string()).unwrap();
    doc.export_encoded(None, &request, LIMIT, 0, None)
        .unwrap()
        .bytes()
        .to_vec()
}

#[test]
fn an_8_bit_export_is_the_filter_applied_then_exported() {
    let pixels = plate();
    let image = unsafe { dr_image_from_rgba(pixels.as_ptr(), W, H) };
    let doc = unsafe { dr_doc_from_image(image) };
    unsafe { dr_image_free(image) };
    let mut err: *mut c_char = ptr::null_mut();
    let flat = unsafe { dr_doc_flattened(doc, 0, ptr::null(), &mut err) };
    assert!(
        !doc.is_null() && !flat.is_null(),
        "{}",
        take_err_string(err)
    );

    let mut sharpen = smart();
    sharpen["method"] = json!("smart_sharpen");
    let result = byte_export(doc, flat, &json!({"format": "png", "sharpen": sharpen}));
    let preview = unsafe { dr_export_result_preview(result, &mut err) };
    assert!(!preview.is_null(), "{}", take_err_string(err));
    let exported = image_bytes(preview);

    let op = CString::new("smart_sharpen").unwrap();
    let params = CString::new(smart().to_string()).unwrap();
    let filtered =
        unsafe { dr_image_filter_op(flat, op.as_ptr(), params.as_ptr(), ptr::null(), &mut err) };
    assert!(!filtered.is_null(), "{}", take_err_string(err));
    assert!(exported == image_bytes(filtered), "the export differs");

    // The explicit Unsharp Mask method is the method-less request.
    let usm = json!({"radius": 1.2, "amount": 1.4, "threshold": 0.01});
    let mut named = usm.clone();
    named["method"] = json!("unsharp_mask");
    let (a, b) = (
        byte_export(doc, flat, &json!({"format": "png", "sharpen": usm})),
        byte_export(doc, flat, &json!({"format": "png", "sharpen": named})),
    );
    assert!(
        result_bytes(a) == result_bytes(b),
        "unsharp_mask by name differs"
    );
    unsafe {
        dr_export_result_free(a);
        dr_export_result_free(b);
        dr_image_free(filtered);
        dr_image_free(preview);
        dr_export_result_free(result);
        dr_image_free(flat);
        dr_doc_free(doc);
    }
}

#[test]
fn a_16_bit_export_is_the_filter_applied_then_exported() {
    let doc = typed_doc();
    let mut sharpen = smart();
    sharpen["method"] = json!("smart_sharpen");
    let exported = typed_bytes(
        &doc,
        &json!({"format": "png", "bit_depth": 16, "sharpen": sharpen}),
    );
    let edit = SelectedEdit::parse(
        &json!({"layer": 0, "op": "smart_sharpen", "params": smart()}).to_string(),
    )
    .unwrap();
    let edited = doc
        .selected_edit(&edit, &[], LIMIT, 0, None)
        .unwrap()
        .expect("an edit");
    let applied = typed_bytes(&edited, &json!({"format": "png", "bit_depth": 16}));
    assert!(exported == applied, "the 16-bit export differs");

    let usm = json!({"radius": 1.2, "amount": 1.4, "threshold": 0.01});
    let mut named = usm.clone();
    named["method"] = json!("unsharp_mask");
    assert!(
        typed_bytes(
            &doc,
            &json!({"format": "png", "bit_depth": 16, "sharpen": usm})
        ) == typed_bytes(
            &doc,
            &json!({"format": "png", "bit_depth": 16, "sharpen": named})
        ),
        "unsharp_mask by name differs"
    );
}

#[test]
fn the_typed_plan_charges_the_kernel() {
    let doc = typed_doc();
    let plan = |request: Value| -> u64 {
        doc.export_plan(None, &ExportRequest::parse(&request.to_string()).unwrap())
            .unwrap()["scratch_bytes"]
            .as_u64()
            .unwrap()
    };
    let plain = plan(json!({"format": "png", "bit_depth": 16}));
    let sharp = plan(json!({"format": "png", "bit_depth": 16, "sharpen":
        {"method": "smart_sharpen", "remove": "gaussian", "noise": 0}}));
    let n = u64::from(W * H);
    let mib = 1024 * 1024;
    assert!(
        sharp - plain >= 16 * n + 4 * n + 2 * mib + 8 * n,
        "{} for {n} pixels",
        sharp - plain
    );
}

#[test]
fn refusals_name_their_cause() {
    for (sharpen, wanted) in [
        (
            json!({"method": "bilateral"}),
            "export sharpen method must be unsharp_mask or smart_sharpen",
        ),
        (
            json!({"method": 3}),
            "export sharpen method must be unsharp_mask or smart_sharpen",
        ),
        (
            json!({"method": "smart_sharpen", "amount": 0}),
            "export sharpen: amount must be finite in [1,500]",
        ),
        (
            json!({"method": "smart_sharpen", "threshold": 0.1}),
            "export sharpen: unexpected smart_sharpen parameter threshold",
        ),
        (
            json!({"method": "unsharp_mask", "noise": 10}),
            "export sharpen has an unknown key \"noise\"",
        ),
        (
            json!({"radius": 65}),
            "export sharpen radius must be in [0,64] pixels",
        ),
    ] {
        let request = json!({"format": "png", "sharpen": sharpen}).to_string();
        assert_eq!(
            ExportRequest::parse(&request).err().as_deref(),
            Some(wanted),
            "{request}"
        );
    }
    let request = json!({"format": "tiff", "bit_depth": 32,
        "sharpen": {"method": "smart_sharpen"}})
    .to_string();
    assert_eq!(
        ExportRequest::parse(&request).err().as_deref(),
        Some("a 32-bit float export cannot sharpen")
    );
}

/// The typed export plan of a 6000×4000 16-bit PNG at size none with the
/// admission's Smart Sharpen (`scripts/smart-sharpen-admission/measure.py`'s
/// PE16): the source's 8n, the sharpen term (the kernel's 774,357,724 —
/// `filter_smart_sharpen.rs`'s derivation — plus the u16 output's 8n), the
/// PNG encoder's 48n + 16·6000 + 128 MiB and one bookkeeping MiB.
const PLAN_E16: u64 = 2_445_720_028;

#[test]
fn the_24_megapixel_typed_plan_is_pinned() {
    let (w, h) = (6000_u32, 4000_u32);
    let doc = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            w,
            h,
            vec![[0_u16; 4]; (w * h) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let request = json!({"format": "png", "bit_depth": 16, "sharpen": {"method": "smart_sharpen",
        "amount": 200, "radius": 4, "noise": 50, "remove": "lens",
        "shadow_fade": 50, "shadow_tonal_width": 50, "shadow_radius": 8,
        "highlight_fade": 50, "highlight_tonal_width": 50, "highlight_radius": 8}});
    let planned = doc
        .export_plan(None, &ExportRequest::parse(&request.to_string()).unwrap())
        .unwrap()["scratch_bytes"]
        .as_u64()
        .unwrap();
    let n = 24_000_000_u64;
    let mib = 1024 * 1024_u64;
    let kernel = 774_357_724_u64;
    assert_eq!(
        8 * n + (kernel + 8 * n) + (48 * n + 16 * 6000 + 128 * mib) + mib,
        PLAN_E16,
        "the derivation"
    );
    assert_eq!(planned, PLAN_E16);
}
