//! Select ▸ Similar and Grow's reference range on every document kind,
//! through the FFI (`selection_similar`'s module doc): each key drops ⌊N/10⌋
//! of the N references' values from each end before its bound widens the
//! range — on a Grayscale document at 8 and 16 bits, an RGB document (the
//! byte path at 8 bits, the typed path at 16) and a CMYK document (its ink's
//! naive RGB). The keys are R, G, B and the halved cyclic differences, bound
//! by t and ⌊t/2⌋. The oracle is a sort-based trim over keys written here,
//! never the core's histograms.
use std::ffi::CString;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba};
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_from_image};
use doctored_core::ffi_gray::dr_precision_doc_open_gray;
use doctored_core::ffi_precision::dr_precision_doc_open;
use doctored_core::ffi_selection_tools::dr_doc_select_similar;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::json;
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_tiff_builder::cmyk_tiff;
use crate::common::{cpath, take_err_string};
use crate::gray_fixture::gray_tiff;
use crate::gray_ops::{free, LIMIT};
use crate::precision_select::select;

/// The scene: `refs` (selected) then the probes, row-major `W` wide, the
/// last row padded with the first probe.
const W: u32 = 10;

fn scene(refs: &[u8], probes: &[u8]) -> (u32, Vec<u8>, Vec<bool>) {
    let mut values = refs.to_vec();
    values.extend_from_slice(probes);
    values.resize(values.len().div_ceil(W as usize) * W as usize, probes[0]);
    let h = values.len() as u32 / W;
    let selected = (0..values.len()).map(|i| i < refs.len()).collect();
    (h, values, selected)
}

/// The oracle: each value sorted, ⌊N/10⌋ dropped from each end when `trim`,
/// widened by `t`; Similar admits every pixel in range (global).
fn oracle(values: &[u8], selected: &[bool], t: u8, trim: bool) -> Vec<bool> {
    let mut refs: Vec<u8> = values
        .iter()
        .zip(selected)
        .filter(|(_, s)| **s)
        .map(|(v, _)| *v)
        .collect();
    refs.sort_unstable();
    let k = if trim { refs.len() / 10 } else { 0 };
    let (lo, hi) = (
        i32::from(refs[k]) - i32::from(t),
        i32::from(refs[refs.len() - 1 - k]) + i32::from(t),
    );
    values
        .iter()
        .map(|v| (lo..=hi).contains(&i32::from(*v)))
        .collect()
}

fn gray_doc(dir: &TempDir, name: &str, h: u32, values: &[u8], bits: u16) -> *mut PrecisionDocument {
    let data: Vec<u8> = if bits == 8 {
        values.to_vec()
    } else {
        values
            .iter()
            .flat_map(|v| (u16::from(*v) * 257).to_le_bytes())
            .collect()
    };
    let path = dir.path().join(name);
    std::fs::write(&path, gray_tiff(W, h, bits, 1, 1, None, None, &data, None)).unwrap();
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
    doc
}

/// Similar (global) on a typed document from the selected pixels at byte
/// tolerance `t` (the typed request's 257·t): whether each pixel is admitted.
fn typed(doc: *const PrecisionDocument, selected: &[bool], t: u8) -> Vec<bool> {
    let coverage: Vec<u16> = selected
        .iter()
        .map(|s| if *s { 65535 } else { 0 })
        .collect();
    let operation = json!({"kind":"similar","tolerance":257 * u32::from(t),"contiguous":false});
    select(doc, &operation, &coverage)
        .unwrap()
        .iter()
        .map(|v| *v == 65535)
        .collect()
}

/// The cases: (references, probes, tolerance). Twenty references, 18 of 100
/// and one each of 80 and 120, at tolerance 0: a trim of two per end leaves
/// [100, 100], where the full box admits 110 and 90; three of 20 at 80 leave
/// one past the trim; nine references trim nothing; a hundred with a ladder
/// of tails trims ten from each end.
fn cases() -> Vec<(Vec<u8>, Vec<u8>, u8)> {
    let mut twenty = vec![100u8; 20];
    twenty[3] = 80;
    twenty[11] = 120;
    let mut three_low = vec![100u8; 20];
    for i in [0, 7, 15] {
        three_low[i] = 80;
    }
    let mut ladder = vec![100u8; 100];
    for (i, v) in [
        (0, 90),
        (1, 91),
        (2, 92),
        (3, 93),
        (4, 94),
        (5, 95),
        (6, 96),
        (7, 97),
    ] {
        ladder[i * 3] = v;
    }
    for i in 0..12 {
        ladder[50 + i] = 104 + (i as u8 % 3);
    }
    let probes = vec![100, 110, 90, 101, 99, 80, 120, 79, 121, 85];
    vec![
        (twenty, probes.clone(), 0),
        (three_low, probes.clone(), 2),
        ([vec![100u8; 8], vec![120u8]].concat(), probes.clone(), 1),
        (ladder, probes, 3),
    ]
}

#[test]
fn a_gray_documents_similar_trims_a_tenth_of_each_end() {
    let dir = TempDir::new().unwrap();
    for bits in [8u16, 16] {
        for (n, (refs, probes, t)) in cases().into_iter().enumerate() {
            let (h, values, selected) = scene(&refs, &probes);
            let doc = gray_doc(&dir, &format!("gray{bits}_{n}.tif"), h, &values, bits);
            let want = oracle(&values, &selected, t, true);
            assert_eq!(typed(doc, &selected, t), want, "{bits}-bit case {n}");
            free(doc);
        }
        // The literal fact: 18 of 20 references at 100 and one each at 80
        // and 120, tolerance 0, admit 100 and refuse 110 and 90.
        let (refs, probes, _) = cases().remove(0);
        let (h, values, selected) = scene(&refs, &probes);
        let doc = gray_doc(&dir, &format!("gray{bits}_fact.tif"), h, &values, bits);
        let got = typed(doc, &selected, 0);
        let at = |v: u8| got[refs.len() + probes.iter().position(|p| *p == v).unwrap()];
        assert!(at(100) && !at(110) && !at(90), "{bits}-bit: {got:?}");
        free(doc);
    }
}

fn byte_rgb(colours: &[[u8; 3]], h: u32) -> *mut DrDocument {
    let raw: Vec<u8> = colours
        .iter()
        .flat_map(|c| [c[0], c[1], c[2], 255])
        .collect();
    let image = unsafe { dr_image_from_rgba(raw.as_ptr(), W, h) };
    assert!(!image.is_null());
    let doc = unsafe { dr_doc_from_image(image) };
    unsafe { dr_image_free(image) };
    assert!(!doc.is_null());
    doc
}

fn byte_similar(doc: *mut DrDocument, selected: &[bool], t: u8) -> Vec<bool> {
    let mask: Vec<u8> = selected.iter().map(|s| if *s { 255 } else { 0 }).collect();
    let mut out = vec![0u8; mask.len()];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_doc_select_similar(
            doc,
            mask.as_ptr(),
            mask.len(),
            t,
            false,
            out.as_mut_ptr(),
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
    out.iter().map(|v| *v == 255).collect()
}

fn cmyk(dir: &TempDir, name: &str, h: u32, colours: &[[u8; 3]]) -> *mut PrecisionDocument {
    // Inks 255 − c on C, M and Y with K 0: the naive RGB is
    // (255 − ink)·(255 − 0)/255, so each pixel reads as the colour c.
    let data: Vec<u8> = colours
        .iter()
        .flat_map(|c| [255 - c[0], 255 - c[1], 255 - c[2], 0])
        .collect();
    let path = dir.path().join(name);
    let icc = press_profile("Similar Trim");
    std::fs::write(&path, cmyk_tiff(W, h, 8, 4, 0, &data, Some(&icc))).unwrap();
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

fn rgb16(colours: &[[u8; 3]], h: u32) -> *mut PrecisionDocument {
    let pixels = colours
        .iter()
        .map(|c| {
            let [r, g, b] = c.map(|v| u16::from(v) * 257);
            [r, g, b, 65535]
        })
        .collect();
    Box::into_raw(Box::new(crate::native_scalar::doc(W, h, pixels)))
}

/// The colour oracle: each key of the references (R, G, B and the halved
/// cyclic differences ⌊(G − R + 255)/2⌋, ⌊(B − G + 255)/2⌋, ⌊(R − B +
/// 255)/2⌋) sorted on its own, ⌊N/10⌋ dropped from each end, widened by t on
/// the channels and ⌊t/2⌋ on the differences; Similar admits every pixel in
/// range (global).
fn colour_oracle(colours: &[[u8; 3]], selected: &[bool], t: u8) -> Vec<bool> {
    let keys = |c: [u8; 3]| {
        let [r, g, b] = c.map(i32::from);
        [
            r,
            g,
            b,
            (g - r + 255).div_euclid(2),
            (b - g + 255).div_euclid(2),
            (r - b + 255).div_euclid(2),
        ]
    };
    let t = i32::from(t);
    let bound = [t, t, t, t / 2, t / 2, t / 2];
    let refs: Vec<[i32; 6]> = colours
        .iter()
        .zip(selected)
        .filter(|(_, s)| **s)
        .map(|(c, _)| keys(*c))
        .collect();
    let k = refs.len() / 10;
    let ranges: Vec<(i32, i32)> = (0..6)
        .map(|key| {
            let mut v: Vec<i32> = refs.iter().map(|r| r[key]).collect();
            v.sort_unstable();
            (v[k] - bound[key], v[v.len() - 1 - k] + bound[key])
        })
        .collect();
    colours
        .iter()
        .map(|c| {
            let p = keys(*c);
            (0..6).all(|key| (ranges[key].0..=ranges[key].1).contains(&p[key]))
        })
        .collect()
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// `n` colours of `field` plus a random offset of up to ±`spread` per
/// channel (never clamped: the fields keep every offset inside 0…255).
fn scattered(field: [u8; 3], spread: i32, n: usize, state: &mut u64) -> Vec<[u8; 3]> {
    (0..n)
        .map(|_| {
            let v = splitmix(state);
            let mut c = [0u8; 3];
            for (k, slot) in c.iter_mut().enumerate() {
                let d = ((v >> (16 * k)) % (2 * spread as u64 + 1)) as i32 - spread;
                *slot = (i32::from(field[k]) + d) as u8;
            }
            c
        })
        .collect()
}

/// One colour case: (references, probes, tolerance).
type ColourCase = (Vec<[u8; 3]>, Vec<[u8; 3]>, u8);

/// The colour cases ([`ColourCase`]).
/// - Twenty references, 18 of grey 100 and two of (100, 112, 100), t 9: the
///   trim drops both variants, so (100, 107, 100) is in (q1 moves 4 against
///   4) and (100, 110, 100) is out, where the full box admits it.
/// - Nine of grey 100 (no trim), t 15: (114, 100, 100) and (100, 100, 114)
///   are in, and (115, 100, 100) and (100, 85, 100), one channel exactly t
///   out, are out (q3, then q2, moves 8 against 7); the seven-projection box
///   admitted both.
/// - Fifty around (37, 180, 90), forty exact and ten within ± 12, t 11, and a
///   hundred around (200, 60, 140) within ± 6, t 24, each with eighty probes
///   within ± (t + 6).
fn colour_cases() -> Vec<ColourCase> {
    let mut state = 0x79_61;
    let mut twenty = vec![[100u8, 100, 100]; 20];
    twenty[4] = [100, 112, 100];
    twenty[13] = [100, 112, 100];
    let mut fifty = vec![[37u8, 180, 90]; 40];
    fifty.extend(scattered([37, 180, 90], 12, 10, &mut state));
    let fifty_probes = scattered([37, 180, 90], 17, 80, &mut state);
    let hundred = scattered([200, 60, 140], 6, 100, &mut state);
    let hundred_probes = scattered([200, 60, 140], 30, 80, &mut state);
    vec![
        (
            twenty,
            vec![
                [100, 107, 100],
                [100, 110, 100],
                [100, 100, 100],
                [100, 93, 100],
            ],
            9,
        ),
        (
            vec![[100, 100, 100]; 9],
            vec![
                [114, 100, 100],
                [115, 100, 100],
                [100, 100, 114],
                [100, 85, 100],
            ],
            15,
        ),
        (fifty, fifty_probes, 11),
        (hundred, hundred_probes, 24),
    ]
}

fn colour_scene(refs: &[[u8; 3]], probes: &[[u8; 3]]) -> (u32, Vec<[u8; 3]>, Vec<bool>) {
    let mut colours = refs.to_vec();
    colours.extend_from_slice(probes);
    colours.resize(colours.len().div_ceil(W as usize) * W as usize, probes[0]);
    let h = colours.len() as u32 / W;
    let selected = (0..colours.len()).map(|i| i < refs.len()).collect();
    (h, colours, selected)
}

/// Every kind but Gray: the byte RGB path, the typed 16-bit RGB path and a
/// CMYK document's naive RGB each trim per key, on the grey cases (where
/// the keys reduce to the grey, `oracle`) and on colour (`colour_oracle`).
#[test]
fn rgb_and_cmyk_documents_trim_a_tenth_of_each_key_too() {
    let dir = TempDir::new().unwrap();
    let grey: Vec<_> = cases()
        .into_iter()
        .map(|(refs, probes, t)| {
            let (h, values, selected) = scene(&refs, &probes);
            let want = oracle(&values, &selected, t, true);
            let colours: Vec<[u8; 3]> = values.iter().map(|v| [*v; 3]).collect();
            (h, colours, selected, t, want)
        })
        .collect();
    let colour: Vec<_> = colour_cases()
        .into_iter()
        .map(|(refs, probes, t)| {
            let (h, colours, selected) = colour_scene(&refs, &probes);
            let want = colour_oracle(&colours, &selected, t);
            (h, colours, selected, t, want)
        })
        .collect();
    for (n, (h, colours, selected, t, want)) in grey.into_iter().chain(colour).enumerate() {
        let byte = byte_rgb(&colours, h);
        assert_eq!(byte_similar(byte, &selected, t), want, "8-bit RGB case {n}");
        unsafe { dr_doc_free(byte) };
        let wide = rgb16(&colours, h);
        assert_eq!(typed(wide, &selected, t), want, "16-bit RGB case {n}");
        free(wide);
        let ink = cmyk(&dir, &format!("cmyk_{n}.tif"), h, &colours);
        assert_eq!(typed(ink, &selected, t), want, "CMYK case {n}");
        free(ink);
    }
    // The literal facts, on the byte path: the trimmed variants widen
    // nothing, and a single-channel step of exactly an odd t is refused.
    let (refs, probes, t) = colour_cases().remove(0);
    let (h, colours, selected) = colour_scene(&refs, &probes);
    let doc = byte_rgb(&colours, h);
    let got = byte_similar(doc, &selected, t);
    assert_eq!(&got[refs.len()..refs.len() + 4], [true, false, true, true]);
    unsafe { dr_doc_free(doc) };
    let (refs, probes, t) = colour_cases().remove(1);
    let (h, colours, selected) = colour_scene(&refs, &probes);
    let doc = byte_rgb(&colours, h);
    let got = byte_similar(doc, &selected, t);
    assert_eq!(&got[refs.len()..refs.len() + 4], [true, false, true, false]);
    unsafe { dr_doc_free(doc) };
}
