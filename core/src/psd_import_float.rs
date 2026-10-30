//! Layered and stored-composite import of 32-bit RGB PSDs and PSBs as 32-bit
//! float documents, and layered import of a 32-bit float RGB layered
//! TIFF (either byte order: a little-endian one arrives as
//! `tiff_layers_order::big_endian_form`'s big-endian form; its flat image is
//! the ordinary float TIFF open's). Every framing, admission, mask and folder rule is
//! the 16-bit one (`psd_import16`): `Container::layer_info` finds Photoshop's
//! `Lr32` block, `Container::sample_bytes` (4) scales every length, row width
//! and budget, and the planes decode through the one seam
//! (`psd_layer_pixels::plane_bytes`, ZIP-with-prediction in Photoshop's
//! byte-plane form, `psd_zip`). This module decodes the admitted planes as
//! big-endian f32 and builds the float stack.
//!
//! Samples are linear light in the file's profile, straight alpha, exactly as
//! `psd_export_float` writes them; they pass ONCE through
//! `float_document::sanitize` (NaN → 0, ±∞ → ±65504, alpha into [0, 1]) with
//! `float_io`'s warnings. The profile must be an RGB matrix/TRC profile that
//! passes the precision check ([`profile_refusal`]); an untagged file is
//! sRGB, and the linear twin of a built-in opens under that built-in
//! ([`document_profile`]). Masks are f32 coverage stored as u16
//! (`MaskRecord::attach_typed(4)`).
//!
//! The float rules, asked by the report and the open alike: a blend key a
//! float document does not admit (`float_document::admitted_blend`) imports
//! as Normal with a loss; a carried style the typed model or the float mode
//! refuses (`typed_style_refusal`, `float_document::style_refusal`) is
//! discarded with a loss; a carried adjustment whose float form is refused
//! (`float_document::refused_operator`) is omitted with a loss; a carried
//! Solid Color fill (Photoshop's pixel-less 32-bit `SoCo`,
//! its colour read through the linear twin) is its canvas raster re-rendered
//! from its description (`psd_fill_layer::render_float`), with its mask,
//! label, reference and kept style, and its render scratch charged
//! (`GroupRecords::fill_scratch_bytes`); a carried Gradient or Pattern fill
//! refuses the layered choice ([`layered_refusal`]). The stored
//! composite is the white-matted `c·a + (1 − a)` of `psd_export_float`,
//! un-matted by [`composite32`].
//!
//! # Admission
//!
//! Layered: retained 16 B a raster pixel and 2 B a mask pixel; scratch the
//! shared decode forecast (`peak_forecast`) at 4 bytes a sample and four
//! planes. Composite: one canvas of 16 B a pixel and the same forecast for a
//! canvas plane set. Measured 2026-10-20 with
//! `core/examples/psd_float_admission.rs` (Release, one case per process,
//! `MallocLargeCache=0`, process limit 8 GiB, three runs; E = the
//! `phys_footprint` interval peak − the footprint before the open, the opened
//! document held): a 6000×4000 one-layer file the example's `save` wrote
//! (768 MB), max E 1,538,344,040 against the plan 4,579,152,804 (2.977 × E;
//! ⌈1.25 × max E⌉ = 1,922,930,050). It covers the margin, so unchanged; the
//! excess is the shared forecast's four copies of the source bytes
//! (`peak_forecast`), which the 8- and 16-bit opens and the export preflight
//! share.

use std::sync::Arc;

use crate::blend::BlendMode;
use crate::float_document::{admitted_blend, refused_operator, sanitize, ToneMap};
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::metadata::Metadata;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, PrecisionImage};
use crate::psd_container::Container;
use crate::psd_group_records::GroupRecords;
use crate::psd_import16::{
    adjustment_layer, emit_typed_tree, group_layer, layered_counts, style_plane, typed_blend,
    typed_style_refusal, with_carried_style,
};
use crate::psd_layer_effects::StyleBody;
use crate::psd_layer_pixels::{peak_forecast, plane_bytes, LayerPixels, PeakInputs};

/// Why this profile cannot be a float document's, or `None` (an untagged
/// file is sRGB): the `float_io` rule — an RGB matrix/TRC model whose curves
/// pass the precision check.
pub(crate) fn profile_refusal(profile: Option<&IccProfile>) -> Option<String> {
    let model = match profile?.model() {
        Some(model) => model,
        None => return Some("a 32-bit float document requires an RGB matrix/TRC profile".into()),
    };
    model.validate_precision().err()
}

/// One raster record's straight RGBA32 samples: every colour and alpha plane
/// as big-endian f32, hidden RGB kept, an absent alpha 1.0.
pub(crate) fn decode32(raw: &LayerPixels<'_>) -> Result<Vec<[f32; 4]>, String> {
    let (width, height) = raw.dimensions()?;
    let mut out = vec![[0.0f32, 0.0, 0.0, 1.0]; width as usize * height as usize];
    for channel in &raw.channels {
        if channel.kind < -1 {
            continue;
        }
        let component = if channel.kind == -1 {
            3
        } else {
            channel.kind as usize
        };
        let plane = plane_bytes(
            channel.bytes,
            width as usize * 4,
            height as usize,
            4,
            raw.large,
        )?;
        for (pixel, sample) in out.iter_mut().zip(plane.chunks_exact(4)) {
            pixel[component] = f32::from_be_bytes([sample[0], sample[1], sample[2], sample[3]]);
        }
    }
    Ok(out)
}

/// The stored composite of a 32-bit RGB file as straight RGBA32: its colour
/// planes and verified merged alpha through the shared plane walk, the white
/// matte removed as `(m − (1 − a)) / a`, a zero alpha's colour the canonical
/// 0, no merged alpha opaque. Not sanitised here: the open does that once. A
/// 32-bit Grayscale file (colour mode 1, #547 phase 3c) has one colour plane,
/// which fills R = G = B, and its merged alpha at plane 1.
pub(crate) fn composite32(container: &Container<'_>) -> Result<Vec<[f32; 4]>, String> {
    if container.kind == crate::psd_framing::LayeredContainer::Tiff {
        return Err(crate::psd_container::TIFF_COMPOSITE.into());
    }
    if let Some(reason) = container.support_refusal() {
        return Err(reason);
    }
    if container.depth != 32 {
        return Err("this PSD is not 32-bit".into());
    }
    let base: usize = if container.color == 1 { 1 } else { 3 };
    let alpha = container.merged_alpha && usize::from(container.channels) > base;
    let kept = base + usize::from(alpha);
    let pixels = container.width as usize * container.height as usize;
    let planes = container.composite_planes(kept, 4)?;
    if planes.len() != kept * pixels * 4 {
        return Err("PSD composite plane length mismatch".into());
    }
    let mut image = vec![[0.0f32, 0.0, 0.0, 1.0]; pixels];
    for (plane_index, plane) in planes.chunks_exact(pixels * 4).enumerate() {
        // Gray: plane 0 is every colour component, plane 1 the alpha.
        let components = match (base, plane_index) {
            (1, 0) => 0..3,
            (1, _) => 3..4,
            (_, c) => c..c + 1,
        };
        for (pixel, sample) in image.iter_mut().zip(plane.chunks_exact(4)) {
            let value = f32::from_be_bytes([sample[0], sample[1], sample[2], sample[3]]);
            pixel[components.clone()].fill(value);
        }
    }
    if alpha {
        for pixel in &mut image {
            let a = pixel[3];
            if a == 1.0 {
                continue;
            }
            if a <= 0.0 || a.is_nan() {
                pixel[..3].fill(0.0);
                continue;
            }
            let a = f64::from(a);
            for color in &mut pixel[..3] {
                *color = ((f64::from(*color) - (1.0 - a)) / a) as f32;
            }
        }
    }
    Ok(image)
}

/// Why the LAYERED choice refuses at 32 bits, or `None`: a carried Gradient
/// or Pattern fill layer, whose 32-bit record Photoshop has not shown (a
/// Solid Color fill is carried).
pub(crate) fn layered_refusal(psd: &psd::Psd, records: &GroupRecords<'_>) -> Option<String> {
    psd.layers().iter().enumerate().find_map(|(index, layer)| {
        let fill = records
            .fill(index)
            .filter(|fill| !matches!(fill.kind, crate::psd_fill_layer::FillKind::Solid { .. }))?;
        Some(format!(
            "Layer {index} (“{}”) is a Photoshop {} fill layer, which Doctored does not read at 32 bits; open the stored composite.",
            layer.name(),
            fill.kind_name().trim_end_matches(" Fill")
        ))
    })
}

/// The admission plan of a 32-bit stored-composite open, `(retained,
/// scratch)`: one canvas of 16 B a pixel and the shared decode forecast for
/// a canvas plane set at 4 bytes a sample and four planes (module doc's
/// measurement). A Gray file decodes at most two of those four planes.
pub(crate) fn composite_plan32(container: &Container<'_>, source_bytes: u64) -> (u64, u64) {
    let canvas_pixels = u64::from(container.width) * u64::from(container.height);
    (
        canvas_pixels.saturating_mul(16),
        peak_forecast(PeakInputs {
            source_bytes,
            metadata_bytes: 0,
            pixels: canvas_pixels,
            mask_pixels: 0,
            largest: canvas_pixels,
            encoded: 0,
            canvas_pixels,
            sample_bytes: 4,
            planes: 4,
        }),
    )
}

/// The admission plan of a 32-bit layered open, `(retained, scratch)`: 16 B
/// a raster pixel and 2 B a mask pixel retained, the shared decode forecast
/// at 4 bytes a sample and four planes (module doc's measurement), plus a
/// carried solid's render scratch in `profile` (`GroupRecords::
/// fill_scratch_bytes`, the 16-bit open's term; `None` where the open
/// carries no fill).
pub(crate) fn layered_plan32(
    container: &Container<'_>,
    records: &GroupRecords<'_>,
    source_bytes: u64,
    profile: Option<&IccProfile>,
) -> Result<(u64, u64), String> {
    let (pixels, masks, largest, encoded) = layered_counts(container, records)?;
    let canvas_pixels = u64::from(container.width) * u64::from(container.height);
    let retained = pixels
        .saturating_mul(16)
        .saturating_add(masks.pixels.saturating_mul(2));
    let scratch = peak_forecast(PeakInputs {
        source_bytes,
        metadata_bytes: records.metadata_byte_count() as u64,
        pixels: pixels.saturating_add(records.groups.len() as u64),
        mask_pixels: masks.pixels,
        largest,
        encoded,
        canvas_pixels,
        sample_bytes: 4,
        planes: 4,
    })
    .saturating_add(records.fill_scratch_bytes(profile));
    Ok((retained, scratch))
}

/// The float twin of `typed_style_refusal`: the typed model's answer, then
/// the float mode's own (`float_document::style_refusal`).
fn style_refusal(body: &StyleBody, plane: (u32, u32)) -> Option<String> {
    let StyleBody::Carried(carried) = body else {
        return None;
    };
    typed_style_refusal(&carried.style, plane)
        .or_else(|| crate::float_document::style_refusal(&carried.style))
}

/// The style body a built record takes: the carried one, or `absent` when
/// the float rules discard it.
fn kept_style<'a>(body: &'a StyleBody, plane: (u32, u32), absent: &'a StyleBody) -> &'a StyleBody {
    if style_refusal(body, plane).is_some() {
        absent
    } else {
        body
    }
}

/// The blend a raster or adjustment record imports with: its typed key when
/// the float mode admits it, else Normal.
fn float_blend(layer: &psd::PsdLayer) -> (BlendMode, BlendMode) {
    let blend = crate::psd::map_psd_blend(layer.blend_mode() as i32);
    let kept = typed_blend(blend, false)
        .filter(|mode| admitted_blend(*mode))
        .unwrap_or(BlendMode::Normal);
    (blend, kept)
}

/// The record's plane for a style's admission (`psd_import16::typed_losses`).
fn record_plane(records: &GroupRecords<'_>, index: usize, canvas: (u32, u32)) -> (u32, u32) {
    if records.carried(index).is_some() || records.fill(index).is_some() {
        canvas
    } else {
        style_plane(Some(&records.layers[index]), canvas)
    }
}

/// Why a carried adjustment record's float form is refused, or `None`: the
/// layer as it would be built, asked `float_document::refused_operator`.
pub(crate) fn adjustment_refusal(
    layer: &psd::PsdLayer,
    carried: &crate::psd_adjustment::Carried,
    profile: &Arc<IccProfile>,
) -> Option<String> {
    let built = PrecisionDocumentLayer::adjustment(
        layer.name(),
        &crate::psd_adjustment::typed_description(&carried.meta),
        1.0,
        true,
        profile,
    );
    match built {
        Ok(built) => refused_operator(&built),
        Err(reason) => Some(reason),
    }
}

/// The losses and observation a 32-bit import adds to the shared report
/// (module doc's float rules), in record order then group order.
pub(crate) fn float_report_lines(
    psd: &psd::Psd,
    records: &GroupRecords<'_>,
    profile: &Arc<IccProfile>,
    canvas: (u32, u32),
    losses: &mut Vec<String>,
    observations: &mut Vec<String>,
) {
    let file = profile;
    let profile = &document_profile(Arc::clone(file));
    for (index, layer) in psd.layers().iter().enumerate() {
        if records.omitted(index) {
            continue;
        }
        let target = format!("Layer {index} (“{}”)", layer.name());
        if let Some(carried) = records.carried(index) {
            if let Some(reason) = adjustment_refusal(layer, carried, profile) {
                losses.push(format!(
                    "{target} is a Photoshop {} adjustment layer that a 32-bit float document cannot carry ({reason}); layered import omits it. The stored composite keeps the saved appearance.",
                    carried.kind
                ));
                continue;
            }
        }
        if let Some(reason) = style_refusal(
            &records.layer_styles[index],
            record_plane(records, index, canvas),
        ) {
            losses.push(format!(
                "{target} has Photoshop layer effects that a 32-bit float document cannot carry ({reason}); they are discarded."
            ));
        }
        let (blend, kept) = float_blend(layer);
        if kept != blend {
            losses.push(format!(
                "{target} has the blend key {blend:?}, which a 32-bit float document cannot represent; it is imported as Normal."
            ));
        }
    }
    for (index, group) in records.groups.iter().enumerate() {
        let id = index as u32 + 1;
        let name = psd.groups()[&id].name();
        if let Some(reason) = style_refusal(&records.group_styles[index], canvas) {
            losses.push(format!(
                "Group {id} (“{name}”) has Photoshop layer effects that a 32-bit float document cannot carry ({reason}); they are discarded."
            ));
        }
        let typed = typed_blend(group.blend, true);
        if !typed.is_some_and(|mode| mode == BlendMode::PassThrough || admitted_blend(mode)) {
            losses.push(format!(
                "Group {id} (“{name}”) has the blend key {:?}, which a 32-bit float document cannot represent; it is imported as Normal.",
                group.blend
            ));
        }
    }
    observations.push(
        "This file opens as a 32-bit float document, including through Import Layers.".into(),
    );
    if !Arc::ptr_eq(file, profile) {
        let (file, builtin) = (file.name(), profile.name());
        observations.push(format!(
            "The embedded profile “{file}” is the linear (gamma 1.0) twin of {builtin} that a 32-bit PSD carries; the document takes {builtin}, and its float samples are unchanged."
        ));
    }
}

/// `file`, or the built-in it is the linear twin of
/// (`icc_builtin::linear_twin_source`). A 32-bit PSD embeds the gamma-1.0 twin
/// of its document's profile (`psd_export_float`), and a document under the
/// twin itself would tag and encode a later 8- or 16-bit export as linear
/// light; taking the built-in back is a relabel, since the twin and its source
/// describe one colour space and the float samples are linear in both. A twin
/// of any other profile cannot be traced to its source and stays as it is.
pub(crate) fn document_profile(file: Arc<IccProfile>) -> Arc<IccProfile> {
    match crate::icc_builtin::linear_twin_source(&file) {
        Some(which) => IccProfile::of_builtin(which),
        None => file,
    }
}

/// The repair counts the open discloses, `float_io`'s two sentences.
#[derive(Default)]
pub(crate) struct Repairs {
    non_finite: u64,
    clamped: u64,
}

impl Repairs {
    /// Sanitises `pixels` in place (`float_document::sanitize`) and counts.
    pub(crate) fn take(&mut self, pixels: &mut [[f32; 4]]) {
        let (non_finite, clamped) = sanitize(pixels.as_flattened_mut(), 4);
        self.non_finite += non_finite;
        self.clamped += clamped;
    }

    pub(crate) fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.non_finite > 0 {
            out.push(format!("{} non-finite samples replaced", self.non_finite));
        }
        if self.clamped > 0 {
            out.push(format!("{} out-of-range samples clamped", self.clamped));
        }
        out
    }
}

/// 0 imports the supported layer subset; 1 opens only the saved composite. A
/// layered decode error is never replaced with a composite implicitly. The
/// job control, when present, is begun ONCE here, as `open16` does.
pub(crate) fn open32(
    bytes: &[u8],
    path: &str,
    mode: i32,
    limit: u64,
    other: u64,
    control: Option<&DrJobControl>,
) -> Result<PrecisionDocument, String> {
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    if !matches!(mode, 0 | 1) {
        return Err("unknown PSD import mode; use 0 (layers) or 1 (stored composite)".into());
    }
    let container = Container::read(bytes).map_err(|e| format!("PSD {path}: {e}"))?;
    if let Some(reason) = container.support_refusal() {
        return Err(format!("PSD {path}: {reason}"));
    }
    if container.depth != 32 {
        return Err(format!(
            "PSD {path}: this is an {}-bit file, not a 32-bit one",
            container.depth
        ));
    }
    // ONE profile handle for the document and every plane it will hold.
    let profile = container
        .profile()
        .map_err(|e| format!("PSD {path}: {e}"))?;
    if let Some(reason) = profile_refusal(profile.as_deref()) {
        return Err(format!("PSD {path}: {reason}"));
    }
    let profile = document_profile(profile.unwrap_or_else(IccProfile::srgb));
    let resolution = container
        .resolution()
        .map_err(|e| format!("PSD {path}: {e}"))?
        .unwrap_or_default();
    let (cw, ch) = (container.width, container.height);
    let light = container.global_light()?.unwrap_or_default();
    let color_samplers = container.color_samplers()?.kept;
    let finish = |document: PrecisionDocument, repairs: &Repairs| {
        let mut document = document.with_import_warnings(repairs.warnings())?;
        document.global_light = light;
        document.color_samplers.clone_from(&color_samplers);
        Ok::<_, String>(document)
    };

    let composite = |job: JobContext<'_>| -> Result<PrecisionDocument, String> {
        job.check()?;
        let (retained, scratch) = composite_plan32(&container, bytes.len() as u64);
        admit(retained, scratch, other, limit)?;
        let mut pixels = composite32(&container).map_err(|e| format!("PSD {path}: {e}"))?;
        let mut repairs = Repairs::default();
        repairs.take(&mut pixels);
        let image =
            PrecisionImage::from_rgba32_extended_decoded(cw, ch, pixels, profile.clone(), None)?;
        let layer = PrecisionDocumentLayer::raster_float("Background", image, (0, 0), 1.0, true)?;
        let document = PrecisionDocument::from_parts_float(
            cw,
            ch,
            Arc::clone(&profile),
            Metadata::default(),
            resolution,
            vec![layer],
            Vec::new(),
            ToneMap::Clip,
        )?;
        finish(document, &repairs)
    };
    if mode == 1 {
        let document = composite(job)?;
        job.report(1.0, 5)?;
        return Ok(document);
    }

    let (psd, records) = crate::psd_import::parse(bytes, &container)
        .map_err(|e| format!("PSD {path}: {e}; inspect the saved-composite option"))?;
    if let Some(reason) = layered_refusal(&psd, &records) {
        return Err(format!("PSD {path}: {reason}"));
    }
    let (retained, scratch) =
        layered_plan32(&container, &records, bytes.len() as u64, Some(&*profile))?;
    admit(retained, scratch, other, limit)?;
    if records.retains_nothing() {
        let document = composite(job)?;
        job.report(1.0, 5)?;
        return Ok(document);
    }

    let canvas = (cw, ch);
    let mut repairs = Repairs::default();
    let absent = StyleBody::Absent;
    let roots = emit_typed_tree(
        &psd,
        &records,
        &mut |index, dissolved| {
            if records.omitted(index) {
                return Ok(None);
            }
            job.check()?;
            let layer = &psd.layers()[index];
            let style = kept_style(
                &records.layer_styles[index],
                record_plane(&records, index, canvas),
                &absent,
            );
            let (_, blend) = float_blend(layer);
            let built = if let Some(carried) = records.carried(index) {
                if adjustment_refusal(layer, carried, &profile).is_some() {
                    return Ok(None);
                }
                adjustment_layer(
                    layer,
                    carried,
                    records.masks[index],
                    4,
                    &profile,
                    blend,
                    style,
                    canvas,
                    records.layer_labels[index],
                    records.layer_references[index],
                )?
            } else if let Some(fill) = records.fill(index) {
                // A carried Solid Color fill (module doc): its canvas raster
                // at the origin, re-rendered from its description.
                let image = crate::psd_fill_layer::render_float(
                    fill,
                    canvas,
                    profile.clone(),
                    JobContext::NONE,
                )?;
                let mut built = PrecisionDocumentLayer::raster_float(
                    layer.name(),
                    image,
                    (0, 0),
                    f32::from(layer.opacity()) / 255.0,
                    // `PsdLayer::visible` actually returns the HIDDEN flag.
                    !layer.visible(),
                )?
                .with_blend_mode(blend)?
                // The crate accessor is INVERTED (`psd.rs`): true = a base.
                .with_clipped(!layer.is_clipping_mask())
                .with_mask(records.masks[index].attach_typed(4)?)
                .with_label(records.layer_labels[index])
                .with_reference(records.layer_references[index]);
                built.description = Some(Arc::new(crate::psd_fill_layer::description(
                    fill, canvas, None,
                )?));
                with_carried_style(built, style, canvas)?
            } else {
                let raw = &records.layers[index];
                // A pixel-less record that is not omitted is an empty
                // canvas-sized transparent raster at the origin.
                let ((w, h), offset, mut samples) = if raw.is_empty() {
                    (canvas, (0, 0), vec![[0.0f32; 4]; cw as usize * ch as usize])
                } else {
                    (
                        raw.dimensions()?,
                        (raw.bounds[1], raw.bounds[0]),
                        decode32(raw)?,
                    )
                };
                repairs.take(&mut samples);
                let image = PrecisionImage::from_rgba32_extended_decoded(
                    w,
                    h,
                    samples,
                    profile.clone(),
                    None,
                )?;
                let built = PrecisionDocumentLayer::raster_float(
                    layer.name(),
                    image,
                    offset,
                    f32::from(layer.opacity()) / 255.0,
                    // `PsdLayer::visible` actually returns the HIDDEN flag.
                    !layer.visible(),
                )?
                .with_blend_mode(blend)?
                // The crate accessor is INVERTED (`psd.rs`): true = a base.
                .with_clipped(!layer.is_clipping_mask())
                .with_mask(records.masks[index].attach_typed(4)?)
                .with_label(records.layer_labels[index])
                .with_reference(records.layer_references[index]);
                with_carried_style(built, style, canvas)?
            };
            // `with_locks` is always LAST (after mask and style).
            Ok(Some(built.with_locks(
                records.layer_features[index].locks() | dissolved,
            )))
        },
        &mut |id, children| {
            let group = &records.groups[id as usize - 1];
            let built = group_layer(
                psd.groups()[&id].name(),
                group,
                children,
                4,
                kept_style(&records.group_styles[id as usize - 1], canvas, &absent),
                canvas,
            )?;
            // A key the float mode refuses imports as Normal (module doc).
            match typed_blend(group.blend, true) {
                Some(mode) if mode != BlendMode::PassThrough && !admitted_blend(mode) => {
                    built.with_blend_mode(BlendMode::Normal)
                }
                _ => Ok(built),
            }
        },
    )
    .map_err(|reason| {
        format!("PSD {path}: {reason}; explicitly choose the stored composite if available")
    })?;
    if roots.is_empty() {
        return Err(format!("PSD {path}: no layer pixels could be imported; explicitly choose the stored composite if available"));
    }
    let document = PrecisionDocument::from_parts_float(
        cw,
        ch,
        Arc::clone(&profile),
        Metadata::default(),
        resolution,
        roots,
        Vec::new(),
        ToneMap::Clip,
    )?;
    let document = finish(document, &repairs)?;
    job.report(1.0, 5)?;
    Ok(document)
}
