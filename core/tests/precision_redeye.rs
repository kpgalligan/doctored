//! Native red-eye references use continuous source codes, exact alpha and the
//! documented neutralize-then-darken formula, independently evaluated in f64.
use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_retouch::RetouchEdit;
use serde_json::json;

fn document(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}
fn fixture() -> (PrecisionDocument, Vec<[u16; 4]>) {
    let mut pixels = vec![[52103, 42007, 39011, 65535]; 40 * 40];
    for y in 8..32 {
        for x in 8..32 {
            pixels[y * 40 + x] = [50007, 10001, 9003, 65535];
        }
    }
    pixels[14 * 40 + 14][3] = 1;
    pixels[25 * 40 + 25] = [65521, 65511, 65501, 1];
    (document(40, 40, pixels.clone()), pixels)
}
fn request() -> RetouchEdit {
    RetouchEdit::parse(&json!({"op":"red_eye","layer":0,"x":0,"y":0,"width":40,"height":40,"pupil_size":1.0,"darken":0.5}).to_string()).unwrap()
}
#[test]
fn normalized_correction_preserves_single_code_alpha_and_partial_selection() {
    let (doc, before) = fixture();
    let mut coverage = vec![65535; 1600];
    coverage[14 * 40 + 14] = 32769;
    coverage[15 * 40 + 15] = 0;
    let out = doc
        .retouch(&request(), &coverage, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap();
    let after = out.layer_source(0).unwrap().to_rgba16().ok().unwrap();
    let p = before[14 * 40 + 14];
    let factor = 32769. / 65535.;
    let neutral = (f64::from(p[1]) + f64::from(p[2])) / 2.;
    let corrected = [neutral * 0.6, f64::from(p[1]) * 0.6, f64::from(p[2]) * 0.6];
    for ch in 0..3 {
        let expected =
            (f64::from(p[ch]) + factor * (corrected[ch] - f64::from(p[ch]))).round() as u16;
        assert!(after[14 * 40 + 14][ch].abs_diff(expected) <= 1);
    }
    for (i, p) in after.iter().enumerate() {
        assert_eq!(p[3], before[i][3]);
        if coverage[i] == 0 || i / 40 < 8 || i / 40 >= 32 || i % 40 < 8 || i % 40 >= 32 {
            assert_eq!(*p, before[i]);
        }
    }
    assert_eq!(
        after[25 * 40 + 25],
        before[25 * 40 + 25],
        "catchlight exact"
    );
    assert!(after.iter().any(|p| p[..3].iter().any(|c| c % 257 != 0)));
}
#[test]
fn red_field_pupil_gate_and_outside_rectangle_are_exact_noops() {
    let doc = document(40, 40, vec![[50007, 10001, 9003, 65535]; 1600]);
    assert!(doc
        .retouch(
            &request(),
            &vec![65535; 1600],
            REFERENCE_WORKING_BYTES,
            0,
            None
        )
        .unwrap()
        .is_none());
    let (doc, _) = fixture();
    let mut req = request();
    req.x = -50;
    req.y = -50;
    req.width = 3;
    req.height = 3;
    assert!(doc
        .retouch(&req, &vec![65535; 1600], REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .is_none());
    req = request();
    req.x = -4;
    req.y = -4;
    req.width = 44;
    req.height = 44;
    let clipped = doc
        .retouch(&req, &vec![65535; 1600], REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap();
    let ordinary = doc
        .retouch(
            &request(),
            &vec![65535; 1600],
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        clipped.layer_source(0).unwrap().to_rgba16().ok(),
        ordinary.layer_source(0).unwrap().to_rgba16().ok()
    );
}
#[test]
fn red_eye_rejects_ignored_arguments_and_cancels_without_a_result() {
    for extra in [
        json!({"matrix":[1,0,0,1,0,0]}),
        json!({"opacity":0.5}),
        json!({"ring":0}),
    ] {
        let mut value = json!({"op":"red_eye","layer":0,"x":0,"y":0,"width":1,"height":1});
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert!(RetouchEdit::parse(&value.to_string()).is_err());
    }
    let (doc, _) = fixture();
    let job = DrJobControl::default();
    job.cancel();
    assert!(doc
        .retouch(
            &request(),
            &vec![65535; 1600],
            REFERENCE_WORKING_BYTES,
            0,
            Some(&job)
        )
        .err()
        .unwrap()
        .contains("cancel"));
    assert_eq!(job.scratch_bytes(), 0);
}
#[test]
fn active_native_classifier_cancellation_is_bounded() {
    let doc = document(1024, 1024, vec![[50007, 10001, 9003, 65535]; 1024 * 1024]);
    let mut req = request();
    req.width = 1024;
    req.height = 1024;
    let coverage = vec![65535; 1024 * 1024];
    let render = |token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match doc.retouch(&req, &coverage, REFERENCE_WORKING_BYTES, 0, Some(control)) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    };
    // The entry is `begin()`'s report; the op's first `reserve` is the next
    // checkpoint, so k = 1 parks on the first checkpoint after it stores.
    let (entry, _) = entry_then_cancel(&|_, stage, _| stage == 1, render);
    let parked = cancelled_parked_at(entry.checkpoints + 2, &|_, stage| stage == 1, render);
    eprintln!("red-eye classifier cancellation: {}", parked_line(&parked));
    assert!(parked.scratch > 0, "worker must actually begin");
}

mod bench {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn active_native_classifier_cancellation_is_bounded() {
        let doc = document(1024, 1024, vec![[50007, 10001, 9003, 65535]; 1024 * 1024]);
        let mut req = request();
        req.width = 1024;
        req.height = 1024;
        let job = DrJobControl::default();
        let coverage = vec![65535; 1024 * 1024];
        std::thread::scope(|scope| {
            let worker = scope
                .spawn(|| doc.retouch(&req, &coverage, REFERENCE_WORKING_BYTES, 0, Some(&job)));
            let deadline = Instant::now() + Duration::from_secs(5);
            while job.scratch_bytes() == 0 && Instant::now() < deadline {
                std::thread::yield_now();
            }
            assert!(job.scratch_bytes() > 0, "worker must actually begin");
            let start = Instant::now();
            job.cancel();
            assert!(worker.join().unwrap().err().unwrap().contains("cancel"));
            assert!(start.elapsed() < Duration::from_secs(5));
        });
        assert_eq!(job.scratch_bytes(), 0);
    }
}
