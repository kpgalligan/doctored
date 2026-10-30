//! The VIEW of a float projection — an exposure in stops and
//! an optional tone map other than the document's own — through
//! `dr_precision_doc_display_rgba8_view`.
//!
//! The oracle is the definition written out here: `gain = 2^stops` on RGB,
//! the map's own formula, then the analytic sRGB transfer function. The
//! document-map, zero-stop view is compared byte for byte with
//! `dr_precision_doc_display_rgba8`.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_float::{
    dr_precision_doc_display_rgba8, dr_precision_doc_display_rgba8_view,
};
use doctored_core::float_document::ToneMap;
use doctored_core::float_view::{FloatView, VIEW_EXPOSURE_STOPS_MAX};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";
const PIXELS: usize = 48 * 32;

fn fixture_document() -> PrecisionDocument {
    PrecisionDocument::open_preserving(FIXTURE, LIMIT, 0, None).expect("the float fixture opens")
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

/// The view's bytes, or the refusal.
fn view(
    document: &PrecisionDocument,
    image: &PrecisionImage,
    map: Option<&str>,
    stops: f64,
) -> Result<Vec<u8>, String> {
    let map = map.map(|text| CString::new(text).unwrap());
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_display_rgba8_view(
            document,
            image,
            map.as_ref().map_or(ptr::null(), |text| text.as_ptr()),
            stops,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        return Err(take_err(err));
    }
    let bytes =
        unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(out), PIXELS * 4) }.to_vec();
    unsafe { dr_image_free(out) };
    Ok(bytes)
}

/// The analytic sRGB transfer function, from the specification.
fn srgb_encode(linear: f64) -> f64 {
    let linear = linear.clamp(0.0, 1.0);
    if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

fn reinhard(v: f64, white: f64) -> f64 {
    let u = v.max(0.0);
    (u * (1.0 + u / (white * white)) / (1.0 + u)).min(1.0)
}

#[test]
fn the_document_view_is_the_display_path_byte_for_byte() {
    let document = fixture_document();
    let rendered = document.render(LIMIT, 0, None).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let display = unsafe {
        dr_precision_doc_display_rgba8(&document, &rendered, LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!display.is_null(), "{}", take_err(err));
    let expected =
        unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(display), PIXELS * 4) }.to_vec();
    unsafe { dr_image_free(display) };
    assert_eq!(view(&document, &rendered, None, 0.0).unwrap(), expected);
    // Naming the document's own map changes nothing either.
    assert_eq!(
        view(&document, &rendered, Some("{\"operator\":\"clip\"}"), 0.0).unwrap(),
        expected
    );
    assert_eq!(FloatView::DOCUMENT, FloatView::parse(None, 0.0).unwrap());
}

#[test]
fn an_exposure_is_a_gain_before_the_map_and_leaves_alpha_alone() {
    let document = fixture_document();
    let rendered = document.render(LIMIT, 0, None).unwrap();
    let source = rendered.to_rgba32().ok().unwrap();
    assert!(source.iter().any(|p| p[..3].iter().any(|&v| v > 1.0)));
    let at_zero = view(&document, &rendered, None, 0.0).unwrap();
    for stops in [-3.0f64, -1.5, 2.0, 20.0, -20.0] {
        let bytes = view(&document, &rendered, None, stops).unwrap();
        let gain = 2f64.powf(stops);
        for (i, pixel) in source.iter().enumerate() {
            for c in 0..3 {
                let expected = (srgb_encode(f64::from(pixel[c]) * gain) * 255.0).round() as i32;
                let got = i32::from(bytes[i * 4 + c]);
                assert!(
                    (got - expected).abs() <= 1,
                    "{stops} stops, pixel {i}:{c} = {got} vs {expected}"
                );
            }
            assert_eq!(bytes[i * 4 + 3], at_zero[i * 4 + 3], "alpha {i}");
        }
    }
    // Three stops down brings the 8.0 patch, pinned white at 0 stops, to
    // exactly white's edge and the 2.0 blue below it.
    assert_eq!(at_zero[..3], [255, 255, 255]);
    assert!(view(&document, &rendered, None, -3.0).unwrap()[2] < 255);
    assert_eq!(FloatView::parse(None, 1.0).unwrap().gain(), 2.0);
    assert_eq!(FloatView::parse(None, -3.0).unwrap().gain(), 0.125);
}

#[test]
fn a_candidate_map_is_that_maps_display_and_changes_no_document() {
    let document = fixture_document();
    let rendered = document.render(LIMIT, 0, None).unwrap();
    let mapped = document
        .with_tone_map(ToneMap::Reinhard { white: 4.0 })
        .unwrap()
        .unwrap();
    let expected = mapped.display_rgba8(&rendered, LIMIT, 0, None).unwrap();
    let candidate = "{\"operator\":\"reinhard\",\"white\":4.0}";
    assert_eq!(
        &view(&document, &rendered, Some(candidate), 0.0).unwrap(),
        expected.as_raw()
    );
    assert_eq!(document.tone_map(), Some(ToneMap::Clip));

    // The exposure applies BEFORE the candidate map.
    let source = rendered.to_rgba32().ok().unwrap();
    let bytes = view(&document, &rendered, Some(candidate), -1.0).unwrap();
    for (i, pixel) in source.iter().enumerate() {
        for c in 0..3 {
            let expected =
                (srgb_encode(reinhard(f64::from(pixel[c]) * 0.5, 4.0)) * 255.0).round() as i32;
            let got = i32::from(bytes[i * 4 + c]);
            assert!(
                (got - expected).abs() <= 1,
                "pixel {i}:{c} = {got} vs {expected}"
            );
        }
    }
}

#[test]
fn a_bad_view_and_a_document_that_is_not_float_refuse_by_name() {
    let document = fixture_document();
    let rendered = document.render(LIMIT, 0, None).unwrap();
    assert_eq!(VIEW_EXPOSURE_STOPS_MAX, 20.0);
    for stops in [20.5, -25.0, f64::NAN, f64::INFINITY] {
        let error = view(&document, &rendered, None, stops).unwrap_err();
        assert!(error.contains("view exposure"), "{stops}: {error}");
    }
    let error = view(&document, &rendered, Some("{\"operator\":\"aces\"}"), 0.0).unwrap_err();
    assert!(error.contains("not one of clip or reinhard"), "{error}");
    let error = view(&document, &rendered, Some("not json"), 0.0).unwrap_err();
    assert!(error.contains("tone map"), "{error}");

    let image = PrecisionImage::from_rgba16(
        2,
        1,
        vec![[100, 200, 300, 65535]; 2],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let plain =
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap();
    let error = view(&plain, &rendered, None, 1.0).unwrap_err();
    assert!(error.contains("32-bit float document"), "{error}");
    // A u16 image under a float document is not a float projection.
    let codes = plain.render(LIMIT, 0, None).unwrap();
    let error = view(&document, &codes, None, 1.0).unwrap_err();
    assert!(error.contains("rgba32_float"), "{error}");
}
