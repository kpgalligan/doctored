//! Thin native local-mask and alternate-development boundary. Host pixel work
//! stays in the pure modules; outputs never publish partially on refusal.
//!
//! All public operations admit before allocation and poll single-use job tokens; the host owns
//! camera decoding, recipe semantics and resource SHA verification.
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, required_handle};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, reserved_vec, PrecisionImage, BOOKKEEPING_BYTES};
use crate::raw_local_limits::MAX_RAW_LOCALS;
use crate::raw_local_mask::{
    RawLocalMask, MAX_RAW_LOCAL_COMMAND, MAX_RAW_LOCAL_PIXELS, PARSE_SCRATCH,
};
use std::ffi::{c_char, CString};
use std::{ptr, sync::Arc};

/// Create/edit source-coordinate native coverage; NULL without error is an exact prior no-op.
/// # Safety
/// prior/control NULL/live; command NULL/terminated UTF8; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_raw_local_mask_edit(
    prior: *const RawLocalMask,
    width: u32,
    height: u32,
    command: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut RawLocalMask {
    unsafe {
        fallible_op(
            err_out,
            "editing local RAW coverage",
            ptr::null_mut(),
            || {
                RawLocalMask::edit(
                    prior.as_ref(),
                    width,
                    height,
                    borrow_cstr_bounded(command, "local mask command", MAX_RAW_LOCAL_COMMAND)?,
                    limit,
                    other,
                    control.as_ref(),
                )
            },
            |v| v.map_or(ptr::null_mut(), boxed),
        )
    }
}
/// Release a mask handle. Shared native resource bytes remain alive in other owners.
/// # Safety
/// mask NULL or a live owned handle returned here, freed exactly once and not concurrently used.
#[no_mangle]
pub unsafe extern "C" fn dr_raw_local_mask_free(mask: *mut RawLocalMask) {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "freeing local RAW coverage",
            (),
            || {
                if !mask.is_null() {
                    drop(Box::from_raw(mask));
                }
                Ok(())
            },
            |v| v,
        )
    }
}
/// Owned bounded dimensions/storage JSON, released with dr_string_free.
/// # Safety
/// mask NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_raw_local_mask_info(
    mask: *const RawLocalMask,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "inspecting local RAW coverage",
            ptr::null_mut(),
            || CString::new(required_handle(mask)?.info().to_string()).map_err(|e| e.to_string()),
            CString::into_raw,
        )
    }
}
/// Borrow immutable complete RZLM bytes. Keep the handle alive; never free the returned pointer.
/// # Safety
/// mask NULL/live; length and err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_raw_local_mask_bytes(
    mask: *const RawLocalMask,
    length: *mut usize,
    err_out: *mut *mut c_char,
) -> *const u8 {
    unsafe {
        if !length.is_null() {
            *length = 0;
        }
        fallible_op(
            err_out,
            "borrowing local RAW coverage",
            ptr::null(),
            || {
                let bytes = required_handle(mask)?.encoded();
                if !length.is_null() {
                    *length = bytes.len();
                }
                Ok(bytes.as_ptr())
            },
            |v| v,
        )
    }
}
/// Admitted copy of a strictly framed version1 RZLM blob. Native pixels are never promoted from bytes.
/// # Safety
/// input NULL/readable for length bytes; control NULL/live single-use; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_raw_local_mask_from_bytes(
    input: *const u8,
    length: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut RawLocalMask {
    unsafe {
        fallible_op(
            err_out,
            "reading local RAW coverage",
            ptr::null_mut(),
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                job.check()?;
                if input.is_null() || length < 16 || length as u64 > MAX_RAW_LOCAL_PIXELS * 2 + 16 {
                    return Err("invalid local-mask input length".into());
                }
                let (w, h) = crate::raw_local_mask::header_dimensions(std::slice::from_raw_parts(
                    input, 16,
                ))?;
                let required = 16 + crate::raw_local_mask::dimensions(w, h)? * 2;
                if length != required {
                    return Err("local mask sample length differs from its dimensions".into());
                }
                let bytes = std::slice::from_raw_parts(input, required);
                let scratch = length as u64 + BOOKKEEPING_BYTES;
                admit(length as u64, scratch, other, limit)?;
                job.reserve(scratch)?;
                let mut result = reserved_vec(length)?;
                for chunk in bytes.chunks(65536) {
                    job.check()?;
                    result.extend_from_slice(chunk);
                }
                job.report(1.0, 5)?;
                RawLocalMask::from_blob(Arc::new(result))
            },
            boxed,
        )
    }
}
/// A new mask from a host-generated byte coverage plane (the Vision subject or people mask):
/// width*height samples, row 0 top, 0 outside…255 inside; sample v is stored as u16 v*257.
/// Returns NULL without error when every sample is 0. NULL with err_out for NULL samples,
/// length != width*height, a zero or over-64-megapixel size, a refused admission or a
/// cancelled control.
/// # Safety
/// samples NULL/readable for length bytes; control NULL/live single-use; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_raw_local_mask_from_coverage(
    samples: *const u8,
    length: usize,
    width: u32,
    height: u32,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut RawLocalMask {
    unsafe {
        fallible_op(
            err_out,
            "reading local RAW coverage",
            ptr::null_mut(),
            || {
                let required = crate::raw_local_mask::dimensions(width, height)?;
                if samples.is_null() || length != required {
                    return Err("local mask coverage length differs from its dimensions".into());
                }
                RawLocalMask::from_coverage(
                    std::slice::from_raw_parts(samples, required),
                    width,
                    height,
                    limit,
                    other,
                    control.as_ref(),
                )
            },
            |v| v.map_or(ptr::null_mut(), boxed),
        )
    }
}
/// Copy complete RZLM bytes atomically; output remains unchanged on refusal/cancellation.
/// # Safety
/// mask/control NULL/live; output NULL/writable for length, no overlap with mask; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_raw_local_mask_copy(
    mask: *const RawLocalMask,
    output: *mut u8,
    length: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "copying local RAW coverage",
            false,
            || {
                let mask = required_handle(mask)?;
                if output.is_null() || length != mask.encoded().len() {
                    return Err("local mask output length differs from its payload".into());
                }
                mask.copy(
                    std::slice::from_raw_parts_mut(output, mask.encoded().len()),
                    limit,
                    other,
                    control.as_ref(),
                )
            },
            |()| true,
        )
    }
}
/// Red RGBA8 display overlay, at most1600pixels per side; never an authoritative coverage input.
/// base/range_json are the selected local's range and the base development it reads: both NULL
/// (no range) or both set, one alone is an error. With them the alpha is shape coverage times
/// the range's coverage of the base sample, whatever `effective` says. An f32 base (a
/// 32-bit float development) is read as linear light in its profile's primaries.
/// # Safety
/// mask/base/control NULL/live; control single-use; range_json NULL/terminated UTF8;
/// err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_raw_local_mask_preview(
    mask: *const RawLocalMask,
    base: *const PrecisionDocument,
    range_json: *const c_char,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    divisor: u32,
    density: f64,
    feather: f64,
    effective: bool,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut crate::DrImage {
    unsafe {
        fallible_op(
            err_out,
            "previewing local RAW coverage",
            ptr::null_mut(),
            || {
                let mask = required_handle(mask)?;
                if base.is_null() != range_json.is_null() {
                    return Err("local range preview requires a base and a range together".into());
                }
                let range = (!range_json.is_null())
                    .then(|| {
                        let text = borrow_cstr_bounded(range_json, "local range", 1024)?;
                        serde_json::from_str::<crate::raw_local_range::LocalRange>(text)
                            .map_err(|e| format!("invalid local range: {e}"))
                    })
                    .transpose()?;
                Ok(crate::DrImage {
                    pixels: mask.preview(
                        crate::raw_local_preview::LocalPreview {
                            x,
                            y,
                            width,
                            height,
                            divisor,
                            density,
                            feather,
                            effective,
                        },
                        base.as_ref().zip(range.as_ref()),
                        limit,
                        other,
                        control.as_ref(),
                    )?,
                })
            },
            boxed,
        )
    }
}
/// The mean CIE L*a*b* (D50, L* clamped to 0…100) of the 5×5 square of base-development
/// pixels about source point (x, y), clipped to the frame: the colour a range's
/// Sample Colour click stores. x in 0…width, y in 0…height. Writes 3 doubles and returns
/// true; false with err_out, lab_out untouched, on refusal. An f32 base (a 32-bit float
/// development) is read as linear light, so a sample above white reads L* 100.
/// # Safety
/// base NULL/live; lab_out NULL or writable for 3 doubles; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_raw_range_sample(
    base: *const PrecisionDocument,
    x: f64,
    y: f64,
    lab_out: *mut f64,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "sampling a local RAW range colour",
            false,
            || {
                let base = required_handle(base)?;
                if lab_out.is_null() {
                    return Err("local range sample requires an output".into());
                }
                let lab = crate::raw_local_develop::raw_range_sample(base, x, y)?;
                std::slice::from_raw_parts_mut(lab_out, 3).copy_from_slice(&lab);
                Ok(())
            },
            |()| true,
        )
    }
}

/// Validate the same preview request and return its actual conservative scratch
/// plan without allocating coverage/display pixels. Returns0 with error on refusal.
/// # Safety
/// mask NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_raw_local_mask_preview_scratch(
    mask: *const RawLocalMask,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    divisor: u32,
    density: f64,
    feather: f64,
    effective: bool,
    err_out: *mut *mut c_char,
) -> u64 {
    unsafe {
        fallible_op(
            err_out,
            "planning local RAW coverage preview",
            0,
            || {
                required_handle(mask)?.preview_scratch(crate::raw_local_preview::LocalPreview {
                    x,
                    y,
                    width,
                    height,
                    divisor,
                    density,
                    feather,
                    effective,
                })
            },
            |v| v,
        )
    }
}
/// Mix up to 16 locals through native coverage, then calibrated vignette. A local's variant
/// is its genuine full-source camera development, or NULL for a local whose only controls are
/// its `tone`; at most 4 are non-NULL. A NULL-variant local reads the development as the
/// earlier locals left it; one with an identity tone changes nothing.
/// Optional paired reference_size/reference_from_source registers the existing
/// vignette in saved source coordinates; absent pair keeps legacy arithmetic.
/// settings (at most 32768 bytes) is an array of `count` objects, one per local in order,
/// unknown members refused: `enabled` (bool), `density` (0…1), `feather` (pixels, 0…256),
/// optional `range` (`raw_local_range::LocalRange`: `luminance` and/or `color`) and optional
/// `tone` (`raw_local_tone::LocalTone`: `exposure` −4…4 stops; `contrast`, `whites`, `blacks`,
/// `saturation`, `clarity`, `texture`, `sharpness` −100…100; `highlights`, `shadows`,
/// `noise_reduction` 0…100; absent keys 0, unknown refused). A range multiplies the local's shape coverage by its coverage of the BASE
/// sample, read as CIE L*a*b* (D50) through the base's own profile; on an enabled local it
/// refuses a base profile with no matrix/TRC model. A tone is applied to the local's source
/// sample before the blend; on a 32-bit float base it refuses by name.
/// Returns NULL without error when the resulting samples equal the base exactly.
/// An f32 base (a 32-bit float development) needs f32 variants and answers an f32 float
/// document: blended in linear light with no clamp and no quantization, values above 1 and
/// below 0 kept, the vignette a linear gain; a u16 variant beside it is refused.
/// # Safety
/// Handles/control NULL/live; arrays NULL iff count0, otherwise count readable entries: masks
/// non-NULL, variants live or NULL; JSON pointers NULL/terminated UTF8; settings required,
/// vignette optional; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_develop_locals(
    base: *const PrecisionDocument,
    variants: *const *const PrecisionDocument,
    masks: *const *const RawLocalMask,
    count: usize,
    settings: *const c_char,
    vignette: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "developing local RAW variants",
            ptr::null_mut(),
            || {
                if count > MAX_RAW_LOCALS || count > 0 && (variants.is_null() || masks.is_null()) {
                    return Err("local development requires zero to sixteen paired inputs".into());
                }
                let variants = if count == 0 {
                    &[][..]
                } else {
                    std::slice::from_raw_parts(variants, count)
                };
                let masks = if count == 0 {
                    &[][..]
                } else {
                    std::slice::from_raw_parts(masks, count)
                };
                let variants = variants.iter().map(|v| v.as_ref()).collect::<Vec<_>>();
                let masks = masks
                    .iter()
                    .map(|v| required_handle(*v))
                    .collect::<Result<Vec<_>, _>>()?;
                crate::raw_local_develop::develop(
                    required_handle(base)?,
                    &variants,
                    &masks,
                    borrow_cstr_bounded(
                        settings,
                        "local blend settings",
                        crate::raw_local_develop::MAX_SETTINGS_BYTES,
                    )?,
                    (!vignette.is_null())
                        .then(|| borrow_cstr_bounded(vignette, "manual vignette", 4096))
                        .transpose()?,
                    limit,
                    other,
                    control.as_ref(),
                )
            },
            |v| v.map_or(ptr::null_mut(), boxed),
        )
    }
}
/// Share genuine u16 image samples/profile in a new plain one-layer typed document.
/// An f32 image (a 32-bit float development's master) answers a one-layer 32-bit
/// float document under the Clip tone map, sharing its samples the same way.
/// # Safety
/// image/control NULL/live, control single-use; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_from_image(
    image: *const PrecisionImage,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "capturing a developed source",
            ptr::null_mut(),
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                job.check()?;
                let image = required_handle(image)?;
                admit(image.retained_bytes(), PARSE_SCRATCH, other, limit)?;
                job.reserve(PARSE_SCRATCH)?;
                let result =
                    if image.storage() == crate::precision_image::PrecisionStorage::Rgba32Float {
                        crate::raw_float_develop::float_document(image.clone(), "Background")?
                    } else {
                        PrecisionDocument::from_image(
                            image.clone(),
                            crate::metadata::Metadata::default(),
                            crate::metadata::Resolution::default(),
                        )?
                    };
                job.report(1.0, 5)?;
                Ok(result)
            },
            boxed,
        )
    }
}

macro_rules! mask_attachment {
    ($doc:ty,$read:ident,$attach:ident) => {
        /// Share one structurally validated native local mask from a retained RAW asset.
        /// # Safety
        /// document NULL/live; err_out NULL/writable. Free result with dr_raw_local_mask_free.
        #[no_mangle]
        pub unsafe extern "C" fn $read(
            document: *const $doc,
            layer: usize,
            resource: usize,
            err_out: *mut *mut c_char,
        ) -> *mut RawLocalMask {
            unsafe {
                fallible_op(
                    err_out,
                    "sharing retained local RAW coverage",
                    ptr::null_mut(),
                    || {
                        crate::raw_local_attachment::from_source(
                            required_handle(document)?,
                            layer,
                            resource,
                        )
                    },
                    boxed,
                )
            }
        }
        /// Share up to 16 immutable mask blobs onto a fresh RAW source revision.
        /// NULL descriptor/count0 preserves an already empty fresh attachment state.
        /// # Safety
        /// document/control NULL/live; mask array NULL iff count0 or count readable
        /// non-NULL handles; descriptor NULL/terminated UTF8; err_out NULL/writable.
        #[no_mangle]
        pub unsafe extern "C" fn $attach(
            document: *const $doc,
            layer: usize,
            descriptor: *const c_char,
            masks: *const *const RawLocalMask,
            count: usize,
            limit: u64,
            other: u64,
            control: *const DrJobControl,
            err_out: *mut *mut c_char,
        ) -> *mut $doc {
            unsafe {
                fallible_op(
                    err_out,
                    "attaching local RAW coverage",
                    ptr::null_mut(),
                    || {
                        use crate::raw_source_edit::RawDocument;
                        let control = control.as_ref();
                        let _run = control.map(DrJobControl::begin).transpose()?;
                        let job = control.map_or(JobContext::NONE, JobContext::new);
                        job.check()?;
                        let document = required_handle(document)?;
                        if count > MAX_RAW_LOCALS || count > 0 && masks.is_null() {
                            return Err(
                                "local attachment requires zero to sixteen mask handles".into()
                            );
                        }
                        let masks = if count == 0 {
                            &[][..]
                        } else {
                            std::slice::from_raw_parts(masks, count)
                        };
                        let masks = masks
                            .iter()
                            .map(|m| required_handle(*m))
                            .collect::<Result<Vec<_>, _>>()?;
                        let text = (!descriptor.is_null())
                            .then(|| {
                                borrow_cstr_bounded(descriptor, "local-mask attachment", 65536)
                            })
                            .transpose()?;
                        let mut inventory = crate::resources::Inventory::default();
                        document.raw_inventory(&mut inventory);
                        for mask in &masks {
                            mask.record_resources(&mut inventory);
                        }
                        let scratch = PARSE_SCRATCH + 512 * text.map_or(0, |t| t.len() as u64);
                        admit(inventory.unique_bytes(), scratch, other, limit)?;
                        job.reserve(scratch)?;
                        let descriptor = text
                            .map(crate::source_attachment::AttachmentDescriptor::parse)
                            .transpose()?;
                        crate::raw_local_attachment::attach(
                            document, layer, descriptor, &masks, limit, other, job,
                        )
                    },
                    |v| v.map_or(ptr::null_mut(), boxed),
                )
            }
        }
    };
}
mask_attachment!(
    crate::doc::DrDocument,
    dr_doc_raw_local_mask,
    dr_doc_attach_raw_local_masks
);
mask_attachment!(
    PrecisionDocument,
    dr_precision_doc_raw_local_mask,
    dr_precision_doc_attach_raw_local_masks
);
