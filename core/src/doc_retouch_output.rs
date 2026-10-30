//! Retouching onto a NEW layer instead of into the photograph.
//!
//! The repair lands on its own raster entry directly above the layer it was
//! aimed at, so the original pixels stay exactly as they were and the whole
//! gesture remains one pure transition. The Clone Stamp needs almost nothing
//! for this — it composites an overlay, and an overlay composites onto an
//! empty layer as happily as onto a full one. The healing family needs the
//! whole of this file, for one reason:
//!
//! # A membrane solve cannot start from nothing
//!
//! `heal_writeback::heal` reads the destination in THREE roles — the
//! `usable` gate (`alpha > 0`), the contour term `b = destination − source`,
//! and the base of the final lerp `out = old + weight·(healed − old)`. On a
//! brand-new transparent layer all three read zeros: the gate excludes every
//! pixel, so the op refuses; and were the gate merely re-pointed somewhere
//! else, the lerp would still start from zero and every strength below 1
//! would fade the repair toward BLACK.
//!
//! So this file does not re-point anything. It runs the EXISTING op, at the
//! caller's own strength, against a ONE-LAYER TEMPORARY document made from
//! the sampling plate — the picture that is actually there — and then lifts
//! the result onto the new layer. Every cap, every contour rule, every
//! refusal and the whole write-back are the shipped ones, reached through the
//! shipped entry point; nothing in `doc_heal`, `doc_inpaint`,
//! `heal_writeback` or `poisson` changes or needs to. Content-Aware Fill's
//! new-layer arm (`doc_content_aware`) solves on its own temporary plate and
//! lifts its result through the same polled rect deposit, via
//! [`DrDocument::content_aware_fill_deposit`].
//!
//! # Why that composites to the same picture
//!
//! Healing the plate at strength `s` gives `h = plate + s·(f − plate)`, which
//! is the in-place answer with the plate as the destination. Depositing `h`
//! at FULL coverage over a backdrop that IS the plate reproduces `h` exactly
//! — so the strength belongs in the solve and the deposit is opaque, not the
//! other way round. Weighting the deposit by the coverage instead would apply
//! the strength twice and, worse, would fade the repair across the rim band
//! inside the α = 128 contour that `doc_heal`'s hard cut exists to keep at
//! full strength.
//!
//! That identity needs the plate to BE the backdrop beneath the new layer,
//! which is why the caller must hand over the current-and-below plate and why
//! [`DrDocument::retouch_layer_refusal`] turns down a target whose enclosing
//! groups would gate the new layer differently from the canvas-space plate.
//! Without that, the deposit's hard alpha edge shows the difference as a
//! full-amplitude outline in the shape of the stroke — the fringe the hard
//! cut was designed to avoid.
//!
//! # Alpha: the one place this family does author coverage
//!
//! Every other op here promises that layer alpha never moves. A retouch layer
//! is the exception by construction: it is transparent until the repair is
//! put on it, so the deposit writes alpha, and writes it as the PLATE's own
//! alpha at that pixel — the coverage the picture already had there, so a
//! translucent area stays translucent instead of becoming a hard opaque
//! patch. Where the picture is fully opaque, which is every photograph, the
//! composite is the in-place result byte for byte. Where it is not, source
//! over necessarily raises the composite's alpha from `a` to `a·(2 − a)`:
//! no deposited alpha can reproduce an in-place RGB rewrite under a partial
//! backdrop, and putting the repair on its own layer means it has coverage of
//! its own. Stated here rather than discovered later.
//!
//! It runs the SHIPPED heal against a one-layer temporary document made from the sampling plate
//! rather than re-pointing `heal_writeback`, which reads the destination in three roles and would
//! otherwise lerp toward black below full strength. The new layer inherits the target's depth and
//! clipped flag; a group with its own blending, opacity, mask or style is refused with a reason,
//! because the plate already carries that gate. The plate is copied ONCE (into the temp
//! raster, `8·w·h` reserved before the copy, `4·w·h` held under the solve via `with_held_bytes`);
//! the deposit reads alpha from the caller's plate slice. The deposit polls (one domain
//! pass, per-tile reads and write, the lift every 4096) under `deposit_scratch`, which charges
//! rewritten tiles by the tiles the domain touches; the retouch layer is a
//! `LayerRaster::filled` plane (≤ 4 tiles), as is `adding_layer_at`'s. Content-Aware
//! Fill's new-layer arm shares that deposit through `content_aware_fill_deposit` (its own
//! `fill_domain` over the shared `domain_where` pass, `fill_deposit_scratch`, which stays under the
//! fill's admission). `deposit_into` reads only the healed rect and rewrites it in place
//! (the new layer is a transparent `filled` plane, so the output bytes are unchanged): one read per
//! tile and `4·w·h` of scratch fewer.

use crate::doc::{DrDocument, Layer, LayerKind};
use crate::doc_group;
use crate::job_control::JobContext;
use crate::layer_raster::{LayerRaster, ALLOCATION};
use crate::ops::CompositeMode;
use crate::poisson::COVERED_THRESHOLD;
use crate::tile_store::TILE_EDGE;
use image::RgbaImage;

/// The name a retouch layer is created with when the host names none.
pub const RETOUCH_LAYER_NAME: &str = "Retouch";

/// One full RGBA8 tile of a `TilePlane`: `TILE_EDGE² · 4` = 256 KiB.
const TILE_BYTES: u64 = TILE_EDGE as u64 * TILE_EDGE as u64 * 4;

/// The retouch layer's own storage: `TilePlane::filled` shares one tile per
/// tile shape, so at most 4 tiles = 1 MiB whatever the canvas.
const RETOUCH_LAYER_TILE_BYTES: u64 = 4 * TILE_BYTES;

impl DrDocument {
    /// Why `idx` cannot take a retouch layer above it, or `None` when it can.
    ///
    /// The deposit's correctness rests on the plate being the backdrop the new
    /// layer composites onto (module doc). A new entry lands inside the same
    /// group as its target, so each enclosing group must meet the level the
    /// way a canvas-space projection of that level does: pass through, fully
    /// opaque, unmasked, unstyled and unclipped. Anything else gates the new
    /// layer a second time over a plate that already carries the gate.
    ///
    /// A GROUP target is refused outright: a group has no pixels of its own to
    /// retouch, so there is nothing for the repair to sit above.
    pub fn retouch_layer_refusal(&self, idx: usize) -> Option<String> {
        let entry = self.layers.get(idx)?;
        if entry.kind == LayerKind::Group {
            return Some(
                "a group has no pixels of its own to retouch; choose the layer inside it."
                    .to_string(),
            );
        }
        let mut cursor = doc_group::parent(&self.layers, idx);
        while let Some(group) = cursor {
            let ancestor = &self.layers[group];
            if !passes_through(ancestor) {
                return Some(format!(
                    "\"{}\" is inside the group \"{}\", which has its own blending, opacity, mask \
                     or style: a repair on a new layer inside it would be gated twice, once in \
                     the sampled picture and again by the group. Retouch on the layer itself, or \
                     move it out of the group.",
                    entry.name, ancestor.name
                ));
            }
            cursor = doc_group::parent(&self.layers, group);
        }
        None
    }

    /// A transparent canvas-sized raster layer directly above `idx`, at
    /// `idx`'s own depth and inside the same group, carrying `idx`'s `clipped`
    /// flag — without which an unclipped insert above a CLIPPED target would
    /// end that target's clip run and re-base the entries above it onto the
    /// near-empty repair.
    ///
    /// Canvas-sized on purpose: every creation path in the crate makes a
    /// canvas-sized layer, the canvas-frame overlays the paint pipeline hands
    /// around assume it, and an extent-fitted layer would have to GROW the
    /// moment a second stroke crossed its edge — which is a later
    /// assignment's work, not something to depend on here. It costs no
    /// canvas-sized buffer: `adding_layer` builds it over
    /// `LayerRaster::filled`, at most [`RETOUCH_LAYER_TILE_BYTES`].
    ///
    /// `None` for an invalid index or growth past native capacity.
    pub fn retouch_layer_above(&self, idx: usize, name: &str) -> Option<(Self, usize)> {
        let clipped = self.layers.get(idx)?.clipped;
        let at = doc_group::subtree(&self.layers, idx).end;
        let mut doc = self.adding_layer(idx, name)?;
        doc.layers.get_mut(at)?.clipped = clipped;
        Some((doc_group::validated(doc)?, at))
    }

    /// Clone Stamp onto a new retouch layer: the same premultiplied
    /// canvas-frame overlay `dr_doc_painting_layer` takes, composited OVER a
    /// fresh transparent layer above `idx`.
    ///
    /// No plate is needed here — the overlay already carries the sampled
    /// pixels, and an OVER composite onto emptiness is exactly the overlay.
    ///
    /// `Err` with the reason from [`Self::retouch_layer_refusal`]; `Ok(None)`
    /// for the usual domain refusals or an overlay that deposits nothing.
    pub fn clone_to_new_layer(
        &self,
        idx: usize,
        overlay: &[u8],
        w: u32,
        h: u32,
        alpha: f32,
        name: &str,
    ) -> Result<Option<Self>, String> {
        if self.layers.get(idx).is_none() || w != self.width || h != self.height {
            return Ok(None);
        }
        if let Some(message) = self.retouch_layer_refusal(idx) {
            return Err(message);
        }
        // The lock gate belongs to the TARGET, not to the brand-new layer,
        // which carries no locks: retouching onto a new layer must refuse on a
        // locked photograph exactly as retouching into it does.
        if self.lock_block(idx, crate::doc_lock::EditKind::Pixels) != 0 {
            return Ok(None);
        }
        // An OVER composite deposits nothing where the overlay is transparent
        // or the strength is zero, and `painting_layer` would then hand back a
        // layer identical to the empty one it started from — which
        // `with_layer_pixels` accepts, since replacing a buffer with an equal
        // one is a legal edit. Left alone that mints a BLANK layer for a
        // stroke that did nothing, an undo step for no change and an entry the
        // user has to notice and delete. So the emptiness is tested here,
        // where it is one pass over the overlay's alpha, before any layer
        // exists at all.
        if alpha <= 0.0 || !overlay.chunks_exact(4).any(|px| px[3] > 0) {
            return Ok(None);
        }
        let Some((doc, at)) = self.retouch_layer_above(idx, name) else {
            return Ok(None);
        };
        Ok(doc.painting_layer(at, overlay, CompositeMode::Over, alpha))
    }

    /// Healing Brush onto a new retouch layer (module doc): heal the SAMPLING
    /// PLATE at the caller's own strength, then lift the result onto a fresh
    /// transparent layer above `idx`.
    ///
    /// `overlay` is the canvas-frame premultiplied buffer `dr_doc_heal_layer`
    /// takes, unchanged in meaning — alpha is the footprint, RGB the
    /// already-aligned source. `plate` is the canvas-sized STRAIGHT RGBA8
    /// current-and-below sampling plate, which must be the picture the new
    /// layer will sit on; `retouch_source` builds it.
    ///
    /// Every cap, contour rule and refusal is [`DrDocument::heal_layer`]'s,
    /// because that is literally what runs — and its checkpoints and reports
    /// are that call's; its reservations carry the temporary plate raster on
    /// top (`retouch_onto_new_layer`).
    // The parameter list deliberately mirrors `dr_doc_heal_to_new_layer`'s C
    // signature one-for-one; bundling them into a struct would only move the
    // count somewhere else.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn heal_to_new_layer(
        &self,
        idx: usize,
        overlay: &[u8],
        w: u32,
        h: u32,
        strength: f32,
        plate: &[u8],
        name: &str,
        control: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        self.retouch_onto_new_layer(idx, overlay, w, h, plate, name, control, |temp, job| {
            temp.heal_layer(0, overlay, w, h, strength, job)
        })
    }

    /// Spot Healing Brush onto a new retouch layer: the same lift, around
    /// `spot_heal_layer_job`. The plate is BOTH what the inpaint samples and
    /// the illumination it matches, which is what the Spot Healing Brush
    /// means on an empty layer — there is nothing else there to read.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn spot_heal_to_new_layer_job(
        &self,
        idx: usize,
        overlay: &[u8],
        w: u32,
        h: u32,
        strength: f32,
        ring: u32,
        seed: u64,
        kind: crate::spot_heal_type::SpotHealType,
        plate: &[u8],
        name: &str,
        control: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        self.retouch_onto_new_layer(idx, overlay, w, h, plate, name, control, |temp, job| {
            temp.spot_heal_layer_job(
                0,
                overlay,
                w,
                h,
                strength,
                ring,
                seed,
                kind,
                // The temp document IS the plate, one layer and nothing else,
                // so the only honest mode is the one that reads that layer.
                crate::retouch_sampling::RetouchSampling::Current,
                false,
                job,
            )
        })
    }

    /// The shared body: validate, refuse, run `solve` against a one-layer
    /// temporary document made from the plate, lift the result.
    ///
    /// The plate is copied ONCE, into the temporary raster: `8·w·h`
    /// is reserved on `control` before the copy (the owned buffer and the
    /// tiles it is cut into, both live while tiling), and the tiles' `4·w·h`
    /// stay held under every reservation `solve` makes. Alpha for the deposit
    /// is read from the caller's `plate` slice, not from a second copy.
    ///
    /// The deposit polls: one canvas pass builds the domain (every
    /// 4096 pixels), [`deposit_scratch`] is reserved, then the healed rect
    /// read and the write poll once per visited tile and the lift every 4096
    /// rect pixels. The new layer is never read: it is transparent.
    #[allow(clippy::too_many_arguments)]
    fn retouch_onto_new_layer(
        &self,
        idx: usize,
        overlay: &[u8],
        w: u32,
        h: u32,
        plate: &[u8],
        name: &str,
        control: JobContext<'_>,
        solve: impl FnOnce(&DrDocument, JobContext<'_>) -> Result<Option<DrDocument>, String>,
    ) -> Result<Option<Self>, String> {
        if self.layers.get(idx).is_none() || w != self.width || h != self.height {
            return Ok(None);
        }
        let Some(n) = (w as usize).checked_mul(h as usize).filter(|n| *n > 0) else {
            return Ok(None);
        };
        if overlay.len() < n * 4 || plate.len() < n * 4 {
            return Ok(None);
        }
        if let Some(message) = self.retouch_layer_refusal(idx) {
            return Err(message);
        }
        // The lock gate belongs to the TARGET: retouching onto a new layer
        // must refuse on a locked photograph exactly as retouching into it
        // does, even though the layer written is a brand-new unlocked one.
        if self.lock_block(idx, crate::doc_lock::EditKind::Pixels) != 0 {
            return Ok(None);
        }
        let plate_bytes = n as u64 * 4;
        control.reserve(plate_bytes * 2)?;
        let plate = &plate[..n * 4];
        let Some(image) = RgbaImage::from_raw(w, h, plate.to_vec()) else {
            return Ok(None);
        };
        let mut temp = DrDocument::from_raster(LayerRaster::unseeded(image));
        // Nothing in the healing family converts colour — it samples the
        // document's own numbers — but a temporary document that claimed a
        // profile it does not have would be a trap for the next reader.
        temp.profile = self.profile.clone();
        let Some(healed) = solve(&temp, control.with_held_bytes(plate_bytes))? else {
            return Ok(None);
        };
        let Some(healed) = healed.raster_layer(0).map(|l| l.pixels.clone()) else {
            return Ok(None);
        };
        let Some(domain) = deposit_domain(overlay, plate, w, HARD_CUT, control)? else {
            return Ok(None);
        };
        control.reserve(deposit_scratch(n as u64, &domain))?;
        let Some((doc, at)) = self.retouch_layer_above(idx, name) else {
            return Ok(None);
        };
        deposit_into(&doc, at, plate, &healed, &domain, control)
    }

    /// Content-Aware Fill's new-layer deposit: lift `healed` — the
    /// fill's result on its temporary plate document — onto a fresh retouch
    /// layer above `idx` through the same polled rect deposit the healing
    /// family uses, rather than through a canvas-sized lifted image.
    ///
    /// `coverage` is the canvas-sized selection, `plate` the canvas-sized
    /// straight RGBA8 current-and-below plate the fill healed. The domain is
    /// the old lift's: covered, over a plate pixel with colour, and changed.
    /// The deposit writes the healed RGB at the plate's alpha, which is the
    /// healed alpha too, since the write-back never moves alpha.
    ///
    /// `Ok(None)` for a size mismatch or an empty domain; `Err` when the
    /// retouch layer cannot be made. Polls exactly as the heal's deposit
    /// does, after one domain pass; reserves [`fill_deposit_scratch`].
    pub(crate) fn content_aware_fill_deposit(
        &self,
        idx: usize,
        name: &str,
        coverage: &[u8],
        plate: &[u8],
        healed: &LayerRaster,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let (w, h) = (self.width, self.height);
        let n = w as usize * h as usize;
        if n == 0 || coverage.len() != n || plate.len() != n * 4 || healed.dimensions() != (w, h) {
            return Ok(None);
        }
        let Some(domain) = fill_domain(coverage, plate, healed, w, job)? else {
            return Ok(None);
        };
        job.reserve(fill_deposit_scratch(n as u64, &domain))?;
        let (doc, at) = self
            .retouch_layer_above(idx, name)
            .ok_or("content-aware output exceeds native capacity")?;
        deposit_into(&doc, at, plate, healed, &domain, job)
    }
}

/// A group that meets its level the way a plain projection of that level does.
fn passes_through(group: &Layer) -> bool {
    group.blend == crate::blend::BlendMode::PassThrough
        && crate::opacity_byte::quantized(group.opacity) >= 1.0
        && !(group.mask_enabled && group.mask.is_some())
        && group.renders_style().is_none()
        && !group.clipped
        && group.visible
}

/// The canvas pixels a deposit may touch (`mask`), their layer-space
/// bounding box `(x, y, w, h)` and the count of tiles holding at least one.
struct DepositDomain {
    mask: Vec<bool>,
    rect: (u32, u32, u32, u32),
    tiles: u64,
}

/// The canvas pixels a deposit may touch: inside the solve's contour and over
/// a plate pixel that carried colour.
///
/// This is the write-back's own domain, re-derived from the two things the
/// caller already has rather than read back out of it — `region >= 128` is
/// `poisson`'s one 50 % rule, and `plate alpha > 0` is `heal_writeback`'s
/// `usable` gate on a canvas-sized destination at offset (0, 0), where the
/// off-canvas and off-layer halves of that gate cannot fire.
///
/// `overlay` is the premultiplied canvas overlay and `plate` the canvas-sized
/// straight RGBA8 plate, both `width` wide; alpha at `i·4 + 3`. One pass,
/// polling every 4096 pixels, builds the mask, its bounding box and
/// its touched-tile count together. `Ok(None)` when the domain is empty.
fn deposit_domain(
    overlay: &[u8],
    plate: &[u8],
    width: u32,
    threshold: u8,
    job: JobContext<'_>,
) -> Result<Option<DepositDomain>, String> {
    domain_where(plate.len() / 4, width, job, |i| {
        overlay[i * 4 + 3] >= threshold && plate[i * 4 + 3] > 0
    })
}

/// The one domain pass both deposits share: over the `n` canvas pixels of a
/// `width`-wide canvas, polling every 4096 pixels and then asking
/// `inside(i)` once per `i`, in order, it builds the mask, its bounding box
/// and its touched-tile count together. `Ok(None)` when no pixel is inside.
fn domain_where(
    n: usize,
    width: u32,
    job: JobContext<'_>,
    mut inside: impl FnMut(usize) -> bool,
) -> Result<Option<DepositDomain>, String> {
    let (w, edge) = (width as usize, TILE_EDGE as usize);
    let columns = w.div_ceil(edge);
    let mut touched = vec![false; columns * (n / w).div_ceil(edge)];
    let mut mask = Vec::with_capacity(n);
    let mut bounds: Option<(usize, usize, usize, usize)> = None;
    for i in 0..n {
        job.every(i)?;
        let inside = inside(i);
        mask.push(inside);
        if !inside {
            continue;
        }
        let (x, y) = (i % w, i / w);
        touched[(y / edge) * columns + x / edge] = true;
        bounds = Some(match bounds {
            None => (x, y, x, y),
            Some((x0, y0, x1, _)) => (x0.min(x), y0, x1.max(x), y),
        });
    }
    let Some((x0, y0, x1, y1)) = bounds else {
        return Ok(None);
    };
    Ok(Some(DepositDomain {
        mask,
        rect: (
            x0 as u32,
            y0 as u32,
            (x1 - x0 + 1) as u32,
            (y1 - y0 + 1) as u32,
        ),
        tiles: touched.iter().filter(|t| **t).count() as u64,
    }))
}

/// Content-Aware Fill's deposit domain: [`domain_where`] with the old lift's
/// predicate — covered, over a plate pixel with colour, and changed by the
/// fill. `healed`'s next sample is pulled for every `i`, inside or not, so
/// the samples stay in step with the indices.
fn fill_domain(
    coverage: &[u8],
    plate: &[u8],
    healed: &LayerRaster,
    width: u32,
    job: JobContext<'_>,
) -> Result<Option<DepositDomain>, String> {
    let mut samples = healed.runs().flatten();
    domain_where(coverage.len(), width, job, |i| {
        let after = samples.next();
        coverage[i] > 0
            && plate[i * 4 + 3] > 0
            && after.is_some_and(|after| plate[i * 4..i * 4 + 4] != after[..])
    })
}

/// What the fill's deposit holds at its peak: the plate `4n`, the
/// healed raster `4n`, the domain mask `n`, the eligibility mask `n`, the new
/// layer's rewritten tiles (at most `4n`: a canvas plane's clipped tiles
/// total `4n`), the one healed rect read `4·w·h`, rewritten in place,
/// and the filled retouch layer. It
/// never exceeds the fill's admission (`doc_content_aware`'s
/// `# Admission`, at least `23n + 8 MiB`; with `w·h ≤ n` this figure is at
/// most `18n` plus one tile), so under the reservation floor it cannot refuse.
fn fill_deposit_scratch(canvas_pixels: u64, domain: &DepositDomain) -> u64 {
    let (_, _, w, h) = domain.rect;
    canvas_pixels
        .saturating_mul(14)
        .saturating_add(u64::from(w) * u64::from(h) * 4)
        .saturating_add(RETOUCH_LAYER_TILE_BYTES)
}

/// What the deposit holds at its peak, reserved before the retouch
/// layer is made: the temporary plate's tiles `4n` (still held), the domain
/// mask `n`, the one healed rect read `4·w·h` (rewritten in place),
/// the healed raster's and the
/// deposit's rewritten tiles — both only tiles holding a domain pixel, so
/// `2·TILE_BYTES` per touched tile rather than `8n`, which would refuse a
/// scattered stroke the solve admitted — and the retouch layer itself.
fn deposit_scratch(canvas_pixels: u64, domain: &DepositDomain) -> u64 {
    let (_, _, w, h) = domain.rect;
    canvas_pixels
        .saturating_mul(5)
        .saturating_add(u64::from(w) * u64::from(h) * 4)
        .saturating_add(domain.tiles.saturating_mul(2 * TILE_BYTES))
        .saturating_add(RETOUCH_LAYER_TILE_BYTES)
}

/// Lift `healed` onto the retouch layer at `at`: the healed colour at the
/// plate's own alpha inside `domain`, untouched elsewhere.
///
/// Both images are canvas-sized and the retouch layer is a canvas-sized layer
/// at offset (0, 0), so the three indices coincide and no offset mapping is
/// needed — the reason [`DrDocument::retouch_layer_above`] makes a full-canvas
/// layer rather than an extent-fitted one.
///
/// Precondition: layer `at` is [`DrDocument::retouch_layer_above`]'s fresh
/// `LayerRaster::filled(.., [0; 4])` plane (`doc.rs`'s
/// `transparent_canvas_layer`, through `adding_layer`), which both callers —
/// `retouch_onto_new_layer` and [`DrDocument::content_aware_fill_deposit`] —
/// guarantee. So the layer rect is never read: the healed rect is
/// rewritten in place, `[0; 4]` outside the domain, and `with_rect`'s bitwise
/// diff and the output bytes are what the two-read lift gave.
///
/// Polls `job`: once per visited tile of the healed rect read and of
/// the write, and every 4096 pixels of the lift. `Ok(None)` when nothing
/// changed; an allocation failure in the read or the write is an `Err`.
fn deposit_into(
    doc: &DrDocument,
    at: usize,
    plate: &[u8],
    healed: &LayerRaster,
    domain: &DepositDomain,
    job: JobContext<'_>,
) -> Result<Option<DrDocument>, String> {
    let Some(layer) = doc.raster_layer(at) else {
        return Ok(None);
    };
    if layer.pixels.dimensions() != (doc.width, doc.height) || layer.offset != (0, 0) {
        return Ok(None);
    }
    // Only the domain's rect is read and written back (13B-2c-2a).
    let width = doc.width;
    if healed.dimensions() != (width, doc.height) {
        return Ok(None);
    }
    // The rect lies inside the canvas-sized healed plane, so the read's `None`
    // can only be an allocation failure: an error, as the write's is. The
    // layer is never read: it is transparent (the doc's precondition), so the
    // healed rect is rewritten in place into the new rect.
    let (rx, ry, rw, rh) = domain.rect;
    let mut hp = healed
        .read_rect_with_job(rx, ry, rw, rh, job)?
        .ok_or_else(|| ALLOCATION.to_string())?;
    let mut changed = false;
    for row in 0..rh as usize {
        for column in 0..rw as usize {
            job.every(row * rw as usize + column)?;
            let i = (ry as usize + row) * width as usize + rx as usize + column;
            let px = &mut hp[row * rw as usize + column];
            *px = if domain.mask[i] {
                [px[0], px[1], px[2], plate[i * 4 + 3]]
            } else {
                [0; 4]
            };
            changed |= *px != [0; 4];
        }
    }
    // Identity result: refuse rather than mint an unchanged copy, and rather
    // than leave a blank layer behind for a stroke that healed nothing.
    if !changed {
        return Ok(None);
    }
    let Some(pixels) = layer.pixels.with_rect_with_job(rx, ry, rw, rh, &hp, job)? else {
        return Ok(None);
    };
    Ok(doc.with_layer_raster(at, pixels))
}

/// The healing brush's coverage contour, re-exported so the ops beside this
/// file do not each spell the threshold.
pub(crate) const HARD_CUT: u8 = COVERED_THRESHOLD;
