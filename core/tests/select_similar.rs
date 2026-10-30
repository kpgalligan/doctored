//! Select ▸ Grow / Similar through the C FFI: the byte export
//! `dr_doc_select_similar` and the typed `similar` kind of
//! `dr_precision_doc_select`. The oracle is the measured Photoshop 26.10.0
//! behaviour (2026-09-27) restated as literal expectations, plus the
//! Doctored conventions the measurement left open, commented as such.
use std::ptr;

use crate::native_scalar as native;
use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_image_from_rgba;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_jobs::*;
use doctored_core::ffi_precision_edit::dr_precision_doc_select;
use doctored_core::ffi_selection_tools::dr_doc_select_similar;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use native::*;
use serde_json::json;

const NO_REFERENCE: &str = "No pixel is more than half selected, so there is no colour to match.";

fn byte_doc(w: u32, h: u32, pixels: &[[u8; 4]]) -> *mut DrDocument {
    let raw: Vec<u8> = pixels.iter().flatten().copied().collect();
    let image = unsafe { dr_image_from_rgba(raw.as_ptr(), w, h) };
    assert!(!image.is_null());
    let doc = unsafe { dr_doc_from_image(image) };
    unsafe { doctored_core::ffi::dr_image_free(image) };
    assert!(!doc.is_null());
    doc
}

/// The byte result, or the refusal with the output checked untouched.
fn byte_similar(
    w: u32,
    h: u32,
    pixels: &[[u8; 4]],
    mask: &[u8],
    tolerance: u8,
    contiguous: bool,
) -> Result<Vec<u8>, String> {
    let doc = byte_doc(w, h, pixels);
    let mut out = vec![7u8; (w * h) as usize];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_doc_select_similar(
            doc,
            mask.as_ptr(),
            mask.len(),
            tolerance,
            contiguous,
            out.as_mut_ptr(),
            &mut e,
        )
    };
    unsafe { dr_doc_free(doc) };
    if ok {
        assert!(e.is_null());
        Ok(out)
    } else {
        assert!(out.iter().all(|&v| v == 7), "a refusal wrote its output");
        Err(unsafe { error(e) })
    }
}

fn typed_select(
    d: &PrecisionDocument,
    tolerance: u16,
    contiguous: bool,
    input: &[u16],
    control: *const DrJobControl,
) -> Result<Vec<u16>, String> {
    let request = json!({"op":"selection","operation":
        {"kind":"similar","tolerance":tolerance,"contiguous":contiguous}});
    let (w, h) = d.dimensions();
    let mut out = vec![999u16; (w * h) as usize];
    let t = text(request);
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_select(
            d,
            t.as_ptr(),
            input.as_ptr(),
            input.len(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            control,
            &mut e,
        )
    };
    if ok {
        Ok(out)
    } else {
        assert!(out.iter().all(|&v| v == 999), "a refusal wrote its output");
        Err(unsafe { error(e) })
    }
}

/// The u16 twin: pixels, coverage and tolerance all ×257.
fn typed_similar(
    w: u32,
    h: u32,
    pixels: &[[u8; 4]],
    mask: &[u8],
    tolerance: u8,
    contiguous: bool,
) -> Result<Vec<u16>, String> {
    let wide = pixels
        .iter()
        .map(|p| p.map(|c| u16::from(c) * 257))
        .collect();
    let d = doc(w, h, wide);
    let input: Vec<u16> = mask.iter().map(|&m| u16::from(m) * 257).collect();
    typed_select(
        &d,
        u16::from(tolerance) * 257,
        contiguous,
        &input,
        ptr::null(),
    )
}

/// Every case runs at both depths; the u16 result must equal the byte
/// result ×257.
fn both(
    w: u32,
    h: u32,
    pixels: &[[u8; 4]],
    mask: &[u8],
    tolerance: u8,
    contiguous: bool,
) -> Vec<u8> {
    let bytes = byte_similar(w, h, pixels, mask, tolerance, contiguous).unwrap();
    let wide = typed_similar(w, h, pixels, mask, tolerance, contiguous).unwrap();
    let scaled: Vec<u16> = bytes.iter().map(|&b| u16::from(b) * 257).collect();
    assert_eq!(wide, scaled, "the u16 twin differs from the byte result");
    bytes
}

fn grey(v: u8) -> [u8; 4] {
    [v, v, v, 255]
}

#[test]
fn red_and_blue_references_span_magenta_and_black_but_not_grey() {
    let pixels = [
        [255, 0, 0, 255],
        [0, 0, 255, 255],
        [255, 0, 255, 255],
        [0, 0, 0, 255],
        [100, 0, 100, 255],
        grey(128),
    ];
    let mask = [255, 255, 0, 0, 0, 0];
    assert_eq!(
        both(6, 1, &pixels, &mask, 0, false),
        [255, 255, 255, 255, 255, 0]
    );
}

#[test]
fn a_grey_reference_admits_plus_minus_tolerance_and_refuses_one_more() {
    let pixels = [grey(100), grey(90), grey(110), grey(89), grey(111)];
    let mask = [255, 0, 0, 0, 0];
    assert_eq!(both(5, 1, &pixels, &mask, 10, false), [255, 255, 255, 0, 0]);
}

#[test]
fn a_hue_shift_counts_double_through_the_difference_projections() {
    let pixels = [grey(100), [105, 95, 100, 255], [106, 94, 100, 255]];
    assert_eq!(both(3, 1, &pixels, &[255, 0, 0], 10, false), [255, 255, 0]);
}

#[test]
fn two_grey_references_admit_greys_between_but_not_a_tinted_pixel() {
    let pixels = [grey(100), grey(120), [110, 100, 120, 255], grey(110)];
    assert_eq!(
        both(4, 1, &pixels, &[255, 255, 0, 0], 0, false),
        [255, 255, 0, 255]
    );
}

#[test]
fn grow_is_four_connected_and_similar_is_global() {
    let a = grey(50);
    let b = grey(200);
    // A 3x3 grid: the reference at (0,0) and a matching pixel at (1,1)
    // that touches it only diagonally.
    let grid = [a, b, b, b, a, b, b, b, b];
    let mut mask = [0u8; 9];
    mask[0] = 255;
    let mut only_seed = [0u8; 9];
    only_seed[0] = 255;
    assert_eq!(both(3, 3, &grid, &mask, 0, true), only_seed);
    let mut global = only_seed;
    global[4] = 255;
    assert_eq!(both(3, 3, &grid, &mask, 0, false), global);

    let row = [a, a, b, a, a];
    let seed = [255, 0, 0, 0, 0];
    assert_eq!(both(5, 1, &row, &seed, 0, true), [255, 255, 0, 0, 0]);
    assert_eq!(both(5, 1, &row, &seed, 0, false), [255, 255, 0, 255, 255]);
}

#[test]
fn a_reference_is_at_least_half_selected() {
    // Doctored convention, not measured: 128 of 255 is a reference, 127 is
    // not (u16: 32896 and 32639 against 32768).
    let pixels = [grey(100), grey(200)];
    assert_eq!(both(2, 1, &pixels, &[128, 0], 0, false), [255, 0]);
    assert_eq!(
        byte_similar(2, 1, &pixels, &[127, 0], 0, false).unwrap_err(),
        NO_REFERENCE
    );
    assert_eq!(
        typed_similar(2, 1, &pixels, &[127, 0], 0, false).unwrap_err(),
        NO_REFERENCE
    );
}

#[test]
fn alpha_is_one_more_projection() {
    // Doctored convention, not measured: alpha is boxed like R, G and B.
    let pixels = [
        [100, 100, 100, 255],
        [100, 100, 100, 250],
        [100, 100, 100, 200],
    ];
    assert_eq!(both(3, 1, &pixels, &[255, 0, 0], 10, false), [255, 255, 0]);
}

#[test]
fn a_partly_selected_pixel_in_range_comes_back_full() {
    // Doctored convention: the result is the admitted region at full
    // coverage, so a partly covered in-range pixel is promoted.
    let pixels = [grey(100), grey(100), grey(200)];
    assert_eq!(both(3, 1, &pixels, &[255, 64, 0], 0, false), [255, 255, 0]);
}

#[test]
fn a_second_call_on_continuous_tone_grows_further() {
    // By design: each call rebuilds the range from the
    // selection the previous call widened.
    let pixels: Vec<[u8; 4]> = (0..6).map(|i| grey(i * 10)).collect();
    let first = both(6, 1, &pixels, &[255, 0, 0, 0, 0, 0], 10, false);
    assert_eq!(first, [255, 255, 0, 0, 0, 0]);
    let second = both(6, 1, &pixels, &first, 10, false);
    assert_eq!(second, [255, 255, 255, 0, 0, 0]);
}

#[test]
fn refusals_leave_the_output_untouched() {
    let pixels = [grey(100), grey(200)];
    assert_eq!(
        byte_similar(2, 1, &pixels, &[0, 0], 32, true).unwrap_err(),
        NO_REFERENCE
    );
    assert_eq!(
        typed_similar(2, 1, &pixels, &[0, 0], 32, true).unwrap_err(),
        NO_REFERENCE
    );
    assert_eq!(
        byte_similar(2, 1, &pixels, &[255], 32, false).unwrap_err(),
        "selection mask length does not match the canvas"
    );

    let doc = byte_doc(2, 1, &pixels);
    let mask = [255u8, 0];
    let mut out = [7u8; 2];
    let mut e = ptr::null_mut();
    unsafe {
        assert!(!dr_doc_select_similar(
            ptr::null(),
            mask.as_ptr(),
            2,
            0,
            false,
            out.as_mut_ptr(),
            &mut e
        ));
        assert!(!dr_doc_select_similar(
            doc,
            ptr::null(),
            2,
            0,
            false,
            out.as_mut_ptr(),
            &mut e
        ));
        assert!(!dr_doc_select_similar(
            doc,
            mask.as_ptr(),
            2,
            0,
            false,
            ptr::null_mut(),
            &mut e
        ));
        assert!(!dr_doc_select_similar(
            ptr::null(),
            ptr::null(),
            0,
            0,
            false,
            ptr::null_mut(),
            ptr::null_mut()
        ));
        dr_doc_free(doc);
    }
    assert!(e.is_null(), "a NULL refusal set err_out");
    assert_eq!(out, [7, 7]);
}

#[test]
fn the_typed_kind_plans_and_requires_its_input() {
    let d = doc(2, 1, vec![[25700; 4], [51400; 4]]);
    let p = plan(
        &d,
        json!({"op":"selection","operation":{"kind":"similar","tolerance":0,"contiguous":false}}),
    );
    assert_eq!(p["input_samples"], 2);
    let e = typed_select(&d, 0, false, &[], ptr::null()).unwrap_err();
    assert!(e.contains("differs from the core plan"), "{e}");

    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let result = typed_select(&d, 0, false, &[65535, 0], control);
    unsafe { dr_job_control_free(control) };
    let e = result.unwrap_err();
    assert!(e.contains("cancel"), "{e}");

    // Both fields are required.
    let t = text(json!({"op":"selection","operation":{"kind":"similar","tolerance":0}}));
    let mut out = [999u16; 2];
    let mut err = ptr::null_mut();
    let input = [65535u16, 0];
    assert!(!unsafe {
        dr_precision_doc_select(
            &d,
            t.as_ptr(),
            input.as_ptr(),
            2,
            out.as_mut_ptr(),
            2,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    });
    assert!(!unsafe { error(err) }.is_empty());
    assert_eq!(out, [999, 999]);
}
