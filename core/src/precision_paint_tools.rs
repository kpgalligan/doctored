//! Native paint/fill parity through the shared W3C blend and tone kernels.
//! Canvas ink is evaluated once at native depth, then only changed ink expands
//! raw raster storage. Mask placement and every untouched sample stay exact.
//!
//! The walk runs over a [`PaintFrame`]: the canvas for every request this
//! API parses (`plan` / `apply`), or another canvas-space rect for a caller
//! that lays its coverage out there (`precision_fill_stroke`'s layer-edge
//! stroke, whose band may reach past the canvas). Nothing grows under
//! Preserve Transparency (`PaintBlend`'s `preserve_alpha`) or a transparency
//! lock. Under either, the composite is the frozen-alpha rule
//! (`blend::composite_source_frozen`): an alpha-0 pixel stays, a
//! semi-transparent one takes `lerp(Cb, B(Cb, Cs), sa)` at its own alpha.
//! The alpha restore (`PrecisionDocument::restoring_alpha`) still runs
//! after, as a backstop.
//!
//! Native paint shares all 27 blend kernels and continuous authored ICC conversion, bucket matching
//! shares `selection_region`, and dodge/burn shares `doc_retouch::shaped`. Paint/fill expands only
//! to changed canvas ink while preserving raw samples and independent masks; native erasure never
//! expands or modifies hidden RGB. Independent stroke/selection planes multiply before final
//! writeback. `precision_brush_tools.rs` qualifies independent blend/alpha/gradient/tone
//! arithmetic, tiny coverage, exact placement, caps and active cancellation. Since 13B-3d-1
//! `precision_paint_tools` walks only the canvas rect its stroke coverage can change
//! (`coverage_rect16`, clipped to the layer for erase/dodge-burn), reads the layer's part of it and
//! writes it back through `with_rect_rgba16` — bucket fill grows its region over one
//! `transient_rgba16` — and only a GROWN extent (changed ink leaving the layer) still rebuilds
//! whole (`rebuilt_rgba16` from rows — new dimensions, so nothing is shared — which since 13B-3d-3a
//! seeds no view).
use crate::blend::{composite_source_frozen, composite_source_into, BlendMode};
use crate::doc_lock::LOCK_TRANSPARENCY;
use crate::float_paint::PaintPlane;
use crate::gradient_field::{FieldStop, FieldStyle};
use crate::job_control::JobContext;
use crate::lab_blend::kind_in;
use crate::layer_rect::{canvas_rect_on_plane, coverage_rect16, plane_rect_offset};
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, pixel_count, reserved_vec, PrecisionRun, BOOKKEEPING_BYTES};
use crate::precision_paint::{canvas_index, store, PaintColorSpace};
use crate::retouch_transform::{merged_extent, StorageExtent};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum NativePaintEdit {
    PaintBlend {
        layer: usize,
        color: [f32; 4],
        color_space: PaintColorSpace,
        opacity: f32,
        blend_mode: u32,
        #[serde(default)]
        selection: bool,
        #[serde(default)]
        preserve_alpha: bool,
        /// One colour channel of a float layer as the target (`float_paint`).
        #[serde(default)]
        plane: Option<PaintPlane>,
    },
    BucketFill {
        layer: usize,
        color: [f32; 4],
        color_space: PaintColorSpace,
        opacity: f32,
        #[serde(default)]
        blend_mode: u32,
        x: i32,
        y: i32,
        tolerance: u16,
        contiguous: bool,
        #[serde(default)]
        selection: bool,
    },
    Gradient {
        layer: usize,
        start: [f32; 4],
        end: [f32; 4],
        color_space: PaintColorSpace,
        opacity: f32,
        #[serde(default)]
        blend_mode: u32,
        from: [f64; 2],
        to: [f64; 2],
        radial: bool,
        #[serde(default)]
        selection: bool,
        /// A stops gradient: 2..32 stops, each a position and a
        /// straight RGBA colour in `color_space`. Empty is the two-colour
        /// gradient above, whose bytes are unchanged; with stops, `start` /
        /// `end` stay required and carry the first and last stop.
        #[serde(default)]
        stops: Vec<FieldStop>,
        /// The stops gradient's shape; absent is `radial`'s. Only with stops.
        #[serde(default)]
        style: Option<FieldStyle>,
        /// Smoothness as a fraction (`gradient_field::smooth_color`): 1
        /// (absent) is the Classic ramp exactly; below 1 only with stops.
        #[serde(default = "crate::gradient_field::full_smoothness")]
        smoothness: f32,
    },
    DodgeBurn {
        layer: usize,
        exposure: f32,
        range: u8,
        burn: bool,
        #[serde(default)]
        selection: bool,
        /// Protect Tones (#1): the three channels move together through
        /// `doc_retouch::protected`; absent is the per-channel curve.
        #[serde(default)]
        protect_tones: bool,
    },
    EraseNative {
        layer: usize,
        opacity: f32,
        #[serde(default)]
        selection: bool,
    },
}
/// The canvas-space rect a native paint walk's coverage is laid out over:
/// the input is `width·height` samples per plane, row 0 at canvas row `y`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PaintFrame {
    pub x: i64,
    pub y: i64,
    pub width: u32,
    pub height: u32,
}

impl PaintFrame {
    /// The document's canvas: the frame every parsed request uses.
    pub(crate) fn canvas(doc: &PrecisionDocument) -> Self {
        Self {
            x: 0,
            y: 0,
            width: doc.width,
            height: doc.height,
        }
    }

    /// The frame's origin as an `i32` storage offset.
    fn origin(&self) -> Result<(i32, i32), String> {
        let too_large = || "native paint frame exceeds the supported range".to_string();
        Ok((
            i32::try_from(self.x).map_err(|_| too_large())?,
            i32::try_from(self.y).map_err(|_| too_large())?,
        ))
    }
}

/// `value − origin` as an `i32` (a layer offset in frame-local coordinates,
/// or back with a negated origin).
pub(crate) fn shifted(value: i32, origin: i64) -> Result<i32, String> {
    i32::try_from(i64::from(value) - origin)
        .map_err(|_| "native paint frame exceeds the supported range".to_string())
}

impl NativePaintEdit {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| format!("invalid native paint tool: {e}"))
    }
    pub(crate) fn target(&self) -> (usize, f32, u32, bool) {
        match *self {
            Self::EraseNative {
                layer,
                opacity,
                selection,
            } => (layer, opacity, 0, selection),
            Self::PaintBlend {
                layer,
                opacity,
                blend_mode,
                selection,
                ..
            }
            | Self::BucketFill {
                layer,
                opacity,
                blend_mode,
                selection,
                ..
            }
            | Self::Gradient {
                layer,
                opacity,
                blend_mode,
                selection,
                ..
            } => (layer, opacity, blend_mode, selection),
            Self::DodgeBurn {
                layer, selection, ..
            } => (layer, 1., 0, selection),
        }
    }
    pub(crate) fn colors(&self) -> (Vec<[f32; 4]>, PaintColorSpace) {
        match *self {
            Self::PaintBlend {
                color, color_space, ..
            }
            | Self::BucketFill {
                color, color_space, ..
            } => (vec![color], color_space),
            // Every stop colour of a stops gradient, so the range check and
            // the `color_space` conversion cover them all.
            Self::Gradient {
                start,
                end,
                color_space,
                ref stops,
                ..
            } => {
                if stops.is_empty() {
                    (vec![start, end], color_space)
                } else {
                    (stops.iter().map(|s| s.color).collect(), color_space)
                }
            }
            Self::DodgeBurn { .. } | Self::EraseNative { .. } => {
                (vec![], PaintColorSpace::Document)
            }
        }
    }
    pub(crate) fn preserves_alpha(&self) -> bool {
        matches!(
            self,
            Self::PaintBlend {
                preserve_alpha: true,
                ..
            }
        )
    }
    /// `PaintBlend`'s colour-channel target, if it names one.
    pub(crate) fn plane(&self) -> Option<PaintPlane> {
        match *self {
            Self::PaintBlend { plane, .. } => plane,
            _ => None,
        }
    }
    /// Whether changed ink may grow the layer: never for the two tools that
    /// only rework existing pixels, under Preserve Transparency, on a
    /// transparency-locked layer (whose alpha, and so its extent, is frozen),
    /// or with a colour-plane target (one channel of the pixels it has).
    pub(crate) fn expands(&self, doc: &PrecisionDocument) -> bool {
        !matches!(self, Self::DodgeBurn { .. } | Self::EraseNative { .. })
            && !self.preserves_alpha()
            && self.plane().is_none()
            && doc.lock_owner(self.target().0, LOCK_TRANSPARENCY).is_none()
    }
    /// The layer's storage extent after the walk; refuses a target that is
    /// not a raster in the document's own storage (u16, or f32 at float).
    pub(crate) fn extent(
        &self,
        doc: &PrecisionDocument,
        frame: PaintFrame,
    ) -> Result<StorageExtent, String> {
        let layer = doc
            .layer(self.target().0)
            .ok_or("native paint target is out of range")?;
        let image = layer
            .source()
            .ok_or("native paint requires a raster target")?;
        let (storage, sentence) = if doc.is_float() {
            (
                crate::precision_image::PrecisionStorage::Rgba32Float,
                "native paint in a 32-bit float document requires f32 storage",
            )
        } else {
            (
                crate::precision_image::PrecisionStorage::Rgba16Unorm,
                "native paint requires u16 storage",
            )
        };
        if image.storage() != storage {
            return Err(sentence.into());
        }
        if layer.retained_source.is_some() {
            return Err("Rasterize the retained source before painting its pixels".into());
        }
        if self.expands(doc) {
            merged_extent(
                (image.dimensions(), layer.offset()),
                ((frame.width, frame.height), frame.origin()?),
            )
        } else {
            Ok((image.dimensions(), layer.offset()))
        }
    }
    pub(crate) fn plan(&self, doc: &PrecisionDocument) -> Result<Value, String> {
        if doc.is_cmyk() {
            return doc.cmyk_dodge_burn_plan(self);
        }
        self.plan_in(doc, PaintFrame::canvas(doc))
    }
    /// [`Self::plan`] with the input laid out over `frame`.
    pub(crate) fn plan_in(
        &self,
        doc: &PrecisionDocument,
        frame: PaintFrame,
    ) -> Result<Value, String> {
        crate::float_paint::admit_ink(self, doc)?;
        let (layer, opacity, mode, selection) = self.target();
        if frame != PaintFrame::canvas(doc)
            && matches!(self, Self::BucketFill { .. } | Self::Gradient { .. })
        {
            return Err("native bucket fill and gradient paint over the canvas only".into());
        }
        if !opacity.is_finite() || !(0. ..=1.).contains(&opacity) || mode > 26 {
            return Err("native paint opacity or blend mode is invalid".into());
        }
        let (colors, space) = self.colors();
        // Linear ink's own range is `admit_ink`'s (above).
        if !matches!(space, PaintColorSpace::Linear)
            && colors
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            return Err("native authored colors must be finite in [0,1]".into());
        }
        match *self {
            Self::Gradient {
                from,
                to,
                ref stops,
                style,
                smoothness,
                ..
            } => {
                crate::gradient_field::validate(from, to)?;
                crate::gradient_field::validate_smoothness(smoothness)?;
                if stops.is_empty() {
                    if style.is_some() {
                        return Err("native gradient style requires stops".into());
                    }
                    if smoothness < 1.0 {
                        return Err("native gradient smoothness requires stops".into());
                    }
                } else {
                    if !crate::style_json::GRADIENT_STOPS.contains(&stops.len()) {
                        return Err("native gradient stops must have 2 to 32 entries".into());
                    }
                    crate::gradient_field::validate_stops(stops)?;
                }
            }
            Self::BucketFill { x, y, .. }
                if canvas_index(i64::from(x), i64::from(y), doc.width, doc.height).is_none() =>
            {
                return Err("native bucket seed is outside canvas".into())
            }
            Self::DodgeBurn {
                exposure, range, ..
            } if !exposure.is_finite() || !(0. ..=1.).contains(&exposure) || range > 2 => {
                return Err("native dodge/burn exposure or range is invalid".into())
            }
            _ => {}
        }
        if doc.is_float() {
            return crate::float_paint::plan_in(self, doc, frame);
        }
        let extent = self.extent(doc, frame)?;
        let n = pixel_count(frame.width, frame.height)? as u64;
        let output = pixel_count(extent.0 .0, extent.0 .1)? as u64;
        let mut scratch = 8 * BOOKKEEPING_BYTES + n * 24 + output * 8;
        if self.preserves_alpha() {
            // The alpha restore's two rect reads and its output rect.
            scratch += output * 24;
        }
        if matches!(space, PaintColorSpace::Srgb) && !colors.is_empty() {
            scratch += crate::precision_paint::srgb_ink_scratch(doc.profile())?;
        }
        let (mut rasters, masks) = doc.native_dimensions();
        let raster = (0..layer)
            .filter(|&i| doc.layer_source(i).is_some())
            .count();
        rasters[raster] = extent.0;
        doc.native_fits_geometry(doc.dimensions(), &rasters, &masks, doc.channels.len())?;
        Ok(
            json!({"retained_bytes":doc.retained_bytes(),"scratch_bytes":scratch,"input_samples":n*if selection{2}else{1},"input_width":frame.width,"input_height":frame.height,"input_planes":if selection{2}else{1}}),
        )
    }
    /// The LOCK gate (`precision_lock`): every native paint tool writes its
    /// target's pixels, so the whole body runs under a `Pixels` edit.
    pub(crate) fn apply(
        &self,
        doc: &PrecisionDocument,
        input: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        if doc.is_cmyk() {
            return doc.cmyk_dodge_burn(self, input, limit, other, job);
        }
        self.apply_in(doc, input, PaintFrame::canvas(doc), limit, other, job)
    }

    /// [`Self::apply`] with `input` laid out over `frame`.
    pub(crate) fn apply_in(
        &self,
        doc: &PrecisionDocument,
        input: &[u16],
        frame: PaintFrame,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        if doc.is_float() {
            return crate::float_paint::apply_in(self, doc, input, frame, limit, other, job);
        }
        let (index, ..) = self.target();
        doc.under_locks(index, crate::doc_lock::EditKind::Pixels, |doc| {
            self.applying_unlocked(doc, input, frame, limit, other, job)
        })
    }

    fn applying_unlocked(
        &self,
        doc: &PrecisionDocument,
        input: &[u16],
        frame: PaintFrame,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        let plan = self.plan_in(doc, frame)?;
        let scratch = plan["scratch_bytes"].as_u64().unwrap();
        admit(doc.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        if input.len() != plan["input_samples"].as_u64().unwrap() as usize {
            return Err("native paint input length differs from plan".into());
        }
        let (index, opacity, mode, selected) = self.target();
        if opacity == 0. || matches!(self, Self::DodgeBurn { exposure, .. } if *exposure == 0.) {
            job.report(1., 5)?;
            return Ok(None);
        }
        let n = pixel_count(frame.width, frame.height)?;
        if let Self::BucketFill { x, y, .. } = *self {
            // Match doc_select's seed admission without rounding the product
            // of two positive native gates down to zero.
            let seed = y as usize * doc.width as usize + x as usize;
            if input[seed] == 0 || (selected && input[n + seed] == 0) {
                job.report(1., 5)?;
                return Ok(None);
            }
        }
        let (colors, color_space) = self.colors();
        let colors = crate::precision_paint::prepare_colors(&colors, color_space, doc)?;
        // A stops gradient's ramp in the document's ink: each stop's
        // position with its converted colour. Empty for every other request.
        let ramp: Vec<FieldStop> = match self {
            Self::Gradient { stops, .. } => stops
                .iter()
                .zip(&colors)
                .map(|(stop, &color)| FieldStop {
                    position: stop.position,
                    color,
                    midpoint: stop.midpoint,
                    opacity_midpoint: stop.opacity_midpoint,
                })
                .collect(),
            _ => Vec::new(),
        };
        let layer = doc.layer(index).unwrap();
        let image = layer.source().unwrap();
        let (w, h) = image.dimensions();
        // The walk is frame-local: the layer's offset moves by `-frame`, and
        // the result's offset moves back at the end. On the canvas frame both
        // shifts are 0.
        let (ox, oy) = (
            shifted(layer.offset().0, frame.x)?,
            shifted(layer.offset().1, frame.y)?,
        );
        let canvas = (frame.width, frame.height);
        let expands = self.expands(doc);
        let region = if let Self::BucketFill {
            x,
            y,
            tolerance,
            contiguous,
            ..
        } = *self
        {
            // The grow is a tap walk over the whole canvas: ONE transient
            // (13B-3c-2c-1's rule), dropped with this block.
            let before = image.transient_rgba16()?;
            let sample = |i: usize| {
                let x = i as u64 % u64::from(doc.width);
                let y = i as u64 / u64::from(doc.width);
                canvas_index(x as i64 - i64::from(ox), y as i64 - i64::from(oy), w, h)
                    .map_or([0; 4], |j| before[j])
            };
            // A Lab document's bucket follows the Lab wand (`lab_select`).
            Some(match doc.lab_grid() {
                Some(grid) => crate::lab_select::grow(
                    canvas,
                    sample,
                    (x as u32, y as u32),
                    tolerance,
                    contiguous,
                    1,
                    grid,
                    job.range(0., 0.3),
                )?,
                None => crate::selection_region::grow(
                    canvas,
                    sample,
                    (x as u32, y as u32),
                    tolerance,
                    contiguous,
                    1,
                    job.range(0., 0.3),
                )?,
            })
        } else {
            None
        };
        // 13B-3d-1: the walk covers only the canvas rect R the stroke's
        // coverage can change (clipped to the layer for the two tools that
        // never expand), read from the layer's rect and `[0; 4]` elsewhere.
        let walk = if expands {
            coverage_rect16(&input[..n], canvas, canvas, (0, 0))
        } else {
            coverage_rect16(&input[..n], canvas, (w, h), (ox, oy)).map(|rect| {
                let (x, y) = plane_rect_offset((ox, oy), rect);
                (x as u32, y as u32, rect.2, rect.3)
            })
        };
        let Some((rx, ry, rw, rh)) = walk else {
            job.report(1., 5)?;
            return Ok(None);
        };
        let area = rw as usize * rh as usize;
        let mut canvas_rect = reserved_vec(area)?;
        canvas_rect.resize(area, [0; 4]);
        let ends = |a: u32, b: u32| (i64::from(a), i64::from(a) + i64::from(b));
        let ((x0, x1), (y0, y1)) = (ends(rx, rw), ends(ry, rh));
        let on_layer = canvas_rect_on_plane(canvas, (w, h), (ox, oy), (x0, y0, x1, y1));
        if let Some(rect) = on_layer {
            let samples = image
                .read_rect_rgba16(rect.0, rect.1, rect.2, rect.3)
                .ok_or("could not allocate precision output")?;
            let (cx, cy) = plane_rect_offset((ox, oy), rect);
            let (dx, dy) = ((cx as u32 - rx) as usize, (cy as u32 - ry) as usize);
            for (row, line) in samples.chunks_exact(rect.2 as usize).enumerate() {
                let at = (dy + row) * rw as usize + dx;
                canvas_rect[at..at + line.len()].copy_from_slice(line);
            }
        }
        let mut bounds = [frame.width, frame.height, 0, 0];
        let mut changed = false;
        let kind = kind_in(
            BlendMode::from_c(mode as i32).unwrap(),
            false,
            doc.lab_grid(),
        );
        // Preserve Transparency or a transparency lock: the frozen-alpha
        // colour rule; the restore below stays as the backstop.
        let frozen = self.preserves_alpha() || doc.lock_owner(index, LOCK_TRANSPARENCY).is_some();
        let lab = doc.is_lab();
        for (k, slot) in canvas_rect.iter_mut().enumerate() {
            job.every(k)?;
            if k & 65535 == 0 {
                job.report(0.3 + 0.5 * k as f64 / area as f64, 3)?;
            }
            let x = rx + (k % rw as usize) as u32;
            let y = ry + (k / rw as usize) as u32;
            let i = y as usize * frame.width as usize + x as usize;
            let original = *slot;
            let mut result = original;
            let coverage = f64::from(input[i]) / 65535.
                * if selected {
                    f64::from(input[n + i]) / 65535.
                } else {
                    1.
                };
            if coverage > 0. && region.as_ref().is_none_or(|r| r[i] > 0) {
                if let Self::EraseNative { .. } = self {
                    // Same destination-out rule as blend::erase_alpha; binary64
                    // keeps two independent native gates until one storage round.
                    result[3] = (f64::from(original[3]) * (1. - coverage * f64::from(opacity)))
                        .round() as u16;
                } else if let Self::DodgeBurn {
                    exposure,
                    range,
                    burn,
                    protect_tones,
                    ..
                } = *self
                {
                    if original[3] > 0 {
                        let mut targets: [f32; 3] =
                            std::array::from_fn(|c| f32::from(original[c]) / 65535.);
                        // A Lab Color document (#1 phase 2e): Dodge and Burn
                        // shape L* alone, the 2b rule that tone ops act on
                        // Lightness; Protect Tones is the one-element [L]
                        // group (the plain curve), and a*, b* keep their codes.
                        let shaped_channels = if lab { 1 } else { 3 };
                        if protect_tones {
                            crate::doc_retouch::protected(
                                &mut targets[..shaped_channels],
                                exposure,
                                range,
                                burn,
                            );
                        } else {
                            targets[..shaped_channels].iter_mut().for_each(|value| {
                                *value = crate::doc_retouch::shaped(*value, exposure, range, burn)
                            });
                        }
                        for (c, &shaped) in targets.iter().enumerate().take(shaped_channels) {
                            result[c] = ((f64::from(original[c]) * (1. - coverage))
                                + f64::from(shaped) * 65535. * coverage)
                                .clamp(0., 65535.)
                                .round() as u16;
                        }
                    }
                } else {
                    let color = if let Self::Gradient {
                        from,
                        to,
                        radial,
                        style,
                        smoothness,
                        ..
                    } = *self
                    {
                        let point = [
                            (i64::from(x) + frame.x) as f64 + 0.5,
                            (i64::from(y) + frame.y) as f64 + 0.5,
                        ];
                        if ramp.is_empty() {
                            let t = crate::gradient_field::parameter(point, from, to, radial);
                            let mut color: [f32; 4] = std::array::from_fn(|c| {
                                (f64::from(colors[0][c]) * (1. - t) + f64::from(colors[1][c]) * t)
                                    as f32
                            });
                            let drag = if radial {
                                FieldStyle::Radial
                            } else {
                                FieldStyle::Linear
                            };
                            if drag.classic() {
                                // The drag's colour is the Classic ramp where
                                // its style is identified (`FieldStyle::classic`);
                                // alpha stays linear.
                                let rgb = crate::gradient_field::classic_two(
                                    [colors[0][0], colors[0][1], colors[0][2]],
                                    [colors[1][0], colors[1][1], colors[1][2]],
                                    t as f32,
                                );
                                color[..3].copy_from_slice(&rgb);
                            }
                            color
                        } else {
                            let style = style.unwrap_or(if radial {
                                FieldStyle::Radial
                            } else {
                                FieldStyle::Linear
                            });
                            let t = crate::gradient_field::styled_parameter(point, from, to, style);
                            crate::gradient_field::stop_color(&ramp, t as f32, style, smoothness)
                        }
                    } else {
                        colors[0]
                    };
                    let alpha = (f64::from(color[3]) * f64::from(opacity) * coverage) as f32;
                    if alpha > 0. {
                        let mut pixel = original.map(|v| f32::from(v) / 65535.);
                        let rgb = [color[0], color[1], color[2]];
                        let xy = (i64::from(x) + frame.x, i64::from(y) + frame.y);
                        if frozen {
                            composite_source_frozen(&mut pixel, rgb, alpha, kind, xy);
                        } else {
                            let pixel = std::slice::from_mut(&mut pixel);
                            composite_source_into(pixel, 0, rgb, alpha, kind, xy);
                        }
                        result = store(pixel);
                        // Sub-quantum alpha cannot create invisible storage or
                        // latent-color-only phantom edits outside the raw layer.
                        if original == [0; 4] && result[3] == 0 {
                            result = original;
                        }
                    }
                }
            }
            if result != original {
                changed = true;
                bounds[0] = bounds[0].min(x);
                bounds[1] = bounds[1].min(y);
                bounds[2] = bounds[2].max(x + 1);
                bounds[3] = bounds[3].max(y + 1);
            }
            *slot = result;
        }
        if !changed {
            job.report(1., 5)?;
            return Ok(None);
        }
        let extent = if expands {
            merged_extent(
                ((w, h), (ox, oy)),
                (
                    (bounds[2] - bounds[0], bounds[3] - bounds[1]),
                    (bounds[0] as i32, bounds[1] as i32),
                ),
            )?
        } else {
            ((w, h), (ox, oy))
        };
        let ((nw, nh), (nx, ny)) = extent;
        let image = match on_layer {
            Some(rect) if extent == ((w, h), (ox, oy)) => {
                // R ∩ layer back through `with_rect` (untouched tiles
                // shared, no view); the changed latch is its bitwise diff.
                let samples = if (rect.2, rect.3) == (rw, rh) {
                    canvas_rect
                } else {
                    let (cx, cy) = plane_rect_offset((ox, oy), rect);
                    let (dx, dy) = ((cx as u32 - rx) as usize, (cy as u32 - ry) as usize);
                    let mut samples = reserved_vec(rect.2 as usize * rect.3 as usize)?;
                    for row in 0..rect.3 as usize {
                        let at = (dy + row) * rw as usize + dx;
                        samples.extend_from_slice(&canvas_rect[at..at + rect.2 as usize]);
                    }
                    samples
                };
                let Some(image) =
                    image.with_rect_rgba16(rect.0, rect.1, rect.2, rect.3, &samples)?
                else {
                    job.report(1., 5)?;
                    return Ok(None);
                };
                image
            }
            _ => {
                // The grown extent E (changed ink left the layer): zeros, the
                // old layer's rows, then R's values over R ∩ E only — R's
                // samples outside E are unchanged off-layer zeros.
                let mut pixels = reserved_vec(pixel_count(nw, nh)?)?;
                let (left, top) = (i64::from(nx), i64::from(ny));
                let (r0, r1) = (x0.max(left), x1.min(left + i64::from(nw)));
                for y in 0..nh {
                    job.check()?;
                    let start = pixels.len();
                    pixels.resize(start + nw as usize, [0; 4]);
                    let row = &mut pixels[start..];
                    let cy = i64::from(y) + top;
                    let ly = cy - i64::from(oy);
                    if (0..i64::from(h)).contains(&ly) {
                        let at = (i64::from(ox) - left) as usize;
                        for (x, run) in image.row_segments(ly as u32, 0, w) {
                            let PrecisionRun::Rgba16(run) = run else {
                                return Err("native paint requires u16 storage".into());
                            };
                            let at = at + x as usize;
                            row[at..at + run.len()].copy_from_slice(run);
                        }
                    }
                    if (y0..y1).contains(&cy) && r0 < r1 {
                        let from = (cy - y0) as usize * rw as usize + (r0 - x0) as usize;
                        let at = (r0 - left) as usize;
                        let len = (r1 - r0) as usize;
                        row[at..at + len].copy_from_slice(&canvas_rect[from..from + len]);
                    }
                }
                image.rebuilt_rgba16(nw, nh, pixels, doc.profile().clone())?
            }
        };
        job.check()?;
        let (nx, ny) = (shifted(nx, -frame.x)?, shifted(ny, -frame.y)?);
        let mut next = doc.replacing_content(
            index,
            Content::Raster {
                image,
                offset: (nx, ny),
            },
        )?;
        if let Some(next) = next.as_mut() {
            if let Some(comps) = doc.comps_after_growth(index, (nx, ny)) {
                next.comps = comps;
            }
        }
        if self.preserves_alpha() {
            next = match next {
                Some(next) => doc.restoring_alpha(index, next)?,
                None => None,
            };
        }
        job.report(1., 5)?;
        Ok(next)
    }
}
