//! A LAYER clipboard promoted into a 32-bit float document.
//! Copied 8- and 16-bit layers paste into a 32-bit document by name, as
//! linear light, and a 32-bit layer of another profile converts (measured
//! against Photoshop 26.10.0, `scripts/photoshop-probe/paste_layers.py`), so
//! `PrecisionDocument::paste_layers` no longer refuses the depth or the
//! profile when the destination is float. This module is the per-record
//! promotion it calls; `doc_paste_layers` keeps the slot, the placement and
//! the splice.
//!
//! # The rule, per record
//!
//! - A raster's pixels go through `precision_clipboard_promote::promoted_image`
//!   (the pixel paste's kernel, one image at a time): a u16 plane is decoded
//!   to linear light and, under Convert, taken through the source →
//!   destination matrix; an f32 plane of another profile takes the matrix
//!   under Convert and is relabelled (`with_profile`, samples shared) under
//!   Preserve. Planes sharing one `plane_key()` and mode promote once.
//! - A PLACED IMAGE (an effective retained source; in the app, a RAW layer)
//!   lands placed: Photoshop 26.10.0 keeps a 16-bit placed
//!   image bound to its original when it is pasted into a 32-bit document
//!   and converts it by the place's own colour rule (`scripts/photoshop-probe/float_placed.py`,
//!   receipt `float-placed-473/take1`, paste verdict `kept`). Under Convert
//!   its content and binding are re-rendered from the master into the
//!   destination's linear light (`retained_source_float::render_typed`);
//!   under Preserve (Don't Convert keeps a placed image's numbers, as it
//!   keeps every layer's) the raster promotes like
//!   any other and the binding is kept with that promoted plane as its
//!   fallback, re-rendered on the next edit.
//! - A TEXT or SHAPE layer lands described: Photoshop 26.10.0
//!   keeps a 16-bit type or shape layer pasted into a 32-bit document and
//!   rewrites its colour numbers so it looks the same (receipt
//!   `float-artwork-473/take2`, PT/PSH). Under Convert the description rides
//!   verbatim with the promoted pixels; under Preserve it rides only when the
//!   record's profile bytes are the destination's, because a relabelled
//!   plane in another profile no longer shows what the description renders
//!   (otherwise it is dropped with a `text or shape` row). Description bytes
//!   ride verbatim; the host reads them in the destination's ink (receipt
//!   take2 PT/PSH, NR).
//! - A STALE retained source and a warp binding are DROPPED and the record's
//!   own pixels kept: the float mode holds neither, and the kept pixels are
//!   the layer's own render, so the look does not change.
//!   Each is one `rasterized` row: a dropped retained source gets `placed
//!   image`, the row reporting the binding lost.
//! - A Pass Through group lands as a Pass Through group, its children
//!   promoted like any other record: the float render composites Pass
//!   Through at 32 bits (measured against Photoshop 26.10.0).
//! - A blend mode a float document refuses ([`FLOAT_RESET_MODES`]) becomes
//!   Normal
//!   (`blend_reset`).
//! - A layer STYLE and Blend If ride verbatim (the float mode
//!   holds every effect, fill opacity, knockout, Blend Interior Effects as
//!   Group and Blend If), except an effect blend mode reset on the way in,
//!   as measured against Photoshop 26.10.0 ([`FLOAT_EFFECT_RESET`]:
//!   Screen on an Outer Glow, an Inner Glow or a Bevel's highlight lands
//!   Linear Dodge), each one an `effect_blend_reset` row. The style's colours
//!   are not touched: they are authored sRGB and a float document composites
//!   them in linear light (`style_float`), so the pasted style keeps its look.
//! - An adjustment is re-bound to the destination profile. Masks, offsets,
//!   names, opacity, visibility, clip flags, labels, locks, links and open
//!   state ride verbatim.
//!
//! # What still refuses
//!
//! [`refusal`] asks `float_document::refused_operator` of each record AS IT
//! WOULD LAND (bindings dropped, mode reset), in postorder,
//! and the first answer refuses the whole paste before any admission or
//! pixel work: ``the copied layer “{name}” cannot be pasted into a 32-bit
//! float document: {sentence}``. That leaves an effect blend mode outside the
//! float set that the paste does not reset (`float_document::style_refusal`),
//! Divide, the one blend mode measured at 32 bits that the float mode lacks
//! (the other nine measured modes ride), every
//! adjustment the float mode cannot evaluate (Hue/Saturation, Color Lookup
//! with an abstract profile, a 1D table, a domain other than 0…1 or entries
//! outside 0…1, and those refused at 32 bits; Levels, `bcs`, Black & White
//! (its Tint too), Channel Mixer, Photo Filter, Exposure of either
//! evaluation, point-form Curves, a calibrated White Balance, Color Balance,
//! map-form Curves and Color Lookup's 3D table within 0…1 land, re-parsed
//! against the destination profile).
//!
//! # Admission
//!
//! [`promotion_terms`] counts the planes that convert: `pixels` their
//! summed pixel count, `held` the f32 planes the result keeps (16 B/px) and
//! `conversion` the promotion's own working memory — bookkeeping, `2 ·
//! held` (each new plane and its adoption copy, the pixel paste's
//! `conversion_bytes` shape) and `TypedTransfer::table_bytes` when a u16
//! plane decodes; all 0 when nothing converts. A placed record re-rendered
//! under Convert counts as a converting plane of its render's dimensions,
//! and `conversion` adds the largest such record's render scratch
//! (`work_plan_within` at fallback depth 32 under the one-shot limit, plus
//! `retained_source_float::float_render_scratch`). The paste adds
//! [`LAYER_PROMOTE_MARGIN_PER_PIXEL`] × `pixels` to its insertion margin.
//! [`promotion_bytes`] is that count's tail over a pixel total.
//!
//! The conversion of a whole 8- or 16-bit document into float
//! (`precision_conversion_float`) is this promotion under Preserve
//! over the document's own records: it refuses on the records as they ARE
//! (no reset ever fires), charges [`promotion_terms`] or [`promotion_bytes`]
//! with the same margin, and runs [`promoted_layers`] under its job.
//!
//! Per record: a raster through `precision_clipboard_promote::promoted_image` (the pixel paste's
//! kernel over ONE image, which `ClipboardPayload::promoted` now calls too; planes sharing one
//! `plane_key()` and mode promoted once), except a placed image under Convert, re-rendered from its
//! master (`render_typed`; under Preserve its binding is kept over the promoted
//! plane); a mode in `FLOAT_RESET_MODES` (the nine of the twelve Photoshop 26.10.0 was
//! MEASURED to land Normal, `scripts/photoshop-probe/paste_layers.py`, that a float document
//! still refuses; Linear Burn, Linear Light and Pin Light are admitted) set to Normal (`blend_reset`);
//! adjustments `rebind`. `refusal` asks `float_document::refused_operator` (now `pub(crate)`) of
//! each record as it would land, postorder, and the first answer refuses the whole paste before any
//! admission: ``the copied layer “{name}” cannot be pasted into a 32-bit float document:
//! {sentence}`` (an effect blend mode outside the float set, Divide, an adjustment the float mode cannot
//! evaluate); a Pass Through group lands as Pass Through; an effect mode in
//! `FLOAT_EFFECT_RESET` becomes Linear Dodge (`effect_blend_reset`). `LAYER_PROMOTE_MARGIN_PER_PIXEL` is 10
//! (the paste passed at 0, its allocation 2.02× the 24 MP peak; the conversion into float
//! measured 10). Byte-different profiles of one colour model
//! convert through the ordinary matrix and are charged as a conversion. `PrecisionDocument::build`
//! stays the one storage gate. Tests: `tests/layer_clipboard_float.rs`
//! (`tests/common/icc_spelling.rs` is its second spelling of sRGB).
//!
//! # Measurement record (2026-10-05)
//!
//! `precision_clipboard_promote`'s method: a Release app with
//! `DR_WORKING_LIMIT_BYTES = 8589934592`, `MallocLargeCache=0`,
//! `DR_BACKGROUND=1`, private `DR_*_DIR`s and a private pasteboard, one
//! fresh instance per run; E = `ri_interval_max_phys_footprint` − the
//! footprint read right after `proc_reset_footprint_interval`, around ONE
//! MCP `paste` (`mode: "layers"`, in place). The float target is the
//! float_11f `scene.tiff` resized to 6000×4000 (`image_size`), the source a
//! 6000×4000 16-bit white document (`new_document`) copied with `mode:
//! "layers"`: case 1 sRGB (the paste's `profile` reply "same"), case 2
//! Display P3 ("converted"). A = `PrecisionDocument::paste_layers_allocation`
//! of the same paste (one "Background" record on each side): `max(conversion,
//! held + insertion + margin)`, the insertion side deciding — 384,000,000
//! held + 4,202,536 insertion + 65 × 24,000,000 canvas margin + K ×
//! 24,000,000 promoted pixels.
//!
//! - With K = 0: sRGB E = 963,068,600 / 963,035,832 / 962,790,072; Display
//!   P3 E = 963,068,600 / 962,806,456 / 963,248,824; the `list_documents`
//!   twin E = 0 every run. A = 1,948,202,536 for both, A / max E = 2.02:
//!   ⌈1.25 × 963,248,824⌉ = 1,204,061,030 ≤ A. Both gates hold without a
//!   promotion margin, so K = 0 is the smallest integer that passes. (The
//!   measured build is the build with the final constant; nothing was
//!   re-fitted.) The shared constant is now 10, fitted by the
//!   conversion into float (`precision_conversion_float`); the paste's
//!   allocation only rose.
//! - Why E is below the same-depth float layers paste's 1,245 MB
//!   (`doc_paste_layers`' record): the promoted plane of a uniform white
//!   source is far smaller than its 384 MB charge, so nearly all of E is the
//!   publication's display preparation, which the canvas margin covers.

use std::sync::Arc;

use serde_json::json;

use crate::blend::BlendMode;
use crate::float_document::refused_operator;
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::precision_clipboard::PasteProfile;
use crate::precision_clipboard_promote::{models, promoted_image};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_evaluation::TypedTransfer;
use crate::precision_image::{pixel_count, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES};
use crate::retained_source::RetainedSource16;
use crate::retained_source_float::{float_render_scratch, render_typed};
use crate::retained_source_render::{profile_cost, work_plan_within, SourceSampling, SourceStage};
use crate::style::{Effect, EffectKind, LayerStyle};

/// One binding a pasted record lost, its pixels kept.
pub struct RasterizedRow {
    pub layer: String,
    /// `placed image`, `text or shape` or `warp`.
    pub reason: &'static str,
}

/// One record whose blend mode became Normal; `from` is the mode's
/// `style::blend_mode_name`.
pub struct ResetRow {
    pub layer: String,
    pub from: String,
}

/// One layer effect whose blend mode the paste resets on the way into a
/// float document ([`FLOAT_EFFECT_RESET`]); `effect` is the
/// effect's JSON `type`, `from` and `to` the modes' `style::blend_mode_name`.
pub struct EffectResetRow {
    pub layer: String,
    pub effect: &'static str,
    pub from: String,
    pub to: String,
}

/// What a layers paste changed so the records could land in a float
/// document. Every list is empty for a paste that promoted nothing.
#[derive(Default)]
pub struct LayerPasteReport {
    pub rasterized: Vec<RasterizedRow>,
    pub blend_reset: Vec<ResetRow>,
    pub effect_blend_reset: Vec<EffectResetRow>,
}

impl LayerPasteReport {
    /// `{"rasterized":[{"layer","reason"}],"blend_reset":[{"layer","from"}],
    /// "effect_blend_reset":[{"layer","effect","from","to"}]}`.
    pub fn json(&self) -> String {
        json!({
            "rasterized": self.rasterized.iter()
                .map(|row| json!({"layer": row.layer, "reason": row.reason}))
                .collect::<Vec<_>>(),
            "blend_reset": self.blend_reset.iter()
                .map(|row| json!({"layer": row.layer, "from": row.from}))
                .collect::<Vec<_>>(),
            "effect_blend_reset": self.effect_blend_reset.iter()
                .map(|row| json!({"layer": row.layer, "effect": row.effect,
                                  "from": row.from, "to": row.to}))
                .collect::<Vec<_>>(),
        })
        .to_string()
    }
}

/// The effect blend modes the paste resets when a 16-bit layer carrying
/// them lands in a 32-bit document: `(effect, from, to)`, the Bevel's row
/// being its HIGHLIGHT (its shadow's Multiply is kept). Measured against
/// Photoshop 26.10.0, not chosen (`scripts/photoshop-probe/float_styles.py`, `paste` verdict,
/// receipt `float-styles-473/take5`): each effect pasted at its 16-bit
/// default mode; these three read Linear Dodge, which is also their 32-bit
/// default, and every other effect's default mode read `kept`. A mode
/// outside the float set on any other slot was not measured and refuses
/// (`float_document::style_refusal`).
pub(crate) const FLOAT_EFFECT_RESET: &[(EffectKind, BlendMode, BlendMode)] = &[
    (
        EffectKind::OuterGlow,
        BlendMode::Screen,
        BlendMode::Addition,
    ),
    (
        EffectKind::InnerGlow,
        BlendMode::Screen,
        BlendMode::Addition,
    ),
    (
        EffectKind::BevelEmboss,
        BlendMode::Screen,
        BlendMode::Addition,
    ),
];

/// The mode an effect slot of `kind` lands with.
fn landed_effect_mode(kind: EffectKind, mode: BlendMode) -> BlendMode {
    FLOAT_EFFECT_RESET
        .iter()
        .find(|(k, from, _)| *k == kind && *from == mode)
        .map_or(mode, |(_, _, to)| *to)
}

/// `style` with every reset effect mode applied, or `None` when none applies.
fn landed_style(style: &LayerStyle) -> Option<LayerStyle> {
    let mut effects = style.effects.clone();
    let mut changed = false;
    for effect in &mut effects {
        let kind = effect.kind();
        let slot = match effect {
            Effect::OuterGlow(e) => &mut e.blend,
            Effect::InnerGlow(e) => &mut e.blend,
            Effect::BevelEmboss(e) => &mut e.highlight_blend,
            _ => continue,
        };
        let landed = landed_effect_mode(kind, *slot);
        if landed != *slot {
            *slot = landed;
            changed = true;
        }
    }
    changed.then(|| LayerStyle {
        effects,
        cache: crate::style_cache::RenderCache::default(),
        ..style.clone()
    })
}

/// The `effect_blend_reset` rows between a record's style and its landed one.
fn effect_reset_rows(layer: &str, from: &LayerStyle, to: &LayerStyle) -> Vec<EffectResetRow> {
    let mut rows = Vec::new();
    for (a, b) in from.effects.iter().zip(&to.effects) {
        let before = crate::float_document::effect_blend_modes(a);
        let after = crate::float_document::effect_blend_modes(b);
        for (x, y) in before.into_iter().zip(after) {
            if let (Some(x), Some(y)) = (x, y) {
                if x != y {
                    rows.push(EffectResetRow {
                        layer: layer.to_string(),
                        effect: a.kind().type_name(),
                        from: crate::style::blend_mode_name(x).to_string(),
                        to: crate::style::blend_mode_name(y).to_string(),
                    });
                }
            }
        }
    }
    rows
}

/// The blend modes that land as Normal when a 16-bit layer carrying them is
/// pasted into a 32-bit document, and the paste does not refuse. Photoshop
/// 26.10.0 was measured landing twelve modes Normal (`paste_layers.py`'s
/// `mode_32` verdict, take 1 of 2026-10-05); decision B admits three
/// of them (Linear Burn, Linear Light, Pin Light), which now ride, so these
/// are the nine a float document refuses. `float_document::admitted_blend` is
/// the master: this list is its complement over the 27 layer modes (Pass
/// Through, a group mode it does not name, is kept), so an edit there edits
/// this list too.
pub(crate) const FLOAT_RESET_MODES: &[BlendMode] = &[
    BlendMode::ColorBurn,
    BlendMode::Screen,
    BlendMode::ColorDodge,
    BlendMode::Overlay,
    BlendMode::SoftLight,
    BlendMode::HardLight,
    BlendMode::VividLight,
    BlendMode::HardMix,
    BlendMode::Exclusion,
];

/// The fitted margin a layers paste into a float document — and the
/// conversion of a document into float (`precision_conversion_float`) —
/// charges per PROMOTED-PLANE pixel, on top of `LAYER_PASTE_MARGIN_PER_PIXEL`
/// × canvas for the paste. Why 10: the paste alone passed at 0 (its
/// allocation is 2.02× the measured peak of a 24 MP promotion, the module
/// doc's record), but the conversion into float, whose plan has no canvas
/// margin, measured E at 32.0 B/px (16→float) and 39.9 B/px (8→float)
/// against plans of 32.3 and 40.3 (`precision_conversion_float`'s record):
/// covering ⌈1.25 × max E⌉ needs 8 (16→float) and 10 (8→float, 9.91 at 61
/// MP), so the smallest integer that passes every case is 10. Raising it
/// only raises the paste's admission.
pub(crate) const LAYER_PROMOTE_MARGIN_PER_PIXEL: u64 = 10;

/// The promotion's admission terms (module doc).
pub(crate) struct PromotionTerms {
    pub conversion: u64,
    pub held: u64,
    pub pixels: u64,
}

/// The mode a record lands with.
fn landed_mode(mode: BlendMode) -> BlendMode {
    if FLOAT_RESET_MODES.contains(&mode) {
        BlendMode::Normal
    } else {
        mode
    }
}

/// The binding a record keeps in a float document: its effective retained source.
fn kept_source(layer: &PrecisionDocumentLayer) -> Option<RetainedSource16> {
    layer.effective_source()
}

/// Whether a record's artwork description lands with it (module doc):
/// always under Convert, under Preserve only in the destination's own
/// profile bytes.
fn keeps_description(
    layer: &PrecisionDocumentLayer,
    profile: &Arc<IccProfile>,
    mode: PasteProfile,
) -> bool {
    match (mode, &layer.content) {
        (PasteProfile::Convert, _) => true,
        (PasteProfile::Preserve, Content::Raster { image, .. }) => {
            image.profile().bytes() == profile.bytes()
        }
        (PasteProfile::Preserve, _) => false,
    }
}

/// The record as it would land, without its pixels promoted: a kept binding
/// ([`kept_source`]) stays, the description only when `description` says so,
/// every other binding is dropped, a reset mode and reset effect modes
/// applied.
fn landed(layer: &PrecisionDocumentLayer, description: bool) -> PrecisionDocumentLayer {
    let mut node = layer.clone();
    node.retained_source = kept_source(layer);
    if !description {
        node.description = None;
    }
    node.warp = None;
    node.blend_mode = landed_mode(layer.blend_mode);
    if let Some(style) = layer.style.as_deref().and_then(landed_style) {
        node.style = Some(Arc::new(style));
    }
    node
}

/// The first record the float mode still cannot hold, as the paste's
/// complete refusal sentence (module doc); `None` when every record lands.
pub(crate) fn refusal(layers: &PrecisionDocument) -> Option<String> {
    fn walk(layers: &[PrecisionDocumentLayer]) -> Option<(String, String)> {
        for layer in layers {
            if let Some(children) = layer.children() {
                if let Some(found) = walk(children) {
                    return Some(found);
                }
            }
            if let Some(reason) = refused_operator(&landed(layer, true)) {
                return Some((layer.name().to_string(), reason));
            }
        }
        None
    }
    walk(layers.root_layers()).map(|(name, reason)| {
        format!("the copied layer “{name}” cannot be pasted into a 32-bit float document: {reason}")
    })
}

/// True when `image` becomes a NEW f32 plane under `mode`; refuses, under
/// Convert between differing profiles, a profile with no matrix/TRC model.
fn converts(
    image: &PrecisionImage,
    profile: &Arc<IccProfile>,
    mode: PasteProfile,
) -> Result<bool, String> {
    let differing = match mode {
        PasteProfile::Convert => models(image.profile(), profile)?.is_some(),
        PasteProfile::Preserve => false,
    };
    Ok(image.storage() == PrecisionStorage::Rgba16Unorm || differing)
}

/// The admission terms of promoting `layers` into `profile` under `mode`:
/// every record, a placed one included, promotes under `mode` itself.
pub(crate) fn promotion_terms(
    layers: &PrecisionDocument,
    profile: &Arc<IccProfile>,
    mode: PasteProfile,
) -> Result<PromotionTerms, String> {
    struct Count<'a> {
        profile: &'a Arc<IccProfile>,
        mode: PasteProfile,
        seen: Vec<(usize, PasteProfile)>,
        pixels: u64,
        decodes: bool,
        render: u64,
    }
    fn walk(layers: &[PrecisionDocumentLayer], count: &mut Count<'_>) -> Result<(), String> {
        for layer in layers {
            if let Some(children) = layer.children() {
                walk(children, count)?;
            }
            let Content::Raster { image, .. } = &layer.content else {
                continue;
            };
            let mode = count.mode;
            if let (PasteProfile::Convert, Some(source)) = (mode, kept_source(layer)) {
                // A re-rendered record: a converting plane of its render's
                // dimensions, and the render's own scratch (module doc).
                let dest =
                    crate::source_transform::bounds(source.asset.dimensions(), source.transform)?;
                count.pixels = count
                    .pixels
                    .checked_add(pixel_count(dest.2, dest.3)? as u64)
                    .ok_or("pasted layers size overflow")?;
                count.decodes = true;
                let plan = work_plan_within(
                    // Kept: `dr_precision_doc_paste_layers` has no
                    // job control, so nothing could cancel a longer render.
                    crate::sampling_gaussian::MAX_DESTRUCTIVE_WORK,
                    0,
                    SourceStage {
                        asset: &source.asset,
                    },
                    (dest.2, dest.3),
                    0,
                    SourceSampling::new(
                        source.asset.dimensions(),
                        (dest.2, dest.3),
                        source.transform,
                        source.filter,
                    ),
                    32,
                    profile_cost(source.asset.profile(), count.profile, true)?,
                )?;
                let render = plan
                    .scratch_bytes
                    .checked_add(float_render_scratch(
                        source.asset.dimensions(),
                        (dest.2, dest.3),
                    )?)
                    .ok_or("pasted layers size overflow")?;
                count.render = count.render.max(render);
                continue;
            }
            let key = (image.plane_key(), mode);
            if count.seen.contains(&key) {
                continue;
            }
            count.seen.push(key);
            if !converts(image, count.profile, mode)? {
                continue;
            }
            let (w, h) = image.dimensions();
            count.pixels = count
                .pixels
                .checked_add(pixel_count(w, h)? as u64)
                .ok_or("pasted layers size overflow")?;
            count.decodes |= image.storage() == PrecisionStorage::Rgba16Unorm;
        }
        Ok(())
    }
    let mut count = Count {
        profile,
        mode,
        seen: Vec::new(),
        pixels: 0,
        decodes: false,
        render: 0,
    };
    walk(layers.root_layers(), &mut count)?;
    promotion_bytes(count.pixels, count.decodes, count.render)
}

/// The terms of `pixels` converting planes (module doc): `held` = 16 B/px,
/// `conversion` = `2 · held` + bookkeeping + the transfer's tables when a
/// u16 plane `decodes` + the largest re-render's `render` scratch, all 0
/// when nothing converts. [`promotion_terms`]' tail, shared with the
/// conversion into float (`precision_conversion_float`), which counts its
/// planes from the structure walk instead.
pub(crate) fn promotion_bytes(
    pixels: u64,
    decodes: bool,
    render: u64,
) -> Result<PromotionTerms, String> {
    let held = pixels
        .checked_mul(PrecisionStorage::Rgba32Float.bytes_per_pixel())
        .ok_or("pasted layers size overflow")?;
    let conversion = if pixels == 0 {
        0
    } else {
        let table = if decodes {
            TypedTransfer::table_bytes()
        } else {
            0
        };
        held.checked_mul(2)
            .and_then(|n| n.checked_add(BOOKKEEPING_BYTES + table))
            .and_then(|n| n.checked_add(render))
            .ok_or("pasted layers size overflow")?
    };
    Ok(PromotionTerms {
        conversion,
        held,
        pixels,
    })
}

/// `layers`' records as they land in a float document of `profile` under
/// `mode` (module doc), with what was changed to get them there. The caller
/// has run [`refusal`] and admitted [`promotion_terms`]. Progress is
/// reported after each record, postorder, as records done over the record
/// count (the conversion into float runs it under a host job).
pub(crate) fn promoted_layers(
    layers: &PrecisionDocument,
    profile: &Arc<IccProfile>,
    mode: PasteProfile,
    job: JobContext<'_>,
) -> Result<(Vec<PrecisionDocumentLayer>, LayerPasteReport), String> {
    struct Promotion<'a> {
        profile: &'a Arc<IccProfile>,
        mode: PasteProfile,
        job: JobContext<'a>,
        done: Vec<((usize, PasteProfile), PrecisionImage)>,
        report: LayerPasteReport,
        records: usize,
        total: f64,
    }
    fn walk(
        layers: &[PrecisionDocumentLayer],
        state: &mut Promotion<'_>,
    ) -> Result<Vec<PrecisionDocumentLayer>, String> {
        let mut out = Vec::with_capacity(layers.len());
        for layer in layers {
            let children = match layer.children() {
                Some(children) => Some(walk(children, state)?),
                None => None,
            };
            let mut node = landed(layer, keeps_description(layer, state.profile, state.mode));
            let name = || layer.name().to_string();
            if let (Some(_), None) = (&layer.retained_source, &node.retained_source) {
                state.report.rasterized.push(RasterizedRow {
                    layer: name(),
                    reason: "placed image",
                });
            }
            if layer.description.is_some() && node.description.is_none() {
                state.report.rasterized.push(RasterizedRow {
                    layer: name(),
                    reason: "text or shape",
                });
            }
            if layer.warp.is_some() {
                state.report.rasterized.push(RasterizedRow {
                    layer: name(),
                    reason: "warp",
                });
            }
            if node.blend_mode != layer.blend_mode {
                state.report.blend_reset.push(ResetRow {
                    layer: name(),
                    from: crate::style::blend_mode_name(layer.blend_mode).to_string(),
                });
            }
            if let (Some(from), Some(to)) = (layer.style.as_deref(), node.style.as_deref()) {
                state
                    .report
                    .effect_blend_reset
                    .extend(effect_reset_rows(layer.name(), from, to));
            }
            node.content = match &layer.content {
                Content::CmykRaster { .. } | Content::CmykAdjustment(_) => {
                    return Err(crate::cmyk_document::refused_sentence("Paste Layers"))
                }
                Content::Raster { .. }
                    if node.retained_source.is_some() && state.mode == PasteProfile::Convert =>
                {
                    // Convert re-renders a kept binding from its master
                    // into this document's linear light.
                    let source = node
                        .retained_source
                        .take()
                        .ok_or("retained source binding became invalid")?;
                    let source = render_typed(source, state.profile, true, state.job)?;
                    let content = Content::Raster {
                        image: source.rendered.clone(),
                        offset: source.rendered_offset,
                    };
                    node.retained_source = Some(source);
                    content
                }
                Content::Raster { image, offset } => {
                    let key = (image.plane_key(), state.mode);
                    let image = match state.done.iter().find(|(k, _)| *k == key) {
                        Some((_, image)) => image.clone(),
                        None => {
                            let image = promoted_image(image, state.profile, key.1, state.job)?;
                            state.done.push((key, image.clone()));
                            image
                        }
                    };
                    // Preserve keeps a kept binding with the promoted plane
                    // as its fallback (pointer-equal), re-rendered on the
                    // next edit.
                    if let Some(source) = node.retained_source.as_mut() {
                        source.rendered = image.clone();
                        source.rendered_offset = *offset;
                    }
                    Content::Raster {
                        image,
                        offset: *offset,
                    }
                }
                Content::Adjustment(adjustment) => {
                    Content::Adjustment(adjustment.rebind(state.profile)?)
                }
                Content::Group(_) => Content::Group(Arc::new(children.unwrap_or_default())),
                Content::PassThrough(_) => {
                    Content::PassThrough(Arc::new(children.unwrap_or_default()))
                }
            };
            out.push(node);
            state.records += 1;
            state.job.report(state.records as f64 / state.total, 5)?;
        }
        Ok(out)
    }
    let mut state = Promotion {
        profile,
        mode,
        job,
        done: Vec::new(),
        report: LayerPasteReport::default(),
        records: 0,
        total: layers.layer_count().max(1) as f64,
    };
    let promoted = walk(layers.root_layers(), &mut state)?;
    Ok((promoted, state.report))
}
