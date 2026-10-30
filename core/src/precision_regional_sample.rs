//! The 16-bit and float regional compositor's CURSOR bodies (moved out of
//! `precision_regional_tree` in 13A-4b-ii-b-1, verbatim): the staged
//! drivers' cursor range (`sample_between`, 13A-4b-ii-a: through
//! pass-through children, their held samples), the guided-node predicate and
//! the cursor paths (`guided`, `guided_paths`, `node_at_mut`). Index
//! arithmetic only around `precision_regional_tree`'s per-sample statements.
//! The contract is `regional`'s module doc.

use crate::precision_regional_tree::{
    clip_unit_sample, finish_pass_through, pass_through_env, sample_layers, Node,
};
use crate::precision_render::Kind;
use crate::precision_style_render::StyleEnv;
use crate::regional_guide::Holds;

const MISMATCH: &str = "regional stage mismatch";

/// Nodes from cursor `from` (inclusive; `&[]` = the start) to cursor `to`
/// (exclusive; `&[]` = the end) onto `acc[i]` at canvas `xy` (13A-4b-ii-a),
/// the byte `ResolvedLevel::sample_between`'s cursor rule over
/// `sample_layers`: the node `u = from[0]` with `from.len() > 1` is RESUMED
/// (its pass-through children from `from[1..]` to (`to[0] == u` ? `to[1..]`
/// : `&[]`), Shallow `holds.at(prefix+[u], xy)`) and, unless `u == to[0]`,
/// FINISHED onto its held sample (`S == None` is a stage mismatch); the node
/// `u = to[0]` with `to.len() > 1` is ENTERED (`holds.write(prefix+[u], xy,
/// acc[i])`, its children from the start to `to[1..]`, Shallow the sample as
/// it stood); every other node in `[from[0], to[0])` is `sample_layers`.
/// 13A-4b-ii-b-1: a cursor may END in a clip unit's member step `[.., u, m]`
/// (`clip_unit_at`): ENTERED, the sample written to the unit's hold and
/// `acc[i]` left as the unit buffer at member `m` (`clip_unit_sample` with
/// `stop`); RESUMED, the unit RE-RUN from its held sample (`None` is a stage
/// mismatch) to the next member cursor in it (`to[0] == u`) or its FINISH.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sample_between(
    nodes: &[Node<'_>],
    from: &[usize],
    to: &[usize],
    prefix: &mut Vec<usize>,
    acc: &mut [[f32; 4]],
    i: usize,
    env: &StyleEnv<'_>,
    xy: (i64, i64),
    holds: &mut Holds,
) -> Result<(), String> {
    let resumed = (from.len() > 1).then(|| from[0]);
    let entered = (to.len() > 1).then(|| to[0]);
    if let Some(u) = resumed {
        let node = nodes.get(u).ok_or(MISMATCH)?;
        prefix.push(u);
        let held = holds.at(prefix, xy);
        if clip_unit_at(nodes, &[u]) {
            prefix.pop();
            let stop = (to.len() > 1 && to[0] == u).then(|| to[1]);
            clip_unit_sample(node, acc, i, held.ok_or(MISMATCH)?, stop, env, xy)?;
        } else {
            let before = held.map(|s| [s]);
            let children = pass_through_env(env, before.as_ref().map(|s| &s[..]));
            let inner = if entered == Some(u) { &to[1..] } else { &[] };
            sample_between(
                &node.children,
                &from[1..],
                inner,
                prefix,
                acc,
                i,
                &children,
                xy,
                holds,
            )?;
            prefix.pop();
            if entered != Some(u) {
                finish_pass_through(acc, i, held.ok_or(MISMATCH)?, node, node.layer, xy)?;
            }
        }
    }
    let start = resumed.map_or(from.first().copied().unwrap_or(0), |u| u + 1);
    let end = to.first().copied().unwrap_or(nodes.len()).min(nodes.len());
    sample_layers(&nodes[start.min(end)..end], acc, i, env, xy)?;
    if let Some(u) = entered.filter(|&u| resumed != Some(u)) {
        let node = nodes.get(u).ok_or(MISMATCH)?;
        prefix.push(u);
        holds.write(prefix, xy, acc[i]);
        if clip_unit_at(nodes, &[u]) {
            prefix.pop();
            let began = acc[i];
            return clip_unit_sample(node, acc, i, began, Some(to[1]), env, xy);
        }
        let before = [acc[i]];
        let children = pass_through_env(env, Some(&before));
        sample_between(
            &node.children,
            &[],
            &to[1..],
            prefix,
            acc,
            i,
            &children,
            xy,
            holds,
        )?;
        prefix.pop();
    }
    Ok(())
}

/// A GUIDED lone node: an adjustment with a guide radius and no members —
/// the walk's staged predicate at an admitted level.
pub(crate) fn guided(node: &Node<'_>) -> bool {
    matches!(&node.layer.kind, Kind::Adjustment(state)
        if state.kernel.guide_radius().is_some())
        && node.members.is_empty()
}

/// Guided lone nodes (`guided`) of `nodes` and, recursively, of every
/// `Kind::PassThrough` child level, as paths, row order (13A-4b-ii-a), and
/// (13A-4b-ii-b-1) `[u, m]` for every `guided` member — of any base, styled
/// included since 13A-4b-ii-b-2 — after the node's own subtree paths. (A `Kind::PassThrough` node in the prepared tree never
/// carries members: `precision_document.rs:288-291`.)
pub(crate) fn guided_paths(nodes: &[Node<'_>]) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    for (u, node) in nodes.iter().enumerate() {
        if guided(node) {
            out.push(vec![u]);
        }
        if let Kind::PassThrough(_) = &node.layer.kind {
            out.extend(guided_paths(&node.children).into_iter().map(|mut path| {
                path.insert(0, u);
                path
            }));
        }
        for (m, member) in node.members.iter().enumerate() {
            if guided(member) {
                out.push(vec![u, m]);
            }
        }
    }
    out
}

/// Whether the node at `path` (node positions through pass-through
/// children) is a CLIP unit — `full` over a non-empty `members`, the
/// reference's `clipped_members` branch (13A-4b-ii-b-1), or a STYLED node
/// with members, `render_styled`'s member loop (13A-4b-ii-b-2).
pub(crate) fn clip_unit_at(nodes: &[Node<'_>], path: &[usize]) -> bool {
    let Some((&u, rest)) = path.split_first() else {
        return false;
    };
    let Some(node) = nodes.get(u) else {
        return false;
    };
    match (&node.layer.kind, rest.is_empty()) {
        (_, true) => (node.full.is_some() || node.style.is_some()) && !node.members.is_empty(),
        (Kind::PassThrough(_), false) => clip_unit_at(&node.children, rest),
        _ => false,
    }
}

/// The node at `path` (node positions down through pass-through children),
/// or a clip unit's member `[.., u, m]` (13A-4b-ii-b-1).
pub(crate) fn node_at_mut<'n, 'a>(
    nodes: &'n mut [Node<'a>],
    path: &[usize],
) -> Option<&'n mut Node<'a>> {
    let (&u, rest) = path.split_first()?;
    let node = nodes.get_mut(u)?;
    if rest.is_empty() {
        return Some(node);
    }
    match (&node.layer.kind, rest) {
        (Kind::PassThrough(_), _) => node_at_mut(&mut node.children, rest),
        (_, [m]) => node.members.get_mut(*m),
        _ => None,
    }
}
