//! Sample All Layers for the Blur, Sharpen and Smudge brushes: the
//! PICTURE they read in place of the target layer, at every depth.
//!
//! [`SAMPLE_ALL_LAYERS`] names which picture: `All`, every visible layer as
//! it composites, those above the target included and hidden ones left out.
//! It is one `retouch_sampling` mode, read by every path here and nowhere
//! else, so the choice is one line. The plate builders are the existing ones,
//! never a new compositor:
//!
//! - byte: [`DrDocument::sampled_region`] over a canvas rect, `render_region`
//!   for `All` (the projection `dr_doc_flattened` returns) and
//!   `inpaint_source::sampling_source` cropped for the other modes;
//! - 16-bit and float: [`PrecisionDocument::sampled_plate`], the canvas-sized
//!   `retouch_source_with_context` plate in the document's render storage;
//! - CMYK: [`PrecisionDocument::cmyk_sampled_passes`], the
//!   `cmyk_retouch_plate` ink split into its two `L` passes
//!   (`cmyk_passes::pass_image`), each read by the RGB walk on its own pass.
//!
//! A brush reads the picture only inside the canvas: its walk rect and halo
//! are clipped to the canvas ([`canvas_reach`]) and Smudge's footprint to the
//! layer ∩ canvas ([`clip_footprint`]), so a layer's pixels outside the
//! canvas are never read or written. The write is the kernel's
//! (`retouch_brush::mix_over`, `retouch_smudge`); this module only finds the
//! samples.
use image::RgbaImage;

use crate::cmyk_passes::{pass_image, InkPass};
use crate::doc::DrDocument;
use crate::float_document::FLOAT_RANGE_MAX;
use crate::float_filter_apply::PLAN_RANGE;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{pixel_count, reserved_vec, PrecisionImage, PrecisionStorage};
use crate::regional_plan::RegionRequest;
use crate::retouch_brush::{halo_in, mix_over, targets_in, RetouchBrushKind, Vibrancy};
use crate::retouch_sampling::RetouchSampling;
use crate::sampling::EXTENDED_ALPHA_FLOOR;

/// The picture Sample All Layers reads (module doc): `All`, every visible
/// layer as it composites, those above the target included. That is the
/// owner's correction of 2026-10-15, which replaced the Current & Below rule
/// that first shipped. The one Photoshop take
/// (`scripts/photoshop-probe/retouch_options_take.py`, take1) was undecided
/// — its scripted Blur under Sample All Layers wrote nothing to the empty
/// target. `#[doc(hidden)] pub` so the black-box tests take the mode from
/// here.
#[doc(hidden)]
pub const SAMPLE_ALL_LAYERS: RetouchSampling = RetouchSampling::All;

/// The canvas rect `(x, y, w, h)` that a layer rect `walk` (layer
/// coordinates) grown by `reach` on every side covers once offset by
/// `offset` and clipped to `canvas`; `None` when nothing is left.
pub(crate) fn canvas_reach(
    walk: (u32, u32, u32, u32),
    reach: u32,
    offset: (i32, i32),
    canvas: (u32, u32),
) -> Option<(u32, u32, u32, u32)> {
    let (x, y, w, h) = walk;
    let x0 = (i64::from(x) - i64::from(reach) + i64::from(offset.0)).max(0);
    let y0 = (i64::from(y) - i64::from(reach) + i64::from(offset.1)).max(0);
    let x1 = (i64::from(x) + i64::from(w) + i64::from(reach) + i64::from(offset.0))
        .min(i64::from(canvas.0));
    let y1 = (i64::from(y) + i64::from(h) + i64::from(reach) + i64::from(offset.1))
        .min(i64::from(canvas.1));
    (x0 < x1 && y0 < y1).then(|| (x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32))
}

/// Smudge's `footprint` `[x0, y0, x1, y1)` (layer coordinates) intersected
/// with the canvas, still in layer coordinates; `None` when it misses.
pub(crate) fn clip_footprint(
    footprint: [usize; 4],
    offset: (i32, i32),
    canvas: (u32, u32),
) -> Option<[usize; 4]> {
    let x0 = (footprint[0] as i64).max(-i64::from(offset.0));
    let y0 = (footprint[1] as i64).max(-i64::from(offset.1));
    let x1 = (footprint[2] as i64).min(i64::from(canvas.0) - i64::from(offset.0));
    let y1 = (footprint[3] as i64).min(i64::from(canvas.1) - i64::from(offset.1));
    (x0 < x1 && y0 < y1).then_some([x0 as usize, y0 as usize, x1 as usize, y1 as usize])
}

/// A canvas rect of a canvas-sized typed plate as straight samples: unit
/// range for u16 storage (`v / 65535`), the stored f32 for float.
pub(crate) fn plate_samples(
    plate: &PrecisionImage,
    (x, y, w, h): (u32, u32, u32, u32),
) -> Result<Vec<[f32; 4]>, String> {
    let alloc = || "could not allocate the sampled picture".to_string();
    match plate.storage() {
        PrecisionStorage::Rgba16Unorm => {
            let codes = plate.read_rect_rgba16(x, y, w, h).ok_or_else(alloc)?;
            let mut out = reserved_vec(codes.len())?;
            out.extend(codes.iter().map(|p| p.map(|v| f32::from(v) / 65535.0)));
            Ok(out)
        }
        PrecisionStorage::Rgba32Float => plate.read_rect_rgba32(x, y, w, h).ok_or_else(alloc),
    }
}

impl DrDocument {
    /// The [`SAMPLE_ALL_LAYERS`] picture over canvas rect `rect` (already
    /// inside the canvas) for target layer `idx`, straight RGBA8.
    pub(crate) fn sampled_region(
        &self,
        idx: usize,
        (x, y, width, height): (u32, u32, u32, u32),
    ) -> Result<RgbaImage, String> {
        if SAMPLE_ALL_LAYERS == RetouchSampling::All {
            let request = RegionRequest {
                x,
                y,
                width,
                height,
                preview_divisor: 1,
            };
            return self.render_region(request, u64::MAX, None);
        }
        let plate = crate::inpaint_source::sampling_source(
            self,
            idx,
            SAMPLE_ALL_LAYERS,
            false,
            0,
            JobContext::NONE,
        )?
        .ok_or("sample all layers requires a raster layer")?;
        Ok(image::imageops::crop_imm(&plate, x, y, width, height).to_image())
    }
}

impl PrecisionDocument {
    /// What [`Self::sampled_plate`] (RGB) or [`Self::cmyk_sampled_passes`]
    /// (CMYK) holds: the plate's own plan, and on CMYK the ink plate's
    /// render (`cmyk_plate_scratch`) plus its two RGBA16 passes (16 B a
    /// canvas pixel).
    pub(crate) fn sampled_plate_plan(&self, layer: usize) -> Result<u64, String> {
        if self.is_cmyk() {
            let (w, h) = self.dimensions();
            let canvas = pixel_count(w, h)? as u64;
            return self
                .cmyk_plate_scratch(SAMPLE_ALL_LAYERS)?
                .checked_add(canvas * 16)
                .ok_or_else(|| "sampled picture plan overflow".to_string());
        }
        self.retouch_source_plan(layer, SAMPLE_ALL_LAYERS)
    }

    /// The canvas-sized [`SAMPLE_ALL_LAYERS`] picture for target `layer` in
    /// the document's render storage, admitted and reserved by its own plan
    /// against `limit` beside `other`.
    pub(crate) fn sampled_plate(
        &self,
        layer: usize,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        self.retouch_source_with_context(layer, SAMPLE_ALL_LAYERS, false, limit, other, job)
    }

    /// A CMYK document's [`SAMPLE_ALL_LAYERS`] ink plate for record `layer`
    /// as its two canvas-sized `L` passes, `[Cmy, K]`. Admission is the
    /// caller's ([`Self::sampled_plate_plan`] is its term).
    pub(crate) fn cmyk_sampled_passes(
        &self,
        layer: usize,
        job: JobContext<'_>,
    ) -> Result<[PrecisionImage; 2], String> {
        let plate = self.cmyk_retouch_plate(layer, SAMPLE_ALL_LAYERS, false, job)?;
        Ok([
            pass_image(&plate, InkPass::Cmy, false, job)?,
            pass_image(&plate, InkPass::K, false, job)?,
        ])
    }
}

/// Smudge's starting field over layer rect `rect` (`[x0, y0, x1, y1)`, inside
/// the canvas) of a layer at `offset`: the typed picture `plate` there,
/// premultiplied in f32.
pub(crate) fn sampled_field(
    plate: &PrecisionImage,
    rect: [usize; 4],
    offset: (i32, i32),
) -> Result<Vec<[f32; 4]>, String> {
    let canvas_rect = (
        (rect[0] as i64 + i64::from(offset.0)) as u32,
        (rect[1] as i64 + i64::from(offset.1)) as u32,
        (rect[2] - rect[0]) as u32,
        (rect[3] - rect[1]) as u32,
    );
    let mut field = plate_samples(plate, canvas_rect)?;
    for p in &mut field {
        *p = [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]];
    }
    Ok(field)
}

/// The float Blur or Sharpen walk under Sample All Layers (the
/// `float_retouch_brush` arm, the 16-bit `precision_retouch_brush` walk its
/// master): `px` is the layer's walk rect `walk` (layer coordinates, a layer
/// at `offset` on `canvas`), rewritten in place; `amount_at(i)` is pixel
/// `i`'s coverage × strength. The targets read `plate` over the walk rect
/// and its float halo on the canvas; each covered pixel becomes
/// `retouch_brush::mix_over` of the layer and `[target, sampled α]`, colour
/// held to ±`FLOAT_RANGE_MAX`, alpha below `EXTENDED_ALPHA_FLOOR` transparent
/// black (or the pixel as it was when it held nothing); `frozen` keeps alpha
/// and leaves alpha-0 pixels. Whether any bit changed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn float_sampled_walk(
    kind: RetouchBrushKind,
    px: &mut [[f32; 4]],
    walk: (u32, u32, u32, u32),
    offset: (i32, i32),
    canvas: (u32, u32),
    plate: &PrecisionImage,
    amount_at: impl Fn(usize) -> f64,
    frozen: bool,
    job: JobContext<'_>,
) -> Result<bool, String> {
    let (rx, ry, rw, rh) = walk;
    let reach = halo_in(kind, &PLAN_RANGE);
    let Some((sx, sy, sw, sh)) = canvas_reach(walk, reach, offset, canvas) else {
        return Ok(false);
    };
    let picture = plate_samples(plate, (sx, sy, sw, sh))?;
    let read = |x: i64, y: i64| -> [f32; 4] {
        let cx = (i64::from(rx) + x + i64::from(offset.0))
            .clamp(i64::from(sx), i64::from(sx + sw) - 1)
            - i64::from(sx);
        let cy = (i64::from(ry) + y + i64::from(offset.1))
            .clamp(i64::from(sy), i64::from(sy + sh) - 1)
            - i64::from(sy);
        picture[cy as usize * sw as usize + cx as usize]
    };
    let target = targets_in(
        kind,
        Vibrancy::Off,
        (rw, rh),
        &PLAN_RANGE,
        read,
        job.range(0., 0.6),
    )?;
    job.report(0.6, 3)?;
    let bounded = |v: f32| v.clamp(-FLOAT_RANGE_MAX, FLOAT_RANGE_MAX);
    let mut changed = false;
    for (at, slot) in px.iter_mut().enumerate() {
        job.every(at)?;
        let old = *slot;
        if frozen && old[3] == 0. {
            continue;
        }
        let amount = amount_at(at);
        if amount == 0. {
            continue;
        }
        let (column, row) = ((at % rw as usize) as i64, (at / rw as usize) as i64);
        let t = target[at];
        let wanted = [t[0], t[1], t[2], read(column, row)[3]];
        let out = mix_over(old, wanted, amount as f32, frozen);
        let mut new = [bounded(out[0]), bounded(out[1]), bounded(out[2]), old[3]];
        if !frozen {
            new[3] = out[3].clamp(0., 1.);
            if new[3] < EXTENDED_ALPHA_FLOOR {
                new = if old[3] == 0. { old } else { [0.; 4] };
            }
        }
        if new.map(f32::to_bits) != old.map(f32::to_bits) {
            *slot = new;
            changed = true;
        }
    }
    Ok(changed)
}
