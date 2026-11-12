//! Edit ▸ Fill… / Stroke…, the Brush, Eraser and Gradient tools and the Paint
//! Bucket and Dodge/Burn in a 32-bit float document: the f32 arm of every
//! `NativePaintEdit` kind. The bucket's region and Dodge/Burn's rule are
//! display-defined and come from `float_paint_tone` (R2–R4 of
//! `display_encoding`): the region gates the walk below, and a Dodge/Burn
//! pixel takes `DodgeBurnLinear::mixed` instead of a composite.
//!
//! The walk mirrors `precision_paint_tools`' `applying_unlocked`, the MASTER
//! walk: the same [`PaintFrame`] layout, the same coverage rect
//! (`coverage_rect16`), the same growth rule (a trimmed layer grows to its
//! union with the changed pixels, comps following, never under Preserve
//! Transparency or a transparency lock), the same frozen-alpha composite under
//! either (`blend::composite_source_frozen`,
//! `lerp(Cb, B(Cb, Cs), sa)` at the pixel's own alpha) and the same
//! Preserve Transparency restore after it, as a backstop
//! (`PrecisionDocument::restoring_alpha`). Only the sample type
//! differs: the backdrop is the straight f32 pixel itself, so there is no
//! quantize step and no sub-quantum rule. A grown layer is built row by row
//! straight into tiles, with no adopted `Vec` to copy.
//!
//! The ink is LINEAR light, like every float sample: an authored colour in
//! the document's own encoded components is decoded through that profile's
//! own tone curves (`LinearInk::decoding`), and an sRGB colour through
//! sRGB's curves and the sRGB→document matrix
//! (`PrecisionTransform::linear`). It is never boosted — in-gamut ink lies in
//! [0, 1] and diffuse white is 1.0 — so under the Clip tone map a fully
//! covered Normal fill displays as the authored colour. Nothing clamps the
//! backdrop: negatives and values above white enter the blend as they are.
//! `color_space` `linear` is ink that is linear light in the
//! document's primaries already — an above-white foreground, or a host's
//! `rgba32` — and is used as is: RGB finite in `0..=FLOAT_RANGE_MAX`, alpha in
//! [0, 1] (`admit_ink`), measured against Photoshop 26.10.0's float-foreground
//! fill (`hdr-colour-317-608/take2`, FG: fill = float). It is this arm's
//! `paint_blend`, `gradient` and `bucket_fill` only: a u16 document, the
//! typed `paint` and `rasterize` refuse it.
//!
//! The blend is `blend::composite_source_into` (`composite_source_frozen`
//! when the alpha is frozen) over the modes `float_document::admitted_blend`
//! names — the layer set, which 32-bit Fill and Stroke composite by the same
//! formulas (measured against Photoshop) — with each mode's kind from
//! `blend_float::blend_kind_at`; every other mode refuses with the
//! float layer-mode sentence (`float_document::blend_refusal`). A result
//! outside the extended range (`precision_image::extended_pixel`) refuses
//! through the f32 writers; it is never repaired.
//!
//! # Brush, Eraser, Gradient
//!
//! The Brush is `paint_blend` over its dab coverage: the walk above, with
//! document, sRGB or linear ink. The Eraser is `erase_native`: alpha becomes
//! `blend::erase_alpha(alpha, coverage × opacity)` and RGB is not read or
//! written (the float Cut's rule), so above-white, negative and hidden colour
//! survive bit for bit; it never grows the layer. The Gradient is `gradient`:
//! `float_paint_ink::FloatInk::Ramp` converts every stop to linear light and
//! interpolates there over `gradient_field`'s shapes, then each pixel takes
//! the `paint_blend` composite at its own ramp colour, so an end above white
//! is reached unclamped. A mask or an alpha channel is u16 coverage at every
//! depth and keeps the scalar `paint` / `gradient` kinds; a colour plane is
//! this walk's `plane` arm (`# Admission`), which is Fill's and
//! Stroke's only. Tests: `tests/float_brush_tools.rs`,
//! `tests/float_fill_stroke.rs`, `tests/float_plane_fill.rs`.
//!
//! All three plan the layer fill's figure (`# Admission`). Measured in
//! Release, one case per process, `MallocLargeCache=0`, a baseline twin and
//! three runs (`examples/float_paint_admission.rs`), on a 6000×4000 opaque
//! float layer under canvas-wide coverage of 65535:
//!
//! | case | plan A | max E (phys_footprint peak over the edit) | ⌈1.25 E⌉ |
//! |---|---|---|---|
//! | B32 (`paint_blend`) | 1,544,388,608 | 769,982,992 | 962,478,740 |
//! | E32 (`erase_native`) | 1,544,388,608 | 769,966,632 | 962,458,290 |
//! | G32 (`gradient`) | 1,544,388,608 | 769,966,608 | 962,458,260 |
//!
//! `A ≥ ⌈1.25 E⌉` in each; the floors are pinned by
//! `selected_color_planes_24mp.rs` (`FLOOR_B32`, `FLOOR_E32`, `FLOOR_G32`).
//!
//! # Admission
//!
//! `paint_blend`'s optional `plane` (`red`, `green`, `blue`, `alpha`) is Fill and Stroke on ONE
//! colour channel of a float layer, under the rule measured against Photoshop 26.10.0
//! (`scripts/photoshop-probe/plane_fill.py`, receipt `plane-fill-431/take1`, branch B431). The
//! value is `plane_swatch::plane_fill_value` of the ink's components AS GIVEN — the swatch's
//! document-encoded components (`3059x`, read `raw` at 32 bits), or the linear RGB of linear ink,
//! unclipped (HDR row H-LIN: (4, 0.5, 0.25) writes 1.5225) — and component k becomes
//! `doc_plane::blend_triple` of itself and that value as gray triples at
//! `t = ink alpha × opacity × coverage`, through `blend_float::blend_kind_at`: Normal, Multiply,
//! Darken, Lighten, Difference, Linear Dodge, Subtract (each measured `scalar`, unclamped) and
//! Dissolve (`dither`). Darker Color and Lighter Color refuse by name on a colour plane
//! (take1's `refused` read the same in Photoshop), the four HSL modes with
//! `precision_plane_math::validate`'s sentence, sRGB ink and `preserve_alpha` with their own. The
//! opacity stays exact (Photoshop quantizes it to a byte, `opacity` `byte`; the decision in force).
//! An Alpha target's value and result are clamped to 0…1.
//!
//! Decision (the take's SOFT verdict, `written`): `t` is NOT scaled by the pixel's stored alpha — a
//! partly transparent pixel reads the full value in Photoshop — so the factor the channel move's
//! lift applies to the same channels (`selected_color_plane::alpha_scaled`, the master of that
//! rule) has no part here, and a stored-alpha-0 pixel takes the write too (unreadable in
//! Photoshop's TIFFs, which carry associated alpha; Doctored's decision by continuity). The alpha
//! floor and the frozen-alpha composite are the layer arm's only; under a transparency lock the
//! lock's own restore (`PrecisionDocument::restoring_alpha`) keeps a stored-alpha-0 pixel's colour
//! and makes an Alpha fill a no-op. A plane never grows the layer (`NativePaintEdit::expands`): the
//! walk rect is clipped to it, every other component is untouched, and a walk that changes no
//! sample is `None`.
//!
//! The plan is the layer fill's figure, `n·48 + output·16 + 8·BOOKKEEPING_BYTES` with `output` the
//! layer (no coverage copy is made: `t` reads the walk's own RGBA). Measured in Release, one case
//! per process, `MallocLargeCache=0`, a baseline twin and three runs, on the 6000×4000 float
//! document and coverage rect (300, 200, 5400, 3600) of `tests/selected_color_planes_24mp.rs`
//! (`plane:"red"`, document ink, Normal, opacity 1):
//!
//! | case | plan A | max E (phys_footprint peak over the edit) | ⌈1.25 E⌉ |
//! |---|---|---|---|
//! | P32 | 1,544,388,608 | 663,552,480 | 829,440,600 |
//!
//! `A ≥ ⌈1.25 E⌉`; the floor is pinned by `selected_color_planes_24mp.rs` (`FLOOR_P32`).

use serde::Deserialize;
use serde_json::{json, Value};

use crate::blend::{composite_source_frozen, composite_source_into, erase_alpha, BlendMode};
use crate::blend_float::blend_kind_at;
use crate::doc_lock::{EditKind, LOCK_TRANSPARENCY};
use crate::doc_plane::{blend_triple, degenerates_on_gray};
use crate::float_document::{admitted_blend, blend_refusal, refused_sentence, FLOAT_RANGE_MAX};
use crate::float_paint_ink::FloatInk;
use crate::float_paint_tone::{idle, tone_terms, BUCKET_REGION_BYTES_PER_PIXEL};
use crate::icc::IccProfile;
use crate::icc_transform::PrecisionTransform;
use crate::job_control::JobContext;
use crate::layer_rect::{canvas_rect_on_plane, coverage_rect16, plane_rect_offset};
use crate::plane_swatch::plane_fill_value;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionRun, BOOKKEEPING_BYTES,
};
use crate::precision_paint::PaintColorSpace;
use crate::precision_paint_tools::{shifted, NativePaintEdit, PaintFrame};
use crate::retouch_transform::merged_extent;
use crate::tile_store::TilePlane;

/// One colour channel of a float layer as `paint_blend`'s target:
/// the request's optional `plane`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PaintPlane {
    Red,
    Green,
    Blue,
    Alpha,
}

impl PaintPlane {
    /// The component's index in a straight RGBA pixel.
    pub(crate) fn component(self) -> usize {
        match self {
            Self::Red => 0,
            Self::Green => 1,
            Self::Blue => 2,
            Self::Alpha => 3,
        }
    }
}

/// Linear ink anywhere but a float document's `paint_blend`, `gradient` and
/// `bucket_fill` (a u16 document, the typed `paint` and `rasterize`).
pub(crate) const LINEAR_ONLY: &str =
    "linear ink is a 32-bit float document's paint and gradient colour";
const LINEAR_RANGE: &str =
    "linear ink must be finite with red, green and blue in [0, 65504] and alpha in [0,1]";
const PLANE_FLOAT: &str = "paint_blend's plane is a 32-bit float document's target; a 16-bit colour plane is a scalar_edit target";
const PLANE_PRESERVE: &str = "a colour-plane fill takes no preserve_alpha";
const PLANE_INK: &str = "a colour-plane fill takes document ink";
const PLANE_PATTERN: &str = "a colour-plane fill takes one colour, not a pattern";

/// The ink and plane refusals of a native paint request, asked FIRST by
/// `NativePaintEdit::plan_in` and by the float walk (`applying_unlocked`)
/// (module doc, `# Admission`).
///
/// Linear ink is `paint_blend`'s, `gradient`'s and `bucket_fill`'s
/// in a float document only ([`LINEAR_ONLY`] elsewhere), finite with RGB in
/// `0..=FLOAT_RANGE_MAX` and
/// alpha in [0, 1]. A `plane` is a float document's target, takes no
/// `preserve_alpha` and no sRGB ink, refuses the four HSL modes with
/// `precision_plane_math::validate`'s sentence (one gray plane has none to
/// compute), a mode outside `admitted_blend` with `blend_refusal`, and Darker
/// Color and Lighter Color by name with one channel targeted (take1's
/// `refused`). A mode number that names no
/// paint mode (none, or a group's Pass Through) is left to the shared
/// validation.
pub(crate) fn admit_ink(edit: &NativePaintEdit, doc: &PrecisionDocument) -> Result<(), String> {
    let (colors, space) = edit.colors();
    if matches!(space, PaintColorSpace::Linear) {
        let inked = matches!(
            edit,
            NativePaintEdit::PaintBlend { .. }
                | NativePaintEdit::Gradient { .. }
                | NativePaintEdit::BucketFill { .. }
        );
        if !doc.is_float() || !inked {
            return Err(LINEAR_ONLY.into());
        }
        let admitted = colors.iter().all(|color| {
            color[..3]
                .iter()
                .all(|v| v.is_finite() && (0.0..=FLOAT_RANGE_MAX).contains(v))
                && color[3].is_finite()
                && (0.0..=1.0).contains(&color[3])
        });
        if !admitted {
            return Err(LINEAR_RANGE.into());
        }
    }
    if edit.plane().is_none() {
        return Ok(());
    }
    if !doc.is_float() {
        return Err(PLANE_FLOAT.into());
    }
    if edit.pattern().is_some() {
        return Err(PLANE_PATTERN.into());
    }
    if edit.preserves_alpha() {
        return Err(PLANE_PRESERVE.into());
    }
    if matches!(space, PaintColorSpace::Srgb) {
        return Err(PLANE_INK.into());
    }
    let (_, opacity, mode, _) = edit.target();
    let Some((code, blend)) = i32::try_from(mode)
        .ok()
        .and_then(|code| Some((code, BlendMode::from_c(code)?)))
    else {
        return Ok(());
    };
    if blend.is_group_only() {
        return Ok(());
    }
    if degenerates_on_gray(blend) {
        crate::precision_plane_math::validate(1, code, f64::from(opacity))?;
    }
    if !admitted_blend(blend) {
        return Err(blend_refusal(blend));
    }
    if matches!(blend, BlendMode::DarkerColor | BlendMode::LighterColor) {
        return Err(refused_sentence(&format!(
            "the {} blend mode on one colour channel",
            crate::style::blend_mode_name(blend)
        )));
    }
    Ok(())
}

/// The float plan: the blend-mode refusal, then the master plan's extent,
/// work and geometry checks over f32 working buffers, plus the bucket's
/// region (`float_paint_tone`'s `# Admission`). The shared
/// opacity/mode/colour validation has already run
/// (`NativePaintEdit::plan_in`).
pub(crate) fn plan_in(
    edit: &NativePaintEdit,
    doc: &PrecisionDocument,
    frame: PaintFrame,
) -> Result<Value, String> {
    let (layer, _, mode, selection) = edit.target();
    let blend = i32::try_from(mode)
        .ok()
        .and_then(BlendMode::from_c)
        .ok_or("native paint opacity or blend mode is invalid")?;
    if !admitted_blend(blend) {
        return Err(blend_refusal(blend));
    }
    let extent = edit.extent(doc, frame)?;
    let n = pixel_count(frame.width, frame.height)? as u64;
    let output = pixel_count(extent.0 .0, extent.0 .1)? as u64;
    // The master's figure at f32 width: the walk rect is 16 bytes a pixel
    // instead of 8 (n·48 for the u16 walk's n·24), and so is the output.
    let mut scratch = 8 * BOOKKEEPING_BYTES + n * 48 + output * 16;
    if matches!(edit, NativePaintEdit::BucketFill { .. }) {
        scratch += n * BUCKET_REGION_BYTES_PER_PIXEL;
    }
    if edit.preserves_alpha() {
        // The alpha restore's two rect reads and its output rect.
        scratch += output * 48;
    }
    scratch += edit.pattern_scratch();
    let (colors, space) = edit.colors();
    if matches!(space, PaintColorSpace::Srgb) && !colors.is_empty() {
        let source = IccProfile::srgb();
        scratch += PrecisionTransform::prepare_scratch_bytes(
            source.model().ok_or("sRGB model missing")?,
            doc.profile()
                .model()
                .ok_or("native paint requires RGB matrix/TRC profile")?,
        );
    }
    let (mut rasters, masks) = doc.native_dimensions();
    let raster = (0..layer)
        .filter(|&i| doc.layer_source(i).is_some())
        .count();
    rasters[raster] = extent.0;
    doc.native_fits_geometry(doc.dimensions(), &rasters, &masks, doc.channels.len())?;
    Ok(
        json!({"retained_bytes":doc.retained_bytes(),"scratch_bytes":scratch,"input_samples":n*if selection{2}else{1},"input_width":frame.width,"input_height":frame.height,"input_planes":if selection{2}else{1}}),
    )
}

/// The float arm of `NativePaintEdit::apply_in`: the LOCK gate, then the
/// walk.
pub(crate) fn apply_in(
    edit: &NativePaintEdit,
    doc: &PrecisionDocument,
    input: &[u16],
    frame: PaintFrame,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<Option<PrecisionDocument>, String> {
    let (index, ..) = edit.target();
    doc.under_locks(index, EditKind::Pixels, |doc| {
        applying_unlocked(edit, doc, input, frame, limit, other, job)
    })
}

/// The walk (module doc); `precision_paint_tools`' `applying_unlocked` is
/// the master copy of every step but the sample type.
fn applying_unlocked(
    edit: &NativePaintEdit,
    doc: &PrecisionDocument,
    input: &[u16],
    frame: PaintFrame,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<Option<PrecisionDocument>, String> {
    // The local plan below has none of the ink and plane refusals, so the
    // walk asks them itself rather than trust its caller to have planned.
    admit_ink(edit, doc)?;
    let plan = plan_in(edit, doc, frame)?;
    let scratch = plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX);
    admit(doc.retained_bytes(), scratch, other, limit)?;
    job.reserve(scratch)?;
    if input.len() as u64 != plan["input_samples"].as_u64().unwrap_or(u64::MAX) {
        return Err("native paint input length differs from plan".into());
    }
    let (index, opacity, mode, selected) = edit.target();
    let n = pixel_count(frame.width, frame.height)?;
    if opacity == 0. || idle(edit, doc, input, n, selected) {
        job.report(1., 5)?;
        return Ok(None);
    }
    let (colors, _) = edit.colors();
    // A colour plane takes the ink's components as given (`# Admission`):
    // its value is their 30/59/11 luma, written raw.
    let plane = edit.plane().map(|plane| {
        let k = plane.component();
        let value = plane_fill_value([colors[0][0], colors[0][1], colors[0][2]]);
        (k, if k == 3 { value.clamp(0.0, 1.0) } else { value })
    });
    // A plane takes the ink as given; every other walk composites the
    // request's linear ink, and the Eraser has none (`FloatInk::of`).
    let paint = match plane {
        Some(_) => None,
        None => FloatInk::of(edit, doc)?,
    };
    let layer = doc
        .layer(index)
        .ok_or("native paint target is out of range")?;
    let image = layer
        .source()
        .ok_or("native paint requires a raster target")?;
    let (w, h) = image.dimensions();
    // Frame-local, as in the master: the layer's offset moves by `-frame`
    // and the result's offset moves back at the end.
    let (ox, oy) = (
        shifted(layer.offset().0, frame.x)?,
        shifted(layer.offset().1, frame.y)?,
    );
    let canvas = (frame.width, frame.height);
    let expands = edit.expands(doc);
    let (region, dodge) = tone_terms(edit, doc, image, (ox, oy), job.range(0., 0.3))?;
    let walk = if expands {
        coverage_rect16(&input[..n], canvas, canvas, (0, 0))
    } else {
        coverage_rect16(&input[..n], canvas, (w, h), (ox, oy)).map(|rect| {
            let (x, y) = plane_rect_offset((ox, oy), rect);
            (x as u32, y as u32, rect.2, rect.3)
        })
    };
    let Some((rx, ry, rw, rh)) = walk else {
        job.report(1., 5)?;
        return Ok(None);
    };
    let area = rw as usize * rh as usize;
    let mut canvas_rect = reserved_vec(area)?;
    canvas_rect.resize(area, [0f32; 4]);
    let ends = |a: u32, b: u32| (i64::from(a), i64::from(a) + i64::from(b));
    let ((x0, x1), (y0, y1)) = (ends(rx, rw), ends(ry, rh));
    let on_layer = canvas_rect_on_plane(canvas, (w, h), (ox, oy), (x0, y0, x1, y1));
    if let Some(rect) = on_layer {
        let samples = image
            .read_rect_rgba32(rect.0, rect.1, rect.2, rect.3)
            .ok_or("could not allocate precision output")?;
        let (cx, cy) = plane_rect_offset((ox, oy), rect);
        let (dx, dy) = ((cx as u32 - rx) as usize, (cy as u32 - ry) as usize);
        for (row, line) in samples.chunks_exact(rect.2 as usize).enumerate() {
            let at = (dy + row) * rw as usize + dx;
            canvas_rect[at..at + line.len()].copy_from_slice(line);
        }
    }
    let mut bounds = [frame.width, frame.height, 0, 0];
    let mut changed = false;
    let kind = blend_kind_at(
        BlendMode::from_c(mode as i32).unwrap_or(BlendMode::Normal),
        true,
    );
    // The master's frozen-alpha rule: Preserve Transparency or a
    // transparency lock; the restore below stays as the backstop.
    let frozen = edit.preserves_alpha() || doc.lock_owner(index, LOCK_TRANSPARENCY).is_some();
    for (k, slot) in canvas_rect.iter_mut().enumerate() {
        job.every(k)?;
        if k & 65535 == 0 {
            job.report(0.3 + 0.5 * k as f64 / area as f64, 3)?;
        }
        let x = rx + (k % rw as usize) as u32;
        let y = ry + (k / rw as usize) as u32;
        let i = y as usize * frame.width as usize + x as usize;
        let coverage = f64::from(input[i]) / 65535.
            * if selected {
                f64::from(input[n + i]) / 65535.
            } else {
                1.
            };
        if coverage <= 0. || region.as_ref().is_some_and(|r| r[i] == 0) {
            continue;
        }
        if let Some(rule) = &dodge {
            // R4 (`display_encoding`): alpha-0 pixels and alpha are kept.
            let after = rule.mixed(*slot, coverage);
            if after.map(f32::to_bits) != slot.map(f32::to_bits) {
                *slot = after;
                changed = true;
                bounds[0] = bounds[0].min(x);
                bounds[1] = bounds[1].min(y);
                bounds[2] = bounds[2].max(x + 1);
                bounds[3] = bounds[3].max(y + 1);
            }
            continue;
        }
        let ink = match (&plane, &paint) {
            (Some(_), _) => colors[0],
            (None, Some(paint)) => paint.at([
                (i64::from(x) + frame.x) as f64 + 0.5,
                (i64::from(y) + frame.y) as f64 + 0.5,
            ]),
            (None, None) => {
                // The Eraser: the float Cut's destination-out rule
                // on alpha alone. RGB is neither read nor written, so colour
                // above white, below zero or hidden under alpha 0 stays.
                let before = slot[3];
                let after = erase_alpha(before, (coverage * f64::from(opacity)) as f32);
                if after.to_bits() != before.to_bits() {
                    slot[3] = after;
                    changed = true;
                    bounds[0] = bounds[0].min(x);
                    bounds[1] = bounds[1].min(y);
                    bounds[2] = bounds[2].max(x + 1);
                    bounds[3] = bounds[3].max(y + 1);
                }
                continue;
            }
        };
        let rgb = [ink[0], ink[1], ink[2]];
        let alpha = (f64::from(ink[3]) * f64::from(opacity) * coverage) as f32;
        if let Some((k, value)) = plane {
            // The plane arm: component k alone, as a gray triple
            // through the one blend table. `alpha` is NOT multiplied by the
            // pixel's stored alpha: the take's SOFT verdict is `written`
            // (a partly transparent pixel reads the full value), so the
            // factor the channel move's lift applies to the same channels
            // (`selected_color_plane::alpha_scaled`, the master of that
            // rule) has no part here, and an alpha-0 pixel takes the write
            // too. The layer arm's alpha floor and frozen-alpha rule below
            // are not this arm's: no alpha is composited.
            if alpha <= 0. {
                continue;
            }
            let xy = (i64::from(x) + frame.x, i64::from(y) + frame.y);
            let before = slot[k];
            let mut after = blend_triple([before; 3], [value; 3], alpha, kind, xy)[0];
            if k == 3 {
                after = after.clamp(0.0, 1.0);
            }
            if after.to_bits() != before.to_bits() {
                slot[k] = after;
                changed = true;
                bounds[0] = bounds[0].min(x);
                bounds[1] = bounds[1].min(y);
                bounds[2] = bounds[2].max(x + 1);
                bounds[3] = bounds[3].max(y + 1);
            }
            continue;
        }
        // Filtered geometry's floor: ink this faint changes an opaque pixel
        // invisibly, and over a transparent one it would only leave an alpha
        // a later mask or opacity could push under the renderer's floor.
        if alpha < crate::sampling::EXTENDED_ALPHA_FLOOR {
            continue;
        }
        let original = *slot;
        let xy = (i64::from(x) + frame.x, i64::from(y) + frame.y);
        if frozen {
            composite_source_frozen(slot, rgb, alpha, kind, xy);
        } else {
            composite_source_into(std::slice::from_mut(slot), 0, rgb, alpha, kind, xy);
        }
        if slot.map(f32::to_bits) != original.map(f32::to_bits) {
            changed = true;
            bounds[0] = bounds[0].min(x);
            bounds[1] = bounds[1].min(y);
            bounds[2] = bounds[2].max(x + 1);
            bounds[3] = bounds[3].max(y + 1);
        }
    }
    if !changed {
        job.report(1., 5)?;
        return Ok(None);
    }
    let extent = if expands {
        merged_extent(
            ((w, h), (ox, oy)),
            (
                (bounds[2] - bounds[0], bounds[3] - bounds[1]),
                (bounds[0] as i32, bounds[1] as i32),
            ),
        )?
    } else {
        ((w, h), (ox, oy))
    };
    let ((nw, nh), (nx, ny)) = extent;
    let image = match on_layer {
        Some(rect) if extent == ((w, h), (ox, oy)) => {
            // R ∩ layer back through `with_rect` (the master's rule).
            let samples = if (rect.2, rect.3) == (rw, rh) {
                canvas_rect
            } else {
                let (cx, cy) = plane_rect_offset((ox, oy), rect);
                let (dx, dy) = ((cx as u32 - rx) as usize, (cy as u32 - ry) as usize);
                let mut samples = reserved_vec(rect.2 as usize * rect.3 as usize)?;
                for row in 0..rect.3 as usize {
                    let at = (dy + row) * rw as usize + dx;
                    samples.extend_from_slice(&canvas_rect[at..at + rect.2 as usize]);
                }
                samples
            };
            let Some(image) = image.with_rect_rgba32(rect.0, rect.1, rect.2, rect.3, &samples)?
            else {
                job.report(1., 5)?;
                return Ok(None);
            };
            image
        }
        _ => {
            // The grown extent E: zeros, the old layer's rows, then R's
            // values over R ∩ E only (the master's rule), built row by row
            // straight into tiles (no adopted `Vec`, no copy).
            pixel_count(nw, nh)?;
            let (left, top) = (i64::from(nx), i64::from(ny));
            let (r0, r1) = (x0.max(left), x1.min(left + i64::from(nw)));
            let fill_row = |y: u32, row: &mut [[f32; 4]]| -> Result<(), String> {
                job.check()?;
                row.fill([0f32; 4]);
                let cy = i64::from(y) + top;
                let ly = cy - i64::from(oy);
                if (0..i64::from(h)).contains(&ly) {
                    let at = (i64::from(ox) - left) as usize;
                    for (x, run) in image.row_segments(ly as u32, 0, w) {
                        let PrecisionRun::Rgba32(run) = run else {
                            return Err(
                                "native paint in a 32-bit float document requires f32 storage"
                                    .into(),
                            );
                        };
                        let at = at + x as usize;
                        row[at..at + run.len()].copy_from_slice(run);
                    }
                }
                if (y0..y1).contains(&cy) && r0 < r1 {
                    let from = (cy - y0) as usize * rw as usize + (r0 - x0) as usize;
                    let at = (r0 - left) as usize;
                    let len = (r1 - r0) as usize;
                    row[at..at + len].copy_from_slice(&canvas_rect[from..from + len]);
                }
                Ok(())
            };
            let plane = TilePlane::from_rows_with_job(nw, nh, fill_row, JobContext::NONE)?;
            PrecisionImage::from_rgba32_tiles_decoded(plane, doc.profile().clone())?
        }
    };
    job.check()?;
    let (nx, ny) = (shifted(nx, -frame.x)?, shifted(ny, -frame.y)?);
    let mut next = doc.replacing_content(
        index,
        Content::Raster {
            image,
            offset: (nx, ny),
        },
    )?;
    if let Some(next) = next.as_mut() {
        if let Some(comps) = doc.comps_after_growth(index, (nx, ny)) {
            next.comps = comps;
        }
    }
    if edit.preserves_alpha() {
        next = match next {
            Some(next) => doc.restoring_alpha(index, next)?,
            None => None,
        };
    }
    job.report(1., 5)?;
    Ok(next)
}
