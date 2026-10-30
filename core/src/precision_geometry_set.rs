//! Typed set geometry resolves one canvas map and one fixed-point membership
//! before the aggregate plane planner sees any destination allocation. A
//! resolved set holds a mapping PER RECORD: `affine_set` / `perspective_set`
//! give every member the same one, while the per-record translations of
//! typed Align and Distribute (`precision_align`) give each member its
//! own through [`PrecisionDocument::translate_records`].
//!
//! Float admits the filtered sets too. The typed `affine_set` takes an
//! optional `follow_links` (default `true`); `false` hands `expand` every entry as unlinked, so
//! membership and the position-lock gate are the roots' subtrees only — the typed twin of byte
//! `dr_doc_with_layer_offset`, which the host's 16-bit offset write sends; the byte request refuses
//! `false` (`core/tests/layer_offset_links.rs`). A resolved set holds a mapping PER
//! RECORD (`ResolvedSet`; `affine_set`/`perspective_set` give every member the same one):
//! `translate_records` / `translation_plan` apply one integer translation per record in ONE edit
//! under the context typed Move sends (an identity Nearest `affine_set`, so its budget, chrome,
//! comps and description rule), refusing any planned plane that is not a `Keep`.

use super::plan::{
    extent, pixel_channels, scale_factor, transform, GeometryPlan, LayerWork, Mapping, WorkKind,
};
use super::{GeometryEdit, GeometryTarget};
use crate::doc_perspective::{parallelogram_affine, solve_rect_to_quad};
use crate::doc_transform::{bounds_of_corners, Affine, EXACT_EPSILON};
use crate::geometry_set::{expand, SetEntry};
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_sampling::GeometryFilter;
use crate::precision_structure_ops::flatten;

/// A set edit resolved: each record's mapping (`None` for a non-member) and
/// the one sampler the members share.
pub(super) struct ResolvedSet {
    pub(super) members: Vec<Option<Mapping>>,
    pub(super) filter: GeometryFilter,
}

pub(super) fn expanded(doc: &PrecisionDocument, edit: &GeometryEdit) -> Result<Vec<usize>, String> {
    let (roots, follow_links) = match edit {
        GeometryEdit::AffineSet {
            layers,
            follow_links,
            ..
        } => (layers.as_slice(), *follow_links),
        GeometryEdit::PerspectiveSet { layers, .. } => (layers.as_slice(), true),
        _ => return Ok(Vec::new()),
    };
    expand(&doc.set_entries(follow_links), roots)
}

pub(super) fn resolve(
    doc: &PrecisionDocument,
    edit: &GeometryEdit,
) -> Result<Option<ResolvedSet>, String> {
    let selected = expanded(doc, edit)?;
    if selected.is_empty() {
        return Ok(None);
    }
    let (mapping, filter) = match edit {
        GeometryEdit::AffineSet { matrix, filter, .. } => {
            let affine = Affine::from_array(*matrix);
            if !affine.is_finite() || affine.inverse().is_none() {
                return Err("typed affine set matrix is nonfinite or singular".into());
            }
            (Mapping::Affine(affine), *filter)
        }
        GeometryEdit::PerspectiveSet {
            source_rect,
            quad,
            filter,
            ..
        } => {
            let [x, y, width, height] = *source_rect;
            if !source_rect.iter().all(|v| v.is_finite()) || width <= 0.0 || height <= 0.0 {
                return Err("typed perspective set source rectangle is invalid".into());
            }
            if !quad.iter().all(|v| v.is_finite()) {
                return Err("typed perspective set quad must be finite".into());
            }
            bounds_of_corners([
                (quad[0], quad[1]),
                (quad[2], quad[3]),
                (quad[4], quad[5]),
                (quad[6], quad[7]),
            ])
            .ok_or("typed perspective set quad exceeds supported bounds")?;
            let dx3 = quad[0] - quad[2] + quad[4] - quad[6];
            let dy3 = quad[1] - quad[3] + quad[5] - quad[7];
            let mapping = if dx3.abs() <= EXACT_EPSILON && dy3.abs() <= EXACT_EPSILON {
                Mapping::Affine(parallelogram_affine((x, y, width, height), quad))
            } else {
                let forward = solve_rect_to_quad((x, y, width, height), quad).ok_or(
                    "typed perspective set quad is singular, concave or crosses the horizon",
                )?;
                Mapping::Perspective {
                    forward: forward.0,
                    inverse: forward
                        .visible_inverse()
                        .ok_or("typed perspective set inverse is invalid")?,
                }
            };
            (mapping, *filter)
        }
        _ => unreachable!(),
    };
    let mut members = vec![None; doc.records];
    for index in selected {
        members[index] = Some(mapping);
    }
    Ok(Some(ResolvedSet { members, filter }))
}

/// The set branch of the plane planner for one member: its raster pixels and
/// its LINKED mask take its mapping (a group's own pixels are its children's;
/// an unlinked mask stays in canvas space).
pub(super) fn plane_work(
    layer: &PrecisionDocumentLayer,
    mapping: Mapping,
    filter: GeometryFilter,
    entry: &mut LayerWork,
) -> Result<(), String> {
    if layer.is_pixel_leaf() {
        entry.image = Some(transform(
            extent(layer, GeometryTarget::Pixels)?,
            mapping,
            filter,
            pixel_channels(layer),
        )?);
    }
    if layer
        .mask
        .as_ref()
        .is_some_and(|mask| mask.properties.linked)
    {
        entry.mask = Some(transform(
            extent(layer, GeometryTarget::Mask)?,
            mapping,
            filter,
            1,
        )?);
    }
    Ok(())
}

/// A member's Scale Effects factor: its mapping's scale over its pixel
/// extent on a raster (`sqrt(|det|)` for an affine), 1 otherwise.
pub(super) fn style_factor(
    layer: &PrecisionDocumentLayer,
    mapping: Mapping,
) -> Result<f64, String> {
    if layer.is_pixel_leaf() {
        scale_factor(mapping, extent(layer, GeometryTarget::Pixels)?)
    } else {
        Ok(1.0)
    }
}

impl PrecisionDocument {
    /// The postorder records as `geometry_set::expand` reads them. With
    /// `follow_links` false every entry is handed over as unlinked, so only
    /// the roots' subtrees are members.
    pub(crate) fn set_entries(&self, follow_links: bool) -> Vec<SetEntry> {
        flatten(&self.layers)
            .iter()
            .map(|record| SetEntry {
                depth: record.depth,
                group: matches!(
                    record.layer.content,
                    Content::Group(_) | Content::PassThrough(_)
                ),
                link: if follow_links { record.layer.link } else { 0 },
            })
            .collect()
    }

    /// The plan of one integer translation per record (`None` stays put),
    /// planned under [`translation_context`]. Every plane it plans must be a
    /// `Keep`: an align or distribute never resamples.
    pub(crate) fn translation_plan(
        &self,
        moves: &[Option<(i32, i32)>],
    ) -> Result<GeometryPlan, String> {
        // The job-less pre-check carries no work refusal;
        // `geometry_resolved` re-plans at the run's cap.
        let plan = super::plan::build(
            self,
            &translation_context(moves),
            Some(translation_set(self, moves)?),
            crate::sampling_gaussian::CONTROLLED_JOB_WORK,
        )?;
        let resamples = plan
            .layers
            .iter()
            .flat_map(|work| [work.image.as_ref(), work.mask.as_ref()])
            .flatten()
            .any(|work| !matches!(work.kind, WorkKind::Keep));
        if resamples {
            return Err("typed align must not resample".into());
        }
        Ok(plan)
    }

    /// Applies one integer translation per record in ONE typed edit, with
    /// typed Move's budget, chrome, comps and description rules (the context
    /// edit is never resolved or lock-checked; the caller gates locks).
    /// `Ok(None)` when nothing moves.
    pub(crate) fn translate_records(
        &self,
        moves: &[Option<(i32, i32)>],
        limit: u64,
        other_retained: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        self.translation_plan(moves)?;
        self.geometry_resolved(
            &translation_context(moves),
            Some(translation_set(self, moves)?),
            limit,
            other_retained,
            job,
        )
    }
}

/// The edit typed Move sends, standing in for a per-record translation: an
/// identity Nearest `affine_set` over the moved records, followed links,
/// not interactive — so the one-shot budget, the headroom, the chrome, the
/// comps and `preserve_exact_set_description` are typed Move's.
fn translation_context(moves: &[Option<(i32, i32)>]) -> GeometryEdit {
    GeometryEdit::AffineSet {
        layers: (0..moves.len()).filter(|&i| moves[i].is_some()).collect(),
        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        filter: GeometryFilter::Nearest,
        follow_links: true,
        interactive: false,
    }
}

fn translation_set(
    doc: &PrecisionDocument,
    moves: &[Option<(i32, i32)>],
) -> Result<ResolvedSet, String> {
    if moves.len() != doc.records {
        return Err("typed translation needs one entry per record".into());
    }
    Ok(ResolvedSet {
        members: moves
            .iter()
            .map(|delta| {
                delta.map(|(dx, dy)| {
                    Mapping::Affine(Affine::from_array([
                        1.0,
                        0.0,
                        0.0,
                        1.0,
                        f64::from(dx),
                        f64::from(dy),
                    ]))
                })
            })
            .collect(),
        filter: GeometryFilter::Nearest,
    })
}
