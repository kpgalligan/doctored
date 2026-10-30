//! Independent native-code histogram/crop/marker oracles. The inspected master
//! deliberately differs from its scaled fallback and includes sub-byte samples.
use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_height, dr_image_pixels_rgba, dr_image_width, dr_string_free};
use doctored_core::ffi_develop_inspect::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::retained_source::{SourceEdit, SourceInput};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr};
use std::ptr;
use std::sync::Arc;

use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
unsafe fn text(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .unwrap()
        .to_owned();
    unsafe { dr_string_free(pointer) };
    text
}
fn image(width: u32, height: u32, pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(width, height, pixels, IccProfile::display_p3(), None).unwrap()
}
fn small() -> PrecisionImage {
    image(
        7,
        5,
        (0..35)
            .map(|i| {
                [
                    137 + i * 1747,
                    257 + i * 523,
                    13 + i * 337,
                    if i % 6 == 0 { 0 } else { 65535 - i * 113 },
                ]
            })
            .collect(),
    )
}
fn parsed(value: Value) -> SourceEdit {
    SourceEdit::parse(&value.to_string()).unwrap()
}
fn typed(source: PrecisionImage) -> PrecisionDocument {
    PrecisionDocument::from_image(source, Metadata::default(), Resolution::default()).unwrap()
}
fn histogram(image: &PrecisionImage) -> Value {
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_image_histogram_checked(image, 1, LIMIT, 0, ptr::null(), &mut error)
    };
    assert!(error.is_null(), "{}", unsafe { text(error) });
    serde_json::from_str(&unsafe { text(result) }).unwrap()
}
fn preview(image: &PrecisionImage, region: [u32; 4], divisor: u32, clipping: u32) -> RgbaImage {
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_image_develop_preview(
            image,
            region[0],
            region[1],
            region[2],
            region[3],
            divisor,
            clipping,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "{}", unsafe { text(error) });
    assert!(!result.is_null());
    let result = unsafe { Box::from_raw(result) };
    let (width, height) = unsafe { (dr_image_width(&*result), dr_image_height(&*result)) };
    let bytes = unsafe {
        std::slice::from_raw_parts(
            dr_image_pixels_rgba(&*result),
            width as usize * height as usize * 4,
        )
    };
    RgbaImage::from_raw(width, height, bytes.to_vec()).unwrap()
}
#[test]
fn full_histogram_uses_genuine_codes_with_exact_endpoints_independent_of_display_bins() {
    let samples: Vec<[u16; 4]> = (0..=65535_u32)
        .map(|i| {
            [
                i as u16,
                (65535 - i) as u16,
                (i * 17) as u16,
                [0, 1, 32768, 65535][i as usize % 4],
            ]
        })
        .collect();
    let source = image(256, 256, samples.clone());
    let actual = histogram(&source);
    let mut expected = vec![vec![0_u64; 256]; 4];
    let mut alpha = vec![0_u64; 256];
    let mut zero = [0_u64; 4];
    let mut maximum = [0_u64; 4];
    let mut visible = 0;
    let byte_bin = |v: u16| ((u64::from(v) * 255 + 32767) / 65535) as usize;
    for p in &samples {
        alpha[byte_bin(p[3])] += 1;
        zero[3] += u64::from(p[3] == 0);
        maximum[3] += u64::from(p[3] == 65535);
        if p[3] == 0 {
            continue;
        }
        visible += 1;
        for channel in 0..3 {
            expected[channel][byte_bin(p[channel])] += 1;
            zero[channel] += u64::from(p[channel] == 0);
            maximum[channel] += u64::from(p[channel] == 65535);
        }
        let luma = 0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]);
        expected[3][(luma * 255. / 65535. + 0.5).floor() as usize] += 1;
    }
    assert_eq!(actual["bins"], json!(expected));
    assert_eq!(actual["alpha_bins"], json!(alpha));
    assert_eq!(actual["total"], visible);
    assert_eq!(actual["sampled_pixels"], 65536);
    assert_eq!(actual["exact_zero_rgba"], json!(zero));
    assert_eq!(actual["exact_max_rgba"], json!(maximum));
    assert!(actual["bins"][0][0].as_u64().unwrap() > zero[0]);
    assert_eq!(actual["stride"], 1);
    assert_eq!(actual["clipping_scope"], "all pixels");
}
#[test]
fn source_master_extraction_ignores_fallback_depth_transform_and_retouch() {
    let original = small();
    let source = typed(original.clone());
    let request = parsed(
        json!({"op":"place","source_id":"camera","name":"Camera","x":0,"y":0,"depth_conversion":"convert_fallback"}),
    );
    let base = DrDocument::from_pixels(RgbaImage::from_pixel(7, 5, Rgba([0; 4])));
    let low = base
        .source_edit_input(Some(SourceInput::Typed(&source)), &request, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let transform =
        parsed(json!({"op":"transform","layer":1,"matrix":[0.5,0,0,0.5,2,3],"filter":"bilinear"}));
    let mut low = low
        .source_edit(None, &transform, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let mut top = DrDocument::from_pixels(RgbaImage::from_pixel(7, 5, Rgba([199, 44, 66, 255])))
        .layers
        .remove(0);
    top.name = "Retouch".into();
    low.layers.push(top);
    let high = source
        .source_edit(
            None,
            &parsed(json!({"op":"retain","source_id":"camera"})),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let mut error = ptr::null_mut();
    for result in [
        unsafe { dr_doc_source_master16(&low, 1, &mut error) },
        unsafe { dr_precision_doc_source_master16(&high, 0, &mut error) },
    ] {
        assert!(error.is_null(), "{}", unsafe { text(error) });
        assert!(!result.is_null());
        let master = *unsafe { Box::from_raw(result) };
        assert_eq!(master.dimensions(), (7, 5));
        assert_eq!(master.to_rgba16().ok(), original.to_rgba16().ok());
        assert!(Arc::ptr_eq(master.profile(), original.profile()));
        assert!(PrecisionImage::ptr_eq(&master, &original));
    }
    assert_ne!(low.layers[1].pixels.dimensions(), original.dimensions());
    low.layers[1].meta = Some("destructive edit".into());
    assert!(unsafe { dr_doc_source_master16(&low, 1, &mut error) }.is_null());
    assert!(!unsafe { text(error) }.is_empty());
}
#[test]
fn detail_is_an_exact_native_pixel_crop_including_hidden_rgb_and_original_icc() {
    let source = small();
    let before = source.to_rgba16().unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_image_develop_detail(&source, 2, 1, 4, 3, LIMIT, 0, ptr::null(), &mut error)
    };
    assert!(error.is_null(), "{}", unsafe { text(error) });
    assert!(!out.is_null());
    let out = *unsafe { Box::from_raw(out) };
    let mut expected = Vec::new();
    for y in 1..4 {
        expected.extend_from_slice(&before[y * 7 + 2..y * 7 + 6]);
    }
    assert_eq!(out.dimensions(), (4, 3));
    assert_eq!(out.to_rgba16().ok().unwrap(), expected);
    assert!(Arc::ptr_eq(out.profile(), source.profile()));
    assert_eq!(
        source.to_rgba16().ok().unwrap(),
        small().to_rgba16().ok().unwrap()
    );
    let plain = preview(&source, [2, 1, 4, 3], 1, 0);
    let bytes: Vec<u8> = expected
        .iter()
        .flatten()
        .map(|v| ((u64::from(*v) * 255 + 32767) / 65535) as u8)
        .collect();
    assert_eq!(plain.as_raw(), &bytes);
}
#[test]
fn fit_preview_cell_markers_include_off_center_endpoints_and_ignore_hidden_rgb() {
    let mut pixels = vec![[32768, 32768, 32768, 65535]; 35];
    pixels[0][0] = 0;
    pixels[1][2] = 65535;
    pixels[3] = [0, 65535, 0, 0]; // endpoints are invisible, so no warning in this cell.
    pixels[6] = [32768, 65535, 32768, 1]; // tiny positive alpha still contributes.
    pixels[34] = [0, 32768, 32768, 65535]; // bottom-right partial cell.
    let source = image(7, 5, pixels);
    let normal = preview(&source, [0, 0, 7, 5], 3, 0);
    assert_eq!(normal.dimensions(), (3, 2));
    assert_eq!(normal.get_pixel(0, 0).0, [128, 128, 128, 255]);
    for (flags, first) in [
        (1, [0, 0, 255, 255]),
        (2, [255, 0, 0, 255]),
        (3, [255, 0, 255, 255]),
    ] {
        let marked = preview(&source, [0, 0, 7, 5], 3, flags);
        assert_eq!(marked.get_pixel(0, 0).0, first);
        assert_eq!(marked.get_pixel(1, 0), normal.get_pixel(1, 0));
        if flags & 2 != 0 {
            assert_eq!(marked.get_pixel(2, 0).0, [255, 0, 0, 255]);
        }
        if flags & 1 != 0 {
            assert_eq!(marked.get_pixel(2, 1).0, [0, 0, 255, 255]);
        }
    }
    let hist = histogram(&source);
    assert_eq!(hist["exact_zero_rgba"][0], 2);
    assert_eq!(hist["exact_max_rgba"][1], 1);
    assert_eq!(hist["exact_max_rgba"][2], 1);
}
#[test]
fn invalid_regions_display_caps_and_budgets_refuse_without_outputs() {
    let source = small();
    let mut error = ptr::null_mut();
    for region in [
        [0, 0, 0, 1],
        [7, 0, 1, 1],
        [0, 4, 1, 2],
        [u32::MAX, 0, 2, 1],
    ] {
        assert!(unsafe {
            dr_precision_image_develop_detail(
                &source,
                region[0],
                region[1],
                region[2],
                region[3],
                LIMIT,
                0,
                ptr::null(),
                &mut error,
            )
        }
        .is_null());
        assert!(!unsafe { text(error) }.is_empty());
        error = ptr::null_mut();
    }
    let wide = image(1601, 1, vec![[1, 2, 3, 65535]; 1601]);
    assert!(unsafe {
        dr_precision_image_develop_detail(&wide, 0, 0, 1025, 1, LIMIT, 0, ptr::null(), &mut error)
    }
    .is_null());
    assert!(unsafe { text(error) }.contains("1024"));
    error = ptr::null_mut();
    for (divisor, flags) in [(0, 0), (1, 4), (1, 0)] {
        assert!(unsafe {
            dr_precision_image_develop_preview(
                &wide,
                0,
                0,
                1601,
                1,
                divisor,
                flags,
                LIMIT,
                0,
                ptr::null(),
                &mut error,
            )
        }
        .is_null());
        assert!(!unsafe { text(error) }.is_empty());
        error = ptr::null_mut();
    }
    let token = DrJobControl::default();
    let need = source.retained_bytes() + 1024 * 1024;
    assert!(unsafe {
        dr_precision_image_histogram_checked(&source, 1, need - 1, 0, &token, &mut error)
    }
    .is_null());
    assert!(unsafe { text(error) }.contains("working limit"));
    assert_eq!(token.scratch_bytes(), 0);
    error = ptr::null_mut();
    assert!(unsafe {
        dr_precision_image_develop_detail(
            &source,
            0,
            0,
            7,
            5,
            need + 35 * 8 - 1,
            0,
            ptr::null(),
            &mut error,
        )
    }
    .is_null());
    assert!(unsafe { text(error) }.contains("working limit"));
    error = ptr::null_mut();
    assert!(unsafe {
        dr_precision_image_develop_preview(
            &source,
            0,
            0,
            7,
            5,
            1,
            0,
            need + 35 * 4 - 1,
            0,
            ptr::null(),
            &mut error,
        )
    }
    .is_null());
    assert!(unsafe { text(error) }.contains("working limit"));
}
#[test]
fn histogram_and_full_cell_marker_scans_cancel_and_release_reservations() {
    let source = image(2048, 2048, vec![[12345, 23456, 34567, 65535]; 2048 * 2048]);
    for markers in [false, true] {
        let render = |token: usize| {
            let token = token as *const DrJobControl;
            let mut error = ptr::null_mut();
            let failed = if markers {
                unsafe {
                    dr_precision_image_develop_preview(
                        &source, 0, 0, 2048, 2048, 2048, 3, LIMIT, 0, token, &mut error,
                    )
                }
                .is_null()
            } else {
                unsafe {
                    dr_precision_image_histogram_checked(&source, 1, LIMIT, 0, token, &mut error)
                }
                .is_null()
            };
            (!failed, unsafe { text(error) })
        };
        let (entry, next) = entry_then_cancel(&|_, stage, _| stage == 6, render);
        let n0 = entry.checkpoints;
        // The marker scan polls inside its kernel between reports: k = 1.
        // The histogram scan checks only at its reports (the next report's
        // check is n0 + 1): k = 0, the park is that report's own check, and
        // the verdict holds the stage alone.
        let k = if markers { 1 } else { 0 };
        if markers {
            assert!(
                next.expect("the entry's kernel never reported again")
                    .checkpoints
                    > n0 + 1 + k,
                "the entry's kernel polls fewer than {} checkpoints before its next report",
                k + 1
            );
        }
        let parked = cancelled_parked_at(
            n0 + 1 + k,
            &|progress, stage| stage == entry.stage && (!markers || progress == entry.progress),
            render,
        );
        println!("markers {markers}: {}", parked_line(&parked));
        assert_eq!(parked.stage, 6);
    }
}
#[test]
fn every_develop_export_is_null_safe_and_cancelled_detail_is_atomic() {
    unsafe {
        assert!(dr_doc_source_master16(ptr::null(), 0, ptr::null_mut()).is_null());
        assert!(dr_precision_doc_source_master16(ptr::null(), 0, ptr::null_mut()).is_null());
        assert!(dr_precision_image_histogram_checked(
            ptr::null(),
            1,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_image_develop_detail(
            ptr::null(),
            0,
            0,
            1,
            1,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_image_develop_preview(
            ptr::null(),
            0,
            0,
            1,
            1,
            1,
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
    }
    let source = small();
    let token = DrJobControl::default();
    token.cancel();
    let mut error = ptr::null_mut();
    assert!(unsafe {
        dr_precision_image_develop_detail(&source, 0, 0, 1, 1, LIMIT, 0, &token, &mut error)
    }
    .is_null());
    assert!(unsafe { text(error) }.contains("cancelled"));
    assert_eq!(token.scratch_bytes(), 0);
}
