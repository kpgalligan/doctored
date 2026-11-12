//! Scalar storage adapter for the one native tone/spatial interpreter. A plane
//! is an opaque gray input, so filters see its codes even when editing alpha0.
//! The selected scalar is extracted before terminal writeback quantization.
//!
//! The shared tone/filter interpreter reads an opaque gray adapter, retaining hidden RGB/alpha
//! scalar codes. Liquify (`liquify`) is refused here, in `plan` and in `evaluate`: it moves a
//! layer's pixels and has no mask, colour-plane or channel form. So are the Develop
//! Filter's masks (`develop_filter` with a non-empty `masks`), by
//! `develop_filter_masks::SCALAR_REFUSAL`.
use crate::adjust_precision::PrecisionAdjustment;
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::ops_spatial::Frame;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{reserved_vec, PrecisionImage, BOOKKEEPING_BYTES};
use crate::selected_adjustment::SelectedEdit;
use crate::selected_auto_levels::AutoLevels;
use crate::selected_filter::SelectedFilter;
use std::sync::Arc;

pub(crate) const LIQUIFY_REFUSAL: &str = "liquify edits layer pixels only";

pub(crate) fn plan(
    edit: &SelectedEdit,
    dimensions: (u32, u32),
    profile: &IccProfile,
) -> Result<u64, String> {
    if edit.op == "liquify" {
        return Err(LIQUIFY_REFUSAL.into());
    }
    if edit.op == "develop_filter" && crate::develop_filter_masks::has_masks(&edit.params) {
        return Err(crate::develop_filter_masks::SCALAR_REFUSAL.into());
    }
    let (w, h) = dimensions;
    let n = u64::from(w) * u64::from(h);
    let extra = if edit.op == "adjustment" {
        let description = edit
            .description
            .as_deref()
            .ok_or("scalar adjustment needs a description")?;
        let (scratch, _) = PrecisionAdjustment::guide_plan_for_description(description, w, h)?;
        scratch + PrecisionAdjustment::prepare_scratch_bytes(description, profile)?
    } else if edit.is_auto() {
        AutoLevels::validate(edit)?;
        4 * 65536 * 8
    } else {
        SelectedFilter::parse(&edit.op, &edit.params)?
            .with_frame(None)
            .scratch(dimensions, crate::sampling_gaussian::CONTROLLED_JOB_WORK)?
    };
    Ok(extra + n * 24 + 8 * BOOKKEEPING_BYTES)
}
pub(crate) fn evaluate(
    edit: &SelectedEdit,
    input: &[u16],
    dimensions: (u32, u32),
    profile: &Arc<IccProfile>,
    coverage: &[u16],
    frame: Option<Frame>,
    job: JobContext<'_>,
) -> Result<Vec<f64>, String> {
    if edit.op == "liquify" {
        return Err(LIQUIFY_REFUSAL.into());
    }
    if edit.op == "develop_filter" && crate::develop_filter_masks::has_masks(&edit.params) {
        return Err(crate::develop_filter_masks::SCALAR_REFUSAL.into());
    }
    let (w, h) = dimensions;
    let mut pixels = reserved_vec(input.len())?;
    for (i, &code) in input.iter().enumerate() {
        job.every(i)?;
        pixels.push([code, code, code, 65535]);
    }
    let image = PrecisionImage::from_rgba16_unseeded(w, h, pixels, Arc::clone(profile), None)?;
    // Unbound: a mask or alpha channel is `[g, g, g]` read back as its luma,
    // so a Lab document's plane answers as on RGB, never in a Lab form.
    let tone = if edit.op == "adjustment" {
        Some(PrecisionAdjustment::parse_unbound(
            edit.description.as_deref().unwrap(),
            profile,
        )?)
    } else {
        None
    };
    let guide = tone
        .as_ref()
        .map(|t| t.guide(w, h, &|i| image.pixel(i), job.range(0., 0.4)))
        .transpose()?
        .flatten();
    let filter = if tone.is_some() {
        None
    } else {
        Some(if edit.is_auto() {
            let doc = PrecisionDocument::from_image(
                image.clone(),
                Default::default(),
                Default::default(),
            )?;
            SelectedFilter::LevelsChannels(AutoLevels::derive(
                &doc,
                edit,
                coverage,
                job.range(0., 0.4),
            )?)
        } else {
            // The caller's run-time frame always replaces a parsed region.
            SelectedFilter::parse(&edit.op, &edit.params)?.with_frame(frame)
        })
    };
    let neighborhood = filter
        .as_ref()
        .map(|f| f.plane(&image, None, job.range(0., 0.7)))
        .transpose()?
        .flatten();
    let mut rng = crate::rng::SplitMix64::new(filter.as_ref().map_or(0, SelectedFilter::seed));
    let mut out = reserved_vec(input.len())?;
    for (i, &original_code) in input.iter().enumerate() {
        job.every(i)?;
        let original = image.pixel(i);
        let rgb = if let Some(t) = &tone {
            let x = (i % w as usize) as u32;
            let y = (i / w as usize) as u32;
            t.apply_at(
                [original[0], original[1], original[2]],
                guide.as_ref().map_or(0., |g| g.at(x, y)),
                (i64::from(x), i64::from(y)),
            )
        } else {
            let result =
                filter
                    .as_ref()
                    .unwrap()
                    .evaluate(i, original, neighborhood.as_deref(), &mut rng);
            [result[0], result[1], result[2]]
        };
        if rgb.iter().any(|v| !v.is_finite()) {
            return Err("scalar filter returned a non-finite value".into());
        }
        // doc_scope::scoped_gray takes the filtered result's Rec.709 gray,
        // including colorizing operators. Read the same three components here
        // before the one native quantization at selected scalar writeback.
        let code = if rgb == [original[0], original[1], original[2]] {
            f64::from(original_code)
        } else {
            let coefficients = [
                crate::blend::LUMA_R,
                crate::blend::LUMA_G,
                crate::blend::LUMA_B,
            ];
            rgb.into_iter()
                .zip(coefficients)
                .map(|(v, k)| f64::from(v.clamp(0., 1.)) * f64::from(k))
                .sum::<f64>()
                * 65535.
        };
        out.push(code);
    }
    job.check()?;
    Ok(out)
}

pub(crate) fn auto_levels(
    edit: &SelectedEdit,
    input: &[u16],
    dimensions: (u32, u32),
    profile: &Arc<IccProfile>,
    coverage: &[u16],
    job: JobContext<'_>,
) -> Result<serde_json::Value, String> {
    if !edit.is_auto() {
        return Err(
            "scalar Auto derivation requires auto_tone, auto_contrast or auto_color".into(),
        );
    }
    let mut pixels = reserved_vec(input.len())?;
    for (i, &v) in input.iter().enumerate() {
        job.every(i)?;
        pixels.push([v, v, v, 65535]);
    }
    let image = PrecisionImage::from_rgba16_unseeded(
        dimensions.0,
        dimensions.1,
        pixels,
        Arc::clone(profile),
        None,
    )?;
    let source = PrecisionDocument::from_image(image, Default::default(), Default::default())?;
    Ok(AutoLevels::derive(&source, edit, coverage, job)?.json())
}
