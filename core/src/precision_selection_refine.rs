//! Native u16 coverage refinement: smooth, signed contour shift, feather, then
//! contrast, in that fixed order. Each geometric stage IS the existing
//! `PlaneOperation` of that kind applied to the previous stage's output, so a
//! refinement can never drift from the standalone Modify Selection kernels it
//! is composed of; only the ordering and the contrast curve live here. The
//! contrast threshold is the native midpoint 32768 (the u16 twin of the byte
//! kernel's 128), and every zero control is skipped, so all-zero controls copy
//! the input codes exactly. Byte refinement (`selection_refine.rs`) is
//! unchanged; this is the depth twin, not a replacement.
//!
//! `precision_selection_refine.rs` is the `refine` kind: the basic Smooth/Shift/Feather/Contrast
//! refinement composed IN THAT ORDER out of those same standalone kinds, with its contrast curve
//! the only new arithmetic (threshold 32768, zero controls copy exactly); `selection_refine.rs`
//! keeps the byte contract unchanged.
use crate::job_control::JobContext;
use crate::precision_selection_plane::{copy, PlaneOperation};
use crate::precision_selection_support::{self as support, Rect};

pub(crate) fn validate(smooth: f32, feather: f32, shift: f32, contrast: f32) -> Result<(), String> {
    if ![smooth, feather, shift, contrast]
        .iter()
        .all(|value| value.is_finite())
    {
        return Err("native refinement controls must be finite".into());
    }
    if !(0. ..=64.).contains(&smooth) || !(0. ..=64.).contains(&feather) {
        return Err("native refinement smooth and feather must be within [0,64]".into());
    }
    if !(-64. ..=64.).contains(&shift) {
        return Err("native refinement shift must be within [-64,64]".into());
    }
    if !(0. ..=1.).contains(&contrast) {
        return Err("native refinement contrast must be within [0,1]".into());
    }
    Ok(())
}

/// The sum of the stages' own forecasts, each the expression the matching
/// `PlaneOperation` kind declares (`support::work`), charged at the INPUT
/// support grown by the CUMULATIVE reach of the stages through that one
/// (clamped to the canvas) — an upper bound on the window each stage's
/// bounded `apply` will run over — plus `n` for the contrast curve. Skipped
/// stages cost nothing; `None` support charges only the contrast term, and
/// `Some((0, 0, w, h))` is the pre-13A-6a canvas sum exactly. Only
/// the whole-plane oracle ([`reference_refine`]) compares it to a bound;
/// [`refine`] runs under its caller's job control and has no work refusal.
pub(crate) fn work_units(
    support: Option<Rect>,
    dimensions: (u32, u32),
    smooth: f32,
    feather: f32,
    shift: f32,
    contrast: f32,
) -> u64 {
    let n = u64::from(dimensions.0) * u64::from(dimensions.1);
    let mut total = if contrast > 0. { n } else { 0 };
    if let Some(bounds) = support {
        let mut reach = 0u32;
        for operation in stages(smooth, feather, shift).into_iter().flatten() {
            reach = reach.saturating_add(support::reach(&operation).unwrap_or(0));
            let window = support::window(bounds, reach, dimensions.0, dimensions.1);
            total = total.saturating_add(support::work(&operation, window));
        }
    }
    total
}

fn stages(smooth: f32, feather: f32, shift: f32) -> [Option<PlaneOperation>; 3] {
    [
        (smooth > 0.).then_some(PlaneOperation::Smooth { radius: smooth }),
        if shift > 0. {
            Some(PlaneOperation::Grow { radius: shift })
        } else if shift < 0. {
            Some(PlaneOperation::Shrink { radius: -shift })
        } else {
            None
        },
        (feather > 0.).then_some(PlaneOperation::Feather { radius: feather }),
    ]
}

pub(crate) fn refine(
    input: &[u16],
    dimensions: (u32, u32),
    smooth: f32,
    feather: f32,
    shift: f32,
    contrast: f32,
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    validate(smooth, feather, shift, contrast)?;
    let mut out = compose(
        input,
        stages(smooth, feather, shift),
        job,
        |operation, plane| operation.apply(plane, dimensions, job),
    )?;
    contrast_curve(&mut out, contrast, job)?;
    job.check()?;
    Ok(out)
}

/// The WHOLE-PLANE composition (the 13A-6a oracle): the canvas sum check,
/// then every stage through `spatial_kernel` over the canvas.
pub(crate) fn reference_refine(
    input: &[u16],
    dimensions: (u32, u32),
    smooth: f32,
    feather: f32,
    shift: f32,
    contrast: f32,
) -> Result<Vec<u16>, String> {
    validate(smooth, feather, shift, contrast)?;
    let canvas = Some((0, 0, dimensions.0, dimensions.1));
    if work_units(canvas, dimensions, smooth, feather, shift, contrast)
        > crate::precision_render::MAX_WORK_UNITS
    {
        return Err("native selection exceeds the work limit".into());
    }
    let job = JobContext::NONE;
    let mut out = compose(
        input,
        stages(smooth, feather, shift),
        job,
        |operation, plane| operation.spatial_kernel(plane, dimensions, job),
    )?;
    contrast_curve(&mut out, contrast, job)?;
    job.check()?;
    Ok(out)
}

/// The ONE stage loop: each present stage runs on the previous stage's
/// output through `run`; with no stage the input is copied exactly.
fn compose(
    input: &[u16],
    stages: [Option<PlaneOperation>; 3],
    job: JobContext<'_>,
    mut run: impl FnMut(&PlaneOperation, &[u16]) -> Result<Vec<u16>, String>,
) -> Result<Vec<u16>, String> {
    let mut current: Option<Vec<u16>> = None;
    for operation in stages.into_iter().flatten() {
        let next = run(&operation, current.as_deref().unwrap_or(input))?;
        current = Some(next);
    }
    match current {
        Some(values) => Ok(values),
        None => copy(input, job),
    }
}

/// The contrast curve around the native midpoint; a no-op at contrast 0.
fn contrast_curve(out: &mut [u16], contrast: f32, job: JobContext<'_>) -> Result<(), String> {
    if contrast > 0. {
        let narrowing = 1. - f64::from(contrast);
        for (i, value) in out.iter_mut().enumerate() {
            job.every(i)?;
            *value = if contrast == 1. {
                if *value >= 32768 {
                    65535
                } else {
                    0
                }
            } else {
                let t = ((f64::from(*value) / 65535. - 0.5) / narrowing + 0.5).clamp(0., 1.);
                (t * 65535.).round() as u16
            };
        }
    }
    Ok(())
}
