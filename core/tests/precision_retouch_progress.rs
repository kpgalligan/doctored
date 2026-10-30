//! A completed source render must not finish the enclosing retouch job's
//! progress. Observe actual PatchMatch work, then cancel while it is active.
use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use std::ffi::{CStr, CString};
use std::ptr;

#[test]
fn rendered_source_and_backdrop_leave_progress_for_the_active_native_solve() {
    let (w, h) = (512u32, 512u32);
    let mut samples = vec![[12001, 23002, 34003, 65535]; (w * h) as usize];
    let mut coverage = vec![0u16; samples.len()];
    for y in 120..390 {
        for x in 120..390 {
            let i = (y * w + x) as usize;
            samples[i][..3].copy_from_slice(&[61001, 60003, 62007]);
            coverage[i] = 65535;
        }
    }
    let doc = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, samples, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    for (mode, output) in [("all", "layer"), ("current_and_below", "new_layer")] {
        let request=CString::new(serde_json::json!({"op":"spot_heal","layer":0,"sample":mode,"output":output,"x":120,"y":120,"width":270,"height":270}).to_string()).unwrap();
        let render = |token: usize| unsafe {
            let mut err = ptr::null_mut();
            let out = dr_precision_doc_edit(
                &doc,
                request.as_ptr(),
                coverage.as_ptr(),
                coverage.len(),
                REFERENCE_WORKING_BYTES,
                0,
                token as *const DrJobControl,
                &mut err,
            );
            if !out.is_null() {
                drop(Box::from_raw(out));
            }
            let message = if err.is_null() {
                String::new()
            } else {
                let message = CStr::from_ptr(err).to_string_lossy().into_owned();
                dr_string_free(err);
                message
            };
            (!out.is_null(), message)
        };
        let (entry, next) =
            entry_then_cancel(&|progress, stage, _| stage == 3 && progress > 0.2, render);
        let n0 = entry.checkpoints;
        assert!(
            next.expect("the entry's kernel never reported again")
                .checkpoints
                > n0 + 3,
            "the entry's kernel polls fewer than three checkpoints before its next report"
        );
        let parked = cancelled_parked_at(
            n0 + 3,
            &|progress, stage| stage == entry.stage && progress == entry.progress,
            render,
        );
        eprintln!("{mode}/{output}: {}", parked_line(&parked));
        assert!(
            (0.2..0.98).contains(&parked.progress),
            "{mode}/{output}: source rendering reported completed retouch progress {}",
            parked.progress
        );
    }
}

mod bench {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn rendered_source_and_backdrop_leave_progress_for_the_active_native_solve() {
        let (w, h) = (512u32, 512u32);
        let mut samples = vec![[12001, 23002, 34003, 65535]; (w * h) as usize];
        let mut coverage = vec![0u16; samples.len()];
        for y in 120..390 {
            for x in 120..390 {
                let i = (y * w + x) as usize;
                samples[i][..3].copy_from_slice(&[61001, 60003, 62007]);
                coverage[i] = 65535;
            }
        }
        let doc = PrecisionDocument::from_image(
            PrecisionImage::from_rgba16(w, h, samples, IccProfile::srgb(), None).unwrap(),
            Metadata::default(),
            Resolution::default(),
        )
        .unwrap();
        for (mode, output) in [("all", "layer"), ("current_and_below", "new_layer")] {
            let request=CString::new(serde_json::json!({"op":"spot_heal","layer":0,"sample":mode,"output":output,"x":120,"y":120,"width":270,"height":270}).to_string()).unwrap();
            let control = DrJobControl::default();
            std::thread::scope(|scope| {
                let worker = scope.spawn(|| unsafe {
                    let mut err = ptr::null_mut();
                    let out = dr_precision_doc_edit(
                        &doc,
                        request.as_ptr(),
                        coverage.as_ptr(),
                        coverage.len(),
                        REFERENCE_WORKING_BYTES,
                        0,
                        &control,
                        &mut err,
                    );
                    if !out.is_null() {
                        drop(Box::from_raw(out));
                    }
                    let message = if err.is_null() {
                        String::new()
                    } else {
                        let message = CStr::from_ptr(err).to_string_lossy().into_owned();
                        dr_string_free(err);
                        message
                    };
                    (out.is_null(), message)
                });
                let deadline = Instant::now() + Duration::from_secs(10);
                while (control.stage() != 3 || control.progress() <= 0.2)
                    && !worker.is_finished()
                    && Instant::now() < deadline
                {
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert_eq!(control.stage(), 3, "actual native inpainting must start");
                let progress = control.progress();
                assert!(
                    (0.2..0.98).contains(&progress),
                    "{mode}/{output}: source rendering reported completed retouch progress {progress}"
                );
                let start = Instant::now();
                control.cancel();
                let (absent, message) = worker.join().unwrap();
                assert!(absent && message.contains("cancel"));
                assert!(start.elapsed() < Duration::from_secs(2));
            });
            assert_eq!(control.scratch_bytes(), 0);
        }
    }
}
