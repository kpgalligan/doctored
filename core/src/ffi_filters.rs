//! C FFI for the additional filters (`dr_image_hue_rotate` etc.), following
//! the conventions of `ffi`.
//!
//! Every function is panic-safe: bodies run under the shared
//! [`pure_op`] wrapper (`ffi_util`) so no unwinding ever crosses the FFI
//! boundary. NULL images and invalid arguments yield NULL results.

use crate::doc::DrDocument;
use crate::ffi_util::pure_op;
use crate::job_control::{DrJobControl, JobContext};
use crate::{ops_filters, DrImage};

/// Rotates hue by `degrees` via the standard hue-rotation matrix. Any finite
/// value is accepted; NULL on NaN (or infinities). Alpha untouched.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_hue_rotate(img: *const DrImage, degrees: f32) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            ops_filters::hue_rotate(&i.pixels, degrees).map(|pixels| DrImage { pixels })
        })
    }
}

/// Luma threshold at `level` in [0, 1]: color channels become black or white
/// by Rec. 709 luma; NULL if `level` is NaN or out of range. Alpha untouched.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_threshold(img: *const DrImage, level: f32) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            ops_filters::threshold(&i.pixels, level).map(|pixels| DrImage { pixels })
        })
    }
}

/// Reduces each color channel to `levels` values (bin
/// `k = floor(c*levels/256)` of the code `c`, output code
/// `floor(255*k/(levels-1))`); NULL unless `levels` is in 2..=64. Alpha
/// untouched.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_posterize(img: *const DrImage, levels: u32) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            ops_filters::posterize(&i.pixels, levels).map(|pixels| DrImage { pixels })
        })
    }
}

/// Mosaic of `block` x `block` cells (alpha-weighted mean color, plain mean
/// alpha); NULL unless `block` is in 1..=1024.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_pixelate(img: *const DrImage, block: u32) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            ops_filters::pixelate(&i.pixels, block).map(|pixels| DrImage { pixels })
        })
    }
}

/// Additive uniform noise in [-amount, +amount] per color channel,
/// deterministic for a given `seed`; NULL unless `amount` is in (0, 1].
/// Alpha untouched.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_noise(
    img: *const DrImage,
    amount: f32,
    seed: u64,
) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            ops_filters::noise(&i.pixels, amount, seed).map(|pixels| DrImage { pixels })
        })
    }
}

/// Sobel edge magnitude on luma, rendered as an opaque grayscale image.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_edge_detect(img: *const DrImage) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            Some(DrImage {
                pixels: ops_filters::edge_detect(&i.pixels),
            })
        })
    }
}

/// Emboss: 3x3 directional relief kernel on luma around mid-gray, opaque.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_image_emboss(img: *const DrImage) -> *mut DrImage {
    unsafe {
        pure_op(img, |i| {
            Some(DrImage {
                pixels: ops_filters::emboss(&i.pixels),
            })
        })
    }
}

/// The `params_json` byte cap of `op`: `liquify`'s stroke list gets
/// `liquify::MAX_REQUEST_BYTES` (8 MiB, the arithmetic is on the constant),
/// `depth_blur`'s depth map `depth_blur::MAX_REQUEST_BYTES` (8 MiB),
/// `develop_filter`'s masks `develop_filter_masks::MAX_REQUEST_BYTES` (16 MiB: the
/// host's generated maps ride in them); every other op keeps
/// `ops_spatial::FILTER_OP_PARAMS_MAX_BYTES` (64 KiB).
fn filter_params_cap(op: &str) -> usize {
    match op {
        "liquify" => crate::liquify::MAX_REQUEST_BYTES,
        "depth_blur" => crate::depth_blur::MAX_REQUEST_BYTES,
        "develop_filter" => crate::develop_filter_masks::MAX_REQUEST_BYTES,
        _ => crate::ops_spatial::FILTER_OP_PARAMS_MAX_BYTES,
    }
}

/// `params_json` as the filter's parameter object, shared by
/// [`dr_image_filter_op`] and [`dr_image_filter_op_scratch_bytes`]: NULL means
/// `{}`, the text is at most `max_bytes` ([`filter_params_cap`] of the op),
/// and it must be a JSON object.
///
/// # Safety
/// `params_json` must be NULL or a valid NUL-terminated C string.
unsafe fn filter_params(
    params_json: *const std::ffi::c_char,
    max_bytes: usize,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    if params_json.is_null() {
        return Ok(serde_json::Map::new());
    }
    let text =
        unsafe { crate::ffi_util::borrow_cstr_bounded(params_json, "filter params", max_bytes) }?;
    match serde_json::from_str(text).map_err(|e| format!("invalid filter params: {e}"))? {
        serde_json::Value::Object(map) => Ok(map),
        _ => Err("filter params must be a JSON object".into()),
    }
}

/// The byte twin of the 16-bit selected filters of the same names, through ONE
/// dispatch (`ops_spatial::filter_op`), twenty-one ops: `box_blur`,
/// `surface_blur`, `add_grain`, `radial_blur`, `clouds`, `difference_clouds`,
/// `offset`, `motion_blur`, `lens_blur`, `tilt_shift`,
/// `lens_correction` (all defaults the exact bypass) and `presence`
/// (Texture, Clarity and Dehaze; all three at 0 the exact bypass) and
/// `develop_filter` (the Develop panel's thirteen global controls in one
/// fixed order; all at 0 the exact bypass; with `masks`, ordered local
/// adjustments through `develop_filter_masks`) and `smart_sharpen` (Smart
/// Sharpen; never a bypass) through `ops_spatial::SpatialKernel`,
/// and `unsharp_mask`,
/// `high_pass`, `dust_and_scratches` and `reduce_noise` through
/// `ops_photographic::apply`, and `blur`, Gaussian Blur
/// through `sampling::fitted_blur` (the fitted kernel from radius
/// `gaussian_blur_filter::FITTED_MIN_RADIUS`, 7, up, `sampling::blur`
/// below it; key `sigma`, above 0 and at most 1000, default 4; never a
/// bypass and no work ceiling), and `liquify`
/// through `liquify::plane` (key `strokes`; no work ceiling, and a bypass when
/// its strokes leave the field the identity), and `depth_blur`
/// through `depth_blur::DepthBlur::plane_in` (Filter ▸ Neural Filters ▸ Depth
/// Blur: the host's depth map in the params; no work ceiling, and a bypass
/// when it would change nothing). `params_json` (NULL means
/// `{}`, at most 64 KiB, or 8 MiB for `liquify` and `depth_blur` and 16 MiB
/// for `develop_filter`) is a JSON object of that op's
/// keys, parsed by `SelectedFilter::parse`
/// with the 16-bit filter's ranges and defaults (the header lists them), plus
/// the optional `region_*` frame of `radial_blur` and `offset`. NULL
/// `img` returns NULL with no error, before `control` is read, and so does an
/// exact bypass. NULL `control` runs uncontrolled; a non-NULL one is claimed
/// (single-use) before the op and parameters are validated, a cancel returns
/// NULL with "operation cancelled", and controlled output is byte-identical to
/// uncontrolled. Any other op, bad JSON, an invalid parameter or (with NULL
/// `control` only) a plan over the destructive work ceiling returns NULL with a
/// message through `err_out`; a controlled call has no work ceiling.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`; `op` and
/// `params_json` must be NULL or valid NUL-terminated C strings; `control` must
/// be NULL or a live job control (it may be cancelled concurrently, not freed
/// during the call); `err_out` must be NULL or point to writable storage. Free
/// the message with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_image_filter_op(
    img: *const DrImage,
    op: *const std::ffi::c_char,
    params_json: *const std::ffi::c_char,
    control: *const DrJobControl,
    err_out: *mut *mut std::ffi::c_char,
) -> *mut DrImage {
    unsafe { filter_op_under(None, img, op, params_json, control, err_out) }
}

/// `dr_image_filter_op` on `img` (a layer of `doc`, or a picture made from
/// one) under `doc`'s evaluation policy: the same ops, keys, caps, control,
/// bypasses and refusals. On a `linear_light` document `liquify` resamples
/// in premultiplied linear light (each tap decoded through the document's
/// transfer, the result encoded once; a pixel the field leaves in place is
/// its stored sample); every other op, and every op on an encoded document,
/// is `dr_image_filter_op`'s result byte for byte. NULL `doc` or `img`
/// returns NULL with no error, before `control` is read.
/// `dr_image_filter_op_scratch_bytes` is the plan for both.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; the rest is
/// `dr_image_filter_op`'s.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_filter_op(
    doc: *const DrDocument,
    img: *const DrImage,
    op: *const std::ffi::c_char,
    params_json: *const std::ffi::c_char,
    control: *const DrJobControl,
    err_out: *mut *mut std::ffi::c_char,
) -> *mut DrImage {
    let Some(doc) = (unsafe { doc.as_ref() }) else {
        return std::ptr::null_mut();
    };
    unsafe { filter_op_under(doc.linear_unit(), img, op, params_json, control, err_out) }
}

/// The body of both filter exports: `linear` is `None` for the bare image
/// (`dr_image_filter_op`) and for an encoded document, and a linear-light
/// document's transfer (`dr_doc_filter_op`).
///
/// # Safety
/// The exports' contracts.
unsafe fn filter_op_under(
    linear: Option<&dyn crate::sampling_linear::LinearUnit>,
    img: *const DrImage,
    op: *const std::ffi::c_char,
    params_json: *const std::ffi::c_char,
    control: *const DrJobControl,
    err_out: *mut *mut std::ffi::c_char,
) -> *mut DrImage {
    unsafe {
        crate::ffi_util::fallible_op(
            err_out,
            "applying a filter",
            std::ptr::null_mut(),
            || {
                let Some(image) = img.as_ref() else {
                    return Ok(None);
                };
                let token = control.as_ref();
                let _run = token.map(DrJobControl::begin).transpose()?;
                let job = token.map_or(JobContext::NONE, JobContext::new);
                let op = crate::ffi_util::borrow_cstr_bounded(op, "filter op", 64)?;
                let params = filter_params(params_json, filter_params_cap(op))?;
                let result =
                    crate::ops_spatial::filter_op_in(&image.pixels, op, &params, linear, job)?;
                job.check()?;
                job.report(1.0, 5)?;
                Ok(result)
            },
            |pixels| {
                pixels.map_or(std::ptr::null_mut(), |pixels| {
                    crate::ffi_util::boxed(DrImage { pixels })
                })
            },
        )
    }
}

/// `dr_image_filter_op`'s planned peak scratch for the same `op` and
/// `params_json` on `img` (`ops_spatial::filter_op_scratch_bytes`): the
/// kernel's plan or its held float plane plus the output, whichever is larger,
/// plus 4 MiB, plus a quarter; `blur` holds no float plane beside its output,
/// so its plan is `sampling::fitted_blur_scratch_bytes` (the n×4 output
/// plus one band of the fitted kernel from radius 7, `sampling::blur`'s
/// below it) plus 4 MiB, plus a quarter. Allocates no pixels. NULL `img` returns 0 with
/// no error, and so does an exact bypass. A refusal `dr_image_filter_op` makes
/// before allocating (op, JSON, parameter) returns 0 with the same message
/// through `err_out`; the plan carries no work refusal. `liquify`'s
/// stroke-work refusal (a stroke list past `liquify::MAX_WORK`) is one of the
/// parameter refusals, not a work ceiling, so this export makes it; its
/// `params_json` may be 8 MiB, as in `dr_image_filter_op`.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`; `op` and
/// `params_json` must be NULL or valid NUL-terminated C strings; `err_out` must
/// be NULL or point to writable storage. Free the message with dr_string_free.
#[no_mangle]
pub unsafe extern "C" fn dr_image_filter_op_scratch_bytes(
    img: *const DrImage,
    op: *const std::ffi::c_char,
    params_json: *const std::ffi::c_char,
    err_out: *mut *mut std::ffi::c_char,
) -> u64 {
    unsafe {
        crate::ffi_util::fallible_op(
            err_out,
            "planning a filter",
            0,
            || {
                let Some(image) = img.as_ref() else {
                    return Ok(0);
                };
                let op = crate::ffi_util::borrow_cstr_bounded(op, "filter op", 64)?;
                let params = filter_params(params_json, filter_params_cap(op))?;
                crate::ops_spatial::filter_op_scratch_bytes(image.pixels.dimensions(), op, &params)
            },
            |bytes| bytes,
        )
    }
}
