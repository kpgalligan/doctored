//! C FFI for spot channels (#547 phase 1a-2): the byte add, set and read,
//! the display overprint of a document and of one bare image, and the typed
//! add and set — the "Spot channels" section of `include/doctored_core.h`.
//! Same conventions as `ffi_channel`: the `ffi_util` helpers own every
//! `catch_unwind`, NULL tolerance comes first, and every slice length is
//! recomputed from the core's own dimensions.

use std::ffi::c_char;
use std::ptr;

use crate::doc::DrDocument;
use crate::ffi_util::{
    boxed, doc_get, fallible_doc_op, fallible_op, mask_slice, read_cstr, required_handle,
};
use crate::icc::IccProfile;
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;
use crate::spot_channel::SpotInk;
use crate::spot_view::image_spot_overprint;
use crate::DrImage;

/// The ink from the C arguments, refused by `SpotInk::new`'s sentences.
fn ink(l: f64, a: f64, b: f64, solidity: u8) -> Result<SpotInk, String> {
    SpotInk::new([l, a, b], solidity)
}

/// Appends a spot channel named `name` with the ink D50 Lab (`l`, `a`, `b`)
/// and `solidity` percent; `coverage` is canvas width × height bytes, row 0
/// top (255 = full ink), the length recomputed from the document. NULL on a
/// NULL doc (no error); NULL with `err_out` set for a NULL name or coverage,
/// an ink out of range, a zero canvas or a list past either channel cap.
///
/// # Safety
/// `doc` must be NULL or a live `DrDocument`; `name` NULL or a NUL-terminated
/// string; `coverage` NULL or readable for width × height bytes; `err_out`
/// NULL or writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_doc_add_spot_channel(
    doc: *const DrDocument,
    name: *const c_char,
    coverage: *const u8,
    l: f64,
    a: f64,
    b: f64,
    solidity: u8,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_doc_op(doc, err_out, "adding a spot channel", |d| {
            let name = read_cstr(name, "name")?;
            let ink = ink(l, a, b, solidity)?;
            if d.width == 0 || d.height == 0 {
                return Err("a spot channel needs a canvas: this document is zero-sized".into());
            }
            let len = (d.width as usize)
                .checked_mul(d.height as usize)
                .ok_or("the canvas is too large")?;
            let coverage = mask_slice(coverage, len).ok_or("spot channel coverage is required")?;
            d.add_spot_channel(&name, ink, coverage)
                .map(Some)
                .ok_or_else(|| "the spot channel does not fit the document's channel budget".into())
        })
    }
}

/// Sets spot channel `i`'s name (NULL keeps it) and ink in one step. NULL
/// with no error for a NULL doc, an alpha channel, an index out of range or
/// an edit that changes nothing; NULL with `err_out` set for an ink out of
/// range or a name that is not UTF-8.
///
/// # Safety
/// `doc` must be NULL or a live `DrDocument`; `name` NULL or a NUL-terminated
/// string; `err_out` NULL or writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_doc_set_spot_channel(
    doc: *const DrDocument,
    i: usize,
    name: *const c_char,
    l: f64,
    a: f64,
    b: f64,
    solidity: u8,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_doc_op(doc, err_out, "setting a spot channel", |d| {
            let name = if name.is_null() {
                None
            } else {
                Some(read_cstr(name, "name")?)
            };
            let ink = ink(l, a, b, solidity)?;
            Ok(d.set_spot_channel(i, name.as_deref(), ink))
        })
    }
}

/// Channel `i`'s ink: writes its D50 Lab to `lab_out[0..3]` and its solidity
/// to `solidity_out` (either may be NULL) and answers true. False, writing
/// nothing, for a NULL doc, an alpha channel or an index out of range.
///
/// # Safety
/// `doc` must be NULL or a live `DrDocument`; `lab_out` NULL or writable for
/// three doubles; `solidity_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_channel_spot(
    doc: *const DrDocument,
    i: usize,
    lab_out: *mut f64,
    solidity_out: *mut u8,
) -> bool {
    let Some(spot) = (unsafe { doc_get(doc, None, |d| Some(d.channel_spot(i))) }) else {
        return false;
    };
    if !lab_out.is_null() {
        let lab = spot.lab();
        unsafe { std::slice::from_raw_parts_mut(lab_out, 3) }.copy_from_slice(&lab);
    }
    if !solidity_out.is_null() {
        unsafe { *solidity_out = spot.solidity() };
    }
    true
}

/// `composite` (a canvas-sized RGBA8 rendering of `doc`) with every spot
/// channel overprinted in list order, each ink in the document's profile
/// (sRGB when the profile cannot be a destination). NULL with no error for a
/// NULL doc or a document without spot channels; NULL with `err_out` set for
/// a NULL composite or one that is not canvas-sized.
///
/// # Safety
/// `doc` must be NULL or a live `DrDocument`; `composite` NULL or a live
/// `DrImage`; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_spot_overprint(
    doc: *const DrDocument,
    composite: *const DrImage,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    if doc.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "overprinting spot channels",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let composite = required_handle(composite)?;
                document.spot_overprint(composite)
            },
            |image| image.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// `composite` with one ink overprinted at `coverage`, which must hold
/// exactly the image's width × height bytes (`coverage_len` is checked
/// against the image, never trusted). `icc`/`icc_len` is the composite's
/// profile; NULL, empty, unreadable or not a possible destination reads as
/// sRGB. NULL with `err_out` set for a NULL composite or coverage, a wrong
/// coverage length or an ink out of range.
///
/// # Safety
/// `composite` must be NULL or a live `DrImage`; `coverage` NULL or readable
/// for `coverage_len` bytes; `icc` NULL or readable for `icc_len` bytes;
/// `err_out` NULL or writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_image_spot_overprint(
    composite: *const DrImage,
    coverage: *const u8,
    coverage_len: usize,
    l: f64,
    a: f64,
    b: f64,
    solidity: u8,
    icc: *const u8,
    icc_len: usize,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "overprinting a spot ink",
            ptr::null_mut(),
            || {
                let image = required_handle(composite)?;
                let ink = ink(l, a, b, solidity)?;
                let (width, height) = image.pixels.dimensions();
                let expected = (width as usize)
                    .checked_mul(height as usize)
                    .ok_or("the image is too large")?;
                if coverage_len != expected {
                    return Err("the spot coverage does not match the image".into());
                }
                let coverage =
                    mask_slice(coverage, expected).ok_or("spot channel coverage is required")?;
                let profile = if icc.is_null() || icc_len == 0 {
                    None
                } else {
                    IccProfile::parse(std::slice::from_raw_parts(icc, icc_len))
                };
                image_spot_overprint(image, coverage, &ink, profile.as_ref())
            },
            boxed,
        )
    }
}

/// The typed add: a spot channel appended through the alpha channel's
/// append (`PrecisionDocument::add_spot_channel`), `coverage` canvas-sized
/// native-endian u16 (65535 = full ink) whose `length` must equal the
/// document's width × height. NULL with `err_out` set for a NULL document,
/// name or coverage, a wrong length, an ink out of range, an Indexed or
/// Bitmap document (by sentence), a cap, an admission refusal or a cancel.
///
/// # Safety
/// `doc` and `control` must be NULL or live; `name` NULL or a NUL-terminated
/// string; `coverage` NULL or readable for `length` u16s; `err_out` NULL or
/// writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_add_spot_channel(
    doc: *const PrecisionDocument,
    name: *const c_char,
    l: f64,
    a: f64,
    b: f64,
    solidity: u8,
    coverage: *const u16,
    length: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "adding a typed spot channel",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let name = read_cstr(name, "name")?;
                let ink = ink(l, a, b, solidity)?;
                let (width, height) = document.dimensions();
                let expected = (width as usize)
                    .checked_mul(height as usize)
                    .ok_or("the canvas is too large")?;
                if length != expected {
                    return Err("the spot coverage does not match the canvas".into());
                }
                if coverage.is_null() {
                    return Err("spot channel coverage is required".into());
                }
                let coverage = std::slice::from_raw_parts(coverage, expected);
                document.add_spot_channel(&name, ink, coverage, limit, other, control.as_ref())
            },
            boxed,
        )
    }
}

/// The typed set: spot channel `index`'s name (NULL keeps it) and ink
/// (`PrecisionDocument::set_spot_channel`). NULL with no error when nothing
/// changes; NULL with `err_out` set for a NULL document, an index out of
/// range, an alpha channel ("the channel is not a spot channel"), a name
/// past the native limit or not UTF-8, an ink out of range or an admission
/// refusal.
///
/// # Safety
/// `doc` must be NULL or live; `name` NULL or a NUL-terminated string;
/// `err_out` NULL or writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_set_spot_channel(
    doc: *const PrecisionDocument,
    index: usize,
    name: *const c_char,
    l: f64,
    a: f64,
    b: f64,
    solidity: u8,
    limit: u64,
    other: u64,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "setting a typed spot channel",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let name = if name.is_null() {
                    None
                } else {
                    Some(read_cstr(name, "name")?)
                };
                let ink = ink(l, a, b, solidity)?;
                document.set_spot_channel(index, name.as_deref(), ink, limit, other)
            },
            |result| result.map_or(ptr::null_mut(), boxed),
        )
    }
}
