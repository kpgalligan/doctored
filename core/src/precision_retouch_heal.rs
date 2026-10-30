//! Native retouch component windows share the membrane writeback kernel.
//! Stroke topology keeps its established half-coverage contour; optional exact
//! canvas selection remains a separate native final gate.
//!
//! `precision_retouch_heal.rs` keeps the historical half-coverage stroke contour; additive selected
//! clone/heal/spot requests retain native selection separately through source-over or the shared
//! membrane writeback.
//!
//! The sample type is the destination's: u16 codes, or a 32-bit float
//! document's W codes (`retouch_encoding`), whose source and
//! destination planes `float_retouch` encodes before the call. Stroke
//! coverage and the selection are u16 at every depth.
use crate::heal_writeback::HealSample;
use crate::job_control::JobContext;
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply<T: HealSample + Into<f32>>(
    canvas: (u32, u32),
    source: &[[T; 4]],
    coverage: &[u16],
    opacity: f32,
    pixels: &mut [[T; 4]],
    dimensions: (u32, u32),
    offset: (i32, i32),
    selection: Option<&[u16]>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let covered: Vec<bool> = coverage
        .iter()
        .zip(source)
        .map(|(c, p)| *c >= 32768 && p[3].number() > 0.0)
        .collect();
    let parts = crate::doc_heal::components_with_context(
        &covered,
        canvas.0 as usize,
        canvas.1 as usize,
        job,
    )?;
    let mut total = 0u64;
    let windows: Vec<_> = parts
        .iter()
        .map(|part| {
            let (x, y, r, b) = part.bounds;
            let (x, y) = (x.saturating_sub(1), y.saturating_sub(1));
            (x, y, (r + 2).min(canvas.0) - x, (b + 2).min(canvas.1) - y)
        })
        .collect();
    for &(_, _, w, h) in &windows {
        let n = u64::from(w) * u64::from(h);
        crate::precision_heal::plan(n)?;
        total += n;
    }
    if total > crate::doc_heal::MAX_HEAL_PLAN_PIXELS {
        return Err(
            "healing components exceed the shared work limit; heal a smaller area or fewer separate parts"
                .into(),
        );
    }
    let mut completed = 0u64;
    for (part, (x, y, w, h)) in parts.iter().zip(windows) {
        job.check()?;
        let area = u64::from(w) * u64::from(h);
        let component_job = job.range(
            completed as f64 / total.max(1) as f64,
            (completed + area) as f64 / total.max(1) as f64,
        );
        let n = w as usize * h as usize;
        // Row-major, so each window pixel's RGB lands at `i * 3`.
        let mut rgb: Vec<T> = Vec::with_capacity(n * 3);
        let mut region = vec![0u16; n];
        for wy in 0..h {
            for wx in 0..w {
                let i = wy as usize * w as usize + wx as usize;
                job.every(i)?;
                let ci = (y + wy) as usize * canvas.0 as usize + (x + wx) as usize;
                rgb.extend_from_slice(&source[ci][..3]);
            }
        }
        for &ci in &part.pixels {
            let (cx, cy) = (ci % canvas.0, ci / canvas.0);
            region[(cy - y) as usize * w as usize + (cx - x) as usize] = 65535;
        }
        crate::heal_writeback::heal_with_selection(
            canvas,
            dimensions,
            pixels.as_flattened_mut(),
            offset,
            (i64::from(x), i64::from(y), w as usize, h as usize),
            &rgb,
            &region,
            &region,
            opacity,
            selection,
            component_job,
        )?;
        completed += area;
    }
    Ok(())
}
