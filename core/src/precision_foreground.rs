//! The 16-bit adapter of foreground-colour estimation (Select and Mask's
//! Decontaminate Colors) over the `foreground_solve` kernel the byte
//! `foreground_estimate` shares, with the byte rules at u16 codes: the source
//! is a canvas-sized RGBA16 composite in the document's profile, opaque
//! everywhere; the matte is supplied and never applied; RGB at matte 0 and
//! 65535 and all source alpha stay exact; strength 0, a binary matte and an
//! identical result are the no-op (`None`); a constant fractional matte
//! cannot separate two colours and refuses.
//!
//! Only the encoded compositing domain is offered at this depth: a `"linear"`
//! estimate is refused by name. A 32-bit float document refuses first, by
//! name (`float_document::decontaminate_refusal`): the solve reads and writes
//! RGBA16 codes clamped to 0…1, so a fringe above white would be clipped.
//! The edge stage itself runs at float (`display_encoding`, rule E).
//! A Lab Color document (#547 phase 6b-ii) runs the same encoded per-channel
//! solve on its L\*a\*b\* codes, the linear mix Normal compositing uses.
use crate::foreground_solve::solve;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::selection_tools::{number, object};
use serde_json::{json, Value};

/// The estimate's plan term per canvas pixel: the solver's bound (128 B — the
/// input pyramid at most 32, the live old and new estimates and the separable
/// resize intermediate at most 60, with the rounding of odd levels and
/// allocator staging, as the byte adapter reserves) and the u16 copy of the
/// source the result is written into (8 B).
///
/// Record ([`crate::edge_refine::BYTES_PER_PIXEL`]'s harness, document and
/// selection; `refine_selection feather 8, decontaminate, output
/// new_layer_with_mask`, so the interval also holds the feather, the cutout
/// assembly and its display): E 1,777,026,320 / 1,537,868,880 / 1,776,993,552
/// against `foreground_estimate_plan` 3,272,388,608 (1.841×).
pub(crate) const FOREGROUND_BYTES_PER_PIXEL: u64 = 136;

const SOURCE_REFUSAL: &str =
    "native foreground estimation requires canvas-sized RGBA16 in the exact document profile";

impl PrecisionDocument {
    /// The estimate's plan for `source`: `retained_bytes`, `scratch_bytes`
    /// (`8·BOOKKEEPING_BYTES + n·136`) and `input_samples` (the canvas pixel
    /// count: the matte). Refuses a 32-bit float document first (module doc),
    /// a source that is not canvas-sized RGBA16 in the document's exact
    /// profile, and a profile with no matrix/TRC model (a Lab Color document
    /// excepted, #547 phase 6b-ii).
    pub fn foreground_estimate_plan(&self, source: &PrecisionImage) -> Result<Value, String> {
        if self.is_float() {
            return Err(crate::float_document::decontaminate_refusal());
        }
        if source.dimensions() != self.dimensions()
            || source.storage() != PrecisionStorage::Rgba16Unorm
            || source.profile().bytes() != self.profile().bytes()
        {
            return Err(SOURCE_REFUSAL.into());
        }
        if self.profile().model().is_none() && !self.is_lab() {
            return Err(
                "foreground estimation requires a matrix/TRC RGB profile; convert the document profile first"
                    .into(),
            );
        }
        let (width, height) = self.dimensions();
        let n = pixel_count(width, height)? as u64;
        Ok(json!({
            "retained_bytes": self.retained_bytes(),
            "scratch_bytes": 8 * BOOKKEEPING_BYTES + n * FOREGROUND_BYTES_PER_PIXEL,
            "input_samples": n,
        }))
    }

    /// Estimates foreground RGB for `source` under the supplied `matte`
    /// (canvas-sized u16 coverage), by the strict JSON `json`:
    /// `{"color_space":"encoded","strength":0..1}` (strength defaults to 1).
    /// The result shares every tile the estimate leaves unchanged with
    /// `source`. `Ok(None)`: strength 0, a matte with no fractional sample, or
    /// no code changing. `Err`: the plan's refusals, `"linear"` (by name), a
    /// matte of another length, a source pixel that is not opaque, a constant
    /// fractional matte, admission, or a cancelled job.
    pub(crate) fn estimate_foreground(
        &self,
        source: &PrecisionImage,
        matte: &[u16],
        json: &str,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionImage>, String> {
        if self.is_float() {
            return Err(crate::float_document::decontaminate_refusal());
        }
        let parameters = object(json, &["color_space", "strength"])?;
        match parameters.get("color_space").and_then(|v| v.as_str()) {
            Some("encoded") => {}
            Some("linear") => {
                return Err(
                    "foreground estimation in a 16-bit document supports color_space encoded; linear is not available at this depth"
                        .into(),
                )
            }
            _ => return Err("foreground estimation requires color_space encoded".into()),
        }
        let strength = number(&parameters, "strength", 1.0, 0.0, 1.0)?;
        let plan = self.foreground_estimate_plan(source)?;
        let n = plan["input_samples"].as_u64().unwrap_or(u64::MAX);
        if matte.len() as u64 != n {
            return Err("foreground alpha dimensions do not match the source".into());
        }
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let (width, height) = self.dimensions();
        let mut pixels = source
            .read_rect_rgba16(0, 0, width, height)
            .ok_or("could not allocate precision output")?;
        let mut fractional = false;
        let (mut minimum, mut maximum) = (u16::MAX, 0);
        for (i, (&a, p)) in matte.iter().zip(&pixels).enumerate() {
            job.every(i)?;
            if p[3] != u16::MAX {
                return Err("foreground estimation requires an opaque source composite".into());
            }
            fractional |= a > 0 && a < u16::MAX;
            minimum = minimum.min(a);
            maximum = maximum.max(a);
        }
        if strength == 0.0 || !fractional {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        if minimum == maximum {
            return Err(
                "constant fractional alpha cannot identify foreground and background colors".into(),
            );
        }
        let mut input: Vec<[f32; 4]> = reserved_vec(pixels.len())?;
        for (i, (p, &a)) in pixels.iter().zip(matte).enumerate() {
            job.every(i)?;
            input.push([
                f32::from(p[0]) / 65535.0,
                f32::from(p[1]) / 65535.0,
                f32::from(p[2]) / 65535.0,
                f32::from(a) / 65535.0,
            ]);
        }
        let estimate = solve(input, (width, height), job)?;
        let mut changed = false;
        for (i, ((p, &a), colors)) in pixels.iter_mut().zip(matte).zip(&estimate).enumerate() {
            job.every(i)?;
            if a == 0 || a == u16::MAX {
                continue;
            }
            for c in 0..3 {
                let original = f32::from(p[c]) / 65535.0;
                let value = original + strength * (colors[c] - original);
                let code = (f64::from(value).clamp(0.0, 1.0) * 65535.0).round() as u16;
                changed |= code != p[c];
                p[c] = code;
            }
        }
        drop(estimate);
        if !changed {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        let result = source.with_rect_rgba16(0, 0, width, height, &pixels)?;
        job.report(1.0, 5)?;
        Ok(result)
    }
}
