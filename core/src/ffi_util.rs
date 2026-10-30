//! Shared plumbing for the FFI shims (`ffi`, `ffi_doc`, `ffi_filters`,
//! `ffi_style`, `ffi_agent`, and the TYPED shims
//! `ffi_precision_metadata` / `ffi_guide`, which share `precision_op`):
//! error reporting through
//! `err_out`, panic-safe wrappers, and the argument mappings every shim
//! needs. Nothing
//! here is exported through the C header — these are the helpers the
//! exported functions are built from, so the conventions (catch_unwind
//! everywhere, NULL tolerance, heap CString errors freed with
//! `dr_string_free`) live in ONE place.

use std::ffi::{c_char, c_int, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

use image::imageops::FilterType;

use crate::doc::DrDocument;
use crate::drdc::MAX_DRDC_LAYERS;
use crate::precision_document::PrecisionDocument;
use crate::DrImage;

/// Stores a heap-allocated copy of `msg` through `err_out` (if non-NULL).
/// Interior NUL bytes are replaced so the `CString` conversion cannot fail.
///
/// # Safety
/// `err_out` must be NULL or a valid pointer to writable `*mut c_char`.
pub(crate) unsafe fn set_err(err_out: *mut *mut c_char, msg: &str) {
    if err_out.is_null() {
        return;
    }
    let sanitized = msg.replace('\0', " ");
    let cstring = CString::new(sanitized)
        .unwrap_or_else(|_| CString::new("doctored-core error").expect("static string"));
    unsafe {
        *err_out = cstring.into_raw();
    }
}

/// Writes an owned heap copy of `text` (JSON, so it holds no NUL) through an
/// optional out-parameter; the host frees it with `dr_string_free`. A NULL
/// `slot` writes nothing.
///
/// # Safety
/// `slot` must be NULL or a valid pointer to writable `*mut c_char`.
pub(crate) unsafe fn write_string(slot: *mut *mut c_char, text: &str) {
    if slot.is_null() {
        return;
    }
    let text = CString::new(text).unwrap_or_else(|_| CString::new("{}").expect("static string"));
    unsafe { *slot = text.into_raw() };
}

/// Reads a required C-string argument into an owned `String`, naming the
/// argument in the error ("`what` is NULL" / "`what` is not valid UTF-8").
///
/// # Safety
/// `ptr` must be NULL or a valid NUL-terminated C string.
pub(crate) unsafe fn read_cstr(ptr: *const c_char, what: &str) -> Result<String, String> {
    unsafe { borrow_cstr_bounded(ptr, what, usize::MAX) }.map(str::to_owned)
}

/// Borrow a required UTF-8 argument without copying a potentially large
/// operator description before admission. The byte cap is checked first.
///
/// # Safety
/// ptr is NULL or NUL-terminated and remains readable for the returned lifetime.
pub(crate) unsafe fn borrow_cstr_bounded<'a>(
    ptr: *const c_char,
    what: &str,
    max_bytes: usize,
) -> Result<&'a str, String> {
    if ptr.is_null() {
        return Err(format!("{what} is NULL"));
    }
    let string = unsafe { CStr::from_ptr(ptr) };
    if string.to_bytes().len() > max_bytes {
        return Err(format!("{what} exceeds the supported byte limit"));
    }
    string
        .to_str()
        .map_err(|_| format!("{what} is not valid UTF-8"))
}

/// Boxes a value and leaks it as the raw handle the C side owns.
pub(crate) fn boxed<T>(value: T) -> *mut T {
    Box::into_raw(Box::new(value))
}

/// Borrow a required typed handle inside a fallible FFI body. Distinct opaque
/// handle types keep precision sources out of legacy byte operations.
///
/// # Safety
/// `value` must be NULL or a live, aligned pointer to `T` for the borrow.
pub(crate) unsafe fn required_handle<'a, T>(value: *const T) -> Result<&'a T, String> {
    unsafe { value.as_ref() }.ok_or_else(|| "required handle is NULL".into())
}

/// Release one owned opaque handle, with the same panic boundary for all types.
///
/// # Safety
/// `value` must be NULL or an unfreed pointer returned by `boxed::<T>`.
pub(crate) unsafe fn free_handle<T>(value: *mut T) {
    if !value.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { Box::from_raw(value) })));
    }
}

/// Query a typed immutable handle; NULL or a panic returns the supplied default.
///
/// # Safety
/// `value` must be NULL or a live, aligned pointer to `T` for the call.
pub(crate) unsafe fn handle_get<T, R>(
    value: *const T,
    default: R,
    query: impl FnOnce(&T) -> R,
) -> R {
    match unsafe { value.as_ref() } {
        Some(value) => catch_unwind(AssertUnwindSafe(|| query(value))).unwrap_or(default),
        None => default,
    }
}

/// Runs a fallible FFI body under [`catch_unwind`], mapping `Ok` through
/// `success` and reporting failures through `err_out`: an `Err` message
/// passes through verbatim, a caught panic becomes the long-form
/// "internal error: `what`". Either failure returns `failure`.
///
/// # Safety
/// `err_out` must be NULL or a valid pointer to writable `*mut c_char`.
pub(crate) unsafe fn fallible_op<T, R>(
    err_out: *mut *mut c_char,
    what: &str,
    failure: R,
    body: impl FnOnce() -> Result<T, String>,
    success: impl FnOnce(T) -> R,
) -> R {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(value)) => success(value),
        Ok(Err(msg)) => {
            unsafe { set_err(err_out, &msg) };
            failure
        }
        Err(_) => {
            unsafe { set_err(err_out, &format!("internal error: {what}")) };
            failure
        }
    }
}

/// Runs a producer that takes no handle, boxing what it makes: `None` or a
/// panic yield NULL. The constructor twin of [`pure_op`] — for exports whose
/// only inputs are plain values (`dr_image_from_rgba`) — and the one place
/// the produce-or-NULL mapping is written.
pub(crate) fn produce_op<T>(op: impl FnOnce() -> Option<T>) -> *mut T {
    match catch_unwind(AssertUnwindSafe(op)) {
        Ok(Some(result)) => boxed(result),
        _ => ptr::null_mut(),
    }
}

/// Runs a pure operation against `img`, boxing the produced image.
/// NULL input, `None`, or a panic all yield NULL.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
pub(crate) unsafe fn pure_op<F>(img: *const DrImage, op: F) -> *mut DrImage
where
    F: FnOnce(&DrImage) -> Option<DrImage>,
{
    if img.is_null() {
        return ptr::null_mut();
    }
    let image = unsafe { &*img };
    produce_op(|| op(image))
}

/// Runs a pure operation against `doc`, boxing the produced document.
/// NULL input, `None`, a panic, or growth past native capacity all yield NULL.
/// Existing oversized documents may still make edits that hold or reduce
/// capacity, keeping cleanup and flattened recovery available. Its `err_out`
/// twin, for an op that names its refusals, is [`fallible_doc_op`].
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
pub(crate) unsafe fn doc_op<F>(doc: *const DrDocument, op: F) -> *mut DrDocument
where
    F: FnOnce(&DrDocument) -> Option<DrDocument>,
{
    if doc.is_null() {
        return ptr::null_mut();
    }
    let document = unsafe { &*doc };
    match catch_unwind(AssertUnwindSafe(|| op(document))) {
        Ok(Some(mut result)) if document.native_edit_fits(&result) => {
            result.detach_invalid_sources();
            Box::into_raw(Box::new(result))
        }
        _ => ptr::null_mut(),
    }
}

/// [`doc_op`]'s `err_out` twin: the same capacity gate and stale-binding
/// cleanup, for an op that names its refusals. NULL input yields NULL with
/// `err_out` untouched; `Ok(None)` yields NULL with no message (a no-change
/// edit, so the host registers no undo step); `Err` yields NULL with the
/// message verbatim; a panic yields "internal error: `what`". An `Ok(Some)`
/// result that grows past native capacity is refused here, the one home of
/// that sentence; otherwise its invalid sources are detached before it is
/// boxed. Both post-steps run inside [`fallible_op`]'s `catch_unwind`.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `err_out`
/// must be NULL or a valid pointer to writable `*mut c_char`.
pub(crate) unsafe fn fallible_doc_op<F>(
    doc: *const DrDocument,
    err_out: *mut *mut c_char,
    what: &str,
    op: F,
) -> *mut DrDocument
where
    F: FnOnce(&DrDocument) -> Result<Option<DrDocument>, String>,
{
    if doc.is_null() {
        return ptr::null_mut();
    }
    let document = unsafe { &*doc };
    unsafe {
        fallible_op(
            err_out,
            what,
            ptr::null_mut(),
            || match op(document)? {
                Some(result) if !document.native_edit_fits(&result) => {
                    Err("the edited document exceeds native capacity".into())
                }
                Some(mut result) => {
                    result.detach_invalid_sources();
                    Ok(Some(result))
                }
                None => Ok(None),
            },
            |result| result.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// [`doc_op`] for TYPED documents: NULL input, `None` and a panic all yield
/// NULL. The ONE such wrapper in the crate — every typed shim that answers
/// `Option<PrecisionDocument>` goes through it.
///
/// There is deliberately NO native-capacity re-check here, unlike
/// [`doc_op`]. The edits that reach it change no plane — the metadata slot,
/// the two ppi scalars, a guide list or the ruler origin — and each is
/// already bounded by the core rules the byte twins enforce (guides by
/// `doc_guide::MAX_GUIDES`, which the typed writer checks again).
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `PrecisionDocument`.
pub(crate) unsafe fn precision_op<F>(doc: *const PrecisionDocument, op: F) -> *mut PrecisionDocument
where
    F: FnOnce(&PrecisionDocument) -> Option<PrecisionDocument>,
{
    if doc.is_null() {
        return ptr::null_mut();
    }
    let document = unsafe { &*doc };
    match catch_unwind(AssertUnwindSafe(|| op(document))) {
        Ok(Some(result)) => boxed(result),
        _ => ptr::null_mut(),
    }
}

/// Runs a pure query against `doc`, returning `default` for NULL input,
/// `None`, or a panic.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
pub(crate) unsafe fn doc_get<T, F>(doc: *const DrDocument, default: T, get: F) -> T
where
    F: FnOnce(&DrDocument) -> Option<T>,
{
    if doc.is_null() {
        return default;
    }
    let document = unsafe { &*doc };
    match catch_unwind(AssertUnwindSafe(|| get(document))) {
        Ok(Some(value)) => value,
        _ => default,
    }
}

/// The image twin of [`doc_get`]: a pure query against `img`, returning
/// `default` for NULL input, `None`, or a panic.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
pub(crate) unsafe fn img_get<T, F>(img: *const DrImage, default: T, get: F) -> T
where
    F: FnOnce(&DrImage) -> Option<T>,
{
    if img.is_null() {
        return default;
    }
    let image = unsafe { &*img };
    match catch_unwind(AssertUnwindSafe(|| get(image))) {
        Ok(Some(value)) => value,
        _ => default,
    }
}

/// Reads an optional caller buffer pointer (a canvas-sized selection mask, a
/// canvas-sized paint overlay, a canvas-sized plane) into a slice of exactly
/// `len` bytes. `len` is always derived from the core's own dimensions, never
/// from the caller.
///
/// # Safety
/// `buffer` must be NULL or valid for `len` bytes for the duration of the
/// caller.
pub(crate) unsafe fn mask_slice<'a>(buffer: *const u8, len: usize) -> Option<&'a [u8]> {
    if buffer.is_null() {
        None
    } else {
        Some(unsafe { std::slice::from_raw_parts(buffer, len) })
    }
}

/// A caller's list of layer indices, refused WHOLE (`None`) when the pointer
/// is NULL, `len` is 0 or past the format's layer cap, an index is not below
/// `count`, or an index repeats. A repeat is an error rather than a silent
/// dedupe because every set op renumbers: a caller that named an entry twice
/// has miscounted something, and answering it with a quietly different set
/// would hide that. Ascending on the way out, which is the order every
/// structural op wants.
///
/// # Safety
/// `ptr` must be NULL or valid for `len` `size_t` values for the duration of
/// the call.
pub(crate) unsafe fn index_slice(
    ptr: *const usize,
    len: usize,
    count: usize,
) -> Option<Vec<usize>> {
    if ptr.is_null() || len == 0 || len > MAX_DRDC_LAYERS as usize {
        return None;
    }
    let raw = unsafe { std::slice::from_raw_parts(ptr, len) };
    let mut out = raw.to_vec();
    out.sort_unstable();
    if out.windows(2).any(|w| w[0] == w[1]) || out.last().is_some_and(|&i| i >= count) {
        return None;
    }
    Some(out)
}

/// Writes as much of `values` as fits into a caller buffer of `cap` entries
/// and reports the TRUE length through `out_len`, so a caller can tell a
/// truncated answer from a complete one. A NULL buffer means "do not report
/// it" and is not an error.
///
/// # Safety
/// `buffer` must be NULL or valid for `cap` `size_t` writes; `out_len` must be
/// NULL or a valid pointer to a writable `size_t`.
pub(crate) unsafe fn write_indices(
    values: &[usize],
    buffer: *mut usize,
    cap: usize,
    out_len: *mut usize,
) {
    if !out_len.is_null() {
        unsafe { *out_len = values.len() };
    }
    if buffer.is_null() {
        return;
    }
    for (i, &value) in values.iter().take(cap).enumerate() {
        unsafe { *buffer.add(i) = value };
    }
}

/// Aspect-fit thumbnail dimensions with the longest side `max(1, max_side)`,
/// each at least 1 — the ONE sizing rule, shared by `dr_doc_layer_thumbnail`
/// and the plane-image getters. `w` and `h` must be non-zero (every caller
/// checks for an empty source first, since there is nothing to scale).
pub(crate) fn thumb_dims(w: u32, h: u32, max_side: u32) -> (u32, u32) {
    let side = max_side.max(1);
    if w >= h {
        let th = (f64::from(h) * f64::from(side) / f64::from(w)).round() as u32;
        (side, th.max(1))
    } else {
        let tw = (f64::from(w) * f64::from(side) / f64::from(h)).round() as u32;
        (tw.max(1), side)
    }
}

/// Maps a raw `DrResizeFilter` value — the ONE mapping shared by
/// `dr_image_resize`, `dr_doc_resize`, and the layer transform.
pub(crate) fn filter_from_c(value: c_int) -> Option<FilterType> {
    match value {
        0 => Some(FilterType::Nearest),
        1 => Some(FilterType::Triangle),
        2 => Some(FilterType::CatmullRom),
        3 => Some(FilterType::Lanczos3),
        _ => None,
    }
}
