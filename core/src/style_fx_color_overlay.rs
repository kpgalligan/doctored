//! Color overlay (Layer Style > Color Overlay): the shape filled
//! with one colour ABOVE the pixels, composited with the effect's OWN blend
//! mode and opacity — never the layer's ("Blend Interior Effects as Group"
//! is off by default). That is why a Multiply text layer with a
//! white Normal overlay renders white: this is how everyone recolours text.
//!
//! The overlay is a function of the SHAPE alone. Its coverage plane is the
//! shape's coverage byte for byte — no blur, no growth, no shift, and no
//! reach of its own (it contributes 0 to `LayerStyle::pad()`) — so an
//! anti-aliased edge or a half-alpha pixel carries exactly its own alpha
//! into the overlay and the colour never spills outside the pixels. The
//! Interior invariant `coverage <= shape.coverage` therefore holds trivially
//! (equality, per pixel), which is what lets `style_composite` draw the
//! contribution straight onto the accumulator. Fill opacity never enters:
//! the overlay is drawn after the pixels, so at 0 % fill the shape IS the
//! overlay colour (the "shape at 0 % fill + overlay" look).
//!
//! Nothing is rendered at opacity 0 (the parser clamps opacity to `[0, 1]`
//! and never produces a NaN, but the check is cheap and keeps the module
//! total on every input) or over a fully transparent shape: an empty layer
//! carrying a pasted style would otherwise cost the compositor one pass
//! over a plane that draws nothing, on every frame, for as long as the
//! layer stays empty.

use crate::job_control::JobContext;
use crate::style::{ColorOverlay, EffectKind};
use crate::style_render::{slot_color, Contribution, Placement, RenderContext, Shape};

/// Renders `fx` over `shape`: one Interior contribution whose coverage is
/// the shape itself (solid colour, the effect's blend and opacity, no
/// shift, no knock-out). Empty at opacity 0 or over a shape with no
/// coverage. The render context is unused — the overlay reads neither the
/// global light nor the canvas (it is never canvas-aligned). Polls `job`
/// before the all-zero scan and the clone (13A-5b-i).
pub(crate) fn render(
    fx: &ColorOverlay,
    shape: &Shape,
    _ctx: &RenderContext,
    job: JobContext<'_>,
) -> Result<Vec<Contribution>, String> {
    if fx.skipped_by_settings() {
        return Ok(Vec::new());
    }
    job.check()?;
    if shape.coverage.iter().all(|&c| c == 0) {
        return Ok(Vec::new());
    }
    job.check()?;
    Ok(vec![Contribution::solid(
        EffectKind::ColorOverlay,
        shape.coverage.clone(),
        slot_color(fx.color, fx.linear),
        fx.blend,
        fx.opacity,
        Placement::Interior,
    )])
}
