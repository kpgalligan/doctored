//! Typed paint, erase, integer source clone and scalar text/shape assembly.
//! Coverage is u16; authored colors are continuous straight encoded RGB in an
//! explicit document or sRGB context. The shared compositor is the only paint
//! kernel. Display pixels and premultiplied RGBA8 overlays are never inputs.
//!
//! The 7E public mode exposes only its explicitly guarded operation/target subset. Since 13B-3d-1
//! paint / erase / clone walk only `coverage_rect16`'s rect on the layer (one `read_rect_rgba16`
//! buffer written in place, `with_rect_rgba16` back), heal solves over the window's rect on the
//! layer (`precision_heal::apply` takes that buffer's plane geometry; a window missing the layer
//! solves over an empty plane, as before) and the clone/heal `SourceView` reads one stored code per
//! tap (`rgba16_at`), so none binds a view.

use serde::Deserialize;

use crate::authored_color::lut_ink;
use crate::blend::{blend_kind, composite_source_frozen, composite_source_into, BlendMode};
use crate::icc::IccProfile;
use crate::icc_transform::PrecisionTransform;
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_rect::{canvas_rect_on_plane, coverage_rect16, plane_rect_offset};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::precision_render::MAX_RECORDS;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaintColorSpace {
    Document,
    Srgb,
    /// Linear light in the document's primaries, used as is: a float
    /// document's `paint_blend` and `gradient` ink only (`float_paint::admit_ink`).
    Linear,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum PaintEdit {
    Paint {
        layer: usize,
        color: [f32; 4],
        color_space: PaintColorSpace,
        opacity: f32,
    },
    Erase {
        layer: usize,
        opacity: f32,
    },
    Clone {
        layer: usize,
        source_layer: usize,
        dx: i32,
        dy: i32,
        opacity: f32,
    },
    Heal {
        layer: usize,
        source_layer: usize,
        dx: i32,
        dy: i32,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        opacity: f32,
    },
    Rasterize {
        name: String,
        width: u32,
        height: u32,
        x: i32,
        y: i32,
        colors: Vec<[f32; 4]>,
        color_space: PaintColorSpace,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct PaintPlan {
    pub retained_bytes: u64,
    pub scratch_bytes: u64,
    pub input_samples: usize,
}

fn normalized(values: &[f32]) -> Result<(), String> {
    if values
        .iter()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        Err("high-depth paint color and opacity must be finite in [0,1]".into())
    } else {
        Ok(())
    }
}

impl PaintEdit {
    pub fn parse(description: &str) -> Result<Self, String> {
        if description.len() > 64 * 1024 {
            return Err("high-depth paint request exceeds 64 KiB".into());
        }
        serde_json::from_str(description)
            .map_err(|e| format!("invalid high-depth paint request: {e}"))
    }
    fn target(&self) -> Option<(usize, f32)> {
        match self {
            Self::Paint { layer, opacity, .. }
            | Self::Erase { layer, opacity }
            | Self::Clone { layer, opacity, .. }
            | Self::Heal { layer, opacity, .. } => Some((*layer, *opacity)),
            Self::Rasterize { .. } => None,
        }
    }
    fn colors(&self) -> (&[[f32; 4]], PaintColorSpace) {
        match self {
            Self::Paint {
                color, color_space, ..
            } => (std::slice::from_ref(color), *color_space),
            Self::Rasterize {
                colors,
                color_space,
                ..
            } => (colors, *color_space),
            _ => (&[], PaintColorSpace::Document),
        }
    }
    fn prepared_colors(&self, doc: &PrecisionDocument) -> Result<Vec<[f32; 4]>, String> {
        let (colors, space) = self.colors();
        prepare_colors(colors, space, doc)
    }
}

/// Shared continuous authored-ink conversion; no pixel rasterization or rounding.
///
/// `Srgb` ink converts into the document once: through the precision
/// transform on a matrix/TRC profile, and on an RGB LUT profile through
/// `authored_color::lut_ink` (relative, BPC off) when it converts both ways,
/// else the numbers as authored — never a refusal.
pub(crate) fn prepare_colors(
    colors: &[[f32; 4]],
    space: PaintColorSpace,
    doc: &PrecisionDocument,
) -> Result<Vec<[f32; 4]>, String> {
    // Held once per call, so the larger LUT variant is not boxed.
    #[allow(clippy::large_enum_variant)]
    enum SrgbInk {
        Matrix(PrecisionTransform),
        Lut(crate::icc_intent::ColorPipeline),
    }
    let conversion = if !colors.is_empty() && matches!(space, PaintColorSpace::Srgb) {
        let src = IccProfile::srgb();
        match doc.profile().model() {
            Some(model) => Some(SrgbInk::Matrix(
                PrecisionTransform::new(src.model().ok_or("missing sRGB model")?, model)
                    .map_err(|e| crate::icc_encode_refusal::own_profile_error(doc.profile(), e))?,
            )),
            None => lut_ink(doc.profile(), true).map(SrgbInk::Lut),
        }
    } else {
        None
    };
    // A Gray document greys every ink by Mode ▸ Grayscale's rule
    // (`gray_document`, "Luminance"): the document-space colour's luminance
    // code, so its storage stays R = G = B.
    let gray = doc.gray_policy();
    // A Lab document's ink is the twin's colour quantized as Mode ▸ Lab
    // Color quantizes (`lab_convert`): the ×257 grid at 8 bits.
    let lab = doc.lab_policy();
    Ok(colors
        .iter()
        .map(|&color| {
            let rgb = [color[0], color[1], color[2]];
            let rgb = match &conversion {
                Some(SrgbInk::Matrix(t)) => t.color(rgb),
                Some(SrgbInk::Lut(pipeline)) => pipeline.color(rgb),
                None => rgb,
            };
            if let Some(policy) = gray {
                let rgb = rgb.map(f64::from);
                let c = (f64::from(policy.luminance_code(doc.profile(), rgb)) / 65535.0) as f32;
                return [c, c, c, color[3]];
            }
            if let Some(policy) = lab {
                let [l, a, b] =
                    rgb.map(|v| (f64::from(policy.code(f64::from(v))) / 65535.0) as f32);
                return [l, a, b, color[3]];
            }
            [rgb[0], rgb[1], rgb[2], color[3]]
        })
        .collect())
}

/// The scratch `Srgb` authored ink takes into `profile` ([`prepare_colors`]):
/// the precision transform's tables on a matrix/TRC profile, the LUT
/// pipeline's on a LUT profile that converts both ways, nothing when the
/// numbers stay.
pub(crate) fn srgb_ink_scratch(profile: &IccProfile) -> Result<u64, String> {
    let srgb = IccProfile::srgb();
    let source = srgb.model().ok_or("missing sRGB model")?;
    Ok(match profile.model() {
        Some(model) => PrecisionTransform::prepare_scratch_bytes(source, model),
        None => lut_ink(profile, true).map_or(0, |pipeline| pipeline.scratch_bytes()),
    })
}

impl PrecisionDocument {
    pub fn paint_plan(&self, edit: &PaintEdit) -> Result<PaintPlan, String> {
        if matches!(edit.colors().1, PaintColorSpace::Linear) {
            return Err(crate::float_paint::LINEAR_ONLY.into());
        }
        let canvas = pixel_count(self.width, self.height)?;
        let mut input_samples = canvas;
        let mut scratch = 4 * BOOKKEEPING_BYTES;
        let output_pixels = if let Some((index, opacity)) = edit.target() {
            normalized(&[opacity])?;
            let image = self
                .layer_source(index)
                .ok_or("high-depth paint requires a raster source target")?;
            if image.storage() != PrecisionStorage::Rgba16Unorm {
                return Err("high-depth paint requires u16 storage".into());
            }
            let (w, h) = image.dimensions();
            pixel_count(w, h)?
        } else if let PaintEdit::Rasterize {
            name,
            width,
            height,
            colors,
            ..
        } = edit
        {
            crate::precision_document::scalar_and_name(name, 1.0)?;
            if self.records >= MAX_RECORDS || colors.is_empty() || colors.len() > 2 {
                return Err("high-depth rasterization requires one or two coverage/color planes and an available layer record".into());
            }
            let pixels = pixel_count(*width, *height)?;
            input_samples = pixels
                .checked_mul(colors.len())
                .ok_or("coverage size overflow")?;
            // The model performs its complete native capacity preflight again
            // on publication; the metadata-only addition check runs before pixels.
            self.preflight_raster_addition((*width, *height), name)?;
            pixels
        } else {
            return Err("invalid high-depth paint target".into());
        };
        let (colors, space) = edit.colors();
        for color in colors {
            normalized(color)?;
        }
        if !colors.is_empty() && matches!(space, PaintColorSpace::Srgb) {
            scratch += srgb_ink_scratch(&self.profile)?;
        }
        if let PaintEdit::Clone { source_layer, .. } | PaintEdit::Heal { source_layer, .. } = edit {
            self.layer_source(*source_layer)
                .ok_or("high-depth clone/heal requires an authoritative raster source")?;
        }
        if let PaintEdit::Heal { width, height, .. } = edit {
            let n = pixel_count(*width, *height)? as u64;
            let (heal_scratch, _) = crate::precision_heal::plan(n)?;
            scratch += heal_scratch;
        }
        scratch = scratch
            .checked_add(output_pixels as u64 * 8)
            .and_then(|n| n.checked_add(input_samples as u64 * 2))
            .ok_or("high-depth paint allocation overflow")?;
        Ok(PaintPlan {
            retained_bytes: self.retained_bytes(),
            scratch_bytes: scratch,
            input_samples,
        })
    }

    pub fn paint(
        &self,
        edit: &PaintEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.paint_with_context(edit, coverage, limit, other, job)
    }
    /// The LOCK gate (`precision_lock`): every typed writer runs under its
    /// target's locks, and paint is a `Pixels` edit. `rasterize` adds a NEW
    /// layer and names no target, so it is ungated — the byte
    /// `adding_image_layer` is not gated either.
    pub(crate) fn paint_with_context(
        &self,
        edit: &PaintEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let Some((index, _)) = edit.target() else {
            return self.painting_unlocked(edit, coverage, limit, other, job);
        };
        self.under_locks(index, crate::doc_lock::EditKind::Pixels, |doc| {
            doc.painting_unlocked(edit, coverage, limit, other, job)
        })
    }

    fn painting_unlocked(
        &self,
        edit: &PaintEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        admit(self.retained_bytes(), 4 * BOOKKEEPING_BYTES, other, limit)?;
        job.reserve(4 * BOOKKEEPING_BYTES)?;
        let plan = self.paint_plan(edit)?;
        if coverage.len() != plan.input_samples {
            return Err("high-depth coverage length does not match its declared extent".into());
        }
        admit(plan.retained_bytes, plan.scratch_bytes, other, limit)?;
        job.reserve(plan.scratch_bytes)?;
        let colors = edit.prepared_colors(self)?;
        if let PaintEdit::Rasterize {
            name,
            width,
            height,
            x,
            y,
            ..
        } = edit
        {
            let n = pixel_count(*width, *height)?;
            let mut samples = reserved_vec(n)?;
            for i in 0..n {
                job.every(i)?;
                let mut pixel = [0.0; 4];
                for (plane, color) in colors.iter().enumerate() {
                    let alpha = crate::precision_render::checked_alpha(
                        color[3],
                        1.0,
                        f32::from(coverage[plane * n + i]) / 65535.0,
                    )?;
                    if alpha > 0.0 {
                        over(&mut pixel, *color, alpha);
                    }
                }
                samples.push(store(pixel));
            }
            let image = PrecisionImage::from_rgba16_unseeded(
                *width,
                *height,
                samples,
                self.profile.clone(),
                None,
            )?;
            let mut layers = self.layers.as_ref().clone();
            layers.push(PrecisionDocumentLayer::raster(
                name,
                image,
                (*x, *y),
                1.0,
                true,
            )?);
            let next = self.rebuilt(
                self.width,
                self.height,
                layers,
                self.channels.as_ref().clone(),
            )?;
            job.report(1.0, 5)?;
            return Ok(Some(next));
        }
        let (index, opacity) = edit.target().ok_or("missing high-depth paint target")?;
        if opacity == 0.0 {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        let layer = self
            .layer(index)
            .ok_or("high-depth layer index is out of range")?;
        let image = layer
            .source()
            .ok_or("high-depth paint requires a raster source")?;
        if image.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("high-depth paint requires u16 storage".into());
        }
        // 13B-3d-1: the walk reads and writes only the rect the edit can
        // change (`layer_rect`'s plane forms) — ONE `read_rect` buffer written
        // in place and handed to `with_rect`, whose bitwise diff is the
        // changed latch — so a dab on a large layer neither copies the plane
        // nor seeds a view.
        let (w, h) = image.dimensions();
        let offset = layer.offset();
        let canvas = (self.width, self.height);
        if let PaintEdit::Heal {
            x,
            y,
            width,
            height,
            ..
        } = edit
        {
            let (x, y) = (i64::from(*x), i64::from(*y));
            let window = (x, y, x + i64::from(*width), y + i64::from(*height));
            let Some(rect) = canvas_rect_on_plane(canvas, (w, h), offset, window) else {
                // A window missing the layer: every `layer_byte` is `None`
                // over an empty plane, so the solve runs as before.
                crate::precision_heal::apply(self, edit, coverage, &mut [], ((0, 0), offset), job)?;
                job.report(1.0, 5)?;
                return Ok(None);
            };
            let mut pixels = read_rect(image, rect)?;
            let plane = ((rect.2, rect.3), plane_rect_offset(offset, rect));
            crate::precision_heal::apply(self, edit, coverage, &mut pixels, plane, job)?;
            return self.rect_written(index, (image, offset), rect, &pixels, job);
        }
        let clone_source = if let PaintEdit::Clone { source_layer, .. } = edit {
            Some(SourceView::new(self, *source_layer)?)
        } else {
            None
        };
        let Some(rect) = coverage_rect16(coverage, canvas, (w, h), offset) else {
            job.report(1.0, 5)?;
            return Ok(None);
        };
        let (rx, ry, rw, rh) = rect;
        let mut pixels = read_rect(image, rect)?;
        // A transparency lock: Paint and Clone write the frozen-alpha rule;
        // `under_locks`' restore stays as the backstop.
        let frozen = self
            .lock_owner(index, crate::doc_lock::LOCK_TRANSPARENCY)
            .is_some();
        for row in 0..rh {
            job.check()?;
            for col in 0..rw {
                let k = row as usize * rw as usize + col as usize;
                job.every(k)?;
                let before = pixels[k];
                let cx = i64::from(rx + col) + i64::from(offset.0);
                let cy = i64::from(ry + row) + i64::from(offset.1);
                let Some(ci) = canvas_index(cx, cy, self.width, self.height) else {
                    continue;
                };
                let amount = f32::from(coverage[ci]) / 65535.0 * opacity;
                if coverage[ci] == 0 {
                    continue;
                }
                match edit {
                    PaintEdit::Erase { .. } => {
                        // Erasure changes coverage alone, retaining latent RGB.
                        pixels[k][3] =
                            (crate::blend::erase_alpha(f32::from(before[3]) / 65535.0, amount)
                                * 65535.0)
                                .round() as u16;
                    }
                    PaintEdit::Paint { .. } => {
                        let color = colors[0];
                        if color[3] > 0.0 {
                            let mut pixel = before.map(|n| f32::from(n) / 65535.0);
                            over_preserving(
                                &mut pixel,
                                color,
                                crate::precision_render::checked_alpha(
                                    color[3],
                                    opacity,
                                    f32::from(coverage[ci]) / 65535.0,
                                )?,
                                frozen,
                            );
                            pixels[k] = store(pixel);
                        }
                    }
                    PaintEdit::Clone { dx, dy, .. } => {
                        let source = clone_source
                            .as_ref()
                            .and_then(|view| view.at(cx + i64::from(*dx), cy + i64::from(*dy)));
                        if let Some(source) = source.filter(|s| s[3] != 0) {
                            // Full copies preserve all four exact source codes.
                            if amount == 1.0 && source[3] == u16::MAX {
                                pixels[k] = source;
                            } else {
                                let color = source.map(|n| f32::from(n) / 65535.0);
                                let mut pixel = before.map(|n| f32::from(n) / 65535.0);
                                over_preserving(
                                    &mut pixel,
                                    color,
                                    crate::precision_render::checked_alpha(
                                        color[3],
                                        opacity,
                                        f32::from(coverage[ci]) / 65535.0,
                                    )?,
                                    frozen,
                                );
                                pixels[k] = store(pixel);
                            }
                        }
                    }
                    _ => unreachable!("validated paint operation"),
                }
            }
            job.report(f64::from(row + 1) / f64::from(rh), 3)?;
        }
        self.rect_written(index, (image, offset), rect, &pixels, job)
    }

    /// Writes the walked rect back (`with_rect_rgba16`: untouched tiles
    /// shared, no view seeded); `Ok(None)` when no sample changed bitwise.
    fn rect_written(
        &self,
        index: usize,
        (image, offset): (&PrecisionImage, (i32, i32)),
        (x, y, w, h): (u32, u32, u32, u32),
        pixels: &[[u16; 4]],
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let Some(image) = image.with_rect_rgba16(x, y, w, h, pixels)? else {
            job.report(1.0, 5)?;
            return Ok(None);
        };
        let result = self.replacing_content(index, Content::Raster { image, offset })?;
        job.report(1.0, 5)?;
        Ok(result)
    }
}

/// The rect's u16 samples (`read_rect_rgba16`, no view); Err when the copy
/// cannot be allocated.
fn read_rect(
    image: &PrecisionImage,
    (x, y, w, h): (u32, u32, u32, u32),
) -> Result<Vec<[u16; 4]>, String> {
    image
        .read_rect_rgba16(x, y, w, h)
        .ok_or_else(|| "could not allocate precision output".to_string())
}

pub(crate) fn over(pixel: &mut [f32; 4], color: [f32; 4], alpha: f32) {
    composite_source_into(
        std::slice::from_mut(pixel),
        0,
        [color[0], color[1], color[2]],
        alpha,
        blend_kind(BlendMode::Normal),
        (0, 0),
    );
}

/// [`over`] onto a pixel whose alpha `frozen` (a transparency lock) keeps:
/// the frozen-alpha rule through `blend::composite_source_frozen` — an
/// alpha-0 pixel stays, a semi-transparent one takes `lerp(Cb, Cs, alpha)` at
/// its own alpha. Unfrozen it IS [`over`].
pub(crate) fn over_preserving(pixel: &mut [f32; 4], color: [f32; 4], alpha: f32, frozen: bool) {
    blend_preserving(
        pixel,
        color,
        alpha,
        blend_kind(BlendMode::Normal),
        (0, 0),
        frozen,
    );
}

/// `color` composited onto `pixel` through `kind` at effective alpha `alpha`:
/// the shared W3C step (`blend::composite_source_into`), or on a `frozen`
/// alpha the frozen-alpha rule through `blend::composite_source_frozen`.
/// `xy` is the pixel's canvas position, which only Dissolve's dither reads.
/// With the Normal kind it IS [`over_preserving`].
pub(crate) fn blend_preserving(
    pixel: &mut [f32; 4],
    color: [f32; 4],
    alpha: f32,
    kind: crate::blend::BlendKind,
    xy: (i64, i64),
    frozen: bool,
) {
    let rgb = [color[0], color[1], color[2]];
    if frozen {
        composite_source_frozen(pixel, rgb, alpha, kind, xy);
    } else {
        composite_source_into(std::slice::from_mut(pixel), 0, rgb, alpha, kind, xy);
    }
}

pub(crate) fn store(pixel: [f32; 4]) -> [u16; 4] {
    pixel.map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16)
}

pub(crate) fn canvas_index(x: i64, y: i64, w: u32, h: u32) -> Option<usize> {
    (x >= 0 && y >= 0 && x < i64::from(w) && y < i64::from(h)).then_some(
        (y.max(0) as usize)
            .saturating_mul(w as usize)
            .saturating_add(x.max(0) as usize),
    )
}

/// One captured leaf lookup per operation; pixel loops never traverse
/// layers. Reads one STORED u16 code per tap (`rgba16_at`), so no view is
/// bound (13B-3d-1).
pub(crate) struct SourceView<'a> {
    image: &'a PrecisionImage,
    offset: (i32, i32),
}
impl<'a> SourceView<'a> {
    pub(crate) fn new(doc: &'a PrecisionDocument, index: usize) -> Result<Self, String> {
        let layer = doc
            .layer(index)
            .ok_or("high-depth source index is out of range")?;
        let image = layer
            .source()
            .ok_or("high-depth source requires a raster leaf")?;
        if image.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("high-depth source requires u16 storage".into());
        }
        Ok(Self {
            image,
            offset: layer.offset(),
        })
    }
    pub(crate) fn at(&self, x: i64, y: i64) -> Option<[u16; 4]> {
        let (width, height) = self.image.dimensions();
        let (x, y) = (x - i64::from(self.offset.0), y - i64::from(self.offset.1));
        canvas_index(x, y, width, height)?;
        self.image.rgba16_at(x as u32, y as u32)
    }
}
