//! The canonical writer for [`LayerStyle`] (`to_json`): every key of every
//! present effect, effects in render order, keys sorted (serde_json's `Map`
//! is BTreeMap-backed — do not enable `preserve_order`), colours as
//! lowercase `#rrggbb`, numbers re-quantized to four decimals and printed
//! as decimals (`5.0`, `0.3333`). What `dr_doc_layer_style` returns and
//! what the DRDC writer stores; the parser is `style_json`, the schema
//! table `style`'s module doc.

use serde_json::{Map, Value};

use crate::blend::BlendMode;
use crate::style::{
    BevelEmboss, BevelTechnique, BlendIf, Effect, GlowFill, GradientFill, Knockout, LayerStyle,
    PatternFill,
};
use crate::style_contour_json::{put_contour, put_flag, put_range};
use crate::style_names::{
    blend_mode_name, color_hex, enum_name, BEVEL_DIRECTIONS, BEVEL_STYLES, BEVEL_TECHNIQUES,
    CHANNELS, GLOW_FILLS, GLOW_SOURCES, KNOCKOUTS, STROKE_FILLS, STROKE_GRADIENT_STYLES,
    STROKE_POSITIONS,
};

impl LayerStyle {
    /// The canonical JSON: every key of every present effect, effects in
    /// render order, sorted keys, `#rrggbb` colours, numbers re-quantized
    /// (`5.0`, `0.3333`), `version` 1. What `dr_doc_layer_style` returns and
    /// what the DRDC writer stores.
    pub fn to_json(&self) -> String {
        let mut root = Map::new();
        root.insert("version".into(), Value::from(1));
        root.insert("fill_opacity".into(), number(self.fill_opacity));
        root.insert(
            "blend_if".into(),
            match &self.blend_if {
                None => Value::Null,
                Some(b) => blend_if_value(b),
            },
        );
        // The ONE exception to "every key" (`style`'s module doc): each of the
        // two knockout-family keys is written only when it is NOT at its
        // default, so every style authored without them — and with it every
        // `.rz`, `LSTY` body, comps payload and committed PSD golden — is
        // byte-identical to what the writer before them produced.
        if self.knockout != Knockout::None {
            root.insert(
                "knockout".into(),
                Value::from(enum_name(&KNOCKOUTS, self.knockout)),
            );
        }
        if self.blend_interior_as_group {
            root.insert("blend_interior_as_group".into(), Value::from(true));
        }
        let mut effects: Vec<&Effect> = self.effects.iter().collect();
        effects.sort_by_key(|e| e.kind());
        root.insert(
            "effects".into(),
            Value::Array(effects.into_iter().map(effect_value).collect()),
        );
        Value::Object(root).to_string()
    }
}

/// The stored f32 re-quantized in f64 and printed as a decimal (`5.0`,
/// `0.3333`) — never the f32's own expansion (`0.33329999446868896`).
pub(crate) fn number(v: f32) -> Value {
    Value::from((f64::from(v) * 1e4).round() / 1e4)
}

/// The canonical `blend_if` object (`channel`, `this_layer`, `underlying`,
/// and `extra` when multichannel):
/// what `to_json` writes inside a style and what the typed document's
/// inspection reports for a layer's own Blend If.
pub(crate) fn blend_if_value(b: &BlendIf) -> Value {
    let mut o = Map::new();
    o.insert(
        "channel".into(),
        Value::from(enum_name(&CHANNELS, b.channel)),
    );
    o.insert("this_layer".into(), ramp_value(b.this_layer));
    o.insert("underlying".into(), ramp_value(b.underlying));
    // `extra` only when there is one, so every
    // single-channel style is byte-identical to the writer before it.
    if b.is_multichannel() {
        let extra = b
            .extra
            .iter()
            .flatten()
            .map(|r| {
                let mut e = Map::new();
                e.insert(
                    "channel".into(),
                    Value::from(enum_name(&CHANNELS, r.channel)),
                );
                e.insert("this_layer".into(), ramp_value(r.this_layer));
                e.insert("underlying".into(), ramp_value(r.underlying));
                Value::Object(e)
            })
            .collect();
        o.insert("extra".into(), Value::Array(extra));
    }
    Value::Object(o)
}

fn ramp_value(r: [u8; 4]) -> Value {
    Value::Array(r.iter().map(|&v| Value::from(v)).collect())
}

pub(crate) fn gradient_value(g: &GradientFill) -> Value {
    let mut o = Map::new();
    o.insert(
        "stops".into(),
        Value::Array(
            g.stops
                .iter()
                .map(|s| {
                    let mut so = Map::new();
                    so.insert("position".into(), number(s.position));
                    so.insert("color".into(), Value::from(color_hex(s.color)));
                    so.insert("opacity".into(), number(s.opacity));
                    put_ink(&mut so, "ink", s.ink);
                    // The midpoints only when not the default 0.5, so
                    // every gradient written before them stays byte-identical.
                    if s.midpoint != 0.5 {
                        so.insert("midpoint".into(), number(s.midpoint));
                    }
                    if s.opacity_midpoint != 0.5 {
                        so.insert("opacity_midpoint".into(), number(s.opacity_midpoint));
                    }
                    Value::Object(so)
                })
                .collect(),
        ),
    );
    // Every style by its name, Shape Burst's too: the stroke table is
    // the five plus `shape_burst`, so the five write as they always did.
    o.insert(
        "style".into(),
        Value::from(enum_name(&STROKE_GRADIENT_STYLES, g.style)),
    );
    o.insert("angle".into(), number(g.angle));
    o.insert("scale".into(), number(g.scale));
    o.insert("reverse".into(), Value::from(g.reverse));
    o.insert("align_with_layer".into(), Value::from(g.align_with_layer));
    // `dither` is written ONLY when true, like the knockout-family keys
    // and the inks, so every style without it stays byte-identical.
    if g.dither {
        o.insert("dither".into(), Value::from(true));
    }
    // `smoothness` likewise only when not the default 1, so every
    // gradient written before it stays byte-identical.
    if g.smoothness != 1.0 {
        o.insert("smoothness".into(), number(g.smoothness));
    }
    Value::Object(o)
}

/// A CMYK ink slot (#1), written ONLY when present — like the two
/// knockout-family keys, so every RGB style, `LSTY` body and PSD golden stays
/// byte-identical to what the writer before inks produced.
fn put_ink(o: &mut Map<String, Value>, key: &str, ink: Option<[f32; 4]>) {
    if let Some(ink) = ink {
        o.insert(
            key.into(),
            Value::Array(ink.iter().map(|&v| number(v)).collect()),
        );
    }
}

/// A wide colour slot (`style`'s WIDE COLOURS), written ONLY when
/// present, each component the stored f32 widened to f64 and NOT quantized,
/// so it re-parses to the same f32 exactly.
fn put_linear(o: &mut Map<String, Value>, key: &str, linear: Option<[f32; 3]>) {
    if let Some(linear) = linear {
        o.insert(
            key.into(),
            Value::Array(linear.iter().map(|&v| Value::from(f64::from(v))).collect()),
        );
    }
}

/// A pattern fill's keys into `o` (`style_pattern_fill`): `pattern`,
/// `angle`, `scale`, `link_with_layer`, `phase` always, `anchor` only when
/// set; sorted by the map.
pub(crate) fn put_pattern_fill(o: &mut Map<String, Value>, fill: &PatternFill) {
    o.insert(
        "pattern".into(),
        crate::style_pattern_fill::pattern_value(&fill.pattern),
    );
    o.insert("angle".into(), number(fill.angle));
    o.insert("scale".into(), number(fill.scale));
    o.insert("link_with_layer".into(), Value::from(fill.link_with_layer));
    o.insert(
        "phase".into(),
        Value::Array(fill.phase.iter().map(|&v| number(v)).collect()),
    );
    if let Some(anchor) = fill.anchor {
        o.insert(
            "anchor".into(),
            Value::Array(anchor.iter().map(|&v| Value::from(v)).collect()),
        );
    }
}

/// `noise`, written only when above 0 — the
/// conditional-key rule (`style`'s module doc), so a style without noise
/// writes the bytes the writer before it wrote.
fn put_noise(o: &mut Map<String, Value>, noise: f32) {
    if noise > 0.0 {
        o.insert("noise".into(), number(noise));
    }
}

/// A bevel's Texture (`style_pattern_fill`'s module doc):
/// `texture_enabled` only when true, and the `texture` object (the fill's
/// keys without `angle`, plus `depth` and `invert`) only when the bevel
/// `bevel_carries_texture` — so every bevel without one writes the bytes the
/// writer before it wrote.
fn put_texture(o: &mut Map<String, Value>, b: &BevelEmboss) {
    put_flag(o, "texture_enabled", b.texture_enabled);
    if crate::style_pattern_fill::bevel_carries_texture(b) {
        let mut t = Map::new();
        put_pattern_fill(&mut t, &b.texture.fill);
        t.remove("angle");
        t.insert("depth".into(), number(b.texture.depth));
        t.insert("invert".into(), Value::from(b.texture.invert));
        o.insert("texture".into(), Value::Object(t));
    }
}

/// A glow's fill (`style_glow_gradient`): `fill_type` only when
/// gradient, `gradient` only when the glow `glow_carries_stops`, `jitter` only
/// when above 0 — so every colour glow writes the bytes the writer before it
/// wrote.
fn put_glow_fill(o: &mut Map<String, Value>, fill_type: GlowFill, g: &GradientFill, jitter: f32) {
    if fill_type == GlowFill::Gradient {
        o.insert(
            "fill_type".into(),
            Value::from(enum_name(&GLOW_FILLS, fill_type)),
        );
    }
    if crate::style_glow_gradient::glow_carries_stops(fill_type, g) {
        o.insert("gradient".into(), gradient_value(g));
    }
    if jitter > 0.0 {
        o.insert("jitter".into(), number(jitter));
    }
}

fn put_blend(o: &mut Map<String, Value>, key: &str, mode: BlendMode) {
    o.insert(key.into(), Value::from(blend_mode_name(mode)));
}

fn effect_value(e: &Effect) -> Value {
    let mut o = Map::new();
    o.insert("type".into(), Value::from(e.kind().type_name()));
    o.insert("enabled".into(), Value::from(e.enabled()));
    match e {
        Effect::DropShadow(d) => {
            put_blend(&mut o, "blend", d.blend);
            o.insert("color".into(), Value::from(color_hex(d.color)));
            put_ink(&mut o, "ink", d.ink);
            put_linear(&mut o, "linear", d.linear);
            o.insert("opacity".into(), number(d.opacity));
            o.insert("angle".into(), number(d.angle));
            o.insert("use_global_light".into(), Value::from(d.use_global_light));
            o.insert("distance".into(), number(d.distance));
            o.insert("spread".into(), number(d.spread));
            o.insert("size".into(), number(d.size));
            o.insert("layer_knocks_out".into(), Value::from(d.layer_knocks_out));
            put_contour(&mut o, "contour", &d.contour);
            put_flag(&mut o, "anti_aliased", d.anti_aliased);
            put_noise(&mut o, d.noise);
        }
        Effect::InnerShadow(s) => {
            put_blend(&mut o, "blend", s.blend);
            o.insert("color".into(), Value::from(color_hex(s.color)));
            put_ink(&mut o, "ink", s.ink);
            put_linear(&mut o, "linear", s.linear);
            o.insert("opacity".into(), number(s.opacity));
            o.insert("angle".into(), number(s.angle));
            o.insert("use_global_light".into(), Value::from(s.use_global_light));
            o.insert("distance".into(), number(s.distance));
            o.insert("choke".into(), number(s.choke));
            o.insert("size".into(), number(s.size));
            put_contour(&mut o, "contour", &s.contour);
            put_flag(&mut o, "anti_aliased", s.anti_aliased);
            put_noise(&mut o, s.noise);
        }
        Effect::OuterGlow(g) => {
            put_blend(&mut o, "blend", g.blend);
            o.insert("color".into(), Value::from(color_hex(g.color)));
            put_ink(&mut o, "ink", g.ink);
            put_linear(&mut o, "linear", g.linear);
            o.insert("opacity".into(), number(g.opacity));
            o.insert("spread".into(), number(g.spread));
            o.insert("size".into(), number(g.size));
            put_contour(&mut o, "contour", &g.contour);
            put_flag(&mut o, "anti_aliased", g.anti_aliased);
            put_range(&mut o, "range", g.range);
            put_noise(&mut o, g.noise);
            put_glow_fill(&mut o, g.fill_type, &g.gradient, g.jitter);
        }
        Effect::InnerGlow(g) => {
            put_blend(&mut o, "blend", g.blend);
            o.insert("color".into(), Value::from(color_hex(g.color)));
            put_ink(&mut o, "ink", g.ink);
            put_linear(&mut o, "linear", g.linear);
            o.insert("opacity".into(), number(g.opacity));
            o.insert("choke".into(), number(g.choke));
            o.insert("size".into(), number(g.size));
            o.insert(
                "source".into(),
                Value::from(enum_name(&GLOW_SOURCES, g.source)),
            );
            put_contour(&mut o, "contour", &g.contour);
            put_flag(&mut o, "anti_aliased", g.anti_aliased);
            put_range(&mut o, "range", g.range);
            put_noise(&mut o, g.noise);
            put_glow_fill(&mut o, g.fill_type, &g.gradient, g.jitter);
        }
        Effect::Stroke(s) => {
            put_blend(&mut o, "blend", s.blend);
            o.insert("opacity".into(), number(s.opacity));
            o.insert("size".into(), number(s.size));
            o.insert(
                "position".into(),
                Value::from(enum_name(&STROKE_POSITIONS, s.position)),
            );
            o.insert(
                "fill_type".into(),
                Value::from(enum_name(&STROKE_FILLS, s.fill_type)),
            );
            o.insert("color".into(), Value::from(color_hex(s.color)));
            put_ink(&mut o, "ink", s.ink);
            put_linear(&mut o, "linear", s.linear);
            o.insert("gradient".into(), gradient_value(&s.gradient));
            // Written only when stored (`style`'s module doc), so a Color or
            // Gradient stroke writes what the writer before it did.
            if crate::style_pattern_fill::stroke_carries_pattern(s) {
                let mut fill = Map::new();
                put_pattern_fill(&mut fill, &s.pattern_fill);
                o.insert("pattern_fill".into(), Value::Object(fill));
            }
        }
        Effect::ColorOverlay(c) => {
            put_blend(&mut o, "blend", c.blend);
            o.insert("color".into(), Value::from(color_hex(c.color)));
            put_ink(&mut o, "ink", c.ink);
            put_linear(&mut o, "linear", c.linear);
            o.insert("opacity".into(), number(c.opacity));
        }
        Effect::GradientOverlay(g) => {
            put_blend(&mut o, "blend", g.blend);
            o.insert("opacity".into(), number(g.opacity));
            o.insert("gradient".into(), gradient_value(&g.gradient));
        }
        Effect::PatternOverlay(p) => {
            put_blend(&mut o, "blend", p.blend);
            o.insert("opacity".into(), number(p.opacity));
            // The fill's keys, flat in the overlay's object.
            put_pattern_fill(&mut o, &p.fill);
        }
        Effect::BevelEmboss(b) => {
            o.insert(
                "style".into(),
                Value::from(enum_name(&BEVEL_STYLES, b.style)),
            );
            // Technique (#2) only when it is not Smooth, so every style
            // without one writes the bytes it did.
            if b.technique != BevelTechnique::Smooth {
                o.insert(
                    "technique".into(),
                    Value::from(enum_name(&BEVEL_TECHNIQUES, b.technique)),
                );
            }
            o.insert("depth".into(), number(b.depth));
            o.insert(
                "direction".into(),
                Value::from(enum_name(&BEVEL_DIRECTIONS, b.direction)),
            );
            o.insert("size".into(), number(b.size));
            o.insert("soften".into(), number(b.soften));
            o.insert("angle".into(), number(b.angle));
            o.insert("use_global_light".into(), Value::from(b.use_global_light));
            o.insert("altitude".into(), number(b.altitude));
            put_blend(&mut o, "highlight_blend", b.highlight_blend);
            o.insert(
                "highlight_color".into(),
                Value::from(color_hex(b.highlight_color)),
            );
            put_ink(&mut o, "highlight_ink", b.highlight_ink);
            put_linear(&mut o, "highlight_linear", b.highlight_linear);
            o.insert("highlight_opacity".into(), number(b.highlight_opacity));
            put_blend(&mut o, "shadow_blend", b.shadow_blend);
            o.insert(
                "shadow_color".into(),
                Value::from(color_hex(b.shadow_color)),
            );
            put_ink(&mut o, "shadow_ink", b.shadow_ink);
            put_linear(&mut o, "shadow_linear", b.shadow_linear);
            o.insert("shadow_opacity".into(), number(b.shadow_opacity));
            put_contour(&mut o, "gloss_contour", &b.gloss_contour);
            put_flag(&mut o, "gloss_anti_aliased", b.gloss_anti_aliased);
            put_flag(&mut o, "contour_enabled", b.contour_enabled);
            put_contour(&mut o, "contour", &b.contour);
            put_flag(&mut o, "contour_anti_aliased", b.contour_anti_aliased);
            put_range(&mut o, "contour_range", b.contour_range);
            put_texture(&mut o, b);
        }
        Effect::Satin(s) => {
            put_blend(&mut o, "blend", s.blend);
            o.insert("color".into(), Value::from(color_hex(s.color)));
            put_ink(&mut o, "ink", s.ink);
            put_linear(&mut o, "linear", s.linear);
            o.insert("opacity".into(), number(s.opacity));
            o.insert("angle".into(), number(s.angle));
            o.insert("distance".into(), number(s.distance));
            o.insert("size".into(), number(s.size));
            o.insert("invert".into(), Value::from(s.invert));
            put_contour(&mut o, "contour", &s.contour);
            put_flag(&mut o, "anti_aliased", s.anti_aliased);
        }
    }
    Value::Object(o)
}
