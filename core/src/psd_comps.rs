//! The PSD side of LAYER COMPS (`doc_comps`) in every mode the PSD codecs
//! carry layers in (the typed ones through `psd_typed_exchange`): what a
//! layered import installs from image resource 1065 and each record's `cmls`
//! settings, and what the writers write back. The bytes, and the CHAIN that
//! reads a record's settings, are `psd_comp_records`'.
//!
//! **Import.** A layer's settings ride in its OWN record, so an entry binds
//! to the layer built from that record (a raster, or a group's opening
//! folder record); no index or `lyid` lookup is involved. Each comp of 1065
//! becomes a Doctored comp with the capture flags `capturedInfo` names, its
//! name and its comment, and one entry per kept record whose settings name
//! its compID: the chain's visibility and its appearance, the chain's
//! opacity, mode and style (`Lefx` and fill, through the record's own style
//! codec). A record no setting of the comp names gets no entry, so applying
//! the comp hides it, which is `doc_comps`' unrecorded rule.
//!
//! **Position is not imported**: a comp that records position keeps its
//! flag and no entry an offset, so applying it moves no layer, and a comp
//! whose records move between comps says so in one sentence. The recorded
//! takes of the `authored_exchange` family measured no pixel layer moving
//! when a comp was applied, in a session or after reopening, by either
//! apply, although the `FXRefPoint`s record every move; the same file
//! written by Doctored moved one layer once, on the first apply. No rule
//! read those readings, so the import keeps the state applying measured on
//! every authored file. A recorded style that does not decode keeps the
//! layer's current style, and a setting the chain does not read is a
//! sentence. A PSD comp holds no Blend If, knockout or Blend Interior
//! Effects as Group (they are the record's own blocks), so an imported
//! comp's style takes those three from its layer and applying it changes
//! none of them. The model's caps (`MAX_LAYER_COMPS`, the name and comment
//! lengths, unique names, the `LCMP` payload) narrow with a loss, never
//! refuse. compID 0, the last document state, is no comp.
//!
//! **The payload cap.** A PSD holds a pattern once and names it from every
//! comp, while the model's entry holds its whole style, so an ordinary file
//! can pass the `LCMP` payload's 8 MiB. The import weighs each comp from the
//! records alone (`psd_comp_limits::weights`: its recorded styles, and
//! `ENTRY_BYTES` an entry, `APPEARANCE_BYTES` more for one with an
//! appearance) and keeps comps in file order while the total fits: a comp
//! that does not fit is imported without its appearance when the rest of it
//! fits, else not at all, each with its sentence. The weight stands in for
//! the payload, which only the built document gives, so [`imported`] still
//! drops trailing comps should the payload itself not fit (layer names past
//! the entry allowance); that last step has no sentence (#13).
//!
//! **Export.** Every comp is written: resource 1065 with compIDs 1, 2, … in
//! list order, `capturedInfo` from the comp's flags and its comment, and on
//! every record a `lyid` (its 1-based file position) and a `shmd` whose
//! `cmls` names that `LyrI`, with one setting per comp that has a bound
//! entry for the layer and a trailing compID-0 setting with the layer's
//! current state. Every setting writes every key (`psd_comp_records`), but
//! `Lefx` on the setting of a comp that records no appearance: the
//! visibility, a pixel layer's recorded offset from its current one (zero
//! when the comp records none), the opacity, fill and mode, and the style
//! through the record's own effects codec and colours (`styles`). What a
//! setting cannot hold is written as the nearest thing and counted for the
//! preflight's sentences (`psd_comp_limits::narrowed`): a style the effects
//! codec cannot write (`psd_layer_effects::bake_reason`) is written as no
//! style; every setting of a record whose style the writer rasterizes into
//! its pixels (`Owner::baked`) is written with no style and 100 % fill, as
//! the record is, and every setting of an adjustment layer's stamp
//! (`Owner::stamped`) with the stamp's own Normal and 100 % besides; and a
//! comp whose recorded Blend If, knockout or interior flag differs from its
//! layer's is written without them, since the file holds only the record's
//! own. The patterns the written comp styles name join the file's pattern
//! table (`psd_comp_limits::recorded_styles`). A document with no comp
//! writes neither block, so its file is the prior writer's.

use crate::blend::BlendMode;
use crate::doc_comps::{
    refuse_duplicate_name, validated_comment, validated_name, CompAppearance, CompCaptures,
    LayerComp, LayerCompEntry, MAX_LAYER_COMPS,
};
use crate::psd_comp_limits::{
    keys_of, parsed, weights, with_layer_keys, Keys, APPEARANCE_BYTES, ENTRY_BYTES,
};
use crate::psd_comp_records::{
    read_comps, settings_unreadable, write_comps, write_shmd, ChainState, FileComp, Setting,
    CAPTURES_APPEARANCE, CAPTURES_POSITION, CAPTURES_VISIBILITY,
};
use crate::psd_container::Container;
use crate::psd_descriptor::Item;
use crate::psd_export_encode::{Record, ResourceBlock};
use crate::psd_group_records::{CompChain, GroupRecords};

/// The record a built layer came from, in the order the importer built them.
#[derive(Clone, Copy, Debug)]
pub(crate) enum CompSource {
    /// Raster record `index` of `GroupRecords::layers`.
    Layer(usize),
    /// Folder `id` (1-based, `GroupRecords::groups[id - 1]`).
    Group(u32),
}

/// What the comps import reads of the layer built from a record.
pub(crate) struct BuiltLayer {
    pub id: u64,
    pub name: String,
    /// The layer's style JSON as a comp records it (`doc_comps`).
    pub style: Option<String>,
}

/// One record's state as the writer reads it: the layer's id, visibility,
/// offset, whether it is a pixel layer, opacity, mode and style JSON.
pub(crate) struct Owner {
    pub id: u64,
    pub visible: bool,
    pub offset: (i32, i32),
    pub pixel: bool,
    pub opacity: f32,
    pub blend: BlendMode,
    pub style: Option<String>,
    /// The writer rasterizes the record's style into its pixels (its bake
    /// decision), so the record writes no effects, fill or ranges.
    pub baked: bool,
    /// The record is the layer's STAMP: an adjustment layer with no PSD
    /// record (or with a style that is rasterized), written as a Normal
    /// pixel layer at 100 % that holds the picture, the layer's own opacity,
    /// mode and style included.
    pub stamped: bool,
}

impl Owner {
    /// The record's pixels hold the layer's style, so no setting writes one.
    pub(crate) fn rasterized(&self) -> bool {
        self.baked || self.stamped
    }
}

/// One kept comp: the file's, its imported name and comment, its flags.
type KeptComp = (FileComp, String, String, CompCaptures);

/// A record's chain state, its style read as JSON (`psd_group_records`).
type State = ChainState<Result<Option<String>, String>>;

/// A writer's comp style as `Lefx` items through its effects codec and
/// colours, `None` for a style the codec refuses (`stamp_records`).
pub(crate) type CompStyles<'a> =
    &'a dyn Fn(&crate::style::LayerStyle) -> Option<Vec<(Vec<u8>, Item)>>;

/// The file's comps after the model's rules, each with its imported name,
/// comment and capture flags, and the sentences the import report states for
/// the file.
fn file_comps(
    container: &Container<'_>,
    records: &GroupRecords<'_>,
) -> (Vec<KeptComp>, Vec<String>) {
    let mut losses = Vec::new();
    let body = match container.resource(1065) {
        Ok(Some(body)) => body,
        _ => return (Vec::new(), losses),
    };
    let comps = match read_comps(body) {
        Ok(comps) => comps,
        Err(reason) => {
            losses.push(format!("The file's layer comps (image resource 1065) could not be read: {reason}; none is imported."));
            return (Vec::new(), losses);
        }
    };
    let mut kept = Vec::new();
    let mut models: Vec<LayerComp> = Vec::new();
    let weights = weights(records);
    let cap = crate::comps_native::MAX_COMPS_JSON_LEN;
    let mut held = 0u64;
    for comp in comps.into_iter().filter(|c| c.id != 0) {
        let label = comp.name.trim().to_string();
        let mut captures = CompCaptures {
            visibility: comp.captured & CAPTURES_VISIBILITY != 0,
            position: comp.captured & CAPTURES_POSITION != 0,
            appearance: comp.captured & CAPTURES_APPEARANCE != 0,
        };
        if captures.is_empty() {
            losses.push(format!(
                "Layer comp “{label}” records nothing; it is not imported."
            ));
            continue;
        }
        if kept.len() >= MAX_LAYER_COMPS {
            losses.push(format!("Layer comp “{label}” is past Doctored's {MAX_LAYER_COMPS}-comp limit; it is not imported."));
            continue;
        }
        let name = match validated_name(&comp.name) {
            Ok(name) => name,
            Err(_) if label.is_empty() => {
                losses.push("A layer comp with no name is not imported.".into());
                continue;
            }
            Err(_) => {
                let mut end = crate::doc_comps::MAX_COMP_NAME_LEN;
                while !label.is_char_boundary(end) {
                    end -= 1;
                }
                losses.push(format!("Layer comp “{label}” has a name longer than {} bytes; it is imported under its first {end} bytes.", crate::doc_comps::MAX_COMP_NAME_LEN));
                label[..end].trim().to_string()
            }
        };
        if refuse_duplicate_name(&models, &name, None).is_err() {
            losses.push(format!("A second layer comp named “{name}” is not imported; comp names are unique in Doctored."));
            continue;
        }
        let comment = match validated_comment(&comp.comment) {
            Ok(comment) => comment,
            Err(_) => {
                let mut end = crate::doc_comps::MAX_COMP_COMMENT_LEN;
                while !comp.comment.is_char_boundary(end) {
                    end -= 1;
                }
                losses.push(format!("Layer comp “{name}” has a comment longer than {} bytes; its first {end} bytes are imported.", crate::doc_comps::MAX_COMP_COMMENT_LEN));
                comp.comment[..end].to_string()
            }
        };
        // The payload cap (module doc), last: a comp another rule drops
        // weighs nothing.
        let (entries, styles) = weights.get(&comp.id).copied().unwrap_or_default();
        let bare = 2 * (name.len() + comment.len()) as u64 + ENTRY_BYTES * (1 + entries);
        let appearance = styles + APPEARANCE_BYTES * entries;
        let whole = bare + if captures.appearance { appearance } else { 0 };
        let mib = cap >> 20;
        if held + whole <= cap {
            held += whole;
        } else if captures.appearance
            && (captures.visibility || captures.position)
            && held + bare <= cap
        {
            losses.push(format!("Layer comp “{name}” is imported without the appearance it records: with it, the file's layer comps are past the {mib} MiB a document's layer comps may hold."));
            captures.appearance = false;
            held += bare;
        } else {
            losses.push(format!("Layer comp “{name}” is not imported: the file's layer comps are past the {mib} MiB a document's layer comps may hold."));
            continue;
        }
        models.push(LayerComp {
            name: name.clone(),
            comment: comment.clone(),
            captures,
            entries: Vec::new(),
        });
        kept.push((comp, name, comment, captures));
    }
    (kept, losses)
}

fn source_chain<'r>(records: &'r GroupRecords<'_>, source: CompSource) -> &'r CompChain {
    static NONE: CompChain = None;
    match source {
        CompSource::Layer(index) => records.layer_comps.get(index).unwrap_or(&NONE),
        CompSource::Group(id) => records
            .group_comps
            .get((id as usize).wrapping_sub(1))
            .unwrap_or(&NONE),
    }
}

/// Each compID's chain state of a decoded chain.
fn states_for(chain: &[State], id: i32) -> Option<&State> {
    chain.iter().find(|state| state.comps.contains(&id))
}

/// The import report's comp sentences for a layered file (module doc): the
/// comp-level ones, one per record whose settings do not decode, and per
/// comp the recorded moves, styles and settings the import does not keep.
/// Decodes no pixel.
pub(crate) fn report_losses(container: &Container<'_>, records: &GroupRecords<'_>) -> Vec<String> {
    let (kept, mut losses) = file_comps(container, records);
    if kept.is_empty() {
        return losses;
    }
    let bodies = records
        .layer_shmd
        .iter()
        .chain(records.groups.iter().map(|g| &g.shmd));
    let unreadable = bodies
        .flatten()
        .filter(|body| settings_unreadable(body))
        .count();
    if unreadable != 0 {
        losses.push(if unreadable == 1 {
            "1 layer record carries layer comp settings that could not be read; that layer is hidden by every imported comp.".to_string()
        } else {
            format!("{unreadable} layer records carry layer comp settings that could not be read; those layers are hidden by every imported comp.")
        });
    }
    for (comp, name, _, captures) in &kept {
        let (mut moved, mut styles, mut unread) = (0usize, 0usize, 0usize);
        for chain in records.layer_comps.iter() {
            let Some(Ok(chain)) = chain else { continue };
            let (Some(state), now) = (states_for(chain, comp.id), states_for(chain, 0)) else {
                continue;
            };
            let at_rest = now.map_or((0.0, 0.0), |s| s.offset);
            if captures.position && state.offset != at_rest {
                moved += 1;
            }
            if captures.appearance && state.style.is_err() {
                styles += 1;
            }
            unread += usize::from(state.unread);
        }
        for chain in records.group_comps.iter() {
            let Some(Ok(chain)) = chain else { continue };
            if let Some(state) = states_for(chain, comp.id) {
                if captures.appearance && state.style.is_err() {
                    styles += 1;
                }
                unread += usize::from(state.unread);
            }
        }
        if moved != 0 {
            losses.push(format!("Layer comp “{name}” records a position for {moved} layer{}; Doctored does not import the positions a PSD's layer comps record, so applying it leaves {} in place.", if moved == 1 { "" } else { "s" }, if moved == 1 { "it" } else { "them" }));
        }
        if styles != 0 {
            losses.push(format!("Layer comp “{name}” records a layer style that could not be read for {styles} layer{}; applying the comp keeps {} current style.", if styles == 1 { "" } else { "s" }, if styles == 1 { "that layer's" } else { "those layers'" }));
        }
        if unread != 0 {
            losses.push(format!("Layer comp “{name}” records settings beyond visibility, position, opacity, fill, mode and style for {unread} layer{}; they are not imported.", if unread == 1 { "" } else { "s" }));
        }
    }
    losses
}

/// The comps a layered import installs. `sources[k]` is the record the
/// k-th built layer came from, and `layer(k)` that layer.
pub(crate) fn imported(
    container: &Container<'_>,
    records: &GroupRecords<'_>,
    sources: &[CompSource],
    layer: impl Fn(usize) -> Option<BuiltLayer>,
) -> Vec<LayerComp> {
    let (kept, _) = file_comps(container, records);
    if kept.is_empty() {
        return Vec::new();
    }
    // Each built layer with its own Blend If, knockout and interior flag,
    // read once: every entry of the layer takes them (module doc).
    let built: Vec<Option<(BuiltLayer, CompSource, Keys)>> = sources
        .iter()
        .enumerate()
        .map(|(k, &source)| {
            layer(k).map(|l| {
                let keys = keys_of(parsed(l.style.as_deref()).as_ref());
                (l, source, keys)
            })
        })
        .collect();
    let mut comps: Vec<LayerComp> = kept
        .into_iter()
        .map(|(comp, name, comment, captures)| LayerComp {
            name,
            comment,
            captures,
            entries: built
                .iter()
                .flatten()
                .filter_map(|(layer, source, keys)| {
                    let Some(Ok(chain)) = source_chain(records, *source) else {
                        return None;
                    };
                    let state = states_for(chain, comp.id)?;
                    Some(entry(layer, keys, state, captures))
                })
                .collect(),
        })
        .collect();
    // The `LCMP` payload's cap itself, after `file_comps`' weighing (module
    // doc): only layer names past the entry allowance can reach it.
    while crate::comps_native::payload(&comps, |_| None).map_or(true, |p| {
        p.len() as u64 > crate::comps_native::MAX_COMPS_JSON_LEN
    }) && comps.pop().is_some()
    {}
    comps
}

/// One layer's entry in an imported comp (module doc).
fn entry(layer: &BuiltLayer, keys: &Keys, state: &State, captures: CompCaptures) -> LayerCompEntry {
    LayerCompEntry {
        layer_id: layer.id,
        layer_name: layer.name.clone(),
        visible: captures.visibility.then_some(state.visible),
        // Position is not imported (module doc).
        offset: None,
        appearance: captures.appearance.then(|| CompAppearance {
            opacity: (state.opacity / 100.0).clamp(0.0, 1.0) as f32,
            blend: state.blend.to_c() as u32,
            style: match &state.style {
                Ok(style) => with_layer_keys(style.as_deref(), keys),
                Err(_) => layer.style.clone(),
            },
        }),
    }
}

/// Resource 1065 for the written comps, or nothing.
pub(crate) fn resource(comps: &[LayerComp]) -> Vec<ResourceBlock> {
    if comps.is_empty() {
        return Vec::new();
    }
    let list: Vec<FileComp> = comps
        .iter()
        .enumerate()
        .map(|(k, comp)| FileComp {
            name: comp.name.clone(),
            comment: comp.comment.clone(),
            id: k as i32 + 1,
            captured: (if comp.captures.visibility {
                CAPTURES_VISIBILITY
            } else {
                0
            }) | (if comp.captures.position {
                CAPTURES_POSITION
            } else {
                0
            }) | (if comp.captures.appearance {
                CAPTURES_APPEARANCE
            } else {
                0
            }),
        })
        .collect();
    vec![ResourceBlock {
        id: 1065,
        name: String::new(),
        body: write_comps(&list),
    }]
}

/// The typed record walk's owners (`psd_export16::records`' FILE order: a
/// group's closer, its children, then its folder record), as
/// [`stamp_records`] reads them. `written` is the writer's decision for a
/// layer as `(Owner::baked, Owner::stamped)`; `styled` is whether the
/// document has a comp to write, so a style is read at all.
pub(crate) fn typed_owners(
    layers: &[crate::precision_document::PrecisionDocumentLayer],
    written: &dyn Fn(&crate::precision_document::PrecisionDocumentLayer) -> (bool, bool),
    styled: bool,
    out: &mut Vec<Option<Owner>>,
) {
    for layer in layers {
        if let Some(children) = layer.children() {
            out.push(None);
            typed_owners(children, written, styled, out);
        }
        let (baked, stamped) = written(layer);
        out.push(Some(Owner {
            id: layer.id,
            visible: layer.visible(),
            offset: layer.offset(),
            pixel: layer.is_pixel_leaf(),
            opacity: layer.opacity(),
            blend: layer.blend_mode(),
            style: styled.then(|| layer.reported_style()).flatten(),
            baked,
            stamped,
        }));
    }
}

/// A comp's recorded style as its fill percent and `Lefx` items through
/// `styles` (the writer's effects codec and colours): 100 % and no item for
/// no style, no item for a style the codec refuses, and both for any style
/// of a `rasterized` record, whose pixels hold it (module doc).
fn style_setting(
    style: Option<&str>,
    rasterized: bool,
    styles: CompStyles<'_>,
) -> (f64, Vec<(Vec<u8>, Item)>) {
    let Some(style) = parsed(style).filter(|_| !rasterized) else {
        return (100.0, Vec::new());
    };
    let fill = f64::from(style.fill_opacity) * 100.0;
    (fill, styles(&style).unwrap_or_default())
}

/// Adds the `lyid` and `shmd` blocks to every record of a layered file
/// (module doc). `owners[r]` is record `r`'s layer, `None` for a closing
/// divider; `styles` writes a style as `Lefx` items. A no-op when the
/// document has no comp.
pub(crate) fn stamp_records(
    records: &mut [Record],
    owners: &[Option<Owner>],
    comps: &[LayerComp],
    styles: CompStyles<'_>,
) {
    if comps.is_empty() {
        return;
    }
    for (r, (record, owner)) in records.iter_mut().zip(owners).enumerate() {
        let lyid = r as i32 + 1;
        record
            .blocks
            .insert(0, (*b"lyid", lyid.to_be_bytes().to_vec()));
        let Some(owner) = owner else {
            continue;
        };
        let (fill, effects) = style_setting(owner.style.as_deref(), owner.rasterized(), styles);
        // A stamp is Normal at 100 % whatever its layer is, in every setting.
        let current = if owner.stamped {
            (100.0, 100.0, BlendMode::Normal)
        } else {
            (f64::from(owner.opacity) * 100.0, fill, owner.blend)
        };
        let mut settings: Vec<Setting> = comps
            .iter()
            .enumerate()
            .filter_map(|(k, comp)| {
                let entry = comp.entries.iter().find(|e| e.layer_id == owner.id)?;
                let offset = entry
                    .offset
                    .filter(|_| owner.pixel)
                    .map_or((0.0, 0.0), |at| {
                        (
                            f64::from(at.0) - f64::from(owner.offset.0),
                            f64::from(at.1) - f64::from(owner.offset.1),
                        )
                    });
                let (blend, effects) = match &entry.appearance {
                    Some(_) if owner.stamped => (current, Some(Vec::new())),
                    Some(appearance) => {
                        let (fill, effects) =
                            style_setting(appearance.style.as_deref(), owner.rasterized(), styles);
                        let mode =
                            BlendMode::from_c(appearance.blend as i32).unwrap_or(BlendMode::Normal);
                        (
                            (f64::from(appearance.opacity) * 100.0, fill, mode),
                            Some(effects),
                        )
                    }
                    None => (current, None),
                };
                Some(Setting {
                    comps: vec![k as i32 + 1],
                    enabled: entry.visible.unwrap_or(owner.visible),
                    offset: Some(offset),
                    blend: Some(blend),
                    effects,
                })
            })
            .collect();
        settings.push(Setting {
            comps: vec![0],
            enabled: owner.visible,
            offset: Some((0.0, 0.0)),
            blend: Some(current),
            effects: Some(effects),
        });
        record
            .blocks
            .push((*b"shmd", write_shmd(lyid, (0.0, 0.0), settings)));
    }
}

/// [`stamp_records`]' `styles` for a writer: `style` as the `Lefx` items of
/// `psd_layer_effects::effect_items` under the document's `light` and the
/// writer's `colours`, or `None` for a style the codec refuses.
pub(crate) fn effect_style_items(
    style: &crate::style::LayerStyle,
    light: crate::style::GlobalLight,
    colours: crate::psd_layer_effects::FxColours<'_>,
) -> Option<Vec<(Vec<u8>, Item)>> {
    crate::psd_layer_effects::effect_items(style, Some(light), colours).ok()
}
