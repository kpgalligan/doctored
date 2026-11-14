//! PATHS — the document's list of named Bézier paths, on the same
//! shelf as the guides and the colour samplers (`doc_guide`, `doc_sampler`):
//! canvas-space document state that never composites, is undoable, is saved
//! in `.rz` (`path_native`) and is moved by every canvas-geometry op the way
//! the pixels move. The geometry of one path is `path_geometry`'s.
//!
//! **The list.** `DrDocument.paths` / `PrecisionDocument.paths` hold the
//! entries (`DocPaths` here, `precision_path::TypedPaths` at high depth). An
//! entry is a [`PathEntry`]: a process-unique id, a name, a kind and a shape.
//! At most one entry is the WORK path — the unnamed scratch path the Pen
//! draws into — and it is always first and always named [`WORK_PATH_NAME`];
//! every other entry is SAVED and has its own unique name. The list is set
//! whole by the pure `with_paths` (add, edit, rename, reorder, delete and
//! clear are one call), which answers `None` for a list a rule refuses and
//! for the list the document already holds. [`list_refusal`] is the one home
//! of the rules and of their sentences; the FFI setters report them.
//!
//! **The clipping path.** At most one SAVED entry may be the document's
//! clipping path ([`ClippingPath`]), the designation image resource 2999
//! names by the path's name (`psd_paths`). It rides on the entry, so it is
//! kept, moved and removed with its path by every edit below, and it is
//! stored in the entry's JSON (`clipping`).
//!
//! **Identity.** An id is kept across every edit, geometry op, undo and depth
//! conversion, so a host's current-path selection stays attached to a path
//! rather than to a position in the list. An entry arriving with id 0 (a new
//! path) or with an id an earlier entry of the same list already has is
//! minted a fresh one. Ids are not persisted: a reopened document mints new
//! ones, as it does for guides.
//!
//! **The geometry rule.** A path point is a point of the canvas's continuous
//! coordinate space, so every whole-canvas op maps each point and handle
//! through the map `doc_sampler` applies to a sampler's pixel centre at that
//! same call site: quarter turns and flips ([`geometry_paths`]), Crop
//! ([`cropped_paths`]), Canvas Size ([`padded_paths`]), Image Size
//! ([`resized_paths`]), the filtered whole-canvas turns ([`matrix_paths`])
//! and Perspective Crop ([`mapped`] with its destination map). Unlike a
//! sampler a path is NEVER dropped and never clamped into the canvas: a path
//! a crop leaves outside the window is still the user's drawing, and it may
//! lie partly or wholly off the canvas at any time. The result is not rounded
//! to pixels; it is stored the way `path_geometry` stores every coordinate
//! (four decimals, inside the ±1,000,000 wall). A per-layer transform moves
//! nothing about the canvas, so it leaves the paths alone. Each of those
//! call sites but Image Size also takes the path records no open read out
//! of the document's 8BIM run (`tiff_paths::canvas_moved`).
//! `precision_path::edited_paths` is the typed geometry fold's one
//! dispatcher over a `GeometryEdit`, and the byte Straighten arm's.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::doc::{DrDocument, Geometry};
use crate::path_geometry::PathShape;

/// The most paths a document may hold. A path is not multiplied by the
/// canvas; what the cap bounds is the Paths panel's list and the names the
/// agent must tell apart.
pub(crate) const MAX_PATHS: usize = 256;

/// The work path's name, at every depth and in every file. A saved path may
/// not take it, so the name always identifies the work path.
pub const WORK_PATH_NAME: &str = "Work Path";

/// The longest saved-path name, in UTF-8 bytes.
pub(crate) const MAX_PATH_NAME_BYTES: usize = 255;

/// The most anchors a document may hold over EVERY path and every live
/// vector-mask shape together.
///
/// Sized so `path_native`'s JSON always fits its length slot: 65,536 anchors
/// × 148 bytes — the worst-case encoded anchor is 118 bytes (six coordinates
/// of at most twelve characters, the keys and a separator) and each anchor
/// can cost one more 30-byte subpath wrapper, since a subpath holds at least
/// one — is 9.25 MiB, and the names and mask records of a full list add
/// under 1 MiB, which stays below `drdc::MAX_DRDC_META_LEN` (16 MiB). The
/// writer therefore never refuses a document `with_paths` built.
pub(crate) const MAX_DOCUMENT_PATH_ANCHORS: usize = 65_536;

/// Which of the two kinds of entry a path is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    /// The one unnamed scratch path, always first.
    Work,
    /// A named path.
    Saved,
}

/// One path of the list. Equality compares the identity as well as the
/// content, which is what `with_paths`' "is this the same list?" test wants.
#[derive(Clone, Debug, PartialEq)]
pub struct PathEntry {
    /// Process-unique and stable across edits; 0 on the way IN means "mint
    /// one". Never persisted.
    pub id: u64,
    pub name: String,
    pub kind: PathKind,
    pub shape: PathShape,
    /// `Some` when this saved path is the document's CLIPPING PATH (module
    /// doc).
    pub clipping: Option<ClippingPath>,
}

/// The longest [`ClippingPath::tail`], in bytes.
pub(crate) const MAX_CLIPPING_TAIL_BYTES: usize = 64;

/// The CLIPPING PATH designation: which saved path a file names in image
/// resource 2999 (`psd_paths`), on the entry itself so that a rename, a
/// delete, a geometry op, undo and a depth conversion carry it with the
/// path. `tail` is the bytes a file's resource 2999 holds after the path's
/// name (its flatness and fill word, `psd_path_records`), kept verbatim and
/// written back after it; it is empty for a designation made in Doctored
/// and for a file's no-flatness tail, both written as that tail.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClippingPath {
    pub tail: Vec<u8>,
}

/// The byte document's path state: ONE field of `DrDocument`, so every carry
/// site carries the whole of it with one line.
#[derive(Clone, Debug, Default)]
pub(crate) struct DocPaths {
    /// The list, the work path first when there is one. `Arc`-shared, like a
    /// layer's pixels: every edit clones the document, and an edit that does
    /// not touch the paths must not copy up to 65,536 anchors to do it.
    pub(crate) entries: Arc<Vec<PathEntry>>,
    /// The VECTOR MASK records (`doc_vector_mask`), one per layer at most
    /// and each bound to its layer by id. A record counts only while it is
    /// LIVE (its layer exists and still holds the plane the record baked);
    /// a dead one rides along until the next vector-mask write prunes it,
    /// and is never saved.
    pub(crate) vector_masks: Arc<Vec<crate::doc_vector_mask::VectorMaskRecord>>,
}

impl DocPaths {
    /// The same state holding `entries` instead; the vector-mask records
    /// are carried.
    pub(crate) fn with_entries(&self, entries: Vec<PathEntry>) -> Self {
        DocPaths {
            entries: Arc::new(entries),
            vector_masks: Arc::clone(&self.vector_masks),
        }
    }

    /// Replaces the entries in place — for a result document still being
    /// built.
    pub(crate) fn set_entries(&mut self, entries: Vec<PathEntry>) {
        self.entries = Arc::new(entries);
    }

    /// The same state with its entries through one of the point maps below —
    /// the form a geometry op's struct literal takes.
    pub(crate) fn moved(&self, map: impl FnOnce(&[PathEntry]) -> Vec<PathEntry>) -> Self {
        self.with_entries(map(self.entries.as_slice()))
    }
}

/// The id counter behind [`PathEntry::id`]. Starts at 1, so 0 is free to mean
/// "no id yet". `Relaxed` is enough: nothing is published through the
/// counter, only made distinct.
static NEXT_PATH_ID: AtomicU64 = AtomicU64::new(1);

/// The document anchor cap's refusal, shared by [`list_refusal`] and the
/// list reader.
pub(crate) fn anchor_cap_sentence() -> String {
    format!("too many path anchors in the document (max {MAX_DOCUMENT_PATH_ANCHORS})")
}

/// Why a document may not hold `list`, or `None` — the ONE home of the list
/// rules: at most [`MAX_PATHS`] entries; at most one work path; every saved
/// name non-empty, at most [`MAX_PATH_NAME_BYTES`] bytes, not
/// [`WORK_PATH_NAME`] and unique; at most one clipping path, never the work
/// path (a file names the clipping path by its saved name), its tail at
/// most [`MAX_CLIPPING_TAIL_BYTES`]; and the list's anchors plus
/// `vector_mask_anchors` (the document's live vector-mask shapes) at most
/// [`MAX_DOCUMENT_PATH_ANCHORS`]. The work path's own name is not a rule: it
/// is always written as [`WORK_PATH_NAME`].
pub(crate) fn list_refusal(list: &[PathEntry], vector_mask_anchors: usize) -> Option<String> {
    if list.len() > MAX_PATHS {
        return Some(format!("too many paths (max {MAX_PATHS})"));
    }
    if list.iter().filter(|e| e.kind == PathKind::Work).count() > 1 {
        return Some("a document holds at most one work path".into());
    }
    if list.iter().filter(|e| e.clipping.is_some()).count() > 1 {
        return Some("a document holds at most one clipping path".into());
    }
    if list
        .iter()
        .any(|e| e.kind == PathKind::Work && e.clipping.is_some())
    {
        return Some("the work path cannot be the clipping path; save it first".into());
    }
    if list.iter().any(|e| {
        e.clipping
            .as_ref()
            .is_some_and(|c| c.tail.len() > MAX_CLIPPING_TAIL_BYTES)
    }) {
        return Some(format!(
            "a clipping path's resource bytes after its name are longer than {MAX_CLIPPING_TAIL_BYTES} bytes"
        ));
    }
    let mut names = HashSet::new();
    for entry in list.iter().filter(|e| e.kind == PathKind::Saved) {
        if entry.name.is_empty() {
            return Some("a saved path needs a name".into());
        }
        if entry.name.len() > MAX_PATH_NAME_BYTES {
            return Some(format!(
                "a path name is longer than {MAX_PATH_NAME_BYTES} bytes"
            ));
        }
        if entry.name == WORK_PATH_NAME {
            return Some(format!("a saved path cannot be named \"{WORK_PATH_NAME}\""));
        }
        if !names.insert(entry.name.as_str()) {
            return Some(format!("two paths are named \"{}\"", entry.name));
        }
    }
    let anchors = list.iter().fold(vector_mask_anchors, |n, e| {
        n.saturating_add(e.shape.anchor_count())
    });
    (anchors > MAX_DOCUMENT_PATH_ANCHORS).then(anchor_cap_sentence)
}

/// `list` in the model's canonical form: the work path first and named
/// [`WORK_PATH_NAME`], and every entry holding an id no other entry of the
/// list has — an id of 0 and a repeated id are minted fresh. The counter is
/// first moved past every id the list brought, so a minted id can never land
/// on one a later entry keeps.
pub(crate) fn normalized(mut list: Vec<PathEntry>) -> Vec<PathEntry> {
    if let Some(at) = list.iter().position(|e| e.kind == PathKind::Work) {
        if at != 0 {
            let work = list.remove(at);
            list.insert(0, work);
        }
    }
    for entry in &list {
        NEXT_PATH_ID.fetch_max(entry.id.saturating_add(1), Ordering::Relaxed);
    }
    let mut seen = HashSet::new();
    for entry in &mut list {
        if entry.kind == PathKind::Work && entry.name != WORK_PATH_NAME {
            entry.name = WORK_PATH_NAME.to_string();
        }
        if entry.id == 0 || !seen.insert(entry.id) {
            entry.id = NEXT_PATH_ID.fetch_add(1, Ordering::Relaxed);
            seen.insert(entry.id);
        }
    }
    list
}

/// `list` as a document holding `current` may take it: every shape stored
/// the way `path_geometry` stores one, the list [`normalized`]. `None` —
/// never a repair — for a shape `PathShape::refusal` refuses, a list
/// [`list_refusal`] refuses, and the list the document already holds.
pub(crate) fn accepted(
    list: Vec<PathEntry>,
    current: &[PathEntry],
    vector_mask_anchors: usize,
) -> Option<Vec<PathEntry>> {
    let list = list
        .into_iter()
        .map(|entry| {
            Some(PathEntry {
                shape: entry.shape.validated().ok()?,
                ..entry
            })
        })
        .collect::<Option<Vec<_>>>()?;
    if list_refusal(&list, vector_mask_anchors).is_some() {
        return None;
    }
    let list = normalized(list);
    (list != current).then_some(list)
}

// -------------------------------------------------------------------- JSON --

/// One entry as JSON: the shape's own keys beside `name` and `kind`, `id`
/// when `with_id` (the FFI form; `.rz` stores none), and for the clipping
/// path `"clipping":{"tail":"<lowercase hex>"}` (absent on every other
/// entry, so a list with no clipping path is the prior JSON).
pub(crate) fn entry_value(entry: &PathEntry, with_id: bool) -> Value {
    let mut value = entry.shape.to_value();
    if let Value::Object(object) = &mut value {
        if with_id {
            object.insert("id".into(), json!(entry.id));
        }
        object.insert("name".into(), json!(entry.name));
        object.insert(
            "kind".into(),
            json!(match entry.kind {
                PathKind::Work => "work",
                PathKind::Saved => "saved",
            }),
        );
        if let Some(clipping) = &entry.clipping {
            let tail: String = clipping.tail.iter().map(|b| format!("{b:02x}")).collect();
            object.insert("clipping".into(), json!({ "tail": tail }));
        }
    }
    value
}

/// An entry's `clipping` key: absent or null is not the clipping path; an
/// object is, its `tail` (hex, absent for none) the resource's bytes after
/// the name.
fn clipping_from_value(v: Option<&Value>) -> Result<Option<ClippingPath>, String> {
    let wrong = || "a path's clipping must be an object with a hex tail".to_string();
    let object = match v {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Object(object)) => object,
        Some(_) => return Err(wrong()),
    };
    let text = match object.get("tail") {
        None | Some(Value::Null) => "",
        Some(Value::String(text)) => text.as_str(),
        Some(_) => return Err(wrong()),
    };
    // The length cap is `list_refusal`'s, with its own sentence.
    if text.len() % 2 != 0 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(wrong());
    }
    let tail = (0..text.len())
        .step_by(2)
        .map(|at| {
            text.get(at..at + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
        })
        .collect::<Option<Vec<u8>>>()
        .ok_or_else(wrong)?;
    Ok(Some(ClippingPath { tail }))
}

/// One entry out of JSON. `kind` is required; `name` may be absent on the
/// work path; an absent `id` is 0.
fn entry_from_value(v: &Value) -> Result<PathEntry, String> {
    let object = v.as_object().ok_or("a path must be an object")?;
    let kind = match object.get("kind").and_then(Value::as_str) {
        Some("work") => PathKind::Work,
        Some("saved") => PathKind::Saved,
        _ => return Err("a path's kind must be work or saved".into()),
    };
    let name = match object.get("name") {
        None if kind == PathKind::Work => WORK_PATH_NAME.to_string(),
        Some(Value::String(name)) => name.clone(),
        _ => return Err("a path's name must be a string".into()),
    };
    let id = match object.get("id") {
        None | Some(Value::Null) => 0,
        Some(id) => id
            .as_u64()
            .ok_or("a path's id must be a non-negative integer")?,
    };
    Ok(PathEntry {
        id,
        name,
        kind,
        shape: PathShape::from_value(v)?,
        clipping: clipping_from_value(object.get("clipping"))?,
    })
}

/// A `paths` array out of JSON. The count is checked before an entry is
/// read and the anchor total as it grows, so a hostile list is refused
/// before it is built; both answer [`list_refusal`]'s sentences.
pub(crate) fn entries_from_value(v: &Value) -> Result<Vec<PathEntry>, String> {
    let list = v.as_array().ok_or("paths must be an array")?;
    if list.len() > MAX_PATHS {
        return Err(format!("too many paths (max {MAX_PATHS})"));
    }
    let mut entries = Vec::with_capacity(list.len());
    let mut anchors = 0usize;
    for value in list {
        let entry = entry_from_value(value)?;
        anchors += entry.shape.anchor_count();
        if anchors > MAX_DOCUMENT_PATH_ANCHORS {
            return Err(anchor_cap_sentence());
        }
        entries.push(entry);
    }
    Ok(entries)
}

/// The FFI's path-list JSON, `{"paths":[{"id":…,"name":…,"kind":…, …shape}]}`.
pub(crate) fn list_json(list: &[PathEntry]) -> String {
    let paths: Vec<Value> = list.iter().map(|e| entry_value(e, true)).collect();
    json!({ "paths": paths }).to_string()
}

/// [`list_json`]'s reader.
pub(crate) fn list_from_json(text: &str) -> Result<Vec<PathEntry>, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| format!("paths json is malformed: {e}"))?;
    entries_from_value(value.get("paths").ok_or("paths json needs a paths array")?)
}

// -------------------------------------------------------------- point maps --

/// The shared body of every helper below: each entry's shape through `map`
/// (`PathShape::mapped`), ids, names and order kept.
pub(crate) fn mapped(list: &[PathEntry], map: impl Fn(f64, f64) -> (f64, f64)) -> Vec<PathEntry> {
    list.iter()
        .map(|entry| PathEntry {
            shape: entry.shape.mapped(&map),
            ..entry.clone()
        })
        .collect()
}

/// Every path through one exact whole-document transform. `w` and `h` are
/// the OLD canvas. The five maps are `doc_sampler::geometry_samplers`': R90
/// `(H − y, x)`, R180 `(W − x, H − y)`, R270 `(y, W − x)`, FlipH
/// `(W − x, y)`, FlipV `(x, H − y)`.
pub(crate) fn geometry_paths(list: &[PathEntry], geom: Geometry, w: u32, h: u32) -> Vec<PathEntry> {
    let (fw, fh) = (f64::from(w), f64::from(h));
    mapped(list, |x, y| match geom {
        Geometry::Rotate90 => (fh - y, x),
        Geometry::Rotate180 => (fw - x, fh - y),
        Geometry::Rotate270 => (y, fw - x),
        Geometry::FlipH => (fw - x, y),
        Geometry::FlipV => (x, fh - y),
    })
}

/// Crop: shifts by `(−x, −y)`. Nothing is dropped.
pub(crate) fn cropped_paths(list: &[PathEntry], x: u32, y: u32) -> Vec<PathEntry> {
    let (dx, dy) = (f64::from(x), f64::from(y));
    mapped(list, |px, py| (px - dx, py - dy))
}

/// Canvas Size: shifts by `origin`, where the old canvas's top-left lands in
/// the new one.
pub(crate) fn padded_paths(list: &[PathEntry], origin: (i32, i32)) -> Vec<PathEntry> {
    let (dx, dy) = (f64::from(origin.0), f64::from(origin.1));
    mapped(list, |px, py| (px + dx, py + dy))
}

/// Image Size: scales by the canvas's own factors.
pub(crate) fn resized_paths(list: &[PathEntry], fx: f64, fy: f64) -> Vec<PathEntry> {
    mapped(list, |px, py| (px * fx, py * fy))
}

/// A filtered whole-canvas turn: each point through the canvas matrix `m`
/// (`[a, b, c, d, tx, ty]`, `Affine::apply`'s convention), then into the
/// window whose top-left is `window` in the matrix's output canvas. Rotate
/// Canvas passes its `RotatedCanvas::matrix` and `(0, 0)`; Straighten its
/// matrix and its crop window's origin.
pub(crate) fn matrix_paths(list: &[PathEntry], m: [f64; 6], window: (i32, i32)) -> Vec<PathEntry> {
    let (wx, wy) = (f64::from(window.0), f64::from(window.1));
    mapped(list, |x, y| {
        (
            m[0] * x + m[2] * y + m[4] - wx,
            m[1] * x + m[3] * y + m[5] - wy,
        )
    })
}

impl DrDocument {
    /// The document's paths, the work path first when there is one.
    pub fn paths(&self) -> &[PathEntry] {
        &self.paths.entries
    }

    /// The same document holding `list` instead ([`accepted`]). `None` —
    /// never an identical copy — for a refused list and for the list the
    /// document already holds.
    pub fn with_paths(&self, list: Vec<PathEntry>) -> Option<Self> {
        let entries = accepted(list, &self.paths.entries, self.vector_mask_anchor_count())?;
        let mut next = Self {
            paths: self.paths.with_entries(entries),
            ..self.clone()
        };
        // A path write is where dead vector-mask records are dropped.
        next.prune_vector_masks();
        Some(next)
    }
}
