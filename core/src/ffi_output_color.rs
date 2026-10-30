//! C FFI for Phase 11C output colour control: profile description, the
//! conversion policy (preflight, options, report), the two explicit Assign
//! workflows, and soft proofing / the gamut mask at both depths — the
//! "Output color control (Phase 11C)" section of `include/doctored_core.h`.
//!
//! Same conventions as every sibling shim: `catch_unwind` through the
//! `ffi_util` helpers, NULL tolerance first, buffer lengths recomputed from
//! the core's own dimensions, and every string returned on the heap and freed
//! with `dr_string_free`.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::doc_color_policy::ConversionOptions;
use crate::drdc::MAX_DRDC_BLOB_LEN;
use crate::ffi_color::{profile_arg, required_profile};
use crate::ffi_util::{
    borrow_cstr_bounded, boxed, doc_get, fallible_doc_op, fallible_op, required_handle,
    write_string,
};
use crate::icc_lut::ProofProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::PrecisionImage;
use crate::soft_proof::{self, ProofSetup};
use crate::DrImage;

/// Borrows a caller profile buffer; `None` ONLY for a NULL pointer.
///
/// # Safety
/// `bytes` must be NULL or valid for `len` bytes.
unsafe fn blob<'a>(bytes: *const u8, len: usize) -> Option<&'a [u8]> {
    if bytes.is_null() {
        None
    } else {
        Some(unsafe { std::slice::from_raw_parts(bytes, len) })
    }
}

/// A heap JSON string for the host.
fn json_string(value: &serde_json::Value) -> Result<CString, String> {
    CString::new(value.to_string()).map_err(|e| e.to_string())
}

/// Parses the options JSON; a NULL pointer means the defaults.
///
/// # Safety
/// `options_json` must be NULL or a valid NUL-terminated C string.
pub(crate) unsafe fn options_arg(options_json: *const c_char) -> Result<ConversionOptions, String> {
    if options_json.is_null() {
        return Ok(ConversionOptions::DEFAULT);
    }
    let text =
        unsafe { borrow_cstr_bounded(options_json, "options_json", MAX_DRDC_BLOB_LEN as usize)? };
    ConversionOptions::parse(text)
}

/// Parses the proof setup JSON; a NULL pointer means the defaults.
///
/// # Safety
/// `setup_json` must be NULL or a valid NUL-terminated C string.
pub(crate) unsafe fn setup_arg(setup_json: *const c_char) -> Result<ProofSetup, String> {
    if setup_json.is_null() {
        return Ok(ProofSetup::default());
    }
    let text =
        unsafe { borrow_cstr_bounded(setup_json, "setup_json", MAX_DRDC_BLOB_LEN as usize)? };
    ProofSetup::parse(text)
}

/// A proof profile from caller bytes.
///
/// # Safety
/// `bytes` must be NULL or valid for `len` bytes.
pub(crate) unsafe fn proof_arg(bytes: *const u8, len: usize) -> Result<ProofProfile, String> {
    let slice = unsafe { blob(bytes, len) }.ok_or("the proof profile is NULL")?;
    if slice.len() > MAX_DRDC_BLOB_LEN as usize {
        return Err("the proof profile exceeds the 16 MiB limit".into());
    }
    ProofProfile::parse(slice)
}

// ----------------------------------------------------------- description --

/// Everything this build can say about profile `bytes`, as a heap JSON object
/// (free with `dr_string_free`): `name`, `version`, `class`, `space`, `pcs`,
/// `model`, `can_decode`, `can_encode`, `intents`, `media_white`, `proofable`,
/// `refusal`, `proof_refusal` (why it cannot be proofed through — null exactly
/// when `proofable`, naming the missing A2B/B2A direction and its
/// floating-point twin) and `high_depth_destination` (whether a 16/32-bit
/// Convert accepts it as a destination). Unlike `dr_icc_inspect` this reads
/// Gray and CMYK profiles too, because a proof setup may name one. NULL on a
/// NULL buffer.
///
/// # Safety
/// `bytes` must be NULL or valid for `len` bytes; `err_out` must be NULL or a
/// valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_icc_describe(
    bytes: *const u8,
    len: usize,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "describing a profile",
            ptr::null_mut(),
            || {
                let slice = blob(bytes, len).ok_or("the profile buffer is NULL")?;
                json_string(&crate::icc::describe_json(slice))
            },
            CString::into_raw,
        )
    }
}

// ------------------------------------------------------- the byte document --

/// What a conversion of `doc` to the profile in `icc` under `options_json`
/// WOULD do: a heap JSON object `{"adjustments":[…],"options":{…}}` (free
/// with `dr_string_free`). NULL with `err_out` set when the conversion would
/// be refused — that refusal sentence is what a host disables its OK button
/// with. The document's own profile bytes are no change at any options,
/// before every refusal: `"adjustments":[]`. A preserve-appearance
/// check renders each baking projection under `control` (may be NULL; a
/// control is single-use): a cancel returns NULL with `operation cancelled`.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `icc` must
/// be NULL or valid for `len` bytes; `options_json` must be NULL or a valid
/// NUL-terminated C string; `control` must be NULL or a valid pointer to a
/// live `DrJobControl`; `err_out` must be NULL or a valid pointer to
/// writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_conversion_preflight(
    doc: *const DrDocument,
    icc: *const u8,
    len: usize,
    options_json: *const c_char,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "preflighting a profile conversion",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let profile = required_profile(icc, len)?;
                let options = options_arg(options_json)?;
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let report = document.conversion_preflight(&profile, &options, job)?;
                json_string(&serde_json::json!({
                    "adjustments": report.to_json(),
                    "options": options.to_json(),
                }))
            },
            CString::into_raw,
        )
    }
}

/// Converts `doc` into the profile in `icc` under `options_json`, writing the
/// adjustment report through `report_out` (may be NULL) as a heap JSON array.
/// NULL WITHOUT an error is the ordinary refusal (nothing would change —
/// always so for the document's own profile bytes, at any options and before
/// every refusal); NULL WITH one is the preflight's sentence. With
/// `"rasterize_sources":true`, settings that cannot keep the retained
/// sources bound rasterize every bound layer first and convert the result,
/// as one document.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `icc` must
/// be NULL or valid for `len` bytes; `options_json` must be NULL or a valid
/// NUL-terminated C string; `control` must be NULL or a valid pointer to a
/// live `DrJobControl`; `report_out` and `err_out` must each be NULL or a
/// valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_convert_to_profile_with(
    doc: *const DrDocument,
    icc: *const u8,
    len: usize,
    options_json: *const c_char,
    control: *const DrJobControl,
    report_out: *mut *mut c_char,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_op(
            err_out,
            "converting to a profile",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let profile = required_profile(icc, len)?;
                let options = options_arg(options_json)?;
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                document.convert_to_profile_with(&profile, &options, job)
            },
            |result| match result {
                Some((document, report)) => {
                    write_string(report_out, &report.to_json().to_string());
                    boxed(document)
                }
                None => ptr::null_mut(),
            },
        )
    }
}

/// Why an Assign on `doc` would reinterpret a retained source, as a heap JSON
/// object `{"refusal":null|string,"retained_sources":n}`
/// (free with `dr_string_free`). NULL on a NULL doc.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_assign_refusal(doc: *const DrDocument) -> *mut c_char {
    unsafe {
        doc_get(doc, ptr::null_mut(), |d| {
            let value = serde_json::json!({
                "refusal": d.assign_refusal(),
                "retained_sources": d.retained_source_count(),
            });
            CString::new(value.to_string()).ok().map(CString::into_raw)
        })
    }
}

/// Why assigning the profile in `icc` to `doc` would land a document that
/// cannot render: the owned sentence (free with `dr_string_free`) when
/// the profile has no matrix/TRC model and the document holds a
/// `document_v2` Exposure/White Balance layer (named) or uses linear-light
/// evaluation; and when the profile's matrix/TRC tone curves
/// fail the precision check and the document uses linear-light evaluation,
/// holds text/shape artwork or holds a `document_v2` Exposure/White Balance
/// layer (named), in Assign words naming the profile, checked in that order.
/// NULL on a NULL doc, on bytes `dr_doc_assign_profile`
/// refuses as a profile, on the document's current profile bytes, and when
/// there is no refusal. `dr_doc_assign_profile` and
/// `dr_doc_assign_profile_rasterizing` refuse with this sentence in
/// `err_out` whenever it is non-NULL.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `icc` must
/// be NULL or valid for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_assign_profile_refusal(
    doc: *const DrDocument,
    icc: *const u8,
    len: usize,
) -> *mut c_char {
    if doc.is_null() {
        return ptr::null_mut();
    }
    let profile = unsafe { profile_arg(icc, len) };
    unsafe {
        doc_get(doc, ptr::null_mut(), |d| {
            let profile = profile?;
            let refusal = d.assign_profile_refusal(&profile)?;
            CString::new(refusal).ok().map(CString::into_raw)
        })
    }
}

/// Rasterizes every layer still bound to a retained source and then assigns
/// the profile in `icc` — ONE result, which the host commits as ONE undo
/// step. NULL with `err_out` untouched on a NULL doc and when nothing would
/// change. NULL with `err_out` set for the profile-argument sentences
/// `dr_doc_assign_profile` names, with `dr_doc_assign_profile_refusal`'s
/// sentence whenever it is non-NULL, and with the rasterization's own
/// sentence when a bound layer cannot be rasterized (its pixels are locked).
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `icc` must
/// be NULL or valid for `len` bytes; `err_out` must be NULL or a writable
/// CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_assign_profile_rasterizing(
    doc: *const DrDocument,
    icc: *const u8,
    len: usize,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_doc_op(
            doc,
            err_out,
            "assigning a profile after rasterizing sources",
            |d| d.assign_profile_rasterizing(&required_profile(icc, len)?),
        )
    }
}

// ------------------------------------------------------ the typed document --

/// The typed twin of `dr_doc_conversion_preflight`. The JSON object adds the
/// `retained_bytes` / `scratch_bytes` plan the typed admission needs. The
/// document's own profile bytes answer that admitted plan with
/// `"adjustments":[]`, before every other refusal. The baking
/// projections run under `control` as the byte twin's do.
///
/// A 32-bit float document has one conversion, the matrix in linear
/// light with no clamp, at relative, perceptual or saturation intent with
/// black-point compensation off and adjustments `keep`. After the same-bytes
/// rule it refuses the first of absolute colorimetric intent, black-point
/// compensation and preserve-appearance adjustments
/// (`float_color::options_refusal`'s sentence), then the profile refusals of
/// the float colour plan.
///
/// # Safety
/// `document` must be NULL or a valid pointer to a live `DrPrecisionDocument`;
/// `icc` must be NULL or valid for `len` bytes; `options_json` must be NULL or
/// a valid NUL-terminated C string; `control` must be NULL or a valid pointer
/// to a live `DrJobControl`; `err_out` must be NULL or a valid pointer to
/// writable `*mut c_char`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_conversion_preflight(
    document: *const PrecisionDocument,
    icc: *const u8,
    len: usize,
    options_json: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "preflighting a high-depth profile conversion",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let profile = required_profile(icc, len)?;
                let options = options_arg(options_json)?;
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let (plan, report) =
                    document.conversion_preflight(&profile, &options, limit, other, job)?;
                json_string(&serde_json::json!({
                    "retained_bytes": plan.retained_bytes,
                    "scratch_bytes": plan.scratch_bytes,
                    "adjustments": report.to_json(),
                    "options": options.to_json(),
                }))
            },
            CString::into_raw,
        )
    }
}

/// The typed twin of `dr_doc_convert_to_profile_with`, including NULL without
/// an error for the document's own profile bytes after the admission.
///
/// A 32-bit float document converts by the matrix in linear light with
/// no clamp (`dr_precision_doc_convert_to_profile`'s float rule) at relative,
/// perceptual or saturation intent with black-point compensation off and
/// adjustments `keep`, which all give the same samples. After the same-bytes
/// rule, and before any source is rasterized, it refuses the first of
/// absolute colorimetric intent, black-point compensation and
/// preserve-appearance adjustments (`float_color::options_refusal`'s
/// sentence), then the profile refusals of the float colour plan.
/// A Grayscale document refuses (`gray_document::RGB_PROFILE_EDIT_REFUSAL`,
/// #1 phase 5a).
/// A Lab Color document refuses (`lab_document::PROFILE_EDIT_REFUSAL`, #1
/// Lab phase 5b): it has no document profile.
///
/// # Safety
/// `document` must be NULL or a valid pointer to a live `DrPrecisionDocument`;
/// `icc` must be NULL or valid for `len` bytes; `options_json` must be NULL or
/// a valid NUL-terminated C string; `control` must be NULL or a valid pointer
/// to a live `DrJobControl`; `report_out` and `err_out` must each be NULL or a
/// valid pointer to writable `*mut c_char`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_convert_to_profile_with(
    document: *const PrecisionDocument,
    icc: *const u8,
    len: usize,
    options_json: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    report_out: *mut *mut c_char,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "converting a high-depth document to a profile",
            ptr::null_mut(),
            || {
                let handle = required_handle(document)?;
                if handle.is_gray() {
                    return Err(crate::gray_document::RGB_PROFILE_EDIT_REFUSAL.into());
                }
                if handle.is_lab() {
                    return Err(crate::lab_document::PROFILE_EDIT_REFUSAL.into());
                }
                let profile = required_profile(icc, len)?;
                let options = options_arg(options_json)?;
                handle.convert_to_profile_with(
                    &profile,
                    &options,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            |result| match result {
                Some((document, report)) => {
                    write_string(report_out, &report.to_json().to_string());
                    boxed(document)
                }
                None => ptr::null_mut(),
            },
        )
    }
}

/// The typed twin of `dr_doc_assign_refusal`; NULL on a NULL handle.
///
/// # Safety
/// `document` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_assign_refusal(
    document: *const PrecisionDocument,
) -> *mut c_char {
    unsafe {
        crate::ffi_util::handle_get(document, ptr::null_mut(), |d| {
            let value = serde_json::json!({
                "refusal": d.assign_refusal(),
                "retained_sources": d.retained_source_count(),
            });
            CString::new(value.to_string())
                .map(CString::into_raw)
                .unwrap_or(ptr::null_mut())
        })
    }
}

/// The typed twin of `dr_doc_assign_profile_rasterizing`.
/// A Grayscale document refuses (`gray_document::RGB_PROFILE_EDIT_REFUSAL`,
/// #1 phase 5a).
/// A Lab Color document refuses (`lab_document::PROFILE_EDIT_REFUSAL`, #1
/// Lab phase 5b): it has no document profile.
///
/// # Safety
/// `document` must be NULL or a valid pointer to a live `DrPrecisionDocument`;
/// `icc` must be NULL or valid for `len` bytes; `control` must be NULL or a
/// valid pointer to a live `DrJobControl`; `err_out` must be NULL or a valid
/// pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_assign_profile_rasterizing(
    document: *const PrecisionDocument,
    icc: *const u8,
    len: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "assigning a profile to a high-depth document",
            ptr::null_mut(),
            || {
                let handle = required_handle(document)?;
                if handle.is_gray() {
                    return Err(crate::gray_document::RGB_PROFILE_EDIT_REFUSAL.into());
                }
                if handle.is_lab() {
                    return Err(crate::lab_document::PROFILE_EDIT_REFUSAL.into());
                }
                let profile = required_profile(icc, len)?;
                handle.assign_profile_rasterizing(&profile, limit, other, control.as_ref())
            },
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}

// ---------------------------------------------------------- soft proofing --

/// Renders `flat` — a projection whose numbers are in the profile `doc_icc`
/// describes — through the proof profile in `proof_icc` under `setup_json`,
/// returning a NEW image the host draws. `info_out` (may be NULL) receives a
/// heap JSON object `{"space":"document"|"srgb","out_of_gamut":n}`; the space
/// is what the result is TAGGED as. The master is never touched. Admits
/// `flat`'s pixels plus the proof's scratch against `limit` (with `other`
/// retained elsewhere), then begins `control` and reserves that scratch, as
/// the typed twin does; on refusal or cancel `info_out` is left untouched.
///
/// # Safety
/// `flat` must be NULL or a valid pointer to a live `DrImage`; both profile
/// buffers must be NULL or valid for their stated lengths; `setup_json` must
/// be NULL or a valid NUL-terminated C string; `control` must be NULL or a
/// valid pointer to a live `DrJobControl`; `info_out` and `err_out` must each
/// be NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_proof_render(
    flat: *const DrImage,
    doc_icc: *const u8,
    doc_len: usize,
    proof_icc: *const u8,
    proof_len: usize,
    setup_json: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    info_out: *mut *mut c_char,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "rendering a soft proof",
            ptr::null_mut(),
            || {
                let image = required_handle(flat)?;
                let document = required_profile(doc_icc, doc_len)?;
                let proof = proof_arg(proof_icc, proof_len)?;
                let setup = setup_arg(setup_json)?;
                let scratch = byte_proof_scratch(image, doc_len, &proof);
                let retained = image.pixels.as_raw().len() as u64;
                crate::precision_image::admit(retained, scratch, other, limit)?;
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                job.reserve(scratch)?;
                let output =
                    soft_proof::proof_render8(&image.pixels, &document, &proof, &setup, job)?;
                let info = output.info_json();
                let (w, h) = output.image.dimensions();
                let image = DrImage::from_rgba(w, h, output.image.into_raw())
                    .ok_or("the proof produced invalid dimensions")?;
                Ok((info, image))
            },
            |(info, image)| {
                write_string(info_out, &info.to_string());
                boxed(image)
            },
        )
    }
}

/// Writes the per-pixel gamut mask for the same inputs into `out`, which the
/// caller declares to be `len` bytes — 255 out of gamut, 0 inside. The length
/// is recomputed from the image's own dimensions and must match exactly.
/// `count_out` (may be NULL) receives how many pixels were flagged. Admits
/// and reserves the same scratch as `dr_proof_render` against `limit`; on
/// refusal or cancel returns false and leaves `out` and `count_out` untouched.
///
/// # Safety
/// `flat` must be NULL or a valid pointer to a live `DrImage`; both profile
/// buffers must be NULL or valid for their stated lengths; `setup_json` must
/// be NULL or a valid NUL-terminated C string; `control` must be NULL or a
/// valid pointer to a live `DrJobControl`; `out` must be NULL or writable
/// for `len` bytes; `count_out` must be NULL or a valid pointer to a writable
/// `uint64_t`; `err_out` must be NULL or a valid pointer to writable
/// `*mut c_char`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_proof_gamut_mask(
    flat: *const DrImage,
    doc_icc: *const u8,
    doc_len: usize,
    proof_icc: *const u8,
    proof_len: usize,
    setup_json: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    out: *mut u8,
    len: usize,
    count_out: *mut u64,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "computing a gamut mask",
            false,
            || {
                if out.is_null() {
                    return Err("the gamut mask buffer is NULL".into());
                }
                let image = required_handle(flat)?;
                let (w, h) = image.pixels.dimensions();
                // Recomputed from the core's own dimensions, never trusted.
                if len != (w as usize) * (h as usize) {
                    return Err("the gamut mask buffer length does not match the image".into());
                }
                let document = required_profile(doc_icc, doc_len)?;
                let proof = proof_arg(proof_icc, proof_len)?;
                let setup = setup_arg(setup_json)?;
                let scratch = byte_proof_scratch(image, doc_len, &proof);
                let retained = image.pixels.as_raw().len() as u64;
                crate::precision_image::admit(retained, scratch, other, limit)?;
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                job.reserve(scratch)?;
                let output = soft_proof::proof_render8_with(
                    &image.pixels,
                    &document,
                    &proof,
                    &setup,
                    true,
                    job,
                )?;
                let mask = output.mask.ok_or("the gamut mask was not produced")?;
                Ok((mask, output.out_of_gamut))
            },
            |(mask, count)| {
                ptr::copy_nonoverlapping(mask.as_ptr(), out, mask.len());
                if !count_out.is_null() {
                    *count_out = count;
                }
                true
            },
        )
    }
}

/// The typed twin of `dr_proof_render`: an already captured u16 projection in
/// (returning its own profile as the document's), an RGBA8 DISPLAY image out.
/// A projection in the Lab twin refuses
/// (`lab_document::PROOF_VIEW_REFUSAL`, #1 Lab phase 5b): a Lab Color
/// document proofs its `dr_precision_image_lab_display` view.
///
/// # Safety
/// `projection` must be NULL or a valid pointer to a live
/// `DrPrecisionImage`; `proof_icc` must be NULL or valid for `proof_len`
/// bytes; `setup_json` must be NULL or a valid NUL-terminated C string;
/// `control` must be NULL or a valid pointer to a live `DrJobControl`;
/// `info_out` and `err_out` must each be NULL or a valid pointer to writable
/// `*mut c_char`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_proof_render(
    projection: *const PrecisionImage,
    proof_icc: *const u8,
    proof_len: usize,
    setup_json: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    info_out: *mut *mut c_char,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "rendering a high-depth soft proof",
            ptr::null_mut(),
            || {
                let image = required_handle(projection)?;
                let proof = proof_arg(proof_icc, proof_len)?;
                let setup = setup_arg(setup_json)?;
                let scratch = typed_proof_scratch(image, &proof);
                crate::precision_image::admit(image.retained_bytes(), scratch, other, limit)?;
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                job.reserve(scratch)?;
                let output = soft_proof::proof_render16(image, &proof, &setup, job)?;
                let info = output.info_json();
                let (w, h) = output.image.dimensions();
                let image = DrImage::from_rgba(w, h, output.image.into_raw())
                    .ok_or("the proof produced invalid dimensions")?;
                Ok((info, image))
            },
            |(info, image)| {
                write_string(info_out, &info.to_string());
                boxed(image)
            },
        )
    }
}

/// The typed twin of `dr_proof_gamut_mask`. Admits and reserves the same
/// scratch as `dr_precision_proof_render` against `limit`; on refusal or
/// cancel returns false and leaves `out` and `count_out` untouched.
/// A projection in the Lab twin refuses as `dr_precision_proof_render` does.
///
/// # Safety
/// `projection` must be NULL or a valid pointer to a live
/// `DrPrecisionImage`; `proof_icc` must be NULL or valid for `proof_len`
/// bytes; `setup_json` must be NULL or a valid NUL-terminated C string;
/// `control` must be NULL or a valid pointer to a live `DrJobControl`; `out`
/// must be NULL or writable for `len` bytes; `count_out` must be NULL or a
/// valid pointer to a writable `uint64_t`; `err_out` must be NULL or a valid
/// pointer to writable `*mut c_char`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_proof_gamut_mask(
    projection: *const PrecisionImage,
    proof_icc: *const u8,
    proof_len: usize,
    setup_json: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    out: *mut u8,
    len: usize,
    count_out: *mut u64,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "computing a high-depth gamut mask",
            false,
            || {
                if out.is_null() {
                    return Err("the gamut mask buffer is NULL".into());
                }
                let image = required_handle(projection)?;
                let (w, h) = image.dimensions();
                if len != (w as usize) * (h as usize) {
                    return Err("the gamut mask buffer length does not match the image".into());
                }
                let proof = proof_arg(proof_icc, proof_len)?;
                let setup = setup_arg(setup_json)?;
                let scratch = typed_proof_scratch(image, &proof);
                crate::precision_image::admit(image.retained_bytes(), scratch, other, limit)?;
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                job.reserve(scratch)?;
                let output = soft_proof::proof_render16_with(image, &proof, &setup, true, job)?;
                let mask = output.mask.ok_or("the gamut mask was not produced")?;
                Ok((mask, output.out_of_gamut))
            },
            |(mask, count)| {
                ptr::copy_nonoverlapping(mask.as_ptr(), out, mask.len());
                if !count_out.is_null() {
                    *count_out = count;
                }
                true
            },
        )
    }
}

/// A proof's scratch over `pixels`: the RGBA8 display image and u8 mask
/// (5 B/px), the parsed proof profile, and the colour memo's peak.
fn proof_scratch(pixels: u64, proof: &ProofProfile) -> u64 {
    pixels * 5 + proof.bytes() + soft_proof::memo_peak_bytes(pixels)
}

/// A typed proof's scratch: `proof_scratch` over the projection's pixels.
fn typed_proof_scratch(image: &PrecisionImage, proof: &ProofProfile) -> u64 {
    let (w, h) = image.dimensions();
    proof_scratch(u64::from(w) * u64::from(h), proof)
}

/// A byte proof's scratch over the flat image's pixels
/// ([`byte_proof_scratch_pixels`]).
fn byte_proof_scratch(image: &DrImage, doc_len: usize, proof: &ProofProfile) -> u64 {
    let (w, h) = image.pixels.dimensions();
    byte_proof_scratch_pixels(u64::from(w) * u64::from(h), doc_len, proof)
}

/// A byte proof's scratch over `pixels`: `proof_scratch`, plus 10 B per
/// document-profile byte — the caller's parsed document profile and
/// `Proofer::new`'s owned re-parse, each at `target_profile`'s 5 B per profile
/// byte (`ffi_precision_color.rs`). Color Range's Out Of Gamut
/// (`ffi_color_range_gamut`) admits the same figure.
pub(crate) fn byte_proof_scratch_pixels(pixels: u64, doc_len: usize, proof: &ProofProfile) -> u64 {
    proof_scratch(pixels, proof) + 10 * doc_len as u64
}
