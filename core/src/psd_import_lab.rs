//! Lab PSD, PSB and layered TIFF import (#1 Lab phase 3a): a colour-mode-9
//! file at 8 or 16 bits as a Lab Color document, its admission and stored
//! composite, and the Lab lines of the shared import report
//! (`psd_import::report`). `psd_import_gray` is the template.
//!
//! The framing, mask and folder rules are the RGB ones, through the same
//! walk (`psd_import::parse_with` with no profile: `GroupRecords::read`
//! decodes a Lab file's layer styles with `FxColours::Lab`, an `LbCl` colour
//! as its sRGB hex through [`lbcl_srgb`]) and the same typed tree build
//! (`psd_import16::emit_typed_tree`, `raster_layer`, `adjustment_layer`,
//! `group_layer`). The document is `lab_document`'s: RGBA16 storage at both
//! depths holding L\*, a\*, b\* in the ICC v4 encoding, every layer in the
//! policy's ONE twin handle. Each stored sample becomes the document's code
//! through `lab_encode::storage_code`'s PSD arm (measured by phase 3a's
//! take, `lab-files-547/take1`, and the readback in `lab_blend_take.py`):
//! an 8-bit sample ×257, a 16-bit L\* as stored and a\*/b\* on Photoshop's
//! 15-bit grid (`lab_encode::psd16_ab_code`). A layered TIFF's layer block
//! stores the PSD encoding too, whatever its flat image's Photometric (H-T,
//! proved by the take's twin files). What a record becomes:
//!
//! * a raster → its L, a, b planes and its transparency (none = opaque), its
//!   exact rectangle and signed offset; a pixel-less one that is not omitted
//!   → a canvas-sized transparent raster;
//! * a carried adjustment record the Lab table holds as a layer
//!   ([`adjustment_refusal`], `lab_adjust::record_refusal`) → a typed
//!   adjustment layer, its Lab form bound at parse on the twin; one it
//!   refuses is omitted and named, and so is every fill record (a Lab file's
//!   fill layers are outside phase 3a);
//! * a folder → a Normal (isolated) or Pass Through group;
//! * on every record: name, opacity, visibility, clipping, the independent
//!   mask (at the file's sample width), the label and the locks, and a blend
//!   key from `lab_document::LAB_BLEND_MODES` (any other is Normal, named in
//!   the report);
//! * its layer style when the Lab document can hold it ([`style_refusal`]:
//!   the 16-bit model's rule, `lab_style::style_refusal`'s effect modes, then
//!   Blend If on Lightness, a and b only). **Blend If:** Photoshop's
//!   blending ranges hold the composite in slot 0 and Lightness, a, b in
//!   slots 1 to 3, which the RGB slot table reads as Gray, Red, Green, Blue;
//!   the take measured an RGB-authored Gray range landing in slot 1 with slot
//!   0 the default (H-B). A Gray range that is the identity or EQUAL to the
//!   Lightness range is a copy and is dropped; any other refuses the style by
//!   `lab_style::channel_sentence`.
//!
//! Style colours stay authored sRGB (`lab_style`'s rule); `with_lab` makes
//! the document. A Lab file's embedded profile (resource 1039 or a TIFF's
//! ICC tag), when present, is not used — L\*a\*b\* values are CIE D50 by
//! definition — and the open and the report name it. A file that imports no
//! record ([`imports_nothing`]), and mode 1, open its saved composite
//! ([`lab_composite`]) as one layer; a TIFF has none here. The plan is the
//! 16-bit RGB open's (`psd_import16::budget` and `composite_plan`): RGBA16
//! retained, the Lab document's own storage.
//!
//! **Not observed by the take (H-L):** Photoshop's Image ▸ Mode ▸ Lab Color
//! (`Mrge` false) DISCARDED the RGB-authored Levels adjustment layer, so no
//! Lab adjustment record was measured; a carried one takes the RGB record
//! order (`psd_adjustment`) and `lab_adjust`'s Lab form, unmeasured.
//!
//! The writer is `psd_export_lab` (phase 3b): its samples are
//! `lab_encode::psd_sample`, this reader's decode inverted, and its
//! composite is matted over the same `lab_encode::LAB_WHITE`.

use std::sync::Arc;

use crate::blend::BlendMode;
use crate::job_control::{DrJobControl, JobContext};
use crate::lab_document::{blend_refusal, LabDepth, LabPolicy};
use crate::lab_encode::{storage_code, LabFileEncoding, LAB_WHITE};
use crate::metadata::Metadata;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, PrecisionImage};
use crate::psd_container::{Container, TIFF_COMPOSITE};
use crate::psd_group_records::GroupRecords;
use crate::psd_import16::{
    adjustment_layer, budget, composite_plan, emit_typed_tree, group_layer, raster_layer,
    typed_blend, typed_style_refusal,
};
use crate::psd_import_gray::record_plane;
use crate::psd_layer_effects::{Carried, StyleBody};
use crate::psd_layer_pixels::LayerPixels;
use crate::style::{BlendIf, BlendIfChannel, LayerStyle};

/// The saved-composite choice of a Lab file, as the report notes it.
pub(crate) const STORED_COMPOSITE_NOTE: &str = "Opens the saved composite as one Lab Color layer with its own L*a*b* values, including reconstruction from white when it has merged transparency; editable layers are not imported.";

/// The Lab import's general limits, stated once (the RGB report's
/// `GENERAL_LIMITATIONS` twin).
pub(crate) const LAB_GENERAL_LIMITATIONS: &[&str] = &[
    "A Lab Color file opens as a Lab Color document with its own L*a*b* values and declared depth (8 or 16 bits), whichever choice is made; an embedded profile is not used, because L*a*b* values are CIE D50 by definition.",
    "Raster layers keep their L*, a*, b* and transparency, rectangle and signed offset; groups keep Normal or Pass Through, their own blend mode, opacity, visibility, mask and open state; every record keeps its name, opacity, visibility, clipping, ordinary 20-byte mask, colour label, locks and Fill (fill opacity) where it can hold one, and a blend mode a Lab Color document offers (any other opens as Normal). Groups deeper than 10 levels are dissolved.",
    "Levels, Curves, Brightness/Contrast, Hue/Saturation, Color Balance, Photo Filter, Color Lookup, Invert, Posterize, Threshold and Gradient Map adjustment layers are carried as Doctored adjustment layers on the L*a*b* values; every other adjustment layer, and every fill layer, carries no pixels and is omitted, reported per record. The stored composite keeps their appearance.",
    "Layer effects (their colours as sRGB), knockout and Blend Interior Effects as Group are carried as a Doctored layer style on rasters, adjustment layers and every group, and Blend If ranges on Lightness, a and b; a style a Lab Color document cannot hold (an effect mode it does not offer, a range on another channel) is discarded, reported per record. Type, shape and smart-object layers import as their pixels only.",
    "Saved alpha and spot channels, guides and the ruler origin are not imported; colour samplers and the global light are.",
];

/// The colour planes of a Lab file.
const LAB_PLANES: usize = 3;

/// The LAYERED and stored-composite admission of a Lab file: the container's
/// side limit, colour mode 9, 8 or 16 bits and the three colour channels.
/// A layered Lab TIFF is admitted (its records are the layers); only its
/// stored composite is the ordinary TIFF open's.
pub(crate) fn lab_admission(c: &Container<'_>) -> Result<(), String> {
    let side = c.kind.max_side();
    if c.width > side || c.height > side {
        return Err(format!(
            "{} canvas exceeds the format's {side}-pixel dimension limit",
            c.kind.name()
        ));
    }
    if c.color != 9 {
        return Err(format!("a {} file is not Lab Color", c.color_name()));
    }
    if !matches!(c.depth, 8 | 16) {
        return Err(format!(
            "{}-bit Lab file cannot be opened: a Lab Color document is 8 or 16 bits per channel",
            c.depth
        ));
    }
    if usize::from(c.channels) < LAB_PLANES {
        return Err("Lab file has too few colour channels".into());
    }
    Ok(())
}

/// A Lab file's saved composite as the document's straight codes: the three
/// planes through `storage_code`'s PSD arm and the verified merged alpha
/// (an 8-bit code ×257). Merged transparency was stored against Lab white,
/// linearly in the codes, so it is unmatted against [`LAB_WHITE`] (widened
/// arithmetic, nearest rounding, saturation); alpha 0 is canonical white-less
/// `[0, 32896, 32896, 0]`.
pub(crate) fn lab_composite(c: &Container<'_>) -> Result<Vec<[u16; 4]>, String> {
    if c.kind == crate::psd_framing::LayeredContainer::Tiff {
        return Err(TIFF_COMPOSITE.into());
    }
    lab_admission(c)?;
    let bytes = c.sample_bytes() as usize;
    let alpha = c.merged_alpha && usize::from(c.channels) > LAB_PLANES;
    let kept = LAB_PLANES + usize::from(alpha);
    let pixels = c.width as usize * c.height as usize;
    let planes = c.composite_planes(kept, bytes)?;
    if planes.len() != kept * pixels * bytes {
        return Err("PSD composite plane length mismatch".into());
    }
    let mut out = vec![[0u16, 0, 0, 65535]; pixels];
    for (component, plane) in planes.chunks_exact(pixels * bytes).enumerate() {
        for (pixel, sample) in out.iter_mut().zip(plane.chunks_exact(bytes)) {
            let v = if bytes == 2 {
                u16::from_be_bytes([sample[0], sample[1]])
            } else {
                u16::from(sample[0])
            };
            pixel[component] = if component == LAB_PLANES {
                if bytes == 2 {
                    v
                } else {
                    v * 257
                }
            } else {
                storage_code(LabFileEncoding::Psd, c.depth, component, v)
            };
        }
    }
    if alpha {
        for pixel in &mut out {
            let a = i64::from(pixel[3]);
            match a {
                0 => *pixel = [0, LAB_WHITE[1], LAB_WHITE[2], 0],
                65535 => {}
                _ => {
                    for (code, white) in pixel[..3].iter_mut().zip(LAB_WHITE) {
                        let numerator =
                            (i64::from(*code) * 65535 - i64::from(white) * (65535 - a)).max(0);
                        *code = ((numerator + a / 2) / a).min(65535) as u16;
                    }
                }
            }
        }
    }
    Ok(out)
}

/// A Lab raster's straight samples as the document's codes: the L, a, b
/// planes through `storage_code`'s PSD arm, channel `-1` the transparency
/// (an 8-bit code ×257), a record with none opaque. `validate` has already
/// required channels 0 to 2.
fn decode_lab(raw: &LayerPixels<'_>, sample_bytes: usize) -> Result<Vec<[u16; 4]>, String> {
    if sample_bytes == 2 {
        let mut samples = raw.decode16()?;
        for pixel in &mut samples {
            for (component, code) in pixel[..3].iter_mut().enumerate() {
                *code = storage_code(LabFileEncoding::Psd, 16, component, *code);
            }
        }
        return Ok(samples);
    }
    Ok(raw
        .decode(3)?
        .pixels()
        .map(|p| {
            let [l, a, b, alpha] = p.0;
            [
                storage_code(LabFileEncoding::Psd, 8, 0, u16::from(l)),
                storage_code(LabFileEncoding::Psd, 8, 1, u16::from(a)),
                storage_code(LabFileEncoding::Psd, 8, 2, u16::from(b)),
                u16::from(alpha) * 257,
            ]
        })
        .collect())
}

/// `style` after Blend If's copy rule (module doc): a Gray range that is
/// the identity or equal to the Lightness (Red) range is dropped.
fn copied(style: &LayerStyle) -> LayerStyle {
    let Some(blend_if) = style.blend_if else {
        return style.clone();
    };
    let lightness = blend_if
        .ranges()
        .find(|range| range.channel == BlendIfChannel::Red && !range.is_identity());
    let kept: Vec<_> = blend_if
        .ranges()
        .filter(|range| !range.is_identity())
        .filter(|range| {
            range.channel != BlendIfChannel::Gray
                || !lightness.is_some_and(|l| {
                    l.this_layer == range.this_layer && l.underlying == range.underlying
                })
        })
        .collect();
    let mut out = style.clone();
    out.blend_if = match kept.split_first() {
        Some((first, rest)) => BlendIf::from_ranges(*first, rest).ok(),
        None => None,
    };
    out
}

/// Why a Lab document cannot carry this record's style, or `None`: the
/// 16-bit model's rule (`typed_style_refusal`), an effect mode outside the
/// Lab list (`lab_style::style_refusal`), then a Blend If range off
/// Lightness, a and b after the copy rule. The open and the report ask this
/// one question.
pub(crate) fn style_refusal(body: &StyleBody, plane: (u32, u32)) -> Option<String> {
    let StyleBody::Carried(carried) = body else {
        return None;
    };
    let style = copied(&carried.style);
    typed_style_refusal(&style, plane)
        .or_else(|| crate::lab_style::style_refusal(&style))
        .or_else(|| {
            style
                .blend_if
                .and_then(|b| crate::lab_style::blend_if_channel_refusal(&b))
        })
}

/// The body a Lab record is built with: none when [`style_refusal`]
/// refuses, else the carried style after the copy rule.
fn lab_body(body: &StyleBody, plane: (u32, u32)) -> StyleBody {
    let StyleBody::Carried(carried) = body else {
        return StyleBody::Absent;
    };
    if style_refusal(body, plane).is_some() {
        return StyleBody::Absent;
    }
    let style = copied(&carried.style);
    StyleBody::Carried(Carried {
        ranges: carried.ranges && style.blend_if.is_some(),
        style,
        effects: carried.effects.clone(),
        fill: carried.fill,
        knockout: carried.knockout,
        interior: carried.interior,
    })
}

/// Why a Lab document cannot hold this carried adjustment record as a layer
/// (`lab_adjust::record_refusal` of its typed description), or `None`.
pub(crate) fn adjustment_refusal(carried: &crate::psd_adjustment::Carried) -> Option<String> {
    crate::lab_adjust::record_refusal(&crate::psd_adjustment::typed_description(&carried.meta))
}

/// True when the Lab open does not import record `index`: the byte walk
/// omits it, it is a fill, or it is an adjustment the Lab table refuses
/// (module doc). The open's emit and the report's loops ask this one
/// question.
pub(crate) fn dropped(psd: &psd::Psd, records: &GroupRecords<'_>, index: usize) -> bool {
    records.omitted(index)
        || records.fill(index).is_some()
        || records.carried(index).is_some_and(|carried| {
            psd.layers().get(index).is_none() || adjustment_refusal(carried).is_some()
        })
}

/// The Lab twin of `GroupRecords::retains_nothing`: no folder and every
/// record [`dropped`], so Import Layers opens the stored composite.
pub(crate) fn imports_nothing(psd: &psd::Psd, records: &GroupRecords<'_>) -> bool {
    records.groups.is_empty() && (0..records.layers.len()).all(|index| dropped(psd, records, index))
}

/// `layer` with a blend mode the Lab document offers
/// (`lab_document::blend_refusal`): any other is Normal (a Pass Through
/// group is untouched).
fn lab_blend(layer: PrecisionDocumentLayer) -> Result<PrecisionDocumentLayer, String> {
    if blend_refusal(layer.blend_mode()).is_none() {
        return Ok(layer);
    }
    layer.with_blend_mode(BlendMode::Normal)
}

/// The layered open's plan, `(retained, scratch)`: the 16-bit RGB open's
/// (`psd_import16::budget`) in the twin, which the report's
/// `layered_plan_bytes` sums.
pub(crate) fn layered_plan(
    c: &Container<'_>,
    records: &GroupRecords<'_>,
    source_bytes: usize,
) -> Result<(u64, u64), String> {
    budget(c, records, source_bytes, &crate::lab_document::lab_twin())
}

/// One `LbCl` colour (L\*, a\*, b\* D50) as the sRGB bytes a Lab
/// document's style stores (`lab_style`'s authored colours): the Lab twin
/// into sRGB, relative colorimetric (`authored_color::authored_color`,
/// Mode ▸ Lab Color's rule reversed), clamped and rounded; black when the
/// pipeline cannot build.
pub(crate) fn lbcl_srgb(lab: [f64; 3]) -> [u8; 3] {
    let unit = [
        (lab[0] / 100.0) as f32,
        ((lab[1] + 128.0) / 255.0) as f32,
        ((lab[2] + 128.0) / 255.0) as f32,
    ];
    crate::authored_color::authored_color(&crate::lab_document::lab_twin(), false, unit)
        .map_or([0; 3], |rgb| {
            rgb.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8)
        })
}

/// The record walk's Lab lines (`psd_import::report`): each kept record's
/// style carried (`psd_import::report_style` of the style the open keeps)
/// or, when [`style_refusal`] refuses, that sentence as a loss; and each
/// blend key the Lab document does not offer.
pub(crate) fn lab_report_lines(
    psd: &psd::Psd,
    records: &GroupRecords<'_>,
    c: &Container<'_>,
    losses: &mut Vec<String>,
    observations: &mut Vec<String>,
) {
    let canvas = (c.width, c.height);
    let mut style = |body: &StyleBody, target: String, plane: (u32, u32)| {
        match style_refusal(
        body, plane,
    ) {
        Some(reason) => losses.push(format!("{target} has Photoshop layer effects that a Lab Color document cannot carry ({reason}); they are discarded.")),
        None => crate::psd_import::report_style(&lab_body(body, plane), &target, losses, observations),
    }
    };
    for (index, layer) in psd.layers().iter().enumerate() {
        if dropped(psd, records, index) {
            continue;
        }
        style(
            &records.layer_styles[index],
            format!("Layer {index} (\u{201c}{}\u{201d})", layer.name()),
            record_plane(records, index, canvas),
        );
    }
    for index in 0..records.groups.len() {
        let id = index as u32 + 1;
        style(
            &records.group_styles[index],
            format!("Group {id} (\u{201c}{}\u{201d})", psd.groups()[&id].name()),
            canvas,
        );
    }
    for (index, layer) in psd.layers().iter().enumerate() {
        if dropped(psd, records, index) {
            continue;
        }
        let blend = typed_blend(crate::psd::map_psd_blend(layer.blend_mode() as i32), false)
            .unwrap_or(BlendMode::Normal);
        if let Some(reason) = blend_refusal(blend) {
            losses.push(format!(
                "Layer {index} (\u{201c}{}\u{201d}) has the blend key {blend:?} ({reason}); it is imported as Normal.",
                layer.name()
            ));
        }
    }
    for (index, group) in records.groups.iter().enumerate() {
        let blend = typed_blend(group.blend, true).unwrap_or(BlendMode::Normal);
        if let Some(reason) = blend_refusal(blend) {
            let id = index as u32 + 1;
            losses.push(format!(
                "Group {id} (\u{201c}{}\u{201d}) has the blend key {blend:?} ({reason}); it is imported as Normal.",
                psd.groups()[&id].name()
            ));
        }
    }
}

/// The sentence naming a Lab file's unused embedded profile, or `None`.
fn unused_profile(c: &Container<'_>) -> Option<String> {
    let bytes = c.profile_bytes().ok().flatten()?;
    let name = crate::icc::IccBytes::new(bytes)
        .map(|view| view.name())
        .unwrap_or_else(|| "unreadable".into());
    Some(format!("The file's embedded profile \u{201c}{name}\u{201d} is not used: a Lab Color document's L*a*b* values are CIE D50 by definition."))
}

/// The file-level Lab lines (`psd_import::report`): the Lab open's own
/// observation, an unused embedded profile, the merged transparency when the
/// saved composite opens, the saved channels and the colour samplers.
pub(crate) fn lab_file_lines(
    c: &Container<'_>,
    composite_available: bool,
    losses: &mut Vec<String>,
    observations: &mut Vec<String>,
) {
    observations.push("This file opens as a Lab Color document, keeping its own L*a*b* values and declared depth, with either choice.".into());
    observations.extend(unused_profile(c));
    if c.merged_alpha && usize::from(c.channels) > LAB_PLANES && composite_available {
        observations.push("Saved-composite transparency is decoded from a preview stored against white; faint transparent colors may have less detail than editable layers.".into());
    }
    let extra = c
        .channels
        .saturating_sub(LAB_PLANES as u16 + u16::from(c.merged_alpha));
    if extra > 0 {
        losses.push(format!("{extra} additional saved channels detected in the header; neither import choice retains them as editable channels."));
    }
    if let Ok(samplers) = c.color_samplers() {
        losses.extend(samplers.losses);
        observations.extend(samplers.observations);
    }
}

impl PrecisionDocument {
    /// Opens a Lab PSD, PSB or layered TIFF at 8 or 16 bits as a Lab Color
    /// document (module doc): `mode` 0 its layers, 1 its saved composite (a
    /// TIFF has none). The job control, when present, is begun ONCE here.
    pub fn open_lab_psd(
        bytes: &[u8],
        path: &str,
        mode: i32,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        if !matches!(mode, 0 | 1) {
            return Err("unknown PSD import mode; use 0 (layers) or 1 (stored composite)".into());
        }
        let container = Container::read(bytes).map_err(|e| format!("PSD {path}: {e}"))?;
        lab_admission(&container).map_err(|e| format!("PSD {path}: {e}"))?;
        let depth = if container.depth == 16 {
            LabDepth::Sixteen
        } else {
            LabDepth::Eight
        };
        let policy = Arc::new(LabPolicy::for_depth(depth));
        // ONE twin handle for the document and every plane it holds.
        let twin = policy.twin().clone();
        let resolution = container
            .resolution()
            .map_err(|e| format!("PSD {path}: {e}"))?
            .unwrap_or_default();
        let light = container
            .global_light()
            .map_err(|e| format!("PSD {path}: {e}"))?
            .unwrap_or_default();
        let color_samplers = container
            .color_samplers()
            .map_err(|e| format!("PSD {path}: {e}"))?
            .kept;
        let (cw, ch) = (container.width, container.height);
        let warnings: Vec<String> = unused_profile(&container).into_iter().collect();
        let finish = |document: PrecisionDocument| -> Result<PrecisionDocument, String> {
            let mut document = document.with_import_warnings(warnings.clone())?;
            document.global_light = light;
            document.color_samplers.clone_from(&color_samplers);
            let document = document
                .with_lab(policy.clone())
                .map_err(|e| format!("PSD {path}: {e}"))?;
            job.report(1.0, 5)?;
            Ok(document)
        };
        let composite = || -> Result<PrecisionDocument, String> {
            job.check()?;
            let (retained, scratch) = composite_plan(cw, ch, bytes.len());
            admit(retained, scratch, other, limit)?;
            let pixels = lab_composite(&container).map_err(|e| format!("PSD {path}: {e}"))?;
            job.check()?;
            let image = PrecisionImage::from_rgba16_decoded(cw, ch, pixels, twin.clone(), None)?;
            PrecisionDocument::from_image(image, Metadata::default(), resolution)
        };
        if mode == 1 {
            return finish(composite()?);
        }
        let (psd, records) = crate::psd_import::parse_with(bytes, &container, None)
            .map_err(|e| format!("PSD {path}: {e}; inspect the saved-composite option"))?;
        let (retained, scratch) = budget(&container, &records, bytes.len(), &twin)?;
        admit(retained, scratch, other, limit)?;
        if imports_nothing(&psd, &records) {
            return finish(composite()?);
        }
        let sample_bytes = container.sample_bytes() as usize;
        let canvas = (cw, ch);
        let roots = emit_typed_tree(
            &psd,
            &records,
            &mut |index, dissolved| {
                if dropped(&psd, &records, index) {
                    return Ok(None);
                }
                job.check()?;
                let layer = &psd.layers()[index];
                let plane = record_plane(&records, index, canvas);
                let body = lab_body(&records.layer_styles[index], plane);
                let built = if let Some(carried) = records.carried(index) {
                    adjustment_layer(
                        layer,
                        carried,
                        records.masks[index],
                        sample_bytes,
                        &twin,
                        typed_blend(crate::psd::map_psd_blend(layer.blend_mode() as i32), false)
                            .unwrap_or(BlendMode::Normal),
                        &body,
                        canvas,
                        records.layer_labels[index],
                        records.layer_references[index],
                    )?
                } else {
                    raster_layer(
                        layer,
                        &records.layers[index],
                        records.masks[index],
                        &twin,
                        canvas,
                        &body,
                        records.layer_labels[index],
                        records.layer_references[index],
                        None,
                        &|raw| decode_lab(raw, sample_bytes),
                        sample_bytes,
                    )?
                };
                // `with_locks` is always LAST (after mask and style).
                Ok(Some(lab_blend(built)?.with_locks(
                    records.layer_features[index].locks() | dissolved,
                )))
            },
            &mut |id, children| {
                let built = group_layer(
                    psd.groups()[&id].name(),
                    &records.groups[id as usize - 1],
                    children,
                    sample_bytes,
                    &lab_body(&records.group_styles[id as usize - 1], canvas),
                    canvas,
                )?;
                lab_blend(built)
            },
        )
        .map_err(|reason| {
            format!("PSD {path}: {reason}; explicitly choose the stored composite if available")
        })?;
        if roots.is_empty() {
            return Err(format!("PSD {path}: no layer pixels could be imported; explicitly choose the stored composite if available"));
        }
        finish(PrecisionDocument::from_parts(
            cw,
            ch,
            Arc::clone(&twin),
            Metadata::default(),
            resolution,
            roots,
        )?)
    }
}
