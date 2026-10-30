//! Layer ▸ Duplicate to New Document:
//! the given entries, each with its whole subtree, copied into a NEW document
//! with the source's canvas, resolution, profile, evaluation policy and global
//! light. ONE call at each depth — `DrDocument::duplicate_to_new_document` at
//! 8 bits, `PrecisionDocument::duplicate_to_new_document` (reached through
//! the `duplicate_to_new_document` structure edit) at 16 bits and 32-bit
//! float — never a host loop over layers.
//!
//! # What is carried
//!
//! Every copied record verbatim — pixels (still shared), offset, mask, style,
//! meta/description, retained source, warp, label, locks, open state — under
//! a freshly minted id, the `duplicating_layer` rule. The roots are the
//! `independent_roots` of the given set, landing at the top level in source
//! paint order (bottom-first at 8 bits, postorder at high depth, which are the
//! same order).
//!
//! Document chrome that belongs to the SOURCE picture is not carried: alpha
//! channels, guides (the ruler origin returns to the canvas's top-left), layer
//! comps and EXIF/XMP/IPTC metadata.
//!
//! # Clipping and links
//!
//! [`plan`] is the one rule both depths share. A copied ROOT keeps `clipped`
//! only when its source base — the nearest unclipped sibling below it in its
//! source level — is also a copied root and every root between that base and
//! it in the new order is a clipped root over the same base. Anything else
//! would clip it to a different layer than it clipped to in the source, so
//! the flag is released. Clipped records INSIDE a copied subtree keep their
//! flag: their level came across whole. A link id is kept only when at least
//! two copied records share it, because a link group of one is no group.
//!
//! `DrDocument::duplicate_to_new_document` (`dr_doc_duplicate_to_new_document`) and
//! `PrecisionDocument::duplicate_to_new_document` (the `{op:"duplicate_to_new_document"}`
//! `StructureEdit`, whose result is a NEW document, never an edit of the source). ONE call at each
//! depth: the given set's `independent_roots` land at the top level in source order with their
//! subtrees, every record carried verbatim under a freshly minted id; canvas, profile `Arc`,
//! resolution, evaluation policy, float policy and global light are the source's, while channels,
//! guides, comps and metadata are dropped and the ruler origin is (0,0). The private `plan` is the
//! one clip/link rule both depths share — a root keeps `clipped` only over a copied base with no
//! foreign root between them, a link id survives only when two copies share it. The typed arm goes
//! `flatten` → slice → `rebuild` → `build`. Tests: `tests/duplicate_to_new_document.rs`.

use std::collections::HashMap;
use std::ops::Range;

use crate::doc::DrDocument;
use crate::doc_group::validated;
use crate::doc_identity::mint_layer_id;
use crate::doc_structure::independent_roots;
use crate::metadata::Metadata;
use crate::precision_document::PrecisionDocument;
use crate::precision_structure_ops::{flatten, rebuild, FlatRecord};

/// What [`plan`] decides, in SOURCE indices.
struct Plan {
    /// Each root's source subtree, in new (source) order.
    ranges: Vec<Range<usize>>,
    /// Roots whose `clipped` flag is released.
    released: Vec<usize>,
    /// Link ids at least two copied records share; every other id is cleared.
    kept_links: Vec<u32>,
}

/// `depths[idx]`'s subtree, `start..idx + 1` — `doc_group::subtree` over a
/// bare depth list, so both depths share this plan.
fn subtree(depths: &[usize], idx: usize) -> Range<usize> {
    let depth = depths[idx];
    let mut start = idx;
    while start > 0 && depths[start - 1] > depth {
        start -= 1;
    }
    start..idx + 1
}

/// The source clip base of `idx`: the nearest UNCLIPPED sibling below it in
/// its level, `None` when there is none (a baseless clip composites as
/// unclipped).
pub(crate) fn clip_base(depths: &[usize], clipped: &[bool], idx: usize) -> Option<usize> {
    let depth = depths[idx];
    let mut i = idx;
    while i > 0 {
        i -= 1;
        // A shallower record below is the level's lower edge: the parent's
        // previous sibling, never a sibling of `idx`.
        if depths[i] < depth {
            return None;
        }
        if depths[i] == depth && !clipped[i] {
            return Some(i);
        }
    }
    None
}

/// The shared plan: `roots` are the independent roots, ascending; `depths`,
/// `clipped` and `links` describe every source record.
fn plan(depths: &[usize], clipped: &[bool], links: &[u32], roots: &[usize]) -> Plan {
    let ranges: Vec<Range<usize>> = roots.iter().map(|&r| subtree(depths, r)).collect();
    let bases: Vec<Option<usize>> = roots
        .iter()
        .map(|&r| {
            if clipped[r] {
                clip_base(depths, clipped, r)
            } else {
                None
            }
        })
        .collect();
    let released = roots
        .iter()
        .enumerate()
        .filter(|&(_, &r)| clipped[r])
        .filter(|&(pos, _)| {
            let Some(base) = bases[pos] else {
                return true;
            };
            let Some(base_pos) = roots.iter().position(|&r| r == base) else {
                return true;
            };
            !(base_pos + 1..pos).all(|between| bases[between] == Some(base))
        })
        .map(|(_, &r)| r)
        .collect();
    let mut counts: HashMap<u32, usize> = HashMap::new();
    for range in &ranges {
        for &link in &links[range.clone()] {
            if link != 0 {
                *counts.entry(link).or_default() += 1;
            }
        }
    }
    let mut kept_links: Vec<u32> = counts
        .into_iter()
        .filter(|&(_, n)| n >= 2)
        .map(|(link, _)| link)
        .collect();
    kept_links.sort_unstable();
    Plan {
        ranges,
        released,
        kept_links,
    }
}

impl DrDocument {
    /// The given entries (subtrees included, subsumed entries dropped) as a
    /// NEW document with this one's canvas, resolution, profile, evaluation
    /// policy and global light — Layer ▸ Duplicate to New Document. Each
    /// copied record keeps every property and mints a fresh id; clipping and
    /// link ids follow the module doc's rule. Channels, guides, comps and
    /// metadata are not carried and the ruler origin is (0, 0).
    ///
    /// `None` for an empty or out-of-range set, or a malformed result. Never
    /// `None` for a valid set.
    pub fn duplicate_to_new_document(&self, idx: &[usize]) -> Option<Self> {
        let roots = independent_roots(&self.layers, idx)?;
        let depths: Vec<usize> = self.layers.iter().map(|l| usize::from(l.depth)).collect();
        let clipped: Vec<bool> = self.layers.iter().map(|l| l.clipped).collect();
        let links: Vec<u32> = self.layers.iter().map(|l| l.link).collect();
        let plan = plan(&depths, &clipped, &links, &roots);
        let mut layers = Vec::new();
        for (range, &root) in plan.ranges.iter().zip(&roots) {
            let shift = self.layers[root].depth;
            for i in range.clone() {
                let source = &self.layers[i];
                let mut layer = source.clone();
                layer.id = mint_layer_id();
                layer.depth = source.depth - shift;
                if !plan.kept_links.contains(&layer.link) {
                    layer.link = 0;
                }
                if i == root && plan.released.contains(&root) {
                    layer.clipped = false;
                }
                layers.push(layer);
            }
        }
        validated(DrDocument {
            width: self.width,
            height: self.height,
            layers,
            global_light: self.global_light,
            channels: Vec::new(),
            guides: Vec::new(),
            ruler_origin: (0.0, 0.0),
            color_samplers: Vec::new(),
            paths: crate::doc_path::DocPaths::default(),
            profile: self.profile.clone(),
            metadata: Metadata::default(),
            resolution: self.resolution,
            linear_light: self.linear_light.clone(),
            comps: Vec::new(),
        })
    }
}

impl PrecisionDocument {
    /// The typed twin of [`DrDocument::duplicate_to_new_document`]: built
    /// through `PrecisionDocument::build` (default metadata, no channels, this
    /// document's float policy) — or, for a CMYK document, `build_cmyk` with
    /// this document's CMYK policy (#1 phase 4c) — then the Gray policy
    /// (#1 phase 4), the Lab policy (#1 Lab phase 4c), the evaluation policy and the global light are copied; comps, guides and the ruler origin keep `build`'s
    /// empty defaults. Refuses an empty or out-of-range set by name.
    pub fn duplicate_to_new_document(&self, idx: &[usize]) -> Result<Self, String> {
        let flat = flatten(&self.layers);
        let roots = crate::precision_structure_ops::independent_roots(&flat, idx)?;
        let depths: Vec<usize> = flat.iter().map(|r| r.depth).collect();
        let clipped: Vec<bool> = flat.iter().map(|r| r.layer.clipped).collect();
        let links: Vec<u32> = flat.iter().map(|r| r.layer.link).collect();
        let plan = plan(&depths, &clipped, &links, &roots);
        let mut copied: Vec<FlatRecord> = Vec::new();
        for (range, &root) in plan.ranges.iter().zip(&roots) {
            let shift = flat[root].depth;
            for i in range.clone() {
                let mut layer = flat[i].layer.clone();
                layer.id = mint_layer_id();
                if !plan.kept_links.contains(&layer.link) {
                    layer.link = 0;
                }
                if i == root && plan.released.contains(&root) {
                    layer.clipped = false;
                }
                copied.push(FlatRecord {
                    layer,
                    depth: flat[i].depth - shift,
                });
            }
        }
        let layers = rebuild(copied)?;
        // #1 phase 4c: a CMYK source's clip is a CMYK document of the
        // source's own policy (its profile and declared depth).
        let mut next = match self.cmyk_policy() {
            Some(policy) => PrecisionDocument::build_cmyk(
                self.width,
                self.height,
                Metadata::default(),
                self.resolution,
                layers,
                Vec::new(),
                policy.clone(),
            )?,
            None => PrecisionDocument::build(
                self.width,
                self.height,
                self.profile.clone(),
                Metadata::default(),
                self.resolution,
                layers,
                Vec::new(),
                self.float.clone(),
            )?,
        };
        // #1 phase 4: a Gray source's clip is a Gray document of the
        // source's own policy (its layers are already in the policy's twin).
        next.gray = self.gray.clone();
        // #1 Lab phase 4c: and a Lab source's a Lab document of its policy.
        next.lab = self.lab.clone();
        next.linear_light = self.linear_light.clone();
        next.global_light = self.global_light;
        Ok(next)
    }
}
