//! The resolution the Magic Wand, the Paint Bucket and Select ▸ Similar and
//! Grow compare a typed document's samples at (`selection_similar`'s module
//! doc, **Resolution**), through `dr_precision_doc_select` and the bucket's
//! `dr_precision_doc_edit` on documents built here with codes off the ×257
//! grid. Every expectation is this file's own oracle, written from the rule's
//! sentences: each R, G and B code is read as its nearest byte (v / 257,
//! rounded) and the tolerance the host sends (8-bit t × 257) as its nearest
//! byte, while alpha keeps its own codes and the tolerance's; a CMYK
//! document's plate at 16 bits is the naive product on the 32768-step grid
//! (each ink i = round(v · 32768 / 65535), R = round((32768 − iC)(32768 −
//! iK) / 32768)) read to a byte as round(R · 255 / 32768); then R, G, B and
//! A each lie within the references' range ± the bound and the halved cyclic
//! differences ⌊(G − R + 255)/2⌋, ⌊(B − G + 255)/2⌋, ⌊(R − B + 255)/2⌋
//! within theirs ± half the colour bound (each key's range first drops
//! ⌊N/10⌋ of the N references' values at each end), global or a
//! four-connected flood. An 8-bit Gray document's
//! Sample Size box averages those bytes, so its wand is the byte wand's
//! (`dr_doc_magic_wand`) on the same pixels. Each scene also shows that the
//! rule it replaced (the keys on the u16 codes, the tolerance as sent) would
//! have answered otherwise.
use std::collections::VecDeque;
use std::ffi::CString;
use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_image_from_rgba};
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_from_image, dr_doc_magic_wand};
use doctored_core::ffi_gray::dr_precision_doc_open_gray;
use doctored_core::ffi_precision::dr_precision_doc_open;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_tiff_builder::cmyk_tiff;
use crate::common::{cpath, take_err_string};
use crate::gray_fixture::gray_tiff;
use crate::gray_ops::{free, layer, LIMIT};
use crate::native_scalar;
use crate::precision_select::select;

const W: u32 = 40;
const H: u32 = 22;
/// The 8-bit tolerance of every case.
const T: i32 = 10;
/// The field: 100 bytes and 100 codes, off the ×257 grid.
const FIELD: u16 = 257 * 100 + 100;
/// The probes' offsets from a byte's ×257 code, on both sides of the edges
/// at ±128.5 and of the field's own offset.
const KS: [i32; 13] = [
    -129, -128, -101, -100, -1, 0, 1, 99, 100, 101, 127, 128, 129,
];
/// A checker of 257·100 − 100 and 257·100 + 100: one byte, 200 codes apart.
const MIXED: (u32, u32, u32, u32) = (2, 13, 8, 6);
/// The host's tolerances: 257·T, and the codes either side of the half-way
/// point to the next byte.
const TOLERANCES: [u16; 3] = [257 * T as u16, 257 * T as u16 + 128, 257 * T as u16 + 129];

/// A u16 code as its nearest byte.
fn byte(v: u16) -> i64 {
    (f64::from(v) / 257.0).round() as i64
}

/// The keys of R, G and B (bytes, `top` 255; or codes, `top` 65535) and
/// alpha: the channels, alpha, then the halved cyclic differences.
fn keys(r: i64, g: i64, b: i64, a: u16, top: i64) -> [i64; 7] {
    [
        r,
        g,
        b,
        i64::from(a),
        (g - r + top).div_euclid(2),
        (b - g + top).div_euclid(2),
        (r - b + top).div_euclid(2),
    ]
}

fn byte_keys(p: [u16; 4]) -> [i64; 7] {
    keys(byte(p[0]), byte(p[1]), byte(p[2]), p[3], 255)
}

fn code_keys(p: [u16; 4]) -> [i64; 7] {
    let [r, g, b, _] = p.map(i64::from);
    keys(r, g, b, p[3], 65535)
}

/// One 16-bit ink on the 32768-step grid.
fn grid(v: u16) -> f64 {
    (f64::from(v) * 32768.0 / 65535.0).round()
}

/// One plate's naive RGB byte at 16 bits (module doc).
fn plate(v: u16, k: u16) -> i64 {
    let r = ((32768.0 - grid(v)) * (32768.0 - grid(k)) / 32768.0 + 0.5).floor();
    (r * 255.0 / 32768.0 + 0.5).floor() as i64
}

/// The region of keys `k` from the references `refs`: each key's range
/// (⌊N/10⌋ values dropped at each end) widened by `colour` on R, G and B,
/// by ⌊colour/2⌋ on the halved differences and by `alpha` on alpha; global,
/// or flooded four-connected from the references, which it always holds.
fn region(
    k: &[[i64; 7]],
    refs: &[usize],
    (colour, alpha): (i64, i64),
    contiguous: bool,
) -> Vec<bool> {
    let dropped = refs.len() / 10;
    let mut lo = [0i64; 7];
    let mut hi = [0i64; 7];
    for j in 0..7 {
        let mut values: Vec<i64> = refs.iter().map(|&i| k[i][j]).collect();
        values.sort();
        let bound = match j {
            3 => alpha,
            0..=2 => colour,
            _ => colour / 2,
        };
        lo[j] = values[dropped] - bound;
        hi[j] = values[values.len() - 1 - dropped] + bound;
    }
    let admitted: Vec<bool> = k
        .iter()
        .map(|p| (0..7).all(|j| lo[j] <= p[j] && p[j] <= hi[j]))
        .collect();
    if !contiguous {
        return admitted;
    }
    let mut out = vec![false; k.len()];
    let mut queue = VecDeque::new();
    for &i in refs {
        out[i] = true;
        queue.push_back(i);
    }
    while let Some(i) = queue.pop_front() {
        let (x, y) = ((i as u32 % W) as i64, (i as u32 / W) as i64);
        for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
            if nx < 0 || ny < 0 || nx >= i64::from(W) || ny >= i64::from(H) {
                continue;
            }
            let j = (ny * i64::from(W) + nx) as usize;
            if !out[j] && admitted[j] {
                out[j] = true;
                queue.push_back(j);
            }
        }
    }
    out
}

/// The rule's bounds of a host tolerance: its nearest byte on the colour
/// keys, itself on alpha.
fn bounds(tolerance: u16) -> (i64, i64) {
    (byte(tolerance), i64::from(tolerance))
}

fn mixed_indices() -> Vec<usize> {
    let (x0, y0, w, h) = MIXED;
    (y0..y0 + h)
        .flat_map(|y| (x0..x0 + w).map(move |x| (y * W + x) as usize))
        .collect()
}

/// The lattice slot of probe `n`: pitch 2 from (1, 1), 19 a row, so every
/// probe has four field neighbours.
fn slot(n: u32) -> usize {
    ((1 + 2 * (n / 19)) * W + 1 + 2 * (n % 19)) as usize
}

/// The neutral scene's codes: the field, single-pixel probes at 257·(100 ±
/// m) + k for m in T − 1 … T + 1 and k in [`KS`] (slots 0…77), and the
/// [`MIXED`] checker.
fn scene() -> Vec<u16> {
    let mut v = vec![FIELD; (W * H) as usize];
    let mut n = 0;
    for m in [T - 1, T, T + 1] {
        for s in [1, -1] {
            for k in KS {
                v[slot(n)] = (257 * (100 + s * m) + k) as u16;
                n += 1;
            }
        }
    }
    for i in mixed_indices() {
        let (x, y) = (i as u32 % W, i as u32 / W);
        v[i] = if (x + y) % 2 == 0 { 25600 } else { 25800 };
    }
    v
}

fn wand(x: u32, y: u32, tolerance: u16, contiguous: bool) -> Value {
    json!({"kind":"magic_wand","x":x,"y":y,"tolerance":tolerance,"contiguous":contiguous})
}

/// The selection as full / empty, every sample one or the other.
fn hard(out: &[u16]) -> Vec<bool> {
    assert!(out.iter().all(|v| *v == 0 || *v == 65535), "a hard result");
    out.iter().map(|v| *v == 65535).collect()
}

/// Every wand (from the field at (0, 0)) and Similar / Grow (from the
/// [`MIXED`] checker) case of one document against the region of its keys
/// `k`; answers how many pixels the region of `old` (the u16 rule's keys,
/// the tolerance as sent on every key) puts on the other side.
fn check_cases(
    doc: *const PrecisionDocument,
    k: &[[i64; 7]],
    old: &[[i64; 7]],
    what: &str,
) -> usize {
    let mixed = mixed_indices();
    let coverage: Vec<u16> = (0..(W * H) as usize)
        .map(|i| if mixed.contains(&i) { 65535 } else { 0 })
        .collect();
    let mut apart = 0;
    for tolerance in TOLERANCES {
        for contiguous in [false, true] {
            let got = hard(&select(doc, &wand(0, 0, tolerance, contiguous), &[]).unwrap());
            let want = region(k, &[0], bounds(tolerance), contiguous);
            assert_eq!(
                got, want,
                "{what} wand t {tolerance} contiguous {contiguous}"
            );
            let operation = json!({"kind":"similar","tolerance":tolerance,"contiguous":contiguous});
            let got = hard(&select(doc, &operation, &coverage).unwrap());
            let want = region(k, &mixed, bounds(tolerance), contiguous);
            assert_eq!(got, want, "{what} similar t {tolerance} grow {contiguous}");
            let t = i64::from(tolerance);
            let replaced = region(old, &mixed, (t, t), contiguous);
            apart += want.iter().zip(&replaced).filter(|(a, b)| a != b).count();
        }
    }
    apart
}

#[test]
fn a_16_bit_gray_document_compares_bytes() {
    let dir = TempDir::new().unwrap();
    let codes = scene();
    let data: Vec<u8> = codes.iter().flat_map(|v| v.to_le_bytes()).collect();
    let path = dir.path().join("gray16.tif");
    std::fs::write(&path, gray_tiff(W, H, 16, 1, 1, None, None, &data, None)).unwrap();
    let c = cpath(&path);
    let working = CString::new("gray_gamma_22").unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_gray(
            c.as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    let held: Vec<[u16; 4]> = layer(doc, 0, 16)
        .iter()
        .map(|p| p.map(|v| v as u16))
        .collect();
    let grey: Vec<u16> = held.iter().map(|p| p[0]).collect();
    assert_eq!(grey, codes, "the document holds the authored codes");
    let k: Vec<[i64; 7]> = held.iter().map(|p| byte_keys(*p)).collect();
    let old: Vec<[i64; 7]> = held.iter().map(|p| code_keys(*p)).collect();
    assert!(
        check_cases(doc, &k, &old, "gray16") > 0,
        "the scene separates the rules"
    );
    free(doc);
}

#[test]
fn a_16_bit_rgb_document_compares_bytes_and_its_bucket_follows() {
    let mut pixels: Vec<[u16; 4]> = scene().iter().map(|&v| [v, v, v, 65535]).collect();
    // Chroma probes (slots 78…89): R, then G, at 257·(100 + m) + k with the
    // other channels the field's, so R−G and R−B (or G−B) move with it.
    let offsets = [
        (10, 100),
        (10, 128),
        (11, -128),
        (11, -101),
        (-10, -100),
        (-10, 101),
    ];
    for n in 0..12 {
        let (m, k) = offsets[n % offsets.len()];
        pixels[slot(78 + n as u32)][n / offsets.len()] = (257 * (100 + m) + k) as u16;
    }
    let d = native_scalar::doc(W, H, pixels.clone());
    let k: Vec<[i64; 7]> = pixels.iter().map(|p| byte_keys(*p)).collect();
    let old: Vec<[i64; 7]> = pixels.iter().map(|p| code_keys(*p)).collect();
    assert!(
        check_cases(&d, &k, &old, "rgb16") > 0,
        "the scene separates the rules"
    );
    // The Paint Bucket fills the wand's contiguous region: red over a scene
    // whose green is never 0.
    for tolerance in TOLERANCES {
        let bucket = json!({"op":"bucket_fill","layer":0,"color":[1,0,0,1],
            "color_space":"document","opacity":1.0,"x":0,"y":0,"tolerance":tolerance,
            "contiguous":true});
        let n = native_scalar::plan(&d, bucket.clone())["input_samples"]
            .as_u64()
            .unwrap() as usize;
        let filled = native_scalar::edit(&d, bucket, &vec![65535; n]).expect("a change");
        let green = native_scalar::plane(&filled, "green", true);
        let got: Vec<bool> = green.iter().map(|g| *g == 0).collect();
        assert_eq!(
            got,
            region(&k, &[0], bounds(tolerance), true),
            "bucket t {tolerance}"
        );
    }
}

#[test]
fn a_16_bit_cmyk_document_takes_the_product_on_the_15_bit_grid() {
    // C = M = Y over K 90 bytes, both off the grid. Each probe moves C, M
    // and Y together (by 3850…4620 codes, 35 a step) or K (by 3250…3880, 30
    // a step), up or down: the plates' product then lies about 10 to 12
    // bytes from the field's, across the bounds, at offsets of its own. The
    // checker splits C and M apart.
    let dir = TempDir::new().unwrap();
    let (c0, k0) = (257 * 60 + 77, 257 * 90 + 77);
    let mut ink: Vec<[i32; 4]> = vec![[c0, c0, c0, k0]; (W * H) as usize];
    let mut n = 0;
    for s in [1, -1] {
        for j in 0..23 {
            let c = c0 + s * (3850 + 35 * j);
            ink[slot(n)] = [c, c, c, k0];
            n += 1;
        }
        for j in 0..22 {
            ink[slot(n)] = [c0, c0, c0, k0 + s * (3250 + 30 * j)];
            n += 1;
        }
    }
    for (j, i) in mixed_indices().into_iter().enumerate() {
        let d = if j % 2 == 0 { -150 } else { 150 };
        ink[i] = [c0 + d, c0 - d, c0 + d, k0];
    }
    let ink: Vec<[u16; 4]> = ink.iter().map(|p| p.map(|v| v as u16)).collect();
    let data: Vec<u8> = ink.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
    let path = dir.path().join("cmyk16.tif");
    let tiff = cmyk_tiff(W, H, 16, 4, 0, &data, Some(&press_profile("Press Select")));
    std::fs::write(&path, tiff).unwrap();
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    let k: Vec<[i64; 7]> = ink
        .iter()
        .map(|p| {
            keys(
                plate(p[0], p[3]),
                plate(p[1], p[3]),
                plate(p[2], p[3]),
                65535,
                255,
            )
        })
        .collect();
    // The rule this one replaced: the 16-bit product on the codes.
    let old: Vec<[i64; 7]> = ink
        .iter()
        .map(|p| {
            let plate =
                |v: u16| ((65535 - i64::from(v)) * (65535 - i64::from(p[3])) + 32767) / 65535;
            keys(plate(p[0]), plate(p[1]), plate(p[2]), 65535, 65535)
        })
        .collect();
    assert!(
        check_cases(doc, &k, &old, "cmyk16") > 0,
        "the scene separates the rules"
    );
    // Nor is it the 8-bit document's product on the inks' nearest bytes.
    let ink8: Vec<[i64; 7]> = ink
        .iter()
        .map(|p| {
            let plate = |v: u16| ((255 - byte(v)) * (255 - byte(p[3])) + 127) / 255;
            keys(plate(p[0]), plate(p[1]), plate(p[2]), 65535, 255)
        })
        .collect();
    assert_ne!(
        ink8, k,
        "the grid's product parts from the inks' 8-bit product"
    );
    free(doc);
}

/// An opaque grey noise of bytes 88…168.
fn noise(n: u32) -> Vec<u8> {
    let mut state = 0x80u64;
    (0..n)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (88 + (state >> 33) % 81) as u8
        })
        .collect()
}

#[test]
fn an_8_bit_gray_wand_averages_bytes_as_the_byte_wand_does() {
    let dir = TempDir::new().unwrap();
    let (w, h) = (36u32, 28u32);
    let grey = noise(w * h);
    let path = dir.path().join("gray8.tif");
    std::fs::write(&path, gray_tiff(w, h, 8, 1, 1, None, None, &grey, None)).unwrap();
    let c = cpath(&path);
    let working = CString::new("gray_gamma_22").unwrap();
    let mut err = ptr::null_mut();
    let typed = unsafe {
        dr_precision_doc_open_gray(
            c.as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!typed.is_null(), "{}", take_err_string(err));
    let rgba: Vec<u8> = grey.iter().flat_map(|&v| [v, v, v, 255]).collect();
    let byte_doc = unsafe {
        let image = dr_image_from_rgba(rgba.as_ptr(), w, h);
        let doc = dr_doc_from_image(image);
        dr_image_free(image);
        doc
    };
    let mut parted = 0;
    for size in [3u32, 5, 11] {
        for seed in [(18u32, 14u32), (0, 0), (35, 3)] {
            for t in [6u8, 12, 20] {
                for contiguous in [false, true] {
                    let mut mask = vec![0u8; (w * h) as usize];
                    assert!(unsafe {
                        dr_doc_magic_wand(
                            byte_doc,
                            seed.0,
                            seed.1,
                            t,
                            contiguous,
                            size,
                            mask.as_mut_ptr(),
                        )
                    });
                    let want: Vec<bool> = mask.iter().map(|m| *m == 255).collect();
                    let operation = json!({"kind":"magic_wand","x":seed.0,"y":seed.1,
                        "tolerance":257 * u32::from(t),"contiguous":contiguous,
                        "sample_size":size});
                    let got = hard(&select(typed, &operation, &[]).unwrap());
                    assert_eq!(
                        got, want,
                        "size {size} seed {seed:?} t {t} contiguous {contiguous}"
                    );
                    if contiguous {
                        continue;
                    }
                    // The rule this one replaced: the truncating mean of the
                    // u16 codes, compared on codes.
                    let r = i64::from(size / 2);
                    let (mut sum, mut count) = (0i64, 0i64);
                    for y in i64::from(seed.1) - r..=i64::from(seed.1) + r {
                        for x in i64::from(seed.0) - r..=i64::from(seed.0) + r {
                            if x >= 0 && y >= 0 && x < i64::from(w) && y < i64::from(h) {
                                sum += 257 * i64::from(grey[(y * i64::from(w) + x) as usize]);
                                count += 1;
                            }
                        }
                    }
                    let (mean, bound) = (sum / count, 257 * i64::from(t));
                    parted += grey
                        .iter()
                        .zip(&want)
                        .filter(|(v, inside)| {
                            ((257 * i64::from(**v) - mean).abs() <= bound) != **inside
                        })
                        .count();
                }
            }
        }
    }
    assert!(
        parted > 0,
        "the boxes separate the bytes' mean from the codes' mean"
    );
    unsafe { dr_doc_free(byte_doc) };
    free(typed);
}
