//! The UNRECORDED rule of layer comps, the one home of the predicate
//! both depths' apply and report call (`doc_comps`, `precision_comps`).
//!
//! **Applying a comp hides every layer whose id no entry of that comp names,
//! unless the layer is a GROUP**, at any depth of the tree, whatever the comp
//! captured and whatever the layer's locks. An unrecorded group keeps its
//! visibility. Measured against Photoshop 26.10.0 by
//! `scripts/photoshop-probe/layer_comps.py` (receipt
//! `doctored-fixtures/photoshop/layer-comps-521/take1/measured.json`): a new
//! layer at the top and inside a recorded group, a placed layer, a duplicated
//! layer and the children of a pasted or a new group are all hidden under
//! every capture set, while the pasted group, a new group (empty or not) and a
//! group made over recorded layers stay visible.
//!
//! Unrecorded is derived at apply time, never persisted, so a `.rz` saved
//! before this rule opens and applies under it with no format change. A stale
//! entry's id (0, or one no layer carries) names nothing, so it cannot make a
//! live layer recorded.

use crate::doc_comps::LayerComp;

/// `layers[i]` = (layer id, is a group), in the depth's own index order (byte
/// `layers`, typed flattened postorder). The positions applying `comp` hides:
/// every non-group layer whose id no entry of `comp` names (a stale entry's id
/// 0 matches nothing), ascending.
pub(crate) fn unrecorded_positions(comp: &LayerComp, layers: &[(u64, bool)]) -> Vec<usize> {
    layers
        .iter()
        .enumerate()
        .filter(|(_, &(id, group))| {
            !group
                && !comp
                    .entries
                    .iter()
                    .any(|entry| entry.layer_id != 0 && entry.layer_id == id)
        })
        .map(|(index, _)| index)
        .collect()
}
