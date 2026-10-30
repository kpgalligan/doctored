//! Pattern Overlay: the effect's model, its JSON, the
//! LINK ANCHOR rules, and the `.rz` word 90 with its predicates. No pixel
//! math: the tile, levels and sampler are `pattern_tile`, the renderer
//! `style_fx_pattern_overlay`.
//!
//! # Placement (FOLLOW, take1 and controller ruling 1)
//!
//! Photoshop starts a LINKED tile at the canvas origin plus `phase` when the
//! style is set, and from then on the tile moves with the layer; the phase it
//! reports is not rewritten by the move. The model stores the layer's offset
//! at that moment as the effect's `anchor`, so the canvas origin of the tile
//! is O = phase + (offset now − anchor) when linked, and O = phase when not.
//! [`PatternFill::origin`] returns it in LAYER px — phase − anchor when
//! linked (a missing anchor counts as the current offset), phase − offset
//! when not — so a linked plane is layer-local and a move reuses the cache.
//!
//! Photoshop's own anchor is the layer's offset when the layer was MADE
//! (take2's ANCHOR-TIME: a layer moved before its style still carries the
//! move). Every layer carries Photoshop's `fxrp` as its REFERENCE point
//! (`Layer::reference`, the typed `PrecisionDocumentLayer::reference`): (0, 0)
//! at creation, moved by every translation (Move, an offset write, a
//! pure-translation transform, at both depths) and mapped by
//! the map of Image Size, Canvas Size, Crop, the Crop tool's angled crop, the
//! quarter turns and flips, and Free Transform (`reference_point`'s measured
//! POINT rule, rounded; `doc_reference` the adapter), never by a placement (a
//! paste lands where it is made, a layer comp's restore) nor by a filtered
//! Rotate Canvas. Those ops map each linked anchor so that offset − anchor
//! moves by the same rule ([`remapped_arc`]), and a quarter turn or a flip
//! also gives each linked fill a turned copy of its tile, Photoshop's new
//! pattern under the same name ([`turned_arc`]). So the point where the layer was made is
//! [`made_at`] = offset − reference, and that is where a linked overlay set
//! without an anchor anchors (AT-CREATION) — O = phase + the reference. The
//! reference persists in `.rz` (`reference_native`, word 91) and PSD (`fxrp`
//! both ways, `psd_pattern_fill::record_reference`).
//!
//! The anchor is captured by the document setters (`set_layer_style`, the
//! typed `with_layer_style`, both through [`anchored`]): an unlinked overlay
//! drops it, a linked one without one takes the made-at point, and a linked
//! one that carries one keeps it (an echo of the style, an undo, a reopened
//! `.rz`). Every Doctored op that translates a layer moves its content and
//! offset together, so the tile follows with no rewrite. The two ops that
//! change the offset WITHOUT moving the content (Delete Cropped Pixels at
//! both depths) rebase the anchor by the same delta ([`rebased`]), which
//! keeps the picture still, and leave the reference alone.
//!
//! # Depths
//!
//! The tile's codes are the DOCUMENT's values at every depth: a 32-bit float
//! document reads code / 255 as linear (take1's VERBATIM), and a CMYK
//! document converts each texel to ink through the leg a Colour Overlay's
//! colour takes (`cmyk_style::effect_inks`, the ink pyramid
//! `pattern_tile::ink_level`, its bytes [`ink_level_bytes`]).
//!
//! # Angle
//!
//! `angle` is Photoshop's `Angl` in degrees, same sign, any finite number
//! normalized into [-180, 180) (`style_json::canonical_angle`); the writer
//! always emits it. The tile turns about its origin O (`pattern_tile`'s
//! module doc holds the fitted model and its residual). An angled overlay
//! also claims the `.rz` word 93 (`pattern_angle_native`).
//!
//! # The JSON (`style`'s module doc is the table)
//!
//! The overlay's `pattern`, `angle`, `scale`, `link_with_layer`, `phase` and
//! `anchor` are the shared pattern fill's keys, held flat in its effect
//! object; their parser and writer are `style_pattern_fill`'s, which a
//! Stroke of Fill Type Pattern shares (its `pattern_fill` object), and a
//! Bevel & Emboss texture (its `texture` object). The anchor
//! rules below reach a Pattern stroke's fill and a bevel's texture as well:
//! every stroke's and every bevel's anchor is MAPPED (a Color stroke's
//! stored tile and a switched-off texture rebase with the rest), and the
//! fill counts as painting when the stroke's Fill Type is Pattern or the
//! bevel's Texture is on.
//!
//! # The word
//!
//! [`WORD`] 90 mirrors `style_instances`' 89: one number at both depths (the
//! first word inside 45 in a byte file after the reference word 91,
//! `reference_native`; in a typed file the word after the CMYK slot and 91,
//! ahead of 89 — a CMYK file's too), no table — the overlay rides inside each record's
//! style JSON — and the claim that some stored style (a layer's or a comp's
//! recorded appearance) [`holds_pattern`]. Both directions are checked before
//! any pixel decodes: the word without such a style is [`INVALID_WORD`], such
//! a style without the word is [`WORD_REFUSAL`]. An older build refuses the
//! word by its number instead of silently dropping the effect. A document
//! with no pattern overlay writes nothing, so its bytes are the prior
//! writer's.

use std::sync::Arc;

use serde_json::{Map, Value};

use crate::adjust_lut::base64_encode;
use crate::blend::BlendMode;
use crate::doc::{DrDocument, Layer};
use crate::doc_comps::LayerComp;
use crate::pattern_tile::{preset_tile, PatternTile, PRESETS};
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::style::{BevelEmboss, Effect, LayerStyle, Strictness, Stroke, StrokeFill};
use crate::style_cache::RenderCache;
use crate::style_pattern_fill::{
    bevel_carries_texture, parse_fill, pattern_fill, placed_fill, stroke_carries_pattern,
    BevelTexture, PatternFill,
};

/// Where a pattern comes from: one of the frozen presets (stored by id) or a
/// custom tile (stored whole, with its name).
#[derive(Clone, Debug, PartialEq)]
pub enum PatternSource {
    Preset(&'static str),
    Custom { name: String },
}

/// A pattern: its source and its tile.
#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    pub source: PatternSource,
    pub tile: Arc<PatternTile>,
}

impl Pattern {
    /// The preset `id`'s pattern; `None` for an unknown id.
    pub fn preset(id: &str) -> Option<Pattern> {
        let (id, _) = PRESETS.iter().copied().find(|(p, _)| *p == id)?;
        Some(Pattern {
            source: PatternSource::Preset(id),
            tile: Arc::new(preset_tile(id)?),
        })
    }

    /// The display name: a preset's frozen name, a custom tile's own.
    pub fn name(&self) -> String {
        match &self.source {
            PatternSource::Preset(id) => PRESETS
                .iter()
                .find(|(p, _)| p == id)
                .map_or(*id, |(_, n)| *n)
                .to_string(),
            PatternSource::Custom { name } => name.clone(),
        }
    }
}

impl Default for Pattern {
    fn default() -> Self {
        Pattern::preset(PRESETS[0].0).unwrap_or_else(|| Pattern {
            source: PatternSource::Preset(PRESETS[0].0),
            tile: Arc::new(PatternTile {
                width: 1,
                height: 1,
                rgba: Arc::from(&[0xffu8, 0xff, 0xff, 0xff][..]),
            }),
        })
    }
}

/// The Pattern Overlay: its switch, blend and opacity over the
/// shared pattern fill (`style_pattern_fill`), whose keys its JSON holds flat.
#[derive(Clone, Debug, PartialEq)]
pub struct PatternOverlay {
    pub enabled: bool,
    pub blend: BlendMode,
    pub opacity: f32,
    pub fill: PatternFill,
}

impl Default for PatternOverlay {
    fn default() -> Self {
        PatternOverlay {
            enabled: true,
            blend: BlendMode::Normal,
            opacity: 1.0,
            fill: PatternFill::default(),
        }
    }
}

impl PatternOverlay {
    /// True iff the renderer emits no contribution for ANY shape
    /// (`style_knockout::has_grouped_interior_effect` relies on it mirroring
    /// the renderer). The NaN clause cannot be reached through JSON or PSD.
    pub(crate) fn skipped_by_settings(&self) -> bool {
        self.opacity.is_nan() || self.opacity <= 0.0
    }
}

// ----------------------------------------------------------------- JSON --

/// The `pattern_overlay` arm of the one parser (`style_json::effect`): the
/// fill's keys flat in the effect object (`style_pattern_fill::parse_fill`).
pub(crate) fn parse(
    o: &Map<String, Value>,
    path: &str,
    enabled: bool,
    blend: BlendMode,
    opacity: f32,
    strictness: Strictness,
) -> Result<PatternOverlay, String> {
    Ok(PatternOverlay {
        enabled,
        blend,
        opacity,
        fill: parse_fill(o, path, strictness)?,
    })
}

/// The presets as `[{"preset", "name", "width", "height", "rgba"}]` in
/// `PRESETS` order (`dr_pattern_presets_json`: the sheet's swatches).
pub fn pattern_presets_json() -> String {
    let list: Vec<Value> = PRESETS
        .iter()
        .filter_map(|(id, name)| {
            let tile = preset_tile(id)?;
            let mut o = Map::new();
            o.insert("preset".into(), Value::from(*id));
            o.insert("name".into(), Value::from(*name));
            o.insert("width".into(), Value::from(tile.width));
            o.insert("height".into(), Value::from(tile.height));
            o.insert("rgba".into(), Value::from(base64_encode(&tile.rgba)));
            Some(Value::Object(o))
        })
        .collect();
    Value::Array(list).to_string()
}

// --------------------------------------------------------------- anchors --

/// `style` with every pattern fill's anchor mapped by `f` — an overlay's,
/// every stroke's `pattern_fill` and every bevel's texture fill; `f`'s
/// second argument says whether the fill paints (an overlay, a stroke whose
/// Fill Type is Pattern, a bevel whose Texture is on). `None` when nothing
/// changes. Fresh cache.
fn mapped(
    style: &LayerStyle,
    f: impl Fn(&PatternFill, bool) -> Option<[i32; 2]>,
) -> Option<LayerStyle> {
    refilled(style, |fill, paints| {
        let anchor = f(fill, paints);
        (anchor != fill.anchor).then(|| PatternFill {
            anchor,
            ..fill.clone()
        })
    })
}

/// `style` with every pattern fill [`mapped`] walks, in its order, replaced
/// by `f`'s answer (`f`'s second argument as there); a `None` keeps the fill.
/// `None` when `f` replaces nothing. Fresh cache.
fn refilled(
    style: &LayerStyle,
    mut f: impl FnMut(&PatternFill, bool) -> Option<PatternFill>,
) -> Option<LayerStyle> {
    let mut changed = false;
    let mut moved = |fill: &PatternFill, paints: bool| match f(fill, paints) {
        Some(next) => {
            changed = true;
            next
        }
        None => fill.clone(),
    };
    let effects = style
        .effects
        .iter()
        .map(|e| match e {
            Effect::PatternOverlay(p) => Effect::PatternOverlay(PatternOverlay {
                fill: moved(&p.fill, true),
                ..p.clone()
            }),
            Effect::Stroke(s) => Effect::Stroke(Stroke {
                pattern_fill: moved(&s.pattern_fill, s.fill_type == StrokeFill::Pattern),
                ..s.clone()
            }),
            Effect::BevelEmboss(b) => Effect::BevelEmboss(BevelEmboss {
                texture: BevelTexture {
                    fill: moved(&b.texture.fill, b.texture_enabled),
                    ..b.texture.clone()
                },
                ..b.clone()
            }),
            other => other.clone(),
        })
        .collect();
    changed.then(|| LayerStyle {
        effects,
        cache: RenderCache::default(),
        ..style.clone()
    })
}

/// Where a layer at `offset` with reference point `reference` was MADE
/// (module doc): offset − reference, saturating.
pub(crate) fn made_at(offset: (i32, i32), reference: (i32, i32)) -> (i32, i32) {
    (
        offset.0.saturating_sub(reference.0),
        offset.1.saturating_sub(reference.1),
    )
}

impl LayerStyle {
    /// This style with every pattern fill's anchor dropped — an overlay's,
    /// every stroke's `pattern_fill` and every bevel's texture: what a style
    /// PRESET keeps (`style_presets`), since an anchor is a fact of the layer
    /// the style was set on, and the setter re-anchors it on the next one.
    /// The same style back (equal) when no fill carries an anchor.
    pub(crate) fn without_anchors(&self) -> LayerStyle {
        mapped(self, |_, _| None).unwrap_or_else(|| self.clone())
    }
}

/// `style` set on a layer made at `made_at` (module doc): an unlinked
/// pattern fill drops its anchor, a linked one without one takes `made_at`,
/// a linked one with one keeps it — for an overlay, a stroke whose Fill
/// Type is Pattern and a bevel whose Texture is on; any other stroke or
/// bevel is left alone. The same style back (equal) when nothing changes.
pub(crate) fn anchored(style: &LayerStyle, made_at: (i32, i32)) -> LayerStyle {
    mapped(style, |fill, paints| {
        if !paints {
            fill.anchor
        } else if !fill.link_with_layer {
            None
        } else {
            Some(fill.anchor.unwrap_or([made_at.0, made_at.1]))
        }
    })
    .unwrap_or_else(|| style.clone())
}

/// [`anchored`] on a shared style: the SAME `Arc` when nothing changes, so a
/// setter keeps its equality test and the cache stays warm.
pub(crate) fn anchored_arc(style: Arc<LayerStyle>, made_at: (i32, i32)) -> Arc<LayerStyle> {
    if !holds_pattern_fill(&style) {
        return style;
    }
    let next = anchored(&style, made_at);
    if next == *style {
        style
    } else {
        Arc::new(next)
    }
}

/// `style` with every anchor, overlay or stroke, moved by `delta`
/// (saturating): the content did not move while the offset did (module doc).
pub(crate) fn rebased(style: &LayerStyle, delta: (i64, i64)) -> LayerStyle {
    let shift =
        |v: i32, d: i64| (i64::from(v) + d).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    mapped(style, |fill, _| {
        fill.anchor
            .map(|a| [shift(a[0], delta.0), shift(a[1], delta.1)])
    })
    .unwrap_or_else(|| style.clone())
}

/// [`rebased`] on an optional shared style: the same `Arc` when nothing
/// changes.
pub(crate) fn rebased_arc(
    style: &Option<Arc<LayerStyle>>,
    delta: (i64, i64),
) -> Option<Arc<LayerStyle>> {
    let current = style.as_ref()?;
    if delta == (0, 0) || !holds_pattern_fill(current) {
        return Some(Arc::clone(current));
    }
    let next = rebased(current, delta);
    if next == **current {
        Some(Arc::clone(current))
    } else {
        Some(Arc::new(next))
    }
}

/// `after` (which is `before` or `before.scaled(f)`: the same effect order)
/// with every fill's anchor replaced by `map(before's anchor of the same
/// fill)`; a fill without an anchor in `before`, or a `None` from `map`,
/// keeps `after`'s anchor. The same `Arc` when nothing changes, and `after`
/// as it is when the two styles' fills do not pair up.
pub(crate) fn remapped_arc(
    after: &Option<Arc<LayerStyle>>,
    before: Option<&LayerStyle>,
    map: impl Fn([i32; 2]) -> Option<[i32; 2]>,
) -> Option<Arc<LayerStyle>> {
    let current = after.as_ref()?;
    let same = Some(Arc::clone(current));
    let Some(before) = before else {
        return same;
    };
    if !holds_pattern_fill(current) {
        return same;
    }
    // `before`'s anchors in `mapped`'s own walk order, so the pairing can
    // never drift from the order every other anchor rule uses.
    let anchors = std::cell::RefCell::new(Vec::new());
    mapped(before, |fill, _| {
        anchors.borrow_mut().push(fill.anchor);
        fill.anchor
    });
    let anchors = anchors.into_inner();
    let visited = std::cell::Cell::new(0usize);
    mapped(current, |fill, _| {
        visited.set(visited.get() + 1);
        fill.anchor
    });
    if visited.get() != anchors.len() {
        return same;
    }
    let index = std::cell::Cell::new(0usize);
    let next = mapped(current, |fill, _| {
        let i = index.get();
        index.set(i + 1);
        anchors[i].and_then(&map).or(fill.anchor)
    });
    match next {
        Some(next) if next != **current => Some(Arc::new(next)),
        _ => same,
    }
}

/// `after` (which is `before`, or `before` with its anchors remapped: the
/// same effect order) with every LINKED fill, in [`mapped`]'s walk, holding
/// `turn(before's pattern of the same fill)` (`doc_reference::TurnedTiles`).
/// An unlinked fill, a `None` from `turn`, or styles whose fills do not pair
/// up keep `after`'s pattern. The same `Arc` when nothing changes.
pub(crate) fn turned_arc(
    after: &Option<Arc<LayerStyle>>,
    before: Option<&LayerStyle>,
    turn: impl Fn(&Pattern) -> Option<Pattern>,
) -> Option<Arc<LayerStyle>> {
    let current = after.as_ref()?;
    let same = Some(Arc::clone(current));
    let Some(before) = before else {
        return same;
    };
    if !holds_pattern_fill(current) {
        return same;
    }
    let mut patterns = Vec::new();
    refilled(before, |fill, _| {
        patterns.push(fill.pattern.clone());
        None
    });
    let mut visited = 0usize;
    refilled(current, |_, _| {
        visited += 1;
        None
    });
    if visited != patterns.len() {
        return same;
    }
    let mut index = 0usize;
    let next = refilled(current, |fill, _| {
        let i = index;
        index += 1;
        if !fill.link_with_layer {
            return None;
        }
        let pattern = turn(&patterns[i])?;
        Some(PatternFill {
            pattern,
            ..fill.clone()
        })
    });
    next.map_or(same, |next| Some(Arc::new(next)))
}

/// Whether `style` holds a pattern fill an anchor rule reaches: any pattern
/// overlay, any stroke that [`stroke_carries_pattern`], or any bevel that
/// [`bevel_carries_texture`] — what CARRIES an anchor, not only what
/// paints, so a Color stroke's stored tile and a switched-off texture rebase
/// too.
pub(crate) fn holds_pattern_fill(style: &LayerStyle) -> bool {
    style.effects.iter().any(|e| match e {
        Effect::PatternOverlay(_) => true,
        Effect::Stroke(s) => stroke_carries_pattern(s),
        Effect::BevelEmboss(b) => bevel_carries_texture(b),
        _ => false,
    })
}

/// The bytes every ENABLED placed tile's PYRAMID level allocates — an
/// overlay's, a Pattern stroke's or an enabled bevel texture's
/// (`style_pattern_fill::placed_fill`; `pattern_tile::TileLevel`: 16 bytes a
/// texel) — the style scratch plan's pattern term
/// (`precision_style_render::style_scratch`). A stroke builds its level once
/// for all its contributions, so each fill is charged once.
pub(crate) fn level_bytes(style: &LayerStyle) -> u64 {
    style
        .enabled_effects()
        .filter_map(placed_fill)
        .map(|fill| {
            let tile = &fill.pattern.tile;
            let (w, h) = crate::pattern_tile::level_dims(
                tile.width,
                tile.height,
                f64::from(fill.scale),
                f64::from(fill.angle),
            );
            16 * u64::from(w) * u64::from(h)
        })
        .sum()
}

/// The bytes a CMYK record's ink pattern allocates beside its planes
/// (`cmyk_style::styled_scratch`): for every ENABLED pattern fill (overlay or
/// Pattern stroke), the tile converted to premultiplied ink and its ink
/// PYRAMID level (`pattern_tile::InkLevel`), 20 bytes a texel each — an exact
/// term, like [`level_bytes`], because `cmyk_style::effect_inks` builds one
/// ink level per effect, shared by a Center stroke's two contributions.
pub(crate) fn ink_level_bytes(style: &LayerStyle) -> u64 {
    style
        .enabled_effects()
        .filter_map(pattern_fill)
        .map(|fill| {
            let tile = &fill.pattern.tile;
            crate::pattern_tile::ink_scratch_bytes(
                tile.width,
                tile.height,
                f64::from(fill.scale),
                f64::from(fill.angle),
            )
        })
        .sum()
}

// ------------------------------------------------------------------ word --

/// The conditional `.rz` word at both depths (module doc).
pub(crate) const WORD: u32 = 90;

/// A pattern overlay in a file that does not claim the word.
pub(crate) const WORD_REFUSAL: &str = "native pattern overlays require DRDC 90";

/// The word in a file holding no pattern overlay.
pub(crate) const INVALID_WORD: &str = "invalid native pattern overlay word";

/// Whether `style` holds a pattern overlay, enabled or not.
pub fn holds_pattern(style: &LayerStyle) -> bool {
    style
        .effects
        .iter()
        .any(|e| matches!(e, Effect::PatternOverlay(_)))
}

/// Whether some comp's recorded appearance holds a pattern overlay (read
/// leniently, as the `.rz` readers read it; one that does not parse claims
/// nothing).
pub(crate) fn comps_pattern(comps: &[LayerComp]) -> bool {
    comps.iter().any(|comp| {
        comp.entries.iter().any(|entry| {
            entry
                .appearance
                .as_ref()
                .and_then(|a| a.style.as_deref())
                .and_then(|json| LayerStyle::from_json_lenient(json).ok())
                .is_some_and(|style| holds_pattern(&style))
        })
    })
}

/// The byte word's predicate: some layer's STORED style or some comp's
/// recorded appearance holds a pattern overlay.
pub(crate) fn styles_pattern(doc: &DrDocument) -> bool {
    pending_pattern(&doc.layers, &doc.comps)
}

/// The byte predicate over a reader's pending records and parsed comps.
pub(crate) fn pending_pattern(layers: &[Layer], comps: &[LayerComp]) -> bool {
    layers
        .iter()
        .any(|l| l.style.as_deref().is_some_and(holds_pattern))
        || comps_pattern(comps)
}

/// The typed predicate: some record of the tree, or some comp.
pub(crate) fn styles_pattern16(doc: &PrecisionDocument) -> bool {
    records_pattern16(&doc.layers) || comps_pattern(&doc.comps)
}

fn records_pattern16(layers: &[PrecisionDocumentLayer]) -> bool {
    layers.iter().any(|l| {
        l.style().is_some_and(|s| holds_pattern(s)) || l.children().is_some_and(records_pattern16)
    })
}
