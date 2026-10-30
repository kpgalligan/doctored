//! The versioned evaluation policy across the FFI (Phase 11E): the policy
//! query, the refusal query, the switch and the operator table at both depths,
//! plus the two image COMPARISONS a host's drive measures the projection with.
//!
//! The conventions are the surface's: NULL tolerance first, `catch_unwind`
//! through the shared `ffi_util` helpers, `Option` for a domain refusal and
//! `Result` for an error reported through `err_out`. `doc_evaluation` and
//! `precision_evaluation` own every decision this file exports.

use std::ffi::{c_char, c_int, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::doc_evaluation::{operators_json, Evaluation};
use crate::ffi_util::{doc_get, doc_op, fallible_op, handle_get, read_cstr, required_handle};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::PrecisionImage;
use crate::DrImage;

/// Maps an evaluation NAME to its `DR_EVALUATION_*` value; -1 for NULL, for
/// invalid UTF-8 and for a name this build does not know.
///
/// # Safety
/// `name` must be NULL or a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn dr_evaluation_from_name(name: *const c_char) -> c_int {
    let Ok(name) = (unsafe { read_cstr(name, "evaluation name") }) else {
        return -1;
    };
    Evaluation::parse(&name).map_or(-1, Evaluation::code)
}

/// The operator table as owned JSON — what linear-light evaluation evaluates
/// today and what it refuses, with the phase that owns each refusal. Free it
/// with `dr_string_free`; NULL only on an allocation failure.
///
/// # Safety
/// The returned pointer is owned by the caller.
#[no_mangle]
pub unsafe extern "C" fn dr_evaluation_operators() -> *mut c_char {
    match std::panic::catch_unwind(|| CString::new(operators_json()).ok()) {
        Ok(Some(value)) => value.into_raw(),
        _ => ptr::null_mut(),
    }
}

/// The document's evaluation policy; `DR_EVALUATION_ENCODED` for NULL.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_evaluation(doc: *const DrDocument) -> c_int {
    unsafe {
        doc_get(doc, Evaluation::Encoded.code(), |d| {
            Some(d.evaluation().code())
        })
    }
}

/// The first operator in this document linear-light evaluation cannot
/// evaluate, as an owned sentence; NULL when there is nothing to refuse —
/// which is every ENCODED document, answered without a walk. Free a non-NULL
/// result with `dr_string_free`.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_linear_light_refusal(doc: *const DrDocument) -> *mut c_char {
    unsafe {
        doc_get(doc, ptr::null_mut(), |d| {
            d.linear_light_refusal()
                .and_then(|reason| CString::new(reason).ok())
                .map(CString::into_raw)
        })
    }
}

/// The document under `policy`. NULL WITHOUT an error when it is already in
/// that policy (the core's no-op rule); NULL WITH one for an unknown policy,
/// an unmodellable profile or an operator this phase does not evaluate.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `err_out`
/// must be NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_set_evaluation(
    doc: *const DrDocument,
    policy: c_int,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    let Some(policy) = Evaluation::from_code(policy) else {
        unsafe { crate::ffi_util::set_err(err_out, "unknown evaluation policy") };
        return ptr::null_mut();
    };
    // The refusal must reach the caller, so the switch is run under
    // `fallible_op` — ONCE: the document it produced is what goes through
    // `doc_op`'s capacity gate, rather than a second build of the transfer
    // tables and a second refusal walk.
    let switched = unsafe {
        fallible_op(
            err_out,
            "setting the evaluation policy",
            Err(()),
            || {
                let document = required_handle(doc)?;
                document.with_evaluation(policy)
            },
            Ok,
        )
    };
    let Ok(switched) = switched else {
        return ptr::null_mut();
    };
    unsafe { doc_op(doc, move |_| switched) }
}

/// The typed document's evaluation policy; `DR_EVALUATION_ENCODED` for NULL.
///
/// # Safety
/// `document` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_evaluation(document: *const PrecisionDocument) -> c_int {
    unsafe {
        handle_get(document, Evaluation::Encoded.code(), |d| {
            d.evaluation().code()
        })
    }
}

/// The typed twin of `dr_doc_linear_light_refusal`; free with `dr_string_free`.
///
/// # Safety
/// `document` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_linear_light_refusal(
    document: *const PrecisionDocument,
) -> *mut c_char {
    unsafe {
        handle_get(document, ptr::null_mut(), |d| {
            d.linear_light_refusal()
                .and_then(|reason| CString::new(reason).ok())
                .map_or(ptr::null_mut(), CString::into_raw)
        })
    }
}

/// The typed twin of `dr_doc_set_evaluation`: NULL without an error when the
/// policy is unchanged, NULL with one for a refusal.
///
/// # Safety
/// `document` must be NULL or a valid pointer to a live `DrPrecisionDocument`;
/// `err_out` must be NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_set_evaluation(
    document: *const PrecisionDocument,
    policy: c_int,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    let Some(policy) = Evaluation::from_code(policy) else {
        unsafe { crate::ffi_util::set_err(err_out, "unknown evaluation policy") };
        return ptr::null_mut();
    };
    unsafe {
        fallible_op(
            err_out,
            "setting the high-depth evaluation policy",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                document.with_evaluation(policy)
            },
            |result| result.map_or(ptr::null_mut(), crate::ffi_util::boxed),
        )
    }
}

/// `{"depth":8,"width":..,"height":..,"mean_abs":..,"max_abs":..,"changed_fraction":..}`
/// over ALL RGBA samples of two images — the measurement a drive states its
/// projection tolerance in. An error (not a refusal) for a NULL handle and for
/// a dimension mismatch, because comparing two different pictures is a caller
/// mistake, not a result. Free a non-NULL result with `dr_string_free`.
///
/// # Safety
/// `a` and `b` must be NULL or valid pointers to live `DrImage`s; `err_out`
/// must be NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_compare(
    a: *const DrImage,
    b: *const DrImage,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "comparing images",
            ptr::null_mut(),
            || {
                let (a, b) = (required_handle(a)?, required_handle(b)?);
                let (aw, ah) = a.pixels.dimensions();
                if (aw, ah) != b.pixels.dimensions() {
                    return Err("compared images have different dimensions".into());
                }
                let samples = a
                    .pixels
                    .as_raw()
                    .iter()
                    .zip(b.pixels.as_raw().iter())
                    .map(|(&x, &y)| u32::from(x.abs_diff(y)));
                Ok(difference_json(8, aw, ah, samples))
            },
            |value| CString::new(value).map_or(ptr::null_mut(), CString::into_raw),
        )
    }
}

/// The typed twin, in codes of 65535 and over RGBA16 UNORM storage only.
///
/// # Safety
/// `a` and `b` must be NULL or valid pointers to live `DrPrecisionImage`s;
/// `err_out` must be NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_compare(
    a: *const PrecisionImage,
    b: *const PrecisionImage,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "comparing high-depth images",
            ptr::null_mut(),
            || {
                let (a, b) = (required_handle(a)?, required_handle(b)?);
                let (aw, ah) = a.dimensions();
                if (aw, ah) != b.dimensions() {
                    return Err("compared images have different dimensions".into());
                }
                let (left, right) = (a.runs_rgba16(), b.runs_rgba16());
                let (Some(mut left), Some(mut right)) = (left, right) else {
                    return Err("comparing high-depth images requires RGBA16 UNORM storage".into());
                };
                // Two cursors over the flat run streams, split at the shorter
                // run; each pair is a plain slice zip in flat sample order,
                // accumulated in nested loops (an iterator chain over the
                // runs does not fold to a straight loop).
                let mut sums = Difference::default();
                let (mut a_run, mut b_run): (&[[u16; 4]], &[[u16; 4]]) = (&[], &[]);
                loop {
                    if a_run.is_empty() {
                        let Some(run) = left.next() else { break };
                        a_run = run;
                    }
                    if b_run.is_empty() {
                        let Some(run) = right.next() else { break };
                        b_run = run;
                    }
                    let n = a_run.len().min(b_run.len());
                    let ((a_head, a_tail), (b_head, b_tail)) =
                        (a_run.split_at(n), b_run.split_at(n));
                    (a_run, b_run) = (a_tail, b_tail);
                    for (x, y) in a_head.iter().zip(b_head) {
                        for c in 0..4 {
                            sums.add(u32::from(x[c].abs_diff(y[c])));
                        }
                    }
                }
                Ok(sums.json(16, aw, ah))
            },
            |value| CString::new(value).map_or(ptr::null_mut(), CString::into_raw),
        )
    }
}

/// The ONE difference summary, over an iterator of per-sample absolute
/// differences in the depth's own codes. An empty image reports zeros rather
/// than a division by zero.
fn difference_json(
    depth: u32,
    width: u32,
    height: u32,
    samples: impl Iterator<Item = u32>,
) -> String {
    let mut sums = Difference::default();
    for value in samples {
        sums.add(value);
    }
    sums.json(depth, width, height)
}

/// `difference_json`'s counters, so the typed compare can accumulate over its
/// run pairs in nested loops and share the one summary.
#[derive(Default)]
struct Difference {
    count: u64,
    changed: u64,
    total: u64,
    max: u32,
}

impl Difference {
    #[inline]
    fn add(&mut self, value: u32) {
        self.count += 1;
        self.total += u64::from(value);
        if value != 0 {
            self.changed += 1;
        }
        self.max = self.max.max(value);
    }

    fn json(&self, depth: u32, width: u32, height: u32) -> String {
        let divisor = self.count.max(1) as f64;
        serde_json::json!({
            "depth": depth,
            "width": width,
            "height": height,
            "mean_abs": self.total as f64 / divisor,
            "max_abs": self.max,
            "changed_fraction": self.changed as f64 / divisor,
        })
        .to_string()
    }
}
