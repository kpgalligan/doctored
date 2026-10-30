//! Reversible coverage refinement (Select and Mask): the edge stage
//! first, then smooth, signed contour shift, feather and contrast, using the
//! existing selection kernels. Zero controls copy exactly.
//!
//! The edge stage is `edge_refine` (the kernel): coverage inside a band of
//! `edge_radius` around the 50% contour, and under the area plane's INCLUDE
//! pixels, is re-estimated from the source's own colours; Smart Radius fades
//! the estimate where the picture has no edge. It runs when `edge_radius` is
//! above 0 or the area plane holds an INCLUDE pixel. It reads the source's
//! pixels and never changes them: colour decontamination is a separate
//! estimate (`foreground_estimate`).

use crate::{
    doc_select::{
        feather_mask_with_context, offset_contour_with_context, smooth_mask_with_context,
    },
    edge_refine::{self, EdgeRefine},
    job_control::JobContext,
    selection_tools::{number, object},
    DrImage,
};

/// Refines `mask` (canvas coverage of `source`) by the strict JSON `json`:
/// `smooth` / `feather` 0…64, `shift` −64…64, `contrast` 0…1, `edge_radius`
/// 0…64 and `smart_radius` (a boolean). `area` is the optional category plane
/// (`edge_refine::AREA_*`; `None` = all automatic). `Err`: an unknown or
/// out-of-range parameter, an area value outside the three categories, a
/// reservation the job's scratch limit refuses, or a cancelled job.
pub(crate) fn refine(
    source: &DrImage,
    mask: &[u8],
    area: Option<&[u8]>,
    json: &str,
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    let p = object(
        json,
        &[
            "smooth",
            "feather",
            "shift",
            "contrast",
            "edge_radius",
            "smart_radius",
        ],
    )?;
    let smooth = number(&p, "smooth", 0.0, 0.0, 64.0)?;
    let feather = number(&p, "feather", 0.0, 0.0, 64.0)?;
    let shift = number(&p, "shift", 0.0, -64.0, 64.0)?;
    let contrast = number(&p, "contrast", 0.0, 0.0, 1.0)?;
    let radius = number(&p, "edge_radius", 0.0, 0.0, edge_refine::MAX_EDGE_RADIUS)?;
    let smart = match p.get("smart_radius") {
        None => false,
        Some(value) => value
            .as_bool()
            .ok_or("smart_radius must be true or false")?,
    };
    let includes = area.map(edge_refine::area_includes).transpose()?;
    let edge = radius > 0.0 || includes == Some(true);
    let (w, h) = source.pixels.dimensions();
    let n = u64::from(w) * u64::from(h);
    // SDF has two float planes + output and <=20*max(w,h)+4 line scratch. The
    // edge stage's bound covers its own planes, the f32 coverage and result
    // beside them, and the four stages that follow (which run after it frees).
    let bytes = if edge {
        edge_refine::BYTES_PER_PIXEL * n
    } else if shift != 0.0 {
        9 * n + 20 * u64::from(w.max(h)) + 4
    } else {
        5 * n
    };
    job.reserve(bytes + 256 * 1024)?;
    let mut out = mask.to_vec();
    // With no edge stage the four stages report exactly as they always have.
    let stages = if edge {
        let coverage: Vec<f32> = mask.iter().map(|v| f32::from(*v) / 255.0).collect();
        let raw = source.pixels.as_raw();
        let refined = edge_refine::refine(
            |i| {
                let p = &raw[i * 4..i * 4 + 4];
                let alpha = f32::from(p[3]) / 255.0;
                [p[0], p[1], p[2]].map(|v| f32::from(v) / 255.0 * alpha)
            },
            &coverage,
            area,
            (w, h),
            EdgeRefine { radius, smart },
            job.range(0.0, 0.5),
        )?;
        for (i, (byte, value)) in out.iter_mut().zip(&refined).enumerate() {
            job.every(i)?;
            *byte = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        job.range(0.5, 1.0)
    } else {
        job
    };
    smooth_mask_with_context(&mut out, w, h, smooth, stages)?;
    stages.report(0.2, 6)?;
    if shift != 0.0 {
        offset_contour_with_context(&mut out, w as usize, h as usize, shift, stages)?;
    }
    stages.report(0.4, 6)?;
    feather_mask_with_context(&mut out, w, h, feather, stages)?;
    stages.report(0.6, 6)?;
    if contrast > 0.0 {
        // Contrast narrows the soft interval symmetrically around 50%; at1,
        // thresholds coverage at128. Avoid divisionbyzero at the endpoint.
        for (i, value) in out.iter_mut().enumerate() {
            stages.every(i)?;
            *value = if contrast == 1.0 {
                if *value >= 128 {
                    255
                } else {
                    0
                }
            } else {
                (((f32::from(*value) / 255.0 - 0.5) / (1.0 - contrast) + 0.5).clamp(0.0, 1.0)
                    * 255.0)
                    .round() as u8
            };
        }
    }
    job.report(1.0, 5)?;
    Ok(out)
}
