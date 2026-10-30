//! Edit ▸ Puppet Warp at 8 bits, the shared oracle, the mesh, the
//! solve and every refusal. `puppet_warp_depths.rs` runs the same oracles at
//! 16 bits and 32-bit float and checks the selection's float form.
//!
//! The yardsticks are the test's own: the Catmull-Rom taps and weights of
//! `common/warp_oracle.rs`, a premultiplied binary64 accumulation, its own
//! point location over the meshes `dr_puppet_mesh_describe` and
//! `dr_puppet_pose_describe` report, its own polar decomposition for the
//! rigidity energy, and exact geometry (a translation, a rotation) for the
//! poses. No golden images.
#![allow(clippy::needless_range_loop)]

use crate::common;
use crate::warp_oracle::taps;

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use common::selected_pixels::take_err;
use doctored_core::doc::{DrDocument, LayerRaster};
use doctored_core::doc_lock::{LOCK_PIXELS, LOCK_POSITION};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_width, dr_string_free};
use doctored_core::ffi_puppet::*;
use doctored_core::ffi_selected_pixels::dr_doc_selected_pixels_edit;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::puppet_arap::PuppetPose;
use doctored_core::puppet_mesh::PuppetMesh;
use image::{GrayImage, RgbaImage};
use serde_json::{json, Value};

pub const LIMIT: u64 = REFERENCE_WORKING_BYTES;
pub const CANVAS: (u32, u32) = (72, 64);
pub const OFFSET: (i32, i32) = (6, 5);
pub const SIZE: (u32, u32) = (44, 36);

// ---------------------------------------------------------------- fixtures --

/// The ellipse fixture in byte codes, layer pixels: opaque inside, a graded
/// rim, and LOUD hidden RGB under alpha 0 in a transparent margin of at least
/// six pixels.
pub fn ellipse(x: u32, y: u32) -> [u8; 4] {
    let (fx, fy) = (f64::from(x) + 0.5 - 22.0, f64::from(y) + 0.5 - 18.0);
    let r = ((fx / 15.0).powi(2) + (fy / 11.0).powi(2)).sqrt();
    let a = if r <= 0.9 {
        255
    } else if r < 1.0 {
        ((1.0 - r) / 0.1 * 255.0) as u8
    } else {
        0
    };
    [
        (x * 29 + 13) as u8,
        (y * 41 + 7) as u8,
        ((x + y) * 17 + 101) as u8,
        a,
    ]
}

pub fn byte_layer(size: (u32, u32), f: impl Fn(u32, u32) -> [u8; 4]) -> RgbaImage {
    RgbaImage::from_fn(size.0, size.1, |x, y| image::Rgba(f(x, y)))
}

pub fn byte_doc_with(canvas: (u32, u32), offset: (i32, i32), layer: RgbaImage) -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::new(canvas.0, canvas.1));
    doc.layers[0].pixels = LayerRaster::new(layer);
    doc.layers[0].offset = offset;
    doc
}

pub fn byte_doc() -> DrDocument {
    byte_doc_with(CANVAS, OFFSET, byte_layer(SIZE, ellipse))
}

/// A document at any depth, for the shared checks.
pub enum TestDoc {
    Byte(DrDocument),
    Typed(PrecisionDocument),
}

/// A layer's rect and its straight samples in storage units (byte codes,
/// u16 codes, or f32 values).
pub struct Samples {
    pub rect: (i64, i64, u32, u32),
    pub px: Vec<[f64; 4]>,
}

impl Samples {
    pub fn at(&self, cx: i64, cy: i64) -> Option<[f64; 4]> {
        let (x, y) = (cx - self.rect.0, cy - self.rect.1);
        if x < 0 || y < 0 || x >= i64::from(self.rect.2) || y >= i64::from(self.rect.3) {
            return None;
        }
        Some(self.px[y as usize * self.rect.2 as usize + x as usize])
    }
}

fn string(text: *mut c_char) -> String {
    let out = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(text) };
    out
}

impl TestDoc {
    pub fn layer(&self, index: usize) -> Samples {
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
                    image
                        .to_rgba32()
                        .unwrap()
                        .iter()
                        .map(|p| p.map(f64::from))
                        .collect()
                } else {
                    image
                        .to_rgba16()
                        .unwrap()
                        .iter()
                        .map(|p| p.map(f64::from))
                        .collect()
                };
                Samples {
                    rect: (i64::from(x), i64::from(y), w, h),
                    px,
                }
            }
        }
    }

    /// The storage maximum (1 at float).
    pub fn max(&self) -> f64 {
        match self {
            TestDoc::Byte(_) => 255.0,
            TestDoc::Typed(d) if d.is_float() => 1.0,
            TestDoc::Typed(_) => 65535.0,
        }
    }

    pub fn is_float(&self) -> bool {
        matches!(self, TestDoc::Typed(d) if d.is_float())
    }

    pub fn mesh_at(
        &self,
        layer: usize,
        options: &Value,
        coverage: Option<&[u16]>,
        limit: u64,
    ) -> Result<*mut PuppetMesh, String> {
        let text = CString::new(options.to_string()).unwrap();
        let (cp, cl) = coverage.map_or((ptr::null(), 0), |c| (c.as_ptr(), c.len()));
        let mut err = ptr::null_mut();
        let out = unsafe {
            match self {
                TestDoc::Byte(d) => {
                    dr_doc_puppet_mesh(d, layer, cp, cl, text.as_ptr(), limit, 0, &mut err)
                }
                TestDoc::Typed(d) => dr_precision_doc_puppet_mesh(
                    d,
                    layer,
                    cp,
                    cl,
                    text.as_ptr(),
                    limit,
                    0,
                    &mut err,
                ),
            }
        };
        if let Some(message) = take_err(err) {
            assert!(out.is_null());
            return Err(message);
        }
        assert!(!out.is_null());
        Ok(out)
    }

    pub fn mesh(&self, options: &Value) -> *mut PuppetMesh {
        self.mesh_at(0, options, None, LIMIT).unwrap()
    }

    pub fn plan(&self, request: &Value) -> Result<Value, String> {
        let text = CString::new(request.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            match self {
                TestDoc::Byte(d) => dr_doc_puppet_warp_plan(d, text.as_ptr(), LIMIT, 0, &mut err),
                TestDoc::Typed(d) => {
                    dr_precision_doc_puppet_warp_plan(d, text.as_ptr(), LIMIT, 0, &mut err)
                }
            }
        };
        if let Some(message) = take_err(err) {
            assert!(out.is_null());
            return Err(message);
        }
        Ok(serde_json::from_str(&string(out)).unwrap())
    }

    pub fn warp(&self, request: &Value) -> Result<Option<TestDoc>, String> {
        let text = CString::new(request.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            match self {
                TestDoc::Byte(d) => {
                    dr_doc_puppet_warp(d, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
                        .cast::<u8>()
                }
                TestDoc::Typed(d) => {
                    dr_precision_doc_puppet_warp(d, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
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
            match self {
                TestDoc::Byte(_) => TestDoc::Byte(*Box::from_raw(out.cast::<DrDocument>())),
                TestDoc::Typed(_) => {
                    TestDoc::Typed(*Box::from_raw(out.cast::<PrecisionDocument>()))
                }
            }
        }))
    }
}

// ------------------------------------------------------------ mesh & pose --

/// A described mesh and (optionally) a described pose.
pub struct Described {
    pub rest: Vec<[f64; 2]>,
    pub triangles: Vec<[usize; 3]>,
    pub positions: Vec<[f64; 2]>,
    pub pins: Vec<(usize, [f64; 2])>,
}

pub fn describe_mesh(mesh: *const PuppetMesh) -> Described {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_puppet_mesh_describe(mesh, &mut err) };
    assert!(take_err(err).is_none());
    let value: Value = serde_json::from_str(&string(text)).unwrap();
    let rest: Vec<[f64; 2]> = value["vertices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| [v[0].as_f64().unwrap(), v[1].as_f64().unwrap()])
        .collect();
    let triangles = value["triangles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| [0, 1, 2].map(|k| t[k].as_u64().unwrap() as usize))
        .collect();
    Described {
        positions: rest.clone(),
        rest,
        triangles,
        pins: Vec::new(),
    }
}

pub fn solve(mesh: *const PuppetMesh, pins: &Value) -> Result<*mut PuppetPose, String> {
    let text = CString::new(pins.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let pose = unsafe { dr_puppet_mesh_solve(mesh, text.as_ptr(), &mut err) };
    if let Some(message) = take_err(err) {
        assert!(pose.is_null());
        return Err(message);
    }
    Ok(pose)
}

pub fn describe_pose(mesh: *const PuppetMesh, pose: *const PuppetPose) -> Described {
    let mut out = describe_mesh(mesh);
    let mut err = ptr::null_mut();
    let text = unsafe { dr_puppet_pose_describe(pose, &mut err) };
    assert!(take_err(err).is_none());
    let value: Value = serde_json::from_str(&string(text)).unwrap();
    let flat: Vec<f64> = value["positions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    out.positions = flat.chunks_exact(2).map(|c| [c[0], c[1]]).collect();
    out.pins = value["pins"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["vertex"].as_u64().unwrap() as usize,
                [
                    p["from"][0].as_f64().unwrap(),
                    p["from"][1].as_f64().unwrap(),
                ],
            )
        })
        .collect();
    out
}

pub fn free_mesh(mesh: *mut PuppetMesh) {
    unsafe { dr_puppet_mesh_free(mesh) };
}

pub fn free_pose(pose: *mut PuppetPose) {
    unsafe { dr_puppet_pose_free(pose) };
}

/// The barycentric coordinates of `p` in triangle `t`, `None` when degenerate.
fn barycentric(t: [[f64; 2]; 3], p: [f64; 2]) -> Option<[f64; 3]> {
    let (a, b, c) = (t[0], t[1], t[2]);
    let det = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
    if det == 0.0 {
        return None;
    }
    let l1 = ((p[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (p[1] - a[1])) / det;
    let l2 = ((b[0] - a[0]) * (p[1] - a[1]) - (p[0] - a[0]) * (b[1] - a[1])) / det;
    Some([1.0 - l1 - l2, l1, l2])
}

impl Described {
    pub fn rest_contains(&self, p: [f64; 2]) -> bool {
        self.triangles.iter().any(|t| {
            barycentric(t.map(|v| self.rest[v]), p).is_some_and(|l| l.iter().all(|&v| v >= -1e-12))
        })
    }

    /// The test's own inverse map: the topmost deformed triangle (the last
    /// in `order`) holding `p`, its barycentric coordinates applied to the
    /// rest triangle.
    pub fn source(&self, order: &[usize], p: [f64; 2]) -> Option<[f64; 2]> {
        for &t in order.iter().rev() {
            let tri = self.triangles[t];
            let Some(l) = barycentric(tri.map(|v| self.positions[v]), p) else {
                continue;
            };
            if l.iter().all(|&v| v >= -1e-9) {
                let r = tri.map(|v| self.rest[v]);
                return Some([
                    l[0] * r[0][0] + l[1] * r[1][0] + l[2] * r[2][0],
                    l[0] * r[0][1] + l[1] * r[1][1] + l[2] * r[2][1],
                ]);
            }
        }
        None
    }

    /// `Σ A_t ‖J_t − R_t‖²_F` with `R_t` the rotation nearest `J_t`.
    pub fn rigid_energy(&self) -> f64 {
        let mut total = 0.0;
        for t in &self.triangles {
            let r = t.map(|v| self.rest[v]);
            let x = t.map(|v| self.positions[v]);
            let e = [
                [r[1][0] - r[0][0], r[2][0] - r[0][0]],
                [r[1][1] - r[0][1], r[2][1] - r[0][1]],
            ];
            let d = [
                [x[1][0] - x[0][0], x[2][0] - x[0][0]],
                [x[1][1] - x[0][1], x[2][1] - x[0][1]],
            ];
            let det = e[0][0] * e[1][1] - e[0][1] * e[1][0];
            let inv = [
                [e[1][1] / det, -e[0][1] / det],
                [-e[1][0] / det, e[0][0] / det],
            ];
            let mut j = [[0.0; 2]; 2];
            for row in 0..2 {
                for col in 0..2 {
                    j[row][col] = d[row][0] * inv[0][col] + d[row][1] * inv[1][col];
                }
            }
            let phi = (j[1][0] - j[0][1]).atan2(j[0][0] + j[1][1]);
            let rot = [[phi.cos(), -phi.sin()], [phi.sin(), phi.cos()]];
            let mut norm = 0.0;
            for row in 0..2 {
                for col in 0..2 {
                    norm += (j[row][col] - rot[row][col]).powi(2);
                }
            }
            total += det.abs() / 2.0 * norm;
        }
        total
    }
}

// ----------------------------------------------------------------- oracle --

/// The test's own premultiplied Catmull-Rom resample of `source` into
/// `dest` through `map` (canvas point → canvas source point): the binary64
/// sums `[Σw·a·r, Σw·a·g, Σw·a·b, Σw·a]` per destination pixel.
pub fn resample(
    source: &Samples,
    dest: (i64, i64, u32, u32),
    map: impl Fn([f64; 2]) -> Option<[f64; 2]>,
) -> Vec<[f64; 4]> {
    let mut out = Vec::new();
    for y in 0..dest.3 {
        for x in 0..dest.2 {
            let p = [
                dest.0 as f64 + f64::from(x) + 0.5,
                dest.1 as f64 + f64::from(y) + 0.5,
            ];
            let mut acc = [0.0; 4];
            if let Some(q) = map(p) {
                let xt = taps(q[0] - source.rect.0 as f64, true);
                let yt = taps(q[1] - source.rect.1 as f64, true);
                for (sy, wy) in &yt {
                    for (sx, wx) in &xt {
                        let Some(px) = source.at(source.rect.0 + sx, source.rect.1 + sy) else {
                            continue;
                        };
                        let w = wy * wx;
                        for k in 0..3 {
                            acc[k] += w * px[3] * px[k];
                        }
                        acc[3] += w * px[3];
                    }
                }
            }
            out.push(acc);
        }
    }
    out
}

/// The visible RGB range of `source` (D3), per channel over alpha > 0.
pub fn visible(source: &Samples) -> ([f64; 3], [f64; 3]) {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for p in source.px.iter().filter(|p| p[3] > 0.0) {
        for k in 0..3 {
            low[k] = low[k].min(p[k]);
            high[k] = high[k].max(p[k]);
        }
    }
    (low, high)
}

/// The oracle's finish: UNORM codes (`max` 255 or 65535) rounded once, or
/// at float D2/D3.
pub fn finish(acc: [f64; 4], max: f64, range: Option<([f64; 3], [f64; 3])>) -> [f64; 4] {
    match range {
        None => {
            let a = acc[3].clamp(0.0, max).round();
            if a == 0.0 {
                return [0.0; 4];
            }
            let c = |k: usize| (acc[k] / acc[3]).clamp(0.0, max).round();
            [c(0), c(1), c(2), a]
        }
        Some((low, high)) => {
            if acc[3] < 1.0 / 16_777_216.0 {
                return [0.0; 4];
            }
            let c = |k: usize| (acc[k] / acc[3]).clamp(low[k], high[k]);
            [c(0), c(1), c(2), acc[3].min(1.0)]
        }
    }
}

/// Every pixel of `got` (a layer) equals the oracle `want` over its rect
/// within `tolerance`, and the oracle puts nothing within three pixels
/// outside it.
pub fn assert_matches(
    doc: &TestDoc,
    got: &Samples,
    source: &Samples,
    map: impl Fn([f64; 2]) -> Option<[f64; 2]>,
    tolerance: f64,
    what: &str,
) {
    let range = doc.is_float().then(|| visible(source));
    let grown = (
        got.rect.0 - 3,
        got.rect.1 - 3,
        got.rect.2 + 6,
        got.rect.3 + 6,
    );
    let sums = resample(source, grown, &map);
    let mut compared = 0;
    for y in 0..grown.3 {
        for x in 0..grown.2 {
            let (cx, cy) = (grown.0 + i64::from(x), grown.1 + i64::from(y));
            let want = finish(
                sums[y as usize * grown.2 as usize + x as usize],
                doc.max(),
                range,
            );
            let Some(have) = got.at(cx, cy) else {
                assert!(
                    want[3] <= tolerance,
                    "{what}: the oracle puts alpha {} at ({cx}, {cy}) outside the result {:?}",
                    want[3],
                    got.rect
                );
                continue;
            };
            assert!(
                (have[3] - want[3]).abs() <= tolerance,
                "{what}: alpha at ({cx}, {cy}) is {} for {}",
                have[3],
                want[3]
            );
            let floor = if range.is_some() { 1e-4 } else { 0.0 };
            if have[3] > floor && want[3] > floor {
                for k in 0..3 {
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
    }
    assert!(
        compared > 100,
        "{what}: only {compared} visible pixels compared"
    );
}

/// Puppet Warp of the ellipse fixture with every pin translated by
/// (7.25, −3.5) equals the oracle's affine resample.
pub fn check_translation(doc: &TestDoc, tolerance: f64) {
    let d = [7.25, -3.5];
    let centre = [f64::from(OFFSET.0) + 22.0, f64::from(OFFSET.1) + 18.0];
    let pins: Vec<Value> = [
        [0.0, 0.0],
        [-10.0, 0.0],
        [10.0, 0.0],
        [0.0, -7.0],
        [0.0, 7.0],
    ]
    .iter()
    .map(|o| {
        let from = [centre[0] + o[0], centre[1] + o[1]];
        json!({"from": from, "to": [from[0] + d[0], from[1] + d[1]]})
    })
    .collect();
    for mode in ["rigid", "normal", "distort"] {
        let request = json!({"op":"puppet_warp","layer":0,"mode":mode,"expansion":4,"pins":pins});
        let out = doc
            .warp(&request)
            .unwrap()
            .expect("a translation changes the layer");
        let source = doc.layer(0);
        let got = out.layer(0);
        assert_matches(
            doc,
            &got,
            &source,
            |p| Some([p[0] - d[0], p[1] - d[1]]),
            tolerance,
            &format!("translation ({mode})"),
        );
    }
}

/// Pins at mesh vertices rotated 30° about the fixture's centre: (a) the
/// pose is the exact rotation within 1e-3 px, pins bit for bit; (b) the
/// pixels equal the oracle's resample through the pose's own map.
pub fn check_rotation(doc: &TestDoc, tolerance: f64) {
    let options = json!({"density":"normal","expansion":4});
    let mesh = doc.mesh(&options);
    let described = describe_mesh(mesh);
    let c = [f64::from(OFFSET.0) + 22.0, f64::from(OFFSET.1) + 18.0];
    let (sin, cos) = 30f64.to_radians().sin_cos();
    let rotate = |v: [f64; 2]| {
        let (x, y) = (v[0] - c[0], v[1] - c[1]);
        [c[0] + cos * x - sin * y, c[1] + sin * x + cos * y]
    };
    let mut chosen: Vec<usize> = Vec::new();
    for o in [
        [-12.0, 0.0],
        [12.0, 0.0],
        [0.0, -9.0],
        [0.0, 9.0],
        [-8.0, -6.0],
        [8.0, 6.0],
    ] {
        let target = [c[0] + o[0], c[1] + o[1]];
        let nearest = (0..described.rest.len())
            .min_by(|&a, &b| {
                let da = (described.rest[a][0] - target[0]).powi(2)
                    + (described.rest[a][1] - target[1]).powi(2);
                let db = (described.rest[b][0] - target[0]).powi(2)
                    + (described.rest[b][1] - target[1]).powi(2);
                da.total_cmp(&db)
            })
            .unwrap();
        if !chosen.contains(&nearest) {
            chosen.push(nearest);
        }
    }
    let pins: Vec<Value> = chosen
        .iter()
        .map(|&v| json!({"from": described.rest[v], "to": rotate(described.rest[v])}))
        .collect();
    let pose = solve(mesh, &json!({"mode":"normal","pins":pins})).unwrap();
    let posed = describe_pose(mesh, pose);
    for (v, rest) in described.rest.iter().enumerate() {
        let want = rotate(*rest);
        let got = posed.positions[v];
        if chosen.contains(&v) {
            assert_eq!(
                got.map(f64::to_bits),
                want.map(f64::to_bits),
                "pinned vertex {v} lands on its target bit for bit"
            );
        } else {
            let error = (got[0] - want[0]).abs().max((got[1] - want[1]).abs());
            assert!(
                error < 1e-3,
                "vertex {v} is {error} px from the exact rotation"
            );
        }
    }
    free_pose(pose);
    free_mesh(mesh);
    let request = json!({"op":"puppet_warp","layer":0,"mode":"normal","expansion":4,"pins":pins});
    let out = doc
        .warp(&request)
        .unwrap()
        .expect("a rotation changes the layer");
    let order: Vec<usize> = (0..posed.triangles.len()).collect();
    assert_matches(
        doc,
        &out.layer(0),
        &doc.layer(0),
        |p| posed.source(&order, p),
        tolerance,
        "rotation",
    );
}

/// Unmoved pins plan 0 scratch bytes and edit nothing, with no error.
pub fn check_identity(doc: &TestDoc) {
    let request = json!({"op":"puppet_warp","layer":0,"pins":[
        {"from":[28.0,23.0],"to":[28.0,23.0]},
        {"from":[20.5,20.25],"to":[20.5,20.25],"rotate":0}]});
    let plan = doc.plan(&request).unwrap();
    assert_eq!(plan["scratch_bytes"], 0, "{plan}");
    assert!(doc.warp(&request).unwrap().is_none());
}

// ------------------------------------------------------------------ tests --

#[test]
fn unmoved_pins_change_nothing() {
    check_identity(&TestDoc::Byte(byte_doc()));
}

#[test]
fn a_translation_of_every_pin_is_the_affine_resample() {
    check_translation(&TestDoc::Byte(byte_doc()), 1.0);
}

#[test]
fn a_rotation_of_every_pin_rotates_the_mesh_and_the_pixels() {
    check_rotation(&TestDoc::Byte(byte_doc()), 1.0);
}

#[test]
fn rigid_deforms_less_than_normal_and_normal_less_than_distort() {
    let doc = TestDoc::Byte(byte_doc_with(
        (80, 40),
        (16, 12),
        byte_layer((48, 16), |_, _| [90, 140, 200, 255]),
    ));
    let mesh = doc.mesh(&json!({"expansion":0}));
    let rest = describe_mesh(mesh).rest;
    let left = rest
        .iter()
        .position(|v| *v == [16.0, 20.0])
        .expect("a vertex at the left middle");
    let right = rest
        .iter()
        .position(|v| *v == [64.0, 20.0])
        .expect("a vertex at the right middle");
    let pins = json!([
        {"from": rest[left], "to": [4.0, 20.0]},
        {"from": rest[right], "to": [76.0, 20.0]},
    ]);
    let energy = |mode: &str| {
        let pose = solve(mesh, &json!({"mode": mode, "pins": pins})).unwrap();
        let posed = describe_pose(mesh, pose);
        assert_eq!(posed.positions[left], [4.0, 20.0]);
        assert_eq!(posed.positions[right], [76.0, 20.0]);
        free_pose(pose);
        posed.rigid_energy()
    };
    let (rigid, normal, distort) = (energy("rigid"), energy("normal"), energy("distort"));
    assert!(
        rigid < normal && normal < distort,
        "rigid {rigid}, normal {normal}, distort {distort}"
    );
    free_mesh(mesh);
}

/// A fixture of scattered pixels, a blob and a one-pixel line.
fn scattered(x: u32, y: u32) -> [u8; 4] {
    let blob = (f64::from(x) - 30.0).powi(2) + (f64::from(y) - 20.0).powi(2) < 49.0;
    let line = y == 34 && (8..50).contains(&x);
    let dot = (x * 7 + y * 13).is_multiple_of(97);
    let a = if blob || line {
        255
    } else if dot {
        (x * 5 + 1) as u8
    } else {
        0
    };
    [x as u8, y as u8, 7, a]
}

#[test]
fn the_mesh_covers_every_opaque_pixel_at_each_expansion() {
    let size = (60, 44);
    let doc = TestDoc::Byte(byte_doc_with(
        (90, 70),
        (12, 9),
        byte_layer(size, scattered),
    ));
    let opaque: Vec<[f64; 2]> = (0..size.1)
        .flat_map(|y| (0..size.0).map(move |x| (x, y)))
        .filter(|&(x, y)| scattered(x, y)[3] > 0)
        .map(|(x, y)| [12.0 + f64::from(x) + 0.5, 9.0 + f64::from(y) + 0.5])
        .collect();
    for e in [0i32, 2, 8] {
        let mesh = doc.mesh(&json!({"expansion": e}));
        let described = describe_mesh(mesh);
        free_mesh(mesh);
        assert!(described.rest.len() <= 3000);
        for p in &opaque {
            assert!(
                described.rest_contains(*p),
                "e {e}: opaque centre {p:?} is outside the mesh"
            );
        }
        if e < 2 {
            continue;
        }
        let reach = f64::from(e - 1);
        for y in -12..(size.1 as i32 + 12) {
            for x in -12..(size.0 as i32 + 12) {
                let p = [12.0 + f64::from(x) + 0.5, 9.0 + f64::from(y) + 0.5];
                let near = opaque
                    .iter()
                    .any(|q| (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) <= reach * reach);
                if near {
                    assert!(
                        described.rest_contains(p),
                        "e {e}: {p:?} lies within {reach} px of an opaque centre"
                    );
                }
            }
        }
    }
}

#[test]
fn a_higher_depth_pins_region_draws_over_an_overlap() {
    let blobs = |x: u32, y: u32| {
        if (12..28).contains(&y) && (4..20).contains(&x) {
            [255, 0, 0, 255]
        } else if (12..28).contains(&y) && (48..64).contains(&x) {
            [0, 0, 255, 255]
        } else {
            [0, 0, 0, 0]
        }
    };
    let doc = TestDoc::Byte(byte_doc_with((80, 40), (0, 0), byte_layer((80, 40), blobs)));
    for (red, blue, want) in [(5, 0, [255.0, 0.0, 0.0]), (-3, 2, [0.0, 0.0, 255.0])] {
        let request = json!({"op":"puppet_warp","layer":0,"mode":"rigid","pins":[
            {"from":[12.0,20.0],"to":[56.0,20.0],"depth":red},
            {"from":[56.0,20.0],"to":[56.0,20.0],"depth":blue}]});
        let out = doc.warp(&request).unwrap().unwrap().layer(0);
        let got = out.at(56, 20).unwrap();
        assert_eq!(
            [got[0], got[1], got[2]],
            want,
            "red depth {red}, blue depth {blue}"
        );
        assert_eq!(got[3], 255.0);
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
        let request = json!({"op":"puppet_warp","layer":0,"pins":[
            {"from":[28.0,23.0],"to":[34.0,21.0]},{"from":[18.0,23.0],"to":[24.0,21.0]}]});
        let TestDoc::Byte(out) = TestDoc::Byte(doc).warp(&request).unwrap().unwrap() else {
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
        let mut checked = 0;
        for y in 0..h {
            for x in 0..w {
                let (cx, cy) = (layer.offset.0 + x as i32, layer.offset.1 + y as i32);
                let (sx, sy) = (cx - 6 - OFFSET.0, cy + 2 - OFFSET.1);
                let before = if sx >= 0 && sy >= 0 && (sx as u32) < SIZE.0 && (sy as u32) < SIZE.1 {
                    gradient(sx as u32, sy as u32)
                } else {
                    255
                };
                let got = after.get_pixel(x, y).0[0];
                if layer.pixels.sample_at(x, y).unwrap()[3] > 0 {
                    assert_eq!(got, before, "mask at ({cx}, {cy})");
                    checked += 1;
                } else {
                    assert!(got == before || got == 255, "mask at ({cx}, {cy})");
                }
            }
        }
        assert!(checked > 100);
    }
}

fn refusal(doc: &TestDoc, request: Value) -> String {
    let planned = doc.plan(&request).expect_err("the plan refuses");
    let edited = doc.warp(&request).err().expect("the edit refuses");
    assert_eq!(planned, edited);
    edited
}

#[test]
fn every_refusal_is_named() {
    let doc = TestDoc::Byte(byte_doc());
    let pin = json!({"from":[28.0,23.0],"to":[30.0,23.0]});
    let with = |key: &str, value: Value| {
        let mut request = json!({"op":"puppet_warp","layer":0,"pins":[pin]});
        request[key] = value;
        request
    };
    let cases: Vec<(Value, String)> = vec![
        (
            with("pins", json!([])),
            "Puppet Warp needs at least one pin.".into(),
        ),
        (
            with("pins", json!(vec![pin.clone(); 257])),
            "Puppet Warp takes at most 256 pins.".into(),
        ),
        (
            with("pins", json!([pin, {"from":[1.0,1.0],"to":[2.0,2.0]}])),
            "Puppet Warp pin 1 is outside the mesh.".into(),
        ),
        (
            with("pins", json!([pin, {"from":[28.25,23.0],"to":[29.0,23.0]}])),
            "Puppet Warp pins 0 and 1 share a mesh vertex.".into(),
        ),
        (
            with(
                "pins",
                json!([{"from":[20.0,20.0],"to":[-500000.0,-500000.0]},
                {"from":[36.0,26.0],"to":[500000.0,500000.0]}]),
            ),
            "Puppet Warp would make the layer larger than the pixel limit.".into(),
        ),
        (
            with("mode", json!("bendy")),
            "Puppet Warp mode must be rigid, normal or distort.".into(),
        ),
        (
            with("mode", json!(2)),
            "Puppet Warp mode must be rigid, normal or distort.".into(),
        ),
        (
            with("density", json!("most")),
            "Puppet Warp density must be fewer, normal or more.".into(),
        ),
        (
            with("expansion", json!(2.5)),
            "Puppet Warp expansion must be a whole number of pixels from -100 to 100.".into(),
        ),
        (
            with("expansion", json!(101)),
            "Puppet Warp expansion must be a whole number of pixels from -100 to 100.".into(),
        ),
        (
            with("expansion", json!(i64::MIN)),
            "Puppet Warp expansion must be a whole number of pixels from -100 to 100.".into(),
        ),
        (
            with(
                "pins",
                json!([{"from":[28.0,23.0],"to":[30.0,23.0],"depth":i64::MIN}]),
            ),
            "Puppet Warp pin 0 depth must be a whole number from -1000 to 1000.".into(),
        ),
        (
            with("pins", json!([pin, {"from":[1.0,"a"],"to":[2.0,2.0]}])),
            "Puppet Warp pin 1 needs from and to as two finite numbers within 1000000.".into(),
        ),
        (
            with("pins", json!([{"from":[1.0,2.0],"to":[2000000.0,2.0]}])),
            "Puppet Warp pin 0 needs from and to as two finite numbers within 1000000.".into(),
        ),
        (
            with("pins", json!([{"from":[1.0,2.0,3.0],"to":[2.0,2.0]}])),
            "Puppet Warp pin 0 needs from and to as two finite numbers within 1000000.".into(),
        ),
        (
            with(
                "pins",
                json!([{"from":[28.0,23.0],"to":[30.0,23.0],"depth":1001}]),
            ),
            "Puppet Warp pin 0 depth must be a whole number from -1000 to 1000.".into(),
        ),
        (
            with(
                "pins",
                json!([{"from":[28.0,23.0],"to":[30.0,23.0],"depth":1.5}]),
            ),
            "Puppet Warp pin 0 depth must be a whole number from -1000 to 1000.".into(),
        ),
        (
            with(
                "pins",
                json!([{"from":[28.0,23.0],"to":[30.0,23.0],"rotate":"left"}]),
            ),
            "Puppet Warp pin 0 rotate must be a finite angle in degrees or null.".into(),
        ),
    ];
    for (request, want) in cases {
        assert_eq!(refusal(&doc, request.clone()), want, "{request}");
    }
    let empty = TestDoc::Byte(byte_doc_with(
        CANVAS,
        OFFSET,
        byte_layer(SIZE, |_, _| [9, 9, 9, 0]),
    ));
    assert_eq!(
        refusal(&empty, with("pins", json!([pin]))),
        "Puppet Warp needs pixels to mesh, and there are none at expansion 2 px."
    );
    let speck = TestDoc::Byte(byte_doc_with(
        CANVAS,
        OFFSET,
        byte_layer(SIZE, |x, y| {
            if (20..23).contains(&x) && (17..20).contains(&y) {
                [1, 2, 3, 255]
            } else {
                [0; 4]
            }
        }),
    ));
    assert_eq!(
        refusal(&speck, with("expansion", json!(-5))),
        "Puppet Warp needs pixels to mesh, and there are none at expansion -5 px."
    );
    for (locks, which) in [(LOCK_PIXELS, "pixels"), (LOCK_POSITION, "position")] {
        let mut locked = byte_doc();
        locked.layers[0].locks = locks;
        assert_eq!(
            refusal(&TestDoc::Byte(locked), with("pins", json!([pin]))),
            format!("layer 0 is locked ({which})")
        );
    }
    let mut described = byte_doc();
    described.layers[0].meta = Some(json!({"type":"text"}).to_string());
    assert_eq!(
        refusal(&TestDoc::Byte(described), with("pins", json!([pin]))),
        "Rasterize layer 0 before Puppet Warp"
    );
}

#[test]
fn the_float_form_refuses_an_empty_selection_by_name() {
    let doc = byte_doc();
    let request =
        json!({"op":"puppet_selected","layer":0,"pins":[{"from":[28.0,23.0],"to":[30.0,23.0]}]});
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
        Some("The selection holds no pixels to puppet warp.")
    );
}

#[test]
fn a_mesh_export_below_its_plan_refuses_with_the_admission_sentence() {
    let doc = TestDoc::Byte(byte_doc());
    let error = doc
        .mesh_at(0, &json!({}), None, 1000)
        .expect_err("the build is admitted first");
    assert!(
        error.starts_with("precision operation needs ")
            && error.ends_with("working limit is 1000 bytes"),
        "{error}"
    );
}

#[test]
fn the_preview_renders_the_proxy_through_the_pose() {
    let doc = TestDoc::Byte(byte_doc());
    let mesh = doc.mesh(&json!({}));
    let pose = solve(
        mesh,
        &json!({"pins":[{"from":[28.0,23.0],"to":[38.0,23.0]}]}),
    )
    .unwrap();
    free_mesh(mesh);
    let layer = byte_layer(SIZE, ellipse);
    let proxy = unsafe { dr_image_from_rgba(layer.as_raw().as_ptr(), SIZE.0, SIZE.1) };
    let mut rect = [0.0f64; 4];
    let mut err = ptr::null_mut();
    let image = unsafe {
        dr_puppet_pose_render(
            pose,
            proxy,
            i64::from(OFFSET.0),
            i64::from(OFFSET.1),
            SIZE.0,
            SIZE.1,
            ptr::null(),
            0,
            rect.as_mut_ptr(),
            &mut err,
        )
    };
    assert!(take_err(err).is_none());
    assert!(!image.is_null());
    assert!(rect[2] > 0.0 && rect[3] > 0.0);
    assert!(unsafe { dr_image_width(image) } > 0);
    let short = [0u8; 3];
    let mut err = ptr::null_mut();
    let refused = unsafe {
        dr_puppet_pose_render(
            pose,
            proxy,
            0,
            0,
            SIZE.0,
            SIZE.1,
            short.as_ptr(),
            short.len(),
            rect.as_mut_ptr(),
            &mut err,
        )
    };
    assert!(refused.is_null());
    assert!(take_err(err).is_some());
    unsafe {
        dr_image_free(image);
        dr_image_free(proxy);
    }
    free_pose(pose);
}

#[test]
fn every_export_tolerates_null() {
    let mut err = ptr::null_mut();
    unsafe {
        assert!(dr_doc_puppet_mesh(
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            LIMIT,
            0,
            &mut err
        )
        .is_null());
        assert!(dr_precision_doc_puppet_mesh(
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            LIMIT,
            0,
            &mut err
        )
        .is_null());
        assert!(dr_puppet_mesh_describe(ptr::null(), &mut err).is_null());
        assert!(dr_puppet_mesh_solve(ptr::null(), ptr::null(), &mut err).is_null());
        assert!(dr_puppet_pose_describe(ptr::null(), &mut err).is_null());
        assert!(dr_puppet_pose_render(
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
        dr_puppet_pose_free(ptr::null_mut());
        dr_puppet_mesh_free(ptr::null_mut());
        assert!(dr_doc_puppet_warp_plan(ptr::null(), ptr::null(), LIMIT, 0, &mut err).is_null());
        assert!(
            dr_doc_puppet_warp(ptr::null(), ptr::null(), LIMIT, 0, ptr::null(), &mut err).is_null()
        );
        assert!(
            dr_precision_doc_puppet_warp_plan(ptr::null(), ptr::null(), LIMIT, 0, &mut err)
                .is_null()
        );
        assert!(dr_precision_doc_puppet_warp(
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
    let none = unsafe { dr_doc_puppet_warp(&doc, ptr::null(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(none.is_null());
    assert_eq!(take_err(err).as_deref(), Some("request is NULL"));
    let mesh = TestDoc::Byte(doc).mesh(&json!({}));
    let mut err = ptr::null_mut();
    assert!(unsafe { dr_puppet_mesh_solve(mesh, ptr::null(), &mut err) }.is_null());
    assert_eq!(take_err(err).as_deref(), Some("pins is NULL"));
    free_mesh(mesh);
}
