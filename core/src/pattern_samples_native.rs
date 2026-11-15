//! The conditional NATIVE PATTERN SAMPLES word: word 115, set when some
//! stored pattern holds native samples (`pattern_samples`,
//! `designs/pattern-tiles.md`) — a custom tile's `native` key in a layer's
//! or a comp's style (a Pattern Overlay, a stroke's `pattern_fill`, a bevel's
//! texture, enabled or not) or in a Pattern Fill layer's description
//! (`pattern_fill.pattern.native`). The samples ride inside JSON already
//! stored, so there is no table; the word exists so that an older build,
//! whose pattern reader ignores the key, refuses the file by number instead
//! of silently drawing the tile's RGBA8 raster.
//!
//! It mirrors `clipping_path_native`'s word 114 in placement (no table, one
//! number at both depths: in a byte file the FIRST word inside 45, ahead of
//! 114; in a typed file the word right after the CMYK slot, ahead of 114, a
//! CMYK file's too) and `gradient_smoothness_native`'s word 103 in its claim,
//! checked both ways before any pixel decodes: the word without such a
//! pattern is [`INVALID_WORD`], such a pattern without the word is
//! [`WORD_REFUSAL`]. No file before this word holds a `native` key, so the
//! second check refuses no prior file. A document with none writes nothing,
//! so its bytes are the prior writer's. `DRDC_VERSION` stays 89.
//!
//! The word also admits a typed file's longer style bodies: an `LSTY` entry
//! whose style holds samples may reach [`MAX_SAMPLES_STYLE_JSON_LEN`]
//! (`precision_style_native`), every other entry keeps the 2 MiB cap.
//!
//! Convert to Profile, a converting layers paste and the comps' twin map a
//! `cmyk16` tile's ink through the conversion's leg, as every other style
//! ink ([`map_style_inks`], `cmyk_profile::converted_inks`).

use std::sync::Arc;

use crate::doc::{DrDocument, Layer};
use crate::doc_comps::LayerComp;
use crate::pattern_samples::SampleKind;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::style::{Effect, LayerStyle};
use crate::style_pattern_fill::PatternFill;

/// The conditional `.rz` word at both depths (module doc).
pub(crate) const WORD: u32 = 115;

/// Native pattern samples in a file that does not claim the word.
pub(crate) const WORD_REFUSAL: &str = "native pattern samples require DRDC 115";

/// The word in a file holding no native pattern samples.
pub(crate) const INVALID_WORD: &str = "invalid native pattern samples word";

/// The largest canonical style body a typed `LSTY` entry whose style holds
/// samples may carry (module doc). A 512² tile with `rgb32` samples is
/// 6,990,512 bytes of base64 (its raster's 1,398,104 and the samples'
/// 5,592,408); 16 MiB, the byte format's own style cap
/// (`drdc::MAX_DRDC_META_LEN`), holds two of them beside the rest of a
/// style.
pub(crate) const MAX_SAMPLES_STYLE_JSON_LEN: usize = crate::drdc::MAX_DRDC_META_LEN as usize;

/// The pattern fills a style stores: an overlay's, every stroke's
/// `pattern_fill` and every bevel's texture (a default one is a preset and
/// holds nothing).
fn fills(style: &LayerStyle) -> impl Iterator<Item = &PatternFill> {
    style.effects.iter().filter_map(|e| match e {
        Effect::PatternOverlay(p) => Some(&p.fill),
        Effect::Stroke(s) => Some(&s.pattern_fill),
        Effect::BevelEmboss(b) => Some(&b.texture.fill),
        _ => None,
    })
}

/// Whether `style` holds a pattern with native samples, on any effect
/// enabled or not.
pub(crate) fn holds_samples(style: &LayerStyle) -> bool {
    fills(style).any(|f| f.pattern.tile.samples.is_some())
}

/// Whether `style` holds a pattern with `cmyk16` samples: the inks a CMYK
/// conversion maps ([`map_style_inks`]).
pub(crate) fn holds_ink_samples(style: &LayerStyle) -> bool {
    fills(style).any(|f| {
        f.pattern
            .tile
            .samples
            .as_deref()
            .is_some_and(|s| s.kind() == SampleKind::Cmyk16)
    })
}

/// `style` with each `cmyk16` tile's ink mapped by `f` (unit C, M, Y, K out
/// of the texel's four codes), every other fill as it is: Convert to
/// Profile's leg (`cmyk_profile::converted_inks`). The same style back
/// (equal) when it holds none.
pub(crate) fn map_style_inks(
    style: &LayerStyle,
    f: &mut dyn FnMut([u16; 4]) -> Result<[f32; 4], String>,
) -> Result<LayerStyle, String> {
    if !holds_ink_samples(style) {
        return Ok(style.clone());
    }
    let mut failed: Option<String> = None;
    let next = crate::style_pattern::refilled(style, |fill, _| {
        let samples = fill.pattern.tile.samples.as_deref()?;
        if samples.kind() != SampleKind::Cmyk16 || failed.is_some() {
            return None;
        }
        match samples.map_inks(f) {
            Ok(mapped) => {
                let mut tile = (*fill.pattern.tile).clone();
                tile.samples = Some(Arc::new(mapped));
                let mut next = fill.clone();
                next.pattern.tile = Arc::new(tile);
                Some(next)
            }
            Err(e) => {
                failed = Some(e);
                None
            }
        }
    });
    if let Some(e) = failed {
        return Err(e);
    }
    Ok(next.unwrap_or_else(|| style.clone()))
}

/// Whether `text` is a Pattern Fill layer's description (`"type":"fill"`,
/// `"fill":"pattern"`) whose `pattern_fill.pattern` holds a `native` object.
/// Anything else, malformed JSON included, claims nothing.
pub(crate) fn describes_samples(text: &str) -> bool {
    if !text.contains("native") {
        return false;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    value.get("type").and_then(serde_json::Value::as_str) == Some("fill")
        && value.get("fill").and_then(serde_json::Value::as_str) == Some("pattern")
        && value
            .get("pattern_fill")
            .and_then(|f| f.get("pattern"))
            .and_then(|p| p.get("native"))
            .is_some_and(serde_json::Value::is_object)
}

/// Whether some comp's recorded appearance holds such a pattern (read
/// leniently, as the `.rz` readers read it; one that does not parse claims
/// nothing).
pub(crate) fn comps_samples(comps: &[LayerComp]) -> bool {
    comps.iter().any(|comp| {
        comp.entries.iter().any(|entry| {
            entry
                .appearance
                .as_ref()
                .and_then(|a| a.style.as_deref())
                .filter(|json| json.contains("native"))
                .and_then(|json| LayerStyle::from_json_lenient(json).ok())
                .is_some_and(|style| holds_samples(&style))
        })
    })
}

/// The byte predicate over a reader's pending records and parsed comps: a
/// stored style, a layer meta's fill description or a comp.
pub(crate) fn pending_samples(layers: &[Layer], comps: &[LayerComp]) -> bool {
    layers.iter().any(|l| {
        l.style.as_deref().is_some_and(holds_samples)
            || l.meta.as_deref().is_some_and(describes_samples)
    }) || comps_samples(comps)
}

/// The byte word's predicate (module doc).
pub(crate) fn styles_samples(doc: &DrDocument) -> bool {
    pending_samples(&doc.layers, &doc.comps)
}

/// The typed predicate: some record of the tree's style, some record's
/// artwork description, or some comp.
pub(crate) fn styles_samples16(doc: &PrecisionDocument) -> bool {
    records_samples16(&doc.layers)
        || (0..doc.layer_count()).any(|i| doc.artwork_description(i).is_some_and(describes_samples))
        || comps_samples(&doc.comps)
}

fn records_samples16(layers: &[PrecisionDocumentLayer]) -> bool {
    layers.iter().any(|l| {
        l.style().is_some_and(|s| holds_samples(s)) || l.children().is_some_and(records_samples16)
    })
}
