//! Typed Blend If: the byte model's `style_model::BlendIf` (a channel and two
//! `[lo0, lo1, hi0, hi1]` ramps over 0..=255) carried per layer by the 16-bit
//! document and applied by `precision_render` exactly where the byte
//! compositor applies it — a weight multiplied into the layer's effective
//! alpha, "underlying" being the accumulator beneath. The ramps are evaluated
//! on 16-bit codes by `style_blend_if::source_weight_16`; the policy is
//! `designs/phase-10d-blend-if-status.md`. This is metadata over the
//! existing f32 compositor, not a new pixel kernel: Blend If is admitted on
//! raster layers, adjustment layers (This Layer reads the adjusted colour,
//! `precision_style_adjustment`; a CMYK one on ink through `cmyk_style_row`)
//! and groups, a Pass Through group included (it then composites ISOLATED,
//! `PrecisionDocumentLayer::isolates`).
//!
//! the typed `Option<BlendIf>` (the byte `style_model::BlendIf`) on raster layers, adjustments
//! and groups: `validate`, `with_blend_if` (identity → `None`), the strict `layer_blend_if` edit
//! (unknown fields refuse, exact no-op `None`).
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, BOOKKEEPING_BYTES};
use crate::style::{BlendIf, BlendIfChannel, Strictness};
use crate::style_json::{parse_blend_if, requested_blend_if_channels};
use serde::Deserialize;

/// The one admission rule: `None` on anything; `Some` on any record that
/// carries one (a raster, an adjustment, a group of either kind), and only on
/// a channel the document's mode has (#547): a CMYK plate needs a CMYK document (`cmyk`), and Red,
/// Green or Blue on one refuses by name.
pub(crate) fn validate(blend_if: Option<BlendIf>, cmyk: bool) -> Result<(), String> {
    let Some(blend_if) = blend_if else {
        return Ok(());
    };
    channel_refusal(blend_if, cmyk).map_or(Ok(()), Err)
}

/// A Blend If channel the document's mode does not have, by name (#547): a
/// CMYK plate on an RGB, float or byte document — "Blend If on the Cyan
/// channel needs a CMYK document" — and Red, Green or Blue on a CMYK one
/// through the mode's own sentence. Every range is checked;
/// the first refusing one names the sentence.
pub(crate) fn channel_refusal(blend_if: BlendIf, cmyk: bool) -> Option<String> {
    blend_if
        .ranges()
        .find_map(|range| channel_sentence(range.channel, cmyk))
}

/// [`channel_refusal`] over a REQUEST's ranges as written
/// (`style_json::requested_blend_if_channels`), identity ranges included —
/// what the setters check before canonicalization drops an identity range:
/// a channel the mode lacks refuses even as an identity ramp (#547).
pub(crate) fn requested_channel_refusal(
    blend_if: &serde_json::Value,
    cmyk: bool,
) -> Option<String> {
    requested_blend_if_channels(blend_if)
        .into_iter()
        .find_map(|channel| channel_sentence(channel, cmyk))
}

fn channel_sentence(channel: BlendIfChannel, cmyk: bool) -> Option<String> {
    let name = channel.display_name();
    if channel.is_cmyk() && !cmyk {
        return Some(format!(
            "Blend If on the {name} channel needs a CMYK document"
        ));
    }
    if channel.is_rgb() && cmyk {
        return Some(crate::cmyk_document::refused_sentence(&format!(
            "Blend If on the {name} channel"
        )));
    }
    None
}

impl PrecisionDocumentLayer {
    pub fn blend_if(&self) -> Option<BlendIf> {
        self.blend_if
    }
    /// Consumes a newly constructed layer. An identity Blend If (both ramps
    /// full) stores as `None`, the byte `LayerStyle` rule, so a document that
    /// carries none keeps its prior native writer; incompatible targets refuse
    /// without rewriting pixels, masks or placement.
    pub fn with_blend_if(mut self, blend_if: Option<BlendIf>) -> Result<Self, String> {
        // The canonical value (`BlendIf::from_ranges`: a channel twice
        // refuses), so the native writer's schema 2 rules always hold.
        let blend_if = blend_if
            .map(|b| {
                let ranges: Vec<_> = b.ranges().collect();
                BlendIf::from_ranges(ranges[0], &ranges[1..])
            })
            .transpose()?
            .filter(|b| !b.is_identity());
        // A layer does not know its document; its subtree's first leaf says
        // which mode it was built for. An empty group says nothing, so its
        // channel is checked by the document that receives it (the CMYK
        // gate, the native reader).
        let cmyk = self
            .mode_hint()
            .unwrap_or_else(|| blend_if.is_some_and(|b| b.ranges().any(|r| r.channel.is_cmyk())));
        validate(blend_if, cmyk)?;
        self.blend_if = blend_if;
        Ok(self)
    }

    /// Whether this record's subtree is CMYK (its first leaf a CMYK raster or
    /// adjustment), RGB (an RGB one), or `None` with no leaf at all.
    pub(crate) fn mode_hint(&self) -> Option<bool> {
        match &self.content {
            Content::CmykRaster { .. } | Content::CmykAdjustment(_) => Some(true),
            Content::Raster { .. } | Content::Adjustment(_) => Some(false),
            Content::Group(children) | Content::PassThrough(children) => {
                children.iter().find_map(|child| child.mode_hint())
            }
        }
    }
}

/// `{op:"layer_blend_if",version:1,layer,blend_if:null|{channel,this_layer,
/// underlying,extra?}}` — the style JSON's own `blend_if` object, parsed
/// strictly.
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum LayerBlendIfEdit {
    LayerBlendIf {
        version: u32,
        layer: usize,
        blend_if: serde_json::Value,
    },
}
impl LayerBlendIfEdit {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text)
            .map_err(|e| format!("invalid native layer blend-if request: {e}"))
    }
    fn target<'a>(
        &self,
        document: &'a PrecisionDocument,
    ) -> Result<(usize, &'a PrecisionDocumentLayer, Option<BlendIf>), String> {
        let Self::LayerBlendIf {
            version,
            layer,
            blend_if,
        } = self;
        if *version != 1 {
            return Err("unsupported native layer blend-if request version".into());
        }
        let old = document
            .layer(*layer)
            .ok_or("native blend-if layer index is out of range")?;
        if let Some(object) = blend_if.as_object() {
            if let Some(key) = object.keys().find(|k| {
                !matches!(
                    k.as_str(),
                    "channel" | "this_layer" | "underlying" | "extra"
                )
            }) {
                return Err(format!("unknown native blend-if field {key}"));
            }
            // Each `extra` range is held to the same strict key set (less
            // `extra` itself), so an unknown key is refused wherever it sits.
            let entries = object.get("extra").and_then(|e| e.as_array());
            for entry in entries.into_iter().flatten() {
                let keys = entry.as_object().into_iter().flat_map(|o| o.keys());
                for key in keys {
                    if !matches!(key.as_str(), "channel" | "this_layer" | "underlying") {
                        return Err(format!("unknown native blend-if field {key}"));
                    }
                }
            }
        }
        let parsed = parse_blend_if(blend_if, Strictness::Strict)?;
        // A channel the document has not refuses even as an identity ramp
        // (#547): the request names a channel this mode cannot read — in any
        // range as written, before canonicalization drops identity ranges.
        if let Some(reason) = requested_channel_refusal(blend_if, document.is_cmyk()) {
            return Err(reason);
        }
        let wanted = parsed.filter(|b| !b.is_identity());
        validate(wanted, document.is_cmyk())?;
        Ok((*layer, old, wanted))
    }
    pub(crate) fn scratch_bytes(&self, document: &PrecisionDocument) -> Result<u64, String> {
        self.target(document)?;
        let names: u64 = (0..document.layer_count())
            .filter_map(|i| document.layer(i))
            .map(|l| l.name().len() as u64)
            .sum();
        Ok(4 * BOOKKEEPING_BYTES
            + names * 2
            + document.layer_count() as u64 * 4096
            + document.channels().len() as u64 * 1024)
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
        let (index, old, wanted) = self.target(document)?;
        if old.blend_if() == wanted {
            return Ok(None);
        }
        let replacement = old.clone().with_blend_if(wanted)?;
        let result = document.replacing_layer(index, replacement)?;
        job.check()?;
        Ok(result)
    }
}
