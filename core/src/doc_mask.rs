//! Independent raw mask storage and nondestructive mask properties.
//!
//! A mask owns a canvas offset and dimensions independently of layer pixels.
//! Density is `1 - density * (1 - coverage)`; feather is the existing scalar
//! Gaussian's radius, using style_render's bounded multiresolution evaluation.
//! The raw plane is never changed by either property. A one-entry cache pins
//! only derived coverage, is keyed by a Weak raw plane plus feather, and does
//! expensive work outside its mutex. Cloned documents can render concurrently.
//! A cancelled derivation (`derived_with_job`, Phase 13A-5a) writes no cache
//! entry: only a completed plane is ever cached.
//!
//! Raw outside coverage is explicitly0/255, default0, including typed masks; feather evaluates the
//! finite complement for outside255 through the shared blur. Mask/style/content caches key that
//! background, and geometric sampling retains it. Two bounded per-mask caches hold feathered raw
//! coverage and pixel-aligned effective coverage; their mutexes never cover rendering work, and
//! resource reporting/eviction includes both. Native v10 stores the earlier byte properties,
//! migrating v1–9 with linked/density1/feather0 defaults. Finite raw scalar edits retain the
//! outside default.

use std::borrow::Cow;
use std::sync::{Arc, Mutex, Weak};

use image::imageops::FilterType;
use image::{GrayImage, Luma, RgbaImage};

use crate::doc::{saturating_i32, DrDocument, Geometry, Layer, LayerKind, MAX_PIXELS};
use crate::doc_lock::EditKind;
use crate::doc_transform::{transform_gray, Affine};
use crate::job_control::JobContext;
use crate::style_render::{blur_plane_with_job, blur_reach};

pub const MAX_MASK_FEATHER: f32 = 1000.0;

/// Shared density arithmetic in the caller's scalar range. Byte callers round
/// only after this operation; typed effective coverage remains normalized f32.
pub(crate) fn density_coverage(coverage: f32, density: f32, maximum: f32) -> f32 {
    if density == 1.0 {
        coverage.clamp(0.0, maximum)
    } else {
        (maximum - density * (maximum - coverage)).clamp(0.0, maximum)
    }
}

#[derive(Clone)]
pub struct MaskProperties {
    pub offset: (i32, i32),
    pub linked: bool,
    pub density: f32,
    pub feather: f32,
    /// Raw coverage outside the stored rectangle; currently the PSD endpoints 0/255.
    pub outside_coverage: u8,
    cache: Arc<Mutex<Option<DerivedMask>>>,
    aligned_cache: Arc<Mutex<Option<AlignedMask>>>,
}

struct AlignedMask {
    raw: Weak<GrayImage>,
    shape: ((i64, i64), (u32, u32), u32, u32, u8),
    plane: Arc<GrayImage>,
}

#[derive(Clone)]
struct DerivedMask {
    raw: Weak<GrayImage>,
    feather: f32,
    outside_coverage: u8,
    plane: Arc<GrayImage>,
    pad: u32,
}

impl PartialEq for MaskProperties {
    fn eq(&self, other: &Self) -> bool {
        self.offset == other.offset
            && self.linked == other.linked
            && self.density == other.density
            && self.feather == other.feather
            && self.outside_coverage == other.outside_coverage
    }
}

impl Default for MaskProperties {
    fn default() -> Self {
        Self::at((0, 0))
    }
}

impl MaskProperties {
    pub fn at(offset: (i32, i32)) -> Self {
        Self {
            offset,
            linked: true,
            density: 1.0,
            feather: 0.0,
            outside_coverage: 0,
            cache: Arc::new(Mutex::new(None)),
            aligned_cache: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn valid(&self, dimensions: (u32, u32)) -> bool {
        matches!(self.outside_coverage, 0 | 255)
            && self.density.is_finite()
            && (0.0..=1.0).contains(&self.density)
            && self.feather.is_finite()
            && (0.0..=MAX_MASK_FEATHER).contains(&self.feather)
            && self.padded_dimensions(dimensions).is_some()
    }

    pub(crate) fn sigma(&self) -> f32 {
        if self.feather == 0.0 {
            0.0
        } else {
            0.3 * (self.feather - 1.0) + 0.8
        }
    }

    pub(crate) fn padded_dimensions(&self, (w, h): (u32, u32)) -> Option<(u32, u32, u32)> {
        let pad = blur_reach(self.sigma());
        let pw = w.checked_add(pad.checked_mul(2)?)?;
        let ph = h.checked_add(pad.checked_mul(2)?)?;
        (w > 0 && h > 0 && u64::from(pw) * u64::from(ph) <= MAX_PIXELS).then_some((pw, ph, pad))
    }

    pub(crate) fn scratch_bytes(&self, dims: (u32, u32)) -> u64 {
        self.padded_dimensions(dims).map_or(u64::MAX, |(w, h, _)| {
            u64::from(w).saturating_mul(u64::from(h)).saturating_mul(12)
        })
    }

    /// The feather blur's WORK over the padded plane of a `dims` raw mask —
    /// the typed `PrecisionMask::plan` figure without its cap or refusal
    /// (Phase 13A-5b-iii-b-2, the byte regional plan's mask term): 0 unfeathered
    /// or past `padded_dimensions`, else `plane_blur::float_work` plus, at
    /// outside coverage 255, two passes over the padded plane. Saturating.
    pub(crate) fn work_units(&self, dims: (u32, u32)) -> u64 {
        if self.feather == 0.0 {
            return 0;
        }
        let Some((pw, ph, _)) = self.padded_dimensions(dims) else {
            return 0;
        };
        let outside = if self.outside_coverage == 255 {
            u64::from(pw)
                .saturating_mul(u64::from(ph))
                .saturating_mul(2)
        } else {
            0
        };
        crate::plane_blur::float_work(pw, ph, self.sigma()).saturating_add(outside)
    }

    pub(crate) fn cached_planes(&self) -> Vec<Arc<GrayImage>> {
        let raw = self
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|c| Arc::clone(&c.plane));
        let aligned = self
            .aligned_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|c| Arc::clone(&c.plane));
        raw.into_iter().chain(aligned).collect()
    }

    pub(crate) fn clear_cache(&self) {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).take();
        self.aligned_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
    }

    /// The derived (feathered) coverage of `raw` and its pad, polling `job`
    /// inside the feather blur; an `Err` writes no cache entry (Phase
    /// 13A-5a).
    pub(crate) fn derived_with_job(
        &self,
        raw: &Arc<GrayImage>,
        job: JobContext<'_>,
    ) -> Result<(Arc<GrayImage>, u32), String> {
        if self.feather == 0.0 {
            return Ok((Arc::clone(raw), 0));
        }
        {
            let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(c) = cache.as_ref().filter(|c| {
                c.feather == self.feather
                    && c.outside_coverage == self.outside_coverage
                    && std::ptr::eq(c.raw.as_ptr(), Arc::as_ptr(raw))
            }) {
                return Ok((Arc::clone(&c.plane), c.pad));
            }
        }
        // All public mutation and native preflight paths validate this bound.
        // Invalid hand-built internal state retains raw coverage defensively.
        let Some((w, h, pad)) = self.padded_dimensions(raw.dimensions()) else {
            return Ok((Arc::clone(raw), 0));
        };
        let mut plane = GrayImage::from_pixel(w, h, Luma([self.outside_coverage]));
        image::imageops::replace(&mut plane, &**raw, i64::from(pad), i64::from(pad));
        // Evaluate the finite deviation as hidden coverage when the background
        // is white. The shared multiresolution blur zero-extends its workspace;
        // complementing avoids a dark seam at that workspace's outer edge.
        if self.outside_coverage == 255 {
            for value in plane.as_mut() {
                *value = 255 - *value;
            }
        }
        blur_plane_with_job(plane.as_mut(), w, h, self.sigma(), job)?;
        if self.outside_coverage == 255 {
            for value in plane.as_mut() {
                *value = 255 - *value;
            }
        }
        let plane = Arc::new(plane);
        *self.cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(DerivedMask {
            raw: Arc::downgrade(raw),
            feather: self.feather,
            outside_coverage: self.outside_coverage,
            plane: Arc::clone(&plane),
            pad,
        });
        Ok((plane, pad))
    }
}

impl Layer {
    /// One aligned effective plane is reused by raster compositing tiles and
    /// styles. Keys contain raw identity and every effective shape input;
    /// moving linked pixels+mask together therefore reuses its coverage.
    pub(crate) fn aligned_mask(&self) -> Option<Arc<GrayImage>> {
        self.aligned_mask_with_job(JobContext::NONE)
            .expect("uncancellable mask alignment")
    }

    /// [`Self::aligned_mask`] polling `job` inside the feather blur and at
    /// every row of the crop (Phase 13A-5b-ii-a, the crop's rows 13A-5b-ii-b);
    /// the same plane. The aligned cache is
    /// stored only after the rect returns `Ok`, so a cancel caches nothing.
    pub(crate) fn aligned_mask_with_job(
        &self,
        job: JobContext<'_>,
    ) -> Result<Option<Arc<GrayImage>>, String> {
        let Some(raw) = self.mask.as_ref() else {
            return Ok(None);
        };
        let p = &self.mask_properties;
        let dimensions = self.pixels.dimensions();
        if p.feather == 0.0
            && p.density == 1.0
            && p.offset == self.offset
            && raw.dimensions() == dimensions
        {
            return Ok(Some(Arc::clone(raw)));
        }
        let shape = (
            (
                i64::from(p.offset.0) - i64::from(self.offset.0),
                i64::from(p.offset.1) - i64::from(self.offset.1),
            ),
            dimensions,
            p.density.to_bits(),
            p.feather.to_bits(),
            p.outside_coverage,
        );
        {
            let cache = p.aligned_cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(c) = cache
                .as_ref()
                .filter(|c| c.shape == shape && std::ptr::eq(c.raw.as_ptr(), Arc::as_ptr(raw)))
            {
                return Ok(Some(Arc::clone(&c.plane)));
            }
        }
        let Some(rect) =
            self.effective_mask_rect_with_job(self.offset, dimensions.0, dimensions.1, job)?
        else {
            return Ok(None);
        };
        let plane = Arc::new(rect.into_owned());
        *p.aligned_cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(AlignedMask {
            raw: Arc::downgrade(raw),
            shape,
            plane: Arc::clone(&plane),
        });
        Ok(Some(plane))
    }

    /// [`Self::active_mask`]'s rule over [`Self::aligned_mask_with_job`]:
    /// `Ok(None)` unless the mask is enabled (Phase 13A-5b-ii-a). The caller
    /// passes `checkpoints_only`.
    pub(crate) fn active_mask_with_job(
        &self,
        job: JobContext<'_>,
    ) -> Result<Option<Arc<GrayImage>>, String> {
        if !self.mask_enabled {
            return Ok(None);
        }
        self.aligned_mask_with_job(job)
    }

    /// Effective coverage aligned to any canvas rectangle. Ignores the enabled
    /// flag so Apply Mask and mask editing previews can inspect disabled masks.
    pub(crate) fn effective_mask_rect(
        &self,
        origin: (i32, i32),
        w: u32,
        h: u32,
    ) -> Option<Cow<'_, GrayImage>> {
        self.effective_mask_rect_with_job(origin, w, h, JobContext::NONE)
            .expect("uncancellable context")
    }

    /// [`Self::effective_mask_rect`] polling `job` inside the feather blur
    /// (`MaskProperties::derived_with_job`, Phase 13A-5b-ii-a) and at the head
    /// of every row of the crop (Phase 13A-5b-ii-b); the same coverage, in the
    /// same row-major order.
    pub(crate) fn effective_mask_rect_with_job(
        &self,
        origin: (i32, i32),
        w: u32,
        h: u32,
        job: JobContext<'_>,
    ) -> Result<Option<Cow<'_, GrayImage>>, String> {
        let Some(raw) = self.mask.as_ref() else {
            return Ok(None);
        };
        let p = &self.mask_properties;
        if p.feather == 0.0 && p.density == 1.0 && p.offset == origin && raw.dimensions() == (w, h)
        {
            return Ok(Some(Cow::Borrowed(raw)));
        }
        let (plane, pad) = p.derived_with_job(raw, job)?;
        let mut crop = Vec::with_capacity(w as usize * h as usize);
        for y in 0..h {
            job.check()?;
            for x in 0..w {
                crop.push(self.effective_coverage_at(
                    &plane,
                    pad,
                    i64::from(origin.0) + i64::from(x),
                    i64::from(origin.1) + i64::from(y),
                ));
            }
        }
        let crop = GrayImage::from_raw(w, h, crop)
            .ok_or_else(|| "internal error: mask crop size".to_string())?;
        Ok(Some(Cow::Owned(crop)))
    }

    /// The per-sample body of [`Self::effective_mask_rect`]: the derived
    /// plane (`MaskProperties::derived_with_job` — the raw plane with pad 0 when
    /// feather is 0) read at canvas (`canvas_x`, `canvas_y`) through the
    /// mask's offset and pad, `outside_coverage` beyond it, then density,
    /// rounded to a byte. The regional compositor (`doc_regional`) reads a
    /// mask sample by sample through this same statement sequence.
    pub(crate) fn effective_coverage_at(
        &self,
        plane: &GrayImage,
        pad: u32,
        canvas_x: i64,
        canvas_y: i64,
    ) -> u8 {
        let p = &self.mask_properties;
        let mx = canvas_x - i64::from(p.offset.0) + i64::from(pad);
        let my = canvas_y - i64::from(p.offset.1) + i64::from(pad);
        let coverage = if mx >= 0
            && my >= 0
            && mx < i64::from(plane.width())
            && my < i64::from(plane.height())
        {
            plane.get_pixel(mx as u32, my as u32)[0]
        } else {
            p.outside_coverage
        };
        density_coverage(f32::from(coverage), p.density, 255.0).round() as u8
    }

    pub(crate) fn translate_mask(&mut self, dx: i64, dy: i64) {
        if self.mask.is_some() {
            self.mask_properties.offset = (
                saturating_i32(i64::from(self.mask_properties.offset.0) + dx),
                saturating_i32(i64::from(self.mask_properties.offset.1) + dy),
            );
            if self.kind == LayerKind::Group {
                self.offset = self.mask_properties.offset;
            }
        }
    }

    pub(crate) fn affine_mask(&mut self, m: Affine, filter: FilterType) -> Option<()> {
        self.affine_mask_with_job(m, filter, JobContext::NONE)
            .ok()
            .flatten()
    }

    /// [`Self::affine_mask`] polling `job` through the resample: refusals
    /// stay `Ok(None)`, `Err` is only the cancel, and nothing is written
    /// before the resample returns.
    pub(crate) fn affine_mask_with_job(
        &mut self,
        m: Affine,
        filter: FilterType,
        job: JobContext<'_>,
    ) -> Result<Option<()>, String> {
        let Some(raw) = &self.mask else {
            return Ok(Some(()));
        };
        let Some(dest) = crate::doc_transform::transformed_plane_bounds(
            raw.dimensions(),
            self.mask_properties.offset,
            &m,
        ) else {
            return Ok(None);
        };
        if !self.mask_properties.valid((dest.2, dest.3)) {
            return Ok(None);
        }
        let Some((mask, offset)) = transform_gray(
            raw,
            self.mask_properties.offset,
            m,
            filter,
            self.mask_properties.outside_coverage,
            job,
        )?
        else {
            return Ok(None);
        };
        if !self.mask_properties.valid(mask.dimensions()) {
            return Ok(None);
        }
        self.mask = Some(mask);
        self.mask_properties.offset = offset;
        if self.kind == LayerKind::Group {
            self.offset = offset;
        }
        Ok(Some(()))
    }

    pub(crate) fn geometry_mask(&mut self, geom: Geometry, cw: u32, ch: u32) {
        let Some(mask) = &self.mask else {
            return;
        };
        let (x, y) = self.mask_properties.offset;
        let (w, h) = mask.dimensions();
        let (x, y, w, h) = (i64::from(x), i64::from(y), i64::from(w), i64::from(h));
        let offset = match geom {
            Geometry::Rotate90 => (i64::from(ch) - y - h, x),
            Geometry::Rotate180 => (i64::from(cw) - x - w, i64::from(ch) - y - h),
            Geometry::Rotate270 => (y, i64::from(cw) - x - w),
            Geometry::FlipH => (i64::from(cw) - x - w, y),
            Geometry::FlipV => (x, i64::from(ch) - y - h),
        };
        self.mask = Some(Arc::new(geom.apply(&**mask)));
        self.mask_properties.offset = (saturating_i32(offset.0), saturating_i32(offset.1));
        if self.kind == LayerKind::Group {
            self.offset = self.mask_properties.offset;
        }
    }

    pub(crate) fn resize_mask(&mut self, fx: f64, fy: f64, filter: FilterType) -> Option<()> {
        let Some(raw) = &self.mask else {
            return Some(());
        };
        let dims = (
            ((f64::from(raw.width()) * fx).round() as u32).max(1),
            ((f64::from(raw.height()) * fy).round() as u32).max(1),
        );
        if !self.mask_properties.valid(dims) {
            return None;
        }
        self.mask = Some(Arc::new(image::imageops::resize(
            &**raw, dims.0, dims.1, filter,
        )));
        self.mask_properties.offset = (
            saturating_i32((f64::from(self.mask_properties.offset.0) * fx).round() as i64),
            saturating_i32((f64::from(self.mask_properties.offset.1) * fy).round() as i64),
        );
        if self.kind == LayerKind::Group {
            self.offset = self.mask_properties.offset;
        }
        Some(())
    }
}

impl DrDocument {
    pub(crate) fn mask_transform_fits(&self, idx: usize, m: &Affine) -> bool {
        let Some(layer) = self.layers.get(idx) else {
            return false;
        };
        let Some(raw) = &layer.mask else {
            return true;
        };
        let Some(dest) = crate::doc_transform::transformed_plane_bounds(
            raw.dimensions(),
            layer.mask_properties.offset,
            m,
        ) else {
            return false;
        };
        layer.mask_properties.valid((dest.2, dest.3))
            && self.native_masks_fit_dimensions(self.layers.iter().enumerate().filter_map(
                |(i, l)| {
                    l.mask.as_ref().map(|mask| {
                        if i == idx {
                            (dest.2, dest.3)
                        } else {
                            mask.dimensions()
                        }
                    })
                },
            ))
    }

    /// Sets nondestructive properties; None for missing/locked mask, invalid
    /// finite range or padded extent, or a no-op. Does not move the mask when
    /// linking: its current displacement relative to pixels is retained.
    pub fn set_mask_properties(
        &self,
        idx: usize,
        linked: bool,
        density: f32,
        feather: f32,
    ) -> Option<Self> {
        self.under_locks(idx, EditKind::Mask, |doc| {
            let layer = doc.layers.get(idx)?;
            let mut properties = layer.mask_properties.clone();
            properties.linked = linked;
            properties.density = density;
            properties.feather = feather;
            if properties == layer.mask_properties
                || !properties.valid(layer.mask.as_ref()?.dimensions())
            {
                return None;
            }
            let mut out = doc.clone();
            out.layers[idx].mask_properties = properties;
            Some(out)
        })
    }

    /// Resamples only raw mask coverage in canvas coordinates, retaining
    /// independent bounds. Integer translations are lossless offset changes.
    /// Position/pixel locks protect content; only Lock All protects masks.
    pub fn transform_mask(&self, idx: usize, m: Affine, filter: FilterType) -> Option<Self> {
        if m == Affine::from_array([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]) {
            return None;
        }
        self.under_locks(idx, EditKind::Mask, |doc| {
            doc.layers.get(idx)?.mask.as_ref()?;
            if !doc.mask_transform_fits(idx, &m) {
                return None;
            }
            let mut out = doc.clone();
            out.layers[idx].affine_mask(m, filter)?;
            doc.native_edit_fits(&out).then_some(out)
        })
    }

    /// Canvas-sized opaque grayscale effective coverage, including density and
    /// feather. Ignores mask-enabled for inspection; `None` for a missing
    /// layer or mask. No app path reads it; it is the mask tests' oracle.
    pub fn layer_effective_mask_image(&self, idx: usize) -> Option<RgbaImage> {
        let mask = self
            .layers
            .get(idx)?
            .effective_mask_rect((0, 0), self.width, self.height)?;
        crate::doc_plane::gray_image(mask.as_raw(), self.width, self.height)
    }
}
