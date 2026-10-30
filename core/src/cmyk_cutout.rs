//! Select and Mask's New Layer outputs on a CMYK document (#1): the CMYK
//! twin of `precision_cutout`'s assembly, `dr_precision_doc_add_cmyk_cutout`.
//!
//! The new top root "Refined Cutout" is a CMYK raster of the visible
//! composite's straight ink (`cmyk_composite_image`: ink and alpha kept, no
//! premultiplication) at the canvas origin, with the u16 matte as its layer
//! mask; every prior root is hidden and kept whole, as `doc_cutout.rs` and
//! `precision_cutout` hide them. New Layer (no mask) is the host's next step:
//! the CMYK Apply Layer Mask bakes the mask into the cutout's alpha.
//!
//! **Decontaminate Colors on ink.** `decontaminate` is 0 (off) or a strength
//! in (0, 1]. When on, `foreground_solve::solve` (the kernel the RGB adapter
//! `precision_foreground` runs) solves the composite's foreground twice with
//! the matte as alpha: C, M and Y as its three channels, then K (in all
//! three). The solver's channels are independent (each one's update reads only
//! that channel and the shared alpha weights), so the K pass is the RGB
//! estimate of a grey whose three channels are K. The RGB adapter's exactness
//! rules hold: ink at matte 0 and 65535 and all alpha are exact; a binary
//! matte leaves the ink as it is; a constant fractional matte cannot separate
//! two inks and refuses, and so does a composite that is not opaque; each
//! changed ink is `original + strength·(estimate − original)`, rounded.
//!
//! The plan is the RGB plan's keys with `foreground_storage`
//! `"cmyka16_unorm"`; its scratch is the RGB cutout's (the matte, eight
//! bookkeeping allowances and 4096 bytes a record) plus the composite raster
//! it builds (`n·10`, the stack's masks and one ink row) and
//! `n·cmyk_paint::CANVAS_BYTES_PER_PIXEL`, the
//! canvas term every CMYK pixel edit charges for the host's display
//! preparation of its result; and, when decontaminating, the solver's bound
//! (`precision_foreground::FOREGROUND_BYTES_PER_PIXEL`, `n·136`) and the
//! contiguous ink copy the estimate is written into (`n·10`).
//!
//! `scripts/cmyk-admission`'s `cutout-16` (Release, 24 MP, `refine_selection`
//! feather 8 to New Layer with Mask, the whole MCP call) read max E
//! 793,674,304 against the first plan without the canvas term (296,512,704,
//! a miss), so the canvas term was added.

use serde_json::{json, Value};

use crate::cmyk_image::{CmykImage, CMYK_BYTES_PER_PIXEL};
use crate::doc_mask::MaskProperties;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::precision_mask::{PrecisionMask, PrecisionPlane};
use crate::precision_render::MAX_RECORDS;
use crate::resources::Inventory;

fn strength(decontaminate: f32) -> Result<f32, String> {
    if !decontaminate.is_finite() || !(0.0..=1.0).contains(&decontaminate) {
        return Err("decontaminate must be 0 (off) or a strength in (0, 1]".into());
    }
    Ok(decontaminate)
}

impl PrecisionDocument {
    /// The CMYK cutout's plan (module doc).
    pub fn cmyk_cutout_plan(&self, decontaminate: f32) -> Result<Value, String> {
        self.require_cmyk_policy("a CMYK cutout")?;
        let strength = strength(decontaminate)?;
        if self.layer_count() >= MAX_RECORDS {
            return Err("native refinement output exceeds the document record limit".into());
        }
        let (mut rasters, mut masks) = self.native_dimensions();
        rasters.push(self.dimensions());
        masks.push(self.dimensions());
        self.native_fits_geometry(self.dimensions(), &rasters, &masks, self.channels.len())?;
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        let n = pixel_count(self.width, self.height)? as u64;
        let matte_bytes = n * 2;
        let mut scratch = matte_bytes
            + 8 * BOOKKEEPING_BYTES
            + self.layer_count() as u64 * 4096
            + n * CMYK_BYTES_PER_PIXEL
            + n * crate::cmyk_paint::CANVAS_BYTES_PER_PIXEL
            + self.cmyk_stack_scratch_bytes()?
            + u64::from(self.width) * 20;
        if strength > 0.0 {
            scratch += n * crate::precision_foreground::FOREGROUND_BYTES_PER_PIXEL
                + n * CMYK_BYTES_PER_PIXEL;
        }
        Ok(
            json!({"schema_version":1,"retained_bytes":inventory.unique_bytes(),"scratch_bytes":scratch,
            "borrowed_matte_bytes":matte_bytes,"dimensions":[self.width,self.height],"new_layer_index":self.layer_count(),
            "foreground_storage":crate::cmyk_document::STORAGE_CMYK,"matte_storage":"u16_unorm",
            "alpha_contract":"composite ink and alpha unchanged at CMYKA16 (decontaminated ink at fractional matte when asked); separate refinement matte; no premultiplication",
            "advanced_controls_enabled":false,"decontaminate":strength}),
        )
    }

    /// Adds the cutout (module doc). `Ok(None)`: no pixel has both composite
    /// alpha and matte coverage. `Err`: the plan's refusals, a matte of
    /// another length, the decontamination refusals, admission, a cancelled
    /// job.
    pub fn add_cmyk_cutout(
        &self,
        matte: &[u16],
        decontaminate: f32,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let plan = self.cmyk_cutout_plan(decontaminate)?;
        let n = self.width as usize * self.height as usize;
        if matte.len() != n {
            return Err("native refinement matte must match the canvas".into());
        }
        let number = |key: &str| plan[key].as_u64().ok_or("invalid CMYK cutout plan");
        let (retained, scratch) = (number("retained_bytes")?, number("scratch_bytes")?);
        let other = other
            .checked_add(number("borrowed_matte_bytes")?)
            .ok_or("native refinement memory overflow")?;
        admit(retained, scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        let image = if decontaminate > 0.0 {
            self.decontaminated_composite(matte, decontaminate, job.range(0., 0.8))?
        } else {
            self.cmyk_composite_image(job.range(0., 0.8))?
        };
        let mut nonempty = false;
        for (y, line) in matte.chunks_exact(self.width as usize).enumerate() {
            job.check()?;
            for (x, run) in image.row_segments(y as u32, 0, self.width) {
                let coverage = &line[x as usize..x as usize + run.len()];
                nonempty |= run.iter().zip(coverage).any(|(p, &c)| p[4] != 0 && c != 0);
            }
            if nonempty {
                break;
            }
        }
        if !nonempty {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        let plane = PrecisionPlane::copy(self.width, self.height, matte, job.range(0.8, 0.9))?;
        let mask = PrecisionMask::new(plane, MaskProperties::at((0, 0)), true)?;
        let cutout =
            PrecisionDocumentLayer::raster_cmyk("Refined Cutout", image, (0, 0), 1.0, true)?
                .with_mask(Some(mask));
        let mut layers = Vec::with_capacity(self.layers.len() + 1);
        for layer in self.layers.iter() {
            job.check()?;
            let mut hidden = layer.clone();
            hidden.visible = false;
            layers.push(hidden);
        }
        layers.push(cutout);
        let result = self.rebuilt(
            self.width,
            self.height,
            layers,
            self.channels.as_ref().clone(),
        )?;
        job.report(1.0, 5)?;
        Ok(Some(result))
    }

    /// The composite with its ink decontaminated under `matte` (module doc).
    fn decontaminated_composite(
        &self,
        matte: &[u16],
        strength: f32,
        job: JobContext<'_>,
    ) -> Result<CmykImage, String> {
        let (width, height) = self.dimensions();
        let mut pixels = self.cmyk_ink_canvas(None, job.range(0., 0.1))?;
        let mut fractional = false;
        let (mut minimum, mut maximum) = (u16::MAX, 0);
        for (i, (&a, p)) in matte.iter().zip(&pixels).enumerate() {
            job.every(i)?;
            if p[4] != u16::MAX {
                return Err("foreground estimation requires an opaque source composite".into());
            }
            fractional |= a > 0 && a < u16::MAX;
            minimum = minimum.min(a);
            maximum = maximum.max(a);
        }
        if fractional {
            if minimum == maximum {
                return Err(
                    "constant fractional alpha cannot identify foreground and background colors"
                        .into(),
                );
            }
            // C, M and Y, then K: the solver's three channels (module doc).
            for (pass, plates) in [(0.1, [0usize, 1, 2]), (0.45, [3, 3, 3])] {
                let mut input: Vec<[f32; 4]> = reserved_vec(pixels.len())?;
                for (p, &a) in pixels.iter().zip(matte) {
                    input.push([
                        f32::from(p[plates[0]]) / 65535.0,
                        f32::from(p[plates[1]]) / 65535.0,
                        f32::from(p[plates[2]]) / 65535.0,
                        f32::from(a) / 65535.0,
                    ]);
                }
                let estimate = crate::foreground_solve::solve(
                    input,
                    (width, height),
                    job.range(pass, pass + 0.35),
                )?;
                let channels = if plates[0] == 3 { 1 } else { 3 };
                for (i, ((p, &a), colors)) in
                    pixels.iter_mut().zip(matte).zip(&estimate).enumerate()
                {
                    job.every(i)?;
                    if a == 0 || a == u16::MAX {
                        continue;
                    }
                    for (c, &plate) in plates.iter().enumerate().take(channels) {
                        let original = f32::from(p[plate]) / 65535.0;
                        let value = original + strength * (colors[c] - original);
                        p[plate] = (f64::from(value).clamp(0.0, 1.0) * 65535.0).round() as u16;
                    }
                }
            }
        }
        CmykImage::from_rows_with_job(
            width,
            height,
            |y, row| {
                let at = y as usize * width as usize;
                row.copy_from_slice(&pixels[at..at + width as usize]);
                Ok(())
            },
            job,
        )
    }
}
