//! #547 phase 6b on a Grayscale document through the FFI: the Histogram,
//! Apply Image's layer write (`scalar_edit` target `gray`), Calculations into
//! a new channel and Stack Modes.
//!
//! # The oracle
//!
//! The RGB TWIN (`gray_ops::twin`: the Gray document converted to RGB
//! through its own profile, every sample kept, so R = G = B) run through the
//! RGB path the host takes on an RGB document. A Gray document is that
//! storage, so every result must equal the twin's: the Histogram report as a
//! whole; Apply Image's layer, read and written as ONE plane on Gray (the
//! `luminance` selection, a one-channel blend, `scalar_edit` target `gray`)
//! against the twin's three (the `color` selections, a three-channel blend,
//! `scalar_edit` on red, green and blue), Gray's R against the twin's R with
//! the twin still R = G = B; the Calculations channel; and each Stack Modes
//! layer.
//!
//! # Bounds, declared before the first run
//!
//! Every comparison is exact.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_gray::dr_precision_doc_convert_to_gray;
use doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float;
use doctored_core::ffi_precision_edit::{
    dr_precision_doc_edit_plan, dr_precision_doc_read_plane, dr_precision_doc_select,
};
use doctored_core::ffi_scoped_statistics::dr_precision_doc_scoped_analysis;
use doctored_core::ffi_selection_u16::dr_selection_u16;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::gray_ops::{assert_neutral, edit, free, layer, plan_json, twin, LIMIT};

const W: u32 = 7;
const H: u32 = 5;
const N: usize = (W * H) as usize;

/// One coloured pixel of layer `k`, every ninth at partial alpha.
fn pixel(i: u32, k: u32) -> [u16; 4] {
    let r = (i * 7919 + 1013 + k * 4099) % 251;
    let g = (i * 104_729 + 37 + k * 271) % 241;
    let b = (i * 1299 + 501 + k * 3343) % 239;
    let a = if (i + k) % 9 == 4 { 30_000 } else { 65_535 };
    [(r * 257 + 40) as u16, (g * 265) as u16, (b * 270) as u16, a]
}

/// A Gray Gamma 2.2 document of `depth` holding `count` canvas-sized layers
/// of coloured pixels, each converted by Image ▸ Mode ▸ Grayscale.
fn gray_stack(depth: u32, count: u32) -> *mut PrecisionDocument {
    let layers = (0..count)
        .map(|k| {
            let pixels = (0..W * H).map(|i| pixel(i, k)).collect();
            let image =
                PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
            PrecisionDocumentLayer::raster(&format!("Layer {k}"), image, (0, 0), 1.0, true).unwrap()
        })
        .collect();
    let rgb = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap();
    let mut rgb = Box::into_raw(Box::new(rgb));
    if depth == 32 {
        let mut err = ptr::null_mut();
        let float =
            unsafe { dr_precision_doc_promote_to_float(rgb, LIMIT, 0, ptr::null(), &mut err) };
        assert!(!float.is_null(), "{}", take_err_string(err));
        free(rgb);
        rgb = float;
    }
    let options = if depth == 32 {
        r#"{"space":"gray_gamma_22"}"#.to_string()
    } else {
        format!(r#"{{"space":"gray_gamma_22","depth":{depth}}}"#)
    };
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let gray = unsafe {
        dr_precision_doc_convert_to_gray(
            rgb,
            ptr::null(),
            0,
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!gray.is_null(), "to gray: {}", take_err_string(err));
    free(rgb);
    assert!(unsafe { &*gray }.is_gray());
    gray
}

fn analysis(doc: *const PrecisionDocument, request: &Value) -> Value {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_scoped_analysis(
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
    plan_json(out, err).unwrap()
}

/// One canvas plane through `dr_precision_doc_select` (the host's
/// `NativeSelectionWork`): `operation` of the composite, or of `layer`.
fn plane(doc: *const PrecisionDocument, layer: Option<usize>, operation: Value) -> Vec<u16> {
    let mut request = json!({"op":"selection","source":"composite","operation":operation});
    if let Some(layer) = layer {
        request["source"] = json!("layer");
        request["layer"] = json!(layer);
    }
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let planned = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    let planned = plan_json(planned, err).unwrap();
    let mut out = vec![999u16; planned["output_samples"].as_u64().unwrap() as usize];
    assert_eq!(out.len(), N);
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_select(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
    out
}

/// The host's `NativePlaneReader.blend`: `base` planes then `source` planes
/// through `dr_selection_u16`'s `blend`.
fn blend(base: &[Vec<u16>], source: &[Vec<u16>], mode: i32, opacity: f64) -> Vec<Vec<u16>> {
    let channels = base.len();
    let request = CString::new(
        json!({"kind":"blend","channels":channels,"mode":mode,"opacity":opacity}).to_string(),
    )
    .unwrap();
    let input: Vec<u16> = base.iter().chain(source).flatten().copied().collect();
    let mut out = vec![0u16; N * channels];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_selection_u16(
            W,
            H,
            request.as_ptr(),
            input.as_ptr(),
            input.len(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
    out.chunks(N).map(<[u16]>::to_vec).collect()
}

/// `scalar_edit` `replace_canvas` of `target` on layer `index`; the
/// document itself when nothing changed.
fn write(
    doc: *mut PrecisionDocument,
    target: &str,
    index: usize,
    samples: &[u16],
) -> *mut PrecisionDocument {
    let request = json!({"op":"scalar_edit","target":target,"index":index,
        "operation":{"kind":"replace_canvas"}});
    match edit(doc, &request, samples).unwrap() {
        Some(next) => {
            free(doc);
            next
        }
        None => doc,
    }
}

#[test]
fn the_histogram_of_a_gray_document_is_its_twins() {
    for depth in [8, 16, 32] {
        let gray = gray_stack(depth, 2);
        let rgb = twin(gray);
        for request in [
            json!({"operation":"histogram","source":"composite"}),
            json!({"operation":"histogram","source":"layer","layer":1}),
            json!({"operation":"histogram","source":"backdrop","below":1}),
        ] {
            assert_eq!(
                analysis(gray, &request),
                analysis(rgb, &request),
                "{depth}: {request}"
            );
        }
        free(rgb);
        free(gray);
    }
}

#[test]
fn apply_image_onto_a_gray_layer_is_the_twins_three_planes() {
    for depth in [8, 16] {
        for mode in [0, 1, 2, 3, 8] {
            let mut gray = gray_stack(depth, 2);
            let mut rgb = twin(gray);
            // Gray: the layer's luminance and the composite's, one plane.
            let original = plane(gray, Some(0), json!({"kind":"luminance"}));
            let incoming = plane(gray, None, json!({"kind":"luminance"}));
            let blended = blend(&[original], &[incoming], mode, 0.6);
            gray = write(gray, "gray", 0, &blended[0]);
            // The twin: red, green and blue, three planes.
            let names = ["red", "green", "blue"];
            let colour =
                |doc, layer, name: &str| plane(doc, layer, json!({"kind":"color","channel":name}));
            let originals: Vec<_> = names.iter().map(|n| colour(rgb, Some(0), n)).collect();
            let incoming: Vec<_> = names.iter().map(|n| colour(rgb, None, n)).collect();
            let blended = blend(&originals, &incoming, mode, 0.6);
            for (name, samples) in names.iter().zip(&blended) {
                rgb = write(rgb, name, 0, samples);
            }
            let ours = layer(gray, 0, depth);
            let theirs = layer(rgb, 0, depth);
            assert_neutral(&ours, &format!("{depth}-bit mode {mode} gray"));
            assert_neutral(&theirs, &format!("{depth}-bit mode {mode} twin"));
            for (i, (a, b)) in ours.iter().zip(&theirs).enumerate() {
                assert_eq!(a, b, "{depth}-bit mode {mode} pixel {i}");
            }
            // The other layer is untouched.
            assert_eq!(layer(gray, 1, depth), layer(rgb, 1, depth));
            free(rgb);
            free(gray);
        }
    }
}

fn channel(doc: *const PrecisionDocument, index: usize) -> Vec<u16> {
    let text =
        CString::new(json!({"op":"read_plane","target":"channel","index":index}).to_string())
            .unwrap();
    let mut out = vec![0u16; N];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_read_plane(
            doc,
            text.as_ptr(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
    out
}

#[test]
fn calculations_into_a_new_channel_is_the_twins() {
    for depth in [8, 16, 32] {
        let gray = gray_stack(depth, 2);
        let rgb = twin(gray);
        let mut channels = Vec::new();
        for doc in [gray, rgb] {
            let base = plane(doc, Some(0), json!({"kind":"luminance"}));
            let source = plane(doc, Some(1), json!({"kind":"luminance"}));
            let blended = blend(&[base], &[source], 1, 1.0);
            let request = json!({"op":"set_channel","index":null,"name":"Calc"});
            let next = edit(doc, &request, &blended[0]).unwrap().unwrap();
            channels.push(channel(next, 0));
            free(next);
        }
        assert_eq!(channels[0], channels[1], "{depth}");
        free(rgb);
        free(gray);
    }
}

#[test]
fn stack_modes_on_gray_are_the_twins() {
    for depth in [8, 16, 32] {
        let gray = gray_stack(depth, 3);
        let rgb = twin(gray);
        for mode in ["mean", "median", "range"] {
            let request = json!({"op":"stack_mode","layers":[0,1,2],"above":2,"mode":mode,
                "name":"Stat"});
            let ours = edit(gray, &request, &[]).unwrap().unwrap();
            let theirs = edit(rgb, &request, &[]).unwrap().unwrap();
            assert!(unsafe { &*ours }.is_gray());
            let count = unsafe { &*ours }.layer_count();
            assert_eq!(count, 4);
            for index in 0..count {
                let a = layer(ours, index, depth);
                let b = layer(theirs, index, depth);
                assert_neutral(&a, &format!("{depth}-bit {mode} layer {index}"));
                assert_eq!(a, b, "{depth}-bit {mode} layer {index}");
            }
            free(theirs);
            free(ours);
        }
        free(rgb);
        free(gray);
    }
}

#[test]
fn the_gray_scalar_target_is_a_gray_layers_replace_canvas_only() {
    let samples = vec![1000u16; N];
    let replace = json!({"op":"scalar_edit","target":"gray","index":0,
        "operation":{"kind":"replace_canvas"}});
    let gray = gray_stack(16, 1);
    let rgb = twin(gray);
    assert_eq!(
        edit(rgb, &replace, &samples).unwrap_err(),
        "scalar target gray is a Grayscale document's layer"
    );
    let fill = json!({"op":"scalar_edit","target":"gray","index":0,
        "operation":{"kind":"fill","value":1000}});
    assert_eq!(
        edit(gray, &fill, &[]).unwrap_err(),
        "scalar target gray takes replace_canvas only"
    );
    let float = gray_stack(32, 1);
    assert_eq!(
        edit(float, &replace, &samples).unwrap_err(),
        "native scalar color plane requires u16 storage"
    );
    // Its plane read refuses for good, by name.
    let read = json!({"op":"read_plane","target":"gray","index":0});
    assert_eq!(
        edit(gray, &read, &[]).unwrap_err(),
        "read_plane of the Gray layer is unavailable in a Grayscale document: its one colour channel, Gray, is the layer, so a scalar read takes target mask or channel."
    );
    free(float);
    free(rgb);
    free(gray);
}
