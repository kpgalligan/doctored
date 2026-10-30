//! Conditional `PATH` table: the document's PATHS and its VECTOR
//! MASKS in `.rz`, at both depths, with one framing.
//!
//! Byte word **74** is the FIRST word inside the identity wrapper 45, ahead
//! of every other conditional word (`45 -> (74) -> (73) -> (72) -> (71) ->
//! …`), and typed word **75** the first inside 46 (`46 -> (75) -> (71) ->
//! (70) -> …`). Tables close in reverse, so `PATH` is the LAST table before
//! `LIDS`: byte `… RSPR -> CSMP -> PATH -> LIDS`, typed `… LGLT -> CSMP ->
//! PATH -> LIDS` (after the read-only `SOCT`/`SOSM` tables of an old file).
//! Both 12E-1 invariants hold — 45/46 is still the first word of every file
//! and `LIDS` the last table — so the host's version sniffer and the
//! identity peel are untouched.
//!
//! Wire format, little-endian: `PATH`, u32 schema = 1, u32 JSON length
//! (1…`drdc::MAX_DRDC_META_LEN`), that many bytes of UTF-8 JSON, a u32 plane
//! count, then per plane u32 width, u32 height and the raw coverage (one u8
//! a sample at byte depth, one little-endian u16 at high depth). The JSON is
//!
//! ```text
//! {"paths":[{"name":s,"kind":"work"|"saved", …shape}],
//!  "vector_masks":[{"layer":i,"enabled":b,"linked":b, …shape,
//!     "raster":null|{"plane":i|null,"x":i32,"y":i32,"linked":b,
//!                    "density":f,"feather":f,"outside":0|255,"enabled":b}}]}
//! ```
//!
//! with `doc_path::entry_value`'s entries (no ids) and `path_geometry`'s
//! shape keys. A vector mask names its layer by its bottom-first record
//! index, and only a LIVE one is written (`doc_vector_mask`). The layer's
//! own mask record in the file is the BAKED plane, which is what makes the
//! picture independent of this table; the table adds the shape, the flags
//! and the raster part. A raster part's `plane` indexes the planes after the
//! JSON, each plane named by exactly one raster part; `null` says the raster
//! part IS the layer's stored mask (the slot holds it verbatim, the shape
//! being disabled or empty). The table is written ONLY when the document
//! holds a path or a live vector mask, so any other document is
//! byte-for-byte the file the prior writer made.
//!
//! The reader REFUSES every broken claim — a bad tag or schema, a length of
//! 0 or past the cap, JSON that is not the shape above, an empty table, a
//! shape `PathShape::from_value` refuses, a list `doc_path::list_refusal`
//! refuses (the anchor cap counts the vector-mask shapes), a vector mask
//! naming no layer or a layer twice, a plane count or plane index that does
//! not pair up, and a plane or raster-part property outside the mask rules —
//! before a single PNG decodes. Whether a record AGREES with its layer's
//! stored mask is the depth reader's check, once the layers are known. Path
//! ids are not stored; a reopen mints fresh ones (`doc_path::normalized`).
//! The writer enforces the reader's caps, and
//! `doc_path::MAX_DOCUMENT_PATH_ANCHORS` is sized so no document's JSON
//! exceeds the length cap.
//!
//! An older BYTE build refuses a 74 file as "unsupported DRDC version 74";
//! an older TYPED build refuses a `46, 75` file with its generic "not a
//! supported … high-depth DRDC document" sentence.

use std::collections::HashSet;
use std::sync::Arc;

use image::GrayImage;
use serde_json::{json, Map, Value};

use crate::doc::MAX_PIXELS;
use crate::doc_mask::MaskProperties;
use crate::doc_path::{entries_from_value, entry_value, list_refusal, normalized, PathEntry};
use crate::drdc::{Reader, MAX_DRDC_META_LEN};
use crate::path_geometry::PathShape;

pub(crate) const BYTE_VERSION: u32 = 74;
pub(crate) const PRECISION_VERSION: u32 = 75;

/// The tag, the schema, the JSON length and the plane count.
const FRAMING_BYTES: u64 = 16;
/// `{"paths":[],"vector_masks":[]}` is 30 bytes.
const WRAPPER_BYTES: u64 = 64;
/// One entry's keys, quotes and separators around its name and subpaths
/// (63 bytes with the longest kind and fill rule).
const ENTRY_BYTES: u64 = 96;
/// `{"anchors":[],"closed":false},` is 30 bytes.
const SUBPATH_BYTES: u64 = 30;
/// One anchor with every coordinate at its twelve-character longest
/// (`-999999.9999`), its keys and its separator.
const ANCHOR_BYTES: u64 = 118;
/// One vector-mask record around its subpaths: its own keys (about 90
/// bytes) and a raster part with two eleven-character offsets and two f32s
/// printed as f64 (about 190).
const MASK_BYTES: u64 = 400;
/// A plane's width and height.
const PLANE_FRAMING_BYTES: u64 = 8;

const INVALID_MASKS: &str = "invalid native path table vector mask section";

/// A raster part's coverage, in whichever form its side holds it.
pub(crate) enum TablePlane<'a> {
    /// As a file stores it: `width * height` samples of the table's depth.
    Raw(&'a [u8]),
    /// A byte document's plane.
    Bytes(Arc<GrayImage>),
    /// A typed document's plane.
    Words(Arc<Vec<u16>>),
}

/// A vector mask's raster part as the table stores it.
pub(crate) struct TableRaster<'a> {
    /// Its own plane and that plane's dimensions, or `None` when the raster
    /// part is the layer's stored mask.
    pub(crate) plane: Option<((u32, u32), TablePlane<'a>)>,
    pub(crate) offset: (i32, i32),
    pub(crate) linked: bool,
    pub(crate) density: f32,
    pub(crate) feather: f32,
    pub(crate) outside: u8,
    pub(crate) enabled: bool,
}

impl TableRaster<'_> {
    /// The raster part's mask properties.
    pub(crate) fn properties(&self) -> MaskProperties {
        let mut properties = MaskProperties::at(self.offset);
        properties.linked = self.linked;
        properties.density = self.density;
        properties.feather = self.feather;
        properties.outside_coverage = self.outside;
        properties
    }
}

/// One vector mask as the table stores it.
pub(crate) struct TableMask<'a> {
    /// The layer's bottom-first record index.
    pub(crate) layer: usize,
    pub(crate) shape: PathShape,
    pub(crate) enabled: bool,
    pub(crate) linked: bool,
    pub(crate) raster: Option<TableRaster<'a>>,
}

/// What [`parse`] reads.
pub(crate) struct ParsedTable<'a> {
    pub(crate) entries: Vec<PathEntry>,
    pub(crate) masks: Vec<TableMask<'a>>,
}

impl ParsedTable<'_> {
    /// The table of a file without one.
    pub(crate) fn empty() -> Self {
        ParsedTable {
            entries: Vec::new(),
            masks: Vec::new(),
        }
    }

    /// The pixels of the raster-part planes the table carries: mask planes
    /// the reader's totals must count.
    pub(crate) fn plane_dimensions(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.masks
            .iter()
            .filter_map(|mask| Some(mask.raster.as_ref()?.plane.as_ref()?.0))
    }
}

/// The shape counts of a list and its masks: `(shapes, subpaths, anchors)`.
fn counts(list: &[PathEntry], masks: &[TableMask<'_>]) -> (u64, u64, u64) {
    let shapes = list
        .iter()
        .map(|e| &e.shape)
        .chain(masks.iter().map(|m| &m.shape));
    shapes.fold((0, 0, 0), |(n, s, a), shape| {
        (
            n + 1,
            s + shape.subpaths.len() as u64,
            a + shape.anchor_count() as u64,
        )
    })
}

/// The raw bytes of the planes `masks` carry at `sample_bytes` a sample,
/// with each plane's framing.
fn plane_bytes(masks: &[TableMask<'_>], sample_bytes: u64) -> u64 {
    masks
        .iter()
        .filter_map(|mask| Some(mask.raster.as_ref()?.plane.as_ref()?.0))
        .fold(0u64, |n, (w, h)| {
            n.saturating_add(PLANE_FRAMING_BYTES)
                .saturating_add(u64::from(w) * u64::from(h) * sample_bytes)
        })
}

/// An upper bound on the table's encoded size; 0 when there is nothing to
/// write. It depends only on counts, name lengths and plane dimensions —
/// never on a coordinate's digits — so it also bounds the table of the same
/// list after any geometry op, which is what lets a geometry plan charge the
/// current list. A name is charged six bytes per byte, JSON's longest
/// escape.
pub(crate) fn encoded_bound(list: &[PathEntry], masks: &[TableMask<'_>], sample_bytes: u64) -> u64 {
    if list.is_empty() && masks.is_empty() {
        return 0;
    }
    let (_, subpaths, anchors) = counts(list, masks);
    let names = list.iter().fold(0u64, |n, e| n + 6 * e.name.len() as u64);
    FRAMING_BYTES
        + WRAPPER_BYTES
        + ENTRY_BYTES * list.len() as u64
        + names
        + MASK_BYTES * masks.len() as u64
        + SUBPATH_BYTES * subpaths
        + ANCHOR_BYTES * anchors
        + plane_bytes(masks, sample_bytes)
}

/// What writing or reading the table holds beside the file's own bytes; 0
/// when there is nothing to write. The writer holds the JSON text and the
/// table Vec (each within [`encoded_bound`], planes included, which
/// over-counts the text) and, like the reader, one `serde_json::Value` tree:
/// per anchor an object node of five entries (a 632-byte B-tree leaf), five
/// key strings and two two-element arrays, about 950 bytes, charged as 1024;
/// a subpath's or a shape's own object is one more leaf with its keys,
/// charged as 1024.
pub(crate) fn transient_bound(
    list: &[PathEntry],
    masks: &[TableMask<'_>],
    sample_bytes: u64,
) -> u64 {
    if list.is_empty() && masks.is_empty() {
        return 0;
    }
    let (shapes, subpaths, anchors) = counts(list, masks);
    2 * encoded_bound(list, masks, sample_bytes) + 1024 * (2 * shapes + subpaths + anchors)
}

/// One mask's JSON record; `plane` is the index its raster part's own plane
/// takes.
fn mask_value(mask: &TableMask<'_>, plane: Option<usize>) -> Value {
    let mut value = mask.shape.to_value();
    if let Value::Object(object) = &mut value {
        object.insert("layer".into(), json!(mask.layer));
        object.insert("enabled".into(), json!(mask.enabled));
        object.insert("linked".into(), json!(mask.linked));
        object.insert(
            "raster".into(),
            match &mask.raster {
                None => Value::Null,
                Some(raster) => json!({
                    "plane": plane,
                    "x": raster.offset.0,
                    "y": raster.offset.1,
                    "linked": raster.linked,
                    "density": raster.density,
                    "feather": raster.feather,
                    "outside": raster.outside,
                    "enabled": raster.enabled,
                }),
            },
        );
    }
    value
}

/// Whether `dimensions` can be a mask plane.
fn plane_fits((w, h): (u32, u32)) -> bool {
    w > 0 && h > 0 && u64::from(w) * u64::from(h) <= MAX_PIXELS
}

/// The table, or an empty Vec when there is nothing to write. It re-checks
/// the reader's rules so the writer never emits a file its reader refuses.
/// `sample_bytes` is 1 at byte depth and 2 at high depth.
pub(crate) fn encode(
    list: &[PathEntry],
    masks: &[TableMask<'_>],
    sample_bytes: u64,
) -> Result<Vec<u8>, String> {
    if list.is_empty() && masks.is_empty() {
        return Ok(Vec::new());
    }
    let mask_anchors: usize = masks.iter().map(|m| m.shape.anchor_count()).sum();
    if let Some(reason) = list_refusal(list, mask_anchors) {
        return Err(reason);
    }
    let shapes = list
        .iter()
        .map(|e| &e.shape)
        .chain(masks.iter().map(|m| &m.shape));
    if let Some(reason) = shapes.into_iter().find_map(PathShape::refusal) {
        return Err(reason);
    }
    let paths: Vec<Value> = list.iter().map(|e| entry_value(e, false)).collect();
    let mut planes = Vec::new();
    let mut records = Vec::with_capacity(masks.len());
    for mask in masks {
        let own = mask.raster.as_ref().and_then(|r| r.plane.as_ref());
        if let Some((dimensions, _)) = own {
            let valid = mask
                .raster
                .as_ref()
                .is_some_and(|r| r.properties().valid(*dimensions));
            if !plane_fits(*dimensions) || !valid {
                return Err("a vector mask's raster part is not a valid mask".into());
            }
        }
        records.push(mask_value(mask, own.map(|_| planes.len())));
        if let Some(plane) = own {
            planes.push(plane);
        }
    }
    let text = json!({ "paths": paths, "vector_masks": records }).to_string();
    let length = u32::try_from(text.len())
        .ok()
        .filter(|&length| length <= MAX_DRDC_META_LEN)
        .ok_or("the document's paths exceed the native path table limit")?;
    let bound = text.len() as u64 + FRAMING_BYTES + plane_bytes(masks, sample_bytes);
    let mut output = crate::drdc_save_plan::reserved(bound)?;
    output.extend_from_slice(b"PATH");
    output.extend_from_slice(&1_u32.to_le_bytes());
    output.extend_from_slice(&length.to_le_bytes());
    output.extend_from_slice(text.as_bytes());
    output.extend_from_slice(&(planes.len() as u32).to_le_bytes());
    for ((w, h), plane) in planes {
        let samples = *w as usize * *h as usize;
        output.extend_from_slice(&w.to_le_bytes());
        output.extend_from_slice(&h.to_le_bytes());
        let wrong = || "a vector mask's raster part does not match its dimensions".to_string();
        match plane {
            TablePlane::Raw(bytes) if bytes.len() as u64 == samples as u64 * sample_bytes => {
                output.extend_from_slice(bytes);
            }
            TablePlane::Bytes(image) if sample_bytes == 1 && image.as_raw().len() == samples => {
                output.extend_from_slice(image.as_raw());
            }
            TablePlane::Words(words) if sample_bytes == 2 && words.len() == samples => {
                for word in words.iter() {
                    output.extend_from_slice(&word.to_le_bytes());
                }
            }
            _ => return Err(wrong()),
        }
    }
    Ok(output)
}

fn field<'v>(object: &'v Map<String, Value>, key: &str) -> Result<&'v Value, String> {
    object.get(key).ok_or_else(|| INVALID_MASKS.to_string())
}

fn boolean(object: &Map<String, Value>, key: &str) -> Result<bool, String> {
    field(object, key)?
        .as_bool()
        .ok_or_else(|| INVALID_MASKS.to_string())
}

fn integer(object: &Map<String, Value>, key: &str) -> Result<i64, String> {
    field(object, key)?
        .as_i64()
        .ok_or_else(|| INVALID_MASKS.to_string())
}

fn number(object: &Map<String, Value>, key: &str) -> Result<f32, String> {
    field(object, key)?
        .as_f64()
        .map(|v| v as f32)
        .filter(|v| v.is_finite())
        .ok_or_else(|| INVALID_MASKS.to_string())
}

/// One `vector_masks` record, its raster part's plane still an index.
fn mask_from_value(
    value: &Value,
    layer_count: usize,
) -> Result<(TableMask<'static>, Option<usize>), String> {
    let invalid = || INVALID_MASKS.to_string();
    let object = value.as_object().ok_or_else(invalid)?;
    let layer = usize::try_from(integer(object, "layer")?)
        .ok()
        .filter(|&layer| layer < layer_count)
        .ok_or_else(invalid)?;
    let shape = PathShape::from_value(value)
        .map_err(|reason| format!("invalid native path table: {reason}"))?;
    let mut plane = None;
    let raster = match field(object, "raster")? {
        Value::Null => None,
        Value::Object(raster) => {
            plane = match field(raster, "plane")? {
                Value::Null => None,
                index => Some(
                    index
                        .as_u64()
                        .and_then(|i| usize::try_from(i).ok())
                        .ok_or_else(invalid)?,
                ),
            };
            let offset =
                |key| i32::try_from(integer(raster, key)?).map_err(|_| INVALID_MASKS.to_string());
            let outside = u8::try_from(integer(raster, "outside")?)
                .ok()
                .filter(|v| matches!(v, 0 | 255))
                .ok_or_else(invalid)?;
            Some(TableRaster {
                plane: None,
                offset: (offset("x")?, offset("y")?),
                linked: boolean(raster, "linked")?,
                density: number(raster, "density")?,
                feather: number(raster, "feather")?,
                outside,
                enabled: boolean(raster, "enabled")?,
            })
        }
        _ => return Err(invalid()),
    };
    Ok((
        TableMask {
            layer,
            shape,
            enabled: boolean(object, "enabled")?,
            linked: boolean(object, "linked")?,
            raster,
        },
        plane,
    ))
}

/// Reads the table of a document of `layer_count` records, `sample_bytes` a
/// coverage sample. Decodes no pixel and copies no plane: a raster part's
/// plane borrows the file.
pub(crate) fn parse<'a>(
    r: &mut Reader<'a>,
    layer_count: usize,
    sample_bytes: usize,
) -> Result<ParsedTable<'a>, String> {
    if r.take(4)? != b"PATH" || r.u32()? != 1 {
        return Err("invalid native path table tag or schema".into());
    }
    let length = r.u32()?;
    if length == 0 || length > MAX_DRDC_META_LEN {
        return Err("invalid native path table length".into());
    }
    let value: Value = serde_json::from_slice(r.take(length as usize)?)
        .map_err(|e| format!("invalid native path table json: {e}"))?;
    let invalid = |reason: String| format!("invalid native path table: {reason}");
    let mut list = entries_from_value(
        value
            .get("paths")
            .ok_or_else(|| invalid("no paths array".into()))?,
    )
    .map_err(invalid)?;
    // An id is never read from a file, whatever the JSON holds.
    for entry in &mut list {
        entry.id = 0;
    }
    let records = value
        .get("vector_masks")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("no vector_masks array".into()))?;
    let wrong = || INVALID_MASKS.to_string();
    if records.len() > layer_count {
        return Err(wrong());
    }
    let mut masks: Vec<(TableMask<'a>, Option<usize>)> = Vec::with_capacity(records.len());
    let mut layers = HashSet::new();
    let mut anchors = 0usize;
    for record in records {
        let (mask, plane) = mask_from_value(record, layer_count)?;
        if !layers.insert(mask.layer) {
            return Err(wrong());
        }
        anchors = anchors.saturating_add(mask.shape.anchor_count());
        masks.push((mask, plane));
    }
    // Each plane is named by exactly one raster part.
    let count = r.u32()? as usize;
    let mut named = HashSet::new();
    for (_, plane) in &masks {
        if let Some(index) = plane {
            if *index >= count || !named.insert(*index) {
                return Err(wrong());
            }
        }
    }
    if named.len() != count {
        return Err(wrong());
    }
    let mut planes = Vec::with_capacity(count);
    for _ in 0..count {
        let dimensions = (r.u32()?, r.u32()?);
        if !plane_fits(dimensions) {
            return Err(wrong());
        }
        let bytes = dimensions.0 as usize * dimensions.1 as usize * sample_bytes;
        planes.push((dimensions, r.take(bytes)?));
    }
    let masks = masks
        .into_iter()
        .map(|(mut mask, plane)| {
            if let (Some(raster), Some(index)) = (&mut mask.raster, plane) {
                let (dimensions, bytes) = planes[index];
                if !raster.properties().valid(dimensions) {
                    return Err(wrong());
                }
                raster.plane = Some((dimensions, TablePlane::Raw(bytes)));
            }
            Ok(mask)
        })
        .collect::<Result<Vec<_>, String>>()?;
    if list.is_empty() && masks.is_empty() {
        return Err(invalid("the table is empty".into()));
    }
    if let Some(reason) = list_refusal(&list, anchors) {
        return Err(invalid(reason));
    }
    Ok(ParsedTable {
        entries: normalized(list),
        masks,
    })
}
