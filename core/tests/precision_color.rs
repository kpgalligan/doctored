//! Typed color/FFI qualification with independent analytic gamma and matrix
//! references. Fixtures contain genuine u16 codes, fractional alpha and latent
//! RGB; no byte ramp is promoted to manufacture precision evidence.

use crate::common;
use crate::group_roots;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use group_roots::GroupRoots;
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_comps::dr_precision_doc_layer_comps_json;
use doctored_core::ffi_output_color::dr_precision_doc_conversion_preflight;
use doctored_core::ffi_precision::*;
use doctored_core::ffi_precision_color::*;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// Minimal matrix/TRC ICC built independently from the native writer and
/// production built-ins. Diagonal matrices isolate the analytic conversion.
fn profile(gamma: u16, red_scale: i32) -> Arc<IccProfile> {
    let mut curve = b"curv\0\0\0\0".to_vec();
    curve.extend_from_slice(&1u32.to_be_bytes());
    curve.extend_from_slice(&gamma.to_be_bytes());
    let mut tags = Vec::new();
    for (axis, signature) in [b"rXYZ", b"gXYZ", b"bXYZ"].into_iter().enumerate() {
        let mut xyz = b"XYZ \0\0\0\0".to_vec();
        for c in 0..3 {
            let value: i32 = if c == axis {
                if c == 0 {
                    red_scale
                } else {
                    65536
                }
            } else {
                0
            };
            xyz.extend_from_slice(&value.to_be_bytes());
        }
        tags.push((*signature, xyz));
    }
    for key in [b"rTRC", b"gTRC", b"bTRC"] {
        tags.push((*key, curve.clone()));
    }
    let mut bytes = vec![0; 132 + 12 * tags.len()];
    bytes[8..12].copy_from_slice(&0x0210_0000u32.to_be_bytes());
    bytes[12..16].copy_from_slice(b"mntr");
    bytes[16..20].copy_from_slice(b"RGB ");
    bytes[20..24].copy_from_slice(b"XYZ ");
    bytes[36..40].copy_from_slice(b"acsp");
    bytes[128..132].copy_from_slice(&(tags.len() as u32).to_be_bytes());
    for (i, (key, payload)) in tags.into_iter().enumerate() {
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        let offset = bytes.len();
        let row = 132 + 12 * i;
        bytes[row..row + 4].copy_from_slice(&key);
        bytes[row + 4..row + 8].copy_from_slice(&(offset as u32).to_be_bytes());
        bytes[row + 8..row + 12].copy_from_slice(&(payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&payload);
    }
    let length = bytes.len() as u32;
    bytes[..4].copy_from_slice(&length.to_be_bytes());
    Arc::new(IccProfile::parse(&bytes).unwrap())
}

fn document(pixels: Vec<[u16; 4]>, profile: Arc<IccProfile>) -> PrecisionDocument {
    let image = PrecisionImage::from_rgba16(pixels.len() as u32, 1, pixels, profile, None).unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}

struct Doc(*mut PrecisionDocument);
impl Doc {
    fn new(document: PrecisionDocument) -> Self {
        Self(Box::into_raw(Box::new(document)))
    }
    fn get(&self) -> &PrecisionDocument {
        unsafe { &*self.0 }
    }
}
impl Drop for Doc {
    fn drop(&mut self) {
        unsafe { dr_precision_doc_free(self.0) }
    }
}
unsafe fn text(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let string = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    string
}
fn description(op: &str, params: serde_json::Value) -> CString {
    CString::new(
        serde_json::json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":params})
            .to_string(),
    )
    .unwrap()
}
fn color(doc: &Doc, profile: &Arc<IccProfile>, convert: bool) -> Doc {
    let mut error = ptr::null_mut();
    let function = if convert {
        dr_precision_doc_convert_to_profile
    } else {
        dr_precision_doc_assign_profile
    };
    let next = unsafe {
        function(
            doc.0,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!next.is_null(), "{}", unsafe { text(error) });
    assert!(error.is_null());
    Doc(next)
}

#[test]
fn assignment_is_exact_and_conversion_preserves_subbyte_rgb_and_alpha() {
    let pixels: Vec<_> = (0..=u16::MAX).map(|v| [v, 65535 - v, v / 2, v]).collect();
    let src = profile(512, 65536);
    let dst = profile(256, 65536);
    let original = Doc::new(document(pixels.clone(), src));
    unsafe {
        let mut error = ptr::null_mut();
        let compact = dr_precision_doc_descriptor(original.0, &mut error);
        assert!(!compact.is_null(), "{}", text(error));
        let compact: serde_json::Value = serde_json::from_str(&text(compact)).unwrap();
        assert_eq!(compact["storage"], "rgba16_unorm");
        assert_eq!(compact["layer_count"], 1);
        assert!(compact.get("layers").is_none());
        assert!(compact.get("import_warnings").is_none());
    }
    let assigned = color(&original, &dst, false);
    assert_eq!(
        assigned
            .get()
            .layer_source(0)
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap(),
        pixels
    );
    assert!(PrecisionImage::ptr_eq(
        assigned.get().layer_source(0).unwrap(),
        original.get().layer_source(0).unwrap()
    ));
    let converted = color(&original, &dst, true);
    let actual = converted
        .get()
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .ok()
        .unwrap();
    for (input, actual) in pixels.iter().zip(actual.iter()) {
        assert_eq!(actual[3], input[3]);
        for c in 0..3 {
            let expected = ((f64::from(input[c]) / 65535.0).powi(2) * 65535.0).round() as u16;
            assert!(
                actual[c].abs_diff(expected) <= 1,
                "{input:?}: {actual:?}, expected{expected}"
            );
        }
    }
    assert_ne!(actual[40000][0], actual[40001][0]);
    assert_eq!(
        original
            .get()
            .layer_source(0)
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap(),
        pixels
    );
    let duplicate = original
        .get()
        .duplicate_layer(0)
        .unwrap()
        .unwrap()
        .group_all_pass_through(0.75)
        .unwrap()
        .unwrap();
    let plan = duplicate.color_plan(&dst, true).unwrap();
    assert_eq!(plan.converted_pixel_bytes, pixels.len() as u64 * 8);
    let duplicated = duplicate
        .convert_to_profile(&dst, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert!(PrecisionImage::ptr_eq(
        duplicated.layer_source(0).unwrap(),
        duplicated.layer_source(1).unwrap()
    ));
}

#[test]
fn high_depth_does_not_skip_a_subbyte_matrix_difference() {
    let src = profile(256, 65536);
    let dst = profile(256, 65537);
    assert!(
        src.model().unwrap().approx_eq(dst.model().unwrap()),
        "fixture is within historical byte tolerance"
    );
    let input = [65535, 22222, 33333, 1];
    let doc = document(vec![input], src);
    let converted = doc
        .convert_to_profile(&dst, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let actual = converted.layer_source(0).unwrap().to_rgba16().ok().unwrap()[0];
    assert_eq!(actual, [65534, 22222, 33333, 1]);
    assert!(converted
        .convert_to_profile(&dst, LIMIT, 0, None)
        .unwrap()
        .is_none());
    assert!(converted
        .assign_profile(&dst, LIMIT, 0, None)
        .unwrap()
        .is_none());
}

#[test]
fn gamma_encoding_has_a_separate_forward_interpolation_bound() {
    // A forward grid interval is1/65536 encoded units. Its interpolation
    // contributes at most one interval, independently of compositor error;
    // include existing float2e-6 and final half-u16 rounding separately.
    let bound = 1.0 / 65536.0 + 2e-6 + 0.5 / 65535.0;
    for (source_gamma, target_gamma) in [(256, 1024), (563, 461), (128, 768)] {
        let pixels: Vec<_> = (0..=u16::MAX).map(|v| [v, v, v, 65535]).collect();
        let doc = document(pixels, profile(source_gamma, 65536));
        let converted = doc
            .convert_to_profile(&profile(target_gamma, 65536), LIMIT, 0, None)
            .unwrap()
            .unwrap();
        let mut maximum = 0.0_f64;
        for (i, pixel) in converted
            .layer_source(0)
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap()
            .iter()
            .enumerate()
        {
            let expected =
                (i as f64 / 65535.0).powf(f64::from(source_gamma) / f64::from(target_gamma));
            maximum = maximum.max((f64::from(pixel[0]) / 65535.0 - expected).abs());
        }
        assert!(
            maximum <= bound,
            "gamma{source_gamma}/{target_gamma}: max{maximum}, bound{bound}"
        );
        eprintln!("ICC gamma{source_gamma}/{target_gamma} maximum normalized error{maximum:.9e}");
    }
}

#[test]
fn typed_adjustment_exports_capture_state_and_refuse_wrong_targets() {
    let input = [[32123, 12345, 56789, 0], [32124, 12346, 56790, 12345]];
    let doc = Doc::new(document(input.to_vec(), profile(256, 65536)));
    let invert = description("invert", serde_json::json!({}));
    unsafe {
        let mut error = ptr::null_mut();
        let plan = dr_precision_doc_adjustment_plan(doc.0, -1, invert.as_ptr(), false, &mut error);
        assert!(!plan.is_null(), "{}", text(error));
        let plan: serde_json::Value = serde_json::from_str(&text(plan)).unwrap();
        assert!(plan["scratch_bytes"].as_u64().unwrap() > 0);
        let named = CString::new("Invert").unwrap();
        let live = Doc(dr_precision_doc_add_adjustment(
            doc.0,
            named.as_ptr(),
            invert.as_ptr(),
            0.75,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        ));
        assert!(!live.0.is_null(), "{}", text(error));
        assert_eq!(
            live.get()
                .layer_source(0)
                .unwrap()
                .to_rgba16()
                .ok()
                .unwrap(),
            input
        );
        assert!(dr_precision_doc_set_adjustment(
            live.0,
            1,
            invert.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error
        )
        .is_null());
        assert!(error.is_null());
        let gray = description("grayscale", serde_json::json!({}));
        let changed = Doc(dr_precision_doc_set_adjustment(
            live.0,
            1,
            gray.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        ));
        assert!(!changed.0.is_null(), "{}", text(error));
        assert_ne!(
            changed.get().adjustment_description(1),
            live.get().adjustment_description(1)
        );
        let applied = Doc(dr_precision_doc_apply_adjustment(
            doc.0,
            0,
            invert.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        ));
        assert!(!applied.0.is_null(), "{}", text(error));
        for (before, after) in input.iter().zip(
            applied
                .get()
                .layer_source(0)
                .unwrap()
                .to_rgba16()
                .ok()
                .unwrap()
                .iter(),
        ) {
            let expected = if before[3] == 0 {
                *before
            } else {
                [
                    65535 - before[0],
                    65535 - before[1],
                    65535 - before[2],
                    before[3],
                ]
            };
            assert_eq!(after, &expected);
        }
        assert!(dr_precision_doc_apply_adjustment(
            live.0,
            1,
            invert.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error
        )
        .is_null());
        assert!(!text(error).is_empty());
        error = ptr::null_mut();
        assert!(
            dr_precision_doc_adjustment_plan(doc.0, -2, invert.as_ptr(), false, &mut error)
                .is_null()
        );
        assert!(!text(error).is_empty());
    }
}

#[test]
fn color_plans_budget_refusals_and_cancellation_preserve_sources() {
    let src = profile(512, 65536);
    let dst = profile(256, 65536);
    let input = [[32123, 12345, 56789, 0], [32124, 12346, 56790, 12345]];
    let doc = Doc::new(document(input.to_vec(), src));
    let plan = doc.get().color_plan(&dst, true).unwrap();
    assert!(doc
        .get()
        .convert_to_profile(&dst, plan.retained_bytes + plan.scratch_bytes - 1, 0, None)
        .is_err());
    assert!(doc
        .get()
        .convert_to_profile(&dst, LIMIT, u64::MAX, None)
        .is_err());
    let control = DrJobControl::default();
    control.cancel();
    assert!(doc
        .get()
        .convert_to_profile(&dst, LIMIT, 0, Some(&control))
        .unwrap_err_string()
        .contains("cancel"));
    assert_eq!(control.scratch_bytes(), 0);
    unsafe {
        let cap = DrJobControl::default();
        assert!(doctored_core::ffi_jobs::dr_job_control_set_scratch_limit(
            &cap, 1
        ));
        let mut cap_error = ptr::null_mut();
        assert!(dr_precision_doc_assign_profile(
            doc.0,
            dst.bytes().as_ptr(),
            dst.bytes().len(),
            LIMIT,
            0,
            &cap,
            &mut cap_error
        )
        .is_null());
        assert!(text(cap_error).contains("working memory"));
        assert_eq!(cap.scratch_bytes(), 0);
        let mut error = ptr::null_mut();
        let plan = dr_precision_doc_color_plan(
            doc.0,
            dst.bytes().as_ptr(),
            dst.bytes().len(),
            true,
            LIMIT,
            0,
            &mut error,
        );
        assert!(!plan.is_null(), "{}", text(error));
        let plan: serde_json::Value = serde_json::from_str(&text(plan)).unwrap();
        assert_eq!(plan["converted_pixel_bytes"], 16);
        assert!(dr_precision_doc_color_plan(
            doc.0,
            dst.bytes().as_ptr(),
            dst.bytes().len(),
            true,
            LIMIT,
            LIMIT,
            &mut error
        )
        .is_null());
        assert!(!text(error).is_empty());
        error = ptr::null_mut();
        assert!(dr_precision_doc_assign_profile(
            doc.0,
            ptr::null(),
            10,
            LIMIT,
            0,
            ptr::null(),
            &mut error
        )
        .is_null());
        assert!(!text(error).is_empty());
        error = ptr::null_mut();
        assert!(dr_precision_doc_convert_to_profile(
            doc.0,
            dst.bytes().as_ptr(),
            dst.bytes().len(),
            1,
            0,
            ptr::null(),
            &mut error
        )
        .is_null());
        assert!(!text(error).is_empty());
    }
    assert_eq!(
        *doc.get().layer_source(0).unwrap().to_rgba16().ok().unwrap(),
        input
    );
}

#[test]
fn a_layer_comp_survives_assign_and_convert_and_stays_bound() {
    // 12E-2: a profile edit changes no RECORD, so the document's LAYER COMPS
    // ride along beside the guides and the ruler origin — and they bind by
    // the id every record kept, so nothing goes stale.
    let source = document(vec![[9000, 21000, 33000, 65535]; 4], profile(2048, 65536));
    let request = CString::new(
        serde_json::json!({"op": "layer_comp", "action": "save", "name": "A"}).to_string(),
    )
    .unwrap();
    let mut error = ptr::null_mut();
    let saved = unsafe {
        dr_precision_doc_edit(
            &source,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!saved.is_null(), "{}", unsafe { text(error) });
    let saved = Doc(saved);
    let listing = |doc: &Doc| -> serde_json::Value {
        let mut err = ptr::null_mut();
        let json = unsafe { dr_precision_doc_layer_comps_json(doc.get(), &mut err) };
        assert!(!json.is_null(), "{}", unsafe { text(err) });
        serde_json::from_str(&unsafe { text(json) }).expect("comp JSON")
    };
    let before = listing(&saved);
    assert_eq!(before["comps"][0]["name"], serde_json::json!("A"));
    assert_eq!(before["comps"][0]["stale_count"], serde_json::json!(0));

    for convert in [false, true] {
        let next = color(&saved, &profile(4096, 32768), convert);
        let after = listing(&next);
        assert_eq!(
            after,
            before,
            "the comp survives {} unchanged and still binds",
            if convert { "a convert" } else { "an assign" }
        );
    }
}

#[test]
fn incompatible_profile_rebinding_refuses_before_changing_any_source() {
    let src = profile(512, 65536);
    let mut unmodelled = src.bytes().to_vec();
    // Remove one matrix tag while retaining a valid RGB ICC container. This
    // is a retained interpretation, but cannot support calibrated evaluation.
    unmodelled[132..136].copy_from_slice(b"priv");
    let unmodelled = Arc::new(IccProfile::parse(&unmodelled).unwrap());
    assert!(unmodelled.model().is_none());
    let source = document(vec![[32123, 12345, 56789, 22222]], src);
    let assigned = source
        .assign_profile(&unmodelled, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        assigned.layer_source(0).unwrap().to_rgba16().ok(),
        source.layer_source(0).unwrap().to_rgba16().ok()
    );
    assert!(source
        .convert_to_profile(&unmodelled, LIMIT, 0, None)
        .is_err());
    let exposure = description(
        "exposure",
        serde_json::json!({"exposure": 0.7, "evaluation":"document_v2"}),
    );
    let live = source
        .add_adjustment("Exposure", exposure.to_str().unwrap(), 1.0, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let before = live.encode_native(LIMIT, 0, None).unwrap();
    assert!(live.assign_profile(&unmodelled, LIMIT, 0, None).is_err());
    assert_eq!(live.encode_native(LIMIT, 0, None).unwrap(), before);
}

#[test]
fn a_lut_assigned_high_depth_document_never_offers_calibrated_it_refuses() {
    let src = profile(512, 65536);
    let mut unmodelled = src.bytes().to_vec();
    // The same priv-tag patch as the rebinding test: a valid RGB container
    // with no matrix/TRC model.
    unmodelled[132..136].copy_from_slice(b"priv");
    let mut destinations = vec![Arc::new(IccProfile::parse(&unmodelled).unwrap())];
    match std::fs::read("/Library/Application Support/Adobe/Color/Profiles/VideoHD16-235.icc") {
        Ok(bytes) => destinations.push(Arc::new(IccProfile::parse(&bytes).unwrap())),
        Err(_) => eprintln!("skipping the LUT half: VideoHD16-235.icc is not installed"),
    }
    let exposure = description(
        "exposure",
        serde_json::json!({"exposure": 0.7, "evaluation":"document_v2"}),
    );
    let name = CString::new("Exposure").unwrap();
    // The query must answer exactly what calibrated Exposure does.
    let offers_and_accepts = |doc: &Doc| -> (bool, bool, String) {
        let mut error = ptr::null_mut();
        let offered = unsafe { dr_precision_doc_profile_is_convertible(doc.0) };
        let live = unsafe {
            dr_precision_doc_add_adjustment(
                doc.0,
                name.as_ptr(),
                exposure.as_ptr(),
                1.0,
                LIMIT,
                0,
                ptr::null(),
                &mut error,
            )
        };
        let accepted = !live.is_null();
        if accepted {
            unsafe { dr_precision_doc_free(live) };
        }
        (offered, accepted, unsafe { text(error) })
    };
    let source = Doc::new(document(vec![[32123, 12345, 56789, 22222]], src));
    let (offered, accepted, error) = offers_and_accepts(&source);
    assert!(offered, "a matrix/TRC source offers calibrated edits");
    assert!(accepted, "{error}");
    for destination in &destinations {
        let assigned = color(&source, destination, false);
        let (offered, accepted, error) = offers_and_accepts(&assigned);
        assert_eq!(offered, accepted);
        assert!(
            !offered,
            "an unmodelled profile never offers calibrated edits"
        );
        assert!(error.contains("matrix/TRC"), "{error}");
    }
}

/// The typed conversion preflight into `profile`: `Ok` with the plan JSON or
/// `Err` with the refusal.
fn preflight(doc: &Doc, profile: &Arc<IccProfile>) -> Result<String, String> {
    let mut error = ptr::null_mut();
    let json = unsafe {
        dr_precision_doc_conversion_preflight(
            doc.0,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            ptr::null(),
            LIMIT,
            0,
            std::ptr::null(),
            &mut error,
        )
    };
    if json.is_null() {
        Err(unsafe { text(error) })
    } else {
        assert!(error.is_null());
        Ok(unsafe { text(json) })
    }
}

#[test]
fn a_conversion_refuses_a_failing_destination_and_converts_out_of_a_failing_source() {
    // A gamma-0 `curv` parses to a matrix/TRC model that is marked
    // precision-incompatible, so encoding INTO it would refuse; the
    // preflight must say so before the user commits. Converting OUT
    // of it decodes through the parser's repair, gamma 0 as linear.
    let bad = profile(0, 65536);
    let good = profile(512, 65536);
    assert!(bad.model().is_some());
    let doc = Doc::new(document(vec![[32123, 12345, 56789, 22222]], good.clone()));

    let e = preflight(&doc, &bad).unwrap_err();
    assert!(e.starts_with("Doctored cannot convert into"), "{e}");
    let plan = |doc: &Doc, convert: bool| -> Result<(), String> {
        let mut error = ptr::null_mut();
        let json = unsafe {
            dr_precision_doc_color_plan(
                doc.0,
                bad.bytes().as_ptr(),
                bad.bytes().len(),
                convert,
                LIMIT,
                0,
                &mut error,
            )
        };
        if json.is_null() {
            Err(unsafe { text(error) })
        } else {
            unsafe { text(json) };
            Ok(())
        }
    };
    let e = plan(&doc, true).unwrap_err();
    assert!(e.contains("high-depth profile"), "{e}");
    plan(&doc, false).expect("an assign plan never validates the curves");

    // Assign still reinterprets into the bad profile, and converting OUT of
    // it is planned and run: `bad` decodes linear, `good` encodes gamma 2 over
    // the same primaries, so the oracle is sqrt per channel; alpha is exact.
    let ramp = [0u16, 1, 255, 1000, 12345, 32768, 50000, 65534, 65535];
    let pixels: Vec<[u16; 4]> = ramp
        .iter()
        .enumerate()
        .map(|(i, &v)| [v, 65535 - v, v / 3, ramp[(i + 3) % ramp.len()]])
        .collect();
    let tagged = Doc::new(document(pixels.clone(), good.clone()));
    let assigned = color(&tagged, &bad, false);
    preflight(&assigned, &good).expect("a failing source converts out");
    let mut error = ptr::null_mut();
    let json = unsafe {
        dr_precision_doc_color_plan(
            assigned.0,
            good.bytes().as_ptr(),
            good.bytes().len(),
            true,
            LIMIT,
            0,
            &mut error,
        )
    };
    assert!(!json.is_null(), "{}", unsafe { text(error) });
    unsafe { text(json) };
    let converted = color(&assigned, &good, true);
    let actual = converted
        .get()
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .ok()
        .unwrap();
    for (pixel, input) in actual.iter().zip(&pixels) {
        for c in 0..3 {
            let oracle = ((f64::from(input[c]) / 65535.0).sqrt() * 65535.0).round();
            assert!(
                (f64::from(pixel[c]) - oracle).abs() <= 1.0,
                "{input:?} -> {pixel:?}"
            );
        }
        assert_eq!(pixel[3], input[3]);
    }
    assert!(!unsafe { dr_precision_doc_profile_is_convertible(assigned.0) });

    // The preflight into P accepts exactly when a document assigned P offers
    // conversion.
    for destination in [&good, &bad] {
        let accepts = preflight(&doc, destination).is_ok();
        // `doc` already carries `good`, so assigning it is the exact no-op.
        let offered = if Arc::ptr_eq(destination, &good) {
            unsafe { dr_precision_doc_profile_is_convertible(doc.0) }
        } else {
            let assigned = color(&doc, destination, false);
            unsafe { dr_precision_doc_profile_is_convertible(assigned.0) }
        };
        assert_eq!(accepts, offered);
    }
}

#[test]
fn cancellation_during_color_work_releases_reservations_and_keeps_source() {
    let input = [32123, 12345, 56789, 22222];
    let source = document(vec![input; 1_000_000], profile(512, 65536));
    let target = profile(256, 65536);
    let render = |token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match source.convert_to_profile(&target, LIMIT, 0, Some(control)) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    };
    // k = 0: the conversion polls once between the entry's report and the
    // next (the next report is at n0 + 2), so n0 + 1 is in-kernel.
    let (entry, next) = entry_then_cancel(&|progress, _, _| progress >= 0.05, render);
    let n0 = entry.checkpoints;
    assert!(
        next.expect("the entry's kernel never reported again")
            .checkpoints
            > n0 + 1,
        "the entry's kernel polls no checkpoint before its next report"
    );
    let parked = cancelled_parked_at(
        n0 + 1,
        &|progress, stage| stage == entry.stage && progress == entry.progress,
        render,
    );
    eprintln!("color conversion cancellation: {}", parked_line(&parked));
    assert!(source
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .ok()
        .unwrap()
        .iter()
        .all(|pixel| *pixel == input));
}

// Avoid a Debug bound on immutable document owners in failure assertions.
trait ErrorString {
    fn unwrap_err_string(self) -> String;
}
impl<T> ErrorString for Result<T, String> {
    fn unwrap_err_string(self) -> String {
        match self {
            Err(e) => e,
            Ok(_) => panic!("expected refusal"),
        }
    }
}

#[test]
fn every_new_color_export_tolerates_null() {
    unsafe {
        let d = ptr::null();
        let s = ptr::null();
        let e = ptr::null_mut();
        assert!(dr_precision_doc_descriptor(d, e).is_null());
        assert!(dr_precision_doc_adjustment_plan(d, -1, s, false, e).is_null());
        assert!(dr_precision_doc_add_adjustment(d, s, s, 1.0, LIMIT, 0, ptr::null(), e).is_null());
        assert!(dr_precision_doc_set_adjustment(d, 0, s, LIMIT, 0, ptr::null(), e).is_null());
        assert!(dr_precision_doc_apply_adjustment(d, 0, s, LIMIT, 0, ptr::null(), e).is_null());
        assert!(dr_precision_doc_color_plan(d, ptr::null(), 0, true, LIMIT, 0, e).is_null());
        assert!(
            dr_precision_doc_assign_profile(d, ptr::null(), 0, LIMIT, 0, ptr::null(), e).is_null()
        );
        assert!(
            dr_precision_doc_convert_to_profile(d, ptr::null(), 0, LIMIT, 0, ptr::null(), e)
                .is_null()
        );
        assert!(!dr_precision_doc_profile_is_convertible(ptr::null()));
    }
}

/// The byte trio (`dr_doc_assign_profile`, `dr_doc_convert_to_profile`,
/// `dr_doc_assign_profile_rasterizing`) and the typed pair name a bad profile
/// argument with the SAME sentence — one home, `ffi_color::check_profile_arg`
/// and `required_profile`.
#[test]
fn byte_and_typed_profile_edits_share_the_argument_sentences() {
    use doctored_core::doc::DrDocument;
    use doctored_core::ffi_color::{dr_doc_assign_profile, dr_doc_convert_to_profile};
    use doctored_core::ffi_doc::dr_doc_free;
    use doctored_core::ffi_output_color::dr_doc_assign_profile_rasterizing;

    let byte = Box::into_raw(Box::new(DrDocument::from_pixels(image::RgbaImage::new(
        2, 2,
    ))));
    let typed = Doc::new(document(vec![[1, 2, 3, 65535]; 2], profile(512, 65536)));
    let over = vec![0u8; 16 * 1024 * 1024 + 1];
    let junk = [7u8; 132];
    let cases: [(*const u8, usize, &str); 4] = [
        (
            ptr::null(),
            10,
            "the profile is not an RGB ICC profile a document can carry",
        ),
        // Non-NULL but empty: passes the argument check, fails the parse.
        (
            junk.as_ptr(),
            0,
            "the profile is not an RGB ICC profile a document can carry",
        ),
        (
            over.as_ptr(),
            over.len(),
            "the profile exceeds the 16 MiB limit",
        ),
        (
            junk.as_ptr(),
            junk.len(),
            "the profile is not an RGB ICC profile a document can carry",
        ),
    ];
    for (bytes, len, expected) in cases {
        let mut answers = Vec::new();
        unsafe {
            for byte_op in [
                dr_doc_assign_profile,
                dr_doc_convert_to_profile,
                dr_doc_assign_profile_rasterizing,
            ] {
                let mut error = ptr::null_mut();
                assert!(byte_op(byte, bytes, len, &mut error).is_null());
                answers.push(text(error));
            }
            for typed_op in [
                dr_precision_doc_assign_profile,
                dr_precision_doc_convert_to_profile,
            ] {
                let mut error = ptr::null_mut();
                assert!(typed_op(typed.0, bytes, len, LIMIT, 0, ptr::null(), &mut error).is_null());
                answers.push(text(error));
            }
        }
        for answer in &answers {
            assert_eq!(answer, expected, "len {len}: {answers:?}");
        }
    }
    unsafe { dr_doc_free(byte) };
}

mod bench {
    use super::*;

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn cancellation_during_color_work_releases_reservations_and_keeps_source() {
        let input = [32123, 12345, 56789, 22222];
        let source = document(vec![input; 1_000_000], profile(512, 65536));
        let target = profile(256, 65536);
        let control = DrJobControl::default();
        std::thread::scope(|scope| {
            let task = scope.spawn(|| source.convert_to_profile(&target, LIMIT, 0, Some(&control)));
            let start = std::time::Instant::now();
            while control.progress() < 0.05 && !task.is_finished() {
                assert!(
                    start.elapsed() < std::time::Duration::from_secs(10),
                    "color worker did not start"
                );
                std::thread::yield_now();
            }
            assert!(
                control.progress() >= 0.05 && !task.is_finished(),
                "worker must reach pixel conversion before cancellation"
            );
            let cancellation_started = std::time::Instant::now();
            control.cancel();
            let failure = task.join().unwrap().unwrap_err_string();
            assert!(failure.contains("cancel"), "{failure}");
            assert!(cancellation_started.elapsed() < std::time::Duration::from_secs(1));
        });
        assert_eq!(control.scratch_bytes(), 0);
        assert!(source
            .layer_source(0)
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap()
            .iter()
            .all(|pixel| *pixel == input));
    }
}
