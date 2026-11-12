//! Layered and stored-composite import of 16-bit RGB PSDs as typed
//! `PrecisionDocument`s. Every framing, admission, mask and folder rule is the
//! 10A/10B one: this module only decodes the admitted planes at their real
//! depth and builds the typed nested stack, which is what a 16-bit file must
//! open into — the byte document has no place to put these samples.
//!
//! `psd_import16.rs` opens genuine 16-bit RGB files as typed `PrecisionDocument`s through the SAME
//! framing, admission, mask and folder rules: `Container::sample_bytes` scales every length, row
//! width and budget; `composite16`/`decode16`/`attach_typed` are the depth twins of the byte paths. The
//! typed open begins its job control ONCE and passes `None` to every
//! `PrecisionImage::from_rgba16_decoded`.

use std::sync::Arc;

use crate::blend::BlendMode;
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::metadata::Metadata;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, PrecisionImage};
use crate::psd::{Node, PsdTree};
use crate::psd_container::Container;
use crate::psd_group_records::GroupRecords;
use crate::psd_layer_effects::StyleBody;
use crate::psd_layer_pixels::{peak_forecast, PeakInputs};
use crate::psd_raster_masks::MaskBudget;
use crate::style::LayerStyle;
use crate::style_cache::RenderCache;

/// The typed equivalent of one PSD blend key, or `None` when the native typed
/// model has no representation for it (`precision_layer_blend`'s frozen set).
/// Pass Through exists only on a folder.
pub(crate) fn typed_blend(mode: BlendMode, group: bool) -> Option<BlendMode> {
    match mode {
        BlendMode::PassThrough => group.then_some(BlendMode::PassThrough),
        mode if crate::precision_layer_blend::COLOR_MODES.contains(&mode) => Some(mode),
        _ => None,
    }
}

/// Why the 16-bit model cannot carry this style, or `None`. The typed
/// document keeps Blend If on the LAYER and the effects/fill in the style, so
/// the one carried `LayerStyle` splits here before the two native admission
/// rules see it — and the report and the open ask the same question, so they
/// can never disagree.
pub(crate) fn typed_style_refusal(style: &LayerStyle, plane: (u32, u32)) -> Option<String> {
    let effects = LayerStyle {
        fill_opacity: style.fill_opacity,
        blend_if: None,
        // Both keys ride the typed style since 12E-3b, and the importer
        // carries them since 12E-3c, so they travel with the effects half.
        knockout: style.knockout,
        blend_interior_as_group: style.blend_interior_as_group,
        effects: style.effects.clone(),
        cache: RenderCache::default(),
    };
    // 12E-3b decision (iv): a style is STORED when it changes a pixel —
    // fill below 1, an enabled effect, or a knockout. `blend_interior_as_group`
    // alone does nothing without an interior effect, so it never stores one.
    let effects = (effects.fill_opacity < 1.0
        || effects.has_enabled_effects()
        || effects.knockout != crate::style::Knockout::None)
        .then_some(effects);
    crate::precision_style::validate(effects.as_ref(), plane)
        .err()
        .or_else(|| crate::precision_blend_if::validate(style.blend_if, false).err())
}

/// The plane `PrecisionDocumentLayer::style_plane` will report for one
/// imported record: a raster's own rectangle, the canvas for an empty record
/// and for a folder.
pub(crate) fn style_plane(
    raw: Option<&crate::psd_layer_pixels::LayerPixels<'_>>,
    canvas: (u32, u32),
) -> (u32, u32) {
    match raw {
        Some(raw) if !raw.is_empty() => raw.dimensions().unwrap_or(canvas),
        _ => canvas,
    }
}

/// One built typed layer with its carried style attached, or untouched when
/// the 16-bit model refuses it — exactly the records `typed_losses` names.
pub(crate) fn with_carried_style(
    built: PrecisionDocumentLayer,
    body: &StyleBody,
    canvas: (u32, u32),
) -> Result<PrecisionDocumentLayer, String> {
    let StyleBody::Carried(carried) = body else {
        return Ok(built);
    };
    let style = carried.style.clone();
    let plane = built.style_plane(canvas);
    if typed_style_refusal(&style, plane).is_some() {
        return Ok(built);
    }
    built.with_layer_style(Some(&style), canvas)
}

/// The losses a 16-bit layered import adds to the shared report: an
/// unrepresentable blend key, and a carried layer style the typed model
/// cannot hold. Clipping is retained on rasters and folders at both depths.
pub(crate) fn typed_losses(
    psd: &psd::Psd,
    records: &GroupRecords<'_>,
    container: &Container<'_>,
    losses: &mut Vec<String>,
    observations: &mut Vec<String>,
) {
    let canvas = (container.width, container.height);
    let check = |body: &StyleBody, target: String, plane: (u32, u32), losses: &mut Vec<String>| {
        let StyleBody::Carried(carried) = body else {
            return;
        };
        if let Some(reason) = typed_style_refusal(&carried.style, plane) {
            losses.push(format!(
                "{target} has Photoshop layer effects that a 16-bit document cannot carry ({reason}); they are discarded."
            ));
        }
    };
    for (index, layer) in psd.layers().iter().enumerate() {
        if records.omitted(index) {
            continue;
        }
        // A carried adjustment is canvas-sized and carries a style, and so is
        // a carried pattern fill.
        let plane = if records.carried(index).is_some() || records.fill(index).is_some() {
            canvas
        } else {
            style_plane(Some(&records.layers[index]), canvas)
        };
        check(
            &records.layer_styles[index],
            format!("Layer {index} (\u{201c}{}\u{201d})", layer.name()),
            plane,
            losses,
        );
    }
    // A group of either kind carries one (a Pass Through one composited
    // isolated).
    for index in 0..records.groups.len() {
        let id = index as u32 + 1;
        check(
            &records.group_styles[index],
            format!("Group {id} (\u{201c}{}\u{201d})", psd.groups()[&id].name()),
            canvas,
            losses,
        );
    }
    for (index, layer) in psd.layers().iter().enumerate() {
        let name = layer.name();
        // An omitted adjustment/fill record is not imported at all, so it has
        // no clipping or blend key left to lose.
        if records.omitted(index) {
            continue;
        }
        let blend = crate::psd::map_psd_blend(layer.blend_mode() as i32);
        if typed_blend(blend, false).is_none() {
            losses.push(format!(
                "Layer {index} (“{name}”) has the blend key {blend:?}, which a 16-bit document cannot represent; it is imported as Normal."
            ));
        }
    }
    for (index, group) in records.groups.iter().enumerate() {
        let id = index as u32 + 1;
        let name = psd.groups()[&id].name();
        if typed_blend(group.blend, true).is_none() {
            losses.push(format!(
                "Group {id} (“{name}”) has the blend key {:?}, which a 16-bit document cannot represent; it is imported as Normal.",
                group.blend
            ));
        }
    }
    observations
        .push("This file opens as a 16-bit document, including through Import Layers.".into());
}

/// What a typed layered import of these records retains and decodes, counted
/// before a single plane is allocated: `(raster pixels, mask budget, largest
/// plane, largest encoded channel)`. A pixel-less record retains nothing when
/// omitted, and a canvas raster (`GroupRecords::canvas_raster`) is the
/// canvas — the byte walk's arithmetic.
/// Shared by the 16-bit RGB budget here and the CMYK plan
/// (`psd_import_cmyk::layered_plan`).
pub(crate) fn layered_counts(
    container: &Container<'_>,
    records: &GroupRecords<'_>,
) -> Result<(u64, MaskBudget, u64, u64), String> {
    let mut masks = MaskBudget::default();
    for mask in records
        .masks
        .iter()
        .chain(records.groups.iter().map(|g| &g.mask))
    {
        masks.add(*mask)?;
    }
    let canvas_pixels = u64::from(container.width) * u64::from(container.height);
    let (mut pixels, mut largest, mut encoded) = (0u64, masks.largest, masks.encoded);
    for (index, layer) in records.layers.iter().enumerate() {
        // Exactly the byte walk's arithmetic: an omitted record retains
        // nothing, a canvas raster (`GroupRecords::canvas_raster`: a retained
        // pixel-less record, a carried fill) is the canvas.
        if records.omitted(index) {
            continue;
        }
        if records.canvas_raster(index) {
            pixels = pixels.saturating_add(canvas_pixels);
            largest = largest.max(canvas_pixels);
            continue;
        }
        let (w, h) = layer.dimensions()?;
        let count = u64::from(w) * u64::from(h);
        pixels = pixels.saturating_add(count);
        largest = largest.max(count);
        encoded = encoded.max(
            layer
                .channels
                .iter()
                .map(|c| c.bytes.len() as u64)
                .max()
                .unwrap_or(0),
        );
    }
    Ok((pixels, masks, largest, encoded))
}

/// The peak decode forecast and retained size of this file's layered import at
/// 16 bits, evaluated before a single plane is allocated. The forecast is the
/// SAME `peak_forecast` the byte importer and the export preflight share,
/// plus a carried solid's render scratch in `profile`, the document's one
/// handle (`GroupRecords::fill_scratch_bytes`). The gray open
/// (`psd_import_gray`, #1 phase 3a) charges the same plan with its twin.
pub(crate) fn budget(
    container: &Container<'_>,
    records: &GroupRecords<'_>,
    source_bytes: usize,
    profile: &IccProfile,
) -> Result<(u64, u64), String> {
    let (pixels, masks, largest, encoded) = layered_counts(container, records)?;
    let canvas_pixels = u64::from(container.width) * u64::from(container.height);
    // The kept spot planes are retained too (`with_psd_spots`), counted
    // here as CMYK's `layered_plan` counts them.
    let retained = pixels
        .saturating_mul(8)
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
        sample_bytes: 2,
        planes: 4,
    })
    .saturating_add(records.fill_scratch_bytes(Some(profile)));
    Ok((retained, scratch))
}

/// How a record's planes become RGBA16 samples (`raster_layer`).
pub(crate) type SampleDecoder<'a> =
    dyn Fn(&crate::psd_layer_pixels::LayerPixels<'_>) -> Result<Vec<[u16; 4]>, String> + 'a;

/// One imported raster layer: its exact 16-bit rectangle, signed offset,
/// opacity, visibility, clipping, representable blend key and independent mask.
/// A carried Pattern, Solid Color or Gradient Fill (`fill`) is its rendered
/// canvas raster at the origin, in the document's profile, with its fill
/// description (`psd_fill_layer`). `samples` decodes the record's planes
/// (`LayerPixels::decode16`, or a gray file's `decode_gray`, #1 phase 3a)
/// and `mask_bytes` is the container's sample width its mask is read at.
#[allow(clippy::too_many_arguments)]
pub(crate) fn raster_layer(
    layer: &psd::PsdLayer,
    raw: &crate::psd_layer_pixels::LayerPixels<'_>,
    mask: crate::psd_raster_masks::MaskRecord<'_>,
    profile: &Arc<IccProfile>,
    canvas: (u32, u32),
    style: &StyleBody,
    label: u8,
    reference: (i32, i32),
    fill: Option<&crate::psd_fill_layer::CarriedFill>,
    samples: &SampleDecoder<'_>,
    mask_bytes: usize,
) -> Result<PrecisionDocumentLayer, String> {
    // A pixel-less record that is not omitted becomes an empty canvas-sized
    // transparent raster at the origin, exactly as in the byte walk.
    let (w, h) = if raw.is_empty() || fill.is_some() {
        canvas
    } else {
        raw.dimensions()?
    };
    let image = match fill {
        Some(fill) => crate::psd_fill_layer::render(
            fill,
            reference,
            // An aligned gradient's box, the plane at the mask's width.
            crate::psd_fill_layer::align_frame(fill, mask, canvas, mask_bytes)?,
            canvas,
            profile.clone(),
            JobContext::NONE,
        )?,
        None => {
            let samples = if raw.is_empty() {
                vec![[0u16; 4]; w as usize * h as usize]
            } else {
                samples(raw)?
            };
            PrecisionImage::from_rgba16_decoded(w, h, samples, profile.clone(), None)?
        }
    };
    let blend = crate::psd::map_psd_blend(layer.blend_mode() as i32);
    // A carried fill is the canvas at the origin whatever its record's
    // rectangle (Photoshop's form has the mask's, `psd_fill_layer`).
    let offset = if raw.is_empty() || fill.is_some() {
        (0, 0)
    } else {
        (raw.bounds[1], raw.bounds[0])
    };
    let built = PrecisionDocumentLayer::raster(
        layer.name(),
        image,
        offset,
        f32::from(layer.opacity()) / 255.0,
        // `PsdLayer::visible` actually returns the record's HIDDEN flag.
        !layer.visible(),
    )?
    .with_blend_mode(typed_blend(blend, false).unwrap_or(BlendMode::Normal))?
    // The crate accessor is INVERTED (`psd.rs`): true means a clipping BASE.
    .with_clipped(!layer.is_clipping_mask());
    let mut built = built;
    if let Some(fill) = fill {
        built.description = Some(Arc::new(crate::psd_fill_layer::description(
            fill, canvas, None,
        )?));
    }
    with_carried_style(
        built
            .with_mask(mask.attach_typed(mask_bytes)?)
            .with_label(label)
            .with_reference(reference),
        style,
        canvas,
    )
}

/// One carried adjustment record as a typed adjustment layer over the typed
/// form of its meta, with its opacity, visibility, clipping, blend key,
/// independent mask and carried style. A typed adjustment carries the
/// same modes a raster does (native v37 / LBLD schema7), so the key takes the
/// raster `typed_blend` path. `mask_bytes` is the container's sample width
/// (2, or 4 for the 32-bit float open, `psd_import_float`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn adjustment_layer(
    layer: &psd::PsdLayer,
    carried: &crate::psd_adjustment::Carried,
    mask: crate::psd_raster_masks::MaskRecord<'_>,
    mask_bytes: usize,
    profile: &Arc<IccProfile>,
    blend: BlendMode,
    style: &StyleBody,
    canvas: (u32, u32),
    label: u8,
    reference: (i32, i32),
) -> Result<PrecisionDocumentLayer, String> {
    let built = PrecisionDocumentLayer::adjustment(
        layer.name(),
        &crate::psd_adjustment::typed_description(&carried.meta),
        f32::from(layer.opacity()) / 255.0,
        // `PsdLayer::visible` actually returns the record's HIDDEN flag.
        !layer.visible(),
        profile,
    )?
    .with_blend_mode(blend)?
    // The crate accessor is INVERTED (`psd.rs`): true means a clipping BASE.
    .with_clipped(!layer.is_clipping_mask());
    with_carried_style(
        built
            .with_mask(mask.attach_typed(mask_bytes)?)
            .with_label(label)
            .with_reference(reference),
        style,
        canvas,
    )
}

/// One imported folder: a pass-through or isolated typed group carrying the
/// admitted folder opacity, visibility, clipping, blend key and independent
/// mask, its plane `mask_bytes` wide (2, or 4 for `psd_import_float`).
pub(crate) fn group_layer(
    name: &str,
    group: &crate::psd_group_records::Group<'_>,
    children: Vec<PrecisionDocumentLayer>,
    mask_bytes: usize,
    style: &StyleBody,
    canvas: (u32, u32),
) -> Result<PrecisionDocumentLayer, String> {
    let blend = typed_blend(group.blend, true);
    let built = if blend == Some(BlendMode::PassThrough) {
        PrecisionDocumentLayer::pass_through(name, children, group.opacity, group.visible)
    } else {
        PrecisionDocumentLayer::isolated_normal(name, children, group.opacity, group.visible)
    }?
    .with_blend_mode(blend.unwrap_or(BlendMode::Normal))?
    .with_clipped(group.clipped)
    .with_open(group.open);
    with_carried_style(
        built
            .with_mask(group.mask.attach_typed(mask_bytes)?)
            .with_label(group.label)
            .with_reference(group.reference),
        style,
        canvas,
    )
    .map(|layer| layer.with_locks(group.features.locks()))
}

/// The ONE typed tree build both typed layered opens share (16-bit RGB here,
/// CMYK in `psd_import_cmyk`): `PsdTree::emit` is postorder with a depth, so
/// one accumulator per level rebuilds the nested stack — a folder takes
/// whatever accumulated one level below it. Groups dissolved past the depth
/// cap emit no folder row, and their contents stay at the cap as siblings.
/// `leaf` builds raster `index` given the OR of its dissolved folders' lock
/// bits (`None` = omitted); `group` builds folder `id` around its children.
/// The first `Err` stops the walk and is the answer; the roots come back
/// bottom-first.
pub(crate) fn emit_typed_tree(
    psd: &psd::Psd,
    records: &GroupRecords<'_>,
    leaf: &mut dyn FnMut(usize, u32) -> Result<Option<PrecisionDocumentLayer>, String>,
    group: &mut dyn FnMut(
        u32,
        Vec<PrecisionDocumentLayer>,
    ) -> Result<PrecisionDocumentLayer, String>,
) -> Result<Vec<PrecisionDocumentLayer>, String> {
    let tree = PsdTree::build(psd);
    let mut levels: Vec<Vec<PrecisionDocumentLayer>> = Vec::new();
    let mut failure: Option<String> = None;
    let group_locks = |id: u32| records.groups[id as usize - 1].features.locks();
    tree.emit(None, 0, 0, &group_locks, &mut |node, depth, dissolved| {
        if failure.is_some() {
            return;
        }
        let depth = depth as usize;
        while levels.len() <= depth + 1 {
            levels.push(Vec::new());
        }
        let built = match node {
            Node::Layer(index) => match leaf(index, dissolved) {
                Ok(Some(layer)) => Ok(layer),
                Ok(None) => return,
                Err(reason) => Err(reason),
            },
            Node::Group(id) => {
                let children = std::mem::take(&mut levels[depth + 1]);
                group(id, children)
            }
        };
        match built {
            Ok(layer) => levels[depth].push(layer),
            Err(reason) => failure = Some(reason),
        }
    });
    if let Some(reason) = failure {
        return Err(reason);
    }
    Ok(levels.into_iter().next().unwrap_or_default())
}

/// The stored composite's plan, `(retained, scratch)`, admitted on its own
/// budget before a single sample is decoded: one retained canvas of RGBA16
/// plus the decode forecast for a canvas-sized 16-bit plane set. Shared by
/// [`open16`] and the gray open (`psd_import_gray`, #1 phase 3a), whose
/// document stores RGBA16 at both depths.
pub(crate) fn composite_plan(width: u32, height: u32, source_bytes: usize) -> (u64, u64) {
    let canvas_pixels = u64::from(width) * u64::from(height);
    (
        canvas_pixels.saturating_mul(8),
        peak_forecast(PeakInputs {
            source_bytes: source_bytes as u64,
            metadata_bytes: 0,
            pixels: canvas_pixels,
            mask_pixels: 0,
            largest: canvas_pixels,
            encoded: 0,
            canvas_pixels,
            sample_bytes: 2,
            planes: 4,
        }),
    )
}

/// 0 imports the supported layer subset; 1 opens only the saved composite. A
/// layered decode error is never replaced with a composite implicitly.
///
/// The job control, when present, is begun ONCE here for the whole open and
/// polled between layer planes; every constructor below therefore receives
/// `None`, because `PrecisionImage::from_rgba16` would begin a control of its
/// own and a single-use token cannot be begun twice.
pub(crate) fn open16(
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
    if container.color == 1 {
        // A gray file opens as a Grayscale document (#1 phase 3a).
        return Err(format!(
            "PSD {path}: this is a Grayscale file; open it as a Grayscale document"
        ));
    }
    if container.depth != 16 {
        return Err(format!(
            "PSD {path}: this is an {}-bit file; open it as an 8-bit document",
            container.depth
        ));
    }
    // ONE profile handle for the document and every plane it will hold:
    // `PrecisionDocument::from_parts` requires pointer equality.
    let profile = container
        .profile()
        .map_err(|e| format!("PSD {path}: {e}"))?
        .unwrap_or_else(IccProfile::srgb);
    let resolution = container
        .resolution()
        .map_err(|e| format!("PSD {path}: {e}"))?
        .unwrap_or_default();
    let (cw, ch) = (container.width, container.height);
    // The file's light becomes the document's own on both paths, as the byte
    // importer's `Container::tag` does; carried effects keep
    // `use_global_light` and read it.
    let light = container.global_light()?.unwrap_or_default();
    // …and so do the colour samplers (image resource 1073), on both paths.
    let color_samplers = container.color_samplers()?.kept;

    let composite = |job: JobContext<'_>| -> Result<PrecisionDocument, String> {
        job.check()?;
        let (retained, scratch) = composite_plan(cw, ch, bytes.len());
        let retained = retained.saturating_add(container.spot_retained_bytes(2));
        admit(retained, scratch, other, limit)?;
        let pixels = container
            .composite16()
            .map_err(|e| format!("PSD {path}: {e}"))?;
        let image = PrecisionImage::from_rgba16_decoded(cw, ch, pixels, profile.clone(), None)?;
        let mut document = PrecisionDocument::from_image(image, Metadata::default(), resolution)?;
        document.global_light = light;
        document.color_samplers.clone_from(&color_samplers);
        // Resources 1025 and 2000–2997 (`psd_paths`).
        document
            .paths
            .set_entries(crate::psd_paths::file_paths(&container).0);
        document.with_psd_spots(&container, limit, other)
    };
    if mode == 1 {
        let document = composite(job)?;
        job.report(1.0, 5)?;
        return Ok(document);
    }

    let (psd, records) = crate::psd_import::parse(bytes, &container)
        .map_err(|e| format!("PSD {path}: {e}; inspect the saved-composite option"))?;
    let (retained, scratch) = budget(&container, &records, bytes.len(), &profile)?;
    admit(retained, scratch, other, limit)?;
    if records.retains_nothing() {
        // Mirrors the byte importer: a file that would retain no record at all
        // — an empty layer section, or nothing but omitted adjustment/fill
        // records — opens as its saved composite, which is also what the
        // compatibility report substitutes for this choice.
        let document = composite(job)?;
        job.report(1.0, 5)?;
        return Ok(document);
    }

    // The record each built entry came from, in postorder (the order the
    // tree is emitted in), for its layer comp settings (`psd_comps`).
    let sources = std::cell::RefCell::new(Vec::new());
    let roots = emit_typed_tree(
        &psd,
        &records,
        &mut |index, dissolved| {
            // The omitted adjustment/fill records of the byte walk.
            if records.omitted(index) {
                return Ok(None);
            }
            sources
                .borrow_mut()
                .push(crate::psd_comps::CompSource::Layer(index));
            let built = if let Some(carried) = records.carried(index) {
                job.check().and_then(|()| {
                    adjustment_layer(
                        &psd.layers()[index],
                        carried,
                        records.masks[index],
                        2,
                        &profile,
                        typed_blend(
                            crate::psd::map_psd_blend(psd.layers()[index].blend_mode() as i32),
                            false,
                        )
                        .unwrap_or(BlendMode::Normal),
                        &records.layer_styles[index],
                        (cw, ch),
                        records.layer_labels[index],
                        records.layer_references[index],
                    )
                })
            } else {
                job.check().and_then(|()| {
                    raster_layer(
                        &psd.layers()[index],
                        &records.layers[index],
                        records.masks[index],
                        &profile,
                        (cw, ch),
                        &records.layer_styles[index],
                        records.layer_labels[index],
                        records.layer_references[index],
                        records.fill(index),
                        &|raw| raw.decode16(),
                        2,
                    )
                })
            };
            // `with_locks` is always LAST (after mask and style).
            built
                .map(|layer| layer.with_locks(records.layer_features[index].locks() | dissolved))
                .map(Some)
        },
        &mut |id, children| {
            sources
                .borrow_mut()
                .push(crate::psd_comps::CompSource::Group(id));
            group_layer(
                psd.groups()[&id].name(),
                &records.groups[id as usize - 1],
                children,
                2,
                &records.group_styles[id as usize - 1],
                (cw, ch),
            )
        },
    )
    .map_err(|reason| {
        format!("PSD {path}: {reason}; explicitly choose the stored composite if available")
    })?;
    if roots.is_empty() {
        return Err(format!("PSD {path}: no layer pixels could be imported; explicitly choose the stored composite if available"));
    }
    let mut document = PrecisionDocument::from_parts(
        cw,
        ch,
        Arc::clone(&profile),
        Metadata::default(),
        resolution,
        roots,
    )?;
    document.global_light = light;
    document.color_samplers = color_samplers;
    // Resources 1025 and 2000–2997 (`psd_paths`).
    document
        .paths
        .set_entries(crate::psd_paths::file_paths(&container).0);
    // FILE record 0, when kept, is root entry 0 (`psd_background`).
    if let Some(role) = crate::psd_background::import_role(&records) {
        document.mark_bottom(role);
    }
    // The vector masks the walk carried (`psd_paths`), each on the record
    // built from its file record.
    for (k, &source) in sources.borrow().iter().enumerate() {
        if let Some(body) = crate::psd_paths::source_vmsk(&records, source) {
            document = document.importing_vector_mask(k, body, limit, other)?;
        }
    }
    document.comps = crate::psd_comps::imported(&container, &records, &sources.borrow(), |k| {
        Some((document.layer_id(k)?, document.layer(k)?.name().to_string()))
    });
    let document = document.with_psd_spots(&container, limit, other)?;
    job.report(1.0, 5)?;
    Ok(document)
}
