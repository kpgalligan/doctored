//! New source/growth FFI bridges: nullable inputs, scalar range, cancellation,
//! native ghost source math and byte expansion independently of host code.
//!
//! `dr_precision_doc_retouch_from` is checked against an oracle worked
//! here: a heal adds to its source the membrane of (destination − source) on
//! the region's edge, and on a linear column ramp read 60 px to the left that
//! difference is one constant, so the healed pixels are the plate's pixels
//! plus the ramp's 60-px step.
use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_doc::{
    dr_doc_layer_height, dr_doc_layer_image, dr_doc_layer_offset_x, dr_doc_layer_offset_y,
    dr_doc_layer_width,
};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_retouch::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::ptr;

const CANVAS: (u32, u32) = (240, 140);
/// How far to the left the request's matrix reads.
const SHIFT: u32 = 60;
/// `(x, y, width, height)` of the red square in the plates' picture.
const SQUARE: (u32, u32, u32, u32) = (70, 50, 40, 40);
const RED: [u16; 4] = [50000, 4000, 4000, 65535];
/// The stroke: a 60×60 box around the square's place `SHIFT` to the right.
const BOX: (u32, u32, u32, u32) = (120, 40, 60, 60);
/// Native codes: two 8-bit codes.
const TOLERANCE: i64 = 2 * 257;

/// The column ramp: a value from x alone, 100 native codes a pixel.
fn ramp(x: u32) -> u16 {
    (10000 + 100 * x) as u16
}

fn inside((x0, y0, w, h): (u32, u32, u32, u32), x: u32, y: u32) -> bool {
    x >= x0 && x < x0 + w && y >= y0 && y < y0 + h
}

fn typed(canvas: (u32, u32), layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        canvas.0,
        canvas.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

fn raster(name: &str, size: (u32, u32), pixels: Vec<[u16; 4]>) -> Layer {
    let image =
        PrecisionImage::from_rgba16(size.0, size.1, pixels, IccProfile::srgb(), None).unwrap();
    Layer::raster(name, image, (0, 0), 1.0, true).unwrap()
}

/// The opaque gray ramp over `size`, with the red square when `square`.
fn ramp_pixels(size: (u32, u32), square: bool) -> Vec<[u16; 4]> {
    (0..size.0 * size.1)
        .map(|i| {
            let (x, y) = (i % size.0, i / size.0);
            if square && inside(SQUARE, x, y) {
                RED
            } else {
                [ramp(x), ramp(x), ramp(x), 65535]
            }
        })
        .collect()
}

/// One canvas-sized layer: the ramp, with the red square when `square`.
fn picture(square: bool) -> PrecisionDocument {
    typed(
        CANVAS,
        vec![raster("Picture", CANVAS, ramp_pixels(CANVAS, square))],
    )
}

fn pixels(document: &PrecisionDocument) -> Vec<[u16; 4]> {
    document.layer_source(0).unwrap().to_rgba16().unwrap()
}

fn box_coverage() -> Vec<u16> {
    (0..CANVAS.0 * CANVAS.1)
        .map(|i| {
            if inside(BOX, i % CANVAS.0, i / CANVAS.0) {
                65535
            } else {
                0
            }
        })
        .collect()
}

/// `op` on layer 0, reading `SHIFT` px to the left of each destination pixel.
fn shifted(op: &str) -> Value {
    json!({"op": op, "layer": 0, "sample": "current",
           "matrix": [1.0, 0.0, 0.0, 1.0, -f64::from(SHIFT), 0.0]})
}

type Outcome = Result<Option<Box<PrecisionDocument>>, String>;

unsafe fn outcome(out: *mut PrecisionDocument, error: *mut std::ffi::c_char) -> Outcome {
    unsafe {
        if !error.is_null() {
            let message = CStr::from_ptr(error).to_string_lossy().into_owned();
            dr_string_free(error);
            assert!(out.is_null(), "a refusal leaves no document");
            return Err(message);
        }
        Ok((!out.is_null()).then(|| Box::from_raw(out)))
    }
}

fn from_plates(
    document: &PrecisionDocument,
    plates: &PrecisionDocument,
    request: &Value,
    input: &[u16],
) -> Outcome {
    let text = CString::new(request.to_string()).unwrap();
    unsafe {
        let mut error = ptr::null_mut();
        let out = dr_precision_doc_retouch_from(
            document,
            plates,
            text.as_ptr(),
            input.as_ptr(),
            input.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        );
        outcome(out, error)
    }
}

fn edited(document: &PrecisionDocument, request: &Value, input: &[u16]) -> Outcome {
    let text = CString::new(request.to_string()).unwrap();
    unsafe {
        let mut error = ptr::null_mut();
        let out = dr_precision_doc_edit(
            document,
            text.as_ptr(),
            input.as_ptr(),
            input.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        );
        outcome(out, error)
    }
}

#[test]
fn retouch_from_heals_the_plates_picture_onto_the_document() {
    // S holds the square, F is the ramp alone (the picture after a fill).
    let (s, f) = (picture(true), picture(false));
    let out = from_plates(&f, &s, &shifted("retouch_heal"), &box_coverage())
        .unwrap()
        .expect("the heal changes the document");
    let (before, after) = (pixels(&f), pixels(&out));
    let moved = (SQUARE.0 + SHIFT, SQUARE.1, SQUARE.2, SQUARE.3);
    let grown = (BOX.0 - 1, BOX.1 - 1, BOX.2 + 2, BOX.3 + 2);
    // The ramp's difference between a pixel and the one the matrix reads.
    let step = i64::from(ramp(SHIFT)) - i64::from(ramp(0));
    let mut worst = 0;
    for y in 0..CANVAS.1 {
        for x in 0..CANVAS.0 {
            let i = (y * CANVAS.0 + x) as usize;
            if !inside(grown, x, y) {
                assert_eq!(after[i], before[i], "({x},{y}) is outside the stroke");
                continue;
            }
            assert_eq!(after[i][3], 65535, "({x},{y}) keeps its alpha");
            for c in 0..3 {
                let want = if inside(moved, x, y) {
                    i64::from(RED[c]) + step
                } else {
                    i64::from(before[i][c])
                };
                let error = (i64::from(after[i][c]) - want).abs();
                assert!(
                    error <= TOLERANCE,
                    "({x},{y}) channel {c}: {} against {want}",
                    after[i][c]
                );
                worst = worst.max(error);
            }
        }
    }
    println!("retouch_from heal: largest error {worst} native codes (bound {TOLERANCE})");
    assert_eq!(pixels(&s), ramp_pixels(CANVAS, true), "the plates stand");
}

#[test]
fn retouch_from_the_document_itself_is_the_ordinary_edit() {
    let plane = box_coverage();
    for (name, document) in [("ramp", picture(false)), ("square", picture(true))] {
        for op in ["retouch_heal", "retouch_clone"] {
            let request = shifted(op);
            let direct = edited(&document, &request, &plane).unwrap();
            let from = from_plates(&document, &document, &request, &plane).unwrap();
            assert_eq!(
                from.as_deref().map(pixels),
                direct.as_deref().map(pixels),
                "{op} on the {name}"
            );
            // The square moves into the box under either op, so this
            // comparison is never two unchanged answers.
            assert!(name != "square" || from.is_some(), "{op} on the {name}");
            println!(
                "retouch_from identity: {op} on the {name} changed {}",
                from.is_some()
            );
        }
    }
}

#[test]
fn retouch_from_refusals_name_the_rule() {
    let (s, f) = (picture(true), picture(false));
    let plane = box_coverage();
    let refusal = |document: &PrecisionDocument, plates: &PrecisionDocument, request: &Value| {
        from_plates(document, plates, request, &plane)
            .err()
            .unwrap()
    };
    let ops = "retouch from another document's plates requires retouch_clone or retouch_heal";
    let spot = json!({"op": "spot_heal", "layer": 0, "x": BOX.0, "y": BOX.1,
                      "width": BOX.2, "height": BOX.3});
    assert_eq!(refusal(&f, &s, &spot), ops);
    assert_eq!(refusal(&f, &s, &json!({"op": "red_eye", "layer": 0})), ops);
    let in_place = "retouch from another document's plates writes its target layer in place: \
                    output must be layer and expand_destination false";
    let mut new_layer = shifted("retouch_heal");
    new_layer["output"] = json!("new_layer");
    assert_eq!(refusal(&f, &s, &new_layer), in_place);
    let mut expanding = shifted("retouch_clone");
    expanding["expand_destination"] = json!(true);
    assert_eq!(refusal(&f, &s, &expanding), in_place);
    // Plates of another canvas size.
    let narrow = (CANVAS.0 - 40, CANVAS.1);
    let other = typed(
        narrow,
        vec![raster("Picture", narrow, ramp_pixels(narrow, true))],
    );
    assert_eq!(
        refusal(&f, &other, &shifted("retouch_heal")),
        "retouch plates must have the document's canvas size"
    );
    // The document has the sampled layer; the plates do not.
    let two = typed(
        CANVAS,
        vec![
            raster("Picture", CANVAS, ramp_pixels(CANVAS, false)),
            raster("Source", CANVAS, ramp_pixels(CANVAS, true)),
        ],
    );
    let mut second = shifted("retouch_heal");
    second["source_layer"] = json!(1);
    assert_eq!(
        refusal(&two, &s, &second),
        "retouch plates have no raster layer at the sampled index"
    );
    // The samples are the request's planned input on the document.
    assert_eq!(
        from_plates(&f, &s, &shifted("retouch_heal"), &plane[1..])
            .err()
            .unwrap(),
        "high-depth input sample length does not match the core plan"
    );
}

#[test]
fn null_safety_sweep_every_retouch_export() {
    unsafe {
        let (typed_doc, plane) = (picture(false), box_coverage());
        let text = CString::new(shifted("retouch_heal").to_string()).unwrap();
        for report in [true, false] {
            // NULL document, plates, request and samples in turn.
            for missing in 0..4 {
                let mut error = ptr::null_mut();
                let err = if report { &mut error } else { ptr::null_mut() };
                let handle = |at: usize| -> *const PrecisionDocument {
                    if missing == at {
                        ptr::null()
                    } else {
                        &typed_doc
                    }
                };
                assert!(dr_precision_doc_retouch_from(
                    handle(0),
                    handle(1),
                    if missing == 2 {
                        ptr::null()
                    } else {
                        text.as_ptr()
                    },
                    if missing == 3 {
                        ptr::null()
                    } else {
                        plane.as_ptr()
                    },
                    plane.len(),
                    REFERENCE_WORKING_BYTES,
                    0,
                    ptr::null(),
                    err
                )
                .is_null());
                assert_eq!(!error.is_null(), report, "argument {missing} is an error");
                if !error.is_null() {
                    dr_string_free(error);
                }
            }
            let mut error = ptr::null_mut();
            let err = if report { &mut error } else { ptr::null_mut() };
            assert!(dr_precision_doc_retouch_source(
                ptr::null(),
                0,
                0,
                false,
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                err
            )
            .is_null());
            if !error.is_null() {
                dr_string_free(error);
            }
            error = ptr::null_mut();
            let err = if report { &mut error } else { ptr::null_mut() };
            assert!(dr_precision_image_transform_retouch_source(
                ptr::null(),
                ptr::null(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                err
            )
            .is_null());
            if !error.is_null() {
                dr_string_free(error);
            }
            error = ptr::null_mut();
            let err = if report { &mut error } else { ptr::null_mut() };
            assert!(
                dr_image_transform_retouch_source(ptr::null(), ptr::null(), ptr::null(), err)
                    .is_null()
            );
            if !error.is_null() {
                dr_string_free(error);
            }
            error = ptr::null_mut();
            let err = if report { &mut error } else { ptr::null_mut() };
            assert!(dr_doc_expand_retouch_destination(ptr::null(), 0, ptr::null(), err).is_null());
            if !error.is_null() {
                dr_string_free(error);
            }
        }
    }
}
#[test]
fn native_and_byte_source_bridges_share_transparent_edge_math() {
    unsafe {
        let input = PrecisionImage::from_rgba16(
            2,
            1,
            vec![[50123, 32107, 10103, 65535], [0, 65535, 0, 0]],
            IccProfile::srgb(),
            None,
        )
        .unwrap();
        let doc = PrecisionDocument::from_image(input, Metadata::default(), Resolution::default())
            .unwrap();
        let mut error = ptr::null_mut();
        let plate = dr_precision_doc_retouch_source(
            &doc,
            0,
            0,
            false,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        );
        assert!(!plate.is_null() && error.is_null());
        let plate = Box::from_raw(plate);
        assert_eq!(
            plate.to_rgba16().ok(),
            doc.layer_source(0).unwrap().to_rgba16().ok()
        );
        let matrix = [1., 0., 0., 1., 0.5, 0.];
        let transformed = dr_precision_image_transform_retouch_source(
            &*plate,
            matrix.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        );
        assert!(!transformed.is_null() && error.is_null());
        let transformed = Box::from_raw(transformed);
        assert_eq!(
            transformed.to_rgba16().ok().unwrap()[0],
            [50123, 32107, 10103, 32768]
        );
        let byte = dr_image_from_rgba([230, 20, 10, 255, 0, 200, 0, 0].as_ptr(), 2, 1);
        let out = dr_image_transform_retouch_source(byte, matrix.as_ptr(), ptr::null(), &mut error);
        assert!(!out.is_null() && error.is_null());
        assert_eq!(
            std::slice::from_raw_parts(dr_image_pixels_rgba(out), 4),
            [230, 20, 10, 128]
        );
        assert!(
            dr_image_transform_retouch_source(byte, ptr::null(), ptr::null(), &mut error).is_null()
        );
        assert!(!error.is_null());
        dr_string_free(error);
        let job = DrJobControl::default();
        job.cancel();
        error = ptr::null_mut();
        assert!(
            dr_image_transform_retouch_source(byte, matrix.as_ptr(), &job, &mut error).is_null()
        );
        assert!(!error.is_null());
        dr_string_free(error);
        assert_eq!(job.scratch_bytes(), 0);
        let job = DrJobControl::default();
        job.cancel();
        error = ptr::null_mut();
        assert!(dr_precision_image_transform_retouch_source(
            &*plate,
            matrix.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            &job,
            &mut error
        )
        .is_null());
        assert!(!error.is_null());
        dr_string_free(error);
        let job = DrJobControl::default();
        job.cancel();
        error = ptr::null_mut();
        assert!(dr_precision_doc_retouch_source(
            &doc,
            0,
            0,
            false,
            REFERENCE_WORKING_BYTES,
            0,
            &job,
            &mut error
        )
        .is_null());
        assert!(!error.is_null());
        dr_string_free(error);
        dr_image_free(out);
        dr_image_free(byte);
    }
}
#[test]
fn byte_growth_is_explicit_and_preserves_existing_storage() {
    unsafe {
        let original =
            DrDocument::from_pixels(RgbaImage::from_pixel(4, 3, Rgba([10, 20, 30, 255])));
        let tiny = original
            .with_layer_pixels(0, RgbaImage::from_pixel(2, 1, Rgba([31, 47, 59, 127])))
            .unwrap()
            .with_layer_offset(0, -1, 1)
            .unwrap();
        let mut error = ptr::null_mut();
        let out = dr_doc_expand_retouch_destination(&tiny, 0, ptr::null(), &mut error);
        assert!(!out.is_null() && error.is_null());
        let out = Box::from_raw(out);
        assert_eq!(
            (dr_doc_layer_width(&*out, 0), dr_doc_layer_height(&*out, 0)),
            (5, 3)
        );
        assert_eq!(
            (
                dr_doc_layer_offset_x(&*out, 0),
                dr_doc_layer_offset_y(&*out, 0)
            ),
            (-1, 0)
        );
        let layer = dr_doc_layer_image(&*out, 0);
        assert!(!layer.is_null());
        let bytes = std::slice::from_raw_parts(dr_image_pixels_rgba(layer), 5 * 3 * 4);
        assert_eq!(&bytes[20..24], [31, 47, 59, 127]);
        assert_eq!(&bytes[24..28], [31, 47, 59, 127]);
        assert_eq!(&bytes[28..32], [0; 4]);
        dr_image_free(layer);
        assert_eq!(
            (dr_doc_layer_width(&tiny, 0), dr_doc_layer_height(&tiny, 0)),
            (2, 1)
        );
        let job = DrJobControl::default();
        job.cancel();
        assert!(dr_doc_expand_retouch_destination(&tiny, 0, &job, &mut error).is_null());
        assert!(!error.is_null());
        dr_string_free(error);
        assert_eq!(job.scratch_bytes(), 0);
    }
}
