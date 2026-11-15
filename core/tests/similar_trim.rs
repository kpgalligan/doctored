//! Select ▸ Similar and Grow's reference range by document kind, through the
//! FFI (`selection_similar`'s module doc): a Grayscale document drops ⌊N/10⌋
//! of the N references' values from each end before the tolerance widens the
//! range, at 8 and 16 bits; an RGB document (the byte path at 8 bits, the
//! typed path at 16) and a CMYK document keep every reference's value. The
//! oracle is a sort-based trim written here, never the core's histograms.
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

fn byte_rgb(values: &[u8], h: u32) -> *mut DrDocument {
    let raw: Vec<u8> = values.iter().flat_map(|v| [*v, *v, *v, 255]).collect();
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

fn cmyk_grey(dir: &TempDir, name: &str, h: u32, values: &[u8]) -> *mut PrecisionDocument {
    // Ink 255 − v on every plate: the naive RGB is (255 − ink)·(255 − 0)/255
    // on C, M and Y with K 0, so each pixel reads as the grey v.
    let data: Vec<u8> = values
        .iter()
        .flat_map(|v| [255 - v, 255 - v, 255 - v, 0])
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

fn rgb16(values: &[u8], h: u32) -> *mut PrecisionDocument {
    let pixels = values
        .iter()
        .map(|v| {
            let c = u16::from(*v) * 257;
            [c, c, c, 65535]
        })
        .collect();
    Box::into_raw(Box::new(crate::native_scalar::doc(W, h, pixels)))
}

#[test]
fn rgb_and_cmyk_documents_keep_every_references_value() {
    let dir = TempDir::new().unwrap();
    for (n, (refs, probes, t)) in cases().into_iter().enumerate() {
        let (h, values, selected) = scene(&refs, &probes);
        let want = oracle(&values, &selected, t, false);
        let byte = byte_rgb(&values, h);
        assert_eq!(byte_similar(byte, &selected, t), want, "8-bit RGB case {n}");
        unsafe { dr_doc_free(byte) };
        let wide = rgb16(&values, h);
        assert_eq!(typed(wide, &selected, t), want, "16-bit RGB case {n}");
        free(wide);
        let cmyk = cmyk_grey(&dir, &format!("cmyk_{n}.tif"), h, &values);
        assert_eq!(typed(cmyk, &selected, t), want, "CMYK case {n}");
        free(cmyk);
    }
}
