//! Type, shape and fill layers on a CMYK document across the FFI (#1,
//! `cmyk_artwork`): the ink publication and the colour leg a description's
//! numbers cross the CMYK boundary through. The conventions are the
//! surface's: NULL tolerance first, `catch_unwind` through `fallible_op`,
//! `Err` through `err_out`, slice lengths recomputed from the raster's own
//! dimensions and the component list before any `from_raw_parts`.

use std::ffi::{c_char, CString};
use std::ptr;

use crate::artwork_document::{ArtworkPublication, MAX_LARGE_DESCRIPTION};
use crate::cmyk_artwork::{artwork_colors, component_planes, CmykArtwork};
use crate::cmyk_artwork_render::cmyk_components;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, required_handle};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::pixel_count;

/// The largest colour request `dr_cmyk_artwork_colors` reads.
const COLORS_REQUEST_MAX: usize = 256 * 1024;

/// Assemble ink components (coverage planes component-major u16; intrinsic
/// RGBA8 sRGB converted through the document's CMYK profile, relative + BPC)
/// into a CMYKA16 raster and publish it with `description`: replacing
/// rewrites `layer` (under its Pixels lock, mask kept); else a NEW leaf at
/// `position` (0 above, 1 top, 2 into_group) relative to anchor `layer`.
/// NULL, no error: no change. Free the result with `dr_precision_doc_free`.
///
/// # Safety
/// `document` and `control` NULL or live; `components`, `name` and
/// `description` NULL or NUL-terminated; `samples` NULL or readable for
/// `sample_count` u16 and `intrinsic` for `intrinsic_count` bytes, each
/// count exactly what the component list names over `width × height`;
/// `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_cmyk_artwork(
    document: *const PrecisionDocument,
    layer: usize,
    replacing: bool,
    position: u32,
    width: u32,
    height: u32,
    components: *const c_char,
    samples: *const u16,
    sample_count: usize,
    intrinsic: *const u8,
    intrinsic_count: usize,
    x: i32,
    y: i32,
    name: *const c_char,
    description: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "publishing CMYK artwork",
            ptr::null_mut(),
            || {
                let document = required_handle(document)?;
                let text =
                    borrow_cstr_bounded(components, "artwork components", MAX_LARGE_DESCRIPTION)?;
                let name = borrow_cstr_bounded(
                    name,
                    "artwork name",
                    crate::drdc::MAX_DRDC_NAME_LEN as usize,
                )?;
                let description =
                    borrow_cstr_bounded(description, "artwork description", MAX_LARGE_DESCRIPTION)?;
                let position = if replacing {
                    None
                } else {
                    Some(crate::layer_insert::InsertPosition::from_code(position)?)
                };
                // The lengths come from the raster's own size and the planes
                // its component list names, never from the caller.
                let n = pixel_count(width, height)?;
                let (planes, assets) = component_planes(&cmyk_components(text)?);
                let expected_samples = n.checked_mul(planes);
                let expected_intrinsic = n.checked_mul(assets * 4);
                if expected_samples != Some(sample_count)
                    || expected_intrinsic != Some(intrinsic_count)
                    || (samples.is_null() && sample_count != 0)
                    || (intrinsic.is_null() && intrinsic_count != 0)
                    || sample_count.saturating_add(intrinsic_count)
                        > crate::artwork_render::MAX_COMPONENT_SAMPLES
                {
                    return Err("CMYK artwork samples do not match their component layout".into());
                }
                let samples = if planes == 0 {
                    &[][..]
                } else {
                    std::slice::from_raw_parts(samples, n * planes)
                };
                let intrinsic = if assets == 0 {
                    &[][..]
                } else {
                    std::slice::from_raw_parts(intrinsic, n * assets * 4)
                };
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                document.cmyk_artwork(
                    &ArtworkPublication {
                        index: layer,
                        replacing,
                        offset: (x, y),
                        name,
                        description,
                        position,
                    },
                    &CmykArtwork {
                        width,
                        height,
                        components: text,
                        samples,
                        intrinsic,
                    },
                    limit,
                    other,
                    job,
                )
            },
            |result| result.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// A description's colours across the CMYK boundary. `request` is
/// `{"values":[[…],…]` (at most 1024), `"intent"`, `"black_point_compensation"`}
/// (`dr_cmyk_color`'s keys and defaults); a NULL or 0-length profile end is
/// sRGB `[r, g, b]`, a CMYK profile end `[c, m, y, k]`, every value 0…1.
/// Answers `{"values":[[…],…]}`: sRGB → ink through `srgb_to_ink`'s leg, ink
/// → sRGB `ink_to_srgb`'s, ink → ink `CmykToCmyk` (identical profile bytes:
/// the values verbatim). Both ends sRGB, or a profile that is not CMYK,
/// refuses by name. Free the answer with `dr_string_free`.
///
/// # Safety
/// Each profile NULL or readable for its length; `request` NULL or
/// NUL-terminated; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_cmyk_artwork_colors(
    source_icc: *const u8,
    source_len: usize,
    destination_icc: *const u8,
    destination_len: usize,
    request: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "converting artwork colours",
            ptr::null_mut(),
            || {
                let request = borrow_cstr_bounded(request, "request", COLORS_REQUEST_MAX)?;
                let cap = crate::drdc::MAX_DRDC_BLOB_LEN as usize;
                if source_len > cap || destination_len > cap {
                    return Err("the CMYK profile exceeds the native byte limit".into());
                }
                let source = crate::ffi_color::blob(source_icc, source_len);
                let destination = crate::ffi_color::blob(destination_icc, destination_len);
                let answer = artwork_colors(source, destination, request)?;
                CString::new(answer).map_err(|e| format!("cannot report artwork colours: {e}"))
            },
            CString::into_raw,
        )
    }
}
