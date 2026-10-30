//! #1 Indexed Color phase 2d: Paste and Paste Special on an Indexed
//! document (`indexed_clipboard`, `dr_precision_doc_indexed_paste`).
//!
//! The oracle is independent of the adapter: Image ▸ Mode ▸ RGB Color, the
//! same paste on that RGB document (masked by `set_mask` for Paste Into and
//! Outside), Merge Down, then the shared fixture's own re-index of the
//! merged layer's canvas window against the old plane with each pixel's own
//! colour. A paste from an Indexed document of the same palette is checked
//! against the index plane pasted by hand.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_clipboard::{
    dr_clipboard_payload_free, dr_precision_doc_clipboard_copy, dr_precision_doc_clipboard_paste,
    DrClipboardPayload,
};
use doctored_core::ffi_gray::dr_precision_doc_new_gray;
use doctored_core::ffi_indexed::{
    dr_precision_doc_indexed_paste, dr_precision_doc_indexed_paste_plan,
};
use doctored_core::ffi_jobs::{
    dr_job_control_create, dr_job_control_free, dr_job_control_set_scratch_limit,
};
use doctored_core::ffi_lab::dr_precision_doc_new_lab;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::indexed_fixture::{
    free, indices, layer16, oracle, paint, palette_of, rgb_doc, set_table, to_indexed, to_rgb,
    window, Ink, LIMIT,
};

const W: u32 = 12;
const H: u32 = 8;

/// The 12 × 8 scene: black, white and (190, 50, 50) bands, its last row
/// clear, under a table of distinct colours with t = 3.
fn scene() -> *mut PrecisionDocument {
    let mut pixels = Vec::new();
    for y in 0..H {
        for x in 0..W {
            pixels.push(if y == H - 1 {
                [0, 0, 0, 0]
            } else if x < 4 {
                [0, 0, 0, 255]
            } else if x < 8 {
                [255, 255, 255, 255]
            } else {
                [190, 50, 50, 255]
            });
        }
    }
    let rgb = rgb_doc(W, H, &pixels);
    let doc = to_indexed(rgb, &json!({"palette":"exact","dither":"none"})).unwrap();
    free(rgb);
    let out = set_table(
        doc,
        &json!({"colors":[[0,0,0],[255,255,255],[190,50,50],[9,9,9]],"transparent_index":3}),
    )
    .unwrap()
    .unwrap();
    free(doc);
    out
}

/// The layer of an Indexed document is its palette's render of its plane.
fn assert_renders(doc: *const PrecisionDocument) {
    let (palette, t) = palette_of(doc);
    for (pixel, &index) in layer16(doc).iter().zip(&indices(doc)) {
        let c = palette[index as usize];
        let alpha = if Some(index as u8) == t { 0 } else { 65535 };
        assert_eq!(
            *pixel,
            [
                u16::from(c[0]) * 257,
                u16::from(c[1]) * 257,
                u16::from(c[2]) * 257,
                alpha
            ]
        );
    }
}

/// A copy of `doc`'s layer 0 over `[x, y, width, height]`.
fn copy(doc: *const PrecisionDocument, window: [u32; 4]) -> *mut DrClipboardPayload {
    let request = json!({"op":"copy","layer":0,"x":window[0],"y":window[1],
                         "width":window[2],"height":window[3]});
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let payload = unsafe {
        dr_precision_doc_clipboard_copy(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!payload.is_null(), "{}", take_err_string(err));
    payload
}

fn request(placement: &str) -> Value {
    json!({"name":"Pasted","anchor":0,"position":"above","placement":placement})
}

/// `dr_precision_doc_indexed_paste`, coverage optional.
fn paste(
    doc: *const PrecisionDocument,
    payload: *const DrClipboardPayload,
    request: &Value,
    mask: Option<(&[u16], u8)>,
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(request.to_string()).unwrap();
    let (coverage, len, outside) = match mask {
        Some((c, o)) => (c.as_ptr(), c.len(), o),
        None => (ptr::null(), 0, 0),
    };
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_indexed_paste(
            doc,
            payload,
            text.as_ptr(),
            coverage,
            len,
            outside,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

/// `dr_precision_doc_clipboard_paste` (today's RGB paste, or the Indexed arm).
fn plain_paste(
    doc: *const PrecisionDocument,
    payload: *const DrClipboardPayload,
    request: &Value,
) -> *mut PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_clipboard_paste(
            doc,
            payload,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    out
}

/// The oracle (module doc).
fn expected(
    doc: *const PrecisionDocument,
    payload: *const DrClipboardPayload,
    request: &Value,
    mask: Option<(&[u16], u8)>,
) -> Vec<i32> {
    let (palette, t) = palette_of(doc);
    let before = indices(doc);
    let rgb = to_rgb(doc);
    let pasted = plain_paste(rgb, payload, request);
    let masked = match mask {
        Some((coverage, outside)) => paint(
            pasted,
            &json!({"op":"set_mask","layer":1,"width":W,"height":H,"x":0,"y":0,
                    "linked":false,"outside_coverage":outside}),
            coverage,
        )
        .unwrap()
        .unwrap(),
        None => pasted,
    };
    let merged = paint(masked, &json!({"op":"merge_down","layer":1}), &[])
        .unwrap()
        .unwrap();
    let out = oracle(&palette, t, Some(&before), &window(merged), Ink::Own).unwrap();
    for d in [merged, rgb] {
        free(d);
    }
    if masked != pasted {
        free(masked);
    }
    free(pasted);
    out
}

/// An RGB document of patch colours: two palette colours, two others and a
/// half-clear pixel.
fn rgb_source() -> *mut PrecisionDocument {
    rgb_doc(
        4,
        3,
        &[
            [255, 255, 255, 255],
            [190, 50, 50, 255],
            [120, 120, 120, 255],
            [250, 240, 10, 255],
            [0, 0, 0, 255],
            [180, 60, 60, 200],
            [20, 20, 20, 60],
            [255, 255, 255, 255],
            [200, 200, 200, 255],
            [10, 10, 10, 255],
            [100, 0, 0, 255],
            [255, 255, 255, 0],
        ],
    )
}

fn gray_source() -> *mut PrecisionDocument {
    let request = CString::new(
        json!({"depth":8,"space":"sgray","fill_srgb":[0.45,0.45,0.45,1.0],
               "layer_name":"Background","ppi":72})
        .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_new_gray(4, 3, ptr::null(), 0, request.as_ptr(), LIMIT, 0, &mut err)
    };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

fn lab_source() -> *mut PrecisionDocument {
    let request = CString::new(
        json!({"depth":8,"fill_srgb":[0.7,0.2,0.2,1.0],"layer_name":"Background","ppi":72})
            .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_new_lab(4, 3, request.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

#[test]
fn a_paste_of_any_mode_is_the_rgb_paste_merged_and_reindexed() {
    let doc = scene();
    for (name, source) in [
        ("rgb", rgb_source()),
        ("gray", gray_source()),
        ("lab", lab_source()),
    ] {
        let payload = copy(source, [0, 0, 4, 3]);
        for placement in ["centered", "in_place"] {
            let request = request(placement);
            let out = paste(doc, payload, &request, None).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(
                indices(out),
                expected(doc, payload, &request, None),
                "{name} {placement}"
            );
            assert_ne!(indices(out), indices(doc), "{name} {placement}");
            let d = unsafe { &*out };
            assert_eq!(d.layer_count(), 1);
            assert_eq!(d.layer(0).unwrap().name(), "Index");
            assert_renders(out);
            // The clipboard's own paste reaches the same adapter.
            let plain = plain_paste(doc, payload, &request);
            assert_eq!(indices(plain), indices(out));
            free(plain);
            free(out);
        }
        unsafe { dr_clipboard_payload_free(payload) };
        free(source);
    }
    free(doc);
}

#[test]
fn a_paste_from_the_same_palette_keeps_every_index() {
    let doc = scene();
    let before = indices(doc);
    let (_, t) = palette_of(doc);
    let t = i32::from(t.unwrap());
    // The source: the same document flipped, so its indices differ.
    let source = paint(doc, &json!({"op":"flip_horizontal"}), &[])
        .unwrap()
        .unwrap();
    let src = indices(source);
    let window = [2u32, 3, 6, 5];
    let payload = copy(source, window);
    let out = paste(doc, payload, &request("in_place"), None).unwrap();
    let mut hand = before.clone();
    for y in window[1]..window[1] + window[3] {
        for x in window[0]..window[0] + window[2] {
            let i = (y * W + x) as usize;
            if src[i] != t {
                hand[i] = src[i];
            }
        }
    }
    assert_eq!(indices(out), hand);
    assert_renders(out);
    unsafe { dr_clipboard_payload_free(payload) };
    free(out);
    free(source);
    free(doc);
}

/// A canvas coverage of 65535 inside `[x0, x1) × [y0, y1)` (or outside it
/// when `invert`), else 0.
fn rect(x0: u32, y0: u32, x1: u32, y1: u32, invert: bool) -> Vec<u16> {
    (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .map(|(x, y)| {
            let inside = (x0..x1).contains(&x) && (y0..y1).contains(&y);
            if inside != invert {
                65535
            } else {
                0
            }
        })
        .collect()
}

#[test]
fn paste_into_and_outside_follow_the_selection() {
    let doc = scene();
    let source = rgb_source();
    let payload = copy(source, [0, 0, 4, 3]);
    let before = indices(doc);
    for (invert, outside) in [(false, 0u8), (true, 255u8)] {
        let coverage = rect(4, 2, 7, 5, invert);
        let request = json!({"name":"Pasted","anchor":0,"position":"above","placement":"into",
                             "frame":[4,2,3,3]});
        let out = paste(doc, payload, &request, Some((&coverage, outside))).unwrap();
        let got = indices(out);
        assert_eq!(
            got,
            expected(doc, payload, &request, Some((&coverage, outside))),
            "invert {invert}"
        );
        for (i, &c) in coverage.iter().enumerate() {
            if c == 0 {
                assert_eq!(got[i], before[i], "pixel {i} outside the mask moved");
            }
        }
        assert_renders(out);
        free(out);
    }
    unsafe { dr_clipboard_payload_free(payload) };
    free(source);
    free(doc);
}

fn plan(
    doc: *const PrecisionDocument,
    payload: *const DrClipboardPayload,
    request: &Value,
) -> Value {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_indexed_paste_plan(doc, payload, text.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    value
}

#[test]
fn a_locked_index_refuses_and_a_controlled_paste_runs() {
    let doc = scene();
    let source = rgb_source();
    let payload = copy(source, [0, 0, 4, 3]);
    let locked = paint(doc, &json!({"op":"layer_lock","layer":0,"locks":2}), &[])
        .unwrap()
        .unwrap();
    let refused = paste(locked, payload, &request("centered"), None).unwrap_err();
    assert!(refused.contains("locked (pixels)"), "{refused}");

    // Through a fresh job control limited to the plan: it begins once and
    // every stage stays within the plan's reservation.
    let request = request("centered");
    let scratch = plan(doc, payload, &request)["scratch_bytes"]
        .as_u64()
        .unwrap();
    let control = unsafe { dr_job_control_create() };
    assert!(unsafe { dr_job_control_set_scratch_limit(control, scratch) });
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_indexed_paste(
            doc,
            payload,
            text.as_ptr(),
            ptr::null(),
            0,
            0,
            LIMIT,
            0,
            control,
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    assert_eq!(indices(out), expected(doc, payload, &request, None));
    unsafe { dr_job_control_free(control) };
    free(out);
    free(locked);
    unsafe { dr_clipboard_payload_free(payload) };
    free(source);
    free(doc);
}

#[test]
fn the_plan_covers_its_terms() {
    let doc = scene();
    let source = rgb_source();
    let payload = copy(source, [0, 0, 4, 3]);
    let value = plan(doc, payload, &request("centered"));
    let retained = value["retained_bytes"].as_u64().unwrap();
    let scratch = value["scratch_bytes"].as_u64().unwrap();
    assert!(retained > 0);
    // The merge's 72 B/px over the canvas and the payload, and the
    // re-index's 10 B/px over the canvas, beside the RGB paste's own term.
    let floor = (u64::from(W * H) + 12) * 72 + u64::from(W * H) * 10;
    assert!(scratch >= floor, "{scratch} < {floor}");
    // A malformed request and a document that is not Indexed refuse.
    let mut err = ptr::null_mut();
    let bad = CString::new("{}").unwrap();
    assert!(
        unsafe { dr_precision_doc_indexed_paste_plan(doc, payload, bad.as_ptr(), &mut err) }
            .is_null()
    );
    assert!(take_err_string(err).contains("invalid clipboard paste request"));
    let mut err = ptr::null_mut();
    let text = CString::new(request("centered").to_string()).unwrap();
    assert!(unsafe {
        dr_precision_doc_indexed_paste_plan(source, payload, text.as_ptr(), &mut err)
    }
    .is_null());
    assert_eq!(take_err_string(err), "the document is not Indexed Color");
    // A coverage of the wrong length refuses.
    let short = vec![65535u16; 3];
    assert!(paste(doc, payload, &request("centered"), Some((&short, 0))).is_err());
    unsafe { dr_clipboard_payload_free(payload) };
    free(source);
    free(doc);
}
