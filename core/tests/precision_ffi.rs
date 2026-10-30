//! Typed bridge integration, source-vs-preview ownership, independent numeric
//! reference and the per-export NULL sweep. No public app mode is enabled.
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_open};
use doctored_core::ffi_precision::*;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};

use crate::group_roots;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
struct Doc(*mut PrecisionDocument);
impl Drop for Doc {
    fn drop(&mut self) {
        unsafe { dr_precision_doc_free(self.0) }
    }
}
struct Image(*mut PrecisionImage);
impl Drop for Image {
    fn drop(&mut self) {
        unsafe { dr_precision_image_free(self.0) }
    }
}

fn path(path: &std::path::Path) -> CString {
    CString::new(path.to_str().unwrap()).unwrap()
}
unsafe fn error(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let result = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    result
}
fn fixture(file: &std::path::Path, pixels: &[[u16; 4]]) {
    let bytes: Vec<_> = pixels
        .iter()
        .flatten()
        .flat_map(|v| v.to_ne_bytes())
        .collect();
    let mut encoded = Vec::new();
    PngEncoder::new(&mut encoded)
        .write_image(&bytes, pixels.len() as u32, 1, ExtendedColorType::Rgba16)
        .unwrap();
    std::fs::write(file, encoded).unwrap();
}
fn open(file: &std::path::Path) -> Doc {
    let mut err = ptr::null_mut();
    let result =
        unsafe { dr_precision_doc_open(path(file).as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!result.is_null(), "{}", unsafe { error(err) });
    assert!(err.is_null());
    Doc(result)
}
fn source(doc: &Doc, index: usize) -> Image {
    let result = unsafe { dr_precision_doc_layer_source(doc.0, index) };
    assert!(!result.is_null());
    Image(result)
}
fn pixels(image: &Image) -> Vec<[u16; 4]> {
    unsafe {
        let count = dr_precision_image_width(image.0) as usize
            * dr_precision_image_height(image.0) as usize;
        std::slice::from_raw_parts(dr_precision_image_pixels_rgba16(image.0).cast(), count).to_vec()
    }
}
fn render(doc: &Doc) -> Image {
    let mut err = ptr::null_mut();
    let result = unsafe { dr_precision_doc_render(doc.0, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!result.is_null(), "{}", unsafe { error(err) });
    Image(result)
}
fn group(doc: &Doc, opacity: f32) -> Doc {
    group_as(doc, opacity, false)
}
/// Every root in one group through the app's own route: the typed
/// `group_layers` edit, then the opacity edit on the new top group.
fn group_as(doc: &Doc, opacity: f32, pass_through: bool) -> Doc {
    let roots = group_roots::root_indices(unsafe { &*doc.0 });
    let request = CString::new(
        serde_json::json!({
            "op": "group_layers", "layers": roots, "name": "Group", "pass_through": pass_through,
        })
        .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let grouped = unsafe {
        dr_precision_doc_edit(
            doc.0,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!grouped.is_null(), "{}", unsafe { error(err) });
    let grouped = Doc(grouped);
    let top = unsafe { &*grouped.0 }.layer_count() - 1;
    let result = unsafe { dr_precision_doc_set_layer_opacity(grouped.0, top, opacity, &mut err) };
    if result.is_null() {
        // Unchanged opacity: NULL without an error.
        assert!(err.is_null(), "{}", unsafe { error(err) });
        grouped
    } else {
        Doc(result)
    }
}

#[test]
fn genuine_codes_cross_two_groups_native_reopen_and_both_exports() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    let expected: Vec<_> = (0..=u16::MAX)
        .map(|v| [v, 65535 - v, v / 2, 65535])
        .collect();
    fixture(&input, &expected);
    let original = open(&input);
    let isolated = group(&group(&original, 1.0), 1.0);
    let grouped = group_as(&isolated, 1.0, true);
    assert_eq!(pixels(&source(&original, 0)), expected);
    assert_eq!(pixels(&render(&grouped)), expected);
    let native = directory.path().join("grouped.rz");
    unsafe {
        let mut err = ptr::null_mut();
        assert!(
            dr_precision_doc_save_native(
                grouped.0,
                path(&native).as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err
            ),
            "{}",
            error(err)
        );
    }
    let reopened = open(&native);
    assert_eq!(pixels(&source(&reopened, 0)), expected);
    assert_eq!(pixels(&render(&reopened)), expected);
    unsafe {
        // This explicit refusal is essential: old document APIs cannot receive
        // a typed v11 file and silently decode its source PNGs to RGBA8.
        let mut err = ptr::null_mut();
        let byte_doc = dr_doc_open(path(&native).as_ptr(), &mut err);
        assert!(byte_doc.is_null());
        assert!(!error(err).is_empty());
        dr_doc_free(byte_doc);
        for (format, name) in [(0, "out.png"), (1, "out.tiff")] {
            let file = directory.path().join(name);
            let mut err = ptr::null_mut();
            let mut report = u32::MAX;
            assert!(
                dr_precision_doc_export16(
                    reopened.0,
                    path(&file).as_ptr(),
                    format,
                    true,
                    false,
                    LIMIT,
                    0,
                    &mut report,
                    &mut err
                ),
                "{}",
                error(err)
            );
            assert_eq!(report, 1 | 16, "ICC and resolution were written");
            // A separate decoder instance checks actual sample depth and data;
            // the TIFF writer is the project's typed layout implementation.
            let decoded = image::open(&file).unwrap();
            assert_eq!(decoded.color(), image::ColorType::Rgba16);
            assert_eq!(
                decoded.to_rgba16().as_raw(),
                &expected.iter().flatten().copied().collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn immutable_capture_preview_and_fractional_alpha_have_distinct_roles() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("input.png");
    let expected = [
        [30000, 12001, 45002, 34567],
        [30001, 12002, 45003, 23456],
        [32123, 12345, 56789, 0],
    ];
    fixture(&file, &expected);
    let captured = open(&file);
    let original = source(&captured, 0);
    let mut err = ptr::null_mut();
    let duplicated = Doc(unsafe { dr_precision_doc_duplicate_layer(captured.0, 0, &mut err) });
    assert!(!duplicated.0.is_null(), "{}", unsafe { error(err) });
    let mut err = ptr::null_mut();
    let changed =
        Doc(unsafe { dr_precision_doc_set_layer_opacity(duplicated.0, 1, 0.625, &mut err) });
    assert!(!changed.0.is_null(), "{}", unsafe { error(err) });
    let grouped = group(&group(&changed, 0.75), 0.5);
    let actual = pixels(&render(&grouped));
    for (actual, input) in actual.iter().zip(expected) {
        // The two layers have the same straight RGB, so only alpha changes.
        // Compute in f64 from exact integer input, with no production kernel.
        let a = f64::from(input[3]) / 65535.0;
        // Each opacity at its byte (#2).
        let q = crate::common::byte_unit;
        let alpha = (a + a * q(0.625) * (1.0 - a)) * q(0.75) * q(0.5);
        let target = (alpha * 65535.0).round() as u16;
        assert!(actual[3].abs_diff(target) <= 1);
        if input[3] > 0 {
            assert_eq!(actual[..3], input[..3]);
        } else {
            assert_eq!(*actual, [0; 4]);
        }
    }
    assert_eq!(
        pixels(&original),
        expected,
        "hidden RGB and source alpha survive captures"
    );
    unsafe {
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_set_layer_opacity(changed.0, 1, 0.625, &mut err).is_null());
        assert!(err.is_null(), "no-op must not become an Undo entry");
        let preview = dr_precision_image_preview_rgba8(original.0, LIMIT, 0, ptr::null(), &mut err);
        assert!(!preview.is_null(), "{}", error(err));
        let bytes = std::slice::from_raw_parts(dr_image_pixels_rgba(preview), expected.len() * 4);
        let oracle: Vec<_> = expected
            .iter()
            .flatten()
            .map(|v| ((u32::from(*v) + 128) / 257) as u8)
            .collect();
        assert_eq!(bytes, oracle);
        assert_eq!(
            bytes[0], bytes[4],
            "preview deliberately collapses adjacent source codes"
        );
        dr_image_free(preview);
        let mut length = 0;
        assert!(!dr_precision_image_profile(original.0, &mut length).is_null());
        assert!(length > 128);
        let docs = [
            captured.0.cast_const(),
            duplicated.0.cast_const(),
            changed.0.cast_const(),
            grouped.0.cast_const(),
        ];
        let images = [original.0.cast_const()];
        let json = dr_precision_resources_report(
            docs.as_ptr(),
            docs.len(),
            images.as_ptr(),
            images.len(),
            &mut err,
        );
        assert!(!json.is_null());
        let report: serde_json::Value = serde_json::from_str(&error(json)).unwrap();
        assert!(
            report["unique_retained_bytes"].as_u64().unwrap()
                < report["referenced_bytes"].as_u64().unwrap()
        );
        // Save the captured OLD owner after newer values and previews exist.
        let saved = directory.path().join("captured.rz");
        assert!(
            dr_precision_doc_save_native(
                captured.0,
                path(&saved).as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err
            ),
            "{}",
            error(err)
        );
        let old = open(&saved);
        assert_eq!(pixels(&source(&old, 0)), expected);
        let info = dr_precision_doc_info(old.0, &mut err);
        let info: serde_json::Value = serde_json::from_str(&error(info)).unwrap();
        assert_eq!(info["storage"], "rgba16_unorm");
        assert_eq!(info["evaluation"], "encoded_rgb_f32_v1");

        // Phase 11E: the linear-light tag is the ONLY other admitted value,
        // and it comes from the document's own policy rather than a constant.
        let mut err = ptr::null_mut();
        let linear =
            doctored_core::ffi_evaluation::dr_precision_doc_set_evaluation(old.0, 1, &mut err);
        assert!(!linear.is_null(), "{}", error(err));
        let linear = Doc(linear);
        let info = dr_precision_doc_info(linear.0, &mut err);
        let info: serde_json::Value = serde_json::from_str(&error(info)).unwrap();
        assert_eq!(info["evaluation"], "linear_light_f32_v1");
    }
}

#[test]
fn refusal_is_atomic_and_cancelled_render_keeps_sources() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("input.png");
    fixture(&file, &[[23456, 12345, 45678, 54321]]);
    let doc = open(&file);
    let destination = directory.path().join("protected.tiff");
    std::fs::write(&destination, b"keep this old destination").unwrap();
    unsafe {
        let mut err = ptr::null_mut();
        let mut report = 123;
        assert!(!dr_precision_doc_export16(
            doc.0,
            path(&destination).as_ptr(),
            99,
            true,
            false,
            LIMIT,
            0,
            &mut report,
            &mut err
        ));
        assert!(!error(err).is_empty());
        assert_eq!(report, 123);
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"keep this old destination"
        );
        let mut err = ptr::null_mut();
        assert!(!dr_precision_doc_save_native(
            doc.0,
            path(&destination).as_ptr(),
            1,
            0,
            ptr::null(),
            &mut err
        ));
        assert!(!error(err).is_empty());
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"keep this old destination"
        );
        let control = DrJobControl::default();
        control.cancel();
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_render(doc.0, LIMIT, 0, &control, &mut err).is_null());
        assert!(!error(err).is_empty());
        assert_eq!(control.scratch_bytes(), 0);
    }
    assert_eq!(pixels(&source(&doc, 0)), [[23456, 12345, 45678, 54321]]);
}

#[test]
fn null_safety_sweep_every_typed_export() {
    unsafe {
        let n = ptr::null();
        let e = ptr::null_mut();
        assert!(dr_precision_doc_open(ptr::null(), LIMIT, 0, ptr::null(), e).is_null());
        // The blank 16-bit document: NULL fill, profile and name.
        assert!(
            doctored_core::ffi_precision_input::dr_precision_doc_create_blank(
                1,
                1,
                ptr::null(),
                ptr::null(),
                0,
                ptr::null(),
                LIMIT,
                0,
                e
            )
            .is_null()
        );
        dr_precision_doc_free(ptr::null_mut());
        // Auto-Blend Layers: a NULL document, request, control and outs.
        assert!(
            doctored_core::ffi_auto_blend::dr_precision_doc_auto_blend_plan(n, ptr::null(), e)
                .is_null()
        );
        assert!(doctored_core::ffi_auto_blend::dr_precision_doc_auto_blend(
            n,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut(),
            e
        )
        .is_null());
        // The float development: the f32 intake with NULL samples,
        // profile and name, the master read and the inspection view.
        assert!(
            doctored_core::ffi_precision_input::dr_precision_doc_from_rgba32(
                ptr::null(),
                0,
                1,
                1,
                ptr::null(),
                0,
                ptr::null(),
                LIMIT,
                0,
                ptr::null(),
                e
            )
            .is_null()
        );
        assert!(
            doctored_core::ffi_develop_inspect::dr_precision_doc_source_master(n, 0, e).is_null()
        );
        assert!(
            doctored_core::ffi_develop_inspect::dr_precision_image_develop_inspection(
                ptr::null(),
                LIMIT,
                0,
                ptr::null(),
                e
            )
            .is_null()
        );
        assert_eq!(
            doctored_core::ffi_develop_inspect::dr_precision_image_storage(ptr::null()),
            0
        );
        let mut peak = 0f32;
        let mut marked = 0u64;
        assert!(
            !doctored_core::ffi_develop_inspect::dr_precision_image_develop_preview_extended(
                ptr::null(),
                0,
                0,
                1,
                1,
                1,
                0,
                ptr::null_mut(),
                8,
                &mut peak,
                &mut marked,
                LIMIT,
                0,
                ptr::null(),
                e
            )
        );
        // The one-image cap: a NULL err_out with a refused extent or depth.
        assert!(!doctored_core::ffi_host_pixels::dr_single_image_capacity(
            0, 0, 16, e
        ));
        assert!(!doctored_core::ffi_host_pixels::dr_single_image_capacity(
            1, 1, 10, e
        ));
        assert!(dr_precision_doc_info(n, e).is_null());
        assert!(dr_precision_doc_layer_source(n, 0).is_null());
        assert!(dr_precision_doc_render(n, LIMIT, 0, ptr::null(), e).is_null());
        assert!(dr_precision_doc_layer_canvas_image(n, 0, LIMIT, 0, ptr::null(), e).is_null());
        assert!(dr_precision_doc_duplicate_layer(n, 0, e).is_null());
        assert!(dr_precision_doc_set_layer_opacity(n, 0, 1.0, e).is_null());
        // The typed lock query (`ffi_precision_inspect`, Phase 12E-1b): a
        // NULL document, an out-of-range index and an unrecognized kind all
        // answer 0, exactly as its byte twin does.
        assert_eq!(
            doctored_core::ffi_precision_inspect::dr_precision_doc_lock_block(n, 0, 0),
            0
        );
        assert_eq!(
            doctored_core::ffi_precision_inspect::dr_precision_doc_lock_block(n, 99, 4),
            0
        );
        assert_eq!(
            doctored_core::ffi_precision_inspect::dr_precision_doc_lock_block(n, 0, -1),
            0
        );
        assert!(
            !doctored_core::ffi_precision_inspect::dr_precision_doc_layer_bounds(
                n,
                0,
                ptr::null_mut()
            )
        );
        assert!(
            !doctored_core::ffi_precision_inspect::dr_precision_doc_layer_at(
                n,
                0,
                0,
                false,
                ptr::null_mut()
            )
        );
        assert!(!dr_precision_doc_save_native(
            n,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            e
        ));
        assert!(!dr_precision_doc_export16(
            n,
            ptr::null(),
            0,
            true,
            false,
            LIMIT,
            0,
            ptr::null_mut(),
            e
        ));
        dr_precision_image_free(ptr::null_mut());
        assert_eq!(dr_precision_image_width(ptr::null()), 0);
        assert_eq!(dr_precision_image_height(ptr::null()), 0);
        assert!(dr_precision_image_pixels_rgba16(ptr::null()).is_null());
        assert_eq!(dr_precision_image_storage_identity(ptr::null()), 0);
        let mut length = 42;
        assert!(dr_precision_image_profile(ptr::null(), &mut length).is_null());
        assert_eq!(length, 0);
        assert!(dr_precision_image_profile(ptr::null(), ptr::null_mut()).is_null());
        assert!(dr_precision_image_preview_rgba8(ptr::null(), LIMIT, 0, ptr::null(), e).is_null());
        assert!(
            doctored_core::ffi_clip_display::dr_precision_image_clip_display(
                ptr::null(),
                1,
                LIMIT,
                0,
                e
            )
            .is_null()
        );
        assert!(
            doctored_core::ffi_precision::dr_precision_image_thumbnail_rgba8(
                ptr::null(),
                34,
                LIMIT,
                0,
                ptr::null(),
                e
            )
            .is_null()
        );
        assert!(
            doctored_core::ffi_precision::dr_precision_doc_layer_mask_thumbnail_rgba8(
                n,
                0,
                34,
                LIMIT,
                0,
                ptr::null(),
                e
            )
            .is_null()
        );
        assert!(
            doctored_core::ffi_precision::dr_precision_doc_channel_thumbnail_rgba8(
                n,
                0,
                34,
                LIMIT,
                0,
                ptr::null(),
                e
            )
            .is_null()
        );
        assert!(
            doctored_core::ffi_precision::dr_precision_doc_layer_mask_canvas_thumbnail_rgba8(
                n,
                0,
                34,
                LIMIT,
                0,
                ptr::null(),
                e
            )
            .is_null()
        );
        assert_eq!(
            doctored_core::ffi_identity::dr_precision_doc_layer_mask_storage_identity(n, 0),
            0
        );
        assert!(dr_precision_resources_report(ptr::null(), 1, ptr::null(), 0, e).is_null());
        // The versioned evaluation policy (ffi_evaluation, Phase 11E).
        assert_eq!(
            doctored_core::ffi_evaluation::dr_precision_doc_evaluation(n),
            0
        );
        assert!(doctored_core::ffi_evaluation::dr_precision_doc_linear_light_refusal(n).is_null());
        assert!(doctored_core::ffi_evaluation::dr_precision_doc_set_evaluation(n, 1, e).is_null());
        assert!(
            doctored_core::ffi_evaluation::dr_precision_doc_set_evaluation(n, 1, ptr::null_mut())
                .is_null()
        );
        assert!(doctored_core::ffi_evaluation::dr_precision_image_compare(
            ptr::null(),
            ptr::null(),
            e
        )
        .is_null());
        assert!(doctored_core::ffi_evaluation::dr_precision_image_compare(
            ptr::null(),
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        // Phase 187b's float artwork render: NULL description, samples,
        // profile and err_out answer NULL.
        assert!(doctored_core::ffi_artwork::dr_artwork_render_float(
            1,
            1,
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            e
        )
        .is_null());
        assert!(dr_precision_resources_report(ptr::null(), 0, ptr::null(), 65_537, e).is_null());
        let empty = dr_precision_resources_report(ptr::null(), 0, ptr::null(), 0, e);
        assert!(!empty.is_null());
        let json: serde_json::Value = serde_json::from_str(&error(empty)).unwrap();
        assert_eq!(json["unique_retained_bytes"], 0);
    }
}
