//! Direct genuine-u16 host decoder intake. No byte or temporary PNG conversion;
//! caller data/profile and an owned exact copy are admitted before allocating.
//!
//! Direct RGBA16 host intake validates dimensions, exact sample count and ICC before copying; since
//! 13B-3e-4a its image (`ffi_precision_input.rs:76`) is built `from_rgba16_unseeded`, so an intake
//! caches no view.
use crate::drdc::{MAX_DRDC_BLOB_LEN, MAX_DRDC_NAME_LEN};
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op};
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::metadata::{Metadata, Resolution};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, reserved_vec, PrecisionImage, BOOKKEEPING_BYTES};
use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;

/// Create one exact straight RGBA16 raster from host-decoded native-endian samples.
/// Sample count must be width*height*4, alpha is not premultiplied, and ICC must
/// be a complete supported RGB matrix/TRC profile. No implicit profile fallback.
/// The one layer is named `layer_name`, or "Background" when it is NULL; a name
/// over the DRDC name limit or not valid UTF-8 returns NULL and sets `err_out`.
/// # Safety
/// Samples/profile are NULL or readable for their stated lengths; layer_name is
/// NULL or a NUL-terminated string; control NULL/live; err_out NULL/writable.
/// Buffers stay alive for the entire synchronous call.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_from_rgba16(
    samples: *const u16,
    sample_count: usize,
    width: u32,
    height: u32,
    profile: *const u8,
    profile_length: usize,
    layer_name: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "capturing host high-depth pixels",
            ptr::null_mut(),
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                job.check()?;
                let name = if layer_name.is_null() {
                    "Background"
                } else {
                    borrow_cstr_bounded(layer_name, "layer name", MAX_DRDC_NAME_LEN as usize)?
                };
                let count = pixel_count(width, height)?;
                if samples.is_null()
                    || sample_count != count.checked_mul(4).ok_or("u16 sample count overflow")?
                {
                    return Err("host u16 samples must match width times height times four".into());
                }
                if profile.is_null()
                    || profile_length == 0
                    || profile_length > MAX_DRDC_BLOB_LEN as usize
                {
                    return Err("host u16 input requires a bounded original ICC profile".into());
                }
                let input_bytes = count as u64 * 8 + profile_length as u64;
                // ICC parsers can materialize sampled curves; 64 times encoded bytes
                // bounds their parsed state and transient clones before the parser runs.
                let scratch = 4 * BOOKKEEPING_BYTES + count as u64 * 8 + profile_length as u64 * 64;
                admit(input_bytes, scratch, other, limit)?;
                job.reserve(scratch)?;
                let profile = Arc::new(
                    IccProfile::parse(std::slice::from_raw_parts(profile, profile_length))
                        .ok_or("host u16 profile is not a supported RGB ICC profile")?,
                );
                if profile.bytes().len() != profile_length || profile.model().is_none() {
                    return Err(
                        "host u16 profile must be exact complete RGB matrix/TRC ICC bytes".into(),
                    );
                }
                let input = std::slice::from_raw_parts(samples, count * 4);
                let mut owned = reserved_vec(count)?;
                for (index, pixel) in input.chunks_exact(4).enumerate() {
                    job.every(index)?;
                    owned.push([pixel[0], pixel[1], pixel[2], pixel[3]]);
                }
                let image =
                    PrecisionImage::from_rgba16_unseeded(width, height, owned, profile, None)?;
                let document = PrecisionDocument::from_image_named(
                    image,
                    Metadata::default(),
                    Resolution::default(),
                    name,
                )?;
                job.report(1., 5)?;
                Ok(document)
            },
            boxed,
        )
    }
}

/// Create a 32-bit float document from host-decoded native-endian f32 samples:
/// one raster layer of straight RGBA, linear light in the profile's primaries,
/// under the Clip tone map (`dr_precision_doc_from_rgba16`'s float twin; the
/// 32-bit float Camera RAW development is its caller). Sample count must be
/// width*height*4 and ICC must be a complete supported RGB matrix/TRC profile.
/// Nothing is repaired: a non-finite sample, an RGB magnitude above 65504 or
/// an alpha outside 0…1 returns NULL and sets `err_out`. The one layer is
/// named `layer_name`, or "Background" when it is NULL.
/// # Safety
/// Samples/profile are NULL or readable for their stated lengths; layer_name is
/// NULL or a NUL-terminated string; control NULL/live; err_out NULL/writable.
/// Buffers stay alive for the entire synchronous call.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_from_rgba32(
    samples: *const f32,
    sample_count: usize,
    width: u32,
    height: u32,
    profile: *const u8,
    profile_length: usize,
    layer_name: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "capturing host float pixels",
            ptr::null_mut(),
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                job.check()?;
                let name = if layer_name.is_null() {
                    "Background"
                } else {
                    borrow_cstr_bounded(layer_name, "layer name", MAX_DRDC_NAME_LEN as usize)?
                };
                let count = pixel_count(width, height)?;
                if samples.is_null()
                    || sample_count != count.checked_mul(4).ok_or("f32 sample count overflow")?
                {
                    return Err("host f32 samples must match width times height times four".into());
                }
                if profile.is_null()
                    || profile_length == 0
                    || profile_length > MAX_DRDC_BLOB_LEN as usize
                {
                    return Err("host f32 input requires a bounded original ICC profile".into());
                }
                let input_bytes = count as u64 * 16 + profile_length as u64;
                // The u16 intake's terms at 16 bytes a pixel: the owned copy,
                // and 64 times the encoded profile for its parsed state.
                let scratch =
                    4 * BOOKKEEPING_BYTES + count as u64 * 16 + profile_length as u64 * 64;
                admit(input_bytes, scratch, other, limit)?;
                job.reserve(scratch)?;
                let profile = Arc::new(
                    IccProfile::parse(std::slice::from_raw_parts(profile, profile_length))
                        .ok_or("host f32 profile is not a supported RGB ICC profile")?,
                );
                if profile.bytes().len() != profile_length || profile.model().is_none() {
                    return Err(
                        "host f32 profile must be exact complete RGB matrix/TRC ICC bytes".into(),
                    );
                }
                let input = std::slice::from_raw_parts(samples, count * 4);
                let mut owned = reserved_vec(count)?;
                for (index, pixel) in input.chunks_exact(4).enumerate() {
                    job.every(index)?;
                    owned.push([pixel[0], pixel[1], pixel[2], pixel[3]]);
                }
                let image =
                    PrecisionImage::from_rgba32_extended(width, height, owned, profile, None)?;
                let document = crate::raw_float_develop::float_document(image, name)?;
                job.report(1., 5)?;
                Ok(document)
            },
            boxed,
        )
    }
}

/// Create a blank 16-bit document (File ▸ New… at 16 bits): one raster
/// layer of `width`×`height` filled with the four straight RGBA16 samples at
/// `fill`, tagged with complete RGB matrix/TRC ICC bytes. The layer is named
/// `layer_name`, or "Background" when it is NULL. A canvas too large to reopen
/// at 16 bits returns NULL with one user-terms sentence naming the size and
/// the way forward (a smaller canvas or 8 bits); NULL/empty/oversized or
/// unsupported profile bytes and a NULL `fill` are refused by name.
/// # Safety
/// `fill` is NULL or readable for four samples; `profile` is NULL or readable
/// for `profile_length` bytes; layer_name is NULL or a NUL-terminated string;
/// err_out NULL/writable. Buffers stay alive for the entire synchronous call.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_create_blank(
    width: u32,
    height: u32,
    fill: *const u16,
    profile: *const u8,
    profile_length: usize,
    layer_name: *const c_char,
    limit: u64,
    other: u64,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "creating a blank high-depth document",
            ptr::null_mut(),
            || {
                if fill.is_null() {
                    return Err("a blank document needs four fill samples".into());
                }
                if profile.is_null()
                    || profile_length == 0
                    || profile_length > MAX_DRDC_BLOB_LEN as usize
                {
                    return Err(crate::precision_blank::NO_PROFILE.into());
                }
                let name = if layer_name.is_null() {
                    "Background"
                } else {
                    borrow_cstr_bounded(layer_name, "layer name", MAX_DRDC_NAME_LEN as usize)?
                };
                let fill = std::slice::from_raw_parts(fill, 4);
                PrecisionDocument::create_blank(
                    width,
                    height,
                    [fill[0], fill[1], fill[2], fill[3]],
                    std::slice::from_raw_parts(profile, profile_length),
                    name,
                    limit,
                    other,
                )
            },
            boxed,
        )
    }
}
