//! C FFI for paths — the "Paths" section of
//! `include/doctored_core.h`, byte exports first and their typed twins
//! after them, over `doc_path` / `precision_path`. The list crosses whole as
//! the path-list JSON `doc_path::list_json` writes and
//! `doc_path::list_from_json` reads.
//!
//! The SHAPE exports after them take no document: one shape as the shape
//! JSON, over the kernels `path_geometry` (coverage, flatten, hit, insert,
//! convert) and `path_trace`. The two that fill a plane or read one
//! (`dr_path_coverage`, `dr_path_trace`) are admitted against the caller's
//! working limit and take a job control; the rest are bounded by the shape
//! caps alone.
//!
//! The VECTOR MASK exports live in `ffi_vector_mask`, which reads its shape
//! JSON through this file's `shape` and returns text through `owned`.

use std::ffi::{c_char, CString};
use std::fmt::Write as _;
use std::ptr;

use serde_json::json;

use crate::doc::DrDocument;
use crate::doc_path::{list_from_json, list_json, list_refusal, PathEntry, MAX_PATHS};
use crate::ffi_util::{
    borrow_cstr_bounded, boxed, doc_get, fallible_doc_op, fallible_op, handle_get,
};
use crate::job_control::{DrJobControl, JobContext};
use crate::landmark_region::product;
use crate::path_geometry::{coverage_scratch, PathHit, PathShape};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, BOOKKEEPING_BYTES};
use crate::style_json::q4_f64;

/// The longest path-list JSON a setter reads. A full document's list is
/// under 11 MiB at four decimals; a host that prints every coordinate at
/// full f64 precision stays under 32 MiB. The cap bounds the parse before
/// any of it is read.
const LIST_JSON_LIMIT: usize = 64 * 1024 * 1024;

/// The longest shape JSON a shape export reads: one shape at its anchor cap
/// is under 5 MiB at four decimals and under 16 MiB at full f64 precision.
const SHAPE_JSON_LIMIT: usize = 16 * 1024 * 1024;

/// What parsing a shape JSON of `bytes` bytes may hold while it is read: the
/// JSON tree and the shape built from it. The tersest anchor, `[x,y],`, is
/// about ten bytes of text and about 200 of tree and model; 24 a byte covers
/// it, and over-covers a verbose anchor.
pub(crate) fn shape_scratch(bytes: usize) -> u64 {
    bytes as u64 * 24 + BOOKKEEPING_BYTES
}

/// The caller's shape JSON as a shape.
///
/// # Safety
/// `json` must be NULL or a NUL-terminated string.
pub(crate) unsafe fn shape(json: *const c_char) -> Result<PathShape, String> {
    let text = unsafe { borrow_cstr_bounded(json, "path shape json", SHAPE_JSON_LIMIT) }?;
    parsed(text)
}

fn parsed(text: &str) -> Result<PathShape, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("path shape json is malformed: {e}"))?;
    PathShape::from_value(&value)
}

/// Text as an owned C string, or NULL when it cannot be one.
pub(crate) fn owned(text: String) -> *mut c_char {
    CString::new(text).map_or(ptr::null_mut(), CString::into_raw)
}

/// A path list as an owned C string, or NULL when it cannot be one.
fn json_string(list: &[PathEntry]) -> *mut c_char {
    CString::new(list_json(list)).map_or(ptr::null_mut(), CString::into_raw)
}

/// The caller's list, with `list_refusal`'s sentence as the error when a
/// list rule or cap refuses it against `vector_mask_anchors`.
///
/// # Safety
/// `json` must be NULL or a NUL-terminated string.
unsafe fn requested(
    json: *const c_char,
    vector_mask_anchors: usize,
) -> Result<Vec<PathEntry>, String> {
    let text = unsafe { borrow_cstr_bounded(json, "paths json", LIST_JSON_LIMIT) }?;
    let list = list_from_json(text)?;
    match list_refusal(&list, vector_mask_anchors) {
        Some(reason) => Err(reason),
        None => Ok(list),
    }
}

/// The most paths a document may hold (`doc_path::MAX_PATHS`).
#[no_mangle]
pub extern "C" fn dr_max_paths() -> usize {
    MAX_PATHS
}

/// The document's paths as the path-list JSON
/// `{"paths":[{"id":u64,"name":string,"kind":"work"|"saved",
/// "fill_rule":…,"subpaths":[…]}]}`, the work path first. NULL on a NULL
/// doc. Free with dr_string_free.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_paths_json(doc: *const DrDocument) -> *mut c_char {
    unsafe { doc_get(doc, ptr::null_mut(), |d| Some(json_string(d.paths()))) }
}

/// The document holding exactly the paths in `json` (the same JSON; an entry
/// with `id` 0 or absent is a new path and is minted an id). NULL on a NULL
/// doc or for the list the document already holds, with err_out untouched;
/// NULL with err_out set for NULL or malformed JSON, a shape past its caps
/// or coordinate wall, and a broken list rule or cap, whose sentence is
/// `doc_path::list_refusal`'s.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `json` must
/// be NULL or a NUL-terminated string; `err_out` must be NULL or a writable
/// CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_with_paths_json(
    doc: *const DrDocument,
    json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_doc_op(doc, err_out, "setting paths", |d| {
            Ok(d.with_paths(requested(json, d.vector_mask_anchor_count())?))
        })
    }
}

/// `dr_doc_paths_json` at high depth: the same JSON. NULL on a NULL doc.
/// Free with dr_string_free.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_paths_json(doc: *const PrecisionDocument) -> *mut c_char {
    unsafe { handle_get(doc, ptr::null_mut(), |d| json_string(d.paths())) }
}

/// `dr_doc_with_paths_json` at high depth, refusing exactly where it
/// refuses and with the same sentences.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`;
/// `json` must be NULL or a NUL-terminated string; `err_out` must be NULL or
/// a writable CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_with_paths_json(
    doc: *const PrecisionDocument,
    json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    if doc.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "setting high-depth paths",
            ptr::null_mut(),
            || {
                let document = &*doc;
                Ok(document.with_paths(requested(json, document.vector_mask_anchor_count())?))
            },
            |result: Option<PrecisionDocument>| result.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// The FILL of the shape in `shape_json` as u16 area coverage over the
/// `width` x `height` window whose top-left pixel is canvas pixel (`x`,
/// `y`), written row-major into `output` (65535 = wholly covered), through
/// the artwork coverage rasterizer under the shape's fill rule. `anti_alias`
/// false cuts each sample at one half (32768 and above is 65535). `clip` is
/// NULL (with `clip_count` 0) or `width * height` u16 samples multiplied
/// into the result, rounded to nearest. `limit` / `other` are the caller's
/// working limit and retained bytes; the charge is the rasterizer's scratch,
/// the plane and the parsed shape. False with `err_out` set, and `output`
/// untouched, for NULL or malformed JSON, a refused shape, buffers that do
/// not match the window, a zero or oversized window, a refused admission,
/// the rasterizer's edge or work caps, or a cancellation.
///
/// # Safety
/// `shape_json` must be NULL or a NUL-terminated string; `clip` must be NULL
/// or readable for `clip_count` u16s; `output` must be NULL or writable for
/// `output_count` u16s and overlap nothing else passed; `control` must be
/// NULL or a live single-use job control; `err_out` must be NULL or a
/// writable CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_path_coverage(
    shape_json: *const c_char,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    anti_alias: bool,
    clip: *const u16,
    clip_count: usize,
    output: *mut u16,
    output_count: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "rasterizing path coverage",
            false,
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                job.check()?;
                let n = pixel_count(width, height)?;
                if output.is_null()
                    || output_count != n
                    || (clip.is_null() && clip_count != 0)
                    || (!clip.is_null() && clip_count != n)
                {
                    return Err("invalid path coverage buffers".into());
                }
                let text = borrow_cstr_bounded(shape_json, "path shape json", SHAPE_JSON_LIMIT)?;
                let input = text.len() as u64 + (n as u64 + clip_count as u64) * 2;
                let scratch = coverage_scratch(width, height) + shape_scratch(text.len());
                admit(input, scratch, other, limit)?;
                job.reserve(scratch)?;
                let shape = parsed(text)?;
                let mut coverage = shape.coverage((x, y), width, height, anti_alias, job)?;
                if !clip.is_null() {
                    // The length is the window's, recomputed above.
                    let clip = std::slice::from_raw_parts(clip, n);
                    for (sample, &keep) in coverage.iter_mut().zip(clip) {
                        *sample = product(*sample, keep);
                    }
                }
                job.check()?;
                ptr::copy_nonoverlapping(coverage.as_ptr(), output, n);
                Ok(())
            },
            |()| true,
        )
    }
}

/// The shape in `shape_json` as polylines:
/// `[{"closed":bool,"points":[[x,y],…]}]`, one entry per subpath in order,
/// every point within `tolerance` pixels of the curve (four decimals). A
/// polyline starts at its subpath's first anchor and passes through every
/// anchor; a closed one covers the closing segment and does NOT repeat its
/// first point. A tolerance below 1/4096 px flattens at 1/4096. NULL with
/// `err_out` set for NULL or malformed JSON, a refused shape, a tolerance
/// that is not a positive finite number, or a shape that needs more than
/// 1,048,576 points at that tolerance. Free with dr_string_free.
///
/// # Safety
/// `shape_json` must be NULL or a NUL-terminated string; `err_out` must be
/// NULL or a writable CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_path_flatten(
    shape_json: *const c_char,
    tolerance: f64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "flattening a path",
            ptr::null_mut(),
            || {
                if !tolerance.is_finite() || tolerance <= 0.0 {
                    return Err("flatten tolerance must be a positive number".into());
                }
                let polylines = shape(shape_json)?.flattened(tolerance)?;
                // Written directly: a JSON tree of a million points would be
                // several times the text.
                let mut text = String::from("[");
                for (index, (points, closed)) in polylines.iter().enumerate() {
                    if index > 0 {
                        text.push(',');
                    }
                    let _ = write!(text, "{{\"closed\":{closed},\"points\":[");
                    for (i, p) in points.iter().enumerate() {
                        let comma = if i > 0 { "," } else { "" };
                        let _ = write!(text, "{comma}[{},{}]", q4_f64(p[0]), q4_f64(p[1]));
                    }
                    text.push_str("]}");
                }
                text.push(']');
                Ok(text)
            },
            owned,
        )
    }
}

/// Traces the region of `coverage` (`width * height` u16 samples, row-major)
/// at or above one half into the shape JSON of closed Bézier subpaths under
/// the even-odd rule, each within `tolerance` pixels of the half-coverage
/// contour (`path_trace`: marching squares with linear edge interpolation,
/// Douglas–Peucker, Schneider's cubic fit). A plane with nothing at or above
/// one half answers a shape with no subpath. `limit` / `other` are the
/// caller's working limit and retained bytes. NULL with `err_out` set for a
/// NULL or mis-sized plane, a zero or oversized one, a tolerance outside
/// 0.25..=64, a refused admission, an outline of more than 2,097,152 contour
/// points, a result past the shape caps (the sentence says to raise the
/// tolerance), or a cancellation. Free with dr_string_free.
///
/// # Safety
/// `coverage` must be NULL or readable for `coverage_count` u16s; `control`
/// must be NULL or a live single-use job control; `err_out` must be NULL or
/// a writable CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_path_trace(
    coverage: *const u16,
    coverage_count: usize,
    width: u32,
    height: u32,
    tolerance: f64,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "tracing a path",
            ptr::null_mut(),
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                job.check()?;
                let n = pixel_count(width, height)?;
                if coverage.is_null() || coverage_count != n {
                    return Err("invalid path trace coverage buffer".into());
                }
                let scratch = crate::path_trace::scratch_bytes(width, height);
                admit(n as u64 * 2, scratch, other, limit)?;
                job.reserve(scratch)?;
                // The length is the plane's, recomputed above.
                let plane = std::slice::from_raw_parts(coverage, n);
                let traced = crate::path_trace::trace(plane, width, height, tolerance, job)?;
                Ok(traced.to_value().to_string())
            },
            owned,
        )
    }
}

/// What lies within `tolerance` pixels of (`x`, `y`) on the shape in
/// `shape_json`: `{"kind":"none"}`, or
/// `{"kind":"anchor"|"handle_in"|"handle_out","subpath":i,"index":i}`, or
/// `{"kind":"segment","subpath":i,"index":i,"t":f}` with the parameter of
/// the segment's nearest point (what dr_path_insert_anchor takes). An anchor
/// wins over a handle and a handle over a segment; a handle sitting on its
/// point is not offered, and with `handles` false no handle is: the answer
/// is an anchor, a segment or none. NULL for NULL or malformed JSON or a
/// refused shape. Free with dr_string_free.
///
/// # Safety
/// `shape_json` must be NULL or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn dr_path_hit(
    shape_json: *const c_char,
    x: f64,
    y: f64,
    tolerance: f64,
    handles: bool,
) -> *mut c_char {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "hit-testing a path",
            ptr::null_mut(),
            || {
                let place = |kind: &str, subpath: usize, index: usize| json!({"kind": kind, "subpath": subpath, "index": index});
                let hit = match shape(shape_json)?.hit([x, y], tolerance, handles) {
                    PathHit::None => json!({"kind": "none"}),
                    PathHit::Anchor { subpath, index } => place("anchor", subpath, index),
                    PathHit::HandleIn { subpath, index } => place("handle_in", subpath, index),
                    PathHit::HandleOut { subpath, index } => place("handle_out", subpath, index),
                    PathHit::Segment { subpath, index, t } => {
                        json!({"kind": "segment", "subpath": subpath, "index": index, "t": t})
                    }
                };
                Ok(hit.to_string())
            },
            owned,
        )
    }
}

/// The shape in `shape_json` with one more anchor on segment `segment` of
/// subpath `subpath` at parameter `t` (dr_path_hit's), its outline
/// unchanged: a curved segment is split by de Casteljau, a straight one gets
/// a corner. Segment `i` runs from anchor `i` to the next; a closed
/// subpath's last segment returns to its first anchor. NULL with `err_out`
/// set for NULL or malformed JSON, a refused shape, no such subpath or
/// segment, a `t` not strictly between 0 and 1, or a shape already at 32768
/// anchors. Free with dr_string_free.
///
/// # Safety
/// `shape_json` must be NULL or a NUL-terminated string; `err_out` must be
/// NULL or a writable CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_path_insert_anchor(
    shape_json: *const c_char,
    subpath: usize,
    segment: usize,
    t: f64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "inserting a path anchor",
            ptr::null_mut(),
            || {
                shape(shape_json)?
                    .inserting_anchor(subpath, segment, t)
                    .map(|out| out.to_value().to_string())
                    .ok_or_else(|| {
                        "no anchor can be inserted there: no such segment, t is not between 0 \
                         and 1, or the path is full"
                            .to_string()
                    })
            },
            owned,
        )
    }
}

/// The shape in `shape_json` with anchor `anchor` of subpath `subpath`
/// converted: to a corner (`smooth` false: both handles onto the point), or
/// to a smooth anchor whose handles lie along the line from the previous
/// anchor to the next, each a third of the way to its neighbour. NULL with
/// `err_out` untouched when the anchor is already exactly that, or when a
/// smooth request has no neighbour at another point to aim at; NULL with
/// `err_out` set for NULL or malformed JSON, a refused shape, or no such
/// subpath or anchor. Free with dr_string_free.
///
/// # Safety
/// `shape_json` must be NULL or a NUL-terminated string; `err_out` must be
/// NULL or a writable CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_path_convert_anchor(
    shape_json: *const c_char,
    subpath: usize,
    anchor: usize,
    smooth: bool,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "converting a path anchor",
            ptr::null_mut(),
            || {
                let shape = shape(shape_json)?;
                if shape
                    .subpaths
                    .get(subpath)
                    .is_none_or(|s| anchor >= s.anchors.len())
                {
                    return Err("the path has no such anchor".into());
                }
                Ok(shape
                    .converting_anchor(subpath, anchor, smooth)
                    .map(|out| out.to_value().to_string()))
            },
            |text: Option<String>| text.map_or(ptr::null_mut(), owned),
        )
    }
}
