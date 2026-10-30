//! Layered CMYK PSD, PSB and TIFF import (#1 phase 3): Import Layers of a
//! colour-mode-4 file as a CMYK document, its plan, and the CMYK lines of the
//! shared import report.
//!
//! The framing, admission, mask and folder rules are the RGB ones, through
//! the same walk (`psd_import::parse` → `GroupRecords`, CMYK-aware since this
//! phase: channel id 3 is black, adjustment bodies decode through
//! `psd_adjustment::decode_cmyk`) and the same typed tree build
//! (`psd_import16::emit_typed_tree`). What a record becomes:
//!
//! * a raster → a CMYK raster (`raster_cmyk`): its ink (`max − stored`, an
//!   8-bit code ×257) and transparency (`-1`; none = opaque,
//!   `LayerPixels::decode_cmyk`), its exact rectangle and signed offset; a
//!   pixel-less one that is not omitted → a canvas-sized transparent raster;
//! * a carried Pattern Fill record (`PtFl`, in any of its
//!   forms) → a CMYK pattern fill layer: its canvas raster at the origin
//!   rendered in the file's ink (`psd_fill_layer::render_cmyk`, the
//!   Pattern Overlay's texel leg) with the RGB fill description, its `-2`
//!   mask, style and locks as a raster's; its tile comes from the same
//!   `Patt`/`Pat2` table;
//! * a carried Solid Color Fill record (`SoCo`, any form) → a CMYK
//!   solid color fill layer the same way: its `CMYC` ink as the version-2
//!   ink description, rendered as one ink `fill` component;
//! * a carried Gradient Fill record (`GdFl`, any form) → a
//!   CMYK gradient fill layer the same way: its stops' `CMYC` inks as the
//!   version-2 `stop_inks` description (`psd_gradient_fill`), rendered as one
//!   ink `gradient` component;
//! * a carried adjustment record — Levels, Curves, Hue/Saturation and (the
//!   styles batch) Brightness/Contrast, Color Balance, Selective Color,
//!   Channel Mixer, Photo Filter, Invert, Posterize, Threshold and Gradient
//!   Map (`psd_adjustment::decode_cmyk`) → a CMYK adjustment record
//!   (`cmyk_adjustment`); any other adjustment or fill record is omitted and
//!   named (`CMYK_KIND`, or the RGB omitted sentence);
//! * a folder → a Normal (isolated) or Pass Through group with its own mode;
//! * on every record: name, opacity, visibility, blend mode (all 27 CMYK
//!   modes; a raster's `pass` key is Normal), clipping, the independent mask
//!   (8-bit samples ×257, `MaskRecord::attach_typed`), the label and the
//!   locks; and (the styles batch) the record's whole layer style — its
//!   effects with their `CMYC` inks, Fill (`iOpa`), knockout and Blend
//!   Interior (`psd_layer_effects`, `FxColours::Cmyk`) — on a raster, a
//!   carried adjustment record or a group of either kind
//!   (a Pass Through one composited isolated), every
//!   inked hex canonicalised under the file's profile
//!   (`cmyk_style::canonical_hex`), and its Blend If ranges on Gray, Cyan,
//!   Magenta, Yellow or Black (`style_blend_if::source_weight_ink`).
//!
//! A carried style is observed with the RGB import's sentence
//! ([`cmyk_report_lines`], `psd_import::report_style`); type, shape and
//! smart-object records import as their pixels with their marker sentences. The document
//! takes the file's CMYK profile (`cmyk_io::psd_policy`: a PSD's resource
//! 1039, a TIFF's ICC tag, else the working CMYK under the untagged
//! sentence) and its declared depth. Colour samplers, guides and saved alpha
//! or spot channels are not imported. A file that retains no record opens
//! its saved composite (`cmyk_io::open_cmyk_composite`), as the RGB opens do.

use std::sync::Arc;

use crate::blend::BlendMode;
use crate::cmyk_adjust_layer::CmykAdjustmentState;
use crate::cmyk_document::{CmykDepth, CmykPolicy};
use crate::cmyk_image::CmykImage;
use crate::job_control::JobContext;
use crate::metadata::Metadata;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, pixel_count};
use crate::psd_container::Container;
use crate::psd_group_records::GroupRecords;
use crate::psd_import16::{emit_typed_tree, layered_counts, typed_blend};
use crate::psd_layer_effects::StyleBody;
use crate::psd_layer_pixels::{peak_forecast, PeakInputs};

/// The canvas term of the layered open's plan: the host's display
/// preparation of the opened document (its view render and display bytes,
/// `cmyk_io::OPEN_BYTES_PER_PIXEL`'s ≈ 27 B/px) beside the stack scratch the
/// first render builds. Declared before measuring;
/// `scripts/cmyk-admission/measure.py`'s `open-psd-layers-16` holds the plan
/// to ⌈1.25 × its measured peak⌉, and a miss raises this term.
pub(crate) const LAYERED_OPEN_BYTES_PER_PIXEL: u64 = 27;

/// The saved-composite choice of a CMYK file, as the report notes it.
pub(crate) const STORED_COMPOSITE_NOTE: &str = "Opens the saved composite as one CMYK layer with its own ink numbers and embedded CMYK profile; editable layers are not imported.";

/// The CMYK import's general limits, stated once (the RGB report's
/// `GENERAL_LIMITATIONS` twin).
pub(crate) const CMYK_GENERAL_LIMITATIONS: &[&str] = &[
    "A CMYK file opens as a CMYK document with its own ink numbers, declared depth and embedded CMYK profile (an untagged file takes the working CMYK profile), whichever choice is made.",
    "Raster layers keep their ink and transparency, rectangle and signed offset; groups keep Normal or Pass Through, their own blend mode, opacity, visibility, mask and open state; every record keeps its name, opacity, visibility, all 27 CMYK blend modes, clipping, ordinary 20-byte mask, colour label, locks and Fill (fill opacity) where it can hold one. Groups deeper than 10 levels are dissolved.",
    "Levels, Curves (point curves), Hue/Saturation, Brightness/Contrast, Color Balance, Selective Color, Channel Mixer, Photo Filter, Invert, Posterize, Threshold and Gradient Map adjustment layers are carried as Doctored CMYK adjustment layers; a Pattern Fill layer (PtFl) is carried as a Doctored pattern fill layer with its pattern from the same Patt/Pat2 table, a Solid Color Fill layer (SoCo) as a Doctored solid color fill layer and a Gradient Fill layer (GdFl) as a Doctored gradient fill layer; every other adjustment or fill layer carries no pixels and is omitted, reported per record. A drawn (mapped) curve is omitted. The stored composite keeps their appearance.",
    "Layer effects (their colours in ink), knockout and Blend Interior Effects as Group are carried as a Doctored layer style on rasters, adjustment layers and every group (a styled Pass Through group composites isolated). Blend If ranges (Gray, Cyan, Magenta, Yellow and Black, one range per restricted channel) carry on rasters, adjustment layers and every group. Type, shape and smart-object layers import as their pixels only.",
    "Saved alpha and spot channels, colour samplers, guides and the ruler origin are not imported; a saved composite's merged transparency is not read.",
];

/// The layered open's plan, `(retained, scratch)`, before a single plane is
/// allocated: retained = 10 B a raster pixel (CMYKA16) and 2 B a mask
/// pixel; scratch = the shared decode forecast at five planes
/// (`peak_forecast`) and [`LAYERED_OPEN_BYTES_PER_PIXEL`] a canvas pixel,
/// plus, when a record carries a fill, one fill's render at a time: its
/// assembled canvas samples (10 B a pixel) and the largest pattern fill's
/// ink level (`pattern_tile::ink_scratch_bytes`).
pub(crate) fn layered_plan(
    container: &Container<'_>,
    records: &GroupRecords<'_>,
    source_bytes: usize,
) -> Result<(u64, u64), String> {
    let (pixels, masks, largest, encoded) = layered_counts(container, records)?;
    let canvas_pixels = pixel_count(container.width, container.height)? as u64;
    let retained = pixels
        .saturating_mul(crate::cmyk_image::CMYK_BYTES_PER_PIXEL)
        .saturating_add(masks.pixels.saturating_mul(2))
        .saturating_add(container.spot_retained_bytes(2));
    let scratch = peak_forecast(PeakInputs {
        source_bytes: source_bytes as u64,
        metadata_bytes: records.metadata_byte_count() as u64,
        pixels: pixels.saturating_add(records.groups.len() as u64),
        mask_pixels: masks.pixels,
        largest,
        encoded,
        canvas_pixels,
        sample_bytes: container.sample_bytes(),
        planes: 5,
    })
    .saturating_add(canvas_pixels.saturating_mul(LAYERED_OPEN_BYTES_PER_PIXEL))
    .saturating_add(fill_scratch(records, canvas_pixels));
    Ok((retained, scratch))
}

/// The fill term of [`layered_plan`]: 0 when no record carries a fill; else
/// the canvas's assembled CMYKA16 samples and the largest pattern fill's ink
/// level (a solid has none), since the fills render one at a time.
fn fill_scratch(records: &GroupRecords<'_>, canvas_pixels: u64) -> u64 {
    let mut fills = records.fills.iter().flatten().peekable();
    if fills.peek().is_none() {
        return 0;
    }
    let level = fills
        .filter_map(|fill| match &fill.kind {
            crate::psd_fill_layer::FillKind::Pattern { fill, .. } => {
                crate::artwork_render::pattern_fill(&serde_json::Value::Object(fill.clone())).ok()
            }
            _ => None,
        })
        .map(|fill| {
            crate::pattern_tile::ink_scratch_bytes(
                fill.pattern.tile.width,
                fill.pattern.tile.height,
                f64::from(fill.scale),
                f64::from(fill.angle),
            )
        })
        .max()
        .unwrap_or(0);
    canvas_pixels.saturating_mul(10).saturating_add(level)
}

/// A built record with its carried style attached (module doc), its inked
/// hex canonical under `policy`; every record kind holds one, a Pass Through
/// group included (it then composites isolated).
fn with_style(
    built: PrecisionDocumentLayer,
    body: &StyleBody,
    canvas: (u32, u32),
    policy: &CmykPolicy,
) -> Result<PrecisionDocumentLayer, String> {
    let StyleBody::Carried(carried) = body else {
        return Ok(built);
    };
    let style = crate::cmyk_style::canonical_hex(&carried.style, policy)?;
    built.with_layer_style(Some(&style), canvas)
}

/// Import Layers of a CMYK PSD, PSB or layered TIFF (module doc). The caller
/// has begun the job control; `limit` and `other` admit the plan.
pub(crate) fn open_cmyk_layers(
    bytes: &[u8],
    path: &str,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<PrecisionDocument, String> {
    let container = Container::read(bytes).map_err(|e| format!("PSD {path}: {e}"))?;
    container
        .cmyk_layered_admission()
        .map_err(|e| format!("PSD {path}: {e}"))?;
    let depth = if container.depth == 16 {
        CmykDepth::Sixteen
    } else {
        CmykDepth::Eight
    };
    let (policy, sentence) =
        crate::cmyk_io::psd_policy(&container, depth).map_err(|e| format!("PSD {path}: {e}"))?;
    let resolution = container
        .resolution()
        .map_err(|e| format!("PSD {path}: {e}"))?
        .unwrap_or_default();
    let (psd, records) = crate::psd_import::parse(bytes, &container)
        .map_err(|e| format!("PSD {path}: {e}; inspect the saved-composite option"))?;
    if records.retains_nothing() {
        // Nothing would be retained, so the layered choice IS the saved
        // composite, as in the RGB opens (a TIFF has none and refuses).
        return PrecisionDocument::open_cmyk_composite(bytes, path, &container, limit, other, job);
    }
    let (retained, scratch) = layered_plan(&container, &records, bytes.len())?;
    admit(retained, scratch, other, limit)?;
    job.reserve(scratch)?;
    let sample_bytes = container.sample_bytes() as usize;
    let canvas = (container.width, container.height);
    let roots = emit_typed_tree(
        &psd,
        &records,
        &mut |index, dissolved| {
            if records.omitted(index) {
                return Ok(None);
            }
            job.check()?;
            let layer = &psd.layers()[index];
            let blend = typed_blend(crate::psd::map_psd_blend(layer.blend_mode() as i32), false)
                .unwrap_or(BlendMode::Normal);
            let opacity = f32::from(layer.opacity()) / 255.0;
            // `PsdLayer::visible` actually returns the record's HIDDEN flag.
            let visible = !layer.visible();
            let built = if let Some(carried) = records.carried(index) {
                PrecisionDocumentLayer::cmyk_adjustment(
                    layer.name(),
                    CmykAdjustmentState::new(&carried.meta)?,
                    opacity,
                    visible,
                )?
            } else if let Some(fill) = records.fill(index) {
                // A carried Pattern or Solid Color Fill:
                // the canvas at the origin, whatever its record's rectangle,
                // as in RGB.
                // An aligned gradient's box, the plane at `attach_typed`'s
                // width (`sample_bytes`).
                let frame = crate::psd_fill_layer::align_frame(
                    fill,
                    records.masks[index],
                    canvas,
                    sample_bytes,
                )?;
                let image = crate::psd_fill_layer::render_cmyk(
                    fill,
                    records.layer_references[index],
                    frame,
                    canvas,
                    &policy,
                    job,
                )?;
                let mut built = PrecisionDocumentLayer::raster_cmyk(
                    layer.name(),
                    image,
                    (0, 0),
                    opacity,
                    visible,
                )?;
                built.description = Some(Arc::new(crate::psd_fill_layer::description(
                    fill,
                    canvas,
                    Some(&policy),
                )?));
                built
            } else {
                let raw = &records.layers[index];
                let (image, offset) = if raw.is_empty() {
                    (
                        CmykImage::filled(canvas.0, canvas.1, crate::cmyk_image::PAPER)?,
                        (0, 0),
                    )
                } else {
                    let (w, h) = raw.dimensions()?;
                    let samples = raw.decode_cmyk(sample_bytes)?;
                    (
                        CmykImage::from_contiguous(w, h, &samples, job)?,
                        (raw.bounds[1], raw.bounds[0]),
                    )
                };
                PrecisionDocumentLayer::raster_cmyk(layer.name(), image, offset, opacity, visible)?
            };
            let built = built
                .with_blend_mode(blend)?
                // The crate accessor is INVERTED (`psd.rs`): true means a
                // clipping BASE.
                .with_clipped(!layer.is_clipping_mask())
                .with_mask(records.masks[index].attach_typed(sample_bytes)?)
                .with_label(records.layer_labels[index])
                .with_reference(records.layer_references[index]);
            let built = with_style(built, &records.layer_styles[index], canvas, &policy)?;
            // `with_locks` is always LAST (after mask and style).
            Ok(Some(built.with_locks(
                records.layer_features[index].locks() | dissolved,
            )))
        },
        &mut |id, children| {
            let group = &records.groups[id as usize - 1];
            let name = psd.groups()[&id].name();
            let blend = typed_blend(group.blend, true);
            let built = if blend == Some(BlendMode::PassThrough) {
                PrecisionDocumentLayer::pass_through(name, children, group.opacity, group.visible)
            } else {
                PrecisionDocumentLayer::isolated_normal(
                    name,
                    children,
                    group.opacity,
                    group.visible,
                )
            }?
            .with_blend_mode(blend.unwrap_or(BlendMode::Normal))?
            .with_clipped(group.clipped)
            .with_open(group.open)
            .with_mask(group.mask.attach_typed(sample_bytes)?)
            .with_label(group.label)
            .with_reference(group.reference);
            Ok(with_style(
                built,
                &records.group_styles[id as usize - 1],
                canvas,
                &policy,
            )?
            .with_locks(group.features.locks()))
        },
    )
    .map_err(|reason| {
        format!("PSD {path}: {reason}; explicitly choose the stored composite if available")
    })?;
    if roots.is_empty() {
        return Err(format!("PSD {path}: no layer pixels could be imported; explicitly choose the stored composite if available"));
    }
    let mut document = PrecisionDocument::build_cmyk(
        container.width,
        container.height,
        Metadata::default(),
        resolution,
        roots,
        Vec::new(),
        Arc::new(policy),
    )?
    .with_import_warnings(sentence.into_iter().collect())?;
    // The file's 1037/1049 light becomes the document's own, so carried
    // effects keep `use_global_light` (the RGB opens' rule).
    if let Some(light) = container
        .global_light()
        .map_err(|e| format!("PSD {path}: {e}"))?
    {
        document.global_light = light;
    }
    let document = document.with_psd_spots(&container, limit, other)?;
    job.report(1.0, 5)?;
    Ok(document)
}

/// The record walk's CMYK lines (`psd_import::report`): every raster's,
/// carried adjustment record's and folder's style through
/// `psd_import::report_style`; every record kind keeps its style (a carried
/// adjustment record and a Pass Through group included).
pub(crate) fn cmyk_report_lines(
    psd: &psd::Psd,
    records: &GroupRecords<'_>,
    _container: &Container<'_>,
    losses: &mut Vec<String>,
    observations: &mut Vec<String>,
) {
    for (index, layer) in psd.layers().iter().enumerate() {
        // An omitted record is not imported at all.
        if records.omitted(index) {
            continue;
        }
        crate::psd_import::report_style(
            &records.layer_styles[index],
            &format!("Layer {index} (“{}”)", layer.name()),
            losses,
            observations,
        );
    }
    for index in 0..records.groups.len() {
        let id = index as u32 + 1;
        crate::psd_import::report_style(
            &records.group_styles[index],
            &format!("Group {id} (“{}”)", psd.groups()[&id].name()),
            losses,
            observations,
        );
    }
}

/// The file-level CMYK lines (`psd_import::report`): the CMYK open's own
/// observation, the untagged profile sentence, the merged transparency and
/// the saved channels the file carries, and its colour samplers. The merged
/// transparency is named only when the saved composite opens (a TIFF has
/// none here), as in the RGB branch.
pub(crate) fn cmyk_file_lines(
    container: &Container<'_>,
    policy: Option<&(CmykPolicy, Option<String>)>,
    composite_available: bool,
    losses: &mut Vec<String>,
    observations: &mut Vec<String>,
) {
    observations.push("This file opens as a CMYK document, keeping its own ink and embedded CMYK profile, with either choice.".into());
    if let Some((_, Some(sentence))) = policy {
        observations.push(sentence.clone());
    }
    if container.merged_alpha && container.channels > 4 && composite_available {
        observations.push(crate::cmyk_io::MERGED_ALPHA.into());
    }
    let spots = container.spot_records();
    let extra = spots.alpha;
    if extra > 0 {
        losses.push(format!("{extra} additional saved channels (spot or alpha) detected in the header; the CMYK composite does not retain them."));
    }
    losses.extend(spots.losses.iter().cloned());
    observations.extend(spots.observation());
    if let Ok(samplers) = container.color_samplers() {
        let n = samplers.kept.len();
        if n > 0 {
            losses.push(format!(
                "{n} colour samplers are not imported into a CMYK document."
            ));
        }
        // The decode's own refusals ("… not imported.") stay true here.
        losses.extend(
            samplers
                .losses
                .into_iter()
                .filter(|line| line.ends_with("not imported.")),
        );
    }
}
