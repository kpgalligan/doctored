//! The byte STOPS gradient (`dr_doc_gradient_stops`): 2..32 stops, each
//! with its own colour and alpha, in one of five shapes over the drag.
//!
//! It is `doc_select`'s two-colour `gradient` walk — the same lock gate, the
//! same selection coverage (`mask_coverage`), the same straight source-over
//! with its frozen-alpha rule (`over_straight`) — with the ramp parameter
//! from `gradient_field::styled_parameter` and the colour from
//! `gradient_field::stop_color` (the Classic ramp for Linear, Angle,
//! Reflected and Radial, blended with the linear one by the tool's
//! Smoothness `k` through `gradient_field::smooth_color`, the fill layers'
//! sampler). The two-colour op and `dr_doc_gradient` keep
//! their f32 arithmetic; their linear and radial drags read the same Classic
//! ramp (`gradient_field::classic_two`).
//!
//! A blend mode other than Normal composites each stop colour through
//! the shared W3C step (`blend::composite_source_into`, or
//! `composite_source_frozen` on a frozen alpha), the statements
//! `doc_paint::paint_walk` runs for a blended fill; Normal keeps
//! `over_straight` byte for byte.

use crate::blend::{blend_kind, composite_source_frozen, composite_source_into, BlendMode};
use crate::doc::DrDocument;
use crate::doc_lock::EditKind;
use crate::doc_select::{mask_coverage, over_straight};
use crate::gradient_field::{
    styled_parameter, validate, validate_smoothness, validate_stops, FieldStop, FieldStyle,
};
use crate::style_json::GRADIENT_STOPS;

impl DrDocument {
    /// Paints a stops gradient onto layer `idx` through `mode` (the whole
    /// layer, scaled by `mask` where given) along the drag `p0 → p1` in
    /// `style`. Stop colours are straight RGBA in 0..1 and interpolate
    /// straight, each segment through its left stop's colour and opacity
    /// midpoints (`gradient_field::stop_color`), the Classic ramp
    /// blended with the linear one by `smoothness` (a fraction, 1 = 100 %,
    /// today's ramp exactly). `None` for a stop count outside 2..=32, a stop
    /// out of range or out of order, a midpoint outside 0.05..=0.95, a
    /// smoothness not finite in 0..=1 (`gradient_field::validate_smoothness`),
    /// `p0 == p1` or a coordinate that is not finite, a
    /// group-only mode (Pass Through), a mask that is not canvas-sized, a
    /// group or out-of-range index, a pixel lock, or a gradient that changes
    /// no pixel.
    #[allow(clippy::too_many_arguments)]
    pub fn gradient_stops(
        &self,
        idx: usize,
        p0: (f32, f32),
        p1: (f32, f32),
        stops: &[FieldStop],
        style: FieldStyle,
        smoothness: f32,
        mode: BlendMode,
        mask: Option<&[u8]>,
    ) -> Option<Self> {
        self.under_locks(idx, EditKind::Pixels, |doc| {
            doc.gradient_stops_unlocked(idx, p0, p1, stops, style, smoothness, mode, mask)
        })
    }

    /// The body of [`Self::gradient_stops`], outside the lock gate.
    #[allow(clippy::too_many_arguments)]
    fn gradient_stops_unlocked(
        &self,
        idx: usize,
        p0: (f32, f32),
        p1: (f32, f32),
        stops: &[FieldStop],
        style: FieldStyle,
        smoothness: f32,
        mode: BlendMode,
        mask: Option<&[u8]>,
    ) -> Option<Self> {
        if mask.is_some_and(|m| m.len() != self.width as usize * self.height as usize)
            || mode.is_group_only()
        {
            return None;
        }
        let from = [f64::from(p0.0), f64::from(p0.1)];
        let to = [f64::from(p1.0), f64::from(p1.1)];
        if !GRADIENT_STOPS.contains(&stops.len())
            || validate_stops(stops).is_err()
            || validate_smoothness(smoothness).is_err()
            || stops
                .iter()
                .any(|s| s.color.iter().any(|v| !(0.0..=1.0).contains(v)))
            || validate(from, to).is_err()
        {
            return None;
        }
        let layer = self.raster_layer(idx)?;
        let (off_x, off_y) = layer.offset;
        let frozen = self.alpha_frozen(idx);
        let kind = blend_kind(mode);
        // With the alpha frozen a blended stop writes RGB only
        // (`doc_paint::paint_walk`'s rule).
        let channels = if frozen { 3 } else { 4 };
        let mut pixels = layer.pixels.to_contiguous().ok()?;
        let mut changed = false;
        for (px_x, px_y, px) in pixels.enumerate_pixels_mut() {
            let coverage = mask_coverage(mask, self.width, self.height, off_x, off_y, px_x, px_y);
            if coverage == 0 {
                continue;
            }
            // Pixel centre in canvas coordinates.
            let point = [
                f64::from(px_x) + f64::from(off_x) + 0.5,
                f64::from(px_y) + f64::from(off_y) + 0.5,
            ];
            let t = styled_parameter(point, from, to, style) as f32;
            let mut paint = crate::gradient_field::stop_color(stops, t, style, smoothness);
            paint[3] *= f32::from(coverage) / 255.0;
            let before = px.0;
            if mode == BlendMode::Normal {
                over_straight(paint, px, frozen);
            } else if paint[3] > 0.0 {
                let mut acc = px.0.map(|v| f32::from(v) / 255.0);
                let rgb = [paint[0], paint[1], paint[2]];
                // Canvas-absolute, so Dissolve's dither is the projection's.
                let xy = (
                    i64::from(px_x) + i64::from(off_x),
                    i64::from(px_y) + i64::from(off_y),
                );
                if frozen {
                    composite_source_frozen(&mut acc, rgb, paint[3], kind, xy);
                } else {
                    composite_source_into(
                        std::slice::from_mut(&mut acc),
                        0,
                        rgb,
                        paint[3],
                        kind,
                        xy,
                    );
                }
                for (byte, value) in px.0.iter_mut().zip(acc).take(channels) {
                    *byte = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            }
            changed |= px.0 != before;
        }
        if !changed {
            return None;
        }
        self.with_layer_pixels(idx, pixels)
    }
}
