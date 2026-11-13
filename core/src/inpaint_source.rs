//! Cancellable sampling-source preparation for every retouch gesture.
//!
//! This is the ONE place the core builds a sampling plate — the Clone Stamp's
//! and the Healing Brush's as much as the Spot Healing Brush's and
//! Content-Aware Fill's, and the 16-bit promotion's. WHICH pixels a mode
//! samples is `retouch_sampling`'s question; getting them within a declared
//! allocation, tiled where the stack allows it and cancellable throughout, is
//! this file's.
//!
//! **Admission (13A-5c-2a).** A cut stack takes bounded tiles (≤ 1024 × 64
//! accumulator pixels) through the existing compositor when every
//! contributing entry is a depth-0, unstyled raster whose metadata is absent,
//! a POINTWISE adjustment, or opaque. That admits:
//!
//! * every mask form — feathered, density, a mask offset or size that
//!   differs from its layer's, outside coverage 255. The aligned plane
//!   (`Layer::aligned_mask_with_job`) is layer-anchored and cached, so a
//!   tile's crop of it is the full frame's crop, byte for byte;
//! * a pointwise adjustment (`Adjustment::is_pointwise`): it reads one
//!   backdrop sample per output sample, so a tile's accumulator holds every
//!   sample it reads;
//! * metadata `Adjustment::from_meta` rejects: `composite_layer_into` never
//!   interprets it (`doc.rs`), so the entry composites as the raster it is;
//! * clipping, blend modes, the profile and canvas-relative Dissolve, as
//!   before.
//!
//! **Tree (13A-5c-2b).** A cut stack holding groups or layer styles, whose
//! contributing non-group entries carry no guide-reading Shadows/Highlights,
//! takes the byte REGIONAL tree (`doc_regional_plate::regional_cut_plate`):
//! admitted once over the canvas with the cut, resolved once over the canvas
//! clip (D5 / Shallow snapshots per sample, materialization over the canvas
//! clip, the group budget charged once — the reference's own rules), then
//! sampled in ≤ 1024 × 64 tiles. Byte for byte `flattened_cut`.
//!
//! **FullFrame.** A guided Shadows/Highlights (the regional tree's one
//! STAGED operator: its guide is built from the whole accumulator it is
//! handed) keeps the full-frame fallback, `flattened_cut_with_job` (owner
//! 13B). [`plate_arm`] decides the three arms in one pass over the cut stack,
//! parsing each contributing entry's metadata once and polling the job before
//! each parse. The plan-time [`pointwise`] parses nothing — any metadata
//! answers "full frame" there, which only over-reserves.
//!
//! **Reservation.** On top of the caller's `base`: the tiled path reserves
//! [`tile_scratch_bytes`] (the effective-mask derivations the compositor may
//! build, `projection_scratch_bytes`' own per-layer mask term over the cut
//! stack); the tree reserves `20 · tile_pixels` (the f32 tile accumulator and
//! its RGBA8 encode) plus the regional admission's preparation scratch (masks,
//! style planes, materializations, one description parse); the fallback
//! reserves `projection_scratch_bytes` less the plate `base` already counts.
//! The tiled path and the fallback add `context_scratch`, the
//! description-context parse's peak (0 under a modelled profile); the tree's
//! preparation already holds it.
//!
//! **Cancellation — the declared bounds.** Every arm runs under
//! `control.checkpoints_only()`, so the plate still reports no progress.
//! Tiled: `T_f + c_tile_row` — a cancel returns within one row of ONE layer's
//! tile composite (≤ 1024 samples), since the tile loop, the context parse and
//! every compositor row poll the job (unchanged). Tree: `T_f + c_tree_row` —
//! one full tile row of ROOT samples through the WHOLE tree (one run-major
//! run, ≤ 1024 samples), polled at every tile row's head and per encode row; its
//! preparation runs under the regional stages' own measured bounds (mask blur
//! 13A-5a, style planes 13A-5b-i, materialization chain 13A-5b-ii-a/b,
//! description parse 13A-5c-1). Fallback: `T_f + c_row` — one row of the
//! running chain primitive (`regional.rs`'s model; the per-row polls of
//! 13A-5b-ii-a/b through `flattened_cut_with_job`, whose context parse, mask
//! blur, group chain and encode all poll the job; unchanged). The predicate
//! pass itself: `T_f + c_parse` (13A-5c-1's model, one meta parse); the
//! `from_meta` transient stays uncharged (13A-5c-3's row).
//!
//! THE sampling-plate builder, shared by every retouch gesture (clone, heal, spot heal, patch,
//! content-aware fill) and by the 16-bit promotion, over THREE arms chosen by `plate_arm` (one pass
//! over the CUT stack, one metadata parse per contributing entry, polled before each). `Tiles`:
//! cancellable raw-layer placement and 64K-pixel tiles through the existing compositor for depth-0
//! unstyled raster stacks with ANY mask form (the aligned plane is layer-anchored and cached),
//! pointwise adjustments and opaque metadata (13A-5c-2a), bound `T_f + c_tile_row`. `FullFrame`:
//! only a guided Shadows/Highlights keeps `flattened_cut_with_job`, which runs under the job and
//! cancels per row, bound `T_f + c_row` (owner 13B). Tiles and FullFrame reserve `context_scratch`
//! beside their render term (`tile_scratch_bytes` / `projection_scratch_bytes`), the tree's
//! preparation holds it, and `promote_flattened_plan` covers all three (its non-cooperative arm
//! adds `(20·tile − 16·n)⁺`). Asking over the CUT stack is what lets ignoring adjustments reach a
//! cheaper arm instead of being forced full-frame by the entry it is cutting.
//! `DrDocument::retouch_plate_full_frame` (`retouch_sampling.rs`, `#[doc(hidden)]`) is the
//! full-frame reference seam the cut-plate tests compare against.

use crate::{
    adjust::Adjustment,
    adjust_calibrated::{context_scratch, validate_context_with_job},
    doc::{DrDocument, Layer, LayerKind},
    doc_group,
    job_control::JobContext,
    retouch_sampling::{cut_of, RetouchSampling},
    style_composite::Cut,
};
use image::RgbaImage;

/// The admission shared by both predicates: a depth-0 unstyled raster whose
/// metadata `meta_admits`. No mask clause — every mask form tiles (module doc).
fn tile_safe(layer: &Layer, meta_admits: fn(&str) -> bool) -> bool {
    layer.kind == LayerKind::Raster
        && layer.depth == 0
        && layer.style.is_none()
        && layer.meta.as_deref().is_none_or(meta_admits)
}

/// PLAN-time admission (`promote_flattened_plan`): parses nothing, so a
/// layer carrying any metadata answers `false`. It may answer full-frame where
/// the render tiles; that only over-reserves.
pub(crate) fn pointwise(doc: &DrDocument) -> bool {
    doc.layers.iter().all(|layer| tile_safe(layer, |_| false))
}

/// The route a sampling plate takes (module doc).
pub(crate) enum PlateArm {
    /// Bounded tiles through the existing compositor (13A-5c-2a).
    Tiles,
    /// The byte regional tree, sampled per tile (13A-5c-2b).
    Tree,
    /// The full-frame fallback, `flattened_cut_with_job`.
    FullFrame,
}

/// RENDER-time admission over a cut stack. A CUT entry contributes nothing, so
/// its metadata, style, depth and mask cannot make any rectangle differ from
/// the crop of the cut projection — and asking the question this way round is
/// what lets `ignore_adjustments` actually reach the tiled path, instead of
/// being forced full-frame by the very adjustment it is cutting.
///
/// `Tiles` when `canvas` has a zero side or every non-cut entry is
/// [`tile_safe`] with metadata that is no adjustment at all or a pointwise
/// one (13A-5c-2a's `pointwise_cut`); else `Tree` when every non-cut,
/// visible, non-group entry's metadata is no adjustment or a pointwise one (a
/// guide-reading Shadows/Highlights is the regional tree's ONLY staged
/// operator); else `FullFrame`. One pass, one `Adjustment::from_meta` per
/// contributing entry, `control.check()` before each parse.
pub(crate) fn plate_arm(
    layers: &[Layer],
    cut: Cut<'_>,
    canvas: (u32, u32),
    control: JobContext<'_>,
) -> Result<PlateArm, String> {
    if canvas.0 == 0 || canvas.1 == 0 {
        return Ok(PlateArm::Tiles);
    }
    let (mut tiles, mut tree) = (true, true);
    for (i, layer) in layers.iter().enumerate() {
        if cut.excludes(i) {
            continue;
        }
        if layer.kind == LayerKind::Group {
            // `tile_safe` refuses a group before reading its metadata, and a
            // group's metadata is never interpreted.
            tiles = false;
            continue;
        }
        let pointwise = match layer.meta.as_deref() {
            Some(meta) => {
                control.check()?;
                Adjustment::from_meta(meta).is_none_or(|a| a.is_pointwise())
            }
            None => true,
        };
        tiles = tiles && pointwise && tile_safe(layer, |_| true);
        tree = tree && (!layer.visible || pointwise);
        if !tiles && !tree {
            return Ok(PlateArm::FullFrame);
        }
    }
    Ok(if tiles {
        PlateArm::Tiles
    } else if tree {
        PlateArm::Tree
    } else {
        PlateArm::FullFrame
    })
}

/// The tiled path's reservation: `projection_scratch_bytes`' own per-layer
/// mask term, over the visible, non-cut, mask-enabled entries whose mask needs
/// derivation — the feather's padded blur plane when feathered, plus the
/// layer-sized aligned plane.
pub(crate) fn tile_scratch_bytes(layers: &[Layer], cut: Cut<'_>) -> u64 {
    let mut bytes = 0u64;
    for (i, layer) in layers.iter().enumerate() {
        if !layer.visible || cut.excludes(i) || !layer.mask_enabled {
            continue;
        }
        let Some(mask) = &layer.mask else {
            continue;
        };
        let p = &layer.mask_properties;
        if p.feather > 0.0
            || p.density != 1.0
            || p.offset != layer.offset
            || mask.dimensions() != layer.pixels.dimensions()
        {
            if p.feather > 0.0 {
                bytes = bytes.saturating_add(p.scratch_bytes(mask.dimensions()));
            }
            bytes = bytes
                .saturating_add(u64::from(layer.pixels.width()) * u64::from(layer.pixels.height()));
        }
    }
    bytes
}

/// What [`sampling_source`] reserves ABOVE its caller's `base` for any
/// non-layer-only mode and cut, metadata only — an admission's plate term.
/// Plan-time [`pointwise`] documents tile whatever the cut, so the uncut
/// [`tile_scratch_bytes`] covers the tiled arm (a cut only removes terms).
/// Otherwise `projection_scratch_bytes + (20·tile_pixels − 16n)⁺` covers the
/// full-frame fallback (the projection less the plate) and the Tree arm
/// (`20·tile + preparation`): `DrDocument::promote_flattened_plan`'s
/// non-cooperative render term is the master of that argument, term by term.
/// Both tiled and fallback add `context_scratch`; the tree's preparation
/// already holds it, so adding it there only over-reserves.
pub(crate) fn plate_scratch_bound(doc: &DrDocument) -> Result<u64, String> {
    let context = context_scratch(&doc.layers, &doc.profile)
        .ok_or("layer metadata exceeds the native slot limit")?;
    let arm = if pointwise(doc) {
        tile_scratch_bytes(&doc.layers, Cut::NONE)
    } else {
        let n = u64::from(doc.width) * u64::from(doc.height);
        let tile = crate::doc_regional_plate::tile_pixels((doc.width, doc.height));
        crate::render_scratch::projection_scratch_bytes(doc)
            .saturating_add(tile.saturating_mul(20).saturating_sub(n.saturating_mul(16)))
    };
    Ok(arm.saturating_add(context))
}

/// The canvas-sized straight-RGBA8 plate `mode` names for target `idx`.
///
/// `base` is the caller's already-reserved working set, which the reservation
/// here is measured on top of; `control` is polled per tile, per compositor
/// row, per tile row through the regional tree, per row inside the full-frame
/// fallback and per bounded row copy, so a
/// plate on a large document is as cancellable as the solve that follows it.
///
/// `Ok(None)` only for [`RetouchSampling::Current`] on an index that names no
/// raster layer — a domain refusal, as everywhere else in this area.
pub(crate) fn sampling_source(
    doc: &DrDocument,
    idx: usize,
    mode: RetouchSampling,
    ignore_adjustments: bool,
    base: u64,
    control: JobContext<'_>,
) -> Result<Option<RgbaImage>, String> {
    control.check()?;
    if mode.is_layer_only() {
        control.reserve(base)?;
        return copy_layer(doc, idx, control);
    }
    let excluded = doc.retouch_cut(idx, mode, ignore_adjustments);
    let cut = cut_of(excluded.as_ref());
    let arm = plate_arm(&doc.layers, cut, (doc.width, doc.height), control)?;
    if let PlateArm::Tree = arm {
        return doc
            .regional_cut_plate(cut, doc_group::MAX_GROUP_BUFFER_PIXELS, base, control)
            .map(Some);
    }
    let tiled = matches!(arm, PlateArm::Tiles);
    // The description-context parse's peak (0 under a modelled profile).
    let context = context_scratch(&doc.layers, &doc.profile)
        .ok_or("layer metadata exceeds the native slot limit")?;
    // A cut only ever REMOVES contributors, so the uncut estimate stays a
    // sound upper bound for the cut render; measuring it over the cut set
    // would reserve less, never more.
    let extra = if tiled {
        tile_scratch_bytes(&doc.layers, cut)
    } else {
        crate::render_scratch::projection_scratch_bytes(doc)
            .saturating_sub(u64::from(doc.width) * u64::from(doc.height) * 4)
    }
    .saturating_add(context);
    control.reserve(base.saturating_add(extra))?;
    if !tiled {
        let result = doc.flattened_cut_with_job(
            doc_group::MAX_GROUP_BUFFER_PIXELS,
            cut,
            control.checkpoints_only(),
        )?;
        control.check()?;
        control.reserve(base)?;
        return Ok(Some(result));
    }
    let mut out = RgbaImage::new(doc.width, doc.height);
    let level = doc_group::top_level(&doc.layers);
    // `flattened_cut`'s refusal precedence: the context before the transfer.
    validate_context_with_job(&doc.layers, &doc.profile, control)?;
    // The evaluation policy's transfer and its refusal walk: a sampling
    // plate is the projection minus some contributors (`doc_evaluation`).
    let transfer = doc.transfer()?;
    let env = crate::style_composite::CompositeEnv {
        cut,
        job: control.checkpoints_only(),
        ..doc.composite_env(None, transfer)
    };
    let budget = doc_group::RenderBudget::default();
    //64K accumulator pixels =1MiB, plus at most a clipped unit's working copy.
    // x/y both bounded so panoramic single-row documents retain the same bound.
    for y in (0..doc.height).step_by(64) {
        for x in (0..doc.width).step_by(1024) {
            control.check()?;
            let w = (doc.width - x).min(1024);
            let h = (doc.height - y).min(64);
            let mut acc = vec![[0f32; 4]; w as usize * h as usize];
            doc_group::composite_level_into(
                &mut acc,
                w,
                h,
                (x as i32, y as i32),
                &doc.layers,
                &level,
                env,
                0,
                &budget,
            )?;
            let tile = crate::doc_evaluation::encode_projection(&acc, w, h, transfer);
            for row in 0..h as usize {
                let dest = ((y as usize + row) * doc.width as usize + x as usize) * 4;
                let src = row * w as usize * 4;
                out.as_mut()[dest..dest + w as usize * 4]
                    .copy_from_slice(&tile.as_raw()[src..src + w as usize * 4]);
            }
        }
    }
    control.check()?;
    Ok(Some(out))
}

/// Raw raster placement mirrors layer_canvas_image's coordinate mapping; copy
/// complete visible row slices to avoid iterating through off-canvas storage.
fn copy_layer(
    doc: &DrDocument,
    idx: usize,
    control: JobContext<'_>,
) -> Result<Option<RgbaImage>, String> {
    let Some(layer) = doc.raster_layer(idx) else {
        return Ok(None);
    };
    let mut out = RgbaImage::new(doc.width, doc.height);
    if let Some((x, y, w, h)) = crate::layer_rect::canvas_clip((doc.width, doc.height), layer) {
        let (ox, oy) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
        let stride = doc.width as usize * 4;
        let target: &mut [u8] = &mut out;
        for ly in y..y + h {
            control.check()?;
            let cy = (oy + i64::from(ly)) as usize;
            for (rx, run) in layer.pixels.row_segments(ly, x, x + w) {
                let at = cy * stride + (ox + i64::from(rx)) as usize * 4;
                target[at..at + run.len() * 4].copy_from_slice(run.as_flattened());
            }
        }
    }
    Ok(Some(out))
}
