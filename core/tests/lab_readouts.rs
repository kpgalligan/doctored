//! #1 Lab Color phase 1c, the readouts, through the FFI against the CIE
//! oracles of `common/lab_fixture.rs`: the gate admits the scalar
//! statistics of a layer mask and (since phase 2d) of a colour plane, and
//! since phase 2e a colour or alpha plane as a read or edit target and the
//! alpha plane's statistics, and the descriptor
//! lists `sample`; `scoped_analysis`'s `sample` of a 16-bit Lab document of
//! sRGB patches decodes within 0.05 of the formula, and the same sample of
//! its render is equal; the histogram of white puts every weight in bins
//! 255 / 128 / 128; and the thumbnail of an 8-bit Lab render is each code
//! / 257 exactly (the raw codes, not the Display P3 view).

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi::{dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width};
use doctored_core::ffi_lab::{dr_precision_doc_convert_to_lab, dr_precision_doc_new_lab};
use doctored_core::ffi_precision::{
    dr_precision_doc_descriptor, dr_precision_doc_free, dr_precision_doc_render,
    dr_precision_image_free, dr_precision_image_thumbnail_rgba8,
};
use doctored_core::ffi_precision_edit::{
    dr_precision_doc_edit, dr_precision_doc_read_plane, dr_precision_doc_scalar_statistics,
};
use doctored_core::ffi_scoped_statistics::{
    dr_precision_doc_scoped_analysis, dr_precision_image_scoped_analysis,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::lab_fixture::{decode, srgb_to_lab};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// `lab_convert.rs`'s sRGB patches (8-bit codes).
const PATCHES: [[u16; 3]; 10] = [
    [200, 40, 90],
    [30, 180, 220],
    [128, 128, 128],
    [250, 250, 10],
    [64, 0, 255],
    [0, 0, 0],
    [255, 255, 255],
    [90, 160, 40],
    [17, 34, 51],
    [240, 128, 200],
];

fn answer(text: *mut c_char, err: *mut c_char) -> Result<Value, String> {
    if text.is_null() {
        return Err(take_err_string(err));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    Ok(value)
}

fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
}

fn new_lab(depth: u32, w: u32, h: u32) -> *mut PrecisionDocument {
    let request = CString::new(format!(r#"{{"depth":{depth},"fill_srgb":[1,1,1,1]}}"#)).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_new_lab(w, h, request.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

/// The patches as one opaque row of a 16-bit sRGB document, converted by
/// Image ▸ Mode ▸ Lab Color at `depth`.
fn patch_lab(depth: u32) -> *mut PrecisionDocument {
    let pixels: Vec<[u16; 4]> = PATCHES
        .iter()
        .map(|p| [p[0] * 257, p[1] * 257, p[2] * 257, 65535])
        .collect();
    let image =
        PrecisionImage::from_rgba16(pixels.len() as u32, 1, pixels, IccProfile::srgb(), None)
            .unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ));
    let options = CString::new(format!(r#"{{"depth":{depth}}}"#)).unwrap();
    let mut err = ptr::null_mut();
    let lab = unsafe {
        dr_precision_doc_convert_to_lab(rgb, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!lab.is_null(), "{}", take_err_string(err));
    free(rgb);
    lab
}

fn edit(
    doc: *const PrecisionDocument,
    request: &Value,
    input: &[u16],
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            if input.is_empty() {
                ptr::null()
            } else {
                input.as_ptr()
            },
            input.len(),
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

fn statistics(doc: *const PrecisionDocument, request: &Value) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_scalar_statistics(
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
    answer(raw, err)
}

fn read_plane(
    doc: *const PrecisionDocument,
    request: &Value,
    n: usize,
) -> Result<Vec<u16>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut out = vec![0u16; n];
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
    if ok {
        Ok(out)
    } else {
        Err(take_err_string(err))
    }
}

fn doc_analysis(doc: *const PrecisionDocument, request: &Value) -> Value {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let raw = unsafe {
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
    answer(raw, err).unwrap()
}

fn image_analysis(image: *const PrecisionImage, request: &Value) -> Value {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let raw = unsafe {
        dr_precision_image_scoped_analysis(
            image,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    answer(raw, err).unwrap()
}

fn render(doc: *const PrecisionDocument) -> *mut PrecisionImage {
    let mut err = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "{}", take_err_string(err));
    image
}

#[test]
fn the_gate_admits_mask_and_plane_statistics_and_since_phase_2e_a_plane_read_and_edit() {
    let (w, h) = (4u32, 3u32);
    let lab = new_lab(16, w, h);
    let mask: Vec<u16> = (0..w * h).map(|i| (i * 5000) as u16).collect();
    let masked = edit(
        lab,
        &json!({"op":"set_mask","layer":0,"width":w,"height":h,"x":0,"y":0,"linked":true}),
        &mask,
    )
    .unwrap();
    let report = statistics(
        masked,
        &json!({"op":"scalar_statistics","target":"mask","index":0}),
    )
    .unwrap();
    assert!(report.is_object(), "{report}");
    for target in ["red", "green", "blue", "alpha"] {
        // Since phase 2d the colour planes' statistics are admitted (they
        // read the L*, a* and b* codes, `lab_targets.rs`); since phase 2e the
        // alpha's, a plane read and a plane edit too.
        let plane = read_plane(
            masked,
            &json!({"op":"read_plane","target":target,"index":0}),
            (w * h) as usize,
        );
        assert!(plane.is_ok(), "read_plane {target}: {plane:?}");
        let stats = statistics(
            masked,
            &json!({"op":"scalar_statistics","target":target,"index":0}),
        );
        assert!(stats.is_ok(), "scalar_statistics {target}: {stats:?}");
        let filled = edit(
            masked,
            &json!({"op":"scalar_edit","target":target,"index":0,
                "operation":{"kind":"fill","value":0,"opacity":1.0}}),
            &[],
        )
        .unwrap_or_else(|e| panic!("scalar_edit {target}: {e}"));
        free(filled);
    }
    let mut err = ptr::null_mut();
    let descriptor = answer(unsafe { dr_precision_doc_descriptor(lab, &mut err) }, err).unwrap();
    assert!(descriptor["supported_operations"]
        .as_array()
        .unwrap()
        .contains(&"sample".into()));
    free(masked);
    free(lab);
}

#[test]
fn a_sample_of_a_16_bit_lab_document_decodes_to_the_cie_formula() {
    let lab = patch_lab(16);
    let projection = render(lab);
    for (x, patch) in PATCHES.iter().enumerate() {
        let request = json!({"operation":"sample","source":"composite","x":x,"y":0,"reach":0});
        let report = doc_analysis(lab, &request);
        let mean: Vec<f64> = report["mean_rgba_codes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let codes = [0, 1, 2, 3].map(|c| mean[c].round() as u16);
        let got = decode(codes);
        let oracle = srgb_to_lab(patch.map(|c| f64::from(c) / 255.0));
        for c in 0..3 {
            assert!(
                (got[c] - oracle[c]).abs() <= 0.05,
                "patch {x} channel {c}: {got:?} vs {oracle:?}"
            );
        }
        assert_eq!(image_analysis(projection, &request), report, "patch {x}");
    }
    unsafe { dr_precision_image_free(projection) };
    free(lab);
}

#[test]
fn the_histogram_of_white_lab_puts_every_weight_in_bins_255_128_128() {
    for depth in [8, 16] {
        let lab = new_lab(depth, 5, 4);
        let report = doc_analysis(lab, &json!({"operation":"histogram","source":"composite"}));
        let rows = report["bin_weight_units"].as_array().unwrap();
        for (c, bin) in [255usize, 128, 128].into_iter().enumerate() {
            let row: Vec<u64> = rows[c]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap())
                .collect();
            let total: u64 = row.iter().sum();
            assert!(total > 0, "depth {depth} channel {c}");
            assert_eq!(row[bin], total, "depth {depth} channel {c}: {row:?}");
        }
        free(lab);
    }
}

#[test]
fn the_thumbnail_of_an_8_bit_lab_render_is_its_codes_over_257() {
    let lab = patch_lab(8);
    let projection = render(lab);
    let codes = unsafe { &*projection }.to_rgba16().unwrap();
    let mut err = ptr::null_mut();
    let thumbnail = unsafe {
        dr_precision_image_thumbnail_rgba8(projection, 0, LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!thumbnail.is_null(), "{}", take_err_string(err));
    let (w, h, bytes) = unsafe {
        let (w, h) = (dr_image_width(thumbnail), dr_image_height(thumbnail));
        let raw = std::slice::from_raw_parts(dr_image_pixels_rgba(thumbnail), (w * h * 4) as usize);
        (w, h, raw.to_vec())
    };
    unsafe { dr_image_free(thumbnail) };
    assert_eq!((w, h), (PATCHES.len() as u32, 1));
    for (x, sample) in codes.iter().enumerate() {
        for c in 0..4 {
            assert_eq!(
                sample[c] % 257,
                0,
                "patch {x} channel {c} on the 8-bit grid"
            );
            assert_eq!(
                u16::from(bytes[x * 4 + c]),
                sample[c] / 257,
                "patch {x} channel {c}"
            );
        }
    }
    unsafe { dr_precision_image_free(projection) };
    free(lab);
}
