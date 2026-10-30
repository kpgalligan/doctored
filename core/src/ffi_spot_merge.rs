//! C FFI for spot channels 1c-1 (#1): Merge Spot Channel and the alpha ↔
//! spot kind change at both depths, and the `region` / `preview_divisor`
//! overprint of a document and of one bare image — the "Spot channels 1c-1"
//! section of `include/doctored_core.h`. Same conventions as
//! `ffi_spot_channel`: the `ffi_util` helpers own every `catch_unwind`, NULL
//! tolerance comes first, and every slice length is recomputed from the
//! core's own dimensions.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::ffi_util::{boxed, fallible_doc_op, fallible_op, mask_slice, required_handle};
use crate::icc::IccProfile;
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;
use crate::spot_channel::SpotInk;
use crate::spot_view::image_spot_overprint_region;
use crate::DrImage;

/// The ink of a kind change: `None` for an alpha channel (the colour is not
/// read), else the C arguments through `SpotInk::new`'s sentences.
fn kind(spot: bool, l: f64, a: f64, b: f64, solidity: u8) -> Result<Option<SpotInk>, String> {
    if spot {
        SpotInk::new([l, a, b], solidity).map(Some)
    } else {
        Ok(None)
    }
}

/// Merge Spot Channel on byte channel `i`: the flattened composite with that
/// ink overprinted as one "Background" layer, the channel removed. NULL with
/// no error for a NULL doc, an alpha channel or an index out of range; NULL
/// with `err_out` set when the flatten fails.
///
/// # Safety
/// `doc` must be NULL or a live `DrDocument`; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_merge_spot_channel(
    doc: *const DrDocument,
    i: usize,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_doc_op(doc, err_out, "merging a spot channel", |d| {
            d.merge_spot_channel(i)
        })
    }
}

/// Byte channel `i` made a spot channel (`spot` true, the ink D50 Lab `l`,
/// `a`, `b` and `solidity` percent) or an alpha channel (`spot` false; the
/// ink arguments are ignored), its plane inverted. NULL with no error for a
/// NULL doc, an index out of range or a channel already that kind; NULL with
/// `err_out` set for an ink out of range.
///
/// # Safety
/// `doc` must be NULL or a live `DrDocument`; `err_out` NULL or writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_doc_set_channel_kind(
    doc: *const DrDocument,
    i: usize,
    spot: bool,
    l: f64,
    a: f64,
    b: f64,
    solidity: u8,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_doc_op(doc, err_out, "changing a channel's kind", |d| {
            let ink = kind(spot, l, a, b, solidity)?;
            Ok(d.set_channel_kind(i, ink))
        })
    }
}

/// Merge Spot Channel's typed plan for channel `index`: owned JSON
/// `{"working_memory_plan_bytes","refusal"}` (the refusal names a Duotone
/// document, an index out of range or an alpha channel). Free it with
/// `dr_string_free`. NULL with `err_out` set for a NULL document.
///
/// # Safety
/// `doc` must be NULL or live; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_merge_spot_channel_plan(
    doc: *const PrecisionDocument,
    index: usize,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning Merge Spot Channel",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let plan = document.merge_spot_channel_plan(index)?;
                CString::new(plan.to_string())
                    .map_err(|_| "the Merge Spot Channel plan is not a C string".to_string())
            },
            CString::into_raw,
        )
    }
}

/// Merge Spot Channel on typed channel `index`, admitted against `limit`
/// beside `other` before anything is allocated. NULL with `err_out` set for
/// a NULL document, the plan's refusals, an admission refusal or a cancel.
///
/// # Safety
/// `doc` and `control` must be NULL or live; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_merge_spot_channel(
    doc: *const PrecisionDocument,
    index: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "merging a typed spot channel",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                document.merge_spot_channel(index, limit, other, control.as_ref())
            },
            boxed,
        )
    }
}

/// The typed kind change: channel `index` made a spot channel (`spot` true,
/// the ink as at byte depth) or an alpha channel (`spot` false, the ink
/// ignored), the plane inverted under the channel invert's plan. NULL with
/// no error when the channel is already that kind; NULL with `err_out` set
/// for a NULL document, an index out of range, an ink out of range, an
/// Indexed or Bitmap document asked for a spot channel, or an admission
/// refusal.
///
/// # Safety
/// `doc` must be NULL or live; `err_out` NULL or writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_set_channel_kind(
    doc: *const PrecisionDocument,
    index: usize,
    spot: bool,
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
            "changing a typed channel's kind",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let ink = kind(spot, l, a, b, solidity)?;
                document.set_channel_kind(index, ink, limit, other)
            },
            |result| result.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// `composite`, a region render of `doc` whose output (i, j) is canvas
/// (`x` + i·`divisor`, `y` + j·`divisor`), with every spot channel
/// overprinted at the same samples in list order. NULL with no error for a
/// NULL doc or a document without spot channels; NULL with `err_out` set
/// for a NULL composite, a zero divisor or a region past the canvas.
///
/// # Safety
/// `doc` must be NULL or a live `DrDocument`; `composite` NULL or a live
/// `DrImage`; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_spot_overprint_region(
    doc: *const DrDocument,
    composite: *const DrImage,
    x: u32,
    y: u32,
    divisor: u32,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    if doc.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "overprinting spot channels on a region",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let composite = required_handle(composite)?;
                document.spot_overprint_region(composite, (x, y), divisor)
            },
            |image| image.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// `composite`, a region render of a `canvas_width` × `canvas_height`
/// canvas, with one ink overprinted at the same samples: output (i, j)
/// reads `coverage` (the CANVAS's plane; `coverage_len` must equal canvas
/// width × height, checked, never trusted) at (`x` + i·`divisor`,
/// `y` + j·`divisor`). `icc`/`icc_len` as `dr_image_spot_overprint`. NULL
/// with `err_out` set for a NULL composite or coverage, a wrong coverage
/// length, a zero divisor, a region past the canvas or an ink out of range.
///
/// # Safety
/// `composite` must be NULL or a live `DrImage`; `coverage` NULL or readable
/// for `coverage_len` bytes; `icc` NULL or readable for `icc_len` bytes;
/// `err_out` NULL or writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_image_spot_overprint_region(
    composite: *const DrImage,
    coverage: *const u8,
    coverage_len: usize,
    canvas_width: u32,
    canvas_height: u32,
    x: u32,
    y: u32,
    divisor: u32,
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
            "overprinting a spot ink on a region",
            ptr::null_mut(),
            || {
                let image = required_handle(composite)?;
                let ink = SpotInk::new([l, a, b], solidity)?;
                let expected = (canvas_width as usize)
                    .checked_mul(canvas_height as usize)
                    .ok_or("the canvas is too large")?;
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
                image_spot_overprint_region(
                    image,
                    coverage,
                    (canvas_width, canvas_height),
                    (x, y),
                    divisor,
                    &ink,
                    profile.as_ref(),
                )
            },
            boxed,
        )
    }
}
