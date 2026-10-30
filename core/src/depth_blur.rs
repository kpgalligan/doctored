//! Filter ▸ Neural Filters ▸ Depth Blur: a disc blur whose radius
//! follows a depth map, plus Haze, Warmth, Brightness,
//! Saturation and Grain, as the twenty-first op of the filter routes
//! (`ops_spatial::filter_op` at 8 bits, `SelectedFilter::DepthBlur` at 16,
//! `float_filter_route::Route::DepthBlur` at 32; no new export). Free functions
//! and two value types over plain samples; it adds no pixel math of its own
//! beyond the depth composite: the blur is Lens Blur's disc
//! (`sampling_disc::disc_plane`), the edge-aware upsample Select and Mask's
//! guided filter (`edge_refine::refine`), the reduction the master box mean
//! (`thumbnail_reduce::box_reduced`), the grade the Develop Filter
//! (`sampling_develop_filter::DevelopFilter`) and the grain Add Grain's plane and
//! combine (`ops_spatial::SpatialKernel::AddGrain`).
//!
//! THE DEPTH INPUT. The host runs the depth model (Depth Anything V2 Small)
//! and sends its map in the params as `depth`: `width × height` samples,
//! larger = nearer as the model emits them, over `frame` (`[x, y, w, h]` in
//! the target plane's pixels: the selection's float, else the layer on the
//! canvas), with an optional subject mask of the same size. The samples are
//! normalized by their min and max to the nearness `n` in 0…1; the depth is
//! `D = 1 − n` (0 nearest).
//!
//! THE FIELD. `n` is resampled bilinearly (`sampling_radial::sample_exact`,
//! the master bilinear read) to a field over the frame at the frame's size
//! divided by the whole number `k = ⌈√(frame pixels / FIELD_MAX_PIXELS)⌉`, then
//! guided by the picture with `edge_refine::refine` (every pixel INCLUDE,
//! Smart Radius off, radius `clamp(⌈field long side / depth long side⌉, 4,
//! 64)`: one depth sample's footprint, so the filter's window always spans at
//! least the blockiness the model's resolution leaves). The guide is the frame
//! itself at `k = 1`, else the frame reduced by `k` once per RGB channel
//! through the master box mean; its byte output is guide precision only. At 32
//! bits the guide is the display-encoded value clamped to 0…1, the value the
//! picture shows. Each pixel then reads the field bilinearly. The subject mask
//! takes the same path.
//!
//! THE RADIUS. With the focus `f` (the subject-weighted median of `D` for
//! Focus Subject, `D` at the focal point, else Focal Distance / 100) and `h =`
//! Focal Range / 100, a pixel's radius is `ρ = R · (strength / 100) ·
//! clamp((|D − f| − h) / (1 − h), 0, 1) · (1 − subject)`, `R` =
//! [`DepthBlur::max_radius`]. Outside the frame `ρ = 0` and there is no haze.
//!
//! THE ORDER. Blur (the disc mean, Box Blur's straight combine; a pixel at
//! `ρ = 0` keeps its source sample exactly), then Haze (RGB lerped toward 1.0
//! by `haze / 100 · HAZE_MAX · D`), then the grade (`DevelopFilter { temperature:
//! warmth, exposure: brightness / 50, saturation: 2 · saturation }`), then the
//! grain (Add Grain at `amount: grain`, its parse-default size 1 and roughness
//! 50, seed [`DEPTH_GRAIN_SEED`]). At 8 and 16 bits every stage runs on the
//! stored samples, the grade through `DevelopFilter::plane_in` at `Unit`. At 32
//! bits blur, haze, warmth and brightness (`DevelopFilter::linear_front`) run on
//! the stored linear values; saturation (`DevelopFilter::finish_op` through the
//! Scaled display form) and the grain delta run on the display-encoded value,
//! decoded back. Output Depth Map Only writes the grey `n` with the source
//! alpha inside the frame (at 32 bits the linear value whose display encoding
//! is `n`) and leaves the rest alone.
use crate::display_encoding::DisplayEncoding;
use crate::edge_refine::{refine, EdgeRefine, AREA_INCLUDE};
use crate::filter_kernels::SampleRange;
use crate::job_control::JobContext;
use crate::ops_spatial::SpatialKernel;
use crate::precision_image::{pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::sampling_develop_filter::DevelopFilter;
use crate::sampling_disc::{disc_plan, disc_plane};
use crate::sampling_radial::sample_exact;
use crate::thumbnail_reduce::box_reduced;
use serde_json::{Map, Value};

/// The byte cap of a `depth_blur` request's JSON text (the `params_json` of
/// `dr_image_filter_op`). The model's map is at most [`MAX_DEPTH_PIXELS`]
/// samples; Depth Anything V2 Small's own is 518 × 392 (203,056), which at
/// the host's six significant digits is about 2 MB with its subject mask, so
/// 8 MiB holds the map and leaves room for a larger model's.
pub(crate) const MAX_REQUEST_BYTES: usize = 8 << 20;
/// The most depth samples a request may carry (a 1024 × 1024 map).
pub(crate) const MAX_DEPTH_PIXELS: u64 = 1 << 20;
/// The most pixels the guided depth field holds. 4.2 million (a 2048 × 2048
/// field) is ten times the model's own resolution on each side's worth of
/// detail, so the guide still places every depth edge to within a pixel of
/// a reduced frame, while the guided filter's ~64 bytes a pixel stay near
/// 270 MB however large the picture.
pub(crate) const FIELD_MAX_PIXELS: u64 = 1 << 22;
/// The haze's strength at Haze 100 on the farthest depth: 60 % of the way to
/// white. Fully white would erase the background entirely, which no real
/// atmosphere does at portrait distances; 0.6 is our own shaping, not a fit.
const HAZE_MAX: f64 = 0.6;
/// The grain's fixed seed, so a replayed step draws the same grain.
const DEPTH_GRAIN_SEED: u64 = 0x5EED_DE97;
/// The largest radius is the plane's longer side over this: a 6000-pixel
/// photograph blurs its background with discs of up to 100 pixels (the disc
/// kernel's own ceiling), which is about a portrait lens wide open, and a
/// 1000-pixel one with discs of 25, so the look belongs to the picture rather
/// than to its pixel count.
const RADIUS_DIVISOR: f64 = 40.0;
/// The depth stage's term per field pixel, beside `edge_refine::BYTES_PER_PIXEL`:
/// the guided nearness and subject (f32 each), the unguided plane each is
/// refined from beside its result and the all-INCLUDE area plane are 17 bytes;
/// the rest is the margin the measured record below set, since the plan must
/// cover the whole call and this is the term a miss raises.
///
/// Record (`scripts/depth-blur-admission/measure.py`: the Release app
/// at `DR_WORKING_LIMIT_BYTES` 8 GiB, `depth_blur` blur_strength 100 on a
/// one-layer 6000×4000 document of two-colour Clouds, a 2000×1333 field;
/// `MallocLargeCache=0`, E = `ri_interval_max_phys_footprint` − pre around
/// the one call, a fresh process per case, runs beside a baseline twin at E ≤
/// 49,152). At 17 the 8-bit plan (`filter_op_scratch_bytes`, 852,661,244) was
/// below ⌈1.25 × E⌉ for E 833,749,664 / 833,847,944 / 833,536,648 (the call's
/// peak is the kernel with the commit and first projection after it, about
/// 450 MB of kernel by its own arithmetic), so the term was raised to the
/// smallest value that covers it: 74 gives `filter_op_scratch_bytes`
/// 1,042,792,315 ≥ 1,042,309,930. 16-bit E 606,077,384 / 606,224,840 against
/// `selected_edit_plan` 1,076,331,004 at 74; 32-bit E 798,130,776 /
/// 798,147,160 against `float_filter_plan` 1,103,840,891 at 74.
const FIELD_BYTES_PER_PIXEL: u64 = 74;

/// The request's keys, in the sheet's order.
const KEYS: [&str; 12] = [
    "focus_subject",
    "focal_point",
    "focal_distance",
    "focal_range",
    "blur_strength",
    "haze",
    "warmth",
    "brightness",
    "saturation",
    "grain",
    "depth_map_only",
    "depth",
];

/// The model's depth over the frame, normalized at parse.
pub(crate) struct DepthPlane {
    width: u32,
    height: u32,
    /// `n` in 0…1, larger = nearer.
    nearness: Vec<f32>,
    /// The subject mask (255 = subject), when the host sent one.
    subject: Option<Vec<u8>>,
    /// `[x, y, w, h]` in the target plane's pixels.
    frame: [u32; 4],
}

/// One parsed Depth Blur request (module doc).
pub(crate) struct DepthBlur {
    focal_distance: f64,
    focal_range: f64,
    blur_strength: f64,
    haze: f64,
    warmth: f64,
    brightness: f64,
    saturation: f64,
    grain: f64,
    focus_subject: bool,
    focal_point: Option<[f64; 2]>,
    depth_only: bool,
    depth: DepthPlane,
}

/// The guided depth field over the frame (module doc, THE FIELD).
struct Field {
    width: usize,
    height: usize,
    nearness: Vec<f32>,
    subject: Option<Vec<f32>>,
}

fn whole(value: &Value, name: &str) -> Result<u32, String> {
    value
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| format!("{name} must be a non-negative integer"))
}

impl DepthPlane {
    fn parse(value: &Value) -> Result<Self, String> {
        let object = value
            .as_object()
            .ok_or("depth must be an object with width, height, samples and frame")?;
        if let Some(key) = object
            .keys()
            .find(|k| !["width", "height", "samples", "frame", "subject"].contains(&k.as_str()))
        {
            return Err(format!("unexpected depth parameter {key}"));
        }
        let field = |name: &str| {
            object
                .get(name)
                .ok_or_else(|| format!("depth needs {name}"))
        };
        let width = whole(field("width")?, "depth width")?;
        let height = whole(field("height")?, "depth height")?;
        let count = u64::from(width) * u64::from(height);
        if count == 0 {
            return Err("depth width and height must be positive".into());
        }
        if count > MAX_DEPTH_PIXELS {
            return Err(format!(
                "depth map exceeds {MAX_DEPTH_PIXELS} samples ({width}×{height})"
            ));
        }
        let count = count as usize;
        let samples = field("samples")?
            .as_array()
            .filter(|a| a.len() == count)
            .ok_or_else(|| format!("depth samples must be {count} numbers"))?;
        let mut raw: Vec<f32> = reserved_vec(count)?;
        for v in samples {
            let n = v.as_f64().ok_or("depth samples must be numbers")? as f32;
            if !n.is_finite() {
                return Err("depth map samples must be finite".into());
            }
            raw.push(n);
        }
        let (min, max) = raw
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
                (lo.min(*v), hi.max(*v))
            });
        let span = f64::from(max) - f64::from(min);
        // Below a millionth the map is one depth: no order to blur by.
        if span <= 1e-6 {
            return Err("depth map has no range".into());
        }
        for v in &mut raw {
            *v = ((f64::from(*v) - f64::from(min)) / span) as f32;
        }
        let frame = field("frame")?
            .as_array()
            .filter(|a| a.len() == 4)
            .ok_or("depth frame must be [x, y, width, height]")?;
        let frame = [
            whole(&frame[0], "depth frame x")?,
            whole(&frame[1], "depth frame y")?,
            whole(&frame[2], "depth frame width")?,
            whole(&frame[3], "depth frame height")?,
        ];
        if frame[2] == 0 || frame[3] == 0 {
            return Err("depth frame width and height must be positive".into());
        }
        let subject = match object.get("subject") {
            None => None,
            Some(v) => {
                let values = v
                    .as_array()
                    .filter(|a| a.len() == count)
                    .ok_or_else(|| format!("depth subject must be {count} values"))?;
                let mut mask: Vec<u8> = reserved_vec(count)?;
                for v in values {
                    mask.push(
                        v.as_u64()
                            .and_then(|v| u8::try_from(v).ok())
                            .ok_or("depth subject values must be integers in [0,255]")?,
                    );
                }
                Some(mask)
            }
        };
        Ok(Self {
            width,
            height,
            nearness: raw,
            subject,
            frame,
        })
    }

    /// The bilinear `(nearness, subject)` at depth-pixel position `(u, v)`
    /// (centres at half-integers).
    fn at(&self, (u, v): (f64, f64)) -> (f64, f64) {
        let read = |i: usize| {
            [
                self.nearness[i],
                self.subject
                    .as_ref()
                    .map_or(0.0, |s| f32::from(s[i]) / 255.0),
                0.0,
                1.0,
            ]
        };
        let s = sample_exact(
            &read,
            (self.width as usize, self.height as usize),
            (u, v),
            true,
        );
        (s[0], s[1])
    }
}

impl DepthBlur {
    /// The request (module doc). Strict keys and ranges; `Err` names what it
    /// refuses: a missing or malformed `depth`, a flat or non-finite map, a
    /// Focus Subject with no subject mask, a focal point outside the depth
    /// frame. Whether the frame lies inside the plane is checked where the
    /// plane is known, by [`Self::plan`] and [`Self::plane_in`], which every
    /// route calls before allocating.
    pub(crate) fn parse(p: &Map<String, Value>) -> Result<Self, String> {
        if let Some(key) = p.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(format!("unexpected depth_blur parameter {key}"));
        }
        let number = |name: &str, default: f64, lo: f64, hi: f64| -> Result<f64, String> {
            let n = p.get(name).map_or(Ok(default), |v| {
                v.as_f64().ok_or_else(|| format!("{name} must be numeric"))
            })?;
            if !n.is_finite() || !(lo..=hi).contains(&n) {
                return Err(format!("{name} must be finite in [{lo},{hi}]"));
            }
            Ok(n)
        };
        let flag = |name: &str| -> Result<bool, String> {
            p.get(name).map_or(Ok(false), |v| {
                v.as_bool()
                    .ok_or_else(|| format!("{name} must be true or false"))
            })
        };
        let focal_point = match p.get("focal_point") {
            None => None,
            Some(v) => {
                let pair = v
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .ok_or("focal_point must be [x, y]")?;
                let x = pair[0].as_f64().filter(|v| v.is_finite());
                let y = pair[1].as_f64().filter(|v| v.is_finite());
                match (x, y) {
                    (Some(x), Some(y)) => Some([x, y]),
                    _ => return Err("focal_point must be two finite numbers".into()),
                }
            }
        };
        let depth = DepthPlane::parse(p.get("depth").ok_or("depth blur needs a depth map")?)?;
        let request = Self {
            focal_distance: number("focal_distance", 0., 0., 100.)?,
            focal_range: number("focal_range", 20., 0., 100.)?,
            blur_strength: number("blur_strength", 50., 0., 100.)?,
            haze: number("haze", 0., 0., 100.)?,
            warmth: number("warmth", 0., -50., 50.)?,
            brightness: number("brightness", 0., -50., 50.)?,
            saturation: number("saturation", 0., -50., 50.)?,
            grain: number("grain", 0., 0., 100.)?,
            focus_subject: flag("focus_subject")?,
            focal_point,
            depth_only: flag("depth_map_only")?,
            depth,
        };
        if request.focus_subject && request.depth.subject.is_none() {
            return Err("focus subject needs the depth input's subject mask".into());
        }
        if let Some([x, y]) = request.focal_point {
            let [fx, fy, fw, fh] = request.depth.frame.map(f64::from);
            if !(fx..=fx + fw).contains(&x) || !(fy..=fy + fh).contains(&y) {
                return Err("depth blur focal point lies outside the depth frame".into());
            }
        }
        Ok(request)
    }

    /// Whether the blur stage runs.
    fn blurs(&self) -> bool {
        !self.depth_only && self.blur_strength > 0.0 && self.focal_range < 100.0
    }

    /// The grade (module doc, THE ORDER), all zero when off.
    fn grade(&self) -> DevelopFilter {
        DevelopFilter {
            temperature: self.warmth,
            tint: 0.0,
            exposure: self.brightness / 50.0,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
            texture: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
            vibrance: 0.0,
            saturation: 2.0 * self.saturation,
        }
    }

    /// Add Grain at the request's amount, its parse defaults otherwise.
    fn grain_kernel(&self) -> SpatialKernel {
        SpatialKernel::AddGrain {
            amount: self.grain,
            size: 1.0,
            roughness: 50.0,
            seed: DEPTH_GRAIN_SEED,
        }
    }

    /// Changes nothing: no blur (strength 0 or range 100), no haze, grade or
    /// grain, and not the depth map. Every route answers it with `None`.
    pub(crate) fn identity(&self) -> bool {
        (self.blur_strength == 0.0 || self.focal_range == 100.0)
            && self.haze == 0.0
            && self.warmth == 0.0
            && self.brightness == 0.0
            && self.saturation == 0.0
            && self.grain == 0.0
            && !self.depth_only
    }

    /// `R`: the plane's longer side over [`RADIUS_DIVISOR`], within the disc
    /// kernel's 1…100.
    pub(crate) fn max_radius(dimensions: (u32, u32)) -> u32 {
        (f64::from(dimensions.0.max(dimensions.1)) / RADIUS_DIVISOR)
            .round()
            .clamp(1.0, 100.0) as u32
    }

    /// The focus `f` (module doc, THE RADIUS), at depth resolution.
    pub(crate) fn focus(&self) -> Result<f64, String> {
        let depth = &self.depth;
        if self.focus_subject {
            let subject = depth
                .subject
                .as_ref()
                .ok_or("focus subject needs the depth input's subject mask")?;
            let mut weighted: Vec<(f64, f64)> = reserved_vec(subject.len())?;
            let mut total = 0.0;
            for (n, s) in depth.nearness.iter().zip(subject) {
                if *s > 0 {
                    let w = f64::from(*s) / 255.0;
                    weighted.push((1.0 - f64::from(*n), w));
                    total += w;
                }
            }
            if total <= 0.0 {
                return Err("focus subject found no subject in the depth input".into());
            }
            weighted.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut running = 0.0;
            for (d, w) in &weighted {
                running += w;
                if running >= total / 2.0 {
                    return Ok(*d);
                }
            }
            return Ok(weighted.last().map_or(0.0, |(d, _)| *d));
        }
        if let Some([x, y]) = self.focal_point {
            let [fx, fy, fw, fh] = depth.frame.map(f64::from);
            let u = (x - fx) / fw * f64::from(depth.width);
            let v = (y - fy) / fh * f64::from(depth.height);
            return Ok(1.0 - depth.at((u, v)).0);
        }
        Ok(self.focal_distance / 100.0)
    }

    /// The frame, refused when it does not lie inside the plane.
    fn frame_in(&self, dimensions: (u32, u32)) -> Result<[usize; 4], String> {
        let [x, y, w, h] = self.depth.frame;
        if u64::from(x) + u64::from(w) > u64::from(dimensions.0)
            || u64::from(y) + u64::from(h) > u64::from(dimensions.1)
        {
            return Err("depth blur frame lies outside the image".into());
        }
        Ok([x as usize, y as usize, w as usize, h as usize])
    }

    /// The field's reduction `k` and size for a frame of `fw × fh`.
    fn field_size(fw: usize, fh: usize) -> (usize, usize, usize) {
        let pixels = fw as f64 * fh as f64;
        let k = (pixels / FIELD_MAX_PIXELS as f64).sqrt().ceil().max(1.0) as usize;
        if k < 2 || fw < k || fh < k {
            (1, fw, fh)
        } else {
            (k, fw / k, fh / k)
        }
    }

    /// `(scratch, work)` of a run on a plane of `dimensions`, refused before
    /// anything is allocated (the frame outside the plane, a plane past the
    /// pixel limit). There is no work ceiling: like Liquify the request bounds
    /// its own work (the disc's radius is at most 100), and the host runs it
    /// under a job control. The scratch is the output plane (the disc plan,
    /// which holds it, or n×16 without a blur), the held fields and the guided
    /// filter's stage at field size, the frame-sized guide channel of a
    /// reduced field, the grade's and the grain's own plans, and two
    /// bookkeeping allowances, summed: the stages follow one another, so the
    /// sum bounds every one of them.
    pub(crate) fn plan(&self, dimensions: (u32, u32)) -> Result<(u64, u64), String> {
        let n = pixel_count(dimensions.0, dimensions.1)? as u64;
        let [_, _, fw, fh] = self.frame_in(dimensions)?;
        let overflow = || "depth blur plan overflow".to_string();
        let (mut scratch, mut work) = if self.blurs() {
            disc_plan(dimensions, Self::max_radius(dimensions), u64::MAX)?
        } else {
            (n.checked_mul(16).ok_or_else(overflow)?, n)
        };
        let (k, w, h) = Self::field_size(fw, fh);
        let field = (w as u64) * (h as u64);
        let per_field = FIELD_BYTES_PER_PIXEL + crate::edge_refine::BYTES_PER_PIXEL;
        scratch = field
            .checked_mul(per_field)
            .and_then(|f| scratch.checked_add(f))
            .ok_or_else(overflow)?;
        if k > 1 {
            scratch = scratch
                .checked_add((fw as u64) * (fh as u64))
                .ok_or_else(overflow)?;
        }
        work = work.saturating_add(field.saturating_mul(64));
        if !self.depth_only {
            let grade = self.grade();
            if !grade.identity() {
                let (s, w) = grade.plan(dimensions, u64::MAX)?;
                scratch = scratch.checked_add(s).ok_or_else(overflow)?;
                work = work.saturating_add(w);
            }
            if self.grain > 0.0 {
                let (s, w) = self.grain_kernel().plan(dimensions, u64::MAX)?;
                scratch = scratch.checked_add(s).ok_or_else(overflow)?;
                work = work.saturating_add(w);
            }
        }
        let scratch = scratch
            .checked_add(2 * BOOKKEEPING_BYTES)
            .ok_or_else(overflow)?;
        Ok((scratch, work))
    }

    /// The guided field over `frame` (module doc, THE FIELD); `guide(i)` is
    /// plane pixel `i`'s guide RGB in 0…1 times its alpha.
    fn field(
        &self,
        guide: &impl Fn(usize) -> [f32; 3],
        plane_width: usize,
        frame: [usize; 4],
        job: JobContext<'_>,
    ) -> Result<Field, String> {
        let [fx, fy, fw, fh] = frame;
        let (k, w, h) = Self::field_size(fw, fh);
        let m = w * h;
        let depth = &self.depth;
        let (dw, dh) = (f64::from(depth.width), f64::from(depth.height));
        // The guide over the field: the frame itself, or each channel reduced
        // by `k` through the master box mean.
        let reduced = if k > 1 {
            let mut channels: Vec<Vec<u8>> = Vec::with_capacity(3);
            for c in 0..3 {
                let mut channel: Vec<u8> = reserved_vec(fw * fh)?;
                for y in 0..fh {
                    job.check()?;
                    for x in 0..fw {
                        let v = guide((fy + y) * plane_width + fx + x)[c];
                        channel.push((f64::from(v.clamp(0.0, 1.0)) * 255.0).round() as u8);
                    }
                }
                let (small, sw, sh) =
                    box_reduced(&channel, fw as u32, fh as u32, k as u32, |v| v, job)?
                        .ok_or("depth blur could not reduce its guide")?;
                if (sw as usize, sh as usize) != (w, h) {
                    return Err("depth blur guide size differs from its field".into());
                }
                channels.push(small);
            }
            Some(channels)
        } else {
            None
        };
        let guide_at = |i: usize| -> [f32; 3] {
            match &reduced {
                Some(channels) => std::array::from_fn(|c| f32::from(channels[c][i]) / 255.0),
                None => guide((fy + i / w) * plane_width + fx + i % w),
            }
        };
        let parameters = EdgeRefine {
            radius: ((w.max(h) as f64 / dw.max(dh)).ceil().clamp(4.0, 64.0)) as f32,
            smart: false,
        };
        let mut area: Vec<u8> = reserved_vec(m)?;
        area.resize(m, AREA_INCLUDE);
        let resampled = |subject: bool, job: JobContext<'_>| -> Result<Vec<f32>, String> {
            let mut plane: Vec<f32> = reserved_vec(m)?;
            for j in 0..h {
                job.check()?;
                for i in 0..w {
                    let u = (i as f64 + 0.5) / w as f64 * dw;
                    let v = (j as f64 + 0.5) / h as f64 * dh;
                    let (n, s) = depth.at((u, v));
                    plane.push(if subject { s } else { n } as f32);
                }
            }
            Ok(plane)
        };
        let dimensions = (w as u32, h as u32);
        let stage = job.range(0.0, 0.5);
        let coarse = resampled(false, stage)?;
        let nearness = refine(
            guide_at,
            &coarse,
            Some(&area),
            dimensions,
            parameters,
            stage,
        )?;
        drop(coarse);
        let subject = match depth.subject {
            Some(_) => {
                let stage = job.range(0.5, 1.0);
                let coarse = resampled(true, stage)?;
                Some(refine(
                    guide_at,
                    &coarse,
                    Some(&area),
                    dimensions,
                    parameters,
                    stage,
                )?)
            }
            None => None,
        };
        Ok(Field {
            width: w,
            height: h,
            nearness,
            subject,
        })
    }

    /// The final straight samples of a plane of `dimensions` read through
    /// `read` (stored samples: 0…1 at `Unit`, linear light at `Extended`),
    /// in the module doc's order. `encoding` is the 32-bit document's display
    /// encoding, which every `Extended` run needs (the guide, and Saturation,
    /// Grain and Output Depth Map Only); `Unit` ignores it. The caller has
    /// answered an identity request.
    pub(crate) fn plane_in(
        &self,
        read: impl Fn(usize) -> [f32; 4],
        dimensions: (u32, u32),
        range: &SampleRange,
        encoding: Option<&DisplayEncoding<'_>>,
        job: JobContext<'_>,
    ) -> Result<Vec<[f32; 4]>, String> {
        self.plan(dimensions)?;
        let frame = self.frame_in(dimensions)?;
        let extended = matches!(range, SampleRange::Extended { .. });
        let encoding = match (extended, encoding) {
            (true, None) => return Err("depth blur at 32 bits needs the display encoding".into()),
            (true, Some(e)) => Some(e),
            (false, _) => None,
        };
        let focus = self.focus()?;
        let (w, h) = (dimensions.0 as usize, dimensions.1 as usize);
        let n = pixel_count(dimensions.0, dimensions.1)?;
        let guide = |i: usize| -> [f32; 3] {
            let p = read(i);
            let rgb = match encoding {
                Some(e) => {
                    let code = e.encode([
                        p[0].clamp(0.0, 1.0),
                        p[1].clamp(0.0, 1.0),
                        p[2].clamp(0.0, 1.0),
                        p[3],
                    ]);
                    [code[0], code[1], code[2]]
                }
                None => [p[0], p[1], p[2]],
            };
            rgb.map(|v| v.clamp(0.0, 1.0) * p[3].clamp(0.0, 1.0))
        };
        let field = self.field(&guide, w, frame, job.range(0.0, 0.3))?;
        let [fx, fy, fw, fh] = frame;
        let (scale_x, scale_y) = (
            field.width as f64 / fw as f64,
            field.height as f64 / fh as f64,
        );
        // `(D, subject)` at plane pixel (x, y), None outside the frame.
        let depth_at = |x: usize, y: usize| -> Option<(f64, f64)> {
            if x < fx || y < fy || x >= fx + fw || y >= fy + fh {
                return None;
            }
            let read = |i: usize| {
                [
                    field.nearness[i],
                    field.subject.as_ref().map_or(0.0, |s| s[i]),
                    0.0,
                    1.0,
                ]
            };
            let q = (
                (x - fx) as f64 * scale_x + 0.5 * scale_x,
                (y - fy) as f64 * scale_y + 0.5 * scale_y,
            );
            let s = sample_exact(&read, (field.width, field.height), q, true);
            Some((1.0 - s[0].clamp(0.0, 1.0), s[1].clamp(0.0, 1.0)))
        };
        let strength = Self::max_radius(dimensions) as f64 * self.blur_strength / 100.0;
        let band = self.focal_range / 100.0;
        let radius = |x: usize, y: usize| -> f64 {
            let Some((d, subject)) = depth_at(x, y) else {
                return 0.0;
            };
            if band >= 1.0 {
                return 0.0;
            }
            strength * (((d - focus).abs() - band) / (1.0 - band)).clamp(0.0, 1.0) * (1.0 - subject)
        };
        let stage = job.range(0.3, 0.8);
        let mut out: Vec<[f32; 4]> = if self.depth_only {
            let mut out = reserved_vec(n)?;
            for i in 0..n {
                stage.every(i)?;
                let source = read(i);
                out.push(match depth_at(i % w, i / w) {
                    None => source,
                    Some((d, _)) => {
                        let g = (1.0 - d) as f32;
                        let grey = [g, g, g, source[3]];
                        match encoding {
                            Some(e) => e.decode(grey),
                            None => grey,
                        }
                    }
                });
            }
            return Ok(out);
        } else if self.blurs() {
            let disc = disc_plane(
                &read,
                dimensions,
                radius,
                Self::max_radius(dimensions),
                None,
                range.alpha_floor(),
                stage,
            )?;
            // Box Blur's straight combine, through the one implementation.
            let combine = SpatialKernel::BoxBlur { radius: 1 };
            let mut disc = disc;
            for (i, sample) in disc.iter_mut().enumerate() {
                stage.every(i)?;
                *sample = if radius(i % w, i / w) > 0.0 {
                    combine.combine_in(read(i), *sample, range)
                } else {
                    read(i)
                };
            }
            disc
        } else {
            let mut out = reserved_vec(n)?;
            for i in 0..n {
                stage.every(i)?;
                out.push(read(i));
            }
            out
        };
        let grade = self.grade();
        let front = if extended {
            Some(grade.linear_front()?)
        } else {
            None
        };
        let back = if extended { grade.finish_op()? } else { None };
        let haze = self.haze / 100.0 * HAZE_MAX;
        for y in 0..h {
            job.check()?;
            for x in 0..w {
                let p = &mut out[y * w + x];
                if p[3] <= 0.0 {
                    continue;
                }
                if haze > 0.0 {
                    if let Some((d, _)) = depth_at(x, y) {
                        let t = haze * d;
                        for c in p.iter_mut().take(3) {
                            *c = (f64::from(*c) + t * (1.0 - f64::from(*c))) as f32;
                        }
                    }
                }
                if let Some(front) = &front {
                    let rgb = front.apply([p[0], p[1], p[2]]);
                    *p = [rgb[0], rgb[1], rgb[2], p[3]];
                }
                if let (Some(op), Some(e)) = (&back, encoding) {
                    let rgb = e.scaled([p[0], p[1], p[2]], |c| op.apply(c));
                    *p = [rgb[0], rgb[1], rgb[2], p[3]];
                }
            }
        }
        if !extended && !grade.identity() {
            let toned =
                grade.plane_in(|i| out[i], dimensions, u64::MAX, range, job.range(0.8, 0.9))?;
            out = toned;
        }
        if self.grain > 0.0 {
            let kernel = self.grain_kernel();
            let grain =
                kernel.plane_in(|i| out[i], dimensions, u64::MAX, range, job.range(0.9, 1.0))?;
            for (p, g) in out.iter_mut().zip(&grain) {
                *p = match encoding {
                    Some(e) => {
                        let code = e.encode(*p);
                        e.decode_changed(*p, code, kernel.combine_in(code, *g, range))
                    }
                    None => kernel.combine_in(*p, *g, range),
                };
            }
        }
        Ok(out)
    }
}
