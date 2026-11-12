//! Independent assembly contracts: RGB stays unbaked, mask survives native I/O,
//! grouped originals remain reusable, cancellation and refusals are atomic.
use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use common::*;
use doctored_core::doc::LayerRaster;
use doctored_core::{ffi::*, ffi_cutout::*, ffi_doc::*, ffi_jobs::*, ffi_mask::*};
use image::RgbaImage;
use std::ptr;

#[test]
fn output_is_an_editable_masked_layer_and_original_group_is_unchanged() {
    let temporary = tempfile::tempdir().unwrap();
    let original = opaque_pattern(29, 19);
    let foreground = solid(29, 19, [51, 102, 153, 255]);
    let alpha: Vec<u8> = (0..29 * 19).map(|i| (i % 29 * 255 / 28) as u8).collect();
    unsafe {
        let image = dr_image_from_rgba(original.as_raw().as_ptr(), 29, 19);
        let base = dr_doc_from_image(image);
        let mut group_at = 0;
        let grouped = doctored_core::ffi_group::dr_doc_group_layers(
            base,
            [0].as_ptr(),
            1,
            c"Source".as_ptr(),
            &mut group_at,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
        );
        assert!(!grouped.is_null());
        let fg = dr_image_from_rgba(foreground.as_raw().as_ptr(), 29, 19);
        let token = dr_job_control_create();
        let budget = dr_doc_refinement_cutout_scratch_bytes(grouped);
        assert!(budget >= 5 * 29 * 19);
        assert!(dr_job_control_set_scratch_limit(token, budget));
        let mut error = ptr::null_mut();
        let output = dr_doc_add_refinement_cutout(grouped, fg, alpha.as_ptr(), token, &mut error);
        assert!(error.is_null(), "{}", take_err_string(error));
        assert!(!output.is_null());
        assert_eq!(dr_doc_layer_count(output), 3);
        assert!(dr_doc_layer_visible(grouped, group_at));
        assert!(!dr_doc_layer_visible(output, group_at));
        assert!(dr_doc_layer_visible(output, 0)); // Child flags survive hiding the root.
        assert_eq!(layer_pixels(output, 0), *original.as_raw());
        assert!(LayerRaster::ptr_eq(
            &(&*grouped).layers[0].pixels,
            &(&*output).layers[0].pixels
        ));
        assert_eq!(layer_pixels(output, 2), *foreground.as_raw());
        assert_eq!(mask_bytes(&*output, 2), alpha);
        assert_eq!((&*output).layers[2].depth, 0);
        let properties = dr_doc_layer_mask_properties(output, 2);
        let properties: serde_json::Value =
            serde_json::from_str(&take_err_string(properties)).unwrap();
        assert_eq!(properties["linked"], true);
        assert_eq!(properties["density"], 1.0);
        assert_eq!(properties["feather"], 0.0);
        let flat = flat_pixels(output);
        for (pixel, &a) in flat.chunks_exact(4).zip(&alpha) {
            assert_eq!(pixel[3], a);
            if a != 0 {
                assert_eq!(&pixel[..3], &[51, 102, 153]);
            }
        }
        let path = cpath(&temporary.path().join("cutout.rz"));
        assert!(dr_doc_save_native(
            output,
            path.as_ptr(),
            std::ptr::null(),
            &mut error
        ));
        let reopened = dr_doc_open(path.as_ptr(), std::ptr::null(), &mut error);
        assert!(!reopened.is_null());
        assert_eq!(flat_pixels(reopened), flat);
        assert_eq!(layer_pixels(reopened, 0), *original.as_raw());
        assert_eq!(layer_pixels(reopened, 2), *foreground.as_raw());
        assert_eq!(mask_bytes(&*reopened, 2), alpha);
        assert_eq!(dr_job_control_scratch_bytes(token), 0);
        assert_eq!(dr_job_control_progress(token), 1.0);
        for doc in [reopened, output, grouped, base] {
            dr_doc_free(doc);
        }
        dr_image_free(fg);
        dr_image_free(image);
        dr_job_control_free(token);
    }
}

#[test]
fn empty_alpha_noop_and_bad_inputs_cancellation_and_admission_refuse() {
    unsafe {
        let bytes = solid(11, 7, [2, 3, 4, 255]);
        let image = dr_image_from_rgba(bytes.as_raw().as_ptr(), 11, 7);
        let doc = dr_doc_from_image(image);
        let mut error = ptr::null_mut();
        assert!(dr_doc_add_refinement_cutout(
            doc,
            image,
            [0; 77].as_ptr(),
            ptr::null(),
            &mut error
        )
        .is_null());
        assert!(error.is_null());
        let small = dr_image_from_rgba([0, 0, 0, 255].as_ptr(), 1, 1);
        assert!(dr_doc_add_refinement_cutout(
            doc,
            small,
            [255; 77].as_ptr(),
            ptr::null(),
            &mut error
        )
        .is_null());
        assert!(!take_err_string(error).is_empty());
        error = ptr::null_mut();
        // A foreground with its own alpha is kept byte for byte on the new
        // layer; the refinement alpha stays a separate mask.
        let half = solid(11, 7, [9, 8, 7, 128]);
        let translucent = dr_image_from_rgba(half.as_raw().as_ptr(), 11, 7);
        let coverage: Vec<u8> = (0..77).map(|i| (i * 3 + 1) as u8).collect();
        let accepted = dr_doc_add_refinement_cutout(
            doc,
            translucent,
            coverage.as_ptr(),
            ptr::null(),
            &mut error,
        );
        assert!(error.is_null(), "{}", take_err_string(error));
        assert!(!accepted.is_null());
        assert_eq!(dr_doc_layer_count(accepted), 2);
        assert_eq!(layer_pixels(accepted, 1), *half.as_raw());
        assert_eq!(mask_bytes(&*accepted, 1), coverage);
        assert!(!dr_doc_layer_visible(accepted, 0));
        dr_doc_free(accepted);
        // Nothing visible anywhere (foreground alpha 0 under a full mask) is
        // the no-op, like an empty mask.
        let clear = dr_image_from_rgba(solid(11, 7, [9, 8, 7, 0]).as_raw().as_ptr(), 11, 7);
        assert!(dr_doc_add_refinement_cutout(
            doc,
            clear,
            [255; 77].as_ptr(),
            ptr::null(),
            &mut error
        )
        .is_null());
        assert!(error.is_null());
        dr_image_free(clear);
        for cancelled in [false, true] {
            let token = dr_job_control_create();
            assert!(dr_job_control_set_scratch_limit(token, 1));
            if cancelled {
                dr_job_control_cancel(token);
            }
            assert!(dr_doc_add_refinement_cutout(
                doc,
                image,
                [255; 77].as_ptr(),
                token,
                &mut error
            )
            .is_null());
            let message = take_err_string(error);
            error = ptr::null_mut();
            assert!(message.contains(if cancelled {
                "cancelled"
            } else {
                "working memory"
            }));
            assert_eq!(dr_job_control_scratch_bytes(token), 0);
            // A failed attempt still consumes the single-use token.
            assert!(dr_doc_add_refinement_cutout(
                doc,
                image,
                [255; 77].as_ptr(),
                token,
                &mut error
            )
            .is_null());
            assert!(take_err_string(error).contains("already been used"));
            error = ptr::null_mut();
            dr_job_control_free(token);
        }
        assert_eq!(layer_pixels(doc, 0), *bytes.as_raw());
        assert!(dr_doc_layer_visible(doc, 0));
        assert_eq!(dr_doc_layer_count(doc), 1);
        for source in [image, small, translucent] {
            dr_image_free(source);
        }
        dr_doc_free(doc);
    }
}

#[test]
fn live_cancel_during_pixel_copy_never_publishes_and_releases_scratch() {
    // A large transfer keeps the actual copy stage observable; cancellation
    // begins only once progress proves it passed validation and admission.
    // The worker is PARKED inside the copy, so the bench twin's retry ladder
    // is not needed here: rung 0's 4096 x 4096 canvas, parked once.
    let height = 4096;
    let bytes = RgbaImage::from_pixel(4096, height, image::Rgba([17, 73, 209, 255]));
    let alpha = vec![127; 4096 * height as usize];
    unsafe {
        let image = dr_image_from_rgba(bytes.as_raw().as_ptr(), 4096, height);
        let doc = dr_doc_from_image(image);
        let (d, f) = (doc as usize, image as usize);
        let alpha = &alpha;
        let render = move |token: usize| {
            let mut error = ptr::null_mut();
            let output = dr_doc_add_refinement_cutout(
                d as _,
                f as _,
                alpha.as_ptr(),
                token as _,
                &mut error,
            );
            if output.is_null() {
                (false, take_err_string(error))
            } else {
                dr_doc_free(output);
                (true, String::new())
            }
        };
        // k = 0: the copy polls only at its per-chunk reports.
        let (entry, _) = entry_then_cancel(&|progress, _, _| progress >= 0.2, render);
        let parked = cancelled_parked_at(
            entry.checkpoints + 1,
            &|_, stage| stage == entry.stage,
            render,
        );
        eprintln!(
            "refinement cutout copy cancellation: {}",
            parked_line(&parked)
        );
        assert_eq!(dr_doc_layer_count(doc), 1);
        assert_eq!(
            std::slice::from_raw_parts(dr_image_pixels_rgba(image), bytes.len()),
            bytes.as_raw()
        );
        dr_doc_free(doc);
        dr_image_free(image);
    }
}

mod bench {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn live_cancel_during_pixel_copy_never_publishes_and_releases_scratch() {
        // A large transfer keeps the actual copy stage observable; cancellation
        // begins only once progress proves it passed validation and admission.
        // Catching that stage LIVE is a scheduling race, not a property of the
        // fixture: under the whole suite's parallel load the observer can be left
        // unscheduled until the copy has already finished, so the window is RETRIED
        // on a taller canvas, and never observing one in ANY attempt still fails —
        // a cancelled copy is proven to publish nothing on every run.
        // The ladder is capped at x2: a third rung would build a 4096 x 12 288
        // source — 200 MB of pixels plus a 50 MB matte, in a suite that already
        // runs its binaries in parallel — to buy one more retry.
        let mut observed_live = false;
        for attempt in 0..2 {
            let height = 4096 * (1 + attempt);
            let bytes = RgbaImage::from_pixel(4096, height, image::Rgba([17, 73, 209, 255]));
            let alpha = vec![127; 4096 * height as usize];
            unsafe {
                let image = dr_image_from_rgba(bytes.as_raw().as_ptr(), 4096, height);
                let doc = dr_doc_from_image(image);
                let token = dr_job_control_create();
                let (d, f, t) = (doc as usize, image as usize, token as usize);
                let worker = std::thread::spawn(move || {
                    let mut error = ptr::null_mut();
                    let output = dr_doc_add_refinement_cutout(
                        d as _,
                        f as _,
                        alpha.as_ptr(),
                        t as _,
                        &mut error,
                    );
                    let message = if error.is_null() {
                        String::new()
                    } else {
                        take_err_string(error)
                    };
                    (output as usize, message)
                });
                let start = Instant::now();
                while dr_job_control_progress(token) < 0.2
                    && !worker.is_finished()
                    && start.elapsed() < Duration::from_secs(10)
                {
                    std::thread::yield_now();
                }
                let live = dr_job_control_progress(token) >= 0.2 && !worker.is_finished();
                dr_job_control_cancel(token);
                let cancelled = Instant::now();
                let (output, error) = worker.join().unwrap();
                // The test is TOTAL for BOTH outcomes, because sampling `live` and
                // calling `cancel` are two statements: an assembly that finishes
                // in that gap publishes a document on a window that looked live,
                // and asserting a refusal there is a race this test would lose.
                // The OUTCOME is what is judged — no output means the cancel
                // landed and must have been reported promptly, an output means the
                // worker won and reported nothing — while a worker-won attempt
                // does not count as live and retries on the next, taller rung.
                let cancelled_live = if output == 0 {
                    assert!(error.contains("cancelled"), "{error}");
                    assert!(cancelled.elapsed() < Duration::from_millis(500));
                    true
                } else {
                    assert!(error.is_empty(), "{error}");
                    dr_doc_free(output as _); // The worker won the race.
                    false
                };
                assert_eq!(dr_job_control_scratch_bytes(token), 0);
                assert_eq!(dr_doc_layer_count(doc), 1);
                assert_eq!(
                    std::slice::from_raw_parts(dr_image_pixels_rgba(image), bytes.len()),
                    bytes.as_raw()
                );
                dr_job_control_free(token);
                dr_doc_free(doc);
                dr_image_free(image);
                observed_live = live && cancelled_live;
            }
            if observed_live {
                break;
            }
        }
        assert!(
            observed_live,
            "fixture did not overlap a live pixel copy in any attempt"
        );
    }
}
