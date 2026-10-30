//! Generative Fill on an 8-bit CMYK document (M3b phase 2,
//! `cmyk_gen_fill`) through the FFI, with a STUB answer in place of the
//! cloud provider.
//!
//! # The oracles
//!
//! `dr_cmyk_artwork_colors` is the independent colour leg: a solid-ink
//! document's input cells are that ink as sRGB (ink → sRGB, the default
//! relative colorimetric with black point compensation), and a solid sRGB
//! answer lands as that colour's ink (sRGB → ink). The `.rz` word is read
//! from the file's own bytes.
//!
//! # Bounds, declared before the first run
//!
//! * Input cells: ±1 byte (an f32 conversion and mean, then one rounding).
//! * Applied ink: ±1 u16 code.
//! * Outside the covered rect: paper at α 0, exact.
//! * Generative Expand: the coverage and the composite inside the old rect,
//!   exact; the ink on the new area, ±1 u16 code.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_cmyk_artwork::dr_cmyk_artwork_colors;
use doctored_core::ffi_gen_expand::dr_precision_doc_gen_expand_coverage;
use doctored_core::ffi_gen_fill::*;
use doctored_core::ffi_gen_variations::*;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::Value;
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_structure_fixture::{
    cmyk_word, composite, free, json, open, save, then, with_levels, LIMIT,
};
use crate::common::take_err_string;

const W: u32 = 64;
const H: u32 = 48;
const NATIVE: (u32, u32) = (64, 64);
const RECT: (u32, u32, u32, u32) = (16, 12, 40, 36);
const STUB: [u8; 3] = [37, 140, 201];
const OTHER: [u8; 3] = [220, 60, 30];
const TAG: &str = r#"{"type":"generated","version":1,"feature":"generative_fill","provider":"fal","model":"fal-ai/flux-pro/v1/fill","prompt":"a boat"}"#;

/// A `W × H` CMYK document at `depth` bits converted from one uniform RGB
/// layer of `rgb` (16-bit codes) into the press profile.
fn cmyk(depth: u32, rgb: [u16; 3]) -> (*mut PrecisionDocument, Vec<u8>) {
    let srgb = IccProfile::srgb();
    let pixels = vec![[rgb[0], rgb[1], rgb[2], 65535]; (W * H) as usize];
    let image = PrecisionImage::from_rgba16(W, H, pixels, srgb.clone(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Base", image, (0, 0), 1.0, true).unwrap();
    let rgb_doc = Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            W,
            H,
            srgb,
            Metadata::default(),
            Resolution::default(),
            vec![layer],
        )
        .unwrap(),
    ));
    let profile = press_profile("Press Generative Fill");
    let options = CString::new(format!("{{\"depth\":{depth}}}")).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            rgb_doc,
            profile.as_ptr(),
            profile.len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to CMYK: {}", take_err_string(err));
    free(rgb_doc);
    (out, profile)
}

/// `dr_cmyk_artwork_colors` of one colour from `source` to `destination`
/// (empty = sRGB).
fn colour(source: &[u8], destination: &[u8], value: &[f64]) -> Vec<f64> {
    let request = CString::new(format!("{{\"values\":[{value:?}]}}")).unwrap();
    let end = |p: &[u8]| {
        if p.is_empty() {
            ptr::null()
        } else {
            p.as_ptr()
        }
    };
    let mut err = ptr::null_mut();
    let text = unsafe {
        dr_cmyk_artwork_colors(
            end(source),
            source.len(),
            end(destination),
            destination.len(),
            request.as_ptr(),
            &mut err,
        )
    };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let reply: Value = json(text, err);
    reply["values"][0]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect()
}

fn coverage() -> Vec<u16> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            if (RECT.0..RECT.2).contains(&x) && (RECT.1..RECT.3).contains(&y) {
                65535
            } else {
                0
            }
        })
        .collect()
}

fn solid(rgb: [u8; 3]) -> Vec<u8> {
    (0..NATIVE.0 * NATIVE.1)
        .flat_map(|_| [rgb[0], rgb[1], rgb[2], 255])
        .collect()
}

fn plan(doc: *const PrecisionDocument) -> Result<u64, String> {
    let mut err = ptr::null_mut();
    let bytes = unsafe { dr_precision_doc_gen_fill_plan(doc, NATIVE.0, NATIVE.1, 3, &mut err) };
    if bytes == 0 {
        Err(take_err_string(err))
    } else {
        Ok(bytes)
    }
}

fn input(doc: *const PrecisionDocument) -> Result<(Vec<u8>, Vec<u8>), String> {
    input_at(doc, 0)
}

fn input_at(doc: *const PrecisionDocument, layer: usize) -> Result<(Vec<u8>, Vec<u8>), String> {
    let bytes = (4 * NATIVE.0 * NATIVE.1) as usize;
    let cover = coverage();
    let mut crop = [0u32; 4];
    let mut image = vec![7u8; bytes];
    let mut mask = vec![7u8; bytes];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_gen_fill_input(
            doc,
            layer,
            cover.as_ptr(),
            cover.len(),
            NATIVE.0,
            NATIVE.1,
            crop.as_mut_ptr(),
            image.as_mut_ptr(),
            image.len(),
            mask.as_mut_ptr(),
            mask.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if ok {
        Ok((image, mask))
    } else {
        Err(take_err_string(err))
    }
}

fn apply(
    doc: *const PrecisionDocument,
    answers: &[[u8; 3]],
) -> Result<(*mut PrecisionDocument, usize), String> {
    apply_over(doc, &coverage(), answers)
}

fn apply_over(
    doc: *const PrecisionDocument,
    cover: &[u16],
    answers: &[[u8; 3]],
) -> Result<(*mut PrecisionDocument, usize), String> {
    let mut bytes = Vec::new();
    let mut sizes = Vec::new();
    for rgb in answers {
        bytes.extend(solid(*rgb));
        sizes.extend([NATIVE.0, NATIVE.1]);
    }
    let name = CString::new("a boat").unwrap();
    let tag = CString::new(TAG).unwrap();
    let mut index = usize::MAX;
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_gen_fill_apply(
            doc,
            0,
            cover.as_ptr(),
            cover.len(),
            NATIVE.0,
            NATIVE.1,
            bytes.as_ptr(),
            bytes.len(),
            sizes.as_ptr(),
            sizes.len(),
            name.as_ptr(),
            tag.as_ptr(),
            &mut index,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok((out, index))
    }
}

fn info(doc: *const PrecisionDocument, layer: usize) -> Option<[u32; 8]> {
    let mut out = [0u32; 8];
    let mut err = ptr::null_mut();
    let found =
        unsafe { dr_precision_doc_gen_fill_variations(doc, layer, out.as_mut_ptr(), &mut err) };
    assert!(err.is_null(), "{}", take_err_string(err));
    found.then_some(out)
}

/// Every covered sample of record `index` is `ink` (±1 code) and opaque;
/// every other one paper at α 0.
fn assert_ink(doc: *const PrecisionDocument, index: usize, ink: &[f64], what: &str) {
    let samples = unsafe { &*doc }.cmyk_layer_samples(index).unwrap();
    for (i, s) in samples.iter().enumerate() {
        let (x, y) = (i as u32 % W, i as u32 / W);
        if (RECT.0..RECT.2).contains(&x) && (RECT.1..RECT.3).contains(&y) {
            for c in 0..4 {
                let want = (ink[c] * 65535.0).round();
                assert!(
                    (f64::from(s[c]) - want).abs() <= 1.0,
                    "{what}: ink {c} {} vs {want}",
                    s[c]
                );
            }
            assert_eq!(s[4], 65535, "{what}: opaque");
        } else {
            assert_eq!(*s, [0, 0, 0, 0, 0], "{what}: paper outside");
        }
    }
}

#[test]
fn the_input_is_the_plate_as_srgb() {
    let (doc, profile) = cmyk(8, [30000, 42000, 18000]);
    assert!(plan(doc).is_ok());
    let ink = unsafe { &*doc }.cmyk_layer_samples(0).unwrap()[0];
    let unit: Vec<f64> = ink[..4].iter().map(|&v| f64::from(v) / 65535.0).collect();
    let want = colour(&profile, &[], &unit);
    let (image, mask) = input(doc).unwrap();
    for (cell, m) in image.chunks(4).zip(mask.chunks(4)) {
        for c in 0..3 {
            let target = (want[c] * 255.0).round();
            assert!(
                (f64::from(cell[c]) - target).abs() <= 1.0,
                "cell {c}: {} vs {target}",
                cell[c]
            );
        }
        assert!(m == [255, 255, 255, 0] || m == [0, 0, 0, 255]);
        let want_alpha = if m == [255, 255, 255, 0] { 0 } else { 255 };
        assert_eq!(cell[3], want_alpha);
    }
    free(doc);
}

#[test]
fn a_solid_answer_lands_as_its_ink_and_select_works() {
    let (doc, profile) = cmyk(8, [30000, 42000, 18000]);
    let (out, index) = apply(doc, &[STUB, OTHER]).unwrap();
    let srgb = |rgb: [u8; 3]| rgb.map(|v| f64::from(v) / 255.0).to_vec();
    assert_ink(out, index, &colour(&[], &profile, &srgb(STUB)), "applied");
    let found = info(out, index).expect("variations");
    assert_eq!((found[0], found[1]), (2, 0));
    let mut err = ptr::null_mut();
    let picked = unsafe {
        dr_precision_doc_gen_fill_select_variation(out, index, 1, LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!picked.is_null(), "{}", take_err_string(err));
    assert_ink(
        picked,
        index,
        &colour(&[], &profile, &srgb(OTHER)),
        "selected",
    );
    assert_eq!(info(picked, index).unwrap()[1], 1);
    // Saved, the document keeps its ordinary rung (87: the masked layer's
    // tag is a description) and claims the variations with word 92 right
    // after it, as the RGB format does; reopened, the variations are live;
    // stripped of 92, the file refuses (GVAR meets the next table's tag).
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("cmyk.rz");
    let bytes = save(picked, &path);
    assert_eq!(cmyk_word(&bytes), 87);
    assert_eq!(
        &bytes[12..16],
        &92u32.to_le_bytes(),
        "word 92 after the rung"
    );
    let reopened = open(&path).unwrap();
    assert_eq!(info(reopened, index), info(picked, index));
    let mut older = bytes.clone();
    older.drain(12..16);
    let older_path = dir.path().join("older.rz");
    std::fs::write(&older_path, &older).unwrap();
    assert!(open(&older_path).is_err());
    for d in [doc, out, picked, reopened] {
        free(d);
    }
}

#[test]
fn sixteen_bit_cmyk_refuses_by_name() {
    let (doc, _) = cmyk(16, [30000, 42000, 18000]);
    let refusal = "generative fill is not available in a 16-bit CMYK document";
    assert_eq!(plan(doc).unwrap_err(), refusal);
    assert_eq!(input(doc).unwrap_err(), refusal);
    assert_eq!(apply(doc, &[STUB]).unwrap_err(), refusal);
    free(doc);
}

/// Canvas Size's growth in the Expand tests: the picture at (8, 6) on a
/// `(W + 16) × (H + 12)` canvas.
const GROWN: (u32, u32) = (W + 16, H + 12);
const AT: (u32, u32) = (8, 6);

fn grow(doc: *mut PrecisionDocument) -> *mut PrecisionDocument {
    let request = format!(
        "{{\"op\":\"canvas_size\",\"width\":{},\"height\":{},\"x\":{},\"y\":{}}}",
        GROWN.0, GROWN.1, AT.0, AT.1
    );
    then(doc, &request, &[])
}

fn expand(doc: *const PrecisionDocument, layer: usize, count: usize) -> Result<Vec<u16>, String> {
    let mut out = vec![7u16; count];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_gen_expand_coverage(
            doc,
            layer,
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

fn inside_old(x: u32, y: u32) -> bool {
    (AT.0..AT.0 + W).contains(&x) && (AT.1..AT.1 + H).contains(&y)
}

#[test]
fn expand_fills_the_new_area_with_ink_and_keeps_the_picture() {
    let (doc, profile) = cmyk(8, [30000, 42000, 18000]);
    let doc = grow(doc);
    let n = (GROWN.0 * GROWN.1) as usize;
    let cover = expand(doc, 0, n).unwrap();
    for (i, c) in cover.iter().enumerate() {
        let (x, y) = (i as u32 % GROWN.0, i as u32 / GROWN.0);
        let want = if inside_old(x, y) { 0 } else { 65535 };
        assert_eq!(*c, want, "coverage at ({x}, {y})");
    }
    let (out, index) = apply_over(doc, &cover, &[STUB, OTHER]).unwrap();
    assert_eq!(index, 1);
    let ink = colour(&[], &profile, &STUB.map(|v| f64::from(v) / 255.0));
    for y in (0..GROWN.1).step_by(3) {
        for x in (0..GROWN.0).step_by(3) {
            let (xi, yi) = (x as i32, y as i32);
            let got = composite(out, xi, yi);
            if inside_old(x, y) {
                assert_eq!(got, composite(doc, xi, yi), "inside at ({x}, {y})");
                continue;
            }
            for c in 0..4 {
                let want = (ink[c] * 65535.0).round();
                assert!(
                    (f64::from(got[c]) - want).abs() <= 1.0,
                    "new area ({x}, {y}) ink {c}: {} vs {want}",
                    got[c]
                );
            }
            assert_eq!(got[4], 65535, "new area ({x}, {y}) opaque");
        }
    }
    free(doc);
    free(out);
}

#[test]
fn expand_refuses_sixteen_bit_cmyk_by_name() {
    let (doc, _) = cmyk(16, [30000, 42000, 18000]);
    let doc = grow(doc);
    assert_eq!(
        expand(doc, 0, (GROWN.0 * GROWN.1) as usize).unwrap_err(),
        "generative expand is not available in a 16-bit CMYK document"
    );
    free(doc);
}

/// An adjustment record is an anchor like any other on 8-bit CMYK: the
/// plate is the full composite, so Fill's input and Expand's coverage do not
/// ask for a raster source.
#[test]
fn fill_and_expand_take_an_adjustment_anchor() {
    let (doc, _) = cmyk(8, [30000, 42000, 18000]);
    let doc = with_levels(doc, 1.0);
    assert_eq!(input_at(doc, 1).unwrap(), input_at(doc, 0).unwrap());
    let doc = grow(doc);
    let n = (GROWN.0 * GROWN.1) as usize;
    assert_eq!(expand(doc, 1, n).unwrap(), expand(doc, 0, n).unwrap());
    free(doc);
}

/// Words 93, 92, 91 and 90 after an 8-bit CMYK file's rung: a Generative
/// Layer with live variations (its `GVAR` entry `ink` 1), a different layer
/// moved since it was made and a third holding an angled Pattern Overlay,
/// all in one document. The words follow the rung in descending order, the
/// tables run GVAR, LREF, CMYK, LIDS, the file reopens with all three facts
/// and re-saves to the same bytes, and dropping 92, or exchanging 92 and 91,
/// refuses instead of misparsing.
#[test]
fn a_cmyk_file_carries_variations_a_moved_layer_and_an_angled_overlay() {
    use crate::cmyk_structure_fixture::then;
    use crate::cmyk_style_fixture::{hard_rect, layer, styled};
    let word = |bytes: &[u8], i: usize| {
        u32::from_le_bytes(bytes[4 + i * 4..8 + i * 4].try_into().unwrap())
    };
    let (doc, _) = cmyk(8, [30000, 42000, 18000]);
    let doc = layer(
        doc,
        "Moved",
        [10.0, 60.0, 30.0, 0.0],
        &hard_rect(W, H, (4, 4, 20, 16)),
    );
    let doc = layer(
        doc,
        "Angled",
        [70.0, 10.0, 0.0, 5.0],
        &hard_rect(W, H, (30, 20, 60, 44)),
    );
    let (filled, generated) = apply(doc, &[STUB, OTHER, STUB]).unwrap();
    free(doc);
    assert_eq!(generated, 1, "the fill lands above layer 0");
    let (shifted, angled) = (2, 3);
    let doc = then(
        filled,
        &format!(
            r#"{{"op":"affine_set","layers":[{shifted}],"matrix":[1,0,0,1,5,-2],"filter":"nearest"}}"#
        ),
        &[],
    );
    let doc = styled(
        doc,
        angled,
        &serde_json::json!({"effects":[{"type":"pattern_overlay",
            "pattern":{"preset":"dots"},"angle":-60,"scale":0.5}]}),
    );
    let live = info(doc, generated).expect("the other layers' edits keep it live");
    let d = unsafe { &*doc };
    assert_eq!(d.layer(shifted).unwrap().reference(), (5, -2));

    let dir = TempDir::new().unwrap();
    let bytes = save(doc, &dir.path().join("all.rz"));
    let rung = cmyk_word(&bytes);
    assert!((82..=88).contains(&rung), "the CMYK rung, got {rung}");
    let words: Vec<u32> = (1..6).map(|i| word(&bytes, i)).collect();
    assert_eq!(words, [rung, 93, 92, 91, 90], "the words after 46");
    let at = |tag: &[u8]| bytes.windows(4).rposition(|w| w == tag).unwrap();
    let (gvar, lref, table, lids) = (at(b"GVAR"), at(b"LREF"), at(b"CMYK"), at(b"LIDS"));
    assert!(
        gvar < lref && lref < table && table < lids,
        "GVAR, LREF, CMYK, LIDS"
    );

    let path = dir.path().join("back.rz");
    std::fs::write(&path, &bytes).unwrap();
    let reopened = open(&path).unwrap();
    let r = unsafe { &*reopened };
    assert_eq!(info(reopened, generated), Some(live), "variations");
    assert_eq!(r.layer(shifted).unwrap().reference(), (5, -2), "reference");
    assert_eq!(r.layer(generated).unwrap().reference(), (0, 0));
    let style = |d: &PrecisionDocument| -> Value {
        serde_json::from_str(&d.layer(angled).unwrap().reported_style().unwrap()).unwrap()
    };
    assert_eq!(style(r), style(d), "the style");
    assert_eq!(style(r)["effects"][0]["angle"], -60.0, "the angle");
    for (x, y) in [
        (0, 0),
        (8, 6),
        (20, 18),
        (40, 30),
        (W as i32 - 1, H as i32 - 1),
    ] {
        assert_eq!(
            composite(reopened, x, y),
            composite(doc, x, y),
            "({x}, {y})"
        );
    }
    assert_eq!(
        save(reopened, &dir.path().join("again.rz")),
        bytes,
        "resave"
    );
    let picked = unsafe {
        let mut err = ptr::null_mut();
        let out = dr_precision_doc_gen_fill_select_variation(
            reopened,
            generated,
            1,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        );
        assert!(!out.is_null(), "{}", take_err_string(err));
        out
    };
    assert_eq!(
        info(picked, generated).unwrap()[1],
        1,
        "select after the reopen"
    );

    // Word 92 dropped: the LREF reader meets GVAR's bytes.
    let mut dropped = bytes.clone();
    dropped.drain(16..20);
    assert_eq!(word(&dropped, 3), 91);
    let cut = dir.path().join("dropped.rz");
    std::fs::write(&cut, &dropped).unwrap();
    let refusal = open(&cut).expect_err("a file without word 92 must refuse");
    assert_eq!(refusal, "unsupported native layer reference schema");
    // 92 and 91 exchanged: the chain is read in one order only.
    let mut swapped = bytes.clone();
    swapped[16..20].copy_from_slice(&91u32.to_le_bytes());
    swapped[20..24].copy_from_slice(&92u32.to_le_bytes());
    let cut = dir.path().join("swapped.rz");
    std::fs::write(&cut, &swapped).unwrap();
    let refusal = open(&cut).expect_err("92 after 91 must refuse");
    // 91 is taken as the reference word, so 92 lands where the body version
    // belongs and is refused there.
    assert!(refusal.ends_with("high-depth DRDC document"), "{refusal}");
    for p in [doc, reopened, picked] {
        free(p);
    }
}
