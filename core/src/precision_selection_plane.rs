//! Native selection-plane edits over shared scalar geometry/Gaussian/distance
//! kernels. Scalar coverage is never premultiplied by a fabricated RGBA alpha.
//!
//! Independent u16 selection transfer combines/inverts/paints native coverage and shares Gaussian,
//! Euclidean contour and affine kernels; byte contracts keep their rounding.
use crate::doc_transform::{Affine, SourceMap};
use crate::gradient_field::{FieldStop, FieldStyle};
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;
use crate::precision_mask::PrecisionPlane;
use crate::precision_sampling::{transform_plane, GeometryFilter};
use crate::precision_selection_support as support;
use crate::selected_pixels_transform::CanvasMap;
use crate::style_model::StrokePosition;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlaneOperation {
    Blend {
        channels: usize,
        mode: i32,
        opacity: f64,
        #[serde(default)]
        invert_base: bool,
        #[serde(default)]
        invert_source: bool,
    },
    Luminosity {
        band: crate::precision_plane_math::LuminosityBand,
        level: u8,
    },
    Invert {},
    Combine {
        mode: CombineMode,
    },
    Feather {
        radius: f32,
    },
    Smooth {
        radius: f32,
    },
    Grow {
        radius: f32,
    },
    Shrink {
        radius: f32,
    },
    Border {
        radius: f32,
    },
    /// Edit ▸ Stroke…'s band at u16 ([`stroke_band`]): `width` pixels at
    /// `location` around the coverage's 50 % contour.
    Stroke {
        width: f32,
        location: StrokePosition,
    },
    Transform {
        matrix: [f64; 6],
        filter: GeometryFilter,
    },
    /// The map taking `source_rect` onto `quad` (TL, TR, BR, BL) — the
    /// selection half of a perspective Free Transform of selected pixels.
    Perspective {
        source_rect: [f64; 4],
        quad: [f64; 8],
        filter: GeometryFilter,
    },
    Paint {
        value: Option<u16>,
    },
    Refine {
        #[serde(default)]
        smooth: f32,
        #[serde(default)]
        feather: f32,
        #[serde(default)]
        shift: f32,
        #[serde(default)]
        contrast: f32,
    },
    /// A stops gradient painted over one canvas coverage plane (the
    /// Gradient tool in Quick Mask): `gradient_field::scalar_over` at each
    /// sample centre, the drag `from → to` in the plane's own coordinates.
    /// Stops are `[g, g, g, alpha]`, `g` the coverage gray over 65535;
    /// `smoothness` is the gradient's Smoothness as a fraction (1
    /// when absent, the Classic ramp exactly).
    Gradient {
        from: [f64; 2],
        to: [f64; 2],
        #[serde(default = "linear")]
        style: FieldStyle,
        stops: Vec<FieldStop>,
        #[serde(default = "full")]
        opacity: f64,
        #[serde(default = "crate::gradient_field::full_smoothness")]
        smoothness: f32,
    },
}
fn linear() -> FieldStyle {
    FieldStyle::Linear
}
fn full() -> f64 {
    1.
}
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CombineMode {
    Replace,
    Add,
    Subtract,
    Intersect,
}
impl PlaneOperation {
    pub(crate) fn planes(&self) -> usize {
        match self {
            Self::Blend { channels, .. } => channels.saturating_mul(2),
            Self::Paint { value: None } => 3,
            Self::Combine { .. } | Self::Paint { .. } => 2,
            _ => 1,
        }
    }
    pub(crate) fn output_planes(&self) -> usize {
        if let Self::Blend { channels, .. } = self {
            *channels
        } else {
            1
        }
    }
    pub(crate) fn plan(&self, dimensions: (u32, u32)) -> Result<u64, String> {
        let n = u64::from(dimensions.0) * u64::from(dimensions.1);
        // Validation only: no kind carries a work refusal. Each runs
        // under its caller's job control, bounded by the scratch below.
        match self {
            Self::Blend {
                channels,
                mode,
                opacity,
                ..
            } => {
                crate::precision_plane_math::validate(*channels, *mode, *opacity)?;
            }
            Self::Luminosity { level, .. } => {
                if !(1..=3).contains(level) {
                    return Err("luminosity level must be1...3".into());
                }
            }
            Self::Feather { radius }
            | Self::Smooth { radius }
            | Self::Grow { radius }
            | Self::Shrink { radius }
            | Self::Border { radius } => {
                if !radius.is_finite() || !(0. ..=250.).contains(radius) {
                    return Err("selection radius must be finite in [0,250]".into());
                }
            }
            Self::Stroke { width, .. } => {
                if !width.is_finite() || *width <= 0. || *width > 250. {
                    return Err("selection stroke width must be finite in (0,250]".into());
                }
            }
            Self::Transform { matrix, .. } => {
                Affine::from_array(*matrix)
                    .inverse()
                    .ok_or("selection transform must be finite and invertible")?;
            }
            Self::Perspective {
                source_rect, quad, ..
            } => {
                CanvasMap::quad(*source_rect, *quad)?.source_map((0, 0), (0, 0))?;
            }
            Self::Refine {
                smooth,
                feather,
                shift,
                contrast,
            } => {
                crate::precision_selection_refine::validate(*smooth, *feather, *shift, *contrast)?;
            }
            Self::Gradient {
                from,
                to,
                stops,
                opacity,
                smoothness,
                ..
            } => {
                crate::gradient_field::validate(*from, *to)?;
                crate::gradient_field::validate_smoothness(*smoothness)?;
                if !crate::style_json::GRADIENT_STOPS.contains(&stops.len()) {
                    return Err("selection gradient stops must have 2 to 32 entries".into());
                }
                crate::gradient_field::validate_stops(stops)?;
                if stops
                    .iter()
                    .any(|s| s.color.iter().any(|v| !(0. ..=1.).contains(v)))
                {
                    return Err("selection gradient stop values must be in [0,1]".into());
                }
                if !opacity.is_finite() || !(0. ..=1.).contains(opacity) {
                    return Err("selection gradient opacity must be finite in [0,1]".into());
                }
            }
            _ => {}
        }
        // A composed refinement also keeps one inter-stage plane alive.
        let staged = if matches!(self, Self::Refine { .. }) {
            2 * n
        } else {
            0
        };
        Ok(n * 24 + 24 * u64::from(dimensions.0.max(dimensions.1)) + 4096 + staged)
    }
    pub(crate) fn apply(
        &self,
        input: &[u16],
        dimensions: (u32, u32),
        job: JobContext<'_>,
    ) -> Result<Vec<u16>, String> {
        self.plan(dimensions)?;
        let (w, h) = dimensions;
        let n = w as usize * h as usize;
        if input.len() != n * self.planes() {
            return Err("native selection input count differs from plan".into());
        }
        if let Self::Blend {
            channels,
            mode,
            opacity,
            invert_base,
            invert_source,
        } = self
        {
            return crate::precision_plane_math::blend(
                input,
                dimensions,
                *channels,
                *mode,
                *opacity,
                *invert_base,
                *invert_source,
                job,
            );
        }
        if let Self::Refine {
            smooth,
            feather,
            shift,
            contrast,
        } = self
        {
            return crate::precision_selection_refine::refine(
                input, dimensions, *smooth, *feather, *shift, *contrast, job,
            );
        }
        if let Self::Transform { matrix, filter } = self {
            let source = PrecisionPlane::copy(w, h, input, job)?;
            let inverse = Affine::from_array(*matrix)
                .inverse()
                .ok_or("invalid selection transform")?;
            let map = SourceMap::new(&inverse, (0, 0), (0, 0));
            return copy(
                transform_plane(&source, dimensions, &map, *filter, job)?.samples(),
                job,
            );
        }
        if let Self::Perspective {
            source_rect,
            quad,
            filter,
        } = self
        {
            let source = PrecisionPlane::copy(w, h, input, job)?;
            let map = CanvasMap::quad(*source_rect, *quad)?.source_map((0, 0), (0, 0))?;
            return copy(
                transform_plane(&source, dimensions, &map, *filter, job)?.samples(),
                job,
            );
        }
        if let Some(reach) = support::reach(self) {
            let Some(bounds) = support::support(input, w, h, job)? else {
                return support::embed(&[], w, h, (0, 0, 0, 0), job);
            };
            let window = support::window(bounds, reach, w, h);
            let crop = support::crop(input, w, window, job)?;
            let out =
                self.spatial_kernel(&crop, (window.2 - window.0, window.3 - window.1), job)?;
            drop(crop);
            return support::embed(&out, w, h, window, job);
        }
        let mut out = reserved_vec(n)?;
        match self {
            Self::Luminosity { band, level } => {
                for (i, &v) in input.iter().enumerate() {
                    job.every(i)?;
                    out.push(
                        (crate::doc_channel::luminosity_coverage(
                            f32::from(v) / 65535.,
                            band.code(),
                            i32::from(*level),
                        )
                        .clamp(0., 1.)
                            * 65535.)
                            .round() as u16,
                    );
                }
            }
            Self::Invert {} => {
                for (i, &v) in input.iter().enumerate() {
                    job.every(i)?;
                    out.push(u16::MAX - v);
                }
            }
            Self::Combine { mode } => {
                for i in 0..n {
                    job.every(i)?;
                    let (a, b) = (input[i], input[n + i]);
                    out.push(match mode {
                        CombineMode::Replace => b,
                        CombineMode::Add => a.max(b),
                        CombineMode::Subtract => a.min(65535 - b),
                        CombineMode::Intersect => a.min(b),
                    });
                }
            }
            Self::Gradient {
                from,
                to,
                style,
                stops,
                opacity,
                smoothness,
            } => {
                for (i, &v) in input.iter().enumerate() {
                    job.every(i)?;
                    let point = [(i % w as usize) as f64 + 0.5, (i / w as usize) as f64 + 0.5];
                    let t = crate::gradient_field::styled_parameter(point, *from, *to, *style);
                    out.push(crate::gradient_field::scalar_over(
                        v,
                        stops,
                        t as f32,
                        *opacity,
                        *style,
                        *smoothness,
                    ));
                }
            }
            Self::Paint { value } => {
                for i in 0..n {
                    job.every(i)?;
                    out.push(if let Some(value) = value {
                        mix(input[i], *value, input[n + i])
                    } else {
                        let gray = input[n + i];
                        let alpha = input[2 * n + i];
                        if gray > alpha {
                            return Err("premultiplied selection gray exceeds alpha".into());
                        }
                        crate::selection_paint::paint_scalar(
                            u64::from(input[i]),
                            u64::from(gray),
                            u64::from(alpha),
                            65535,
                        ) as u16
                    });
                }
            }
            _ => unreachable!(),
        }
        job.check()?;
        Ok(out)
    }
    /// Today's Feather/Smooth and Grow/Shrink/Border arms, statement for
    /// statement, over `input` of `dimensions` — the ONE kernel body `apply`
    /// runs over the support window and `reference_apply` over the canvas.
    /// Stroke is [`stroke_band`], built from the Grow/Shrink arms.
    pub(crate) fn spatial_kernel(
        &self,
        input: &[u16],
        dimensions: (u32, u32),
        job: JobContext<'_>,
    ) -> Result<Vec<u16>, String> {
        if let Self::Stroke { width, location } = *self {
            return stroke_band(input, dimensions, width, location, job);
        }
        let (w, h) = dimensions;
        let n = w as usize * h as usize;
        let mut out = reserved_vec(n)?;
        match self {
            Self::Feather { radius } | Self::Smooth { radius } => {
                if *radius == 0. {
                    return copy(input, job);
                }
                let mut plane = reserved_vec(n)?;
                for (i, &value) in input.iter().enumerate() {
                    job.every(i)?;
                    plane.push(f32::from(value) / 65535.);
                }
                crate::gaussian_plane::feather(&mut plane, w, h, *radius, job)?;
                for (i, mut value) in plane.into_iter().enumerate() {
                    job.every(i)?;
                    if matches!(self, Self::Smooth { .. }) {
                        value = value * value * (3. - 2. * value);
                    }
                    out.push((value.clamp(0., 1.) * 65535.).round() as u16);
                }
            }
            Self::Grow { radius } | Self::Shrink { radius } | Self::Border { radius } => {
                if *radius == 0. {
                    return copy(input, job);
                }
                let sdf = crate::doc_select::signed_distance_from_membership(
                    w as usize,
                    h as usize,
                    |i| input[i] >= 32768,
                    job,
                )?;
                for (i, s) in sdf.into_iter().enumerate() {
                    job.every(i)?;
                    let d = match self {
                        Self::Grow { .. } => s + radius,
                        Self::Shrink { .. } => s - radius,
                        _ => radius * 0.5 - s.abs(),
                    };
                    out.push(((d + 0.5).clamp(0., 1.) * 65535.).round() as u16);
                }
            }
            _ => {
                return Err(
                    "spatial kernel covers feather, smooth, grow, shrink, border and stroke".into(),
                )
            }
        }
        job.check()?;
        Ok(out)
    }
}
/// Edit ▸ Stroke…'s band over `input` of `dimensions`, `width` pixels at
/// `location` — the u16 twin of `doc_fill_stroke::stroke_mask`, the BYTE
/// MASTER whose doc comment is this function's specification. The bands
/// come from the Grow/Shrink arms of [`PlaneOperation::spatial_kernel`]:
///
/// - Outside: `grow(w) · (65535 − cov) / 65535`, rounded.
/// - Inside: `max(cov ⊖ shrink(w), cov ⊖ cov16((k + 1) − 0.5 − w))`, `⊖`
///   saturating and `k` the whole-pixel distance to the plane's edge — the
///   byte master's canvas-edge ring, so Select All ▸ Inside draws a frame.
/// - Center: Outside at `w/2` plus Inside at `w/2`, saturating.
fn stroke_band(
    input: &[u16],
    dimensions: (u32, u32),
    width: f32,
    location: StrokePosition,
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    match location {
        StrokePosition::Outside => outside_band(input, dimensions, width, job),
        StrokePosition::Inside => inside_band(input, dimensions, width, job),
        StrokePosition::Center => {
            let half = width / 2.;
            let mut band = outside_band(input, dimensions, half, job)?;
            let inner = inside_band(input, dimensions, half, job)?;
            for (i, (b, v)) in band.iter_mut().zip(inner).enumerate() {
                job.every(i)?;
                *b = b.saturating_add(v);
            }
            Ok(band)
        }
    }
}

/// `grow(px)` knocked out by the coverage (`stroke_band`'s Outside).
fn outside_band(
    input: &[u16],
    dimensions: (u32, u32),
    px: f32,
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    let grown = PlaneOperation::Grow { radius: px }.spatial_kernel(input, dimensions, job)?;
    let mut out = reserved_vec(grown.len())?;
    for (i, (&g, &c)) in grown.iter().zip(input).enumerate() {
        job.every(i)?;
        out.push(((u64::from(g) * u64::from(65535 - c) + 32767) / 65535) as u16);
    }
    Ok(out)
}

/// `cov ⊖ shrink(px)` joined with the plane-edge ring (`stroke_band`'s
/// Inside; the byte master's `inside`).
fn inside_band(
    input: &[u16],
    dimensions: (u32, u32),
    px: f32,
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    let shrunk = PlaneOperation::Shrink { radius: px }.spatial_kernel(input, dimensions, job)?;
    let (w, h) = (dimensions.0 as usize, dimensions.1 as usize);
    let mut out = reserved_vec(shrunk.len())?;
    for (i, (&s, &c)) in shrunk.iter().zip(input).enumerate() {
        job.every(i)?;
        let (x, y) = (i % w, i / w);
        let k = x.min(w - 1 - x).min(y).min(h - 1 - y);
        let edge = (((k + 1) as f32 - 0.5 - px + 0.5).clamp(0., 1.) * 65535.).round() as u16;
        out.push(c.saturating_sub(s).max(c.saturating_sub(edge)));
    }
    Ok(out)
}

pub(crate) fn copy(input: &[u16], job: JobContext<'_>) -> Result<Vec<u16>, String> {
    let mut out = reserved_vec(input.len())?;
    for chunk in input.chunks(4096) {
        job.check()?;
        out.extend_from_slice(chunk);
    }
    Ok(out)
}
/// Positive integer scalar interpolation preserves both endpoints and tiny
/// contributions. This is the u16 specialization of doc_scope's scalar lerp.
pub(crate) fn mix(a: u16, b: u16, weight: u16) -> u16 {
    let w = u64::from(weight);
    ((u64::from(a) * (65535 - w) + u64::from(b) * w + 32767) / 65535) as u16
}

/// The retained WHOLE-PLANE reference (the 13A-6a oracle): parse `request`,
/// validate as `plan` does, then run Feather/Smooth/Grow/Shrink/Border/Stroke through
/// `spatial_kernel` over the whole canvas behind the pre-13A-6a canvas work
/// check, and Refine through `precision_selection_refine::reference_refine`.
/// No job control. A black-box seam for tests only.
#[doc(hidden)]
pub fn reference_apply(
    request: &str,
    input: &[u16],
    width: u32,
    height: u32,
) -> Result<Vec<u16>, String> {
    let op: PlaneOperation = serde_json::from_str(request)
        .map_err(|e| format!("invalid native selection operation: {e}"))?;
    let dimensions = (width, height);
    op.plan(dimensions)?;
    if input.len() != width as usize * height as usize * op.planes() {
        return Err("native selection input count differs from plan".into());
    }
    if let PlaneOperation::Refine {
        smooth,
        feather,
        shift,
        contrast,
    } = op
    {
        return crate::precision_selection_refine::reference_refine(
            input, dimensions, smooth, feather, shift, contrast,
        );
    }
    if support::reach(&op).is_none() {
        return Err("reference covers spatial kinds and refine only".into());
    }
    if support::work(&op, (0, 0, width, height)) > crate::precision_render::MAX_WORK_UNITS {
        return Err("native selection exceeds the work limit".into());
    }
    op.spatial_kernel(input, dimensions, JobContext::NONE)
}
