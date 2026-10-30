//! Edit ▸ Perspective Warp at 8 bits, the planes, the seams, the
//! extension, straightening and every refusal. `perspective_warp_depths.rs`
//! runs the pixel oracle at 16 bits and 32-bit float and checks the
//! selection's float form.
//!
//! The yardsticks are the test's own: a direct linear transform (an 8×8
//! solve) for each plane's homography, `puppet_warp.rs`'s Catmull-Rom
//! resample over `common/warp_oracle.rs`'s taps, exact geometry for a
//! translation, and the `perspective_warp::probe` seam for where the map
//! sends a point. No golden images.
#![allow(clippy::needless_range_loop)]

use crate::common;
use crate::puppet_warp::{byte_doc_with, byte_layer, finish, resample, visible, Samples, TestDoc};

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use common::selected_pixels::take_err;
use doctored_core::doc::DrDocument;
use doctored_core::doc_lock::{LOCK_PIXELS, LOCK_POSITION};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_width, dr_string_free};
use doctored_core::ffi_perspective_warp::*;
use doctored_core::ffi_selected_pixels::dr_doc_selected_pixels_edit;
use doctored_core::perspective_warp::probe;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use image::GrayImage;
use serde_json::{json, Value};

pub const LIMIT: u64 = REFERENCE_WORKING_BYTES;
pub const CANVAS: (u32, u32) = (140, 120);
pub const OFFSET: (i32, i32) = (10, 8);
pub const SIZE: (u32, u32) = (120, 100);

// ---------------------------------------------------------------- fixtures --

/// A textured layer in byte codes, layer pixels: every channel varies and
/// the alpha is graded, so a premultiplied mistake shows.
pub fn texture(x: u32, y: u32) -> [u8; 4] {
    [
        (x * 29 + 13) as u8,
        (y * 41 + 7) as u8,
        ((x + y) * 17 + 101) as u8,
        (160 + (x * 7 + y * 3) % 96) as u8,
    ]
}

pub fn byte_doc() -> DrDocument {
    byte_doc_with(CANVAS, OFFSET, byte_layer(SIZE, texture))
}

/// The keystone fixture: one quad over most of the layer.
pub const KEY_FROM: [[f64; 2]; 4] = [[30.0, 30.0], [90.0, 28.0], [92.0, 80.0], [28.0, 84.0]];
pub const KEY_TO: [[f64; 2]; 4] = [[35.0, 32.0], [85.0, 30.0], [100.0, 88.0], [20.0, 90.0]];

pub fn flat(q: [[f64; 2]; 4]) -> Vec<f64> {
    q.iter().flatten().copied().collect()
}

pub fn quad(from: [[f64; 2]; 4], to: [[f64; 2]; 4]) -> Value {
    json!({"from": flat(from), "to": flat(to)})
}

fn string(text: *mut c_char) -> String {
    let out = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(text) };
    out
}

pub fn plan_at(doc: &TestDoc, request: &Value, limit: u64) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        match doc {
            TestDoc::Byte(d) => dr_doc_perspective_warp_plan(d, text.as_ptr(), limit, 0, &mut err),
            TestDoc::Typed(d) => {
                dr_precision_doc_perspective_warp_plan(d, text.as_ptr(), limit, 0, &mut err)
            }
        }
    };
    if let Some(message) = take_err(err) {
        assert!(out.is_null());
        return Err(message);
    }
    Ok(serde_json::from_str(&string(out)).unwrap())
}

pub fn warp(doc: &TestDoc, request: &Value) -> Result<Option<TestDoc>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        match doc {
            TestDoc::Byte(d) => {
                dr_doc_perspective_warp(d, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
                    .cast::<u8>()
            }
            TestDoc::Typed(d) => {
                dr_precision_doc_perspective_warp(d, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
                    .cast::<u8>()
            }
        }
    };
    if let Some(message) = take_err(err) {
        assert!(out.is_null());
        return Err(message);
    }
    if out.is_null() {
        return Ok(None);
    }
    Ok(Some(unsafe {
        match doc {
            TestDoc::Byte(_) => TestDoc::Byte(*Box::from_raw(out.cast::<DrDocument>())),
            TestDoc::Typed(_) => TestDoc::Typed(*Box::from_raw(out.cast::<PrecisionDocument>())),
        }
    }))
}

pub fn layout(fields: &Value) -> Result<Value, String> {
    let text = CString::new(fields.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_perspective_warp_layout(text.as_ptr(), &mut err) };
    if let Some(message) = take_err(err) {
        assert!(out.is_null());
        return Err(message);
    }
    Ok(serde_json::from_str(&string(out)).unwrap())
}

/// `probe` of `points`, every one required to map.
pub fn sources(
    fields: &Value,
    subject: (i64, i64, u32, u32),
    points: &[[f64; 2]],
) -> Vec<(Option<usize>, [f64; 2])> {
    probe(&fields.to_string(), subject, points)
        .unwrap()
        .into_iter()
        .enumerate()
        .map(|(i, p)| p.unwrap_or_else(|| panic!("{:?} maps nowhere", points[i])))
        .collect()
}

// ------------------------------------------------------------------ oracle --

/// The test's own direct linear transform: the homography (h₈ = 1) taking
/// each `src[k]` to `dst[k]`, by Gaussian elimination with partial pivoting.
pub fn dlt(src: [[f64; 2]; 4], dst: [[f64; 2]; 4]) -> [f64; 9] {
    let mut m = [[0.0f64; 9]; 8];
    for k in 0..4 {
        let ([x, y], [u, v]) = (src[k], dst[k]);
        m[2 * k] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        m[2 * k + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }
    for c in 0..8 {
        let pivot = (c..8)
            .max_by(|&a, &b| m[a][c].abs().total_cmp(&m[b][c].abs()))
            .unwrap();
        m.swap(c, pivot);
        for r in 0..8 {
            if r != c {
                let f = m[r][c] / m[c][c];
                for k in c..9 {
                    m[r][k] -= f * m[c][k];
                }
            }
        }
    }
    let mut h = [1.0; 9];
    for r in 0..8 {
        h[r] = m[r][8] / m[r][r];
    }
    h
}

pub fn apply(h: &[f64; 9], p: [f64; 2]) -> [f64; 2] {
    let w = h[6] * p[0] + h[7] * p[1] + h[8];
    [
        (h[0] * p[0] + h[1] * p[1] + h[2]) / w,
        (h[3] * p[0] + h[4] * p[1] + h[5]) / w,
    ]
}

/// Signed distance from `p` to the convex quad's sides, positive inside.
pub fn depth_in(q: &[[f64; 2]; 4], p: [f64; 2]) -> f64 {
    let turn = {
        let (a, b, c) = (q[0], q[1], q[2]);
        ((b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0])).signum()
    };
    (0..4)
        .map(|k| {
            let (a, b) = (q[k], q[(k + 1) % 4]);
            let len = (b[0] - a[0]).hypot(b[1] - a[1]);
            turn * ((b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])) / len
        })
        .fold(f64::INFINITY, f64::min)
}

/// Every pixel of `got` whose centre `region` admits equals the test's own
/// resample of `source` through `inverse` within `tolerance`.
pub fn assert_plane(
    doc: &TestDoc,
    got: &Samples,
    source: &Samples,
    inverse: impl Fn([f64; 2]) -> [f64; 2],
    region: impl Fn([f64; 2]) -> bool,
    tolerance: f64,
    what: &str,
) {
    let range = doc.is_float().then(|| visible(source));
    let mut compared = 0;
    for y in 0..got.rect.3 {
        for x in 0..got.rect.2 {
            let (cx, cy) = (got.rect.0 + i64::from(x), got.rect.1 + i64::from(y));
            let p = [cx as f64 + 0.5, cy as f64 + 0.5];
            if !region(p) {
                continue;
            }
            let sums = resample(source, (cx, cy, 1, 1), |q| Some(inverse(q)));
            let want = finish(sums[0], doc.max(), range);
            let have = got.at(cx, cy).unwrap();
            for k in 0..4 {
                assert!(
                    (have[k] - want[k]).abs() <= tolerance,
                    "{what}: channel {k} at ({cx}, {cy}) is {} for {}",
                    have[k],
                    want[k]
                );
            }
            compared += 1;
        }
    }
    assert!(compared > 100, "{what}: only {compared} pixels compared");
}

/// One keystone quad: every pixel at least 1 px inside the warped quad is
/// the test's own homography resample.
pub fn check_keystone(doc: &TestDoc, tolerance: f64) {
    let request = json!({"op":"perspective_warp","layer":0,"quads":[quad(KEY_FROM, KEY_TO)]});
    let out = warp(doc, &request)
        .unwrap()
        .expect("a keystone changes the layer");
    let inverse = dlt(KEY_TO, KEY_FROM);
    assert_plane(
        doc,
        &out.layer(0),
        &doc.layer(0),
        |p| apply(&inverse, p),
        |p| depth_in(&KEY_TO, p) >= 1.0,
        tolerance,
        "keystone",
    );
}

/// Unmoved corners plan 0 scratch bytes and edit nothing, with no error.
pub fn check_identity(doc: &TestDoc) {
    let request = json!({"op":"perspective_warp","layer":0,"quads":[quad(KEY_FROM, KEY_FROM)]});
    let plan = plan_at(doc, &request, LIMIT).unwrap();
    assert_eq!(plan["scratch_bytes"], 0, "{plan}");
    assert!(warp(doc, &request).unwrap().is_none());
}

// ------------------------------------------------------------------- tests --

#[test]
fn unmoved_corners_change_nothing() {
    check_identity(&TestDoc::Byte(byte_doc()));
    let described = layout(&json!({"quads":[quad(KEY_FROM, KEY_FROM)]})).unwrap();
    assert_eq!(described["identity"], true);
}

#[test]
fn one_keystone_quad_is_the_homography_resample() {
    check_keystone(&TestDoc::Byte(byte_doc()), 1.0);
}

#[test]
fn the_extension_continues_the_plane_across_its_border() {
    let from = [[60.0, 50.0], [140.0, 50.0], [140.0, 110.0], [60.0, 110.0]];
    let to = [[64.0, 48.0], [136.0, 52.0], [146.0, 114.0], [56.0, 112.0]];
    let fields = json!({"quads":[quad(from, to)]});
    let inverse = dlt(to, from);
    let centre = [100.0, 81.5];
    let mut points = Vec::new();
    for k in 0..4 {
        let (a, b) = (to[k], to[(k + 1) % 4]);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let mut n = [(b[1] - a[1]) / len, -(b[0] - a[0]) / len];
        if (centre[0] - a[0]) * n[0] + (centre[1] - a[1]) * n[1] > 0.0 {
            n = [-n[0], -n[1]];
        }
        for i in 0..64 {
            let t = (f64::from(i) + 0.5) / 64.0;
            points.push([
                a[0] + t * (b[0] - a[0]) + 1e-3 * n[0],
                a[1] + t * (b[1] - a[1]) + 1e-3 * n[1],
            ]);
        }
    }
    for (p, (plane, source)) in points
        .iter()
        .zip(sources(&fields, (0, 0, 200, 160), &points))
    {
        assert_eq!(plane, None, "{p:?} lies outside the plane");
        let want = apply(&inverse, *p);
        let error = (source[0] - want[0]).hypot(source[1] - want[1]);
        assert!(
            error <= 1e-6,
            "{p:?}: source {source:?} is {error} px from {want:?}"
        );
    }
}

#[test]
fn two_quads_under_one_homography_are_one_plane() {
    let g = [1.02, 0.05, 3.0, -0.03, 0.98, 2.0, 0.0012, 0.0006, 1.0];
    let a = [[24.0, 24.0], [60.0, 24.0], [60.0, 86.0], [24.0, 86.0]];
    let b = [[60.0, 24.0], [100.0, 24.0], [100.0, 86.0], [60.0, 86.0]];
    let warped = |q: [[f64; 2]; 4]| q.map(|p| apply(&g, p));
    let request = json!({"op":"perspective_warp","layer":0,
        "quads":[quad(a, warped(a)), quad(b, warped(b))]});
    let described = layout(&json!({"quads": request["quads"]})).unwrap();
    assert_eq!(described["shared_edges"].as_array().unwrap().len(), 1);
    let doc = TestDoc::Byte(byte_doc());
    let out = warp(&doc, &request).unwrap().unwrap();
    let union = [a[0], b[1], b[2], a[3]];
    let inverse = dlt(warped(union), union);
    assert_plane(
        &doc,
        &out.layer(0),
        &doc.layer(0),
        |p| apply(&inverse, p),
        |p| depth_in(&warped(union), p) >= 1.0,
        1.0,
        "two quads",
    );
}

/// The objection's case: separate homographies would slide 10 px along the
/// shared edge; the affine seam keeps both sides on the same points.
#[test]
fn joined_planes_meet_without_a_gap_or_an_overlap() {
    let a_from = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
    let a_to = [[-50.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
    let b = [[100.0, 0.0], [200.0, 0.0], [200.0, 100.0], [100.0, 100.0]];
    let fields = json!({"quads":[quad(a_from, a_to), quad(b, b)]});
    let subject = (-100, -50, 400, 200);
    let eps = 1e-3;
    let mut points = Vec::new();
    for i in 0..64 {
        let y = (f64::from(i) + 0.5) / 64.0 * 100.0;
        points.push([100.0 - eps, y]);
        points.push([100.0 + eps, y]);
    }
    let mapped = sources(&fields, subject, &points);
    for pair in mapped.chunks(2) {
        let ((pa, sa), (pb, sb)) = (pair[0], pair[1]);
        assert_eq!((pa, pb), (Some(0), Some(1)), "each side's own plane");
        let slide = (sa[0] - sb[0]).hypot(sa[1] - sb[1]);
        assert!(
            slide <= 0.01,
            "the seam slides {slide} px ({sa:?} / {sb:?})"
        );
    }
    let mut lattice = Vec::new();
    let mut owner = Vec::new();
    for y in 0..50 {
        for x in 0..125 {
            let p = [-50.0 + f64::from(x) * 2.0 + 0.25, f64::from(y) * 2.0 + 0.25];
            let in_a = depth_in(&a_to, p) > 0.1;
            let in_b = depth_in(&b, p) > 0.1;
            if in_a || in_b {
                lattice.push(p);
                owner.push(if in_a { 0 } else { 1 });
            }
        }
    }
    assert!(lattice.len() > 1000);
    for ((p, (plane, _)), want) in lattice
        .iter()
        .zip(sources(&fields, subject, &lattice))
        .zip(owner)
    {
        assert_eq!(plane, Some(want), "{p:?}");
    }
}

#[test]
fn the_vanishing_guard_keeps_a_steep_plane_bounded() {
    let from = [
        [955.0, 955.0],
        [1055.0, 955.0],
        [1055.0, 1055.0],
        [955.0, 1055.0],
    ];
    let to = [
        [955.0, 955.0],
        [1055.0, 955.0],
        [1355.0, 1355.0],
        [955.0, 1055.0],
    ];
    let fields = json!({"quads":[quad(from, to)]});
    let centre = [1080.0, 1080.0];
    let c2 = to[2];
    let mut points = Vec::new();
    for degrees in [-30.0f64, -15.0, 0.0, 15.0, 30.0] {
        let base = (c2[1] - centre[1]).atan2(c2[0] - centre[0]) + degrees.to_radians();
        points.push([c2[0] + 20.0 * base.cos(), c2[1] + 20.0 * base.sin()]);
    }
    for (p, (_, source)) in points
        .iter()
        .zip(sources(&fields, (0, 0, 6000, 4000), &points))
    {
        let distance = (source[0] - from[2][0]).hypot(source[1] - from[2][1]);
        assert!(
            distance <= 155.0,
            "{p:?} maps to {source:?}, {distance} px from the rest corner"
        );
    }
}

#[test]
fn a_translated_quad_carries_the_whole_layer() {
    let size = (600, 400);
    let doc = TestDoc::Byte(byte_doc_with(size, (0, 0), byte_layer(size, texture)));
    let from = [
        [250.0, 150.0],
        [350.0, 150.0],
        [350.0, 250.0],
        [250.0, 250.0],
    ];
    let to = from.map(|p| [p[0] + 10.0, p[1]]);
    let request = json!({"op":"perspective_warp","layer":0,"quads":[quad(from, to)]});
    let out = warp(&doc, &request).unwrap().unwrap().layer(0);
    let before = doc.layer(0);
    let r = out.rect;
    assert!(
        r.0 <= 10 && r.1 <= 0 && r.0 + i64::from(r.2) >= 610 && r.1 + i64::from(r.3) >= 400,
        "the result {r:?} covers the layer rect moved by (10, 0)"
    );
    let mut far = 0;
    for y in r.1..r.1 + i64::from(r.3) {
        for x in r.0..r.0 + i64::from(r.2) {
            let have = out.at(x, y).unwrap();
            match before.at(x - 10, y) {
                Some(want) => {
                    assert_eq!(have, want, "({x}, {y})");
                    if (x - 300).abs().max((y - 200).abs()) >= 250 {
                        far += 1;
                    }
                }
                None => assert_eq!(have[3], 0.0, "({x}, {y}) lies outside the moved layer"),
            }
        }
    }
    assert!(far > 1000, "only {far} pixels far from the quad");
}

#[test]
fn straightening_makes_edges_exactly_level_and_plumb() {
    let from = [[10.0, 10.0], [60.0, 10.0], [60.0, 50.0], [10.0, 50.0]];
    let to = [[10.0, 10.0], [60.0, 14.0], [62.0, 52.0], [8.0, 50.0]];
    let to_of = |described: &Value, v: usize| {
        let p = &described["vertices"][v]["to"];
        [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()]
    };
    let level =
        layout(&json!({"quads":[quad(from, to)],"straighten":[{"quad":0,"edge":0}]})).unwrap();
    assert_eq!(level["straighten"][0]["direction"], "horizontal");
    assert_eq!(to_of(&level, 0), [10.0, 12.0]);
    assert_eq!(to_of(&level, 1), [60.0, 12.0]);
    let plumb = layout(&json!({"quads":[quad(from, to)],
        "straighten":[{"quad":0,"edge":1,"direction":"auto"}]}))
    .unwrap();
    assert_eq!(plumb["straighten"][0]["direction"], "vertical");
    assert_eq!(to_of(&plumb, 1), [61.0, 14.0]);
    assert_eq!(to_of(&plumb, 2), [61.0, 52.0]);
    // A chain: two quads' top edges share a vertex, so all three corners
    // take one y, the mean in vertex order, or the anchor's.
    let a = [[0.0, 0.0], [50.0, 0.0], [50.0, 40.0], [0.0, 40.0]];
    let b = [[50.0, 0.0], [100.0, 0.0], [100.0, 40.0], [50.0, 40.0]];
    let a_to = [[0.0, 2.0], [50.0, 5.0], [50.0, 40.0], [0.0, 40.0]];
    let b_to = [[50.0, 5.0], [100.0, 1.0], [100.0, 40.0], [50.0, 40.0]];
    let chain = json!({"quads":[quad(a, a_to), quad(b, b_to)],
        "straighten":[{"quad":0,"edge":0},{"quad":1,"edge":0,"direction":"near_horizontal"}]});
    let joined = layout(&chain).unwrap();
    let mean = (2.0 + 5.0 + 1.0) / 3.0;
    for v in [0, 1, 4] {
        assert_eq!(
            to_of(&joined, v)[1].to_bits(),
            f64::to_bits(mean),
            "vertex {v}"
        );
    }
    let mut anchored = chain.clone();
    anchored["anchor"] = json!([50.0, 5.0]);
    let kept = layout(&anchored).unwrap();
    for v in [0, 1, 4] {
        assert_eq!(to_of(&kept, v)[1].to_bits(), 5f64.to_bits(), "vertex {v}");
    }
    assert_eq!(to_of(&kept, 1), [50.0, 5.0], "the anchor stays bit for bit");
    // As drawn moves nothing; near_* off its axis is dropped.
    let slant = [[0.0, 0.0], [40.0, 30.0], [40.0, 70.0], [0.0, 40.0]];
    let square = [[0.0, 0.0], [40.0, 0.0], [40.0, 40.0], [0.0, 40.0]];
    let drawn =
        layout(&json!({"quads":[quad(square, slant)],"straighten":[{"quad":0,"edge":0}]})).unwrap();
    let angle = drawn["straighten"][0]["direction"].as_f64().unwrap();
    assert!((angle - 36.869_897_645_844_02).abs() < 1e-9, "{angle}");
    for (v, p) in slant.iter().enumerate() {
        assert_eq!(to_of(&drawn, v), *p);
    }
    // Straighten Both: a near_* entry off its axis leaves the edge to the next.
    let both = layout(&json!({"quads":[quad(from, to)],"straighten":[
        {"quad":0,"edge":0,"direction":"near_vertical"},
        {"quad":0,"edge":0,"direction":"near_horizontal"}]}))
    .unwrap();
    assert_eq!(
        both["straighten"],
        json!([{"quad":0,"edge":0,"direction":"horizontal"}])
    );
    for direction in ["near_horizontal", "near_vertical"] {
        let dropped = layout(&json!({"quads":[quad(square, slant)],
            "straighten":[{"quad":0,"edge":0,"direction":direction}]}))
        .unwrap();
        assert_eq!(dropped["straighten"], json!([]), "{direction}");
        for (v, p) in slant.iter().enumerate() {
            assert_eq!(to_of(&dropped, v), *p);
        }
    }
    // The straightened warp leaves no hole beside its plane.
    let fields = json!({"quads":[quad(from.map(|p| [p[0] + 30.0, p[1] + 30.0]),
        to.map(|p| [p[0] + 30.0, p[1] + 30.0]))],"straighten":[{"quad":0,"edge":0},{"quad":0,"edge":1}]});
    let warped = layout(&fields).unwrap();
    let corners: Vec<[f64; 2]> = (0..4).map(|v| to_of(&warped, v)).collect();
    let q = [corners[0], corners[1], corners[2], corners[3]];
    let mut points = Vec::new();
    for y in 0..120 {
        for x in 0..140 {
            let p = [f64::from(x) + 0.5, f64::from(y) + 0.5];
            let d = depth_in(&q, p);
            if (-20.0..0.0).contains(&d) {
                points.push(p);
            }
        }
    }
    assert!(points.len() > 1000);
    for (p, mapped) in points
        .iter()
        .zip(probe(&fields.to_string(), (0, 0, 140, 120), &points).unwrap())
    {
        assert!(mapped.is_some(), "{p:?} beside the plane maps nowhere");
    }
}

#[test]
fn a_linked_mask_warps_along_and_an_unlinked_mask_stays() {
    let gradient = |x: u32, y: u32| ((x * 5 + y * 3) % 256) as u8;
    for linked in [true, false] {
        let mut doc = byte_doc();
        let mask = Arc::new(GrayImage::from_fn(SIZE.0, SIZE.1, |x, y| {
            image::Luma([gradient(x, y)])
        }));
        let mut properties = MaskProperties::at(OFFSET);
        properties.linked = linked;
        properties.outside_coverage = 255;
        doc.layers[0].mask = Some(mask.clone());
        doc.layers[0].mask_properties = properties;
        let to = KEY_FROM.map(|p| [p[0] + 6.0, p[1] - 2.0]);
        let request = json!({"op":"perspective_warp","layer":0,"quads":[quad(KEY_FROM, to)]});
        let Some(TestDoc::Byte(out)) = warp(&TestDoc::Byte(doc), &request).unwrap() else {
            panic!("the warp changes the layer");
        };
        let layer = &out.layers[0];
        let after = layer.mask.as_ref().unwrap();
        if !linked {
            assert!(Arc::ptr_eq(after, &mask));
            assert_eq!(layer.mask_properties.offset, OFFSET);
            continue;
        }
        assert_eq!(layer.mask_properties.offset, layer.offset);
        let (w, h) = after.dimensions();
        let mut checked = 0;
        for y in 0..h {
            for x in 0..w {
                let (cx, cy) = (layer.offset.0 + x as i32, layer.offset.1 + y as i32);
                let (sx, sy) = (cx - 6 - OFFSET.0, cy + 2 - OFFSET.1);
                let inside = sx >= 2 && sy >= 2 && sx + 2 < SIZE.0 as i32 && sy + 2 < SIZE.1 as i32;
                if inside {
                    assert_eq!(
                        after.get_pixel(x, y).0[0],
                        gradient(sx as u32, sy as u32),
                        "mask at ({cx}, {cy})"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 1000);
    }
}

fn refusal(doc: &TestDoc, request: Value) -> String {
    let planned = plan_at(doc, &request, LIMIT).expect_err("the plan refuses");
    let edited = warp(doc, &request).err().expect("the edit refuses");
    assert_eq!(planned, edited);
    edited
}

/// Two joined quads whose homographies slide along the shared edge, with
/// the far corners pulled across so that the seam correction turns a
/// sub-triangle inside out.
pub fn folding_seam() -> Value {
    let a = [[0.0, 0.0], [100.0, 0.0], [100.0, 20.0], [0.0, 20.0]];
    let a_to = [[-26.0, -2.0], [86.0, 5.0], [100.0, 20.0], [0.0, 20.0]];
    let b = [[0.0, 20.0], [100.0, 20.0], [100.0, 100.0], [0.0, 100.0]];
    let b_to = [[0.0, 20.0], [100.0, 20.0], [72.0, 103.0], [18.0, 42.0]];
    json!([quad(a, a_to), quad(b, b_to)])
}

#[test]
fn every_refusal_is_named() {
    let doc = TestDoc::Byte(byte_doc());
    let good = quad(KEY_FROM, KEY_TO);
    let with = |quads: Value| json!({"op":"perspective_warp","layer":0,"quads":quads});
    let square = [[40.0, 40.0], [60.0, 40.0], [60.0, 60.0], [40.0, 60.0]];
    let bow = [[40.0, 40.0], [60.0, 60.0], [60.0, 40.0], [40.0, 60.0]];
    let mirrored = [[40.0, 40.0], [40.0, 60.0], [60.0, 60.0], [60.0, 40.0]];
    let line = [[40.0, 40.0], [50.0, 40.0], [60.0, 40.0], [50.0, 60.0]];
    let degenerate = |i: usize| {
        format!("Perspective Warp quad {i} is degenerate: its corners must form a convex quad with no three in a line.")
    };
    let folds = |i: usize| {
        format!("Perspective Warp quad {i} folds: its warped corners must form a convex quad with no three in a line, turning the same way.")
    };
    let coordinates = |i: usize| {
        format!(
            "Perspective Warp quad {i} needs from and to as eight finite numbers within 1000000."
        )
    };
    let straighten = |s: usize| {
        format!("Perspective Warp straighten {s} must name a quad, an edge from 0 to 3 and a direction.")
    };
    let mut cases: Vec<(Value, String)> = vec![
        (
            with(json!([])),
            "Perspective Warp needs at least one quad.".into(),
        ),
        (
            json!({"op":"perspective_warp","layer":0}),
            "Perspective Warp needs at least one quad.".into(),
        ),
        (
            with(json!(vec![good.clone(); 65])),
            "Perspective Warp takes at most 64 quads.".into(),
        ),
        (
            with(json!([good, {"from":[1.0,2.0],"to":[1.0,2.0]}])),
            coordinates(1),
        ),
        (
            with(json!([{"from": flat(square), "to": [2e6, 0, 0, 0, 0, 0, 0, 0]}])),
            coordinates(0),
        ),
        (
            with(json!([{"from": flat(square), "to": flat(square), "via": 1}])),
            coordinates(0),
        ),
        (with(json!([7])), coordinates(0)),
        (with(json!([quad(bow, bow)])), degenerate(0)),
        (with(json!([good, quad(line, line)])), degenerate(1)),
        (with(json!([quad(square, bow)])), folds(0)),
        (with(json!([quad(square, mirrored)])), folds(0)),
        (with(folding_seam()), folds(1)),
        (
            with(json!([
                quad(square, square),
                quad(
                    [[60.0, 40.0], [80.0, 40.0], [80.0, 60.0], [60.0, 60.0]],
                    [[61.0, 40.0], [80.0, 40.0], [80.0, 60.0], [60.0, 60.0]]
                )
            ])),
            "Perspective Warp quads 0 and 1 share a corner but warp it to different points.".into(),
        ),
        (
            with(json!([quad(
                square,
                [
                    [-500000.0, -500000.0],
                    [500000.0, -500000.0],
                    [500000.0, 500000.0],
                    [-500000.0, 500000.0]
                ]
            )])),
            "Perspective Warp would make the layer larger than the pixel limit.".into(),
        ),
    ];
    for (entry, s) in [
        (json!([{"quad":1,"edge":0}]), 0),
        (json!([{"quad":0,"edge":0},{"quad":0,"edge":4}]), 1),
        (json!([{"quad":0,"edge":0,"direction":"diagonal"}]), 0),
        (json!([{"quad":0,"edge":0,"direction":"up","x":1}]), 0),
        (json!([3]), 0),
    ] {
        let mut request = with(json!([quad(KEY_FROM, KEY_TO)]));
        request["straighten"] = entry;
        cases.push((request, straighten(s)));
    }
    let mut many = with(json!([quad(KEY_FROM, KEY_TO)]));
    many["straighten"] = json!(vec![json!({"quad":0,"edge":0}); 257]);
    cases.push((
        many,
        "Perspective Warp takes at most 256 straighten entries.".into(),
    ));
    for (request, want) in cases {
        assert_eq!(refusal(&doc, request.clone()), want, "{request}");
    }
    for anchor in [json!([1.0, 2.0]), json!("corner")] {
        assert_eq!(
            layout(&json!({"quads":[quad(KEY_FROM, KEY_TO)],"anchor":anchor})).unwrap_err(),
            "Perspective Warp anchor must be a quad corner."
        );
    }
    let request = with(json!([quad(KEY_FROM, KEY_TO)]));
    for (locks, which) in [(LOCK_PIXELS, "pixels"), (LOCK_POSITION, "position")] {
        let mut locked = byte_doc();
        locked.layers[0].locks = locks;
        assert_eq!(
            refusal(&TestDoc::Byte(locked), request.clone()),
            format!("layer 0 is locked ({which})")
        );
    }
    let mut described = byte_doc();
    described.layers[0].meta = Some(json!({"type":"text"}).to_string());
    assert_eq!(
        refusal(&TestDoc::Byte(described), request.clone()),
        "Rasterize layer 0 before Perspective Warp"
    );
    let mut out_of_range = request;
    out_of_range["layer"] = json!(3);
    assert_eq!(
        refusal(&doc, out_of_range),
        "Perspective Warp layer index is out of range"
    );
}

#[test]
fn the_float_form_refuses_an_empty_selection_by_name() {
    let doc = byte_doc();
    let request = json!({"op":"perspective_selected","layer":0,"quads":[quad(KEY_FROM, KEY_TO)]});
    let text = CString::new(request.to_string()).unwrap();
    let coverage = vec![0u16; (CANVAS.0 * CANVAS.1) as usize];
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_selected_pixels_edit(
            &doc,
            text.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(
        take_err(err).as_deref(),
        Some("The selection holds no pixels to perspective warp.")
    );
}

#[test]
fn a_plan_below_the_working_limit_refuses_with_the_admission_sentence() {
    let doc = TestDoc::Byte(byte_doc());
    let request = json!({"op":"perspective_warp","layer":0,"quads":[quad(KEY_FROM, KEY_TO)]});
    let error = plan_at(&doc, &request, 1000).expect_err("the warp is admitted first");
    assert!(
        error.starts_with("precision operation needs ")
            && error.ends_with("working limit is 1000 bytes"),
        "{error}"
    );
}

#[test]
fn the_preview_renders_the_proxy_through_the_warp() {
    let layer = byte_layer(SIZE, texture);
    let proxy = unsafe { dr_image_from_rgba(layer.as_raw().as_ptr(), SIZE.0, SIZE.1) };
    let render = |fields: &Value, coverage: &[u8]| {
        let text = CString::new(fields.to_string()).unwrap();
        let mut rect = [0.0f64; 4];
        let mut err = ptr::null_mut();
        let (cp, cl) = if coverage.is_empty() {
            (ptr::null(), 0)
        } else {
            (coverage.as_ptr(), coverage.len())
        };
        let image = unsafe {
            dr_perspective_warp_preview(
                text.as_ptr(),
                proxy,
                i64::from(OFFSET.0),
                i64::from(OFFSET.1),
                SIZE.0,
                SIZE.1,
                cp,
                cl,
                rect.as_mut_ptr(),
                &mut err,
            )
        };
        (image, rect, take_err(err))
    };
    for to in [KEY_TO, KEY_FROM] {
        let (image, rect, err) = render(&json!({"quads":[quad(KEY_FROM, to)]}), &[]);
        assert!(err.is_none(), "{err:?}");
        assert!(!image.is_null());
        assert!(rect[2] > 0.0 && rect[3] > 0.0);
        assert!(unsafe { dr_image_width(image) } > 0);
        unsafe { dr_image_free(image) };
    }
    let (refused, _, err) = render(&json!({"quads":[quad(KEY_FROM, KEY_TO)]}), &[0u8; 3]);
    assert!(refused.is_null());
    assert!(err.is_some());
    unsafe { dr_image_free(proxy) };
}

#[test]
fn every_export_tolerates_null() {
    let mut err = ptr::null_mut();
    unsafe {
        assert!(dr_perspective_warp_layout(ptr::null(), &mut err).is_null());
        assert!(dr_perspective_warp_preview(
            ptr::null(),
            ptr::null(),
            0,
            0,
            1,
            1,
            ptr::null(),
            0,
            ptr::null_mut(),
            &mut err
        )
        .is_null());
        assert!(
            dr_doc_perspective_warp_plan(ptr::null(), ptr::null(), LIMIT, 0, &mut err).is_null()
        );
        assert!(
            dr_doc_perspective_warp(ptr::null(), ptr::null(), LIMIT, 0, ptr::null(), &mut err)
                .is_null()
        );
        assert!(dr_precision_doc_perspective_warp_plan(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            &mut err
        )
        .is_null());
        assert!(dr_precision_doc_perspective_warp(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
    }
    assert!(err.is_null(), "a NULL handle reports no error");
    let doc = byte_doc();
    let mut err = ptr::null_mut();
    let none =
        unsafe { dr_doc_perspective_warp(&doc, ptr::null(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(none.is_null());
    assert_eq!(take_err(err).as_deref(), Some("request is NULL"));
}

/// The admission drive's joined case: two quads sharing x = 3000 whose
/// homographies disagree along it prepare on a 6000×4000 layer.
#[test]
fn a_large_joined_pair_with_a_sliding_seam_prepares() {
    let a = [
        [300.0, 300.0],
        [3000.0, 300.0],
        [3000.0, 3700.0],
        [300.0, 3700.0],
    ];
    let a_to = [
        [600.0, 500.0],
        [3000.0, 300.0],
        [3000.0, 3700.0],
        [300.0, 3700.0],
    ];
    let b = [
        [3000.0, 300.0],
        [5700.0, 300.0],
        [5700.0, 3700.0],
        [3000.0, 3700.0],
    ];
    let b_to = [
        [3000.0, 300.0],
        [5700.0, 300.0],
        [5400.0, 3500.0],
        [3000.0, 3700.0],
    ];
    let fields = json!({"quads":[quad(a, a_to), quad(b, b_to)]});
    let points = [[1500.0, 2000.0], [4500.0, 2000.0]];
    let mapped = sources(&fields, (0, 0, 6000, 4000), &points);
    assert_eq!(mapped[0].0, Some(0));
    assert_eq!(mapped[1].0, Some(1));
}
