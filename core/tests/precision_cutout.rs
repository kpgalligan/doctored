//! Native source-alpha/matte separation and immutable cutout FFI qualification.
use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use doctored_core::{
    doc_mask::MaskProperties,
    ffi::dr_string_free,
    ffi_precision::dr_precision_doc_free,
    ffi_precision_cutout::{
        dr_precision_doc_add_refinement_cutout, dr_precision_doc_refinement_cutout_plan,
    },
    icc::IccProfile,
    job_control::DrJobControl,
    metadata::{Metadata, Resolution},
    precision_document::{PrecisionDocument, PrecisionDocumentLayer},
    precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES},
    precision_mask::{PrecisionMask, PrecisionPlane},
    precision_mask_edit::MaskEdit,
};
use std::{ffi::CStr, ptr, sync::Arc};

fn fixture() -> (PrecisionDocument, PrecisionImage, Vec<u16>) {
    let profile = IccProfile::srgb();
    let original =
        PrecisionImage::from_rgba16(3, 2, vec![[1003, 2005, 3007, 1]; 6], profile.clone(), None)
            .unwrap();
    let root = PrecisionDocumentLayer::isolated_normal(
        "Original group",
        vec![
            PrecisionDocumentLayer::raster("Original pixels", original, (0, 0), 0.7, true).unwrap(),
        ],
        0.8,
        true,
    )
    .unwrap()
    .with_mask(Some(
        PrecisionMask::new(
            PrecisionPlane::from_u16(2, 1, vec![7, 32769], None).unwrap(),
            MaskProperties::at((-1, 0)),
            true,
        )
        .unwrap(),
    ));
    let doc = PrecisionDocument::from_parts(
        3,
        2,
        profile,
        Metadata::default(),
        Resolution::default(),
        vec![root],
    )
    .unwrap();
    // An independently captured equal-byte profile must rebind without color conversion.
    let independently_captured_profile =
        Arc::new(IccProfile::parse(doc.profile().bytes()).unwrap());
    assert!(!Arc::ptr_eq(&independently_captured_profile, doc.profile()));
    let foreground = PrecisionImage::from_rgba16(
        3,
        2,
        vec![
            [101, 202, 303, 1],
            [1101, 2202, 3303, 7],
            [11001, 22002, 33003, 32769],
            [4001, 5003, 6005, 65535],
            [65001, 55003, 45005, 0],
            [101, 65534, 513, 65535],
        ],
        independently_captured_profile,
        None,
    )
    .unwrap();
    (doc, foreground, vec![1, 257, 32771, 65534, 123, 65535])
}

#[test]
fn ffi_retains_native_source_alpha_and_independent_matte_through_reopen() {
    unsafe {
        let (doc, foreground, matte) = fixture();
        let mut error = ptr::null_mut();
        let plan = dr_precision_doc_refinement_cutout_plan(&doc, &foreground, &mut error);
        assert!(error.is_null() && !plan.is_null());
        let plan_value: serde_json::Value =
            serde_json::from_str(CStr::from_ptr(plan).to_str().unwrap()).unwrap();
        dr_string_free(plan);
        assert_eq!(plan_value["borrowed_matte_bytes"], 12);
        let output = dr_precision_doc_add_refinement_cutout(
            &doc,
            &foreground,
            matte.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        );
        assert!(error.is_null() && !output.is_null());
        let result = &*output;
        assert_eq!(result.layer_count(), doc.layer_count() + 1);
        assert!(!result.root_layers()[0].visible());
        assert!(result.root_layers()[0].children().unwrap()[0].visible());
        assert!(doc.root_layers()[0].visible());
        let added = result.layer(result.layer_count() - 1).unwrap();
        assert_eq!(
            added.source().unwrap().to_rgba16().ok(),
            foreground.to_rgba16().ok()
        );
        assert!(PrecisionImage::ptr_eq(added.source().unwrap(), &foreground));
        assert!(Arc::ptr_eq(
            added.source().unwrap().profile(),
            result.profile()
        ));
        assert_eq!(added.mask().unwrap().plane().samples(), matte);
        assert_ne!(
            added.mask().unwrap().plane().samples().as_ptr(),
            matte.as_ptr()
        );
        assert_eq!(
            result.layer_source(0).unwrap().to_rgba16().ok(),
            doc.layer_source(0).unwrap().to_rgba16().ok()
        );
        assert_eq!(
            result.root_layers()[0].mask().unwrap().plane().samples(),
            &[7, 32769]
        );
        let encoded = result
            .encode_native(REFERENCE_WORKING_BYTES, 0, None)
            .unwrap();
        let reopened =
            PrecisionDocument::parse_native(&encoded, REFERENCE_WORKING_BYTES, 0, None).unwrap();
        let last = reopened.layer(reopened.layer_count() - 1).unwrap();
        assert_eq!(
            last.source().unwrap().to_rgba16().ok(),
            foreground.to_rgba16().ok()
        );
        assert_eq!(last.mask().unwrap().plane().samples(), matte);
        assert_eq!(last.source().unwrap().to_rgba16().ok().unwrap()[0][3], 1);
        assert_eq!(last.mask().unwrap().plane().samples()[0], 1);
        assert!(!reopened.root_layers()[0].visible());
        dr_precision_doc_free(output);
    }
}

#[test]
fn changing_only_mask_reveals_tiny_source_alpha_and_preserves_foreground_codes() {
    let (doc, foreground, matte) = fixture();
    let cutout = doc
        .add_refinement_cutout(&foreground, &matte, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap();
    let index = cutout.layer_count() - 1;
    let full = vec![65535; 6];
    let mask = MaskEdit::SetMask {
        layer: index,
        width: 3,
        height: 2,
        x: 0,
        y: 0,
        linked: false,
        density: 1.0,
        feather: 0.0,
        enabled: true,
        outside_coverage: 0,
    };
    let unmasked = cutout
        .mask_edit(&mask, &full, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap();
    let image = unmasked.render(REFERENCE_WORKING_BYTES, 0, None).unwrap();
    assert_eq!(image.to_rgba16().ok().unwrap()[0], [101, 202, 303, 1]);
    assert_eq!(
        unmasked.layer_source(index).unwrap().to_rgba16().ok(),
        foreground.to_rgba16().ok()
    );
    assert_eq!(
        cutout
            .layer(index)
            .unwrap()
            .mask()
            .unwrap()
            .plane()
            .samples(),
        matte
    );
    let shifted = unmasked
        .mask_edit(
            &MaskEdit::MaskProperties {
                layer: index,
                x: 1,
                y: -1,
                linked: false,
                density: 1.0,
                feather: 0.0,
                enabled: true,
            },
            &[],
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        shifted.layer_source(index).unwrap().to_rgba16().ok(),
        foreground.to_rgba16().ok()
    );
    assert_eq!(
        shifted
            .layer(index)
            .unwrap()
            .mask()
            .unwrap()
            .properties()
            .offset,
        (1, -1)
    );
    let reusable = shifted
        .mask_edit(
            &MaskEdit::SetChannel {
                index: None,
                name: "Refinement coverage".into(),
            },
            &matte,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(reusable.channels()[0].plane().samples(), matte);
    let reopened = PrecisionDocument::parse_native(
        &reusable
            .encode_native(REFERENCE_WORKING_BYTES, 0, None)
            .unwrap(),
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .unwrap();
    assert_eq!(reopened.channels()[0].plane().samples(), matte);
}

/// Rule C: in a 32-bit float document the foreground is the float
/// composite, every bit kept (8.0 stays 8.0), and the matte is its mask; a
/// foreground in the other storage refuses at either depth.
#[test]
fn a_float_document_keeps_its_float_foreground_bits_and_the_matte_as_mask() {
    unsafe {
        let pixels = vec![
            [8.0_f32, 0.5, -0.25, 1.0],
            [0.18, 2.0, 1.0, 1.0],
            [1.0, 1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0, 1.0],
            [4.0, 4.0, 4.0, 0.5],
            [0.3, 0.2, 0.1, 1.0],
        ];
        let doc = crate::float_fixture::float_doc(
            3,
            2,
            &IccProfile::srgb(),
            std::slice::from_ref(&pixels),
        );
        let foreground = doc.layer_source(0).unwrap();
        let matte = vec![0_u16, 257, 65535, 32768, 1, 65534];
        let mut error = ptr::null_mut();
        let plan = dr_precision_doc_refinement_cutout_plan(&doc, foreground, &mut error);
        assert!(error.is_null() && !plan.is_null());
        let plan_value: serde_json::Value =
            serde_json::from_str(CStr::from_ptr(plan).to_str().unwrap()).unwrap();
        dr_string_free(plan);
        assert_eq!(plan_value["foreground_storage"], "rgba32_float");
        let output = dr_precision_doc_add_refinement_cutout(
            &doc,
            foreground,
            matte.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        );
        assert!(error.is_null() && !output.is_null());
        let result = &*output;
        assert!(result.is_float());
        assert_eq!(result.layer_count(), 2);
        assert!(!result.root_layers()[0].visible());
        let added = result.layer(1).unwrap();
        let source = added.source().unwrap();
        assert_eq!(source.storage(), PrecisionStorage::Rgba32Float);
        let bits = |p: &[[f32; 4]]| p.iter().map(|v| v.map(f32::to_bits)).collect::<Vec<_>>();
        assert_eq!(bits(&source.to_rgba32().unwrap()), bits(&pixels));
        assert_eq!(source.to_rgba32().unwrap()[0][0], 8.0);
        assert_eq!(added.mask().unwrap().plane().samples(), matte);
        dr_precision_doc_free(output);

        // A 16-bit foreground in a float document, and the float composite in
        // a 16-bit document, refuse.
        let (wide, wide_foreground, _) = fixture();
        assert!(doc.refinement_cutout_plan(&wide_foreground).is_err());
        assert!(doc
            .add_refinement_cutout(&wide_foreground, &matte, REFERENCE_WORKING_BYTES, 0, None)
            .is_err());
        assert!(wide.refinement_cutout_plan(foreground).is_err());
        assert_eq!(
            wide.refinement_cutout_plan(&wide_foreground).unwrap()["foreground_storage"],
            "rgba16_unorm"
        );
    }
}

#[test]
fn ffi_noop_bad_inputs_budget_and_cancel_preserve_owners() {
    unsafe {
        let (doc, foreground, matte) = fixture();
        let mut error = ptr::null_mut();
        let empty = [0; 6];
        let result = dr_precision_doc_add_refinement_cutout(
            &doc,
            &foreground,
            empty.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        );
        assert!(result.is_null() && error.is_null());
        assert!(doc
            .add_refinement_cutout(&foreground, &[], REFERENCE_WORKING_BYTES, 0, None)
            .is_err());
        let other_profile = PrecisionImage::from_rgba16(
            3,
            2,
            vec![[0, 0, 0, 65535]; 6],
            IccProfile::display_p3(),
            None,
        )
        .unwrap();
        assert!(doc.refinement_cutout_plan(&other_profile).is_err());
        let token = DrJobControl::default();
        token.cancel();
        for (limit, control) in [
            (1, ptr::null()),
            (REFERENCE_WORKING_BYTES, &token as *const _),
        ] {
            let result = dr_precision_doc_add_refinement_cutout(
                &doc,
                &foreground,
                matte.as_ptr(),
                limit,
                0,
                control,
                &mut error,
            );
            assert!(result.is_null() && !error.is_null());
            dr_string_free(error);
            error = ptr::null_mut();
        }
        assert_eq!(token.scratch_bytes(), 0);
        assert!(doc.root_layers()[0].visible());
        assert_eq!(foreground.to_rgba16().ok().unwrap()[0][3], 1);
    }
}

#[test]
fn live_native_assembly_cancellation_releases_the_mask_copy_without_publication() {
    let (w, h) = (2048, 2048);
    let foreground = PrecisionImage::from_rgba16(
        w,
        h,
        vec![[101, 202, 303, 1]; w as usize * h as usize],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let document = PrecisionDocument::from_image(
        foreground.clone(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let matte = vec![1; w as usize * h as usize];
    let render = |token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match document.add_refinement_cutout(
            &foreground,
            &matte,
            REFERENCE_WORKING_BYTES,
            0,
            Some(control),
        ) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    };
    // The entry is `begin()`'s report; the op's first `reserve` is the next
    // checkpoint, so k = 1 parks on the first checkpoint after it stores.
    let (entry, _) = entry_then_cancel(&|_, stage, _| stage == 1, render);
    let parked = cancelled_parked_at(entry.checkpoints + 2, &|_, stage| stage == 1, render);
    eprintln!("native assembly cancellation: {}", parked_line(&parked));
    assert!(parked.scratch > 0);
    assert!(document.root_layers()[0].visible());
    assert_eq!(foreground.to_rgba16().ok().unwrap()[0], [101, 202, 303, 1]);
}

mod bench {
    use super::*;

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn live_native_assembly_cancellation_releases_the_mask_copy_without_publication() {
        use std::time::{Duration, Instant};
        let (w, h) = (2048, 2048);
        let foreground = PrecisionImage::from_rgba16(
            w,
            h,
            vec![[101, 202, 303, 1]; w as usize * h as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap();
        let document = PrecisionDocument::from_image(
            foreground.clone(),
            Metadata::default(),
            Resolution::default(),
        )
        .unwrap();
        let matte = vec![1; w as usize * h as usize];
        let token = DrJobControl::default();
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                document.add_refinement_cutout(
                    &foreground,
                    &matte,
                    REFERENCE_WORKING_BYTES,
                    0,
                    Some(&token),
                )
            });
            let deadline = Instant::now() + Duration::from_secs(10);
            while token.scratch_bytes() == 0 {
                assert!(Instant::now() < deadline, "native assembly did not start");
                std::thread::sleep(Duration::from_millis(1));
            }
            let start = Instant::now();
            token.cancel();
            let error = worker.join().unwrap().err().unwrap();
            assert!(error.contains("cancelled"));
            assert!(start.elapsed() < Duration::from_millis(500));
        });
        assert_eq!(token.scratch_bytes(), 0);
        assert!(document.root_layers()[0].visible());
        assert_eq!(foreground.to_rgba16().ok().unwrap()[0], [101, 202, 303, 1]);
    }
}
