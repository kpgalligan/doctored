//! Destructive Image ▸ Adjustments in a 32-bit float document: the float
//! arms of `selected_edit_plan`, `selected_edit_unlocked`
//! (`selected_adjustment`) and `applying_adjustment_unlocked`
//! (`precision_adjustment`). Each adjustment kept at 32 bits applies
//! destructively with the same result as its layer (measured against
//! Photoshop 26.10.0: `judged.json`, Applied-same), so a pixel takes exactly
//! the layer's form: `PrecisionAdjustment::apply_linear` (`adjust_float`),
//! then the straight, alpha-preserving coverage lerp [`mix_float`] with no
//! clamp. The output goes through `PrecisionImage::rebuilt_rgba32_extended`,
//! whose range check refuses a sample outside ±65504; a non-finite component
//! refuses with the render's own sentence. A bitwise-unchanged result, and an
//! identity description, give `None`. A selected request whose op is a filter
//! the float mode runs (`float_document::admits_filter`) is
//! handed to `float_filter_apply`, which reuses [`mix_float`] and
//! [`APPLY_BYTES_PER_PIXEL`].
//!
//! The applied set is the operator table's `<op>_apply` rows
//! (`float_document::admits_apply`): Levels, Black & White with its Tint,
//! Channel Mixer, Photo Filter, Exposure and point-form Curves, map-form
//! Curves, Color Balance, Color Lookup (every table), `bcs` in both its
//! legacy and its modern form (destructive equals layer, by decision — no
//! Photoshop take fits the destructive legacy pivot), Hue/Saturation, the
//! `document_v2` White Balance, Hue Rotate, Grayscale,
//! Sepia and the display forms (Threshold, Posterize, Gradient Map,
//! Vibrance, Selective Color, Replace Color), each equal to its layer. The
//! apply passes each pixel's IMAGE position to the form (`apply_at`'s
//! destructive convention). An op whose LAYER form refuses (Invert,
//! Shadows/Highlights, the legacy White Balance) keeps that refusal here,
//! word for word, and every other op names the apply
//! (`float_document::apply_refusal`).
//!
//! # Admission
//! The plan's scratch is `6·BOOKKEEPING + preparation + 2·description +
//! n·APPLY_BYTES_PER_PIXEL` plus the borrowed canvas coverage on the
//! selected path ([`coverage_scratch`]: `canvas·2` at 5/4), which the host
//! builds only after the plan. The
//! per-pixel term covers the n×16 output vector and the n×16 tiles
//! `rebuilt_rgba32_extended` copies it into while it is still held, 32
//! bytes a pixel, at 5/4 (`APPLY_BYTES_PER_PIXEL` 40). Measured 2026-10-09
//! (Apple M2 Max, release, one case per process, three runs,
//! `MallocLargeCache=0`, E = `ri_interval_max_phys_footprint` − pre, a
//! baseline twin per case reading E 0): an opaque 6000×4000 float layer in
//! an sRGB document (ramps over [−0.25, 3.75] and [0.02, 2.02], a hashed
//! blue in [0, 2]), every tile changed;
//! S = `selected_edit` with no coverage, C = with a canvas-sized coverage
//! (half full, half 32768) built before the call, A = `apply_adjustment`.
//! Levels is take2's T1, Photo Filter `#ec8a00` at 0.25 with luminosity
//! held.
//!
//! | case | E (three runs) | retained | scratch | scratch / max E |
//! |---|---|---|---|---|
//! | S Levels | 769,933,864 / 769,983,016 / 769,966,632 | 385,635,175 | 1,030,511,674 | 1.338 |
//! | S Photo Filter | 769,966,632 / 769,983,016 / 769,950,248 | 385,635,175 | 1,030,505,008 | 1.338 |
//! | C Levels | 769,950,224 / 769,999,400 / 769,966,608 | 385,635,175 | 1,030,511,674 | 1.338 |
//! | A Levels | 769,933,864 / 769,933,864 / 769,966,632 | 385,635,175 | 970,511,674 | 1.260 |
//! | C Levels, coverage inside | 818,020,928 / 818,037,312 / 818,037,312 | 385,635,175 | 1,030,511,674 | 1.260 |
//!
//! E is 32 bytes a pixel plus about 2 MB whatever the op; ⌈1.25 × max E⌉ of
//! the first four rows is 962,499,250, which `float_adjust_apply_24mp` pins.
//!
//! The last row is from 2026-10-12 (the same method, case
//! `levels_c` of `tests/float_adjust_apply_24mp.rs::measure`): the same C
//! request with its coverage built INSIDE the measured interval, which is
//! when the host builds it, so E gains the coverage's `canvas·2`. The plan
//! charged that coverage once, with no margin: 1,018,511,674 against
//! ⌈1.25 × 818,037,312⌉ = 1,022,546,640, short by 4 MB (the float filters'
//! covered case missed the same way, `float_filter_apply`'s table). The
//! coverage scales with the CANVAS and the per-pixel term with the layer, so
//! a larger `APPLY_BYTES_PER_PIXEL` would not cover a small layer on a large
//! canvas; the coverage is charged at 5/4 instead ([`coverage_scratch`]),
//! which is what the scratch column shows. `float_adjust_apply_24mp` pins
//! ⌈1.25 × 818,037,312⌉ for the selected plans.
//!
//! The display forms' preparation builds an encoder inside
//! `PrecisionAdjustment::prepare_scratch_bytes`' 4 MiB. Case `vibrance_c`
//! (2026-10-12, the same method, the display-scaled Vibrance under
//! `levels_c`'s coverage, a `base` twin reading E 0): E 818,053,696 /
//! 818,037,312 / 818,053,696 against a plan of 1,030,501,774 ≥
//! ⌈1.25 × max E⌉ = 1,022,567,120, which `float_adjust_apply_24mp` pins
//! (`VIBRANCE_COVERED_FLOOR`).
use serde_json::{json, Value};

use crate::adjust_precision::PrecisionAdjustment;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, reserved_vec, BOOKKEEPING_BYTES};
use crate::selected_adjustment::{coverage_at, validate_coverage, SelectedEdit};

/// Scratch bytes per target pixel (module doc's `# Admission`).
pub(crate) const APPLY_BYTES_PER_PIXEL: u64 = 40;

/// What a selected float edit's plan charges for the canvas-sized u16
/// coverage the host builds after the plan: its `canvas·2` bytes at 5/4
/// (module doc's `# Admission`, the last row of the table). `None` on
/// overflow.
pub(crate) fn coverage_scratch(canvas: u64) -> Option<u64> {
    let bytes = canvas.checked_mul(2)?;
    bytes.checked_add(bytes.div_ceil(4))
}

/// The render's sentence for a non-finite adjusted component
/// (`precision_render::adjust_backdrop_sample16`).
const NONFINITE: &str = "high-depth adjustment produced nonfinite RGB";

/// What a float document refuses for a selected request that is neither an
/// adjustment nor a filter it runs (the twelve other filters and the
/// automatic tones): the sentence the u16 arm gives every non-u16 source,
/// unchanged.
pub(crate) const NOT_U16: &str = "selected edits require native u16 storage";

/// The refusal a destructive float apply of `tone` carries, or `None` when
/// the float mode applies it: the LAYER form's refusal first (the same
/// sentence the layer gives), then an op with no `<op>_apply` row.
fn apply_refusal(tone: &PrecisionAdjustment) -> Option<String> {
    if let Err(reason) = tone.apply_linear([0.0; 3], (0, 0)) {
        return Some(reason);
    }
    let op = tone.operation();
    (!crate::float_document::admits_apply(op)).then(|| crate::float_document::apply_refusal(op))
}

/// `selected_adjustment::mix_codes`'s alpha-preserving branch (the master)
/// over stored float components: `old + t·(after − old)` in f64 with
/// `t = weight / 65535`, no clamp and no quantization; alpha is kept bit for
/// bit. Full coverage takes `after` verbatim, which is what the layer's own
/// mix does at `k ≥ 1` (`blend::mix_adjusted_rgb`), so an unselected apply
/// equals the layer render bit for bit. The caller answers weight 0.
pub(crate) fn mix_float(before: [f32; 4], after: [f32; 3], weight: u16) -> [f32; 4] {
    if weight == u16::MAX {
        return [after[0], after[1], after[2], before[3]];
    }
    let t = f64::from(weight) / 65535.;
    let mut out = before;
    for c in 0..3 {
        let old = f64::from(before[c]);
        out[c] = (old + t * (f64::from(after[c]) - old)) as f32;
    }
    out
}

impl PrecisionDocument {
    /// The scratch a float apply of `description` over `count` target pixels
    /// charges, `coverage` being the borrowed canvas coverage's bytes
    /// (module doc's `# Admission`). An op the float mode does not apply is
    /// refused by name first. No work refusal: the apply runs under
    /// its caller's job control, bounded by this plan's memory.
    pub(crate) fn float_apply_scratch(
        &self,
        description: &str,
        count: u64,
        coverage: u64,
    ) -> Result<u64, String> {
        if let Some(reason) =
            apply_refusal(&PrecisionAdjustment::parse(description, self.profile())?)
        {
            return Err(reason);
        }
        let preparation = PrecisionAdjustment::prepare_scratch_bytes(description, self.profile())?;
        6u64.checked_mul(BOOKKEEPING_BYTES)
            .and_then(|n| n.checked_add(preparation))
            .and_then(|n| n.checked_add(description.len() as u64 * 2))
            .and_then(|n| n.checked_add(count.checked_mul(APPLY_BYTES_PER_PIXEL)?))
            .and_then(|n| n.checked_add(coverage))
            .ok_or_else(|| "adjustment scratch overflow".into())
    }

    /// `selected_edit_plan`'s float arm: an adjustment the float mode
    /// applies, refused by name otherwise, before any allocation the plan
    /// charges; a filter the float mode runs is `float_filter_plan`'s.
    pub(crate) fn float_selected_edit_plan(&self, edit: &SelectedEdit) -> Result<Value, String> {
        if crate::float_document::admits_filter(&edit.op) {
            return self.float_filter_plan(edit);
        }
        let image = self
            .layer_source(edit.layer)
            .ok_or("selected edit requires a raster layer")?;
        let description = float_description(edit)?;
        let (w, h) = image.dimensions();
        let count = u64::from(w) * u64::from(h);
        let (cw, ch) = self.dimensions();
        let canvas = u64::from(cw) * u64::from(ch);
        let coverage = coverage_scratch(canvas).ok_or("adjustment scratch overflow")?;
        let scratch = self.float_apply_scratch(description, count, coverage)?;
        // What the result keeps beyond its source: the rewritten layer's f32
        // samples.
        Ok(
            json!({"retained_bytes":self.retained_bytes(),"scratch_bytes":scratch,
            "output_bytes":count * 16,
            "input_samples":canvas,"input_width":cw,"input_height":ch}),
        )
    }

    /// `selected_edit_unlocked`'s float arm; a filter the float mode runs is
    /// `float_filter_unlocked`'s.
    pub(crate) fn float_selected_edit_unlocked(
        &self,
        edit: &SelectedEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        if crate::float_document::admits_filter(&edit.op) {
            return self.float_filter_unlocked(edit, coverage, limit, other, job);
        }
        let scratch = self.float_selected_edit_plan(edit)?["scratch_bytes"]
            .as_u64()
            .unwrap_or(u64::MAX);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        validate_coverage(self, coverage)?;
        self.float_applied(edit.layer, float_description(edit)?, coverage, job)
    }

    /// `applying_adjustment_unlocked`'s float arm: the whole layer.
    pub(crate) fn float_applying_adjustment_unlocked(
        &self,
        index: usize,
        description: &str,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let image = self
            .layer_source(index)
            .ok_or("destructive high-depth adjustment requires a raster source")?;
        let (w, h) = image.dimensions();
        let count = u64::from(w) * u64::from(h);
        let scratch = self.float_apply_scratch(description, count, 0)?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        self.float_applied(index, description, &[], job)
    }

    /// Parse, refuse, then apply `description` to record `index` under
    /// `coverage` (empty: the whole layer).
    fn float_applied(
        &self,
        index: usize,
        description: &str,
        coverage: &[u16],
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let tone = PrecisionAdjustment::parse(description, self.profile())?;
        if let Some(reason) = apply_refusal(&tone) {
            return Err(reason);
        }
        job.check()?;
        if tone.is_identity() {
            job.report(1., 5)?;
            return Ok(None);
        }
        let layer = self.layer(index).ok_or("invalid selected layer")?;
        let image = layer
            .source()
            .ok_or("selected edit requires raster source")?;
        let runs = image
            .runs_rgba32()
            .ok_or("a 32-bit float apply requires f32 storage")?;
        let (w, h) = image.dimensions();
        let canvas = self.dimensions();
        let mut pixels = reserved_vec(w as usize * h as usize)?;
        let mut changed = false;
        let mut i = 0;
        for run in runs {
            for before in run {
                job.every(i)?;
                let weight = coverage_at(coverage, canvas, (w, h), layer.offset(), i);
                // The IMAGE position, `apply_at`'s destructive convention.
                let xy = ((i % w as usize) as i64, (i / w as usize) as i64);
                i += 1;
                // A transparent pixel and an unselected one are copied, as
                // the layer skips a transparent backdrop.
                let after = if before[3] == 0.0 || weight == 0 {
                    *before
                } else {
                    let rgb = tone.apply_linear([before[0], before[1], before[2]], xy)?;
                    if rgb.iter().any(|v| !v.is_finite()) {
                        return Err(NONFINITE.into());
                    }
                    mix_float(*before, rgb, weight)
                };
                changed |= after.map(f32::to_bits) != before.map(f32::to_bits);
                pixels.push(after);
            }
        }
        job.check()?;
        let out = if changed {
            let image = image.rebuilt_rgba32_extended(w, h, pixels, self.profile().clone())?;
            self.replacing_content(
                index,
                Content::Raster {
                    image,
                    offset: layer.offset(),
                },
            )?
        } else {
            None
        };
        job.report(1., 5)?;
        Ok(out)
    }
}

/// The adjustment description a float selected request carries; the
/// automatic tones (`auto_tone`, `auto_contrast`, `auto_color`) refuse with
/// `float_document::auto_refusal`, and the filters the float mode does not
/// run keep the u16 arm's refusal.
fn float_description(edit: &SelectedEdit) -> Result<&str, String> {
    match edit.description.as_deref() {
        Some(description) if edit.op == "adjustment" => Ok(description),
        _ if matches!(
            edit.op.as_str(),
            "auto_tone" | "auto_contrast" | "auto_color"
        ) =>
        {
            Err(crate::float_document::auto_refusal())
        }
        _ => Err(NOT_U16.into()),
    }
}
