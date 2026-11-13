//! Strict inverse source-coordinate maps shared by RAW images and RZLM coverage.
//!
//! Source sampling follows local composition and its single vignette quantization. An optional
//! registered vignette reference changes only its coordinate field; absent reference retains the
//! historical arithmetic. Source replacement precomposes the inverse coordinate map with existing
//! affine/projective binding; no new native framing is added. Its source bind (`:392`) and Seeded
//! output (`:466`) are HEAD's after 13B-3e-3b: the bind moves onto one transient in 13B-3e-4 with
//! the host-intake seeds, and the output feeds the develop preview's kept view (13B-3e-3b D1/D4).
//! Since 13B-3e-4a the source is a `storage()` gate at HEAD's position (HEAD's sentence) and ONE
//! `transient_rgba16` after the `plan.unchanged` return (an identity copies nothing; a failed copy
//! answers "could not allocate precision output"); the output (now `:471`) stays Seeded, feeding
//! `preview_view_rgba16`, kept exception by decision (13B-3e-4c-1 D2).
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, reserved_vec, PrecisionImage, BOOKKEEPING_BYTES};
use crate::resources::Inventory;
use crate::sampling::Kernel;
use serde::Deserialize;
use serde_json::{json, Value};

const IDENTITY: [f64; 6] = [1., 0., 0., 1., 0., 0.];

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Filter {
    Bilinear,
    Bicubic,
}
impl Filter {
    pub(crate) fn kernel(self) -> Kernel {
        match self {
            Self::Bilinear => Kernel::Bilinear,
            Self::Bicubic => Kernel::Bicubic,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RawSourceMap {
    Exif {
        version: u32,
        orientation: u32,
    },
    Affine {
        version: u32,
        width: u32,
        height: u32,
        inverse: [f64; 6],
        filter: Filter,
    },
    Lens {
        version: u32,
        profile_id: String,
        center: [f64; 2],
        radial: [f64; 3],
        red_scale: f64,
        blue_scale: f64,
        filter: Filter,
        reference_size: Option<[u32; 2]>,
        reference_from_source: Option<[f64; 6]>,
    },
}

#[derive(Clone, Copy)]
pub(crate) struct ExactMap(pub [i64; 6]);
impl ExactMap {
    pub(crate) fn source(self, x: u32, y: u32) -> (i64, i64) {
        let m = self.0;
        let x = i64::from(x);
        let y = i64::from(y);
        // The affine is in edge coordinates. For signed unit axes, a
        // destination centre maps onto a source centre exactly.
        (
            m[0] * x + m[2] * y + m[4] + (m[0] + m[2] - 1) / 2,
            m[1] * x + m[3] * y + m[5] + (m[1] + m[3] - 1) / 2,
        )
    }
}

pub(crate) struct MapPlan {
    pub(crate) map: RawSourceMap,
    pub(crate) source: (u32, u32),
    pub(crate) destination: (u32, u32),
    pub(crate) exact: Option<ExactMap>,
    pub(crate) unchanged: bool,
    pub(crate) scratch: u64,
    pub(crate) work: u64,
}
impl MapPlan {
    pub(crate) fn report(&self) -> Value {
        json!({"width":self.destination.0,"height":self.destination.1,
            "scratch_bytes":self.scratch,"work_units":self.work,
            "exact":self.exact.is_some(),"unchanged":self.unchanged})
    }
    pub(crate) fn source_at(&self, x: f64, y: f64, channel: u32) -> Result<[f64; 2], String> {
        match &self.map {
            RawSourceMap::Exif { .. } | RawSourceMap::Affine { .. } => {
                let m = self.map.affine(self.source).ok_or("missing affine map")?;
                Ok([m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]])
            }
            RawSourceMap::Lens {
                center,
                radial,
                red_scale,
                blue_scale,
                reference_size,
                reference_from_source,
                ..
            } => {
                let [rw, rh] = reference_size.unwrap_or([self.source.0, self.source.1]);
                let b = reference_from_source.unwrap_or(IDENTITY);
                let c = [center[0] * f64::from(rw), center[1] * f64::from(rh)];
                let radius = f64::from(rw.max(rh)) / 2.0;
                let p = [b[0] * x + b[2] * y + b[4], b[1] * x + b[3] * y + b[5]];
                let scale = match channel {
                    0 => *red_scale,
                    1 => 1.0,
                    2 => *blue_scale,
                    _ => return Err("invalid RAW geometry channel".into()),
                };
                let mapped = crate::lens_model::radial_map(c, radius, p, *radial, scale);
                let det = b[0] * b[3] - b[1] * b[2];
                let px = mapped[0] - b[4];
                let py = mapped[1] - b[5];
                Ok([
                    (b[3] * px - b[2] * py) / det,
                    (-b[1] * px + b[0] * py) / det,
                ])
            }
        }
    }
    pub(crate) fn kernel(&self) -> Kernel {
        match &self.map {
            RawSourceMap::Exif { .. } => Kernel::Bilinear,
            RawSourceMap::Affine { filter, .. } | RawSourceMap::Lens { filter, .. } => {
                filter.kernel()
            }
        }
    }
}

fn valid_affine(m: [f64; 6]) -> Result<(), String> {
    if !m.iter().all(|v| v.is_finite())
        || m[..4].iter().any(|v| v.abs() > 1e6)
        || m[4..].iter().any(|v| v.abs() > 1e9)
        || (m[0] * m[3] - m[1] * m[2]).abs() < 1e-12
    {
        return Err("RAW source matrix is nonfinite, singular or outside bounds".into());
    }
    Ok(())
}
fn exact_affine(m: [f64; 6]) -> Option<ExactMap> {
    if !m.iter().all(|v| v.fract() == 0.0 && v.abs() <= 1e9) {
        return None;
    }
    let a = m.map(|v| v as i64);
    if a[..4].iter().all(|v| v.abs() <= 1)
        && a[0].abs() + a[2].abs() == 1
        && a[1].abs() + a[3].abs() == 1
        && a[0].abs() + a[1].abs() == 1
        && a[2].abs() + a[3].abs() == 1
    {
        Some(ExactMap(a))
    } else {
        None
    }
}
impl RawSourceMap {
    pub(crate) fn parse(request: &str) -> Result<Self, String> {
        if request.len() > 4096 {
            return Err("RAW source map exceeds 4096 bytes".into());
        }
        serde_json::from_str(request).map_err(|e| format!("invalid RAW source map: {e}"))
    }
    pub(crate) fn affine(&self, source: (u32, u32)) -> Option<[f64; 6]> {
        let (w, h) = (f64::from(source.0), f64::from(source.1));
        match self {
            Self::Affine { inverse, .. } => Some(*inverse),
            Self::Exif { orientation, .. } => Some(match orientation {
                1 => IDENTITY,
                2 => [-1., 0., 0., 1., w, 0.],
                3 => [-1., 0., 0., -1., w, h],
                4 => [1., 0., 0., -1., 0., h],
                5 => [0., 1., 1., 0., 0., 0.],
                6 => [0., -1., 1., 0., 0., h],
                7 => [0., -1., -1., 0., w, h],
                8 => [0., 1., -1., 0., w, 0.],
                _ => return None,
            }),
            Self::Lens { .. } => None,
        }
    }
    pub(crate) fn plan(
        self,
        source: (u32, u32),
        channel: u32,
        image: bool,
    ) -> Result<MapPlan, String> {
        if channel > 2 || (channel != 1 && !matches!(self, Self::Lens { .. })) {
            return Err("RAW source map channel is unsupported".into());
        }
        crate::raw_local_mask::dimensions(source.0, source.1)?;
        let destination = match &self {
            Self::Exif {
                version,
                orientation,
            } => {
                if *version != 1 || !(1..=8).contains(orientation) {
                    return Err("unsupported RAW source map version or EXIF orientation".into());
                }
                if *orientation >= 5 {
                    (source.1, source.0)
                } else {
                    source
                }
            }
            Self::Affine {
                version,
                width,
                height,
                inverse,
                ..
            } => {
                if *version != 1 {
                    return Err("unsupported RAW source map version".into());
                }
                valid_affine(*inverse)?;
                (*width, *height)
            }
            Self::Lens {
                version,
                profile_id,
                center,
                radial,
                red_scale,
                blue_scale,
                reference_size,
                reference_from_source,
                ..
            } => {
                if *version != 1 {
                    return Err("unsupported RAW source map version".into());
                }
                if profile_id.is_empty()
                    || profile_id.len() > 128
                    || profile_id.contains('\0')
                    || !center
                        .iter()
                        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                    || !radial
                        .iter()
                        .all(|v| v.is_finite() && (-1.0..=1.0).contains(v))
                    || ![red_scale, blue_scale]
                        .iter()
                        .all(|v| v.is_finite() && (0.9..=1.1).contains(*v))
                    || reference_size.is_some() != reference_from_source.is_some()
                {
                    return Err("invalid RAW lens profile or reference registration".into());
                }
                let [rw, rh] = reference_size.unwrap_or([source.0, source.1]);
                crate::raw_local_mask::dimensions(rw, rh)?;
                let b = reference_from_source.unwrap_or(IDENTITY);
                valid_affine(b)?;
                let c = [center[0] * f64::from(rw), center[1] * f64::from(rh)];
                let r = f64::from(rw.max(rh)) / 2.0;
                let mut rmax2: f64 = 0.0;
                for x in [0.0, f64::from(source.0)] {
                    for y in [0.0, f64::from(source.1)] {
                        let dx = (b[0] * x + b[2] * y + b[4] - c[0]) / r;
                        let dy = (b[1] * x + b[3] * y + b[5] - c[1]) / r;
                        rmax2 = rmax2.max(dx * dx + dy * dy);
                    }
                }
                if crate::lens_model::radial_folds(*radial, rmax2) {
                    return Err("RAW lens profile folds or exceeds its reference bounds".into());
                }
                source
            }
        };
        let n = crate::raw_local_mask::dimensions(destination.0, destination.1)?;
        if let Some(m) = self.affine(source) {
            valid_affine(m)?;
            for x in [0.0, f64::from(destination.0)] {
                for y in [0.0, f64::from(destination.1)] {
                    let p = [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
                    if p.iter().any(|v| !v.is_finite() || v.abs() > 1e9) {
                        return Err("RAW source map corners exceed coordinate bounds".into());
                    }
                }
            }
        }
        let exact = self
            .affine(source)
            .and_then(exact_affine)
            .or_else(|| match &self {
                Self::Lens {
                    radial,
                    red_scale,
                    blue_scale,
                    ..
                } if *radial == [0.0; 3] && *red_scale == 1.0 && *blue_scale == 1.0 => {
                    exact_affine(IDENTITY)
                }
                _ => None,
            });
        let unchanged = destination == source && exact.is_some_and(|v| v.0 == [1, 0, 0, 1, 0, 0]);
        let taps: u64 = if exact.is_some() {
            1
        } else {
            match &self {
                Self::Affine {
                    filter: Filter::Bicubic,
                    ..
                }
                | Self::Lens {
                    filter: Filter::Bicubic,
                    ..
                } => 16,
                _ => 4,
            }
        };
        let work = (n as u64)
            .checked_mul(taps)
            .and_then(|v| v.checked_mul(if image { 3 } else { 1 }))
            .ok_or("RAW source map work overflows")?;
        let scratch = 8 * BOOKKEEPING_BYTES
            + if unchanged {
                0
            } else {
                (n as u64)
                    * if image && exact.is_none() {
                        56
                    } else if image {
                        8
                    } else {
                        2
                    }
                    + if image {
                        0
                    } else {
                        crate::raw_local_mask::RAW_LOCAL_HEADER as u64
                    }
            };
        Ok(MapPlan {
            map: self,
            source,
            destination,
            exact,
            unchanged,
            scratch,
            work,
        })
    }
}

impl PrecisionDocument {
    pub fn raw_source_geometry_plan(
        &self,
        request: &str,
        limit: u64,
        other: u64,
    ) -> Result<Value, String> {
        let image = crate::raw_local_develop::plain_image(self)?;
        if image.storage() == crate::precision_image::PrecisionStorage::Rgba32Float {
            return Ok(self.float_geometry_plan(request, limit, other)?.report());
        }
        if image.storage() != crate::precision_image::PrecisionStorage::Rgba16Unorm {
            return Err(
                "RAW source geometry requires genuine RGBA16 storage; byte/float unsupported"
                    .into(),
            );
        }
        let plan = RawSourceMap::parse(request)?.plan(self.dimensions(), 1, true)?;
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        admit(inventory.unique_bytes(), plan.scratch, other, limit)?;
        Ok(plan.report())
    }
    /// The plan of an f32 source (a 32-bit float development): the u16 plan
    /// with the float arm's scratch ([`float_scratch`]), admitted.
    fn float_geometry_plan(
        &self,
        request: &str,
        limit: u64,
        other: u64,
    ) -> Result<MapPlan, String> {
        let mut plan = RawSourceMap::parse(request)?.plan(self.dimensions(), 1, true)?;
        plan.scratch = float_scratch(&plan)?;
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        admit(inventory.unique_bytes(), plan.scratch, other, limit)?;
        Ok(plan)
    }
    pub fn raw_source_geometry(
        &self,
        request: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let image = crate::raw_local_develop::plain_image(self)?;
        if image.storage() == crate::precision_image::PrecisionStorage::Rgba32Float {
            let plan = self.float_geometry_plan(request, limit, other)?;
            job.reserve(plan.scratch)?;
            if plan.unchanged {
                job.report(1.0, 5)?;
                return Ok(None);
            }
            return float_geometry(image, &plan, job).map(Some);
        }
        if image.storage() != crate::precision_image::PrecisionStorage::Rgba16Unorm {
            return Err(
                "RAW source geometry requires genuine RGBA16 storage; byte/float unsupported"
                    .into(),
            );
        }
        let plan = RawSourceMap::parse(request)?.plan(self.dimensions(), 1, true)?;
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        admit(inventory.unique_bytes(), plan.scratch, other, limit)?;
        job.reserve(plan.scratch)?;
        if plan.unchanged {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        let source = image.transient_rgba16()?;
        let (dw, dh) = plan.destination;
        let n = crate::raw_local_mask::dimensions(dw, dh)?;
        let mut output = reserved_vec(n)?;
        if let Some(exact) = plan.exact {
            for y in 0..dh {
                job.check()?;
                for x in 0..dw {
                    job.every(x as usize)?;
                    let (sx, sy) = exact.source(x, y);
                    output.push(
                        if sx >= 0
                            && sy >= 0
                            && sx < i64::from(plan.source.0)
                            && sy < i64::from(plan.source.1)
                        {
                            source[sy as usize * plan.source.0 as usize + sx as usize]
                        } else {
                            [0; 4]
                        },
                    );
                }
                job.report(f64::from(y + 1) / f64::from(dh), 6)?;
            }
        } else {
            let mut channels = [Vec::new(), Vec::new(), Vec::new()];
            for channel in 0..3 {
                let mut values = reserved_vec(n)?;
                values.resize(n, [0.0_f64; 2]);
                crate::sampling::map_into::<_, _, 2>(
                    plan.source,
                    plan.destination,
                    plan.kernel(),
                    |x, y, _| plan.source_at(x, y, channel),
                    |i| source[i],
                    |acc, p, w| {
                        let alpha = f64::from(p[3]) / 65535.0;
                        acc[0] += (f64::from(p[channel as usize]) / 65535.0) * alpha * w;
                        acc[1] += alpha * w;
                    },
                    |acc| acc,
                    |i, p| values[i] = p,
                    job,
                )?;
                channels[channel as usize] = values;
            }
            for (i, green) in channels[1].iter().enumerate().take(n) {
                job.every(i)?;
                let alpha = (green[1].clamp(0.0, 1.0) * 65535.0).round() as u16;
                let mut p = [0; 4];
                p[3] = alpha;
                if alpha > 0 {
                    for channel in 0..3 {
                        let a = channels[channel][i][1];
                        if a > 0.0 {
                            p[channel] = ((channels[channel][i][0] / a).clamp(0.0, 1.0) * 65535.0)
                                .round() as u16;
                        }
                    }
                }
                output.push(p);
            }
        }
        job.check()?;
        // Seeded: feeds `preview_view_rgba16` (kept exception, 13B-3e-4c-1 D2).
        let result = PrecisionImage::from_rgba16(dw, dh, output, image.profile().clone(), None)?;
        let doc = PrecisionDocument::from_image(
            result,
            crate::metadata::Metadata::default(),
            crate::metadata::Resolution::default(),
        )?;
        job.report(1.0, 5)?;
        Ok(Some(doc))
    }
}

/// The f32 arm's working memory for `plan`: nothing beyond the bookkeeping
/// for an identity, else the one transient copy of the source (16 bytes a
/// source pixel) and, per destination pixel, 32 bytes for an exact map (the
/// output plane and the tiles it is copied into) or 80 for a resample (the
/// three channel accumulators, 16 bytes each, beside the output plane; the
/// accumulators are dropped before the tiles are built, so 80 covers both
/// phases). The u16 arm's 8 and 56 are the same terms at 8 bytes a pixel
/// without the tiles, which its measured margin covers.
fn float_scratch(plan: &MapPlan) -> Result<u64, String> {
    if plan.unchanged {
        return Ok(8 * BOOKKEEPING_BYTES);
    }
    let source = crate::raw_local_mask::dimensions(plan.source.0, plan.source.1)? as u64;
    let destination =
        crate::raw_local_mask::dimensions(plan.destination.0, plan.destination.1)? as u64;
    let each = if plan.exact.is_some() { 32 } else { 80 };
    destination
        .checked_mul(each)
        .and_then(|bytes| bytes.checked_add(source.checked_mul(16)?))
        .and_then(|bytes| bytes.checked_add(8 * BOOKKEEPING_BYTES))
        .ok_or_else(|| "RAW source map work overflows".to_string())
}

/// `raw_source_geometry`'s walk over an f32 source (a 32-bit float
/// development), the u16 arm above being the master copy of the maps, the
/// kernel, the per-channel lens coordinates and the premultiplied binary64
/// accumulation. What the storage changes: an exact map copies samples bit
/// for bit; a resample divides by the accumulated alpha and clamps each RGB
/// output to the SOURCE plane's visible range
/// (`precision_sampling::visible_range`, `float_document`'s D3, so a bicubic
/// lobe cannot ring past the brightest highlight or below the darkest
/// sample), clamps alpha to 0…1 and drops an alpha below
/// `sampling::EXTENDED_ALPHA_FLOOR` to transparent. Nothing is quantized. The
/// result passes `from_rgba32_extended`, which refuses and never repairs.
fn float_geometry(
    image: &PrecisionImage,
    plan: &MapPlan,
    job: JobContext<'_>,
) -> Result<PrecisionDocument, String> {
    let source = image.transient_rgba32()?;
    let (dw, dh) = plan.destination;
    let n = crate::raw_local_mask::dimensions(dw, dh)?;
    let mut output: Vec<[f32; 4]> = reserved_vec(n)?;
    if let Some(exact) = plan.exact {
        for y in 0..dh {
            job.check()?;
            for x in 0..dw {
                job.every(x as usize)?;
                let (sx, sy) = exact.source(x, y);
                output.push(
                    if sx >= 0
                        && sy >= 0
                        && sx < i64::from(plan.source.0)
                        && sy < i64::from(plan.source.1)
                    {
                        source[sy as usize * plan.source.0 as usize + sx as usize]
                    } else {
                        [0.0; 4]
                    },
                );
            }
            job.report(f64::from(y + 1) / f64::from(dh), 6)?;
        }
    } else {
        let range = crate::precision_sampling::visible_range(source.iter().copied(), job)?;
        let mut channels = [Vec::new(), Vec::new(), Vec::new()];
        for channel in 0..3 {
            let mut values = reserved_vec(n)?;
            values.resize(n, [0.0_f64; 2]);
            crate::sampling::map_into::<_, _, 2>(
                plan.source,
                plan.destination,
                plan.kernel(),
                |x, y, _| plan.source_at(x, y, channel),
                |i| source[i],
                |acc, p: [f32; 4], w| {
                    let alpha = f64::from(p[3]);
                    acc[0] += f64::from(p[channel as usize]) * alpha * w;
                    acc[1] += alpha * w;
                },
                |acc| acc,
                |i, p| values[i] = p,
                job,
            )?;
            channels[channel as usize] = values;
        }
        for (i, green) in channels[1].iter().enumerate().take(n) {
            job.every(i)?;
            let alpha = green[1].clamp(0.0, 1.0) as f32;
            let mut p = [0.0_f32; 4];
            if alpha >= crate::sampling::EXTENDED_ALPHA_FLOOR {
                p[3] = alpha;
                for channel in 0..3 {
                    let a = channels[channel][i][1];
                    if a > 0.0 {
                        p[channel] = (channels[channel][i][0] / a)
                            .clamp(f64::from(range.min[channel]), f64::from(range.max[channel]))
                            as f32;
                    }
                }
            }
            output.push(p);
        }
    }
    job.check()?;
    let result =
        PrecisionImage::from_rgba32_extended(dw, dh, output, image.profile().clone(), None)?;
    let doc = crate::raw_float_develop::float_document(result, "Background")?;
    job.report(1.0, 5)?;
    Ok(doc)
}
