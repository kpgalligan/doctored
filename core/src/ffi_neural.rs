//! The Neural Filters bridge (M4): Super Zoom's and JPEG Artifacts
//! Removal's stage, info and documents, over `neural_stage` and
//! `doc_neural`. The host runs the model inside the stage through a C
//! predict callback; every buffer the callback sees is the core's own, its
//! lengths the grid's.

use std::ffi::{c_char, c_void, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, required_handle};
use crate::job_control::DrJobControl;
use crate::neural_stage::{run_bytes, run_wide, Predict, Stage};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::PrecisionImage;
use crate::DrImage;

/// The host's model, called once per tile: `(context, tile, input,
/// input_len, output, output_len)`; false is a failure.
pub type DrNeuralPredict =
    Option<unsafe extern "C" fn(*mut c_void, u64, *const f32, usize, *mut f32, usize) -> bool>;

/// The stage request `stage_json` names.
///
/// # Safety
/// `stage_json` must be NULL or NUL-terminated.
unsafe fn stage(stage_json: *const c_char) -> Result<Stage, String> {
    Stage::parse(unsafe { borrow_cstr_bounded(stage_json, "stage_json", 4096) }?)
}

/// Runs `body` with the host's callback as a [`Predict`], or `None` when it
/// is NULL.
///
/// # Safety
/// `predict` must be NULL or callable with `context` for the call.
unsafe fn with_predict<R>(
    predict: DrNeuralPredict,
    context: *mut c_void,
    body: impl FnOnce(Option<&mut Predict<'_>>) -> R,
) -> R {
    match predict {
        None => body(None),
        Some(function) => {
            let mut call = |tile: u64, input: &[f32], output: &mut [f32]| {
                // Both slices are the core's own buffers at the grid's lengths.
                unsafe {
                    function(
                        context,
                        tile,
                        input.as_ptr(),
                        input.len(),
                        output.as_mut_ptr(),
                        output.len(),
                    )
                }
            };
            body(Some(&mut call))
        }
    }
}

/// The stage's figures for a `width × height` source at `bytes_per_sample`
/// (1 or 2): `{"tiles", "output_width", "output_height", "input_len",
/// "plane_len", "planned_bytes"}`, or NULL with `err_out` for a bad request
/// or an output past the 100-megapixel limit.
///
/// # Safety
/// `stage_json` must be NULL or NUL-terminated; `err_out` NULL or writable.
/// Free the answer with `dr_string_free`.
#[no_mangle]
pub unsafe extern "C" fn dr_neural_stage_info(
    width: u32,
    height: u32,
    stage_json: *const c_char,
    bytes_per_sample: u32,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning the neural filter",
            ptr::null_mut(),
            || {
                let stage = stage(stage_json)?;
                let info = stage.info(width, height, bytes_per_sample)?;
                CString::new(info.to_string())
                    .map_err(|e| format!("could not report the stage: {e}"))
            },
            CString::into_raw,
        )
    }
}

/// One stage over an 8-bit image: Noise Reduction on `img`, the model
/// through `predict` once per tile in index order on this thread, the stitch,
/// Sharpen on the output. NULL with `err_out` untouched when the stage
/// changes nothing; every other NULL carries a message: a NULL `img`, a bad
/// request, `"model": true` with a NULL `predict`, the output limit, "the
/// model failed on tile N", the working limit (`limit` beside `other`), a
/// cancel.
///
/// # Safety
/// `img` must be NULL or a live `DrImage`; `stage_json` NULL or
/// NUL-terminated; `predict` NULL or callable with `context`; `control` NULL
/// or live until the call returns; `err_out` NULL or writable. Free with
/// `dr_image_free`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_image_neural_stage(
    img: *const DrImage,
    stage_json: *const c_char,
    predict: DrNeuralPredict,
    context: *mut c_void,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "running the neural filter",
            ptr::null_mut(),
            || {
                let image = required_handle(img)?;
                let stage = stage(stage_json)?;
                with_predict(predict, context, |predict| {
                    run_bytes(
                        &image.pixels,
                        &stage,
                        predict,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    )
                })
            },
            |out| out.map_or(ptr::null_mut(), |pixels| boxed(DrImage { pixels })),
        )
    }
}

/// [`dr_image_neural_stage`] over a 16-bit image (RGBA16 only; a float
/// refuses "neural filters are not available in a 32-bit document").
///
/// # Safety
/// `img` must be NULL or a live `PrecisionImage`; the rest as
/// `dr_image_neural_stage`. Free with `dr_precision_image_free`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_image_neural_stage(
    img: *const PrecisionImage,
    stage_json: *const c_char,
    predict: DrNeuralPredict,
    context: *mut c_void,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionImage {
    unsafe {
        fallible_op(
            err_out,
            "running the neural filter",
            ptr::null_mut(),
            || {
                let image = required_handle(img)?;
                let stage = stage(stage_json)?;
                with_predict(predict, context, |predict| {
                    run_wide(
                        image,
                        &stage,
                        predict,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    )
                })
            },
            |out| out.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Super Zoom's new 8-bit document: one raster layer "Background" holding
/// `zoomed`, in `source`'s profile at its resolution. NULL with `err_out`
/// for a NULL handle or a zoomed image past the 100-megapixel limit.
///
/// # Safety
/// `source` must be NULL or a live `DrDocument`; `zoomed` NULL or a live
/// `DrImage`; `err_out` NULL or writable. Free with `dr_doc_free`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_super_zoom_document(
    source: *const DrDocument,
    zoomed: *const DrImage,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_op(
            err_out,
            "making the super zoom document",
            ptr::null_mut(),
            || {
                let document = required_handle(source)?;
                let image = required_handle(zoomed)?;
                document.super_zoom_document(&image.pixels)
            },
            boxed,
        )
    }
}

/// Super Zoom's new 16-bit document, admitted against `limit` beside
/// `other_retained_bytes`. A float or CMYK source refuses by name.
///
/// # Safety
/// `source` must be NULL or a live `PrecisionDocument`; `zoomed` NULL or a
/// live `PrecisionImage`; `control` NULL or live until the call returns;
/// `err_out` NULL or writable. Free with `dr_precision_doc_free`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_super_zoom_document(
    source: *const PrecisionDocument,
    zoomed: *const PrecisionImage,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "making the super zoom document",
            ptr::null_mut(),
            || {
                let document = required_handle(source)?;
                let image = required_handle(zoomed)?;
                document.super_zoom_document(image, limit, other_retained_bytes, control.as_ref())
            },
            boxed,
        )
    }
}

/// JPEG Artifacts Removal's 16-bit apply: `pixels` (the layer's own size)
/// replace layer `layer`'s raster in place, dropping its description, or
/// with `new_layer` land on a new raster layer `name` directly above it at
/// the same offset. NULL with `err_out` untouched when nothing changed (a
/// pixel-locked layer in place, identical pixels); every other NULL carries
/// a message.
///
/// # Safety
/// `doc` must be NULL or a live `PrecisionDocument`; `pixels` NULL or a live
/// `PrecisionImage`; `name` NULL or NUL-terminated; `control` NULL or live
/// until the call returns; `err_out` NULL or writable. Free with
/// `dr_precision_doc_free`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_neural_layer_apply(
    doc: *const PrecisionDocument,
    layer: usize,
    pixels: *const PrecisionImage,
    new_layer: bool,
    name: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "applying the neural filter",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let image = required_handle(pixels)?;
                let name = if name.is_null() {
                    None
                } else {
                    Some(borrow_cstr_bounded(name, "name", 1024)?)
                };
                document.neural_layer_apply(
                    layer,
                    image,
                    new_layer,
                    name,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            |out| out.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Colorize (M4 phase 2): the document profile's bytes, or an empty slice for a NULL, empty or
/// oversized buffer (which then refuses as an unsupported profile).
///
/// # Safety
/// `profile` must be NULL or valid for `length` bytes for the call.
unsafe fn profile_slice<'a>(profile: *const u8, length: usize) -> &'a [u8] {
    if profile.is_null() || length == 0 || length > crate::drdc::MAX_DRDC_BLOB_LEN as usize {
        return &[];
    }
    unsafe { std::slice::from_raw_parts(profile, length) }
}

/// `count` checked against `per_cell × side²`, the length the core itself
/// computes from `side`; `what` names the buffer in the refusal.
fn side_len(side: u32, per_cell: usize, count: usize, what: &str) -> Result<usize, String> {
    crate::neural_colorize::check_side(side)?;
    let expected = per_cell * side as usize * side as usize;
    if count != expected {
        return Err(format!(
            "the colorize {what} must hold {per_cell} × side² samples"
        ));
    }
    Ok(expected)
}

/// Colorize's planned working bytes for a `width × height` layer at model
/// side `side` and `bytes_per_sample` (1 or 2); 0 with `err_out` for a side
/// outside 16…1024, another sample size or an empty image.
///
/// # Safety
/// `err_out` must be NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_neural_colorize_plan(
    width: u32,
    height: u32,
    side: u32,
    bytes_per_sample: u32,
    err_out: *mut *mut c_char,
) -> u64 {
    unsafe {
        fallible_op(
            err_out,
            "planning colorize",
            0,
            || crate::neural_colorize_run::plan(width, height, side, bytes_per_sample),
            |bytes| bytes,
        )
    }
}

/// Colorize's model input from an 8-bit layer: `image_out` receives the
/// `3 × side²` channel-major gray RGB of the layer's L* in the document
/// profile `profile`. False with `err_out` for a NULL `img` or `image_out`, a
/// count other than `3 × side²`, an unsupported profile, the working limit
/// (`limit` beside `other_retained_bytes`) or a cancel.
///
/// # Safety
/// `img` must be NULL or a live `DrImage`; `profile` NULL or valid for
/// `profile_length` bytes; `image_out` NULL or writable for `image_count`
/// floats; `control` NULL or live until the call returns; `err_out` NULL or
/// writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_image_neural_colorize_input(
    img: *const DrImage,
    profile: *const u8,
    profile_length: usize,
    side: u32,
    image_out: *mut f32,
    image_count: usize,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "preparing colorize",
            false,
            || {
                let image = required_handle(img)?;
                if image_out.is_null() {
                    return Err("the colorize input buffer is NULL".into());
                }
                let len = side_len(side, 3, image_count, "input")?;
                let out = std::slice::from_raw_parts_mut(image_out, len);
                crate::neural_colorize_run::input_bytes(
                    &image.pixels,
                    profile_slice(profile, profile_length),
                    side,
                    out,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            |()| true,
        )
    }
}

/// [`dr_image_neural_colorize_input`] over a 16-bit image (RGBA16 only; a
/// float refuses "neural filters are not available in a 32-bit document").
///
/// # Safety
/// `img` must be NULL or a live `PrecisionImage`; the rest as
/// `dr_image_neural_colorize_input`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_image_neural_colorize_input(
    img: *const PrecisionImage,
    profile: *const u8,
    profile_length: usize,
    side: u32,
    image_out: *mut f32,
    image_count: usize,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "preparing colorize",
            false,
            || {
                let image = required_handle(img)?;
                if image_out.is_null() {
                    return Err("the colorize input buffer is NULL".into());
                }
                let len = side_len(side, 3, image_count, "input")?;
                let out = std::slice::from_raw_parts_mut(image_out, len);
                crate::neural_colorize_run::input_wide(
                    image,
                    profile_slice(profile, profile_length),
                    side,
                    out,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            |()| true,
        )
    }
}

/// Colorize applied to an 8-bit layer: the model's `2 × side²` channel-major
/// a*, b* `chroma` adjusted by `adjust_json` (`{"saturation",
/// "cyan_red", "magenta_green", "yellow_blue"}` −50…50 and
/// `"color_artifact_reduction"` 0…100, each optional) and carried onto the
/// layer's own L* in the document profile `profile`. NULL with `err_out`
/// untouched when the output equals the input; every other NULL carries a
/// message: a NULL handle or buffer, a count other than `2 × side²`, a bad
/// adjustment, a non-finite chroma, an unsupported profile, the working
/// limit, a cancel.
///
/// # Safety
/// `img` must be NULL or a live `DrImage`; `profile` NULL or valid for
/// `profile_length` bytes; `chroma` NULL or readable for `chroma_count`
/// floats; `adjust_json` NULL or NUL-terminated; `control` NULL or live until
/// the call returns; `err_out` NULL or writable. Free with `dr_image_free`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_image_neural_colorize_apply(
    img: *const DrImage,
    profile: *const u8,
    profile_length: usize,
    side: u32,
    chroma: *const f32,
    chroma_count: usize,
    adjust_json: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "running colorize",
            ptr::null_mut(),
            || {
                let image = required_handle(img)?;
                if chroma.is_null() {
                    return Err("the colorize chroma buffer is NULL".into());
                }
                let len = side_len(side, 2, chroma_count, "chroma")?;
                let chroma = std::slice::from_raw_parts(chroma, len);
                let adjust = borrow_cstr_bounded(adjust_json, "adjust_json", 4096)?;
                crate::neural_colorize_run::apply_bytes(
                    &image.pixels,
                    profile_slice(profile, profile_length),
                    side,
                    chroma,
                    adjust,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            |out| out.map_or(ptr::null_mut(), |pixels| boxed(DrImage { pixels })),
        )
    }
}

/// [`dr_image_neural_colorize_apply`] over a 16-bit image (RGBA16 only; a
/// float refuses by name). NULL with `err_out` untouched when unchanged.
///
/// # Safety
/// `img` must be NULL or a live `PrecisionImage`; the rest as
/// `dr_image_neural_colorize_apply`. Free with `dr_precision_image_free`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_image_neural_colorize_apply(
    img: *const PrecisionImage,
    profile: *const u8,
    profile_length: usize,
    side: u32,
    chroma: *const f32,
    chroma_count: usize,
    adjust_json: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionImage {
    unsafe {
        fallible_op(
            err_out,
            "running colorize",
            ptr::null_mut(),
            || {
                let image = required_handle(img)?;
                if chroma.is_null() {
                    return Err("the colorize chroma buffer is NULL".into());
                }
                let len = side_len(side, 2, chroma_count, "chroma")?;
                let chroma = std::slice::from_raw_parts(chroma, len);
                let adjust = borrow_cstr_bounded(adjust_json, "adjust_json", 4096)?;
                crate::neural_colorize_run::apply_wide(
                    image,
                    profile_slice(profile, profile_length),
                    side,
                    chroma,
                    adjust,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            |out| out.map_or(ptr::null_mut(), boxed),
        )
    }
}
