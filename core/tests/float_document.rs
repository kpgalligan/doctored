//! Phase 11F: the 32-bit float document's contract — the tone maps, the
//! storage gate, the evaluation invariant, the finite/alpha import rules and
//! the bounded operator set.
//!
//! Nothing here compares against a golden. The fixture's samples are read
//! back through the 11D `tiff_float` seam, which is tested independently of
//! the document path in `tiff_float_tests.rs`; every number the document
//! produces is compared against arithmetic written out in this file.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc_evaluation::Evaluation;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_float::*;
use doctored_core::float_document::{sanitize, ToneMap, FLOAT_RANGE_MAX};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::scoped_statistics::AnalysisRequest;
use doctored_core::tiff_float::{self, FloatTiff};
use tempfile::TempDir;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

fn fixture_document() -> PrecisionDocument {
    PrecisionDocument::open_preserving(FIXTURE, LIMIT, 0, None).expect("the float fixture opens")
}

fn fixture_samples() -> FloatTiff {
    let bytes = std::fs::read(FIXTURE).expect("the float fixture is committed");
    tiff_float::decode(&bytes, FIXTURE, LIMIT).expect("the 11D seam reads it")
}

fn take_err(err: *mut c_char) -> String {
    if err.is_null() {
        return String::new();
    }
    let text = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    text
}

// ------------------------------------------------------------ tone maps --

#[test]
fn tone_map_parse_admits_exactly_its_own_fields() {
    assert_eq!(
        ToneMap::parse("{\"operator\":\"clip\"}").unwrap(),
        ToneMap::Clip
    );
    // `white` is admitted for reinhard ONLY, and its absence is the default.
    assert_eq!(
        ToneMap::parse("{\"operator\":\"reinhard\"}").unwrap(),
        ToneMap::Reinhard { white: 4.0 }
    );
    assert_eq!(
        ToneMap::parse("{\"operator\":\"reinhard\",\"white\":2.5}").unwrap(),
        ToneMap::Reinhard { white: 2.5 }
    );
    let clip_white = ToneMap::parse("{\"operator\":\"clip\",\"white\":4.0}").unwrap_err();
    assert!(
        clip_white.contains("unknown tone map field \"white\""),
        "{clip_white}"
    );
    let extra = ToneMap::parse("{\"operator\":\"reinhard\",\"white\":4.0,\"x\":1}").unwrap_err();
    assert!(extra.contains("unknown tone map field \"x\""), "{extra}");
    assert!(ToneMap::parse("{\"operator\":\"aces\"}")
        .unwrap_err()
        .contains("aces"));
    assert!(ToneMap::parse("{}").unwrap_err().contains("operator"));
    assert!(ToneMap::parse("{\"operator\":\"reinhard\",\"white\":0.5}")
        .unwrap_err()
        .contains("white"));
    // The canonical writer emits `white` only where it means something.
    assert_eq!(ToneMap::Clip.to_json(), "{\"operator\":\"clip\"}");
    assert_eq!(
        ToneMap::Reinhard { white: 4.0 }.to_json(),
        "{\"operator\":\"reinhard\",\"white\":4.0}"
    );
}

#[test]
fn tone_maps_agree_with_their_formulas() {
    for v in [-1.0_f32, -0.25, 0.0, 0.5, 1.0, 4.0, 8.0, 65504.0] {
        let clip = ToneMap::Clip.apply([v; 3])[0];
        assert_eq!(clip, v.clamp(0.0, 1.0));
        for white in [1.0_f32, 4.0, 16.0] {
            let got = ToneMap::Reinhard { white }.apply([v; 3])[0];
            let u = f64::from(v).max(0.0);
            let expected =
                (u * (1.0 + u / (f64::from(white) * f64::from(white))) / (1.0 + u)).min(1.0);
            assert!(
                (f64::from(got) - expected).abs() <= 1e-6,
                "reinhard({v}, W={white}) = {got} vs {expected}"
            );
        }
        // W maps exactly to white.
        let at_white = ToneMap::Reinhard { white: 4.0 }.apply([4.0; 3])[0];
        assert!((at_white - 1.0).abs() <= 1e-6, "{at_white}");
    }
}

// ------------------------------------------------------------ the import --

#[test]
fn the_fixture_opens_as_float_with_exact_samples_and_no_warnings() {
    let document = fixture_document();
    assert!(document.is_float());
    assert_eq!(document.dimensions(), (48, 32));
    assert_eq!(document.tone_map(), Some(ToneMap::Clip));
    assert_eq!(document.evaluation(), Evaluation::LinearLight);
    assert_eq!(
        document.info().unwrap()["import_warnings"],
        serde_json::json!([])
    );
    assert_eq!(document.info().unwrap()["storage"], "rgba32_float");
    assert_eq!(document.info().unwrap()["range"], "extended");

    let source = document.layer_source(0).expect("one raster");
    assert_eq!(source.storage(), PrecisionStorage::Rgba32Float);
    let file = fixture_samples();
    let pixels = source.to_rgba32().ok().unwrap();
    assert_eq!(pixels.len() * 4, file.samples.len());
    for (i, pixel) in pixels.iter().enumerate() {
        for (c, value) in pixel.iter().enumerate() {
            assert_eq!(
                value.to_bits(),
                file.samples[i * 4 + c].to_bits(),
                "sample {i}:{c}"
            );
        }
    }
    // The three qualifying regions are genuinely there.
    assert_eq!(pixels[0][..3], [8.0, 4.0, 2.0]);
    assert_eq!(pixels[12][..3], [-0.25, 0.5, 0.75]);
    assert_eq!(pixels[24 * 48][3], 0.5);
}

#[test]
fn the_finite_and_alpha_rules_repair_at_import_and_disclose_their_counts() {
    // NaN -> 0, +inf -> 65504, -inf -> -65504, a magnitude past the range
    // clamped, and alpha clamped into [0,1]; every repair counted.
    let samples = vec![
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        2.0, // alpha above one
        1e9,
        -1e9,
        0.5,
        -0.5, // alpha below zero
    ];
    let mut copy = samples.clone();
    let (non_finite, clamped) = sanitize(&mut copy, 4);
    assert_eq!((non_finite, clamped), (3, 4));
    assert_eq!(
        copy,
        vec![
            0.0,
            FLOAT_RANGE_MAX,
            -FLOAT_RANGE_MAX,
            1.0,
            FLOAT_RANGE_MAX,
            -FLOAT_RANGE_MAX,
            0.5,
            0.0
        ]
    );

    let directory = TempDir::new().unwrap();
    let path = directory.path().join("nonfinite.tiff");
    let bytes = tiff_float::encode_to_vec(
        &FloatTiff {
            width: 2,
            height: 1,
            channels: 4,
            alpha: Some(false),
            samples,
            icc: None,
            resolution: None,
            metadata: Metadata::default(),
            non_finite: 3,
        },
        false,
    )
    .unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let document =
        PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    let warnings = document.info().unwrap()["import_warnings"].clone();
    let text = warnings.to_string();
    assert!(text.contains("3 non-finite samples replaced"), "{text}");
    assert!(text.contains("4 out-of-range samples clamped"), "{text}");
    let pixels = document.layer_source(0).unwrap().to_rgba32().ok().unwrap();
    assert_eq!(pixels[0], [0.0, FLOAT_RANGE_MAX, -FLOAT_RANGE_MAX, 1.0]);
    assert_eq!(pixels[1], [FLOAT_RANGE_MAX, -FLOAT_RANGE_MAX, 0.5, 0.0]);
}

#[test]
fn associated_alpha_is_unpremultiplied_and_disclosed() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("associated.tiff");
    let bytes = tiff_float::encode_to_vec(
        &FloatTiff {
            width: 2,
            height: 1,
            channels: 4,
            alpha: Some(true),
            samples: vec![0.25, 0.5, 1.0, 0.5, 0.3, 0.4, 0.5, 0.0],
            icc: None,
            resolution: None,
            metadata: Metadata::default(),
            non_finite: 0,
        },
        false,
    )
    .unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let document =
        PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    let pixels = document.layer_source(0).unwrap().to_rgba32().ok().unwrap();
    assert_eq!(pixels[0], [0.5, 1.0, 2.0, 0.5]);
    // Alpha zero has no colour to recover: the canonical zero, not a NaN.
    assert_eq!(pixels[1], [0.0, 0.0, 0.0, 0.0]);
    let text = document.info().unwrap()["import_warnings"].to_string();
    assert!(text.contains("associated alpha unpremultiplied"), "{text}");
}

// ------------------------------------------------------ the storage gate --

#[test]
fn from_image_with_a_float_image_still_refuses_with_the_u16_sentence() {
    let image = PrecisionImage::from_rgba32_extended(
        1,
        1,
        vec![[2.0, -0.5, 0.25, 1.0]],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let error = PrecisionDocument::from_image(image, Metadata::default(), Resolution::default())
        .err()
        .expect("an F32 image is refused");
    assert_eq!(
        error,
        "high-depth documents require explicit RGBA16 UNORM sources"
    );
}

#[test]
fn a_u16_document_is_never_float_and_answers_without_a_walk() {
    let image = PrecisionImage::from_rgba16(
        1,
        1,
        vec![[1000, 2000, 3000, 65535]],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let document =
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap();
    assert!(!document.is_float());
    assert_eq!(document.tone_map(), None);
    assert_eq!(document.float_refusal(), None);
    assert!(document.with_tone_map(ToneMap::Clip).is_err());
    assert_eq!(document.info().unwrap()["storage"], "rgba16_unorm");
}

// ----------------------------------------------- the evaluation invariant --

#[test]
fn evaluation_is_fixed_to_linear_light_on_a_float_document() {
    let document = fixture_document();
    assert!(document
        .with_evaluation(Evaluation::LinearLight)
        .unwrap()
        .is_none());
    let error = document
        .with_evaluation(Evaluation::Encoded)
        .err()
        .expect("encoded is refused");
    assert_eq!(
        error,
        "evaluation is fixed to linear light in a 32-bit float document"
    );
    // The 11E guard answers with the FLOAT table, so the host's existing
    // pre-check refuses an unadmitted float edit with no new guard.
    assert_eq!(document.linear_light_refusal(), None);
}

// ------------------------------------------------ the admitted operators --

#[test]
fn an_exposure_layer_evaluates_in_linear_light_over_extended_range() {
    let document = fixture_document();
    let edited = document
        .add_adjustment(
            "Exposure",
            "{\"type\":\"adjust\",\"evaluation\":\"precision_v1\",\"op\":\"exposure\",\"params\":{\"exposure\":1.0}}",
            1.0,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(edited.float_refusal(), None);
    let rendered = edited.render(LIMIT, 0, None).unwrap();
    assert_eq!(rendered.storage(), PrecisionStorage::Rgba32Float);
    let source = document.layer_source(0).unwrap().to_rgba32().ok().unwrap();
    let out = rendered.to_rgba32().ok().unwrap();
    for (i, pixel) in out.iter().enumerate() {
        if source[i][3] == 0.0 {
            continue;
        }
        for c in 0..3 {
            // +1 EV in linear light is a gain of exactly two; the reference
            // basis matrices are mutual inverses, so nothing else moves.
            let expected = f64::from(source[i][c]) * 2.0;
            let got = f64::from(pixel[c]);
            let tolerance = 1e-5 * expected.abs().max(1e-3);
            assert!(
                (got - expected).abs() <= tolerance,
                "pixel {i}:{c} = {got} vs {expected}"
            );
        }
        assert_eq!(pixel[3], source[i][3]);
    }
    // 8.0 became 16.0 and -0.25 became -0.5: above white and below zero both
    // survived a render.
    assert!((out[0][0] - 16.0).abs() <= 1e-3, "{}", out[0][0]);
    assert!((out[12][0] + 0.5).abs() <= 1e-5, "{}", out[12][0]);
}

#[test]
fn an_unadmitted_operator_refuses_by_name_and_blocks_the_render() {
    let document = fixture_document();
    // Invert is refused by decision: 1 − v has no meaning above white.
    let vibrance = document
        .add_adjustment(
            "Invert",
            "{\"type\":\"adjust\",\"evaluation\":\"precision_v1\",\"op\":\"invert\",\"params\":{}}",
            1.0,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let reason = vibrance.float_refusal().expect("invert is refused");
    assert!(reason.contains("invert"), "{reason}");
    assert!(
        reason.contains("is unavailable in a 32-bit float document"),
        "{reason}"
    );
    // The projection is the invariant: the refusal reaches the render too.
    assert_eq!(
        vibrance
            .render(LIMIT, 0, None)
            .err()
            .expect("the render refuses"),
        reason
    );
    assert_eq!(vibrance.linear_light_refusal(), Some(reason));
}

#[test]
fn an_unadmitted_blend_mode_refuses_by_name() {
    let document = fixture_document();
    let doubled = document.duplicate_layer(0).unwrap().unwrap();
    assert_eq!(doubled.float_refusal(), None);
    let screen = unsafe {
        let request =
            CString::new("{\"op\":\"layer_blend\",\"version\":1,\"layer\":1,\"blend_mode\":2}")
                .unwrap();
        let handle = Box::into_raw(Box::new(doubled.clone()));
        let mut err: *mut c_char = ptr::null_mut();
        let out = doctored_core::ffi_precision_edit::dr_precision_doc_edit(
            handle,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        );
        drop(Box::from_raw(handle));
        assert!(err.is_null(), "{}", take_err(err));
        assert!(!out.is_null());
        *Box::from_raw(out)
    };
    let reason = screen.float_refusal().expect("Screen is refused");
    assert!(reason.contains("screen blend mode"), "{reason}");
    // Overlay inverts about white, so it is refused too (decision B).
    let divide = unsafe {
        let request =
            CString::new("{\"op\":\"layer_blend\",\"version\":1,\"layer\":1,\"blend_mode\":3}")
                .unwrap();
        let handle = Box::into_raw(Box::new(doubled.clone()));
        let mut err: *mut c_char = ptr::null_mut();
        let out = doctored_core::ffi_precision_edit::dr_precision_doc_edit(
            handle,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        );
        drop(Box::from_raw(handle));
        assert!(err.is_null(), "{}", take_err(err));
        assert!(!out.is_null());
        *Box::from_raw(out)
    };
    let reason = divide.float_refusal().expect("Overlay is refused");
    assert!(reason.contains("the overlay blend mode"), "{reason}");
    // Multiply, Darken, Lighten and Difference beside Normal, the nine
    // modes measured and built (Linear Dodge, Subtract, Dissolve, Darker
    // and Lighter Color, Hue, Saturation, Color, Luminosity) and the added four
    // (Linear Burn, Linear Light, Pin Light, Divide): none refuses.
    for mode in [
        1, 6, 7, 8, 12, 13, 14, 15, 16, 17, 19, 20, 22, 23, 24, 25, 26,
    ] {
        let request =
            format!("{{\"op\":\"layer_blend\",\"version\":1,\"layer\":1,\"blend_mode\":{mode}}}");
        let edited = unsafe {
            let text = CString::new(request).unwrap();
            let handle = Box::into_raw(Box::new(doubled.clone()));
            let mut err: *mut c_char = ptr::null_mut();
            let out = doctored_core::ffi_precision_edit::dr_precision_doc_edit(
                handle,
                text.as_ptr(),
                ptr::null(),
                0,
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            );
            drop(Box::from_raw(handle));
            assert!(err.is_null(), "{}", take_err(err));
            *Box::from_raw(out)
        };
        assert_eq!(edited.float_refusal(), None, "mode {mode}");
        assert!(edited.render(LIMIT, 0, None).is_ok(), "mode {mode}");
    }
}

// ------------------------------------------------- display and readouts --

#[test]
fn display_goes_through_the_document_map_and_direct_conversion_refuses() {
    let document = fixture_document();
    let rendered = document.render(LIMIT, 0, None).unwrap();
    assert!(rendered
        .to_rgba8(LIMIT, 0, None)
        .unwrap_err()
        .contains("tone map"));
    let clipped = document.display_rgba8(&rendered, LIMIT, 0, None).unwrap();
    let mapped = document
        .with_tone_map(ToneMap::Reinhard { white: 4.0 })
        .unwrap()
        .unwrap();
    let reinhard = mapped.display_rgba8(&rendered, LIMIT, 0, None).unwrap();
    // The above-white patch is pinned white under clip and is genuinely
    // below white under a Reinhard map that puts W at 4.
    assert_eq!(clipped.get_pixel(0, 0).0[..3], [255, 255, 255]);
    // Blue is 2.0 there, half of W: Reinhard keeps it genuinely below white
    // while clip has already thrown the distinction away.
    assert!(
        reinhard.get_pixel(0, 0).0[2] < 255,
        "{}",
        reinhard.get_pixel(0, 0).0[2]
    );
    assert_eq!(reinhard.get_pixel(0, 0).0[0], 255);
    // A negative component reads as zero either way.
    assert_eq!(clipped.get_pixel(12, 0).0[0], 0);
    assert_eq!(reinhard.get_pixel(12, 0).0[0], 0);
    assert_eq!(mapped.tone_map(), Some(ToneMap::Reinhard { white: 4.0 }));
    // An unchanged map is a no-op, never a phantom edit.
    assert!(mapped
        .with_tone_map(ToneMap::Reinhard { white: 4.0 })
        .unwrap()
        .is_none());
}

#[test]
fn the_extended_readouts_report_above_white_and_below_zero() {
    let document = fixture_document();
    let rendered = document.render(LIMIT, 0, None).unwrap();
    let sample = rendered.float_sample(0, 0, 0).unwrap();
    assert_eq!(sample["storage"], "rgba32_float");
    assert_eq!(sample["center_rgba32"][0], 8.0);
    assert_eq!(sample["above_white"][0], true);
    assert_eq!(sample["below_zero"][0], false);
    assert!((sample["stops"][0].as_f64().unwrap() - 3.0).abs() < 1e-9);
    let negative = rendered.float_sample(12, 0, 0).unwrap();
    assert_eq!(negative["below_zero"][0], true);

    let request =
        AnalysisRequest::parse(r#"{"operation":"histogram","source":"composite"}"#).unwrap();
    let histogram = rendered
        .scoped_analysis(&request, &[], LIMIT, 0, None)
        .unwrap();
    assert_eq!(histogram["storage"], "rgba32_float");
    assert_eq!(histogram["schema_version"], 2);
    assert_eq!(histogram["range"]["min_ev"], -12.0);
    assert_eq!(histogram["range"]["max_ev"], 6.0);
    assert_eq!(histogram["sampled_pixels"], 48 * 32);
    assert_eq!(histogram["max_rgba"][0], 8.0);
    assert_eq!(histogram["min_rgba"][0], -0.25);
    assert!(histogram["above_white_rgba"][0].as_u64().unwrap() > 0);
    assert!(histogram["below_zero_rgba"][0].as_u64().unwrap() > 0);
    assert!(
        histogram["above_white_weight_units_rgba"][0]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        histogram["below_zero_weight_units_rgba"][0]
            .as_u64()
            .unwrap()
            > 0
    );
    let bins = histogram["bin_weight_units"][0].as_array().unwrap();
    assert_eq!(bins.len(), 256);
    let total: u64 = bins.iter().map(|v| v.as_u64().unwrap()).sum();
    assert_eq!(
        total,
        histogram["sampled_rgb_weight_units"].as_u64().unwrap()
    );
}

// ----------------------------------------------------------------- FFI --

#[test]
fn the_float_exports_tolerate_null_and_report_the_mode() {
    unsafe {
        assert!(!dr_precision_doc_is_float(ptr::null()));
        assert!(dr_precision_doc_tone_map(ptr::null()).is_null());
        assert!(dr_precision_doc_float_refusal(ptr::null()).is_null());
        let mut err: *mut c_char = ptr::null_mut();
        assert!(dr_precision_doc_set_tone_map(ptr::null(), ptr::null(), &mut err).is_null());
        assert!(!take_err(err).is_empty());
        let mut err: *mut c_char = ptr::null_mut();
        assert!(
            dr_precision_doc_float_to_u16(ptr::null(), LIMIT, 0, ptr::null(), &mut err).is_null()
        );
        assert!(!take_err(err).is_empty());
        let mut err: *mut c_char = ptr::null_mut();
        assert!(dr_precision_doc_display_rgba8(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!take_err(err).is_empty());
        let mut err: *mut c_char = ptr::null_mut();
        assert!(dr_precision_doc_display_rgba8_view(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0.0,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!take_err(err).is_empty());
        let mut err: *mut c_char = ptr::null_mut();
        assert!(dr_precision_doc_display_thumbnail_rgba8(
            ptr::null(),
            ptr::null(),
            34,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!take_err(err).is_empty());
        let mut err: *mut c_char = ptr::null_mut();
        assert!(dr_precision_image_float_sample(ptr::null(), 0, 0, 0, &mut err).is_null());
        assert!(!take_err(err).is_empty());

        let table = dr_float_operators();
        assert!(!table.is_null());
        let text = take_err(table);
        assert!(text.contains("\"admitted\""), "{text}");
        assert!(text.contains("\"refused\""), "{text}");
        assert!(text.contains("reinhard"), "{text}");

        let document = Box::into_raw(Box::new(fixture_document()));
        assert!(dr_precision_doc_is_float(document));
        let map = dr_precision_doc_tone_map(document);
        assert_eq!(take_err(map), "{\"operator\":\"clip\"}");
        assert!(dr_precision_doc_float_refusal(document).is_null());
        // `clip` with a white point reaches the core refusal, which is why
        // the host forwards `white` verbatim.
        let request = CString::new("{\"operator\":\"clip\",\"white\":2.0}").unwrap();
        let mut err: *mut c_char = ptr::null_mut();
        assert!(dr_precision_doc_set_tone_map(document, request.as_ptr(), &mut err).is_null());
        assert!(take_err(err).contains("unknown tone map field \"white\""));
        // An unchanged map is NULL WITHOUT an error.
        let same = CString::new("{\"operator\":\"clip\"}").unwrap();
        let mut err: *mut c_char = ptr::null_mut();
        assert!(dr_precision_doc_set_tone_map(document, same.as_ptr(), &mut err).is_null());
        assert!(err.is_null());
        drop(Box::from_raw(document));
    }
}
