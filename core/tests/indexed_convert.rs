//! #1 Indexed Color phase 1a's conversions through the FFI: RGB → Indexed
//! and back exact, an 8-bit Grayscale source in its twin, the hard refusals
//! by sentence, the one-layer rule (`flatten_required`, then `flatten`),
//! Indexed → Grayscale through the unchanged Gray conversion, every edit
//! request refused by the mode's gate, and the null sweep of every export.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_gray::{dr_precision_doc_convert_to_gray, dr_precision_doc_new_gray};
use doctored_core::ffi_indexed::{
    dr_indexed_read_act, dr_indexed_write_act, dr_path_indexed_source,
    dr_precision_doc_convert_indexed_to_rgb, dr_precision_doc_convert_to_indexed,
    dr_precision_doc_convert_to_indexed_plan, dr_precision_doc_indexed_color_table,
    dr_precision_doc_indexed_index_at, dr_precision_doc_indexed_paste,
    dr_precision_doc_indexed_paste_plan, dr_precision_doc_open_indexed,
    dr_precision_doc_set_color_table, dr_precision_doc_set_color_table_plan,
};
use doctored_core::ffi_lab::dr_precision_doc_new_lab;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::json;

use crate::common::take_err_string;
use crate::indexed_fixture::{
    descriptor, free, indices, opaque, paint, plan, render16, rgb_doc, to_indexed, LIMIT,
};

const FLATTEN: &str = "Indexed Color holds one layer: flatten the image.";

fn to_rgb(doc: *const PrecisionDocument) -> Result<*mut PrecisionDocument, String> {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_convert_indexed_to_rgb(doc, &mut err) };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

fn new_gray(depth: u32) -> *mut PrecisionDocument {
    let request = CString::new(
        json!({"depth":depth,"space":"sgray","fill_srgb":[0.25,0.25,0.25,1.0],
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

fn colors() -> Vec<[u8; 3]> {
    vec![
        [255, 0, 0],
        [0, 128, 0],
        [10, 20, 30],
        [255, 0, 0],
        [9, 9, 9],
        [0, 128, 0],
    ]
}

#[test]
fn rgb_to_indexed_and_back_is_exact() {
    let doc = rgb_doc(3, 2, &opaque(&colors()));
    let indexed = to_indexed(doc, &json!({"palette":"exact"})).unwrap();
    let d = descriptor(indexed);
    assert_eq!(d["color_mode"], json!("indexed"));
    assert_eq!(d["indexed"]["colors"], json!(4));
    assert_eq!(indices(indexed), vec![0, 1, 2, 0, 3, 1]);
    assert_eq!(unsafe { &*indexed }.layer(0).unwrap().name(), "Index");
    assert_eq!(render16(indexed), render16(doc));
    let rgb = to_rgb(indexed).unwrap();
    assert_eq!(descriptor(rgb)["color_mode"], json!("rgb"));
    assert_eq!(render16(rgb), render16(doc));
    assert_eq!(indices(rgb), vec![-1; 6], "not Indexed any more");
    assert_eq!(
        to_rgb(doc).unwrap_err(),
        "the document is not Indexed Color"
    );
    for d in [rgb, indexed, doc] {
        free(d);
    }
}

#[test]
fn an_8_bit_grayscale_document_indexes_its_twin() {
    let gray = new_gray(8);
    let indexed = to_indexed(gray, &json!({"palette":"exact","dither":"none"})).unwrap();
    assert_eq!(descriptor(indexed)["color_mode"], json!("indexed"));
    assert_eq!(
        unsafe { &*indexed }.profile().bytes(),
        unsafe { &*gray }.profile().bytes(),
        "the Gray twin becomes the Indexed profile"
    );
    assert_eq!(render16(indexed), render16(gray));
    // Indexed → Grayscale is the unchanged Gray conversion.
    let options = CString::new(json!({"space":"sgray","depth":8}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let back = unsafe {
        dr_precision_doc_convert_to_gray(
            indexed,
            ptr::null(),
            0,
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!back.is_null(), "{}", take_err_string(err));
    assert_eq!(descriptor(back)["color_mode"], json!("gray"));
    for d in [back, indexed, gray] {
        free(d);
    }
}

#[test]
fn the_hard_refusals_name_the_route() {
    let gray16 = new_gray(16);
    let request = CString::new(
        json!({"depth":8,"fill_srgb":[0.5,0.5,0.5,1.0],"layer_name":"Background","ppi":72})
            .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let lab = unsafe { dr_precision_doc_new_lab(2, 2, request.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!lab.is_null(), "{}", take_err_string(err));
    let options = json!({"palette":"web"});
    assert_eq!(
        to_indexed(gray16, &options).unwrap_err(),
        "Indexed Color converts an 8-bit RGB or Grayscale document; convert to 8 \
         Bits/Channel first."
    );
    assert_eq!(
        to_indexed(lab, &options).unwrap_err(),
        "Indexed Color converts an 8-bit RGB or Grayscale document; convert to \
         RGB Color first."
    );
    assert_eq!(
        plan(lab, &options)["refusal"],
        json!(
            "Indexed Color converts an 8-bit RGB or Grayscale document; convert \
             to RGB Color first."
        )
    );
    let rgb = rgb_doc(1, 1, &[[1, 2, 3, 255]]);
    let indexed = to_indexed(rgb, &options).unwrap();
    assert_eq!(
        to_indexed(indexed, &options).unwrap_err(),
        "the document is already Indexed Color"
    );
    assert!(to_indexed(rgb, &json!({"palette":"web","bogus":1}))
        .unwrap_err()
        .contains("unknown key"));
    assert!(to_indexed(rgb, &json!({})).unwrap_err().contains("palette"));
    for d in [indexed, rgb, lab, gray16] {
        free(d);
    }
}

#[test]
fn more_than_one_plain_layer_must_flatten() {
    let raster = |name: &str, rgb: [u16; 3], opacity: f32| {
        let pixels = vec![[rgb[0], rgb[1], rgb[2], 65535]; 4];
        PrecisionDocumentLayer::raster(
            name,
            PrecisionImage::from_rgba16(2, 2, pixels, IccProfile::srgb(), None).unwrap(),
            (0, 0),
            opacity,
            true,
        )
        .unwrap()
    };
    let doc = Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            2,
            2,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            vec![
                raster("Back", [0, 0, 0], 1.0),
                raster("Top", [65535, 65535, 65535], 1.0),
            ],
        )
        .unwrap(),
    ));
    let options = json!({"palette":"exact","dither":"none"});
    let p = plan(doc, &options);
    assert_eq!(
        (
            p["flatten_required"].clone(),
            p["refusal"].clone(),
            p["flattens"].clone()
        ),
        (json!(true), json!(FLATTEN), json!(false))
    );
    assert_eq!(to_indexed(doc, &options).unwrap_err(), FLATTEN);
    let flatten = json!({"palette":"exact","dither":"none","flatten":true});
    let p = plan(doc, &flatten);
    assert_eq!(
        (p["refusal"].clone(), p["flattens"].clone()),
        (json!(null), json!(true))
    );
    let indexed = to_indexed(doc, &flatten).unwrap();
    let d = descriptor(indexed);
    assert_eq!(
        (d["layer_count"].clone(), d["indexed"]["colors"].clone()),
        (json!(1), json!(2))
    );
    // The white top layer covers the black one; Exact pads white with black.
    assert_eq!(indices(indexed), vec![0; 4]);
    assert_eq!(render16(indexed)[0], [65535; 4]);
    free(indexed);
    free(doc);
}

fn edit(doc: *const PrecisionDocument, request: serde_json::Value) -> String {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
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
    assert!(out.is_null());
    take_err_string(err)
}

#[test]
fn every_edit_request_refuses_by_the_modes_sentence() {
    let rgb = rgb_doc(2, 1, &[[1, 2, 3, 255], [4, 5, 6, 255]]);
    let indexed = to_indexed(rgb, &json!({"palette":"exact"})).unwrap();
    // Since phase 3a the row's visibility and order are admitted.
    let hidden = paint(
        indexed,
        &json!({"op":"layer_visible","layer":0,"visible":false}),
        &[],
    )
    .unwrap()
    .expect("hiding the layer is an edit");
    assert!(!unsafe { &*hidden }.layer(0).unwrap().visible());
    free(hidden);
    assert_eq!(
        paint(
            indexed,
            &json!({"op":"move_layer_to","from":0,"to":0,"depth":0}),
            &[]
        ),
        Ok(None)
    );
    assert_eq!(
        edit(indexed, json!({"op":"add_layer","name":"New","index":1})),
        "add_layer is not available in Indexed Color."
    );
    assert_eq!(
        edit(indexed, json!({"op":"flatten"})),
        "flatten needs more than one layer, and an Indexed Color document holds one."
    );
    assert_eq!(
        edit(indexed, json!({"op":"liquify","layer":0})),
        "liquify is not available in Indexed Color."
    );
    free(indexed);
    free(rgb);
}

#[test]
fn an_unknown_dither_names_the_four() {
    let rgb = rgb_doc(2, 1, &[[1, 2, 3, 255], [4, 5, 6, 255]]);
    assert_eq!(
        to_indexed(rgb, &json!({"palette":"web","dither":"ordered"})).unwrap_err(),
        "mode option dither must be none, diffusion, pattern or noise"
    );
    free(rgb);
}

#[test]
fn every_indexed_export_tolerates_null() {
    let mut err = ptr::null_mut();
    unsafe {
        assert_eq!(
            dr_path_indexed_source(ptr::null()),
            0,
            "DR_INDEXED_SOURCE_NONE"
        );
        assert!(
            dr_precision_doc_open_indexed(ptr::null(), LIMIT, 0, ptr::null(), &mut err).is_null()
        );
        assert!(!err.is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(
            dr_precision_doc_open_indexed(ptr::null(), LIMIT, 0, ptr::null(), ptr::null_mut())
                .is_null()
        );
        assert!(
            dr_precision_doc_convert_to_indexed_plan(ptr::null(), ptr::null(), &mut err).is_null()
        );
        assert!(!err.is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(dr_precision_doc_convert_to_indexed_plan(
            ptr::null(),
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_convert_to_indexed(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(dr_precision_doc_convert_indexed_to_rgb(ptr::null(), &mut err).is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_precision_doc_convert_indexed_to_rgb(ptr::null(), ptr::null_mut()).is_null());
        assert_eq!(dr_precision_doc_indexed_index_at(ptr::null(), 0, 0), -1);
        // Phase 1c's colour table exports.
        err = ptr::null_mut();
        assert!(dr_precision_doc_indexed_color_table(ptr::null(), &mut err).is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_precision_doc_indexed_color_table(ptr::null(), ptr::null_mut()).is_null());
        err = ptr::null_mut();
        assert!(
            dr_precision_doc_set_color_table_plan(ptr::null(), ptr::null(), &mut err).is_null()
        );
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(
            dr_precision_doc_set_color_table_plan(ptr::null(), ptr::null(), ptr::null_mut())
                .is_null()
        );
        err = ptr::null_mut();
        assert!(dr_precision_doc_set_color_table(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_precision_doc_set_color_table(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        err = ptr::null_mut();
        assert!(dr_indexed_read_act(ptr::null(), &mut err).is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_indexed_read_act(ptr::null(), ptr::null_mut()).is_null());
        err = ptr::null_mut();
        assert!(!dr_indexed_write_act(ptr::null(), ptr::null(), &mut err));
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(!dr_indexed_write_act(
            ptr::null(),
            ptr::null(),
            ptr::null_mut()
        ));
        // Phase 2d's paste exports.
        err = ptr::null_mut();
        assert!(dr_precision_doc_indexed_paste_plan(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_precision_doc_indexed_paste_plan(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        err = ptr::null_mut();
        assert!(dr_precision_doc_indexed_paste(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_precision_doc_indexed_paste(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
    }
    // Off the canvas, and NULL options on a live document (the empty object:
    // palette is required).
    let rgb = rgb_doc(1, 1, &[[1, 2, 3, 255]]);
    let indexed = to_indexed(rgb, &json!({"palette":"web"})).unwrap();
    assert_eq!(
        unsafe { dr_precision_doc_indexed_index_at(indexed, 1, 0) },
        -1
    );
    assert_eq!(
        unsafe { dr_precision_doc_indexed_index_at(indexed, 0, 1) },
        -1
    );
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_indexed(rgb, ptr::null(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(out.is_null());
    assert!(take_err_string(err).contains("palette"));
    free(indexed);
    free(rgb);
}
