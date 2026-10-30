//! Layer styles: the per-layer effect stack plus the blending options beyond
//! opacity and mode — `fill_opacity` and Blend If — and the document's shared
//! global light. This module holds the MODEL: the types, their defaults, the
//! identity rule and the semantics helpers the compositor and the transforms
//! need. The JSON codec lives in `style_json`, the plane renderer and its
//! cache in `style_render`, the compositing assembly in `style_composite`,
//! and each effect's algorithm in its own `style_fx_*` module. The header's
//! "Layer styles" section points here for the full key table — this module
//! doc IS the schema, the way `adjust`'s is for adjustment meta.
//!
//! # The style JSON (the contract across the FFI, in `.rz`, and over MCP)
//!
//! One JSON object per layer:
//!
//! ```json
//! {
//!   "version": 1,
//!   "fill_opacity": 1.0,
//!   "blend_if": null,
//!   "effects": [ { "type": "drop_shadow", ... }, ... ]
//! }
//! ```
//!
//! | key            | type / range                                                        | default |
//! |----------------|---------------------------------------------------------------------|---------|
//! | `version`      | integer, ignored on input, written as 1                             | 1       |
//! | `fill_opacity` | number 0..=1 (opacity of the PIXELS only, never of effects); composited at its byte below 32 bits (`opacity_byte`) | 1.0 |
//! | `blend_if`     | `null` or the object below                                          | null    |
//! | `knockout`     | `"none"` \| `"shallow"` \| `"deep"` (`style_knockout`)              | `"none"`|
//! | `blend_interior_as_group` | bool — the layer's blend mode applies to the five GROUPED interior effects (`style_knockout`) | false |
//! | `effects`      | array of effect objects: up to 10 of a repeating `type`, one of any other (`style_instances`) | `[]`    |
//!
//! `knockout` and `blend_interior_as_group` are the ONE exception to "the
//! canonical writer emits EVERY key": each is written only when it is NOT at
//! its default (see below), so every style, `.rz` file, `LSTY` body, comps
//! payload and committed PSD golden authored without them is byte-identical to
//! what the writer before them produced. A GradientFill's `dither`
//! follows the same rule: written only when true. So does a stroke's
//! `pattern_fill`: written only when its Fill Type is Pattern or the
//! fill is not the default (`style_pattern_fill::stroke_carries_pattern`).
//!
//! `blend_if` object: `channel` — `"gray"` | `"red"` | `"green"` | `"blue"` |
//! `"cyan"` | `"magenta"` | `"yellow"` | `"black"` (default `"gray"`; the
//! three colour channels serve an RGB document and the four plates a CMYK
//! one, each refused by name on the other, #1); `this_layer` and
//! `underlying` — arrays of exactly 4 integers 0..=255 with
//! `lo0 <= lo1 <= hi0 <= hi1` (default `[0, 0, 255, 255]` = full weight
//! everywhere); `extra` — optional array of objects with the
//! same three keys, one more range per channel, whose weights MULTIPLY with
//! the primary's (`style_blend_if`). A channel named twice is malformed. The
//! parser canonicalizes (`BlendIf::from_ranges`): identity ranges dropped,
//! the rest in the order gray, red, green, blue, cyan, magenta, yellow,
//! black, the first of them the primary keys. `extra` is written only when
//! more than one range remains, so every single-channel style is
//! byte-identical.
//!
//! CMYK INKS (#1, `cmyk_style`): beside every colour slot a CMYK document's
//! style may carry the slot's ink, straight `[c, m, y, k]` in `0..=1`
//! (four-decimal quantized, clamped): `ink` beside `color` on drop_shadow,
//! inner_shadow, outer_glow, inner_glow, stroke, color_overlay and satin;
//! `highlight_ink` / `shadow_ink` on bevel_emboss; `ink` on a gradient stop.
//! Each is optional and written ONLY when present, so every RGB style, `.rz`,
//! `LSTY` body and PSD golden without one is byte-identical. On a CMYK
//! document the ink is AUTHORITATIVE and its `color` is the ink's sRGB under
//! the document's profile (`cmyk_style::canonical_hex`); a slot with no ink
//! converts its `color` at composite time. An RGB, float or byte setter drops
//! every ink (`LayerStyle::without_inks`).
//!
//! WIDE COLOURS (`authored_color::wide_into`): beside the same seven
//! effects' `color` a slot may carry `linear`, and a bevel `highlight_linear`
//! / `shadow_linear`: `[r, g, b]`, the colour as LINEAR-LIGHT sRGB-primaries
//! components, extended range (negative outside sRGB's gamut), each clamped
//! into `±4` and NOT quantized (written as the stored f32 widened to f64, so
//! it round-trips exactly); anything but 3 finite numbers is `<path>.linear
//! must be an array of 3 numbers` in both modes. A `linear` that IS an sRGB
//! byte (components in 0..=1, its sRGB numbers within 5e-3 of a code) is
//! dropped and `color` becomes that byte; otherwise `color` is replaced by
//! the nearest byte of its clamped sRGB numbers, so every reader of `color`
//! agrees with it. Written ONLY when present, so every style without one is
//! byte-identical. The composite converts it into the document's numbers at
//! 8, 16 and 32 bits; a CMYK document reads the slot's ink or `color`; a
//! gradient stop has none. In `.rz` it claims conditional word 100
//! (`wide_color_native`). `LayerStyle::map_wide_slots` is the one walk over
//! the wide slots (a Grayscale document greys them, `gray_style`).
//!
//! Every effect object has `type` (required string) and `enabled` (bool,
//! default true; a disabled effect is kept, serialized, and not rendered).
//! `blend` values are the 27 layer blend modes as snake_case names:
//! `normal, dissolve, darken, multiply, color_burn, linear_burn, darker_color,
//! lighten, screen, color_dodge, linear_dodge, lighter_color, overlay,
//! soft_light, hard_light, vivid_light, linear_light, pin_light, hard_mix,
//! difference, exclusion, subtract, divide, hue, saturation, color,
//! luminosity` (`linear_dodge` is `BlendMode::Addition`). Colours are
//! `"#rrggbb"` (case-insensitive; `"#rrggbbaa"` accepted, alpha ignored).
//! Angles are degrees (0 = light from the right, 90 = from the top). Pixel
//! sizes are canvas pixels.
//!
//! | `type`             | keys (type, range, default)                                                    |
//! |--------------------|--------------------------------------------------------------------------------|
//! | `drop_shadow`      | `blend` (multiply), `color` (#000000), `opacity` 0..=1 (0.75), `angle` (120),  |
//! |                    | `use_global_light` (true), `distance` px 0..=30000 (5), `spread` 0..=1 fraction |
//! |                    | of size (0), `size` px 0..=250 (5), `layer_knocks_out` bool (true — "Layer     |
//! |                    | Knocks Out Drop Shadow": the shadow is not drawn under the layer's own shape)   |
//! | `inner_shadow`     | `blend` (multiply), `color` (#000000), `opacity` (0.75), `angle` (120),         |
//! |                    | `use_global_light` (true), `distance` (5), `choke` 0..=1 (0), `size` (5)       |
//! | `outer_glow`       | `blend` (screen), `color` (#ffffbe), `opacity` (0.75), `spread` (0), `size` (5) |
//! | `inner_glow`       | `blend` (screen), `color` (#ffffbe), `opacity` (0.75), `choke` (0), `size` (5), |
//! |                    | `source` `"edge"` \| `"center"` (edge)                                          |
//! | `stroke`           | `blend` (normal), `opacity` (1.0), `size` px 0..=250 (3), `position`            |
//! |                    | `"outside"` \| `"inside"` \| `"center"` (outside), `fill_type` `"color"` \|     |
//! |                    | `"gradient"` \| `"pattern"` (color), `color` (#000000), `gradient`              |
//! |                    | (GradientFill; its `style` may also be `"shape_burst"`, the stroke's own,       |
//! |                    | `style_stroke_fill`), `pattern_fill` (PatternFill, below)                       |
//! | `color_overlay`    | `blend` (normal), `color` (#ff0000), `opacity` (1.0)                            |
//! | `gradient_overlay` | `blend` (normal), `opacity` (1.0), `gradient` (GradientFill)                    |
//! | `pattern_overlay`  | `blend` (normal), `opacity` (1.0), and the PatternFill keys held flat:         |
//! |                    | `pattern` (`{"preset": id}` with id one of                                      |
//! |                    | checkerboard, stripes, dots, grid — the default checkerboard — or `{"name",     |
//! |                    | "width", "height", "rgba"}`: sides 1..=512, base64 RGBA8), `angle` (0: any    |
//! |                    | number, normalized into -180..180, always written), `scale` 0.01..=10 (1.0),   |
//! |                    | `link_with_layer` (true), `phase` [x, y] px each -30000..=30000 ([0, 0]),      |
//! |                    | `anchor` [x, y] integers: where the layer was made (offset − reference; written |
//! |                    | only when set; `style_pattern`)                                                 |
//! | `bevel_emboss`     | `style` `"outer_bevel"` \| `"inner_bevel"` \| `"emboss"` \| `"pillow_emboss"`  |
//! |                    | (inner_bevel), `depth` 0.01..=10 (1.0 = 100 %), `direction` `"up"` \|           |
//! |                    | `"down"` (up), `size` px 0..=250 (5), `soften` px 0..=16 (0), `angle` (120),    |
//! |                    | `use_global_light` (true), `altitude` 0..=90 (30), `highlight_blend` (screen),  |
//! |                    | `highlight_color` (#ffffff), `highlight_opacity` (0.75), `shadow_blend`         |
//! |                    | (multiply), `shadow_color` (#000000), `shadow_opacity` (0.75), `technique`      |
//! |                    | `"smooth"` \| `"chisel_hard"` \| `"chisel_soft"` (smooth, written only when    |
//! |                    | not smooth; #2)                                                               |
//! | `satin`            | `blend` (multiply), `color` (#000000), `opacity` (0.5), `angle` (19),           |
//! |                    | `distance` px (11), `size` px (14), `invert` bool (true)                        |
//! | contours           | `drop_shadow`, `inner_shadow`, `satin`: `contour` (Contour, Linear),            |
//! |                    | `anti_aliased` bool (false); `outer_glow`, `inner_glow`: those plus `range`     |
//! |                    | 0.01..=1 (0.5 = 50 %); `bevel_emboss`: `gloss_contour` (Linear),                |
//! |                    | `gloss_anti_aliased` (false), `contour_enabled` (false), `contour` (Linear),    |
//! |                    | `contour_anti_aliased` (false), `contour_range` 0.01..=1 (0.5). Each is written |
//! |                    | only when it is not at its default (`style_contour_json`)                       |
//! | noise              | `drop_shadow`, `inner_shadow`, `outer_glow`, `inner_glow`: `noise` 0..=1 (0 =   |
//! |                    | 0 %; `style_noise`), written only when above 0                                  |
//! | texture            | `bevel_emboss`: `texture_enabled` bool (false, written only when true),         |
//! |                    | `texture` (BevelTexture, below; written only when Texture is on or the texture  |
//! |                    | is not the default, `style_pattern_fill::bevel_carries_texture`)                |
//! | glow fill          | `outer_glow`, `inner_glow`: `fill_type` `"color"` \| `"gradient"` (color,       |
//! |                    | written only when gradient), `gradient` (GradientFill, its `stops` only: every  |
//! |                    | other key is reset to its default; written only when the fill is Gradient or   |
//! |                    | the stops are not the default), `jitter` 0..=1 (0 = 0 %; written                |
//! |                    | only when above 0; `style_glow_gradient`)                                       |
//!
//! Contour object (`style_contour`): `{"preset": id}` with id one of
//! linear, cone, cone_inverted, gaussian, half_round, ring, ring_double,
//! rolling_slope_descending, rounded_steps, sawtooth_1, or `{"points":
//! [{"x", "y", "corner"?}]}`: 2..=16 points, each value clamped into 0..=255
//! and quantized, stable-sorted by `x`, a duplicate `x` keeping the first;
//! fewer than 2 distinct `x` is `effects[i].contour.points must hold 2 to 16
//! points`, a non-object `effects[i].contour must be an object`; an unknown
//! preset is malformed when strict and Linear when lenient; `corner` is
//! written only when true.
//!
//! GradientFill object (shared by `stroke` and `gradient_overlay`; a glow's
//! keeps its `stops` only): `stops` —
//! array of 2..=32 `{ "position": 0..=1, "color": hex, "opacity": 0..=1 }`,
//! each with optional `midpoint` and `opacity_midpoint` 0.05..=0.95 (0.5;
//! clamped, rounded to 0.01, written only when not 0.5; the last stop's reset
//! to 0.5 after the sort; `gradient_field::midpoint_fraction`; a glow's
//! and a legacy gradient map's reset to 0.5, a `method: "classic"` or
//! `"perceptual"` map's kept)
//! (stable-sorted by position on parse; default
//! `[{0, "#000000", 1}, {1, "#ffffff", 1}]`); `style` `"linear"` |
//! `"radial"` | `"angle"` | `"reflected"` | `"diamond"` (linear), and on a
//! `stroke` only `"shape_burst"` (`t` follows the band's distance, the
//! box and angle unused; on a `gradient_overlay` it is an unknown enum
//! string); `angle` deg (90); `scale` 0.1..=1.5 (1.0); `reverse` bool
//! (false); `align_with_layer` bool (true); `dither` bool (false, written
//! only when true; `style_gradient`); `smoothness` 0..=1 (1 = 100 %,
//! the Classic ramp; 0 the linear one; written only when not 1;
//! `gradient_field::smooth_color`; a glow's resets to 1, a legacy gradient
//! map ignores it and a `method: "classic"` or `"perceptual"` map reads it).
//!
//! PatternFill object (a `stroke`'s `pattern_fill`, a
//! `pattern_overlay`'s own keys): `pattern`, `angle`, `scale`,
//! `link_with_layer`, `phase`, `anchor` as in the `pattern_overlay` row, with
//! the same ranges, defaults and refusals (`style_pattern_fill`); a stroke's
//! refusals name `effects[i].pattern_fill.<key>`, and a `pattern_fill` that
//! is not an object is `effects[i].pattern_fill must be an object`.
//!
//! BevelTexture object (a `bevel_emboss`'s `texture`,
//! `style_texture`): the PatternFill keys but `angle` (a parsed `angle` is
//! discarded and none is written: the PSD bevel texture has no Angle), plus
//! `depth` -10..=10 (1.0 = +100 %) and `invert` bool (false);
//! refusals name `effects[i].texture.<key>`, and a `texture` that is not an
//! object is `effects[i].texture must be an object`.
//!
//! Parse rules (STRICT mode, the FFI setter): unknown keys anywhere are
//! ignored by the parser, but both style SETTERS refuse one by name after it
//! (`style_json_keys`); a missing key takes its default; a key
//! present with the wrong JSON type, a non-finite number, an unknown enum
//! string, an unparseable colour, an unknown effect `type`, a second effect
//! of a single `type` (`duplicates an earlier effect`), an eleventh of a
//! repeating one (`exceeds 10 instances of <type>`), an unknown `knockout`
//! string, or a Blend If ramp out of order makes the whole style
//! malformed (an error naming the path, e.g. `effects[1].size must be a
//! number`). Numeric ranges are CLAMPED, not refused; angles are normalized
//! into `[-180, 180)`; every number is quantized to 4 decimals at parse time
//! so the stored value IS its canonical form. LENIENT mode (the DRDC reader
//! only): an unknown effect `type` is skipped, a duplicate single `type`
//! keeps the first and a repeating one its first ten, an unknown enum string
//! takes the enum's default; every other rule stays strict. One parser, one
//! `Strictness` parameter. The canonical writer (`to_json`) emits EVERY key
//! of every present effect, effects in render order (within a repeating
//! kind the later instance draws on top), sorted keys, lowercase
//! colours — with the conditional keys stated in the table above (the two
//! knockout-family keys, the inks, `dither`, `smoothness`, the stop
//! midpoints, `pattern_fill`, `anchor` and the contour keys, `noise`,
//! `texture_enabled` and `texture`, and a glow's
//! `fill_type`, `gradient` and `jitter`). Stroke emboss and glow technique
//! are NOT modelled.
//!
//! # Conventions (numerics; one home each, in `style_render`)
//!
//! Light offset `(round(-d cos a), round(+d sin a))` in y-down pixel space
//! (120° casts down-right). `size` is the blur extent in px with Gaussian
//! `sigma = size / 2` (bevel, except a Smooth bevel, whose measured law is
//! a box pair, `style_bevel_smooth`; the outer glow, drop
//! shadow, inner shadow and satin `size / 2.5`, their `style_matte`
//! members);
//! `spread`/`choke` is a fraction of size — dilate (or erode) by
//! `size * spread`, then blur by `size * (1 - spread)`. The inner glow is
//! the measured law where `size * (1 - choke) > 0`, a band of `round(S *
//! choke)` px (`S` the rounded size), measured from the anti-aliased
//! contour (`style_aa_band`), and a box pair of the rest, with no Gaussian
//! (`style_fx_inner_glow`). Effects
//! with `use_global_light` read the document's `GlobalLight` (angle,
//! altitude; defaults 120°, 30°) instead of their own angle.
//!
//! # Identity, lifetime, scaling
//!
//! A style is IDENTITY when `fill_opacity == 1`, `blend_if` is absent or
//! full-weight, `knockout` is `none` and no effect is enabled.
//! `blend_interior_as_group` is deliberately NOT part of the question: alone
//! it renders nothing (it only re-routes GROUPED interior effects, of which
//! there are none), so a style carrying nothing but that flag clears.
//! THE ONE RULE:
//! [`DrDocument::set_layer_style`] with an identity style behaves exactly
//! like clearing — an identity style is never stored, so
//! `Layer::style.is_some()` means "renders something" on every surface.
//!
//! A style rides along like meta (setters, painting, mask ops, geometry,
//! reordering, crop/canvas-resize), is copied by Duplicate Layer, and is
//! DROPPED where the layer stops being itself: Merge Down and Flatten bake
//! the effects into pixels through the projection and clear it. An RGB
//! adjustment layer's style takes its shape from the layer's mask
//! (`style_adjustment`). Free Transform, perspective
//! and Image Size apply "Scale Effects" in the core ([`LayerStyle::scaled`]
//! by the mean scale factor), so every host path agrees.
//!
//! `LayerStyle` (the identity rule, `pad`, Scale Effects), `scaled_style`/`quad_mean_scale`, and
//! the `impl DrDocument` block (`set_layer_style`, `set_global_light`).

use std::sync::Arc;

use crate::adjust::Adjustment;
use crate::doc::{DrDocument, Layer};
use crate::style_cache::RenderCache;
use crate::style_json::{Range, RANGES};
use crate::style_render::light_offset;

pub use crate::pattern_tile::{PatternTile, MAX_TILE_SIDE};
pub use crate::style_contour::{
    Contour, ContourCurve, ContourPoint, ContourPreset, DEFAULT_RANGE, MAX_CONTOUR_POINTS,
};
pub use crate::style_effects::{
    BevelEmboss, ColorOverlay, DropShadow, Effect, EffectKind, GradientOverlay, InnerGlow,
    InnerShadow, OuterGlow, Satin, Stroke,
};
pub use crate::style_instances::{is_stacked, MAX_INSTANCES};
pub use crate::style_model::{
    BevelDirection, BevelStyle, BevelTechnique, BlendIf, BlendIfChannel, BlendIfRange, GlobalLight,
    GlowFill, GlowSource, GradientFill, GradientStop, GradientStyle, Knockout, Strictness,
    StrokeFill, StrokePosition, FULL_RAMP, MAX_EXTRA_RANGES,
};
pub use crate::style_names::{blend_mode_from_name, blend_mode_name, color_hex, parse_color};
pub use crate::style_pattern::{
    holds_pattern, pattern_presets_json, Pattern, PatternOverlay, PatternSource,
};
pub use crate::style_pattern_fill::{BevelTexture, PatternFill};

/// Largest padding a style may ask a shape plane for, in px on each side.
/// The per-effect caps (size 250, soften 16, distance 30000) already bound
/// every reach except the inner shadow's `distance`, which shifts inside the
/// plane; 1024 keeps a hostile distance from turning a 1x1 layer into a
/// multi-megapixel plane while still covering every size the dialog offers.
pub(crate) const MAX_PAD: u32 = 1024;

// ------------------------------------------------------------ the style --

/// A layer's complete style: the blending options and the effect stack.
/// INVARIANT: `effects` is stable-sorted by [`Effect::kind`], with at most
/// one effect of a single kind and at most [`MAX_INSTANCES`] of a kind that
/// [`EffectKind::repeats`] (`style_instances`: within a kind, array order is
/// render order) — the parser enforces it and [`LayerStyle::normalized`]
/// re-establishes it for directly constructed values.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerStyle {
    pub fill_opacity: f32,
    pub blend_if: Option<BlendIf>,
    /// Blending Options > Knockout (`style_knockout`). `Knockout::None` is
    /// today's compositing statement for statement.
    pub knockout: Knockout,
    /// Blending Options > "Blend Interior Effects as Group": the layer's own
    /// blend mode applies to Inner Glow, Satin and the three Overlays
    /// (`style_knockout`). Renders nothing on its own, which is why it is not
    /// part of the identity question.
    pub blend_interior_as_group: bool,
    pub effects: Vec<Effect>,
    /// The rendered-plane cache (see `style_render`); lives in the style so
    /// every document sharing this `Arc<LayerStyle>` shares the planes.
    pub(crate) cache: RenderCache,
}

impl Default for LayerStyle {
    fn default() -> Self {
        LayerStyle {
            fill_opacity: 1.0,
            blend_if: None,
            knockout: Knockout::None,
            blend_interior_as_group: false,
            effects: Vec::new(),
            cache: RenderCache::default(),
        }
    }
}

impl LayerStyle {
    /// True when the style renders nothing at all: fill opacity 1, no
    /// (effective) Blend If, knockout `None`, and no ENABLED effect. Identity
    /// styles are never stored (see [`DrDocument::set_layer_style`]).
    ///
    /// `blend_interior_as_group` is NOT here on purpose (module doc): without
    /// a grouped interior effect it changes no pixel, so a style carrying only
    /// that flag clears exactly as an empty one does.
    pub fn is_identity(&self) -> bool {
        self.fill_opacity >= 1.0
            && self.blend_if.is_none_or(|b| b.is_identity())
            && self.knockout == Knockout::None
            && !self.has_enabled_effects()
    }

    /// Whether any effect is enabled (the only case that renders planes).
    pub fn has_enabled_effects(&self) -> bool {
        self.effects.iter().any(Effect::enabled)
    }

    /// The FIRST effect of `kind` (its lowest instance), present or not
    /// enabled.
    pub fn effect(&self, kind: EffectKind) -> Option<&Effect> {
        self.effects.iter().find(|e| e.kind() == kind)
    }

    /// Enabled effects only, in render order.
    pub(crate) fn enabled_effects(&self) -> impl Iterator<Item = &Effect> {
        self.effects.iter().filter(|e| e.enabled())
    }

    /// Padding, in px on each side, the shape plane needs so every enabled
    /// effect renders exactly as it would on an infinite plane: the largest
    /// per-effect reach plus one, capped at [`MAX_PAD`]. Zero with no
    /// enabled effect. Because a reach also bounds how far an input change
    /// moves an output, this is the radius of the cache's incremental
    /// re-render windows too (`style_cache`).
    pub fn pad(&self) -> u32 {
        self.wanted_pad().min(MAX_PAD)
    }

    /// The padding the enabled effects ask for before the cap.
    fn wanted_pad(&self) -> u32 {
        self.enabled_effects()
            .map(|e| e.reach().saturating_add(1))
            .max()
            .unwrap_or(0)
    }

    /// True when [`pad`](Self::pad) is the cap rather than the effects'
    /// full reach. A FULL render stays exact then: only an inner shadow's
    /// or a satin's `distance` can exceed the cap, their shifts fill from
    /// beyond the plane with what an infinite plane holds there (255 for
    /// the inverted plane, 0 for the blurred coverage — every blur reach is
    /// far below the cap), and both are clipped to the shape. But an input
    /// change can then move an output by MORE than `pad`, so the cache's
    /// incremental sub-plane re-render — whose windows are sized by `pad` —
    /// must fall back to a full render (`style_cache`).
    pub(crate) fn pad_is_capped(&self) -> bool {
        self.wanted_pad() > MAX_PAD
    }

    /// The composite-time offset of every enabled drop shadow under `light`
    /// (`style_render::light_offset` of its resolved angle and distance), in
    /// render order — the only contributions that land beyond the padded
    /// plane, and what Merge Down grows its extent by in those directions.
    /// Empty without an enabled drop shadow.
    pub fn shadow_shifts(&self, light: GlobalLight) -> Vec<(i32, i32)> {
        self.enabled_effects()
            .filter_map(|e| match e {
                Effect::DropShadow(d) => Some(light_offset(d.resolved_angle(light), d.distance)),
                _ => None,
            })
            .collect()
    }

    /// The enabled gradients (stroke fill or overlay), if any.
    fn gradients(&self) -> impl Iterator<Item = &GradientFill> {
        self.enabled_effects().filter_map(|e| match e {
            Effect::Stroke(s) if s.fill_type == StrokeFill::Gradient => Some(&s.gradient),
            Effect::GradientOverlay(g) => Some(&g.gradient),
            _ => None,
        })
    }

    /// True when a rendered plane depends on where the layer sits on the
    /// canvas: a gradient with `align_with_layer == false`, or an enabled
    /// placed tile (an overlay, a Pattern stroke or a bevel's enabled
    /// texture, `style_pattern_fill::placed_fill`) without Link with Layer
    /// (its tile stays on the canvas grid, `style_pattern`).
    pub fn needs_canvas_alignment(&self) -> bool {
        self.gradients().any(|g| !g.align_with_layer)
            || self
                .enabled_effects()
                .filter_map(crate::style_pattern_fill::placed_fill)
                .any(|fill| !fill.link_with_layer)
    }

    /// True when a rendered plane depends on the layer's content bounds: a
    /// gradient with `align_with_layer == true` spans them (the cache cannot
    /// patch such planes locally when the bounds change).
    pub fn needs_shape_bounds(&self) -> bool {
        self.gradients().any(|g| g.align_with_layer)
    }

    /// True when an ENABLED stroke fills with a Shape Burst gradient:
    /// its plane follows the band's distance field (`style_stroke_fill`), which
    /// an incremental window cannot patch (`style_cache`); likewise an ENABLED
    /// Gradient glow, whose `t` field (`style_glow_gradient`) is the same kind
    /// of per-pixel `BurstField`.
    pub(crate) fn holds_shape_burst(&self) -> bool {
        self.enabled_effects().any(|e| match e {
            Effect::Stroke(s) => {
                s.fill_type == StrokeFill::Gradient && s.gradient.style == GradientStyle::ShapeBurst
            }
            Effect::OuterGlow(g) => g.fill_type == GlowFill::Gradient,
            Effect::InnerGlow(g) => g.fill_type == GlowFill::Gradient,
            _ => false,
        })
    }

    /// True when any ENABLED effect reads the document's global light.
    pub fn uses_global_light(&self) -> bool {
        self.enabled_effects().any(|e| match e {
            Effect::DropShadow(d) => d.use_global_light,
            Effect::InnerShadow(s) => s.use_global_light,
            Effect::BevelEmboss(b) => b.use_global_light,
            _ => false,
        })
    }

    /// Re-establishes the effects invariant for a directly constructed
    /// style: stable-sorted by kind, the first of a duplicated single kind
    /// kept and the first [`MAX_INSTANCES`] of a repeating one, in their
    /// order. The cache is fresh.
    pub fn normalized(mut self) -> Self {
        self.effects.sort_by_key(Effect::kind);
        let mut kept: Vec<Effect> = Vec::with_capacity(self.effects.len());
        for effect in self.effects {
            let held = kept.iter().filter(|e| e.kind() == effect.kind()).count();
            if crate::style_instances::refusal(effect.kind(), held).is_none() {
                kept.push(effect);
            }
        }
        self.effects = kept;
        self.cache = RenderCache::default();
        self
    }

    /// "Scale Effects": every pixel-valued field multiplied by `factor` —
    /// shadow/glow `size`, shadow `distance`, stroke `size`, bevel `size` and
    /// `soften`, satin `distance` and `size`, a pattern fill's `scale`,
    /// `phase` and `anchor` (rounded; an overlay's, a stroke's when it
    /// carries one, and a bevel texture's when the bevel carries one,
    /// `PatternFill::scaled`) — re-clamped and re-quantized
    /// exactly as the parser would. `spread`/`choke` are fractions of size,
    /// and depth, opacities, angles, colours and the gradient scale are not
    /// pixel quantities, so they stay. Fresh cache.
    pub fn scaled(&self, factor: f64) -> LayerStyle {
        let px = |v: f32, range: Range| range.apply(f64::from(v) * factor);
        let effects = self
            .effects
            .iter()
            .map(|e| match e {
                Effect::DropShadow(d) => Effect::DropShadow(DropShadow {
                    distance: px(d.distance, RANGES.distance),
                    size: px(d.size, RANGES.size),
                    ..d.clone()
                }),
                Effect::InnerShadow(s) => Effect::InnerShadow(InnerShadow {
                    distance: px(s.distance, RANGES.distance),
                    size: px(s.size, RANGES.size),
                    ..s.clone()
                }),
                Effect::OuterGlow(g) => Effect::OuterGlow(OuterGlow {
                    size: px(g.size, RANGES.size),
                    ..g.clone()
                }),
                Effect::InnerGlow(g) => Effect::InnerGlow(InnerGlow {
                    size: px(g.size, RANGES.size),
                    ..g.clone()
                }),
                // A stroke's pattern fill scales only when it is stored
                // (`stroke_carries_pattern`), so a Color or Gradient stroke's
                // default fill stays default and claims nothing.
                Effect::Stroke(s) => Effect::Stroke(Stroke {
                    size: px(s.size, RANGES.size),
                    pattern_fill: if crate::style_pattern_fill::stroke_carries_pattern(s) {
                        s.pattern_fill.scaled(factor)
                    } else {
                        s.pattern_fill.clone()
                    },
                    ..s.clone()
                }),
                // A texture scales only when carried
                // (`bevel_carries_texture`), so a default one stays default.
                Effect::BevelEmboss(b) => Effect::BevelEmboss(BevelEmboss {
                    size: px(b.size, RANGES.size),
                    soften: px(b.soften, RANGES.soften),
                    texture: if crate::style_pattern_fill::bevel_carries_texture(b) {
                        BevelTexture {
                            fill: b.texture.fill.scaled(factor),
                            ..b.texture.clone()
                        }
                    } else {
                        b.texture.clone()
                    },
                    ..b.clone()
                }),
                Effect::Satin(s) => Effect::Satin(Satin {
                    distance: px(s.distance, RANGES.distance),
                    size: px(s.size, RANGES.size),
                    ..s.clone()
                }),
                Effect::PatternOverlay(p) => Effect::PatternOverlay(PatternOverlay {
                    fill: p.fill.scaled(factor),
                    ..p.clone()
                }),
                Effect::ColorOverlay(_) | Effect::GradientOverlay(_) => e.clone(),
            })
            .collect();
        LayerStyle {
            fill_opacity: self.fill_opacity,
            blend_if: self.blend_if,
            // Neither key is a pixel quantity, so both ride along unscaled.
            knockout: self.knockout,
            blend_interior_as_group: self.blend_interior_as_group,
            effects,
            cache: RenderCache::default(),
        }
    }
}

// ------------------------------------------------------------- CMYK inks --

/// [`LayerStyle::map_inks`]'s rewrite of one present ink to a new ink and hex.
pub(crate) type InkMap<'a> = dyn FnMut([f32; 4]) -> Result<([f32; 4], [u8; 3]), String> + 'a;

/// One colour slot's rewrite: the slot's ink and authored hex in, both out.
pub(crate) type SlotMap<'a> =
    dyn FnMut(Option<[f32; 4]>, [u8; 3]) -> Result<(Option<[f32; 4]>, [u8; 3]), String> + 'a;

/// [`LayerStyle::map_wide_slots`]'s rewrite of one wide slot: its `(linear,
/// color)` in, the slot's `(color, linear)` out, as
/// `style_json::normalized_wide` returns them.
pub(crate) type WideMap<'a> = dyn FnMut([f32; 3], [u8; 3]) -> ([u8; 3], Option<[f32; 3]>) + 'a;

fn mapped_fill(fill: &GradientFill, f: &mut SlotMap<'_>) -> Result<GradientFill, String> {
    let mut out = fill.clone();
    for stop in &mut out.stops {
        let (ink, color) = f(stop.ink, stop.color)?;
        stop.ink = ink;
        stop.color = color;
    }
    Ok(out)
}

/// A glow's stops through [`mapped_fill`] when the glow stores them
/// (`style_glow_gradient::glow_carries_stops`); a Color glow's default stops
/// are no slot, so they stay default and its bytes stay the prior writer's.
fn glow_stops(
    fill_type: GlowFill,
    gradient: &GradientFill,
    f: &mut SlotMap<'_>,
) -> Result<GradientFill, String> {
    if crate::style_glow_gradient::glow_carries_stops(fill_type, gradient) {
        mapped_fill(gradient, f)
    } else {
        Ok(gradient.clone())
    }
}

/// A slot's wide colour after a slot map: kept while the map left
/// its `color` alone, dropped when the map rewrote it (a CMYK edit's new hex),
/// so a wide colour never disagrees with its slot's `color`.
fn kept(linear: Option<[f32; 3]>, before: [u8; 3], after: [u8; 3]) -> Option<[f32; 3]> {
    if before == after {
        linear
    } else {
        None
    }
}

impl LayerStyle {
    /// This style with every colour slot (module doc's CMYK inks: the seven
    /// effects' `color`/`ink`, the bevel's two sides, every gradient stop of a
    /// stroke and an overlay, and of a glow that stores one) rewritten by
    /// `f`, in render order; a fresh
    /// cache. The ONE walk over the slots the ink helpers below and
    /// `cmyk_style` share.
    pub(crate) fn map_colour_slots(&self, f: &mut SlotMap<'_>) -> Result<LayerStyle, String> {
        let mut effects = Vec::with_capacity(self.effects.len());
        for effect in &self.effects {
            effects.push(match effect {
                Effect::DropShadow(e) => {
                    let (ink, color) = f(e.ink, e.color)?;
                    Effect::DropShadow(DropShadow {
                        ink,
                        linear: kept(e.linear, e.color, color),
                        color,
                        ..e.clone()
                    })
                }
                Effect::InnerShadow(e) => {
                    let (ink, color) = f(e.ink, e.color)?;
                    Effect::InnerShadow(InnerShadow {
                        ink,
                        linear: kept(e.linear, e.color, color),
                        color,
                        ..e.clone()
                    })
                }
                Effect::OuterGlow(e) => {
                    let (ink, color) = f(e.ink, e.color)?;
                    Effect::OuterGlow(OuterGlow {
                        ink,
                        linear: kept(e.linear, e.color, color),
                        color,
                        gradient: glow_stops(e.fill_type, &e.gradient, f)?,
                        ..e.clone()
                    })
                }
                Effect::InnerGlow(e) => {
                    let (ink, color) = f(e.ink, e.color)?;
                    Effect::InnerGlow(InnerGlow {
                        ink,
                        linear: kept(e.linear, e.color, color),
                        color,
                        gradient: glow_stops(e.fill_type, &e.gradient, f)?,
                        ..e.clone()
                    })
                }
                Effect::Stroke(e) => {
                    let (ink, color) = f(e.ink, e.color)?;
                    let gradient = mapped_fill(&e.gradient, f)?;
                    Effect::Stroke(Stroke {
                        ink,
                        linear: kept(e.linear, e.color, color),
                        color,
                        gradient,
                        ..e.clone()
                    })
                }
                Effect::ColorOverlay(e) => {
                    let (ink, color) = f(e.ink, e.color)?;
                    Effect::ColorOverlay(ColorOverlay {
                        ink,
                        linear: kept(e.linear, e.color, color),
                        color,
                        ..e.clone()
                    })
                }
                Effect::GradientOverlay(e) => Effect::GradientOverlay(GradientOverlay {
                    gradient: mapped_fill(&e.gradient, f)?,
                    ..e.clone()
                }),
                // The tile's samples are document values: no colour slot.
                Effect::PatternOverlay(_) => effect.clone(),
                Effect::BevelEmboss(e) => {
                    let (highlight_ink, highlight_color) = f(e.highlight_ink, e.highlight_color)?;
                    let (shadow_ink, shadow_color) = f(e.shadow_ink, e.shadow_color)?;
                    Effect::BevelEmboss(BevelEmboss {
                        highlight_ink,
                        highlight_linear: kept(
                            e.highlight_linear,
                            e.highlight_color,
                            highlight_color,
                        ),
                        highlight_color,
                        shadow_ink,
                        shadow_linear: kept(e.shadow_linear, e.shadow_color, shadow_color),
                        shadow_color,
                        ..e.clone()
                    })
                }
                Effect::Satin(e) => {
                    let (ink, color) = f(e.ink, e.color)?;
                    Effect::Satin(Satin {
                        ink,
                        linear: kept(e.linear, e.color, color),
                        color,
                        ..e.clone()
                    })
                }
            });
        }
        Ok(LayerStyle {
            fill_opacity: self.fill_opacity,
            blend_if: self.blend_if,
            knockout: self.knockout,
            blend_interior_as_group: self.blend_interior_as_group,
            effects,
            cache: RenderCache::default(),
        })
    }

    /// This style with every WIDE colour slot (module doc's WIDE COLOURS: the
    /// seven effects' `color` with a `linear`, the bevel's two sides)
    /// rewritten by `f`, `(linear, color)` in and the slot's `(color,
    /// linear)` out, as `style_json::normalized_wide` returns them; a `None`
    /// out drops the slot's `linear`. A slot with no `linear` is untouched; a
    /// fresh cache. `gray_style`'s wide pass.
    pub(crate) fn map_wide_slots(&self, f: &mut WideMap<'_>) -> LayerStyle {
        let mut slot = |linear: Option<[f32; 3]>, color: [u8; 3]| match linear {
            Some(linear) => {
                let (color, linear) = f(linear, color);
                (color, linear)
            }
            None => (color, None),
        };
        let effects = self
            .effects
            .iter()
            .map(|effect| match effect {
                Effect::DropShadow(e) => {
                    let (color, linear) = slot(e.linear, e.color);
                    Effect::DropShadow(DropShadow {
                        color,
                        linear,
                        ..e.clone()
                    })
                }
                Effect::InnerShadow(e) => {
                    let (color, linear) = slot(e.linear, e.color);
                    Effect::InnerShadow(InnerShadow {
                        color,
                        linear,
                        ..e.clone()
                    })
                }
                Effect::OuterGlow(e) => {
                    let (color, linear) = slot(e.linear, e.color);
                    Effect::OuterGlow(OuterGlow {
                        color,
                        linear,
                        ..e.clone()
                    })
                }
                Effect::InnerGlow(e) => {
                    let (color, linear) = slot(e.linear, e.color);
                    Effect::InnerGlow(InnerGlow {
                        color,
                        linear,
                        ..e.clone()
                    })
                }
                Effect::Stroke(e) => {
                    let (color, linear) = slot(e.linear, e.color);
                    Effect::Stroke(Stroke {
                        color,
                        linear,
                        ..e.clone()
                    })
                }
                Effect::ColorOverlay(e) => {
                    let (color, linear) = slot(e.linear, e.color);
                    Effect::ColorOverlay(ColorOverlay {
                        color,
                        linear,
                        ..e.clone()
                    })
                }
                Effect::BevelEmboss(e) => {
                    let (highlight_color, highlight_linear) =
                        slot(e.highlight_linear, e.highlight_color);
                    let (shadow_color, shadow_linear) = slot(e.shadow_linear, e.shadow_color);
                    Effect::BevelEmboss(BevelEmboss {
                        highlight_color,
                        highlight_linear,
                        shadow_color,
                        shadow_linear,
                        ..e.clone()
                    })
                }
                Effect::Satin(e) => {
                    let (color, linear) = slot(e.linear, e.color);
                    Effect::Satin(Satin {
                        color,
                        linear,
                        ..e.clone()
                    })
                }
                // Gradient stops and pattern tiles carry no wide colour.
                Effect::GradientOverlay(_) | Effect::PatternOverlay(_) => effect.clone(),
            })
            .collect();
        LayerStyle {
            effects,
            cache: RenderCache::default(),
            ..self.clone()
        }
    }

    /// Whether any colour slot carries a CMYK ink.
    pub(crate) fn has_inks(&self) -> bool {
        let mut found = false;
        let _ = self.map_colour_slots(&mut |ink, color| {
            found |= ink.is_some();
            Ok((ink, color))
        });
        found
    }

    /// This style with every ink dropped (an RGB, float or byte setter, and
    /// CMYK → RGB: module doc). The authored hex stays.
    pub(crate) fn without_inks(&self) -> LayerStyle {
        self.map_colour_slots(&mut |_, color| Ok((None, color)))
            .unwrap_or_else(|_: String| self.clone())
    }

    /// This style with every PRESENT ink rewritten by `f` to a new ink and
    /// hex; ink-less slots are untouched.
    pub(crate) fn map_inks(&self, f: &mut InkMap<'_>) -> Result<LayerStyle, String> {
        self.map_colour_slots(&mut |ink, color| match ink {
            Some(ink) => f(ink).map(|(ink, color)| (Some(ink), color)),
            None => Ok((None, color)),
        })
    }

    /// Whether any of the style's Blend If ranges reads a CMYK plate.
    pub(crate) fn has_cmyk_blend_if(&self) -> bool {
        self.blend_if
            .is_some_and(|b| b.ranges().any(|r| r.channel.is_cmyk()))
    }

    /// This style without the Blend If ranges that read a CMYK plate — what
    /// the byte document's readers keep of one a newer file carries (#1).
    /// Fresh cache.
    pub(crate) fn without_cmyk_blend_if(&self) -> LayerStyle {
        if !self.has_cmyk_blend_if() {
            return self.clone();
        }
        LayerStyle {
            blend_if: self.blend_if.and_then(BlendIf::without_cmyk),
            cache: RenderCache::default(),
            ..self.clone()
        }
    }
}

/// [`LayerStyle::scaled`] on an optional shared style: the SAME `Arc` comes
/// back when there is no style, `factor` is within 1e-6 of 1 (an exact
/// translation or rotation: `sqrt(|det|)` of a rotation is 1 only up to
/// floating-point residue), `factor` is not a positive finite number, or
/// nothing would change after re-quantization — so a no-op transform keeps
/// the cache warm.
pub(crate) fn scaled_style(
    style: &Option<Arc<LayerStyle>>,
    factor: f64,
) -> Option<Arc<LayerStyle>> {
    let current = style.as_ref()?;
    if !factor.is_finite() || factor <= 0.0 || (factor - 1.0).abs() < 1e-6 {
        return Some(Arc::clone(current));
    }
    let scaled = current.scaled(factor);
    if scaled == **current {
        return Some(Arc::clone(current));
    }
    Some(Arc::new(scaled))
}

/// The mean scale a perspective warp applies to a `lw` x `lh` layer whose
/// corners land on `quad` (x, y pairs in corner order): the square root of
/// the area ratio, with the quad's area from the shoelace formula. The area
/// — never the bounding box — is what stays 1 for a pure rotation, and it
/// equals `sqrt(|det|)` when the quad is a parallelogram, so the affine and
/// perspective commit paths agree.
pub(crate) fn quad_mean_scale(quad: [f64; 8], lw: u32, lh: u32) -> f64 {
    let mut twice_area = 0.0;
    for i in 0..4 {
        let (x0, y0) = (quad[2 * i], quad[2 * i + 1]);
        let j = (i + 1) % 4;
        let (x1, y1) = (quad[2 * j], quad[2 * j + 1]);
        twice_area += x0 * y1 - x1 * y0;
    }
    let source = f64::from(lw) * f64::from(lh);
    if source <= 0.0 {
        return 1.0;
    }
    (twice_area.abs() / 2.0 / source).sqrt()
}

impl Layer {
    /// The style the compositor must render for this layer: `Some` only when
    /// a style is present and the meta does not parse as an adjustment. An
    /// ADJUSTMENT layer's style is answered by `renders_adjustment_style`
    /// (`style_adjustment`, which gives it the backdrop's shape), and
    /// `renders_any_style` asks both. Identity styles are never stored, so
    /// `Some` means "renders something".
    ///
    /// A GROUP may carry a style. A group's shape
    /// is its own rendered projection — `doc_group::rendered_group` hands the
    /// style to the SYNTHETIC layer, so `Shape::of_layer` gets real pixels and
    /// nothing here needs a group branch.
    pub(crate) fn renders_style(&self) -> Option<&LayerStyle> {
        let style = self.style.as_deref()?;
        if self
            .meta
            .as_deref()
            .and_then(Adjustment::from_meta)
            .is_some()
        {
            return None;
        }
        Some(style)
    }
}

impl DrDocument {
    /// Pure setter: replaces layer `idx`'s style with `style`, or clears it
    /// when `None`. An identity style CLEARS — it is mapped to `None` before
    /// anything else, so a stored style always renders something. A pattern
    /// overlay is anchored next (`style_pattern::anchored`). `None`
    /// (a refusal, never an identical copy) on an out-of-range `idx` or when
    /// the new value equals the current one, "no style" included.
    ///
    /// The one side effect, invisible to every projection: the style being
    /// replaced hands its rendered-plane cache to the new one
    /// ([`LayerStyle::inherit_planes`]) and keeps none of it. The planes
    /// are a memo, not state — a document that still holds the old `Arc`
    /// (an undo snapshot, the sheet's preview base) re-renders once if it
    /// is ever projected again — and without the hand-over every style
    /// version an agent or the Layer Style sheet minted would pin a full
    /// plane set for as long as the undo stack held its document (tens of
    /// versions of a photo-sized layer is gigabytes), while the new version
    /// re-blurred planes that only changed colour.
    pub fn set_layer_style(&self, idx: usize, style: Option<Arc<LayerStyle>>) -> Option<Self> {
        let style = style.filter(|s| !s.is_identity());
        let layer = self.layers.get(idx)?;
        // A linked pattern overlay anchors where the layer was made
        // (`style_pattern`'s Placement).
        let made_at = crate::style_pattern::made_at(layer.offset, layer.reference);
        let style = style.map(|s| crate::style_pattern::anchored_arc(s, made_at));
        if layer.style == style {
            return None;
        }
        if let Some(previous) = layer.style.as_deref() {
            match style.as_deref() {
                Some(next) => next.inherit_planes(previous),
                None => previous.cache.clear(),
            }
        }
        let mut doc = self.clone();
        doc.layers[idx].style = style;
        Some(doc)
    }

    /// Pure setter: replaces the document's global light with `light`
    /// sanitized ([`GlobalLight::sane`]). `None` on a non-finite component
    /// or when nothing changes after sanitizing.
    pub fn set_global_light(&self, light: GlobalLight) -> Option<Self> {
        if !light.angle.is_finite() || !light.altitude.is_finite() {
            return None;
        }
        let light = light.sane();
        if light == self.global_light {
            return None;
        }
        let mut doc = self.clone();
        doc.global_light = light;
        Some(doc)
    }
}
