//! The Photoshop STYLE LIBRARY writer (`.asl`): the Styles panel's
//! Export Styles… / Export Group… and MCP `export_style_presets`, through
//! `dr_asl_encode`. The layout is `asl`'s (its module doc), which reads every
//! file written here back: `u16` 2, `8BSL`, `u16` 3, the pattern section
//! (`psd_patterns::tail_blocks`' `Patt` body: every distinct pattern the
//! presets' pattern fills hold), the style count and each style record
//! (`u32` length, then the record). Records take Photoshop 26.10's own form
//! (its libraries `styles_a.asl` / `styles_b.asl`; Photoshop refused
//! the earlier form with "program error"): the record is zero-padded to a
//! multiple of 4 and its length counts that padding, so nothing is padded
//! outside a record.
//!
//! A record is the IDENTITY descriptor (`null`: `Nm  ` the name, `Idnt` the
//! 8-4-4-4-12 hex text, `psd_patterns::uuid_text`, of
//! `psd_patterns::fnv1a128` over the name and the canonical style JSON, so
//! the same preset always gets the same id) and the STYLE (`Styl`:
//! `documentMode`, an empty `documentMode` object; `Lefx`, `Scl ` and then
//! the effects: the items a PSD record's effects block frames,
//! `psd_layer_effects::effect_items`, less that body's `masterFXSwitch` and
//! `numModifyingFX`, which Photoshop's libraries do not hold, written with
//! each effect's OWN stored angle and altitude — a library carries no
//! document light — and colours as sRGB, `FxColours::Rgb(None)`: a wide
//! colour writes its sRGB numbers unquantized, which re-read as the same wide
//! colour inside sRGB's gamut and as its clamped byte outside it; then
//! `blendOptions` only when the preset carries a blending option (layer
//! blending, fill not 1, Blend Interior, a knockout or a non-identity Blend
//! If), and then always Photoshop's full form, S9 of `styles_b.asl`: `Opct`,
//! `Md  `, `fillOpacity`, `blendInterior`, `knockout` and `Blnd`, in that
//! order, with Normal, 100 %, false and `None` for the unset ones, and in
//! `Blnd` one entry per non-identity range, in canonical channel order,
//! `asl::BLND_CHANNELS` / `asl::BLND_RAMPS`, empty when there is none.
//! Photoshop 26.10 crashed on a `blendOptions` without `Opct`/`Md  `,
//! and its form has no "style options without layer blending", so a
//! preset with blending null but a style blending option re-reads with
//! blending Normal 100 %).
//!
//! A preset whose style does not canonicalize (`style_presets::preset_style`)
//! or that Photoshop's dialog cannot hold (`psd_layer_effects::bake_reason`),
//! or any of whose Blend If ranges reads a CMYK plate, is skipped and named
//! "<name>: <reason>". The output stays inside the reader's caps
//! (`asl::MAX_ASL_STYLES`, `asl::MAX_ASL_BYTES`).

use serde_json::{json, Value};

use crate::asl::{BLND_CHANNELS, BLND_RAMPS, MAX_ASL_BYTES, MAX_ASL_STYLES};
use crate::blend::BlendMode;
use crate::psd_descriptor::{self, Descriptor, Item, RefItem};
use crate::psd_layer_effects::{effect_items, FxColours};
use crate::psd_layer_effects_fx::{blend_item, percent};
use crate::psd_patterns::{fnv1a128, tail_blocks, uuid_text};
use crate::style::{blend_mode_from_name, BlendIfRange, Knockout, LayerStyle, Strictness};
use crate::style_names::{enum_name, CHANNELS};
use crate::style_presets::preset_style;

/// One preset to write: its name, its style and, when it includes blending
/// options, the layer mode and opacity (0..=1).
pub(crate) struct AslPreset {
    pub name: String,
    pub style: LayerStyle,
    pub blending: Option<(BlendMode, f32)>,
}

const CMYK_BLEND_IF: &str = "Blend If on a CMYK plate cannot be written to a style library";

/// The library holding every preset that can be written, and a sentence
/// naming each skipped one (module doc). `Err` when none is left or the file
/// would pass the reader's caps.
pub(crate) fn write(presets: &[AslPreset]) -> Result<(Vec<u8>, Vec<String>), String> {
    let mut skipped = Vec::new();
    let mut kept: Vec<(&AslPreset, LayerStyle, Vec<u8>)> = Vec::new();
    for preset in presets {
        match record(preset) {
            Ok((style, bytes)) => kept.push((preset, style, bytes)),
            Err(reason) => skipped.push(format!("{}: {reason}", preset.name)),
        }
    }
    if kept.is_empty() {
        return Err(if skipped.is_empty() {
            "there are no style presets to write".to_string()
        } else {
            format!(
                "no style preset can be written to a style library ({})",
                skipped.join("; ")
            )
        });
    }
    if kept.len() > MAX_ASL_STYLES {
        return Err(format!(
            "a style library holds at most {MAX_ASL_STYLES} styles, not {}",
            kept.len()
        ));
    }
    let mut out = Vec::new();
    out.extend_from_slice(&2u16.to_be_bytes());
    out.extend_from_slice(b"8BSL");
    out.extend_from_slice(&3u16.to_be_bytes());
    let patterns = tail_blocks(kept.iter().map(|(_, style, _)| style), false);
    let section = patterns
        .first()
        .map_or(&[][..], |(_, body)| body.as_slice());
    put_u32(&mut out, section.len())?;
    out.extend_from_slice(section);
    put_u32(&mut out, kept.len())?;
    for (_, _, bytes) in &kept {
        put_u32(&mut out, bytes.len())?;
        out.extend_from_slice(bytes);
        if out.len() > MAX_ASL_BYTES {
            return Err(crate::asl::too_large(out.len()));
        }
    }
    Ok((out, skipped))
}

fn put_u32(out: &mut Vec<u8>, len: usize) -> Result<(), String> {
    let len = u32::try_from(len).map_err(|_| crate::asl::too_large(len))?;
    out.extend_from_slice(&len.to_be_bytes());
    Ok(())
}

/// One preset's canonical style and its record bytes (padded, module doc),
/// or why it is skipped.
fn record(preset: &AslPreset) -> Result<(LayerStyle, Vec<u8>), String> {
    let json = preset.style.to_json();
    let style = preset_style(&json)?;
    let canonical = style.to_json();
    let mut lefx = effect_items(&style, None, FxColours::Rgb(None))?;
    // The PSD body's envelope keys a library's `Lefx` does not hold.
    lefx.retain(|(key, _)| key != b"masterFXSwitch" && key != b"numModifyingFX");
    let blending = blend_options(&style, preset.blending)?;
    let id = fnv1a128(&[preset.name.as_bytes(), &[0], canonical.as_bytes()]);
    let identity = Descriptor {
        class: b"null".to_vec(),
        items: vec![
            (b"Nm  ".to_vec(), Item::Text(preset.name.clone())),
            (b"Idnt".to_vec(), Item::Text(uuid_text(id))),
        ],
    };
    let mut items = vec![
        (
            b"documentMode".to_vec(),
            Item::Object(Descriptor {
                class: b"documentMode".to_vec(),
                items: vec![],
            }),
        ),
        (
            b"Lefx".to_vec(),
            Item::Object(Descriptor {
                class: b"Lefx".to_vec(),
                items: lefx,
            }),
        ),
    ];
    if !blending.is_empty() {
        items.push((
            b"blendOptions".to_vec(),
            Item::Object(Descriptor {
                class: b"blendOptions".to_vec(),
                items: blending,
            }),
        ));
    }
    let styl = Descriptor {
        class: b"Styl".to_vec(),
        items,
    };
    let mut bytes = psd_descriptor::write(&identity);
    bytes.extend_from_slice(&psd_descriptor::write(&styl));
    bytes.resize(bytes.len().next_multiple_of(4), 0);
    Ok((style, bytes))
}

/// The `blendOptions` items (module doc).
fn blend_options(
    style: &LayerStyle,
    blending: Option<(BlendMode, f32)>,
) -> Result<Vec<(Vec<u8>, Item)>, String> {
    let blend_if = style.blend_if.filter(|b| !b.is_identity());
    if blending.is_none()
        && style.fill_opacity == 1.0
        && !style.blend_interior_as_group
        && style.knockout == Knockout::None
        && blend_if.is_none()
    {
        return Ok(Vec::new());
    }
    let (mode, opacity) = blending.unwrap_or((BlendMode::Normal, 1.0));
    // `None` is the charID of the stringID `none`, and Photoshop writes an
    // id that has a four-char code as that code; `shallow` and `deep` have
    // none, so they stay stringIDs with their length (S9 of `styles_b.asl`),
    // `Item::EnumStringId` so the four-byte `deep` is not written as a charID.
    let kind = b"knockout".to_vec();
    let knockout = match style.knockout {
        Knockout::None => Item::Enum(kind, b"None".to_vec()),
        Knockout::Shallow => Item::EnumStringId(kind, b"shallow".to_vec()),
        Knockout::Deep => Item::EnumStringId(kind, b"deep".to_vec()),
    };
    // The ranges in canonical order (`BlendIf::from_ranges`), the identity
    // ones left out; Photoshop writes only the restricted channels.
    let entries = match blend_if {
        Some(blend_if) => blend_if
            .ranges()
            .filter(|r| !r.is_identity())
            .map(|r| blnd_entry(&r))
            .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };
    Ok(vec![
        (b"Opct".to_vec(), percent(opacity.clamp(0.0, 1.0))),
        (b"Md  ".to_vec(), blend_item(mode)),
        (b"fillOpacity".to_vec(), percent(style.fill_opacity)),
        (
            b"blendInterior".to_vec(),
            Item::Bool(style.blend_interior_as_group),
        ),
        (b"knockout".to_vec(), knockout),
        (b"Blnd".to_vec(), Item::List(entries)),
    ])
}

/// One `Blnd` entry (module doc).
fn blnd_entry(range: &BlendIfRange) -> Result<Item, String> {
    let name = enum_name(&CHANNELS, range.channel);
    let channel = BLND_CHANNELS
        .iter()
        .find(|(_, n)| *n == name)
        .map(|(id, _)| *id)
        .ok_or(CMYK_BLEND_IF)?;
    let mut items = vec![(
        b"Chnl".to_vec(),
        Item::Reference(vec![RefItem::Enumerated {
            class: b"Chnl".to_vec(),
            kind: b"Chnl".to_vec(),
            value: channel.to_vec(),
        }]),
    )];
    for (keys, ramp) in BLND_RAMPS.iter().zip([range.this_layer, range.underlying]) {
        for (key, value) in keys.iter().zip(ramp) {
            items.push((key.to_vec(), Item::Long(i32::from(value))));
        }
    }
    Ok(Item::Object(Descriptor {
        class: b"Blnd".to_vec(),
        items,
    }))
}

/// `dr_asl_encode`'s body: `[{"name", "style": object, "blending": {"mode",
/// "opacity"} | null}]` in, `{"asl": "<standard base64>", "skipped":
/// [sentence]}` out. A preset whose name is not a string, whose style does
/// not parse or whose blending does not read is skipped and named; `Err` on
/// JSON that is not such a list, more than [`MAX_ASL_STYLES`] presets, or
/// [`write`]'s refusals.
pub(crate) fn encode_json(json: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(json).map_err(|e| format!("not valid JSON: {e}"))?;
    let list = value
        .as_array()
        .ok_or("presets must be an array of {name, style, blending}")?;
    if list.len() > MAX_ASL_STYLES {
        return Err(format!(
            "a style library holds at most {MAX_ASL_STYLES} styles, not {}",
            list.len()
        ));
    }
    let mut presets = Vec::with_capacity(list.len());
    let mut skipped = Vec::new();
    for (i, entry) in list.iter().enumerate() {
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .map_or_else(|| format!("preset {}", i + 1), str::to_string);
        match preset(entry, &name) {
            Ok(preset) => presets.push(preset),
            Err(reason) => skipped.push(format!("{name}: {reason}")),
        }
    }
    let (bytes, written_skipped) = write(&presets).map_err(|reason| {
        if skipped.is_empty() {
            reason
        } else {
            format!("{reason}; {}", skipped.join("; "))
        }
    })?;
    skipped.extend(written_skipped);
    Ok(json!({
        "asl": crate::adjust_lut::base64_encode(&bytes),
        "skipped": skipped,
    })
    .to_string())
}

/// One input entry as a preset to write.
fn preset(entry: &Value, name: &str) -> Result<AslPreset, String> {
    let style = entry.get("style").ok_or("style is missing")?;
    let style = LayerStyle::from_value(style, Strictness::Strict)?;
    let blending = match entry.get("blending") {
        None | Some(Value::Null) => None,
        Some(b) => {
            let mode = b
                .get("mode")
                .and_then(Value::as_str)
                .and_then(blend_mode_from_name)
                .ok_or("blending.mode must be a blend mode name")?;
            let opacity = b
                .get("opacity")
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite())
                .ok_or("blending.opacity must be a number")?;
            Some((mode, opacity.clamp(0.0, 1.0) as f32))
        }
    };
    Ok(AslPreset {
        name: name.to_string(),
        style,
        blending,
    })
}
