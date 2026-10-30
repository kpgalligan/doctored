//! The Magic Wand's Sample Size: Point Sample and the 3, 5, 11, 31, 51
//! and 101 by 101 Average. The reference the seven-projection box is anchored
//! at is the per-channel truncating mean of the box about the seed, clipped to
//! the canvas, over the codes the wand compares: bytes (`dr_doc_magic_wand`,
//! `dr_doc_magic_wand_layer`), native 16-bit codes and a CMYK document's naive
//! ink RGB (`dr_precision_doc_select`'s `magic_wand` with `sample_size`).
//! Contiguous marks the seed whatever the reference admits. Every expectation
//! is this file's own box mean, predicate and four-connected flood, written
//! without the core. Photoshop 26.10.0's take1
//! (`fixtures/photoshop_color_range`, `color_range_671_take.py`) left the
//! averaging UNMEASURED — its scripted click carries no Sample Size, every
//! size giving one mask — and the fixture pins that reason and the Point
//! Sample residual at the pixel the click landed on.
use std::collections::VecDeque;
use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_doc_open};
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit_plan, dr_precision_doc_select};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_tiff_builder::cmyk_tiff;
use crate::common::{cpath, take_err_string};
use crate::native_scalar;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const SIZES: [u32; 7] = [1, 3, 5, 11, 31, 51, 101];

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The oracle reference: the truncating mean of the `size` box about `seed`
/// over the in-canvas pixels.
fn mean(img: &[[i64; 4]], w: usize, h: usize, seed: (usize, usize), size: u32) -> [i64; 4] {
    let r = i64::from(size / 2);
    let (mut sum, mut count) = ([0i64; 4], 0i64);
    for y in seed.1 as i64 - r..=seed.1 as i64 + r {
        for x in seed.0 as i64 - r..=seed.0 as i64 + r {
            if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                continue;
            }
            let p = img[y as usize * w + x as usize];
            for c in 0..4 {
                sum[c] += p[c];
            }
            count += 1;
        }
    }
    sum.map(|v| v / count)
}

/// The seven-projection predicate.
fn similar(p: [i64; 4], seed: [i64; 4], tol: i64) -> bool {
    let project = |c: [i64; 4]| {
        [
            c[0],
            c[1],
            c[2],
            c[3],
            c[0] - c[1],
            c[1] - c[2],
            c[0] - c[2],
        ]
    };
    let (a, b) = (project(p), project(seed));
    (0..7).all(|k| (a[k] - b[k]).abs() <= tol)
}

/// The oracle region against `reference`: global, or four-connected from
/// `seed`, which is always in it.
fn region(
    img: &[[i64; 4]],
    w: usize,
    h: usize,
    seed: (usize, usize),
    reference: [i64; 4],
    tol: i64,
    contiguous: bool,
) -> Vec<bool> {
    let admitted: Vec<bool> = img.iter().map(|&p| similar(p, reference, tol)).collect();
    if !contiguous {
        return admitted;
    }
    let mut out = vec![false; w * h];
    out[seed.1 * w + seed.0] = true;
    let mut queue = VecDeque::from([seed]);
    while let Some((x, y)) = queue.pop_front() {
        let mut next = Vec::new();
        if x > 0 {
            next.push((x - 1, y));
        }
        if x + 1 < w {
            next.push((x + 1, y));
        }
        if y > 0 {
            next.push((x, y - 1));
        }
        if y + 1 < h {
            next.push((x, y + 1));
        }
        for (nx, ny) in next {
            let i = ny * w + nx;
            if !out[i] && admitted[i] {
                out[i] = true;
                queue.push_back((nx, ny));
            }
        }
    }
    out
}

/// Opaque grey noise within ±`spread` of 128, with a bright square at the
/// top-left so the clipped boxes there average differently.
fn noise(w: u32, h: u32, spread: i64, salt: u64) -> Vec<u8> {
    let mut state = 0x6710 ^ salt;
    let mut out = Vec::new();
    for i in 0..w * h {
        let (x, y) = (i % w, i / w);
        let v = splitmix(&mut state);
        let channel = |c: usize| {
            let base = if x < 12 && y < 12 { 200 } else { 128 };
            let delta = ((v >> (16 * c)) % (2 * spread as u64 + 1)) as i64 - spread;
            (base + delta).clamp(0, 255) as u8
        };
        out.extend([channel(0), channel(1), channel(2), 255]);
    }
    out
}

fn codes(bytes: &[u8]) -> Vec<[i64; 4]> {
    bytes
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]].map(i64::from))
        .collect()
}

fn byte_doc(pixels: &[u8], w: u32, h: u32) -> *mut DrDocument {
    unsafe {
        let image = dr_image_from_rgba(pixels.as_ptr(), w, h);
        let doc = dr_doc_from_image(image);
        dr_image_free(image);
        doc
    }
}

fn wand(
    doc: *const DrDocument,
    seed: (u32, u32),
    tol: u8,
    contiguous: bool,
    size: u32,
) -> Vec<bool> {
    let n = unsafe { dr_doc_width(doc) * dr_doc_height(doc) } as usize;
    let mut mask = vec![7u8; n];
    assert!(unsafe {
        dr_doc_magic_wand(
            doc,
            seed.0,
            seed.1,
            tol,
            contiguous,
            size,
            mask.as_mut_ptr(),
        )
    });
    assert!(mask.iter().all(|&m| m == 0 || m == 255), "a hard mask");
    mask.iter().map(|&m| m == 255).collect()
}

#[test]
fn every_sample_size_anchors_the_byte_wand_at_its_clipped_box_mean() {
    let (w, h) = (120u32, 90u32);
    let pixels = noise(w, h, 40, 1);
    let img = codes(&pixels);
    let doc = byte_doc(&pixels, w, h);
    for seed in [(60u32, 45u32), (3, 2), (119, 89), (10, 80)] {
        for size in SIZES {
            let reference = mean(&img, 120, 90, (seed.0 as usize, seed.1 as usize), size);
            for tol in [8u8, 20, 40] {
                for contiguous in [true, false] {
                    let want = region(
                        &img,
                        120,
                        90,
                        (seed.0 as usize, seed.1 as usize),
                        reference,
                        i64::from(tol),
                        contiguous,
                    );
                    assert_eq!(
                        wand(doc, seed, tol, contiguous, size),
                        want,
                        "seed {seed:?} size {size} t{tol} contiguous {contiguous}"
                    );
                }
            }
        }
    }
    unsafe { dr_doc_free(doc) };
}

#[test]
fn an_averaged_reference_parts_from_the_point_sample_on_a_checker() {
    // A 2-pixel checker of 100 and 160: the seed is 160, its 5×5 mean is
    // 129 or 131, so at tolerance 20 Point selects only the 160s and the
    // average selects neither.
    let (w, h) = (20u32, 20u32);
    let mut pixels = Vec::new();
    for i in 0..w * h {
        let (x, y) = (i % w, i / w);
        let v = if (x / 2 + y / 2) % 2 == 0 { 160 } else { 100 };
        pixels.extend([v, v, v, 255]);
    }
    let doc = byte_doc(&pixels, w, h);
    let point = wand(doc, (10, 10), 20, false, 1);
    let averaged = wand(doc, (10, 10), 20, false, 5);
    assert_eq!(point.iter().filter(|&&s| s).count(), 200);
    assert!(averaged.iter().all(|&s| !s));
    // Contiguous still marks the seed itself.
    let seeded = wand(doc, (10, 10), 20, true, 5);
    assert_eq!(seeded.iter().filter(|&&s| s).count(), 1);
    assert!(seeded[10 * 20 + 10]);
    unsafe { dr_doc_free(doc) };
}

#[test]
fn the_layer_wand_takes_the_same_sample_size() {
    let (w, h) = (40u32, 30u32);
    let pixels = noise(w, h, 30, 2);
    let img = codes(&pixels);
    let doc = byte_doc(&pixels, w, h);
    for size in [1, 11, 101] {
        let mut mask = vec![0u8; (w * h) as usize];
        assert!(unsafe {
            dr_doc_magic_wand_layer(doc, 0, 20, 15, 25, true, size, mask.as_mut_ptr())
        });
        let want = region(
            &img,
            40,
            30,
            (20, 15),
            mean(&img, 40, 30, (20, 15), size),
            25,
            true,
        );
        let got: Vec<bool> = mask.iter().map(|&m| m == 255).collect();
        assert_eq!(got, want, "size {size}");
    }
    unsafe { dr_doc_free(doc) };
}

#[test]
fn an_unlisted_sample_size_and_null_inputs_refuse_and_write_nothing() {
    let pixels = noise(8, 6, 10, 3);
    let doc = byte_doc(&pixels, 8, 6);
    let mut mask = vec![9u8; 48];
    unsafe {
        for size in [0, 2, 7, 102, u32::MAX] {
            assert!(!dr_doc_magic_wand(
                doc,
                1,
                1,
                10,
                true,
                size,
                mask.as_mut_ptr()
            ));
            assert!(!dr_doc_magic_wand_layer(
                doc,
                0,
                1,
                1,
                10,
                true,
                size,
                mask.as_mut_ptr()
            ));
        }
        assert!(!dr_doc_magic_wand(
            ptr::null(),
            1,
            1,
            10,
            true,
            3,
            mask.as_mut_ptr()
        ));
        assert!(!dr_doc_magic_wand(doc, 1, 1, 10, true, 3, ptr::null_mut()));
        assert!(!dr_doc_magic_wand_layer(
            ptr::null(),
            0,
            1,
            1,
            10,
            true,
            3,
            mask.as_mut_ptr()
        ));
        assert!(!dr_doc_magic_wand_layer(
            doc,
            0,
            1,
            1,
            10,
            true,
            3,
            ptr::null_mut()
        ));
        assert!(!dr_doc_magic_wand(
            doc,
            8,
            1,
            10,
            true,
            3,
            mask.as_mut_ptr()
        ));
    }
    assert!(mask.iter().all(|&m| m == 9), "a refusal writes nothing");
    unsafe { dr_doc_free(doc) };
}

/// `dr_precision_doc_select` of a composite `magic_wand`, or its error.
fn native_wand(doc: *const PrecisionDocument, operation: Value) -> Result<Vec<u16>, String> {
    let request = json!({"op":"selection","operation":operation});
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let plan = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    if plan.is_null() {
        return Err(take_err_string(err));
    }
    let planned: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(plan) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(plan) };
    let mut out = vec![0u16; planned["output_samples"].as_u64().unwrap() as usize];
    let ok = unsafe {
        dr_precision_doc_select(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
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

#[test]
fn sixteen_bit_codes_average_natively() {
    let (w, h) = (60u32, 50u32);
    let mut state = 0x16;
    let native: Vec<[u16; 4]> = (0..w * h)
        .map(|_| {
            let v = splitmix(&mut state);
            let c = |s: u32| 30000 + ((v >> s) % 8001) as u16;
            [c(0), c(16), c(32), 65535]
        })
        .collect();
    let img: Vec<[i64; 4]> = native.iter().map(|p| p.map(i64::from)).collect();
    let d = native_scalar::doc(w, h, native);
    for size in SIZES {
        for (seed, tol, contiguous) in [((30, 25), 2000, true), ((1, 48), 2500, false)] {
            let got = native_wand(
                &d,
                json!({"kind":"magic_wand","x":seed.0,"y":seed.1,"tolerance":tol,
                    "contiguous":contiguous,"sample_size":size}),
            )
            .unwrap();
            let reference = mean(&img, 60, 50, seed, size);
            let want = region(&img, 60, 50, seed, reference, tol, contiguous);
            let got: Vec<bool> = got.iter().map(|&c| c == 65535).collect();
            assert_eq!(got, want, "size {size} seed {seed:?}");
        }
    }
    // Absent sample_size is Point Sample.
    assert_eq!(
        native_wand(&d, json!({"kind":"magic_wand","x":3,"y":3,"tolerance":900})).unwrap(),
        native_wand(
            &d,
            json!({"kind":"magic_wand","x":3,"y":3,"tolerance":900,"sample_size":1})
        )
        .unwrap()
    );
    for size in [0, 4, 200] {
        assert_eq!(
            native_wand(
                &d,
                json!({"kind":"magic_wand","x":3,"y":3,"tolerance":900,"sample_size":size})
            )
            .unwrap_err(),
            "wand sample_size must be 1, 3, 5, 11, 31, 51 or 101"
        );
    }
}

/// The ink's naive RGB at 16 bits, restated from `selection_region`'s module
/// doc: `round((65535 − ink)(65535 − K) / 65535)` per plate, half up.
fn ink_rgb(p: [u16; 5]) -> [i64; 4] {
    let plate = |v: u16| {
        let product = (65535 - i64::from(v)) * (65535 - i64::from(p[3]));
        (product + 32767) / 65535
    };
    [plate(p[0]), plate(p[1]), plate(p[2]), i64::from(p[4])]
}

#[test]
fn a_cmyk_wand_averages_the_inks_naive_rgb() {
    let dir = TempDir::new().unwrap();
    let (w, h) = (48u32, 36u32);
    let mut state = 0xC0;
    let ink: Vec<[u16; 4]> = (0..w * h)
        .map(|_| {
            let v = splitmix(&mut state);
            let c = |s: u32| 20000 + ((v >> s) % 6001) as u16;
            [c(0), c(16), c(32), ((v >> 48) % 4001) as u16]
        })
        .collect();
    let data: Vec<u8> = ink.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
    let path = dir.path().join("wand.tif");
    let icc = press_profile("Press Select");
    std::fs::write(&path, cmyk_tiff(w, h, 16, 4, 0, &data, Some(&icc))).unwrap();
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "open: {}", take_err_string(err));
    let img: Vec<[i64; 4]> = ink
        .iter()
        .map(|p| ink_rgb([p[0], p[1], p[2], p[3], 65535]))
        .collect();
    for size in [1, 3, 11, 51] {
        for contiguous in [true, false] {
            let got = native_wand(
                doc,
                json!({"kind":"magic_wand","x":24,"y":18,"tolerance":3000,
                    "contiguous":contiguous,"sample_size":size}),
            )
            .unwrap();
            let reference = mean(&img, 48, 36, (24, 18), size);
            let want = region(&img, 48, 36, (24, 18), reference, 3000, contiguous);
            let got: Vec<bool> = got.iter().map(|&c| c == 65535).collect();
            assert_eq!(got, want, "size {size} contiguous {contiguous}");
        }
    }
    assert_eq!(
        native_wand(
            doc,
            json!({"kind":"magic_wand","x":2,"y":2,"tolerance":10,"sample_size":9})
        )
        .unwrap_err(),
        "wand sample_size must be 1, 3, 5, 11, 31, 51 or 101"
    );
    unsafe { dr_precision_doc_free(doc) };
}

/// take1's wand masks: the 1 and 101 by 101 cases at one click, tolerance
/// and contiguity are identical in Photoshop (why the averaging is
/// UNMEASURED), and Doctored's Point Sample wand seeded where Photoshop's
/// click landed, (x + 1, y + 1), differs from each mask by exactly the
/// recorded count (the P7 residual on noise; 0 on the contiguous ones).
#[test]
fn photoshops_take1_wand_masks_pin_the_unmeasured_sample_size() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/photoshop_color_range");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(folder.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["wand"]["verdict"], "UNMEASURED");
    assert_eq!(manifest["wand"]["member"], json!(["TC", "S1"]));
    let input = image::open(folder.join("w.png")).unwrap().to_rgba8();
    let (w, h) = input.dimensions();
    let doc = byte_doc(input.as_raw(), w, h);
    let mut masks = std::collections::BTreeMap::new();
    let mut checked = 0;
    for case in manifest["cases"].as_array().unwrap() {
        if case["kind"] != "wand" {
            continue;
        }
        let photoshop: Vec<bool> = image::open(folder.join(case["photoshop"].as_str().unwrap()))
            .unwrap()
            .to_rgba8()
            .pixels()
            .map(|p| p.0[0] >= 128)
            .collect();
        let landed = case["landed"].as_array().unwrap();
        let seed = (
            landed[0].as_u64().unwrap() as u32,
            landed[1].as_u64().unwrap() as u32,
        );
        let tolerance = case["tolerance"].as_u64().unwrap() as u8;
        let contiguous = case["contiguous"].as_bool().unwrap();
        let got = wand(doc, seed, tolerance, contiguous, 1);
        let mismatched = got.iter().zip(&photoshop).filter(|(a, b)| a != b).count();
        assert_eq!(
            mismatched as u64,
            case["point_p7_mismatch"].as_u64().unwrap(),
            "{}",
            case["name"]
        );
        let key = (
            case["x"].as_u64(),
            case["y"].as_u64(),
            tolerance,
            contiguous,
        );
        masks.entry(key).or_insert_with(Vec::new).push(photoshop);
        checked += 1;
    }
    assert_eq!(checked, 4);
    for (key, group) in masks {
        assert_eq!(group.len(), 2, "{key:?}");
        assert_eq!(group[0], group[1], "{key:?}: Photoshop's sizes part");
    }
    unsafe { dr_doc_free(doc) };
}
