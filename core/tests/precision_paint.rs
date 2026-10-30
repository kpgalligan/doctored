//! Independent f64 paint/erase references and the actual typed FFI transfer.
//! These sources contain genuine adjacent u16 codes, including alpha below1/255.
use crate::group_roots;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_doc_save_native};
use doctored_core::ffi_precision_edit::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_paint::PaintEdit;
use group_roots::GroupRoots;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;

fn doc(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}
unsafe fn error(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        None
    } else {
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        unsafe { dr_string_free(p) };
        Some(s)
    }
}
fn edit(
    doc: &PrecisionDocument,
    request: Value,
    samples: &[u16],
) -> Option<Box<PrecisionDocument>> {
    let request = CString::new(request.to_string()).unwrap();
    unsafe {
        let mut err = ptr::null_mut();
        let result = dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        );
        assert!(err.is_null(), "{:?}", error(err));
        (!result.is_null()).then(|| Box::from_raw(result))
    }
}
fn source(doc: &PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    doc.layer_source(index).unwrap().to_rgba16().unwrap()
}
fn over(dst: [f64; 4], src: [f64; 4]) -> [f64; 4] {
    let a = src[3] + dst[3] * (1.0 - src[3]);
    if a == 0.0 {
        return dst;
    }
    [
        (src[0] * src[3] + dst[0] * dst[3] * (1.0 - src[3])) / a,
        (src[1] * src[3] + dst[1] * dst[3] * (1.0 - src[3])) / a,
        (src[2] * src[3] + dst[2] * dst[3] * (1.0 - src[3])) / a,
        a,
    ]
}

#[test]
fn paint_all_u16_coverage_codes_against_premultiplied_f64_oracle() {
    let before: Vec<_> = (0..=u16::MAX)
        .map(|i| {
            [
                i,
                i.wrapping_add(1),
                i.wrapping_add(2),
                [0, 1, 2, 3, 128, 32768, 65535][i as usize % 7],
            ]
        })
        .collect();
    let coverage: Vec<_> = (0..=u16::MAX).collect();
    let d = doc(256, 256, before.clone());
    let color = [0.312_501_f32, 0.500_009, 0.812_503, 0.625];
    let out = edit(
        &d,
        json!({"op":"paint","layer":0,"color":color,"color_space":"document","opacity":0.75}),
        &coverage,
    )
    .unwrap();
    let mut worst = 0f64;
    for (i, p) in source(&out, 0).iter().enumerate() {
        if i == 0 {
            assert_eq!(*p, before[i]);
            continue;
        }
        let dst = before[i].map(|v| f64::from(v) / 65535.0);
        let mut src = color.map(f64::from);
        src[3] *= f64::from(coverage[i]) / 65535.0 * 0.75;
        let expected = over(dst, src);
        for c in 0..4 {
            worst = worst.max((f64::from(p[c]) / 65535.0 - expected[c]).abs());
        }
    }
    assert!(worst <= 2e-6 + 0.5 / 65535.0, "error {worst}");
    assert_eq!(source(&d, 0), before);
    eprintln!("paint all-code maximum normalized error {worst:.12e}");
}

#[test]
fn erase_keeps_exact_rgb_and_zero_coverage_and_reports_real_noops() {
    let before: Vec<_> = (0..=u16::MAX).map(|i| [i, 43210, 12345, i]).collect();
    let d = doc(256, 256, before.clone());
    let coverage: Vec<_> = (0..=u16::MAX).collect();
    let out = edit(&d, json!({"op":"erase","layer":0,"opacity":0.5}), &coverage).unwrap();
    for (i, p) in source(&out, 0).iter().enumerate() {
        assert_eq!(p[..3], before[i][..3]);
        let expected = f64::from(before[i][3]) * (1.0 - f64::from(coverage[i]) / 65535.0 * 0.5);
        assert!((f64::from(p[3]) - expected).abs() <= 0.505);
    }
    assert!(edit(&d, json!({"op":"erase","layer":0,"opacity":0.0}), &coverage).is_none());
    assert!(edit(&d,json!({"op":"paint","layer":0,"color":[0.1,0.2,0.3,0.0],"color_space":"document","opacity":1.0}),&coverage).is_none());
}

#[test]
fn clone_uses_captured_original_samples_and_integer_canvas_placement() {
    let profile = IccProfile::srgb();
    let samples = vec![
        [10001, 20001, 30001, 65535],
        [10002, 20002, 30002, 65535],
        [111, 222, 333, 0],
        [12345, 23456, 34567, 2],
    ];
    let image = PrecisionImage::from_rgba16(4, 1, samples.clone(), profile.clone(), None).unwrap();
    let target =
        PrecisionImage::from_rgba16(5, 1, vec![[7, 8, 9, 3]; 5], profile.clone(), None).unwrap();
    let d = PrecisionDocument::from_parts(
        5,
        1,
        profile,
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster("Target", target, (0, 0), 1.0, true).unwrap(),
            Layer::raster("Source", image, (1, 0), 0.1, false).unwrap(),
        ],
    )
    .unwrap();
    let out = edit(
        &d,
        json!({"op":"clone","layer":0,"source_layer":1,"dx":1,"dy":0,"opacity":1.0}),
        &[65535; 5],
    )
    .unwrap();
    assert_eq!(source(&out, 0)[0], samples[0]);
    assert_eq!(source(&out, 0)[1], samples[1]);
    assert_eq!(source(&out, 0)[2], [7, 8, 9, 3]);
    assert_eq!(source(&out, 0)[4], [7, 8, 9, 3]);
    let expected = over(
        [7., 8., 9., 3.].map(|v| v / 65535.0),
        samples[3].map(|v| f64::from(v) / 65535.0),
    );
    for (c, e) in expected.iter().enumerate() {
        assert!((f64::from(source(&out, 0)[3][c]) / 65535.0 - e).abs() <= 2e-6 + 0.5 / 65535.0);
    }
    assert_eq!(source(&d, 1), samples);
    let self_d = doc(
        4,
        1,
        vec![
            [10000, 2, 3, 65535],
            [10001, 2, 3, 65535],
            [10002, 2, 3, 65535],
            [10003, 2, 3, 65535],
        ],
    );
    let out = edit(
        &self_d,
        json!({"op":"clone","layer":0,"source_layer":0,"dx":-1,"dy":0,"opacity":1.0}),
        &[65535; 4],
    )
    .unwrap();
    assert_eq!(
        source(&out, 0).iter().map(|p| p[0]).collect::<Vec<_>>(),
        [10000, 10000, 10001, 10002]
    );
}

#[test]
fn two_color_rasterization_retains_f32_until_one_final_storage_boundary() {
    let d = doc(3, 1, vec![[1, 2, 3, 65535]; 3]);
    let colors = [
        [0.123_451_f32, 0.234_561, 0.345_671, 0.500_001],
        [0.512_351, 0.612_361, 0.712_371, 0.333_331],
    ];
    let coverage = [1, 2, 65535, 3, 1001, 32769];
    let out=edit(&d,json!({"op":"rasterize","name":"Shape","width":3,"height":1,"x":-1,"y":2,"colors":colors,"color_space":"document"}),&coverage).unwrap();
    assert_eq!(out.layer(1).unwrap().offset(), (-1, 2));
    for i in 0..3 {
        let mut expected = [0.; 4];
        for k in 0..2 {
            let mut color = colors[k].map(f64::from);
            color[3] *= f64::from(coverage[k * 3 + i]) / 65535.;
            expected = over(expected, color);
        }
        for (c, e) in expected.iter().enumerate() {
            assert!((f64::from(source(&out, 1)[i][c]) / 65535.0 - e).abs() <= 2e-6 + 0.5 / 65535.0);
        }
    }
    assert_eq!(source(&out, 0), source(&d, 0));
    let encoded = out.encode_native(LIMIT, 0, None).unwrap();
    let reopened = PrecisionDocument::parse_native(&encoded, LIMIT, 0, None).unwrap();
    assert_eq!(source(&reopened, 1), source(&out, 1));
}

#[test]
fn strict_requests_reject_wrong_targets_lengths_fields_colors_and_budget() {
    let d = doc(2, 1, vec![[1, 2, 3, 4]; 2]);
    for request in [
        json!({"op":"paint","layer":0,"color":[0.1,0.2,0.3,1.0],"opacity":1.0}),
        json!({"op":"paint","layer":0,"color":[0.1,0.2,0.3,1.01],"color_space":"document","opacity":1.0}),
        json!({"op":"erase","layer":0,"opacity":1.0,"preview":true}),
        json!({"op":"erase","layer":500,"opacity":1.0}),
        json!({"op":"clone","layer":0,"source_layer":99,"dx":0,"dy":0,"opacity":1.0}),
        json!({"op":"rasterize","name":"X","width":0,"height":1,"x":0,"y":0,"colors":[[1.0,0.0,0.0,1.0]],"color_space":"document"}),
        json!({"op":"content_aware_fill","layer":0}),
    ] {
        let c = CString::new(request.to_string()).unwrap();
        unsafe {
            let mut err = ptr::null_mut();
            let p = dr_precision_doc_edit_plan(&d, c.as_ptr(), LIMIT, 0, &mut err);
            assert!(p.is_null());
            assert!(error(err).is_some());
        }
    }
    let c = CString::new("{\"op\":\"erase\",\"layer\":0,\"opacity\":1.0}").unwrap();
    unsafe {
        for (length, limit) in [(1, LIMIT), (2, 1)] {
            let mut err = ptr::null_mut();
            let result = dr_precision_doc_edit(
                &d,
                c.as_ptr(),
                [65535u16; 2].as_ptr(),
                length,
                limit,
                0,
                ptr::null(),
                &mut err,
            );
            assert!(result.is_null());
            assert!(error(err).is_some());
        }
    }
    let grouped = d.group_all(1.0).unwrap().unwrap();
    assert!(grouped
        .paint_plan(&PaintEdit::parse("{\"op\":\"erase\",\"layer\":1,\"opacity\":1.0}").unwrap())
        .is_err());
    assert_eq!(source(&d, 0), [[1, 2, 3, 4]; 2]);
}

#[test]
fn read_planes_are_exact_and_output_is_unchanged_on_errors_and_cancellation() {
    let d = doc(
        4,
        1,
        vec![
            [1, 257, 258, 0],
            [2, 258, 259, 1],
            [3, 259, 260, 2],
            [4, 260, 261, 65535],
        ],
    );
    for (channel, target) in ["red", "green", "blue", "alpha"].into_iter().enumerate() {
        let request =
            CString::new(json!({"op":"read_plane","target":target,"index":0}).to_string()).unwrap();
        let mut out = [999u16; 4];
        unsafe {
            let mut err = ptr::null_mut();
            let plan = dr_precision_doc_edit_plan(&d, request.as_ptr(), LIMIT, 0, &mut err);
            assert!(err.is_null());
            let parsed: Value =
                serde_json::from_str(CStr::from_ptr(plan).to_str().unwrap()).unwrap();
            dr_string_free(plan);
            assert_eq!(parsed["output_samples"], 4);
            assert_eq!(parsed["input_samples"], 0);
            assert!(dr_precision_doc_read_plane(
                &d,
                request.as_ptr(),
                out.as_mut_ptr(),
                4,
                LIMIT,
                0,
                ptr::null(),
                &mut err
            ));
            assert!(err.is_null());
            assert_eq!(
                out,
                source(&d, 0)
                    .iter()
                    .map(|p| p[channel])
                    .collect::<Vec<_>>()
                    .as_slice()
            );
            let control = DrJobControl::default();
            control.cancel();
            out.fill(999);
            assert!(!dr_precision_doc_read_plane(
                &d,
                request.as_ptr(),
                out.as_mut_ptr(),
                4,
                LIMIT,
                0,
                &control,
                &mut err
            ));
            assert!(error(err).is_some());
            assert_eq!(out, [999; 4]);
            err = ptr::null_mut();
            assert!(!dr_precision_doc_read_plane(
                &d,
                request.as_ptr(),
                out.as_mut_ptr(),
                3,
                LIMIT,
                0,
                ptr::null(),
                &mut err
            ));
            assert!(error(err).is_some());
            assert_eq!(out, [999; 4]);
        }
    }
}

#[test]
fn new_7d_exports_null_safety_and_cancelled_edit() {
    unsafe {
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_edit_plan(ptr::null(), ptr::null(), LIMIT, 0, &mut err).is_null());
        assert!(error(err).is_some());
        err = ptr::null_mut();
        assert!(dr_precision_doc_edit(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(error(err).is_some());
        err = ptr::null_mut();
        assert!(!dr_precision_doc_read_plane(
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        ));
        assert!(error(err).is_some());
        assert!(
            dr_precision_doc_edit_plan(ptr::null(), ptr::null(), LIMIT, 0, ptr::null_mut())
                .is_null()
        );
        assert!(dr_precision_doc_edit(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(!dr_precision_doc_read_plane(
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        ));
        dr_precision_doc_free(ptr::null_mut());
    }
    let d = doc(4, 1, vec![[1, 2, 3, 4]; 4]);
    let control = DrJobControl::default();
    control.cancel();
    let e = PaintEdit::parse("{\"op\":\"erase\",\"layer\":0,\"opacity\":1.0}").unwrap();
    assert!(d
        .paint(&e, &[65535; 4], LIMIT, 0, Some(&control))
        .err()
        .unwrap()
        .contains("cancel"));
    assert_eq!(control.scratch_bytes(), 0);
}

#[test]
fn edited_native_writer_is_available_through_ffi() {
    let d = doc(
        2,
        1,
        vec![[10001, 20001, 30001, 1], [10002, 20002, 30002, 65535]],
    );
    let out=edit(&d,json!({"op":"paint","layer":0,"color":[0.312501,0.512503,0.712507,0.5],"color_space":"srgb","opacity":0.625}),&[32769,65535]).unwrap();
    let path = std::env::temp_dir().join(format!("rz-7d-paint-{}.rz", std::process::id()));
    let c = CString::new(path.to_str().unwrap()).unwrap();
    unsafe {
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_save_native(
            &*out,
            c.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        ));
        assert!(err.is_null(), "{:?}", error(err));
    }
    let reopened =
        PrecisionDocument::parse_native(&std::fs::read(&path).unwrap(), LIMIT, 0, None).unwrap();
    assert_eq!(source(&reopened, 0), source(&out, 0));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn authored_alpha_underflow_refuses_before_any_storage_is_published() {
    let d = doc(1, 1, vec![[12001, 23002, 34003, 1]]);
    for request in [
        json!({"op":"paint","layer":0,"color":[0.5,0.6,0.7,1e-30],"color_space":"document","opacity":1.0}),
        json!({"op":"paint","layer":0,"color":[0.5,0.6,0.7,0.5],"color_space":"document","opacity":1e-40}),
        json!({"op":"rasterize","name":"Tiny","width":1,"height":1,"x":0,"y":0,"colors":[[0.5,0.6,0.7,1e-30]],"color_space":"document"}),
        json!({"op":"clone","layer":0,"source_layer":0,"dx":0,"dy":0,"opacity":1e-40}),
    ] {
        let edit = PaintEdit::parse(&request.to_string()).unwrap();
        let control = DrJobControl::default();
        assert!(d.paint(&edit, &[1], LIMIT, 0, Some(&control)).is_err());
        assert_eq!(control.scratch_bytes(), 0);
    }
    assert_eq!(source(&d, 0), [[12001, 23002, 34003, 1]]);
}

#[test]
fn ffi_token_scratch_cap_precedes_json_parse_and_read_failure_is_atomic() {
    use doctored_core::ffi_jobs::dr_job_control_set_scratch_limit;
    let d = doc(2, 1, vec![[1, 2, 3, 4]; 2]);
    unsafe {
        let edit = CString::new("this is intentionally invalid JSON").unwrap();
        let control = DrJobControl::default();
        assert!(dr_job_control_set_scratch_limit(&control, 1));
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_edit(
            &d,
            edit.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            &control,
            &mut err
        )
        .is_null());
        let message = error(err).unwrap();
        assert!(message.contains("working memory"), "{message}");
        assert_eq!(control.scratch_bytes(), 0);
        let read_control = DrJobControl::default();
        assert!(dr_job_control_set_scratch_limit(&read_control, 1));
        let mut out = [12345u16; 2];
        err = ptr::null_mut();
        assert!(!dr_precision_doc_read_plane(
            &d,
            edit.as_ptr(),
            out.as_mut_ptr(),
            2,
            LIMIT,
            0,
            &read_control,
            &mut err
        ));
        assert!(error(err).unwrap().contains("working memory"));
        assert_eq!(out, [12345; 2]);
        assert_eq!(read_control.scratch_bytes(), 0);
    }
}

#[test]
fn disabled_effective_mask_read_inspects_properties_without_changing_render_gate() {
    let d = doc(3, 1, vec![[12001, 23002, 34003, 65535]; 3]);
    let masked = edit(
        &d,
        json!({"op":"set_mask","layer":0,"width":3,"height":1,"density":0.5,"enabled":false}),
        &[0, 1, 65535],
    )
    .unwrap();
    let request =
        CString::new(json!({"op":"read_plane","target":"effective_mask","index":0}).to_string())
            .unwrap();
    let mut out = [0u16; 3];
    unsafe {
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_read_plane(
            &*masked,
            request.as_ptr(),
            out.as_mut_ptr(),
            3,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        ));
        assert!(err.is_null(), "{:?}", error(err));
    }
    assert_eq!(out, [32768, 32768, 65535]);
    assert_eq!(
        masked
            .render(LIMIT, 0, None)
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap(),
        source(&d, 0)
    );
}
