//! Retained-master canvas maps for typed geometry. The planner and execution
//! share this forward map; a filtered fallback is never used as source input.

use crate::doc_rotate::{rotated_canvas, rotation_kind, RotationKind};
use crate::doc_transform::{exact_form, Affine, Exact};
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_sampling::GeometryFilter;
use crate::source_transform::SourceTransform;

use super::plan::{LayerWork, Mapping};
use super::set::ResolvedSet;
use super::{GeometryEdit, GeometryTarget};

pub(super) fn incremental(
    edit: &GeometryEdit,
    canvas: (u32, u32),
    set: Option<Mapping>,
) -> Result<SourceTransform, String> {
    if let Some(mapping) = set {
        return Ok(match mapping {
            Mapping::Affine(affine) => SourceTransform::Affine([
                affine.a, affine.b, affine.c, affine.d, affine.tx, affine.ty,
            ]),
            Mapping::Perspective { forward, .. } => SourceTransform::Projective(forward),
        });
    }
    let (w, h) = (f64::from(canvas.0), f64::from(canvas.1));
    let matrix = match edit {
        GeometryEdit::Resize { width, height, .. } => [
            f64::from(*width) / w,
            0.,
            0.,
            f64::from(*height) / h,
            0.,
            0.,
        ],
        GeometryEdit::Crop { x, y, .. } => [1., 0., 0., 1., -f64::from(*x), -f64::from(*y)],
        GeometryEdit::CanvasSize { x, y, .. } => [1., 0., 0., 1., f64::from(*x), f64::from(*y)],
        GeometryEdit::Straighten { matrix, x, y, .. } => [
            matrix[0],
            matrix[1],
            matrix[2],
            matrix[3],
            matrix[4] - f64::from(*x),
            matrix[5] - f64::from(*y),
        ],
        GeometryEdit::RotateCanvas { degrees, .. } => {
            let (kind, angle) = rotation_kind(*degrees).ok_or("rotation degrees must be finite")?;
            match kind {
                RotationKind::Identity => [1., 0., 0., 1., 0., 0.],
                RotationKind::Rotate90 => [0., 1., -1., 0., h, 0.],
                RotationKind::Rotate180 => [-1., 0., 0., -1., w, h],
                RotationKind::Rotate270 => [0., -1., 1., 0., 0., w],
                RotationKind::Filtered => {
                    rotated_canvas(canvas.0, canvas.1, angle)
                        .ok_or("the rotated canvas exceeds supported bounds")?
                        .matrix
                }
            }
        }
        GeometryEdit::Rotate90 => [0., 1., -1., 0., h, 0.],
        GeometryEdit::Rotate180 => [-1., 0., 0., -1., w, h],
        GeometryEdit::Rotate270 => [0., -1., 1., 0., 0., w],
        GeometryEdit::FlipHorizontal => [-1., 0., 0., 1., w, 0.],
        GeometryEdit::FlipVertical => [1., 0., 0., -1., 0., h],
        _ => return Err("retained source has no document geometry map".into()),
    };
    Ok(SourceTransform::Affine(matrix))
}

/// The plan's retained-source BINDING for record `index`: when the record
/// is selected (a set member, the leaf's pixel target, or every record of a
/// whole-document edit) its binding takes the edit's incremental map composed
/// onto its own, and `entry` records the accumulated map, the sampler, the
/// source render's cost and whether the planned plane IS that render. The
/// render's work is bounded by `budget::work_limit(edit, one_shot_cap)`.
#[allow(clippy::too_many_arguments)]
pub(super) fn bind(
    doc: &PrecisionDocument,
    layer: &PrecisionDocumentLayer,
    index: usize,
    edit: &GeometryEdit,
    leaf: Option<(usize, GeometryTarget, Mapping, GeometryFilter)>,
    set: Option<&ResolvedSet>,
    one_shot_cap: u64,
    entry: &mut LayerWork,
) -> Result<(), String> {
    let (Some(source), Some(work)) = (layer.effective_source(), &mut entry.image) else {
        return Ok(());
    };
    let member = set.and_then(|set| set.members[index]);
    let selected = member.is_some()
        || leaf.is_some_and(|(leaf_index, target, _, _)| {
            leaf_index == index && matches!(target, GeometryTarget::Pixels)
        });
    if !selected && (leaf.is_some() || set.is_some()) {
        return Ok(());
    }
    let incremental = incremental(
        edit,
        doc.dimensions(),
        member.or_else(|| leaf.map(|(_, _, mapping, _)| mapping)),
    )?;
    let accumulated = crate::source_transform::compose(incremental, source.transform)?;
    let source_dest = crate::source_transform::bounds(source.asset.dimensions(), accumulated)?;
    // An exact integer translation (typed Move, Align and Distribute)
    // resamples nothing, so it keeps the binding's own filter: the byte move
    // (`doc_align::translate_entry`) never touches it, so a Move's Nearest
    // request must not rewrite it.
    let translate = matches!(incremental, SourceTransform::Affine(m)
        if matches!(exact_form(&Affine::from_array(m)), Some(Exact::Translate)));
    let filter = match edit {
        _ if translate => source.filter,
        GeometryEdit::AffineSet { filter, .. }
        | GeometryEdit::PerspectiveSet { filter, .. }
        | GeometryEdit::Perspective { filter, .. }
        | GeometryEdit::Resize { filter, .. }
        | GeometryEdit::Straighten { filter, .. }
        | GeometryEdit::RotateCanvas { filter, .. } => *filter,
        _ => source.filter,
    };
    let exact = matches!(incremental, SourceTransform::Affine(m)
        if exact_form(&Affine::from_array(m)).is_some())
        && source_dest == work.dest;
    if !exact {
        work.dest = source_dest;
    }
    // A placed image takes the edit's own work limit (its caller's cap for
    // every edit but a preview), a preview `MAX_WORK_UNITS`. An exact
    // layer's render never runs, so its work refuses nothing.
    let work_limit = if exact {
        crate::sampling_gaussian::CONTROLLED_JOB_WORK
    } else {
        super::budget::work_limit(edit, one_shot_cap)
    };
    let mut cost = crate::retained_source_render::work_plan_within(
        work_limit,
        0,
        crate::retained_source_render::SourceStage {
            asset: &source.asset,
        },
        (work.dest.2, work.dest.3),
        0,
        crate::retained_source_render::SourceSampling::new(
            source.asset.dimensions(),
            (work.dest.2, work.dest.3),
            accumulated,
            filter,
        ),
        doc.source_fallback_depth(),
        crate::retained_source_render::profile_cost(source.asset.profile(), &doc.profile, true)?,
    )?;
    if doc.is_float() {
        cost.scratch_bytes = cost
            .scratch_bytes
            .checked_add(crate::retained_source_float::float_render_scratch(
                source.asset.dimensions(),
                (work.dest.2, work.dest.3),
            )?)
            .ok_or("source render size overflow")?;
    }
    entry.source_transform = Some(accumulated);
    entry.source_filter = Some(filter);
    entry.source_cost = Some(cost);
    entry.source_exact = exact;
    Ok(())
}
