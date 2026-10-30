//! Image ▸ Adjustments ▸ Equalize on the documents (#547): the adapters of
//! `equalize`'s kernel (the histogram, its table and the scope).
//!
//! * **8-bit RGB** ([`equalize_image`], `dr_image_equalize`): the summed R, G
//!   and B histogram of a byte image at 256 levels, each sample weighted by
//!   the caller's image-sized mask × alpha, and the table applied to EVERY
//!   pixel. The mask is histogram weight only, never a write gate: the host
//!   crops its canvas selection to the layer's placement for the weight and
//!   writes the result through its own selection (`OperationCapture`), so a
//!   feathered edge is applied once.
//! * **Typed** ([`PrecisionDocument::equalize`], the selected edit
//!   `{"op":"equalize","params":{"scope":…}}`): the canvas coverage is mapped
//!   onto the layer (`selected_adjustment::coverage_at`, the placement rule
//!   every selected edit uses). The histogram weight is coverage × alpha; the
//!   write is lerped by the coverage under `scope` `selected` and covers the
//!   whole layer under `entire` or with no selection.
//!   - RGB 16-bit: 65536 levels on the stored codes; the lerp is
//!     `mix_codes`' alpha-preserving branch.
//!   - Lab Color (8 and 16 bits, #547 phase 2c): the 16-bit arm on the
//!     Lightness plane alone — the histogram counts L\* only and the table
//!     writes L\* only, a\* and b\* kept bit for bit.
//!   - RGB 32-bit float: 65536 levels on the display encoding E
//!     (`display_encoding`): a sample in 0…1 counts at `round(65535·E)` and
//!     is decoded through D from its table value, keeping its bits when the
//!     table maps its level to itself; a sample above white counts at the top
//!     level and one below zero at level 0, and both are WRITTEN UNCHANGED
//!     (float colour is never clamped). A non-finite component is skipped and
//!     kept. The lerp is `float_adjust_apply::mix_float`.
//!   - CMYK: one histogram over the four `L = 65535 − ink` plates, at 256
//!     levels (`round(L / 257)`) in an 8-bit document and 65536 in a 16-bit
//!     one; the table acts on `L` and the lerp is on ink, as every CMYK
//!     adjustment's apply.
//!
//!   A target that is not a raster refuses by name. An alpha-0 pixel is
//!   skipped and alpha is never written. No changed sample is `None`.
//!
//! # Admission
//!
//! Each arm charges its shipped selected-edit figure plus the table and the
//! histogram, `levels × 8` bytes ([`TABLE_BYTES_PER_LEVEL`]): the 16-bit arm
//! the Auto tones' (`selected_edit_plan`), the float arm the float apply's
//! (`float_adjust_apply`), the CMYK arm the destructive CMYK adjustment's
//! (`cmyk_adjust_layer`). Measured by `scripts/cmyk-admission/measure.py`
//! (`equalize-16`, `equalize-rgb16`, `equalize-rgb32`). As every 16-bit RGB
//! selected edit, the 16-bit arm's call peaks in the host's display
//! preparation, which `PreparedDocumentEdit` admits after it, so its row is
//! gated with that charge (`ProjectionMargin`'s H2).

use serde_json::{json, Value};

use crate::cmyk_adjust_layer::DISPLAY_BYTES_PER_PIXEL;
use crate::cmyk_composite::{code, unit};
use crate::cmyk_image::CmykImage;
use crate::dr_image::DrImage;
use crate::equalize::{parse_scope, EqualizeScope, Histogram};
use crate::float_adjust_apply::{coverage_scratch, mix_float, APPLY_BYTES_PER_PIXEL};
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::selected_adjustment::{coverage_at, mix_codes, validate_coverage, SelectedEdit};

/// The table (u16) and the histogram (f64) per level, rounded up to 8.
pub(crate) const TABLE_BYTES_PER_LEVEL: u64 = 8;

/// Equalize on a byte image (module doc): `weight` is an image-sized u8 mask
/// (histogram weight only) or `None` for every pixel. `None` when no byte
/// changes, or when the weighted pixels hold fewer than two levels.
pub(crate) fn equalize_image(image: &DrImage, weight: Option<&[u8]>) -> Option<DrImage> {
    let (w, h) = image.pixels.dimensions();
    let n = w as usize * h as usize;
    if weight.is_some_and(|m| m.len() != n) {
        return None;
    }
    let mut histogram = Histogram::new(256).ok()?;
    for (i, px) in image.pixels.pixels().enumerate() {
        let mask = weight.map_or(255, |m| m[i]);
        if px.0[3] == 0 || mask == 0 {
            continue;
        }
        let k = f64::from(mask) / 255.0 * f64::from(px.0[3]) / 255.0;
        for c in 0..3 {
            histogram.add(usize::from(px.0[c]), k);
        }
    }
    let lut = histogram.lut()?;
    let mut out = image.pixels.clone();
    let mut changed = false;
    for px in out.pixels_mut() {
        if px.0[3] == 0 {
            continue;
        }
        for c in 0..3 {
            let next = lut[usize::from(px.0[c])] as u8;
            changed |= next != px.0[c];
            px.0[c] = next;
        }
    }
    changed.then_some(DrImage { pixels: out })
}

/// The typed target's refusal (module doc): a raster leaf, or by name.
fn raster_refusal(doc: &PrecisionDocument, layer: usize) -> Result<(), String> {
    let record = doc
        .layer(layer)
        .ok_or("high-depth layer index is out of range")?;
    match &record.content {
        Content::Raster { .. } | Content::CmykRaster { .. } => Ok(()),
        Content::Group(_) | Content::PassThrough(_) => {
            Err("Equalize needs a raster layer: a group has no pixels of its own".into())
        }
        Content::Adjustment(_) | Content::CmykAdjustment(_) => Err(
            "Equalize needs a raster layer: an adjustment layer has no pixels of its own".into(),
        ),
    }
}

impl PrecisionDocument {
    /// The plan of a typed Equalize (`selected_edit_plan`'s keys; module
    /// doc's `# Admission`).
    pub(crate) fn equalize_plan(&self, edit: &SelectedEdit) -> Result<Value, String> {
        parse_scope(&edit.params)?;
        raster_refusal(self, edit.layer)?;
        let record = self
            .layer(edit.layer)
            .ok_or("high-depth layer index is out of range")?;
        let (cw, ch) = self.dimensions();
        let canvas = pixel_count(cw, ch)? as u64;
        let (scratch, output) = if let Some(image) = record.cmyk_source() {
            let bits = self.require_cmyk_policy("Equalize")?.depth.bits();
            let levels: u64 = if bits <= 8 { 256 } else { 65536 };
            let (w, h) = image.dimensions();
            let layer = pixel_count(w, h)? as u64;
            let scratch = 2 * BOOKKEEPING_BYTES
                + canvas * DISPLAY_BYTES_PER_PIXEL
                + layer * 10
                + canvas * 2
                + levels * TABLE_BYTES_PER_LEVEL;
            (scratch, layer * 10)
        } else {
            let image = record.source().ok_or("Equalize needs a raster layer")?;
            let (w, h) = image.dimensions();
            let count = pixel_count(w, h)? as u64;
            let table = 65536 * TABLE_BYTES_PER_LEVEL;
            if self.is_float() {
                let coverage = coverage_scratch(canvas).ok_or("equalize scratch overflow")?;
                let scratch =
                    6 * BOOKKEEPING_BYTES + count * APPLY_BYTES_PER_PIXEL + coverage + table;
                (scratch, count * 16)
            } else {
                let output = count * 8;
                let scratch =
                    6 * BOOKKEEPING_BYTES + count * 8 + 4 * 65536 * 8 + output + canvas * 2 + table;
                (scratch, output)
            }
        };
        Ok(
            json!({"retained_bytes":self.retained_bytes(),"scratch_bytes":scratch,
            "output_bytes":output,"input_samples":canvas,"input_width":cw,"input_height":ch}),
        )
    }

    /// Equalize on raster `edit.layer` within the canvas `coverage` (empty:
    /// no selection), under the record's `Pixels` lock (module doc).
    pub(crate) fn equalize(
        &self,
        edit: &SelectedEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        validate_coverage(self, coverage)?;
        self.under_locks(edit.layer, crate::doc_lock::EditKind::Pixels, |doc| {
            doc.equalizing_unlocked(edit, coverage, limit, other, job)
        })
    }

    fn equalizing_unlocked(
        &self,
        edit: &SelectedEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let plan = self.equalize_plan(edit)?;
        let scratch = plan["scratch_bytes"]
            .as_u64()
            .ok_or("invalid equalize plan")?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        // The write gate: the coverage under `selected`, none otherwise.
        let gate: &[u16] = match parse_scope(&edit.params)? {
            EqualizeScope::Selected => coverage,
            EqualizeScope::Entire => &[],
        };
        let record = self
            .layer(edit.layer)
            .ok_or("high-depth layer index is out of range")?;
        let offset = record.offset();
        let canvas = self.dimensions();
        let content = if let Some(image) = record.cmyk_source() {
            self.equalized_cmyk(image, offset, coverage, gate, job)?
                .map(|image| Content::CmykRaster { image, offset })
        } else {
            let image = record.source().ok_or("Equalize needs a raster layer")?;
            let (w, h) = image.dimensions();
            let weight = |i: usize| coverage_at(coverage, canvas, (w, h), offset, i);
            let write = |i: usize| coverage_at(gate, canvas, (w, h), offset, i);
            let image = if self.is_float() {
                self.equalized_float(image, weight, write, job)?
            } else {
                equalized_rgba16(self, image, weight, write, job)?
            };
            image.map(|image| Content::Raster { image, offset })
        };
        let out = match content {
            Some(content) => self.replacing_content(edit.layer, content)?,
            None => None,
        };
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// The float arm (module doc).
    fn equalized_float(
        &self,
        image: &crate::precision_image::PrecisionImage,
        weight: impl Fn(usize) -> u16,
        write: impl Fn(usize) -> u16,
        job: JobContext<'_>,
    ) -> Result<Option<crate::precision_image::PrecisionImage>, String> {
        let encoding = self.display_encoding()?;
        let (w, h) = image.dimensions();
        let runs = || {
            image
                .runs_rgba32()
                .ok_or_else(|| "a 32-bit float equalize requires f32 storage".to_string())
        };
        // The level of one component: `None` for a non-finite one.
        let level = |linear: f32, code: f32| -> Option<usize> {
            if !linear.is_finite() {
                None
            } else if linear > 1.0 {
                Some(65535)
            } else if linear < 0.0 {
                Some(0)
            } else {
                Some((f64::from(code.clamp(0.0, 1.0)) * 65535.0).round() as usize)
            }
        };
        let mut histogram = Histogram::new(65536)?;
        let mut i = 0usize;
        for run in runs()? {
            for px in run {
                job.every(i)?;
                let k = f64::from(weight(i)) / 65535.0 * f64::from(px[3]).clamp(0.0, 1.0);
                i += 1;
                if k <= 0.0 || !px[3].is_finite() {
                    continue;
                }
                let code = encoding.encode(*px);
                for c in 0..3 {
                    if let Some(level) = level(px[c], code[c]) {
                        histogram.add(level, k);
                    }
                }
            }
        }
        let Some(lut) = histogram.lut() else {
            return Ok(None);
        };
        let mut pixels = reserved_vec(w as usize * h as usize)?;
        let mut changed = false;
        let mut i = 0usize;
        for run in runs()? {
            for px in run {
                job.every(i)?;
                let t = write(i);
                i += 1;
                // A transparent pixel, or one whose α is not finite (the
                // histogram skipped it), is kept as it is.
                if px[3] == 0.0 || !px[3].is_finite() || t == 0 {
                    pixels.push(*px);
                    continue;
                }
                let code = encoding.encode(*px);
                let mut target = [px[0], px[1], px[2]];
                let mut moved = code;
                let mut changes = [false; 3];
                for c in 0..3 {
                    // Out of range and non-finite components are kept.
                    if !px[c].is_finite() || px[c] > 1.0 || px[c] < 0.0 {
                        continue;
                    }
                    let from = (f64::from(code[c].clamp(0.0, 1.0)) * 65535.0).round() as usize;
                    let to = lut[from];
                    if usize::from(to) != from {
                        moved[c] = f32::from(to) / 65535.0;
                        changes[c] = true;
                    }
                }
                if changes.contains(&true) {
                    let decoded = encoding.decode(moved);
                    for c in 0..3 {
                        if changes[c] {
                            target[c] = decoded[c];
                        }
                    }
                }
                let after = mix_float(*px, target, t);
                changed |= after.map(f32::to_bits) != px.map(f32::to_bits);
                pixels.push(after);
            }
        }
        job.check()?;
        if !changed {
            return Ok(None);
        }
        image
            .rebuilt_rgba32_extended(w, h, pixels, self.profile().clone())
            .map(Some)
    }

    /// The CMYK arm (module doc).
    fn equalized_cmyk(
        &self,
        image: &CmykImage,
        offset: (i32, i32),
        coverage: &[u16],
        gate: &[u16],
        job: JobContext<'_>,
    ) -> Result<Option<CmykImage>, String> {
        let bits = self.require_cmyk_policy("Equalize")?.depth.bits();
        let eight = bits <= 8;
        let canvas = self.dimensions();
        let (w, h) = image.dimensions();
        let level = |ink: u16| -> usize {
            let l = u32::from(65535 - ink);
            (if eight { (l + 128) / 257 } else { l }) as usize
        };
        let mut histogram = Histogram::new(if eight { 256 } else { 65536 })?;
        for y in 0..h {
            job.check()?;
            let mut x = 0usize;
            for (_, run) in image.row_segments(y, 0, w) {
                for s in run {
                    let i = y as usize * w as usize + x;
                    x += 1;
                    let k = f64::from(coverage_at(coverage, canvas, (w, h), offset, i)) / 65535.0
                        * f64::from(s[4])
                        / 65535.0;
                    if k <= 0.0 {
                        continue;
                    }
                    for &ink in &s[..4] {
                        histogram.add(level(ink), k);
                    }
                }
            }
        }
        let Some(lut) = histogram.lut() else {
            return Ok(None);
        };
        let scale: u32 = if eight { 257 } else { 1 };
        let mut changed = false;
        let out = CmykImage::from_rows_with_job(
            w,
            h,
            |y, row| {
                let mut x = 0usize;
                for (_, run) in image.row_segments(y, 0, w) {
                    for s in run {
                        let i = y as usize * w as usize + x;
                        let t = coverage_at(gate, canvas, (w, h), offset, i);
                        row[x] = if s[4] == 0 || t == 0 {
                            *s
                        } else {
                            let mut next = *s;
                            for c in 0..4 {
                                let l = u32::from(lut[level(s[c])]) * scale;
                                let ink = 65535 - l.min(65535) as u16;
                                next[c] = if t == u16::MAX {
                                    ink
                                } else {
                                    let k = f32::from(t) / 65535.0;
                                    code(unit(s[c]) + (unit(ink) - unit(s[c])) * k)
                                };
                            }
                            next
                        };
                        changed |= row[x] != *s;
                        x += 1;
                    }
                }
                job.every(y as usize * w as usize)
            },
            job.range(0.5, 1.0),
        )?;
        Ok(changed.then_some(out))
    }
}

/// The 16-bit RGB arm (module doc).
fn equalized_rgba16(
    doc: &PrecisionDocument,
    image: &crate::precision_image::PrecisionImage,
    weight: impl Fn(usize) -> u16,
    write: impl Fn(usize) -> u16,
    job: JobContext<'_>,
) -> Result<Option<crate::precision_image::PrecisionImage>, String> {
    let (w, h) = image.dimensions();
    let runs = || {
        image
            .runs_rgba16()
            .ok_or_else(|| "selected edit requires u16 source".to_string())
    };
    // A Lab Color document equalizes Lightness alone (module doc).
    let channels = if doc.is_lab() { 1 } else { 3 };
    let mut histogram = Histogram::new(65536)?;
    let mut i = 0usize;
    for run in runs()? {
        for px in run {
            job.every(i)?;
            let k = f64::from(weight(i)) / 65535.0 * f64::from(px[3]) / 65535.0;
            i += 1;
            if k <= 0.0 {
                continue;
            }
            for &v in &px[..channels] {
                histogram.add(usize::from(v), k);
            }
        }
    }
    let Some(lut) = histogram.lut() else {
        return Ok(None);
    };
    let mut pixels = reserved_vec(w as usize * h as usize)?;
    let mut changed = false;
    let mut i = 0usize;
    for run in runs()? {
        for px in run {
            job.every(i)?;
            let t = write(i);
            i += 1;
            let after = if px[3] == 0 || t == 0 {
                *px
            } else {
                let mapped: [f32; 4] = [
                    f32::from(lut[usize::from(px[0])]) / 65535.0,
                    f32::from(lut[usize::from(px[1])]) / 65535.0,
                    f32::from(lut[usize::from(px[2])]) / 65535.0,
                    f32::from(px[3]) / 65535.0,
                ];
                let mut mixed = mix_codes(*px, mapped, t)?;
                if channels == 1 {
                    mixed[1] = px[1];
                    mixed[2] = px[2];
                }
                mixed
            };
            changed |= after != *px;
            pixels.push(after);
        }
    }
    job.check()?;
    if !changed {
        return Ok(None);
    }
    image
        .rebuilt_rgba16(w, h, pixels, doc.profile().clone())
        .map(Some)
}
