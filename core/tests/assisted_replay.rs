//! Assisted annotation replay FFI: independent capsule oracle for the label
//! plane, stack reconstruction, and the exact set of documents that refuse.
//! Every call goes through the exported C functions; nothing imports the
//! module's own types.
use doctored_core::{
    ffi::dr_string_free,
    ffi_assisted::{dr_assisted_replay_plane, dr_assisted_replay_summary},
};
use serde_json::{json, Value};
use std::{
    ffi::{CStr, CString},
    ptr,
};

const FILLER: u8 = 73;

fn digest(tag: char) -> String {
    std::iter::repeat_n(tag, 64).collect()
}

/// One event, with only the fields that kind carries.
fn event(index: u32, gesture: Option<u32>, kind: &str, elapsed: u64, result: &str) -> Value {
    json!({"index":index,"gesture_index":gesture,"kind":kind,"origin":"human","tool":Value::Null,
        "diameter":Value::Null,"points":Value::Null,"elapsed_ms":elapsed,"result":result,
        "detail":Value::Null,"label_plane_sha256":digest('b')})
}

fn stroke(
    index: u32,
    gesture: u32,
    tool: &str,
    diameter: u32,
    points: &[[i64; 2]],
    elapsed: u64,
    result: &str,
) -> Value {
    let mut value = event(index, Some(gesture), "stroke", elapsed, result);
    value["tool"] = json!(tool);
    value["diameter"] = json!(diameter);
    value["points"] = json!(points);
    value
}

/// A valid in-progress human replay carrying `events`.
fn replay(width: u32, height: u32, events: Vec<Value>) -> Value {
    let gestures = events
        .iter()
        .filter(|e| matches!(e["kind"].as_str(), Some("stroke" | "undo" | "redo")))
        .count();
    json!({
        "replay_schema_version":1,"protocol_id":"phase9i-assisted-edges-v1",
        "workspace_id":"workspace","session_id":"session","case_id":"case",
        "width":width,"height":height,"origin":"human","human_origin":true,"scoreable":true,
        "annotator_id":"tester","annotator_reports_prior_exposure":false,
        "blinding_attestation":{"source_only":true,"text":"source only","attested_at_utc":"2026-09-15T00:00:00Z"},
        "displayed_assets":{"source_png_sha256":digest('0'),"source_plane_sha256":digest('1'),
            "prior_png_sha256":digest('2'),"prior_plane_sha256":digest('3'),"task_card_sha256":digest('4')},
        "budget":{"seconds":300,"modifying_gestures":20,"brush_diameters":[3,9,27,81],"max_stroke_points":256},
        "started_at_utc":"2026-09-15T00:00:00Z","ended_at_utc":Value::Null,
        "elapsed_ms":1000,"modifying_gestures":gestures,"outcome":Value::Null,
        "events":events,"label_plane":Value::Null,"write_planes":Value::Null,
        "program":{"app_version":"test","rasterizer":"rz_trimap_strokes/v1"}
    })
}

fn plane(document: &Value, width: u32, height: u32) -> Result<Vec<u8>, String> {
    plane_text(&document.to_string(), width, height)
}

fn plane_text(text: &str, width: u32, height: u32) -> Result<Vec<u8>, String> {
    let encoded = CString::new(text).expect("replay without interior NUL");
    let mut out = vec![FILLER; width as usize * height as usize];
    let mut error = ptr::null_mut();
    unsafe {
        let ok = dr_assisted_replay_plane(
            encoded.as_ptr(),
            width,
            height,
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
            assert!(message.is_empty());
            Ok(out)
        } else {
            assert!(out.iter().all(|byte| *byte == FILLER), "buffer was touched");
            assert!(!message.is_empty());
            Err(message)
        }
    }
}

fn summary(document: &Value) -> Result<Value, String> {
    let encoded = CString::new(document.to_string()).expect("replay without interior NUL");
    let mut error = ptr::null_mut();
    unsafe {
        let text = dr_assisted_replay_summary(encoded.as_ptr(), &mut error);
        if text.is_null() {
            assert!(!error.is_null());
            let message = CStr::from_ptr(error).to_string_lossy().into_owned();
            dr_string_free(error);
            return Err(message);
        }
        assert!(error.is_null());
        let value = CStr::from_ptr(text).to_string_lossy().into_owned();
        dr_string_free(text);
        Ok(serde_json::from_str(&value).expect("summary JSON"))
    }
}

/// Independent capsule oracle: endpoint half planes plus cross-product
/// distance to the infinite line, never the production projected point.
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

/// The whole applied set, rasterized independently: all-128 base, then each
/// stroke's consecutive segments in order, last write wins.
fn oracle(width: u32, height: u32, strokes: &[(u8, u32, Vec<[i64; 2]>)]) -> Vec<u8> {
    let mut out = vec![128u8; width as usize * height as usize];
    for (label, diameter, points) in strokes {
        let radius = f64::from(*diameter) / 2.0;
        for y in 0..height {
            for x in 0..width {
                let p = [f64::from(x) + 0.5, f64::from(y) + 0.5];
                let hit = (0..points.len().saturating_sub(1).max(1)).any(|index| {
                    let a = points[index];
                    let b = points[(index + 1).min(points.len() - 1)];
                    inside(
                        p,
                        [a[0] as f64 + 0.5, a[1] as f64 + 0.5],
                        [b[0] as f64 + 0.5, b[1] as f64 + 0.5],
                        radius,
                    )
                });
                if hit {
                    out[(y * width + x) as usize] = *label;
                }
            }
        }
    }
    out
}

#[test]
fn null_arguments_refuse_without_writing() {
    let mut out = [FILLER; 4];
    let mut error = ptr::null_mut();
    unsafe {
        assert!(!dr_assisted_replay_plane(
            ptr::null(),
            2,
            2,
            out.as_mut_ptr(),
            ptr::null(),
            &mut error
        ));
        assert!(error.is_null());
        let valid = CString::new(replay(2, 2, vec![]).to_string()).expect("replay");
        assert!(!dr_assisted_replay_plane(
            valid.as_ptr(),
            2,
            2,
            ptr::null_mut(),
            ptr::null(),
            &mut error
        ));
        assert!(error.is_null());
        assert!(dr_assisted_replay_summary(ptr::null(), &mut error).is_null());
        assert!(!error.is_null());
        dr_string_free(error);
        assert!(dr_assisted_replay_summary(ptr::null(), ptr::null_mut()).is_null());
    }
    assert!(out.iter().all(|byte| *byte == FILLER));
}

#[test]
fn two_stroke_plane_matches_the_independent_capsule_oracle() {
    let (w, h) = (31u32, 19u32);
    let first = vec![[4, 4], [20, 10], [24, 4]];
    let second = vec![[10, 2], [10, 16]];
    let document = replay(
        w,
        h,
        vec![
            stroke(0, 1, "foreground", 9, &first, 1000, "accepted"),
            stroke(1, 2, "background", 3, &second, 2000, "accepted"),
        ],
    );
    let produced = plane(&document, w, h).expect("plane");
    let expected = oracle(w, h, &[(255, 9, first), (0, 3, second)]);
    assert_eq!(produced, expected);
    assert!(produced.contains(&255));
    assert!(produced.contains(&0));
    assert!(produced.contains(&128));
    let reported = summary(&document).expect("summary");
    assert_eq!(reported["active_strokes"], json!(2));
    assert_eq!(reported["modifying_gestures"], json!(2));
    assert_eq!(reported["final"], json!(false));
    assert_eq!(reported["outcome"], Value::Null);
    assert_eq!(reported["human_origin"], json!(true));
    assert_eq!(reported["scoreable"], json!(true));
}

#[test]
fn undo_redo_reconstruct_the_applied_stroke_set() {
    let (w, h) = (24u32, 24u32);
    let first = vec![[2, 2], [20, 2]];
    let second = vec![[2, 8], [20, 8]];
    let third = vec![[2, 14], [20, 14]];
    // stroke, stroke, undo, stroke: the third stroke clears the redo stack,
    // so the second can never come back and exactly two strokes are applied.
    let document = replay(
        w,
        h,
        vec![
            stroke(0, 1, "foreground", 9, &first, 100, "accepted"),
            stroke(1, 2, "background", 9, &second, 200, "accepted"),
            event(2, Some(3), "undo", 300, "accepted"),
            stroke(3, 4, "foreground", 9, &third, 400, "accepted"),
        ],
    );
    assert_eq!(summary(&document).expect("summary")["active_strokes"], 2);
    assert_eq!(
        plane(&document, w, h).expect("plane"),
        oracle(w, h, &[(255, 9, first.clone()), (255, 9, third.clone())])
    );

    // A redo after that stroke has nothing to restore.
    let cleared = replay(
        w,
        h,
        vec![
            stroke(0, 1, "foreground", 9, &first, 100, "accepted"),
            stroke(1, 2, "background", 9, &second, 200, "accepted"),
            event(2, Some(3), "undo", 300, "accepted"),
            stroke(3, 4, "foreground", 9, &third, 400, "accepted"),
            event(4, Some(5), "redo", 500, "accepted"),
        ],
    );
    assert!(plane(&cleared, w, h)
        .expect_err("empty redo stack")
        .contains("redo"));

    // Undo on an empty active stack refuses.
    let empty = replay(w, h, vec![event(0, Some(1), "undo", 100, "accepted")]);
    assert!(plane(&empty, w, h)
        .expect_err("empty active stack")
        .contains("undo"));
    assert!(summary(&empty).is_err());

    // An undo that is later redone restores that exact stroke.
    let restored = replay(
        w,
        h,
        vec![
            stroke(0, 1, "foreground", 9, &first, 100, "accepted"),
            stroke(1, 2, "background", 9, &second, 200, "accepted"),
            event(2, Some(3), "undo", 300, "accepted"),
            event(3, Some(4), "redo", 400, "accepted"),
        ],
    );
    assert_eq!(summary(&restored).expect("summary")["active_strokes"], 2);
    assert_eq!(
        plane(&restored, w, h).expect("plane"),
        oracle(w, h, &[(255, 9, first), (0, 9, second)])
    );
}

#[test]
fn rejected_strokes_cost_a_gesture_and_are_not_applied() {
    let (w, h) = (24u32, 24u32);
    let applied = vec![[2, 2], [20, 2]];
    let refused = vec![[2, 20], [20, 20]];
    for (result, elapsed) in [
        ("rejected:rasterizer_limit", 1000u64),
        ("rejected:time_budget", 300_001),
    ] {
        let document = replay(
            w,
            h,
            vec![
                stroke(0, 1, "foreground", 9, &applied, 100, "accepted"),
                stroke(1, 2, "background", 81, &refused, elapsed, result),
            ],
        );
        let reported = summary(&document).expect("summary");
        assert_eq!(reported["modifying_gestures"], json!(2));
        assert_eq!(reported["active_strokes"], json!(1));
        assert_eq!(
            plane(&document, w, h).expect("plane"),
            oracle(w, h, &[(255, 9, applied.clone())])
        );
    }
}

#[test]
fn every_invalid_document_refuses_with_the_buffer_untouched() {
    let (w, h) = (24u32, 24u32);
    let points = vec![[2, 2], [20, 2]];
    let one = |value: Value| replay(w, h, vec![value]);
    let mut cases: Vec<Value> = vec![
        // A time-budget rejection exactly at the budget is not over it.
        one(stroke(
            0,
            1,
            "foreground",
            9,
            &points,
            300_000,
            "rejected:time_budget",
        )),
        // Only the two declared reasons exist.
        one(stroke(
            0,
            1,
            "foreground",
            9,
            &points,
            1000,
            "rejected:other",
        )),
        // An accepted gesture cannot land past the time budget.
        one(stroke(0, 1, "foreground", 9, &points, 300_001, "accepted")),
        // Diameters are the four offered sizes.
        one(stroke(0, 1, "foreground", 10, &points, 100, "accepted")),
        // Points are inside the canvas.
        one(stroke(
            0,
            1,
            "foreground",
            9,
            &[[2, 2], [24, 2]],
            100,
            "accepted",
        )),
        // At most 256 recorded samples.
        one(stroke(
            0,
            1,
            "foreground",
            9,
            &(0..257).map(|i| [i % 24, i / 24]).collect::<Vec<_>>(),
            100,
            "accepted",
        )),
        // A view event carries no brush geometry.
        one({
            let mut view = event(0, None, "view", 100, "accepted");
            view["diameter"] = json!(9);
            view
        }),
    ];

    // A 21st modifying gesture.
    let overrun: Vec<Value> = (0..21)
        .map(|index| {
            stroke(
                index,
                index + 1,
                "foreground",
                3,
                &points,
                100 + u64::from(index),
                "accepted",
            )
        })
        .collect();
    cases.push(replay(w, h, overrun));

    // A clock that runs backwards.
    cases.push(replay(
        w,
        h,
        vec![
            stroke(0, 1, "foreground", 9, &points, 500, "accepted"),
            event(1, None, "view", 400, "accepted"),
        ],
    ));
    // A terminal event that is not last.
    let mut early = replay(
        w,
        h,
        vec![
            event(0, None, "submit", 500, "accepted"),
            event(1, None, "view", 600, "accepted"),
        ],
    );
    early["outcome"] = json!("submitted");
    cases.push(early);
    // A terminal event the outcome disagrees with.
    let mut mismatch = replay(w, h, vec![event(0, None, "abandon", 500, "accepted")]);
    mismatch["outcome"] = json!("submitted");
    cases.push(mismatch);
    // budget_exhausted with neither budget spent.
    let mut unspent = replay(
        w,
        h,
        vec![event(0, None, "budget_exhausted", 500, "accepted")],
    );
    unspent["outcome"] = json!("budget_exhausted");
    cases.push(unspent);
    // An event whose origin is not the replay's.
    let mut mixed = replay(w, h, vec![event(0, None, "view", 500, "accepted")]);
    mixed["events"][0]["origin"] = json!("synthetic");
    cases.push(mixed);
    // A human session with no attestation.
    let mut unattested = replay(w, h, vec![]);
    unattested["blinding_attestation"] = Value::Null;
    cases.push(unattested);
    // A synthetic session carrying an annotator id.
    let mut named = replay(w, h, vec![]);
    named["origin"] = json!("synthetic");
    named["human_origin"] = json!(false);
    named["scoreable"] = json!(false);
    cases.push(named);
    // An unknown field anywhere in the document.
    let mut extra = replay(w, h, vec![]);
    extra["notes"] = json!("unexpected");
    cases.push(extra);
    // A declared budget that is not this protocol's.
    let mut rebudgeted = replay(w, h, vec![]);
    rebudgeted["budget"]["seconds"] = json!(600);
    cases.push(rebudgeted);
    // A counted-gesture total that disagrees with the events.
    let mut miscounted = replay(w, h, vec![]);
    miscounted["modifying_gestures"] = json!(3);
    cases.push(miscounted);

    for document in cases {
        assert!(
            plane(&document, w, h).is_err(),
            "accepted an invalid document: {document}"
        );
        assert!(summary(&document).is_err());
    }

    // A payload past the 4 MiB limit, refused before anything is parsed.
    let mut huge = replay(w, h, vec![]);
    huge["session_id"] = json!("s".repeat(4 * 1024 * 1024 + 16));
    let text = huge.to_string();
    assert!(text.len() > 4 * 1024 * 1024);
    assert!(plane_text(&text, w, h)
        .expect_err("oversized")
        .contains("4 MiB"));

    // Dimensions that are not the replay's own.
    let valid = replay(w, h, vec![]);
    assert!(plane(&valid, w, h + 1).is_err());
}

#[test]
fn a_plane_past_the_rasterizer_cap_refuses_while_the_summary_still_reports() {
    let (w, h) = (4096u32, 4096u32);
    // Four diagonals, each of whose clipped segment box is the whole canvas:
    // 4 x 16.8 M segment-box visits is past the rasterizer's 64 M cap.
    let corners: [[[i64; 2]; 2]; 4] = [
        [[0, 0], [4095, 4095]],
        [[4095, 0], [0, 4095]],
        [[1, 0], [4094, 4095]],
        [[4094, 0], [1, 4095]],
    ];
    let events = corners
        .iter()
        .enumerate()
        .map(|(index, points)| {
            let index = index as u32;
            stroke(
                index,
                index + 1,
                "foreground",
                81,
                points,
                100 + u64::from(index),
                "accepted",
            )
        })
        .collect();
    let document = replay(w, h, events);
    let message = plane(&document, w, h).expect_err("four full-canvas capsules");
    assert!(
        message.starts_with("rasterizer_limit: "),
        "unexpected message: {message}"
    );
    let reported = summary(&document).expect("summary is independent of rasterization");
    assert_eq!(reported["active_strokes"], json!(4));
    assert_eq!(reported["modifying_gestures"], json!(4));
    assert_eq!(reported["final"], json!(false));
}

#[test]
fn a_finished_synthetic_session_reports_its_outcome() {
    let (w, h) = (16u32, 16u32);
    let mut document = replay(
        w,
        h,
        vec![
            stroke(0, 1, "unknown", 27, &[[4, 4], [12, 12]], 100, "accepted"),
            event(1, None, "submit", 200, "accepted"),
        ],
    );
    document["origin"] = json!("synthetic");
    document["human_origin"] = json!(false);
    document["scoreable"] = json!(false);
    document["annotator_id"] = Value::Null;
    document["blinding_attestation"] = Value::Null;
    document["outcome"] = json!("submitted");
    document["ended_at_utc"] = json!("2026-09-15T00:05:00Z");
    document["label_plane"] = json!({"encoding":"trimap_u8_0_128_255","sha256":digest('c')});
    document["write_planes"] = json!({"alpha_write":"constant_65535","rgb_write":"constant_65535",
        "protection":"constant_0","alpha_write_sha256":digest('d'),
        "rgb_write_sha256":digest('e'),"protection_sha256":digest('f')});
    for value in document["events"].as_array_mut().expect("events") {
        value["origin"] = json!("synthetic");
    }
    let reported = summary(&document).expect("summary");
    assert_eq!(reported["final"], json!(true));
    assert_eq!(reported["outcome"], json!("submitted"));
    assert_eq!(reported["human_origin"], json!(false));
    assert_eq!(reported["scoreable"], json!(false));
    assert_eq!(reported["elapsed_ms"], json!(1000));
    let produced = plane(&document, w, h).expect("plane");
    assert!(produced.iter().all(|byte| *byte == 128));
}
