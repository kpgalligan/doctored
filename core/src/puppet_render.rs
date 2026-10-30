//! Puppet Warp's render: the piecewise-affine inverse map of a solved
//! pose, and the resample through it. Free functions over a
//! `puppet_arap::PuppetPose`; the adapters supply each depth's
//! accumulate/finish (the persisted warp's, `doc_warp` / `precision_warp`).
//!
//! THE MAP. Each deformed triangle draws at its depth: the depth of the pin
//! nearest its rest centroid (ties to the lower pin index). Triangles draw in
//! ascending (depth, index) order and the topmost one holding a point wins,
//! which is how Pin Depth decides what shows where the mesh overlaps itself.
//! The point's barycentric coordinates in that deformed triangle, applied to
//! the rest triangle, give the source point. A point outside every deformed
//! triangle maps to (−1e9, −1e9), where every tap misses, so it renders
//! transparent (or a mask's outside coverage). Containment is closed with a
//! 1e-9 barycentric slack, so a point on a shared edge is never lost between
//! its two triangles.
//!
//! Perspective Warp builds the same map from its own triangles
//! (`PoseMap::from_triangles`): a triangle may instead take its source
//! through a homography (`TriangleSource::Projective`), which is exact for
//! the straight-edged image of a plane, and carries a tag `locate` answers.
//!
//! THE RESAMPLE is `sampling::map_into` (the one binary64 tap walk) with the
//! Catmull-Rom kernel the persisted warp and Liquify use. The layer form's
//! destination is the deformed mesh's bounding box rounded outward, not
//! clipped to the canvas: everything a tap can reach lies inside the mesh.
//! The interactive preview resamples the workspace's RGBA8 proxy with the
//! bilinear kernel instead, because it runs on every pointer move and the
//! commit re-renders the layer at full size anyway.
use image::RgbaImage;

use crate::doc_warp::{accumulate_rgba8, finish_rgba8};
use crate::job_control::JobContext;
use crate::puppet_arap::PuppetPose;
use crate::puppet_mesh::{outward_bounds, Rect};
use crate::sampling::{map_into, Kernel};

/// The plan's working bytes per destination pixel on top of the stored
/// output (the per-pixel point location, the tiled adoption of the output).
///
/// Record (the Release app at `DR_WORKING_LIMIT_BYTES` 8 GiB, a fresh
/// instance per case, `MallocLargeCache=0`, E = `ri_interval_max_phys_footprint`
/// − pre around the one `puppet_warp` call, 3 runs + a baseline twin;
/// `scripts/puppet-warp-admission/measure.py`; Apple M2 Max, 96 GiB,
/// 2026-10-13): a 6000×4000 one-layer document of two-colour Clouds, four pins
/// held near the corners and one moved by (60, 40), so the warped layer is
/// 6113×4087. P is the core plan's `scratch_bytes` plus the host's 24n
/// projection (`PuppetWarpWork.prepareLayer`), declared before the runs.
/// - 8-bit: P 1,189,135,940; E 389,415,152 / 389,333,232 / 389,366,000 (3.054×).
/// - 16-bit: P 1,385,071,264; E 586,727,856 / 366,133,488 / 368,541,936 (2.361×).
/// - 32-bit float: P 2,176,707,784; E 785,220,160 / 784,990,760 / 785,121,832
///   (2.772×).
/// - The selection's float at 8 bits (a 4000×2400 selection, one pin held,
///   one moved), P the selected-pixels plan the host admits, 780,406,668 when
///   measured, 802,503,820 since the float's landing term also charges
///   [`MAP_BYTES`] (P + 22,097,152); E 380,764,448 / 380,666,144 /
///   380,797,216 (2.107×).
///
/// The baseline twins read E ≤ 98,304. Every case clears ⌈1.25 × max E⌉ at
/// this margin, so it stays 4. Perspective Warp widened
/// [`MAP_BYTES`]' per-triangle term from 1 to 2 MiB, which adds 1,048,576 to
/// each layer P above and only raises the ratios.
pub(crate) const MARGIN_BYTES_PER_PIXEL: u64 = 4;
/// The source point of a destination outside the mesh.
const NOWHERE: [f64; 2] = [-1e9, -1e9];
/// Closed containment slack, in barycentric units.
const SLACK: f64 = 1e-9;
/// The bucket grid's entry budget; a pose whose triangles would overfill it
/// takes coarser buckets.
const MAX_BUCKET_ENTRIES: u64 = 1_000_000;
/// A [`PoseMap`]'s working memory at most: the entries (4 bytes), the
/// offsets and the fill cursor (8 each) at the bucket budget, plus 2 MiB for
/// the per-triangle records, boxes and draw order (`O(triangles)`; a
/// [`MapTriangle`] carries a projective source as nine binary64s).
pub(crate) const MAP_BYTES: u64 = 20 * MAX_BUCKET_ENTRIES + 2 * 1024 * 1024;
/// The largest side of an interactive preview, in pixels.
const MAX_PREVIEW_SIDE: f64 = 4096.0;

/// An inverse map [`render`] resamples through: the source point of a
/// destination canvas point, or `None` where it renders transparent (or a
/// mask's outside coverage). [`PoseMap`] is Puppet Warp's and Perspective
/// Warp's; `align_projection::LayerProjection` is Auto-Align's Cylindrical
/// and Spherical reprojection.
pub(crate) trait InverseMap {
    fn source_point(&self, p: [f64; 2]) -> Option<[f64; 2]>;
}

/// Where a map triangle's points come from: the rest triangle's point at the
/// same barycentric coordinates (piecewise affine, Puppet Warp's mesh), or a
/// 3×3 row-major homography applied to the canvas point (a plane of
/// Perspective Warp, whose straight-edged image this triangle is).
#[derive(Clone, Copy, Debug)]
pub(crate) enum TriangleSource {
    Rest([[f64; 2]; 3]),
    Projective([f64; 9]),
}

/// One deformed triangle of an inverse map: its corners on the canvas, where
/// its points come from, and a caller's tag ([`PoseMap::locate`] answers it).
#[derive(Clone, Copy, Debug)]
pub(crate) struct MapTriangle {
    pub(crate) corners: [[f64; 2]; 3],
    pub(crate) source: TriangleSource,
    pub(crate) tag: u32,
}

/// An inverse map: deformed triangles in ascending draw order and a bucket
/// grid over them.
pub(crate) struct PoseMap {
    triangles: Vec<MapTriangle>,
    dest: Rect,
    origin: [f64; 2],
    cell: f64,
    columns: usize,
    rows: usize,
    /// CSR: bucket `b`'s triangles are `entries[offsets[b]..offsets[b + 1]]`,
    /// in ascending draw order.
    offsets: Vec<usize>,
    entries: Vec<u32>,
}

impl PoseMap {
    /// A pose's map: each deformed triangle at its depth (see the module
    /// doc), sources at rest, tags the triangle indices.
    pub(crate) fn new(pose: &PuppetPose) -> Result<Self, String> {
        let mesh = pose.mesh();
        let rest = mesh.vertices();
        let triangles = mesh.triangles();
        let positions = pose.positions();
        let pins = pose.pins();
        let depth = |t: &[u32; 3]| -> i32 {
            let c = [
                (rest[t[0] as usize][0] + rest[t[1] as usize][0] + rest[t[2] as usize][0]) / 3.0,
                (rest[t[0] as usize][1] + rest[t[1] as usize][1] + rest[t[2] as usize][1]) / 3.0,
            ];
            let mut best: Option<(f64, i32)> = None;
            for p in pins {
                let d = (p.from[0] - c[0]).powi(2) + (p.from[1] - c[1]).powi(2);
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, p.depth));
                }
            }
            best.map_or(0, |(_, depth)| depth)
        };
        let mut order: Vec<(i32, u32)> = triangles
            .iter()
            .enumerate()
            .map(|(i, t)| (depth(t), i as u32))
            .collect();
        order.sort_unstable();
        let records = order
            .iter()
            .map(|&(_, t)| {
                let tri = triangles[t as usize];
                MapTriangle {
                    corners: tri.map(|v| positions[v as usize]),
                    source: TriangleSource::Rest(tri.map(|v| rest[v as usize])),
                    tag: t,
                }
            })
            .collect();
        Self::from_triangles(records, mesh.spacing())
    }

    /// The map of `triangles`, given in ascending draw order (the last one
    /// holding a point wins), bucketed at `cell` px (at least 1, doubled
    /// until the entries fit the budget).
    pub(crate) fn from_triangles(triangles: Vec<MapTriangle>, cell: f64) -> Result<Self, String> {
        let corners: Vec<[f64; 2]> = triangles.iter().flat_map(|t| t.corners).collect();
        let dest = outward_bounds(&corners);
        drop(corners);
        let origin = [dest.0 as f64, dest.1 as f64];
        let boxes: Vec<[f64; 4]> = triangles
            .iter()
            .map(|t| {
                let p = t.corners;
                [
                    p[0][0].min(p[1][0]).min(p[2][0]),
                    p[0][1].min(p[1][1]).min(p[2][1]),
                    p[0][0].max(p[1][0]).max(p[2][0]),
                    p[0][1].max(p[1][1]).max(p[2][1]),
                ]
            })
            .collect();
        let mut cell = cell.max(1.0);
        let span = |lo: f64, hi: f64, cell: f64, origin: f64| -> (usize, usize) {
            (
                ((lo - origin) / cell).floor().max(0.0) as usize,
                ((hi - origin) / cell).floor().max(0.0) as usize,
            )
        };
        let (columns, rows) = loop {
            let columns = (f64::from(dest.2) / cell).floor() as usize + 1;
            let rows = (f64::from(dest.3) / cell).floor() as usize + 1;
            let mut total = (columns as u64).saturating_mul(rows as u64);
            for b in &boxes {
                let (x0, x1) = span(b[0], b[2], cell, origin[0]);
                let (y0, y1) = span(b[1], b[3], cell, origin[1]);
                total = total.saturating_add(((x1 - x0 + 1) * (y1 - y0 + 1)) as u64);
            }
            if total <= MAX_BUCKET_ENTRIES {
                break (columns, rows);
            }
            cell *= 2.0;
        };
        let clamp = |(a, b): (usize, usize), limit: usize| (a.min(limit - 1), b.min(limit - 1));
        let mut counts = vec![0usize; columns * rows + 1];
        for b in &boxes {
            let (x0, x1) = clamp(span(b[0], b[2], cell, origin[0]), columns);
            let (y0, y1) = clamp(span(b[1], b[3], cell, origin[1]), rows);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    counts[y * columns + x + 1] += 1;
                }
            }
        }
        for i in 1..counts.len() {
            counts[i] += counts[i - 1];
        }
        let offsets = counts.clone();
        let mut fill = counts;
        let mut entries = vec![0u32; *offsets.last().unwrap_or(&0)];
        for (t, b) in boxes.iter().enumerate() {
            let (x0, x1) = clamp(span(b[0], b[2], cell, origin[0]), columns);
            let (y0, y1) = clamp(span(b[1], b[3], cell, origin[1]), rows);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let bucket = y * columns + x;
                    entries[fill[bucket]] = t as u32;
                    fill[bucket] += 1;
                }
            }
        }
        Ok(Self {
            triangles,
            dest,
            origin,
            cell,
            columns,
            rows,
            offsets,
            entries,
        })
    }

    /// The deformed triangles' outward-rounded bounding box.
    pub(crate) fn dest_rect(&self) -> Rect {
        self.dest
    }

    /// The rest point the canvas point `p` comes from, or `None` outside
    /// every deformed triangle.
    pub(crate) fn source_point(&self, p: [f64; 2]) -> Option<[f64; 2]> {
        self.locate(p).map(|(_, q)| q)
    }

    /// The topmost deformed triangle holding `p`: its tag and the source
    /// point, or `None` outside every triangle.
    pub(crate) fn locate(&self, p: [f64; 2]) -> Option<(u32, [f64; 2])> {
        let fx = ((p[0] - self.origin[0]) / self.cell).floor();
        let fy = ((p[1] - self.origin[1]) / self.cell).floor();
        if !(fx >= 0.0 && fy >= 0.0) || fx as usize >= self.columns || fy as usize >= self.rows {
            return None;
        }
        let bucket = fy as usize * self.columns + fx as usize;
        for &t in self.entries[self.offsets[bucket]..self.offsets[bucket + 1]]
            .iter()
            .rev()
        {
            let triangle = &self.triangles[t as usize];
            let [x0, x1, x2] = triangle.corners;
            let e1 = [x1[0] - x0[0], x1[1] - x0[1]];
            let e2 = [x2[0] - x0[0], x2[1] - x0[1]];
            let d = e1[0] * e2[1] - e2[0] * e1[1];
            if d == 0.0 || !d.is_finite() {
                continue;
            }
            let q = [p[0] - x0[0], p[1] - x0[1]];
            let l1 = (q[0] * e2[1] - e2[0] * q[1]) / d;
            let l2 = (e1[0] * q[1] - q[0] * e1[1]) / d;
            let l0 = 1.0 - l1 - l2;
            if l0 >= -SLACK && l1 >= -SLACK && l2 >= -SLACK {
                let source = match triangle.source {
                    TriangleSource::Rest([r0, r1, r2]) => [
                        l0 * r0[0] + l1 * r1[0] + l2 * r2[0],
                        l0 * r0[1] + l1 * r1[1] + l2 * r2[1],
                    ],
                    TriangleSource::Projective(h) => {
                        let w = h[6] * p[0] + h[7] * p[1] + h[8];
                        [
                            (h[0] * p[0] + h[1] * p[1] + h[2]) / w,
                            (h[3] * p[0] + h[4] * p[1] + h[5]) / w,
                        ]
                    }
                };
                return Some((triangle.tag, source));
            }
        }
        None
    }
}

impl InverseMap for PoseMap {
    fn source_point(&self, p: [f64; 2]) -> Option<[f64; 2]> {
        PoseMap::source_point(self, p)
    }
}

/// Resample the plane standing at `source` into `dest` (both canvas rects)
/// through `map`; the arguments after them are `map_into`'s.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render<M: InverseMap + ?Sized, P: Copy, Q, const C: usize>(
    map: &M,
    source: Rect,
    dest: Rect,
    read: impl Fn(usize) -> P,
    accumulate: impl Fn(&mut [f64; C], P, f64),
    finish: impl Fn([f64; C]) -> Q,
    write: impl FnMut(usize, Q),
    job: JobContext<'_>,
) -> Result<(), String> {
    let (sx, sy) = (source.0 as f64, source.1 as f64);
    let (dx, dy) = (dest.0 as f64, dest.1 as f64);
    map_into(
        (source.2, source.3),
        (dest.2, dest.3),
        Kernel::Bicubic,
        |x, y, _| {
            Ok(map
                .source_point([dx + x, dy + y])
                .map_or(NOWHERE, |q| [q[0] - sx, q[1] - sy]))
        },
        read,
        accumulate,
        finish,
        write,
        job,
    )
}

/// The workspace preview: `proxy` (a picture of the canvas rect `source` at
/// any scale) through `map`, optionally times a proxy-sized `coverage`. The
/// answer is the picture and the canvas rect `[x, y, w, h]` it covers.
pub(crate) fn render_preview(
    map: &PoseMap,
    proxy: &RgbaImage,
    source: Rect,
    coverage: Option<&[u8]>,
) -> Result<(RgbaImage, [f64; 4]), String> {
    let (pw, ph) = proxy.dimensions();
    if pw == 0 || ph == 0 || source.2 == 0 || source.3 == 0 {
        return Err("the Puppet Warp preview needs a non-empty proxy and source".into());
    }
    if coverage.is_some_and(|c| c.len() != pw as usize * ph as usize) {
        return Err("the Puppet Warp preview coverage must be proxy-sized".into());
    }
    let sx = f64::from(pw) / f64::from(source.2);
    let sy = f64::from(ph) / f64::from(source.3);
    let dest = map.dest_rect();
    let side = f64::from(dest.2.max(dest.3));
    let k = sx.min(sy).min(MAX_PREVIEW_SIDE / side);
    let ow = ((f64::from(dest.2) * k).ceil() as u32).max(1);
    let oh = ((f64::from(dest.3) * k).ceil() as u32).max(1);
    let raw = proxy.as_raw();
    let mut out = vec![0u8; ow as usize * oh as usize * 4];
    let (ox, oy) = (dest.0 as f64, dest.1 as f64);
    map_into(
        (pw, ph),
        (ow, oh),
        Kernel::Bilinear,
        |x, y, _| {
            Ok(map
                .source_point([ox + x / k, oy + y / k])
                .map_or(NOWHERE, |q| {
                    [(q[0] - source.0 as f64) * sx, (q[1] - source.1 as f64) * sy]
                }))
        },
        |i| {
            let p = [raw[i * 4], raw[i * 4 + 1], raw[i * 4 + 2], raw[i * 4 + 3]];
            (p, coverage.map_or(1.0, |c| f64::from(c[i]) / 255.0))
        },
        |acc: &mut [f64; 4], (p, c), weight| accumulate_rgba8(acc, p, weight * c),
        finish_rgba8,
        |i, p| out[i * 4..i * 4 + 4].copy_from_slice(&p),
        JobContext::NONE,
    )?;
    let image =
        RgbaImage::from_raw(ow, oh, out).ok_or("the Puppet Warp preview dimensions are invalid")?;
    Ok((image, [ox, oy, f64::from(ow) / k, f64::from(oh) / k]))
}
