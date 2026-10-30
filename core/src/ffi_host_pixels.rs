//! Bounded layout/profile admission before host pixel buffers are copied.
//! `dr_single_image_capacity` is the one-image cap a host asks before decoding.
//! `dr_precision_doc_from_host_hdr_pixels` is the HDR still's float intake
//! (`float_host_intake` owns its rules).
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op};
use crate::float_host_intake::{linear_profile, primaries_named, HdrEncoding};
use crate::hdr_transfer::HdrTransfer;
use crate::host_pixels::{HostLayout, HostPixelPlan};
use crate::job_control::{DrJobControl, JobContext};
use std::ffi::{c_char, CStr};
use std::ptr;

struct Input {
    bytes: *const u8,
    length: usize,
    width: u32,
    height: u32,
    row_bytes: usize,
    layout: *const c_char,
    orientation: u32,
    profile: *const u8,
    profile_length: usize,
}
unsafe fn intake<T>(
    input: Input,
    depth: u8,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    produce: impl FnOnce(
        &HostPixelPlan,
        &[u8],
        std::sync::Arc<crate::icc::IccProfile>,
        JobContext<'_>,
    ) -> Result<T, String>,
) -> Result<T, String> {
    let control = unsafe { control.as_ref() };
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    job.check()?;
    if input.bytes.is_null() || input.profile.is_null() || input.layout.is_null() {
        return Err("host samples, layout and original ICC are required".into());
    }
    let name = unsafe { CStr::from_ptr(input.layout) }
        .to_str()
        .map_err(|_| "host layout is not UTF-8")?;
    if name.len() > 64 {
        return Err("host layout name exceeds its limit".into());
    }
    let layout = HostLayout::parse(name)?;
    if layout.depth != depth {
        return Err("host layout does not match the requested document depth".into());
    }
    let plan = HostPixelPlan::new(
        (input.width, input.height),
        input.row_bytes,
        input.length,
        layout,
        input.orientation,
        input.profile_length,
    )?;
    let profile = plan.profile(
        unsafe { std::slice::from_raw_parts(input.profile, input.profile_length) },
        limit,
        other,
        job,
    )?;
    let bytes = unsafe { std::slice::from_raw_parts(input.bytes, input.length) };
    let result = produce(&plan, bytes, profile, job)?;
    job.report(1., 5)?;
    Ok(result)
}

macro_rules! export {
    ($name:ident, $document:ty, $depth:expr, $method:ident) => {
        /// Copy native integer platform samples into one original-profile raster.
        /// Layout fixes packing/endian/alpha; orientation is exact EXIF1–8.
        /// Padding/length/ICC/admission validate before allocating any output.
        /// # Safety
        /// Data/profile are NULL or readable for stated lengths; layout is NULL
        /// or a live NUL-terminated string. Token and err_out are NULL or valid.
        #[no_mangle]
        pub unsafe extern "C" fn $name(
            bytes: *const u8,
            length: usize,
            width: u32,
            height: u32,
            row_bytes: usize,
            layout: *const c_char,
            orientation: u32,
            profile: *const u8,
            profile_length: usize,
            limit: u64,
            other: u64,
            control: *const DrJobControl,
            err_out: *mut *mut c_char,
        ) -> *mut $document {
            unsafe {
                fallible_op(
                    err_out,
                    "capturing native host pixels",
                    ptr::null_mut(),
                    || {
                        intake(
                            Input {
                                bytes,
                                length,
                                width,
                                height,
                                row_bytes,
                                layout,
                                orientation,
                                profile,
                                profile_length,
                            },
                            $depth,
                            limit,
                            other,
                            control,
                            |plan, bytes, profile, job| plan.$method(bytes, profile, job),
                        )
                    },
                    boxed,
                )
            }
        }
    };
}
export!(dr_doc_from_host_pixels, crate::doc::DrDocument, 8, byte);
export!(
    dr_precision_doc_from_host_pixels,
    crate::precision_document::PrecisionDocument,
    16,
    typed
);

/// true when one image of `width` × `height` at `depth` (8 or 16) fits the
/// one-image cap a document keeps (the 100,000,000-pixel limit, the 8-bit
/// decoder allocation and the 1 GiB native PNG record bound; every extent
/// within the pixel limit fits at either depth); false sets
/// `err_out` to the core's sentence (the pixel-limit or too-large sentence,
/// or the unsupported-depth error for any other depth). Never panics.
///
/// # Safety
/// `err_out` must be NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_single_image_capacity(
    width: u32,
    height: u32,
    depth: u32,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "checking single image capacity",
            false,
            || {
                // A depth past u8 refuses as depth 10 does; never truncated.
                let depth = u8::try_from(depth).unwrap_or(0);
                crate::native_sources::single_image_capacity((width, height), depth)
            },
            |()| true,
        )
    }
}

/// A host-decoded HDR still as a 32-bit float document, one
/// layer at the `clip` map. `transfer` `"linear"` takes an `rgba32f_le`,
/// `rgba32f_le_premultiplied` or `rgba32f_le_opaque` layout of linear light
/// (1.0 = the SDR base's white) and the exact matrix/TRC ICC it is in, which
/// becomes the document profile; `primaries` must be NULL. `"pq"` and
/// `"hlg"` take an integer 16-bit or `rgb10_packed_le` layout of BT.2100
/// signal codes and `primaries` `"bt709"`, `"display_p3"` or `"bt2020"`;
/// `profile` must be NULL with length 0. 203 cd/m² opens as 1.0; BT.2020 is
/// converted in linear light to sRGB primaries (disclosed as an import
/// warning). Orientation is EXIF 1–8. Padding, length, layout, profile and
/// admission (`limit` beside `other_retained`) are checked before any output
/// is allocated; a non-finite f32 sample, or one past ±65504, refuses by name.
///
/// # Safety
/// `bytes` must be NULL or readable for `length` bytes; `profile` NULL or
/// readable for `profile_length` bytes; `layout`, `transfer` and `primaries`
/// NULL or live NUL-terminated strings; `control` NULL or a live single-use
/// token; `err_out` NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_from_host_hdr_pixels(
    bytes: *const u8,
    length: usize,
    width: u32,
    height: u32,
    row_bytes: usize,
    layout: *const c_char,
    orientation: u32,
    transfer: *const c_char,
    primaries: *const c_char,
    profile: *const u8,
    profile_length: usize,
    limit: u64,
    other_retained: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut crate::precision_document::PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "capturing native host HDR pixels",
            ptr::null_mut(),
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                job.check()?;
                if bytes.is_null() {
                    return Err("host HDR samples are required".into());
                }
                let name = borrow_cstr_bounded(layout, "host layout", 64)?;
                let transfer = borrow_cstr_bounded(transfer, "host HDR transfer", 64)?;
                let layout = HostLayout::parse(name)?;
                let linear = match transfer {
                    "linear" => true,
                    "pq" | "hlg" => false,
                    other => {
                        return Err(format!(
                            "host HDR transfer \"{other}\" is not one of linear, pq or hlg"
                        ))
                    }
                };
                if linear {
                    if !primaries.is_null() {
                        return Err("linear HDR host pixels take their primaries from the profile; primaries must be NULL".into());
                    }
                    if profile.is_null() || profile_length == 0 {
                        return Err("linear HDR host pixels require their original ICC".into());
                    }
                } else if !profile.is_null() || profile_length != 0 {
                    return Err("PQ and HLG host pixels carry no ICC; profile must be NULL".into());
                }
                let plan = HostPixelPlan::hdr(
                    (width, height),
                    row_bytes,
                    length,
                    layout,
                    orientation,
                    profile_length,
                )?;
                let input = std::slice::from_raw_parts(bytes, length);
                plan.admit_float(input, profile_length, limit, other_retained, job)?;
                let encoding = if linear {
                    HdrEncoding::Linear(linear_profile(std::slice::from_raw_parts(
                        profile,
                        profile_length,
                    ))?)
                } else {
                    HdrEncoding::Signal {
                        transfer: HdrTransfer::parse(transfer)?,
                        primaries: primaries_named(borrow_cstr_bounded(
                            primaries,
                            "host HDR primaries",
                            64,
                        )?)?,
                    }
                };
                let document = plan.float(input, encoding, job)?;
                job.report(1., 5)?;
                Ok(document)
            },
            boxed,
        )
    }
}
