//! #547 Bitmap phase 2a-ii through the FFI: the resampled geometry (Image
//! Size, the Crop tool's straighten, the layer's affine and perspective
//! transforms and their sets), the selected pixels' moves (their transforms
//! and arbitrary rotation refuse since phase 2b), Cut, the refusals
//! that remain, each plan and the `.rz` round trip (`bitmap_edit`, the rule
//! `bitmap_method::white_over`).
//!
//! The oracle is independent of the adapter: Image ▸ Mode ▸ Grayscale at
//! Size Ratio 1, the same request on that Gray document (the background key
//! dropped), then `bitmap_edits::thresholded`, written from the rule's
//! definition, over the canvas window, white or black as the key says. A
//! whole-pixel move and a nearest resize are also checked against the plane
//! moved by hand, and Cut against the rect whitened by hand.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_clipboard::{dr_clipboard_payload_free, dr_precision_doc_clipboard_cut};
use doctored_core::ffi_precision::{
    dr_precision_doc_duplicate_layer, dr_precision_doc_save_native,
};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::bitmap_edits::{black_key, scene, scene_bits, thresholded, H, W};
use crate::bitmap_mode::{bits, to_gray};
use crate::common::{cpath, take_err_string};
use crate::indexed_fixture::{descriptor, edit_plan, free, open_rgb, paint, window, LIMIT};

/// The adapter's declared bytes per output pixel (`bitmap_edit`).
const BYTES_PER_PIXEL: u64 = 34;

/// A canvas affine `[a, b, c, d, tx, ty]` turning `degrees` about `(cx, cy)`.
fn rotation(degrees: f64, cx: f64, cy: f64) -> [f64; 6] {
    let (s, c) = degrees.to_radians().sin_cos();
    [c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy]
}

/// A canvas coverage of the rect `(x, y, w, h)` at full strength.
fn rect_coverage(x: u32, y: u32, w: u32, h: u32) -> Vec<u16> {
    (0..H)
        .flat_map(|py| {
            (0..W).map(move |px| {
                if px >= x && px < x + w && py >= y && py < y + h {
                    65535
                } else {
                    0
                }
            })
        })
        .collect()
}

/// `request` as the Gray twin takes it: the background key dropped.
fn twin_request(request: &Value) -> Value {
    let mut twin = request.clone();
    twin.as_object_mut().unwrap().remove("indexed_background");
    twin
}

/// The resampled geometry requests of (c), each with its input.
fn geometry_cases() -> Vec<(Value, Vec<u16>)> {
    let centre = rotation(5.0, 6.0, 4.0);
    let mut straighten_black = json!({"op":"straighten","matrix":rotation(30.0, 6.0, 4.0),
                                      "filter":"bicubic","x":-2,"y":-2,"width":16,"height":12});
    straighten_black["indexed_background"] = black_key();
    vec![
        (
            json!({"op":"resize","width":24,"height":16,"filter":"bicubic"}),
            vec![],
        ),
        (
            json!({"op":"resize","width":6,"height":4,"filter":"bicubic"}),
            vec![],
        ),
        (
            json!({"op":"resize","width":24,"height":16,"filter":"nearest"}),
            vec![],
        ),
        (straighten_black, vec![]),
        (
            json!({"op":"straighten","matrix":centre,"filter":"bicubic",
                   "x":1,"y":1,"width":10,"height":6}),
            vec![],
        ),
        (
            json!({"op":"affine","layer":0,"target":"pixels","matrix":centre,
                   "filter":"bilinear"}),
            vec![],
        ),
        (
            json!({"op":"affine","layer":0,"target":"pixels",
                   "matrix":rotation(20.0, 6.0, 4.0),"filter":"bicubic"}),
            vec![],
        ),
        (
            json!({"op":"affine_set","layers":[0],"matrix":rotation(15.0, 6.0, 4.0),
                   "filter":"bicubic"}),
            vec![],
        ),
        (
            json!({"op":"perspective_set","layers":[0],"source_rect":[0.0,0.0,12.0,8.0],
                   "quad":[1.0,0.0,11.0,1.0,12.0,8.0,0.0,7.0],"filter":"bilinear"}),
            vec![],
        ),
    ]
}

/// The selected pixels' requests of (d), each with its rect coverage.
fn selected_cases() -> Vec<(Value, Vec<u16>)> {
    let mut moved_black = json!({"op":"move_selected","layer":0,"dx":3,"dy":2});
    moved_black["indexed_background"] = black_key();
    vec![
        (
            json!({"op":"move_selected","layer":0,"dx":3,"dy":2}),
            rect_coverage(1, 1, 6, 4),
        ),
        (moved_black, rect_coverage(1, 1, 6, 4)),
    ]
}

/// Each request on the scene is a Bitmap document whose bits are the Gray
/// twin's result thresholded; NULL with no error only where that threshold
/// is the scene itself on the same canvas, which is exactly `still`'s
/// requests (a small turn whose soft edges stay on their side of 128).
fn assert_thresholded(cases: Vec<(Value, Vec<u16>)>, still: &[Value]) {
    let doc = scene();
    let gray = to_gray(doc, &json!({})).unwrap();
    for (request, input) in cases {
        let twin = paint(gray, &twin_request(&request), &input)
            .unwrap_or_else(|e| panic!("{request} on Gray: {e}"))
            .unwrap_or_else(|| panic!("{request} on Gray: nothing changed"));
        let background = if request.get("indexed_background").is_some() {
            0
        } else {
            255
        };
        let expected = thresholded(&window(twin), background);
        let out = paint(doc, &request, &input).unwrap_or_else(|e| panic!("{request}: {e}"));
        assert_eq!(out.is_none(), still.contains(&request), "{request}");
        let Some(out) = out else {
            assert_eq!(
                (unsafe { &*twin }.dimensions(), expected),
                ((W, H), scene_bits()),
                "{request}: NULL, yet bits move"
            );
            free(twin);
            continue;
        };
        assert_eq!(
            unsafe { &*out }.dimensions(),
            unsafe { &*twin }.dimensions(),
            "{request}"
        );
        assert_eq!(bits(out), expected, "{request}");
        let d = descriptor(out);
        assert_eq!(
            (d["color_mode"].clone(), d["layer_count"].clone()),
            (json!("bitmap"), json!(1)),
            "{request}"
        );
        free(twin);
        free(out);
    }
    free(gray);
    free(doc);
}

#[test]
fn each_resampled_op_is_the_gray_op_thresholded() {
    // The 5° bilinear Free Transform moves each edge under half a pixel.
    let still = json!({"op":"affine","layer":0,"target":"pixels",
                       "matrix":rotation(5.0, 6.0, 4.0),"filter":"bilinear"});
    assert_thresholded(geometry_cases(), &[still]);
}

#[test]
fn a_nearest_resize_doubles_each_bit() {
    let doc = scene();
    let before = scene_bits();
    let out = paint(
        doc,
        &json!({"op":"resize","width":24,"height":16,"filter":"nearest"}),
        &[],
    )
    .unwrap()
    .unwrap();
    let doubled: Vec<u8> = (0..2 * H)
        .flat_map(|y| (0..2 * W).map(move |x| (x / 2, y / 2)))
        .map(|(x, y)| before[(y * W + x) as usize])
        .collect();
    assert_eq!(bits(out), doubled);
    free(out);
    free(doc);
}

#[test]
fn each_selected_pixels_edit_is_the_gray_op_thresholded() {
    // Every move lands bits (the selected pixels' transforms refuse since
    // phase 2b: `the_refusals_that_remain`).
    assert_thresholded(selected_cases(), &[]);
}

#[test]
fn a_whole_pixel_move_carries_the_bits_and_vacates_white() {
    let doc = scene();
    let before = scene_bits();
    let (rx, ry, rw, rh) = (1u32, 1u32, 6u32, 4u32);
    let (dx, dy) = (3u32, 2u32);
    let out = paint(
        doc,
        &json!({"op":"move_selected","layer":0,"dx":dx,"dy":dy}),
        &rect_coverage(rx, ry, rw, rh),
    )
    .unwrap()
    .unwrap();
    let inside = |x: u32, y: u32| x >= rx && x < rx + rw && y >= ry && y < ry + rh;
    let expected: Vec<u8> = (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .map(|(x, y)| {
            if x >= dx && y >= dy && inside(x - dx, y - dy) {
                before[((y - dy) * W + x - dx) as usize]
            } else if inside(x, y) {
                1
            } else {
                before[(y * W + x) as usize]
            }
        })
        .collect();
    assert_eq!(bits(out), expected);
    free(out);
    free(doc);
}

/// `dr_precision_doc_clipboard_cut` of the rect under the rect's coverage
/// (the host's selection): whether a payload came back, and the document
/// (`None` for NULL with no error).
fn cut(
    doc: *const PrecisionDocument,
    rect: (u32, u32, u32, u32),
    black: bool,
) -> Result<(bool, Option<*mut PrecisionDocument>), String> {
    let mut request = json!({"op":"cut","layer":0,"x":rect.0,"y":rect.1,
                             "width":rect.2,"height":rect.3});
    if black {
        request["indexed_background"] = black_key();
    }
    let coverage = rect_coverage(rect.0, rect.1, rect.2, rect.3);
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let mut payload = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_clipboard_cut(
            doc,
            text.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut payload,
            &mut err,
        )
    };
    if !err.is_null() {
        assert!(payload.is_null() && out.is_null());
        return Err(take_err_string(err));
    }
    let had_payload = !payload.is_null();
    unsafe { dr_clipboard_payload_free(payload) };
    Ok((had_payload, (!out.is_null()).then_some(out)))
}

#[test]
fn cut_takes_the_background_bit() {
    let doc = scene();
    let before = scene_bits();
    let rect = (1u32, 1u32, 8u32, 5u32);
    let inside =
        |x: u32, y: u32| x >= rect.0 && x < rect.0 + rect.2 && y >= rect.1 && y < rect.1 + rect.3;
    for (black, bit) in [(false, 1u8), (true, 0u8)] {
        let (payload, out) = cut(doc, rect, black).unwrap();
        assert!(payload, "black {black}");
        let out = out.expect("the cut moves bits");
        let expected: Vec<u8> = (0..H)
            .flat_map(|y| (0..W).map(move |x| (x, y)))
            .map(|(x, y)| {
                if inside(x, y) {
                    bit
                } else {
                    before[(y * W + x) as usize]
                }
            })
            .collect();
        assert_eq!(bits(out), expected, "black {black}");
        assert_eq!(descriptor(out)["color_mode"], json!("bitmap"));
        free(out);
    }
    // An all-white rect (x 6.., away from the black marker) moves no bit:
    // NULL with no error, the payload still set.
    let (payload, out) = cut(doc, (6, 0, 3, 4), false).unwrap();
    assert!(payload);
    assert!(out.is_none());
    free(doc);
}

#[test]
fn the_refusals_that_remain() {
    let doc = scene();
    for (request, sentence) in [
        (
            json!({"op":"selection","operation":{"kind":"magic_wand"}}),
            "magic_wand is not available in Bitmap mode.".to_string(),
        ),
        // Phase 2b's walk: Edit ▸ Free Transform and Transform are disabled
        // in Bitmap mode.
        (
            json!({"op":"transform_selected","layer":0,"matrix":rotation(30.0, 4.0, 3.0),
                   "filter":"bicubic"}),
            "transform_selected is not available in Bitmap mode.".to_string(),
        ),
        // And Image ▸ Image Rotation ▸ Arbitrary, the one leaf arbitrary
        // rotation serves (the controller's ruling: the leaf wins).
        (
            json!({"op":"rotate_canvas","degrees":30.0,"filter":"bicubic"}),
            "rotate_canvas is not available in Bitmap mode.".to_string(),
        ),
        // Phase 3: Color Range and Edit ▸ Transform's Puppet Warp, for good.
        (
            json!({"op":"selection","operation":{"kind":"color_range"}}),
            "color_range is not available in Bitmap mode.".to_string(),
        ),
        (
            json!({"op":"puppet_selected","layer":0}),
            "puppet_selected is not available in Bitmap mode.".to_string(),
        ),
    ] {
        assert_eq!(
            paint(doc, &request, &[]).unwrap_err(),
            sentence,
            "{request}"
        );
    }
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_duplicate_layer(doc, 0, &mut err) };
    assert!(out.is_null());
    assert_eq!(
        take_err_string(err),
        "duplicate_layer is not available in Bitmap mode."
    );
    free(doc);
}

fn scratch(plan: &Value) -> u64 {
    plan["scratch_bytes"].as_u64().unwrap()
}

#[test]
fn each_plan_covers_the_republish() {
    let doc = scene();
    let gray = to_gray(doc, &json!({})).unwrap();
    for (request, input) in geometry_cases().into_iter().chain(selected_cases()) {
        let twin_request = twin_request(&request);
        let bitmap = edit_plan(doc, &request).unwrap();
        let twin = edit_plan(gray, &twin_request).unwrap();
        assert_eq!(bitmap["input_samples"], twin["input_samples"], "{request}");
        let result = paint(gray, &twin_request, &input).unwrap().unwrap();
        let (w, h) = unsafe { &*result }.dimensions();
        free(result);
        let floor = scratch(&twin) + u64::from(w) * u64::from(h) * BYTES_PER_PIXEL;
        assert!(
            scratch(&bitmap) >= floor,
            "{request}: {} < {floor}",
            scratch(&bitmap)
        );
    }
    free(gray);
    free(doc);
}

#[test]
fn a_resampled_document_round_trips_through_rz() {
    let dir = TempDir::new().unwrap();
    let doc = scene();
    let resized = paint(
        doc,
        &json!({"op":"resize","width":24,"height":16,"filter":"bicubic"}),
        &[],
    )
    .unwrap()
    .unwrap();
    let path = dir.path().join("resized.rz");
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_native(resized, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        },
        "{}",
        take_err_string(err)
    );
    let back = open_rgb(&path).unwrap();
    assert_eq!(descriptor(back)["color_mode"], json!("bitmap"));
    assert_eq!(
        unsafe { &*back }.dimensions(),
        unsafe { &*resized }.dimensions()
    );
    assert_eq!(bits(back), bits(resized));
    for d in [back, resized, doc] {
        free(d);
    }
}
