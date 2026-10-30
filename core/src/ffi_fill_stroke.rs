//! C FFI for Edit ▸ Fill… and Edit ▸ Stroke… (`doc_fill_stroke`). Thin
//! shims only.

use std::ffi::{c_char, c_int};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

use crate::blend::BlendMode;
use crate::doc::DrDocument;
use crate::ffi_util::{doc_get, doc_op, fallible_doc_op, mask_slice};
use crate::job_control::{DrJobControl, JobContext};
use crate::style::StrokePosition;

/// `DrStrokeLocation`'s three codes, as `dr_selection_stroke` reads them.
fn stroke_location(location: c_int) -> Option<StrokePosition> {
    match location {
        0 => Some(StrokePosition::Inside),
        1 => Some(StrokePosition::Center),
        2 => Some(StrokePosition::Outside),
        _ => None,
    }
}

/// Fills layer `idx` with the straight RGBA8 colour `rgba` through blend
/// mode `mode` (`DrBlendMode`) at `opacity` (clamped to [0, 1]), weighted by
/// `mask` — a canvas-sized selection coverage buffer, or NULL for the whole
/// canvas (`w`/`h` must equal the canvas size either way). With
/// `preserve_transparency` the layer's original alpha is restored after the
/// blend. The layer never grows. NULL on NULL `doc`/`rgba`, dimension
/// mismatch, unknown or Pass Through mode, NaN opacity, a non-raster or
/// out-of-range idx, a pixel-locked layer, or when no pixel would change.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `rgba` must
/// be NULL or point to 4 readable bytes; `mask` must be NULL or valid for
/// canvas width*height bytes.
// The parameter list deliberately mirrors the C signature one-for-one;
// bundling them into a struct would only move the count somewhere else.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_doc_fill_selection(
    doc: *const DrDocument,
    idx: usize,
    mask: *const u8,
    w: u32,
    h: u32,
    rgba: *const u8,
    mode: c_int,
    opacity: f32,
    preserve_transparency: bool,
) -> *mut DrDocument {
    if rgba.is_null() {
        return ptr::null_mut();
    }
    let color = unsafe { [*rgba, *rgba.add(1), *rgba.add(2), *rgba.add(3)] };
    let body = |d: &DrDocument| {
        let mode = BlendMode::from_c(mode)?;
        // Validate against the canvas dimensions before touching `mask`, so
        // the raw read below is bounded by the canvas buffer size.
        if w != d.width || h != d.height {
            return None;
        }
        let len = (d.width as usize).checked_mul(d.height as usize)?;
        let mask = unsafe { mask_slice(mask, len) };
        d.fill_selection(idx, mask, color, mode, opacity, preserve_transparency)
    };
    unsafe { doc_op(doc, body) }
}

/// Replaces a canvas-sized selection mask in place with the Edit ▸ Stroke…
/// band `width_px` wide at `location` (`DrStrokeLocation`: 0 Inside,
/// 1 Center, 2 Outside): Outside `dilate(mask) × (255 − mask)`, Inside
/// `mask − erode(mask)` with the canvas edge as a contour (so a Select All
/// strokes a frame), Center half of each. The band is then the mask
/// `dr_doc_fill_selection` fills through. `width_px <= 0` returns true with
/// an all-zero mask. Returns false, the mask untouched, on NULL mask, zero
/// dimensions, a non-finite width, or an unknown location. The shape of
/// `dr_selection_border` (`ffi_doc.rs`).
///
/// # Safety
/// `mask` must be NULL or valid and writable for width*height bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_selection_stroke(
    mask: *mut u8,
    width: u32,
    height: u32,
    width_px: f32,
    location: c_int,
) -> bool {
    let Some(location) = stroke_location(location) else {
        return false;
    };
    if mask.is_null() || width == 0 || height == 0 || !width_px.is_finite() {
        return false;
    }
    let len = width as usize * height as usize;
    let slice = unsafe { std::slice::from_raw_parts_mut(mask, len) };
    catch_unwind(AssertUnwindSafe(|| {
        crate::doc_fill_stroke::stroke_mask(slice, width, height, width_px, location);
    }))
    .is_ok()
}

/// The working memory `dr_doc_stroke_layer_edge` reserves for layer `idx` at
/// `width_px`, in bytes — what a host admits before the call. 0 for a NULL
/// doc, an index that is out of range or not raster, a width that is
/// non-finite or <= 0, or a layer with no alpha > 0 pixel (nothing to
/// outline); `u64::MAX` when the stroke's frame cannot be represented.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_stroke_layer_edge_scratch(
    doc: *const DrDocument,
    idx: usize,
    width_px: f32,
) -> u64 {
    unsafe { doc_get(doc, 0, |d| Some(d.stroke_layer_edge_scratch(idx, width_px))) }
}

/// Edit ▸ Stroke… with nothing selected: strokes layer `idx`'s own alpha
/// edge `width_px` wide at `location` (`DrStrokeLocation`) in LAYER space,
/// including any part beyond the canvas, with the straight RGBA8 `rgba`
/// through `mode` (`DrBlendMode`) at `opacity` (clamped to [0, 1]). The
/// layer grows to its union with the band's bounds — past the canvas if the
/// band reaches there; recorded comp offsets follow the grown origin —
/// unless `preserve_transparency` or a transparency lock keeps its alpha,
/// in which case it never grows.
///
/// NULL with `err_out` untouched: a NULL doc, and no change (a layer with
/// no alpha > 0 pixel, an empty band, or a result identical to the grown
/// input — opacity 0, say). NULL WITH a message: an unknown location, NULL
/// `rgba`, an unknown or Pass Through mode, an index that is out of range
/// or not raster, a retained-source layer ("Rasterize the retained source
/// before stroking its pixels"), a pixel-locked layer, a width that is
/// non-finite or <= 0, NaN opacity, a reservation refusal (`control`'s
/// scratch limit below `dr_doc_stroke_layer_edge_scratch`), cancellation,
/// or a result past native capacity.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `rgba` must
/// be NULL or point to 4 readable bytes; `control` must be NULL or a live,
/// unstarted job control; `err_out` must be NULL or a writable CString
/// pointer, freed with dr_string_free.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_doc_stroke_layer_edge(
    doc: *const DrDocument,
    idx: usize,
    width_px: f32,
    location: c_int,
    rgba: *const u8,
    mode: c_int,
    opacity: f32,
    preserve_transparency: bool,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_doc_op(doc, err_out, "stroking a layer's edge", |d| {
            let location = stroke_location(location).ok_or("unknown stroke location")?;
            if rgba.is_null() {
                return Err("a stroke colour is required".into());
            }
            let color = [*rgba, *rgba.add(1), *rgba.add(2), *rgba.add(3)];
            let mode = BlendMode::from_c(mode).ok_or("unknown blend mode")?;
            let control = control.as_ref();
            let _run = control.map(DrJobControl::begin).transpose()?;
            let job = control.map_or(JobContext::NONE, JobContext::new);
            d.stroke_layer_edge(
                idx,
                width_px,
                location,
                color,
                mode,
                opacity,
                preserve_transparency,
                job,
            )
        })
    }
}
