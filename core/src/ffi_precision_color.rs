//! Internal typed tone/color bridge. The old byte owners never enter these
//! calls. Pixel and parameter semantics live in the shared adjustment/ICC
//! kernels; this module only validates borrowed arguments and owns results.
//! On a CMYK document the four adjustment exports take their CMYK arms
//! (`cmyk_adjust_layer`: Levels and Curves on ink, every other op refused
//! by name). On a Grayscale document each first asks `gray_adjust::admitted`
//! (#1 phase 2b), which refuses by name or hands back the description to
//! run (a Gradient Map's stops greyed). On a Lab Color document each asks
//! `lab_adjust::admit` (#1 phase 2b), which refuses by name; the admitted
//! description runs in its Lab form, bound at parse.

use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;

use crate::adjust_calibrated::precision_model;
use crate::drdc::{MAX_DRDC_BLOB_LEN, MAX_DRDC_NAME_LEN};
use crate::ffi_color::{check_profile_arg, required_profile};
use crate::ffi_output_color::options_arg;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, handle_get, required_handle};
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_color::PrecisionColorPlan;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, BOOKKEEPING_BYTES};

/// Owned JSON preparation/writeback plan. index=-1 appends, >=0 selects a target.
/// # Safety
/// document NULL/live; description NULL/terminated UTF-8; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_adjustment_plan(
    document: *const PrecisionDocument,
    index: i64,
    description: *const c_char,
    destructive: bool,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning high-depth adjustment",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let index = match index {
                    -1 => None,
                    value if value >= 0 => Some(
                        usize::try_from(value)
                            .map_err(|_| "high-depth layer index exceeds addressable range")?,
                    ),
                    _ => {
                        return Err(
                            "high-depth adjustment plan index must be -1 or nonnegative".into()
                        )
                    }
                };
                let description =
                    borrow_cstr_bounded(description, "description", MAX_DRDC_BLOB_LEN as usize)?;
                let description =
                    crate::gray_adjust::admitted(document, description, !destructive)?;
                crate::lab_adjust::admit(document, &description, !destructive)?;
                let description = &*description;
                if document.is_cmyk() {
                    return CString::new(
                        document
                            .cmyk_adjustment_plan(index, description, destructive)?
                            .to_string(),
                    )
                    .map_err(|e| e.to_string());
                }
                let scratch = document.adjustment_scratch_bytes(index, description, destructive)?;
                CString::new(
                    serde_json::json!({
                        "retained_bytes": document.retained_bytes(), "scratch_bytes": scratch,
                    })
                    .to_string(),
                )
                .map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Append a live adjustment using the strict high-depth shared meta schema.
/// # Safety
/// document is NULL/live; strings NULL/terminated UTF-8; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_add_adjustment(
    document: *const PrecisionDocument,
    name: *const c_char,
    description: *const c_char,
    opacity: f32,
    limit: u64,
    other: u64,
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
                let description = crate::gray_adjust::admitted(document, description, true)?;
                crate::lab_adjust::admit(document, &description, true)?;
                let description = &*description;
                if document.is_cmyk() {
                    let control = control.as_ref();
                    let _run = control.map(DrJobControl::begin).transpose()?;
                    let job = control.map_or(JobContext::NONE, JobContext::new);
                    return document.cmyk_add_adjustment(
                        name,
                        description,
                        opacity,
                        limit,
                        other,
                        job,
                    );
                }
                document.add_adjustment(name, description, opacity, limit, other, control.as_ref())
            },
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Replace a live adjustment description. Unchanged state returns NULL/no error.
/// # Safety
/// document is NULL/live; description NULL/terminated UTF-8; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_set_adjustment(
    document: *const PrecisionDocument,
    index: usize,
    description: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "setting high-depth adjustment",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let description =
                    borrow_cstr_bounded(description, "description", MAX_DRDC_BLOB_LEN as usize)?;
                let description = crate::gray_adjust::admitted(document, description, true)?;
                crate::lab_adjust::admit(document, &description, true)?;
                let description = &*description;
                if document.is_cmyk() {
                    let control = control.as_ref();
                    let _run = control.map(DrJobControl::begin).transpose()?;
                    let job = control.map_or(JobContext::NONE, JobContext::new);
                    return document.cmyk_set_adjustment(index, description, limit, other, job);
                }
                document.set_adjustment(index, description, limit, other, control.as_ref())
            },
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Apply the shared adjustment to one raster source, preserving alpha: u16,
/// or f32 under the layer's float form in a float document.
/// # Safety
/// document/control NULL/live; description NULL/terminated UTF-8; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_apply_adjustment(
    document: *const PrecisionDocument,
    index: usize,
    description: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "applying high-depth adjustment",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let description =
                    borrow_cstr_bounded(description, "description", MAX_DRDC_BLOB_LEN as usize)?;
                let description = crate::gray_adjust::admitted(document, description, false)?;
                crate::lab_adjust::admit(document, &description, false)?;
                let description = &*description;
                if document.is_cmyk() {
                    let control = control.as_ref();
                    let _run = control.map(DrJobControl::begin).transpose()?;
                    let job = control.map_or(JobContext::NONE, JobContext::new);
                    return document.cmyk_apply_adjustment(
                        index,
                        description,
                        &[],
                        limit,
                        other,
                        job,
                    );
                }
                document.apply_adjustment(index, description, limit, other, control.as_ref())
            },
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Argument parsing can retain three shared-tag TRC copies plus ICC bytes.
/// The extra copy covers Arc adoption and borrowed host bytes; the fixed
/// allowance covers bounded names/headers. Admit before calling the parser.
unsafe fn target_profile(
    document: &PrecisionDocument,
    bytes: *const u8,
    length: usize,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<Arc<IccProfile>, String> {
    job.check()?;
    check_profile_arg(bytes, length)?;
    let scratch = 5 * length as u64 + BOOKKEEPING_BYTES;
    admit(document.retained_bytes(), scratch, other, limit)?;
    job.reserve(scratch)?;
    unsafe { required_profile(bytes, length) }
}

/// Owned JSON admission plan including borrowed ICC bytes and caller live state.
/// Free with dr_string_free. An assign plan (`convert` false) refuses a
/// destination whose matrix/TRC tone curves fail the precision check only for
/// a linear-light document, one holding text/shape artwork or one
/// holding a calibrated (`document_v2`) Exposure/White Balance adjustment,
/// grouped ones included, in Assign words naming the profile; it never
/// refuses for a retained source (the assign itself does). A 32-bit float
/// document answers `float_color`'s plan: a profile with no matrix/TRC
/// model refuses by name for Assign and Convert, failing tone curves refuse
/// an Assign in the float Assign words and a Convert in the destination
/// sentence, and a Convert charges the converted planes at 16 B/px plus the
/// largest once more, plus a quarter.
/// # Safety
/// document NULL/live; icc NULL/readable for length bytes; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_color_plan(
    document: *const PrecisionDocument,
    icc: *const u8,
    length: usize,
    convert: bool,
    limit: u64,
    other: u64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning high-depth color edit",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let profile =
                    target_profile(document, icc, length, limit, other, JobContext::NONE)?;
                color_plan_json(&document.color_plan(&profile, convert)?, length)
            },
            CString::into_raw,
        )
    }
}

/// Owned JSON admission plan of `dr_precision_doc_convert_to_profile_with`
/// under `options_json` (NULL = the defaults), in `dr_precision_doc_color_plan`'s
/// shape and with its profile allowance. A preserve-appearance conversion that
/// bakes adjustments charges the bakes and the baked layers; otherwise
/// the figures equal the convert colour plan's. Free with dr_string_free.
/// # Safety
/// document NULL/live; icc NULL/readable for length bytes; options_json NULL or
/// terminated UTF-8; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_conversion_plan(
    document: *const PrecisionDocument,
    icc: *const u8,
    length: usize,
    options_json: *const c_char,
    limit: u64,
    other: u64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning a high-depth profile conversion",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let options = options_arg(options_json)?;
                let profile =
                    target_profile(document, icc, length, limit, other, JobContext::NONE)?;
                color_plan_json(&document.conversion_plan(&profile, &options)?, length)
            },
            CString::into_raw,
        )
    }
}

/// The two plan exports' JSON: the plan's scratch plus the profile argument's
/// allowance (`target_profile`).
fn color_plan_json(plan: &PrecisionColorPlan, length: usize) -> Result<CString, String> {
    let scratch = plan.scratch_bytes + 5 * length as u64 + BOOKKEEPING_BYTES;
    CString::new(
        serde_json::json!({
            "retained_bytes": plan.retained_bytes,
            "scratch_bytes": scratch,
            "converted_pixel_bytes": plan.converted_pixel_bytes,
        })
        .to_string(),
    )
    .map_err(|e| e.to_string())
}

/// True when the typed document's profile is an RGB matrix/TRC model passing the
/// high-depth precision checks: the precondition of document_v2 calibrated
/// Exposure/White Balance and of a high-depth conversion's DESTINATION (a source
/// that fails them still converts out). False on a NULL document and for a
/// LUT or other unmodelled profile. Typed twin of dr_doc_profile_is_convertible.
///
/// # Safety
/// `document` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_profile_is_convertible(
    document: *const PrecisionDocument,
) -> bool {
    unsafe { handle_get(document, false, |d| precision_model(d.profile()).is_ok()) }
}

/// Assign a captured profile without changing samples. Exact ICC identity is a no-op.
/// Refuses with `dr_precision_doc_assign_refusal`'s sentence while a layer
/// holds a retained source. In a 32-bit float document every f32
/// sample is kept bit for bit on the same storage and relabelled, the tone map
/// is kept, and a profile with no matrix/TRC model or with tone curves that
/// fail the precision check refuses by name.
/// A Grayscale document refuses (`gray_document::RGB_PROFILE_EDIT_REFUSAL`,
/// #1 phase 5a).
/// A Lab Color document refuses (`lab_document::PROFILE_EDIT_REFUSAL`, #1
/// Lab phase 5b): it has no document profile.
/// # Safety
/// document/control NULL/live; icc NULL/readable for length bytes; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_assign_profile(
    document: *const PrecisionDocument,
    icc: *const u8,
    length: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "assigning high-depth profile",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                if document.is_gray() {
                    return Err(crate::gray_document::RGB_PROFILE_EDIT_REFUSAL.into());
                }
                if document.is_lab() {
                    return Err(crate::lab_document::PROFILE_EDIT_REFUSAL.into());
                }
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let profile = target_profile(document, icc, length, limit, other, job)?;
                if let Some(reason) = document.assign_refusal_for(&profile) {
                    return Err(reason);
                }
                let other = other
                    .checked_add(length as u64)
                    .ok_or("high-depth retained byte calculation overflow")?;
                document.color_edit_with_context(&profile, false, limit, other, job)
            },
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Convert stored RGB through the shared ICC kernel, preserving exact source alpha.
/// In a 32-bit float document the conversion is the matrix between the
/// two profiles' primaries on linear RGB in f64, rounded once to f32, with no
/// clamp and alpha kept; a profile with no matrix/TRC model, a destination
/// whose tone curves fail the precision check and a converted component
/// outside ±65504 refuse by name.
/// A Grayscale document refuses (`gray_document::RGB_PROFILE_EDIT_REFUSAL`,
/// #1 phase 5a).
/// A Lab Color document refuses (`lab_document::PROFILE_EDIT_REFUSAL`, #1
/// Lab phase 5b): it has no document profile.
/// # Safety
/// document/control NULL/live; icc NULL/readable for length bytes; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_convert_to_profile(
    document: *const PrecisionDocument,
    icc: *const u8,
    length: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "converting high-depth profile",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                if document.is_gray() {
                    return Err(crate::gray_document::RGB_PROFILE_EDIT_REFUSAL.into());
                }
                if document.is_lab() {
                    return Err(crate::lab_document::PROFILE_EDIT_REFUSAL.into());
                }
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let profile = target_profile(document, icc, length, limit, other, job)?;
                let other = other
                    .checked_add(length as u64)
                    .ok_or("high-depth retained byte calculation overflow")?;
                document.color_edit_with_context(&profile, true, limit, other, job)
            },
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}
