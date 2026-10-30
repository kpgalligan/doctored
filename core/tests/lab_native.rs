//! #1 Lab Color phase 1 in `.rz` (word 107 and its `LAB ` table): a Lab
//! document of two rasters, a group, a mask, a hidden layer and opacity 0.5
//! reopens at 8 and 16 bits with `color_mode` lab and its depth, the same
//! render, and a re-save byte-equal to the first file; the reader refuses a
//! bad schema, a bad depth and a body profile that is not the Lab twin by
//! name (byte-patched files, as `gray_files.rs` does).

use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_lab::dr_precision_doc_convert_to_lab;
use doctored_core::ffi_precision::{
    dr_precision_doc_descriptor, dr_precision_doc_free, dr_precision_doc_info,
    dr_precision_doc_open, dr_precision_doc_save_native,
};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const W: u32 = 6;

fn json_of(text: *mut std::ffi::c_char, err: *mut std::ffi::c_char) -> Value {
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    json_of(unsafe { dr_precision_doc_descriptor(doc, &mut err) }, err)
}

fn info(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    json_of(unsafe { dr_precision_doc_info(doc, &mut err) }, err)
}

fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
}

fn render16(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn edit_with(
    doc: *const PrecisionDocument,
    request: &Value,
    input: &[u16],
) -> *mut PrecisionDocument {
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
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    out
}

fn save(doc: *const PrecisionDocument, path: &Path) {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_precision_doc_save_native(doc, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) },
        "{}",
        take_err_string(err)
    );
}

fn open(path: &Path) -> Result<*mut PrecisionDocument, String> {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

fn raster(name: &str, seed: u32, opacity: f32) -> Layer {
    let pixels: Vec<[u16; 4]> = (0..W)
        .map(|i| {
            let v = |k: u32| ((i * 9973 + seed * k) % 65536) as u16;
            [v(17), v(31), v(53), 65535 - (i * 4000) as u16]
        })
        .collect();
    Layer::raster(
        name,
        PrecisionImage::from_rgba16(W, 1, pixels, IccProfile::srgb(), None).unwrap(),
        (0, 0),
        opacity,
        true,
    )
    .unwrap()
}

/// The structure (module doc) as a Lab document of `depth`.
fn structured_lab(depth: u32) -> *mut PrecisionDocument {
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            W,
            1,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            vec![raster("Back", 11, 1.0), raster("Top", 29, 0.5)],
        )
        .unwrap(),
    ));
    let mask: Vec<u16> = (0..W as u16).map(|i| 65535 - i * 9000).collect();
    let masked = edit_with(
        rgb,
        &json!({"op":"set_mask","layer":1,"width":W,"height":1,"x":0,"y":0,"linked":true}),
        &mask,
    );
    let grouped = edit_with(
        masked,
        &json!({"op":"group_layers","layers":[1],"name":"Group"}),
        &[],
    );
    let hidden = edit_with(
        grouped,
        &json!({"op":"layer_visible","layer":0,"visible":false}),
        &[],
    );
    let options = CString::new(format!(r#"{{"depth":{depth}}}"#)).unwrap();
    let mut err = ptr::null_mut();
    let lab = unsafe {
        dr_precision_doc_convert_to_lab(hidden, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!lab.is_null(), "{}", take_err_string(err));
    for doc in [hidden, grouped, masked, rgb] {
        free(doc);
    }
    lab
}

#[test]
fn a_structured_lab_document_round_trips_at_8_and_16_bits() {
    let dir = TempDir::new().unwrap();
    for depth in [8, 16] {
        let lab = structured_lab(depth);
        let d = descriptor(lab);
        assert_eq!(
            (d["color_mode"].clone(), d["lab"]["depth"].clone()),
            ("lab".into(), depth.into())
        );
        let path = dir.path().join(format!("lab{depth}.rz"));
        save(lab, &path);
        let reopened = open(&path).unwrap();
        let r = descriptor(reopened);
        assert_eq!(
            (
                r["color_mode"].clone(),
                r["lab"]["depth"].clone(),
                r["layer_count"].clone()
            ),
            ("lab".into(), json!(depth), d["layer_count"].clone())
        );
        assert_eq!(info(reopened)["layers"], info(lab)["layers"]);
        assert_eq!(render16(reopened), render16(lab));
        let again = dir.path().join(format!("lab{depth}-again.rz"));
        save(reopened, &again);
        assert_eq!(
            std::fs::read(&again).unwrap(),
            std::fs::read(&path).unwrap(),
            "{depth}-bit re-save"
        );
        free(reopened);
        free(lab);
    }
}

#[test]
fn the_native_reader_refuses_a_lab_table_that_disagrees_with_the_body() {
    let dir = TempDir::new().unwrap();
    let lab = structured_lab(16);
    let path = dir.path().join("r16.rz");
    save(lab, &path);
    let twin = unsafe { &*lab }.profile().bytes().to_vec();
    free(lab);
    let bytes = std::fs::read(&path).unwrap();
    // The LAB table: tag, schema u32 = 1, depth u8.
    let at = bytes
        .windows(8)
        .rposition(|w| w == b"LAB \x01\x00\x00\x00")
        .expect("LAB table");
    assert_eq!(bytes[at + 8], 16);
    // The body's profile slot holds the twin verbatim.
    let profile_at = bytes
        .windows(twin.len())
        .position(|w| w == twin.as_slice())
        .expect("the twin in the body");
    let reopen = |patch: &dyn Fn(&mut Vec<u8>), name: &str| -> String {
        let mut patched = bytes.clone();
        patch(&mut patched);
        let path: PathBuf = dir.path().join(name);
        std::fs::write(&path, &patched).unwrap();
        open(&path).expect_err(name)
    };
    assert_eq!(
        reopen(&|b| b[at + 4] = 2, "schema.rz"),
        "unsupported native Lab schema"
    );
    assert_eq!(
        reopen(&|b| b[at + 8] = 32, "depth.rz"),
        "invalid native Lab depth"
    );
    // The low byte of the `desc` text's first UTF-16 unit ("L" → "M"):
    // still a profile, no longer the twin.
    let desc = twin
        .windows(4)
        .position(|w| w == b"mluc")
        .expect("the desc tag");
    assert_eq!(
        reopen(&|b| b[profile_at + desc + 29] ^= 0x01, "profile.rz"),
        "native Lab document profile is not the Lab twin"
    );
}
