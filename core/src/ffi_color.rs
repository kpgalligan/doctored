//! C FFI for colour management and metadata (`dr_builtin_profile*`,
//! `dr_icc_inspect`, `dr_doc_*profile*`, `dr_doc_metadata*`,
//! `dr_doc_*resolution*`, `dr_format_carries`, `dr_path_metadata_walked`,
//! `dr_doc_save_image`) — the "Colour management and metadata" section of
//! `include/doctored_core.h`.
//! Same conventions as the sibling shims: `catch_unwind` through the
//! `ffi_util` helpers, NULL tolerance first, and buffer lengths recomputed
//! from the core's own stored length, never trusted from the caller.
//!
//! Every enum parameter is declared `c_int` and mapped through a `from_c`
//! (the `ffi_channel::Plane::from_c` reason applies verbatim): callers do
//! pass values outside an enum, and materializing one from an out-of-range
//! discriminant would be undefined behaviour.

use std::ffi::{c_char, c_int, CString};
use std::panic::catch_unwind;
use std::ptr;
use std::sync::Arc;

use crate::doc::DrDocument;
use crate::doc_color::{format_carries, metadata_walked, AdoptOutcome, MetadataKind};
use crate::dr_image::Format;
use crate::drdc::MAX_DRDC_BLOB_LEN;
use crate::ffi_util::{boxed, doc_get, fallible_doc_op, fallible_op, read_cstr};
use crate::icc::{self, IccProfile, Inspect};
use crate::job_control::DrJobControl;
use crate::DrImage;

/// The `DR_ICC_*` classification values.
const ICC_NOT_ICC: c_int = 0;
const ICC_NOT_RGB: c_int = 1;
const ICC_RGB_UNCONVERTIBLE: c_int = 2;
const ICC_RGB_MATRIX: c_int = 3;
const ICC_NOT_IMAGE_PROFILE: c_int = 4;
const ICC_RGB_LUT: c_int = 5;

/// The `DR_ADOPT_*` outcome values.
const ADOPT_UNCHANGED: c_int = 0;
const ADOPT_CONVERTED: c_int = 1;
const ADOPT_KEPT_UNCONVERTIBLE: c_int = 2;

fn inspect_to_c(value: Inspect) -> c_int {
    match value {
        Inspect::NotIcc => ICC_NOT_ICC,
        Inspect::NotRgb => ICC_NOT_RGB,
        Inspect::NotImageProfile => ICC_NOT_IMAGE_PROFILE,
        Inspect::RgbUnconvertible => ICC_RGB_UNCONVERTIBLE,
        Inspect::RgbMatrix => ICC_RGB_MATRIX,
        Inspect::RgbLut => ICC_RGB_LUT,
    }
}

fn adopt_to_c(value: AdoptOutcome) -> c_int {
    match value {
        AdoptOutcome::Unchanged => ADOPT_UNCHANGED,
        AdoptOutcome::Converted => ADOPT_CONVERTED,
        AdoptOutcome::KeptUnconvertible => ADOPT_KEPT_UNCONVERTIBLE,
    }
}

/// A heap C string for the host, freed with `dr_string_free`; interior NULs
/// are replaced so the conversion cannot fail. (`ffi_channel`'s
/// `dr_doc_channel_name` is the same five lines inline; this is the only
/// other place a name crosses the boundary.)
fn heap_string(s: &str, fallback: &str) -> *mut c_char {
    let sanitized = s.replace('\0', " ");
    CString::new(sanitized)
        .unwrap_or_else(|_| CString::new(fallback).expect("static string"))
        .into_raw()
}

/// Borrows a caller buffer as a slice; `None` ONLY for a NULL pointer. A
/// non-NULL pointer with a zero length is an empty slice, not an absent one,
/// so a caller can store an empty packet and NULL keeps its one meaning:
/// clear.
///
/// # Safety
/// `bytes` must be NULL or valid for `len` bytes.
pub(crate) unsafe fn blob<'a>(bytes: *const u8, len: usize) -> Option<&'a [u8]> {
    if bytes.is_null() {
        None
    } else {
        // `from_raw_parts` accepts a zero length for any non-NULL, aligned
        // pointer, and u8 is aligned everywhere.
        Some(unsafe { std::slice::from_raw_parts(bytes, len) })
    }
}

/// A profile a setter may store: an RGB ICC profile no larger than the DRDC
/// blob cap. `None` is a refusal, never an error.
///
/// The cap appears twice on purpose, and the two say different things. Here
/// it refuses the ARGUMENT whole, so a host handing over 20 MiB is told no
/// rather than quietly given the 3 KB profile hiding at the front of it;
/// inside `IccProfile::parse` it bounds what a document may ever STORE, on
/// every path a profile enters one — this setter, the DRDC reader, and
/// `DrDocument::open`, which lifts whatever a decoder inflated out of a file.
///
/// # Safety
/// `bytes` must be NULL or valid for `len` bytes.
pub(crate) unsafe fn profile_arg(bytes: *const u8, len: usize) -> Option<Arc<IccProfile>> {
    let slice = unsafe { blob(bytes, len) }?;
    if slice.len() > MAX_DRDC_BLOB_LEN as usize {
        return None;
    }
    IccProfile::parse(slice).map(Arc::new)
}

/// The profile-argument sentences, checked without reading the buffer: NULL
/// is not a profile, and a buffer past the DRDC blob cap is refused whole
/// (`profile_arg`'s reason). The ONE home of both sentences, at both depths,
/// for every export that takes a document profile — the typed
/// `target_profile` calls it before admission, so an argument that can never
/// be carried is refused before anything is reserved.
pub(crate) fn check_profile_arg(bytes: *const u8, len: usize) -> Result<(), String> {
    if bytes.is_null() {
        return Err(NOT_A_PROFILE.into());
    }
    if len > MAX_DRDC_BLOB_LEN as usize {
        return Err("the profile exceeds the 16 MiB limit".into());
    }
    Ok(())
}

/// The sentence for bytes that are not an RGB ICC profile a document can
/// carry — a NULL buffer included.
const NOT_A_PROFILE: &str = "the profile is not an RGB ICC profile a document can carry";

/// A document profile from caller bytes, as an error rather than a refusal:
/// `check_profile_arg`'s two sentences, then the parse.
///
/// # Safety
/// `bytes` must be NULL or valid for `len` bytes.
pub(crate) unsafe fn required_profile(
    bytes: *const u8,
    len: usize,
) -> Result<Arc<IccProfile>, String> {
    check_profile_arg(bytes, len)?;
    unsafe { profile_arg(bytes, len) }.ok_or_else(|| NOT_A_PROFILE.into())
}

// -------------------------------------------- the profiles this build writes --

/// Byte length of a built-in profile; 0 for a value outside
/// `DrBuiltinProfile`.
#[no_mangle]
pub extern "C" fn dr_builtin_profile_len(which: c_int) -> usize {
    catch_unwind(|| IccProfile::builtin_from_c(which).map_or(0, |p| p.bytes().len())).unwrap_or(0)
}

/// Copies a built-in profile into `out`, which the caller declares to be
/// `len` bytes. The length is recomputed from the core's own and must match
/// exactly; false on a NULL buffer, a length that disagrees, or a value
/// outside `DrBuiltinProfile`.
///
/// # Safety
/// `out` must be NULL or writable for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_builtin_profile(which: c_int, out: *mut u8, len: usize) -> bool {
    if out.is_null() {
        return false;
    }
    catch_unwind(|| {
        let Some(profile) = IccProfile::builtin_from_c(which) else {
            return false;
        };
        let bytes = profile.bytes();
        if bytes.len() != len {
            return false;
        }
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len()) };
        true
    })
    .unwrap_or(false)
}

/// Classifies `bytes` as one of the `DR_ICC_*` values and, when `name_out`
/// is non-NULL, writes the profile's display name there as a heap string
/// freed with `dr_string_free` (NULL for `DR_ICC_NOT_ICC`).
///
/// # Safety
/// `bytes` must be NULL or valid for `len` bytes; `name_out` must be NULL or
/// a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_icc_inspect(
    bytes: *const u8,
    len: usize,
    name_out: *mut *mut c_char,
) -> c_int {
    let (kind, name) = catch_unwind(|| {
        let Some(slice) = (unsafe { blob(bytes, len) }) else {
            return (ICC_NOT_ICC, None);
        };
        let mut name = String::new();
        let kind = icc::inspect(slice, &mut name);
        let name = (kind != Inspect::NotIcc).then_some(name);
        (inspect_to_c(kind), name)
    })
    .unwrap_or((ICC_NOT_ICC, None));
    if !name_out.is_null() {
        let value = name.map_or(ptr::null_mut(), |n| heap_string(&n, "Untitled profile"));
        unsafe { *name_out = value };
    }
    kind
}

/// True when both buffers are matrix/TRC RGB profiles describing the SAME
/// colour space — the question `dr_doc_convert_to_profile` refuses on, asked
/// without a document. False on a NULL pointer, on bytes that are not an RGB
/// ICC profile, and on a profile this library cannot model: those are other
/// refusals, and `dr_icc_inspect` is what names them.
///
/// # Safety
/// `a` and `b` must each be NULL or valid for their stated length.
#[no_mangle]
pub unsafe extern "C" fn dr_icc_describes_same_space(
    a: *const u8,
    a_len: usize,
    b: *const u8,
    b_len: usize,
) -> bool {
    catch_unwind(|| {
        let (Some(a), Some(b)) = (unsafe { blob(a, a_len) }, unsafe { blob(b, b_len) }) else {
            return false;
        };
        icc::describes_same_space(a, b)
    })
    .unwrap_or(false)
}

// ------------------------------------------------- the document's profile --

/// Byte length of the document's ICC profile; 0 on a NULL doc. A document
/// always has a profile, so this is never 0 for a live one.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_icc_profile_len(doc: *const DrDocument) -> usize {
    unsafe { doc_get(doc, 0, |d| Some(d.profile.bytes().len())) }
}

/// Copies the document's ICC profile into `out`, which the caller declares
/// to be `len` bytes; false on a NULL doc, a NULL buffer, or a `len` that
/// disagrees with the core's own length.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `out` must
/// be NULL or writable for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_icc_profile(
    doc: *const DrDocument,
    out: *mut u8,
    len: usize,
) -> bool {
    if out.is_null() {
        return false;
    }
    unsafe {
        doc_get(doc, false, |d| {
            let bytes = d.profile.bytes();
            // Recomputed from the core's own stored length, never trusted
            // from the caller.
            if bytes.len() != len {
                return None;
            }
            ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len());
            Some(true)
        })
    }
}

/// Heap copy of the document profile's display name (free with
/// `dr_string_free`); NULL on a NULL doc.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_profile_name(doc: *const DrDocument) -> *mut c_char {
    unsafe {
        doc_get(doc, ptr::null_mut(), |d| {
            Some(heap_string(d.profile.name(), "Untitled profile"))
        })
    }
}

/// True when the document's profile is an RGB **matrix/TRC** profile. False
/// on a NULL doc and for a LUT-based profile, which is kept and re-embedded
/// but has no matrix model.
///
/// This keeps its exact pre-11C meaning on purpose: `document_v2` Exposure
/// and White Balance need a matrix/TRC document and read this bit. Ask
/// `dr_doc_profile_can_decode` for the wider "this build can convert the
/// document's colour" question a LUT-tagged document now answers true.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_profile_is_convertible(doc: *const DrDocument) -> bool {
    unsafe { doc_get(doc, false, |d| Some(d.profile.model().is_some())) }
}

/// True when this build can turn the document's pixel numbers into PCS
/// colour: a matrix/TRC profile, or a LUT profile with a device→PCS table
/// (Phase 11C). False on a NULL doc. This is the Convert to Profile SOURCE
/// gate; see `dr_doc_profile_is_convertible` for the narrower one.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_profile_can_decode(doc: *const DrDocument) -> bool {
    unsafe { doc_get(doc, false, |d| Some(d.profile.can_decode())) }
}

/// Reinterprets the document in the profile `bytes` describe: pixels
/// unchanged, profile replaced. NULL with `err_out` untouched on a NULL doc
/// and on bytes identical to the document's current profile (no change).
/// NULL with `err_out` set, the typed pair's sentences, for a NULL buffer or
/// bytes that are not an RGB ICC profile ("the profile is not an RGB ICC
/// profile a document can carry") and a payload over 16 MiB ("the profile
/// exceeds the 16 MiB limit"); then with `dr_doc_assign_refusal`'s sentence
/// for a document holding a retained source, and with
/// `dr_doc_assign_profile_refusal`'s sentence whenever that function is
/// non-NULL. `bytes` NULL is NOT a clear: a document always
/// has a profile, so pass the sRGB built-in to reset it.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `bytes`
/// must be NULL or valid for `len` bytes; `err_out` must be NULL or a
/// writable CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_assign_profile(
    doc: *const DrDocument,
    bytes: *const u8,
    len: usize,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_doc_op(doc, err_out, "assigning a profile", |d| {
            d.assign_profile(&required_profile(bytes, len)?)
        })
    }
}

/// Transforms every layer's pixels into the profile `bytes` describe and
/// replaces the profile, so the picture looks the same and its numbers
/// change. Masks, alpha channels and layer metadata are untouched. NULL with
/// `err_out` untouched on a NULL doc and for a target equivalent to the
/// document's current space (no change). NULL with `err_out` set for the
/// profile-argument sentences `dr_doc_assign_profile` names; for anything
/// `dr_doc_conversion_preflight` refuses at the default options, in its
/// words (among them a retained source under a profile with no matrix/TRC
/// model, and a `document_v2` Exposure/White Balance layer or linear-light
/// evaluation carried into a profile with no matrix/TRC model); and for a
/// source profile that cannot decode or a destination that cannot encode.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `bytes`
/// must be NULL or valid for `len` bytes; `err_out` must be NULL or a
/// writable CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_convert_to_profile(
    doc: *const DrDocument,
    bytes: *const u8,
    len: usize,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_doc_op(doc, err_out, "converting to a profile", |d| {
            d.convert_to_profile(&required_profile(bytes, len)?)
        })
    }
}

/// Brings a freshly opened document into the host's working space, once:
/// converts when the two differ, does nothing when they agree, and keeps the
/// document's own profile when it is one this library cannot convert from.
/// `outcome_out` (may be NULL) receives one of the `DR_ADOPT_*` values and is
/// written EVEN WHEN the return is NULL, so a "nothing changed" adoption
/// still says which of the three cases it was.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `bytes`
/// must be NULL or valid for `len` bytes; `outcome_out` must be NULL or a
/// valid pointer to a writable `int`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_adopt_working_space(
    doc: *const DrDocument,
    bytes: *const u8,
    len: usize,
    outcome_out: *mut c_int,
) -> *mut DrDocument {
    let working = unsafe { profile_arg(bytes, len) };
    let (next, outcome) = unsafe {
        doc_get(doc, (None, ADOPT_UNCHANGED), |d| {
            let working = working.as_ref()?;
            let (next, outcome) = d.adopt_working_space(working);
            Some((next, adopt_to_c(outcome)))
        })
    };
    if !outcome_out.is_null() {
        unsafe { *outcome_out = outcome };
    }
    next.map_or(ptr::null_mut(), |d| Box::into_raw(Box::new(d)))
}

// ------------------------------------------------------ metadata packets --

/// Byte length of one metadata packet; 0 on a NULL doc, an absent packet or
/// a value outside `DrMetadataKind`.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_metadata_len(doc: *const DrDocument, kind: c_int) -> usize {
    unsafe {
        doc_get(doc, 0, |d| {
            Some(packet(d, MetadataKind::from_c(kind)?)?.len())
        })
    }
}

/// Copies one metadata packet into `out`, which the caller declares to be
/// `len` bytes; false on a NULL doc, a NULL buffer, an absent packet, a
/// value outside `DrMetadataKind`, or a `len` that disagrees with the core's
/// own length.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `out` must
/// be NULL or writable for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_metadata(
    doc: *const DrDocument,
    kind: c_int,
    out: *mut u8,
    len: usize,
) -> bool {
    if out.is_null() {
        return false;
    }
    unsafe {
        doc_get(doc, false, |d| {
            let bytes = packet(d, MetadataKind::from_c(kind)?)?;
            if bytes.len() != len {
                return None;
            }
            ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len());
            Some(true)
        })
    }
}

fn packet(doc: &DrDocument, kind: MetadataKind) -> Option<&[u8]> {
    let meta = &doc.metadata;
    match kind {
        MetadataKind::Exif => meta.exif.as_deref(),
        MetadataKind::Xmp => meta.xmp.as_deref(),
        MetadataKind::Iptc => meta.iptc.as_deref(),
    }
}

/// Stores one metadata packet verbatim, or CLEARS it when `bytes` is NULL —
/// a zero `len` with a non-NULL pointer stores an EMPTY packet, which the
/// format distinguishes from an absent one. NULL on a NULL doc or a value
/// the document already carries, with err_out untouched; NULL with err_out
/// set for a value outside `DrMetadataKind` or a payload over 16 MiB (the
/// DRDC writer's blob cap, enforced here so a document can never hold a
/// packet the format would refuse).
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `bytes`
/// must be NULL or valid for `len` bytes; `err_out` must be NULL or a
/// writable CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_set_metadata(
    doc: *const DrDocument,
    kind: c_int,
    bytes: *const u8,
    len: usize,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    if doc.is_null() {
        return ptr::null_mut();
    }
    let value = unsafe { blob(bytes, len) };
    unsafe {
        fallible_op(
            err_out,
            "setting metadata",
            ptr::null_mut(),
            || (*doc).set_metadata(MetadataKind::from_c_checked(kind)?, value),
            |r| r.map_or(ptr::null_mut(), boxed),
        )
    }
}

// ----------------------------------------------------------- resolution --

/// The document's horizontal print resolution in ppi; 0.0 on a NULL doc.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_resolution_x(doc: *const DrDocument) -> f32 {
    unsafe { doc_get(doc, 0.0, |d| Some(d.resolution.x)) }
}

/// The document's vertical print resolution in ppi; 0.0 on a NULL doc.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_resolution_y(doc: *const DrDocument) -> f32 {
    unsafe { doc_get(doc, 0.0, |d| Some(d.resolution.y)) }
}

/// Pure setter for the document's print resolution. NULL on a NULL doc or
/// no change after sanitizing (clamped to [1, 30000], quantized to four
/// decimals like the global light, so a reported value echoed back is "no
/// change"), with err_out untouched; NULL with err_out set for a non-finite
/// or non-positive component. Pixels never change; only the print size does.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `err_out`
/// must be NULL or a writable CString pointer, freed with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_set_resolution(
    doc: *const DrDocument,
    ppi_x: f32,
    ppi_y: f32,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    if doc.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "setting print resolution",
            ptr::null_mut(),
            || (*doc).set_resolution(ppi_x, ppi_y),
            |r| r.map_or(ptr::null_mut(), boxed),
        )
    }
}

// ----------------------------------------------------- saving a flat file --

/// Which of the profile, the three packets and the resolution `format` is
/// able to carry, as a bitmask of the `DR_CARRIES_*` values; 0 for a value
/// outside `DrFormat`.
#[no_mangle]
pub extern "C" fn dr_format_carries(format: c_int) -> u32 {
    // Pure arithmetic that cannot panic; the guard is the family's uniform
    // shape, not a defence against a known unwind.
    catch_unwind(|| Format::from_c(format).map_or(0, format_carries)).unwrap_or(0)
}

/// True when opening `path` would preserve whatever EXIF, XMP and IPTC the
/// file holds: a JPEG, a PNG or a native `.rz`. False for every other
/// container — a TIFF, WebP, GIF, BMP or PSD arrives as pixels and drops its
/// capture data — for a file that cannot be read, and for a NULL path.
///
/// Ask it beside the open, so a host can say the capture data was never read
/// in rather than implying the file carried none.
///
/// # Safety
/// `path` must be NULL or a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn dr_path_metadata_walked(path: *const c_char) -> bool {
    catch_unwind(|| {
        let Ok(path) = (unsafe { read_cstr(path, "path") }) else {
            return false;
        };
        metadata_walked(&path)
    })
    .unwrap_or(false)
}

/// Encodes the document to `path` as a flat image, embedding its colour
/// profile (unless `embed_profile` is false) and its EXIF, XMP and IPTC
/// packets (unless `strip_metadata` is true), format permitting, plus its
/// print resolution wherever the format has somewhere to put one.
///
/// `flat` may be NULL, in which case the document is flattened here; when it
/// is non-NULL its pixels are what gets encoded and the document supplies
/// only the profile, the packets and the resolution. `flat` is a
/// caller-supplied composite of THIS document; passing an unrelated image is
/// a caller bug, and the canvas dimensions are not re-checked.
///
/// `carried_out` (may be NULL) receives the `DR_CARRIES_*` bits actually
/// written. A blob the format cannot carry, one too large for its segment,
/// or an 8BIM run that filters to nothing is DROPPED, not an error. Atomic
/// like `dr_image_save`. `control` is optional and single-use, begun once for
/// the whole save; a cancel answers exactly `operation cancelled` and keeps
/// the previous file.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `flat` must
/// be NULL or a valid pointer to a live `DrImage`; `path` must be NULL or a
/// valid NUL-terminated C string; `carried_out` must be NULL or a valid
/// pointer to a writable `uint32_t`; `control` must be NULL or a live
/// single-use token; `err_out` must be NULL or a valid pointer to writable
/// `*mut c_char`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_doc_save_image(
    doc: *const DrDocument,
    flat: *const DrImage,
    path: *const c_char,
    format: c_int,
    jpeg_quality: u8,
    embed_profile: bool,
    strip_metadata: bool,
    carried_out: *mut u32,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    let body = || {
        if doc.is_null() {
            return Err("document is NULL".to_string());
        }
        let document = unsafe { &*doc };
        let image = if flat.is_null() {
            None
        } else {
            Some(unsafe { &*flat })
        };
        let path = unsafe { read_cstr(path, "path") }?;
        let format =
            Format::from_c(format).ok_or_else(|| format!("unknown format value {format}"))?;
        document.save_image(
            image,
            &path,
            format,
            jpeg_quality,
            embed_profile,
            strip_metadata,
            unsafe { control.as_ref() },
        )
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while saving image",
            false,
            body,
            |carried| {
                if !carried_out.is_null() {
                    *carried_out = carried;
                }
                true
            },
        )
    }
}
