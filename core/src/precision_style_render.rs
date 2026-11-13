//! Typed layer styles at composite time, under the shape-quantized
//! byte-plane policy (`designs/phase-10d-layer-styles-status.md`):
//!
//! - The SHAPE is the one depth reduction. Plane pixel `(x, y)` holds
//!   `((a16 as f32) * cov / 257.0).round().clamp(0, 255) as u8`, `a16` the
//!   layer's 16-bit alpha code (a group's f32 alpha × 65535) and `cov` the
//!   prepared mask's coverage (1 unmasked); `bounds` is the bbox of
//!   `a16 > 0`, the mask left out — `style_render::Shape::of_layer`'s rule
//!   one depth up. For a promoted document (every code `257·c8`, masks
//!   `257·m8` at density 1) the bytes are bit-identical to the byte shape
//!   (`257a·(257m/65535) = a·m/255`, and 255 is odd so no product ties).
//! - The SHIPPED u8 pipeline (`style_render::render_effects_with_job`, the nine
//!   `style_fx_*`, `plane_blur`, `grow_mask`/`shrink_mask`) renders every
//!   contribution over that shape; nothing here is new pixel math. Planes
//!   are rendered per render — there is no typed `RenderCache`.
//! - Contributions composite in f32 as `cov / 255` with the authored colour
//!   `c / 255` through the byte `authored_color::AuthoredColors` for a
//!   non-sRGB profile (`DrDocument::style_colors`'s rule), via the byte
//!   `style_composite::composite_contribution`. The layer's own pixels go
//!   through the typed twin of `composite_pixels_into` with
//!   `sa = checked_alpha(alpha, opacity, cov) × fill × weight`.
//! - The ORDER is `style_composite`'s: Blend If snapshot of `layer rect ∩
//!   below_landing` (only when Below contributions exist) → Below onto the
//!   accumulator at `c.opacity × opacity` → pixels with the layer's blend →
//!   Interior. Opacity < 1 with planes and a non-Dissolve blend is the
//!   package model on ONE canvas-sized copy of the accumulator (the direct
//!   path at opacity 1, `mix_premultiplied` back over the affected window
//!   only) — no strips. A styled clip BASE draws its pixels into a private
//!   buffer at Normal / 1 with `fill × weight`, its footprint is the SHAPE
//!   (`clip_pass_shape`), and the members land with the base's alpha frozen
//!   (`clip_unit`'s Open / Settle / Close) — the loop lives HERE so the
//!   per-member step is the byte one.
//!
//! Scratch is planned by [`style_scratch`] from what the u8 renderer
//! actually allocates, because the typed plan is a hard gate.
//!
//! The renderer builds `typed_shape` (`round(a16·mask/257)` — the ONE depth reduction;
//! bit-identical to `Shape::of_layer` for promoted codes), renders through the SHIPPED
//! `style_render::render_effects_with_job` with no cache, and composites in the byte
//! `style_composite` order through the byte `composite_contribution` / `clip_pass_shape` /
//! `BackdropSnapshot` / `mix_premultiplied` (now `pub(crate)`): Below, the typed pixel twin at
//! `checked_alpha × fill × weight`, Interior. Effects read the document's global light
//! (`precision_global_light`) through the scene's `StyleEnv` and the style extents' `light`
//! parameter.
//!
//! A 32-bit FLOAT scene runs this same path with three statements swapped through
//! the env, each measured (`style_float`): every contribution's colour and blend come from
//! `StyleEnv::style_colors` (the authored colour in LINEAR light, the effect's float kind), the
//! pixels' and buffers' kinds from `StyleEnv::kind` (`blend_float::blend_kind_at`;
//! `lab_blend::kind_in` on a Lab scene), and the Blend
//! If weight from `style_blend_if::source_weight_float`. The package mix keeps extended-range
//! colour unclamped (`style_composite::mix_premultiplied_in`). Nothing else differs: the planes,
//! the order and the arithmetic are this file's.

use std::convert::Infallible;
use std::sync::Arc;

use crate::authored_color::AuthoredColors;
use crate::blend::{composite_buffer_into, composite_source_into, BlendKind, BlendMode};
use crate::blend_fill::FillBlend;
use crate::clip_unit::{clip_pass, ClipStep};
use crate::doc::MAX_PIXELS;
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::lab_blend::{fill_in, kind_in, LabGrid};
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_evaluation::TypedTransfer;
use crate::precision_image::{pixel_count, reserved_vec, PrecisionImage, PrecisionStorage};
use crate::precision_mask::PreparedMask;
use crate::precision_render::checked_alpha;
use crate::precision_style_adjustment::{interior_layer, Adjusted16};
use crate::style::{BlendIf, Effect, GlobalLight, LayerStyle};
use crate::style_blend_if::{source_weight_16, source_weight_float};
use crate::style_composite::{
    affected_window, below_landing, clip_pass_shape, composite_contribution, holds_inside_band,
    mix_premultiplied_in, occluding_fill, replace_under_band, style_grown, BackdropSnapshot,
    BandBackdrop, CompositeEnv, Cut, BAND_BACKDROP_BYTES,
};
use crate::style_float::FloatStyle;
use crate::style_render::{
    render_effects_with_job, sigma_for_size, Contribution, RenderContext, RenderedEffects, Shape,
    StyleColors,
};

/// The pixels a typed style decorates: a raster's own image at its canvas
/// offset, or an isolated group's rendered f32 buffer — one plane of the
/// accumulator's frame, whose pixel (0, 0) is canvas pixel `origin` — or a
/// styled ADJUSTMENT's shape (`precision_style_adjustment`): opaque
/// over the frame, so its mask over the canvas is the shape.
pub(crate) enum StyledPixels<'a> {
    Image {
        image: &'a PrecisionImage,
        offset: (i32, i32),
    },
    Buffer {
        pixels: &'a [[f32; 4]],
        width: u32,
        height: u32,
        origin: (i32, i32),
    },
    Opaque {
        width: u32,
        height: u32,
        origin: (i32, i32),
    },
}

impl StyledPixels<'_> {
    pub(crate) fn dimensions(&self) -> (u32, u32) {
        match self {
            StyledPixels::Image { image, .. } => image.dimensions(),
            StyledPixels::Buffer { width, height, .. }
            | StyledPixels::Opaque { width, height, .. } => (*width, *height),
        }
    }
    pub(crate) fn offset(&self) -> (i32, i32) {
        match self {
            StyledPixels::Image { offset, .. } => *offset,
            StyledPixels::Buffer { origin, .. } | StyledPixels::Opaque { origin, .. } => *origin,
        }
    }
    /// The pixel at plane `(x, y)`: a raster through
    /// `PrecisionImage::sample_at` (no view), a buffer by index; `None`
    /// outside `width`×`height`.
    pub(crate) fn sample_at(&self, x: u32, y: u32) -> Option<[f32; 4]> {
        match self {
            StyledPixels::Image { image, .. } => image.sample_at(x, y),
            StyledPixels::Buffer {
                pixels,
                width,
                height,
                ..
            } => (x < *width && y < *height)
                .then(|| pixels[y as usize * *width as usize + x as usize]),
            StyledPixels::Opaque { width, height, .. } => {
                (x < *width && y < *height).then_some(OPAQUE)
            }
        }
    }
    /// Whether these pixels are a raster's STORED samples (encoded under
    /// `linear_light`), as against a composited buffer or the adjustment
    /// shape.
    pub(crate) fn stored(&self) -> bool {
        matches!(self, StyledPixels::Image { .. })
    }
    /// The alpha of `pixel` (read from `self`) as a 16-bit CODE: the stored
    /// u16 for a raster (exact), the f32 alpha × 65535 for a rendered group.
    /// `round(v/65535 · 65535)` in f32 returns every u16 code `v` exactly
    /// (checked for all 65536), so the raster answer is still the stored
    /// code.
    pub(crate) fn alpha16_code(&self, pixel: [f32; 4]) -> f32 {
        match self {
            StyledPixels::Image { .. } => (pixel[3] * 65535.0).round(),
            StyledPixels::Buffer { .. } | StyledPixels::Opaque { .. } => pixel[3] * 65535.0,
        }
    }
    /// Visits plane row `y`'s columns `x0..min(x1, width)` in order with
    /// their pixels — a raster through `PrecisionImage::row_segments` (no
    /// view), a buffer by index. Visits nothing when `y >= height` or `x0 >=
    /// x1`; the first error stops the walk and is returned.
    pub(crate) fn visit_row<E>(
        &self,
        y: u32,
        x0: u32,
        x1: u32,
        mut visit: impl FnMut(u32, [f32; 4]) -> Result<(), E>,
    ) -> Result<(), E> {
        match self {
            StyledPixels::Image { image, .. } => {
                for (x, run) in image.row_segments(y, x0, x1) {
                    for j in 0..run.len() {
                        visit(x + j as u32, run.pixel(j))?;
                    }
                }
            }
            StyledPixels::Buffer {
                pixels,
                width,
                height,
                ..
            } => {
                let x1 = x1.min(*width);
                if y < *height && x0 < x1 {
                    let row = y as usize * *width as usize;
                    for x in x0..x1 {
                        visit(x, pixels[row + x as usize])?;
                    }
                }
            }
            StyledPixels::Opaque { width, height, .. } => {
                if y < *height {
                    for x in x0..x1.min(*width) {
                        visit(x, OPAQUE)?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// [`StyledPixels::Opaque`]'s one pixel.
const OPAQUE: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// One styled typed layer as the compositor sees it.
pub(crate) struct StyledLayer<'a> {
    pub pixels: StyledPixels<'a>,
    pub mask: Option<&'a PreparedMask<'a>>,
    pub style: &'a LayerStyle,
    pub blend_if: Option<&'a BlendIf>,
    pub blend: BlendMode,
    pub opacity: f32,
    /// Whether some clipped member of this unit carries `knockout != none`, so
    /// the unit's base-only snapshot is worth cloning (D4).
    pub members_want_base: bool,
    /// A styled ADJUSTMENT's interior (`precision_style_adjustment`):
    /// `pixels` is then its [`StyledPixels::Opaque`] shape and step 3 applies
    /// the adjustment in place. `None` for every other layer.
    pub adjusted: Option<&'a Adjusted16<'a>>,
}

/// What every typed styled composite reads beyond the layer: the document's
/// global light, the document canvas and the authored-colour conversion.
#[derive(Clone, Copy)]
pub(crate) struct StyleEnv<'a> {
    pub light: GlobalLight,
    pub canvas: (u32, u32),
    pub colors: Option<&'a AuthoredColors>,
    /// The scene's EVALUATION POLICY as tables (`precision_evaluation`):
    /// `Some` under `linear_light`, where the accumulator holds linear
    /// components — a stored pixel and an effect's ink are decoded as they
    /// enter it ([`StyleEnv::style_colors`], [`PixelPass16::transfer`]) and
    /// Blend If reads encoded values ([`StyleEnv::blend_if_weight`]).
    pub transfer: Option<&'a TypedTransfer>,
    /// True for a 32-bit FLOAT scene (`float_document`): [`StyleEnv::kind`]
    /// and [`StyleEnv::blend_if_weight`] read it, and the package mix keeps
    /// extended-range colour unclamped.
    pub float: bool,
    /// A float scene's style statements (`style_float`), built once per
    /// render beside `colors_for`; `None` on every u16 scene, so
    /// [`StyleEnv::style_colors`] answers `Encoded16(colors)` there — the
    /// byte statements, with a gradient's Dither at half a 16-bit code.
    pub float_style: Option<&'a FloatStyle>,
    /// What a KNOCKOUT punches through to (`style_knockout`, D4) — the BYTE
    /// struct, because every typed plane is indexed accumulator-locally over
    /// the render's one frame and so already aligned with it (a merge's
    /// captured planes are windowed onto its frame before the render,
    /// `precision_merge_extent::windowed_seed`). `NONE` — both transparent — is every
    /// isolated group buffer, every bake and every scene with no knockout.
    pub knockout: crate::style_knockout::KnockoutBackdrop<'a>,
    /// A merge's `B0` CAPTURE (`precision_style_knockout`), or `None` for
    /// every ordinary render. When it is `Some`, every recursive arm returns
    /// as soon as it is filled and never touches its half-rendered buffer.
    pub capture: Option<&'a crate::precision_style_knockout::TypedCapture>,
    /// A Lab Color document's grid: every kind, special Fill and
    /// effect ink of the scene is `lab_blend`'s; `None` on every other scene.
    pub lab: Option<LabGrid>,
}

impl<'a> StyleEnv<'a> {
    /// The conversion every authored style colour takes into `profile`'s
    /// space (`DrDocument::style_colors`), or `None` for sRGB and for a LUT
    /// profile that cannot convert both ways. Built ONCE per render.
    pub(crate) fn colors_for(profile: &IccProfile) -> Option<AuthoredColors> {
        AuthoredColors::into_document(profile)
    }

    /// A float scene's [`FloatStyle`] for `profile`, `None` for a u16 scene —
    /// built ONCE per render beside [`Self::colors_for`].
    pub(crate) fn float_style_for(
        profile: &IccProfile,
        float: bool,
    ) -> Result<Option<FloatStyle>, String> {
        float.then(|| FloatStyle::for_profile(profile)).transpose()
    }

    /// How a contribution's authored colour and blend become its ink here:
    /// `Linear16` (the `Encoded16` ink, decoded per sample) under
    /// `linear_light`.
    pub(crate) fn style_colors(&self) -> StyleColors<'a> {
        match (self.float_style, self.transfer) {
            (Some(float), _) => StyleColors::Float(float),
            (None, Some(transfer)) => StyleColors::Linear16(self.colors, transfer),
            (None, None) => StyleColors::Encoded16(self.colors, self.lab),
        }
    }

    /// The blend behaviour of a layer's (or a buffer's) mode at this scene's
    /// depth: `blend::blend_kind` on a u16 scene, the Lab arm on a Lab one
    /// (`lab_blend::kind_in`).
    pub(crate) fn kind(&self, mode: BlendMode) -> BlendKind {
        kind_in(mode, self.float, self.lab)
    }

    /// The Lab grid of a Lab Color scene, `None` for every other.
    pub(crate) fn lab(&self) -> Option<LabGrid> {
        self.lab
    }

    /// The Blend If weight at this scene's depth ([`blend_if_weight_in`]):
    /// `source` is the layer's sample — `stored` when it is a raster's own
    /// stored pixel, false for a composited one (a group's buffer, a styled
    /// adjustment's adjusted backdrop) — and `backdrop` the accumulator's.
    pub(crate) fn blend_if_weight(
        &self,
        blend_if: &BlendIf,
        source: [f32; 3],
        stored: bool,
        backdrop: [f32; 3],
    ) -> f32 {
        blend_if_weight_in(
            blend_if,
            source,
            stored,
            backdrop,
            self.float,
            self.transfer,
        )
    }
}

/// The typed Blend If weight: `source_weight_float` on a float scene (the
/// STORED linear values, measured); `source_weight_16` on a u16 one. Under
/// `linear_light` both sides are read ENCODED, as the ranges are authored —
/// a stored `source` is its code already, a composited one and the
/// accumulator's `backdrop` go through `precision_evaluation::encoded_view16`
/// — so a range selects the pixels it selects in the encoded document over
/// the same picture. The ONE typed weight: [`StyleEnv::blend_if_weight`] and
/// every per-sample body that carries the depth as fields (`PixelPass16`)
/// call it.
pub(crate) fn blend_if_weight_in(
    blend_if: &BlendIf,
    source: [f32; 3],
    stored: bool,
    backdrop: [f32; 3],
    float: bool,
    transfer: Option<&TypedTransfer>,
) -> f32 {
    if float {
        return source_weight_float(blend_if, source, backdrop);
    }
    match transfer {
        None => source_weight_16(blend_if, source, backdrop),
        Some(transfer) => {
            let source = if stored {
                source
            } else {
                transfer.encode(source)
            };
            source_weight_16(blend_if, source, transfer.encode(backdrop))
        }
    }
}

/// `(w + 2·pad) · (h + 2·pad)`, checked — the ONE padded-plane arithmetic
/// the edit, the native reader and the geometry plan all gate on.
pub(crate) fn plane_pixels(dims: (u32, u32), pad: u32) -> Result<u64, String> {
    let pad = u64::from(pad) * 2;
    u64::from(dims.0)
        .checked_add(pad)
        .and_then(|w| u64::from(dims.1).checked_add(pad).map(|h| (w, h)))
        .and_then(|(w, h)| w.checked_mul(h))
        .ok_or_else(|| "typed layer style plane arithmetic overflow".into())
}

fn padded(dims: (u32, u32), pad: u32) -> Result<(u32, u32), String> {
    let w = dims
        .0
        .checked_add(pad.checked_mul(2).ok_or("pad overflow")?);
    let h = dims
        .1
        .checked_add(pad.checked_mul(2).ok_or("pad overflow")?);
    match (w, h) {
        (Some(w), Some(h)) => Ok((w, h)),
        _ => Err("typed layer style plane exceeds the pixel limit".into()),
    }
}

/// The quantized typed shape (module doc) on a plane padded by `pad`;
/// `None` when that plane would exceed `MAX_PIXELS`.
pub(crate) fn typed_shape(
    pixels: &StyledPixels<'_>,
    mask: Option<&PreparedMask<'_>>,
    pad: u32,
) -> Option<Shape> {
    let (lw, lh) = pixels.dimensions();
    let (ox, oy) = pixels.offset();
    if plane_pixels((lw, lh), pad).ok()? > MAX_PIXELS {
        return None;
    }
    let (w, h) = padded((lw, lh), pad).ok()?;
    let mut coverage = vec![0u8; w as usize * h as usize];
    let (mut bx0, mut by0, mut bx1, mut by1) = (i64::from(lw), i64::from(lh), 0i64, 0i64);
    for y in 0..lh {
        let row = (y + pad) as usize * w as usize + pad as usize;
        let mut row_min = lw;
        let mut row_max = 0;
        pixels
            .visit_row(y, 0, lw, |x, px| {
                let a16 = pixels.alpha16_code(px);
                let cov = mask.map_or(1.0, |m| {
                    m.at(i64::from(ox) + i64::from(x), i64::from(oy) + i64::from(y))
                });
                coverage[row + x as usize] = typed_coverage_byte(a16, cov);
                if a16 > 0.0 {
                    row_min = row_min.min(x);
                    row_max = x;
                }
                Ok::<(), Infallible>(())
            })
            .unwrap_or_else(|never| match never {});
        if row_min < lw {
            bx0 = bx0.min(i64::from(row_min));
            bx1 = bx1.max(i64::from(row_max) + 1);
            by0 = by0.min(i64::from(y));
            by1 = i64::from(y) + 1;
        }
    }
    let bounds = (bx1 > bx0).then_some((bx0, by0, bx1, by1));
    Some(Shape {
        lw,
        lh,
        w,
        h,
        origin: (-i64::from(pad), -i64::from(pad)),
        coverage,
        bounds,
    })
}

/// `typed_shape`'s pixel (module doc): `round(a16 · coverage / 257)` clamped
/// to a byte — the ONE depth reduction. The regional compositor
/// (`precision_regional_tree`) reads a styled clip base's pad-0 shape sample
/// by sample through this same statement.
#[inline]
pub(crate) fn typed_coverage_byte(a16: f32, coverage: f32) -> u8 {
    (a16 * coverage / 257.0).round().clamp(0.0, 255.0) as u8
}

fn saturating_i32(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// The per-member renderer of a styled clip base: `(count, render)`; the
/// loop and the per-member clamp live in [`composite_styled_into`]. The third
/// callback argument is the unit's BASE-ONLY snapshot (D4), which the member
/// reads as BOTH knockout planes; `None` when no member asked for one.
pub(crate) type Members<'m> = Option<(
    usize,
    &'m mut dyn FnMut(usize, &mut [[f32; 4]], Option<&[[f32; 4]]>) -> Result<(), String>,
)>;

/// The styled-layer path (module doc) onto `acc`, an accumulator of `dims`
/// whose pixel (0, 0) is canvas pixel `origin`. The planes report inside
/// `job`'s range; everything after them only polls it (13A-5b-iii-c-2).
pub(crate) fn composite_styled_into(
    acc: &mut [[f32; 4]],
    dims: (u32, u32),
    origin: (i32, i32),
    layer: &StyledLayer<'_>,
    env: &StyleEnv<'_>,
    members: Members<'_>,
    job: JobContext<'_>,
) -> Result<(), String> {
    job.check()?;
    let style = layer.style;
    let planes = if style.has_enabled_effects() {
        let shape = typed_shape(&layer.pixels, layer.mask, style.pad())
            .ok_or("typed layer style plane exceeds the pixel limit")?;
        job.check()?;
        let ctx = RenderContext {
            light: env.light,
            canvas: env.canvas,
            layer_offset: layer.pixels.offset(),
            lab_tiles: env.colors.is_some_and(AuthoredColors::converts_tiles),
        };
        Some(render_effects_with_job(style, shape, &ctx, job)?)
    } else {
        None
    };
    job.check()?;
    match planes.as_ref() {
        Some(planes) if layer.opacity < 1.0 && layer.blend != BlendMode::Dissolve => {
            composite_package_into(acc, dims, origin, layer, env, planes, members, job)
        }
        planes => composite_direct_into(
            acc,
            dims,
            origin,
            layer,
            env,
            planes,
            layer.opacity,
            members,
            job,
        ),
    }
}

/// The four steps straight onto `acc`, every contribution and the pixels
/// scaled by `opacity` (`style_composite::composite_direct_into`).
#[allow(clippy::too_many_arguments)]
fn composite_direct_into(
    acc: &mut [[f32; 4]],
    dims: (u32, u32),
    origin: (i32, i32),
    layer: &StyledLayer<'_>,
    env: &StyleEnv<'_>,
    planes: Option<&RenderedEffects>,
    opacity: f32,
    members: Members<'_>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let (acc_w, acc_h) = dims;
    let offset = layer.pixels.offset();
    let rel = (
        saturating_i32(i64::from(offset.0) - i64::from(origin.0)),
        saturating_i32(i64::from(offset.1) - i64::from(origin.1)),
    );
    // Knockout / "Blend Interior Effects as Group" route step 3 through the
    // typed unit pass; everything else is untouched (the byte
    // `style_composite::composite_direct_into` rule).
    let routed = crate::style_knockout::routes_unit(layer.style, planes);
    // A unit pass ALWAYS needs the full-rect Blend If snapshot, because its
    // pixel pass writes into a private buffer and would otherwise read
    // "Underlying" from that buffer's transparency (the byte rule).
    let snapshot_rect = match (members.is_none() && !routed, planes) {
        (true, None) => None,
        (true, Some(p)) => below_landing(rel, &p.shape, &p.below),
        (false, _) => Some((i64::MIN, i64::MIN, i64::MAX, i64::MAX)),
    };
    let backdrop = layer
        .blend_if
        .map(|b| {
            Ok::<_, String>((
                b,
                snapshot_rect
                    .map(|r| {
                        BackdropSnapshot::of(
                            acc,
                            acc_w,
                            acc_h,
                            rel,
                            layer.pixels.dimensions(),
                            r,
                            job,
                        )
                    })
                    .transpose()?,
            ))
        })
        .transpose()?;
    let blend_if = backdrop.as_ref().map(|(b, s)| (*b, s.as_ref()));
    let occluding = occluding_fill(
        layer.style,
        layer.blend_if,
        layer.blend,
        env.float,
        layer.adjusted.is_some(),
    );

    if let Some(planes) = planes {
        for c in &planes.below {
            job.check()?;
            composite_contribution(
                acc,
                acc_w,
                acc_h,
                origin,
                rel,
                &planes.shape,
                c,
                opacity,
                occluding,
                env.style_colors(),
                job,
            )?;
        }
    }

    // B0 for the inside bands (`style_composite::composite_direct_into`'s
    // step 4): after the Below group, before the layer's own interior, once
    // over the union of every band.
    let band = planes
        .map(|p| (p.inside_bands(), p))
        .filter(|(bands, _)| !bands.is_empty())
        .map(|(bands, p)| {
            let of: Vec<&Contribution> = bands.iter().map(|&j| &p.interior[j]).collect();
            BandBackdrop::of(
                acc,
                acc_w,
                acc_h,
                rel,
                &p.shape,
                &of,
                env.knockout,
                layer.style.knockout,
                job,
            )
            .map(|b0| (bands, b0))
        })
        .transpose()?;

    // A styled ADJUSTMENT's interior is read HERE, after its own Below
    // group (POST), and applied in place (`precision_style_adjustment`).
    if routed {
        let frame = layer
            .adjusted
            .map(|a| a.interior_frame(acc, dims, origin, env, job))
            .transpose()?;
        let interior = frame
            .as_deref()
            .map(|frame| interior_layer(layer, frame, dims, origin));
        crate::precision_style_knockout::composite_interior_unit_into(
            acc,
            dims,
            origin,
            interior.as_ref().unwrap_or(layer),
            env,
            planes,
            blend_if,
            opacity,
            members,
            job,
        )?;
    } else {
        match members {
            None => match layer.adjusted {
                Some(a) => {
                    a.interior_into(acc, dims, origin, layer, blend_if, opacity, None, env, job)?
                }
                None => composite_pixels_into(
                    acc,
                    dims,
                    origin,
                    layer,
                    layer.style.fill_opacity,
                    blend_if,
                    layer.blend,
                    opacity,
                    env,
                    job,
                )?,
            },
            Some((count, render_member)) => {
                let mut buf: Vec<[f32; 4]> = reserved_vec(acc.len())?;
                buf.resize(acc.len(), [0.0; 4]);
                // An adjustment base puts no pixels in the unit: its interior
                // meets the members' buffer below.
                if layer.adjusted.is_none() {
                    composite_pixels_into(
                        &mut buf,
                        dims,
                        origin,
                        layer,
                        layer.style.fill_opacity,
                        blend_if,
                        BlendMode::Normal,
                        1.0,
                        env,
                        job,
                    )?;
                }
                // The footprint is the SHAPE: the effects' plane when there is
                // one, else a pad-0 shape built for the purpose (the byte rule);
                // only a plane past MAX_PIXELS falls back to the pixel pass's
                // own alpha, which the admission gates make unreachable.
                let local;
                let footprint: Option<&Shape> = match planes {
                    Some(p) => Some(&p.shape),
                    None => {
                        local = typed_shape(&layer.pixels, layer.mask, 0);
                        local.as_ref()
                    }
                };
                let base_alpha: Vec<f32> = if footprint.is_none() {
                    buf.iter().map(|px| px[3]).collect()
                } else {
                    Vec::new()
                };
                let clip = |buf: &mut [[f32; 4]], step| match footprint {
                    Some(shape) => clip_pass_shape(buf, acc_w, acc_h, rel, shape, step, job),
                    None => {
                        clip_pass(buf, &base_alpha, step);
                        Ok(())
                    }
                };
                // `clip_unit`'s Open (#2), then D4: the members punch to this
                // buffer as it stands after the base's pixel pass (opened) and
                // before any member, never to the enclosing backdrop. The clone
                // happens only when a member asks for it.
                clip(&mut buf, ClipStep::Open)?;
                let base_only = layer.members_want_base.then(|| buf.clone());
                for i in 0..count {
                    render_member(i, &mut buf, base_only.as_deref())?;
                    job.check()?;
                    clip(&mut buf, ClipStep::Settle)?;
                }
                clip(&mut buf, ClipStep::Close)?;
                job.check()?;
                match layer.adjusted {
                    Some(a) => a.interior_into(
                        acc,
                        dims,
                        origin,
                        layer,
                        blend_if,
                        opacity,
                        Some(&buf),
                        env,
                        job,
                    )?,
                    None => composite_buffer_into(
                        acc,
                        &buf,
                        acc_w,
                        acc_h,
                        origin,
                        env.kind(layer.blend),
                        opacity,
                        job,
                    )?,
                }
            }
        }
    }

    if let Some(planes) = planes {
        // Under "Blend Interior Effects as Group" the five GROUPED
        // contributions were already composited INSIDE the unit; only inner
        // shadow, inside/centre stroke and bevel remain out here (D6).
        let grouped = crate::style_knockout::grouped_interior(layer.style, Some(planes));
        for (j, c) in planes
            .interior
            .iter()
            .enumerate()
            .filter(|(_, c)| !(grouped && crate::style_knockout::is_grouped_interior(c.kind)))
        {
            job.check()?;
            match band.as_ref().filter(|(bands, _)| bands.contains(&j)) {
                Some((_, b0)) => replace_under_band(
                    acc,
                    acc_w,
                    acc_h,
                    origin,
                    rel,
                    &planes.shape,
                    c,
                    b0,
                    opacity,
                    env.style_colors(),
                    job,
                )?,
                None => composite_contribution(
                    acc,
                    acc_w,
                    acc_h,
                    origin,
                    rel,
                    &planes.shape,
                    c,
                    opacity,
                    occluding,
                    env.style_colors(),
                    job,
                )?,
            }
        }
    }
    Ok(())
}

/// The package model on ONE copy of the accumulator: the direct path at
/// opacity 1, mixed back at `opacity` over the affected window only.
#[allow(clippy::too_many_arguments)]
fn composite_package_into(
    acc: &mut [[f32; 4]],
    dims: (u32, u32),
    origin: (i32, i32),
    layer: &StyledLayer<'_>,
    env: &StyleEnv<'_>,
    planes: &RenderedEffects,
    members: Members<'_>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let (acc_w, acc_h) = dims;
    let offset = layer.pixels.offset();
    let rel = (
        saturating_i32(i64::from(offset.0) - i64::from(origin.0)),
        saturating_i32(i64::from(offset.1) - i64::from(origin.1)),
    );
    let (x0, y0, x1, y1) = affected_window(rel, &planes.shape, &planes.below);
    let (x0, y0) = (x0.max(0), y0.max(0));
    let (x1, y1) = (x1.min(i64::from(acc_w)), y1.min(i64::from(acc_h)));
    if x1 <= x0 || y1 <= y0 {
        return Ok(());
    }
    let mut copy: Vec<[f32; 4]> = reserved_vec(acc.len())?;
    copy.extend_from_slice(acc);
    composite_direct_into(
        &mut copy,
        dims,
        origin,
        layer,
        env,
        Some(planes),
        1.0,
        members,
        job,
    )?;
    for y in y0..y1 {
        job.check()?;
        let row = y as usize * acc_w as usize;
        for x in x0..x1 {
            let i = row + x as usize;
            acc[i] = mix_premultiplied_in(acc[i], copy[i], layer.opacity, !env.float);
        }
    }
    Ok(())
}

/// The layer's pixels into `target` with `sa = checked_alpha(alpha,
/// opacity, coverage) × fill × blend-if weight` and blend `mode` — the
/// typed twin of `style_composite::composite_pixels_into` (the master
/// copy): the same iteration and the same direct write of a fresh
/// transparent pixel under Normal, with the mask read through the prepared
/// typed mask at canvas coordinates and the weight through
/// `source_weight_16` (`source_weight_float` when `float`, whose `mode`
/// kind is `blend_float::blend_kind_at`'s; `lab_blend::kind_in` and
/// `fill_in` route a Lab scene's, `lab`, to its arms).
/// `fill_opacity` is a PARAMETER rather than `layer.style.fill_opacity`,
/// because the unit pass of `precision_style_knockout` runs the pixels at fill
/// 1 when the grouped interiors move inside the unit (D6) and pays the fill
/// once for the whole unit. The scene's `float`, `lab` and `transfer` come
/// from its [`StyleEnv`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn composite_pixels_into(
    target: &mut [[f32; 4]],
    dims: (u32, u32),
    origin: (i32, i32),
    layer: &StyledLayer<'_>,
    fill_opacity: f32,
    blend_if: Option<(&BlendIf, Option<&BackdropSnapshot>)>,
    mode: BlendMode,
    opacity: f32,
    env: &StyleEnv<'_>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let (float, lab) = (env.float, env.lab());
    let fill = crate::opacity_byte::composite(fill_opacity, float);
    if fill <= 0.0 || opacity <= 0.0 {
        return Ok(());
    }
    let (acc_w, acc_h) = dims;
    let kind = kind_in(mode, float, lab);
    let direct = mode == BlendMode::Normal;
    let (lw, lh) = layer.pixels.dimensions();
    let offset = layer.pixels.offset();
    let rel_x = i64::from(offset.0) - i64::from(origin.0);
    let rel_y = i64::from(offset.1) - i64::from(origin.1);
    let x0 = rel_x.max(0);
    let y0 = rel_y.max(0);
    let x1 = (rel_x + i64::from(lw)).min(i64::from(acc_w));
    let y1 = (rel_y + i64::from(lh)).min(i64::from(acc_h));
    let pass = PixelPass16 {
        joint: crate::opacity_byte::joint(fill, opacity, float),
        blend_if: blend_if.map(|(b, _)| b),
        kind,
        direct,
        opacity,
        float,
        fill_blend: fill_in(mode, fill, float, lab),
        transfer: env.transfer,
        stored: layer.pixels.stored(),
    };
    // The layer-space span, once per layer; `(0, 0)` unless the target span
    // is non-empty, so no negative reaches a `u32`.
    let (lx0, lx1) = if x0 < x1 {
        ((x0 - rel_x) as u32, (x1 - rel_x) as u32)
    } else {
        (0, 0)
    };
    for ay in y0..y1 {
        job.check()?;
        let ly = (ay - rel_y) as u32;
        layer.pixels.visit_row(ly, lx0, lx1, |lx, px| {
            let ax = i64::from(lx) + rel_x;
            let canvas_xy = (ax + i64::from(origin.0), ay + i64::from(origin.1));
            let coverage = layer.mask.map_or(1.0, |m| m.at(canvas_xy.0, canvas_xy.1));
            let ai = (ay as u64 * u64::from(acc_w) + ax as u64) as usize;
            let under = match blend_if {
                None => target[ai],
                Some((_, snapshot)) => snapshot.and_then(|s| s.at(ax, ay)).unwrap_or(target[ai]),
            };
            composite_pixels_sample16(target, ai, px, coverage, under, &pass, canvas_xy)
        })?;
    }
    Ok(())
}

/// The loop-invariant inputs of one typed `composite_pixels_into` pass (the
/// byte `style_composite::PixelPass`'s twin).
pub(crate) struct PixelPass16<'a> {
    /// The Fill and `opacity` as the one weight they scale the pixels by
    /// (`opacity_byte::joint`, #2).
    pub joint: f32,
    /// The Blend If ranges; `None` = weight 1.
    pub blend_if: Option<&'a BlendIf>,
    pub kind: BlendKind,
    /// `mode == BlendMode::Normal`: a transparent target pixel is written
    /// directly.
    pub direct: bool,
    /// Already > 0.
    pub opacity: f32,
    /// A 32-bit float scene: the Blend If weight is `source_weight_float`.
    pub float: bool,
    /// The special Fill of five modes (`blend_fill`, #2; never in a float
    /// scene), `None` for the plain one.
    pub fill_blend: Option<FillBlend>,
    /// The scene's evaluation policy (`StyleEnv::transfer`).
    pub transfer: Option<&'a TypedTransfer>,
    /// Whether the pixels are a raster's STORED samples
    /// ([`StyledPixels::stored`]): under `linear_light` those are decoded as
    /// they composite, while a group buffer is linear already.
    pub stored: bool,
}

/// One sample of the typed `composite_pixels_into`'s loop, statement for
/// statement: the Blend If weight from `under` (the underlying composite),
/// `sa = checked_alpha(alpha, joint, coverage) · weight`, the direct
/// transparent-target write under Normal, else `composite_source_into`. The
/// regional compositor (`precision_regional_tree`) runs a styled sample
/// through this same body. Under `linear_light` the weight reads both sides
/// encoded ([`blend_if_weight_in`]) and a stored pixel is decoded before it
/// composites (`precision_render::composite_raster_sample16`'s rule).
#[inline]
pub(crate) fn composite_pixels_sample16(
    target: &mut [[f32; 4]],
    ai: usize,
    px: [f32; 4],
    coverage: f32,
    under: [f32; 4],
    pass: &PixelPass16<'_>,
    canvas_xy: (i64, i64),
) -> Result<(), String> {
    let sample = [px[0], px[1], px[2]];
    let weight = match pass.blend_if {
        None => 1.0,
        Some(b) => blend_if_weight_in(
            b,
            sample,
            pass.stored,
            [under[0], under[1], under[2]],
            pass.float,
            pass.transfer,
        ),
    };
    let cs = match pass.transfer {
        Some(transfer) if pass.stored => transfer.decode(sample),
        _ => sample,
    };
    if let Some(special) = pass.fill_blend {
        // The special Fill (`blend_fill`): `sa` without the Fill.
        let sa = checked_alpha(px[3], pass.opacity, coverage)? * weight;
        if sa > 0.0 {
            special.composite(&mut target[ai][..], &cs, sa);
        }
        return Ok(());
    }
    let sa = checked_alpha(px[3], pass.joint, coverage)? * weight;
    if sa <= 0.0 {
        return Ok(());
    }
    if pass.direct && target[ai][3] <= 0.0 {
        target[ai] = [cs[0], cs[1], cs[2], sa];
        return Ok(());
    }
    composite_source_into(target, ai, cs, sa, pass.kind, canvas_xy);
    Ok(())
}

/// The Gaussian sigma an effect's plane blur runs at (0 for one that does
/// not blur), for the scratch plan.
fn blur_sigma(effect: &Effect) -> f32 {
    use crate::style_matte::law_applies;
    match effect {
        // The law's box pair runs no Gaussian (`style_matte`); elsewhere the
        // member's sigma is 0 as well (Size·(1 − Spread) == 0).
        Effect::DropShadow(e) if law_applies(e.size, e.spread) => 0.0,
        Effect::DropShadow(e) => crate::style_fx_drop_shadow::MATTE.split(e.size, e.spread).1,
        Effect::OuterGlow(e) if law_applies(e.size, e.spread) => 0.0,
        Effect::OuterGlow(e) => crate::style_matte::OUTER_GLOW.split(e.size, e.spread).1,
        Effect::InnerShadow(e) if law_applies(e.size, e.choke) => 0.0,
        Effect::InnerShadow(e) => crate::style_fx_inner_shadow::MATTE.split(e.size, e.choke).1,
        // Neither regime runs the Gaussian: the law's box pair, or today's
        // kernel at Size·(1 − Choke) == 0, where sigma is 0
        // (`style_fx_inner_glow`).
        Effect::InnerGlow(_) => 0.0,
        // A Smooth bevel runs the measured law's box pairs, no Gaussian
        // (`style_bevel_smooth`); a Chisel today's, whose sigma bounds the
        // Chisel law's (`style_bevel_chisel`: Chisel Soft's blur at
        // `size · CHISEL_SOFT_RATIO`, and no Soften Gaussian).
        Effect::BevelEmboss(e) => {
            if crate::style_bevel_smooth::applies(e.style, e.technique) {
                0.0
            } else {
                sigma_for_size(e.size).max(sigma_for_size(e.soften))
            }
        }
        // The law's box pair above size 0, no Gaussian (`style_fx_satin`).
        Effect::Satin(e) => crate::style_fx_satin::satin_sigma(e.size),
        Effect::Stroke(_)
        | Effect::ColorOverlay(_)
        | Effect::GradientOverlay(_)
        | Effect::PatternOverlay(_) => 0.0,
    }
}

/// `(scratch bytes, work units)` one styled composite costs beyond the
/// accumulator, from what the u8 renderer allocates: with `P` the padded
/// plane's pixels and `enabled` the enabled-effect count, `P · (2 +
/// 2·enabled)` u8 planes (the shape, its `inverted()` and at most two
/// retained contribution planes per effect) plus `plane_blur::float_plan`
/// at the largest blur sigma (≥ 32·P, which dominates the EDT's two f32
/// planes, the bevel's f32 shade and a textured bevel's `4·P` f32 texture
/// plane — built after the blurs return, never alive during one,
/// `style_fx_bevel_emboss`, and the law's box pair (the drop shadow, inner
/// shadow, outer glow, inner glow and satin, `style_matte::law_matte_with_job`),
/// two f64 planes plus the ring, at most `24·P`, and the band it boxes on
/// an anti-aliased layer, its transient planes at most `12·P` before the
/// box pair starts plus its offset table, `style_aa_band`, and the Smooth
/// bevels' law, its soften box pair beside the f32 shading,
/// at most `28·P` with the ring, `style_bevel_smooth`, and the Chisel law,
/// the same box pair on its height after the EDT is dropped, at most `28·P`,
/// `style_bevel_chisel`) plus a strip-sized
/// f32 window, all
/// only when an effect is enabled; `16·w·h` for the Blend If snapshot of
/// the layer rect when `blend_if`; `20·w·h` for the inside band's B0 store
/// (`style_composite::BandBackdrop`) when the style holds an enabled Inside or
/// Center Stroke of size > 0; and `16·canvas` for the package copy
/// when `package` is `Some(canvas)` (opacity < 1 and blend ≠ Dissolve);
/// plus `16·level` for each enabled placed tile's PYRAMID level (a pattern
/// overlay's, a Pattern stroke's, built once per stroke, or an enabled bevel
/// texture's), the level's exact allocation (`style_pattern::level_bytes`);
/// plus `2·P` for
/// each enabled Shape Burst stroke, its one u16 field
/// (`style_stroke_fill`; Center's two halves share it); plus one high-depth
/// encoder (`authored_color::wide_scratch_bytes`) when the style holds a
/// wide colour (`wide_color_native::holds_wide`), which a 16-bit
/// composite builds on first use.
/// With NO enabled effect the renderer allocates no effect plane, but a
/// styled clip BASE still builds the pad-0 u8 shape its members are
/// clamped to, so `P` is charged for every fill-only style rather than
/// threading a `clip_base` flag through the plan: at most one byte per
/// pixel over-charged on a layer that is not a clip base.
/// Work is `P · (2 + 8·enabled)`.
pub(crate) fn style_scratch(
    style: &LayerStyle,
    dims: (u32, u32),
    blend_if: bool,
    package: Option<(u32, u32)>,
) -> Result<(u64, u64), String> {
    let pad = style.pad();
    let p = plane_pixels(dims, pad)?;
    if p > MAX_PIXELS {
        return Err("typed layer style plane exceeds the pixel limit".into());
    }
    let (wp, hp) = padded(dims, pad)?;
    let enabled = style.enabled_effects().count() as u64;
    let mut scratch = p;
    if enabled > 0 {
        let sigma = style.enabled_effects().map(blur_sigma).fold(0.0, f32::max);
        scratch = p
            .checked_mul(2 + 2 * enabled)
            .and_then(|v| v.checked_add(crate::plane_blur::float_plan(wp, hp, sigma).ok()?.0))
            .and_then(|v| v.checked_add(16 * u64::from(hp.min(256)) * u64::from(wp)))
            .and_then(|v| v.checked_add(crate::style_pattern::level_bytes(style)))
            .and_then(|v| v.checked_add(p.checked_mul(2 * shape_bursts(style))?))
            .ok_or("typed layer style scratch overflow")?;
    }
    if blend_if {
        scratch = scratch
            .checked_add(16 * u64::from(dims.0) * u64::from(dims.1))
            .ok_or("typed layer style scratch overflow")?;
    }
    if holds_inside_band(style) {
        scratch = scratch
            .checked_add(BAND_BACKDROP_BYTES * u64::from(dims.0) * u64::from(dims.1))
            .ok_or("typed layer style scratch overflow")?;
    }
    if let Some(canvas) = package {
        scratch = scratch
            .checked_add(16 * u64::from(canvas.0) * u64::from(canvas.1))
            .ok_or("typed layer style scratch overflow")?;
    }
    if crate::wide_color_native::holds_wide(style) {
        scratch = scratch
            .checked_add(crate::authored_color::wide_scratch_bytes())
            .ok_or("typed layer style scratch overflow")?;
    }
    let work = p
        .checked_mul(2 + 8 * enabled)
        .ok_or("typed layer style work overflow")?;
    Ok((scratch, work))
}

/// The enabled effects that hold a per-pixel `t` field (`style_scratch`'s
/// `2·P` term each): Shape Burst strokes and Gradient glows
/// (`style_glow_gradient`).
fn shape_bursts(style: &LayerStyle) -> u64 {
    style
        .enabled_effects()
        .filter(|e| match e {
            Effect::Stroke(s) => {
                s.fill_type == crate::style::StrokeFill::Gradient
                    && s.gradient.style == crate::style::GradientStyle::ShapeBurst
            }
            Effect::OuterGlow(g) => g.fill_type == crate::style::GlowFill::Gradient,
            Effect::InnerGlow(g) => g.fill_type == crate::style::GlowFill::Gradient,
            _ => false,
        })
        .count() as u64
}

/// The byte compositing environment a typed document presents to
/// `style_grown`: the document's light and the canvas.
fn byte_env<'a>(
    profile: &'a IccProfile,
    canvas: (u32, u32),
    light: GlobalLight,
) -> CompositeEnv<'a> {
    CompositeEnv {
        light,
        canvas,
        colors: None,
        profile,
        cut: Cut::NONE,
        // `style_grown` is pure geometry — the pad and the shadow shift — so
        // neither a backdrop nor a capture can reach it.
        knockout: crate::style_knockout::KnockoutBackdrop::NONE,
        capture: None,
        // `style_grown` is pure geometry, so the evaluation policy cannot
        // reach it either.
        transfer: None,
        job: JobContext::NONE,
    }
}

/// `rect` grown by what `style` draws outside it —
/// `style_composite::style_grown` (the master) in the byte environment a
/// typed document presents: its light and its canvas.
pub(crate) fn style_grown16(
    rect: (i64, i64, i64, i64),
    style: &LayerStyle,
    profile: &IccProfile,
    canvas: (u32, u32),
    light: GlobalLight,
) -> (i64, i64, i64, i64) {
    style_grown(rect, style, byte_env(profile, canvas, light))
}

/// The canvas rect `(x0, y0, x1, y1)` a styled typed RASTER's bake occupies
/// (`style_composite::style_grown` with the document's light); `None` for an
/// unstyled layer or a group.
pub(crate) fn style_extent16_of(
    layer: &PrecisionDocumentLayer,
    profile: &IccProfile,
    canvas: (u32, u32),
    light: GlobalLight,
) -> Option<(i64, i64, i64, i64)> {
    let style = layer.style()?;
    // A CMYK raster grows by its style the same way (#1).
    let (w, h) = layer
        .source()
        .map(|image| image.dimensions())
        .or_else(|| layer.cmyk_source().map(|image| image.dimensions()))?;
    let (x, y) = layer.offset();
    let rect = (
        i64::from(x),
        i64::from(y),
        i64::from(x) + i64::from(w),
        i64::from(y) + i64::from(h),
    );
    Some(style_grown16(rect, style, profile, canvas, light))
}

/// A bake's placement: the grown rect's origin and its `(w, h)`.
type BakeExtent = ((i32, i32), (u32, u32));

/// The bake's own extent checks: the grown rect as `(origin, (w, h))`.
fn bake_extent(
    layer: &PrecisionDocumentLayer,
    profile: &IccProfile,
    canvas: (u32, u32),
    light: GlobalLight,
) -> Result<BakeExtent, String> {
    let (x0, y0, x1, y1) = style_extent16_of(layer, profile, canvas, light)
        .ok_or("this layer renders no layer style")?;
    let (uw, uh) = ((x1 - x0).max(0) as u64, (y1 - y0).max(0) as u64);
    if uw == 0 || uh == 0 {
        return Err(format!(
            "Layer “{}” with its rasterized style is empty; clear the style or export the flattened composite.",
            layer.name()
        ));
    }
    if uw.saturating_mul(uh) > MAX_PIXELS {
        return Err(crate::psd_export_rasterize::oversize_refusal(
            layer.name(),
            uw,
            uh,
        ));
    }
    if x0 < i64::from(i32::MIN) || y0 < i64::from(i32::MIN) {
        return Err(format!(
            "Layer “{}” with its rasterized style starts past the coordinate limit; move it onto the canvas or export the flattened composite.",
            layer.name()
        ));
    }
    Ok(((x0 as i32, y0 as i32), (uw as u32, uh as u32)))
}

/// Layer `layer` (a styled raster) with its style rendered into u16 pixels
/// over a transparent accumulator at opacity 1, Normal and no Blend If —
/// the byte `rasterized_style` rule, with an ENABLED mask inside the bake.
/// Returns the grown rect's origin and the standalone image.
pub(crate) fn bake_style16(
    layer: &PrecisionDocumentLayer,
    profile: &Arc<IccProfile>,
    env: &StyleEnv<'_>,
    job: JobContext<'_>,
) -> Result<((i32, i32), PrecisionImage), String> {
    let (origin, (uw, uh)) = bake_extent(layer, profile, env.canvas, env.light)?;
    let style = layer.style().ok_or("this layer renders no layer style")?;
    let image = layer.source().ok_or("this layer has no raster pixels")?;
    let count = pixel_count(uw, uh)?;
    let mut acc: Vec<[f32; 4]> = reserved_vec(count)?;
    acc.resize(count, [0.0; 4]);
    let prepared = layer
        .mask()
        .filter(|m| m.enabled())
        .map(|m| m.prepare(job.range(0.0, 0.2)))
        .transpose()?;
    // A BAKE clears both knockout keys, the byte bake's rule: the accumulator
    // is transparent, so `B0` is absent by D4 and the unit pass would only add
    // arithmetic for a backdrop that is not there.
    let baked = LayerStyle {
        fill_opacity: style.fill_opacity,
        blend_if: None,
        knockout: crate::style::Knockout::None,
        blend_interior_as_group: false,
        effects: style.effects.clone(),
        cache: crate::style_cache::RenderCache::default(),
    };
    let styled = StyledLayer {
        pixels: StyledPixels::Image {
            image,
            offset: layer.offset(),
        },
        mask: prepared.as_ref(),
        style: &baked,
        blend_if: None,
        blend: BlendMode::Normal,
        opacity: 1.0,
        members_want_base: false,
        adjusted: None,
    };
    composite_styled_into(
        &mut acc,
        (uw, uh),
        origin,
        &styled,
        env,
        None,
        // The bake's planes keep reporting nothing (13A-5b-iii-c-2 moved the
        // narrowing out of `composite_styled_into` to its call site).
        job.range(0.2, 0.9).checkpoints_only(),
    )?;
    // Under `linear_light` the accumulator is linear: the bake is encoded
    // back, as the projection is (`precision_render`'s one re-encode).
    let image = PrecisionImage::from_float_samples_unseeded(
        uw,
        uh,
        profile.clone(),
        PrecisionStorage::Rgba16Unorm,
        acc.into_iter()
            .map(|px| crate::precision_evaluation::encoded_view16(px, env.transfer)),
        job.range(0.9, 1.0),
    )?;
    Ok((origin, image))
}

impl PrecisionDocument {
    /// The canvas rect a styled typed raster's PSD bake will occupy, without
    /// reading a pixel; `None` for an unstyled layer or a group.
    pub fn style_extent16(&self, index: usize) -> Option<(i64, i64, i64, i64)> {
        style_extent16_of(
            self.layer(index)?,
            &self.profile,
            self.dimensions(),
            self.global_light,
        )
    }
}
