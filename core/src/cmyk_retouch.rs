//! The retouch commands on a CMYK document's raster records (#1): the
//! Clone Stamp, the Healing Brush (and Patch and Content-Aware Move's heal,
//! which are its requests), Spot Healing (and Remove) and Content-Aware Fill
//! — `precision_retouch`'s `RetouchEdit`, routed here at the top of its plan,
//! its walk and `retouch_from`'s refusal. Red Eye refuses by name.
//!
//! * **The sampling plate** (`cmyk_retouch_plate`): the source layer's own
//!   ink on paper (Current), or the CMYK composite with the records
//!   `retouch_sampling` excludes at opacity 0 (Current & Below, All).
//! * **Clone** (two passes): a Normal clone at opacity 1 onto a BLANK pass
//!   of the destination, in both passes, from the plate's passes, gives the
//!   source ink S with its coverage as alpha. S is composited onto the
//!   destination per pixel through `cmyk_composite::over_mode` (or
//!   `over_mode_frozen` under a transparency lock) at the stroke opacity —
//!   every mode of `CMYK_BLEND_MODES`.
//! * **Heal** (two passes): the RGB walk in place on the destination's
//!   passes reading the plate's passes (`sample` current, layer 0), joined
//!   by `cmyk_passes`.
//! * **Spot Heal and Content-Aware Fill** (native four-ink synthesis): the
//!   shared PatchMatch search, vote and membrane at C = 4 on the ink codes
//!   (`precision_inpaint::apply::<_, 4>` over a `SynthesisPlate::Cmyk`);
//!   alpha follows the RGB writeback.
//! * **Output**: `output:"new_layer"` writes onto blank paper (clone) or the
//!   Current & Below plate (the heal family), then the RGB cleanup rule —
//!   coverage below 32768 (1 for Content-Aware Fill), or an unchanged
//!   pixel, becomes transparent paper — and inserts a CMYK raster above the
//!   target under the RGB arm's name. `expand_destination` pads the target
//!   with paper at α 0 to `retouch_transform::expanded_extent` first.
//!
//! The plan restates the RGB arm's on ink: the plate (canvas × 10, plus the
//! composite's figure in Current & Below and All), the host's display
//! preparation (canvas × `DISPLAY_BYTES_PER_PIXEL`) and bookkeeping; a
//! two-pass op adds the RGB plan of one pass twice (each already charges its
//! stroke planes and destination), the destination's passes (px × 34) and
//! the plate's (canvas × 16); synthesis adds the stroke planes (`n × 40`, or
//! 44 with a selection), the destination at 20 B a pixel and
//! `precision_inpaint::estimate` at four channels. Measured by
//! `scripts/cmyk-admission/measure.py` (`retouch-clone-16`,
//! `retouch-heal-16`, `spot-heal-16`).

use crate::blend::BlendMode;
use crate::cmyk_adjust_layer::DISPLAY_BYTES_PER_PIXEL;
use crate::cmyk_composite::{codes, over_mode, over_mode_frozen, unit, InkPixel};
use crate::cmyk_document::{blend_refusal, refused_sentence};
use crate::cmyk_image::{CmykImage, PAPER};
use crate::cmyk_passes::{
    join_images, pass_document, pass_image, pass_result, InkPass, BLANK_PASS,
};
use crate::doc_lock::LOCK_TRANSPARENCY;
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::layer_rect::{canvas_rect_on_plane, plane_rect_offset};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{pixel_count, PrecisionImage, BOOKKEEPING_BYTES};
use crate::precision_paint::PaintPlan;
use crate::precision_retouch::{RetouchEdit, RetouchOp, RetouchOutput};
use crate::retouch_sampling::RetouchSampling;
use crate::retouch_transform::StorageExtent;

/// `edit` as a pass runs it (module doc): layer 0 in place, the plate's
/// pass document sampled as Current; a clone at opacity 1, Normal.
fn pass_edit(edit: &RetouchEdit) -> RetouchEdit {
    let mut pass = edit.clone();
    pass.layer = 0;
    pass.source_layer = None;
    pass.sample = "current".into();
    pass.ignore_adjustments = false;
    pass.output = RetouchOutput::Layer;
    pass.expand_destination = false;
    if edit.op == RetouchOp::RetouchClone {
        pass.opacity = 1.0;
        pass.blend_mode = 0;
    }
    pass
}

impl PrecisionDocument {
    /// CMYK raster `index`'s image and offset, in `precision_retouch`'s
    /// sentences.
    fn cmyk_retouch_target(&self, index: usize) -> Result<(&CmykImage, (i32, i32)), String> {
        let target = self
            .layer(index)
            .ok_or("retouch target layer is out of range")?;
        let image = target
            .cmyk_source()
            .ok_or("retouch requires a raster target")?;
        Ok((image, target.offset()))
    }

    /// The destination's storage extent (`RetouchEdit::extent` on ink).
    fn cmyk_retouch_extent(&self, edit: &RetouchEdit) -> Result<StorageExtent, String> {
        let (image, offset) = self.cmyk_retouch_target(edit.layer)?;
        if edit.output == RetouchOutput::NewLayer {
            return Ok((self.dimensions(), (0, 0)));
        }
        if edit.expand_destination {
            crate::retouch_transform::expanded_extent(self.dimensions(), image.dimensions(), offset)
        } else {
            Ok((image.dimensions(), offset))
        }
    }

    /// `retouch_plan` on a CMYK document (module doc).
    pub(crate) fn cmyk_retouch_plan(&self, edit: &RetouchEdit) -> Result<PaintPlan, String> {
        self.require_cmyk_policy("A CMYK retouch")?;
        if edit.op == RetouchOp::RedEye {
            return Err(refused_sentence("Red Eye"));
        }
        // Content-Aware Fill Transparent Areas' pad (phase 2b-1) is
        // Auto-Blend's, which does not run in CMYK.
        if edit.fill_transparent {
            return Err("fill_transparent is not available in a CMYK document".into());
        }
        let mode = edit.mode()?;
        if !edit.opacity.is_finite() || !(0.0..=1.0).contains(&edit.opacity) || edit.ring > 512 {
            return Err("retouch opacity must be normalized; ring must be 0 through 512".into());
        }
        if edit.source_layer.is_some() && mode != RetouchSampling::Current {
            return Err("source_layer requires sample=current".into());
        }
        let blend =
            BlendMode::from_c(edit.blend_mode as i32).ok_or("retouch blend mode is invalid")?;
        if let Some(refusal) = blend_refusal(blend) {
            return Err(refusal);
        }
        if edit.blend_mode != 0 && edit.output == RetouchOutput::NewLayer {
            return Err(
                "a clone onto a new layer has nothing to blend with; blend_mode requires output layer"
                    .into(),
            );
        }
        crate::retouch_transform::validate(&edit.matrix)?;
        self.cmyk_retouch_target(edit.layer)?;
        let source = edit.source_layer.unwrap_or(edit.layer);
        if self.layer(source).and_then(|l| l.cmyk_source()).is_none() {
            return Err("retouch requires a raster source layer".into());
        }
        let (dimensions, offset) = self.cmyk_retouch_extent(edit)?;
        let (cw, ch) = self.dimensions();
        let n = pixel_count(cw, ch)? as u64;
        let output = pixel_count(dimensions.0, dimensions.1)? as u64;
        // The stroke planes and destination (`n × 40/44 + output × 20`) are
        // charged only by the synthesis arm below: each two-pass RGB plan
        // already charges its own (`precision_retouch`'s buffers term), so
        // charging them here too counted them three times.
        let mut scratch =
            n * DISPLAY_BYTES_PER_PIXEL + 8 * BOOKKEEPING_BYTES + self.cmyk_plate_scratch(mode)?;
        if edit.output == RetouchOutput::NewLayer {
            self.retouch_output_refusal(edit.layer)?;
            self.preflight_raster_addition(self.dimensions(), edit.output_name())?;
            scratch += self.cmyk_plate_scratch(RetouchSampling::CurrentAndBelow)?;
        } else {
            let (mut rasters, masks) = self.native_dimensions();
            let raster = (0..edit.layer)
                .filter(|&i| self.layer(i).and_then(|l| l.cmyk_source()).is_some())
                .count();
            if let Some(slot) = rasters.get_mut(raster) {
                *slot = dimensions;
            }
            self.native_fits_geometry(self.dimensions(), &rasters, &masks, self.channels.len())?;
        }
        match edit.op {
            RetouchOp::RetouchClone | RetouchOp::RetouchHeal => {
                // One pass's RGB plan on a stand-in of the destination.
                let image = PrecisionImage::filled_rgba16(
                    dimensions.0,
                    dimensions.1,
                    BLANK_PASS,
                    IccProfile::srgb(),
                )?;
                let stand_in = pass_document(self.dimensions(), image, offset, 0)?;
                let rgb = stand_in.retouch_plan(&pass_edit(edit))?.scratch_bytes;
                scratch += 2 * rgb + output * 34 + n * 16;
            }
            RetouchOp::SpotHeal | RetouchOp::ContentAwareFill => {
                let bounds = edit.bounds(self)?;
                scratch += n * (if edit.selection { 44 } else { 40 }) + output * 20;
                scratch += crate::precision_inpaint::estimate(
                    self.dimensions(),
                    bounds,
                    u64::from(bounds.2) * u64::from(bounds.3),
                    edit.ring,
                    synthesis_caller(edit),
                    edit.spot_type,
                    4,
                )?
                .0;
            }
            RetouchOp::RedEye => {}
        }
        if let Some(mask) = &edit.source_mask {
            mask.validate(self.width, self.height)?;
        }
        Ok(PaintPlan {
            retained_bytes: self.retained_bytes(),
            scratch_bytes: scratch,
            input_samples: n as usize * if edit.selection { 2 } else { 1 },
        })
    }

    /// `retouch_from`'s refusals on a CMYK document: its own, with the
    /// plates a CMYK document of this canvas holding a CMYK raster at the
    /// sampled index.
    pub(crate) fn cmyk_retouch_from_refusal(
        &self,
        plates: &PrecisionDocument,
        edit: &RetouchEdit,
    ) -> Result<(), String> {
        if !matches!(edit.op, RetouchOp::RetouchClone | RetouchOp::RetouchHeal) {
            return Err(
                "retouch from another document's plates requires retouch_clone or retouch_heal"
                    .into(),
            );
        }
        if edit.output != RetouchOutput::Layer || edit.expand_destination {
            return Err(
                "retouch from another document's plates writes its target layer in place: output must be layer and expand_destination false"
                    .into(),
            );
        }
        if plates.dimensions() != self.dimensions() {
            return Err("retouch plates must have the document's canvas size".into());
        }
        if !plates.is_cmyk() {
            return Err("retouch plates must have the document's bit depth".into());
        }
        if plates
            .layer(edit.source_layer.unwrap_or(edit.layer))
            .and_then(|l| l.cmyk_source())
            .is_none()
        {
            return Err("retouch plates have no raster layer at the sampled index".into());
        }
        Ok(())
    }

    /// `retouching_unlocked`'s CMYK arm (module doc), after the shared
    /// prelude: `coverage` is the stroke within the selection domain,
    /// `selection` the write gate, `scratch` the admitted plan's.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cmyk_retouching(
        &self,
        plates: &PrecisionDocument,
        edit: &RetouchEdit,
        coverage: &[u16],
        selection: Option<&[u16]>,
        scratch: u64,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let (image, target_offset) = self.cmyk_retouch_target(edit.layer)?;
        let (dimensions, offset) = self.cmyk_retouch_extent(edit)?;
        let (cw, ch) = self.dimensions();
        let frozen = edit.output == RetouchOutput::Layer
            && self.lock_owner(edit.layer, LOCK_TRANSPARENCY).is_some();
        let dest = match edit.output {
            RetouchOutput::NewLayer if edit.op == RetouchOp::RetouchClone => {
                CmykImage::filled(cw, ch, PAPER)?
            }
            RetouchOutput::NewLayer => self.cmyk_retouch_plate(
                edit.layer,
                RetouchSampling::CurrentAndBelow,
                false,
                job.range(0.0, 0.08),
            )?,
            RetouchOutput::Layer if (dimensions, offset) == (image.dimensions(), target_offset) => {
                image.clone()
            }
            RetouchOutput::Layer => padded(image, target_offset, (dimensions, offset), job)?,
        };
        let plate = plates.cmyk_retouch_plate(
            edit.source_layer.unwrap_or(edit.layer),
            edit.mode()?,
            edit.ignore_adjustments,
            job.range(0.08, 0.2),
        )?;
        let io = (coverage, selection);
        let stage = job.range(0.2, 0.95);
        let result = match edit.op {
            RetouchOp::RetouchClone => {
                self.cmyk_cloned(edit, &dest, offset, &plate, io, frozen, limit, other, stage)?
            }
            RetouchOp::RetouchHeal => {
                self.cmyk_healed(edit, &dest, offset, &plate, io, frozen, limit, other, stage)?
            }
            RetouchOp::SpotHeal | RetouchOp::ContentAwareFill => {
                self.cmyk_synthesized(edit, &dest, offset, &plate, io, scratch, stage)?
            }
            RetouchOp::RedEye => return Err(refused_sentence("Red Eye")),
        };
        let Some(out) = result else {
            job.report(1.0, 5)?;
            return Ok(None);
        };
        let next = if edit.output == RetouchOutput::NewLayer {
            let threshold = if edit.op == RetouchOp::ContentAwareFill {
                1
            } else {
                32768
            };
            let cleaned = CmykImage::from_rows_with_job(
                cw,
                ch,
                |y, row| {
                    let mut x = 0usize;
                    for ((_, run), (_, base)) in
                        out.row_segments(y, 0, cw).zip(dest.row_segments(y, 0, cw))
                    {
                        for (s, b) in run.iter().zip(base) {
                            let i = y as usize * cw as usize + x;
                            row[x] = if coverage[i] < threshold || s == b {
                                PAPER
                            } else {
                                *s
                            };
                            x += 1;
                        }
                    }
                    job.check()
                },
                job.range(0.95, 0.98),
            )?;
            let clipped = self.layer(edit.layer).is_some_and(|l| l.clipped());
            let leaf =
                PrecisionDocumentLayer::raster_cmyk(edit.output_name(), cleaned, (0, 0), 1., true)?
                    .with_clipped(clipped);
            Some(self.retouch_insert_leaf(edit.layer, leaf)?)
        } else {
            let mut next =
                self.replacing_content(edit.layer, Content::CmykRaster { image: out, offset })?;
            if let Some(next) = next.as_mut() {
                if let Some(comps) = self.comps_after_growth(edit.layer, offset) {
                    next.comps = comps;
                }
            }
            next
        };
        job.report(1.0, 5)?;
        Ok(next)
    }

    /// The pass input: the domain coverage, then the selection plane.
    fn cmyk_pass_input(coverage: &[u16], selection: Option<&[u16]>) -> Vec<u16> {
        let mut input = coverage.to_vec();
        if let Some(selection) = selection {
            input.extend_from_slice(selection);
        }
        input
    }

    /// Runs `edit`'s RGB walk on `dest`'s two passes (blank for a clone)
    /// over the plate's passes; each result's layer-0 image.
    #[allow(clippy::too_many_arguments)]
    fn cmyk_retouch_passes(
        &self,
        edit: &RetouchEdit,
        dest: &CmykImage,
        offset: (i32, i32),
        plate: &CmykImage,
        (coverage, selection): (&[u16], Option<&[u16]>),
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<[PrecisionImage; 2]>, String> {
        let blank = edit.op == RetouchOp::RetouchClone;
        let pass = pass_edit(edit);
        let input = Self::cmyk_pass_input(coverage, selection);
        let held = self.retained_bytes().saturating_add(other);
        let mut out = Vec::with_capacity(2);
        let mut changed = false;
        for (k, which) in [InkPass::Cmy, InkPass::K].into_iter().enumerate() {
            let range = job.range(k as f64 * 0.45, (k + 1) as f64 * 0.45);
            let doc = pass_document(
                self.dimensions(),
                pass_image(dest, which, blank, range)?,
                offset,
                0,
            )?;
            let plates = Self::cmyk_plate_document(plate, which)?;
            let result = doc.retouching_unlocked(&plates, &pass, &input, limit, held, range)?;
            changed |= result.is_some();
            out.push(pass_result(result.as_ref().unwrap_or(&doc))?.clone());
        }
        if !changed {
            return Ok(None);
        }
        let b = out.pop().ok_or("internal error: a CMYK pass is missing")?;
        let a = out.pop().ok_or("internal error: a CMYK pass is missing")?;
        Ok(Some([a, b]))
    }

    /// The Clone Stamp on ink (module doc).
    #[allow(clippy::too_many_arguments)]
    fn cmyk_cloned(
        &self,
        edit: &RetouchEdit,
        dest: &CmykImage,
        offset: (i32, i32),
        plate: &CmykImage,
        io: (&[u16], Option<&[u16]>),
        frozen: bool,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<CmykImage>, String> {
        let Some([a, b]) = self.cmyk_retouch_passes(
            edit,
            dest,
            offset,
            plate,
            io,
            limit,
            other,
            job.range(0.0, 0.9),
        )?
        else {
            return Ok(None);
        };
        let mode =
            BlendMode::from_c(edit.blend_mode as i32).ok_or("retouch blend mode is invalid")?;
        let (w, h) = dest.dimensions();
        let mut changed = false;
        let out = CmykImage::from_rows_with_job(
            w,
            h,
            |y, row| {
                let mut x = 0u32;
                for (_, run) in dest.row_segments(y, 0, w) {
                    for s in run {
                        let pa = a.rgba16_at(x, y).unwrap_or(BLANK_PASS);
                        let pb = b.rgba16_at(x, y).unwrap_or(BLANK_PASS);
                        if pa[3] != pb[3] {
                            return Err("internal error: the CMYK passes disagree on alpha".into());
                        }
                        let mut next = *s;
                        if pa[3] != 0 {
                            let ink = [
                                unit(65535 - pa[0]),
                                unit(65535 - pa[1]),
                                unit(65535 - pa[2]),
                                unit(65535 - pb[1]),
                            ];
                            let sa = unit(pa[3]) * edit.opacity;
                            let mut pixel: InkPixel = s.map(unit);
                            let xy = (
                                i64::from(x) + i64::from(offset.0),
                                i64::from(y) + i64::from(offset.1),
                            );
                            if frozen {
                                over_mode_frozen(&mut pixel, ink, sa, mode, xy);
                            } else {
                                over_mode(&mut pixel, ink, sa, mode, xy);
                            }
                            next = codes(pixel);
                            // Sub-quantum alpha cannot create invisible storage.
                            if *s == PAPER && next[4] == 0 {
                                next = *s;
                            }
                        }
                        changed |= next != *s;
                        row[x as usize] = next;
                        x += 1;
                    }
                }
                job.every(y as usize * w as usize)
            },
            job.range(0.9, 1.0),
        )?;
        Ok(changed.then_some(out))
    }

    /// The Healing Brush on ink (module doc).
    #[allow(clippy::too_many_arguments)]
    fn cmyk_healed(
        &self,
        edit: &RetouchEdit,
        dest: &CmykImage,
        offset: (i32, i32),
        plate: &CmykImage,
        io: (&[u16], Option<&[u16]>),
        frozen: bool,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<CmykImage>, String> {
        let Some([a, b]) = self.cmyk_retouch_passes(
            edit,
            dest,
            offset,
            plate,
            io,
            limit,
            other,
            job.range(0.0, 0.9),
        )?
        else {
            return Ok(None);
        };
        join_images(dest, &a, Some(&b), frozen, job.range(0.9, 1.0))
    }

    /// Spot Healing and Content-Aware Fill on the four inks (module doc).
    #[allow(clippy::too_many_arguments)]
    fn cmyk_synthesized(
        &self,
        edit: &RetouchEdit,
        dest: &CmykImage,
        offset: (i32, i32),
        plate: &CmykImage,
        (coverage, selection): (&[u16], Option<&[u16]>),
        scratch: u64,
        job: JobContext<'_>,
    ) -> Result<Option<CmykImage>, String> {
        let bounds = edit.bounds(self)?;
        let estimate = crate::precision_inpaint::estimate(
            self.dimensions(),
            bounds,
            u64::from(bounds.2) * u64::from(bounds.3),
            edit.ring,
            synthesis_caller(edit),
            edit.spot_type,
            4,
        )?
        .0;
        let synthesis = crate::precision_inpaint::plan(
            self.dimensions(),
            coverage,
            edit.ring,
            edit.op == RetouchOp::ContentAwareFill,
            edit.spot_type,
            scratch.saturating_sub(estimate),
            4,
            job,
        )?;
        let eligibility = edit
            .source_mask
            .as_ref()
            .map(|m| m.compile(self.width, self.height, job))
            .transpose()?;
        let canvas = self.dimensions();
        let rect = synthesis
            .window_union()
            .and_then(|b| canvas_rect_on_plane(canvas, dest.dimensions(), offset, b));
        let (mut pixels, plane) = match rect {
            Some(rect) => (
                dest.read_rect(rect, job)?,
                ((rect.2, rect.3), plane_rect_offset(offset, rect)),
            ),
            None => (Vec::new(), ((0, 0), offset)),
        };
        crate::precision_inpaint::apply::<u16, 4>(
            &synthesis,
            crate::precision_inpaint::SynthesisPlate::Cmyk(plate),
            coverage,
            eligibility.as_deref(),
            edit.opacity,
            edit.seed,
            pixels.as_flattened_mut(),
            plane.0,
            plane.1,
            selection,
            job,
        )?;
        match rect {
            Some(rect) => dest.with_rect(rect, &pixels, job),
            None => Ok(None),
        }
    }
}

/// The synthesis caller of a Spot Heal or Content-Aware Fill request.
fn synthesis_caller(edit: &RetouchEdit) -> crate::doc_inpaint::Caller {
    if edit.op == RetouchOp::ContentAwareFill {
        crate::doc_inpaint::Caller::Fill
    } else {
        crate::doc_inpaint::Caller::SpotHeal
    }
}

/// `image` (at `from`) on paper at α 0 over the grown extent `to`.
fn padded(
    image: &CmykImage,
    from: (i32, i32),
    ((w, h), to): StorageExtent,
    job: JobContext<'_>,
) -> Result<CmykImage, String> {
    let (iw, ih) = image.dimensions();
    let dx = i64::from(from.0) - i64::from(to.0);
    let dy = i64::from(from.1) - i64::from(to.1);
    CmykImage::from_rows_with_job(
        w,
        h,
        |y, row| {
            row.fill(PAPER);
            let ly = i64::from(y) - dy;
            if (0..i64::from(ih)).contains(&ly) {
                for (lx, run) in image.row_segments(ly as u32, 0, iw) {
                    let at = (i64::from(lx) + dx) as usize;
                    row[at..at + run.len()].copy_from_slice(run);
                }
            }
            job.check()
        },
        job,
    )
}
