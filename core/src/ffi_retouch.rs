//! Source display/captured plate and explicit byte destination-growth bridges,
//! and the typed retouch whose plate comes from another document.
use crate::doc::DrDocument;
use crate::dr_image::DrImage;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, required_handle};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{PrecisionImage, PrecisionStorage};
use crate::precision_retouch::RetouchEdit;
use crate::retouch_sampling::RetouchSampling;

/// Transforms the captured native plate before display quantization.
/// # Safety
/// image/control NULL/live; matrix NULL or six readable f64; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_transform_retouch_source(
    image: *const PrecisionImage,
    matrix: *const f64,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionImage {
    unsafe {
        fallible_op(
            err_out,
            "transforming native retouch source",
            ptr::null_mut(),
            || {
                let image = required_handle(image)?;
                if matrix.is_null() {
                    return Err("retouch source matrix is required".into());
                }
                let matrix: [f64; 6] = std::slice::from_raw_parts(matrix, 6)
                    .try_into()
                    .map_err(|_| "invalid matrix")?;
                let (w, h) = image.dimensions();
                // The output plane in the plate's own storage: 8 bytes a
                // pixel at 16 bits, 16 at 32-bit float.
                let scratch =
                    u64::from(w) * u64::from(h) * image.storage().bytes_per_pixel() + 4096;
                crate::precision_image::admit(image.retained_bytes(), scratch, other, limit)?;
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                job.reserve(scratch)?;
                let profile = image.profile().clone();
                match image.storage() {
                    PrecisionStorage::Rgba16Unorm => {
                        let samples = image.transient_rgba16()?;
                        let out =
                            crate::retouch_transform::transform(&samples, (w, h), &matrix, job)?;
                        PrecisionImage::from_rgba16_unseeded(w, h, out, profile, None)
                    }
                    PrecisionStorage::Rgba32Float => {
                        let samples = image.transient_rgba32()?;
                        let out = crate::retouch_transform::transform_extended(
                            &samples,
                            (w, h),
                            &matrix,
                            job,
                        )?;
                        PrecisionImage::from_rgba32_extended(w, h, out, profile, None)
                    }
                }
            },
            boxed,
        )
    }
}
use std::ffi::c_char;
use std::ptr;

/// Captured authoritative native canvas plate. Source/display owners are separate.
/// On a CMYK document (#1) the plate's Display P3 RGBA16 view.
/// # Safety
/// Handles NULL/live; err_out NULL/writable; owner keeps all handles alive.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_retouch_source(
    doc: *const PrecisionDocument,
    index: usize,
    sample: u32,
    ignore_adjustments: bool,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionImage {
    unsafe {
        fallible_op(
            err_out,
            "reading native retouch source",
            ptr::null_mut(),
            || {
                let doc = required_handle(doc)?;
                let mode =
                    RetouchSampling::from_code(sample).ok_or("invalid retouch sample mode")?;
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                // A CMYK document's plate as the view, a display adapter
                // (#1, `cmyk_retouch`).
                if doc.is_cmyk() {
                    return doc.cmyk_retouch_view(
                        index,
                        mode,
                        ignore_adjustments,
                        limit,
                        other,
                        job,
                    );
                }
                doc.retouch_source_with_context(index, mode, ignore_adjustments, limit, other, job)
            },
            boxed,
        )
    }
}

/// `dr_precision_doc_edit` for one `retouch_clone` or `retouch_heal` request,
/// with the sampling plate rendered from `plates` instead of from `document`
/// (`PrecisionDocument::retouch_from`). The request must write its
/// target layer in place (output `layer`, no `expand_destination`) and
/// `plates` must have `document`'s canvas size, its bit depth and a
/// raster layer at the sampled index (a CMYK document takes CMYK plates
/// with a CMYK raster there, #1; a Grayscale document asks its edit gate,
/// `gray_document::admit_edit`, and takes Grayscale plates only,
/// `gray_filter::CLONE_SOURCE_REFUSAL`; a Lab Color document asks
/// `lab_document::admit_edit` and takes Lab Color plates only,
/// `lab_retouch::CLONE_SOURCE_REFUSAL`, #1 phase 2e). `length` must equal the request's planned `input_samples`
/// on `document`, recomputed here before the samples are borrowed. NULL with
/// no error: nothing would change. NULL with an error: a refusal.
/// # Safety
/// document/plates/control NULL/live; request NULL/terminated UTF-8; samples
/// NULL or readable for length u16s; err_out NULL/writable. Owners outlive
/// the call.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_retouch_from(
    document: *const PrecisionDocument,
    plates: *const PrecisionDocument,
    text: *const c_char,
    samples: *const u16,
    length: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "applying high-depth retouch from plates",
            ptr::null_mut(),
            || {
                let doc = required_handle(document)?;
                let plates = required_handle(plates)?;
                let text = borrow_cstr_bounded(text, "request", 64 * 1024)?;
                if doc.is_bitmap() {
                    crate::bitmap_document::admit_edit(text)?;
                    // #1 Bitmap phase 3: Clone Stamp is admitted, but a
                    // retouch from another state is Content-Aware Move's,
                    // which a Bitmap document is not offered.
                    return Err(crate::bitmap_document::refused_sentence(
                        "content_aware_move",
                    ));
                }
                if doc.is_gray() {
                    crate::gray_document::admit_edit(text, doc.is_float())?;
                    if !plates.is_gray() {
                        return Err(crate::gray_filter::CLONE_SOURCE_REFUSAL.into());
                    }
                }
                if doc.lab_policy().is_some() {
                    crate::lab_document::admit_edit(text)?;
                    if !plates.is_lab() {
                        return Err(crate::lab_retouch::CLONE_SOURCE_REFUSAL.into());
                    }
                }
                if doc.is_indexed() {
                    crate::indexed_document::admit_edit(text)?;
                    // #1 Indexed phase 3c: Clone Stamp is admitted, but a
                    // retouch from another state is Content-Aware Move's,
                    // which does not run on indexed colour.
                    return Err(crate::indexed_document::refused_sentence(
                        "content_aware_move",
                    ));
                }
                let edit = RetouchEdit::parse(text)?;
                doc.retouch_from_refusal(plates, &edit)?;
                let expected = doc.retouch_plan(&edit)?.input_samples;
                if length != expected || samples.is_null() {
                    return Err(
                        "high-depth input sample length does not match the core plan".into(),
                    );
                }
                let samples = std::slice::from_raw_parts(samples, expected);
                doc.retouch_from(plates, &edit, samples, limit, other, control.as_ref())
            },
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Same native premultiplied bilinear source mapping used by typed retouch.
/// # Safety
/// image/control NULL/live; matrix NULL or six readable f64; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_image_transform_retouch_source(
    image: *const DrImage,
    matrix: *const f64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "transforming retouch source",
            ptr::null_mut(),
            || {
                let image = required_handle(image)?;
                if matrix.is_null() {
                    return Err("retouch source matrix is required".into());
                }
                let matrix: [f64; 6] = std::slice::from_raw_parts(matrix, 6)
                    .try_into()
                    .map_err(|_| "invalid matrix")?;
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let pixels = image.pixels.width() as u64 * image.pixels.height() as u64;
                job.reserve(pixels * 12 + 4096)?;
                let input: Vec<[u8; 4]> = image.pixels.pixels().map(|p| p.0).collect();
                let out = crate::retouch_transform::transform(
                    &input,
                    image.pixels.dimensions(),
                    &matrix,
                    job,
                )?;
                let bytes = out.into_iter().flatten().collect();
                DrImage::from_rgba(image.pixels.width(), image.pixels.height(), bytes)
                    .ok_or("retouch transformed extent refused".into())
            },
            boxed,
        )
    }
}

/// Expands storage to its union with the canvas; old samples/mask placement exact.
/// # Safety
/// document/control NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_expand_retouch_destination(
    document: *const DrDocument,
    index: usize,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_op(
            err_out,
            "expanding retouch destination",
            ptr::null_mut(),
            || {
                let doc = required_handle(document)?;
                let layer = doc
                    .raster_layer(index)
                    .ok_or("retouch requires raster pixels")?;
                if layer.retained_source.is_some() {
                    return Err(
                        "Rasterize the retained source before expanding retouch pixels".into(),
                    );
                }
                if doc.lock_block(index, crate::doc_lock::EditKind::Pixels) != 0 {
                    return Err("retouch target pixels are locked".into());
                }
                let (dimensions, offset) = crate::retouch_transform::expanded_extent(
                    (doc.width, doc.height),
                    layer.pixels.dimensions(),
                    layer.offset,
                )?;
                if !doc.native_layers_fit_dimensions(doc.layers.iter().enumerate().map(|(i, l)| {
                    if i == index {
                        dimensions
                    } else {
                        l.pixels.dimensions()
                    }
                })) {
                    return Err("retouch expansion exceeds native capacity".into());
                }
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                job.reserve(u64::from(dimensions.0) * u64::from(dimensions.1) * 4 + 4096)?;
                if dimensions == layer.pixels.dimensions() && offset == layer.offset {
                    return Ok(doc.clone());
                }
                crate::retouch_transform::grown_layer(doc, index, (dimensions, offset), job)
            },
            boxed,
        )
    }
}
