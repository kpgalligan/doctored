//! Read-only inspection of genuine developed pixels. Exact detail uses the
//! shared nearest geometry walk, and display conversion uses the shared byte
//! quantizer. Fit-view clipping scans every source cell, independently of the
//! one center sample chosen for its explicitly approximate color preview.
//!
//! Exact detail uses the existing nearest geometry walk and is capped at 1024 pixels per side; byte
//! display uses the shared quantizer and is capped at 1600 per side. Divisor 1 preserves source
//! pixel positions; larger fit divisors sample cell centres for colour while endpoint markers
//! inspect every alpha-positive native pixel in the cell, including partial edge cells.
//! Blue/red/magenta indicate any RGB component at exactly 0/65535/both, independently of rounded
//! display bins. The checked `precision_stats.rs` histogram accepts the caller's live-set
//! allowance; the Develop host uses stride 1 over the complete source. All regions, sample storage,
//! output bounds and working-memory requirements are validated before allocation; optional job
//! tokens are single-use. Source extraction shares original samples/profile and cannot promote a
//! byte fallback. The broader Phase 8D UI/automation qualification remains separate from kernel
//! tests; camera decoding and recipe interpretation stay in the host. The preview still binds its
//! source's view: its tile band walk MISSED the 13B-3e-3b warm bound, so the read stays on
//! `preview_view_rgba16`, kept exception by decision (13B-3e-4c-1 D2) (13B-3e-3b D4); since
//! 13B-3e-4a the host intakes and the develop output are unseeded, so the FIRST preview of such a
//! plane materializes that view (later calls hit the cache).
//!
//! The extended preview (#4, [`PrecisionImage::develop_preview_extended`]) is the f32
//! twin of `develop_preview` at Fit and at 100 % (divisor 1): the same cells ([`cell_span`]), each
//! centre sample through `display_encoding`'s rule X at gain 1 (`extended_display_sample`, the one
//! display path the editor's EDR view uses) into RGBA binary16, admitted as that view's measured
//! plan (`float_view::extended_display_scratch`) at the preview's own size. Its clipping markers
//! OR every alpha-positive sample of the cell through `clipped_channels_float` (at or below 0.0,
//! at or above 1.0: the ⌥ display's inclusive float rule) and are written as opaque binary16
//! blue/red/magenta; the peak it returns excludes them.
use crate::clip_display::{clipped_channels, clipped_channels_float};
use crate::display_encoding::extended_display_sample;
use crate::doc::DrDocument;
use crate::doc_transform::{Affine, SourceMap};
use crate::float_view::extended_display_scratch;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{
    admit, reserved_vec, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::precision_quantize::nearest;
use crate::precision_sampling::{transform_image, GeometryFilter};
use image::RgbaImage;

const MAX_DETAIL_SIDE: u32 = 1024;
const MAX_PREVIEW_SIDE: u32 = 1600;

impl DrDocument {
    /// Share a genuine retained u16 master, never its byte fallback or affine.
    pub fn source_master16(&self, index: usize) -> Result<PrecisionImage, String> {
        self.layers
            .get(index)
            .and_then(|l| l.effective_source(&self.profile))
            .and_then(|s| s.asset.rgba16().cloned())
            .ok_or_else(|| "layer has no valid retained u16 master".into())
    }
}
impl PrecisionDocument {
    /// Share a genuine retained u16 master, before fallback geometry and retouch.
    pub fn source_master16(&self, index: usize) -> Result<PrecisionImage, String> {
        self.layer(index)
            .and_then(|l| l.effective_source())
            .and_then(|s| s.asset.rgba16().cloned())
            .ok_or_else(|| "layer has no valid retained u16 master".into())
    }
    /// Share the retained developed master at its own depth, before fallback
    /// geometry and retouch: a 16-bit SDR development's u16 master or a
    /// 32-bit float (linear) development's f32 one. Refuses a byte master
    /// and a missing or stale binding.
    pub fn source_master(&self, index: usize) -> Result<PrecisionImage, String> {
        self.layer(index)
            .and_then(|l| l.effective_source())
            .and_then(|s| s.asset.rgba16().or(s.asset.rgba32()).cloned())
            .ok_or_else(|| "layer has no valid retained u16 or f32 master".into())
    }
}

#[derive(Clone, Copy)]
pub struct DevelopRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
pub struct DevelopPreview {
    pub region: DevelopRegion,
    pub divisor: u32,
    pub clipping: u32,
}
impl DevelopRegion {
    fn validate_bounds(self, image: &PrecisionImage) -> Result<(), String> {
        let (width, height) = image.dimensions();
        if self.width == 0
            || self.height == 0
            || self.x.checked_add(self.width).is_none_or(|v| v > width)
            || self.y.checked_add(self.height).is_none_or(|v| v > height)
        {
            return Err("develop inspection region must lie inside its source".into());
        }
        Ok(())
    }
    fn validate(self, image: &PrecisionImage) -> Result<(), String> {
        self.validate_bounds(image)?;
        if image.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("develop inspection requires genuine rgba16_unorm samples".into());
        }
        Ok(())
    }
}

/// The cell of output index `index` along one axis of a region starting at
/// `start` and `extent` long, at `divisor`: `begin..end`, clipped to the
/// region so an edge cell may be partial, and its centre
/// `begin + floor((end − begin) / 2)`. The ONE statement of the Fit cell
/// geometry, shared by `develop_preview` and `develop_preview_extended`.
fn cell_span(start: u32, extent: u32, index: u32, divisor: u32) -> (u32, u32, u32) {
    let begin = start + index * divisor;
    let end = begin.saturating_add(divisor).min(start + extent);
    (begin, end, begin + (end - begin) / 2)
}

/// The byte length of [`PrecisionImage::develop_preview_extended`]'s output
/// for `region` at `divisor`: `ceil(width / divisor) · ceil(height / divisor)`
/// RGBA binary16 pixels (4 × 2 bytes). Refuses divisor 0 and an output side
/// past 1600. The FFI computes the buffer it lends from this, never from the
/// caller's length.
pub(crate) fn extended_preview_len(region: DevelopRegion, divisor: u32) -> Result<usize, String> {
    if divisor == 0 {
        return Err("develop extended preview needs a positive divisor".into());
    }
    let width = region.width.div_ceil(divisor);
    let height = region.height.div_ceil(divisor);
    if width > MAX_PREVIEW_SIDE || height > MAX_PREVIEW_SIDE {
        return Err("develop extended preview is limited to 1600 pixels per side".into());
    }
    // RGBA binary16: four 2-byte samples a pixel (the format).
    Ok(width as usize * height as usize * (4 * 2))
}

impl PrecisionImage {
    /// The image the develop inspection reads (histogram, detail, preview and
    /// their clipping markers, all defined on u16 codes). A u16 image is
    /// shared as it is. An f32 image (a 32-bit float development) is its
    /// SDR-clipped view: each RGB sample clamped to 0…1, encoded by the
    /// image's own profile curve and rounded once to a u16 code
    /// (`float_document::encoded_u16` under a Clip policy, the loop a float
    /// document's display runs); alpha passes through. A sample at or above
    /// white therefore reads 65535, which is what the highlight marker shows.
    /// Charges 16 bytes a pixel for the f32 case: the u16 plane and the
    /// tiles it is copied into.
    pub fn develop_inspection(
        &self,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        if self.storage() == crate::precision_image::PrecisionStorage::Rgba16Unorm {
            job.report(1., 5)?;
            return Ok(self.clone());
        }
        let (width, height) = self.dimensions();
        let scratch = BOOKKEEPING_BYTES + u64::from(width) * u64::from(height) * 16;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let policy = crate::float_document::FloatPolicy::for_profile(
            self.profile(),
            crate::float_document::ToneMap::Clip,
        )?;
        let result = crate::float_document::encoded_u16(&policy, self, job)?;
        job.report(1., 5)?;
        Ok(result)
    }

    /// One source pixel per result pixel, including hidden RGB and exact alpha.
    /// The fixed side cap keeps detail extraction independent of camera size.
    pub fn develop_detail(
        &self,
        region: DevelopRegion,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        region.validate(self)?;
        if region.width > MAX_DETAIL_SIDE || region.height > MAX_DETAIL_SIDE {
            return Err("develop detail is limited to 1024 pixels per side".into());
        }
        let scratch = BOOKKEEPING_BYTES + u64::from(region.width) * u64::from(region.height) * 8;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let inverse =
            Affine::from_array([1., 0., 0., 1., f64::from(region.x), f64::from(region.y)]);
        let map = SourceMap::new(&inverse, (0, 0), (0, 0));
        let result = transform_image(
            self,
            (region.width, region.height),
            &map,
            GeometryFilter::Nearest,
            None,
            job,
        )?;
        job.report(1., 5)?;
        Ok(result)
    }

    /// Bounded byte display only. Divisor 1 is pixel-native; larger divisors
    /// choose floor(cell_width/2), floor(cell_height/2) from each clipped cell.
    /// Clipping bits1/2 select exact RGB0/RGB65535 on alpha-positive pixels;
    /// blue/red/magenta markers OR every sample in that cell, not just its center.
    pub fn develop_preview(
        &self,
        preview: DevelopPreview,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<RgbaImage, String> {
        let DevelopPreview {
            region,
            divisor,
            clipping,
        } = preview;
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        region.validate(self)?;
        if divisor == 0 || clipping > 3 {
            return Err(
                "develop preview needs a positive divisor and clipping flags 0 to 3".into(),
            );
        }
        let width = region.width.div_ceil(divisor);
        let height = region.height.div_ceil(divisor);
        if width > MAX_PREVIEW_SIDE || height > MAX_PREVIEW_SIDE {
            return Err("develop preview is limited to 1600 pixels per side".into());
        }
        let scratch = BOOKKEEPING_BYTES + u64::from(width) * u64::from(height) * 4;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        // The exception kept by decision (13B-3e-4c-1 D2), bound once.
        let pixels = self
            .preview_view_rgba16()
            .ok_or("develop inspection requires u16 pixels")?;
        let stride = self.dimensions().0 as usize;
        let mut out = reserved_vec(width as usize * height as usize * 4)?;
        let mut visited = 0;
        for row in 0..height {
            job.report(f64::from(row) / f64::from(height), 6)?;
            let (y0, y1, cy) = cell_span(region.y, region.height, row, divisor);
            for column in 0..width {
                job.every(column as usize)?;
                let (x0, x1, cx) = cell_span(region.x, region.width, column, divisor);
                let center = cy as usize * stride + cx as usize;
                let mut value = pixels[center].map(nearest);
                let (mut zero, mut maximum) = (false, false);
                if clipping != 0 {
                    for y in y0..y1 {
                        for x in x0..x1 {
                            job.every(visited)?;
                            visited += 1;
                            let p = pixels[y as usize * stride + x as usize];
                            zero |=
                                clipping & 1 != 0 && clipped_channels(p, 0, u16::MAX, false) != 0;
                            maximum |=
                                clipping & 2 != 0 && clipped_channels(p, 0, u16::MAX, true) != 0;
                        }
                    }
                    value = match (zero, maximum) {
                        (true, true) => [255, 0, 255, 255],
                        (true, false) => [0, 0, 255, 255],
                        (false, true) => [255, 0, 0, 255],
                        _ => value,
                    };
                }
                out.extend_from_slice(&value);
            }
        }
        job.report(1., 5)?;
        RgbaImage::from_raw(width, height, out)
            .ok_or_else(|| "invalid develop preview dimensions".into())
    }

    /// The extended-range preview of a 32-bit float development (#4)
    /// at Fit or 100 % (divisor 1): output `ceil(region / divisor)` per side,
    /// at most 1600, each pixel the cell-centre sample `develop_preview`
    /// picks, through `extended_display_sample(pixel, 1.0)` (rule X: RGB held
    /// to ±65504, times alpha, binary16 nearest-even), written into `out` as
    /// RGBA binary16 little-endian, premultiplied, linear in the image's
    /// primaries. Clipping bits 1/2 mark a cell any of whose alpha-positive
    /// samples has an RGB channel at or below 0.0 / at or above 1.0
    /// (`clipped_channels_float`, the ⌥ display's inclusive float rule): the
    /// cell is written opaque binary16 blue (0,0,1,1), red (1,0,0,1) or
    /// magenta (1,0,1,1). Returns `(peak, marked)`: the largest RGB component
    /// of the picture samples (markers excluded, so clipping 0 writes exactly
    /// what it always did) and the count of marked output pixels. Admits
    /// `extended_display_scratch` at the output size, the EDR view's measured
    /// plan. Refuses a region outside the image, u16 storage, clipping past 3,
    /// divisor 0, an output side past 1600, an `out` that is not exactly the
    /// output's bytes, scratch past `limit` beside `other` and a cancel. The
    /// walk writes a private buffer (inside the plan) copied into `out` only
    /// once it completes, so every refusal, a cancel included, leaves `out`
    /// as it was.
    pub fn develop_preview_extended(
        &self,
        preview: DevelopPreview,
        out: &mut [u8],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<(f32, u64), String> {
        let DevelopPreview {
            region,
            divisor,
            clipping,
        } = preview;
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        region.validate_bounds(self)?;
        if self.storage() != PrecisionStorage::Rgba32Float {
            return Err("develop extended preview requires rgba32_float samples".into());
        }
        if clipping > 3 {
            return Err("develop extended preview needs clipping flags 0 to 3".into());
        }
        let expected = extended_preview_len(region, divisor)?;
        if out.len() != expected {
            return Err(
                "the develop extended preview buffer must hold width × height × 8 bytes".into(),
            );
        }
        let width = region.width.div_ceil(divisor);
        let height = region.height.div_ceil(divisor);
        let scratch = extended_display_scratch(width, height)?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        // Binary16 0.0 and 1.0: the markers are opaque display primaries.
        const ZERO: u16 = 0x0000;
        const ONE: u16 = 0x3C00;
        let mut peak = f32::NEG_INFINITY;
        let mut marked = 0u64;
        let mut visited = 0;
        let mut written = reserved_vec(expected)?;
        for row in 0..height {
            job.report(f64::from(row) / f64::from(height), 6)?;
            let (y0, y1, cy) = cell_span(region.y, region.height, row, divisor);
            for column in 0..width {
                job.every(column as usize)?;
                let (x0, x1, cx) = cell_span(region.x, region.width, column, divisor);
                let pixel = self
                    .sample_at(cx, cy)
                    .ok_or("develop extended preview sample outside its source")?;
                let mut sample = extended_display_sample(pixel, 1.0);
                for bits in &sample[..3] {
                    peak = peak.max(half::f16::from_bits(*bits).to_f32());
                }
                if clipping != 0 {
                    let (mut low, mut high) = (false, false);
                    for y in y0..y1 {
                        for x in x0..x1 {
                            job.every(visited)?;
                            visited += 1;
                            let p = self
                                .sample_at(x, y)
                                .ok_or("develop extended preview sample outside its source")?;
                            low |= clipping & 1 != 0 && clipped_channels_float(p, false) != 0;
                            high |= clipping & 2 != 0 && clipped_channels_float(p, true) != 0;
                        }
                    }
                    let marker = match (low, high) {
                        (true, true) => Some([ONE, ZERO, ONE, ONE]),
                        (true, false) => Some([ZERO, ZERO, ONE, ONE]),
                        (false, true) => Some([ONE, ZERO, ZERO, ONE]),
                        _ => None,
                    };
                    if let Some(marker) = marker {
                        sample = marker;
                        marked += 1;
                    }
                }
                for bits in sample {
                    written.extend_from_slice(&bits.to_le_bytes());
                }
            }
        }
        job.report(1., 5)?;
        out.copy_from_slice(&written);
        Ok((peak, marked))
    }
}
