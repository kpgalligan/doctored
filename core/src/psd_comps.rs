//! The PSD side of LAYER COMPS (`doc_comps`) at RGB 8 and 16 bits: what a
//! layered import installs from image resource 1065 and each record's `cmls`
//! settings, and what the RGB writers write back. The bytes are
//! `psd_comp_records`'.
//!
//! **Import.** A layer's settings ride in its OWN record, so an entry binds
//! to the layer built from that record (a raster, or a group's opening
//! folder record); no index or `lyid` lookup is involved. Each comp of 1065
//! that records visibility (`capturedInfo` bit 0) becomes a Doctored comp
//! capturing visibility only, with one entry per kept record whose settings
//! name its compID, shown unless that setting's `enab` is false. A record no
//! setting of the comp names gets no entry, so applying the comp hides it,
//! which is `doc_comps`' unrecorded rule. A comp that records no visibility is
//! not imported, and a comp's position and appearance are not imported; each
//! is one loss sentence. The model's caps (`MAX_LAYER_COMPS`, the name and
//! comment lengths, unique names, the `LCMP` payload) narrow with a loss,
//! never refuse. compID 0, the last document state, is no comp.
//!
//! **Export.** The comps that record visibility are written: resource 1065
//! with compIDs 1, 2, … in list order and `capturedInfo` 1, and on every
//! record a `lyid` (its 1-based file position) and a `shmd` whose `cmls`
//! names that `LyrI`, with one setting per written comp that has a bound
//! entry for the layer (an explicit `enab`) and a trailing compID-0 setting
//! with the layer's current visibility, the authored layout. A comp's
//! position and appearance are not written, and a comp that records no
//! visibility is not written at all: the preflight says both. A document with
//! no written comp writes neither block, so its file is the prior writer's.

use crate::doc_comps::{
    refuse_duplicate_name, validated_comment, validated_name, LayerComp, LayerCompEntry,
    MAX_LAYER_COMPS,
};
use crate::psd_comp_records::{
    read_comps, read_settings, write_comps, write_shmd, FileComp, Setting, CAPTURES_APPEARANCE,
    CAPTURES_POSITION, CAPTURES_VISIBILITY,
};
use crate::psd_container::Container;
use crate::psd_export_encode::{Record, ResourceBlock};
use crate::psd_group_records::GroupRecords;

/// The record a built layer came from, in the order the importer built them.
#[derive(Clone, Copy, Debug)]
pub(crate) enum CompSource {
    /// Raster record `index` of `GroupRecords::layers`.
    Layer(usize),
    /// Folder `id` (1-based, `GroupRecords::groups[id - 1]`).
    Group(u32),
}

/// The file's comps after the model's rules, each with its imported name and
/// comment, and the sentences the import report states for the file.
fn file_comps(container: &Container<'_>) -> (Vec<(FileComp, String, String)>, Vec<String>) {
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
    let mut kept: Vec<(FileComp, String, String)> = Vec::new();
    let mut models: Vec<LayerComp> = Vec::new();
    for comp in comps.into_iter().filter(|c| c.id != 0) {
        let label = comp.name.trim().to_string();
        if comp.captured & CAPTURES_VISIBILITY == 0 {
            losses.push(format!("Layer comp “{label}” records no layer visibility, only position or appearance, which Doctored does not import; it is not imported."));
            continue;
        }
        let recorded: Vec<&str> = [
            (CAPTURES_POSITION, "position"),
            (CAPTURES_APPEARANCE, "appearance"),
        ]
        .into_iter()
        .filter(|(bit, _)| comp.captured & bit != 0)
        .map(|(_, what)| what)
        .collect();
        if !recorded.is_empty() {
            losses.push(format!(
                "Layer comp “{label}” also records layer {}, which is not imported; it is imported with its visibility.",
                recorded.join(" and ")
            ));
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
        models.push(LayerComp {
            name: name.clone(),
            comment: comment.clone(),
            captures: VISIBILITY_ONLY,
            entries: Vec::new(),
        });
        kept.push((comp, name, comment));
    }
    (kept, losses)
}

const VISIBILITY_ONLY: crate::doc_comps::CompCaptures = crate::doc_comps::CompCaptures {
    visibility: true,
    position: false,
    appearance: false,
};

fn source_settings<'a>(records: &GroupRecords<'a>, source: CompSource) -> Option<&'a [u8]> {
    match source {
        CompSource::Layer(index) => records.layer_shmd.get(index).copied().flatten(),
        CompSource::Group(id) => records.groups.get(id as usize - 1)?.shmd,
    }
}

/// The import report's comp sentences for an RGB 8- or 16-bit file
/// (module doc): the comp-level ones, then one per record whose settings do
/// not decode. Decodes no pixel.
pub(crate) fn report_losses(container: &Container<'_>, records: &GroupRecords<'_>) -> Vec<String> {
    let (kept, mut losses) = file_comps(container);
    if kept.is_empty() {
        return losses;
    }
    let bodies = records
        .layer_shmd
        .iter()
        .chain(records.groups.iter().map(|g| &g.shmd));
    let unreadable = bodies
        .flatten()
        .filter(|body| read_settings(body).is_err())
        .count();
    if unreadable != 0 {
        losses.push(if unreadable == 1 {
            "1 layer record carries layer comp settings that could not be read; that layer is hidden by every imported comp.".to_string()
        } else {
            format!("{unreadable} layer records carry layer comp settings that could not be read; those layers are hidden by every imported comp.")
        });
    }
    losses
}

/// The comps a layered RGB import installs. `sources[k]` is the record the
/// k-th built layer came from, and `layer(k)` its id and name.
pub(crate) fn imported(
    container: &Container<'_>,
    records: &GroupRecords<'_>,
    sources: &[CompSource],
    layer: impl Fn(usize) -> Option<(u64, String)>,
) -> Vec<LayerComp> {
    let (kept, _) = file_comps(container);
    if kept.is_empty() {
        return Vec::new();
    }
    let settings: Vec<Vec<Setting>> = sources
        .iter()
        .map(|&source| {
            source_settings(records, source)
                .and_then(|body| read_settings(body).ok().flatten())
                .unwrap_or_default()
        })
        .collect();
    let mut comps: Vec<LayerComp> = kept
        .into_iter()
        .map(|(comp, name, comment)| LayerComp {
            name,
            comment,
            captures: VISIBILITY_ONLY,
            entries: settings
                .iter()
                .enumerate()
                .filter_map(|(k, settings)| {
                    let setting = settings.iter().find(|s| s.comps.contains(&comp.id))?;
                    let (layer_id, layer_name) = layer(k)?;
                    Some(LayerCompEntry {
                        layer_id,
                        layer_name,
                        visible: Some(setting.enabled.unwrap_or(true)),
                        offset: None,
                        appearance: None,
                    })
                })
                .collect(),
        })
        .collect();
    // The `LCMP` payload's cap: a comp is a table over every layer, so a
    // file of many comps over many layers can pass it; the trailing comps go.
    while crate::comps_native::payload(&comps, |_| None).map_or(true, |p| {
        p.len() as u64 > crate::comps_native::MAX_COMPS_JSON_LEN
    }) && comps.pop().is_some()
    {}
    comps
}

/// The comps the RGB writers write (module doc), as their indices.
pub(crate) fn written(comps: &[LayerComp]) -> Vec<usize> {
    (0..comps.len())
        .filter(|&i| comps[i].captures.visibility)
        .collect()
}

/// `(written, narrowed)`: how many comps the RGB layered writer writes, and
/// how many of those also record position or appearance it does not write.
pub(crate) fn counts(comps: &[LayerComp]) -> (usize, usize) {
    let written = written(comps);
    let narrowed = written
        .iter()
        .filter(|&&i| comps[i].captures.position || comps[i].captures.appearance)
        .count();
    (written.len(), narrowed)
}

/// Resource 1065 for the written comps, or nothing.
pub(crate) fn resource(comps: &[LayerComp]) -> Vec<ResourceBlock> {
    let written = written(comps);
    if written.is_empty() {
        return Vec::new();
    }
    let list: Vec<FileComp> = written
        .iter()
        .enumerate()
        .map(|(k, &i)| FileComp {
            name: comps[i].name.clone(),
            comment: comps[i].comment.clone(),
            id: k as i32 + 1,
            captured: CAPTURES_VISIBILITY,
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
/// [`stamp_records`] reads them.
pub(crate) fn typed_owners(
    layers: &[crate::precision_document::PrecisionDocumentLayer],
    out: &mut Vec<Option<(u64, bool)>>,
) {
    for layer in layers {
        if let Some(children) = layer.children() {
            out.push(None);
            typed_owners(children, out);
        }
        out.push(Some((layer.id, layer.visible())));
    }
}

/// Adds the `lyid` and `shmd` blocks to every record of a layered file
/// (module doc). `owners[r]` is record `r`'s layer as `(id, visible)`, `None`
/// for a closing divider. A no-op when no comp is written.
pub(crate) fn stamp_records(
    records: &mut [Record],
    owners: &[Option<(u64, bool)>],
    comps: &[LayerComp],
) {
    let written = written(comps);
    if written.is_empty() {
        return;
    }
    for (r, (record, owner)) in records.iter_mut().zip(owners).enumerate() {
        let lyid = r as i32 + 1;
        record
            .blocks
            .insert(0, (*b"lyid", lyid.to_be_bytes().to_vec()));
        let Some((id, visible)) = *owner else {
            continue;
        };
        let mut settings: Vec<Setting> = written
            .iter()
            .enumerate()
            .filter_map(|(k, &i)| {
                let entry = comps[i].entries.iter().find(|e| e.layer_id == id)?;
                Some(Setting {
                    comps: vec![k as i32 + 1],
                    enabled: entry.visible,
                })
            })
            .collect();
        settings.push(Setting {
            comps: vec![0],
            enabled: Some(visible),
        });
        record.blocks.push((*b"shmd", write_shmd(lyid, &settings)));
    }
}
