//! C FFI for guides and the ruler origin (`dr_max_guides`, `dr_doc_guide_*`,
//! `dr_doc_ruler_origin`, `dr_doc_set_ruler_origin`) — the "Guides, rulers
//! and snapping" section of `include/doctored_core.h`. Same conventions as
//! the sibling shims: `catch_unwind` through the `ffi_util` helpers, NULL
//! tolerance first, narrow `unsafe` blocks.
//!
//! Like the channel family there is no `err_out` anywhere: every refusal is
//! NULL / 0 / -1 and the reasons are in the doc comments. A host that needs
//! to tell one refusal from another (out of canvas, already occupied, list
//! full) checks before it calls — which is why [`dr_max_guides`] exists.
//!
//! `orientation` is declared `c_int` and mapped through
//! [`GuideOrientation::from_c`], for the reason `ffi_channel` gives of
//! `DrPlane`: materializing an enum from an out-of-range discriminant would
//! be undefined behaviour.
//!
//! **No bulk getter, deliberately.** The host caches the list in its own
//! array, refreshed once per document change, so a canvas redraw makes zero
//! FFI calls; three calls per guide per change, on a list that is under ten
//! entries in any real document, is far under the five-calls-per-channel
//! `dr_doc_channel_*` reload this crate already accepts.
//!
//! **The TYPED twins (Phase 12E-1c)** close the file: ten
//! `dr_precision_doc_*` exports over `precision_guide`, each the byte
//! export's contract verbatim, with the same sentinels (0 / -1 / -1.0 /
//! false / NULL) and the same refusals — the host's own pre-checks
//! (`AgentServer+Guides.swift`) depend on that, and the Swift facade picks
//! one family by depth rather than phrasing anything twice. They live here
//! rather than in a typed file of their own because a contract stated once
//! cannot drift: the byte twin is always the line above. Getters go through
//! `handle_get`, mutators through the shared `ffi_util::precision_op`.

use std::ffi::{c_double, c_int};
use std::panic::catch_unwind;
use std::ptr;

use crate::doc::DrDocument;
use crate::doc_guide::{GuideOrientation, MAX_GUIDES};
use crate::ffi_util::{doc_get, doc_op, handle_get, precision_op};
use crate::precision_document::PrecisionDocument;

/// The largest guide list any document may carry — `doc_guide::MAX_GUIDES`,
/// today 1024.
///
/// It takes no arguments because, unlike the channel budget
/// (`dr_max_channels_at`), the cap does NOT vary with the canvas: a guide is
/// a line, not a plane, so no canvas size makes fewer of them fit. It is
/// exported rather than left as a Swift literal because a host must be able
/// to tell "the list is full" apart from "that guide already exists" when
/// `dr_doc_add_guide` answers a single NULL, and one exported constant cannot
/// drift from the core's.
#[no_mangle]
pub extern "C" fn dr_max_guides() -> usize {
    // Pure arithmetic that cannot panic; the guard is the family's uniform
    // shape, not a defence against a known unwind.
    catch_unwind(|| MAX_GUIDES).unwrap_or(0)
}

/// Number of guides on the document; 0 on a NULL doc.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_guide_count(doc: *const DrDocument) -> usize {
    unsafe { doc_get(doc, 0, |d| Some(d.guides.len())) }
}

/// Guide `i`'s stable identity — unique among every guide this process has
/// minted, kept by a move, the geometry ops and undo/redo, and re-minted on
/// load. 0 on a NULL doc or an out-of-range index, which no live guide ever
/// answers.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_guide_id(doc: *const DrDocument, i: usize) -> u64 {
    unsafe { doc_get(doc, 0, |d| Some(d.guides.get(i)?.id)) }
}

/// Guide `i`'s orientation as a `DrGuideOrientation`; -1 on a NULL doc or an
/// out-of-range index, which is outside the enum on purpose.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_guide_orientation(doc: *const DrDocument, i: usize) -> c_int {
    unsafe { doc_get(doc, -1, |d| Some(d.guides.get(i)?.orientation.to_c())) }
}

/// Guide `i`'s canvas position — the x of a vertical guide, the y of a
/// horizontal one. -1.0 on a NULL doc or an out-of-range index: a legal
/// position is never negative, so the sentinel cannot collide with an answer.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_guide_position(doc: *const DrDocument, i: usize) -> c_double {
    unsafe { doc_get(doc, -1.0, |d| Some(d.guides.get(i)?.position)) }
}

/// Adds a guide at `position` and returns a NEW document. NULL on a NULL doc,
/// an orientation outside `DrGuideOrientation`, a non-finite position, a
/// position outside [0, canvas extent], a position a guide of the same
/// orientation already occupies, or a list already holding `dr_max_guides()`.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_add_guide(
    doc: *const DrDocument,
    orientation: c_int,
    position: c_double,
) -> *mut DrDocument {
    unsafe {
        doc_op(doc, |d| {
            d.add_guide(GuideOrientation::from_c(orientation)?, position)
        })
    }
}

/// Moves guide `i` along its own axis, keeping its identity and orientation.
/// NULL on a NULL doc, an out-of-range index, a non-finite or out-of-canvas
/// position, the position the guide already has, or a position another guide
/// of the same orientation already occupies.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_move_guide(
    doc: *const DrDocument,
    i: usize,
    position: c_double,
) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.move_guide(i, position)) }
}

/// Drops guide `i`. NULL on a NULL doc or an out-of-range index.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_remove_guide(doc: *const DrDocument, i: usize) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.remove_guide(i)) }
}

/// Drops every guide. NULL on a NULL doc or an already empty list — clearing
/// nothing is not an edit.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_clear_guides(doc: *const DrDocument) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.clear_guides()) }
}

/// Writes the ruler origin as two doubles (x, y) into `out_xy`; false on a
/// NULL doc or a NULL buffer.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `out_xy`
/// must be NULL or writable for 2 doubles.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_ruler_origin(
    doc: *const DrDocument,
    out_xy: *mut c_double,
) -> bool {
    if out_xy.is_null() {
        return false;
    }
    unsafe {
        doc_get(doc, false, |d| {
            let origin = [d.ruler_origin.0, d.ruler_origin.1];
            ptr::copy_nonoverlapping(origin.as_ptr(), out_xy, origin.len());
            Some(true)
        })
    }
}

/// Moves the ruler zero point to canvas (x, y). NULL on a NULL doc, a
/// non-finite component, a point outside the canvas, or the origin the
/// document already has.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_set_ruler_origin(
    doc: *const DrDocument,
    x: c_double,
    y: c_double,
) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.set_ruler_origin(x, y)) }
}

// --------------------------------------------------------- typed twins --

/// `dr_doc_guide_count` at high depth; 0 on a NULL doc.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_guide_count(doc: *const PrecisionDocument) -> usize {
    unsafe { handle_get(doc, 0, |d| d.guides().len()) }
}

/// `dr_doc_guide_id` at high depth; 0 on a NULL doc or a bad index.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_guide_id(doc: *const PrecisionDocument, i: usize) -> u64 {
    unsafe { handle_get(doc, 0, |d| d.guides().get(i).map_or(0, |g| g.id)) }
}

/// `dr_doc_guide_orientation` at high depth; -1 on a NULL doc or a bad index.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_guide_orientation(
    doc: *const PrecisionDocument,
    i: usize,
) -> c_int {
    unsafe {
        handle_get(doc, -1, |d| {
            d.guides().get(i).map_or(-1, |g| g.orientation.to_c())
        })
    }
}

/// `dr_doc_guide_position` at high depth; -1.0 on a NULL doc or a bad index.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_guide_position(
    doc: *const PrecisionDocument,
    i: usize,
) -> c_double {
    unsafe {
        handle_get(doc, -1.0, |d| {
            d.guides().get(i).map_or(-1.0, |g| g.position)
        })
    }
}

/// `dr_doc_ruler_origin` at high depth; false on a NULL doc or a NULL buffer.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`;
/// `out_xy` must be NULL or writable for 2 doubles.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_ruler_origin(
    doc: *const PrecisionDocument,
    out_xy: *mut c_double,
) -> bool {
    if out_xy.is_null() {
        return false;
    }
    unsafe {
        handle_get(doc, false, |d| {
            let origin = [d.ruler_origin().0, d.ruler_origin().1];
            ptr::copy_nonoverlapping(origin.as_ptr(), out_xy, origin.len());
            true
        })
    }
}

/// `dr_doc_add_guide` at high depth, refusing exactly where it refuses.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_add_guide(
    doc: *const PrecisionDocument,
    orientation: c_int,
    position: c_double,
) -> *mut PrecisionDocument {
    unsafe {
        precision_op(doc, |d| {
            d.add_guide(GuideOrientation::from_c(orientation)?, position)
        })
    }
}

/// `dr_doc_move_guide` at high depth.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_move_guide(
    doc: *const PrecisionDocument,
    i: usize,
    position: c_double,
) -> *mut PrecisionDocument {
    unsafe { precision_op(doc, |d| d.move_guide(i, position)) }
}

/// `dr_doc_remove_guide` at high depth.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_remove_guide(
    doc: *const PrecisionDocument,
    i: usize,
) -> *mut PrecisionDocument {
    unsafe { precision_op(doc, |d| d.remove_guide(i)) }
}

/// `dr_doc_clear_guides` at high depth.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_clear_guides(
    doc: *const PrecisionDocument,
) -> *mut PrecisionDocument {
    unsafe { precision_op(doc, |d| d.clear_guides()) }
}

/// `dr_doc_set_ruler_origin` at high depth.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_set_ruler_origin(
    doc: *const PrecisionDocument,
    x: c_double,
    y: c_double,
) -> *mut PrecisionDocument {
    unsafe { precision_op(doc, |d| d.set_ruler_origin(x, y)) }
}
