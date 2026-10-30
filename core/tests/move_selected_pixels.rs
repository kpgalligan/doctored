//! Move Selected Pixels, black-box through both FFI seams: the byte
//! `dr_doc_selected_pixels_edit` / `_plan` and the typed
//! `dr_precision_doc_edit` / `_plan` with `{"op":"move_selected",…}`.
//!
//! Every numeric case runs at 8 AND 16 bits against an independent f64
//! oracle written here from the measured Photoshop contract, not from the
//! kernel: the hole is `a·(1−c)` with RGB kept, the landing is the W3C
//! source-over `co = cs·sa + cb·ab·(1−sa)`, `ao = sa + ab·(1−sa)` of the
//! ORIGINAL float `(rgb, a·c)`, the extent is `layer ∪ ((F + d) ∩ canvas)`,
//! and a pixel the move does not touch keeps its stored bits. The oracle
//! accepts both roundings only within 0.05 of a code tie, where f32 and f64
//! may legitimately disagree; everywhere else the match is exact.

use crate::common;

use std::ffi::CString;
use std::ptr;
use std::sync::Arc;

use common::selected_pixels::*;
use doctored_core::doc_lock::{LOCK_ALL, LOCK_PIXELS, LOCK_POSITION, LOCK_TRANSPARENCY};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi_selected_pixels::{
    dr_doc_selected_pixels_edit, dr_doc_selected_pixels_edit_plan,
};
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_capability::capability;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use image::{GrayImage, Luma, RgbaImage};
use serde_json::json;

const EMPTY: &str = "The selection holds no pixels to move.";

fn request(layer: usize, dx: i32, dy: i32) -> String {
    json!({"op":"move_selected","layer":layer,"dx":dx,"dy":dy}).to_string()
}

fn duplicate(layer: usize, dx: i32, dy: i32) -> String {
    json!({"op":"move_selected","layer":layer,"dx":dx,"dy":dy,"duplicate":true}).to_string()
}

impl Doc {
    fn moved(&self, layer: usize, d: (i32, i32), coverage: &[u16]) -> Doc {
        self.edit(&request(layer, d.0, d.1), coverage)
            .expect("the move runs")
            .expect("the move changes something")
    }
}

// ----------------------------------------------------------------- oracle --

/// The expected layer: extent, then every pixel. `None` stored bits keep.
/// `hole` false is the ⌥-drag duplicate: the original keeps its
/// alpha, so the hole's coverage is 0 and only the landing composites.
fn oracle(
    depth: Depth,
    canvas: (u32, u32),
    plane: &Plane,
    coverage: &[u16],
    d: (i64, i64),
    hole: bool,
) -> (Plane, Vec<Option<[f64; 4]>>) {
    let max = depth.max();
    let on =
        |x: i64, y: i64| x >= 0 && y >= 0 && x < i64::from(canvas.0) && y < i64::from(canvas.1);
    let cov = |x: i64, y: i64| {
        if on(x, y) {
            f64::from(coverage[y as usize * canvas.0 as usize + x as usize]) / 65535.0
        } else {
            0.0
        }
    };
    let unit = |p: [u16; 4]| p.map(|v| f64::from(v) / max);
    // F: bbox of on-canvas layer pixels with a·c > 0.
    let (lx, ly) = (i64::from(plane.offset.0), i64::from(plane.offset.1));
    let (lw, lh) = (i64::from(plane.size.0), i64::from(plane.size.1));
    let mut f: Option<(i64, i64, i64, i64)> = None;
    for y in ly..ly + lh {
        for x in lx..lx + lw {
            let a = plane.at(x, y).unwrap()[3];
            if on(x, y) && a > 0 && cov(x, y) > 0.0 {
                f = Some(match f {
                    None => (x, y, x + 1, y + 1),
                    Some((a0, b0, a1, b1)) => (a0.min(x), b0.min(y), a1.max(x + 1), b1.max(y + 1)),
                });
            }
        }
    }
    let f = f.expect("the fixture selects something");
    let landing = (
        (f.0 + d.0).max(0),
        (f.1 + d.1).max(0),
        (f.2 + d.0).min(i64::from(canvas.0)),
        (f.3 + d.1).min(i64::from(canvas.1)),
    );
    let mut extent = (lx, ly, lx + lw, ly + lh);
    if landing.0 < landing.2 && landing.1 < landing.3 {
        extent = (
            extent.0.min(landing.0),
            extent.1.min(landing.1),
            extent.2.max(landing.2),
            extent.3.max(landing.3),
        );
    }
    let mut stored = Vec::new();
    let mut expected = Vec::new();
    for y in extent.1..extent.3 {
        for x in extent.0..extent.2 {
            let original = plane.at(x, y).unwrap_or_default();
            let (qx, qy) = (x - d.0, y - d.1);
            let float = (on(x, y) && qx >= f.0 && qx < f.2 && qy >= f.1 && qy < f.3)
                .then(|| (unit(plane.at(qx, qy).unwrap()), cov(qx, qy)));
            stored.push(original);
            let c = if hole { cov(x, y) } else { 0.0 };
            expected.push(oracle_pixel(unit(original), c, float));
        }
    }
    (
        Plane {
            offset: (extent.0 as i32, extent.1 as i32),
            size: ((extent.2 - extent.0) as u32, (extent.3 - extent.1) as u32),
            px: stored,
        },
        expected,
    )
}

fn assert_matches(
    depth: Depth,
    got: &Plane,
    expected: &(Plane, Vec<Option<[f64; 4]>>),
    what: &str,
) {
    let (stored, values) = expected;
    assert_eq!(got.offset, stored.offset, "{what} {depth:?}: offset");
    assert_eq!(got.size, stored.size, "{what} {depth:?}: size");
    let max = depth.max();
    for (i, (value, got)) in values.iter().zip(&got.px).enumerate() {
        let Some(value) = value else {
            assert_eq!(*got, stored.px[i], "{what} {depth:?}: untouched pixel {i}");
            continue;
        };
        for k in 0..4 {
            let x = value[k].clamp(0.0, 1.0) * max;
            let ok = if (x - x.floor() - 0.5).abs() < 0.05 {
                got[k] == x.floor() as u16 || got[k] == x.ceil() as u16
            } else {
                got[k] == x.round() as u16
            };
            assert!(
                ok,
                "{what} {depth:?}: pixel {i} component {k}: got {} want {x}",
                got[k]
            );
        }
    }
}

// ------------------------------------------------------------------ cases --

#[test]
fn a_hard_rect_moves_exactly_and_leaves_a_transparent_hole() {
    let canvas = (16, 12);
    for depth in DEPTHS {
        let plane = Plane::of(depth, (0, 0), canvas, pattern);
        let cov = coverage(canvas, rect(3, 2, 5, 4));
        let out = doc(depth, canvas, &plane).moved(0, (4, 3), &cov).plane(0);
        assert_eq!((out.offset, out.size), ((0, 0), canvas));
        for y in 0..12i64 {
            for x in 0..16i64 {
                let got = out.at(x, y).unwrap();
                let landed = (7..12).contains(&x) && (5..9).contains(&y);
                let hole = (3..8).contains(&x) && (2..6).contains(&y);
                if landed {
                    assert_eq!(
                        got,
                        plane.at(x - 4, y - 3).unwrap(),
                        "{depth:?} landed {x},{y}"
                    );
                } else if hole {
                    let before = plane.at(x, y).unwrap();
                    assert_eq!(got[3], 0, "{depth:?} hole alpha {x},{y}");
                    assert_eq!(got[..3], before[..3], "{depth:?} hole RGB kept {x},{y}");
                } else {
                    assert_eq!(got, plane.at(x, y).unwrap(), "{depth:?} untouched {x},{y}");
                }
            }
        }
    }
}

#[test]
fn a_binary_disc_hole_keeps_its_original_rgb() {
    let canvas = (20, 20);
    let disc = |x: u32, y: u32| {
        let (dx, dy) = (f64::from(x) - 9.5, f64::from(y) - 9.5);
        if dx * dx + dy * dy <= 36.0 {
            65535
        } else {
            0
        }
    };
    for depth in DEPTHS {
        let plane = Plane::of(depth, (0, 0), canvas, pattern);
        let cov = coverage(canvas, disc);
        let out = doc(depth, canvas, &plane).moved(0, (1, 0), &cov).plane(0);
        let mut holes = 0;
        for y in 0..20u32 {
            for x in 1..20u32 {
                let here = cov[(y * 20 + x) as usize];
                let from = cov[(y * 20 + x - 1) as usize];
                if here == 65535 && from == 0 {
                    holes += 1;
                    let got = out.at(i64::from(x), i64::from(y)).unwrap();
                    let before = plane.at(i64::from(x), i64::from(y)).unwrap();
                    assert_eq!(got[3], 0, "{depth:?} {x},{y}");
                    assert_eq!(got[..3], before[..3], "{depth:?} {x},{y}: RGB must survive");
                }
            }
            // Column 0 lies outside the disc and is untouched.
            let got = out.at(0, i64::from(y)).unwrap();
            assert_eq!(got, plane.at(0, i64::from(y)).unwrap());
        }
        assert!(holes > 5, "{depth:?}: the disc has a left rim");
    }
}

#[test]
fn a_feathered_move_matches_the_hole_and_source_over_oracle() {
    let canvas = (18, 14);
    let feather = |x: u32, y: u32| -> u16 {
        let (dx, dy) = (f64::from(x) - 7.3, f64::from(y) - 6.1);
        let r = (dx * dx + dy * dy).sqrt();
        ((1.0 - ((r - 2.0) / 4.0).clamp(0.0, 1.0)) * 65535.0).round() as u16
    };
    let alpha = |x: u32, y: u32| -> [u8; 4] {
        let mut p = pattern(x, y);
        p[3] = ((x * 23 + y * 31) % 200 + 40) as u8;
        p
    };
    for depth in DEPTHS {
        let plane = Plane::of(depth, (0, 0), canvas, alpha);
        let cov = coverage(canvas, feather);
        for d in [(3, 2), (-2, 1), (1, -3)] {
            let out = doc(depth, canvas, &plane).moved(0, d, &cov).plane(0);
            let want = oracle(
                depth,
                canvas,
                &plane,
                &cov,
                (i64::from(d.0), i64::from(d.1)),
                true,
            );
            assert_matches(depth, &out, &want, &format!("feathered {d:?}"));
        }
    }
}

#[test]
fn the_landing_is_clipped_to_the_canvas_and_off_canvas_pixels_keep_their_bits() {
    let canvas = (16, 10);
    for depth in DEPTHS {
        // The layer already extends 8 px past the right edge.
        let plane = Plane::of(depth, (8, 0), (16, 10), pattern);
        let cov = coverage(canvas, rect(12, 2, 4, 5));
        let out = doc(depth, canvas, &plane).moved(0, (3, 1), &cov).plane(0);
        let want = oracle(depth, canvas, &plane, &cov, (3, 1), true);
        assert_matches(depth, &out, &want, "clip");
        assert_eq!((out.offset, out.size), ((8, 0), (16, 10)));
        for y in 0..10 {
            for x in 16..24 {
                assert_eq!(out.at(x, y), plane.at(x, y), "{depth:?} off-canvas {x},{y}");
            }
        }
    }
}

#[test]
fn a_landing_outside_the_layer_grows_it_and_keeps_originals_in_place() {
    let canvas = (20, 16);
    for depth in DEPTHS {
        let plane = Plane::of(depth, (2, 3), (6, 4), pattern);
        let cov = coverage(canvas, rect(3, 3, 3, 3));
        let out = doc(depth, canvas, &plane).moved(0, (9, 7), &cov).plane(0);
        let want = oracle(depth, canvas, &plane, &cov, (9, 7), true);
        assert_matches(depth, &out, &want, "grow");
        assert_eq!(out.offset, (2, 3));
        assert_eq!(out.size, (13, 10));
        for (x, y) in [(2, 3), (7, 6), (6, 6)] {
            assert_eq!(
                out.at(x, y),
                plane.at(x, y),
                "{depth:?} original at {x},{y}"
            );
        }
    }
}

#[test]
fn the_layer_mask_stays_put() {
    let canvas = (20, 16);
    let values: Vec<u8> = (0..60).map(|i| (i * 7 + 3) as u8).collect();
    let props = MaskProperties::at((1, 2));
    let cov = coverage(canvas, rect(3, 3, 3, 3));
    // 8 bits.
    let plane = Plane::of(Depth::D8, (2, 3), (6, 4), pattern);
    let mut base = byte_doc(canvas, &plane);
    let mask = Arc::new(GrayImage::from_fn(10, 6, |x, y| {
        Luma([values[(y * 10 + x) as usize]])
    }));
    base.layers[0].mask = Some(mask.clone());
    base.layers[0].mask_properties = props.clone();
    base.layers[0].mask_enabled = false;
    let out = edit8(&base, &request(0, 9, 7), &cov, None)
        .unwrap()
        .unwrap();
    let layer = &out.layers[0];
    assert_eq!(layer.mask.as_deref().unwrap().as_raw(), mask.as_raw());
    assert!(layer.mask_properties == props);
    assert!(!layer.mask_enabled);
    assert_eq!(layer.offset, (2, 3));
    // 16 bits.
    let plane = Plane::of(Depth::D16, (2, 3), (6, 4), pattern);
    let samples: Vec<u16> = values.iter().map(|v| u16::from(*v) * 257 + 1).collect();
    let typed_mask = PrecisionMask::new(
        PrecisionPlane::from_u16(10, 6, samples.clone(), None).unwrap(),
        props.clone(),
        false,
    )
    .unwrap();
    let base = typed_doc(
        canvas,
        vec![typed_layer(&plane).with_mask(Some(typed_mask))],
    );
    let out = edit16(&base, &request(0, 9, 7), &cov, None)
        .unwrap()
        .unwrap();
    let mask = out.layer(0).unwrap().mask().unwrap();
    assert_eq!(mask.plane().samples(), &samples[..]);
    assert!(mask.properties() == &props);
    assert!(!mask.enabled());
    assert_eq!(out.layer(0).unwrap().offset(), (2, 3));
}

#[test]
fn a_round_trip_restores_alpha_everywhere_and_rgb_where_visible() {
    let canvas = (16, 16);
    let square = |x: u32, y: u32| {
        let mut p = pattern(x, y);
        p[3] = if (4..9).contains(&x) && (5..10).contains(&y) {
            255
        } else {
            0
        };
        p
    };
    for depth in DEPTHS {
        let plane = Plane::of(depth, (0, 0), canvas, square);
        let there = coverage(canvas, rect(3, 4, 7, 7));
        let back = coverage(canvas, rect(6, 6, 7, 7));
        let start = doc(depth, canvas, &plane);
        let out = start
            .moved(0, (3, 2), &there)
            .moved(0, (-3, -2), &back)
            .plane(0);
        assert_eq!((out.offset, out.size), ((0, 0), canvas));
        for (got, want) in out.px.iter().zip(&plane.px) {
            assert_eq!(got[3], want[3], "{depth:?} alpha");
            if want[3] > 0 {
                assert_eq!(got, want, "{depth:?} visible RGB");
            }
        }
    }
}

#[test]
fn identity_is_null_and_an_empty_area_refuses() {
    let canvas = (10, 8);
    for depth in DEPTHS {
        let transparent = |x: u32, y: u32| {
            let mut p = pattern(x, y);
            p[3] = if x < 5 { 255 } else { 0 };
            p
        };
        let plane = Plane::of(depth, (0, 0), canvas, transparent);
        let base = doc(depth, canvas, &plane);
        let cov = coverage(canvas, rect(1, 1, 3, 3));
        assert!(
            base.edit(&request(0, 0, 0), &cov).unwrap().is_none(),
            "{depth:?}"
        );
        let empty = coverage(canvas, rect(6, 1, 3, 3));
        assert_eq!(
            base.edit(&request(0, 2, 0), &empty).err().as_deref(),
            Some(EMPTY)
        );
        let none = vec![0u16; 80];
        assert_eq!(
            base.edit(&request(0, 2, 0), &none).err().as_deref(),
            Some(EMPTY)
        );
    }
}

#[test]
fn pixel_locks_refuse_and_transparency_and_position_locks_allow() {
    let canvas = (12, 8);
    let cov = coverage(canvas, rect(2, 2, 3, 3));
    for depth in DEPTHS {
        let plane = Plane::of(depth, (0, 0), canvas, pattern);
        for locks in [LOCK_PIXELS, LOCK_ALL] {
            let err = match depth {
                Depth::D8 => {
                    let mut d = byte_doc(canvas, &plane);
                    d.layers[0].locks = locks;
                    edit8(&d, &request(0, 2, 1), &cov, None).err()
                }
                Depth::D16 => {
                    let d = typed_doc(canvas, vec![typed_layer(&plane)])
                        .with_layer_locks(0, locks)
                        .unwrap()
                        .unwrap();
                    edit16(&d, &request(0, 2, 1), &cov, None).err()
                }
            };
            assert!(err.unwrap().contains("locked"), "{depth:?} locks {locks}");
        }
        for locks in [LOCK_TRANSPARENCY, LOCK_POSITION] {
            let out = match depth {
                Depth::D8 => {
                    let mut d = byte_doc(canvas, &plane);
                    d.layers[0].locks = locks;
                    Doc::Byte(d).moved(0, (2, 1), &cov).plane(0)
                }
                Depth::D16 => {
                    let d = typed_doc(canvas, vec![typed_layer(&plane)])
                        .with_layer_locks(0, locks)
                        .unwrap()
                        .unwrap();
                    Doc::Typed(d).moved(0, (2, 1), &cov).plane(0)
                }
            };
            // The hole is punched: the move is not a stamp.
            assert_eq!(out.at(2, 2).unwrap()[3], 0, "{depth:?} locks {locks}");
            assert_eq!(out.at(4, 3), plane.at(2, 2), "{depth:?} locks {locks}");
        }
    }
    // A pixels-locked parent group covers its child.
    let plane = Plane::of(Depth::D8, (0, 0), canvas, pattern);
    let (mut d, group, _, _) = byte_doc(canvas, &plane).group_layers(&[0], "G").unwrap();
    d.layers[group].locks = LOCK_PIXELS;
    let err = edit8(&d, &request(0, 2, 1), &cov, None).err().unwrap();
    assert!(err.contains("inside group"), "{err}");
    let plane = Plane::of(Depth::D16, (0, 0), canvas, pattern);
    let group =
        PrecisionDocumentLayer::isolated_normal("G", vec![typed_layer(&plane)], 1.0, true).unwrap();
    let d = typed_doc(canvas, vec![group])
        .with_layer_locks(1, LOCK_PIXELS)
        .unwrap()
        .unwrap();
    let err = edit16(&d, &request(0, 2, 1), &cov, None).err().unwrap();
    assert!(err.contains("inside group"), "{err}");
}

#[test]
fn groups_adjustments_and_malformed_requests_refuse() {
    let canvas = (8, 6);
    let cov = coverage(canvas, rect(1, 1, 3, 3));
    let plane8 = Plane::of(Depth::D8, (0, 0), canvas, pattern);
    let (grouped, group, _, _) = byte_doc(canvas, &plane8).group_layers(&[0], "G").unwrap();
    let err = edit8(&grouped, &request(group, 1, 0), &cov, None)
        .err()
        .unwrap();
    assert!(err.contains("is a group"), "{err}");
    let mut adjusted = byte_doc(canvas, &plane8)
        .adding_image_layer(0, RgbaImage::new(8, 6), "Fx")
        .unwrap();
    adjusted.layers[1].meta = Some(adjustment_meta());
    let err = edit8(&adjusted, &request(1, 1, 0), &cov, None)
        .err()
        .unwrap();
    assert!(err.contains("adjustment layer"), "{err}");

    let plane16 = Plane::of(Depth::D16, (0, 0), canvas, pattern);
    let group =
        PrecisionDocumentLayer::isolated_normal("G", vec![typed_layer(&plane16)], 1.0, true)
            .unwrap();
    let grouped = typed_doc(canvas, vec![group]);
    let err = edit16(&grouped, &request(1, 1, 0), &cov, None)
        .err()
        .unwrap();
    assert!(err.contains("is a group"), "{err}");
    let adjusted = typed_doc(canvas, vec![typed_layer(&plane16)])
        .add_adjustment("Fx", &adjustment_meta(), 1.0, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let err = edit16(&adjusted, &request(1, 1, 0), &cov, None)
        .err()
        .unwrap();
    assert!(err.contains("adjustment layer"), "{err}");

    for depth in DEPTHS {
        let base = doc(depth, canvas, &Plane::of(depth, (0, 0), canvas, pattern));
        let short = &cov[..cov.len() - 1];
        assert!(
            base.edit(&request(0, 1, 0), short).is_err(),
            "{depth:?} length"
        );
        assert!(
            base.edit(&request(9, 1, 0), &cov).is_err(),
            "{depth:?} index"
        );
        let extra = json!({"op":"move_selected","layer":0,"dx":1,"dy":0,"x":1}).to_string();
        let err = base.edit(&extra, &cov).err().unwrap();
        assert!(err.contains("unknown field"), "{depth:?} {err}");
    }
}

#[test]
fn a_float_document_moves_its_layer_pixels() {
    let path = "tests/fixtures/float_11f/scene.tiff";
    if !std::path::Path::new(path).exists() {
        return;
    }
    let doc = PrecisionDocument::open_preserving(path, LIMIT, 0, None).unwrap();
    assert!(doc.is_float());
    let (w, h) = doc.dimensions();
    let cov = vec![65535u16; (w * h) as usize];
    // Admitted; the float arm's own tests are `float_selected_pixels.rs`.
    let moved = edit16(&doc, &request(0, 1, 0), &cov, None)
        .unwrap()
        .unwrap();
    assert!(moved.is_float());
}

#[test]
fn the_plan_is_independent_of_the_delta_and_refuses_a_group() {
    let canvas = (12, 9);
    let plane8 = Plane::of(Depth::D8, (-2, 1), (10, 6), pattern);
    let d8 = byte_doc(canvas, &plane8);
    let a = plan8(&d8, &request(0, 1, 0)).unwrap();
    assert_eq!(a, plan8(&d8, &request(0, 50, -7)).unwrap());
    assert_eq!(a["input_samples"], 108);
    assert!(a["scratch_bytes"].as_u64().unwrap() > 0);
    let (grouped, group, _, _) = d8.group_layers(&[0], "G").unwrap();
    assert!(plan8(&grouped, &request(group, 1, 0)).is_err());

    let plane16 = Plane::of(Depth::D16, (-2, 1), (10, 6), pattern);
    let d16 = typed_doc(canvas, vec![typed_layer(&plane16)]);
    let b = plan16(&d16, &request(0, 1, 0)).unwrap();
    assert_eq!(b, plan16(&d16, &request(0, 50, -7)).unwrap());
    assert_eq!(b["input_samples"], 108);
    let group =
        PrecisionDocumentLayer::isolated_normal("G", vec![typed_layer(&plane16)], 1.0, true)
            .unwrap();
    assert!(plan16(&typed_doc(canvas, vec![group]), &request(1, 1, 0)).is_err());
}

#[test]
fn the_layer_capability_admits_sixteen_and_thirty_two_bits() {
    assert_eq!(
        capability("selected_pixels_move", "raster", 16)["supported"],
        true
    );
    assert_eq!(
        capability("selected_pixels_move", "group", 16)["supported"],
        false
    );
    assert_eq!(
        capability("selected_pixels_move", "raster", 32)["supported"],
        true
    );
}

#[test]
fn a_cancelled_control_returns_operation_cancelled() {
    let canvas = (8, 6);
    let cov = coverage(canvas, rect(1, 1, 3, 3));
    let control = DrJobControl::default();
    control.cancel();
    let d8 = byte_doc(canvas, &Plane::of(Depth::D8, (0, 0), canvas, pattern));
    let err = edit8(&d8, &request(0, 1, 0), &cov, Some(&control))
        .err()
        .unwrap();
    assert_eq!(err, "operation cancelled");
    let control = DrJobControl::default();
    control.cancel();
    let d16 = typed_doc(
        canvas,
        vec![typed_layer(&Plane::of(Depth::D16, (0, 0), canvas, pattern))],
    );
    let err = edit16(&d16, &request(0, 1, 0), &cov, Some(&control))
        .err()
        .unwrap();
    assert_eq!(err, "operation cancelled");
}

#[test]
fn both_exports_tolerate_null() {
    let text = CString::new(request(0, 1, 0)).unwrap();
    let canvas = (4, 4);
    let d = byte_doc(canvas, &Plane::of(Depth::D8, (0, 0), canvas, pattern));
    let cov = [65535u16; 16];
    unsafe {
        assert!(
            dr_doc_selected_pixels_edit_plan(ptr::null(), text.as_ptr(), ptr::null_mut()).is_null()
        );
        assert!(dr_doc_selected_pixels_edit_plan(&d, ptr::null(), ptr::null_mut()).is_null());
        let mut err = ptr::null_mut();
        assert!(dr_doc_selected_pixels_edit_plan(ptr::null(), text.as_ptr(), &mut err).is_null());
        assert!(take_err(err).is_some());
        for (doc, request, coverage) in [
            (ptr::null(), text.as_ptr(), cov.as_ptr()),
            (&d as *const _, ptr::null(), cov.as_ptr()),
            (&d as *const _, text.as_ptr(), ptr::null()),
        ] {
            assert!(dr_doc_selected_pixels_edit(
                doc,
                request,
                coverage,
                16,
                LIMIT,
                0,
                ptr::null(),
                ptr::null_mut()
            )
            .is_null());
            let mut err = ptr::null_mut();
            assert!(dr_doc_selected_pixels_edit(
                doc,
                request,
                coverage,
                16,
                LIMIT,
                0,
                ptr::null(),
                &mut err
            )
            .is_null());
            assert!(take_err(err).is_some());
        }
    }
}

// -------------------------------------------------------------- duplicate --

#[test]
fn a_duplicated_hard_rect_keeps_the_original_and_lands_a_copy() {
    let canvas = (16, 12);
    for depth in DEPTHS {
        let plane = Plane::of(depth, (0, 0), canvas, pattern);
        let cov = coverage(canvas, rect(3, 2, 5, 4));
        let out = doc(depth, canvas, &plane)
            .edit(&duplicate(0, 4, 3), &cov)
            .unwrap()
            .expect("the copy changes something")
            .plane(0);
        assert_eq!((out.offset, out.size), ((0, 0), canvas));
        for y in 0..12i64 {
            for x in 0..16i64 {
                let got = out.at(x, y).unwrap();
                let landed = (7..12).contains(&x) && (5..9).contains(&y);
                let want = if landed {
                    plane.at(x - 4, y - 3).unwrap()
                } else {
                    plane.at(x, y).unwrap()
                };
                assert_eq!(got, want, "{depth:?} {x},{y}");
            }
        }
        let want = oracle(depth, canvas, &plane, &cov, (4, 3), false);
        assert_matches(depth, &out, &want, "duplicate hard");
    }
}

#[test]
fn a_duplicated_feathered_float_matches_the_no_hole_oracle() {
    let canvas = (18, 14);
    let feather = |x: u32, y: u32| -> u16 {
        let (dx, dy) = (f64::from(x) - 7.3, f64::from(y) - 6.1);
        let r = (dx * dx + dy * dy).sqrt();
        ((1.0 - ((r - 2.0) / 4.0).clamp(0.0, 1.0)) * 65535.0).round() as u16
    };
    let alpha = |x: u32, y: u32| -> [u8; 4] {
        let mut p = pattern(x, y);
        p[3] = ((x * 23 + y * 31) % 200 + 40) as u8;
        p
    };
    for depth in DEPTHS {
        let plane = Plane::of(depth, (0, 0), canvas, alpha);
        let cov = coverage(canvas, feather);
        // (2, 1) lands over F itself: the copy composites over the original.
        for d in [(3, 2), (2, 1), (-2, 1), (1, -3)] {
            let out = doc(depth, canvas, &plane)
                .edit(&duplicate(0, d.0, d.1), &cov)
                .unwrap()
                .unwrap()
                .plane(0);
            let want = oracle(
                depth,
                canvas,
                &plane,
                &cov,
                (i64::from(d.0), i64::from(d.1)),
                false,
            );
            assert_matches(depth, &out, &want, &format!("duplicate feathered {d:?}"));
        }
    }
}

#[test]
fn a_duplicate_landing_outside_the_layer_grows_it() {
    let canvas = (20, 16);
    for depth in DEPTHS {
        let plane = Plane::of(depth, (2, 3), (6, 4), pattern);
        let cov = coverage(canvas, rect(3, 3, 3, 3));
        let out = doc(depth, canvas, &plane)
            .edit(&duplicate(0, 9, 7), &cov)
            .unwrap()
            .unwrap()
            .plane(0);
        let want = oracle(depth, canvas, &plane, &cov, (9, 7), false);
        assert_matches(depth, &out, &want, "duplicate grow");
        assert_eq!((out.offset, out.size), ((2, 3), (13, 10)));
        // The original float is still there, untouched.
        for (x, y) in [(3, 3), (5, 5), (4, 4)] {
            assert_eq!(out.at(x, y), plane.at(x, y), "{depth:?} original {x},{y}");
        }
    }
}

#[test]
fn duplicate_false_is_the_plain_move_and_a_zero_delta_is_null() {
    let canvas = (12, 9);
    for depth in DEPTHS {
        let plane = Plane::of(depth, (0, 0), canvas, pattern);
        let base = doc(depth, canvas, &plane);
        let cov = coverage(canvas, rect(2, 2, 4, 3));
        let plain = base
            .edit(&request(0, 3, 1), &cov)
            .unwrap()
            .unwrap()
            .plane(0);
        let explicit = json!({"op":"move_selected","layer":0,"dx":3,"dy":1,"duplicate":false});
        let same = base
            .edit(&explicit.to_string(), &cov)
            .unwrap()
            .unwrap()
            .plane(0);
        assert_eq!(plain, same, "{depth:?}");
        assert!(
            base.edit(&duplicate(0, 0, 0), &cov).unwrap().is_none(),
            "{depth:?}"
        );
        let empty = vec![0u16; 108];
        assert_eq!(
            base.edit(&duplicate(0, 2, 0), &empty).err().as_deref(),
            Some(EMPTY)
        );
    }
}
