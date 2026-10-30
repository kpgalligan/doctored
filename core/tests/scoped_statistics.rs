//! Independent rational counts, native-code sums and contribution boundaries.
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::scoped_statistics::AnalysisRequest;
use serde_json::{json, Value};

fn image(pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(
        pixels.len() as u32,
        1,
        pixels,
        IccProfile::display_p3(),
        None,
    )
    .unwrap()
}
fn document(pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(image(pixels), Metadata::default(), Resolution::default())
        .unwrap()
}
fn read(doc: &PrecisionDocument, request: Value, coverage: &[u16]) -> Value {
    doc.scoped_analysis(
        &AnalysisRequest::parse(&request.to_string()).unwrap(),
        coverage,
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .unwrap()
}

#[test]
fn selected_histogram_exact_endpoints_ignore_display_stride_and_keep_coverage_one() {
    let pixels = vec![
        [0, 1, 65535, 1],
        [1, 2, 65534, 2],
        [65535, 65535, 0, 65535],
        [7, 8, 9, 0],
        [0, 0, 0, 1],
    ];
    let weights = [1u16, 32768, 65535, 123, 0];
    let doc = document(pixels.clone());
    let full = read(
        &doc,
        json!({"operation":"histogram","source":"layer","layer":0}),
        &weights,
    );
    let sparse = read(
        &doc,
        json!({"operation":"histogram","source":"layer","layer":0,"stride":3}),
        &weights,
    );
    for key in [
        "exact_zero_rgba",
        "exact_max_rgba",
        "exact_zero_weight_units_rgba",
        "exact_max_weight_units_rgba",
        "weighted_code_sums_rgba",
        "full_rgb_weight_units",
        "full_alpha_weight_units",
    ] {
        assert_eq!(full[key], sparse[key], "{key}");
    }
    assert_eq!(full["exact_zero_rgba"], json!([1, 0, 1, 1]));
    assert_eq!(
        full["exact_zero_weight_units_rgba"],
        json!([1, 0, 65535, 123])
    );
    assert_eq!(full["full_rgb_weight_units"], 98304);
    assert_eq!(full["full_alpha_weight_units"], 98427);
    assert_eq!(sparse["sampled_rgb_weight_units"], 1);
    assert_eq!(sparse["bin_weight_units"][0][0], 1);
    assert_eq!(sparse["bins"][0][0].as_f64().unwrap(), 1. / 65535.);
    for c in 0..4 {
        let sum: u64 = pixels
            .iter()
            .zip(weights)
            .filter(|(p, _)| c == 3 || p[3] > 0)
            .map(|(p, w)| u64::from(p[c]) * u64::from(w))
            .sum();
        assert_eq!(full["weighted_code_sums_rgba"][c], sum);
    }
    assert!(full["profile_name"].as_str().unwrap().contains("P3"));
}

#[test]
fn readout_weights_hidden_rgb_and_divides_by_exact_selection_mass() {
    let doc = document(vec![[1, 2, 3, 0], [2, 3, 4, 1], [65535, 257, 256, 65535]]);
    let weights = [1u16, 2, 3];
    let sample = read(
        &doc,
        json!({"operation":"sample","source":"layer","layer":0,"x":1,"y":0,"reach":1}),
        &weights,
    );
    assert_eq!(sample["weight_units"], 6);
    assert_eq!(
        sample["weighted_sum_rgba_codes"],
        json!([196610, 779, 779, 196607])
    );
    assert_eq!(sample["mean_rgba_codes"][0].as_f64().unwrap(), 196610. / 6.);
    assert_eq!(
        sample["normalized_rgba"][0].as_f64().unwrap(),
        196610. / 6. / 65535.
    );
    let zero =
        AnalysisRequest::parse(r#"{"operation":"sample","source":"layer","layer":0,"x":0,"y":0}"#)
            .unwrap();
    assert!(doc
        .scoped_analysis(&zero, &[0; 3], REFERENCE_WORKING_BYTES, 0, None)
        .is_err());
}

#[test]
fn raw_layer_scope_maps_offsets_and_counts_transparent_canvas_padding() {
    let source = PrecisionDocumentLayer::raster(
        "offset",
        image(vec![[30000, 30001, 30002, 1], [31000, 31001, 31002, 65535]]),
        (1, 0),
        0.25,
        false,
    )
    .unwrap();
    let doc = PrecisionDocument::from_parts(
        4,
        1,
        IccProfile::display_p3(),
        Metadata::default(),
        Resolution::default(),
        vec![source],
    )
    .unwrap();
    let hist = read(
        &doc,
        json!({"operation":"histogram","source":"layer","layer":0}),
        &[],
    );
    assert_eq!(hist["full_rgb_pixel_count"], 2);
    assert_eq!(hist["exact_zero_rgba"][3], 2);
    assert_eq!(hist["weighted_code_sums_rgba"][0], 61000u64 * 65535);
    let sample = read(
        &doc,
        json!({"operation":"sample","source":"layer","layer":0,"x":0,"y":0}),
        &[],
    );
    assert_eq!(sample["center_rgba_codes"], json!([0, 0, 0, 0]));
    let composite = read(
        &doc,
        json!({"operation":"histogram","source":"composite"}),
        &[],
    );
    assert_eq!(composite["full_rgb_pixel_count"], 0);
}

#[test]
fn partial_backdrop_keeps_ancestor_group_gate_and_excludes_boundary() {
    // Index0 opaque black base; group children1red,2blue; group3; top4white.
    // below2 means opaque black plus red at group opacity 1/2, which composites
    // at its byte 128/255 (#2) = native [32896,0,0,65535] (128·257).
    let leaf = |name: &str, p: [u16; 4]| {
        PrecisionDocumentLayer::raster(name, image(vec![p]), (0, 0), 1., true).unwrap()
    };
    let group = PrecisionDocumentLayer::isolated_normal(
        "half group",
        vec![
            leaf("red", [65535, 0, 0, 65535]),
            leaf("blue", [0, 0, 65535, 65535]),
        ],
        0.5,
        true,
    )
    .unwrap();
    let doc = PrecisionDocument::from_parts(
        1,
        1,
        IccProfile::display_p3(),
        Metadata::default(),
        Resolution::default(),
        vec![
            leaf("base", [0, 0, 0, 65535]),
            group,
            leaf("top", [65535; 4]),
        ],
    )
    .unwrap();
    let source = read(
        &doc,
        json!({"operation":"sample","source":"backdrop","below":2,"x":0,"y":0}),
        &[],
    );
    assert_eq!(source["center_rgba_codes"], json!([32896, 0, 0, 65535]));
    let empty = read(
        &doc,
        json!({"operation":"sample","source":"backdrop","below":0,"x":0,"y":0}),
        &[],
    );
    assert_eq!(empty["center_rgba_codes"], json!([0, 0, 0, 0]));
    let all = read(
        &doc,
        json!({"operation":"sample","source":"backdrop","below":5,"x":0,"y":0}),
        &[],
    );
    assert_eq!(
        all["center_rgba_codes"],
        json!([65535, 65535, 65535, 65535])
    );
}

#[test]
fn scoped_analysis_refuses_bad_shapes_budget_and_cancel_without_source_changes() {
    let pixels = vec![[30001, 30002, 30003, 1]; 1024];
    let doc = document(pixels.clone());
    let request =
        AnalysisRequest::parse(r#"{"operation":"histogram","source":"layer","layer":0}"#).unwrap();
    assert!(doc
        .scoped_analysis(&request, &[1; 3], REFERENCE_WORKING_BYTES, 0, None)
        .is_err());
    assert!(doc.scoped_analysis(&request, &[], 1, 0, None).is_err());
    let token = DrJobControl::default();
    token.cancel();
    assert!(doc
        .scoped_analysis(&request, &[], REFERENCE_WORKING_BYTES, 0, Some(&token))
        .is_err());
    assert_eq!(
        doc.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
        pixels
    );
    for bad in [
        r#"{"operation":"sample","source":"composite"}"#,
        r#"{"operation":"histogram","source":"backdrop"}"#,
        r#"{"operation":"histogram","source":"layer","layer":0,"below":1}"#,
    ] {
        assert!(AnalysisRequest::parse(bad).is_err());
    }
}

#[test]
fn byte_raw_adjustment_analysis_refuses_placeholders_before_plan_or_read() {
    use doctored_core::doc::DrDocument;
    use doctored_core::ffi::dr_string_free;
    use doctored_core::ffi_scoped_statistics::{
        dr_doc_scoped_analysis, dr_doc_scoped_analysis_plan,
    };
    use std::ffi::{CStr, CString};
    use std::ptr;
    let mut doc = DrDocument::from_pixels(image::RgbaImage::from_pixel(
        1,
        1,
        image::Rgba([41, 82, 123, 255]),
    ));
    doc.layers[0].meta = Some(r#"{"type":"adjust","op":"invert","params":{}}"#.into());
    for operation in ["sample", "histogram"] {
        let value = if operation == "sample" {
            json!({"operation":operation,"source":"layer","layer":0,"x":0,"y":0})
        } else {
            json!({"operation":operation,"source":"layer","layer":0})
        };
        let request = AnalysisRequest::parse(&value.to_string()).unwrap();
        assert!(doc
            .scoped_analysis_plan(&request)
            .unwrap_err()
            .contains("no raw pixels"));
        assert!(doc
            .scoped_analysis(&request, &[], REFERENCE_WORKING_BYTES, 0, None)
            .unwrap_err()
            .contains("no raw pixels"));
        let request = CString::new(value.to_string()).unwrap();
        let mut error = ptr::null_mut();
        let plan = unsafe { dr_doc_scoped_analysis_plan(&doc, request.as_ptr(), &mut error) };
        assert!(plan.is_null());
        assert!(!error.is_null());
        assert!(unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .contains("no raw pixels"));
        unsafe { dr_string_free(error) };
        error = ptr::null_mut();
        let result = unsafe {
            dr_doc_scoped_analysis(
                &doc,
                request.as_ptr(),
                ptr::null(),
                0,
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        };
        assert!(result.is_null());
        assert!(!error.is_null());
        assert!(unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .contains("no raw pixels"));
        unsafe { dr_string_free(error) };
    }
    assert_eq!(
        doc.layers[0].pixels.contiguous_image().get_pixel(0, 0).0,
        [41, 82, 123, 255]
    );
}

#[test]
fn byte_raw_group_analysis_keeps_real_children_even_with_adjustment_shaped_group_metadata() {
    use doctored_core::doc::DrDocument;
    use doctored_core::ffi::dr_string_free;
    use doctored_core::ffi_scoped_statistics::dr_doc_scoped_analysis;
    use std::ffi::{CStr, CString};
    use std::ptr;
    let source = DrDocument::from_pixels(image::RgbaImage::from_pixel(
        1,
        1,
        image::Rgba([41, 82, 123, 255]),
    ));
    let (mut doc, group, _, _) = source.group_layers(&[0], "Raw Group").unwrap();
    doc.layers[group].meta = Some(r#"{"type":"adjust","op":"invert","params":{}}"#.into());
    doc.layers[group].opacity = 0.25;
    doc.layers[group].visible = false;
    let value = json!({"operation":"sample","source":"layer","layer":group,"x":0,"y":0});
    let request = AnalysisRequest::parse(&value.to_string()).unwrap();
    assert!(doc.scoped_analysis_plan(&request).is_ok());
    let result = doc
        .scoped_analysis(&request, &[], REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
    assert_eq!(result["center_rgba_codes"], json!([41, 82, 123, 255]));
    let request = CString::new(value.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_doc_scoped_analysis(
            &doc,
            request.as_ptr(),
            ptr::null(),
            0,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null());
    assert!(!result.is_null());
    let value: Value =
        serde_json::from_slice(unsafe { CStr::from_ptr(result) }.to_bytes()).unwrap();
    assert_eq!(value["center_rgba_codes"], json!([41, 82, 123, 255]));
    unsafe { dr_string_free(result) };
}

mod projection {
    //! A supplied (retained) projection answers the composite scope with
    //! the document path as the oracle — same `Value`, no second render.
    use super::*;
    use doctored_core::ffi::dr_string_free;
    use doctored_core::ffi_scoped_statistics::{
        dr_precision_doc_scoped_analysis, dr_precision_doc_scoped_analysis_plan,
        dr_precision_image_scoped_analysis, dr_precision_image_scoped_analysis_plan,
    };
    use doctored_core::scoped_statistics::STATISTICS_SCRATCH;
    use std::ffi::{c_char, CStr, CString};
    use std::ptr;

    const W: u32 = 5;
    const H: u32 = 4;

    fn two_layer() -> PrecisionDocument {
        let base = (0..W * H)
            .map(|i| {
                let v = (i * 3001) as u16;
                [v, 65535 - v, (i * 17) as u16, 65535]
            })
            .collect::<Vec<_>>();
        let top = (0..6)
            .map(|i| [(i * 9000) as u16, 40000, 0, [0, 1, 32768, 65535][i % 4]])
            .collect::<Vec<_>>();
        let layers = vec![
            PrecisionDocumentLayer::raster(
                "base",
                PrecisionImage::from_rgba16(W, H, base, IccProfile::display_p3(), None).unwrap(),
                (0, 0),
                1.,
                true,
            )
            .unwrap(),
            PrecisionDocumentLayer::raster(
                "top",
                PrecisionImage::from_rgba16(3, 2, top, IccProfile::display_p3(), None).unwrap(),
                (1, 1),
                0.6,
                true,
            )
            .unwrap(),
        ];
        PrecisionDocument::from_parts(
            W,
            H,
            IccProfile::display_p3(),
            Metadata::default(),
            Resolution::default(),
            layers,
        )
        .unwrap()
    }

    fn requests() -> Vec<Value> {
        vec![
            json!({"operation":"histogram","source":"composite","stride":1}),
            json!({"operation":"histogram","source":"composite","stride":3}),
            json!({"operation":"sample","source":"composite","x":2,"y":2,"reach":0}),
            json!({"operation":"sample","source":"composite","x":2,"y":1,"reach":1}),
        ]
    }

    fn coverage() -> Vec<u16> {
        (0..W * H).map(|i| ((i * 7919) % 65536) as u16).collect()
    }

    unsafe fn answer(value: *mut c_char, error: *mut c_char) -> Result<Value, String> {
        if value.is_null() {
            assert!(!error.is_null());
            let text = unsafe { CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned();
            unsafe { dr_string_free(error) };
            return Err(text);
        }
        let parsed = serde_json::from_slice(unsafe { CStr::from_ptr(value) }.to_bytes()).unwrap();
        unsafe { dr_string_free(value) };
        Ok(parsed)
    }

    fn image_ffi(
        image: &PrecisionImage,
        request: &Value,
        coverage: &[u16],
    ) -> Result<Value, String> {
        let request = CString::new(request.to_string()).unwrap();
        let mut error = ptr::null_mut();
        unsafe {
            let out = dr_precision_image_scoped_analysis(
                image,
                request.as_ptr(),
                if coverage.is_empty() {
                    ptr::null()
                } else {
                    coverage.as_ptr()
                },
                coverage.len(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            );
            answer(out, error)
        }
    }

    fn doc_ffi(
        doc: &PrecisionDocument,
        request: &Value,
        coverage: &[u16],
    ) -> Result<Value, String> {
        let request = CString::new(request.to_string()).unwrap();
        let mut error = ptr::null_mut();
        unsafe {
            let out = dr_precision_doc_scoped_analysis(
                doc,
                request.as_ptr(),
                if coverage.is_empty() {
                    ptr::null()
                } else {
                    coverage.as_ptr()
                },
                coverage.len(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            );
            answer(out, error)
        }
    }

    fn run(image: &PrecisionImage, request: &Value, coverage: &[u16]) -> Result<Value, String> {
        image.scoped_analysis(
            &AnalysisRequest::parse(&request.to_string()).unwrap(),
            coverage,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
    }

    #[test]
    fn projection_composite_equals_the_document_path_through_api_and_ffi() {
        let doc = two_layer();
        let projection = doc.render(REFERENCE_WORKING_BYTES, 0, None).unwrap();
        let weights = coverage();
        for request in requests() {
            for coverage in [&[][..], &weights[..]] {
                let oracle = read(&doc, request.clone(), coverage);
                assert_eq!(
                    run(&projection, &request, coverage).unwrap(),
                    oracle,
                    "{request}"
                );
                assert_eq!(
                    image_ffi(&projection, &request, coverage).unwrap(),
                    doc_ffi(&doc, &request, coverage).unwrap(),
                    "{request}"
                );
                assert_eq!(image_ffi(&projection, &request, coverage).unwrap(), oracle);
            }
        }
        let zero = json!({"operation":"sample","source":"composite","x":0,"y":0});
        let none = vec![0u16; (W * H) as usize];
        let doc_error = doc
            .scoped_analysis(
                &AnalysisRequest::parse(&zero.to_string()).unwrap(),
                &none,
                REFERENCE_WORKING_BYTES,
                0,
                None,
            )
            .unwrap_err();
        assert_eq!(run(&projection, &zero, &none).unwrap_err(), doc_error);
        assert_eq!(
            image_ffi(&projection, &zero, &none).unwrap_err(),
            doc_ffi(&doc, &zero, &none).unwrap_err()
        );
    }

    #[test]
    fn projection_plan_is_statistics_scratch_plus_bookkeeping_below_the_render_plan() {
        let doc = two_layer();
        let projection = doc.render(REFERENCE_WORKING_BYTES, 0, None).unwrap();
        let request = CString::new(r#"{"operation":"histogram","source":"composite"}"#).unwrap();
        let mut error = ptr::null_mut();
        let image_plan = unsafe {
            answer(
                dr_precision_image_scoped_analysis_plan(&projection, request.as_ptr(), &mut error),
                error,
            )
        }
        .unwrap();
        let mut error = ptr::null_mut();
        let doc_plan = unsafe {
            answer(
                dr_precision_doc_scoped_analysis_plan(&doc, request.as_ptr(), &mut error),
                error,
            )
        }
        .unwrap();
        assert_eq!(
            image_plan["scratch_bytes"],
            STATISTICS_SCRATCH + 1024 * 1024
        );
        assert_eq!(image_plan["coverage_denominator"], 65535);
        assert!(
            image_plan["scratch_bytes"].as_u64().unwrap()
                < doc_plan["scratch_bytes"].as_u64().unwrap()
        );
    }

    #[test]
    fn projection_refuses_other_sources_bad_coverage_budget_and_cancel() {
        let doc = two_layer();
        let projection = doc.render(REFERENCE_WORKING_BYTES, 0, None).unwrap();
        for request in [
            json!({"operation":"histogram","source":"layer","layer":0}),
            json!({"operation":"sample","source":"backdrop","below":1,"x":0,"y":0}),
        ] {
            let parsed = AnalysisRequest::parse(&request.to_string()).unwrap();
            assert_eq!(
                projection.scoped_analysis_plan(&parsed).unwrap_err(),
                "projection analysis takes the composite source only"
            );
            assert_eq!(
                run(&projection, &request, &[]).unwrap_err(),
                "projection analysis takes the composite source only"
            );
            let text = CString::new(request.to_string()).unwrap();
            let mut error = ptr::null_mut();
            let plan = unsafe {
                answer(
                    dr_precision_image_scoped_analysis_plan(&projection, text.as_ptr(), &mut error),
                    error,
                )
            };
            assert!(plan
                .unwrap_err()
                .contains("projection analysis takes the composite source only"));
            assert!(image_ffi(&projection, &request, &[])
                .unwrap_err()
                .contains("projection analysis takes the composite source only"));
        }
        let histogram = json!({"operation":"histogram","source":"composite"});
        assert!(run(&projection, &histogram, &[1; 3]).is_err());
        assert!(image_ffi(&projection, &histogram, &[1; 3]).is_err());
        let parsed = AnalysisRequest::parse(&histogram.to_string()).unwrap();
        assert!(projection
            .scoped_analysis(&parsed, &[], 1, 0, None)
            .is_err());
        let token = DrJobControl::default();
        token.cancel();
        assert!(projection
            .scoped_analysis(&parsed, &[], REFERENCE_WORKING_BYTES, 0, Some(&token))
            .unwrap_err()
            .contains("cancelled"));
        assert_eq!(token.scratch_bytes(), 0);
    }

    #[test]
    fn projection_exports_are_null_safe() {
        let doc = two_layer();
        let projection = doc.render(REFERENCE_WORKING_BYTES, 0, None).unwrap();
        let request = CString::new(r#"{"operation":"histogram","source":"composite"}"#).unwrap();
        unsafe {
            assert!(dr_precision_image_scoped_analysis_plan(
                ptr::null(),
                request.as_ptr(),
                ptr::null_mut()
            )
            .is_null());
            assert!(dr_precision_image_scoped_analysis_plan(
                &projection,
                ptr::null(),
                ptr::null_mut()
            )
            .is_null());
            for (image, text) in [
                (ptr::null(), request.as_ptr()),
                (&projection as *const PrecisionImage, ptr::null()),
            ] {
                assert!(dr_precision_image_scoped_analysis(
                    image,
                    text,
                    ptr::null(),
                    0,
                    REFERENCE_WORKING_BYTES,
                    0,
                    ptr::null(),
                    ptr::null_mut(),
                )
                .is_null());
            }
        }
    }
}

mod byte_projection {
    //! A supplied (retained) 8-bit projection answers the composite
    //! scope with the byte document path as the oracle — same `Value`, no
    //! second render.
    use super::*;
    use doctored_core::doc::DrDocument;
    use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_string_free};
    use doctored_core::ffi_scoped_statistics::{
        dr_doc_projection_scoped_analysis, dr_doc_projection_scoped_analysis_plan,
        dr_doc_scoped_analysis, dr_doc_scoped_analysis_plan,
    };
    use doctored_core::scoped_statistics::STATISTICS_SCRATCH;
    use doctored_core::DrImage;
    use image::{Rgba, RgbaImage};
    use std::ffi::{c_char, CStr, CString};
    use std::ptr;

    const W: u32 = 5;
    const H: u32 = 4;

    /// An owned straight RGBA8 projection: the pixels for the API and the
    /// same pixels as a `DrImage` handle (`dr_image_from_rgba`) for the FFI.
    struct Projection {
        pixels: RgbaImage,
        handle: *mut DrImage,
    }
    impl Projection {
        fn new(pixels: RgbaImage) -> Self {
            let handle = unsafe {
                dr_image_from_rgba(pixels.as_raw().as_ptr(), pixels.width(), pixels.height())
            };
            assert!(!handle.is_null());
            Self { pixels, handle }
        }
    }
    impl Drop for Projection {
        fn drop(&mut self) {
            unsafe { dr_image_free(self.handle) };
        }
    }

    /// A base with varied codes under a 3x2 partial-alpha top at (1, 1) and
    /// opacity 0.6 — the byte twin of `projection::two_layer`.
    fn two_layer() -> DrDocument {
        let base = RgbaImage::from_fn(W, H, |x, y| {
            let i = y * W + x;
            Rgba([(i * 13) as u8, 255 - (i * 11) as u8, (i * 17) as u8, 255])
        });
        let top = RgbaImage::from_fn(3, 2, |x, y| {
            let i = (y * 3 + x) as usize;
            Rgba([(i * 40) as u8, 160, 0, [0, 1, 128, 255][i % 4]])
        });
        let doc = DrDocument::from_pixels(base);
        let mut doc = doc.adding_image_layer(0, top, "top").unwrap();
        doc.layers[1].offset = (1, 1);
        doc.layers[1].opacity = 0.6;
        doc
    }

    fn requests() -> Vec<Value> {
        vec![
            json!({"operation":"histogram","source":"composite","stride":1}),
            json!({"operation":"histogram","source":"composite","stride":3}),
            json!({"operation":"sample","source":"composite","x":2,"y":2,"reach":0}),
            json!({"operation":"sample","source":"composite","x":2,"y":1,"reach":1}),
        ]
    }

    fn coverage() -> Vec<u16> {
        (0..W * H).map(|i| ((i * 7919) % 65536) as u16).collect()
    }

    unsafe fn answer(value: *mut c_char, error: *mut c_char) -> Result<Value, String> {
        if value.is_null() {
            assert!(!error.is_null());
            let text = unsafe { CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned();
            unsafe { dr_string_free(error) };
            return Err(text);
        }
        let parsed = serde_json::from_slice(unsafe { CStr::from_ptr(value) }.to_bytes()).unwrap();
        unsafe { dr_string_free(value) };
        Ok(parsed)
    }

    fn projection_ffi(
        doc: &DrDocument,
        projection: &Projection,
        request: &Value,
        coverage: &[u16],
    ) -> Result<Value, String> {
        let request = CString::new(request.to_string()).unwrap();
        let mut error = ptr::null_mut();
        unsafe {
            let out = dr_doc_projection_scoped_analysis(
                doc,
                projection.handle,
                request.as_ptr(),
                if coverage.is_empty() {
                    ptr::null()
                } else {
                    coverage.as_ptr()
                },
                coverage.len(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            );
            answer(out, error)
        }
    }

    fn doc_ffi(doc: &DrDocument, request: &Value, coverage: &[u16]) -> Result<Value, String> {
        let request = CString::new(request.to_string()).unwrap();
        let mut error = ptr::null_mut();
        unsafe {
            let out = dr_doc_scoped_analysis(
                doc,
                request.as_ptr(),
                if coverage.is_empty() {
                    ptr::null()
                } else {
                    coverage.as_ptr()
                },
                coverage.len(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            );
            answer(out, error)
        }
    }

    fn run(
        doc: &DrDocument,
        projection: &RgbaImage,
        request: &Value,
        coverage: &[u16],
    ) -> Result<Value, String> {
        doc.projection_scoped_analysis(
            projection,
            &AnalysisRequest::parse(&request.to_string()).unwrap(),
            coverage,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
    }

    fn oracle(doc: &DrDocument, request: &Value, coverage: &[u16]) -> Result<Value, String> {
        doc.scoped_analysis(
            &AnalysisRequest::parse(&request.to_string()).unwrap(),
            coverage,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
    }

    #[test]
    fn byte_projection_composite_equals_the_document_path_through_api_and_ffi() {
        let doc = two_layer();
        let projection = Projection::new(doc.flattened().unwrap());
        let weights = coverage();
        for request in requests() {
            for coverage in [&[][..], &weights[..]] {
                let expected = oracle(&doc, &request, coverage).unwrap();
                assert_eq!(
                    run(&doc, &projection.pixels, &request, coverage).unwrap(),
                    expected,
                    "{request}"
                );
                assert_eq!(
                    projection_ffi(&doc, &projection, &request, coverage).unwrap(),
                    doc_ffi(&doc, &request, coverage).unwrap(),
                    "{request}"
                );
                assert_eq!(
                    projection_ffi(&doc, &projection, &request, coverage).unwrap(),
                    expected
                );
            }
        }
        let zero = json!({"operation":"sample","source":"composite","x":0,"y":0});
        let none = vec![0u16; (W * H) as usize];
        let doc_error = oracle(&doc, &zero, &none).unwrap_err();
        assert_eq!(
            run(&doc, &projection.pixels, &zero, &none).unwrap_err(),
            doc_error
        );
        assert_eq!(
            projection_ffi(&doc, &projection, &zero, &none).unwrap_err(),
            doc_ffi(&doc, &zero, &none).unwrap_err()
        );
    }

    #[test]
    fn byte_projection_plan_is_statistics_scratch_plus_bookkeeping_below_the_render_plan() {
        let doc = two_layer();
        let request = CString::new(r#"{"operation":"histogram","source":"composite"}"#).unwrap();
        let mut error = ptr::null_mut();
        let projection_plan = unsafe {
            answer(
                dr_doc_projection_scoped_analysis_plan(&doc, request.as_ptr(), &mut error),
                error,
            )
        }
        .unwrap();
        let mut error = ptr::null_mut();
        let doc_plan = unsafe {
            answer(
                dr_doc_scoped_analysis_plan(&doc, request.as_ptr(), &mut error),
                error,
            )
        }
        .unwrap();
        assert_eq!(
            projection_plan["scratch_bytes"],
            STATISTICS_SCRATCH + 1024 * 1024
        );
        assert_eq!(projection_plan["coverage_denominator"], 65535);
        assert!(
            projection_plan["scratch_bytes"].as_u64().unwrap()
                < doc_plan["scratch_bytes"].as_u64().unwrap()
        );
    }

    #[test]
    fn byte_projection_refuses_other_sources_wrong_size_bad_coverage_budget_and_cancel() {
        let doc = two_layer();
        let projection = Projection::new(doc.flattened().unwrap());
        for request in [
            json!({"operation":"histogram","source":"layer","layer":0}),
            json!({"operation":"sample","source":"backdrop","below":1,"x":0,"y":0}),
        ] {
            let parsed = AnalysisRequest::parse(&request.to_string()).unwrap();
            assert_eq!(
                doc.projection_scoped_analysis_plan(&parsed).unwrap_err(),
                "projection analysis takes the composite source only"
            );
            assert_eq!(
                run(&doc, &projection.pixels, &request, &[]).unwrap_err(),
                "projection analysis takes the composite source only"
            );
            let text = CString::new(request.to_string()).unwrap();
            let mut error = ptr::null_mut();
            let plan = unsafe {
                answer(
                    dr_doc_projection_scoped_analysis_plan(&doc, text.as_ptr(), &mut error),
                    error,
                )
            };
            assert!(plan
                .unwrap_err()
                .contains("projection analysis takes the composite source only"));
            assert!(projection_ffi(&doc, &projection, &request, &[])
                .unwrap_err()
                .contains("projection analysis takes the composite source only"));
        }
        let histogram = json!({"operation":"histogram","source":"composite"});
        let wrong = Projection::new(RgbaImage::new(W + 1, H));
        assert_eq!(
            run(&doc, &wrong.pixels, &histogram, &[]).unwrap_err(),
            "projection is not the canvas size"
        );
        assert_eq!(
            projection_ffi(&doc, &wrong, &histogram, &[]).unwrap_err(),
            "projection is not the canvas size"
        );
        assert!(run(&doc, &projection.pixels, &histogram, &[1; 3]).is_err());
        assert!(projection_ffi(&doc, &projection, &histogram, &[1; 3]).is_err());
        let parsed = AnalysisRequest::parse(&histogram.to_string()).unwrap();
        assert!(doc
            .projection_scoped_analysis(&projection.pixels, &parsed, &[], 1, 0, None)
            .is_err());
        let token = DrJobControl::default();
        token.cancel();
        assert!(doc
            .projection_scoped_analysis(
                &projection.pixels,
                &parsed,
                &[],
                REFERENCE_WORKING_BYTES,
                0,
                Some(&token)
            )
            .unwrap_err()
            .contains("cancelled"));
        assert_eq!(token.scratch_bytes(), 0);
    }

    #[test]
    fn byte_projection_exports_are_null_safe() {
        let doc = two_layer();
        let projection = Projection::new(doc.flattened().unwrap());
        let request = CString::new(r#"{"operation":"histogram","source":"composite"}"#).unwrap();
        unsafe {
            assert!(dr_doc_projection_scoped_analysis_plan(
                ptr::null(),
                request.as_ptr(),
                ptr::null_mut()
            )
            .is_null());
            assert!(
                dr_doc_projection_scoped_analysis_plan(&doc, ptr::null(), ptr::null_mut())
                    .is_null()
            );
            for (document, image, text) in [
                (
                    ptr::null(),
                    projection.handle as *const DrImage,
                    request.as_ptr(),
                ),
                (&doc as *const DrDocument, ptr::null(), request.as_ptr()),
                (
                    &doc as *const DrDocument,
                    projection.handle as *const DrImage,
                    ptr::null(),
                ),
            ] {
                assert!(dr_doc_projection_scoped_analysis(
                    document,
                    image,
                    text,
                    ptr::null(),
                    0,
                    REFERENCE_WORKING_BYTES,
                    0,
                    ptr::null(),
                    ptr::null_mut(),
                )
                .is_null());
            }
        }
    }
}
