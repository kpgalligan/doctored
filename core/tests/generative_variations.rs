//! Generative Layer variations (M3b phase 2) through the FFI:
//! `dr_*_gen_fill_apply` with several results, `dr_*_gen_fill_variations`,
//! `_select_variation`, `_more_input` and `_add_variations`, their `.rz`
//! word 92 and `GVAR` table, and the live rule's detach cases, at 8 and 16
//! bits, with STUB answers in place of the cloud provider.
//!
//! # The oracles
//!
//! Written from the header's contract, independent of the implementation: a
//! solid answer lands as its own colour on the covered pixels and nothing
//! outside the selection moves (the composite there is bit-exact against
//! the pre-fill composite); a 2 × 2 answer is the bilinear interpolation
//! between its four pixel centres, clamped at the edges (phase 1's read-back
//! oracle); selecting back to variation 0 restores exactly the pixels the
//! apply rendered; more-input's two pictures are phase 1's input on the
//! pre-fill document with the same coverage; a detached layer answers no
//! variations, and its file carries no word 92.
//!
//! # Bounds, declared before the first run
//!
//! * A solid answer and every pixel outside the selection: exact at 8 bits;
//!   the solid answer ±1 code at 16 bits.
//! * The bilinear read-back: ±1 code.
//! * Re-selection after a reopen, more-input and the round trips: exact.
#![allow(clippy::needless_range_loop)]

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_color::dr_doc_assign_profile;
use doctored_core::ffi_doc::{
    dr_doc_native_reopen_plan, dr_doc_open, dr_doc_painting_layer, dr_doc_painting_layer_mask,
    dr_doc_removing_layer_mask, dr_doc_save_native, dr_doc_with_layer_offset,
};
use doctored_core::ffi_gen_fill::*;
use doctored_core::ffi_gen_variations::*;
use doctored_core::ffi_precision::{dr_precision_doc_open, dr_precision_doc_save_native};
use doctored_core::ffi_precision_color::dr_precision_doc_convert_to_profile;
use doctored_core::ffi_precision_conversion::{
    dr_doc_promote_structure, dr_precision_doc_narrow_structure_to_u8,
};
use doctored_core::gen_variations::check_caps;
use doctored_core::icc::IccProfile;
use serde_json::json;
use tempfile::TempDir;

use crate::common::{cpath, flat_pixels, layer_bytes, take_err_string};
use crate::identity::drdc_word;
use crate::sky_replacement::{build, Depth, Doc, LIMIT};

const DEPTHS: [Depth; 2] = [Depth::Byte, Depth::Sixteen];
const W: u32 = 64;
const H: u32 = 48;
pub(crate) const NATIVE: (u32, u32) = (64, 64);
const RECT: (u32, u32, u32, u32) = (16, 12, 40, 36);
const TAG: &str = r#"{"type":"generated","version":1,"feature":"generative_fill","provider":"gemini","model":"gemini-nano-banana-2.1","prompt":"a red boat"}"#;
const TAG2: &str = r#"{"type":"generated","version":1,"feature":"generative_fill","provider":"fal","model":"fal-ai/flux-pro/v1/fill","prompt":"a blue boat"}"#;
pub(crate) const COLORS: [[u8; 3]; 4] =
    [[200, 30, 40], [20, 180, 60], [30, 60, 220], [250, 250, 10]];
const WORD: u32 = 92;

pub(crate) type Picture = (Vec<u8>, (u32, u32));

// ---------------------------------------------------------------- calls --

pub(crate) fn solid(size: (u32, u32), rgb: [u8; 3]) -> Picture {
    let pixels = (0..size.0 * size.1)
        .flat_map(|_| [rgb[0], rgb[1], rgb[2], 255])
        .collect();
    (pixels, size)
}

fn flatten(results: &[Picture]) -> (Vec<u8>, Vec<u32>) {
    let mut bytes = Vec::new();
    let mut sizes = Vec::new();
    for (pixels, size) in results {
        bytes.extend_from_slice(pixels);
        sizes.extend([size.0, size.1]);
    }
    (bytes, sizes)
}

fn rect_coverage(w: u32, h: u32, (x0, y0, x1, y1): (u32, u32, u32, u32)) -> Vec<u16> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if (x0..x1).contains(&x) && (y0..y1).contains(&y) {
                65535
            } else {
                0
            }
        })
        .collect()
}

fn gradient(w: u32, h: u32) -> Vec<[f64; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (f64::from(i % w), f64::from(i / w));
            [
                (x * 4.0).round() / 255.0,
                (y * 5.0).round() / 255.0,
                ((x + y) % 13.0 * 19.0).round() / 255.0,
                1.0,
            ]
        })
        .collect()
}

fn base(depth: Depth) -> Doc {
    build(depth, W, H, &[gradient(W, H)])
}

pub(crate) fn apply(
    doc: &Doc,
    coverage: &[u16],
    results: &[Picture],
    tag: &str,
) -> Result<(Doc, usize), String> {
    let (bytes, sizes) = flatten(results);
    let name = CString::new("a red boat").unwrap();
    let tag = CString::new(tag).unwrap();
    let mut index = usize::MAX;
    let mut err = ptr::null_mut();
    let out = unsafe {
        match doc {
            Doc::Byte(d) => {
                let out = dr_doc_gen_fill_apply(
                    d,
                    0,
                    coverage.as_ptr(),
                    coverage.len(),
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
                );
                (!out.is_null()).then(|| Doc::Byte(*Box::from_raw(out)))
            }
            Doc::Typed(d) => {
                let out = dr_precision_doc_gen_fill_apply(
                    d,
                    0,
                    coverage.as_ptr(),
                    coverage.len(),
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
                );
                (!out.is_null()).then(|| Doc::Typed(*Box::from_raw(out)))
            }
        }
    };
    out.map(|doc| (doc, index))
        .ok_or_else(|| take_err_string(err))
}

pub(crate) fn info(doc: &Doc, layer: usize) -> Result<Option<[u32; 8]>, String> {
    let mut out = [0u32; 8];
    let mut err = ptr::null_mut();
    let found = unsafe {
        match doc {
            Doc::Byte(d) => dr_doc_gen_fill_variations(d, layer, out.as_mut_ptr(), &mut err),
            Doc::Typed(d) => {
                dr_precision_doc_gen_fill_variations(d, layer, out.as_mut_ptr(), &mut err)
            }
        }
    };
    if found {
        Ok(Some(out))
    } else if err.is_null() {
        Ok(None)
    } else {
        Err(take_err_string(err))
    }
}

pub(crate) fn select(doc: &Doc, layer: usize, index: u32) -> Result<Option<Doc>, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        match doc {
            Doc::Byte(d) => {
                let out = dr_doc_gen_fill_select_variation(
                    d,
                    layer,
                    index,
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                );
                (!out.is_null()).then(|| Doc::Byte(*Box::from_raw(out)))
            }
            Doc::Typed(d) => {
                let out = dr_precision_doc_gen_fill_select_variation(
                    d,
                    layer,
                    index,
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                );
                (!out.is_null()).then(|| Doc::Typed(*Box::from_raw(out)))
            }
        }
    };
    match out {
        Some(doc) => Ok(Some(doc)),
        None if err.is_null() => Ok(None),
        None => Err(take_err_string(err)),
    }
}

fn more(doc: &Doc, layer: usize) -> Result<(Vec<u8>, Vec<u8>), String> {
    more_at(doc, layer, LIMIT)
}

fn more_at(doc: &Doc, layer: usize, limit: u64) -> Result<(Vec<u8>, Vec<u8>), String> {
    let bytes = (4 * NATIVE.0 * NATIVE.1) as usize;
    let mut image = vec![7u8; bytes];
    let mut mask = vec![7u8; bytes];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        match doc {
            Doc::Byte(d) => dr_doc_gen_fill_more_input(
                d,
                layer,
                image.as_mut_ptr(),
                image.len(),
                mask.as_mut_ptr(),
                mask.len(),
                limit,
                0,
                ptr::null(),
                &mut err,
            ),
            Doc::Typed(d) => dr_precision_doc_gen_fill_more_input(
                d,
                layer,
                image.as_mut_ptr(),
                image.len(),
                mask.as_mut_ptr(),
                mask.len(),
                limit,
                0,
                ptr::null(),
                &mut err,
            ),
        }
    };
    if ok {
        Ok((image, mask))
    } else {
        Err(take_err_string(err))
    }
}

fn input(doc: &Doc, coverage: &[u16]) -> (Vec<u8>, Vec<u8>) {
    input_at(doc, coverage, LIMIT).unwrap()
}

fn input_at(doc: &Doc, coverage: &[u16], limit: u64) -> Result<(Vec<u8>, Vec<u8>), String> {
    let bytes = (4 * NATIVE.0 * NATIVE.1) as usize;
    let mut crop = [0u32; 4];
    let mut image = vec![7u8; bytes];
    let mut mask = vec![7u8; bytes];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        match doc {
            Doc::Byte(d) => dr_doc_gen_fill_input(
                d,
                0,
                coverage.as_ptr(),
                coverage.len(),
                NATIVE.0,
                NATIVE.1,
                crop.as_mut_ptr(),
                image.as_mut_ptr(),
                image.len(),
                mask.as_mut_ptr(),
                mask.len(),
                limit,
                0,
                ptr::null(),
                &mut err,
            ),
            Doc::Typed(d) => dr_precision_doc_gen_fill_input(
                d,
                0,
                coverage.as_ptr(),
                coverage.len(),
                NATIVE.0,
                NATIVE.1,
                crop.as_mut_ptr(),
                image.as_mut_ptr(),
                image.len(),
                mask.as_mut_ptr(),
                mask.len(),
                limit,
                0,
                ptr::null(),
                &mut err,
            ),
        }
    };
    if ok {
        Ok((image, mask))
    } else {
        Err(take_err_string(err))
    }
}

fn add(doc: &Doc, layer: usize, results: &[Picture], tag: &str) -> Result<Option<Doc>, String> {
    let (bytes, sizes) = flatten(results);
    let tag = CString::new(tag).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        match doc {
            Doc::Byte(d) => {
                let out = dr_doc_gen_fill_add_variations(
                    d,
                    layer,
                    bytes.as_ptr(),
                    bytes.len(),
                    sizes.as_ptr(),
                    sizes.len(),
                    tag.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                );
                (!out.is_null()).then(|| Doc::Byte(*Box::from_raw(out)))
            }
            Doc::Typed(d) => {
                let out = dr_precision_doc_gen_fill_add_variations(
                    d,
                    layer,
                    bytes.as_ptr(),
                    bytes.len(),
                    sizes.as_ptr(),
                    sizes.len(),
                    tag.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                );
                (!out.is_null()).then(|| Doc::Typed(*Box::from_raw(out)))
            }
        }
    };
    match out {
        Some(doc) => Ok(Some(doc)),
        None if err.is_null() => Ok(None),
        None => Err(take_err_string(err)),
    }
}

fn save(doc: &Doc, dir: &TempDir, name: &str) -> Vec<u8> {
    let path = dir.path().join(name);
    let c = cpath(&path);
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        match doc {
            Doc::Byte(d) => dr_doc_save_native(d, c.as_ptr(), &mut err),
            Doc::Typed(d) => {
                dr_precision_doc_save_native(d, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
            }
        }
    };
    assert!(ok, "save: {}", take_err_string(err));
    std::fs::read(&path).unwrap()
}

fn open(depth: Depth, bytes: &[u8], dir: &TempDir, name: &str) -> Result<Doc, String> {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    let c = cpath(&path);
    let mut err: *mut c_char = ptr::null_mut();
    unsafe {
        match depth {
            Depth::Byte => {
                let out = dr_doc_open(c.as_ptr(), &mut err);
                if out.is_null() {
                    Err(take_err_string(err))
                } else {
                    Ok(Doc::Byte(*Box::from_raw(out)))
                }
            }
            Depth::Sixteen => {
                let out = dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err);
                if out.is_null() {
                    Err(take_err_string(err))
                } else {
                    Ok(Doc::Typed(*Box::from_raw(out)))
                }
            }
        }
    }
}

fn insert_word(bytes: &[u8], at: usize, word: u32) -> Vec<u8> {
    let cut = 4 + at * 4;
    let mut out = bytes[..cut].to_vec();
    out.extend_from_slice(&word.to_le_bytes());
    out.extend_from_slice(&bytes[cut..]);
    out
}

fn remove_word(bytes: &[u8], at: usize) -> Vec<u8> {
    let cut = 4 + at * 4;
    let mut out = bytes[..cut].to_vec();
    out.extend_from_slice(&bytes[cut + 4..]);
    out
}

// -------------------------------------------------------------- helpers --

/// Layer `idx`'s own samples as unit floats.
fn unit_layer(doc: &Doc, idx: usize) -> Vec<[f64; 4]> {
    match doc {
        Doc::Byte(d) => layer_bytes(d, idx)
            .chunks(4)
            .map(|p| [0, 1, 2, 3].map(|c| f64::from(p[c]) / 255.0))
            .collect(),
        Doc::Typed(d) => d
            .layer(idx)
            .unwrap()
            .source()
            .unwrap()
            .to_rgba16()
            .unwrap()
            .iter()
            .map(|p| p.map(|v| f64::from(v) / 65535.0))
            .collect(),
    }
}

/// The projection as unit samples.
fn projection(doc: &Doc) -> Vec<f64> {
    match doc {
        Doc::Byte(d) => flat_pixels(d as *const DrDocument)
            .iter()
            .map(|&v| f64::from(v) / 255.0)
            .collect(),
        Doc::Typed(d) => d
            .render(LIMIT, 0, None)
            .unwrap()
            .to_rgba16()
            .unwrap()
            .iter()
            .flat_map(|p| p.map(|v| f64::from(v) / 65535.0))
            .collect(),
    }
}

fn mask_plane(doc: &Doc, idx: usize) -> Vec<u16> {
    match doc {
        Doc::Byte(d) => d.layers[idx]
            .mask
            .as_ref()
            .unwrap()
            .as_raw()
            .iter()
            .map(|&v| u16::from(v) * 257)
            .collect(),
        Doc::Typed(d) => d
            .layer(idx)
            .unwrap()
            .mask()
            .unwrap()
            .plane()
            .samples()
            .to_vec(),
    }
}

fn tag_of(doc: &Doc, idx: usize) -> Option<String> {
    match doc {
        Doc::Byte(d) => d.layers[idx].meta.clone(),
        Doc::Typed(d) => d.artwork_description(idx).map(str::to_string),
    }
}

fn assert_codes(depth: Depth, got: f64, want: f64, what: &str) {
    let diff = (got - want).abs() * depth.max();
    assert!(
        diff <= 1.0 + 1e-6,
        "{what} at {depth:?}: got {got}, want {want} ({diff} codes)"
    );
}

/// The solid colour `rgb` on every covered pixel of layer `idx`.
fn assert_solid(doc: &Doc, depth: Depth, idx: usize, rgb: [u8; 3], what: &str) {
    let pixels = unit_layer(doc, idx);
    let (x0, y0, x1, y1) = RECT;
    for y in y0..y1 {
        for x in x0..x1 {
            let p = pixels[(y * W + x) as usize];
            for c in 0..3 {
                assert_codes(depth, p[c], f64::from(rgb[c]) / 255.0, what);
            }
            assert_eq!(p[3], 1.0, "{what}: opaque");
        }
    }
}

/// A three-variation layer over the gradient at `depth`, and its index.
fn filled(depth: Depth) -> (Doc, usize) {
    let coverage = rect_coverage(W, H, RECT);
    let results: Vec<Picture> = COLORS[..3].iter().map(|&c| solid(NATIVE, c)).collect();
    apply(&base(depth), &coverage, &results, TAG).unwrap()
}

fn no_variations(index: usize) -> String {
    format!("generative layer {index} holds no variations")
}

/// The detach contract: no variations, select refuses by name, and the file
/// carries no word 92.
fn assert_detached(doc: &Doc, index: usize, dir: &TempDir, what: &str) {
    assert_eq!(info(doc, index).unwrap(), None, "{what}: info");
    assert_eq!(
        select(doc, index, 1).err().as_deref(),
        Some(no_variations(index).as_str()),
        "{what}: select"
    );
    let bytes = save(doc, dir, &format!("{}.rz", what.replace(' ', "_")));
    assert_ne!(drdc_word(&bytes, 1), WORD, "{what}: word");
    assert!(!bytes.windows(4).any(|w| w == b"GVAR"), "{what}: table");
}

// ---------------------------------------------------------------- tests --

#[test]
fn apply_keeps_every_result_and_renders_the_first() {
    for depth in DEPTHS {
        let doc = base(depth);
        let before = projection(&doc);
        let (out, index) = filled(depth);
        let found = info(&out, index).unwrap().expect("variations");
        assert_eq!(found[0], 3, "count");
        assert_eq!(found[1], 0, "selected");
        assert_eq!((found[2], found[3]), NATIVE, "native");
        assert_solid(&out, depth, index, COLORS[0], "result 0");
        let after = projection(&out);
        let coverage = rect_coverage(W, H, RECT);
        for (i, c) in coverage.iter().enumerate() {
            if *c == 0 {
                assert_eq!(after[4 * i..4 * i + 4], before[4 * i..4 * i + 4], "outside");
            } else {
                for k in 0..3 {
                    assert_codes(
                        depth,
                        after[4 * i + k],
                        f64::from(COLORS[0][k]) / 255.0,
                        "covered composite",
                    );
                }
            }
        }
        // The anchor itself holds no variations.
        assert_eq!(info(&out, 0).unwrap(), None);
    }
}

#[test]
fn select_rerenders_from_each_variation_and_keeps_the_mask() {
    let corners: [[u8; 3]; 4] = [[0, 0, 0], [255, 0, 40], [0, 255, 80], [255, 255, 120]];
    let two: Vec<u8> = corners
        .iter()
        .flat_map(|c| [c[0], c[1], c[2], 255])
        .collect();
    for depth in DEPTHS {
        let coverage = rect_coverage(W, H, RECT);
        let results = vec![
            solid(NATIVE, COLORS[0]),
            (two.clone(), (2, 2)),
            solid(NATIVE, COLORS[2]),
        ];
        let (doc, index) = apply(&base(depth), &coverage, &results, TAG).unwrap();
        let crop = info(&doc, index).unwrap().unwrap();
        let applied = unit_layer(&doc, index);
        let mask = mask_plane(&doc, index);
        let one = select(&doc, index, 1).unwrap().expect("a change");
        assert_eq!(info(&one, index).unwrap().unwrap()[1], 1);
        let pixels = unit_layer(&one, index);
        let [cx, cy, cw, ch] = [crop[4], crop[5], crop[6], crop[7]].map(f64::from);
        let (x0, y0, x1, y1) = RECT;
        for y in 0..H {
            for x in 0..W {
                let p = pixels[(y * W + x) as usize];
                if !((x0..x1).contains(&x) && (y0..y1).contains(&y)) {
                    assert_eq!(p[3], 0.0, "transparent outside the rect");
                    continue;
                }
                let qx = (f64::from(x) + 0.5 - cx) * 2.0 / cw;
                let qy = (f64::from(y) + 0.5 - cy) * 2.0 / ch;
                let (fx, fy) = ((qx - 0.5).clamp(0.0, 1.0), (qy - 0.5).clamp(0.0, 1.0));
                for c in 0..3 {
                    let v = |k: usize| f64::from(corners[k][c]) / 255.0;
                    let want = (1.0 - fx) * (1.0 - fy) * v(0)
                        + fx * (1.0 - fy) * v(1)
                        + (1.0 - fx) * fy * v(2)
                        + fx * fy * v(3);
                    assert_codes(depth, p[c], want, "bilinear variation");
                }
                assert_eq!(p[3], 1.0);
            }
        }
        assert_eq!(mask_plane(&one, index), mask, "the mask is kept");
        let two_doc = select(&one, index, 2).unwrap().expect("a change");
        assert_solid(&two_doc, depth, index, COLORS[2], "variation 2");
        let zero = select(&two_doc, index, 0).unwrap().expect("a change");
        assert_eq!(
            unit_layer(&zero, index),
            applied,
            "back to the applied pixels"
        );
        assert_eq!(mask_plane(&zero, index), mask);
        // Re-selecting the current variation changes nothing.
        assert!(select(&zero, index, 0).unwrap().is_none());
        // Refusals by name.
        assert_eq!(
            select(&zero, index, 3).err().as_deref(),
            Some("generative variation 3 is out of range (the layer holds 3)")
        );
        assert_eq!(
            select(&zero, 0, 1).err().as_deref(),
            Some(no_variations(0).as_str())
        );
        assert_eq!(
            select(&zero, 9, 1).err().as_deref(),
            Some("generative fill layer 9 is out of range")
        );
        let locked = match &zero {
            Doc::Byte(d) => Doc::Byte(d.with_layer_locks(index, 2).unwrap()),
            Doc::Typed(d) => Doc::Typed(d.with_layer_locks(index, 2).unwrap().unwrap()),
        };
        let refusal = select(&locked, index, 1).err().unwrap();
        assert!(refusal.contains("locked"), "{refusal}");
        // A transparency lock keeps the layer live through a select.
        let frozen = match &zero {
            Doc::Byte(d) => Doc::Byte(d.with_layer_locks(index, 1).unwrap()),
            Doc::Typed(d) => Doc::Typed(d.with_layer_locks(index, 1).unwrap().unwrap()),
        };
        let picked = select(&frozen, index, 2).unwrap().expect("a change");
        assert_solid(&picked, depth, index, COLORS[2], "frozen select");
        assert_eq!(info(&picked, index).unwrap().unwrap()[1], 2);
    }
}

#[test]
fn more_input_is_phase_one_input_and_add_appends_under_the_caps() {
    for depth in DEPTHS {
        let coverage = rect_coverage(W, H, RECT);
        let (doc, index) = filled(depth);
        assert_eq!(more(&doc, index).unwrap(), input(&base(depth), &coverage));
        assert_eq!(
            more(&doc, 0).err().as_deref(),
            Some(no_variations(0).as_str())
        );
        // Add one with its own tag: appended, selected, rendered, tag swapped.
        let added = add(&doc, index, &[solid(NATIVE, COLORS[3])], TAG2)
            .unwrap()
            .expect("a change");
        let found = info(&added, index).unwrap().unwrap();
        assert_eq!((found[0], found[1]), (4, 3));
        assert_solid(&added, depth, index, COLORS[3], "added");
        assert_eq!(tag_of(&added, index).as_deref(), Some(TAG2));
        let back = select(&added, index, 0).unwrap().unwrap();
        assert_eq!(tag_of(&back, index).as_deref(), Some(TAG));
        // 24 are kept, a 25th is refused.
        let four: Vec<Picture> = COLORS.iter().map(|&c| solid((8, 8), c)).collect();
        let mut grown = added;
        for _ in 0..5 {
            grown = add(&grown, index, &four, TAG).unwrap().unwrap();
        }
        assert_eq!(info(&grown, index).unwrap().unwrap()[0], 24);
        assert_eq!(
            add(&grown, index, &four[..1], TAG).err().as_deref(),
            Some("a generative layer keeps at most 24 variations")
        );
    }
    assert!(check_caps(&[80 << 20]).is_ok());
    assert_eq!(
        check_caps(&[(80 << 20) + 1]).unwrap_err(),
        "a generative variation is over 80 MiB"
    );
    assert!(check_caps(&[64 << 20, 64 << 20, 64 << 20, 64 << 20]).is_ok());
    assert_eq!(
        check_caps(&[64 << 20, 64 << 20, 64 << 20, (64 << 20) + 1]).unwrap_err(),
        "a generative layer keeps at most 256 MiB of variations"
    );
    assert_eq!(
        check_caps(&[1; 25]).unwrap_err(),
        "a generative layer keeps at most 24 variations"
    );
}

#[test]
fn variations_round_trip_under_word_92() {
    let dir = TempDir::new().unwrap();
    for depth in DEPTHS {
        let tag = format!("{depth:?}");
        let plain = save(&base(depth), &dir, &format!("{tag}-plain.rz"));
        assert_ne!(drdc_word(&plain, 1), WORD, "no variations, no word");
        let (doc, index) = filled(depth);
        let bytes = save(&doc, &dir, &format!("{tag}-a.rz"));
        assert_eq!(drdc_word(&bytes, 1), WORD);
        let reopened = open(depth, &bytes, &dir, &format!("{tag}-b.rz")).unwrap();
        assert_eq!(info(&reopened, index).unwrap(), info(&doc, index).unwrap());
        assert_eq!(unit_layer(&reopened, index), unit_layer(&doc, index));
        assert_eq!(
            save(&reopened, &dir, &format!("{tag}-c.rz")),
            bytes,
            "resave"
        );
        for k in 1..3u32 {
            let want = select(&doc, index, k).unwrap().unwrap();
            let got = select(&reopened, index, k).unwrap().unwrap();
            assert_eq!(
                unit_layer(&got, index),
                unit_layer(&want, index),
                "variation {k}"
            );
        }
        // The word is the claim: removed, the file refuses; added to a
        // variation-free file, the table is missing.
        let stripped = remove_word(&bytes, 1);
        assert!(open(depth, &stripped, &dir, &format!("{tag}-d.rz")).is_err());
        let forged = insert_word(&plain, 1, WORD);
        let refusal = open(depth, &forged, &dir, &format!("{tag}-e.rz"))
            .err()
            .unwrap();
        assert!(
            refusal.ends_with("invalid native generative variations table tag or schema"),
            "{refusal}"
        );
        // A forged count and a PNG size mismatch refuse.
        let at = bytes.windows(4).position(|w| w == b"GVAR").unwrap();
        let mut count = bytes.clone();
        count[at + 12 + 57..at + 12 + 61].copy_from_slice(&25u32.to_le_bytes());
        let refusal = open(depth, &count, &dir, &format!("{tag}-f.rz"))
            .err()
            .unwrap();
        assert!(
            refusal.contains("invalid native generative variations table: "),
            "{refusal}"
        );
        let mut size = bytes.clone();
        size[at + 12 + 61..at + 12 + 65].copy_from_slice(&(NATIVE.0 + 1).to_le_bytes());
        let refusal = open(depth, &size, &dir, &format!("{tag}-g.rz"))
            .err()
            .unwrap();
        assert!(
            refusal.contains("invalid native generative variations table: "),
            "{refusal}"
        );
    }
}

#[test]
fn every_destructive_edit_detaches_the_variations() {
    let dir = TempDir::new().unwrap();
    // 16 bits: a pixel edit drops the description.
    let (doc, index) = filled(Depth::Sixteen);
    let Doc::Typed(typed) = &doc else {
        unreachable!()
    };
    let edited = crate::native_scalar::edit(
        typed,
        json!({"op":"scalar_edit","target":"red","index":index,
               "operation":{"kind":"fill","value":40000},"selection":false}),
        &[],
    )
    .expect("an edit");
    assert_detached(&Doc::Typed(edited), index, &dir, "16 pixel edit");
    // 16 bits: Convert to Profile.
    let p3 = IccProfile::display_p3();
    let mut err = ptr::null_mut();
    let converted = unsafe {
        dr_precision_doc_convert_to_profile(
            typed,
            p3.bytes().as_ptr(),
            p3.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!converted.is_null(), "{}", take_err_string(err));
    let converted = Doc::Typed(*unsafe { Box::from_raw(converted) });
    assert_detached(&converted, index, &dir, "16 convert");
    // 8 bits: a paint stroke, a move, Assign Profile and a mask deletion.
    let (doc, index) = filled(Depth::Byte);
    let Doc::Byte(byte) = &doc else {
        unreachable!()
    };
    let mut overlay = vec![0u8; (4 * W * H) as usize];
    for p in overlay.chunks_mut(4).take(200) {
        p.copy_from_slice(&[255, 255, 255, 255]);
    }
    let painted = unsafe { dr_doc_painting_layer(byte, index, overlay.as_ptr(), W, H, 0, 1.0) };
    assert!(!painted.is_null());
    assert_detached(
        &Doc::Byte(*unsafe { Box::from_raw(painted) }),
        index,
        &dir,
        "8 paint",
    );
    let moved = unsafe { dr_doc_with_layer_offset(byte, index, 3, 2) };
    assert!(!moved.is_null());
    assert_detached(
        &Doc::Byte(*unsafe { Box::from_raw(moved) }),
        index,
        &dir,
        "8 move",
    );
    let mut err = ptr::null_mut();
    let assigned =
        unsafe { dr_doc_assign_profile(byte, p3.bytes().as_ptr(), p3.bytes().len(), &mut err) };
    assert!(!assigned.is_null(), "{}", take_err_string(err));
    assert_detached(
        &Doc::Byte(*unsafe { Box::from_raw(assigned) }),
        index,
        &dir,
        "8 assign",
    );
    let unmasked = unsafe { dr_doc_removing_layer_mask(byte, index, false) };
    assert!(!unmasked.is_null());
    let unmasked = Doc::Byte(*unsafe { Box::from_raw(unmasked) });
    assert_eq!(
        more(&unmasked, index).err().as_deref(),
        Some(no_variations(index).as_str())
    );
    assert_detached(&unmasked, index, &dir, "8 mask delete");
}

#[test]
fn a_mask_stroke_keeps_the_variations_live() {
    let (doc, index) = filled(Depth::Byte);
    let Doc::Byte(byte) = &doc else {
        unreachable!()
    };
    // Black hides a hole inside the rect, leaving its bounding box.
    let hole = (24u32, 20u32, 32u32, 28u32);
    let mut overlay = vec![0u8; (4 * W * H) as usize];
    for y in hole.1..hole.3 {
        for x in hole.0..hole.2 {
            let at = 4 * (y * W + x) as usize;
            overlay[at..at + 4].copy_from_slice(&[0, 0, 0, 255]);
        }
    }
    let painted = unsafe { dr_doc_painting_layer_mask(byte, index, overlay.as_ptr(), W, H) };
    assert!(!painted.is_null());
    let painted = Doc::Byte(*unsafe { Box::from_raw(painted) });
    assert!(info(&painted, index).unwrap().is_some(), "still live");
    let coverage: Vec<u16> = rect_coverage(W, H, RECT)
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let (x, y) = (i as u32 % W, i as u32 / W);
            if (hole.0..hole.2).contains(&x) && (hole.1..hole.3).contains(&y) {
                0
            } else {
                c
            }
        })
        .collect();
    assert_eq!(mask_plane(&painted, index), coverage);
    assert_eq!(
        more(&painted, index).unwrap(),
        input(&base(Depth::Byte), &coverage)
    );
    let picked = select(&painted, index, 1).unwrap().expect("a change");
    assert_eq!(
        mask_plane(&picked, index),
        coverage,
        "the painted mask is kept"
    );
}

#[test]
fn the_depth_conversions_keep_the_variations() {
    let (doc, index) = filled(Depth::Byte);
    let Doc::Byte(byte) = &doc else {
        unreachable!()
    };
    let mut err = ptr::null_mut();
    let typed = unsafe { dr_doc_promote_structure(byte, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!typed.is_null(), "{}", take_err_string(err));
    let typed = Doc::Typed(*unsafe { Box::from_raw(typed) });
    assert_eq!(info(&typed, index).unwrap(), info(&doc, index).unwrap());
    let picked = select(&typed, index, 2).unwrap().expect("a change");
    assert_solid(&picked, Depth::Sixteen, index, COLORS[2], "16 after 8→16");
    let Doc::Typed(typed_doc) = &picked else {
        unreachable!()
    };
    let mut err = ptr::null_mut();
    let narrowed = unsafe {
        dr_precision_doc_narrow_structure_to_u8(typed_doc, 0, LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!narrowed.is_null(), "{}", take_err_string(err));
    let narrowed = Doc::Byte(*unsafe { Box::from_raw(narrowed) });
    assert_eq!(info(&narrowed, index).unwrap().unwrap()[1], 2);
    let back = select(&narrowed, index, 0).unwrap().expect("a change");
    assert_solid(&back, Depth::Byte, index, COLORS[0], "8 after 16→8");
}

/// The byte count a working-limit refusal says it needs.
fn needs(refusal: &str) -> u64 {
    let rest = refusal
        .strip_prefix("precision operation needs ")
        .unwrap_or_else(|| panic!("not a working-limit refusal: {refusal}"));
    rest.split(' ').next().unwrap().parse().unwrap()
}

#[test]
fn more_input_admits_its_plan_and_the_coverage_copy_first() {
    let n = u64::from(W * H);
    let coverage = rect_coverage(W, H, RECT);
    for depth in DEPTHS {
        let (doc, index) = filled(depth);
        // Retained + the input plan, from phase 1's input on the same document.
        let fill = needs(&input_at(&doc, &coverage, 1).unwrap_err());
        let total = needs(&more_at(&doc, index, 1).unwrap_err());
        assert_eq!(total, fill + 2 * n, "{depth:?}: the plan and the u16 copy");
        assert_eq!(
            more_at(&doc, index, total - 1).unwrap_err(),
            format!(
                "precision operation needs {total} bytes including retained inputs; \
                 working limit is {} bytes",
                total - 1
            ),
            "{depth:?}: one byte under"
        );
        assert!(more_at(&doc, index, LIMIT).is_ok());
    }
}

/// The byte `.rz` reopen plan counts each stored picture and tag once and
/// the binding's rendered raster not at all (the reader rebuilds it from the
/// layer's own pixels).
#[test]
fn the_byte_reopen_plan_counts_the_stored_pictures_once() {
    let (w, h) = (256u32, 256u32);
    let retained = |doc: &Doc| {
        let Doc::Byte(d) = doc else { unreachable!() };
        let (mut kept, mut decode) = (0u64, 0u64);
        let mut err = ptr::null_mut();
        let ok = unsafe { dr_doc_native_reopen_plan(d, &mut kept, &mut decode, &mut err) };
        assert!(ok, "{}", take_err_string(err));
        kept
    };
    let doc = build(Depth::Byte, w, h, &[gradient(w, h)]);
    let coverage = rect_coverage(w, h, (16, 16, 200, 200));
    let results: Vec<Picture> = COLORS[..3].iter().map(|&c| solid(NATIVE, c)).collect();
    let (out, index) = apply(&doc, &coverage, &results, TAG).unwrap();
    let Doc::Byte(d) = &out else { unreachable!() };
    let table = &d.layers[index].generated.as_ref().expect("a binding").table;
    assert_eq!(table.items.len(), 3);
    let stored: u64 = table
        .items
        .iter()
        .map(|v| (v.png.len() + v.tag.len()) as u64)
        .sum();
    let n = u64::from(w * h);
    let floor = 4 * n + n + stored;
    let grew = retained(&out) - retained(&doc);
    assert!(grew >= floor, "grew {grew}, floor {floor}");
    assert!(
        grew < floor + 4 * n,
        "grew {grew}: the rendered raster is counted twice (floor {floor})"
    );
}

/// An angled Pattern Overlay (words 93 and 90): the preset `dots` at 30°.
const ANGLED: &str = r#"{"effects":[{"type":"pattern_overlay","pattern":{"preset":"dots"},"angle":30,"scale":0.5}]}"#;

/// `doc`'s layer `idx` offset to `to` (byte `dr_doc_with_layer_offset`; typed
/// a pure-translation `affine_set` through `dr_precision_doc_edit`), so its
/// reference point (word 91, `LREF`) is `to`.
fn moved(doc: &Doc, idx: usize, to: (i32, i32)) -> Doc {
    match doc {
        Doc::Byte(d) => {
            let out = unsafe { dr_doc_with_layer_offset(d, idx, to.0, to.1) };
            assert!(!out.is_null(), "the move");
            Doc::Byte(*unsafe { Box::from_raw(out) })
        }
        Doc::Typed(d) => Doc::Typed(typed_edit(
            d,
            &json!({"op":"affine_set","layers":[idx],
                "matrix":[1, 0, 0, 1, to.0, to.1],"filter":"nearest"}),
        )),
    }
}

/// `doc`'s layer `idx` styled with `style` (byte `dr_doc_set_layer_style`;
/// typed the `layer_style` edit).
fn with_style(doc: &Doc, idx: usize, style: &str) -> Doc {
    match doc {
        Doc::Byte(d) => Doc::Byte(crate::common::styled(d, idx, style)),
        Doc::Typed(d) => {
            let style: serde_json::Value = serde_json::from_str(style).unwrap();
            Doc::Typed(typed_edit(
                d,
                &json!({"op":"layer_style","version":1,"layer":idx,"style":style}),
            ))
        }
    }
}

fn typed_edit(
    doc: &doctored_core::precision_document::PrecisionDocument,
    request: &serde_json::Value,
) -> doctored_core::precision_document::PrecisionDocument {
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        doctored_core::ffi_precision_edit::dr_precision_doc_edit(
            doc,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    *unsafe { Box::from_raw(out) }
}

/// Layer `idx`'s reference point.
fn reference_of(doc: &Doc, idx: usize) -> (i32, i32) {
    match doc {
        Doc::Byte(d) => d.layers[idx].reference,
        Doc::Typed(d) => d.layer(idx).unwrap().reference(),
    }
}

/// Layer `idx`'s stored style as the core reports it.
fn style_of(doc: &Doc, idx: usize) -> serde_json::Value {
    let text = match doc {
        Doc::Byte(d) => d.layers[idx]
            .style
            .as_deref()
            .map(doctored_core::style::LayerStyle::to_json)
            .expect("a style"),
        Doc::Typed(d) => d.layer(idx).unwrap().reported_style().expect("a style"),
    };
    serde_json::from_str(&text).unwrap()
}

/// Exchanges conditional words `a` and `b` (indices as `drdc_word`'s).
fn swap_words(bytes: &[u8], a: usize, b: usize) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let (wa, wb) = (drdc_word(bytes, a), drdc_word(bytes, b));
    out[4 + a * 4..8 + a * 4].copy_from_slice(&wb.to_le_bytes());
    out[4 + b * 4..8 + b * 4].copy_from_slice(&wa.to_le_bytes());
    out
}

/// Words 93, 92, 91 and 90 in ONE file, at 8 and 16 bits: a Generative
/// Layer with live variations, a different layer moved since it was made
/// and a third holding an angled Pattern Overlay. The words follow the
/// identity wrapper in descending order, the file reopens with all three
/// facts and re-saves to the same bytes, and dropping 92, or exchanging 92
/// and 91, refuses instead of misparsing.
#[test]
fn variations_a_moved_layer_and_an_angled_overlay_share_one_file() {
    let dir = TempDir::new().unwrap();
    for depth in DEPTHS {
        let tag = format!("{depth:?}");
        let opaque = |rgb: [f64; 3]| vec![[rgb[0], rgb[1], rgb[2], 1.0]; (W * H) as usize];
        let doc = build(
            depth,
            W,
            H,
            &[
                gradient(W, H),
                opaque([0.2, 0.4, 0.6]),
                opaque([0.7, 0.3, 0.1]),
            ],
        );
        let coverage = rect_coverage(W, H, RECT);
        let results: Vec<Picture> = COLORS[..3].iter().map(|&c| solid(NATIVE, c)).collect();
        let (doc, generated) = apply(&doc, &coverage, &results, TAG).unwrap();
        assert_eq!(generated, 1, "{tag}: the fill lands above layer 0");
        let (shifted, angled) = (2, 3);
        let doc = moved(&doc, shifted, (7, -3));
        let doc = with_style(&doc, angled, ANGLED);
        let live = info(&doc, generated).unwrap();
        assert!(
            live.is_some(),
            "{tag}: the other layers' edits keep it live"
        );
        assert_eq!(reference_of(&doc, shifted), (7, -3), "{tag}");

        let bytes = save(&doc, &dir, &format!("{tag}-all.rz"));
        let wrapper = if depth == Depth::Byte { 45 } else { 46 };
        let words: Vec<u32> = (0..5).map(|i| drdc_word(&bytes, i)).collect();
        assert_eq!(words, [wrapper, 93, 92, 91, 90], "{tag}: the word order");
        let gvar = bytes.windows(4).position(|w| w == b"GVAR").unwrap();
        let lref = bytes.windows(4).position(|w| w == b"LREF").unwrap();
        let lids = bytes.windows(4).rposition(|w| w == b"LIDS").unwrap();
        assert!(gvar < lref && lref < lids, "{tag}: GVAR, LREF, LIDS");

        let reopened = open(depth, &bytes, &dir, &format!("{tag}-back.rz")).unwrap();
        assert_eq!(
            info(&reopened, generated).unwrap(),
            live,
            "{tag}: variations"
        );
        assert_eq!(
            unit_layer(&reopened, generated),
            unit_layer(&doc, generated),
            "{tag}: the rendered variation"
        );
        let want = select(&doc, generated, 2).unwrap().unwrap();
        let got = select(&reopened, generated, 2).unwrap().unwrap();
        assert_eq!(
            unit_layer(&got, generated),
            unit_layer(&want, generated),
            "{tag}: variation 2 after the reopen"
        );
        assert_eq!(
            reference_of(&reopened, shifted),
            (7, -3),
            "{tag}: reference"
        );
        assert_eq!(reference_of(&reopened, generated), (0, 0), "{tag}");
        let style = style_of(&reopened, angled);
        assert_eq!(style, style_of(&doc, angled), "{tag}: the style");
        assert_eq!(style["effects"][0]["angle"], 30.0, "{tag}: the angle");
        assert_eq!(projection(&reopened), projection(&doc), "{tag}: composite");
        assert_eq!(
            save(&reopened, &dir, &format!("{tag}-again.rz")),
            bytes,
            "{tag}: resave"
        );

        // Word 92 dropped: the LREF reader meets GVAR's bytes.
        let dropped = remove_word(&bytes, 2);
        assert_eq!(drdc_word(&dropped, 2), 91);
        let dropped_refusal = open(depth, &dropped, &dir, &format!("{tag}-dropped.rz"))
            .err()
            .expect("a file without word 92 must refuse");
        assert!(
            dropped_refusal.ends_with("unsupported native layer reference schema"),
            "{tag}: {dropped_refusal}"
        );
        // 92 and 91 exchanged: the chain is read in one order only.
        let swapped = swap_words(&bytes, 2, 3);
        assert_eq!(drdc_word(&swapped, 2), 91);
        let refusal = open(depth, &swapped, &dir, &format!("{tag}-swapped.rz"))
            .err()
            .expect("92 after 91 must refuse");
        // 91 is taken as the reference word, so 92 lands where the body
        // version belongs and is refused there.
        let want = if depth == Depth::Byte {
            "unsupported DRDC version 92"
        } else {
            "high-depth DRDC document"
        };
        assert!(refusal.ends_with(want), "{tag}: {refusal}");
    }
}
