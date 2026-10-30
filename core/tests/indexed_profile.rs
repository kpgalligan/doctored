//! #1 Indexed Color phase 3a: Edit ▸ Assign Profile and Convert to Profile
//! on an Indexed document (`indexed_profile`).
//!
//! The oracles are independent of the adapter: Assign keeps every index,
//! the table's codes and every stored sample, only the profile changing;
//! Convert keeps every index and the transparent index, and each opaque
//! pixel's new table entry is that pixel converted by the RGB route (Image ▸
//! Mode ▸ RGB Color, then the same Convert to Profile on that RGB document,
//! read at 8-bit codes).

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_indexed::dr_precision_doc_set_color_table_plan;
use doctored_core::ffi_lab::dr_precision_doc_new_lab;
use doctored_core::ffi_output_color::dr_precision_doc_convert_to_profile_with;
use doctored_core::ffi_precision::dr_precision_doc_save_native;
use doctored_core::ffi_precision_color::{
    dr_precision_doc_assign_profile, dr_precision_doc_color_plan,
    dr_precision_doc_convert_to_profile,
};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::common::{cpath, take_err_string};
use crate::icc_spelling::adobe_rgb_1998;
use crate::indexed_fixture::{
    color_table, free, indices, layer16, open_rgb, palette_of, rgb_doc, to_indexed, to_rgb, window,
    LIMIT,
};

const NOT_RGB: &str = "An Indexed Color document's colour table is RGB: assign or convert to an RGB profile, or change the mode with Image ▸ Mode.";

const W: u32 = 6;
const H: u32 = 4;

/// A 6 × 4 sRGB scene of five colours and a clear pixel, converted Exact:
/// the palette holds a transparent index.
fn scene() -> *mut PrecisionDocument {
    let colors = [
        [0, 0, 0, 255],
        [255, 255, 255, 255],
        [190, 50, 50, 255],
        [40, 160, 60, 255],
        [40, 60, 200, 255],
        [0, 0, 0, 0],
    ];
    let pixels: Vec<[u8; 4]> = (0..W * H)
        .map(|i| colors[(i as usize * 7 + i as usize / W as usize) % colors.len()])
        .collect();
    let rgb = rgb_doc(W, H, &pixels);
    let out = to_indexed(rgb, &json!({"palette":"exact","dither":"none"})).unwrap();
    free(rgb);
    out
}

/// A NULL result is `Ok(None)` with no error, else the error.
fn result(
    out: *mut PrecisionDocument,
    err: *mut std::ffi::c_char,
) -> Result<Option<*mut PrecisionDocument>, String> {
    if !out.is_null() {
        return Ok(Some(out));
    }
    if err.is_null() {
        Ok(None)
    } else {
        Err(take_err_string(err))
    }
}

fn assign(
    doc: *const PrecisionDocument,
    icc: &[u8],
) -> Result<Option<*mut PrecisionDocument>, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_assign_profile(
            doc,
            icc.as_ptr(),
            icc.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err)
}

fn convert(
    doc: *const PrecisionDocument,
    icc: &[u8],
    options: Option<&Value>,
) -> Result<Option<*mut PrecisionDocument>, String> {
    let text = options.map(|o| CString::new(o.to_string()).unwrap());
    let mut err = ptr::null_mut();
    let mut report = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_profile_with(
            doc,
            icc.as_ptr(),
            icc.len(),
            text.as_ref().map_or(ptr::null(), |t| t.as_ptr()),
            LIMIT,
            0,
            ptr::null(),
            &mut report,
            &mut err,
        )
    };
    if !report.is_null() {
        unsafe { dr_string_free(report) };
    }
    result(out, err)
}

fn profile_bytes(doc: *const PrecisionDocument) -> Vec<u8> {
    unsafe { &*doc }.profile().bytes().to_vec()
}

#[test]
fn assign_keeps_every_index_and_code() {
    let doc = scene();
    let adobe = adobe_rgb_1998(false);
    assert_ne!(profile_bytes(doc), adobe);
    let out = assign(doc, &adobe)
        .unwrap()
        .expect("a new profile is an edit");
    assert!(unsafe { &*out }.is_indexed());
    assert_eq!(indices(out), indices(doc));
    assert_eq!(color_table(out), color_table(doc));
    assert_eq!(layer16(out), layer16(doc));
    assert_eq!(profile_bytes(out), adobe);
    // The document's own profile bytes change nothing.
    assert_eq!(assign(doc, &profile_bytes(doc)), Ok(None));
    free(out);
    free(doc);
}

/// `dr_precision_doc_convert_to_profile` (the default conversion).
fn convert_default(
    doc: *const PrecisionDocument,
    icc: &[u8],
) -> Result<Option<*mut PrecisionDocument>, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_profile(
            doc,
            icc.as_ptr(),
            icc.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err)
}

/// The Lab twin's bytes: an RGB-data profile that holds L*a*b* codes, read
/// from a new Lab document.
fn lab_twin() -> Vec<u8> {
    let request = CString::new(
        json!({"depth":8,"fill_srgb":[0.5,0.5,0.5,1.0],"layer_name":"Background","ppi":72})
            .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let lab = unsafe { dr_precision_doc_new_lab(2, 2, request.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!lab.is_null(), "{}", take_err_string(err));
    let bytes = profile_bytes(lab);
    free(lab);
    bytes
}

#[test]
fn a_profile_that_is_not_rgb_refuses() {
    let doc = scene();
    // A CMYK profile is refused by the profile argument itself, before the
    // document is asked.
    let press = press_profile("Indexed Press");
    let not_a_profile = "the profile is not an RGB ICC profile a document can carry";
    assert_eq!(assign(doc, &press), Err(not_a_profile.into()));
    assert_eq!(convert(doc, &press, None), Err(not_a_profile.into()));
    assert_eq!(convert_default(doc, &press), Err(not_a_profile.into()));
    // The Lab twin parses as RGB data but is no RGB colour space: the
    // Indexed document's own refusal.
    let twin = lab_twin();
    assert_eq!(assign(doc, &twin), Err(NOT_RGB.into()));
    assert_eq!(convert(doc, &twin, None), Err(NOT_RGB.into()));
    assert_eq!(convert_default(doc, &twin), Err(NOT_RGB.into()));
    free(doc);
}

#[test]
fn convert_converts_the_table_and_keeps_every_index() {
    let dir = TempDir::new().unwrap();
    let doc = scene();
    let (palette, t) = palette_of(doc);
    assert!(t.is_some());
    let before = indices(doc);
    let adobe = adobe_rgb_1998(false);
    for options in [
        None,
        Some(json!({"intent":"perceptual","black_point_compensation":true})),
    ] {
        let out = convert(doc, &adobe, options.as_ref())
            .unwrap()
            .expect("a new profile is an edit");
        assert!(unsafe { &*out }.is_indexed());
        assert_eq!(indices(out), before, "{options:?}");
        let (converted, converted_t) = palette_of(out);
        assert_eq!(converted_t, t);
        assert_eq!(converted.len(), palette.len());
        assert_ne!(converted, palette, "{options:?}: the table converts");
        assert_eq!(profile_bytes(out), adobe);
        // The RGB route, pixel by pixel.
        let rgb = to_rgb(doc);
        let twin = convert(rgb, &adobe, options.as_ref()).unwrap().unwrap();
        for (i, px) in window(twin).iter().enumerate() {
            if px[3] == 255 {
                assert_eq!(
                    converted[before[i] as usize],
                    [px[0], px[1], px[2]],
                    "{options:?}: pixel {i}"
                );
            }
        }
        // The converted document reopens through `.rz`.
        let path = dir.path().join("converted.rz");
        let c = cpath(&path);
        let mut err = ptr::null_mut();
        assert!(
            unsafe {
                dr_precision_doc_save_native(out, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
            },
            "{}",
            take_err_string(err)
        );
        let back = open_rgb(&path).unwrap();
        assert_eq!(indices(back), before);
        assert_eq!(color_table(back), color_table(out));
        assert_eq!(profile_bytes(back), adobe);
        free(back);
        free(twin);
        free(rgb);
        free(out);
    }
    assert_eq!(convert(doc, &profile_bytes(doc), None), Ok(None));
    free(doc);
}

#[test]
fn the_default_convert_is_the_with_convert() {
    let doc = scene();
    let before = indices(doc);
    let adobe = adobe_rgb_1998(false);
    let out = convert_default(doc, &adobe)
        .unwrap()
        .expect("a new profile is an edit");
    let with = convert(doc, &adobe, None).unwrap().unwrap();
    assert!(unsafe { &*out }.is_indexed());
    assert_eq!(indices(out), before);
    assert_eq!(color_table(out), color_table(with));
    assert_eq!(profile_bytes(out), adobe);
    // The document's own profile changes nothing.
    assert_eq!(convert_default(doc, &profile_bytes(doc)), Ok(None));
    free(with);
    free(out);
    free(doc);
}

fn json_of(text: *mut std::ffi::c_char, err: *mut std::ffi::c_char) -> Value {
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn color_plan(doc: *const PrecisionDocument, icc: &[u8], convert: bool) -> Value {
    let mut err = ptr::null_mut();
    json_of(
        unsafe {
            dr_precision_doc_color_plan(doc, icc.as_ptr(), icc.len(), convert, LIMIT, 0, &mut err)
        },
        err,
    )
}

#[test]
fn the_plans_are_the_storage_assign_and_the_table_rerender() {
    let doc = scene();
    let rgb = to_rgb(doc);
    let adobe = adobe_rgb_1998(false);
    let assign_plan = color_plan(doc, &adobe, false);
    let storage = color_plan(rgb, &adobe, false);
    assert_eq!(assign_plan["scratch_bytes"], storage["scratch_bytes"]);
    assert_eq!(assign_plan["retained_bytes"], storage["retained_bytes"]);
    let table = CString::new(color_table(doc).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let rerender = json_of(
        unsafe { dr_precision_doc_set_color_table_plan(doc, table.as_ptr(), &mut err) },
        err,
    )["working_memory_plan_bytes"]
        .as_u64()
        .unwrap();
    let convert_plan = color_plan(doc, &adobe, true);
    assert_eq!(
        convert_plan["scratch_bytes"].as_u64().unwrap(),
        assign_plan["scratch_bytes"].as_u64().unwrap() + rerender
    );
    free(rgb);
    free(doc);
}
