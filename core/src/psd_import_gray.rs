//! Grayscale PSD, PSB and layered TIFF import (#547 phase 3a): a colour-mode-1
//! file at 8 or 16 bits as a Gray document, its policy, and the gray lines of
//! the shared import report. A 32-bit gray PSD or PSB (phase 3c) is
//! dispatched to `psd_import_gray32` after the policy, and every gray rule
//! here takes its `float` form there (the float document's blend and effect
//! modes, `psd_import_float::adjustment_refusal` on top of the Gray table).
//!
//! The framing, admission, mask and folder rules are the RGB ones, through
//! the same walk (`psd_import::parse_with`, whose `GroupRecords::read` decodes
//! a gray file's layer styles with `FxColours::Gray`: a `Grsc` colour through
//! the file's Gray profile, `gray_style::grsc_colour`) and the same typed tree
//! build (`psd_import16::emit_typed_tree`, `raster_layer`, `adjustment_layer`,
//! `group_layer`). The document is the Gray document of `gray_document`'s
//! module doc: RGBA16 storage at both depths with R = G = B, every layer in
//! the policy's ONE twin handle. What a record becomes:
//!
//! * a raster → its gray plane into R = G = B and its transparency
//!   (`LayerPixels::decode_gray`; an 8-bit code ×257, none = opaque), its
//!   exact rectangle and signed offset; a pixel-less one that is not omitted
//!   → a canvas-sized transparent raster;
//! * a carried adjustment record the Gray table holds as a layer
//!   ([`adjustment_refusal`], `gray_adjust::record_refusal`) → a typed
//!   adjustment layer; one it refuses is omitted and named, and so is every
//!   fill record (a gray file's fill layers are outside phase 3a). **The
//!   Gray channel record** (#547 phase 3a's take, `gray-files-547/take1`):
//!   Photoshop keeps a Grayscale Levels in its CHANNEL record (record 1) with
//!   the composite record the identity, and the RGB decoder reads record 1 as
//!   `r`; a Levels or Curves meta with `r` (or `r_map`) and no `g`/`b` is
//!   carried with `g` and `b` equal to it ([`gray_meta`]) — the kernel's own
//!   order (the channel, then the master) on the one channel, exactly;
//! * a folder → a Normal (isolated) or Pass Through group;
//! * on every record: name, opacity, visibility, clipping, the independent
//!   mask (at the file's sample width), the label and the locks, and a blend
//!   key from the Grayscale list (`gray_document::GRAY_BLEND_MODES`; any
//!   other is Normal, named in the report);
//! * its layer style when the Gray document can hold it ([`style_refusal`]:
//!   the 16-bit model's rule, the Grayscale effect modes and Blend If on Gray
//!   only), else none, named in the report. **Blend If's Gray channel:**
//!   Photoshop's blending ranges hold the composite Gray in slot 0 and the
//!   file's one channel in slot 1, which the RGB slot table reads as Red; the
//!   take measured Photoshop keeping a Grayscale document's Blend If in slot
//!   1 with slot 0 the default ([`copied`]): a Red range with no Gray range
//!   IS the Gray range, a Red range EQUAL to the Gray range is a copy and is
//!   dropped, and any other range off Gray refuses the style by name.
//!
//! After the build every style colour is greyed (`gray_style::grey_records`,
//! an `RGBC` that is not neutral included) and `with_gray` runs the mode's
//! open checks. The document takes the file's Gray profile ([`gray_policy`]:
//! `Container::profile_bytes`, a PSD's resource 1039 or a TIFF's ICC tag,
//! through `GrayPolicy::new`; an untagged file takes the working Gray under
//! `gray_io::untagged_sentence`), its declared depth, light and colour
//! samplers. At 32 bits a profile that is the linear twin of a built-in
//! (`icc_builtin::gray_linear_twin_source`, the profile our 32-bit export
//! embeds) is relabelled to that built-in, and the report discloses it
//! ([`gray_file_lines`]); any other Gray profile is kept verbatim. A file
//! that imports no record ([`imports_nothing`]: every record omitted, a fill
//! or a refused adjustment, and no folder), and mode 1, open
//! its saved composite (`Container::composite16`, or the byte composite ×257
//! at 8 bits) as one layer; a TIFF has none here. The plan is the 16-bit RGB
//! open's (`psd_import16::budget` and `composite_plan`): RGBA16 retained, the
//! Gray document's own storage.
//!
//! A Duotone file (colour mode 8 at 8 or 16 bits, #547 Duotone phase 1b)
//! opens here too: `Container::duotone_gray` is the Gray file it stores, and
//! its inks (`duotone_files::container_inks`, carrying the file block) go on
//! the policy as the document is finished, so it opens as a Duotone document.

use std::sync::Arc;

use crate::blend::BlendMode;
use crate::gray_document::{blend_refusal, GrayDepth, GrayPolicy};
use crate::gray_working::GrayWorkingSpace;
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::metadata::Metadata;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, PrecisionImage};
use crate::psd_container::Container;
use crate::psd_group_records::GroupRecords;
use crate::psd_import16::{
    adjustment_layer, budget, composite_plan, emit_typed_tree, group_layer, raster_layer,
    style_plane, typed_blend, typed_style_refusal,
};
use crate::psd_layer_effects::{Carried, StyleBody};
use crate::style::{BlendIf, BlendIfChannel, LayerStyle};

/// The saved-composite choice of a gray file, as the report notes it.
pub(crate) const STORED_COMPOSITE_NOTE: &str = "Opens the saved composite as one Grayscale layer with its own gray values and embedded Gray profile, including reconstruction from white when it has merged transparency; editable layers are not imported.";

/// The gray import's general limits, stated once (the RGB report's
/// `GENERAL_LIMITATIONS` twin).
pub(crate) const GRAY_GENERAL_LIMITATIONS: &[&str] = &[
    "A Grayscale file opens as a Grayscale document with its own gray values, declared depth (8, 16 or 32 bits) and embedded Gray profile (an untagged file takes the working Gray), whichever choice is made.",
    "Raster layers keep their gray and transparency, rectangle and signed offset; groups keep Normal or Pass Through, their own blend mode, opacity, visibility, mask and open state; every record keeps its name, opacity, visibility, clipping, ordinary 20-byte mask, colour label, locks and Fill (fill opacity) where it can hold one, and a blend mode from the 21 Photoshop offers in Grayscale (any other opens as Normal). Groups deeper than 10 levels are dissolved.",
    "Levels, Curves, Brightness/Contrast, Exposure, Invert, Posterize, Threshold and Gradient Map adjustment layers are carried as Doctored adjustment layers on the gray values; every other adjustment layer, and every fill layer, carries no pixels and is omitted, reported per record. The stored composite keeps their appearance.",
    "Layer effects (their colours as gray), knockout and Blend Interior Effects as Group are carried as a Doctored layer style on rasters, adjustment layers and every group, and Blend If ranges on Gray; a style a Grayscale document cannot hold (an effect mode outside the Grayscale list, a range on another channel) is discarded, reported per record. Type, shape and smart-object layers import as their pixels only.",
    "Saved alpha and spot channels, guides and the ruler origin are not imported; colour samplers and the global light are.",
];

/// The gray depth of a container's declared depth.
fn depth_of(container: &Container<'_>) -> GrayDepth {
    match container.depth {
        8 => GrayDepth::Eight,
        32 => GrayDepth::ThirtyTwo,
        _ => GrayDepth::Sixteen,
    }
}

/// A gray file's policy: its embedded profile (`Container::profile_bytes`)
/// through `GrayPolicy::new`, whose sentence refuses a profile that is not a
/// usable Gray one; an untagged file the `working` Gray, with
/// `gray_io::untagged_sentence` as its warning. At 32 bits the linear twin
/// of a built-in (`icc_builtin::gray_linear_twin_source`) takes that
/// built-in back: a relabel, since 32-bit storage is linear `Y` under both.
pub(crate) fn gray_policy(
    c: &Container<'_>,
    working: GrayWorkingSpace,
) -> Result<(GrayPolicy, Option<String>), String> {
    let depth = depth_of(c);
    match c.profile_bytes()? {
        Some(bytes) => {
            let source = (depth == GrayDepth::ThirtyTwo)
                .then(|| crate::icc_builtin::gray_linear_twin_source(bytes))
                .flatten();
            let policy = match source {
                Some(space) => GrayPolicy::for_space(space, depth)?,
                None => GrayPolicy::new(bytes, depth)?,
            };
            Ok((policy, None))
        }
        None => {
            let policy = GrayPolicy::for_space(working, depth)?;
            let sentence = crate::gray_io::untagged_sentence(policy.name());
            Ok((policy, Some(sentence)))
        }
    }
}

/// `style` after Blend If's Gray-channel rule (module doc): slot 1 (read as
/// Red) is the Gray range when slot 0 holds none, and a copy of it is
/// dropped.
fn copied(style: &LayerStyle) -> LayerStyle {
    let Some(blend_if) = style.blend_if else {
        return style.clone();
    };
    let gray = blend_if
        .ranges()
        .find(|range| range.channel == BlendIfChannel::Gray && !range.is_identity());
    let kept: Vec<_> = blend_if
        .ranges()
        .filter(|range| !range.is_identity())
        .filter_map(|range| match (range.channel, gray) {
            (BlendIfChannel::Red, None) => Some(crate::style::BlendIfRange {
                channel: BlendIfChannel::Gray,
                ..range
            }),
            (BlendIfChannel::Red, Some(gray))
                if gray.this_layer == range.this_layer && gray.underlying == range.underlying =>
            {
                None
            }
            _ => Some(range),
        })
        .collect();
    let mut out = style.clone();
    out.blend_if = match kept.split_first() {
        Some((first, rest)) => BlendIf::from_ranges(*first, rest).ok(),
        None => None,
    };
    out
}

/// Why a Gray document cannot carry this record's style, or `None`: the
/// 16-bit model's rule (`typed_style_refusal`), an effect mode outside the
/// Grayscale list (`gray_style::style_refusal`; at 32 bits, `float`, also
/// one the float document refuses), then a Blend If range off Gray after the
/// copy rule. The open and the report ask this one question.
pub(crate) fn style_refusal(body: &StyleBody, plane: (u32, u32), float: bool) -> Option<String> {
    let StyleBody::Carried(carried) = body else {
        return None;
    };
    let style = copied(&carried.style);
    typed_style_refusal(&style, plane)
        .or_else(|| crate::gray_style::style_refusal(&style, float))
        .or_else(|| {
            style
                .blend_if
                .and_then(|b| b.ranges().find(|r| r.channel != BlendIfChannel::Gray))
                .map(|range| crate::gray_style::channel_sentence(range.channel))
        })
}

/// The body a gray record is built with: none when [`style_refusal`]
/// refuses, else the carried style after the copy rule.
pub(crate) fn gray_body(body: &StyleBody, plane: (u32, u32), float: bool) -> StyleBody {
    let StyleBody::Carried(carried) = body else {
        return StyleBody::Absent;
    };
    if style_refusal(body, plane, float).is_some() {
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

/// A gray file's adjustment meta with its Gray channel record carried
/// (module doc): a Levels or Curves meta whose `r` (or `r_map`) stands alone
/// gets `g` and `b` equal to it; every other meta is returned unchanged.
fn gray_meta(meta: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(meta) else {
        return meta.to_string();
    };
    let op = value
        .get("op")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let Some(params) = value
        .get_mut("params")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return meta.to_string();
    };
    let keys: &[[&str; 3]] = match op.as_deref() {
        Some("levels") => &[["r", "g", "b"]],
        Some("curves") => &[["r", "g", "b"], ["r_map", "g_map", "b_map"]],
        _ => &[],
    };
    let mut changed = false;
    for [r, g, b] in keys {
        if let (Some(channel), None, None) = (params.get(*r), params.get(*g), params.get(*b)) {
            let channel = channel.clone();
            params.insert((*g).to_string(), channel.clone());
            params.insert((*b).to_string(), channel);
            changed = true;
        }
    }
    if changed {
        value.to_string()
    } else {
        meta.to_string()
    }
}

/// `carried` with its meta through [`gray_meta`].
pub(crate) fn gray_carried(
    carried: &crate::psd_adjustment::Carried,
) -> crate::psd_adjustment::Carried {
    crate::psd_adjustment::Carried {
        key: carried.key,
        kind: carried.kind,
        meta: gray_meta(&carried.meta),
    }
}

/// Why a Gray document cannot hold this carried adjustment record as a
/// layer (`gray_adjust::record_refusal` of its typed description after
/// [`gray_meta`]), or `None`. At 32 bits (`float`, the policy's twin) the
/// float form the gray-carried meta builds must also pass
/// (`psd_import_float::adjustment_refusal`).
pub(crate) fn adjustment_refusal(
    layer: &psd::PsdLayer,
    carried: &crate::psd_adjustment::Carried,
    float: Option<&Arc<IccProfile>>,
) -> Option<String> {
    crate::gray_adjust::record_refusal(&crate::psd_adjustment::typed_description(&gray_meta(
        &carried.meta,
    )))
    .or_else(|| {
        float.and_then(|twin| {
            crate::psd_import_float::adjustment_refusal(layer, &gray_carried(carried), twin)
        })
    })
}

/// True when the gray open does not import record `index`: the byte walk
/// omits it, it is a fill (a gray file's fill layers are not carried), or it
/// is an adjustment the Gray table refuses (module doc). The open's emit, the
/// report's style loop and its blend loop ask this one question.
pub(crate) fn dropped(
    psd: &psd::Psd,
    records: &GroupRecords<'_>,
    index: usize,
    float: Option<&Arc<IccProfile>>,
) -> bool {
    records.omitted(index)
        || records.fill(index).is_some()
        || records.carried(index).is_some_and(|carried| {
            psd.layers()
                .get(index)
                .is_none_or(|layer| adjustment_refusal(layer, carried, float).is_some())
        })
}

/// The gray twin of `GroupRecords::retains_nothing`: no folder and every
/// record [`dropped`], so Import Layers opens the stored composite (as the
/// RGB open does for a file of only pixel-less records) and the report gives
/// the layered choice the composite's availability.
pub(crate) fn imports_nothing(
    psd: &psd::Psd,
    records: &GroupRecords<'_>,
    float: Option<&Arc<IccProfile>>,
) -> bool {
    records.groups.is_empty()
        && (0..records.layers.len()).all(|index| dropped(psd, records, index, float))
}

/// `layer` with a blend mode the Gray document offers
/// (`gray_document::blend_refusal`, `float` for a 32-bit one): any other is
/// Normal (a Pass Through group is untouched).
pub(crate) fn gray_blend(
    layer: PrecisionDocumentLayer,
    float: bool,
) -> Result<PrecisionDocumentLayer, String> {
    if blend_refusal(layer.blend_mode(), float).is_none() {
        return Ok(layer);
    }
    layer.with_blend_mode(BlendMode::Normal)
}

/// The plane a record's style is checked against: a carried adjustment and
/// a fill are canvas-sized, a raster its rectangle.
pub(crate) fn record_plane(
    records: &GroupRecords<'_>,
    index: usize,
    canvas: (u32, u32),
) -> (u32, u32) {
    if records.carried(index).is_some() || records.fill(index).is_some() {
        canvas
    } else {
        style_plane(Some(&records.layers[index]), canvas)
    }
}

/// The record walk's gray lines (`psd_import::report`): each record's style
/// carried (`psd_import::report_style` of the style the open keeps) or, when
/// [`style_refusal`] refuses, that sentence as a loss; and each blend key
/// the Gray document does not offer (`float`, the twin, at 32 bits).
pub(crate) fn gray_report_lines(
    psd: &psd::Psd,
    records: &GroupRecords<'_>,
    c: &Container<'_>,
    float: Option<&Arc<IccProfile>>,
    losses: &mut Vec<String>,
    observations: &mut Vec<String>,
) {
    let canvas = (c.width, c.height);
    let thirty_two = float.is_some();
    let document = if thirty_two {
        "a 32-bit Grayscale document"
    } else {
        "a Grayscale document"
    };
    let mut style = |body: &StyleBody, target: String, plane: (u32, u32)| match style_refusal(
        body, plane, thirty_two,
    ) {
        Some(reason) => {
            let loss = format!("{target} has Photoshop layer effects that {document} cannot carry ({reason}); they are discarded.");
            losses.push(loss);
        }
        None => crate::psd_import::report_style(
            &gray_body(body, plane, thirty_two),
            &target,
            losses,
            observations,
        ),
    };
    for (index, layer) in psd.layers().iter().enumerate() {
        if dropped(psd, records, index, float) {
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
        if dropped(psd, records, index, float) {
            continue;
        }
        let blend = typed_blend(crate::psd::map_psd_blend(layer.blend_mode() as i32), false)
            .unwrap_or(BlendMode::Normal);
        if blend_refusal(blend, thirty_two).is_some() {
            losses.push(format!(
                "Layer {index} (\u{201c}{}\u{201d}) has the blend key {blend:?}, which {document} does not offer; it is imported as Normal.",
                layer.name()
            ));
        }
    }
    for (index, group) in records.groups.iter().enumerate() {
        let blend = typed_blend(group.blend, true).unwrap_or(BlendMode::Normal);
        if blend_refusal(blend, thirty_two).is_some() {
            let id = index as u32 + 1;
            losses.push(format!(
                "Group {id} (\u{201c}{}\u{201d}) has the blend key {blend:?}, which {document} does not offer; it is imported as Normal.",
                psd.groups()[&id].name()
            ));
        }
    }
}

/// The file-level gray lines (`psd_import::report`): the gray open's own
/// observation (at 32 bits, for a linear twin [`gray_policy`] relabels, the
/// relabel in its place), an untagged file's working-Gray observation (the
/// report takes no working Gray, so it names none), the merged transparency
/// when the saved composite opens, the saved channels and the colour
/// samplers.
pub(crate) fn gray_file_lines(
    c: &Container<'_>,
    policy: Option<&(GrayPolicy, Option<String>)>,
    composite_available: bool,
    losses: &mut Vec<String>,
    observations: &mut Vec<String>,
) {
    let embedded = c.profile_bytes().ok().flatten();
    let relabelled = match (policy, embedded) {
        (Some((policy, None)), Some(bytes)) if c.depth == 32 && bytes != policy.bytes() => {
            crate::icc::IccBytes::new(bytes).map(|view| (view.name(), policy.name().to_string()))
        }
        _ => None,
    };
    match relabelled {
        Some((file, builtin)) => {
            observations.push("This file opens as a Grayscale document, keeping its own gray values and declared depth, with either choice.".into());
            observations.push(format!(
                "The embedded profile \u{201c}{file}\u{201d} is the linear (gamma 1.0) twin of {builtin} that a 32-bit PSD carries; the document takes {builtin}, and its float samples are unchanged."
            ));
        }
        None => observations.push("This file opens as a Grayscale document, keeping its own gray values, declared depth and embedded Gray profile, with either choice.".into()),
    }
    if let Some((_, Some(_))) = policy {
        observations.push("The file has no usable embedded Gray profile; the working Gray (Color Settings) is assigned as it is opened.".into());
    }
    if c.merged_alpha && c.channels > 1 && composite_available {
        observations.push("Saved-composite transparency is decoded from a preview stored against white; faint transparent colors may have less detail than editable layers.".into());
    }
    let spots = c.spot_records();
    let extra = spots.alpha;
    if extra > 0 {
        losses.push(format!("{extra} additional saved channels detected in the header; neither import choice retains them as editable channels."));
    }
    losses.extend(spots.losses.iter().cloned());
    observations.extend(spots.observation());
    if let Ok(samplers) = c.color_samplers() {
        losses.extend(samplers.losses);
        observations.extend(samplers.observations);
    }
}

impl PrecisionDocument {
    /// Opens a Grayscale PSD, PSB or layered TIFF at 8 or 16 bits, or a PSD
    /// or PSB at 32 (`psd_import_gray32`, module doc): `mode` 0 its layers,
    /// 1 its saved composite (a TIFF has none).
    /// `working` is the working Gray's wire name, assigned to an untagged
    /// file. The job control, when present, is begun ONCE here.
    pub fn open_gray_psd(
        bytes: &[u8],
        path: &str,
        mode: i32,
        working: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        if !matches!(mode, 0 | 1) {
            return Err("unknown PSD import mode; use 0 (layers) or 1 (stored composite)".into());
        }
        let working = GrayWorkingSpace::from_name(working)
            .ok_or_else(|| format!("\"{working}\" is not a Gray working space"))?;
        let read = Container::read(bytes).map_err(|e| format!("PSD {path}: {e}"))?;
        // A Duotone file (#547 Duotone phase 1b) is the Gray file it stores
        // plus its inks (`duotone_files`), which `finish` puts on the policy.
        let inks = (read.color == 8)
            .then(|| crate::duotone_files::container_inks(&read))
            .transpose()
            .map_err(|e| format!("PSD {path}: {e}"))?;
        let container = read.duotone_gray().unwrap_or(read);
        if container.color != 1 {
            return Err(format!(
                "PSD {path}: a {} file is not Grayscale",
                container.color_name()
            ));
        }
        if let Some(reason) = container.support_refusal() {
            return Err(format!("PSD {path}: {reason}"));
        }
        let (policy, sentence) =
            gray_policy(&container, working).map_err(|e| format!("PSD {path}: {e}"))?;
        if container.depth == 32 {
            return crate::psd_import_gray32::open_gray32(
                bytes,
                path,
                mode,
                &container,
                &policy,
                sentence.into_iter().collect(),
                limit,
                other,
                job,
            );
        }
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
        let warnings: Vec<String> = sentence.into_iter().collect();
        let finish = |document: PrecisionDocument| -> Result<PrecisionDocument, String> {
            let mut document = document.with_import_warnings(warnings.clone())?;
            document.global_light = light;
            document.color_samplers.clone_from(&color_samplers);
            let policy = match &inks {
                Some(inks) => policy.with_duotone(Some(Arc::new(inks.clone()))),
                None => policy.clone(),
            };
            let document = document
                .with_gray(Arc::new(policy))
                .map_err(|e| format!("PSD {path}: {e}"))?
                .with_psd_spots(&container, limit, other)?;
            job.report(1.0, 5)?;
            Ok(document)
        };
        let composite = || -> Result<PrecisionDocument, String> {
            job.check()?;
            let (retained, scratch) = composite_plan(cw, ch, bytes.len());
            let retained = retained.saturating_add(container.spot_retained_bytes(2));
            admit(retained, scratch, other, limit)?;
            let pixels = if container.depth == 16 {
                container.composite16()
            } else {
                container.composite().map(|image| {
                    image
                        .pixels()
                        .map(|p| p.0.map(|v| u16::from(v) * 257))
                        .collect()
                })
            }
            .map_err(|e| format!("PSD {path}: {e}"))?;
            job.check()?;
            let image = PrecisionImage::from_rgba16_decoded(cw, ch, pixels, twin.clone(), None)?;
            PrecisionDocument::from_image(image, Metadata::default(), resolution)
        };
        if mode == 1 {
            return finish(composite()?);
        }
        let (psd, records) = crate::psd_import::parse_with(bytes, &container, Some(&policy))
            .map_err(|e| format!("PSD {path}: {e}; inspect the saved-composite option"))?;
        let (retained, scratch) = budget(&container, &records, bytes.len(), &twin)?;
        admit(retained, scratch, other, limit)?;
        if imports_nothing(&psd, &records, None) {
            return finish(composite()?);
        }
        let sample_bytes = container.sample_bytes() as usize;
        let canvas = (cw, ch);
        let roots = emit_typed_tree(
            &psd,
            &records,
            &mut |index, dissolved| {
                if dropped(&psd, &records, index, None) {
                    return Ok(None);
                }
                job.check()?;
                let layer = &psd.layers()[index];
                let plane = record_plane(&records, index, canvas);
                let body = gray_body(&records.layer_styles[index], plane, false);
                let built = if let Some(carried) = records.carried(index) {
                    adjustment_layer(
                        layer,
                        &gray_carried(carried),
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
                        &|raw| raw.decode_gray(sample_bytes),
                        sample_bytes,
                    )?
                };
                // `with_locks` is always LAST (after mask and style).
                Ok(Some(gray_blend(built, false)?.with_locks(
                    records.layer_features[index].locks() | dissolved,
                )))
            },
            &mut |id, children| {
                let built = group_layer(
                    psd.groups()[&id].name(),
                    &records.groups[id as usize - 1],
                    children,
                    sample_bytes,
                    &gray_body(&records.group_styles[id as usize - 1], canvas, false),
                    canvas,
                )?;
                gray_blend(built, false)
            },
        )
        .map_err(|reason| {
            format!("PSD {path}: {reason}; explicitly choose the stored composite if available")
        })?;
        if roots.is_empty() {
            return Err(format!("PSD {path}: no layer pixels could be imported; explicitly choose the stored composite if available"));
        }
        let mut roots = roots;
        crate::gray_style::grey_records(&mut roots, &twin, &policy)
            .map_err(|e| format!("PSD {path}: {e}"))?;
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
