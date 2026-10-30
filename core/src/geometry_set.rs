//! Shared postorder subtree and link expansion for byte and typed geometry.
//! Both document models expose the same bottom-first level sequence here, so
//! an entry reached through two roots or a transitive link is transformed once.
//!
//! A perspective set solves one canvas map for all raster content and linked masks, including group
//! and adjustment masks. `geometry_affine_set` and `geometry_perspective_set` are distinct
//! capability families; the historical leaf-family target table remains unchanged.

use std::collections::HashSet;

#[derive(Clone, Copy)]
pub(crate) struct SetEntry {
    pub depth: usize,
    pub group: bool,
    pub link: u32,
}

pub(crate) fn expand(entries: &[SetEntry], roots: &[usize]) -> Result<Vec<usize>, String> {
    if roots.is_empty() {
        return Err("geometry set must contain at least one layer".into());
    }
    let mut roots_seen = HashSet::new();
    for &index in roots {
        if index >= entries.len() {
            return Err("geometry set layer index is out of range".into());
        }
        if !roots_seen.insert(index) {
            return Err("geometry set contains a duplicate layer index".into());
        }
    }
    let mut included = vec![false; entries.len()];
    for &root in roots {
        include_subtree(entries, &mut included, root);
    }
    loop {
        let links: HashSet<u32> = entries
            .iter()
            .zip(&included)
            .filter_map(|(entry, selected)| (*selected && entry.link != 0).then_some(entry.link))
            .collect();
        let mut grew = false;
        for (index, entry) in entries.iter().enumerate() {
            if entry.link != 0 && links.contains(&entry.link) && !included[index] {
                include_subtree(entries, &mut included, index);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    Ok(included
        .iter()
        .enumerate()
        .filter_map(|(index, selected)| selected.then_some(index))
        .collect())
}

fn include_subtree(entries: &[SetEntry], included: &mut [bool], root: usize) {
    included[root] = true;
    if !entries[root].group {
        return;
    }
    let depth = entries[root].depth;
    for index in (0..root).rev() {
        if entries[index].depth <= depth {
            break;
        }
        included[index] = true;
    }
}
