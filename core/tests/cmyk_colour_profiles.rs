//! Assign / Convert to Profile, 8/16 Bits/Channel and File ▸ New in a CMYK
//! document (#547 phase 5), through the FFI, against oracles written here
//! and in `common/cmyk_fixture.rs`: the CMYK → CMYK conversion against the
//! two synthetic presses' own tables in closed form (A2B → BPC → B2A, both
//! intents the fixture defines, BPC on and off); Assign's untouched
//! samples; the 8-bit grid `round(v / 257) · 257`; New's fill; the `.rz`
//! round trip of a converted, masked, Curves-adjusted document at words 82
//! and 83; every refusal by its sentence. No test here sets or clears the
//! process working CMYK profile: this binary runs with none set.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::{
    dr_precision_doc_cmyk_info, dr_precision_doc_cmyk_profile, dr_precision_doc_convert_to_cmyk,
};
use doctored_core::ffi_cmyk_colour::*;
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_open, dr_precision_doc_render,
    dr_precision_doc_save_native, dr_precision_image_free,
};
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::Value;
use tempfile::TempDir;

use crate::cmyk_fixture::{
    press_profile, press_profile_b, press_profile_without_b2a, press_to_press_b,
};
use crate::cmyk_tiff_builder::cmyk_tiff;
use crate::common::{cpath, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

fn free(document: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(document) };
}

fn json(text: *mut std::ffi::c_char, err: *mut std::ffi::c_char) -> Value {
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn open(path: &std::path::Path) -> *mut PrecisionDocument {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "open {path:?}: {}", take_err_string(err));
    doc
}

/// A 16-bit CMYK TIFF of `samples` in `icc`, opened as a CMYK document.
fn cmyk_doc(
    dir: &TempDir,
    name: &str,
    w: u32,
    h: u32,
    samples: &[[u16; 4]],
    icc: &[u8],
) -> *mut PrecisionDocument {
    let data: Vec<u8> = samples
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let path = dir.path().join(name);
    std::fs::write(&path, cmyk_tiff(w, h, 16, 4, 0, &data, Some(icc))).unwrap();
    open(&path)
}

fn samples(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 5]> {
    unsafe { &*doc }
        .cmyk_layer_samples(index)
        .expect("a CMYK record")
}

fn profile_bytes(doc: *const PrecisionDocument) -> Vec<u8> {
    let mut length = 0usize;
    let bytes = unsafe { dr_precision_doc_cmyk_profile(doc, &mut length) };
    unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec()
}

fn info(doc: *const PrecisionDocument) -> Value {
    let text = unsafe { dr_precision_doc_cmyk_info(doc) };
    json(text, ptr::null_mut())
}

/// The profile edit: `Ok(None)` is NULL with no error.
fn profile_edit(
    doc: *const PrecisionDocument,
    icc: Option<&[u8]>,
    request: &str,
) -> Result<Option<*mut PrecisionDocument>, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_cmyk_profile_edit(
            doc,
            icc.map_or(ptr::null(), |b| b.as_ptr()),
            icc.map_or(0, |b| b.len()),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if !out.is_null() {
        return Ok(Some(out));
    }
    if err.is_null() {
        Ok(None)
    } else {
        Err(take_err_string(err))
    }
}

fn plan(doc: *const PrecisionDocument, icc: Option<&[u8]>, request: &str) -> Value {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    json(
        unsafe {
            dr_precision_doc_cmyk_profile_plan(
                doc,
                icc.map_or(ptr::null(), |b| b.as_ptr()),
                icc.map_or(0, |b| b.len()),
                text.as_ptr(),
                &mut err,
            )
        },
        err,
    )
}

fn code(v: f64) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

/// A 5⁴ grid of inks.
fn ink_grid() -> Vec<[u16; 4]> {
    let level = |i: u16| i * 16383 + i / 4 * 3;
    let mut out = Vec::new();
    for c in 0..5u16 {
        for m in 0..5u16 {
            for y in 0..5u16 {
                for k in 0..5u16 {
                    out.push([level(c), level(m), level(y), level(k)]);
                }
            }
        }
    }
    out
}

#[test]
fn convert_to_profile_lands_on_the_second_presss_own_tables() {
    let dir = TempDir::new().unwrap();
    let grid = ink_grid();
    let doc = cmyk_doc(
        &dir,
        "grid.tif",
        grid.len() as u32,
        1,
        &grid,
        &press_profile("Press A"),
    );
    let b = press_profile_b("Press B");
    for (intent, perceptual) in [("relative", false), ("perceptual", true)] {
        for bpc in [false, true] {
            let request = format!(
                r#"{{"op":"convert","intent":"{intent}","black_point_compensation":{bpc}}}"#
            );
            let converted = profile_edit(doc, Some(&b), &request).unwrap().unwrap();
            assert_eq!(
                profile_bytes(converted),
                b,
                "the destination's bytes are kept"
            );
            for (ink, sample) in grid.iter().zip(samples(converted, 0)) {
                let expected =
                    press_to_press_b(ink.map(|v| f64::from(v) / 65535.0), perceptual, bpc);
                for c in 0..4 {
                    assert!(
                        sample[c].abs_diff(code(expected[c])) <= 1,
                        "{intent} bpc {bpc} {ink:?}: ink {c} {} vs oracle {}",
                        sample[c],
                        code(expected[c])
                    );
                }
                assert_eq!(sample[4], 65535, "alpha is carried");
            }
            free(converted);
        }
    }
    // Converting into the bytes it already holds changes nothing.
    assert_eq!(
        profile_edit(doc, Some(&press_profile("Press A")), r#"{"op":"convert"}"#),
        Ok(None)
    );
    free(doc);
}

#[test]
fn assign_keeps_every_sample_and_changes_only_the_profile() {
    let dir = TempDir::new().unwrap();
    let grid = ink_grid();
    let doc = cmyk_doc(&dir, "assign.tif", 25, 25, &grid, &press_profile("Press A"));
    let b = press_profile_b("Press B");
    let assigned = profile_edit(doc, Some(&b), r#"{"op":"assign"}"#)
        .unwrap()
        .unwrap();
    assert_eq!(samples(assigned, 0), samples(doc, 0));
    assert_eq!(profile_bytes(assigned), b);
    assert_eq!(info(assigned)["profile"], "Press B");
    assert_eq!(info(assigned)["depth"], 16);
    // The same bytes again is no edit.
    assert_eq!(
        profile_edit(assigned, Some(&b), r#"{"op":"assign"}"#),
        Ok(None)
    );
    free(assigned);
    free(doc);
}

#[test]
fn depth_rounds_raster_and_mask_samples_to_the_eight_bit_grid() {
    let dir = TempDir::new().unwrap();
    let (w, h) = (6u32, 4u32);
    let ink: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            [
                (i * 2731 % 65536) as u16,
                (i * 911 + 77) as u16,
                (65535 - i * 1999) as u16,
                (i * 129 + 1) as u16,
            ]
        })
        .collect();
    let doc = cmyk_doc(&dir, "depth.tif", w, h, &ink, &press_profile("Press A"));
    let mask: Vec<u16> = (0..w * h).map(|i| (i * 2857 + 3) as u16).collect();
    let request = CString::new(format!(
        r#"{{"op":"set_mask","layer":0,"width":{w},"height":{h}}}"#
    ))
    .unwrap();
    let mut err = ptr::null_mut();
    let masked = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            mask.as_ptr(),
            mask.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!masked.is_null(), "{}", take_err_string(err));
    let grid = |v: u16| ((f64::from(v) / 257.0).round() * 257.0) as u16;
    let eight = profile_edit(masked, None, r#"{"op":"depth","depth":8}"#)
        .unwrap()
        .unwrap();
    assert_eq!(info(eight)["depth"], 8);
    for (before, after) in samples(masked, 0).iter().zip(samples(eight, 0)) {
        assert_eq!(
            after,
            before.map(grid),
            "every raster sample, alpha included"
        );
    }
    let rounded = unsafe { &*eight }.layer(0).unwrap().mask().unwrap();
    let expected: Vec<u16> = mask.iter().map(|v| grid(*v)).collect();
    assert_eq!(
        rounded.plane().samples(),
        &expected[..],
        "every mask sample"
    );
    // The same depth is no edit; 8 → 16 changes only the declared depth.
    assert_eq!(
        profile_edit(eight, None, r#"{"op":"depth","depth":8}"#),
        Ok(None)
    );
    let sixteen = profile_edit(eight, None, r#"{"op":"depth","depth":16}"#)
        .unwrap()
        .unwrap();
    assert_eq!(info(sixteen)["depth"], 16);
    assert_eq!(samples(sixteen, 0), samples(eight, 0));
    assert_eq!(profile_bytes(sixteen), profile_bytes(doc));
    for d in [doc, masked, eight, sixteen] {
        free(d);
    }
}

#[test]
fn a_new_cmyk_document_is_one_filled_record() {
    let a = press_profile("Press A");
    let request = cr#"{"depth":8,"fill":[0.2,0.4,0.6,0.1,1],"layer_name":"Paper","ppi":300}"#;
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_new_cmyk(
            7,
            5,
            a.as_ptr(),
            a.len(),
            request.as_ptr(),
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    let eight = |v: f64| ((code(v) as f64 / 257.0).round() * 257.0) as u16;
    let expected = [eight(0.2), eight(0.4), eight(0.6), eight(0.1), 65535];
    assert!(samples(doc, 0).iter().all(|s| *s == expected));
    assert_eq!(samples(doc, 0).len(), 35);
    let d = unsafe { &*doc };
    assert_eq!(d.layer(0).unwrap().name(), "Paper");
    assert_eq!(d.resolution().x, 300.0);
    assert_eq!(info(doc)["depth"], 8);
    assert_eq!(profile_bytes(doc), a);
    // The defaults: 16 bits, transparent paper, "Background".
    let plain = unsafe {
        dr_precision_doc_new_cmyk(2, 2, a.as_ptr(), a.len(), ptr::null(), LIMIT, 0, &mut err)
    };
    assert!(!plain.is_null(), "{}", take_err_string(err));
    assert!(samples(plain, 0).iter().all(|s| *s == [0; 5]));
    assert_eq!(info(plain)["depth"], 16);
    // No profile and no working CMYK profile.
    let none =
        unsafe { dr_precision_doc_new_cmyk(2, 2, ptr::null(), 0, ptr::null(), LIMIT, 0, &mut err) };
    assert!(none.is_null());
    assert_eq!(
        take_err_string(err),
        "no working CMYK profile is set, so a CMYK profile must be named"
    );
    let bad = unsafe {
        dr_precision_doc_new_cmyk(
            2,
            2,
            a.as_ptr(),
            a.len(),
            cr#"{"fill":[2,0,0,0,1]}"#.as_ptr(),
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(bad.is_null());
    assert_eq!(
        take_err_string(err),
        "a new CMYK document's fill is [c, m, y, k, a], each 0…1"
    );
    free(doc);
    free(plain);
}

fn save(doc: *const PrecisionDocument, path: &std::path::Path) -> Vec<u8> {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_precision_doc_save_native(doc, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) },
        "{}",
        take_err_string(err)
    );
    std::fs::read(path).unwrap()
}

/// The version words after DRDC and 46, up to the body version (< 40).
fn words(bytes: &[u8]) -> Vec<u32> {
    let mut out = Vec::new();
    let mut at = 8;
    loop {
        let word = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        if word < 40 {
            return out;
        }
        out.push(word);
        at += 4;
    }
}

fn render(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    let mut err = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "{}", take_err_string(err));
    let pixels = unsafe { &*image }.to_rgba16().unwrap();
    unsafe { dr_precision_image_free(image) };
    pixels
}

#[test]
fn a_converted_document_saves_and_reopens_bit_exact() {
    let dir = TempDir::new().unwrap();
    let (w, h) = (5u32, 4u32);
    let pixels: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            [
                (i * 7919 % 65536) as u16,
                (i * 3001) as u16,
                (i * 911) as u16,
                65535,
            ]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ));
    let a = press_profile("Press A");
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_convert_to_cmyk(
            rgb,
            a.as_ptr(),
            a.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    let b = press_profile_b("Press B");
    // A plain converted document still writes word 82.
    let plain = profile_edit(doc, Some(&b), r#"{"op":"convert"}"#)
        .unwrap()
        .unwrap();
    assert_eq!(words(&save(plain, &dir.path().join("plain.rz")))[0], 82);
    // A mask and a Curves record: word 83, both carried through Convert.
    let mask: Vec<u16> = (0..w * h).map(|i| (i * 3203) as u16).collect();
    let request = CString::new(format!(
        r#"{{"op":"set_mask","layer":0,"width":{w},"height":{h}}}"#
    ))
    .unwrap();
    let masked = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            mask.as_ptr(),
            mask.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!masked.is_null(), "{}", take_err_string(err));
    let curves = CString::new(
        r#"{"type":"adjust","evaluation":"precision_v1","op":"curves","params":{"k":[[0,0],[90,150],[255,255]]}}"#,
    )
    .unwrap();
    let adjusted = unsafe {
        dr_precision_doc_add_adjustment(
            masked,
            c"Tone".as_ptr(),
            curves.as_ptr(),
            0.8,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!adjusted.is_null(), "{}", take_err_string(err));
    let converted = profile_edit(adjusted, Some(&b), r#"{"op":"convert"}"#)
        .unwrap()
        .unwrap();
    let bytes = save(converted, &dir.path().join("converted.rz"));
    assert_eq!(words(&bytes)[0], 83);
    let again = open(&dir.path().join("converted.rz"));
    let (x, y) = (unsafe { &*converted }, unsafe { &*again });
    assert_eq!(x.layer_count(), 2);
    assert_eq!(y.layer_count(), 2);
    assert_eq!(samples(converted, 0), samples(again, 0));
    assert_eq!(profile_bytes(again), b);
    assert_eq!(info(again)["depth"], 16);
    assert_eq!(
        x.layer(0).unwrap().mask().unwrap().plane().samples(),
        y.layer(0).unwrap().mask().unwrap().plane().samples()
    );
    assert_eq!(
        x.layer(0).unwrap().mask().unwrap().plane().samples(),
        &mask[..]
    );
    assert_eq!(x.adjustment_description(1), y.adjustment_description(1));
    assert!(x.adjustment_description(1).unwrap().contains("\"k\""));
    assert_eq!(render(converted), render(again));
    for d in [rgb, doc, plain, masked, adjusted, converted, again] {
        free(d);
    }
}

#[test]
fn every_profile_refusal_names_itself() {
    let dir = TempDir::new().unwrap();
    let doc = cmyk_doc(
        &dir,
        "r.tif",
        1,
        1,
        &[[1000, 2000, 3000, 4000]],
        &press_profile("Press A"),
    );
    let srgb = IccProfile::srgb().bytes().to_vec();
    assert!(profile_edit(doc, Some(&srgb), r#"{"op":"assign"}"#)
        .unwrap_err()
        .starts_with("cannot use the CMYK profile"));
    let no_b2a = press_profile_without_b2a("Proof Only");
    assert_eq!(
        profile_edit(doc, Some(&no_b2a), r#"{"op":"convert"}"#).unwrap_err(),
        "the CMYK profile \"Proof Only\" has no B2A (PCS-to-device) table, so nothing can be converted into it"
    );
    // …which Assign accepts: it converts nothing into the profile.
    let assigned = profile_edit(doc, Some(&no_b2a), r#"{"op":"assign"}"#)
        .unwrap()
        .unwrap();
    // The plan names the same refusal.
    let refused = plan(doc, Some(&no_b2a), r#"{"op":"convert"}"#);
    assert!(refused["refusal"].as_str().unwrap().contains("has no B2A"));
    assert_eq!(refused["flatten_required"], false);
    assert!(refused["working_memory_plan_bytes"].as_u64().unwrap() > 0);
    assert_eq!(
        profile_edit(doc, None, r#"{"op":"assign"}"#).unwrap_err(),
        "no working CMYK profile is set, so a CMYK profile must be named"
    );
    assert_eq!(
        profile_edit(doc, None, r#"{"op":"convert","dither":true}"#).unwrap_err(),
        "the CMYK profile request has an unknown key \"dither\""
    );
    assert_eq!(
        profile_edit(doc, None, r#"{"intent":"relative"}"#).unwrap_err(),
        "the CMYK profile request needs an \"op\""
    );
    assert_eq!(
        profile_edit(doc, None, r#"{"op":"depth"}"#).unwrap_err(),
        "a depth change needs \"depth\": 8 or 16"
    );
    assert_eq!(
        profile_edit(doc, None, r#"{"op":"depth","depth":12}"#).unwrap_err(),
        "the CMYK depth must be 8 or 16"
    );
    assert_eq!(
        profile_edit(doc, None, r#"{"op":"assign","flatten":true}"#).unwrap_err(),
        "\"flatten\" belongs to the convert op only"
    );
    // An RGB document.
    let image = PrecisionImage::from_rgba16(1, 1, vec![[0, 0, 0, 65535]], IccProfile::srgb(), None)
        .unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ));
    assert_eq!(
        profile_edit(rgb, Some(&press_profile_b("B")), r#"{"op":"convert"}"#).unwrap_err(),
        "Convert to Profile requires a CMYK document"
    );
    assert_eq!(
        profile_edit(rgb, Some(&press_profile_b("B")), r#"{"op":"assign"}"#).unwrap_err(),
        "Assign Profile requires a CMYK document"
    );
    assert_eq!(
        profile_edit(rgb, None, r#"{"op":"depth","depth":8}"#).unwrap_err(),
        "8 Bits/Channel requires a CMYK document"
    );
    for d in [doc, assigned, rgb] {
        free(d);
    }
}

/// Convert and 8/16 Bits reach a 4a group's members, not only root
/// records: the member lands on the second press's own tables (the oracle
/// above) and on the 8-bit grid, and the group is kept.
#[test]
fn convert_and_depth_reach_the_members_of_a_group() {
    let dir = TempDir::new().unwrap();
    let grid = ink_grid();
    let doc = cmyk_doc(
        &dir,
        "grouped.tif",
        grid.len() as u32,
        1,
        &grid,
        &press_profile("Press A"),
    );
    let request =
        CString::new(r#"{"op":"group_layers","layers":[0],"name":"G","pass_through":false}"#)
            .unwrap();
    let mut err = ptr::null_mut();
    let grouped = unsafe {
        dr_precision_doc_edit(
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
    assert!(!grouped.is_null(), "{}", take_err_string(err));
    let is_group =
        |d: *const PrecisionDocument| unsafe { &*d }.layer(1).unwrap().children().is_some();
    assert!(
        is_group(grouped),
        "the member is record 0, its group record 1"
    );
    let b = press_profile_b("Press B");
    let converted = profile_edit(
        grouped,
        Some(&b),
        r#"{"op":"convert","intent":"relative","black_point_compensation":true}"#,
    )
    .unwrap()
    .unwrap();
    assert!(is_group(converted), "the group is kept");
    for (ink, sample) in grid.iter().zip(samples(converted, 0)) {
        let expected = press_to_press_b(ink.map(|v| f64::from(v) / 65535.0), false, true);
        for c in 0..4 {
            assert!(
                sample[c].abs_diff(code(expected[c])) <= 1,
                "{ink:?}: member ink {c} {} vs oracle {}",
                sample[c],
                code(expected[c])
            );
        }
    }
    let on_grid = |v: u16| ((f64::from(v) / 257.0).round() * 257.0) as u16;
    let eight = profile_edit(grouped, None, r#"{"op":"depth","depth":8}"#)
        .unwrap()
        .unwrap();
    assert!(is_group(eight), "the group is kept");
    for (before, after) in samples(grouped, 0).iter().zip(samples(eight, 0)) {
        assert_eq!(after, before.map(on_grid), "every member sample");
    }
    for d in [doc, grouped, converted, eight] {
        free(d);
    }
}
