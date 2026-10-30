//! The FLOAT's own rect and the selection-bounded plan, black-box
//! through the FFI at 8 and 16 bits: `dr_doc_selected_pixels_bounds` /
//! `dr_precision_doc_selected_pixels_bounds`, and the optional `bounds` key
//! of the `move_selected` / `transform_selected` request.
//!
//! The float-rect oracle is written here by brute force over the canvas: the
//! bbox of every on-canvas pixel whose stored alpha is above zero and whose
//! coverage is above zero. The plan oracle is the spec's arithmetic, stated
//! independently of the kernel.

use crate::common;

use std::ptr;

use common::selected_pixels::*;
use doctored_core::ffi_selected_pixels::{
    dr_doc_selected_pixels_bounds, dr_precision_doc_selected_pixels_bounds,
};
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use serde_json::{json, Value};

const OUTSIDE: &str = "selected pixels coverage lies outside the request's bounds";
const BAD_BOUNDS: &str = "selected pixels bounds must be [x, y, w, h] with w and h at least 1";

/// A float-rect fixture: name, canvas, layer plane, coverage.
type Case = (&'static str, (u32, u32), Plane, Vec<u16>);

/// The brute-force F: canvas bbox of alpha > 0 AND coverage > 0.
fn brute_float(canvas: (u32, u32), plane: &Plane, coverage: &[u16]) -> Option<[u32; 4]> {
    let mut found: Option<(u32, u32, u32, u32)> = None;
    for y in 0..canvas.1 {
        for x in 0..canvas.0 {
            let c = coverage[(y * canvas.0 + x) as usize];
            let a = plane.at(i64::from(x), i64::from(y)).map_or(0, |p| p[3]);
            if c > 0 && a > 0 {
                found = Some(match found {
                    None => (x, y, x, y),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                });
            }
        }
    }
    found.map(|(x0, y0, x1, y1)| [x0, y0, x1 - x0 + 1, y1 - y0 + 1])
}

fn scratch(plan: &Value) -> u64 {
    plan["scratch_bytes"].as_u64().unwrap()
}

fn with_bounds(request: Value, bounds: [i64; 4]) -> String {
    let mut request = request;
    request["bounds"] = json!(bounds);
    request.to_string()
}

fn moved(dx: i32, dy: i32) -> Value {
    json!({"op":"move_selected","layer":0,"dx":dx,"dy":dy})
}

fn affine(m: [f64; 6], filter: &str) -> Value {
    json!({"op":"transform_selected","layer":0,"matrix":m,"filter":filter})
}

fn quad(rect: [f64; 4], q: [f64; 8]) -> Value {
    json!({"op":"transform_selected","layer":0,"source_rect":rect,"quad":q,"filter":"bilinear"})
}

// ------------------------------------------------------------- float rect --

#[test]
fn the_float_rect_matches_the_brute_force_bbox() {
    let margin = |x: u32, y: u32| {
        let mut p = pattern(x, y);
        p[3] = if (4..10).contains(&x) && (3..8).contains(&y) {
            255
        } else {
            0
        };
        p
    };
    let soft = |x: u32, y: u32| {
        let mut p = pattern(x, y);
        p[3] = ((x * 37 + y * 13) % 5 * 60) as u8;
        p
    };
    let feather = |x: u32, y: u32| -> u16 {
        let (dx, dy) = (f64::from(x) - 6.4, f64::from(y) - 5.2);
        let r = (dx * dx + dy * dy).sqrt();
        ((1.0 - ((r - 1.5) / 3.5).clamp(0.0, 1.0)) * 65535.0).round() as u16
    };
    for depth in DEPTHS {
        let cases: Vec<Case> = vec![
            (
                "transparent margin",
                (16, 12),
                Plane::of(depth, (0, 0), (16, 12), margin),
                coverage((16, 12), rect(1, 1, 14, 10)),
            ),
            (
                "soft edge",
                (14, 12),
                Plane::of(depth, (0, 0), (14, 12), soft),
                coverage((14, 12), feather),
            ),
            (
                "offset, partly off-canvas",
                (12, 9),
                Plane::of(depth, (-3, 2), (10, 12), soft),
                coverage((12, 9), feather),
            ),
            (
                "offset inside",
                (20, 16),
                Plane::of(depth, (5, 4), (6, 5), margin),
                coverage((20, 16), rect(0, 0, 20, 16)),
            ),
        ];
        for (what, canvas, plane, cov) in cases {
            let want = brute_float(canvas, &plane, &cov);
            assert!(want.is_some(), "{what}: the fixture selects something");
            let got = doc(depth, canvas, &plane).float_bounds(0, &cov).unwrap();
            assert_eq!(got, want, "{what} {depth:?}");
        }
    }
}

#[test]
fn an_empty_float_answers_false_with_no_error() {
    let canvas = (10, 8);
    let half = |x: u32, y: u32| {
        let mut p = pattern(x, y);
        p[3] = if x < 5 { 255 } else { 0 };
        p
    };
    for depth in DEPTHS {
        let base = doc(depth, canvas, &Plane::of(depth, (0, 0), canvas, half));
        // Over transparent pixels, nothing at all, and off the layer.
        assert_eq!(
            base.float_bounds(0, &coverage(canvas, rect(6, 1, 3, 3))),
            Ok(None)
        );
        assert_eq!(base.float_bounds(0, &[0u16; 80]), Ok(None));
        let small = doc(depth, canvas, &Plane::of(depth, (0, 0), (3, 3), pattern));
        assert_eq!(
            small.float_bounds(0, &coverage(canvas, rect(5, 4, 3, 3))),
            Ok(None)
        );
    }
}

#[test]
fn the_query_refuses_what_the_move_refuses() {
    let canvas = (8, 6);
    let cov = coverage(canvas, rect(1, 1, 3, 3));
    let plane8 = Plane::of(Depth::D8, (0, 0), canvas, pattern);
    let d8 = byte_doc(canvas, &plane8);
    assert!(bounds8(&d8, 0, &cov[1..], None)
        .unwrap_err()
        .contains("canvas-sized"));
    assert!(bounds8(&d8, 3, &cov, None)
        .unwrap_err()
        .contains("out of range"));
    let (grouped, group, _, _) = d8.group_layers(&[0], "G").unwrap();
    assert!(bounds8(&grouped, group, &cov, None)
        .unwrap_err()
        .contains("is a group"));
    let mut adjusted = byte_doc(canvas, &plane8)
        .adding_image_layer(0, image::RgbaImage::new(8, 6), "Fx")
        .unwrap();
    adjusted.layers[1].meta = Some(adjustment_meta());
    assert!(bounds8(&adjusted, 1, &cov, None)
        .unwrap_err()
        .contains("adjustment layer"));

    let plane16 = Plane::of(Depth::D16, (0, 0), canvas, pattern);
    let d16 = typed_doc(canvas, vec![typed_layer(&plane16)]);
    assert!(bounds16(&d16, 0, &cov[1..], None)
        .unwrap_err()
        .contains("canvas-sized"));
    assert!(bounds16(&d16, 3, &cov, None)
        .unwrap_err()
        .contains("out of range"));
    let group =
        PrecisionDocumentLayer::isolated_normal("G", vec![typed_layer(&plane16)], 1.0, true)
            .unwrap();
    let grouped = typed_doc(canvas, vec![group]);
    assert!(bounds16(&grouped, 1, &cov, None)
        .unwrap_err()
        .contains("is a group"));
    let adjusted = typed_doc(canvas, vec![typed_layer(&plane16)])
        .add_adjustment("Fx", &adjustment_meta(), 1.0, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert!(bounds16(&adjusted, 1, &cov, None)
        .unwrap_err()
        .contains("adjustment layer"));

    let path = "tests/fixtures/float_11f/scene.tiff";
    if std::path::Path::new(path).exists() {
        let float = PrecisionDocument::open_preserving(path, LIMIT, 0, None).unwrap();
        let (w, h) = float.dimensions();
        let full = vec![65535u16; (w * h) as usize];
        // A float layer answers its float's rect.
        assert!(bounds16(&float, 0, &full, None).unwrap().is_some());
    }
}

#[test]
fn a_cancelled_control_answers_operation_cancelled() {
    let canvas = (8, 6);
    let cov = coverage(canvas, rect(1, 1, 3, 3));
    let control = DrJobControl::default();
    control.cancel();
    let d8 = byte_doc(canvas, &Plane::of(Depth::D8, (0, 0), canvas, pattern));
    assert_eq!(
        bounds8(&d8, 0, &cov, Some(&control)).unwrap_err(),
        "operation cancelled"
    );
    let control = DrJobControl::default();
    control.cancel();
    let d16 = typed_doc(
        canvas,
        vec![typed_layer(&Plane::of(Depth::D16, (0, 0), canvas, pattern))],
    );
    assert_eq!(
        bounds16(&d16, 0, &cov, Some(&control)).unwrap_err(),
        "operation cancelled"
    );
}

#[test]
fn both_bounds_exports_tolerate_null() {
    let canvas = (4, 4);
    let cov = coverage(canvas, rect(1, 1, 2, 2));
    let d8 = byte_doc(canvas, &Plane::of(Depth::D8, (0, 0), canvas, pattern));
    let d16 = typed_doc(
        canvas,
        vec![typed_layer(&Plane::of(Depth::D16, (0, 0), canvas, pattern))],
    );
    unsafe {
        let mut out = [7u32; 4];
        // NULL document, with and without err_out.
        assert!(!dr_doc_selected_pixels_bounds(
            ptr::null(),
            0,
            cov.as_ptr(),
            16,
            ptr::null(),
            out.as_mut_ptr(),
            ptr::null_mut()
        ));
        let mut err = ptr::null_mut();
        assert!(!dr_doc_selected_pixels_bounds(
            ptr::null(),
            0,
            cov.as_ptr(),
            16,
            ptr::null(),
            out.as_mut_ptr(),
            &mut err
        ));
        assert!(take_err(err).is_some());
        let mut err = ptr::null_mut();
        assert!(!dr_precision_doc_selected_pixels_bounds(
            ptr::null(),
            0,
            cov.as_ptr(),
            16,
            ptr::null(),
            out.as_mut_ptr(),
            &mut err
        ));
        assert!(take_err(err).is_some());
        // NULL coverage.
        let mut err = ptr::null_mut();
        assert!(!dr_doc_selected_pixels_bounds(
            &d8,
            0,
            ptr::null(),
            16,
            ptr::null(),
            out.as_mut_ptr(),
            &mut err
        ));
        assert!(take_err(err).is_some());
        let mut err = ptr::null_mut();
        assert!(!dr_precision_doc_selected_pixels_bounds(
            &d16,
            0,
            ptr::null(),
            16,
            ptr::null(),
            out.as_mut_ptr(),
            &mut err
        ));
        assert!(take_err(err).is_some());
        assert_eq!(out, [7; 4], "a refusal writes nothing");
        // NULL out_rect: the answer is still reported, nothing is written.
        assert!(dr_doc_selected_pixels_bounds(
            &d8,
            0,
            cov.as_ptr(),
            16,
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut()
        ));
        assert!(dr_precision_doc_selected_pixels_bounds(
            &d16,
            0,
            cov.as_ptr(),
            16,
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut()
        ));
    }
}

// ----------------------------------------------------------- bounded plan --

#[test]
fn the_bounded_plan_charges_the_selection_rect() {
    let canvas = (12, 9);
    let text = with_bounds(moved(1, 0), [2, 2, 3, 3]);
    let d8 = byte_doc(canvas, &Plane::of(Depth::D8, (0, 0), canvas, pattern));
    // 16·|B| + 4·|bbox(B ∪ (B + d))| + 4 MiB = 144 + 48 + 4194304.
    assert_eq!(scratch(&plan8(&d8, &text).unwrap()), 4_194_496);
    let d16 = typed_doc(
        canvas,
        vec![typed_layer(&Plane::of(Depth::D16, (0, 0), canvas, pattern))],
    );
    // 144 + 8·12 + 4194304.
    assert_eq!(scratch(&plan16(&d16, &text).unwrap()), 4_194_544);
    // Bounds that miss the layer charge the bookkeeping alone.
    let small = byte_doc(canvas, &Plane::of(Depth::D8, (0, 0), (3, 3), pattern));
    let text = with_bounds(moved(1, 0), [6, 5, 3, 3]);
    assert_eq!(scratch(&plan8(&small, &text).unwrap()), 4 * 1024 * 1024);
}

#[test]
fn the_bounded_plan_never_exceeds_the_unbounded_one() {
    let canvas = (24, 18);
    let requests: Vec<Value> = vec![
        moved(1, 0),
        moved(-5, 3),
        moved(30, 2),
        moved(-2, -40),
        affine([1.0, 0.0, 0.0, 1.0, 3.0, -2.0], "bicubic"),
        affine([0.0, 1.0, -1.0, 0.0, 20.0, 1.0], "nearest"),
        affine([0.8660254, 0.5, -0.5, 0.8660254, 4.0, -3.0], "bilinear"),
        affine([1.7, 0.0, 0.0, 0.6, -6.0, 5.0], "lanczos3"),
        affine([0.5, 0.2, 0.1, 0.5, 30.0, 30.0], "bicubic"),
        quad(
            [4.0, 3.0, 8.0, 6.0],
            [2.0, 2.0, 15.0, 4.0, 14.0, 12.0, 3.0, 10.0],
        ),
    ];
    let bounds: [[i64; 4]; 5] = [
        [4, 3, 8, 6],
        [0, 0, 24, 18],
        [-10, -10, 60, 60],
        [20, 14, 9, 9],
        [1, 1, 1, 1],
    ];
    for depth in DEPTHS {
        for (offset, size) in [((0, 0), (24, 18)), ((-4, 3), (14, 20)), ((6, 5), (7, 6))] {
            let base = doc(depth, canvas, &Plane::of(depth, offset, size, pattern));
            for request in &requests {
                let unbounded = scratch(&base.plan(&request.to_string()).unwrap());
                for b in bounds {
                    let bounded = scratch(&base.plan(&with_bounds(request.clone(), b)).unwrap());
                    assert!(
                        bounded <= unbounded,
                        "{depth:?} layer {offset:?}/{size:?} {request} bounds {b:?}: \
                         {bounded} > {unbounded}"
                    );
                }
            }
        }
    }
}

// ----------------------------------------------------------------- bounds --

#[test]
fn bounds_change_no_pixel_and_refuse_coverage_past_them() {
    let canvas = (18, 14);
    let feather = |x: u32, y: u32| -> u16 {
        let (dx, dy) = (f64::from(x) - 7.3, f64::from(y) - 6.1);
        let r = (dx * dx + dy * dy).sqrt();
        ((1.0 - ((r - 2.0) / 3.0).clamp(0.0, 1.0)) * 65535.0).round() as u16
    };
    let alpha = |x: u32, y: u32| -> [u8; 4] {
        let mut p = pattern(x, y);
        p[3] = ((x * 23 + y * 31) % 200 + 40) as u8;
        p
    };
    let requests = [
        moved(3, 2),
        moved(-12, 1),
        json!({"op":"move_selected","layer":0,"dx":4,"dy":-2,"duplicate":true}),
        affine([0.0, 1.0, -1.0, 0.0, 20.0, 0.0], "nearest"),
        affine([0.9, 0.3, -0.3, 0.9, 2.0, 1.0], "bicubic"),
        quad(
            [2.0, 1.0, 11.0, 11.0],
            [1.0, 0.0, 14.0, 2.0, 13.0, 13.0, 2.0, 12.0],
        ),
    ];
    for depth in DEPTHS {
        let plane = Plane::of(depth, (0, 0), canvas, alpha);
        let cov = coverage(canvas, feather);
        // The selection's integral rect, and a looser one.
        let tight = brute_float(canvas, &Plane::of(depth, (0, 0), canvas, pattern), &cov).unwrap();
        let tight = tight.map(i64::from);
        let base = doc(depth, canvas, &plane);
        for request in &requests {
            let plain = base
                .edit(&request.to_string(), &cov)
                .unwrap()
                .unwrap()
                .plane(0);
            for b in [tight, [0, 0, 18, 14], [-5, -5, 40, 40]] {
                let bounded = base
                    .edit(&with_bounds(request.clone(), b), &cov)
                    .unwrap()
                    .unwrap()
                    .plane(0);
                assert_eq!(plain, bounded, "{depth:?} {request} bounds {b:?}");
            }
            let short = [tight[0] + 1, tight[1], tight[2] - 1, tight[3]];
            assert_eq!(
                base.edit(&with_bounds(request.clone(), short), &cov)
                    .err()
                    .as_deref(),
                Some(OUTSIDE),
                "{depth:?} {request}"
            );
        }
    }
}

#[test]
fn malformed_bounds_refuse() {
    let canvas = (8, 6);
    let cov = coverage(canvas, rect(1, 1, 3, 3));
    for depth in DEPTHS {
        let base = doc(depth, canvas, &Plane::of(depth, (0, 0), canvas, pattern));
        for b in [
            [0, 0, 0, 3],
            [0, 0, 3, -1],
            [1_i64 << 32, 0, 3, 3],
            [0, 0, 3, 1_i64 << 31],
        ] {
            let text = with_bounds(moved(1, 0), b);
            assert_eq!(
                base.edit(&text, &cov).err().as_deref(),
                Some(BAD_BOUNDS),
                "{b:?}"
            );
            assert_eq!(base.plan(&text).err().as_deref(), Some(BAD_BOUNDS), "{b:?}");
        }
        for bad in [json!([0, 0, 3]), json!([0.5, 0, 3, 3]), json!("0,0,3,3")] {
            let mut text = moved(1, 0);
            text["bounds"] = bad.clone();
            let text = text.to_string();
            assert_eq!(
                base.edit(&text, &cov).err().as_deref(),
                Some(BAD_BOUNDS),
                "{depth:?} {bad}"
            );
            assert_eq!(
                base.plan(&text).err().as_deref(),
                Some(BAD_BOUNDS),
                "{depth:?} {bad}"
            );
        }
        // An explicit null is an absent `bounds`.
        let mut null = moved(1, 0);
        null["bounds"] = Value::Null;
        let null = null.to_string();
        let absent = moved(1, 0).to_string();
        let want = base.edit(&absent, &cov).unwrap().unwrap().plane(0);
        let got = base.edit(&null, &cov).unwrap().unwrap().plane(0);
        assert_eq!(got, want, "{depth:?} null bounds");
        assert_eq!(base.plan(&null), base.plan(&absent), "{depth:?} null plan");
    }
}

#[test]
fn duplicate_keys_refuse() {
    let canvas = (8, 6);
    let cov = coverage(canvas, rect(1, 1, 3, 3));
    let requests = [
        r#"{"op":"move_selected","layer":0,"dx":1,"dx":500,"dy":0}"#,
        r#"{"op":"transform_selected","layer":0,"matrix":[1,0,0,1,2,0],"filter":"nearest","filter":"bilinear"}"#,
    ];
    for depth in DEPTHS {
        let base = doc(depth, canvas, &Plane::of(depth, (0, 0), canvas, pattern));
        for text in requests {
            let err = base.edit(text, &cov).err().unwrap_or_default();
            assert!(
                err.contains("duplicate field"),
                "{depth:?} edit {text}: {err}"
            );
            let err = base.plan(text).err().unwrap_or_default();
            assert!(
                err.contains("duplicate field"),
                "{depth:?} plan {text}: {err}"
            );
        }
    }
}
