//! Auto-Align Layers' Geometric Distortion on Collage and Reposition:
//! `fit_distortion` on pure-shift and similarity frames at 8 bits, 16 bits
//! and 32-bit float, and `project_layer`'s `"plane"` through
//! the FFI (`dr_doc_fit_distortion`, `dr_precision_doc_fit_distortion`,
//! `dr_doc_project_layer(_plan)`).
//!
//! # The oracle
//!
//! Real-photo texture: the committed CC0 photograph
//! `core/tests/fixtures/registration_photo/sky.jpg` (2048 × 1366; source in
//! the folder's `provenance.json`), sampled with the test's own bilinear
//! (pixel centres at half-integers, clamped at the edge). Each layer is a
//! 600 × 400 view of it at the canvas origin, the bottom one the reference,
//! whose pixel u shows the photo at O0 + u (O0 = (420, 90): every sample,
//! about x 355…1385, y 0…580, lies in the cloud field or the sun, clear of
//! the rooftops and of the sparse dark sky left of x = 350; nothing clamps
//! at the top). The SHIFT set's other layers show it at O0 + (150, 20) and
//! O0 + (300, −25); the SIMILARITY set's add +2° at scale 1.0 and −3° at
//! scale 1.03 about the view centre. Each frame is distorted with k = −0.05
//! in the core's convention by the test's own Newton inverse (the frame's
//! pixel p shows the view's u with p = c + R·q·(1 + k·|q|²), q = (u − c)/R,
//! c = (300, 200), R = 300). The analytic lens-free maps (a layer's
//! corrected point to the reference's) are the fit's input.
//!
//! (g1) The fit finds |k + 0.05| ≤ 0.005 and each returned map puts the
//! rect's corners within 1 px of the analytic map. (g2) Each layer projected
//! onto the plane with its fitted map and k: its content rect within 1 px of
//! the oracle's (the destination pixel centres whose ANALYTIC inverse lands
//! in the layer's pixel-centre rect; 600 × 400 on the SHIFT set, take3's
//! Reposition D rows), and the mean |out − photo at O0 + p| ≤ 3 codes over
//! pixels whose source (the corrected point and the lens's point, both
//! clipped) is at least 4 px inside. (g3) The undistorted SHIFT
//! set fits |k| ≤ 0.002, and the identity plane with no `distortion` leaves
//! an opaque 8-bit layer byte-identical. (g4) The refusal sentences. No
//! golden images.
//!
//! Measured on this texture (release, `--nocapture`): k −0.04991 (8-bit
//! shift), −0.04993 (16-bit and float shift, 8-bit similarity); the shift
//! plane's mean 0.455 codes over 2,088,576 samples and the similarity
//! plane's 0.484 over 2,130,978; the undistorted shift 0.00001. Every
//! tolerance above was declared on the earlier texture and is unchanged.
#![allow(clippy::needless_range_loop)]

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::OnceLock;

use crate::common::selected_pixels::take_err;
use crate::float_fixture::float_doc;
use doctored_core::doc::{DrDocument, LayerRaster};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_align_distortion::{dr_doc_fit_distortion, dr_precision_doc_fit_distortion};
use doctored_core::ffi_align_projection::{dr_doc_project_layer, dr_doc_project_layer_plan};
use doctored_core::ffi_group::dr_doc_layer_bounds;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::RgbaImage;
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const SIZE: (u32, u32) = (600, 400);
/// The lens's centre and normalising radius for a 600 × 400 frame at the
/// origin, and the view centre the similarities turn about.
const CENTRE: [f64; 2] = [300.0, 200.0];
const RADIUS: f64 = 300.0;
const K: f64 = -0.05;
/// The reference's offset into the photo.
const O0: [f64; 2] = [420.0, 90.0];
/// Each non-reference layer's (offset from O0, degrees, scale).
const SHIFT: [([f64; 2], f64, f64); 2] = [([150.0, 20.0], 0.0, 1.0), ([300.0, -25.0], 0.0, 1.0)];
const SIMILARITY: [([f64; 2], f64, f64); 2] =
    [([150.0, 20.0], 2.0, 1.0), ([300.0, -25.0], -3.0, 1.03)];

/// The photograph, RGBA8.
fn photo() -> &'static RgbaImage {
    static PHOTO: OnceLock<RgbaImage> = OnceLock::new();
    PHOTO.get_or_init(|| {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/registration_photo/sky.jpg"
        );
        image::open(path).unwrap().to_rgba8()
    })
}

/// The photo's encoded channel `c` (0…1) at the continuous point `p`,
/// bilinear between pixel centres, clamped at the edge.
fn sample(p: [f64; 2], c: usize) -> f64 {
    let image = photo();
    let (w, h) = image.dimensions();
    let (x, y) = (p[0] - 0.5, p[1] - 0.5);
    let (i, j) = (x.floor(), y.floor());
    let (s, t) = (x - i, y - j);
    let at = |i: f64, j: f64| {
        let i = i.clamp(0.0, f64::from(w - 1)) as u32;
        let j = j.clamp(0.0, f64::from(h - 1)) as u32;
        f64::from(image.get_pixel(i, j).0[c]) / 255.0
    };
    let top = at(i, j) * (1.0 - s) + at(i + 1.0, j) * s;
    let bottom = at(i, j + 1.0) * (1.0 - s) + at(i + 1.0, j + 1.0) * s;
    top * (1.0 - t) + bottom * t
}

/// The core's convention forward: the frame point p that shows the view's
/// point u.
fn distort(u: [f64; 2], k: f64) -> [f64; 2] {
    let q = [(u[0] - CENTRE[0]) / RADIUS, (u[1] - CENTRE[1]) / RADIUS];
    let f = 1.0 + k * (q[0] * q[0] + q[1] * q[1]);
    [CENTRE[0] + RADIUS * q[0] * f, CENTRE[1] + RADIUS * q[1] * f]
}

/// The test's own inverse of [`distort`]: Newton on ρ(1 + kρ²) = ρ_p.
fn undistort(p: [f64; 2], k: f64) -> [f64; 2] {
    let q = [(p[0] - CENTRE[0]) / RADIUS, (p[1] - CENTRE[1]) / RADIUS];
    let target = q[0].hypot(q[1]);
    if target == 0.0 {
        return p;
    }
    let mut rho = target;
    for _ in 0..50 {
        let step = (rho * (1.0 + k * rho * rho) - target) / (1.0 + 3.0 * k * rho * rho);
        rho -= step;
        if step.abs() < 1e-14 {
            break;
        }
    }
    let s = rho / target;
    [CENTRE[0] + RADIUS * q[0] * s, CENTRE[1] + RADIUS * q[1] * s]
}

fn apply(m: &[f64; 9], p: [f64; 2]) -> [f64; 2] {
    let w = m[6] * p[0] + m[7] * p[1] + m[8];
    [
        (m[0] * p[0] + m[1] * p[1] + m[2]) / w,
        (m[3] * p[0] + m[4] * p[1] + m[5]) / w,
    ]
}

/// The analytic map of a layer (offset `d` from O0, `degrees`, `scale`):
/// its corrected point u shows the photo at O0 + d + c + S·(u − c), the
/// reference's corrected point r shows it at O0 + r, so r = d + c + S·(u − c).
fn similarity((d, degrees, scale): ([f64; 2], f64, f64)) -> [f64; 9] {
    let a = degrees.to_radians();
    let (m0, m1, m3, m4) = (
        scale * a.cos(),
        -scale * a.sin(),
        scale * a.sin(),
        scale * a.cos(),
    );
    [
        m0,
        m1,
        d[0] + CENTRE[0] - (m0 * CENTRE[0] + m1 * CENTRE[1]),
        m3,
        m4,
        d[1] + CENTRE[1] - (m3 * CENTRE[0] + m4 * CENTRE[1]),
        0.0,
        0.0,
        1.0,
    ]
}

/// The three analytic maps, the reference's the identity.
fn maps(set: &[([f64; 2], f64, f64); 2]) -> Vec<[f64; 9]> {
    let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    std::iter::once(identity)
        .chain(set.iter().map(|l| similarity(*l)))
        .collect()
}

/// Each frame's encoded samples (0…1): the frame's pixel centre undistorted
/// by `k`, through the layer's analytic map, read from the photo at O0 + r.
fn frames(set: &[([f64; 2], f64, f64); 2], k: f64) -> Vec<Vec<[f64; 3]>> {
    maps(set)
        .iter()
        .map(|m| {
            (0..SIZE.0 * SIZE.1)
                .map(|i| {
                    let p = [f64::from(i % SIZE.0) + 0.5, f64::from(i / SIZE.0) + 0.5];
                    let u = if k == 0.0 { p } else { undistort(p, k) };
                    let r = apply(m, u);
                    let at = [O0[0] + r[0], O0[1] + r[1]];
                    std::array::from_fn(|c| sample(at, c))
                })
                .collect()
        })
        .collect()
}

fn byte_doc(frames: &[Vec<[f64; 3]>]) -> DrDocument {
    let image = |samples: &Vec<[f64; 3]>| {
        RgbaImage::from_fn(SIZE.0, SIZE.1, |x, y| {
            let p = samples[(y * SIZE.0 + x) as usize];
            image::Rgba([
                (p[0] * 255.0).round() as u8,
                (p[1] * 255.0).round() as u8,
                (p[2] * 255.0).round() as u8,
                255,
            ])
        })
    };
    let mut doc = DrDocument::from_pixels(image(&frames[0]));
    let base = doc.layers[0].clone();
    for samples in &frames[1..] {
        let mut layer = base.clone();
        layer.pixels = LayerRaster::new(image(samples));
        doc.layers.push(layer);
    }
    doc
}

fn typed16(frames: &[Vec<[f64; 3]>]) -> PrecisionDocument {
    let records = frames
        .iter()
        .enumerate()
        .map(|(k, samples)| {
            let px = samples
                .iter()
                .map(|p| {
                    let c = |v: f64| (v * 65535.0).round() as u16;
                    [c(p[0]), c(p[1]), c(p[2]), 65535]
                })
                .collect();
            let image =
                PrecisionImage::from_rgba16(SIZE.0, SIZE.1, px, IccProfile::srgb(), None).unwrap();
            PrecisionDocumentLayer::raster(&format!("L{k}"), image, (0, 0), 1.0, true).unwrap()
        })
        .collect();
    PrecisionDocument::from_parts(
        SIZE.0,
        SIZE.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        records,
    )
    .unwrap()
}

/// sRGB decode: float layers hold linear light.
fn decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn typed32(frames: &[Vec<[f64; 3]>]) -> PrecisionDocument {
    let records: Vec<Vec<[f32; 4]>> = frames
        .iter()
        .map(|samples| {
            samples
                .iter()
                .map(|p| {
                    [
                        decode(p[0]) as f32,
                        decode(p[1]) as f32,
                        decode(p[2]) as f32,
                        1.0,
                    ]
                })
                .collect()
        })
        .collect();
    float_doc(SIZE.0, SIZE.1, &IccProfile::srgb(), &records)
}

fn string(text: *mut c_char) -> String {
    let out = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(text) };
    out
}

enum TestDoc {
    Byte(DrDocument),
    Typed(PrecisionDocument),
}

impl TestDoc {
    fn fit(&self, request: &Value) -> Result<Value, String> {
        let text = CString::new(request.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            match self {
                TestDoc::Byte(d) => {
                    dr_doc_fit_distortion(d, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
                }
                TestDoc::Typed(d) => dr_precision_doc_fit_distortion(
                    d,
                    text.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                ),
            }
        };
        if let Some(message) = take_err(err) {
            assert!(out.is_null());
            return Err(message);
        }
        Ok(serde_json::from_str(&string(out)).unwrap())
    }
}

/// `project_layer` on a byte document, the plan's rect checked against the
/// edit's.
fn project(doc: &DrDocument, request: &Value) -> Result<DrDocument, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let plan = unsafe { dr_doc_project_layer_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    let planned = take_err(err);
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_project_layer(doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    if let Some(message) = take_err(err) {
        assert!(out.is_null());
        assert_eq!(
            planned.as_deref(),
            Some(message.as_str()),
            "plan and edit agree"
        );
        return Err(message);
    }
    assert!(planned.is_none() && !plan.is_null());
    let plan: Value = serde_json::from_str(&string(plan)).unwrap();
    assert!(!out.is_null());
    let next = unsafe { *Box::from_raw(out) };
    let layer = &next.layers[request["layer"].as_u64().unwrap() as usize];
    let (w, h) = layer.pixels.dimensions();
    assert_eq!(
        plan["rect"],
        json!([layer.offset.0, layer.offset.1, w, h]),
        "the plan's rect is the edit's"
    );
    Ok(next)
}

/// The request: the reference with the identity, then each layer anchored
/// to the reference with its analytic map (the host's planar request).
fn request(set: &[([f64; 2], f64, f64); 2]) -> Value {
    json!({
        "op": "fit_distortion",
        "layers": maps(set).iter().enumerate()
            .map(|(layer, m)| json!({"layer": layer, "anchor": 0, "map": m.to_vec()}))
            .collect::<Vec<_>>(),
    })
}

fn plane(layer: usize, map: &[f64; 9], distortion: Option<f64>) -> Value {
    let mut out = json!({
        "op": "project_layer", "layer": layer, "projection": "plane", "map": map.to_vec(),
    });
    if let Some(k) = distortion {
        out["distortion"] = json!(k);
    }
    out
}

fn bounds(doc: &DrDocument, layer: usize) -> [i64; 4] {
    let mut xywh = [0i32; 4];
    assert!(unsafe { dr_doc_layer_bounds(doc, layer, xywh.as_mut_ptr()) });
    xywh.map(i64::from)
}

/// The fitted coefficient and maps, checked against (g1)'s bounds.
fn fitted(doc: &TestDoc, set: &[([f64; 2], f64, f64); 2], label: &str) -> (f64, Vec<[f64; 9]>) {
    let fitted = doc.fit(&request(set)).unwrap();
    let coefficient = fitted["coefficient"].as_f64().unwrap();
    println!(
        "{label}: coefficient {coefficient:.5}, matches {}, residual {:.4}",
        fitted["matches"], fitted["residual"]
    );
    assert!(
        (coefficient - K).abs() <= 0.005,
        "{label}: coefficient {coefficient}"
    );
    assert_eq!(fitted["layers"], json!([0, 1, 2]));
    let got: Vec<[f64; 9]> = fitted["maps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            let v: Vec<f64> = m
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            v.try_into().unwrap()
        })
        .collect();
    assert_eq!(got.len(), 3);
    for (layer, (got, want)) in got.iter().zip(maps(set)).enumerate() {
        for corner in [[0.0, 0.0], [600.0, 0.0], [600.0, 400.0], [0.0, 400.0]] {
            let (a, b) = (apply(got, corner), apply(&want, corner));
            let off = (a[0] - b[0]).hypot(a[1] - b[1]);
            assert!(
                off <= 1.0,
                "{label} layer {layer} corner {corner:?}: {a:?} against {b:?}"
            );
        }
    }
    (coefficient, got)
}

/// The inverse of an affine map (the analytic maps are affine).
fn affine_inverse(m: &[f64; 9]) -> [f64; 9] {
    let det = m[0] * m[4] - m[1] * m[3];
    let (a, b, c, d) = (m[4] / det, -m[1] / det, -m[3] / det, m[0] / det);
    [
        a,
        b,
        -(a * m[2] + b * m[5]),
        c,
        d,
        -(c * m[2] + d * m[5]),
        0.0,
        0.0,
        1.0,
    ]
}

/// (g2) on one set at 8 bits.
fn planar_matches_the_photo(set: &[([f64; 2], f64, f64); 2], label: &str) {
    let doc = byte_doc(&frames(set, K));
    let (k, fitted_maps) = fitted(&TestDoc::Byte(doc.clone()), set, label);
    let mut out = doc;
    for (layer, m) in fitted_maps.iter().enumerate() {
        out = project(&out, &plane(layer, m, Some(k))).unwrap();
    }
    let (mut sum, mut count) = (0.0, 0usize);
    for (layer, m) in maps(set).iter().enumerate() {
        let back = affine_inverse(m);
        // The oracle's rect: destination pixel centres whose analytic
        // inverse lands in the layer's pixel-centre rect, its lens point too.
        let inside = |q: [f64; 2], margin: f64| {
            q[0] >= margin
                && q[1] >= margin
                && q[0] <= f64::from(SIZE.0) - margin
                && q[1] <= f64::from(SIZE.1) - margin
        };
        let (mut low, mut high) = ([i64::MAX; 2], [i64::MIN; 2]);
        for y in -100..600i64 {
            for x in -100..1100i64 {
                let p = [x as f64 + 0.5, y as f64 + 0.5];
                let u = apply(&back, p);
                if inside(u, 0.5) && inside(distort(u, K), 0.5) {
                    low = [low[0].min(x), low[1].min(y)];
                    high = [high[0].max(x + 1), high[1].max(y + 1)];
                }
            }
        }
        let want = [low[0], low[1], high[0] - low[0], high[1] - low[1]];
        let got = bounds(&out, layer);
        println!("{label} layer {layer}: content {got:?}, oracle {want:?}");
        assert!(
            got.iter().zip(want).all(|(g, w)| (g - w).abs() <= 1),
            "{label} layer {layer}: content {got:?} against the oracle's {want:?}"
        );
        let entry = &out.layers[layer];
        let (w, h) = entry.pixels.dimensions();
        let (x0, y0) = (f64::from(entry.offset.0), f64::from(entry.offset.1));
        let px = entry.pixels.read_rect(0, 0, w, h).unwrap();
        for (i, pixel) in px.iter().enumerate() {
            let p = [
                x0 + f64::from(i as u32 % w) + 0.5,
                y0 + f64::from(i as u32 / w) + 0.5,
            ];
            // Both the corrected point and the lens's source point are
            // clipped, so both must be well inside.
            let corrected = apply(&back, p);
            if !inside(corrected, 4.0) || !inside(distort(corrected, K), 4.0) {
                continue;
            }
            assert_eq!(pixel[3], 255, "{label} layer {layer} alpha at {p:?}");
            for c in 0..3 {
                let want = sample([O0[0] + p[0], O0[1] + p[1]], c) * 255.0;
                sum += (f64::from(pixel[c]) - want).abs();
                count += 1;
            }
        }
    }
    let mean = sum / count as f64;
    println!("{label}: mean |out - photo| {mean:.3} codes over {count}");
    assert!(count > 3 * 3 * 150_000, "{count} samples compared");
    assert!(mean <= 3.0, "{label}: mean {mean} codes");
}

#[test]
fn a_shift_set_fits_the_lens_at_8_16_and_float() {
    let frames = frames(&SHIFT, K);
    fitted(&TestDoc::Byte(byte_doc(&frames)), &SHIFT, "8-bit shift");
    fitted(&TestDoc::Typed(typed16(&frames)), &SHIFT, "16-bit shift");
    fitted(&TestDoc::Typed(typed32(&frames)), &SHIFT, "float shift");
}

#[test]
fn a_similarity_set_fits_the_lens_at_8_bits() {
    let frames = frames(&SIMILARITY, K);
    fitted(
        &TestDoc::Byte(byte_doc(&frames)),
        &SIMILARITY,
        "8-bit similarity",
    );
}

#[test]
fn the_shift_plane_is_the_photo() {
    planar_matches_the_photo(&SHIFT, "8-bit shift plane");
}

#[test]
fn the_similarity_plane_is_the_photo() {
    planar_matches_the_photo(&SIMILARITY, "8-bit similarity plane");
}

#[test]
fn an_undistorted_shift_fits_zero_and_the_identity_plane_changes_nothing() {
    let doc = byte_doc(&frames(&SHIFT, 0.0));
    let fitted = TestDoc::Byte(doc.clone()).fit(&request(&SHIFT)).unwrap();
    let coefficient = fitted["coefficient"].as_f64().unwrap();
    println!("undistorted shift: coefficient {coefficient:.5}");
    assert!(coefficient.abs() <= 0.002, "coefficient {coefficient}");

    let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let out = project(&doc, &plane(1, &identity, None)).unwrap();
    let (before, after) = (&doc.layers[1], &out.layers[1]);
    assert_eq!(before.offset, after.offset);
    assert_eq!(before.pixels.dimensions(), after.pixels.dimensions());
    let (w, h) = before.pixels.dimensions();
    assert_eq!(
        before.pixels.read_rect(0, 0, w, h).unwrap(),
        after.pixels.read_rect(0, 0, w, h).unwrap(),
        "the identity plane is byte-identical"
    );
}

#[test]
fn plane_refusals_name_the_reason() {
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(
        60,
        40,
        image::Rgba([90, 120, 150, 255]),
    ));
    let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let good = plane(0, &identity, Some(-0.05));
    assert!(project(&doc, &good).is_ok());
    for key in ["focal", "rotation", "centre", "origin"] {
        let mut mixed = good.clone();
        mixed[key] = json!(1);
        assert_eq!(
            project(&doc, &mixed).err().expect("the request refuses"),
            format!("invalid project layer request: unknown field `{key}`")
        );
    }
    let surface = json!({
        "op": "project_layer", "layer": 0, "projection": "spherical", "focal": 60.0,
        "rotation": identity.to_vec(), "centre": [30.0, 20.0], "origin": [30.5, 20.5],
        "map": identity.to_vec(),
    });
    assert_eq!(
        project(&doc, &surface).err().expect("the request refuses"),
        "invalid project layer request: unknown field `map`"
    );
    let invertible = "project layer map must be invertible with a positive determinant";
    assert_eq!(
        project(
            &doc,
            &plane(0, &[-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], None)
        )
        .err()
        .expect("the request refuses"),
        invertible,
        "a reflection's determinant is below 0"
    );
    assert_eq!(
        project(
            &doc,
            &plane(0, &[1.0, 2.0, 0.0, 2.0, 4.0, 0.0, 0.0, 0.0, 1.0], None)
        )
        .err()
        .expect("the request refuses"),
        invertible,
        "a singular map"
    );
    let mut short = good.clone();
    short["map"] = json!([1, 0, 0, 0, 1, 0, 0, 0]);
    assert_eq!(
        project(&doc, &short).err().expect("the request refuses"),
        "invalid project layer request: map must be 9 finite numbers"
    );
    assert_eq!(
        project(&doc, &plane(0, &identity, Some(-1.0)))
            .err()
            .expect("the request refuses"),
        "invalid project layer request: distortion folds the layer"
    );
}
