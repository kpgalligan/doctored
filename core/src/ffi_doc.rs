//! C FFI for the layered document model (`dr_doc_*`). Follows the same
//! conventions as `ffi`: catch_unwind everywhere, NULL-tolerant, errors via
//! heap CStrings released with dr_string_free.

use std::ffi::{c_char, c_double, c_int, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

use image::imageops::FilterType;
use image::{GrayImage, RgbaImage};

use crate::adjust::Adjustment;
use crate::doc::{BlendMode, DrDocument, LayerKind, MaskKind, MAX_PIXELS};
use crate::doc_transform::Affine;
use crate::drdc::MAX_DRDC_META_LEN;
use crate::ffi_util::{
    boxed, doc_get, doc_op, fallible_op, filter_from_c, mask_slice, read_cstr, thumb_dims,
};
use crate::job_control::{DrJobControl, JobContext};
use crate::ops::CompositeMode;
use crate::style_composite::Cut;
use crate::DrImage;

/// Opens a document: "DRDC" (written; legacy "RZDC" read) files load the
/// native layered format, "8BPS"
/// files import Photoshop layers (falling back to the flattened composite on
/// per-layer failures), anything else decodes as a single-layer image.
/// Returns NULL on failure and writes a message through `err_out` (caller
/// frees with `dr_string_free`). `control` is optional and single-use, begun
/// once for the whole open; a cancel answers exactly `operation cancelled`.
///
/// # Safety
/// `path` must be NULL or a valid NUL-terminated C string; `control` must be
/// NULL or a live single-use token; `err_out` must be NULL or a valid pointer
/// to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_open(
    path: *const c_char,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    let body = || {
        let path = unsafe { read_cstr(path, "path") }?;
        DrDocument::open_controlled(&path, unsafe { control.as_ref() })
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while opening document",
            ptr::null_mut(),
            body,
            boxed,
        )
    }
}

/// Wraps an image as a single-"Background"-layer document. NULL if `img` is
/// NULL.
///
/// # Safety
/// `img` must be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_from_image(img: *const DrImage) -> *mut DrDocument {
    if img.is_null() {
        return ptr::null_mut();
    }
    let image = unsafe { &*img };
    match catch_unwind(AssertUnwindSafe(|| {
        DrDocument::from_pixels(image.pixels.clone())
    })) {
        Ok(doc) => Box::into_raw(Box::new(doc)),
        Err(_) => ptr::null_mut(),
    }
}

/// Cheap copy (shares layer pixel buffers). NULL only if `doc` is NULL.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_clone(doc: *const DrDocument) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| Some(d.clone())) }
}

/// Frees a document. NULL is a safe no-op.
///
/// # Safety
/// `doc` must be NULL or a pointer previously returned by this library that
/// has not yet been freed.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_free(doc: *mut DrDocument) {
    if doc.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { Box::from_raw(doc) })));
}

/// Writes the native DRDC format (all layers preserved), atomically like
/// `dr_image_save`. Returns false on failure and writes a message through
/// `err_out`. `control` is optional and single-use, begun once for the whole
/// save; a cancel answers exactly `operation cancelled` and keeps the
/// previous file, and one arriving after the rename leaves the save a
/// success.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `path` must
/// be NULL or a valid NUL-terminated C string; `control` must be NULL or a
/// live single-use token; `err_out` must be NULL or a valid pointer to
/// writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_save_native(
    doc: *const DrDocument,
    path: *const c_char,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    let body = || {
        if doc.is_null() {
            return Err("document is NULL".to_string());
        }
        let document = unsafe { &*doc };
        let path = unsafe { read_cstr(path, "path") }?;
        document.save_native_controlled(&path, unsafe { control.as_ref() })
    };
    unsafe {
        fallible_op(err_out, "panic while saving document", false, body, |()| {
            true
        })
    }
}

/// The 8-bit native writer's planned peak scratch (`drdc_save_plan`): the
/// encoded-body bound, the tables held beside it, the largest plane's encode
/// transient and fixed overhead (no added margin). It does not depend on cached
/// contiguous views. Metadata only; allocates no pixels. NULL or error: 0 with
/// `err_out` set (free with `dr_string_free`).
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `err_out`
/// must be NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_native_save_scratch_bytes(
    doc: *const DrDocument,
    err_out: *mut *mut c_char,
) -> u64 {
    let body = || {
        if doc.is_null() {
            return Err("document is NULL".to_string());
        }
        let document = unsafe { &*doc };
        document.native_save_scratch_bytes()
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while planning the native save",
            0,
            body,
            |bytes| bytes,
        )
    }
}

/// 8-bit native reopen plan (`drdc_reopen_plan`): writes the bytes the reader
/// retains to `retained_out` and its decode peak to `decode_out`; metadata and
/// pointer identity only, independent of cached views. A NULL `doc` or out
/// pointer, or an error: false with `err_out` set (free with
/// `dr_string_free`) and nothing written.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`;
/// `retained_out` and `decode_out` must be NULL or writable for one
/// `uint64_t`; `err_out` must be NULL or a valid pointer to writable
/// `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_native_reopen_plan(
    doc: *const DrDocument,
    retained_out: *mut u64,
    decode_out: *mut u64,
    err_out: *mut *mut c_char,
) -> bool {
    let body = || {
        if doc.is_null() {
            return Err("document is NULL".to_string());
        }
        if retained_out.is_null() || decode_out.is_null() {
            return Err("reopen plan output is NULL".to_string());
        }
        let document = unsafe { &*doc };
        document.native_reopen_plan()
    };
    let success = |plan: crate::drdc_reopen_plan::NativeReopenPlan| {
        unsafe {
            *retained_out = plan.retained_bytes;
            *decode_out = plan.decode_bytes;
        }
        true
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while planning the native reopen",
            false,
            body,
            success,
        )
    }
}

/// Canvas width in pixels; 0 if `doc` is NULL.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_width(doc: *const DrDocument) -> u32 {
    unsafe { doc_get(doc, 0, |d| Some(d.width)) }
}

/// Canvas height in pixels; 0 if `doc` is NULL.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_height(doc: *const DrDocument) -> u32 {
    unsafe { doc_get(doc, 0, |d| Some(d.height)) }
}

/// Number of layers (index 0 = bottom); 0 if `doc` is NULL.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_count(doc: *const DrDocument) -> usize {
    unsafe { doc_get(doc, 0, |d| Some(d.layers.len())) }
}

/// Heap copy of the layer's name (free with `dr_string_free`); NULL on NULL
/// doc or out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_name(doc: *const DrDocument, idx: usize) -> *mut c_char {
    unsafe {
        doc_get(doc, ptr::null_mut(), |d| {
            let name = &d.layers.get(idx)?.name;
            let sanitized = name.replace('\0', " ");
            let cstring = CString::new(sanitized)
                .unwrap_or_else(|_| CString::new("Layer").expect("static string"));
            Some(cstring.into_raw())
        })
    }
}

/// The layer's opacity in [0, 1]; 0.0 on NULL doc or out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_opacity(doc: *const DrDocument, idx: usize) -> f32 {
    unsafe { doc_get(doc, 0.0, |d| Some(d.layers.get(idx)?.opacity)) }
}

/// The layer's blend mode; DR_BLEND_NORMAL on NULL doc or out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_blend_mode(doc: *const DrDocument, idx: usize) -> c_int {
    unsafe { doc_get(doc, 0, |d| Some(d.layers.get(idx)?.blend.to_c())) }
}

/// The layer's visibility flag; false on NULL doc or out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_visible(doc: *const DrDocument, idx: usize) -> bool {
    unsafe { doc_get(doc, false, |d| Some(d.layers.get(idx)?.visible)) }
}

/// The rect the four geometry getters report: a RASTER entry's pixel buffer
/// rect, unchanged, and for a GROUP — which has no buffer of its own — the
/// UNION of its raster descendants' buffer rects (0, 0, 0, 0 when it holds
/// none). One meaning for both kinds, "the rectangle this entry's pixels live
/// in", and O(number of descendants) either way. `None` for an out-of-range
/// index, which the getters map to 0.
///
/// Deliberately NOT the content box. These four are read once per row on
/// every layers-panel reload and once per row in `get_document`, so answering
/// a group with `layer_bounds` cost four per-pixel scans of the whole subtree
/// per row — seconds of blocked main thread on a photo-sized document with a
/// few groups in it. `dr_doc_layer_bounds` is still the content box on EVERY
/// entry, and the callers that mean the opaque box (align, distribute, the
/// agent's `content_*` keys) ask for it by name.
fn entry_rect(doc: &DrDocument, idx: usize) -> Option<(i32, i32, u32, u32)> {
    doc.layers.get(idx)?;
    Some(doc.layer_pixel_rect(idx).unwrap_or((0, 0, 0, 0)))
}

/// The layer's canvas x offset; 0 on NULL doc or out-of-range idx. On a GROUP
/// it is its descendants' buffer union's origin (see `entry_rect`).
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_offset_x(doc: *const DrDocument, idx: usize) -> i32 {
    unsafe { doc_get(doc, 0, |d| Some(entry_rect(d, idx)?.0)) }
}

/// The layer's canvas y offset; 0 on NULL doc or out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_offset_y(doc: *const DrDocument, idx: usize) -> i32 {
    unsafe { doc_get(doc, 0, |d| Some(entry_rect(d, idx)?.1)) }
}

/// The layer's pixel width; 0 on NULL doc or out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_width(doc: *const DrDocument, idx: usize) -> u32 {
    unsafe { doc_get(doc, 0, |d| Some(entry_rect(d, idx)?.2)) }
}

/// The layer's pixel height; 0 on NULL doc or out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_height(doc: *const DrDocument, idx: usize) -> u32 {
    unsafe { doc_get(doc, 0, |d| Some(entry_rect(d, idx)?.3)) }
}

/// Copy of a layer's pixels at the layer's own size; NULL on NULL doc, an
/// out-of-range idx, or a GROUP, which has no pixels of its own.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_image(doc: *const DrDocument, idx: usize) -> *mut DrImage {
    unsafe {
        doc_get(doc, ptr::null_mut(), |d| {
            let pixels = d.raster_layer(idx)?.pixels.to_contiguous().ok()?;
            Some(Box::into_raw(Box::new(DrImage { pixels })))
        })
    }
}

/// A layer's own pixels on a transparent CANVAS-sized image, placed at its
/// offset — the single-layer counterpart of `dr_doc_flattened`. Opacity,
/// blend mode, visibility and the layer mask are ignored: they describe how
/// the layer composites, not what its pixels are. NULL on NULL doc or
/// out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_canvas_image(
    doc: *const DrDocument,
    idx: usize,
) -> *mut DrImage {
    unsafe {
        doc_get(doc, ptr::null_mut(), |d| {
            let pixels = d.layer_canvas_image(idx)?;
            Some(Box::into_raw(Box::new(DrImage { pixels })))
        })
    }
}

/// Aspect-fit thumbnail of a layer with longest side `max(1, max_side)`
/// (Triangle filter; tiny layers are upscaled). A raster at least 16× the
/// thumbnail per axis is box-reduced out of its tiles first, measured within 2/255 of
/// the single Triangle pass, so no whole-layer copy is made; a smaller one
/// keeps that single pass over a transient copy, byte for byte
/// (`layer_thumbnail::fitted_rgba8`). On a GROUP it is that group's
/// PROJECTION, aspect-fit — a layers panel wanting a cheap group row should
/// draw a folder glyph rather than ask for one projection per reload. NULL on
/// NULL doc, out-of-range idx, an empty-sized layer, an absurd target size,
/// or an allocation failure.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_thumbnail(
    doc: *const DrDocument,
    idx: usize,
    max_side: u32,
) -> *mut DrImage {
    unsafe {
        doc_get(doc, ptr::null_mut(), |d| {
            let layer = d.layers.get(idx)?;
            let pixels = if layer.kind == LayerKind::Group {
                let source = d.layer_canvas_image(idx)?;
                let (lw, lh) = source.dimensions();
                if lw == 0 || lh == 0 {
                    return None;
                }
                let (tw, th) = thumb_dims(lw, lh, max_side);
                crate::ops::resize(&source, tw, th, FilterType::Triangle)?
            } else {
                crate::layer_thumbnail::fitted_rgba8(&layer.pixels, max_side)?
            };
            Some(Box::into_raw(Box::new(DrImage { pixels })))
        })
    }
}

/// Canvas-sized straight-alpha projection. A zero `group_buffer_pixels` uses
/// the production limit; a positive value may only lower that limit. Returns
/// NULL and sets `err_out` on a rendering failure or NULL document; never
/// returns a partial image. Lower budgets support capacity qualification.
/// The render runs under `control` (may be NULL; a control is single-use): a
/// cancel returns NULL with `operation cancelled`.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `control`
/// must be NULL or a valid pointer to a live `DrJobControl`; `err_out` must
/// be NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_flattened(
    doc: *const DrDocument,
    group_buffer_pixels: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "panic while rendering document",
            ptr::null_mut(),
            || {
                let document = doc.as_ref().ok_or("document is NULL")?;
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let budget = if group_buffer_pixels == 0 {
                    crate::doc_group::MAX_GROUP_BUFFER_PIXELS
                } else {
                    group_buffer_pixels
                };
                let pixels = document.flattened_cut_with_job(budget, Cut::NONE, job)?;
                Ok(DrImage { pixels })
            },
            boxed,
        )
    }
}

/// Pure setter: returns a new document with layer `idx` renamed. NULL on NULL
/// args or out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `name` must
/// be NULL or a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_with_layer_name(
    doc: *const DrDocument,
    idx: usize,
    name: *const c_char,
) -> *mut DrDocument {
    if name.is_null() {
        return ptr::null_mut();
    }
    let name = unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
    unsafe { doc_op(doc, |d| d.with_layer_name(idx, &name)) }
}

/// Pure setter: returns a new document with layer `idx`'s COLOUR LABEL
/// replaced (0 none, 1..=`DR_LAYER_LABEL_MAX` the Photoshop `lclr` palette).
/// NULL on a NULL doc, an out-of-range idx, a label above the maximum, and
/// for the label the layer already carries — the purity rule every other
/// setter follows, so a no-op never registers an undo step.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_with_layer_label(
    doc: *const DrDocument,
    idx: usize,
    label: u32,
) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.with_layer_label(idx, u8::try_from(label).ok()?)) }
}

/// Layer `idx`'s COLOUR LABEL; 0 on a NULL doc or an out-of-range index,
/// which is also what an unlabelled layer answers.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_label(doc: *const DrDocument, idx: usize) -> u32 {
    unsafe { doc_get(doc, 0, |d| Some(u32::from(d.layer_label(idx)))) }
}

/// Pure setter: returns a new document with layer `idx`'s opacity replaced
/// (clamped to [0, 1]). NULL on NULL doc or out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_with_layer_opacity(
    doc: *const DrDocument,
    idx: usize,
    opacity: f32,
) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.with_layer_opacity(idx, opacity)) }
}

/// Pure setter: returns a new document with layer `idx`'s blend mode
/// replaced. NULL on NULL doc, out-of-range idx, or an unknown mode value.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_with_layer_blend_mode(
    doc: *const DrDocument,
    idx: usize,
    mode: c_int,
) -> *mut DrDocument {
    unsafe {
        doc_op(doc, |d| {
            d.with_layer_blend_mode(idx, BlendMode::from_c(mode)?)
        })
    }
}

/// Pure setter: returns a new document with layer `idx`'s visibility
/// replaced. NULL on NULL doc or out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_with_layer_visible(
    doc: *const DrDocument,
    idx: usize,
    visible: bool,
) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.with_layer_visible(idx, visible)) }
}

/// Pure setter: returns a new document with layer `idx`'s canvas offset
/// replaced. NULL on NULL doc or out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_with_layer_offset(
    doc: *const DrDocument,
    idx: usize,
    x: i32,
    y: i32,
) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.with_layer_offset(idx, x, y)) }
}

/// Pure setter: returns a new document with layer `idx`'s pixels replaced by
/// a copy of `img` (any size; offset and properties kept). NULL on NULL args
/// or out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `img` must
/// be NULL or a valid pointer to a live `DrImage`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_with_layer_pixels(
    doc: *const DrDocument,
    idx: usize,
    img: *const DrImage,
) -> *mut DrDocument {
    if img.is_null() {
        return ptr::null_mut();
    }
    let image = unsafe { &*img };
    unsafe { doc_op(doc, |d| d.with_layer_pixels(idx, image.pixels.clone())) }
}

/// Replaces layer `idx`'s pixels, offset and mask in ONE pure step — the
/// re-render primitive for a described layer whose raster, position and
/// mask change together. `src` is straight RGBA8, `w * h * 4` bytes, row 0
/// top; `mask` is NULL (the layer ends with no mask; `mask_enabled` resets
/// to true) or exactly `w * h` coverage bytes at the pixels' size
/// (`mask_enabled` is kept). Name, opacity, blend mode, visibility,
/// metadata, style and clipped flag survive. NULL on out-of-range idx, NULL
/// src, zero dimensions, or dimensions past the `MAX_PIXELS` ceiling —
/// bounded before anything is read, as for `dr_image_from_rgba`.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `src` must
/// be NULL or a valid pointer to at least `w * h * 4` readable bytes; `mask`
/// must be NULL or a valid pointer to at least `w * h` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_set_layer_content(
    doc: *const DrDocument,
    idx: usize,
    src: *const u8,
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    mask: *const u8,
) -> *mut DrDocument {
    // The dimensions are the buffers' only declared length, so they are
    // bounded before any slice is built from them — the
    // dr_image_from_rgba rule.
    if src.is_null() || w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return ptr::null_mut();
    }
    unsafe {
        doc_op(doc, |d| {
            // Both lengths are recomputed from the same dimensions that
            // size the layer, with checked arithmetic — never trusted from
            // a separate length argument.
            let count = (w as usize).checked_mul(h as usize)?;
            let len = count.checked_mul(4)?;
            let src = std::slice::from_raw_parts(src, len);
            let pixels = RgbaImage::from_raw(w, h, src.to_vec())?;
            let mask = if mask.is_null() {
                None
            } else {
                let plane = std::slice::from_raw_parts(mask, count);
                Some(GrayImage::from_raw(w, h, plane.to_vec())?)
            };
            d.set_layer_content(idx, pixels, (x, y), mask)
        })
    }
}

/// Inserts a layer with a copy of `img`'s pixels (offset 0) above `idx`. NULL
/// on NULL args or out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `img` must
/// be NULL or a valid pointer to a live `DrImage`; `name` must be NULL or a
/// valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_adding_image_layer(
    doc: *const DrDocument,
    idx: usize,
    img: *const DrImage,
    name: *const c_char,
) -> *mut DrDocument {
    if img.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    let image = unsafe { &*img };
    let name = unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
    unsafe {
        doc_op(doc, |d| {
            d.adding_image_layer(idx, image.pixels.clone(), &name)
        })
    }
}

/// `dr_doc_adding_image_layer`'s placement with the new layer at `(x, y)`: a
/// paste is MADE there, so its Photoshop reference point (`fxrp`) is (0, 0).
/// NULL on NULL args or out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `img` must
/// be NULL or a valid pointer to a live `DrImage`; `name` must be NULL or a
/// valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_add_image_layer_at(
    doc: *const DrDocument,
    idx: usize,
    img: *const DrImage,
    name: *const c_char,
    x: i32,
    y: i32,
) -> *mut DrDocument {
    if img.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    let image = unsafe { &*img };
    let name = unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
    unsafe {
        doc_op(doc, |d| {
            d.add_image_layer_at(idx, image.pixels.clone(), &name, (x, y))
        })
    }
}

/// Duplicates layer `idx` (" copy" appended to the name), inserting the
/// duplicate above it. NULL on NULL doc or out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_duplicating_layer(
    doc: *const DrDocument,
    idx: usize,
) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.duplicating_layer(idx)) }
}

/// Removes layer `idx`. NULL on NULL doc, out-of-range idx, or if it is the
/// last remaining layer.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_removing_layer(
    doc: *const DrDocument,
    idx: usize,
) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.removing_layer(idx)) }
}

/// Merges layer `idx` (idx >= 1) into the layer below it: both layers' modes
/// and opacities are baked into the merged pixels (same kernel as the
/// projection), so the merged layer is Normal at opacity 1, keeping only the
/// lower layer's name and visibility, and covers the union of both extents.
/// NULL on NULL doc, idx == 0 / out of range, or a hidden LOWER layer.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_merging_down(
    doc: *const DrDocument,
    idx: usize,
) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.merging_down(idx)) }
}

/// Single-layer document containing the projection, named "Background". NULL
/// only if `doc` is NULL.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_flattening(doc: *const DrDocument) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.flattening().ok()) }
}

/// Paints a canvas-frame PREMULTIPLIED RGBA8 overlay (`src`, `w * h * 4`
/// bytes, row-major with no padding; `w`/`h` must equal the canvas size)
/// onto layer `idx`, mapped through the layer's offset. `alpha` is clamped
/// to [0, 1] and scales the overlay's alpha; OVER paints the overlay over the
/// layer, ERASE uses the overlay's alpha to erase layer alpha and ignores its
/// colour, and where the overlay is fully transparent the layer's bytes pass
/// through exactly. NULL on NULL args,
/// dimension mismatch, unknown mode, NaN alpha, out-of-range idx, or when
/// the layer's extent does not intersect the canvas (no pixel could change).
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `src` must
/// be NULL or a valid pointer to at least `w * h * 4` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_painting_layer(
    doc: *const DrDocument,
    idx: usize,
    src: *const u8,
    w: u32,
    h: u32,
    mode: c_int,
    alpha: f32,
) -> *mut DrDocument {
    if doc.is_null() || src.is_null() {
        return ptr::null_mut();
    }
    let document = unsafe { &*doc };
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let mode = CompositeMode::from_c(mode)?;
        // Validate against the canvas dimensions before touching `src`, so
        // the raw read below is bounded by the canvas buffer size.
        if w != document.width || h != document.height {
            return None;
        }
        let len = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
        let src = unsafe { std::slice::from_raw_parts(src, len) };
        document.painting_layer(idx, src, mode, alpha)
    }));
    match outcome {
        Ok(Some(result)) => Box::into_raw(Box::new(result)),
        _ => ptr::null_mut(),
    }
}

/// Paints a canvas-frame PREMULTIPLIED RGBA8 overlay (`src`, `w`/`h` must
/// equal the canvas size) onto layer `idx` through blend mode `mode`
/// (`DrBlendMode`), scaled by `alpha` (clamped to [0, 1]) — the paint
/// tools' Blend option. `DR_BLEND_NORMAL` delegates to
/// `dr_doc_painting_layer` with `DR_COMPOSITE_OVER` (byte-identical,
/// refusal rules included). NULL on NULL args, dimension mismatch, unknown
/// mode, NaN alpha, out-of-range idx, a layer extent that misses the
/// canvas, or — non-Normal modes only — when no pixel would change.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `src` must
/// be NULL or a valid pointer to at least `w * h * 4` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_painting_layer_blend(
    doc: *const DrDocument,
    idx: usize,
    src: *const u8,
    w: u32,
    h: u32,
    mode: c_int,
    alpha: f32,
) -> *mut DrDocument {
    if src.is_null() {
        return ptr::null_mut();
    }
    let body = |d: &DrDocument| {
        let mode = BlendMode::from_c(mode)?;
        // Validate against the canvas dimensions before touching `src`, so
        // the raw read below is bounded by the canvas buffer size.
        if w != d.width || h != d.height {
            return None;
        }
        let len = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
        let src = unsafe { std::slice::from_raw_parts(src, len) };
        d.painting_layer_blend(idx, src, mode, alpha)
    };
    unsafe { doc_op(doc, body) }
}

/// The colour brush's Blend option: `dr_doc_painting_layer_blend` with the
/// STRAIGHT ink `rgb` (3 bytes) and only the canvas-frame overlay's alpha
/// (`src`, `w`/`h` must equal the canvas size; byte `4i + 3` per pixel) read,
/// as coverage — so a low-coverage stroke blends the exact colour.
/// `DR_BLEND_NORMAL` delegates to `dr_doc_painting_layer` with
/// `DR_COMPOSITE_OVER` and the overlay itself (byte-identical). NULL on the
/// refusals `dr_doc_painting_layer_blend` names, plus a NULL `rgb`.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `src` must
/// be NULL or a valid pointer to at least `w * h * 4` readable bytes; `rgb`
/// must be NULL or a valid pointer to at least 3 readable bytes.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_doc_painting_layer_blend_color(
    doc: *const DrDocument,
    idx: usize,
    src: *const u8,
    w: u32,
    h: u32,
    rgb: *const u8,
    mode: c_int,
    alpha: f32,
) -> *mut DrDocument {
    if src.is_null() || rgb.is_null() {
        return ptr::null_mut();
    }
    let body = |d: &DrDocument| {
        let mode = BlendMode::from_c(mode)?;
        // Validate against the canvas dimensions before touching `src`, so
        // the raw read below is bounded by the canvas buffer size.
        if w != d.width || h != d.height {
            return None;
        }
        let len = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
        let src = unsafe { std::slice::from_raw_parts(src, len) };
        let rgb = unsafe { [*rgb, *rgb.add(1), *rgb.add(2)] };
        d.painting_layer_blend_color(idx, src, rgb, mode, alpha)
    };
    unsafe { doc_op(doc, body) }
}

/// Dodges (brightens, `burn` false) or burns (darkens, `burn` true) layer
/// `idx` where a stroke overlay covers it. `src` is the same canvas-frame
/// premultiplied RGBA8 overlay `dr_doc_painting_layer` takes (`w`/`h` must
/// equal the canvas size); only its alpha channel is read, as per-pixel
/// stroke coverage. `exposure` is clamped to [0, 1]; `range` selects the
/// tonal band (0 shadows, 1 midtones, 2 highlights). `protect_tones` moves
/// each pixel's three channels together (`doc_retouch::protected`, #1).
/// Layer alpha is never
/// touched. NULL on NULL args, dimension mismatch, non-finite exposure,
/// range > 2, out-of-range idx, a layer extent that misses the canvas, or
/// when no pixel would change.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `src` must
/// be NULL or a valid pointer to at least `w * h * 4` readable bytes.
// The parameter list mirrors the C declaration one-for-one; bundling the
// arguments into a struct would only move the count somewhere else.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_doc_dodge_burn_layer(
    doc: *const DrDocument,
    idx: usize,
    src: *const u8,
    w: u32,
    h: u32,
    exposure: f32,
    range: u8,
    burn: bool,
    protect_tones: bool,
) -> *mut DrDocument {
    if src.is_null() {
        return ptr::null_mut();
    }
    let body = |d: &DrDocument| {
        // Validate against the canvas dimensions before touching `src`, so
        // the raw read below is bounded by the canvas buffer size.
        if w != d.width || h != d.height {
            return None;
        }
        let len = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
        let src = unsafe { std::slice::from_raw_parts(src, len) };
        d.dodge_burn_layer(idx, src, w, h, exposure, range, burn, protect_tones)
    };
    unsafe { doc_op(doc, body) }
}

/// Rotates the whole document 90 degrees clockwise.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_rotate90(doc: *const DrDocument) -> *mut DrDocument {
    unsafe { doc_op(doc, DrDocument::rotate90) }
}

/// Rotates the whole document 180 degrees.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_rotate180(doc: *const DrDocument) -> *mut DrDocument {
    unsafe { doc_op(doc, DrDocument::rotate180) }
}

/// Rotates the whole document 90 degrees counter-clockwise.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_rotate270(doc: *const DrDocument) -> *mut DrDocument {
    unsafe { doc_op(doc, DrDocument::rotate270) }
}

/// Mirrors the whole document left-right.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_flip_horizontal(doc: *const DrDocument) -> *mut DrDocument {
    unsafe { doc_op(doc, DrDocument::flip_horizontal) }
}

/// Mirrors the whole document top-bottom.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_flip_vertical(doc: *const DrDocument) -> *mut DrDocument {
    unsafe { doc_op(doc, DrDocument::flip_vertical) }
}

/// Moves the canvas window (offsets shift, layer pixels untouched);
/// bounds-checked against the canvas like `dr_image_crop`. NULL on NULL doc
/// or an invalid rect.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_crop(
    doc: *const DrDocument,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.crop(x, y, w, h)) }
}

/// Changes the canvas size without scaling: every layer's offset shifts by
/// (`origin_x`, `origin_y`), layer pixels untouched. NULL on NULL doc, zero
/// dimension, a canvas over the total-pixel limit, or nothing would change
/// (the same size at origin (0, 0)); the host answers that request as a
/// successful no-op before calling.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_canvas_resize(
    doc: *const DrDocument,
    w: u32,
    h: u32,
    origin_x: i32,
    origin_y: i32,
) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.canvas_resize(w, h, (origin_x, origin_y))) }
}

/// Scales the canvas and every layer proportionally; limits and filter
/// mapping as `dr_image_resize`. NULL on NULL doc, invalid target, or an
/// unknown filter value.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_resize(
    doc: *const DrDocument,
    w: u32,
    h: u32,
    filter: c_int,
) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.resize(w, h, filter_from_c(filter)?)) }
}

/// Transforms ONE layer by an arbitrary affine matrix given in CANVAS
/// coordinates. `affine` points to exactly six doubles in `CGAffineTransform`
/// order — `[a, b, c, d, tx, ty]`, mapping `(x, y)` to
/// `(a*x + c*y + tx, b*x + d*y + ty)`. The layer's new offset and size are the
/// outward-rounded bounding box of its transformed rect; pixels are
/// inverse-mapped and resampled with `filter` (the shared `DrResizeFilter`
/// values), samples outside the source coming out fully transparent. A layer
/// mask is carried through identically and `meta` is preserved.
///
/// NULL on NULL args, out-of-range idx, a non-finite or singular matrix, an
/// unknown filter value, or a destination extent that is empty or past the
/// pixel ceiling.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `affine`
/// must be NULL or a valid pointer to six readable `double`s.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_transform_layer(
    doc: *const DrDocument,
    idx: usize,
    affine: *const c_double,
    filter: c_int,
) -> *mut DrDocument {
    if affine.is_null() {
        return ptr::null_mut();
    }
    // The element count is fixed by the contract (six), never a caller-
    // supplied length, so the slice is bounded before anything reads it.
    let m = unsafe { std::slice::from_raw_parts(affine, 6) };
    let m = Affine::from_array([m[0], m[1], m[2], m[3], m[4], m[5]]);
    unsafe { doc_op(doc, |d| d.transform_layer(idx, m, filter_from_c(filter)?)) }
}

/// Maps layer `idx`'s rect corner-for-corner onto a destination quad: eight
/// doubles, the canvas points of the source rect's TL, TR, BR, BL corners.
/// A parallelogram quad delegates to the affine `transform_layer`, exact
/// fast paths included. NULL for a NULL doc/quad, a non-finite coordinate,
/// a concave/self-intersecting/collapsed quad, an unknown filter, or a
/// destination extent that is empty, outside i32 or over the pixel budget.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `quad`
/// must be NULL or a valid pointer to at least eight readable doubles.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_perspective_layer(
    doc: *const DrDocument,
    idx: usize,
    quad: *const c_double,
    filter: c_int,
) -> *mut DrDocument {
    if quad.is_null() {
        return ptr::null_mut();
    }
    // The element count is fixed by the contract (eight), never a caller-
    // supplied length, so the slice is bounded before anything reads it.
    let q = unsafe { std::slice::from_raw_parts(quad, 8) };
    let q = [q[0], q[1], q[2], q[3], q[4], q[5], q[6], q[7]];
    unsafe { doc_op(doc, |d| d.perspective_layer(idx, q, filter_from_c(filter)?)) }
}

// ------------------------------------------------------- selection & fill --

/// Similar-color selection from the flattened composite: writes a
/// canvas-sized 0/255 mask (w*h bytes, row 0 top) into `mask_out`.
/// `tolerance` is the maximum per-channel RGBA difference; `contiguous`
/// restricts the selection to the connected region around the seed;
/// `sample_size` is the Sample Size (1 = Point Sample; 3, 5, 11, 31, 51 or
/// 101 = the mean of that square about the seed, clipped to the canvas).
/// Returns false on NULL/out-of-canvas input or any other sample size.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`;
/// `mask_out` must be NULL or writable for canvas width*height bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_magic_wand(
    doc: *const DrDocument,
    x: u32,
    y: u32,
    tolerance: u8,
    contiguous: bool,
    sample_size: u32,
    mask_out: *mut u8,
) -> bool {
    if doc.is_null() || mask_out.is_null() {
        return false;
    }
    let document = unsafe { &*doc };
    let mask = catch_unwind(AssertUnwindSafe(|| {
        document.magic_wand(x, y, tolerance, contiguous, sample_size)
    }));
    match mask {
        Ok(Some(mask)) => {
            unsafe { ptr::copy_nonoverlapping(mask.as_ptr(), mask_out, mask.len()) };
            true
        }
        _ => false,
    }
}

/// Similar-color selection from layer `idx` ALONE on a transparent canvas
/// (the wand's "Sample: Current layer"): writes a canvas-sized 0/255 mask
/// into `mask_out`, `sample_size` as `dr_doc_magic_wand`'s. Returns false on
/// NULL input, a seed outside the canvas, an out-of-range index, an
/// adjustment layer or an unlisted sample size.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`;
/// `mask_out` must be NULL or writable for canvas width*height bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_magic_wand_layer(
    doc: *const DrDocument,
    idx: usize,
    x: u32,
    y: u32,
    tolerance: u8,
    contiguous: bool,
    sample_size: u32,
    mask_out: *mut u8,
) -> bool {
    if mask_out.is_null() {
        return false;
    }
    let mask = unsafe {
        doc_get(doc, None, |d| {
            Some(d.magic_wand_layer(idx, x, y, tolerance, contiguous, sample_size))
        })
    };
    match mask {
        Some(mask) => {
            unsafe { ptr::copy_nonoverlapping(mask.as_ptr(), mask_out, mask.len()) };
            true
        }
        None => false,
    }
}

/// Bucket fill on layer `idx`: grows a similar-color region over the
/// layer's own pixels from canvas point (x, y) and paints `rgba`
/// source-over it. `mask` is a canvas-sized selection coverage buffer or
/// NULL; a seed outside the canvas, the layer, or the mask yields NULL.
///
/// # Safety
/// `doc` must be NULL or a valid live `DrDocument`; `rgba` must point to
/// 4 bytes; `mask` must be NULL or valid for canvas width*height bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_bucket_fill(
    doc: *const DrDocument,
    idx: usize,
    x: i32,
    y: i32,
    tolerance: u8,
    rgba: *const u8,
    contiguous: bool,
    mask: *const u8,
) -> *mut DrDocument {
    if rgba.is_null() {
        return ptr::null_mut();
    }
    let color = unsafe { [*rgba, *rgba.add(1), *rgba.add(2), *rgba.add(3)] };
    unsafe {
        doc_op(doc, |d| {
            let mask = mask_slice(mask, d.width as usize * d.height as usize);
            d.bucket_fill(idx, x, y, tolerance, color, contiguous, mask)
        })
    }
}

/// Paints a two-color gradient source-over layer `idx` (the whole layer,
/// scaled by `mask` where given): kind 0 = linear along p0->p1 (clamped
/// past the ends), kind 1 = radial from p0 with radius |p1-p0|. Colors
/// interpolate straight RGBA. NULL if p0 == p1, coordinates are not
/// finite, or the kind is unknown.
///
/// # Safety
/// `doc` must be NULL or a valid live `DrDocument`; `start_rgba` and
/// `end_rgba` must point to 4 bytes; `mask` must be NULL or valid for
/// canvas width*height bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_gradient(
    doc: *const DrDocument,
    idx: usize,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    start_rgba: *const u8,
    end_rgba: *const u8,
    kind: c_int,
    mask: *const u8,
) -> *mut DrDocument {
    if start_rgba.is_null() || end_rgba.is_null() {
        return ptr::null_mut();
    }
    let radial = match kind {
        0 => false,
        1 => true,
        _ => return ptr::null_mut(),
    };
    let start = unsafe {
        [
            *start_rgba,
            *start_rgba.add(1),
            *start_rgba.add(2),
            *start_rgba.add(3),
        ]
    };
    let end = unsafe {
        [
            *end_rgba,
            *end_rgba.add(1),
            *end_rgba.add(2),
            *end_rgba.add(3),
        ]
    };
    unsafe {
        doc_op(doc, |d| {
            let mask = mask_slice(mask, d.width as usize * d.height as usize);
            d.gradient(idx, (x0, y0), (x1, y1), start, end, radial, mask)
        })
    }
}

/// Clears the selected region of layer `idx` to transparency, in PROPORTION
/// to the selection's coverage: with coverage `c` at the pixel's canvas
/// position, `alpha' = round(alpha * (255 - c) / 255)`, and a pixel whose
/// alpha reaches 0 has its color zeroed while a partly surviving one keeps
/// its RGB (straight alpha). `mask` is a canvas-sized coverage buffer;
/// `w`/`h` must equal the canvas size. Only pixels change — the layer's
/// mask, offset and properties survive. A mask selecting nothing succeeds
/// with the pixels unchanged. NULL on NULL args, dimension mismatch, or
/// out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid live `DrDocument`; `mask` must be NULL or
/// valid for at least `w * h` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_clear_selection(
    doc: *const DrDocument,
    idx: usize,
    mask: *const u8,
    w: u32,
    h: u32,
) -> *mut DrDocument {
    unsafe {
        doc_op(doc, |d| {
            // Validate against the canvas dimensions before touching `mask`,
            // so the raw read below is bounded by the canvas buffer size.
            if w != d.width || h != d.height {
                return None;
            }
            let len = (w as usize).checked_mul(h as usize)?;
            d.clear_selection(idx, mask_slice(mask, len)?)
        })
    }
}

/// Gaussian-feathers a selection mask in place (width*height coverage
/// bytes, row 0 top). Sampling clamps to the canvas edges, so a
/// selection touching the border keeps full coverage there. The work is
/// bounded by the selection: the kernel runs over the nonzero bytes'
/// bounding rect grown by its half width. Returns false, the mask
/// untouched, on NULL mask, zero dimensions, or a radius that is not
/// finite or is above 250 (the typed selection's limit,
/// `selection_limits::MAX_SELECTION_RADIUS`); `radius <= 0` returns true
/// and leaves the mask untouched.
///
/// # Safety
/// `mask` must be NULL or valid and writable for width*height bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_selection_feather(
    mask: *mut u8,
    width: u32,
    height: u32,
    radius: f32,
) -> bool {
    if mask.is_null()
        || width == 0
        || height == 0
        || !radius.is_finite()
        || radius > crate::selection_limits::MAX_SELECTION_RADIUS
    {
        return false;
    }
    let len = width as usize * height as usize;
    let slice = unsafe { std::slice::from_raw_parts_mut(mask, len) };
    catch_unwind(AssertUnwindSafe(|| {
        crate::doc_select::feather_mask(slice, width, height, radius);
    }))
    .is_ok()
}

/// Grows (dilates) a selection mask in place by `radius` pixels of true
/// Euclidean distance, working on the signed distance field of the mask's
/// 50% contour (coverage binarized at >= 128 — any feathered softness
/// deliberately resolves to its 50% contour): edges move outward by
/// `radius`, corners round into circular arcs, and the new edge carries a
/// fresh ~1px anti-aliased ramp. The canvas boundary is not a contour, so
/// full and empty masks are fixed points. Returns false on NULL mask, zero
/// dimensions, or a non-finite radius; `radius <= 0` returns true and
/// leaves the mask untouched, exactly like `dr_selection_feather`.
///
/// # Safety
/// `mask` must be NULL or valid and writable for width*height bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_selection_grow(
    mask: *mut u8,
    width: u32,
    height: u32,
    radius: f32,
) -> bool {
    if mask.is_null() || width == 0 || height == 0 || !radius.is_finite() {
        return false;
    }
    let len = width as usize * height as usize;
    let slice = unsafe { std::slice::from_raw_parts_mut(mask, len) };
    catch_unwind(AssertUnwindSafe(|| {
        crate::doc_select::grow_mask(slice, width, height, radius);
    }))
    .is_ok()
}

/// Shrinks (erodes) a selection mask in place by `radius` pixels: grow with
/// `-radius`, with the same 50%-contour binarization, fixed points, and
/// error/no-op conventions as `dr_selection_grow`.
///
/// # Safety
/// `mask` must be NULL or valid and writable for width*height bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_selection_shrink(
    mask: *mut u8,
    width: u32,
    height: u32,
    radius: f32,
) -> bool {
    if mask.is_null() || width == 0 || height == 0 || !radius.is_finite() {
        return false;
    }
    let len = width as usize * height as usize;
    let slice = unsafe { std::slice::from_raw_parts_mut(mask, len) };
    catch_unwind(AssertUnwindSafe(|| {
        crate::doc_select::shrink_mask(slice, width, height, radius);
    }))
    .is_ok()
}

/// Replaces a selection mask in place with an anti-aliased band `width_px`
/// wide straddling its 50% contour (coverage binarized at >= 128, as in
/// `dr_selection_grow`). A full or empty mask has no contour and comes back
/// empty. Returns false on NULL mask, zero dimensions, or a non-finite
/// width; `width_px <= 0` returns true and leaves the mask untouched.
///
/// # Safety
/// `mask` must be NULL or valid and writable for width*height bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_selection_border(
    mask: *mut u8,
    width: u32,
    height: u32,
    width_px: f32,
) -> bool {
    if mask.is_null() || width == 0 || height == 0 || !width_px.is_finite() {
        return false;
    }
    let len = width as usize * height as usize;
    let slice = unsafe { std::slice::from_raw_parts_mut(mask, len) };
    catch_unwind(AssertUnwindSafe(|| {
        crate::doc_select::border_mask(slice, width, height, width_px);
    }))
    .is_ok()
}

/// Smooths a selection mask in place: a Gaussian blur with exactly
/// `dr_selection_feather`'s sigma mapping followed by a smoothstep contrast
/// remap (`t = v/255`, `v' = round(255 * t^2 * (3 - 2t))`) — corners round
/// and jagged edges reconcile while a long straight edge stays put. Never
/// binarizes. The blur is bounded by the selection, as in
/// `dr_selection_feather`. Returns false, the mask untouched, on NULL mask,
/// zero dimensions, or a radius that is not finite or is above 250;
/// `radius <= 0` returns true and leaves the mask untouched.
///
/// # Safety
/// `mask` must be NULL or valid and writable for width*height bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_selection_smooth(
    mask: *mut u8,
    width: u32,
    height: u32,
    radius: f32,
) -> bool {
    if mask.is_null()
        || width == 0
        || height == 0
        || !radius.is_finite()
        || radius > crate::selection_limits::MAX_SELECTION_RADIUS
    {
        return false;
    }
    let len = width as usize * height as usize;
    let slice = unsafe { std::slice::from_raw_parts_mut(mask, len) };
    catch_unwind(AssertUnwindSafe(|| {
        crate::doc_select::smooth_mask(slice, width, height, radius);
    }))
    .is_ok()
}

// ------------------------------------------------------------ layer masks --

/// Maps a raw `DrMaskKind` value onto a `MaskKind`. `selection` is read only
/// for `DR_MASK_FROM_SELECTION`, and only after `w`/`h` have been checked
/// against the canvas — as in `dr_doc_painting_layer`, the slice length comes
/// from the document, so a caller's dimensions can only reject the buffer,
/// never widen the read. `None` on an unknown kind, a NULL selection, or a
/// selection that is not canvas-sized.
///
/// # Safety
/// `selection` must be NULL or valid for `w * h` readable bytes.
unsafe fn mask_kind_from_c<'a>(
    doc: &DrDocument,
    kind: c_int,
    selection: *const u8,
    w: u32,
    h: u32,
) -> Option<MaskKind<'a>> {
    match kind {
        0 => Some(MaskKind::RevealAll),
        1 => Some(MaskKind::HideAll),
        2 => {
            if w != doc.width || h != doc.height {
                return None;
            }
            let len = (w as usize).checked_mul(h as usize)?;
            let selection = unsafe { mask_slice(selection, len)? };
            Some(MaskKind::FromSelection(selection))
        }
        _ => None,
    }
}

/// Gives layer `idx` a mask (replacing any existing one) and enables it. The
/// mask is created at exactly the layer's pixel size; a `DR_MASK_FROM_SELECTION`
/// buffer is canvas-sized (`w`/`h` must equal the canvas) and is cropped to
/// the layer's rect. NULL on NULL doc, out-of-range idx, an unknown kind, or a
/// NULL / wrongly sized selection buffer.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `selection`
/// must be NULL or a valid pointer to at least `w * h` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_adding_layer_mask(
    doc: *const DrDocument,
    idx: usize,
    kind: c_int,
    selection: *const u8,
    w: u32,
    h: u32,
) -> *mut DrDocument {
    unsafe {
        doc_op(doc, |d| {
            d.add_mask(idx, mask_kind_from_c(d, kind, selection, w, h)?)
        })
    }
}

/// Drops layer `idx`'s mask; with `apply` the coverage is first baked into the
/// layer's alpha REGARDLESS of the enabled flag. NULL on NULL doc,
/// out-of-range idx, or a layer with no mask.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_removing_layer_mask(
    doc: *const DrDocument,
    idx: usize,
    apply: bool,
) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.remove_mask(idx, apply)) }
}

/// Pure setter: returns a new document with layer `idx`'s mask enabled or
/// disabled (a disabled mask is retained but ignored while compositing). NULL
/// on NULL doc, out-of-range idx, or a layer with no mask.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_with_layer_mask_enabled(
    doc: *const DrDocument,
    idx: usize,
    enabled: bool,
) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.set_mask_enabled(idx, enabled)) }
}

/// Paints a canvas-frame PREMULTIPLIED RGBA8 overlay (`src`, `w`/`h` must
/// equal the canvas size — the same buffer `dr_doc_painting_layer` takes) onto
/// layer `idx`'s MASK, mapped through the layer's offset: white reveals, black
/// hides, the overlay's alpha carries the stroke's coverage. NULL on NULL
/// args, dimension mismatch, out-of-range idx, a layer with no mask, or when
/// the layer's extent does not intersect the canvas (no mask pixel could
/// change).
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `src` must be
/// NULL or a valid pointer to at least `w * h * 4` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_painting_layer_mask(
    doc: *const DrDocument,
    idx: usize,
    src: *const u8,
    w: u32,
    h: u32,
) -> *mut DrDocument {
    unsafe {
        doc_op(doc, |d| {
            // Validate against the canvas dimensions before touching `src`, so
            // the raw read below is bounded by the canvas buffer size.
            if w != d.width || h != d.height {
                return None;
            }
            let len = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
            d.paint_mask(idx, mask_slice(src, len)?)
        })
    }
}

/// Layer `idx`'s mask as an opaque grayscale image at the LAYER's size; NULL
/// on NULL doc, out-of-range idx, or a layer with no mask.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_mask_image(
    doc: *const DrDocument,
    idx: usize,
) -> *mut DrImage {
    unsafe {
        doc_get(doc, ptr::null_mut(), |d| {
            let pixels = d.mask_image(idx)?;
            Some(Box::into_raw(Box::new(DrImage { pixels })))
        })
    }
}

/// Layer `idx`'s mask as its Layers-panel thumbnail: opaque gray
/// RGBA8, the picture `dr_doc_layer_mask_image` answers (a group's
/// canvas-aligned view, any other layer's raw mask at its stored size),
/// aspect-fit with the longest side `max_side`, never upscaled (0 = full
/// size). Byte-identical to that image followed by `dr_image_resize`
/// (FILTER_BILINEAR) below 16× the thumbnail per axis; a larger mask is
/// box-reduced first, within 2/255, with no whole-mask image. NULL on NULL
/// doc, out-of-range idx, a layer with no mask, a zero-size mask or an
/// allocation failure.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_mask_thumbnail(
    doc: *const DrDocument,
    idx: usize,
    max_side: u32,
) -> *mut DrImage {
    unsafe {
        doc_get(doc, ptr::null_mut(), |d| {
            let pixels = d.layer_mask_thumbnail(idx, max_side)?;
            Some(Box::into_raw(Box::new(DrImage { pixels })))
        })
    }
}

/// Whether layer `idx` has a mask; false on NULL doc or out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_has_mask(doc: *const DrDocument, idx: usize) -> bool {
    unsafe { doc_get(doc, false, |d| Some(d.layers.get(idx)?.mask.is_some())) }
}

/// Whether layer `idx` has a mask AND it is enabled; false on NULL doc,
/// out-of-range idx, or a layer with no mask (there is nothing to enable).
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_mask_enabled(doc: *const DrDocument, idx: usize) -> bool {
    unsafe {
        doc_get(doc, false, |d| {
            let layer = d.layers.get(idx)?;
            Some(layer.mask.is_some() && layer.mask_enabled)
        })
    }
}

// ---------------------------------------------------------- clipping masks --

/// Pure setter: returns a new document with layer `idx`'s clipped flag
/// replaced. A clipped layer is confined to the alpha footprint of the first
/// unclipped layer beneath it (its base); groups are positional and re-derived
/// at every composite, so the flag is all there is to set. NULL on NULL doc or
/// out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_with_layer_clipped(
    doc: *const DrDocument,
    idx: usize,
    clipped: bool,
) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.with_layer_clipped(idx, clipped)) }
}

/// The layer's clipped flag; false on NULL doc or out-of-range idx.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_clipped(doc: *const DrDocument, idx: usize) -> bool {
    unsafe { doc_get(doc, false, |d| Some(d.layers.get(idx)?.clipped)) }
}

// --------------------------------------------------------- layer metadata --

/// Heap copy of layer `idx`'s metadata blob (free with `dr_string_free`);
/// NULL on NULL doc, out-of-range idx, or a layer with no metadata. The core
/// never parses the string — it is the host's blob, returned exactly as it was
/// stored, except that interior NUL bytes (which a DRDC file could carry, its
/// reader being lenient) become spaces so a C string can hold it, as in
/// `dr_doc_layer_name`.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_meta(doc: *const DrDocument, idx: usize) -> *mut c_char {
    unsafe {
        doc_get(doc, ptr::null_mut(), |d| {
            let meta = d.layers.get(idx)?.meta.as_deref()?;
            let sanitized = meta.replace('\0', " ");
            Some(CString::new(sanitized).ok()?.into_raw())
        })
    }
}

/// Pure setter: returns a new document with layer `idx`'s metadata replaced by
/// a copy of `meta`, or CLEARED when `meta` is NULL. The blob is stored
/// verbatim and never interpreted. NULL on NULL doc, out-of-range idx, a
/// payload that is not valid UTF-8 (refused rather than mangled — the host
/// owns the encoding), or one longer than the DRDC writer's cap (so a
/// document can never hold metadata the native format would refuse to store).
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `meta` must
/// be NULL or a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_with_layer_meta(
    doc: *const DrDocument,
    idx: usize,
    meta: *const c_char,
) -> *mut DrDocument {
    let value = if meta.is_null() {
        None
    } else {
        let Ok(s) = unsafe { CStr::from_ptr(meta) }.to_str() else {
            return ptr::null_mut();
        };
        if s.len() > MAX_DRDC_META_LEN as usize {
            return ptr::null_mut();
        }
        Some(s.to_string())
    };
    unsafe {
        doc_op(doc, move |d| {
            d.layers.get(idx)?;
            let mut out = d.clone();
            out.layers[idx].meta = value;
            Some(out)
        })
    }
}

/// Whether layer `idx`'s metadata parses as a color-adjustment description
/// (`{"type":"adjust", ...}` — the one meta shape the compositor interprets;
/// see `adjust` for the schema), i.e. whether the compositor treats the layer
/// as an adjustment layer. False on NULL doc, out-of-range idx, absent meta,
/// or meta that does not parse as an adjustment.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_is_adjustment(doc: *const DrDocument, idx: usize) -> bool {
    unsafe {
        doc_get(doc, false, |d| {
            // Never for a group: the core does not interpret a group's meta,
            // so a blob that happens to parse cannot make one an adjustment.
            let meta = d.raster_layer(idx)?.meta.as_deref()?;
            Some(Adjustment::from_meta(meta).is_some())
        })
    }
}
