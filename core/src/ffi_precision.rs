//! Typed internal high-depth bridge. These handles are incompatible with the
//! legacy byte image/document API. Explicit RGBA8 previews are separate values.
//! Every fallible export uses the shared panic/error boundary.

use std::collections::HashSet;
use std::ffi::{c_char, CString};
use std::ptr;

use crate::ffi_resources::handles;
use crate::ffi_util::{boxed, fallible_op, free_handle, handle_get, read_cstr, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_codec::PrecisionFormat;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::PrecisionImage;
use crate::resources::Inventory;
use crate::DrImage;

/// Open genuine Gray/GrayA/RGB/RGBA16 PNG/TIFF or typed native v11. No byte
/// fallback (a grayscale source becomes a 16-bit RGB document; see
/// `gray16_import`).
/// # Safety
/// path is NULL or a terminated UTF-8 string; control is NULL/live; err_out is NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_open(
    path: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "opening high-depth document",
            ptr::null_mut(),
            || {
                PrecisionDocument::open_preserving(
                    &read_cstr(path, "path")?,
                    limit,
                    other,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Free a typed document. NULL is a no-op.
/// # Safety
/// document is NULL or an unfreed owned precision document; no concurrent users remain.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_free(document: *mut PrecisionDocument) {
    unsafe { free_handle(document) }
}

/// Owned JSON storage/evaluation/structure/resource plan. Free with dr_string_free.
/// # Safety
/// document is NULL/live; err_out is NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_info(
    document: *const PrecisionDocument,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "inspecting high-depth document",
            ptr::null_mut(),
            || {
                CString::new(required_handle(document)?.info()?.to_string())
                    .map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Compact owned resource/storage JSON without layer names or descriptions.
/// Free the returned string with dr_string_free.
/// # Safety
/// document is NULL/live; err_out is NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_descriptor(
    document: *const PrecisionDocument,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "describing high-depth document",
            ptr::null_mut(),
            || {
                CString::new(required_handle(document)?.descriptor()?.to_string())
                    .map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Clone a raster leaf's exact typed source; a group or invalid index returns NULL.
/// # Safety
/// document is NULL/live. Returned owner is freed with dr_precision_image_free.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_layer_source(
    document: *const PrecisionDocument,
    index: usize,
) -> *mut PrecisionImage {
    unsafe {
        handle_get(document, ptr::null_mut(), |d| {
            d.layer_source(index)
                .cloned()
                .map_or(ptr::null_mut(), boxed)
        })
    }
}

/// Authoritative u16 projection; groups retain f32 intermediates. NULL on failure.
/// # Safety
/// document/control are NULL/live; optional control is single-use; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_render(
    document: *const PrecisionDocument,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionImage {
    unsafe {
        fallible_op(
            err_out,
            "rendering high-depth document",
            ptr::null_mut(),
            || required_handle(document)?.render(limit, other, control.as_ref()),
            boxed,
        )
    }
}

/// Record `index`'s raw pixels on a transparent canvas-sized typed image in
/// the document's render storage (RGBA16, or RGBA32F for a float document):
/// a raster's exact samples at its offset, a group's RAW projection (the
/// rule Copy of a group uses, `raw_group_document`). An adjustment,
/// an out-of-range index, scratch past `limit` and a cancel refuse by name;
/// NULL document is NULL with "required handle is NULL", as
/// `dr_precision_doc_render`.
/// # Safety
/// document/control are NULL/live; optional control is single-use; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_layer_canvas_image(
    document: *const PrecisionDocument,
    index: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionImage {
    unsafe {
        fallible_op(
            err_out,
            "rendering high-depth layer",
            ptr::null_mut(),
            || required_handle(document)?.layer_canvas_image(index, limit, other, control.as_ref()),
            boxed,
        )
    }
}

/// Pure layer duplication. Invalid input errors; a no-op returns NULL without error.
/// # Safety
/// document is NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_duplicate_layer(
    document: *const PrecisionDocument,
    index: usize,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "duplicating high-depth layer",
            ptr::null_mut(),
            || required_handle(document)?.duplicate_layer(index),
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Pure opacity edit. An unchanged value returns NULL without error.
/// # Safety
/// document is NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_set_layer_opacity(
    document: *const PrecisionDocument,
    index: usize,
    opacity: f32,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "setting high-depth layer opacity",
            ptr::null_mut(),
            || required_handle(document)?.set_layer_opacity(index, opacity),
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Save exact typed state atomically. Does not render or consume a byte preview.
/// # Safety
/// document/control NULL/live, path NULL/terminated UTF-8, err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_save_native(
    document: *const PrecisionDocument,
    path: *const c_char,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "saving high-depth native document",
            false,
            || {
                required_handle(document)?.save_native(
                    &read_cstr(path, "path")?,
                    limit,
                    other,
                    control.as_ref(),
                )
            },
            |()| true,
        )
    }
}

/// Export authoritative PNG16 (0) or TIFF16 (1), returning DR_CARRIES_* bits
/// through report_out only on success. Compound codec export is noncancellable.
/// # Safety
/// document NULL/live, path NULL/terminated UTF-8; report_out/err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_export16(
    document: *const PrecisionDocument,
    path: *const c_char,
    format: u32,
    embed_profile: bool,
    strip_metadata: bool,
    limit: u64,
    other: u64,
    report_out: *mut u32,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "exporting high-depth document",
            false,
            || {
                let format = match format {
                    0 => PrecisionFormat::Png,
                    1 => PrecisionFormat::Tiff,
                    _ => return Err("unsupported high-depth export format".into()),
                };
                required_handle(document)?.export16(
                    &read_cstr(path, "path")?,
                    format,
                    embed_profile,
                    strip_metadata,
                    limit,
                    other,
                )
            },
            |report| {
                if !report_out.is_null() {
                    *report_out = report;
                }
                true
            },
        )
    }
}

/// Free a typed image. NULL is a no-op.
/// # Safety
/// image NULL or an unfreed owned precision image; no concurrent users remain.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_free(image: *mut PrecisionImage) {
    unsafe { free_handle(image) }
}

/// Width of typed source/projection, zero for NULL.
/// # Safety
/// image NULL/live.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_width(image: *const PrecisionImage) -> u32 {
    unsafe { handle_get(image, 0, |i| i.dimensions().0) }
}

/// Height of typed source/projection, zero for NULL.
/// # Safety
/// image NULL/live.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_height(image: *const PrecisionImage) -> u32 {
    unsafe { handle_get(image, 0, |i| i.dimensions().1) }
}

/// Borrowed native-endian straight RGBA16, width*height*4 u16 elements, row0 top.
/// NULL for NULL or a non-u16 prototype image, and when the view cannot be
/// allocated. Source latent RGB is preserved.
/// # Safety
/// image NULL/live; returned memory remains borrowed only until image is freed.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_pixels_rgba16(
    image: *const PrecisionImage,
) -> *const u16 {
    unsafe {
        // 13B-3e-4a: the image pins a private copy it lends
        // (`pinned_rgba16`), so the pointer stays valid while the handle
        // lives; an allocation failure answers NULL.
        handle_get(image, ptr::null(), |i| {
            i.pinned_rgba16().map_or(ptr::null(), |p| p.as_ptr().cast())
        })
    }
}

/// Opaque identity of the image's pixel storage (`PrecisionImage::plane_key`:
/// its tile index, or its one contiguous buffer); equal nonzero values share
/// every sample, nothing copied. 0 for NULL. Reads no pixel, allocates
/// nothing, binds no view or pin.
/// # Safety
/// image NULL/live.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_storage_identity(image: *const PrecisionImage) -> u64 {
    unsafe { handle_get(image, 0, |i| i.plane_key() as u64) }
}

/// Borrowed ICC bytes. Writes length0/NULL for a NULL image.
/// # Safety
/// image NULL/live, length_out NULL/writable. Borrow valid until image is freed.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_profile(
    image: *const PrecisionImage,
    length_out: *mut usize,
) -> *const u8 {
    if !length_out.is_null() {
        unsafe {
            *length_out = 0;
        }
    }
    unsafe {
        handle_get(image, ptr::null(), |i| {
            let bytes = i.profile().bytes();
            if !length_out.is_null() {
                *length_out = bytes.len();
            }
            bytes.as_ptr()
        })
    }
}

/// Explicit lossy byte preview, freed by dr_image_free. Carries no ICC itself:
/// host must retain source profile separately. Never an authoritative source.
/// # Safety
/// image/control NULL/live, control single-use, err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_preview_rgba8(
    image: *const PrecisionImage,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "creating high-depth display preview",
            ptr::null_mut(),
            || {
                Ok(DrImage {
                    pixels: required_handle(image)?.to_rgba8(limit, other, control.as_ref())?,
                })
            },
            boxed,
        )
    }
}

/// Layer-panel thumbnail of a u16 image as RGBA8 (no dither), freed by
/// dr_image_free: aspect-fit with the longest side max_side, never upscaled
/// (max_side 0 or a smaller image keeps its size). A smaller layer is the
/// whole image quantized then Triangle-resized at 8 bits, byte for byte the
/// dr_precision_image_preview_rgba8 + dr_image_resize pair; one at least 16x
/// the thumbnail per axis is box-reduced out of its tiles, resized at u16 and
/// quantized once, measured within 2/255 of that pair, with no whole-layer copy.
/// Refuses f32 storage (`to_rgba8`'s sentence), a working-limit overflow and
/// a cancel; carries no ICC.
/// # Safety
/// image/control NULL/live, control single-use, err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_image_thumbnail_rgba8(
    image: *const PrecisionImage,
    max_side: u32,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "creating high-depth layer thumbnail",
            ptr::null_mut(),
            || {
                Ok(DrImage {
                    pixels: required_handle(image)?.thumbnail_rgba8(
                        max_side,
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

/// Record `idx`'s raw u16 mask plane as a Layers-panel thumbnail: opaque
/// gray RGBA8, freed by dr_image_free; no ICC. Aspect-fit with the longest
/// side max_side, never upscaled; each code reads as (v + 128) / 257. Below
/// 16x the thumbnail per axis it is byte for byte that gray image through
/// dr_image_resize (FILTER_BILINEAR); a larger plane is box-reduced first,
/// measured within 2/255, with no whole-plane image. The raw plane at its
/// stored size: no density, feather or enabled flag. Refuses an out-of-range
/// index, a record with no mask, scratch past limit and a cancel. A GROUP's
/// mask (isolated or pass-through) answers its CANVAS view instead — exactly
/// dr_precision_doc_layer_mask_canvas_thumbnail_rgba8, the picture
/// dr_doc_layer_mask_thumbnail shows for a byte group; every other
/// record the raw plane at its stored size.
/// # Safety
/// document/control NULL/live, control single-use, err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_layer_mask_thumbnail_rgba8(
    document: *const PrecisionDocument,
    idx: usize,
    max_side: u32,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "creating high-depth mask thumbnail",
            ptr::null_mut(),
            || {
                Ok(DrImage {
                    pixels: required_handle(document)?.layer_mask_thumbnail_rgba8(
                        idx,
                        max_side,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    )?,
                })
            },
            boxed,
        )
    }
}

/// Alpha channel `idx` as the 16-bit Channels panel's row thumbnail:
/// opaque gray RGBA8 over the canvas, freed by dr_image_free; no ICC.
/// Aspect-fit with the longest side max_side, never upscaled (0 = full
/// size); each code reads as (v + 128) / 257. Below 16x the thumbnail per
/// axis it is byte for byte that gray image through dr_image_resize
/// (FILTER_BILINEAR); a larger plane is box-reduced first, within 2/255,
/// with no canvas-sized copy. Refuses an out-of-range index,
/// scratch past limit and a cancel.
/// # Safety
/// document/control NULL/live, control single-use, err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_channel_thumbnail_rgba8(
    document: *const PrecisionDocument,
    idx: usize,
    max_side: u32,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "creating high-depth channel thumbnail",
            ptr::null_mut(),
            || {
                Ok(DrImage {
                    pixels: required_handle(document)?.channel_thumbnail_rgba8(
                        idx,
                        max_side,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    )?,
                })
            },
            boxed,
        )
    }
}

/// Record `idx`'s mask as the 16-bit Channels panel's row thumbnail:
/// the CANVAS view (the raw plane at its offset, outside coverage beyond it;
/// no density, feather or enabled flag), otherwise exactly
/// dr_precision_doc_channel_thumbnail_rgba8. Refuses an out-of-range index,
/// a record with no mask, scratch past limit and a cancel.
/// # Safety
/// document/control NULL/live, control single-use, err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_layer_mask_canvas_thumbnail_rgba8(
    document: *const PrecisionDocument,
    idx: usize,
    max_side: u32,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "creating high-depth mask thumbnail",
            ptr::null_mut(),
            || {
                Ok(DrImage {
                    pixels: required_handle(document)?.layer_mask_canvas_thumbnail_rgba8(
                        idx,
                        max_side,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    )?,
                })
            },
            boxed,
        )
    }
}

/// One deduplicated inventory of typed source/projection/captured handles.
/// Returns owned JSON, freed by dr_string_free. NULL entries ignored.
/// # Safety
/// Arrays NULL with count0, or readable for count pointers (<=65536). Each
/// non-NULL handle remains live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_resources_report(
    documents: *const *const PrecisionDocument,
    document_count: usize,
    images: *const *const PrecisionImage,
    image_count: usize,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "inspecting high-depth resources",
            ptr::null_mut(),
            || {
                let documents = handles(documents, document_count, "precision document")?;
                let images = handles(images, image_count, "precision image")?;
                let mut inventory = Inventory::default();
                let mut seen = HashSet::new();
                for document in documents {
                    if seen.insert(document as *const PrecisionDocument as usize) {
                        document.record_resources(&mut inventory);
                    }
                }
                seen.clear();
                for image in images {
                    if seen.insert(image as *const PrecisionImage as usize) {
                        image.record_resources(&mut inventory);
                    }
                }
                CString::new(inventory.report(0, 0).to_string()).map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}
