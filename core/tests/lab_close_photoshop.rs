//! #547 Lab close: Photoshop's layered menu walk on a 16-bit RGB document
//! and the same document converted to Lab Color (`~/devel/doctored-fixtures/
//! photoshop/lab-close-547/take1`, never committed; `scripts/photoshop-probe/
//! lab_close_take.py`, whose docstring fixes the documents, validity, `ITEMS`
//! and build table), read back in place.
//!
//! The record: take1 is VALID and its build table is [`RECORDED`], each
//! `ITEMS` path with its fate, pinned from take1's `decided.json`. A
//! `refuse_for_good` row with a core seam answers that seam's sentence: Sky
//! Replacement's (`dr_precision_doc_sky_replace`). Auto-Blend Layers refuses
//! for good on the host only (`LabProof.disabledTools`): its core Lab arm
//! stays, because Merge Panorama (recorded, shipped) blends a Lab stack through
//! it. SKIP when the take is absent or HOME is unset.

use std::ffi::CString;
use std::path::PathBuf;
use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_image_from_rgba};
use doctored_core::ffi_sky::dr_precision_doc_sky_replace;
use serde_json::Value;

use crate::common::take_err_string;
use crate::gray_ops::{free, LIMIT};
use crate::lab_targets::{pattern, srgb_doc, to_lab, H, W};

/// take1's build table: each `ITEMS` path and its fate.
const RECORDED: &[(&str, &str)] = &[
    ("Edit > Auto-Align Layers", "keep"),
    ("Edit > Auto-Blend Layers", "refuse_for_good"),
    ("Edit > Sky Replacement", "refuse_for_good"),
    ("File > Automate > Photomerge", "recorded"),
    ("Select > Subject", "keep"),
    ("Select > Sky", "keep"),
    ("Select > Focus Area", "recorded"),
    ("Filter > Neural Filters", "keep"),
    ("File > Place Embedded", "lift"),
    ("File > Place Linked", "lift"),
    ("File > Export > Layers to Files", "recorded"),
    ("File > Automate > Contact Sheet II", "recorded"),
    ("File > File Info", "lift"),
    ("Edit > Transform > Distort", "lift"),
];

/// Sky Replacement's core sentence on a Lab document (`doc_sky_replace`).
const SKY_REFUSAL: &str = "sky replacement is not available in a Lab Color document";

fn sky_replacement_error() -> String {
    let rgb = srgb_doc(pattern());
    let lab = to_lab(rgb, r#"{"depth":16}"#).unwrap();
    let sky = vec![120u8; (4 * W * H) as usize];
    let sky = unsafe { dr_image_from_rgba(sky.as_ptr(), W, H) };
    assert!(!sky.is_null());
    let cover: Vec<u16> = (0..W * H)
        .map(|i| if i / W < H / 2 { 65_535 } else { 0 })
        .collect();
    let params = CString::new("{}").unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_sky_replace(
            lab,
            0,
            sky,
            cover.as_ptr(),
            cover.len(),
            params.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    unsafe { dr_image_free(sky) };
    free(lab);
    free(rgb);
    take_err_string(err)
}

#[test]
fn the_layered_menu_walk_take() {
    let Some(root) = std::env::var("HOME").ok().map(|home| {
        PathBuf::from(home).join("devel/doctored-fixtures/photoshop/lab-close-547/take1")
    }) else {
        eprintln!("SKIP: HOME is unset, so the Photoshop take cannot be found");
        return;
    };
    let Some(decided) = std::fs::read_to_string(root.join("decided.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    else {
        eprintln!("SKIP: {} has no decided.json", root.display());
        return;
    };
    assert_eq!(decided["valid"], true, "{}", decided["checks"]);
    let rows: Vec<(String, String)> = decided["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["path"].as_str().unwrap().to_string(),
                row["fate"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let recorded: Vec<(String, String)> = RECORDED
        .iter()
        .map(|(path, fate)| (path.to_string(), fate.to_string()))
        .collect();
    assert_eq!(rows, recorded);
    // Nothing is enabled only in Lab, and no row needs the controller.
    assert_eq!(decided["enabled_only_in_lab"], serde_json::json!([]));
    assert!(rows.iter().all(|(_, fate)| fate != "report"));
    // The refused-for-good rows with a core seam answer its sentence.
    for (path, fate) in RECORDED {
        if *fate == "refuse_for_good" && *path == "Edit > Sky Replacement" {
            assert_eq!(sky_replacement_error(), SKY_REFUSAL);
        }
    }
}
