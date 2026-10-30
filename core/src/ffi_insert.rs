//! Positional insertion across the boundary: the byte `adding_layer_at` and
//! the typed `add_adjustment_at`. Thin shims only — the slot rule lives in
//! `layer_insert`, the typed sequence in `precision_insert`. A Grayscale or
//! Lab Color document's adjustment passes its mode's table first
//! (`gray_adjust::admitted`, `lab_adjust::admit`).

use std::ffi::c_char;
use std::ptr;

use crate::doc::DrDocument;
use crate::drdc::{MAX_DRDC_BLOB_LEN, MAX_DRDC_NAME_LEN};
use crate::ffi_util::{borrow_cstr_bounded, boxed, doc_op, fallible_op, required_handle};
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_insert::InsertPosition;
use crate::precision_document::PrecisionDocument;

/// Inserts a transparent canvas-sized layer at the slot `position` names
/// relative to `anchor`. NULL for a refused position, an out-of-range anchor,
/// growth past native capacity, or a panic.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `name` must
/// be NULL or a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_adding_layer_at(
    doc: *const DrDocument,
    anchor: usize,
    position: u32,
    name: *const c_char,
) -> *mut DrDocument {
    let Ok(position) = InsertPosition::from_code(position) else {
        return ptr::null_mut();
    };
    let Ok(name) = (unsafe { borrow_cstr_bounded(name, "name", MAX_DRDC_NAME_LEN as usize) })
    else {
        return ptr::null_mut();
    };
    unsafe {
        doc_op(doc, |document| {
            document.adding_layer_at(anchor, position, name)
        })
    }
}

/// Adds a live adjustment at the slot `position` names relative to `anchor` —
/// `dr_precision_doc_add_adjustment`'s positional twin, with that export's
/// admission, job and refusal contract verbatim.
///
/// # Safety
/// `document` is NULL or live; strings are NULL or NUL-terminated UTF-8;
/// `err_out` is NULL or writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_add_adjustment_at(
    document: *const PrecisionDocument,
    name: *const c_char,
    description: *const c_char,
    opacity: f32,
    anchor: usize,
    position: u32,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "adding high-depth adjustment",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let name = borrow_cstr_bounded(name, "name", MAX_DRDC_NAME_LEN as usize)?;
                let description =
                    borrow_cstr_bounded(description, "description", MAX_DRDC_BLOB_LEN as usize)?;
                let position = InsertPosition::from_code(position)?;
                let description = crate::gray_adjust::admitted(document, description, true)?;
                crate::lab_adjust::admit(document, &description, true)?;
                let description = &*description;
                // A CMYK document's Levels/Curves record (`cmyk_adjust_layer`).
                if document.is_cmyk() {
                    let control = control.as_ref();
                    let _run = control.map(DrJobControl::begin).transpose()?;
                    let job = control.map_or(JobContext::NONE, JobContext::new);
                    return document.cmyk_add_adjustment_at(
                        name,
                        description,
                        opacity,
                        anchor,
                        position,
                        limit,
                        other_retained_bytes,
                        job,
                    );
                }
                document.add_adjustment_at(
                    name,
                    description,
                    opacity,
                    anchor,
                    position,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}
