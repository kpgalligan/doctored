//! KERNEL: the Layer Style Stroke's Fill Type Gradient options that only the
//! Stroke has: the SHAPE BURST ramp across the band, and the
//! box a Gradient Style spans on a stroke.
//!
//! # Shape Burst
//!
//! A Shape Burst stroke's `t` follows the band's own distance across its
//! width, never the box or the angle. `D` is the 8-neighbour chamfer
//! (`style_stroke_distance::contour_distance`, steps 1 and √2) from the
//! pixel to the nearest pixel on the OTHER side of the coverage ≥ 128
//! contour, so a pixel next to the contour reads 1 (why the 3 × 3 measure
//! and not the band's 5 × 5 one: `style_stroke_distance`, "The contour
//! distance"); each pixel lies on exactly one side. `t` runs from 0 at the
//! band's OUTER edge to 1 at its INNER edge:
//!
//! - Outside: `t = 1 − D / (size + 1)` on the outside pixels;
//! - Inside: `t = D / (size + 1)` on the inside pixels;
//! - Center: ONE ramp across the whole band, `(size / 2 + 1 − D) / (size +
//!   2)` outside and `(D + size / 2) / (size + 2)` inside, continuous across
//!   the contour;
//!
//! with `D` clamped to `[0, size + 1]` first. Scale does not change it. The
//! colour is then the Classic ramp of `t` (`GradientSampler::classic`, which
//! is true for a Shape Burst fill with a field), at every depth and in CMYK
//! ink through the one renderer.
//!
//! Declared, not measured (no Photoshop row covers them): a band pixel
//! beyond the law's reach, and a plane whose other side is empty (`D`
//! `+inf`), read the clamped end (Outside and Center-outside `t = 0`, Inside
//! and Center-inside `t = 1`); a pixel on the side a position does not
//! measure reads `D = 0` (Outside gives the inside pixels `t = 1`, Inside
//! gives the outside pixels `t = 0`). The band's coverage keeps the seeded
//! 5 × 5 chamfer (`style_stroke_distance`, #578), so a shape with no pixel
//! at coverage ≥ 128 (alpha 102 everywhere) still draws its Outside band, in
//! the first stop's colour. The output is finite everywhere.
//!
//! Why: `scripts/photoshop-probe/gradient_identify_673_fit.py
//! --judge-phase3` on take 2 of the gradient identification (Photoshop
//! 26.10.0; 108 Shape Burst rows, Outside, Inside and Center at Size 4, 16
//! and 32 on a hard rect, a disc and an anti-aliased disc, Smoothness 100
//! and 0, 8 and 16 bits). The law has no free number; it ships by rule (ii):
//! every existing `stroke-fill-557` Shape Burst row fell and none rose
//! (`sb_o` 4.69 / 2.88 → 1.44 / 0.78 codes, `sb_i` 4.69 / 2.74 → 1.44 /
//! 0.68, `sb_c` 17.19 / 14.33 → 1.21 / 0.50, `sb_os` at Scale 50 as `sb_o`,
//! `sb_o` at 16 bits 5.14 / 2.85 → 1.39 / 0.69). 80 of take 2's 108 rows
//! meet 2 / 0.5; the rest miss on the mean (0.52 to 0.63) at Size 32 and
//! at Size 16 Center at 16 bits, and three Size 32 Inside disc rows also
//! miss on the max (2.40 to 3 codes). The rule it replaces
//! (`stroke_fill_take.py`'s EDGE-OUT, `D − 0.5`, `size`, each Center half
//! over its own `size / 2`) was a linear-ramp fit to this law under a
//! Classic ramp.
//!
//! The field is built ONCE per stroke ([`burst_field`]) and shared by its
//! contributions, so Center's two halves read one `u16` plane (`2·P` bytes,
//! `precision_style_render::style_scratch`'s term). It is stored as
//! `t × 65535` per plane pixel; the sampler reads it at the pixel
//! (`style_gradient::BurstField`). Each distance plane is dropped before the
//! next is built, so Center holds one `f32` plane at a time.
//!
//! # The box
//!
//! A Gradient Style other than Shape Burst spans [`stroke_box`]: with Align
//! with Layer, the layer's content bounds GROWN by the band's outward reach
//! (`size` Outside, `size / 2` Center, 0 Inside); without it the canvas, as
//! the Gradient Overlay does. The same take's linear and radial rows chose
//! the grown box (BAND, worst-row mean 10.3 codes, against 33.1 for the
//! content bounds — the clamp past the bounds' ends decides it; the rest of
//! the residual is the smoothed ramp again).

use std::sync::Arc;

use crate::job_control::JobContext;
use crate::style::{GradientFill, GradientStyle, Stroke, StrokeFill, StrokePosition};
use crate::style_gradient::{gradient_box, BurstField};
use crate::style_render::{RenderContext, Shape};
use crate::style_stroke_distance::contour_distance;

/// The Shape Burst `t × 65535` of every plane pixel of a `w × h` `coverage`
/// plane for a stroke at `position` and `size` (module doc). Polls `job` once
/// per row of each pass. A plane that is not `w × h`, or a non-finite or
/// non-positive `size`, reads 0 everywhere. `_scale` is the gradient's Scale,
/// unused: the take's IGNORED.
pub(crate) fn burst_t(
    coverage: &[u8],
    w: u32,
    h: u32,
    position: StrokePosition,
    size: f32,
    _scale: f32,
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    job.check()?;
    let (wu, hu) = (w as usize, h as usize);
    let mut out = vec![0u16; wu * hu];
    if coverage.len() != wu * hu || !size.is_finite() || size <= 0.0 {
        return Ok(out);
    }
    let code = |t: f32| -> u16 { (t.clamp(0.0, 1.0) * 65535.0).round() as u16 };
    // The law (module doc), `D` clamped first so `+inf` reads the end.
    let law = |d: f32, inside: bool| -> f32 {
        let d = d.clamp(0.0, size + 1.0);
        match position {
            StrokePosition::Outside => 1.0 - d / (size + 1.0),
            StrokePosition::Inside => d / (size + 1.0),
            StrokePosition::Center if inside => (d + size / 2.0) / (size + 2.0),
            StrokePosition::Center => (size / 2.0 + 1.0 - d) / (size + 2.0),
        }
    };
    // The outside pixels measure to the inside, then the inside pixels to
    // the outside; a side the position does not measure reads `D = 0`.
    // One distance plane at a time.
    for to_inside in [true, false] {
        let measured = if to_inside {
            position != StrokePosition::Inside
        } else {
            position != StrokePosition::Outside
        };
        let d = if measured {
            Some(contour_distance(coverage, wu, hu, to_inside, job)?)
        } else {
            None
        };
        job.check()?;
        for (i, (slot, &c)) in out.iter_mut().zip(coverage).enumerate() {
            if (c < 128) != to_inside {
                continue;
            }
            let di = d.as_ref().map_or(0.0, |d| d[i]);
            *slot = code(law(di, !to_inside));
        }
    }
    Ok(out)
}

/// The Shape Burst field of `fx` over `shape` (module doc), built once per
/// stroke; `None` unless `fx` fills with a Shape Burst gradient.
pub(crate) fn burst_field(
    fx: &Stroke,
    shape: &Shape,
    job: JobContext<'_>,
) -> Result<Option<Arc<BurstField>>, String> {
    if fx.fill_type != StrokeFill::Gradient || fx.gradient.style != GradientStyle::ShapeBurst {
        return Ok(None);
    }
    let t = burst_t(
        &shape.coverage,
        shape.w,
        shape.h,
        fx.position,
        fx.size,
        fx.gradient.scale,
        job,
    )?;
    Ok(Some(Arc::new(BurstField {
        origin: shape.origin,
        w: shape.w,
        h: shape.h,
        t: Arc::from(t),
    })))
}

/// The box a stroke's gradient spans (module doc): `gradient_box` (the ONE
/// mapping the Gradient Overlay shares), grown by the band's outward reach
/// when the fill is aligned with the layer.
pub(crate) fn stroke_box(
    fill: &GradientFill,
    fx: &Stroke,
    shape: &Shape,
    ctx: &RenderContext,
) -> (f32, f32, f32, f32) {
    let (bx, by, bw, bh) = gradient_box(fill, shape, ctx);
    if !fill.align_with_layer || !fx.size.is_finite() || fx.size <= 0.0 {
        return (bx, by, bw, bh);
    }
    let reach = match fx.position {
        StrokePosition::Outside => fx.size,
        StrokePosition::Center => fx.size / 2.0,
        StrokePosition::Inside => 0.0,
    };
    (bx - reach, by - reach, bw + 2.0 * reach, bh + 2.0 * reach)
}
