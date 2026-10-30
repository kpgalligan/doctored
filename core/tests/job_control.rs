//! Worker-token behavior through the FFI: independent periodic reconstruction,
//! ordinary no-op compatibility, cancellation at each expensive stage,
//! memory-budget refusal and drop of all scratch reservations.

use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use common::*;
use doctored_core::doc::LayerRaster;
use doctored_core::{doc::DrDocument, ffi_jobs::*, job_control::DrJobControl};
use image::{Rgba, RgbaImage};
use std::{ptr, thread};

fn run(
    doc: &DrDocument,
    mask: &[u8],
    control: &DrJobControl,
    spot: bool,
    sample_all: bool,
) -> (Option<DrDocument>, Option<String>) {
    if !spot {
        return fill_edit(
            doc,
            0,
            mask,
            (doc.width, doc.height),
            (0, 12345),
            sample_all,
            control,
        );
    }
    let mut err = ptr::null_mut();
    let out = unsafe {
        let mut overlay = vec![0; mask.len() * 4];
        for (p, a) in overlay.chunks_exact_mut(4).zip(mask) {
            p[3] = *a;
        }
        dr_doc_spot_heal_layer_job(
            doc,
            0,
            overlay.as_ptr(),
            doc.width,
            doc.height,
            1.0,
            0,
            12345,
            0,
            if sample_all { 2 } else { 0 },
            false,
            control,
            &mut err,
        )
    };
    take_doc(out, err)
}

#[test]
fn both_job_entries_reconstruct_a_periodic_texture() {
    for spot in [false, true] {
        let original = doc_of(32, 32, |x, _| {
            let v = if (x / 2) % 2 == 0 { 40 } else { 200 };
            [v, v, v]
        });
        let mask = mask_of(32, 32, |x, y| {
            (12..20).contains(&x) && (12..20).contains(&y)
        });
        let broken = punch(&original, &mask, [128; 3]);
        let control = unsafe { dr_job_control_create() };
        assert!(!control.is_null());
        let (out, error) = run(&broken, &mask, unsafe { &*control }, spot, false);
        assert!(error.is_none(), "{error:?}");
        let out = out.unwrap();
        assert_eq!(
            *out.layers[0].pixels.contiguous_image(),
            *original.layers[0].pixels.contiguous_image(),
            "independent periodic oracle"
        );
        unsafe {
            assert_eq!(dr_job_control_progress(control), 1.0);
            assert_eq!(dr_job_control_stage(control), 5);
            assert_eq!(dr_job_control_scratch_bytes(control), 0);
            assert!(!dr_job_control_cancelled(control));
            assert!(!dr_job_control_set_scratch_limit(control, 1));
        }
        let (out, error) = run(&broken, &mask, unsafe { &*control }, spot, false);
        assert!(out.is_none());
        assert_eq!(error.as_deref(), Some("job control has already been used"));
        unsafe { dr_job_control_free(control) };
    }
}

#[test]
fn cancellation_before_start_noop_and_scratch_refusal_are_distinct() {
    let doc = doc_of(64, 64, |_, _| [70, 120, 180]);
    let mask = mask_of(64, 64, |x, y| {
        (24..40).contains(&x) && (24..40).contains(&y)
    });
    for spot in [false, true] {
        let cancelled = unsafe { dr_job_control_create() };
        unsafe {
            dr_job_control_cancel(cancelled);
            dr_job_control_cancel(cancelled);
        }
        let (out, error) = run(&doc, &mask, unsafe { &*cancelled }, spot, false);
        assert!(out.is_none());
        assert_eq!(error.as_deref(), Some("operation cancelled"));
        unsafe {
            assert_eq!(dr_job_control_scratch_bytes(cancelled), 0);
            dr_job_control_free(cancelled)
        };
        let budget = unsafe { dr_job_control_create() };
        unsafe {
            assert!(dr_job_control_set_scratch_limit(budget, 1));
        }
        let (out, error) = run(&doc, &mask, unsafe { &*budget }, spot, false);
        assert!(out.is_none());
        assert!(error.unwrap().contains("working-memory limit"));
        unsafe {
            assert_eq!(dr_job_control_scratch_bytes(budget), 0);
            dr_job_control_free(budget)
        };
        let noop = unsafe { dr_job_control_create() };
        let (out, error) = run(&doc, &vec![0; 64 * 64], unsafe { &*noop }, spot, false);
        assert!(out.is_none());
        assert!(error.is_none());
        unsafe {
            assert_eq!(dr_job_control_progress(noop), 1.0);
            assert_eq!(dr_job_control_scratch_bytes(noop), 0);
            dr_job_control_free(noop)
        };
    }
}

/// A reservation over the scratch limit that also reaches the host's
/// whole working limit says closing other documents will not help; one under
/// the whole limit, or with no whole limit set, keeps the remainder advice.
#[test]
fn a_reservation_over_the_whole_working_limit_says_so() {
    let doc = doc_of(64, 64, |_, _| [70, 120, 180]);
    let mask = mask_of(64, 64, |x, y| {
        (24..40).contains(&x) && (24..40).contains(&y)
    });
    for spot in [false, true] {
        for (working, on_its_own) in [(Some(1), true), (Some(u64::MAX), false), (None, false)] {
            let control = unsafe { dr_job_control_create() };
            unsafe {
                assert!(dr_job_control_set_scratch_limit(control, 1));
                if let Some(working) = working {
                    assert!(dr_job_control_set_working_limit(control, working));
                }
            }
            let (out, error) = run(&doc, &mask, unsafe { &*control }, spot, false);
            assert!(out.is_none());
            let error = error.unwrap();
            if on_its_own {
                assert!(error.contains("on its own"), "{error}");
                assert!(!error.contains("Close other documents"), "{error}");
            } else {
                assert!(error.contains("Close other documents"), "{error}");
            }
            unsafe {
                assert!(!dr_job_control_set_working_limit(control, 1));
                assert_eq!(dr_job_control_scratch_bytes(control), 0);
                dr_job_control_free(control)
            };
        }
    }
}

/// Cancels the fill (or spot heal) with the worker PARKED 16 checkpoints
/// after the first report of `stage` (stage 3: the first at or past progress
/// 0.30): past the stage's boundary, inside its kernel, where the bench
/// twin's 8 ms sleep aimed.
fn cancel_at_stage(width: u32, height: u32, stage: u32, spot: bool, diagonal: bool) {
    // Integer texture generation is independent of the fill's kernels/RNG.
    let pixels = RgbaImage::from_fn(width, height, |x, y| {
        Rgba([
            ((x.wrapping_mul(13) + y.wrapping_mul(7)) % 251) as u8,
            ((x / 4 + y / 3) % 229) as u8,
            ((x ^ y) % 239) as u8,
            255,
        ])
    });
    let doc = DrDocument::from_pixels(pixels);
    // The fill runs through the edit export, whose admission refuses a padded
    // box over `MAX_SOLVE_WINDOW_PIXELS` before stage 2: the full-canvas
    // diagonal of 48 MP pads to 48 MP. So the diagonal is confined to the
    // first 6000 x 4000 of the canvas, whose window (6512 x 4512 = 29.4 MP)
    // the admission passes; on the 24 MP canvas that is the whole diagonal.
    let (dw, dh) = (width.min(6000), height.min(4000));
    let mask = mask_of(width, height, |x, y| {
        if diagonal {
            x < dw
                && y < dh
                && (i64::from(x) * i64::from(dh) / i64::from(dw) - i64::from(y)).abs() < 2
        } else {
            (width / 2 - 400..width / 2 + 400).contains(&x)
                && (height / 2 - 400..height / 2 + 400).contains(&y)
        }
    });
    let before = doc.layers[0].pixels.clone();
    let render = |token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        let (out, error) = run(&doc, &mask, control, spot, false);
        (out.is_some(), error.unwrap_or_default())
    };
    let (entry, _) = entry_then_cancel(
        &|progress, s, _| s == stage && (stage != 3 || progress >= 0.30),
        render,
    );
    let parked = cancelled_parked_at(entry.checkpoints + 1 + 16, &|_, s| s == entry.stage, render);
    assert!(parked.progress >= entry.progress && parked.progress <= 1.0);
    eprintln!(
        "{width}x{height}, stage {stage}, spot={spot}, {}",
        parked_line(&parked)
    );
    assert!(
        LayerRaster::ptr_eq(&doc.layers[0].pixels, &before),
        "input handle must stay unchanged"
    );
}

#[test]
fn cancel_during_planning_on_24_and_48_megapixels() {
    cancel_at_stage(6000, 4000, 2, false, true);
    cancel_at_stage(8000, 6000, 2, false, true);
}
#[test]
fn cancel_during_patchmatch_fill_and_spot_healing() {
    for spot in [false, true] {
        cancel_at_stage(1800, 1200, 3, spot, false);
    }
}
#[test]
fn cancel_during_poisson_blending() {
    cancel_at_stage(1800, 1200, 4, false, false);
}

#[test]
fn job_ffi_nulls_and_invalid_input_lengths_are_safe() {
    unsafe {
        dr_job_control_free(ptr::null_mut());
        dr_job_control_cancel(ptr::null());
        assert!(!dr_job_control_cancelled(ptr::null()));
        assert_eq!(dr_job_control_progress(ptr::null()), 0.0);
        assert_eq!(dr_job_control_stage(ptr::null()), 0);
        assert_eq!(dr_job_control_scratch_bytes(ptr::null()), 0);
        assert!(!dr_job_control_set_scratch_limit(ptr::null(), 1));
        assert!(!dr_job_control_set_working_limit(ptr::null(), 1));
        assert!(!dr_job_control_begin_compound(ptr::null(), ptr::null_mut()));
        assert!(dr_doc_spot_heal_layer_job(
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            1.0,
            0,
            0,
            0,
            0,
            false,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        let doc = doc_of(8, 8, |_, _| [80; 3]);
        let one = [0u8; 4];
        let control = dr_job_control_create();
        let mut err = ptr::null_mut();
        let out = dr_doc_spot_heal_layer_job(
            &doc,
            0,
            one.as_ptr(),
            1,
            1,
            1.0,
            0,
            0,
            0,
            0,
            false,
            control,
            &mut err,
        );
        let (out, error) = take_doc(out, err);
        assert!(out.is_none());
        assert!(error.is_none());
        dr_job_control_free(control);
        let overlay = [255u8; 256];
        let mut err = ptr::null_mut();
        let out = dr_doc_spot_heal_layer_job(
            &doc,
            0,
            overlay.as_ptr(),
            8,
            8,
            1.0,
            0,
            0,
            0,
            0,
            false,
            ptr::null(),
            &mut err,
        );
        let (out, error) = take_doc(out, err);
        assert!(out.is_none());
        assert_eq!(error.as_deref(), Some("job control is NULL"));
    }
}

/// Compare bounded plain-raster source tiles with the declared full-frame group
/// fallback at their preparation cancellation boundaries.
#[test]
fn sample_all_preparation_cancellation_on_plain_48mp_and_grouped_24mp() {
    for grouped in [false, true] {
        let (w, h) = if grouped { (6000, 4000) } else { (8000, 6000) };
        let mut doc =
            DrDocument::from_pixels(RgbaImage::from_pixel(w, h, Rgba([70, 120, 180, 255])));
        let mut second = doc.layers[0].clone();
        second.opacity = 0.5;
        doc.layers.push(second);
        if grouped {
            let (mut result, group, _, _) = doc.group_layers(&[0, 1], "sampling group").unwrap();
            result.layers[group].opacity = 0.99;
            doc = result;
        }
        let mask = mask_of(w, h, |x, y| {
            (w / 2 - 400..w / 2 + 400).contains(&x) && (h / 2 - 400..h / 2 + 400).contains(&y)
        });
        let render = |token: usize| {
            let control = unsafe { &*(token as *const DrJobControl) };
            let (out, error) = run(&doc, &mask, control, false, true);
            (out.is_some(), error.unwrap_or_default())
        };
        // Parked 16 checkpoints past the entry, where the bench twin's 10 ms
        // sleep aimed.
        let (entry, _) = entry_then_cancel(&|progress, _, _| progress >= 0.03, render);
        let parked = cancelled_parked_at(
            entry.checkpoints + 1 + 16,
            &|_, stage| stage == entry.stage,
            render,
        );
        eprintln!(
            "{w}x{h} grouped={grouped} Sample All preparation {}",
            parked_line(&parked)
        );
    }
}

#[test]
fn tiled_sample_all_matches_full_composite_across_tile_edges_masks_offsets_and_blends() {
    use doctored_core::doc::BlendMode;
    use std::sync::Arc;
    for blend in [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Hue,
        BlendMode::Dissolve,
    ] {
        let (w, h) = (1100, 96);
        let mut doc = doc_of(w, h, |x, y| {
            [
                ((x * 13 + y * 7) % 251) as u8,
                ((x + y) % 229) as u8,
                ((x ^ y) % 239) as u8,
            ]
        });
        let mut overlay = DrDocument::from_pixels(RgbaImage::from_fn(1080, 100, |x, y| {
            Rgba([((x + y) % 255) as u8, 90, 200, 180])
        }))
        .layers
        .remove(0);
        overlay.offset = (13, -3);
        overlay.mask_properties.offset = overlay.offset;
        overlay.opacity = 0.61;
        overlay.blend = blend;
        overlay.clipped = true;
        overlay.mask = Some(Arc::new(image::GrayImage::from_fn(1080, 100, |x, y| {
            image::Luma([((x * 7 + y * 3) % 256) as u8])
        })));
        overlay.mask_enabled = true;
        doc.layers.push(overlay);
        let full = doc.flattened().unwrap();
        assert!(full.pixels().all(|p| p[3] == 255));
        // The independent reference source is the existing full-frame compositor.
        // Placing that opaque projection above the same destination preserves the
        // destination illumination while giving the fill exactly those samples.
        let mut reference = doc.clone();
        reference.layers.truncate(1);
        reference
            .layers
            .push(DrDocument::from_pixels(full).layers.remove(0));
        let mask = mask_of(w, h, |x, y| {
            (1016..1040).contains(&x) && (56..80).contains(&y)
        });
        let (expected, error) = fill(&reference, 0, &mask, (w, h), (0, 12345), true);
        assert!(error.is_none());
        let token = unsafe { dr_job_control_create() };
        let (actual, error) = run(&doc, &mask, unsafe { &*token }, false, true);
        assert!(error.is_none(), "{error:?}");
        assert_eq!(
            *actual.unwrap().layers[0].pixels.contiguous_image(),
            *expected.unwrap().layers[0].pixels.contiguous_image(),
            "tile source differs for {blend:?}"
        );
        unsafe { dr_job_control_free(token) };
    }
}

#[test]
fn compound_parent_claim_is_single_use_and_precancellation_is_atomic() {
    unsafe {
        for cancelled in [false, true] {
            let parent = dr_job_control_create();
            assert!(!parent.is_null());
            assert!(dr_job_control_set_scratch_limit(parent, 4096));
            if cancelled {
                dr_job_control_cancel(parent);
            }
            let mut error = ptr::null_mut();
            assert_eq!(
                dr_job_control_begin_compound(parent, &mut error),
                !cancelled
            );
            if cancelled {
                assert!(take_err_string(error).contains("cancelled"));
            } else {
                assert!(error.is_null());
            }
            assert_eq!(dr_job_control_scratch_bytes(parent), 0);
            assert_eq!(dr_job_control_progress(parent), 0.0);
            assert!(!dr_job_control_set_scratch_limit(parent, 8192));
            error = ptr::null_mut();
            assert!(!dr_job_control_begin_compound(parent, &mut error));
            assert!(take_err_string(error).contains("already been used"));
            dr_job_control_free(parent);
        }
    }
}

#[test]
fn compound_and_ordinary_job_claims_cannot_reuse_each_others_tokens() {
    let document = doc_of(8, 8, |_, _| [70, 120, 180]);
    let empty = vec![0; 64];
    unsafe {
        let parent = dr_job_control_create();
        assert!(dr_job_control_begin_compound(parent, ptr::null_mut()));
        let (output, error) = run(&document, &empty, &*parent, false, false);
        assert!(output.is_none());
        assert_eq!(error.as_deref(), Some("job control has already been used"));
        assert_eq!(dr_job_control_scratch_bytes(parent), 0);
        dr_job_control_free(parent);

        let ordinary = dr_job_control_create();
        let (output, error) = run(&document, &empty, &*ordinary, false, false);
        assert!(output.is_none() && error.is_none());
        let mut error = ptr::null_mut();
        assert!(!dr_job_control_begin_compound(ordinary, &mut error));
        assert!(take_err_string(error).contains("already been used"));
        assert_eq!(dr_job_control_scratch_bytes(ordinary), 0);
        dr_job_control_free(ordinary);
    }
}

mod bench {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn cancel_during_planning_on_24_and_48_megapixels() {
        cancel_at_stage(6000, 4000, 2, false, true);
        cancel_at_stage(8000, 6000, 2, false, true);
    }

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn cancel_during_patchmatch_fill_and_spot_healing() {
        for spot in [false, true] {
            cancel_at_stage(1800, 1200, 3, spot, false);
        }
    }

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn cancel_during_poisson_blending() {
        cancel_at_stage(1800, 1200, 4, false, false);
    }

    /// Compare bounded plain-raster source tiles with the declared full-frame group
    /// fallback at their preparation cancellation boundaries.
    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn sample_all_preparation_cancellation_on_plain_48mp_and_grouped_24mp() {
        for grouped in [false, true] {
            let (w, h) = if grouped { (6000, 4000) } else { (8000, 6000) };
            let mut doc =
                DrDocument::from_pixels(RgbaImage::from_pixel(w, h, Rgba([70, 120, 180, 255])));
            let mut second = doc.layers[0].clone();
            second.opacity = 0.5;
            doc.layers.push(second);
            if grouped {
                let (mut result, group, _, _) =
                    doc.group_layers(&[0, 1], "sampling group").unwrap();
                result.layers[group].opacity = 0.99;
                doc = result;
            }
            let mask = mask_of(w, h, |x, y| {
                (w / 2 - 400..w / 2 + 400).contains(&x) && (h / 2 - 400..h / 2 + 400).contains(&y)
            });
            let handle = unsafe { dr_job_control_create() };
            let control = unsafe { &*handle };
            thread::scope(|scope| {
                let worker = scope.spawn(|| run(&doc, &mask, control, false, true));
                let deadline = Instant::now() + Duration::from_secs(30);
                while unsafe { dr_job_control_progress(control) } < 0.03 && !worker.is_finished() {
                    assert!(Instant::now() < deadline);
                    thread::sleep(Duration::from_millis(1));
                }
                assert!(!worker.is_finished());
                thread::sleep(Duration::from_millis(10));
                let started = Instant::now();
                unsafe { dr_job_control_cancel(control) };
                let (out, error) = worker.join().unwrap();
                let elapsed = started.elapsed();
                assert!(out.is_none());
                assert_eq!(error.as_deref(), Some("operation cancelled"));
                assert!(
                    elapsed < Duration::from_secs(2),
                    "source preparation cancellation took {elapsed:?}"
                );
                assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
                eprintln!(
                    "{w}x{h} grouped={grouped} Sample All preparation cancel-to-return {} ms",
                    elapsed.as_secs_f64() * 1000.0
                );
            });
            unsafe { dr_job_control_free(handle) };
        }
    }

    fn cancel_at_stage(width: u32, height: u32, stage: u32, spot: bool, diagonal: bool) {
        // Integer texture generation is independent of the fill's kernels/RNG.
        let pixels = RgbaImage::from_fn(width, height, |x, y| {
            Rgba([
                ((x.wrapping_mul(13) + y.wrapping_mul(7)) % 251) as u8,
                ((x / 4 + y / 3) % 229) as u8,
                ((x ^ y) % 239) as u8,
                255,
            ])
        });
        let doc = DrDocument::from_pixels(pixels);
        // The fill runs through the edit export, whose admission refuses a padded
        // box over `MAX_SOLVE_WINDOW_PIXELS` before stage 2: the full-canvas
        // diagonal of 48 MP pads to 48 MP. So the diagonal is confined to the
        // first 6000 x 4000 of the canvas, whose window (6512 x 4512 = 29.4 MP)
        // the admission passes; on the 24 MP canvas that is the whole diagonal.
        let (dw, dh) = (width.min(6000), height.min(4000));
        let mask = mask_of(width, height, |x, y| {
            if diagonal {
                x < dw
                    && y < dh
                    && (i64::from(x) * i64::from(dh) / i64::from(dw) - i64::from(y)).abs() < 2
            } else {
                (width / 2 - 400..width / 2 + 400).contains(&x)
                    && (height / 2 - 400..height / 2 + 400).contains(&y)
            }
        });
        let before = doc.layers[0].pixels.clone();
        let handle = unsafe { dr_job_control_create() };
        let control = unsafe { &*handle };
        thread::scope(|scope| {
            let worker = scope.spawn(|| run(&doc, &mask, control, spot, false));
            let deadline = Instant::now() + Duration::from_secs(60);
            let mut previous = 0.0;
            while (unsafe { dr_job_control_stage(control) } != stage
                || (stage == 3 && unsafe { dr_job_control_progress(control) } < 0.30))
                && !worker.is_finished()
            {
                let progress = unsafe { dr_job_control_progress(control) };
                assert!(progress >= previous && progress <= 1.0);
                previous = progress;
                assert!(
                    Instant::now() < deadline,
                    "worker never reached stage {stage}"
                );
                thread::sleep(Duration::from_millis(1));
            }
            assert!(
                !worker.is_finished(),
                "job completed before cancellation at stage {stage}"
            );
            // Give the stage's kernel time to start, so a boundary-only token would fail.
            thread::sleep(Duration::from_millis(8));
            let start = Instant::now();
            unsafe { dr_job_control_cancel(control) };
            let (out, error) = worker.join().unwrap();
            let elapsed = start.elapsed();
            assert!(out.is_none());
            assert_eq!(error.as_deref(), Some("operation cancelled"));
            assert!(
                elapsed < Duration::from_secs(2),
                "cooperative cancellation exceeded 2 s: {elapsed:?}"
            );
            unsafe {
                assert!(dr_job_control_cancelled(control));
                assert_eq!(dr_job_control_scratch_bytes(control), 0)
            };
            eprintln!(
                "{width}x{height}, stage {stage}, spot={spot}, cancel-to-return {} ms",
                elapsed.as_secs_f64() * 1000.0
            );
        });
        assert!(
            LayerRaster::ptr_eq(&doc.layers[0].pixels, &before),
            "input handle must stay unchanged"
        );
        unsafe { dr_job_control_free(handle) };
    }
}
