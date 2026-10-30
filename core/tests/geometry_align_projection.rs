//! Auto-Align Layers' Cylindrical and Spherical reprojection:
//! `project_layer` at 8 bits, 16 bits and 32-bit float through the FFI
//! (`dr_doc_project_layer(_plan)`, `dr_precision_doc_project_layer(_plan)`).
//!
//! # The oracle
//!
//! The layer holds a SMOOTH analytic picture, each pixel the picture at its
//! centre. The test writes its own inverse map from the convention in the
//! header (a destination point's longitude and height or latitude, the ray,
//! the rotation, the pinhole), and every destination pixel whose source point
//! lies at least [`INSIDE`] px inside the layer (so every Catmull-Rom tap is
//! inside it) is compared with the picture evaluated there: at 8 bits the
//! mean |Δ| is at most 1 code and the largest 4, scaled by 257 at 16 bits and
//! by 1/255 at float. A pixel whose source point is outside the layer (by
//! more than rounding) must be transparent. The destination rect is checked
//! against the test's own forward map walked densely over the pixel-centre
//! perimeter, under the pixel-span rule, and the plan's
//! rect against the edit's. No golden images.
#![allow(clippy::needless_range_loop)]

use std::f64::consts::PI;
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use crate::common::selected_pixels::take_err;
use crate::float_fixture::float_doc;
use doctored_core::doc::{DrDocument, LayerRaster};
use doctored_core::doc_lock::LOCK_POSITION;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_align_projection::{
    dr_doc_project_layer, dr_doc_project_layer_plan, dr_precision_doc_project_layer,
    dr_precision_doc_project_layer_plan,
};
use doctored_core::ffi_group::dr_doc_layer_bounds;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{GrayImage, RgbaImage};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CANVAS: (u32, u32) = (140, 100);
const OFFSET: (i32, i32) = (10, 6);
const SIZE: (u32, u32) = (120, 80);
const FOCAL: f64 = 150.0;
/// How far inside the layer a compared source point must be: Catmull-Rom
/// reads two pixels each side.
const INSIDE: f64 = 3.0;

/// The smooth picture, 0…1, at a layer-local continuous point.
fn picture(u: f64, v: f64, channel: usize) -> f64 {
    let c = channel as f64;
    0.5 + 0.35 * (u / 7.0 + c).sin() * (v / 9.0 - c / 2.0).cos()
}

fn byte_doc() -> DrDocument {
    let image = RgbaImage::from_fn(SIZE.0, SIZE.1, |x, y| {
        let (u, v) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
        image::Rgba([
            (picture(u, v, 0) * 255.0).round() as u8,
            (picture(u, v, 1) * 255.0).round() as u8,
            (picture(u, v, 2) * 255.0).round() as u8,
            255,
        ])
    });
    let mut doc = DrDocument::from_pixels(RgbaImage::new(CANVAS.0, CANVAS.1));
    doc.layers[0].pixels = LayerRaster::new(image);
    doc.layers[0].offset = OFFSET;
    doc
}

fn typed16() -> PrecisionDocument {
    let mut px = Vec::new();
    for y in 0..SIZE.1 {
        for x in 0..SIZE.0 {
            let (u, v) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
            let c = |k| (picture(u, v, k) * 65535.0).round() as u16;
            px.push([c(0), c(1), c(2), 65535]);
        }
    }
    let image = PrecisionImage::from_rgba16(SIZE.0, SIZE.1, px, IccProfile::srgb(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("L", image, OFFSET, 1.0, true).unwrap();
    PrecisionDocument::from_parts(
        CANVAS.0,
        CANVAS.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap()
}

/// The float twin: one canvas-sized record, the picture over the whole of it.
fn typed32() -> PrecisionDocument {
    let record: Vec<[f32; 4]> = (0..SIZE.0 * SIZE.1)
        .map(|i| {
            let (u, v) = (f64::from(i % SIZE.0) + 0.5, f64::from(i / SIZE.0) + 0.5);
            [
                picture(u, v, 0) as f32,
                picture(u, v, 1) as f32,
                picture(u, v, 2) as f32,
                1.0,
            ]
        })
        .collect();
    float_doc(SIZE.0, SIZE.1, &IccProfile::srgb(), &[record])
}

enum TestDoc {
    Byte(DrDocument),
    Typed(PrecisionDocument),
}

/// A layer's canvas rect and straight samples in storage units.
struct Samples {
    rect: (i64, i64, u32, u32),
    px: Vec<[f64; 4]>,
}

fn string(text: *mut c_char) -> String {
    let out = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(text) };
    out
}

impl TestDoc {
    fn layer(&self, index: usize) -> Samples {
        match self {
            TestDoc::Byte(d) => {
                let layer = &d.layers[index];
                let (w, h) = layer.pixels.dimensions();
                let px = layer.pixels.read_rect(0, 0, w, h).unwrap();
                Samples {
                    rect: (i64::from(layer.offset.0), i64::from(layer.offset.1), w, h),
                    px: px.iter().map(|p| p.map(f64::from)).collect(),
                }
            }
            TestDoc::Typed(d) => {
                let layer = d.layer(index).unwrap();
                let image = layer.source().unwrap();
                let (w, h) = image.dimensions();
                let (x, y) = layer.offset();
                let px = if d.is_float() {
                    let samples = image.to_rgba32().unwrap();
                    samples.iter().map(|p| p.map(f64::from)).collect()
                } else {
                    let samples = image.to_rgba16().unwrap();
                    samples.iter().map(|p| p.map(f64::from)).collect()
                };
                Samples {
                    rect: (i64::from(x), i64::from(y), w, h),
                    px,
                }
            }
        }
    }

    fn max(&self) -> f64 {
        match self {
            TestDoc::Byte(_) => 255.0,
            TestDoc::Typed(d) if d.is_float() => 1.0,
            TestDoc::Typed(_) => 65535.0,
        }
    }

    fn plan(&self, request: &Value) -> Result<Value, String> {
        let text = CString::new(request.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            match self {
                TestDoc::Byte(d) => dr_doc_project_layer_plan(d, text.as_ptr(), LIMIT, 0, &mut err),
                TestDoc::Typed(d) => {
                    dr_precision_doc_project_layer_plan(d, text.as_ptr(), LIMIT, 0, &mut err)
                }
            }
        };
        if let Some(message) = take_err(err) {
            assert!(out.is_null());
            return Err(message);
        }
        Ok(serde_json::from_str(&string(out)).unwrap())
    }

    fn project(&self, request: &Value) -> Result<TestDoc, String> {
        let text = CString::new(request.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            match self {
                TestDoc::Byte(d) => {
                    dr_doc_project_layer(d, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
                        .cast::<u8>()
                }
                TestDoc::Typed(d) => dr_precision_doc_project_layer(
                    d,
                    text.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                )
                .cast::<u8>(),
            }
        };
        if let Some(message) = take_err(err) {
            assert!(out.is_null());
            return Err(message);
        }
        assert!(!out.is_null(), "the edit never answers no change");
        Ok(unsafe {
            match self {
                TestDoc::Byte(_) => TestDoc::Byte(*Box::from_raw(out.cast::<DrDocument>())),
                TestDoc::Typed(_) => {
                    TestDoc::Typed(*Box::from_raw(out.cast::<PrecisionDocument>()))
                }
            }
        })
    }
}

/// Row-major R_y(yaw) · R_x(pitch).
fn rotation(yaw_degrees: f64, pitch_degrees: f64) -> [f64; 9] {
    let (a, b) = (yaw_degrees.to_radians(), pitch_degrees.to_radians());
    let yaw = [a.cos(), 0.0, a.sin(), 0.0, 1.0, 0.0, -a.sin(), 0.0, a.cos()];
    let pitch = [1.0, 0.0, 0.0, 0.0, b.cos(), -b.sin(), 0.0, b.sin(), b.cos()];
    let mut out = [0.0; 9];
    for i in 0..3 {
        for j in 0..3 {
            out[i * 3 + j] = (0..3).map(|k| yaw[i * 3 + k] * pitch[k * 3 + j]).sum();
        }
    }
    out
}

/// The test's own map: a canvas point of the projection to the canvas point
/// of the layer (no clip), or `None` past the surface or behind the camera.
struct Oracle {
    spherical: bool,
    r: [f64; 9],
    centre: [f64; 2],
    origin: [f64; 2],
}

impl Oracle {
    fn inverse(&self, p: [f64; 2]) -> Option<[f64; 2]> {
        let theta = (p[0] - self.origin[0]) / FOCAL;
        let t = (p[1] - self.origin[1]) / FOCAL;
        if theta.abs() > PI || (self.spherical && t.abs() > PI / 2.0) {
            return None;
        }
        let d = if self.spherical {
            [theta.sin() * t.cos(), t.sin(), theta.cos() * t.cos()]
        } else {
            [theta.sin(), t, theta.cos()]
        };
        let m = &self.r;
        let r = [
            m[0] * d[0] + m[1] * d[1] + m[2] * d[2],
            m[3] * d[0] + m[4] * d[1] + m[5] * d[2],
            m[6] * d[0] + m[7] * d[1] + m[8] * d[2],
        ];
        (r[2] > 1e-6).then(|| {
            [
                self.centre[0] + FOCAL * r[0] / r[2],
                self.centre[1] + FOCAL * r[1] / r[2],
            ]
        })
    }

    fn forward(&self, q: [f64; 2]) -> [f64; 2] {
        let v = [
            (q[0] - self.centre[0]) / FOCAL,
            (q[1] - self.centre[1]) / FOCAL,
            1.0,
        ];
        let m = &self.r;
        // Rᵀ · v.
        let d = [
            m[0] * v[0] + m[3] * v[1] + m[6] * v[2],
            m[1] * v[0] + m[4] * v[1] + m[7] * v[2],
            m[2] * v[0] + m[5] * v[1] + m[8] * v[2],
        ];
        let around = (d[0] * d[0] + d[2] * d[2]).sqrt();
        let t = if self.spherical {
            d[1].atan2(around)
        } else {
            d[1] / around
        };
        [
            self.origin[0] + FOCAL * d[0].atan2(d[2]),
            self.origin[1] + FOCAL * t,
        ]
    }
}

fn request(oracle: &Oracle, layer: usize) -> Value {
    json!({
        "op": "project_layer", "layer": layer,
        "projection": if oracle.spherical { "spherical" } else { "cylindrical" },
        "focal": FOCAL, "rotation": oracle.r.to_vec(),
        "centre": oracle.centre.to_vec(), "origin": oracle.origin.to_vec(),
    })
}

/// The reprojection of `doc`'s layer 0 against the oracle at `tolerance`
/// (mean, max) in 8-bit codes; answers the projected layer's rect.
fn check(doc: &TestDoc, spherical: bool, yaw: f64, pitch: f64) -> (i64, i64, u32, u32) {
    let before = doc.layer(0);
    let (lx, ly, lw, lh) = before.rect;
    let centre = [
        lx as f64 + f64::from(lw) / 2.0,
        ly as f64 + f64::from(lh) / 2.0,
    ];
    let oracle = Oracle {
        spherical,
        r: rotation(yaw, pitch),
        centre,
        origin: [centre[0] + 7.0, centre[1] - 3.0],
    };
    let request = request(&oracle, 0);
    let plan = doc.plan(&request).unwrap();
    let out = doc.project(&request).unwrap();
    let after = out.layer(0);
    let rect = after.rect;
    assert_eq!(
        plan["rect"],
        json!([rect.0, rect.1, rect.2, rect.3]),
        "the plan's rect is the edit's"
    );
    assert_eq!(plan["vertices"], json!(0));
    let bytes = match doc {
        TestDoc::Byte(_) => 4,
        TestDoc::Typed(d) if d.is_float() => 16,
        TestDoc::Typed(_) => 8,
    };
    // The documented scratch of an unlinked layer of `src` pixels projected
    // onto `dest` pixels: PLAN_BOOKKEEPING (2 MiB); the source copy at the
    // depth's bytes a pixel; the output at those bytes plus the shared
    // 4-byte map margin; PROJECTION_MARGIN_BYTES_PER_PIXEL (31) less that
    // shared 4, so 27 a destination pixel; and at float the output's
    // adoption copy, 16 a pixel plus 64 a 256 × 256 tile (TILE_EDGE).
    let src = u64::from(lw) * u64::from(lh);
    let dest = u64::from(rect.2) * u64::from(rect.3);
    let tiles = u64::from(rect.2.div_ceil(256)) * u64::from(rect.3.div_ceil(256));
    let bookkeeping = 2_097_152;
    let documented = match bytes {
        4 => bookkeeping + 4 * src + (4 + 4 + 27) * dest,
        8 => bookkeeping + 8 * src + (8 + 4 + 27) * dest,
        _ => bookkeeping + 16 * src + (16 + 4 + 27 + 16) * dest + 64 * tiles,
    };
    assert_eq!(
        plan["scratch_bytes"],
        json!(documented),
        "the plan's scratch is the documented formula"
    );
    assert_eq!(
        plan["output_bytes"],
        json!(u64::from(rect.2) * u64::from(rect.3) * bytes),
        "the plan's output is the projected layer"
    );

    // The destination rect: the pixels whose centres the forward image of
    // the PIXEL-CENTRE perimeter reaches, walked densely (rule (b): per axis
    // ceil(lo − ½ − 0.01) to floor(hi − ½ + 0.01) + 1).
    let (x0, y0) = (lx as f64, ly as f64);
    let (x1, y1) = (x0 + f64::from(lw), y0 + f64::from(lh));
    let mut low = [f64::INFINITY; 2];
    let mut high = [f64::NEG_INFINITY; 2];
    let steps = 4000;
    let (c0, c1) = ([x0 + 0.5, y0 + 0.5], [x1 - 0.5, y1 - 0.5]);
    for k in 0..=steps {
        let t = f64::from(k) / f64::from(steps);
        for q in [
            [c0[0] + (c1[0] - c0[0]) * t, c0[1]],
            [c0[0] + (c1[0] - c0[0]) * t, c1[1]],
            [c0[0], c0[1] + (c1[1] - c0[1]) * t],
            [c1[0], c0[1] + (c1[1] - c0[1]) * t],
        ] {
            let p = oracle.forward(q);
            for i in 0..2 {
                low[i] = low[i].min(p[i]);
                high[i] = high[i].max(p[i]);
            }
        }
    }
    let want = [
        (low[0] - 0.5 - 0.01).ceil() as i64,
        (low[1] - 0.5 - 0.01).ceil() as i64,
        (high[0] - 0.5 + 0.01).floor() as i64 + 1,
        (high[1] - 0.5 + 0.01).floor() as i64 + 1,
    ];
    let got = [
        rect.0,
        rect.1,
        rect.0 + i64::from(rect.2),
        rect.1 + i64::from(rect.3),
    ];
    for i in 0..4 {
        assert!(
            (got[i] - want[i]).abs() <= 1,
            "dest rect {got:?} against the perimeter's {want:?}"
        );
    }

    let scale = doc.max() / 255.0;
    let (mut sum, mut count, mut worst, mut clear) = (0.0, 0usize, 0.0f64, 0usize);
    for y in 0..rect.3 {
        for x in 0..rect.2 {
            let p = after.px[(y * rect.2 + x) as usize];
            let centre_point = [
                rect.0 as f64 + f64::from(x) + 0.5,
                rect.1 as f64 + f64::from(y) + 0.5,
            ];
            let source = oracle.inverse(centre_point);
            let margin = |q: [f64; 2]| (q[0] - x0).min(x1 - q[0]).min(q[1] - y0).min(y1 - q[1]);
            match source {
                Some(q) if margin(q) >= INSIDE => {
                    assert!(
                        (p[3] - doc.max()).abs() <= scale * 0.5,
                        "alpha {} at ({x}, {y})",
                        p[3]
                    );
                    for c in 0..3 {
                        let want = picture(q[0] - x0, q[1] - y0, c) * doc.max();
                        let delta = (p[c] - want).abs() / scale;
                        sum += delta;
                        worst = worst.max(delta);
                        count += 1;
                    }
                }
                Some(q) if margin(q) > -1e-6 => {}
                _ => {
                    assert_eq!(p[3], 0.0, "no source at ({x}, {y})");
                    clear += 1;
                }
            }
        }
    }
    let mean = sum / count as f64;
    assert!(
        count > 3 * 2000,
        "compared {count} samples (the layer must land inside its rect)"
    );
    assert!(clear > 0, "the projected rect has transparent corners");
    assert!(mean <= 1.0, "mean |Δ| {mean} codes");
    assert!(worst <= 4.0, "max |Δ| {worst} codes");
    rect
}

#[test]
fn cylindrical_is_the_test_inverse_at_8_16_and_float() {
    for doc in [
        TestDoc::Byte(byte_doc()),
        TestDoc::Typed(typed16()),
        TestDoc::Typed(typed32()),
    ] {
        check(&doc, false, 15.0, 4.0);
        // The reference itself: R = I.
        check(&doc, false, 0.0, 0.0);
    }
}

#[test]
fn spherical_is_the_test_inverse_at_8_16_and_float() {
    for doc in [
        TestDoc::Byte(byte_doc()),
        TestDoc::Typed(typed16()),
        TestDoc::Typed(typed32()),
    ] {
        check(&doc, true, -20.0, 6.0);
        check(&doc, true, 0.0, 0.0);
    }
}

/// Take2's C rows (`~/devel/doctored-fixtures/photoshop/panorama-564/take2`)
/// from the take's own geometry, through the core alone: three 600 × 400
/// pinhole views at f 600, yawed −20, 0 and +20°, stacked bottom first with
/// the bottom one the reference. Each layer is projected with the rotation
/// the geometry gives (the reference's ray to the layer's: a yaw of the
/// reference minus the layer's) and the host's origin, the reference's axis
/// on a pixel centre (300.5, 200.5). Each layer's content box
/// (`dr_doc_layer_bounds`, what the app's canvas is set from), shifted to
/// the union's origin, must be within 1 px of take2's C row: x 0, 209 and
/// 419, widths 555, 556 and 555, at y 0 and 399 high (Cylindrical) or 385
/// (Spherical). The end-to-end check of these 20° frames waits on phase 3:
/// `AutoAlign.register` reaches about 10–15° at this focal.
#[test]
fn take2_rows_from_the_takes_focal_and_yaw() {
    const TAKE: (u32, u32) = (600, 400);
    const TAKE_FOCAL: f64 = 600.0;
    const YAWS: [f64; 3] = [-20.0, 0.0, 20.0];
    const ROWS: [[i64; 2]; 3] = [[0, 555], [209, 556], [419, 555]];
    for (projection, want_height) in [("cylindrical", 399i64), ("spherical", 385)] {
        let mut doc = DrDocument::from_pixels(RgbaImage::from_pixel(
            TAKE.0,
            TAKE.1,
            image::Rgba([128, 128, 128, 255]),
        ));
        let base = doc.layers[0].clone();
        doc.layers.push(base.clone());
        doc.layers.push(base);
        let centre = [f64::from(TAKE.0) / 2.0, f64::from(TAKE.1) / 2.0];
        let origin = [centre[0] + 0.5, centre[1] + 0.5];
        for (layer, yaw) in YAWS.iter().enumerate() {
            let r = rotation(YAWS[0] - yaw, 0.0);
            let request = json!({
                "op": "project_layer", "layer": layer, "projection": projection,
                "focal": TAKE_FOCAL, "rotation": r.to_vec(),
                "centre": centre.to_vec(), "origin": origin.to_vec(),
            });
            let TestDoc::Byte(out) = TestDoc::Byte(doc).project(&request).unwrap() else {
                unreachable!()
            };
            doc = out;
        }
        let boxes: Vec<[i64; 4]> = (0..3)
            .map(|layer| {
                let mut xywh = [0i32; 4];
                assert!(unsafe { dr_doc_layer_bounds(&doc, layer, xywh.as_mut_ptr()) });
                xywh.map(i64::from)
            })
            .collect();
        let x0 = boxes.iter().map(|b| b[0]).min().unwrap();
        let y0 = boxes.iter().map(|b| b[1]).min().unwrap();
        let rows: Vec<[i64; 4]> = boxes
            .iter()
            .map(|b| [b[0] - x0, b[1] - y0, b[2], b[3]])
            .collect();
        println!("take2 {projection}: rows (x, y, w, h) {rows:?}");
        for (row, want) in rows.iter().zip(ROWS) {
            let want = [want[0], 0, want[1], want_height];
            assert!(
                row.iter()
                    .zip(want)
                    .all(|(got, want)| (got - want).abs() <= 1),
                "{projection} row {row:?} against take2's {want:?}"
            );
        }
    }
}

#[test]
fn a_linked_mask_reprojects_along_and_an_unlinked_mask_stays() {
    for linked in [true, false] {
        let mut doc = byte_doc();
        let mask = Arc::new(GrayImage::from_pixel(SIZE.0, SIZE.1, image::Luma([200])));
        let mut properties = MaskProperties::at(OFFSET);
        properties.linked = linked;
        properties.outside_coverage = 255;
        doc.layers[0].mask = Some(mask.clone());
        doc.layers[0].mask_properties = properties;
        let centre = [70.0, 46.0];
        let oracle = Oracle {
            spherical: false,
            r: rotation(10.0, 0.0),
            centre,
            origin: centre,
        };
        let TestDoc::Byte(out) = TestDoc::Byte(doc).project(&request(&oracle, 0)).unwrap() else {
            unreachable!()
        };
        let layer = &out.layers[0];
        let after = layer.mask.as_ref().unwrap();
        if !linked {
            assert!(Arc::ptr_eq(after, &mask));
            assert_eq!(layer.mask_properties.offset, OFFSET);
            continue;
        }
        assert_eq!(layer.mask_properties.offset, layer.offset);
        assert_eq!(after.dimensions(), layer.pixels.dimensions());
        let (w, h) = after.dimensions();
        let (mut inside, mut outside) = (0, 0);
        for y in 0..h {
            for x in 0..w {
                let p = [
                    f64::from(layer.offset.0) + f64::from(x) + 0.5,
                    f64::from(layer.offset.1) + f64::from(y) + 0.5,
                ];
                let got = after.get_pixel(x, y).0[0];
                let (x0, y0) = (f64::from(OFFSET.0), f64::from(OFFSET.1));
                let (x1, y1) = (x0 + f64::from(SIZE.0), y0 + f64::from(SIZE.1));
                match oracle.inverse(p) {
                    Some(q)
                        if q[0] >= x0 + INSIDE
                            && q[0] <= x1 - INSIDE
                            && q[1] >= y0 + INSIDE
                            && q[1] <= y1 - INSIDE =>
                    {
                        assert!(got.abs_diff(200) <= 1, "mask {got} at ({x}, {y})");
                        inside += 1;
                    }
                    Some(q) if q[0] >= x0 && q[0] <= x1 && q[1] >= y0 && q[1] <= y1 => {}
                    _ => {
                        assert_eq!(got, 255, "outside coverage at ({x}, {y})");
                        outside += 1;
                    }
                }
            }
        }
        assert!(
            inside > 1000 && outside > 0,
            "inside {inside}, outside {outside}"
        );
    }
}

fn refusal(doc: &TestDoc, request: Value) -> String {
    let planned = doc.plan(&request).expect_err("the plan refuses");
    let edited = doc.project(&request).err().expect("the edit refuses");
    assert_eq!(planned, edited);
    planned
}

#[test]
fn refusals_name_the_reason() {
    let doc = TestDoc::Byte(byte_doc());
    let good = request(
        &Oracle {
            spherical: false,
            r: rotation(10.0, 0.0),
            centre: [70.0, 46.0],
            origin: [70.0, 46.0],
        },
        0,
    );
    let with = |key: &str, value: Value| {
        let mut r = good.clone();
        r[key] = value;
        r
    };
    assert_eq!(
        refusal(&doc, with("focal", json!(0.0))),
        "project layer focal must be a finite number above 0"
    );
    let rotation_refusal =
        "project layer rotation must be an orthonormal 3x3 matrix with determinant 1";
    assert_eq!(
        refusal(
            &doc,
            with("rotation", json!([1, 0, 0, 0, 1, 0, 0, 0, 1.001]))
        ),
        rotation_refusal
    );
    assert_eq!(
        refusal(&doc, with("rotation", json!([-1, 0, 0, 0, 1, 0, 0, 0, 1]))),
        rotation_refusal,
        "a reflection is not a rotation"
    );
    assert_eq!(
        refusal(&doc, with("rotation", json!([1, 0, 0, 0, 1, 0, 0, 0]))),
        "invalid project layer request: rotation must be 9 finite numbers"
    );
    assert_eq!(
        refusal(&doc, with("projection", json!("perspective"))),
        "invalid project layer request: projection must be cylindrical, spherical or plane"
    );
    assert_eq!(
        refusal(&doc, with("extra", json!(1))),
        "invalid project layer request: unknown field `extra`"
    );
    assert_eq!(
        refusal(&doc, with("layer", json!(3))),
        "Auto-Align Layers layer index is out of range"
    );
    // A layer turned round behind the reference straddles the seam.
    assert_eq!(
        refusal(&doc, with("rotation", json!(rotation(180.0, 0.0).to_vec()))),
        "Auto-Align Layers cannot project layer 0: it reaches round past the far side of the \
         projection."
    );
    assert_eq!(
        refusal(&doc, with("origin", json!([3.0e9, 0.0]))),
        "Auto-Align Layers would make the layer larger than the pixel limit."
    );
    let mut locked = byte_doc();
    locked.layers[0].locks = LOCK_POSITION;
    assert_eq!(
        refusal(&TestDoc::Byte(locked), good.clone()),
        "layer 0 is locked (position)"
    );
    let mut described = byte_doc();
    described.layers[0].meta = Some(json!({"type":"text"}).to_string());
    assert_eq!(
        refusal(&TestDoc::Byte(described), good.clone()),
        "Rasterize layer 0 before Auto-Align Layers"
    );
    assert_eq!(
        refusal(&TestDoc::Typed(typed16()), with("focal", json!(-3.0))),
        "project layer focal must be a finite number above 0"
    );
}

#[test]
fn null_handles_answer_null_with_no_error() {
    let text = CString::new("{}").unwrap();
    let mut err = ptr::null_mut();
    unsafe {
        assert!(
            dr_doc_project_layer_plan(ptr::null(), text.as_ptr(), LIMIT, 0, &mut err).is_null()
        );
        assert!(
            dr_doc_project_layer(ptr::null(), text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
                .is_null()
        );
        let doc = byte_doc();
        assert!(dr_doc_project_layer_plan(&doc, ptr::null(), LIMIT, 0, ptr::null_mut()).is_null());
    }
    assert!(err.is_null());
}
