//! Phase 13B-3a: the typed accessor contract on `PrecisionImage` — the owned
//! `to_rgba16` / `to_rgba32` copies (since 13B-3e-4c-1 there is no public
//! whole-plane view; the one crate-private view read is pinned by
//! `preview_view_exception.rs`), allocation identity (`ptr_eq`) and
//! allocation-free sample equality (`same_samples`). Every oracle is built
//! independently of the accessor: a patterned `Vec` from a closure, captured
//! before the constructor adopts it.
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_jobs::dr_job_control_set_scratch_limit;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_paint::PaintEdit;
use serde_json::json;

const W: u32 = 7;
const H: u32 = 5;

fn pattern16(width: u32, height: u32) -> Vec<[u16; 4]> {
    (0..width * height)
        .map(|i| {
            let i = i as u16;
            [
                i.wrapping_mul(977),
                65535 - i.wrapping_mul(131),
                i ^ 0x5a5a,
                if i.is_multiple_of(3) { 0 } else { 30000 + i },
            ]
        })
        .collect()
}
fn pattern32(width: u32, height: u32) -> Vec<[f32; 4]> {
    (0..width * height)
        .map(|i| {
            let v = i as f32 / (width * height) as f32;
            [
                v,
                1.0 - v,
                v * 0.5,
                if i.is_multiple_of(2) { 1.0 } else { v },
            ]
        })
        .collect()
}
fn image16(pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap()
}
fn image32(pixels: Vec<[f32; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba32(W, H, pixels, IccProfile::srgb(), None).unwrap()
}

#[test]
fn repeated_owned_copies_equal_the_samples_and_refuse_the_other_storage() {
    let source = pattern16(W, H);
    let expected = source.clone();
    let image = image16(source);
    assert_eq!(image.dimensions(), (W, H));
    assert_eq!(image.storage(), PrecisionStorage::Rgba16Unorm);
    let first = image.to_rgba16().unwrap();
    let second = image.to_rgba16().unwrap();
    assert_eq!(first, expected);
    assert_eq!(second, expected);
    assert_eq!(
        image.to_rgba32().unwrap_err(),
        "precision image storage is not RGBA32 float"
    );

    let source = pattern32(W, H);
    let expected = source.clone();
    let float = image32(source);
    assert_eq!(float.storage(), PrecisionStorage::Rgba32Float);
    let first = float.to_rgba32().unwrap();
    let second = float.to_rgba32().unwrap();
    assert_eq!(first, expected);
    assert_eq!(second, expected);
    assert_eq!(
        float.to_rgba16().unwrap_err(),
        "precision image storage is not RGBA16"
    );
}

#[test]
fn owned_copies_equal_the_samples_in_a_new_allocation_and_refuse_the_other_storage() {
    let expected = pattern16(W, H);
    let image = image16(expected.clone());
    let copy = image.to_rgba16().unwrap();
    assert_eq!(copy, expected);
    assert_eq!(
        image.to_rgba32().unwrap_err(),
        "precision image storage is not RGBA32 float"
    );

    let expected = pattern32(W, H);
    let float = image32(expected.clone());
    let copy = float.to_rgba32().unwrap();
    assert_eq!(copy.len(), expected.len());
    for (a, b) in copy.iter().zip(expected.iter()) {
        assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits));
    }
    assert_eq!(
        float.to_rgba16().unwrap_err(),
        "precision image storage is not RGBA16"
    );
}

#[test]
fn ptr_eq_is_allocation_identity_with_storage_in_the_answer() {
    let image = image16(pattern16(W, H));
    let clone = image.clone();
    assert!(PrecisionImage::ptr_eq(&image, &clone));
    assert!(PrecisionImage::ptr_eq(&image, &image));
    // Equal samples in a second allocation are NOT the same image.
    let twin = image16(pattern16(W, H));
    assert!(!PrecisionImage::ptr_eq(&image, &twin));
    let float = image32(pattern32(W, H));
    assert!(!PrecisionImage::ptr_eq(&image, &float));
    assert!(!PrecisionImage::ptr_eq(&float, &image));
    assert!(PrecisionImage::ptr_eq(&float, &float.clone()));
    // A same-storage conversion shares the handle, so it is the same image.
    let reinterpreted = image
        .convert(
            PrecisionStorage::Rgba16Unorm,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap();
    assert!(PrecisionImage::ptr_eq(&image, &reinterpreted));
}

#[test]
fn same_samples_compares_samples_without_identity_and_f32_bitwise() {
    let image = image16(pattern16(W, H));
    let twin = image16(pattern16(W, H));
    assert!(!PrecisionImage::ptr_eq(&image, &twin));
    assert!(image.same_samples(&twin));
    assert!(twin.same_samples(&image));
    assert!(image.same_samples(&image.clone()));

    let mut changed = pattern16(W, H);
    changed[W as usize * H as usize - 1][2] ^= 1;
    assert!(!image.same_samples(&image16(changed)));

    // Same sample count, other dimensions.
    let transposed =
        PrecisionImage::from_rgba16(H, W, pattern16(W, H), IccProfile::srgb(), None).unwrap();
    assert!(!image.same_samples(&transposed));

    // Storage mismatch, even where the values coincide at 0 and 1.
    let zeros16 =
        PrecisionImage::from_rgba16(1, 1, vec![[0; 4]], IccProfile::srgb(), None).unwrap();
    let zeros32 =
        PrecisionImage::from_rgba32(1, 1, vec![[0.0; 4]], IccProfile::srgb(), None).unwrap();
    assert!(!zeros16.same_samples(&zeros32));
    assert!(!zeros32.same_samples(&zeros16));

    // f32 is compared by BITS: -0.0 is not 0.0.
    let positive =
        PrecisionImage::from_rgba32(1, 1, vec![[0.0, 0.5, 0.25, 1.0]], IccProfile::srgb(), None)
            .unwrap();
    let negative =
        PrecisionImage::from_rgba32(1, 1, vec![[-0.0, 0.5, 0.25, 1.0]], IccProfile::srgb(), None)
            .unwrap();
    assert!(!positive.same_samples(&negative));
    let equal =
        PrecisionImage::from_rgba32(1, 1, vec![[0.0, 0.5, 0.25, 1.0]], IccProfile::srgb(), None)
            .unwrap();
    assert!(positive.same_samples(&equal));
    let float = image32(pattern32(W, H));
    assert!(float.same_samples(&float.clone()));
    assert!(float.same_samples(&image32(pattern32(W, H))));
}

#[test]
fn float_constructors_keep_their_refusals_and_their_admission_order() {
    for bad in [f32::NAN, f32::INFINITY, -0.25, 1.5] {
        let error =
            PrecisionImage::from_rgba32(1, 1, vec![[bad, 0.0, 0.0, 1.0]], IccProfile::srgb(), None)
                .err()
                .unwrap();
        assert_eq!(
            error,
            "precision float components must be finite and normalized to [0,1]"
        );
    }
    let extended = PrecisionImage::from_rgba32_extended(
        1,
        1,
        vec![[2.0, -1.0, 0.5, 1.0]],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    assert_eq!(extended.to_rgba32().ok().unwrap()[0], [2.0, -1.0, 0.5, 1.0]);
    let error = PrecisionImage::from_rgba32_extended(
        1,
        1,
        vec![[f32::NAN, 0.0, 0.0, 1.0]],
        IccProfile::srgb(),
        None,
    )
    .err()
    .unwrap();
    assert_eq!(
        error,
        "32-bit float samples must be finite within ±65504 with alpha in [0,1]"
    );

    // Order oracle: the reservation precedes the sample validation, so a NaN
    // buffer under a budget below the 1 MiB bookkeeping reservation answers
    // the RESERVATION refusal, never the finite-sample sentence.
    for extended in [false, true] {
        let control = DrJobControl::default();
        assert!(unsafe { dr_job_control_set_scratch_limit(&control, 1024 * 1024 - 1) });
        let pixels = vec![[f32::NAN, 0.0, 0.0, 1.0]];
        let result = if extended {
            PrecisionImage::from_rgba32_extended(1, 1, pixels, IccProfile::srgb(), Some(&control))
        } else {
            PrecisionImage::from_rgba32(1, 1, pixels, IccProfile::srgb(), Some(&control))
        };
        let error = result.err().unwrap();
        assert!(error.contains("working memory"), "{error}");
        assert!(!error.contains("finite"), "{error}");
    }
}

fn take_error(error: *mut c_char) -> Option<String> {
    if error.is_null() {
        return None;
    }
    unsafe {
        let text = CStr::from_ptr(error).to_string_lossy().into_owned();
        dr_string_free(error);
        Some(text)
    }
}

#[test]
fn document_edits_keep_identity_for_structure_and_mint_it_for_pixels() {
    let image = image16(pattern16(W, H));
    let document =
        PrecisionDocument::from_image(image.clone(), Metadata::default(), Resolution::default())
            .unwrap();
    assert!(PrecisionImage::ptr_eq(
        document.layer_source(0).unwrap(),
        &image
    ));

    let request =
        CString::new(json!({"op":"rename_layer","layer":0,"name":"Renamed"}).to_string()).unwrap();
    let mut error = ptr::null_mut();
    let renamed = unsafe {
        dr_precision_doc_edit(
            &document,
            request.as_ptr(),
            ptr::null(),
            0,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert_eq!(take_error(error), None);
    assert!(!renamed.is_null());
    let renamed = unsafe { Box::from_raw(renamed) };
    assert_eq!(renamed.layer(0).unwrap().name(), "Renamed");
    assert!(PrecisionImage::ptr_eq(
        renamed.layer_source(0).unwrap(),
        document.layer_source(0).unwrap()
    ));

    let erase = PaintEdit::parse("{\"op\":\"erase\",\"layer\":0,\"opacity\":1.0}").unwrap();
    let coverage = vec![65535_u16; (W * H) as usize];
    let painted = document
        .paint(&erase, &coverage, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap();
    let after = painted.layer_source(0).unwrap();
    assert!(!PrecisionImage::ptr_eq(
        after,
        document.layer_source(0).unwrap()
    ));
    assert!(!after.same_samples(document.layer_source(0).unwrap()));
    assert!(after.to_rgba16().ok().unwrap().iter().all(|p| p[3] == 0));
}

#[test]
fn an_owned_copy_does_not_change_retained_bytes() {
    let image = image16(pattern16(W, H));
    let figure = image.retained_bytes();
    let reference = image16(pattern16(W, H)).retained_bytes();
    assert_eq!(figure, reference);
    let copy = image.to_rgba16().ok().unwrap();
    assert_eq!(image.retained_bytes(), figure);
    drop(copy);
    assert_eq!(image.retained_bytes(), figure);

    let float = image32(pattern32(W, H));
    let figure = float.retained_bytes();
    let copy = float.to_rgba32().ok().unwrap();
    assert_eq!(float.retained_bytes(), figure);
    drop(copy);
    assert_eq!(float.retained_bytes(), figure);
}
