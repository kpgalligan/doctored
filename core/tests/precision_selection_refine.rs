//! Native u16 Refine Selection qualification through the additive
//! `dr_selection_u16` contract, plus the typed cutout the refined coverage
//! feeds. Stage composition is checked against the standalone selection kinds
//! the refinement is built from, and the depth receipt is a contrast pair whose
//! distinct u16 outputs share one displayed byte — contrast 1 thresholds at the
//! native midpoint 32768, which is exactly the byte-128 boundary (257*128 =
//! 32896 projects from 32768 through (v+128)/257), so it proves nothing at all
//! about precision and is only checked as a plain threshold.
use crate::native_scalar as native;

use doctored_core::ffi_selection_u16::{dr_selection_u16, dr_selection_u16_plan};
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask_edit::MaskEdit;
use native::{edit, q, selection, LIMIT};
use serde_json::{json, Value};
use std::ptr;

/// The one contrast expression, written here as the independent oracle in the
/// exact f64 order the contract pins: normalize, widen, clamp, quantize.
fn contrast(value: u16, amount: f64) -> u16 {
    if amount == 0. {
        return value;
    }
    if amount == 1. {
        return if value >= 32768 { 65535 } else { 0 };
    }
    q(((f64::from(value) / 65535. - 0.5) / (1. - amount) + 0.5).clamp(0., 1.) * 65535.)
}

fn byte(value: u16) -> u8 {
    ((u32::from(value) + 128) / 257) as u8
}

/// The 9G 7x5 coprime pattern: every code distinct, no byte-aligned values.
fn pattern() -> Vec<u16> {
    (0..35).map(|i| ((i * 1877 + 1) % 65536) as u16).collect()
}

fn refuse(request: Value) -> String {
    let text = native::text(request);
    let mut error = ptr::null_mut();
    let plan = unsafe { dr_selection_u16_plan(7, 5, text.as_ptr(), LIMIT, 0, &mut error) };
    assert!(plan.is_null());
    let message = unsafe { native::error(error) };
    assert!(!message.is_empty());
    message
}

#[test]
fn zero_controls_copy_every_native_code_exactly() {
    let values = [1, 3, 257, 32769, 65534, 65535, 0];
    assert_eq!(selection(7, 1, json!({"kind":"refine"}), &values), values);
    assert_eq!(
        selection(
            7,
            1,
            json!({"kind":"refine","smooth":0,"feather":0,"shift":0,"contrast":0}),
            &values
        ),
        values
    );
}

#[test]
fn contrast_thresholds_at_the_native_midpoint_and_separates_codes_inside_one_byte() {
    // Contrast 1 is a plain threshold. Its boundary is also the byte-128
    // boundary, so this says nothing about sub-byte precision.
    assert_eq!(
        selection(2, 1, json!({"kind":"refine","contrast":1}), &[32767, 32768]),
        [0, 65535]
    );
    assert_eq!((byte(32767), byte(32768)), (127, 128));

    // The depth receipt: two inputs that share displayed byte 128 must produce
    // two distinct u16 codes that also both display as byte 128.
    let pair = selection(
        2,
        1,
        json!({"kind":"refine","contrast":0.25}),
        &[32770, 32771],
    );
    assert_eq!(pair, [32771, 32772]);
    assert_eq!(
        (byte(32770), byte(32771), byte(32771), byte(32772)),
        (128, 128, 128, 128)
    );
    assert_eq!(
        [contrast(32770, 0.25), contrast(32771, 0.25)],
        [pair[0], pair[1]]
    );

    let values = pattern();
    let out = selection(7, 5, json!({"kind":"refine","contrast":0.5}), &values);
    let expected: Vec<u16> = values.iter().map(|&v| contrast(v, 0.5)).collect();
    assert_eq!(out, expected);
}

#[test]
fn each_single_control_equals_the_standalone_selection_kind() {
    let values = pattern();
    for radius in [0.731, 2.] {
        assert_eq!(
            selection(7, 5, json!({"kind":"refine","smooth":radius}), &values),
            selection(7, 5, json!({"kind":"smooth","radius":radius}), &values)
        );
        assert_eq!(
            selection(7, 5, json!({"kind":"refine","feather":radius}), &values),
            selection(7, 5, json!({"kind":"feather","radius":radius}), &values)
        );
        assert_eq!(
            selection(7, 5, json!({"kind":"refine","shift":radius}), &values),
            selection(7, 5, json!({"kind":"grow","radius":radius}), &values)
        );
        assert_eq!(
            selection(7, 5, json!({"kind":"refine","shift":-radius}), &values),
            selection(7, 5, json!({"kind":"shrink","radius":radius}), &values)
        );
    }
}

#[test]
fn all_four_controls_equal_the_chained_kinds_in_smooth_shift_feather_contrast_order() {
    let values = pattern();
    for shift in [1.25_f64, -1.25] {
        let smoothed = selection(7, 5, json!({"kind":"smooth","radius":0.9}), &values);
        let moved = selection(
            7,
            5,
            json!({"kind":if shift > 0. {"grow"} else {"shrink"},"radius":shift.abs()}),
            &smoothed,
        );
        let feathered = selection(7, 5, json!({"kind":"feather","radius":1.4}), &moved);
        let expected: Vec<u16> = feathered.iter().map(|&v| contrast(v, 0.25)).collect();
        let out = selection(
            7,
            5,
            json!({"kind":"refine","smooth":0.9,"shift":shift,"feather":1.4,"contrast":0.25}),
            &values,
        );
        assert_eq!(out, expected);
        assert_ne!(out, values);
    }
}

#[test]
fn out_of_range_controls_unknown_fields_and_cancellation_never_publish() {
    for request in [
        json!({"kind":"refine","smooth":65}),
        json!({"kind":"refine","feather":65}),
        json!({"kind":"refine","shift":-65}),
        json!({"kind":"refine","contrast":1.5}),
        json!({"kind":"refine","contrast":-0.5}),
        json!({"kind":"refine","edge_radius":2}),
        json!({"kind":"refine","decontaminate":true}),
    ] {
        refuse(request);
    }
    let values = pattern();
    let mut out = vec![999_u16; values.len()];
    let text = native::text(json!({"kind":"refine","smooth":1,"contrast":0.5}));
    let token = DrJobControl::default();
    token.cancel();
    assert!(!unsafe {
        dr_selection_u16(
            7,
            5,
            text.as_ptr(),
            values.as_ptr(),
            values.len(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            &token,
            ptr::null_mut(),
        )
    });
    assert_eq!(out, vec![999_u16; values.len()]);
    assert_eq!(token.scratch_bytes(), 0);
}

#[test]
fn refined_coverage_becomes_a_reusable_typed_mask_over_an_unchanged_rgba16_layer() {
    let alpha = [1, 7, 32769, 65535, 0, 65535];
    let pixels: Vec<[u16; 4]> = alpha
        .iter()
        .enumerate()
        .map(|(i, &a)| [1003 + i as u16, 2005, 3007, a])
        .collect();
    let document = native::doc(3, 2, pixels);
    let coverage = [65535, 1, 257, 32768, 0, 32770];
    let refined = selection(3, 2, json!({"kind":"refine","contrast":0.25}), &coverage);
    assert_eq!(refined, [65535, 0, 0, 32768, 0, 32771]);

    let masked = document
        .mask_edit(
            &MaskEdit::SetMask {
                layer: 0,
                width: 3,
                height: 2,
                x: 0,
                y: 0,
                linked: true,
                density: 1.0,
                feather: 0.0,
                enabled: true,
                outside_coverage: 0,
            },
            &refined,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let original = document.layer_source(0).unwrap().clone();
    assert!(PrecisionImage::ptr_eq(
        masked.layer_source(0).unwrap(),
        &original
    ));
    assert_eq!(
        masked
            .layer_source(0)
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap()
            .iter()
            .map(|p| p[3])
            .collect::<Vec<_>>(),
        alpha
    );
    assert_eq!(
        masked.layer(0).unwrap().mask().unwrap().plane().samples(),
        refined
    );

    let rendered = masked.render(REFERENCE_WORKING_BYTES, 0, None).unwrap();
    let samples = rendered.to_rgba16().ok().unwrap();
    assert!(samples[3][3].abs_diff(32768) <= 1, "{:?}", samples[3]);
    assert_eq!(samples[0][3], 1);

    let moved = masked
        .mask_edit(
            &MaskEdit::MaskProperties {
                layer: 0,
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
        moved.layer(0).unwrap().mask().unwrap().properties().offset,
        (1, -1)
    );
    let translated = edit(
        &moved,
        json!({"op":"affine","layer":0,"target":"mask","matrix":[1,0,0,1,1,0],"filter":"nearest"}),
        &[],
    )
    .unwrap();
    assert_eq!(
        translated
            .layer(0)
            .unwrap()
            .mask()
            .unwrap()
            .properties()
            .offset,
        (2, -1)
    );
    for document in [&moved, &translated] {
        assert!(PrecisionImage::ptr_eq(
            document.layer_source(0).unwrap(),
            &original
        ));
    }

    let reusable = translated
        .mask_edit(
            &MaskEdit::SetChannel {
                index: None,
                name: "Refinement coverage".into(),
            },
            &refined,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(reusable.channels()[0].plane().samples(), refined);

    let reopened = PrecisionDocument::parse_native(
        &reusable
            .encode_native(REFERENCE_WORKING_BYTES, 0, None)
            .unwrap(),
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .unwrap();
    assert_eq!(reopened.channels()[0].plane().samples(), refined);
    assert_eq!(
        reopened.layer(0).unwrap().mask().unwrap().plane().samples(),
        refined
    );
    assert_eq!(
        reopened
            .layer_source(0)
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap()
            .iter()
            .map(|p| p[3])
            .collect::<Vec<_>>(),
        alpha
    );
}
