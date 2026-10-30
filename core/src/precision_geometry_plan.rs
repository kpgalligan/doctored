//! Allocation-free pixel planning for the typed geometry operation. Only the
//! bounded record/extent lists are allocated here; native capacity and aggregate
//! tap work are checked before any destination image or scalar plane exists.
use super::set::ResolvedSet;
use super::{GeometryEdit, GeometryTarget};
use crate::cmyk_image::CmykImage;
use crate::doc_perspective::{
    parallelogram_affine, solve_rect_to_quad, transformed_projective_bounds,
};
use crate::doc_transform::{
    bounds_of_corners, exact_form, transformed_plane_bounds, Affine, Exact, EXACT_EPSILON,
};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{pixel_count, PrecisionImage};
use crate::precision_sampling::{
    adoption_copy_bytes, cmyk_adoption_copy_bytes, cmyk_source_copy_bytes,
    float_adoption_copy_bytes, float_source_copy_bytes, resize_cost_channels, source_copy_bytes,
    uniform_band_bytes, uniform_source, GeometryFilter,
};

pub(super) type Extent = (i32, i32, u32, u32);
#[derive(Clone, Copy)]
pub(super) enum WorkKind {
    Keep,
    Resize(GeometryFilter),
    Affine {
        inverse: Affine,
        filter: GeometryFilter,
    },
    Perspective {
        inverse: [f64; 9],
        filter: GeometryFilter,
    },
}
#[derive(Clone)]
pub(super) struct PlaneWork {
    pub(super) source: Extent,
    pub(super) dest: Extent,
    pub(super) kind: WorkKind,
    pub(super) work: u64,
    output: u64,
    temporary: u64,
    /// A uniform u16 raster `transform_image` resamples into a tile band
    /// (`precision_sampling::uniform_source`); set in `build`.
    pub(super) uniform: bool,
}
#[derive(Clone)]
pub(super) struct LayerWork {
    pub(super) image: Option<PlaneWork>,
    pub(super) mask: Option<PlaneWork>,
    pub(super) source_transform: Option<crate::source_transform::SourceTransform>,
    pub(super) source_filter: Option<GeometryFilter>,
    pub(super) source_cost: Option<crate::retained_source::SourcePlan>,
    pub(super) source_exact: bool,
    /// The Scale Effects factor this layer's style takes (`style::scaled`):
    /// `sqrt(W'H'/WH)` for Resize on every layer, `sqrt(|det|)` for Affine
    /// and `quad_mean_scale` for Perspective on the targeted pixel layer,
    /// 1 otherwise.
    pub(super) style_factor: f64,
    /// How far Delete Cropped Pixels moved this raster's offset WITHOUT
    /// moving its content (`canvas::deleting_cropped`): what a linked pattern
    /// overlay's anchor is rebased by (`style_pattern::rebased`).
    pub(super) anchor_shift: (i64, i64),
    /// The record's Free Transform matrix `[a, b, c, d, tx, ty]` (a leaf
    /// `Mapping::Affine` with target Pixels — an affine or a parallelogram
    /// perspective — or a set member's `Mapping::Affine`), which its
    /// reference point and linked anchors follow
    /// (`doc_reference::follow_record`); `None` otherwise.
    pub(super) reference_map: Option<[f64; 6]>,
}
impl Default for LayerWork {
    fn default() -> Self {
        LayerWork {
            image: None,
            mask: None,
            source_transform: None,
            source_filter: None,
            source_cost: None,
            source_exact: false,
            style_factor: 1.0,
            anchor_shift: (0, 0),
            reference_map: None,
        }
    }
}
pub struct GeometryPlan {
    pub width: u32,
    pub height: u32,
    pub retained_bytes: u64,
    pub scratch_bytes: u64,
    pub work_units: u64,
    pub(super) layers: Vec<LayerWork>,
    pub(super) channels: Vec<Option<PlaneWork>>,
    pub(super) transpose_resolution: bool,
}
#[derive(Clone, Copy)]
pub(super) enum Mapping {
    Affine(Affine),
    Perspective {
        forward: [f64; 9],
        inverse: [f64; 9],
    },
}
pub(super) fn scale_factor(mapping: Mapping, source: Extent) -> Result<f64, String> {
    match mapping {
        Mapping::Affine(matrix) => Ok(matrix.determinant().abs().sqrt()),
        Mapping::Perspective { forward, .. } => {
            let (x, y, w, h) = source;
            let corners = [
                (f64::from(x), f64::from(y)),
                (f64::from(x) + f64::from(w), f64::from(y)),
                (f64::from(x) + f64::from(w), f64::from(y) + f64::from(h)),
                (f64::from(x), f64::from(y) + f64::from(h)),
            ];
            let mut quad = [0.0; 8];
            for (index, (x, y)) in corners.into_iter().enumerate() {
                let denominator = forward[6] * x + forward[7] * y + forward[8];
                if !denominator.is_finite() || denominator <= 0.0 {
                    return Err("typed perspective set crosses the horizon".into());
                }
                quad[index * 2] = (forward[0] * x + forward[1] * y + forward[2]) / denominator;
                quad[index * 2 + 1] = (forward[3] * x + forward[4] * y + forward[5]) / denominator;
            }
            Ok(crate::style::quad_mean_scale(quad, w, h))
        }
    }
}
pub(super) fn affine(
    source: Extent,
    matrix: Affine,
    filter: GeometryFilter,
    channels: u64,
) -> Result<PlaneWork, String> {
    if !matrix.is_finite() {
        return Err("typed affine matrix must be finite".into());
    }
    let inverse = matrix.inverse().ok_or("typed affine matrix is singular")?;
    let dest = transformed_plane_bounds((source.2, source.3), (source.0, source.1), &matrix)
        .ok_or("typed affine destination is outside supported bounds")?;
    let exact = exact_form(&matrix).filter(|form| {
        let dimensions = match form {
            Exact::Geom(geometry) if geometry.swaps_axes() => (source.3, source.2),
            _ => (source.2, source.3),
        };
        (dest.2, dest.3) == dimensions
    });
    let kind =
        if matches!(exact, Some(Exact::Translate)) && (dest.2, dest.3) == (source.2, source.3) {
            WorkKind::Keep
        } else {
            WorkKind::Affine {
                inverse,
                filter: if exact.is_some() {
                    GeometryFilter::Nearest
                } else {
                    filter
                },
            }
        };
    work(source, dest, kind, channels)
}
pub(super) fn transform(
    source: Extent,
    mapping: Mapping,
    filter: GeometryFilter,
    channels: u64,
) -> Result<PlaneWork, String> {
    match mapping {
        Mapping::Affine(m) => affine(source, m, filter, channels),
        Mapping::Perspective { forward, inverse } => {
            let dest = transformed_projective_bounds(
                (source.2, source.3),
                (source.0, source.1),
                &forward,
            )
            .ok_or(
                "typed perspective destination crosses the horizon or exceeds supported bounds",
            )?;
            work(
                source,
                dest,
                WorkKind::Perspective { inverse, filter },
                channels,
            )
        }
    }
}
/// One plane's work over `channels` u16 planes: 4 an RGBA16 raster, 1 a
/// mask or channel, 5 a CMYK document's CMYKA16 raster (#1 phase 4b,
/// [`pixel_channels`]), whose output is charged 10 B/px.
pub(super) fn work(
    source: Extent,
    dest: Extent,
    kind: WorkKind,
    channels: u64,
) -> Result<PlaneWork, String> {
    let n = pixel_count(dest.2, dest.3)? as u64;
    let (output, temporary, taps) = match kind {
        WorkKind::Keep => (0, 0, 0),
        WorkKind::Resize(filter) => {
            resize_cost_channels((source.2, source.3), (dest.2, dest.3), filter, channels)?
        }
        WorkKind::Affine { filter, .. } | WorkKind::Perspective { filter, .. } => {
            (n * channels * 2, 0, n * filter.taps().pow(2) * channels)
        }
    };
    // Initialization, resampling, and final byte no-op comparison are separate
    // visits. Zero-allocation moves require no full pixel visit.
    let visited = if output > 0 { 2 * n * channels } else { 0 };
    Ok(PlaneWork {
        source,
        dest,
        kind,
        work: taps
            .checked_add(visited)
            .ok_or("typed geometry work overflow")?,
        output,
        temporary,
        uniform: false,
    })
}
/// The u16 planes a pixel leaf's raster resamples over: 5 for a CMYK
/// document's CMYKA16 ink (#1 phase 4b), 4 for RGBA16 (float charges its
/// own factor in `build`).
pub(super) fn pixel_channels(layer: &PrecisionDocumentLayer) -> u64 {
    if matches!(layer.content, Content::CmykRaster { .. }) {
        5
    } else {
        4
    }
}
pub(super) fn extent(
    layer: &PrecisionDocumentLayer,
    target: GeometryTarget,
) -> Result<Extent, String> {
    match target {
        GeometryTarget::Pixels => match &layer.content {
            Content::Raster { image, offset } => {
                let (w, h) = image.dimensions();
                Ok((offset.0, offset.1, w, h))
            }
            Content::CmykRaster { image, offset } => {
                let (w, h) = image.dimensions();
                Ok((offset.0, offset.1, w, h))
            }
            _ => Err("typed geometry pixels target requires a raster leaf".into()),
        },
        GeometryTarget::Mask => layer
            .mask
            .as_ref()
            .map(|m| {
                let (w, h) = m.plane.dimensions();
                (m.properties.offset.0, m.properties.offset.1, w, h)
            })
            .ok_or("typed geometry target has no mask".into()),
    }
}
fn leaf_mapping(
    doc: &PrecisionDocument,
    edit: &GeometryEdit,
) -> Result<Option<(usize, GeometryTarget, Mapping, GeometryFilter)>, String> {
    match edit {
        GeometryEdit::Affine {
            layer,
            target,
            matrix,
            filter,
            ..
        } => {
            extent(
                doc.layer(*layer)
                    .ok_or("typed geometry layer index is out of range")?,
                *target,
            )?;
            let m = Affine::from_array(*matrix);
            if !m.is_finite() || m.inverse().is_none() {
                return Err("typed affine matrix is nonfinite or singular".into());
            }
            Ok(Some((*layer, *target, Mapping::Affine(m), *filter)))
        }
        GeometryEdit::Perspective {
            layer,
            target,
            quad,
            filter,
            ..
        } => {
            let source = extent(
                doc.layer(*layer)
                    .ok_or("typed geometry layer index is out of range")?,
                *target,
            )?;
            if !quad.iter().all(|v| v.is_finite()) {
                return Err("typed perspective quad must be finite".into());
            }
            let rect = (
                f64::from(source.0),
                f64::from(source.1),
                f64::from(source.2),
                f64::from(source.3),
            );
            let mapping = if (quad[0] - quad[2] + quad[4] - quad[6]).abs() <= EXACT_EPSILON
                && (quad[1] - quad[3] + quad[5] - quad[7]).abs() <= EXACT_EPSILON
            {
                Mapping::Affine(parallelogram_affine(rect, quad))
            } else {
                let forward = solve_rect_to_quad(rect, quad)
                    .ok_or("typed perspective quad is singular, concave or crosses the horizon")?;
                Mapping::Perspective {
                    forward: forward.0,
                    inverse: forward
                        .visible_inverse()
                        .ok_or("typed perspective inverse is invalid")?,
                }
            };
            // Validate the exact user corners independently of solve residue.
            bounds_of_corners([
                (quad[0], quad[1]),
                (quad[2], quad[3]),
                (quad[4], quad[5]),
                (quad[6], quad[7]),
            ])
            .ok_or("typed perspective quad exceeds supported bounds")?;
            Ok(Some((*layer, *target, mapping, *filter)))
        }
        _ => Ok(None),
    }
}
pub(super) fn rounded_offset(value: f64) -> Result<i32, String> {
    let value = value.round();
    if !value.is_finite() || value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
        return Err("typed geometry offset exceeds i32 coordinates".into());
    }
    Ok(value as i32)
}
fn scaled_size(value: u32, scale: f64) -> Result<u32, String> {
    let value = (f64::from(value) * scale).round().max(1.0);
    if !value.is_finite() || value > f64::from(u32::MAX) {
        return Err("typed resize dimension exceeds supported bounds".into());
    }
    Ok(value as u32)
}
pub(super) fn whole(
    source: Extent,
    edit: &GeometryEdit,
    canvas: (u32, u32),
    channels: u64,
) -> Result<PlaneWork, String> {
    let (w, h) = (f64::from(canvas.0), f64::from(canvas.1));
    match edit {
        GeometryEdit::Resize {
            width,
            height,
            filter,
        } => {
            let (sx, sy) = (f64::from(*width) / w, f64::from(*height) / h);
            let dest = (
                rounded_offset(f64::from(source.0) * sx)?,
                rounded_offset(f64::from(source.1) * sy)?,
                scaled_size(source.2, sx)?,
                scaled_size(source.3, sy)?,
            );
            let kind = if (source.2, source.3) == (dest.2, dest.3) {
                WorkKind::Keep
            } else {
                WorkKind::Resize(*filter)
            };
            work(source, dest, kind, channels)
        }
        GeometryEdit::Crop { x, y, .. } => {
            let dest = (
                rounded_offset(f64::from(source.0) - f64::from(*x))?,
                rounded_offset(f64::from(source.1) - f64::from(*y))?,
                source.2,
                source.3,
            );
            work(source, dest, WorkKind::Keep, channels)
        }
        // Canvas Size is a generalized Crop and its per-plane rule lives in
        // the `canvas` submodule (G1). The two filtered canvas operations
        // share its dispatcher.
        GeometryEdit::CanvasSize { x, y, .. } => {
            super::canvas::size_plane(source, *x, *y, channels)
        }
        GeometryEdit::Straighten { .. } | GeometryEdit::RotateCanvas { .. } => {
            super::canvas::plane(source, canvas, edit, channels)
        }
        other => {
            let matrix = match other {
                GeometryEdit::Rotate90 => [0.0, 1.0, -1.0, 0.0, h, 0.0],
                GeometryEdit::Rotate180 => [-1.0, 0.0, 0.0, -1.0, w, h],
                GeometryEdit::Rotate270 => [0.0, -1.0, 1.0, 0.0, 0.0, w],
                GeometryEdit::FlipHorizontal => [-1.0, 0.0, 0.0, 1.0, w, 0.0],
                GeometryEdit::FlipVertical => [1.0, 0.0, 0.0, -1.0, 0.0, h],
                _ => return Err("invalid whole-document geometry request".into()),
            };
            affine(
                source,
                Affine::from_array(matrix),
                GeometryFilter::Nearest,
                channels,
            )
        }
    }
}
/// The plan for `edit`; a set edit's per-record mappings come from `set`
/// when given (`translate_records`), else are resolved from `edit`. Every
/// edit but a preview (`interactive`, `MAX_WORK_UNITS`) has its work bounded
/// by `one_shot_cap` (`CONTROLLED_JOB_WORK` for a plan,
/// `destructive_work_cap(job)` for a run).
pub(super) fn build(
    doc: &PrecisionDocument,
    edit: &GeometryEdit,
    set: Option<ResolvedSet>,
    one_shot_cap: u64,
) -> Result<GeometryPlan, String> {
    let leaf = leaf_mapping(doc, edit)?;
    let set = set.map_or_else(|| super::set::resolve(doc, edit), |set| Ok(Some(set)))?;
    let canvas = doc.dimensions();
    let (width, height) = match edit {
        GeometryEdit::Resize { width, height, .. } => (*width, *height),
        GeometryEdit::Crop {
            x,
            y,
            width,
            height,
        } => {
            if u64::from(*x) + u64::from(*width) > u64::from(canvas.0)
                || u64::from(*y) + u64::from(*height) > u64::from(canvas.1)
            {
                return Err("typed crop must lie inside the current canvas".into());
            }
            (*width, *height)
        }
        GeometryEdit::CanvasSize { width, height, .. } => (*width, *height),
        GeometryEdit::Straighten { .. } | GeometryEdit::RotateCanvas { .. } => {
            super::canvas::window(canvas, edit, doc.channels.len())?
        }
        GeometryEdit::Rotate90 | GeometryEdit::Rotate270 => (canvas.1, canvas.0),
        _ => canvas,
    };
    pixel_count(width, height)?;
    let mut plan = GeometryPlan {
        width,
        height,
        retained_bytes: doc.retained_bytes(),
        scratch_bytes: 4 * 1024 * 1024,
        work_units: 0,
        layers: Vec::with_capacity(doc.records),
        channels: Vec::with_capacity(doc.channels.len()),
        transpose_resolution: matches!(edit, GeometryEdit::Rotate90 | GeometryEdit::Rotate270)
            || matches!(
                edit,
                GeometryEdit::RotateCanvas { degrees, .. }
                    if matches!(
                        crate::doc_rotate::rotation_kind(*degrees),
                        Some((
                            crate::doc_rotate::RotationKind::Rotate90
                                | crate::doc_rotate::RotationKind::Rotate270,
                            _
                        ))
                    )
            ),
    };
    #[allow(clippy::too_many_arguments)]
    fn visit(
        doc: &PrecisionDocument,
        layers: &[PrecisionDocumentLayer],
        plan: &mut GeometryPlan,
        edit: &GeometryEdit,
        canvas: (u32, u32),
        leaf: Option<(usize, GeometryTarget, Mapping, GeometryFilter)>,
        set: Option<&ResolvedSet>,
        one_shot_cap: u64,
    ) -> Result<(), String> {
        for layer in layers {
            if let Content::Group(children) | Content::PassThrough(children) = &layer.content {
                visit(doc, children, plan, edit, canvas, leaf, set, one_shot_cap)?;
            }
            let member = set.and_then(|set| set.members[plan.layers.len()]);
            let style_factor = match edit {
                _ if set.is_some() => {
                    member.map_or(Ok(1.0), |mapping| super::set::style_factor(layer, mapping))?
                }
                GeometryEdit::Resize { width, height, .. } => (f64::from(*width)
                    * f64::from(*height)
                    / (f64::from(canvas.0) * f64::from(canvas.1)))
                .sqrt(),
                GeometryEdit::Affine {
                    layer: index,
                    target: GeometryTarget::Pixels,
                    matrix,
                    ..
                } if *index == plan.layers.len() => {
                    Affine::from_array(*matrix).determinant().abs().sqrt()
                }
                GeometryEdit::Perspective {
                    layer: index,
                    target: GeometryTarget::Pixels,
                    quad,
                    ..
                } if *index == plan.layers.len() => {
                    let (_, _, lw, lh) = extent(layer, GeometryTarget::Pixels)?;
                    crate::style::quad_mean_scale(*quad, lw, lh)
                }
                // Straighten's byte twin is `transform_layer_unlocked_with_job`, so the
                // factor is the matrix's own `sqrt(|det|)` — and only a RASTER
                // record's pixels are resampled by it (G6).
                GeometryEdit::Straighten { matrix, .. } if layer.is_pixel_leaf() => {
                    Affine::from_array(*matrix).determinant().abs().sqrt()
                }
                _ => 1.0,
            };
            let mut entry = LayerWork {
                style_factor,
                ..LayerWork::default()
            };
            if let Some((index, target, mapping, filter)) = leaf {
                if index == plan.layers.len() {
                    if let (GeometryTarget::Pixels, Mapping::Affine(m)) = (target, mapping) {
                        entry.reference_map = Some([m.a, m.b, m.c, m.d, m.tx, m.ty]);
                    }
                    match target {
                        GeometryTarget::Pixels => {
                            entry.image = Some(transform(
                                extent(layer, target)?,
                                mapping,
                                filter,
                                pixel_channels(layer),
                            )?);
                            if layer.mask.as_ref().is_some_and(|m| m.properties.linked) {
                                entry.mask = Some(transform(
                                    extent(layer, GeometryTarget::Mask)?,
                                    mapping,
                                    filter,
                                    1,
                                )?);
                            }
                        }
                        GeometryTarget::Mask => {
                            entry.mask =
                                Some(transform(extent(layer, target)?, mapping, filter, 1)?)
                        }
                    }
                }
            } else if let Some(set) = set {
                if let Some(mapping) = member {
                    if let Mapping::Affine(m) = mapping {
                        entry.reference_map = Some([m.a, m.b, m.c, m.d, m.tx, m.ty]);
                    }
                    super::set::plane_work(layer, mapping, set.filter, &mut entry)?;
                }
            } else {
                if layer.is_pixel_leaf() {
                    let channels = pixel_channels(layer);
                    let image = whole(
                        extent(layer, GeometryTarget::Pixels)?,
                        edit,
                        canvas,
                        channels,
                    )?;
                    // Delete Cropped Pixels clips an eligible raster.
                    entry.image = Some(if super::canvas::trims(layer) {
                        let before = (i64::from(image.dest.0), i64::from(image.dest.1));
                        let trimmed = super::canvas::deleting_cropped(image, edit, channels)?;
                        entry.anchor_shift = (
                            i64::from(trimmed.dest.0) - before.0,
                            i64::from(trimmed.dest.1) - before.1,
                        );
                        trimmed
                    } else {
                        image
                    });
                }
                if layer.mask.is_some() {
                    entry.mask = Some(whole(
                        extent(layer, GeometryTarget::Mask)?,
                        edit,
                        canvas,
                        1,
                    )?);
                }
            }
            if let (Some(mask), Some(work)) = (&layer.mask, &entry.mask) {
                if !mask.properties.valid((work.dest.2, work.dest.3)) {
                    return Err("typed geometry exceeds padded mask bounds".into());
                }
            }
            super::source::bind(
                doc,
                layer,
                plan.layers.len(),
                edit,
                leaf,
                set,
                one_shot_cap,
                &mut entry,
            )?;
            if let (Content::Raster { image, .. }, Some(work)) = (&layer.content, &mut entry.image)
            {
                work.uniform = matches!(
                    work.kind,
                    WorkKind::Affine { .. } | WorkKind::Perspective { .. }
                ) && uniform_source(image, (work.dest.2, work.dest.3)).is_some();
            }
            // The ONE typed geometry plane gate: a styled layer's padded plane
            // at its planned destination extents with the scaled pad, refused
            // here before any destination allocates or resamples.
            if layer.style.is_some() {
                let pad = crate::style::scaled_style(&layer.style, entry.style_factor)
                    .map_or(0, |s| s.pad());
                let dims = match &entry.image {
                    Some(work) if layer.is_pixel_leaf() => (work.dest.2, work.dest.3),
                    None if layer.is_pixel_leaf() => {
                        let (_, _, w, h) = extent(layer, GeometryTarget::Pixels)?;
                        (w, h)
                    }
                    _ => (plan.width, plan.height),
                };
                if crate::precision_style_render::plane_pixels(dims, pad)? > crate::doc::MAX_PIXELS
                {
                    return Err(format!(
                        "the layer style plane of “{}” would exceed the pixel limit after this geometry; reduce the effect sizes or clear its style first",
                        layer.name()
                    ));
                }
            }
            plan.layers.push(entry);
        }
        Ok(())
    }
    visit(
        doc,
        &doc.layers,
        &mut plan,
        edit,
        canvas,
        leaf,
        set.as_ref(),
        one_shot_cap,
    )?;
    for _ in doc.channels.iter() {
        let source = (0, 0, canvas.0, canvas.1);
        plan.channels.push(if leaf.is_some() || set.is_some() {
            None
        } else if let GeometryEdit::Crop { x, y, .. } = edit {
            let inverse = Affine::from_array([1.0, 0.0, 0.0, 1.0, f64::from(*x), f64::from(*y)]);
            let kind = if *x == 0 && *y == 0 && (width, height) == canvas {
                WorkKind::Keep
            } else {
                WorkKind::Affine {
                    inverse,
                    filter: GeometryFilter::Nearest,
                }
            };
            Some(work(source, (0, 0, width, height), kind, 1)?)
        } else if matches!(
            edit,
            GeometryEdit::Straighten { .. } | GeometryEdit::RotateCanvas { .. }
        ) {
            Some(super::canvas::channel(canvas, (width, height), edit)?)
        } else if let GeometryEdit::CanvasSize { x, y, .. } = edit {
            // A canvas plane is genuinely re-laid out rather than merely
            // shifted, so it takes its own branch exactly as Crop does.
            Some(super::canvas::size_channel(
                source,
                canvas,
                (width, height),
                *x,
                *y,
            )?)
        } else {
            Some(whole(source, edit, canvas, 1)?)
        });
    }
    let mut rasters = Vec::new();
    let mut masks = Vec::new();
    for (index, entry) in plan.layers.iter().enumerate() {
        let layer = doc.layer(index).ok_or("invalid typed layer traversal")?;
        let dimensions = layer
            .source()
            .map(PrecisionImage::dimensions)
            .or_else(|| layer.cmyk_source().map(CmykImage::dimensions));
        if let Some(dimensions) = dimensions {
            rasters.push(
                entry
                    .image
                    .as_ref()
                    .map_or(dimensions, |w| (w.dest.2, w.dest.3)),
            );
        }
        if let Some(mask) = &layer.mask {
            masks.push(
                entry
                    .mask
                    .as_ref()
                    .map_or(mask.plane.dimensions(), |w| (w.dest.2, w.dest.3)),
            );
        }
    }
    doc.native_fits_geometry((width, height), &rasters, &masks, doc.channels.len())?;
    // A RASTER destination on a float document is RGBA32 — 16 B/px where
    // `work` counted the u16 form's 8 — so each raster output is charged
    // TWICE there. Masks and channels are u16 planes at either depth and read
    // their samples in place. A resampled raster also binds its source's
    // contiguous copy (`source_copy_bytes`; `float_source_copy_bytes` at
    // float) while it resamples, and that copy is still bound when the
    // contiguous output is tiled into the destination (`adoption_copy_bytes`;
    // `float_adoption_copy_bytes` at float). The separable temporaries
    // are dropped before that adoption, so its plane transient is the copy
    // plus the larger of `temporary` and the adoption — a MAX across planes,
    // since everything drops as each plane returns. Masks and channels adopt
    // their buffer (`PrecisionPlane::from_u16`) and pay neither. A uniform u16
    // raster (`PlaneWork::uniform`) pays neither copy either: its
    // transient is one tile band (`uniform_band_bytes`); its output stays n·8,
    // the case where no tile is shared.
    //
    // A layer whose retained source is re-rendered (`source_cost`
    // and not `source_exact`) never executes its IMAGE plane —
    // `Execution::layers` renders only the source there, and its progress
    // range is the source render's units — so that plane never adds work
    // units. Its output and temporary are still charged to the scratch in
    // every edit: the skip under a one-shot commit or a preview, which
    // existed only to admit 24 MP placed work at 2 GiB, is gone.
    // The converse holds for an exact layer (`source_exact`): only its
    // raster plane runs and its progress range is that plane's units, so its
    // never-run source render adds no work units; that render's scratch is
    // still charged.
    let raster_bytes: u64 = if doc.is_float() { 2 } else { 1 };
    let mut temporary = 0;
    for (work, factor, raster, rerendered) in plan
        .layers
        .iter()
        .flat_map(|w| {
            let rerendered = w.source_cost.is_some() && !w.source_exact;
            [
                (w.image.as_ref(), raster_bytes, true, rerendered),
                (w.mask.as_ref(), 1, false, false),
            ]
        })
        .chain(plan.channels.iter().map(|c| (c.as_ref(), 1, false, false)))
        .filter_map(|(work, factor, raster, rerendered)| {
            work.map(|work| (work, factor, raster, rerendered))
        })
    {
        if !rerendered {
            plan.work_units = plan
                .work_units
                .checked_add(work.work)
                .ok_or("typed geometry work overflow")?;
        }
        plan.scratch_bytes = plan
            .scratch_bytes
            .checked_add(
                work.output
                    .checked_mul(factor)
                    .ok_or("typed geometry scratch overflow")?,
            )
            .ok_or("typed geometry scratch overflow")?;
        let plane = if raster && work.uniform {
            uniform_band_bytes((work.dest.2, work.dest.3))?
        } else if raster && !matches!(work.kind, WorkKind::Keep) {
            // A CMYK document's rasters are all CMYKA16 (#1 phase 4b):
            // `transform_cmyk` / `resize_cmyk` bind the 10 B/px copies.
            let (source_copy, adoption_copy): (fn(_) -> _, fn(_) -> _) = if doc.is_float() {
                (float_source_copy_bytes, float_adoption_copy_bytes)
            } else if doc.is_cmyk() {
                (cmyk_source_copy_bytes, cmyk_adoption_copy_bytes)
            } else {
                (source_copy_bytes, adoption_copy_bytes)
            };
            let adoption = if work.output > 0 {
                adoption_copy((work.dest.2, work.dest.3))?
            } else {
                0
            };
            source_copy((work.source.2, work.source.3))?
                .checked_add(work.temporary.max(adoption))
                .ok_or("typed geometry scratch overflow")?
        } else {
            work.temporary
        };
        temporary = temporary.max(plane);
    }
    for layer in &plan.layers {
        let Some(source) = layer.source_cost else {
            continue;
        };
        plan.scratch_bytes = plan
            .scratch_bytes
            .checked_add(source.scratch_bytes)
            .ok_or("typed retained source geometry scratch overflow")?;
        if !layer.source_exact {
            plan.work_units = plan
                .work_units
                .checked_add(source.work_units)
                .ok_or("typed retained source geometry work overflow")?;
        }
    }
    plan.scratch_bytes += temporary
        + doc.records as u64
            * (std::mem::size_of::<LayerWork>()
                + 2 * std::mem::size_of::<PrecisionDocumentLayer>()
                + 256) as u64
        + doc.channels.len() as u64 * (std::mem::size_of::<PlaneWork>() + 256) as u64;
    plan.scratch_bytes = super::budget::with_headroom(edit, doc.is_cmyk(), plan.scratch_bytes)?;
    if plan.work_units > super::budget::work_limit(edit, one_shot_cap) {
        return Err("typed geometry exceeds the bounded pixel-work limit".into());
    }
    Ok(plan)
}
