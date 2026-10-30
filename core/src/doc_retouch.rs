//! Destructive retouch brushes on the layered document: dodge (brighten)
//! and burn (darken).
//!
//! The stroke arrives as the SAME canvas-frame premultiplied RGBA8 overlay
//! the paint pipeline hands `dr_doc_painting_layer` — but here ONLY its
//! alpha channel is read, as per-pixel stroke coverage (0..=255). The tone
//! math runs on the layer's STRAIGHT (non-premultiplied) color channels;
//! layer alpha is never touched.

use crate::doc::DrDocument;
use crate::doc_lock::EditKind;
use crate::doc_paint::overlay_rect;

/// One channel of dodge/burn in unit range: maps `v` in [0, 1] to its
/// fully-exposed value for `range` (0 shadows, 1 midtones, 2 highlights).
///
/// These curves are our own shaping, not a GIMP port, and every one of them
/// is MONOTONE in `v` — the property that matters most for a tone tool. An
/// earlier polynomial-weight family (`v + e*w(v)*(1-v)` with banded `w`)
/// inverted tones inside its band above exposure 1/3 (its slope at the band
/// end is `1 - 3e`), which reads as solarization. The replacements:
///
/// - midtones: classic gamma — dodge `v^(1/(1+e))`, burn `v^(1+e)`;
///   strictly monotone, largest effect at mid grey, endpoints fixed.
/// - shadows: a black-point move by `k = e/3` — dodge lifts it
///   (`k + v*(1-k)`), burn crushes it (`max(0, (v-k)/(1-k))`); linear, so
///   monotone, with the absolute effect largest in the shadows and white
///   fixed. The burn clamp is a plateau at 0, never an inversion.
/// - highlights: the mirrored white-point move — dodge `min(1, v/(1-k))`,
///   burn `v*(1-k)`; effect largest in the highlights and black fixed.
///
/// `k < 1` always (`e <= 1`), so the divisions are safe; full exposure
/// moves an endpoint by a third. All are the identity at exposure 0 and
/// stay in [0, 1] by construction, so no clamp is needed before
/// quantization.
pub(crate) fn shaped(v: f32, e: f32, range: u8, burn: bool) -> f32 {
    let k = e / 3.0;
    match (range, burn) {
        (0, false) => k + v * (1.0 - k),
        (0, true) => ((v - k) / (1.0 - k)).max(0.0),
        (1, false) => v.powf(1.0 / (1.0 + e)),
        (1, true) => v.powf(1.0 + e),
        (2, false) => (v / (1.0 - k)).min(1.0),
        (2, true) => v * (1.0 - k),
        _ => unreachable!("range is validated by dodge_burn_layer"),
    }
}

/// Protect Tones (#1): the fully exposed values of one channel GROUP — an
/// RGB pixel's [R, G, B], or one of a CMYK document's two `L` passes — moved
/// together so the group keeps its hue and saturation. With `V` the group's
/// largest value, every value is scaled by `shaped(V) / V`, so the brightest
/// channel takes the plain curve and the others keep their ratios to it; a
/// group at `V = 0` (black) takes `shaped(0)` on each value. Every result
/// stays in [0, 1]: each value is at most `V`, so at most `shaped(V)`. A group
/// of one value, or of equal values, is the plain curve — so a single plane
/// and the CMYK K pass `[L_k; 3]` are unchanged by it. `values` are
/// overwritten with their targets; the caller's coverage lerp follows.
pub(crate) fn protected(values: &mut [f32], e: f32, range: u8, burn: bool) {
    let v = values.iter().copied().fold(0.0f32, f32::max);
    if v <= 0.0 {
        let floor = shaped(0.0, e, range, burn);
        values.iter_mut().for_each(|value| *value = floor);
        return;
    }
    let scale = shaped(v, e, range, burn) / v;
    values.iter_mut().for_each(|value| *value *= scale);
}

impl DrDocument {
    /// Dodges (brightens, `burn` false) or burns (darkens, `burn` true)
    /// layer `idx` where a stroke overlay covers it.
    ///
    /// `overlay` is the SAME canvas-frame premultiplied RGBA8 buffer
    /// `dr_doc_painting_layer` takes (`w`/`h` must equal the canvas size);
    /// ONLY its alpha channel is read, as per-pixel stroke coverage
    /// 0..=255. `exposure` is clamped to [0, 1]; `range` selects the tonal
    /// band (0 shadows, 1 midtones, 2 highlights — see [`shaped`]). The
    /// overlay is mapped through the layer's offset exactly as
    /// `painting_layer`: overlay outside the layer's extent is ignored and
    /// the layer does not grow. Layer alpha is never touched, and fully
    /// transparent pixels are skipped (their RGB is latent garbage, not
    /// tone). `protect_tones` moves the three channels together through
    /// [`protected`] instead of each through [`shaped`]; off, the bytes are
    /// the curve's per channel, as before #1.
    ///
    /// Domain refusals (`None`): `idx` out of range; `w`/`h` not the canvas
    /// size; `overlay` shorter than `w*h*4`; non-finite `exposure`;
    /// `range > 2`; a layer extent that misses the canvas; or no pixel
    /// actually changing (exposure 0, zero coverage, dodging pure white) —
    /// an identical copy would register a phantom undo step in the app — or a
    /// GROUP index, which has no pixels of its own. A PIXELS edit under
    /// `doc_lock`.
    // The parameter list deliberately mirrors `dr_doc_dodge_burn_layer`'s C
    // signature one-for-one; bundling them into a struct would only move
    // the count somewhere else.
    #[allow(clippy::too_many_arguments)]
    pub fn dodge_burn_layer(
        &self,
        idx: usize,
        overlay: &[u8],
        w: u32,
        h: u32,
        exposure: f32,
        range: u8,
        burn: bool,
        protect_tones: bool,
    ) -> Option<Self> {
        self.under_locks(idx, EditKind::Pixels, |doc| {
            doc.dodge_burn_layer_unlocked(idx, overlay, w, h, exposure, range, burn, protect_tones)
        })
    }

    /// The body of [`Self::dodge_burn_layer`], outside the lock gate. Split
    /// out only so the gate is one line; that method is its only caller.
    #[allow(clippy::too_many_arguments)]
    fn dodge_burn_layer_unlocked(
        &self,
        idx: usize,
        overlay: &[u8],
        w: u32,
        h: u32,
        exposure: f32,
        range: u8,
        burn: bool,
        protect_tones: bool,
    ) -> Option<Self> {
        let layer = self.raster_layer(idx)?;
        if w != self.width || h != self.height || !exposure.is_finite() || range > 2 {
            return None;
        }
        let expected = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
        if overlay.len() < expected {
            return None;
        }
        let e = exposure.clamp(0.0, 1.0);
        // Canvas-frame overlay -> layer pixels: the same offset mapping as
        // `DrDocument::painting_layer` in `doc.rs` (the master copy of this
        // intersection), bounded by the overlay's nonzero rect on the layer
        // (`doc_paint::overlay_rect`, 13B-2c-2a) — a superset of the pixels
        // whose alpha coverage is nonzero. `None` when the overlay cannot
        // reach any layer pixel.
        let (off_x, off_y) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
        let (rx, ry, rw, rh) = overlay_rect(overlay, (self.width, self.height), layer)?;
        let mut px = layer.pixels.read_rect(rx, ry, rw, rh)?.into_flattened();
        let raw: &mut [u8] = &mut px;
        let mut changed = false;
        for row in 0..rh as usize {
            let ly = i64::from(ry) + row as i64;
            for column in 0..rw as usize {
                let lx = i64::from(rx) + column as i64;
                let cx = (lx + off_x) as u64;
                let cy = (ly + off_y) as u64;
                let si = ((cy * u64::from(self.width) + cx) * 4) as usize;
                let coverage = overlay[si + 3];
                if coverage == 0 {
                    continue;
                }
                let di = (row * rw as usize + column) * 4;
                if raw[di + 3] == 0 {
                    continue;
                }
                let c = f32::from(coverage) / 255.0;
                let mut targets: [f32; 3] =
                    std::array::from_fn(|ch| f32::from(raw[di + ch]) / 255.0);
                if protect_tones {
                    protected(&mut targets, e, range, burn);
                } else {
                    targets = targets.map(|v| shaped(v, e, range, burn));
                }
                for (ch, &target) in targets.iter().enumerate() {
                    let old = raw[di + ch];
                    let v = f32::from(old) / 255.0;
                    // Coverage blends toward the shaped value; round
                    // half-up back to a byte.
                    let out = v + c * (target - v);
                    let new = (out * 255.0 + 0.5).floor() as u8;
                    if new != old {
                        raw[di + ch] = new;
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            // Identity result: refuse rather than mint an unchanged copy.
            return None;
        }
        let pixels = layer
            .pixels
            .with_rect(rx, ry, rw, rh, px.as_chunks::<4>().0)
            .ok()??;
        self.with_layer_raster(idx, pixels)
    }
}
