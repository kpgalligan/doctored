//! 32-bit Grayscale PSD and PSB import (#1 phase 3c): a colour-mode-1 file
//! at 32 bits as a 32-bit Gray document, its layers (mode 0) or its stored
//! composite (mode 1). `psd_import_gray::open_gray_psd` reads the policy and
//! dispatches here.
//!
//! The structure is the 32-bit RGB open's (`psd_import_float::open32`) and the
//! rules are the gray open's (`psd_import_gray`, in their `float` form):
//!
//! * a raster → its one gray plane (big-endian f32, linear `Y`) into
//!   R = G = B and its transparency ([`decode_gray32`]; none = opaque),
//!   sanitised once (`psd_import_float::Repairs`), as a float raster with its
//!   rectangle and signed offset; a pixel-less one that is not omitted → a
//!   canvas-sized transparent raster;
//! * a carried adjustment the Gray table holds AND whose float form builds
//!   (`psd_import_gray::adjustment_refusal` with the twin) → a typed
//!   adjustment layer; every other, and every fill record, is omitted (the
//!   gray rule, not the float layered refusal of a fill);
//! * a folder → a Normal (isolated) or Pass Through group;
//! * on every record the gray open's name, opacity, visibility, clipping,
//!   mask (f32 coverage stored as u16, `MaskRecord::attach_typed(4)`), label,
//!   reference and locks, a blend mode the 32-bit Gray document offers
//!   (`psd_import_gray::gray_blend`, else Normal) and a style it can hold.
//!
//! The layered choice ends as the gray open's does (`psd_typed_exchange`:
//! the Background mark, the carried vector masks, the comps).
//!
//! A file that imports no record, and mode 1, open the stored composite
//! (`psd_import_float::composite32`, one plane into R = G = B, un-matted) as
//! one "Background". Every layer is in the policy's ONE twin handle; the
//! document is `from_parts_float` with `ToneMap::Clip`, then `with_gray`.
//!
//! Admission is the 32-bit RGB open's measured plans
//! (`psd_import_float::composite_plan32` / `layered_plan32`), which charge
//! four decoded planes where a gray record decodes at most two.

use std::sync::Arc;

use crate::blend::BlendMode;
use crate::float_document::ToneMap;
use crate::gray_document::GrayPolicy;
use crate::job_control::JobContext;
use crate::metadata::Metadata;
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, PrecisionImage};
use crate::psd_container::Container;
use crate::psd_import16::{
    adjustment_layer, emit_typed_tree, group_layer, typed_blend, with_carried_style,
};
use crate::psd_import_float::{composite32, composite_plan32, layered_plan32, Repairs};
use crate::psd_import_gray::{
    dropped, gray_blend, gray_body, gray_carried, imports_nothing, record_plane,
};
use crate::psd_layer_pixels::{plane_bytes, LayerPixels};

/// One gray raster record's straight samples as RGBA32 with R = G = B:
/// channel 0 is the gray plane, `-1` the transparency (big-endian f32 each),
/// and a record with none is opaque. Every other channel (a mask) is skipped.
pub(crate) fn decode_gray32(raw: &LayerPixels<'_>) -> Result<Vec<[f32; 4]>, String> {
    let (width, height) = raw.dimensions()?;
    let mut out = vec![[0.0f32, 0.0, 0.0, 1.0]; width as usize * height as usize];
    for channel in &raw.channels {
        if !matches!(channel.kind, -1 | 0) {
            continue;
        }
        let plane = plane_bytes(
            channel.bytes,
            width as usize * 4,
            height as usize,
            4,
            raw.large,
        )?;
        for (pixel, sample) in out.iter_mut().zip(plane.chunks_exact(4)) {
            let value = f32::from_be_bytes([sample[0], sample[1], sample[2], sample[3]]);
            if channel.kind == 0 {
                pixel[..3].fill(value);
            } else {
                pixel[3] = value;
            }
        }
    }
    Ok(out)
}

/// Opens a 32-bit Grayscale PSD or PSB (module doc) under `policy`, its
/// `warnings` (the untagged sentence, if any) first. `job` is already begun
/// by the caller.
#[allow(clippy::too_many_arguments)]
pub(crate) fn open_gray32(
    bytes: &[u8],
    path: &str,
    mode: i32,
    container: &Container<'_>,
    policy: &GrayPolicy,
    warnings: Vec<String>,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<PrecisionDocument, String> {
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
    // `layered` installs a layered open's mark, vector masks and comps on
    // the finished Gray document (`psd_typed_exchange`).
    let finish =
        |document: PrecisionDocument,
         repairs: &Repairs,
         layered: &dyn Fn(PrecisionDocument) -> Result<PrecisionDocument, String>| {
            let mut all = warnings.clone();
            all.extend(repairs.warnings());
            let mut document = document.with_import_warnings(all)?;
            document.global_light = light;
            document.color_samplers.clone_from(&color_samplers);
            // Resources 1025 and 2000–2997 (`psd_paths`).
            document
                .paths
                .set_entries(crate::psd_paths::file_paths(container).0);
            let document = document
                .with_gray(Arc::new(policy.clone()))
                .map_err(|e| format!("PSD {path}: {e}"))?;
            let document = layered(document)?;
            job.report(1.0, 5)?;
            Ok::<_, String>(document)
        };
    let composite = || -> Result<PrecisionDocument, String> {
        job.check()?;
        let (retained, scratch) = composite_plan32(container, bytes.len() as u64);
        admit(retained, scratch, other, limit)?;
        let mut pixels = composite32(container).map_err(|e| format!("PSD {path}: {e}"))?;
        let mut repairs = Repairs::default();
        repairs.take(&mut pixels);
        let image =
            PrecisionImage::from_rgba32_extended_decoded(cw, ch, pixels, twin.clone(), None)?;
        let layer = PrecisionDocumentLayer::raster_float("Background", image, (0, 0), 1.0, true)?;
        let document = PrecisionDocument::from_parts_float(
            cw,
            ch,
            Arc::clone(&twin),
            Metadata::default(),
            resolution,
            vec![layer],
            Vec::new(),
            ToneMap::Clip,
        )?;
        finish(document, &repairs, &Ok)
    };
    if mode == 1 {
        return composite();
    }

    let (psd, records) = crate::psd_import::parse_with(bytes, container, Some(policy))
        .map_err(|e| format!("PSD {path}: {e}; inspect the saved-composite option"))?;
    let (retained, scratch) = layered_plan32(container, &records, bytes.len() as u64, None)?;
    admit(retained, scratch, other, limit)?;
    let float = Some(&twin);
    if imports_nothing(&psd, &records, float) {
        return composite();
    }
    let canvas = (cw, ch);
    let mut repairs = Repairs::default();
    // The record each built entry came from, in the order the tree is
    // emitted (`psd_typed_exchange`).
    let sources = std::cell::RefCell::new(Vec::new());
    let roots = emit_typed_tree(
        &psd,
        &records,
        &mut |index, dissolved| {
            if dropped(&psd, &records, index, float) {
                return Ok(None);
            }
            sources
                .borrow_mut()
                .push(crate::psd_comps::CompSource::Layer(index));
            job.check()?;
            let layer = &psd.layers()[index];
            let body = gray_body(
                &records.layer_styles[index],
                record_plane(&records, index, canvas),
                true,
            );
            let blend = typed_blend(crate::psd::map_psd_blend(layer.blend_mode() as i32), false)
                .unwrap_or(BlendMode::Normal);
            let built = if let Some(carried) = records.carried(index) {
                adjustment_layer(
                    layer,
                    &gray_carried(carried),
                    records.masks[index],
                    4,
                    &twin,
                    blend,
                    &body,
                    canvas,
                    records.layer_labels[index],
                    records.layer_references[index],
                )?
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
                        decode_gray32(raw)?,
                    )
                };
                repairs.take(&mut samples);
                let image = PrecisionImage::from_rgba32_extended_decoded(
                    w,
                    h,
                    samples,
                    twin.clone(),
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
                with_carried_style(built, &body, canvas)?
            };
            // `with_locks` is always LAST (after mask and style).
            Ok(Some(gray_blend(built, true)?.with_locks(
                records.layer_features[index].locks() | dissolved,
            )))
        },
        &mut |id, children| {
            sources
                .borrow_mut()
                .push(crate::psd_comps::CompSource::Group(id));
            let built = group_layer(
                psd.groups()[&id].name(),
                &records.groups[id as usize - 1],
                children,
                4,
                &gray_body(&records.group_styles[id as usize - 1], canvas, true),
                canvas,
            )?;
            gray_blend(built, true)
        },
    )
    .map_err(|reason| {
        format!("PSD {path}: {reason}; explicitly choose the stored composite if available")
    })?;
    if roots.is_empty() {
        return Err(format!("PSD {path}: no layer pixels could be imported; explicitly choose the stored composite if available"));
    }
    let mut roots = roots;
    crate::gray_style::grey_records(&mut roots, &twin, policy)
        .map_err(|e| format!("PSD {path}: {e}"))?;
    let document = PrecisionDocument::from_parts_float(
        cw,
        ch,
        Arc::clone(&twin),
        Metadata::default(),
        resolution,
        roots,
        Vec::new(),
        ToneMap::Clip,
    )?;
    finish(document, &repairs, &|document| {
        crate::psd_typed_exchange::install(
            document,
            container,
            &records,
            &sources.borrow(),
            |index| !dropped(&psd, &records, index, float),
            limit,
            other,
            job,
        )
    })
}
