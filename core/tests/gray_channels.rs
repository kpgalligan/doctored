//! #1 phase 2d, alpha channels on a Grayscale document, through the FFI.
//! An alpha channel is a colour-free u16 plane at every depth, so the oracle
//! is the RGB TWIN (the Gray document converted to RGB through its own
//! profile bytes) given the same request: New Channel, a scalar Fill, a
//! paint stroke and a Gaussian Blur on `target: "channel"`, `read_plane` and
//! `scalar_statistics` of it, Rename, Duplicate and Delete each leave the
//! channels exactly the twin's and the layer's samples exactly what they
//! were, at 8, 16 and 32 bits (a filter at 32 bits against a 16-bit twin:
//! the channel's u16 plane route, `filtered_as_sixteen`); the `.rz` round
//! trip keeps the channel; and a
//! colour-plane target (`scalar_edit` red, and at 32 bits `paint_blend`
//! `plane: "red"`) refuses with the core's plane sentence, for good.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::{dr_precision_doc_open, dr_precision_doc_save_native};
use doctored_core::ffi_precision_edit::{
    dr_precision_doc_read_plane, dr_precision_doc_scalar_statistics,
};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::gray_ops::{edit, free, gray_doc, layer, plan_samples, twin, LIMIT};

const W: u32 = 12;
const H: u32 = 8;

/// The plane sentence (`gray_document::PLANE_REFUSAL`).
const PLANE_REFUSAL: &str = "A colour plane is unavailable in a Grayscale document: its one colour channel, Gray, is the layer, and Doctored offers the Red, Green, Blue and Alpha targets on RGB documents only.";

fn plane(seed: u32) -> Vec<u16> {
    (0..W * H)
        .map(|i| (i.wrapping_mul(2_654_435_761).wrapping_add(seed * 7919) % 65536) as u16)
        .collect()
}

fn channels(doc: *const PrecisionDocument) -> Vec<(String, Vec<u16>)> {
    unsafe { &*doc }
        .channels()
        .iter()
        .map(|c| (c.name().to_string(), c.plane().samples().to_vec()))
        .collect()
}

/// `request` on both documents (each freed), the channels equal after it
/// and each layer's samples unchanged.
fn both(
    docs: (*mut PrecisionDocument, *mut PrecisionDocument),
    request: Value,
    input: &[u16],
    depth: u32,
) -> (*mut PrecisionDocument, *mut PrecisionDocument) {
    let what = format!("{} at {depth}", request);
    let (gray, rgb) = docs;
    let (gray_layer, rgb_layer) = (layer(gray, 0, depth), layer(rgb, 0, depth));
    let g = edit(gray, &request, input)
        .unwrap_or_else(|e| panic!("{what} gray: {e}"))
        .unwrap_or_else(|| panic!("{what} gray changed nothing"));
    let t = edit(rgb, &request, input)
        .unwrap_or_else(|e| panic!("{what} twin: {e}"))
        .unwrap_or_else(|| panic!("{what} twin changed nothing"));
    assert_eq!(channels(g), channels(t), "{what}: channels");
    assert_eq!(layer(g, 0, depth), gray_layer, "{what}: gray layer");
    assert_eq!(layer(t, 0, depth), rgb_layer, "{what}: twin layer");
    assert!(unsafe { &*g }.is_gray(), "{what}");
    free(gray);
    free(rgb);
    (g, t)
}

fn read_channel(doc: *const PrecisionDocument, index: usize) -> Vec<u16> {
    let request =
        CString::new(json!({"op":"read_plane","target":"channel","index":index}).to_string())
            .unwrap();
    let mut out = vec![0u16; (W * H) as usize];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_read_plane(
            doc,
            request.as_ptr(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "read_plane: {}", take_err_string(err));
    out
}

fn statistics(doc: *const PrecisionDocument, target: &str) -> Result<Value, String> {
    let request =
        CString::new(json!({"op":"scalar_statistics","target":target,"index":0}).to_string())
            .unwrap();
    let mut err = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_scalar_statistics(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if raw.is_null() {
        return Err(take_err_string(err));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(raw) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(raw) };
    Ok(value)
}

/// A filter on a 32-bit Gray document's channel: the channel is a u16
/// plane at every depth and, as its layer mask since phase 2c, a Grayscale
/// document filters it through the u16 plane route at 32 bits too, where a
/// float RGB document takes the float plane route (`float_plane_filter`). The
/// oracle is therefore the same request on a 16-bit RGB twin holding the
/// same channel samples; the 32-bit twin then takes the result as its
/// channel, so the later steps compare like with like.
fn filtered_as_sixteen(
    docs: (*mut PrecisionDocument, *mut PrecisionDocument),
    request: &Value,
) -> (*mut PrecisionDocument, *mut PrecisionDocument) {
    let (gray, rgb) = docs;
    let gray_layer = layer(gray, 0, 32);
    let samples = channels(gray)[0].1.clone();
    let sixteen = gray_doc(W, H, 16);
    let oracle = twin(sixteen);
    let set = json!({"op":"set_channel","index":null,"name":"Alpha 1"});
    let oracle_set = edit(oracle, &set, &samples).unwrap().unwrap();
    let want = edit(oracle_set, request, &[]).unwrap().unwrap();
    let g = edit(gray, request, &[]).unwrap().unwrap();
    assert_eq!(
        channels(g),
        channels(want),
        "blur at 32: the u16 plane route"
    );
    assert_eq!(layer(g, 0, 32), gray_layer, "blur at 32: gray layer");
    let replace = json!({"op":"set_channel","index":0,"name":"Alpha 1"});
    let t = edit(rgb, &replace, &channels(g)[0].1).unwrap().unwrap();
    for d in [want, oracle_set, oracle, sixteen, gray, rgb] {
        free(d);
    }
    (g, t)
}

fn scalar(operation: Value) -> Value {
    json!({"op":"scalar_edit","target":"channel","index":0,"operation":operation})
}

#[test]
fn alpha_channels_edit_as_on_the_rgb_twin_and_leave_the_layer_alone() {
    for depth in [8, 16, 32] {
        let gray = gray_doc(W, H, depth);
        let rgb = twin(gray);
        let docs = both(
            (gray, rgb),
            json!({"op":"set_channel","index":null,"name":"Alpha 1"}),
            &plane(1),
            depth,
        );
        assert_eq!(channels(docs.0)[0].0, "Alpha 1");
        let docs = both(
            docs,
            scalar(json!({"kind":"fill","value":20000,"opacity":0.5})),
            &[],
            depth,
        );
        // A paint stroke: its payload is the canvas stroke coverage.
        let paint = scalar(json!({"kind":"paint","value":60000,"opacity":0.7}));
        let n = plan_samples(docs.0, &paint).unwrap();
        let coverage: Vec<u16> = (0..n).map(|i| (i * 977 % 65536) as u16).collect();
        let docs = both(docs, paint, &coverage, depth);
        let blur =
            scalar(json!({"kind":"filter","edit":{"layer":0,"op":"blur","params":{"sigma":1.5}}}));
        let docs = if depth == 32 {
            filtered_as_sixteen(docs, &blur)
        } else {
            both(docs, blur, &[], depth)
        };
        assert_eq!(read_channel(docs.0, 0), channels(docs.0)[0].1);
        assert_eq!(read_channel(docs.0, 0), read_channel(docs.1, 0));
        assert_eq!(
            statistics(docs.0, "channel").unwrap(),
            statistics(docs.1, "channel").unwrap()
        );
        let docs = both(
            docs,
            json!({"op":"rename_channel","index":0,"name":"Matte"}),
            &[],
            depth,
        );
        let docs = both(
            docs,
            json!({"op":"duplicate_channel","index":0}),
            &[],
            depth,
        );
        assert_eq!(channels(docs.0).len(), 2);
        assert_eq!(channels(docs.0)[1].0, "Matte copy");
        let (g, t) = both(docs, json!({"op":"remove_channel","index":0}), &[], depth);
        assert_eq!(channels(g).len(), 1);
        free(g);
        free(t);
    }
}

#[test]
fn an_rz_round_trip_keeps_the_channel() {
    let dir = TempDir::new().unwrap();
    for depth in [8, 16, 32] {
        let gray = gray_doc(W, H, depth);
        let doc = edit(
            gray,
            &json!({"op":"set_channel","index":null,"name":"Alpha 1"}),
            &plane(2),
        )
        .unwrap()
        .unwrap();
        let path = dir.path().join(format!("channel-{depth}.rz"));
        let c = cpath(&path);
        let mut err = ptr::null_mut();
        assert!(
            unsafe {
                dr_precision_doc_save_native(doc, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
            },
            "{}",
            take_err_string(err)
        );
        let back = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
        assert!(!back.is_null(), "{}", take_err_string(err));
        assert!(unsafe { &*back }.is_gray());
        assert_eq!(channels(back), channels(doc), "depth {depth}");
        assert_eq!(layer(back, 0, depth), layer(doc, 0, depth), "depth {depth}");
        for d in [back, doc, gray] {
            free(d);
        }
    }
}

#[test]
fn a_colour_plane_target_refuses_for_good() {
    for depth in [8, 16, 32] {
        let gray = gray_doc(W, H, depth);
        let request = json!({"op":"scalar_edit","target":"red","index":0,
            "operation":{"kind":"fill","value":20000}});
        assert_eq!(edit(gray, &request, &[]).unwrap_err(), PLANE_REFUSAL);
        assert_eq!(statistics(gray, "green").unwrap_err(), PLANE_REFUSAL);
        if depth == 32 {
            let paint = json!({"op":"paint_blend","layer":0,"color":[1.0,0.0,0.0,1.0],
                "color_space":"srgb","opacity":1.0,"blend_mode":0,"plane":"red"});
            assert_eq!(edit(gray, &paint, &[]).unwrap_err(), PLANE_REFUSAL);
        }
        free(gray);
    }
}
