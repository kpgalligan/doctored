//! C FFI for Edit ▸ Perspective Warp: the workspace's layout and
//! preview calls (`dr_perspective_warp_layout`, `dr_perspective_warp_preview`)
//! and the committed layer edit at both depths
//! (`dr_doc_perspective_warp(_plan)`, `dr_precision_doc_perspective_warp(_plan)`).
//! The float form commits through the selected-pixels edits
//! (`"op":"perspective_selected"`).
//!
//! Thin shims over the shared `ffi_util` helpers; nothing unwinds across the
//! boundary.
use std::ffi::{c_char, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::doc_perspective_warp::PerspectiveWarpRequest;
use crate::doc_puppet::REQUEST_LIMIT;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_doc_op, fallible_op, required_handle};
use crate::job_control::{DrJobControl, JobContext};
use crate::perspective_plane::{PerspectiveSpec, FIELDS_LIMIT};
use crate::perspective_warp::{map, prepare, Prepared};
use crate::precision_document::PrecisionDocument;
use crate::puppet_render::render_preview;
use crate::DrImage;

fn json_string(value: serde_json::Value) -> Result<*mut c_char, String> {
    CString::new(value.to_string())
        .map(CString::into_raw)
        .map_err(|e| e.to_string())
}

/// The resolved layout of `fields` (`{"quads","straighten"?,"anchor"?}`):
/// `{"vertices":[{"from","to"}],"quads","shared_edges","straighten",
/// "identity"}`. NULL `fields` returns NULL with no error; a refusal returns
/// NULL with its sentence.
///
/// # Safety
/// `fields` is NULL or NUL terminated; `err_out` is NULL or writable. Free
/// the string with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_perspective_warp_layout(
    fields: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    if fields.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "resolving a perspective warp layout",
            ptr::null_mut(),
            || {
                let text = borrow_cstr_bounded(fields, "fields", FIELDS_LIMIT)?;
                json_string(crate::perspective_warp::describe_fields(text)?)
            },
            |s| s,
        )
    }
}

/// The workspace preview: `proxy` (a picture of the layer's canvas rect `sx,
/// sy, sw, sh` at any scale, the extension's subject), optionally times
/// `coverage` (proxy-sized bytes), through the warp of `fields`. The canvas
/// rect the answer covers is written to `out_rect` (4 doubles: x, y, w, h).
/// An identity layout answers the proxy itself (times the coverage) over
/// the source rect. NULL `fields` or `proxy` returns NULL with no error; a
/// refusal, an empty source, a coverage of another length or a NULL
/// `out_rect` returns NULL with a message.
///
/// # Safety
/// `fields` is NULL or NUL terminated; `proxy` is NULL or live; `coverage` is
/// NULL or valid for `coverage_len` bytes; `out_rect` is NULL or valid for 4
/// doubles; `err_out` is NULL or writable. Free the image with dr_image_free.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_perspective_warp_preview(
    fields: *const c_char,
    proxy: *const DrImage,
    sx: i64,
    sy: i64,
    sw: u32,
    sh: u32,
    coverage: *const u8,
    coverage_len: usize,
    out_rect: *mut f64,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    if fields.is_null() || proxy.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "rendering a perspective warp preview",
            ptr::null_mut(),
            || {
                if out_rect.is_null() {
                    return Err("perspective warp preview needs an out_rect".into());
                }
                let spec = PerspectiveSpec::from_fields(borrow_cstr_bounded(
                    fields,
                    "fields",
                    FIELDS_LIMIT,
                )?)?;
                let proxy = &required_handle(proxy)?.pixels;
                let (pw, ph) = proxy.dimensions();
                if pw == 0 || ph == 0 || sw == 0 || sh == 0 {
                    return Err(
                        "the perspective warp preview needs a non-empty proxy and source".into(),
                    );
                }
                let len = pw as usize * ph as usize;
                let coverage = if coverage.is_null() {
                    None
                } else if coverage_len != len {
                    return Err("perspective warp preview coverage must be proxy-sized".into());
                } else {
                    Some(std::slice::from_raw_parts(coverage, len))
                };
                let source = (sx, sy, sw, sh);
                let (image, rect) = match prepare(&spec, source, u64::MAX, 0, JobContext::NONE)? {
                    Prepared::Identity => {
                        let mut image = proxy.clone();
                        if let Some(coverage) = coverage {
                            for (pixel, &c) in image.pixels_mut().zip(coverage) {
                                let a = u32::from(pixel.0[3]) * u32::from(c);
                                pixel.0[3] = ((a + 127) / 255) as u8;
                            }
                        }
                        let rect = [sx as f64, sy as f64, f64::from(sw), f64::from(sh)];
                        (image, rect)
                    }
                    Prepared::Ready {
                        pose,
                        layout,
                        plane,
                        ..
                    } => {
                        let map = map(&layout, &plane, &pose)?;
                        render_preview(&map, proxy, source, coverage)?
                    }
                };
                std::ptr::copy_nonoverlapping(rect.as_ptr(), out_rect, 4);
                Ok(DrImage { pixels: image })
            },
            boxed,
        )
    }
}

macro_rules! warp_ffi {
    ($doc:ty, $plan:ident, $edit:ident, $op:path) => {
        /// The plan of a `perspective_warp` request:
        /// `{"retained_bytes","scratch_bytes","rect":[x,y,w,h],"vertices"}`;
        /// an identity layout answers `scratch_bytes` 0. NULL document
        /// returns NULL with no error; a refusal returns NULL with its
        /// sentence.
        ///
        /// # Safety
        /// `document` is NULL or live; `request` is NULL or NUL terminated;
        /// `err_out` is NULL or writable. Free the string with
        /// dr_string_free.
        #[no_mangle]
        pub unsafe extern "C" fn $plan(
            document: *const $doc,
            request: *const c_char,
            limit: u64,
            other: u64,
            err_out: *mut *mut c_char,
        ) -> *mut c_char {
            if document.is_null() {
                return ptr::null_mut();
            }
            unsafe {
                fallible_op(
                    err_out,
                    "planning a perspective warp",
                    ptr::null_mut(),
                    || {
                        let request = PerspectiveWarpRequest::parse(borrow_cstr_bounded(
                            request,
                            "request",
                            REQUEST_LIMIT,
                        )?)?;
                        json_string(
                            required_handle(document)?
                                .perspective_warp_plan(&request, limit, other)?,
                        )
                    },
                    |s| s,
                )
            }
        }

        /// The edit of a `perspective_warp` request: a new document, or
        /// NULL with no error for an identity layout. NULL document returns
        /// NULL with no error; a refusal returns NULL with its sentence.
        ///
        /// # Safety
        /// Handles are NULL or live for the call; `request` is NULL or NUL
        /// terminated; `err_out` is NULL or writable.
        #[no_mangle]
        pub unsafe extern "C" fn $edit(
            document: *const $doc,
            request: *const c_char,
            limit: u64,
            other: u64,
            control: *const DrJobControl,
            err_out: *mut *mut c_char,
        ) -> *mut $doc {
            if document.is_null() {
                return ptr::null_mut();
            }
            unsafe {
                $op(
                    document,
                    err_out,
                    "editing a perspective warp",
                    |doc: &$doc| {
                        let request = PerspectiveWarpRequest::parse(borrow_cstr_bounded(
                            request,
                            "request",
                            REQUEST_LIMIT,
                        )?)?;
                        doc.perspective_warp(&request, limit, other, control.as_ref())
                    },
                )
            }
        }
    };
}
warp_ffi!(
    DrDocument,
    dr_doc_perspective_warp_plan,
    dr_doc_perspective_warp,
    fallible_doc_op
);
warp_ffi!(
    PrecisionDocument,
    dr_precision_doc_perspective_warp_plan,
    dr_precision_doc_perspective_warp,
    precision_doc_op
);

/// [`fallible_doc_op`]'s typed twin for this file: NULL document yields
/// NULL, `Ok(None)` NULL with no message, `Err` its message. A copy of
/// `ffi_puppet`'s private `precision_doc_op` (the master), kept private as
/// that one is.
///
/// # Safety
/// `doc` is NULL or live; `err_out` is NULL or writable.
unsafe fn precision_doc_op(
    doc: *const PrecisionDocument,
    err_out: *mut *mut c_char,
    what: &str,
    op: impl FnOnce(&PrecisionDocument) -> Result<Option<PrecisionDocument>, String>,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            what,
            ptr::null_mut(),
            || op(required_handle(doc)?),
            |result| result.map_or(ptr::null_mut(), boxed),
        )
    }
}
