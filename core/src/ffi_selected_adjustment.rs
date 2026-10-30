//! Additive selected native tone/filter bridge. Every request plans and admits
//! before coverage borrowing or pixel allocation; results are immutable. A
//! Grayscale or Lab Color document's request first passes its mode's gate
//! (`gray_adjust::admit_selected`, `lab_adjust::admit_selected`).
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, required_handle};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, BOOKKEEPING_BYTES};
use crate::selected_adjustment::{validate_coverage, SelectedEdit};
use crate::selected_auto_levels::AutoLevels;
use std::ffi::{c_char, CString};
use std::ptr;
const REQUEST_LIMIT: usize = 20 * 1024 * 1024;
unsafe fn request<'a>(
    doc: *const PrecisionDocument,
    text: *const c_char,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<(&'a PrecisionDocument, SelectedEdit, u64), String> {
    let doc = unsafe { required_handle(doc) }?;
    let text = unsafe { borrow_cstr_bounded(text, "selected edit request", REQUEST_LIMIT) }?;
    // Borrow first; JSON string/Value allocations start only after admission.
    let parse_bytes = text.len() as u64 * 64 + 4 * BOOKKEEPING_BYTES;
    admit(doc.retained_bytes(), parse_bytes, other, limit)?;
    job.reserve(parse_bytes)?;
    let mut edit = SelectedEdit::parse(text)?;
    // A Bitmap document's gate (#1 Bitmap phase 3): Liquify only. It is
    // Gray underneath, so it goes before the Gray gate.
    crate::bitmap_document::admit_selected(doc, &edit)?;
    // A Grayscale document's gate (#1 phase 2b): Gradient Map stops greyed.
    crate::gray_adjust::admit_selected(doc, &mut edit)?;
    // A Lab Color document's gate (#1 phases 2b, 2c): adjustments by its
    // table, Auto, Equalize and the filters (Clouds' colours as Lab codes).
    crate::lab_adjust::admit_selected(doc, &mut edit)?;
    Ok((doc, edit, parse_bytes))
}
fn plan(
    doc: &PrecisionDocument,
    edit: &SelectedEdit,
    parse_bytes: u64,
    limit: u64,
    other: u64,
) -> Result<serde_json::Value, String> {
    let mut plan = doc.selected_edit_plan(edit)?;
    // Parsing temporaries have gone by kernel preparation. The core plan
    // separately retains the captured description throughout evaluation.
    let scratch = plan["scratch_bytes"].as_u64().unwrap().max(parse_bytes);
    admit(doc.retained_bytes(), scratch, other, limit)?;
    plan["scratch_bytes"] = scratch.into();
    Ok(plan)
}
unsafe fn coverage<'a>(
    doc: &PrecisionDocument,
    samples: *const u16,
    length: usize,
) -> Result<&'a [u16], String> {
    let (w, h) = doc.dimensions();
    let expected = w as usize * h as usize;
    if length != 0 && (samples.is_null() || length != expected) {
        return Err("selected coverage must be empty or canvas-sized u16".into());
    }
    let samples = if length == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(samples, length) }
    };
    validate_coverage(doc, samples)?;
    Ok(samples)
}
/// Plan maximum canvas coverage and all kernel scratch before host allocation.
/// # Safety
/// doc NULL/live; text NULL/terminated UTF8; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_selected_edit_plan(
    doc: *const PrecisionDocument,
    text: *const c_char,
    limit: u64,
    other: u64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning selected native edit",
            ptr::null_mut(),
            || {
                let (doc, edit, parse_bytes) = request(doc, text, limit, other, JobContext::NONE)?;
                CString::new(plan(doc, &edit, parse_bytes, limit, other)?.to_string())
                    .map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}
/// Native selected edit; NULL/no error is exact no-op. Coverage is independent
/// of all neighborhood reads and automatic analysis scope. Empty is whole source.
/// # Safety
/// doc/control NULL/live; text NULL/terminated UTF8; samples NULL or readable for
/// length u16s; err_out NULL/writable. Caller retains inputs until return.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_selected_edit(
    doc: *const PrecisionDocument,
    text: *const c_char,
    samples: *const u16,
    length: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "applying selected native edit",
            ptr::null_mut(),
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let (doc, edit, parse_bytes) = request(doc, text, limit, other, job)?;
                let plan = plan(doc, &edit, parse_bytes, limit, other)?;
                let scratch = plan["scratch_bytes"].as_u64().unwrap();
                job.reserve(scratch)?;
                doc.selected_edit_with_context(
                    &edit,
                    coverage(doc, samples, length)?,
                    limit,
                    other,
                    job.with_reservation_floor(scratch),
                )
            },
            |doc| doc.map_or(ptr::null_mut(), boxed),
        )
    }
}
/// Resolve automatic parameters against captured native samples for Actions.
/// Same request/coverage contract as selected_edit; returns arrays and identity.
/// # Safety
/// Same pointer/lifetime preconditions as dr_precision_doc_selected_edit.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_selected_auto_levels(
    doc: *const PrecisionDocument,
    text: *const c_char,
    samples: *const u16,
    length: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "deriving selected native auto levels",
            ptr::null_mut(),
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let (doc, edit, parse_bytes) = request(doc, text, limit, other, job)?;
                if !edit.is_auto() {
                    return Err("automatic derivation requires an auto tone request".into());
                }
                let plan = plan(doc, &edit, parse_bytes, limit, other)?;
                job.reserve(plan["scratch_bytes"].as_u64().unwrap())?;
                let levels = AutoLevels::derive(doc, &edit, coverage(doc, samples, length)?, job)?;
                job.report(1., 5)?;
                CString::new(levels.json().to_string()).map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}
