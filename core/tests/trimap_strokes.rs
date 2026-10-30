//! Categorical brush FFI: independent geometric oracle and atomic refusal.
use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use doctored_core::job_control::DrJobControl;
use doctored_core::{ffi::dr_string_free, ffi_jobs::*, ffi_trimap::rz_trimap_strokes};
use serde_json::{json, Value};
use std::{
    ffi::{CStr, CString},
    ptr,
};

fn payload(w: u32, h: u32, strokes: Value) -> Value {
    json!({"version":1,"width":w,"height":h,"strokes":strokes})
}
fn invoke(base: &[u8], w: u32, h: u32, value: &Value) -> Result<Vec<u8>, String> {
    let encoded = CString::new(value.to_string()).unwrap();
    let mut out = vec![73; base.len()];
    let mut error = ptr::null_mut();
    unsafe {
        let ok = rz_trimap_strokes(
            base.as_ptr(),
            w,
            h,
            encoded.as_ptr(),
            out.as_mut_ptr(),
            ptr::null(),
            &mut error,
        );
        let message = if error.is_null() {
            String::new()
        } else {
            let value = CStr::from_ptr(error).to_string_lossy().into_owned();
            dr_string_free(error);
            value
        };
        if ok {
            Ok(out)
        } else {
            assert!(out.iter().all(|x| *x == 73));
            Err(message)
        }
    }
}

// Independent capsule oracle: endpoint half planes plus cross-product distance
// to the infinite line between them, without the production projected point.
fn inside(p: [f64; 2], a: [f64; 2], b: [f64; 2], radius: f64) -> bool {
    let v = [b[0] - a[0], b[1] - a[1]];
    let pa = [p[0] - a[0], p[1] - a[1]];
    let pb = [p[0] - b[0], p[1] - b[1]];
    if pa[0] * v[0] + pa[1] * v[1] <= 0.0 {
        return pa[0].hypot(pa[1]) <= radius;
    }
    if pb[0] * v[0] + pb[1] * v[1] >= 0.0 {
        return pb[0].hypot(pb[1]) <= radius;
    }
    (pa[0] * v[1] - pa[1] * v[0]).abs() <= radius * v[0].hypot(v[1])
}

#[test]
fn categorical_round_caps_clipping_and_sparse_lines_match_independent_oracle() {
    let (w, h) = (31, 19);
    let base: Vec<_> = (0..w * h).map(|i| [0, 128, 255][i as usize % 3]).collect();
    let cases = [
        ([-10.0, -7.0], [20.5, 16.5], 2.0),
        ([5.5, 5.5], [5.5, 5.5], 0.5),
        ([0.0, 10.0], [30.0, 10.0], 1.5),
        ([99.0, 90.0], [120.0, 80.0], 2.0),
        ([0.1, 0.2], [29.7, 17.3], 3.125),
    ];
    for (a, b, radius) in cases {
        let out = invoke(
            &base,
            w,
            h,
            &payload(
                w,
                h,
                json!([{"label":"unknown","radius":radius,"points":[a,b]}]),
            ),
        )
        .unwrap();
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                assert_eq!(
                    out[i],
                    if inside([x as f64 + 0.5, y as f64 + 0.5], a, b, radius) {
                        128
                    } else {
                        base[i]
                    },
                    "({x},{y}) segment {a:?}->{b:?}"
                );
            }
        }
    }
}

#[test]
fn ordered_labels_pin_or_relax_constraints_and_single_dab_matches_zero_length() {
    let strokes = json!([
        {"label":"foreground","radius":4,"points":[[4.5,4.5]]},
        {"label":"background","radius":2,"points":[[4.5,4.5]]},
        {"label":"unknown","radius":0.5,"points":[[4.5,4.5]]}
    ]);
    let base = vec![128; 81];
    let out = invoke(&base, 9, 9, &payload(9, 9, strokes.clone())).unwrap();
    assert_eq!(out[4 * 9 + 4], 128);
    assert_eq!(out[4 * 9 + 5], 0);
    assert_eq!(out[4 * 9 + 7], 255);
    assert_eq!(out[0], 128);
    let mut repeated = strokes;
    for stroke in repeated.as_array_mut().unwrap() {
        stroke["points"] = json!([[4.5, 4.5], [4.5, 4.5]]);
    }
    assert_eq!(out, invoke(&base, 9, 9, &payload(9, 9, repeated)).unwrap());
    let encoded = CString::new(payload(9, 9, json!([])).to_string()).unwrap();
    let mut in_place = out.clone();
    unsafe {
        assert!(rz_trimap_strokes(
            in_place.as_ptr(),
            9,
            9,
            encoded.as_ptr(),
            in_place.as_mut_ptr(),
            ptr::null(),
            ptr::null_mut()
        ));
    }
    assert_eq!(in_place, out);
}

#[test]
fn malformed_payloads_labels_dimensions_and_limits_refuse_atomically() {
    let valid = payload(
        1,
        1,
        json!([{"label":"foreground","radius":1,"points":[[0,0]]}]),
    );
    let mut bad = Vec::new();
    for (key, value) in [
        ("version", json!(2)),
        ("version", json!(true)),
        ("width", json!(2)),
        ("extra", json!(0)),
    ] {
        let mut p = valid.clone();
        p[key] = value;
        bad.push(p);
    }
    for (key, value) in [
        ("radius", json!(0.49)),
        ("radius", json!(513)),
        ("radius", Value::Null),
        ("label", json!("soft")),
        ("extra", json!(0)),
        ("points", json!([])),
        ("points", json!([[100001, 0]])),
        ("points", json!([[0, 0, 0]])),
        ("points", json!([[true, 0]])),
        ("points", json!(vec![[0, 0]; 8193])),
    ] {
        let mut p = valid.clone();
        p["strokes"][0][key] = value;
        bad.push(p);
    }
    let mut p = valid.clone();
    p["strokes"] = json!(vec![valid["strokes"][0].clone(); 257]);
    bad.push(p);
    let mut stroke = valid["strokes"][0].clone();
    stroke["points"] = json!(vec![[0, 0]; 8192]);
    let mut p = valid.clone();
    p["strokes"] = json!(vec![stroke; 9]);
    bad.push(p);
    for value in bad {
        assert!(!invoke(&[128], 1, 1, &value).unwrap_err().is_empty());
    }
    assert!(invoke(&[127], 1, 1, &valid)
        .unwrap_err()
        .contains("noncategorical"));
    assert!(invoke(&[128], 0, 1, &valid)
        .unwrap_err()
        .contains("dimensions"));
    assert!(invoke(&[128], u32::MAX, u32::MAX, &valid)
        .unwrap_err()
        .contains("dimensions"));
    unsafe {
        let mut out = 73;
        let mut err = ptr::null_mut();
        let big = CString::new(" ".repeat(4 * 1024 * 1024 + 1)).unwrap();
        assert!(!rz_trimap_strokes(
            [128].as_ptr(),
            1,
            1,
            big.as_ptr(),
            &mut out,
            ptr::null(),
            &mut err
        ));
        assert_eq!(out, 73);
        assert!(!err.is_null());
        dr_string_free(err);
    }
}

#[test]
fn repeated_segment_box_work_is_bounded_before_publication() {
    // Each diagonal box covers the full canvas, even when most pixel centers
    // miss the thin stroke. Bounding only union area would allow unbounded work.
    let points: Vec<_> = (0..1001)
        .map(|i| if i % 2 == 0 { [0, 0] } else { [256, 256] })
        .collect();
    let p = payload(
        256,
        256,
        json!([{"label":"foreground","radius":0.5,"points":points}]),
    );
    assert!(invoke(&vec![128; 256 * 256], 256, 256, &p)
        .unwrap_err()
        .contains("64 million"));
}

#[test]
fn scratch_refusal_precancel_and_token_reuse_keep_output_unchanged() {
    let encoded = CString::new(payload(1, 1, json!([])).to_string()).unwrap();
    unsafe {
        for cancelled in [true, false] {
            let token = dr_job_control_create();
            let mut out = 73;
            if cancelled {
                dr_job_control_cancel(token);
            } else {
                assert!(dr_job_control_set_scratch_limit(token, 1));
            }
            assert!(!rz_trimap_strokes(
                [128].as_ptr(),
                1,
                1,
                encoded.as_ptr(),
                &mut out,
                token,
                ptr::null_mut()
            ));
            assert_eq!(out, 73);
            assert_eq!(dr_job_control_scratch_bytes(token), 0);
            assert!(!rz_trimap_strokes(
                [128].as_ptr(),
                1,
                1,
                encoded.as_ptr(),
                &mut out,
                token,
                ptr::null_mut()
            ));
            assert_eq!(out, 73);
            dr_job_control_free(token);
        }
    }
}

#[test]
fn live_cancellation_during_repeated_painting_is_atomic_and_releases_scratch() {
    let points: Vec<_> = (0..901)
        .map(|i| if i % 2 == 0 { [0, 0] } else { [256, 256] })
        .collect();
    let encoded = CString::new(
        payload(
            256,
            256,
            json!([{"label":"foreground","radius":0.5,"points":points}]),
        )
        .to_string(),
    )
    .unwrap();
    let written = std::sync::Mutex::new(Vec::new());
    let render = |token: usize| unsafe {
        let mut out = vec![73; 256 * 256];
        let base = vec![128; out.len()];
        let mut error = ptr::null_mut();
        let ok = rz_trimap_strokes(
            base.as_ptr(),
            256,
            256,
            encoded.as_ptr(),
            out.as_mut_ptr(),
            token as *const DrJobControl,
            &mut error,
        );
        *written.lock().unwrap() = out;
        let message = if error.is_null() {
            String::new()
        } else {
            let message = CStr::from_ptr(error).to_string_lossy().into_owned();
            dr_string_free(error);
            message
        };
        (ok, message)
    };
    let (entry, next) = entry_then_cancel(&|_, stage, _| stage == 6, render);
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
    eprintln!(
        "trimap live cancellation after paint began: {}",
        parked_line(&parked)
    );
    assert!(written.lock().unwrap().iter().all(|v| *v == 73));
}

#[test]
fn required_null_arguments_refuse() {
    let encoded = CString::new(payload(1, 1, json!([])).to_string()).unwrap();
    let mut out = 73;
    unsafe {
        assert!(!rz_trimap_strokes(
            ptr::null(),
            1,
            1,
            encoded.as_ptr(),
            &mut out,
            ptr::null(),
            ptr::null_mut()
        ));
        assert!(!rz_trimap_strokes(
            [128].as_ptr(),
            1,
            1,
            ptr::null(),
            &mut out,
            ptr::null(),
            ptr::null_mut()
        ));
        assert!(!rz_trimap_strokes(
            [128].as_ptr(),
            1,
            1,
            encoded.as_ptr(),
            ptr::null_mut(),
            ptr::null(),
            ptr::null_mut()
        ));
    }
    assert_eq!(out, 73);
}

mod bench {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn live_cancellation_during_repeated_painting_is_atomic_and_releases_scratch() {
        let points: Vec<_> = (0..901)
            .map(|i| if i % 2 == 0 { [0, 0] } else { [256, 256] })
            .collect();
        let encoded = CString::new(
            payload(
                256,
                256,
                json!([{"label":"foreground","radius":0.5,"points":points}]),
            )
            .to_string(),
        )
        .unwrap();
        unsafe {
            let token = dr_job_control_create();
            let address = token as usize;
            let worker = std::thread::spawn(move || {
                let token = address as *const doctored_core::job_control::DrJobControl;
                let mut out = vec![73; 256 * 256];
                let base = vec![128; out.len()];
                let ok = rz_trimap_strokes(
                    base.as_ptr(),
                    256,
                    256,
                    encoded.as_ptr(),
                    out.as_mut_ptr(),
                    token,
                    ptr::null_mut(),
                );
                (ok, out)
            });
            let deadline = Instant::now() + Duration::from_secs(3);
            while dr_job_control_stage(token) != 6
                && !worker.is_finished()
                && Instant::now() < deadline
            {
                std::thread::yield_now();
            }
            assert_eq!(
                dr_job_control_stage(token),
                6,
                "worker must reach actual pixel iteration"
            );
            let started = Instant::now();
            dr_job_control_cancel(token);
            let (ok, out) = worker.join().unwrap();
            assert!(!ok);
            assert!(out.iter().all(|v| *v == 73));
            assert_eq!(dr_job_control_scratch_bytes(token), 0);
            assert!(started.elapsed() < Duration::from_secs(2));
            eprintln!(
                "trimap live cancellation after paint began: {:?}",
                started.elapsed()
            );
            dr_job_control_free(token);
        }
    }
}
