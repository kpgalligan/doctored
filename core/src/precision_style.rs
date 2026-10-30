//! Typed layer styles: the byte `style::LayerStyle` (fill opacity and the
//! nine effects; its `blend_if` is always `None` here — the layer's Blend If
//! stays in the v35 `blend_if` field of `precision_blend_if`) carried per
//! layer by the 16-bit document and rendered by `precision_style_render`
//! under the shape-quantized byte-plane policy of
//! `designs/phase-10d-layer-styles-status.md`. A style is admitted on raster
//! layers (a CMYK one included, with its colours in ink: #547, `cmyk_style`),
//! adjustment layers (effects shaped by the mask over the canvas, the
//! adjustment applied after the Below effects, `precision_style_adjustment`;
//! a CMYK one included, on ink through `cmyk_style_row`) and groups, a Pass
//! Through group included (it then composites ISOLATED,
//! `PrecisionDocumentLayer::isolates`, as take1's P1–P3 measured), and a
//! style whose padded plane would exceed `doc::MAX_PIXELS` refuses UP FRONT
//! — at the edit, at the native reader's attach step and in the geometry
//! plan — never as a silent no-effects render.
//!
//! the typed `Option<Arc<LayerStyle>>` (fill opacity and the nine effects; `blend_if` always
//! `None`, the layer's field holds it) on raster layers, adjustments and groups, under the
//! shape-quantized byte-plane policy of `designs/phase-10d-layer-styles-status.md`: `validate` (no
//! Blend If inside, non-identity, and the padded plane `plane_pixels` ≤ `MAX_PIXELS` —
//! the ONE gate, also run at the v36 attach walk before any PNG decode and in
//! `precision_geometry_plan::build` against the planned destination extents with the scaled pad),
//! `with_layer_style` (sets BOTH fields from one whole style), `reported_style` (the merged
//! canonical JSON the `style` info row carries), and the strict `layer_style` edit (identity/null
//! clear, exact no-op `None`; a key the schema does not name refuses by name, `style_json_keys`).
use std::sync::Arc;

use crate::doc::MAX_PIXELS;
use crate::job_control::JobContext;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, BOOKKEEPING_BYTES};
use crate::precision_style_render::plane_pixels;
use crate::style::{LayerStyle, Strictness};
use crate::style_cache::RenderCache;
use serde::Deserialize;

/// The one admission rule for a STORED typed style: `None` on anything;
/// `Some` only when it carries no Blend If (that field lives on the layer),
/// is not an identity, and its padded plane — `plane` is the raster's
/// dimensions or, for an adjustment or a group, the canvas — stays within
/// `MAX_PIXELS`.
pub(crate) fn validate(style: Option<&LayerStyle>, plane: (u32, u32)) -> Result<(), String> {
    let Some(style) = style else {
        return Ok(());
    };
    if style.blend_if.is_some() {
        return Err(
            "a typed layer style carries its Blend If on the layer, not in the style".into(),
        );
    }
    if style.is_identity() {
        return Err("an identity typed layer style is never stored".into());
    }
    if plane_pixels(plane, style.pad())? > MAX_PIXELS {
        return Err("typed layer style plane exceeds the pixel limit".into());
    }
    Ok(())
}

impl PrecisionDocumentLayer {
    /// The stored effects/fill style (never an identity, never with a Blend If).
    pub fn style(&self) -> Option<&Arc<LayerStyle>> {
        self.style.as_ref()
    }

    /// The plane the style's shape is rendered on: a raster's own pixels, an
    /// adjustment's or a group's the canvas.
    pub(crate) fn style_plane(&self, canvas: (u32, u32)) -> (u32, u32) {
        self.source()
            .map(|image| image.dimensions())
            .or(self.cmyk_source().map(|image| image.dimensions()))
            .unwrap_or(canvas)
    }

    /// Consumes a newly constructed layer and sets BOTH fields from one
    /// whole style: the Blend If through [`Self::with_blend_if`] (identity
    /// → `None`), and the effects/fill/KNOCKOUT through `style` iff it is
    /// non-identity WITH BLEND IF REMOVED, else `None`. An oversize plane
    /// refuses without rewriting pixels, masks or
    /// placement.
    ///
    /// The storage rule is decision (iv) of 12E-3b: `fill_opacity < 1 ||
    /// has_enabled_effects || knockout != none`, carrying BOTH keys verbatim.
    /// A lone `blend_interior_as_group` beside only a Blend If is therefore
    /// DROPPED at 16 bits — it renders nothing at either depth, because the
    /// flag needs a grouped interior to move.
    pub fn with_layer_style(
        self,
        style: Option<&LayerStyle>,
        canvas: (u32, u32),
    ) -> Result<Self, String> {
        let mut next = self.with_blend_if(style.and_then(|s| s.blend_if))?;
        let effective = style
            .filter(|s| {
                s.fill_opacity < 1.0
                    || s.has_enabled_effects()
                    || s.knockout != crate::style::Knockout::None
            })
            .map(|s| LayerStyle {
                fill_opacity: s.fill_opacity,
                blend_if: None,
                knockout: s.knockout,
                blend_interior_as_group: s.blend_interior_as_group,
                effects: s.effects.clone(),
                cache: RenderCache::default(),
            })
            // A linked pattern overlay anchors where the layer was made
            // (`style_pattern`'s Placement).
            .map(|s| {
                let made_at = crate::style_pattern::made_at(next.offset(), next.reference());
                crate::style_pattern::anchored(&s, made_at)
            });
        validate(effective.as_ref(), next.style_plane(canvas))?;
        next.style = effective.map(Arc::new);
        Ok(next)
    }

    /// The canonical style JSON of the two fields merged back into one
    /// `LayerStyle` (`fill_opacity`, the layer's `blend_if`, the effects);
    /// `None` iff both fields are `None`.
    pub fn reported_style(&self) -> Option<String> {
        if self.style.is_none() && self.blend_if.is_none() {
            return None;
        }
        let merged = LayerStyle {
            fill_opacity: self.style.as_ref().map_or(1.0, |s| s.fill_opacity),
            blend_if: self.blend_if,
            // Both keys are carried VERBATIM since 12E-3b (decision iv);
            // `to_json` omits each at its default.
            knockout: self
                .style
                .as_ref()
                .map_or(crate::style::Knockout::None, |s| s.knockout),
            blend_interior_as_group: self
                .style
                .as_ref()
                .is_some_and(|s| s.blend_interior_as_group),
            effects: self
                .style
                .as_ref()
                .map(|s| s.effects.clone())
                .unwrap_or_default(),
            cache: RenderCache::default(),
        };
        Some(merged.to_json())
    }
}

/// `{op:"layer_style",version:1,layer,style:null|{…}}` — the whole style
/// JSON (the `style` module's schema), parsed strictly; null clears both
/// fields, an identity clears, and the exact no-op is `None`. The optional
/// `scale` (finite and > 0) replaces the parsed style with
/// `LayerStyle::scaled(scale)` before those checks: the host's reduced
/// style preview sends `1 / factor`.
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum LayerStyleEdit {
    LayerStyle {
        version: u32,
        layer: usize,
        style: serde_json::Value,
        #[serde(default)]
        scale: Option<f64>,
    },
}
impl LayerStyleEdit {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| format!("invalid native layer style request: {e}"))
    }
    fn target<'a>(
        &self,
        document: &'a PrecisionDocument,
    ) -> Result<(usize, &'a PrecisionDocumentLayer, PrecisionDocumentLayer), String> {
        let Self::LayerStyle {
            version,
            layer,
            style,
            scale,
        } = self;
        if *version != 1 {
            return Err("unsupported native layer style request version".into());
        }
        if scale.is_some_and(|s| !s.is_finite() || s <= 0.0) {
            return Err("native layer style scale must be finite and greater than 0".into());
        }
        let old = document
            .layer(*layer)
            .ok_or("native layer style index is out of range")?;
        let wanted = if style.is_null() {
            None
        } else {
            let parsed = LayerStyle::from_value(style, Strictness::Strict)
                .map_err(|e| format!("invalid native layer style: {e}"))?;
            // A key the schema does not name refuses by name rather than
            // being dropped, as the Blend If edit does.
            if let Some(path) = crate::style_json_keys::unknown_key(style) {
                return Err(format!(
                    "invalid native layer style: unknown layer style field {path}"
                ));
            }
            Some(parsed)
        };
        let wanted = match scale {
            Some(scale) => wanted.map(|style| style.scaled(*scale)),
            None => wanted,
        };
        // A FLOAT document refuses the one part of a style it cannot hold BY
        // NAME, with the sentence its own operator table carries
        // (`float_document::style_refusal`, decision viii of 12E-3b) — rather
        // than accepting the edit and leaving a document that refuses to
        // render. Clearing one is always allowed.
        if document.is_float() {
            if let Some(reason) = wanted
                .as_ref()
                .and_then(crate::float_document::style_refusal)
            {
                return Err(reason);
            }
        }
        // A CMYK document (#547, `cmyk_style`): a Blend If on Red, Green or
        // Blue refuses by name (`cmyk_document::style_refusal`), here as at
        // the mode's gate, an
        // ink-less black shadow slot takes
        // K100 (`with_black_as_k`, take1's D-K), and every inked slot's hex
        // becomes its ink's sRGB under the document's profile
        // (`canonical_hex`). Every other
        // document drops the inks (`style`'s CMYK INKS).
        let wanted = match document.cmyk_policy() {
            Some(policy) => {
                // Every range as written first (an identity Red included,
                // which canonicalization would drop), then the stored value.
                if let Some(reason) = style
                    .get("blend_if")
                    .and_then(|b| crate::precision_blend_if::requested_channel_refusal(b, true))
                {
                    return Err(reason);
                }
                if let Some(reason) = wanted
                    .as_ref()
                    .and_then(crate::cmyk_document::style_refusal)
                {
                    return Err(reason);
                }
                wanted
                    .map(|style| {
                        let style = crate::cmyk_style::with_black_as_k(&style);
                        crate::cmyk_style::canonical_hex(&style, policy)
                    })
                    .transpose()?
            }
            None => wanted.map(|style| style.without_inks()),
        };
        // A Grayscale document (#547 phase 2d, `gray_style`): an effect mode
        // outside the Grayscale list refuses by name, and every colour is
        // stored as its gray, so the stored style is what renders R = G = B.
        // Its Blend If channels were asked at the mode's gate.
        let wanted = match document.gray_policy() {
            Some(policy) => wanted
                .map(|style| {
                    if let Some(reason) =
                        crate::gray_style::style_refusal(&style, document.is_float())
                    {
                        return Err(reason);
                    }
                    crate::gray_style::greyed(&style, document.profile(), policy)
                })
                .transpose()?,
            None => wanted,
        };
        // A Lab Color document (#547 phase 2e, `lab_style`): an effect mode
        // outside the Lab list and a Pattern Overlay or Pattern stroke refuse
        // by name; every other style is stored as written (its colours
        // convert at composite). Its Blend If was asked at the mode's gate.
        if document.is_lab() {
            if let Some(reason) = wanted.as_ref().and_then(crate::lab_style::style_refusal) {
                return Err(reason);
            }
        }
        let replacement = old
            .clone()
            .with_layer_style(wanted.as_ref(), document.dimensions())?;
        Ok((*layer, old, replacement))
    }
    pub(crate) fn scratch_bytes(&self, document: &PrecisionDocument) -> Result<u64, String> {
        let (_, old, replacement) = self.target(document)?;
        let names: u64 = (0..document.layer_count())
            .filter_map(|i| document.layer(i))
            .map(|l| l.name().len() as u64)
            .sum();
        // The style text `apply` holds is the old and new canonical bodies it
        // compares beside the request's parsed style: four times the larger
        // canonical body, bounded by the cap the native writer enforces — not
        // the cap itself, which a 2 MiB custom tile made 8 MiB a call.
        // A long request's plan is the larger of this and the parse charge
        // (`ffi_precision_edit::request`, which records the measurement); for
        // a 512² tile this term is the larger by a few KB.
        let style = [old.reported_style(), replacement.reported_style()]
            .iter()
            .map(|s| s.as_ref().map_or(0, String::len))
            .max()
            .unwrap_or(0)
            .min(crate::precision_style_native::MAX_STYLE_JSON_LEN) as u64;
        Ok(4 * BOOKKEEPING_BYTES
            + names * 2
            + document.layer_count() as u64 * 4096
            + document.channels().len() as u64 * 1024
            + style * 4)
    }
    pub(crate) fn apply(
        &self,
        document: &PrecisionDocument,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        let scratch = self.scratch_bytes(document)?;
        admit(document.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        job.check()?;
        let (index, old, replacement) = self.target(document)?;
        if old.reported_style() == replacement.reported_style() {
            return Ok(None);
        }
        let result = document.replacing_layer(index, replacement)?;
        job.check()?;
        Ok(result)
    }
}
