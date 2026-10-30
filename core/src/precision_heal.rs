//! Bounded typed clone-source healing. The explicit solve window uses the
//! existing membrane classifier/solver and shared byte/u16 writeback. Coverage
//! selects the normalized half-coverage contour; healed RGB blends at opacity,
//! while destination alpha and all storage outside the window remain exact.

use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::reserved_vec;
use crate::precision_paint::{canvas_index, PaintEdit, SourceView};

pub(crate) fn plan(pixels: u64) -> Result<(u64, u64), String> {
    if pixels > crate::doc_heal::MAX_SOLVE_WINDOW_PIXELS {
        return Err(
            "high-depth healing window exceeds the shared solve limit; heal a smaller area or fewer separate parts"
                .into(),
        );
    }
    // Region/input buffers, b/u, all multigrid levels, coarse corrections,
    // classifications and residuals. This intentionally exceeds the byte
    // solver's live-array inventory and never assumes a reduced-resolution solve.
    Ok((
        pixels * 160,
        crate::poisson::precision_sweep_budget(pixels as usize).saturating_mul(3),
    ))
}

/// Heals `pixels`, a `plane.0` buffer standing at canvas `plane.1` (the
/// target's rect under the window, 13B-3d-1, or an empty plane when the
/// window misses the layer).
pub(crate) fn apply(
    doc: &PrecisionDocument,
    edit: &PaintEdit,
    coverage: &[u16],
    pixels: &mut [[u16; 4]],
    plane: ((u32, u32), (i32, i32)),
    job: JobContext<'_>,
) -> Result<(), String> {
    let PaintEdit::Heal {
        layer,
        source_layer,
        dx,
        dy,
        x,
        y,
        width,
        height,
        opacity,
    } = edit
    else {
        return Err("invalid high-depth healing request".into());
    };
    let target = doc
        .layer(*layer)
        .ok_or("high-depth healing target is out of range")?;
    target
        .source()
        .ok_or("high-depth healing requires a raster target")?;
    let source_view = SourceView::new(doc, *source_layer)?;
    let n = *width as usize * *height as usize;
    let mut source = reserved_vec(n * 3)?;
    let mut region = reserved_vec(n)?;
    let mut write = reserved_vec(n)?;
    for wy in 0..*height {
        job.check()?;
        for wx in 0..*width {
            let i = wy as usize * *width as usize + wx as usize;
            job.every(i)?;
            let cx = i64::from(*x) + i64::from(wx);
            let cy = i64::from(*y) + i64::from(wy);
            let ci = canvas_index(cx, cy, doc.width, doc.height);
            let rgba = source_view.at(cx + i64::from(*dx), cy + i64::from(*dy));
            let usable_source = rgba.is_some_and(|s| s[3] > 0);
            let covered = ci.is_some_and(|j| coverage[j] >= 32768) && usable_source;
            let sample = rgba.unwrap_or([0; 4]);
            source.extend_from_slice(&sample[..3]);
            region.push(if covered { u16::MAX } else { 0 });
            write.push(if covered { u16::MAX } else { 0 });
        }
    }
    crate::heal_writeback::heal(
        doc.dimensions(),
        plane.0,
        pixels.as_flattened_mut(),
        plane.1,
        (
            i64::from(*x),
            i64::from(*y),
            *width as usize,
            *height as usize,
        ),
        &source,
        &region,
        &write,
        *opacity,
        job,
    )?;
    Ok(())
}
