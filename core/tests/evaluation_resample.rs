//! P7-24 phase 2: a linear-light document's geometry resamples layer colour
//! in premultiplied LINEAR light — Image Size, Free Transform (affine,
//! perspective and a set) and Warp, at both depths — against
//! `linear_oracle`: each tap decoded through the analytic curve, the weights
//! the kernel's own definition gives, the result re-encoded once.
//!
//! The fixtures put every checked sample where the weights are exact without
//! the walk's edge renormalization: a one-pixel checkerboard halved
//! Bilinear has the taps 1/8, 3/8, 3/8, 1/8 on every interior sample, and a
//! half-pixel translation Bilinear averages two neighbours. What still copies
//! codes — Nearest, an integer translation — must, bit for bit; and a
//! transparent pixel's hidden colour must never reach a neighbour.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_geometry::dr_doc_geometry_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::warp::WarpEdit;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::linear_oracle::{
    assert_within, encode, lin, linear, take_err, typed_code, typed_edit, typed_encode,
    typed_linear, typed_within,
};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const A: [u8; 3] = [30, 60, 200];
const B: [u8; 3] = [220, 180, 40];

fn byte_geometry(doc: &DrDocument, request: &Value) -> DrDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_geometry_edit(doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!out.is_null(), "{request}: {}", take_err(err));
    *unsafe { Box::from_raw(out) }
}

fn byte_layer(doc: &DrDocument) -> (RgbaImage, (i32, i32)) {
    (
        doc.layers[0].pixels.contiguous_image().as_ref().clone(),
        doc.layers[0].offset,
    )
}

fn typed_from(image: &RgbaImage) -> PrecisionDocument {
    let samples = image
        .pixels()
        .map(|p| p.0.map(typed_code))
        .collect::<Vec<_>>();
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            image.width(),
            image.height(),
            samples,
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

fn typed_layer(doc: &PrecisionDocument) -> (Vec<[u16; 4]>, u32) {
    let image = doc.layer_source(0).unwrap();
    (image.to_rgba16().unwrap(), image.dimensions().0)
}

fn checker(side: u32) -> RgbaImage {
    RgbaImage::from_fn(side, side, |x, y| {
        let v = if (x + y) % 2 == 0 { 0 } else { 255 };
        Rgba([v, v, v, 255])
    })
}

/// Left half `A`, right half `B`, opaque.
fn halves(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, _| {
        let c = if x < w / 2 { A } else { B };
        Rgba([c[0], c[1], c[2], 255])
    })
}

const HALF: &str = "bilinear";

fn resize(w: u32, h: u32, filter: &str) -> Value {
    json!({"op":"resize","width":w,"height":h,"filter":filter})
}

#[test]
fn image_size_halves_a_checkerboard_to_the_linear_mean_at_byte_depth() {
    let doc = DrDocument::from_pixels(checker(8));
    let (encoded, _) = byte_layer(&byte_geometry(&doc, &resize(4, 4, HALF)));
    let (switched, _) = byte_layer(&byte_geometry(&linear(&doc), &resize(4, 4, HALF)));
    let mean = encode(0.5);
    assert_eq!(mean, 188, "the oracle's linear mean");
    for y in 1..3 {
        for x in 1..3 {
            // The encoded document keeps its code-domain mean (128): its
            // arm is untouched.
            assert_within(
                encoded.get_pixel(x, y).0,
                [128, 128, 128, 255],
                1,
                "encoded",
            );
            assert_within(
                switched.get_pixel(x, y).0,
                [mean, mean, mean, 255],
                1,
                &format!("linear at ({x}, {y})"),
            );
        }
    }
}

#[test]
fn image_size_halves_a_checkerboard_to_the_linear_mean_at_typed_depth() {
    let doc = typed_from(&checker(8));
    let request = resize(4, 4, HALF).to_string();
    let (encoded, w) = typed_layer(&typed_edit(&doc, &request).unwrap());
    let (switched, _) = typed_layer(&typed_edit(&typed_linear(&doc), &request).unwrap());
    let mean = typed_encode(0.5);
    for y in 1..3 {
        for x in 1..3 {
            let i = (y * w + x) as usize;
            typed_within(encoded[i], [32768, 32768, 32768, 65535], "typed encoded");
            typed_within(
                switched[i],
                [mean, mean, mean, 65535],
                &format!("typed linear at ({x}, {y})"),
            );
        }
    }
}

/// The geometry requests that move a layer half a pixel to the right, as an
/// affine, a perspective quad and a set.
fn half_pixel_moves(w: u32, h: u32) -> Vec<Value> {
    let (w, h) = (f64::from(w), f64::from(h));
    vec![
        json!({"op":"affine","layer":0,"target":"pixels","matrix":[1,0,0,1,0.5,0],"filter":HALF}),
        json!({"op":"perspective","layer":0,"target":"pixels",
               "quad":[0.5,0,w + 0.5,0,w + 0.5,h,0.5,h],"filter":HALF}),
        json!({"op":"affine_set","layers":[0],"matrix":[1,0,0,1,0.5,0],"filter":HALF}),
    ]
}

/// The seam of a half-pixel move of [`halves`]: every opaque sample is `A`,
/// `B` or the one mixed column, which must be `mixed`.
fn assert_seam<P: Copy + PartialEq + std::fmt::Debug>(
    row: &[P],
    a: P,
    b: P,
    opaque: impl Fn(P) -> bool,
    within: impl Fn(P, &str),
    what: &str,
) {
    let mut seams = 0;
    for (x, &p) in row.iter().enumerate() {
        if !opaque(p) || p == a || p == b {
            continue;
        }
        within(p, &format!("{what} seam at {x}"));
        seams += 1;
    }
    assert_eq!(seams, 1, "{what}: one mixed column in {row:?}");
}

#[test]
fn a_half_pixel_move_mixes_the_seam_in_linear_light_at_byte_depth() {
    let doc = linear(&DrDocument::from_pixels(halves(8, 4)));
    let mixed = [0, 1, 2].map(|c| encode((lin(A[c]) + lin(B[c])) / 2.0));
    let warp = WarpEdit::parse(
        &json!({"op":"set","layer":0,"target":"pixels","columns":2,"rows":2,
                "displacements":[[0.5,0],[0.5,0],[0.5,0],[0.5,0]],"filter":HALF})
        .to_string(),
    )
    .unwrap();
    let warped = doc
        .warp_edit(&warp, LIMIT, 0, None)
        .unwrap()
        .expect("the warp changes the layer");
    let mut results: Vec<(String, DrDocument)> = half_pixel_moves(8, 4)
        .into_iter()
        .map(|request| (request["op"].to_string(), byte_geometry(&doc, &request)))
        .collect();
    results.push(("warp".into(), warped));
    for (what, moved) in results {
        let (image, _) = byte_layer(&moved);
        let row: Vec<[u8; 4]> = (0..image.width())
            .map(|x| image.get_pixel(x, 1).0)
            .collect();
        assert_seam(
            &row,
            [A[0], A[1], A[2], 255],
            [B[0], B[1], B[2], 255],
            |p| p[3] == 255,
            |p, label| assert_within(p, [mixed[0], mixed[1], mixed[2], 255], 1, label),
            &what,
        );
    }
}

#[test]
fn a_half_pixel_move_mixes_the_seam_in_linear_light_at_typed_depth() {
    let doc = typed_linear(&typed_from(&halves(8, 4)));
    let mixed = [0, 1, 2].map(|c| typed_encode((lin(A[c]) + lin(B[c])) / 2.0));
    let warp = WarpEdit::parse(
        &json!({"op":"set","layer":0,"target":"pixels","columns":2,"rows":2,
                "displacements":[[0.5,0],[0.5,0],[0.5,0],[0.5,0]],"filter":HALF})
        .to_string(),
    )
    .unwrap();
    let warped = doc
        .warp_edit(&warp, LIMIT, 0, None)
        .unwrap()
        .expect("the warp changes the layer");
    let mut results: Vec<(String, PrecisionDocument)> = half_pixel_moves(8, 4)
        .into_iter()
        .map(|request| {
            let moved = typed_edit(&doc, &request.to_string()).expect("the move changes");
            (request["op"].to_string(), moved)
        })
        .collect();
    results.push(("warp".into(), warped));
    let (a, b) = (A.map(typed_code), B.map(typed_code));
    for (what, moved) in results {
        let (samples, w) = typed_layer(&moved);
        let row = &samples[w as usize..2 * w as usize];
        assert_seam(
            row,
            [a[0], a[1], a[2], 65535],
            [b[0], b[1], b[2], 65535],
            |p| p[3] == 65535,
            |p, label| typed_within(p, [mixed[0], mixed[1], mixed[2], 65535], label),
            &format!("typed {what}"),
        );
    }
}

/// A transparent white pixel beside an opaque dark one: halved, the result
/// is the dark colour at half coverage — the hidden white never mixes in.
#[test]
fn a_transparent_pixel_leaks_nothing_into_a_resized_neighbour() {
    let image = RgbaImage::from_fn(2, 1, |x, _| {
        if x == 0 {
            Rgba([255, 255, 255, 0])
        } else {
            Rgba([20, 30, 40, 255])
        }
    });
    let doc = linear(&DrDocument::from_pixels(image.clone()));
    let (out, _) = byte_layer(&byte_geometry(&doc, &resize(1, 1, HALF)));
    assert_eq!(out.get_pixel(0, 0).0, [20, 30, 40, 128]);
    let typed = typed_linear(&typed_from(&image));
    let (samples, _) = typed_layer(&typed_edit(&typed, &resize(1, 1, HALF).to_string()).unwrap());
    typed_within(
        samples[0],
        [typed_code(20), typed_code(30), typed_code(40), 32768],
        "typed",
    );
}

/// Nearest and an integer move are permutations of stored samples: the
/// linear-light document returns its codes bit for bit.
#[test]
fn nearest_and_an_integer_move_copy_stored_codes() {
    let source = checker(8);
    let doc = DrDocument::from_pixels(source.clone());
    for request in [
        resize(4, 4, "nearest"),
        json!({"op":"affine","layer":0,"target":"pixels","matrix":[1,0,0,1,2,1],"filter":HALF}),
    ] {
        let encoded = byte_layer(&byte_geometry(&doc, &request));
        let switched = byte_layer(&byte_geometry(&linear(&doc), &request));
        assert_eq!(switched, encoded, "{request}");
    }
    let typed = typed_from(&source);
    for request in [
        resize(4, 4, "nearest"),
        json!({"op":"affine","layer":0,"target":"pixels","matrix":[1,0,0,1,2,1],"filter":HALF}),
    ] {
        let request = request.to_string();
        let encoded = typed_layer(&typed_edit(&typed, &request).unwrap());
        let switched = typed_layer(&typed_edit(&typed_linear(&typed), &request).unwrap());
        assert_eq!(switched, encoded, "{request}");
    }
}
