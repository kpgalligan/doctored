//! Reproducible Phase 4 migration measurements. Run in isolation with
//! `cargo run --release --example phase4_latency`; output is JSON lines. The
//! temporary native/PNG files are removed on return. This is a synthetic timing
//! probe, not a photographic quality or peak-process-memory qualification.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::time::Instant;

use doctored_core::ffi::*;
use doctored_core::ffi_color::dr_doc_save_image;
use doctored_core::ffi_doc::*;
use serde_json::json;

#[path = "../tests/common/phase4_corpus.rs"]
mod phase4_corpus;

fn message(error: *mut c_char) -> String {
    if error.is_null() {
        return "operation refused without error".into();
    }
    let message = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    message
}

fn timed<T>(width: u32, height: u32, run: u32, operation: &str, work: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = work();
    println!(
        "{}",
        json!({"width":width,"height":height,"run":run,"operation":operation,
        "elapsed_ms":start.elapsed().as_secs_f64()*1000.0})
    );
    result
}

fn main() {
    let directory = tempfile::tempdir().expect("temporary probe directory");
    for (width, height) in phase4_corpus::DIMENSIONS {
        let pixels = phase4_corpus::image(width, height);
        let image = unsafe { dr_image_from_rgba(pixels.as_ptr(), width, height) };
        drop(pixels);
        assert!(!image.is_null());
        let document = unsafe { dr_doc_from_image(image) };
        assert!(!document.is_null());
        let native = CString::new(directory.path().join("probe.rz").to_str().unwrap()).unwrap();
        let png = CString::new(directory.path().join("probe.png").to_str().unwrap()).unwrap();
        let angle = 10f64.to_radians();
        let matrix = [
            angle.cos() * 0.9,
            angle.sin() * 0.9,
            -angle.sin() * 0.9,
            angle.cos() * 0.9,
            0.0,
            0.0,
        ];
        for run in 0..3 {
            let full = timed(width, height, run, "invert_full", || unsafe {
                dr_image_invert(image)
            });
            assert!(!full.is_null());
            unsafe { dr_image_free(full) };
            let rotated = timed(width, height, run, "rotate90", || unsafe {
                dr_doc_rotate90(document)
            });
            assert!(!rotated.is_null());
            unsafe { dr_doc_free(rotated) };
            let transformed = timed(
                width,
                height,
                run,
                "affine_10deg_90pct_bilinear",
                || unsafe { dr_doc_transform_layer(document, 0, matrix.as_ptr(), 1) },
            );
            assert!(!transformed.is_null());
            unsafe { dr_doc_free(transformed) };
            let mut error = ptr::null_mut();
            let saved = timed(width, height, run, "native_save", || unsafe {
                dr_doc_save_native(document, native.as_ptr(), std::ptr::null(), &mut error)
            });
            assert!(saved, "{}", message(error));
            let opened = timed(width, height, run, "native_open", || unsafe {
                dr_doc_open(native.as_ptr(), std::ptr::null(), &mut error)
            });
            assert!(!opened.is_null(), "{}", message(error));
            assert_eq!(unsafe { dr_doc_width(opened) }, width);
            assert_eq!(unsafe { dr_doc_height(opened) }, height);
            unsafe { dr_doc_free(opened) };
            let saved = timed(
                width,
                height,
                run,
                "png_export_with_projection",
                || unsafe {
                    dr_doc_save_image(
                        document,
                        ptr::null(),
                        png.as_ptr(),
                        0,
                        90,
                        true,
                        false,
                        ptr::null_mut(),
                        std::ptr::null(),
                        &mut error,
                    )
                },
            );
            assert!(saved, "{}", message(error));
        }
        println!(
            "{}",
            json!({"width":width,"height":height,"native_bytes":std::fs::metadata(directory.path().join("probe.rz")).unwrap().len(),
            "png_bytes":std::fs::metadata(directory.path().join("probe.png")).unwrap().len()})
        );
        unsafe {
            dr_doc_free(document);
            dr_image_free(image);
        }
    }
}
