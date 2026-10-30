//! Resource accounting through the C API. Oracles derive storage from known
//! dimensions and ownership changes, not from the accounting implementation.
//!
//! The LUT memo is process-wide, so its eviction assertions must not race
//! another test's cache mutation or limit snapshot: every test takes the
//! `suite_serial` lock first.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;
use std::time::Instant;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::*;
use doctored_core::ffi_channel::*;
use doctored_core::ffi_color::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_resources::*;
use doctored_core::ffi_style::*;
use doctored_core::DrImage;
use serde_json::Value;

use crate::phase4_corpus;
use crate::resource_report;
use crate::rgba_layer;
use crate::serial;

struct Doc(*mut DrDocument);

impl Drop for Doc {
    fn drop(&mut self) {
        unsafe { dr_doc_free(self.0) };
    }
}

impl Doc {
    fn checked(value: *mut DrDocument) -> Self {
        assert!(!value.is_null());
        Self(value)
    }

    fn from_pixels(width: u32, height: u32, pixels: &[u8]) -> Self {
        let image = unsafe { dr_image_from_rgba(pixels.as_ptr(), width, height) };
        assert!(!image.is_null());
        let document = Self::checked(unsafe { dr_doc_from_image(image) });
        unsafe { dr_image_free(image) };
        document
    }

    fn renamed(&self, name: &str) -> Self {
        let name = CString::new(name).unwrap();
        Self::checked(unsafe { dr_doc_with_layer_name(self.0, 0, name.as_ptr()) })
    }
}

unsafe fn json_result(result: *mut c_char, error: *mut c_char) -> Value {
    if !error.is_null() {
        let message = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(error) };
        panic!("{message}");
    }
    assert!(!result.is_null());
    let value = serde_json::from_slice(unsafe { CStr::from_ptr(result) }.to_bytes()).unwrap();
    unsafe { dr_string_free(result) };
    value
}

fn report(
    documents: &[*const DrDocument],
    images: &[*const DrImage],
    scratch: u64,
    limit: u64,
) -> Value {
    resource_report::report(documents, images, &[], &[], scratch, limit)
        .unwrap_or_else(|message| panic!("{message}"))
}

fn evict(documents: &[*const DrDocument]) -> Value {
    resource_report::evict(documents).unwrap_or_else(|message| panic!("{message}"))
}

fn category(report: &Value, category: &str, field: &str) -> u64 {
    report["categories"][category][field].as_u64().unwrap_or(0)
}

#[test]
fn snapshots_deduplicate_arc_storage_and_distinguish_equal_independent_pixels() {
    let _serial = serial();
    let dimensions = (17, 13);
    let pixels = vec![127; dimensions.0 * dimensions.1 * 4];
    let base = Doc::from_pixels(dimensions.0 as u32, dimensions.1 as u32, &pixels);
    let renamed = base.renamed("Renamed");
    // Equal pixels created separately own a different allocation: byte equality
    // cannot make them shared, unlike the cheap renamed snapshot.
    let independent = Doc::from_pixels(dimensions.0 as u32, dimensions.1 as u32, &pixels);
    let result = report(
        &[base.0, renamed.0, independent.0, base.0, ptr::null()],
        &[],
        0,
        0,
    );
    assert_eq!(result["document_handle_count"], 3);
    assert_eq!(
        category(&result, "layer_pixels", "unique_bytes"),
        (pixels.len() * 2) as u64
    );
    assert_eq!(
        category(&result, "layer_pixels", "referenced_bytes"),
        (pixels.len() * 3) as u64
    );
    assert_eq!(category(&result, "layer_pixels", "allocation_count"), 2);
    assert!(result["shared_bytes_saved"].as_u64().unwrap() >= pixels.len() as u64);
    // Every document adopts the same built-in sRGB Arc, so byte/profile object
    // storage is referenced three times but measured once.
    assert_eq!(
        category(&result, "profile", "referenced_bytes"),
        3 * category(&result, "profile", "unique_bytes")
    );
}

#[test]
fn masks_channels_metadata_and_projection_are_in_the_live_set() {
    let _serial = serial();
    let width = 11u32;
    let height = 7u32;
    let samples = width as usize * height as usize;
    let base = Doc::from_pixels(width, height, &vec![255; samples * 4]);
    let masked = Doc::checked(unsafe { dr_doc_adding_layer_mask(base.0, 0, 0, ptr::null(), 0, 0) });
    let name = CString::new("Selection").unwrap();
    let channel = Doc::checked(unsafe {
        dr_doc_add_channel(
            masked.0,
            name.as_ptr(),
            vec![83; samples].as_ptr(),
            width,
            height,
            255,
            0,
            0,
            0.5,
        )
    });
    let packet = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/">resource fixture</x:xmpmeta>"#;
    let metadata = Doc::checked(unsafe {
        dr_doc_set_metadata(channel.0, 1, packet.as_ptr(), packet.len(), ptr::null_mut())
    });
    let renamed = metadata.renamed("Kept by undo");
    let mut error = ptr::null_mut();
    let projection = unsafe { dr_doc_flattened(metadata.0, 0, std::ptr::null(), &mut error) };
    assert!(!projection.is_null() && error.is_null());
    let result = report(
        &[metadata.0, renamed.0],
        &[projection, projection, ptr::null()],
        0,
        0,
    );
    unsafe { dr_image_free(projection) };
    assert_eq!(result["image_handle_count"], 1);
    for kind in ["layer_masks", "saved_channels"] {
        assert_eq!(category(&result, kind, "unique_bytes"), samples as u64);
        assert_eq!(
            category(&result, kind, "referenced_bytes"),
            2 * samples as u64
        );
    }
    assert_eq!(
        category(&result, "capture_metadata", "unique_bytes"),
        packet.len() as u64
    );
    assert_eq!(
        category(&result, "capture_metadata", "referenced_bytes"),
        (packet.len() * 2) as u64
    );
    assert_eq!(
        category(&result, "image_pixels", "unique_bytes"),
        (samples * 4) as u64
    );
}

fn styled_document() -> Doc {
    let base = Doc::from_pixels(31, 19, &[255; 31 * 19 * 4]);
    let style =
        CString::new(r##"{"effects":[{"type":"color_overlay","color":"#12abef"}]}"##).unwrap();
    let mut error = ptr::null_mut();
    let styled = unsafe { dr_doc_set_layer_style(base.0, 0, style.as_ptr(), &mut error) };
    assert!(error.is_null());
    Doc::checked(styled)
}

#[test]
fn eviction_removes_cache_planes_without_changing_projection_or_shared_pixels() {
    let _serial = serial();
    let styled = styled_document();
    let snapshot = styled.renamed("Same style cache");
    let mut error = ptr::null_mut();
    let before = unsafe { dr_doc_flattened(styled.0, 0, std::ptr::null(), &mut error) };
    assert!(!before.is_null() && error.is_null());
    let before_pixels =
        unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(before), 31 * 19 * 4) }.to_vec();
    unsafe { dr_image_free(before) };
    let documents = [styled.0 as *const DrDocument, snapshot.0];
    let cached = report(&documents, &[], 0, 0);
    // A color overlay retains one shape and one matching coverage plane. The
    // style contract pads even a zero-reach effect by one guard pixel per side.
    // The two snapshots share the entire cache.
    assert_eq!(
        category(&cached, "style_cache_planes", "unique_bytes"),
        2 * (31 + 2) * (19 + 2)
    );
    let cleared = evict(&documents);
    assert!(cleared["removed_cache_bytes"].as_u64().unwrap() >= 2 * 31 * 19);
    let after_report = report(&documents, &[], 0, 0);
    assert_eq!(
        category(&after_report, "style_cache_planes", "unique_bytes"),
        0
    );
    assert_eq!(
        category(&cached, "layer_pixels", "unique_bytes"),
        category(&after_report, "layer_pixels", "unique_bytes")
    );
    let after = unsafe { dr_doc_flattened(styled.0, 0, std::ptr::null(), &mut error) };
    assert!(!after.is_null() && error.is_null());
    let after_pixels =
        unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(after), before_pixels.len()) };
    assert_eq!(before_pixels, after_pixels);
    unsafe { dr_image_free(after) };
}

#[test]
fn limits_include_scratch_without_claiming_process_memory_coverage() {
    let _serial = serial();
    let doc = Doc::from_pixels(7, 5, &[255; 7 * 5 * 4]);
    let base = report(&[doc.0], &[], 0, 0);
    // One flat RGBA8 projection needs its four f32 accumulator components plus
    // four output bytes at each of 35 pixels; retained source is separate.
    assert_eq!(base["projection_scratch_estimate_bytes"], 35 * (4 * 4 + 4));
    let retained = base["unique_retained_bytes"].as_u64().unwrap();
    let at_limit = report(&[doc.0], &[], 8192, retained + 8192);
    assert_eq!(at_limit["within_working_limit"], true);
    assert_eq!(at_limit["estimated_working_bytes"], retained + 8192);
    let exceeded = report(&[doc.0], &[], 8192, retained + 8191);
    assert_eq!(exceeded["within_working_limit"], false);
    assert_eq!(exceeded["unique_retained_bytes"], retained);
    assert!(!exceeded["known_unaccounted"].as_array().unwrap().is_empty());
    let overflow = report(&[doc.0], &[], u64::MAX, 1);
    assert_eq!(overflow["estimated_working_bytes"], u64::MAX);
    assert_eq!(overflow["within_working_limit"], false);
}

#[test]
fn immutable_handles_are_send_sync_and_render_inventory_eviction_can_overlap() {
    let _serial = serial();
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<DrDocument>();
    assert_send_sync::<DrImage>();
    let owned = styled_document();
    let document = unsafe { Box::from_raw(owned.0) };
    std::mem::forget(owned);
    let document = Arc::new(*document);
    let expected = document.flattened().unwrap().into_raw();
    std::thread::scope(|scope| {
        for _ in 0..3 {
            let document = Arc::clone(&document);
            let expected = &expected;
            scope.spawn(move || {
                for _ in 0..40 {
                    let mut error = ptr::null_mut();
                    let image =
                        unsafe { dr_doc_flattened(&*document, 0, std::ptr::null(), &mut error) };
                    assert!(!image.is_null() && error.is_null());
                    let actual = unsafe {
                        std::slice::from_raw_parts(dr_image_pixels_rgba(image), expected.len())
                    };
                    assert_eq!(actual, expected);
                    unsafe { dr_image_free(image) };
                }
            });
        }
        for _ in 0..40 {
            let result = report(&[&*document], &[], 0, 0);
            assert_eq!(
                category(&result, "layer_pixels", "unique_bytes"),
                31 * 19 * 4
            );
            evict(&[&*document]);
        }
    });
}

#[test]
fn counted_arrays_reject_null_and_oversized_extents_but_accept_empty_sets() {
    let _serial = serial();
    let mut error = ptr::null_mut();
    let empty = unsafe {
        dr_mixed_resources_report_with_raw_masks(
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            0,
            &mut error,
        )
    };
    assert_eq!(
        unsafe { json_result(empty, error) }["document_handle_count"],
        0
    );
    for (documents, images) in [(1, 0), (0, 1), (65_537, 0), (0, 65_537)] {
        error = ptr::null_mut();
        let result = unsafe {
            dr_mixed_resources_report_with_raw_masks(
                ptr::null(),
                documents,
                ptr::null(),
                images,
                ptr::null(),
                0,
                ptr::null(),
                0,
                ptr::null(),
                0,
                0,
                0,
                &mut error,
            )
        };
        assert!(result.is_null() && !error.is_null());
        unsafe { dr_string_free(error) };
    }
    error = ptr::null_mut();
    let result =
        unsafe { dr_mixed_resources_evict_caches(ptr::null(), 1, ptr::null(), 0, &mut error) };
    assert!(result.is_null() && !error.is_null());
    unsafe { dr_string_free(error) };
    assert!(unsafe {
        dr_mixed_resources_report_with_raw_masks(
            ptr::null(),
            1,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            0,
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        dr_mixed_resources_evict_caches(ptr::null(), 1, ptr::null(), 0, ptr::null_mut())
    }
    .is_null());
}

#[test]
fn global_lut_cache_counts_retained_payloads_after_document_close_and_evicts_them() {
    let _serial = serial();
    evict(&[]);
    let base = Doc::from_pixels(2, 2, &[127; 16]);
    // IEEE754 little-endian [0,0,0,1,1,1], encoded as standard base64.
    // An ignored padding field takes the >64KiB memo path with a tiny table,
    // giving an independently known 24-byte table allocation.
    let meta = serde_json::json!({
        "type":"adjust", "op":"color_lookup", "params": {
            "kind":"1d", "size":2, "table":"AAAAAAAAAAAAAAAAAACAPwAAgD8AAIA/",
            "padding":"x".repeat(70_000)
        }
    })
    .to_string();
    let cmeta = CString::new(meta.clone()).unwrap();
    let adjusted = Doc::checked(unsafe { dr_doc_with_layer_meta(base.0, 0, cmeta.as_ptr()) });
    assert!(unsafe { dr_doc_layer_is_adjustment(adjusted.0, 0) });
    drop(adjusted);
    drop(base);
    let cached = report(&[], &[], 0, 0);
    assert_eq!(cached["document_handle_count"], 0);
    assert_eq!(category(&cached, "lut_cache", "allocation_count"), 4);
    assert!(category(&cached, "lut_cache", "unique_bytes") >= meta.len() as u64 + 24);
    let cleared = evict(&[]);
    assert!(cleared["removed_cache_bytes"].as_u64().unwrap() >= meta.len() as u64 + 24);
    assert_eq!(
        category(&report(&[], &[], 0, 0), "lut_cache", "unique_bytes"),
        0
    );
}

#[test]
#[ignore = "explicit fixed24/48MP resource qualification; allocates large image buffers"]
fn fixed_24_48mp_corpus_retention() {
    let _serial = serial();
    for (width, height) in phase4_corpus::DIMENSIONS {
        let samples = width as usize * height as usize;
        let mut pixels = phase4_corpus::image(width, height).into_raw();
        let base = Doc::from_pixels(width, height, &pixels);
        let snapshots: Vec<_> = (1..20)
            .map(|index| base.renamed(&format!("History {index}")))
            .collect();
        pixels[0] ^= 255;
        let edited = Doc::checked(rgba_layer::with_layer_rgba(
            base.0, 0, &pixels, width, height,
        ));
        drop(pixels);
        let mut error = ptr::null_mut();
        let projection = unsafe { dr_doc_flattened(edited.0, 0, std::ptr::null(), &mut error) };
        assert!(!projection.is_null() && error.is_null());
        let mut documents = vec![base.0 as *const DrDocument, edited.0];
        documents.extend(
            snapshots
                .iter()
                .map(|document| document.0 as *const DrDocument),
        );
        let start = Instant::now();
        let measured = report(
            &documents,
            &[projection],
            8 * 1024 * 1024,
            2 * 1024 * 1024 * 1024,
        );
        let elapsed = start.elapsed();
        // 13B-2b: the one-pixel edit shares every tile but (0, 0).
        assert_eq!(
            category(&measured, "layer_pixels", "unique_bytes"),
            (samples * 4 + 256 * 256 * 4) as u64
        );
        assert_eq!(
            category(&measured, "layer_pixels", "referenced_bytes"),
            (samples * 4 * 21) as u64
        );
        assert_eq!(
            category(&measured, "image_pixels", "unique_bytes"),
            (samples * 4) as u64
        );
        println!(
            "RESOURCE_CORPUS {}MP width={width} height={height} inventory_us={} {measured}",
            samples / 1_000_000,
            elapsed.as_micros()
        );
        unsafe { dr_image_free(projection) };
    }
}
