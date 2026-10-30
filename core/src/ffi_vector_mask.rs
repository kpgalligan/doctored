//! C FFI for vector masks — the vector-mask exports of the "Paths"
//! section of `include/doctored_core.h`: read, set, remove and the two
//! flags, byte then typed, over `doc_vector_mask` / `precision_vector_mask`.
//! Split from `ffi_path`, which keeps the path-list and shape exports and the
//! shape JSON reader (`shape`) and C-string helper (`owned`) these share.

use std::ffi::c_char;
use std::ptr;

use crate::doc::DrDocument;
use crate::ffi_path::{owned, shape};
use crate::ffi_util::{boxed, doc_get, doc_op, fallible_doc_op, fallible_op, handle_get};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;

/// Layer `layer`'s vector mask as JSON: the shape JSON's own keys beside
/// `"enabled"`, `"linked"`, `"has_raster"` (a raster mask is kept beside the
/// shape), `"raster_enabled"` and `"raster"` — null, or the raster part's
/// own mask-properties object (`dr_doc_layer_mask_properties`' keys), which
/// is what a host's read-modify-write of a mask property starts from. NULL
/// on a NULL doc, an out-of-range index and a layer with no vector mask.
/// Free with dr_string_free.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_vector_mask_json(
    doc: *const DrDocument,
    layer: usize,
) -> *mut c_char {
    unsafe {
        doc_get(doc, ptr::null_mut(), |d| {
            d.vector_mask(layer).map(|view| owned(view.json()))
        })
    }
}

/// The document with layer `layer`'s vector mask set to the shape in
/// `shape_json` — a new vector mask (the layer's ordinary mask, if any,
/// kept beside it as its raster part), or the same one with a new shape. An
/// empty shape is Reveal All. NULL with err_out untouched on a NULL doc, an
/// out-of-range index, Lock All and the shape the mask already has; NULL
/// with err_out set for NULL or malformed JSON, a refused shape, a shape
/// that takes the document past its anchor cap (`too many path anchors in
/// the document (max 65536)`), the rasterizer's refusal, growth past native
/// capacity and a cancellation.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`;
/// `shape_json` must be NULL or a NUL-terminated string; `control` must be
/// NULL or a live single-use job control; `err_out` must be NULL or a
/// writable CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_set_vector_mask(
    doc: *const DrDocument,
    layer: usize,
    shape_json: *const c_char,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_doc_op(doc, err_out, "setting a vector mask", |d| {
            let control = control.as_ref();
            let _run = control.map(DrJobControl::begin).transpose()?;
            let job = control.map_or(JobContext::NONE, JobContext::new);
            job.check()?;
            d.set_vector_mask(layer, &shape(shape_json)?, job)
        })
    }
}

/// The document with layer `layer`'s vector mask removed: its raster part
/// alone is the layer's mask again, or the layer has none. NULL on a NULL
/// doc, an out-of-range index, a layer with no vector mask and Lock All.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_remove_vector_mask(
    doc: *const DrDocument,
    layer: usize,
) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.remove_vector_mask(layer)) }
}

/// The document with layer `layer`'s vector mask enabled or disabled and
/// linked or unlinked, rebaked. NULL with err_out untouched on a NULL doc, a
/// layer with no vector mask, Lock All and the flags it already has; NULL
/// with err_out set for the rasterizer's refusal, growth past native
/// capacity and a cancellation.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `control`
/// must be NULL or a live single-use job control; `err_out` must be NULL or
/// a writable CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_set_vector_mask_properties(
    doc: *const DrDocument,
    layer: usize,
    enabled: bool,
    linked: bool,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_doc_op(doc, err_out, "setting vector mask properties", |d| {
            let control = control.as_ref();
            let _run = control.map(DrJobControl::begin).transpose()?;
            let job = control.map_or(JobContext::NONE, JobContext::new);
            job.check()?;
            d.set_vector_mask_properties(layer, enabled, linked, job)
        })
    }
}

/// `dr_doc_vector_mask_json` at high depth: the same JSON. NULL on a NULL
/// doc, an out-of-range index and a record with no vector mask. Free with
/// dr_string_free.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_vector_mask_json(
    doc: *const PrecisionDocument,
    layer: usize,
) -> *mut c_char {
    unsafe {
        handle_get(doc, ptr::null_mut(), |d| {
            d.vector_mask(layer)
                .map_or(ptr::null_mut(), |view| owned(view.json()))
        })
    }
}

/// A typed vector-mask op's answer as a handle: NULL with err_out untouched
/// on a NULL doc and for `Ok(None)`, NULL with err_out set for an `Err`.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`;
/// `err_out` must be NULL or a writable CString pointer.
unsafe fn typed_op(
    doc: *const PrecisionDocument,
    err_out: *mut *mut c_char,
    what: &str,
    op: impl FnOnce(&PrecisionDocument) -> Result<Option<PrecisionDocument>, String>,
) -> *mut PrecisionDocument {
    if doc.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            what,
            ptr::null_mut(),
            || op(&*doc),
            |result: Option<PrecisionDocument>| result.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// `dr_doc_set_vector_mask` at high depth, admitted against the caller's
/// working `limit` and `other` retained bytes (the bake's scratch is the
/// coverage rasterizer's, a plane and the raster part's feather). NULL with
/// err_out untouched on a NULL doc and for the shape the mask already has;
/// NULL with err_out set otherwise — an out-of-range index and a lock
/// included, which the typed core names.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`;
/// `shape_json` must be NULL or a NUL-terminated string; `control` must be
/// NULL or a live single-use job control; `err_out` must be NULL or a
/// writable CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_set_vector_mask(
    doc: *const PrecisionDocument,
    layer: usize,
    shape_json: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        typed_op(doc, err_out, "setting a high-depth vector mask", |d| {
            d.set_vector_mask(layer, &shape(shape_json)?, limit, other, control.as_ref())
        })
    }
}

/// `dr_doc_remove_vector_mask` at high depth. NULL with err_out untouched on
/// a NULL doc and a record with no vector mask; NULL with err_out set for a
/// lock and a refused admission.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`;
/// `err_out` must be NULL or a writable CString pointer, freed with
/// dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_remove_vector_mask(
    doc: *const PrecisionDocument,
    layer: usize,
    limit: u64,
    other: u64,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        typed_op(doc, err_out, "removing a high-depth vector mask", |d| {
            d.remove_vector_mask(layer, limit, other)
        })
    }
}

/// `dr_doc_set_vector_mask_properties` at high depth, admitted like
/// `dr_precision_doc_set_vector_mask`. NULL with err_out untouched on a NULL
/// doc, a record with no vector mask and the flags it already has; NULL with
/// err_out set for a lock, a refused admission, the rasterizer's refusal and
/// a cancellation.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`;
/// `control` must be NULL or a live single-use job control; `err_out` must
/// be NULL or a writable CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_set_vector_mask_properties(
    doc: *const PrecisionDocument,
    layer: usize,
    enabled: bool,
    linked: bool,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        typed_op(
            doc,
            err_out,
            "setting high-depth vector mask properties",
            |d| {
                d.set_vector_mask_properties(layer, enabled, linked, limit, other, control.as_ref())
            },
        )
    }
}
