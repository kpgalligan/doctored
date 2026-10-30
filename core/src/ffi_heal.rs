//! C FFI for retouching: healing (`dr_doc_heal_layer`) and red-eye
//! (`dr_doc_red_eye_layer`). Spot healing's entry point is
//! `dr_doc_spot_heal_layer_job` (in `ffi_jobs`), and Content-Aware Fill's is
//! `ffi_content_aware`'s `dr_doc_content_aware_fill_edit`.
//!
//! Same conventions as `ffi_doc`: the shared helpers in `ffi_util` supply
//! catch_unwind and NULL tolerance, slice lengths are recomputed from the
//! core's own dimensions rather than trusted from the caller, and the
//! ops that can refuse WITH a message go through `fallible_op` — `Ok(None)`
//! is NULL with no message (a domain refusal), `Err` is NULL with one.

use std::ffi::{c_char, CStr};
use std::ptr;

use crate::doc::DrDocument;
use crate::doc_retouch_output::RETOUCH_LAYER_NAME;
use crate::dr_image::DrImage;
use crate::ffi_util::{boxed, doc_op, fallible_op};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_image::BOOKKEEPING_BYTES;
use crate::retouch_sampling::RetouchSampling;
use crate::spot_heal_type::SpotHealType;

/// Poisson-blends the source patch `src` carries into layer `idx`: the
/// source's texture with the destination's illumination. `src` is the same
/// canvas-frame premultiplied overlay `dr_doc_painting_layer` takes (`w`/`h`
/// must equal the canvas size); its alpha is the footprint's coverage and
/// its RGB the already-aligned source. `strength` is clamped to [0, 1].
/// NULL with a message when one region's box exceeds the documented memory
/// limit, or when every region's box together exceeds the documented
/// working-area limit for one call — both measured before anything is
/// healed; NULL with no message on NULL args, dimension mismatch, non-finite
/// strength, out-of-range idx, an empty covered set, a layer extent that
/// misses the canvas, or when no pixel would change.
///
/// `control` may be NULL. When it is not, the token is single-use (a consumed
/// one is refused with "job control has already been used"); the NULL-argument
/// and shape refusals above come first and leave it unclaimed. A cancel
/// returns NULL with "operation cancelled", never a partial document. Scratch
/// is reserved against the token's working-memory limit before the plan and
/// again before the layer read, and is 0 on return. Stages: 1 (claimed), 2
/// (plan measured), 4 per component window, then 1.0 / 5 on return whatever
/// the result.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `src` must
/// be NULL or valid for `w * h * 4` bytes; `control` must be NULL or a valid
/// pointer to a live `DrJobControl`; `err_out` must be NULL or a valid
/// pointer to writable `*mut c_char`.
// The parameter list mirrors the C declaration in `doctored_core.h`
// one-for-one; bundling them into a struct would only move the count
// somewhere else.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_doc_heal_layer(
    doc: *const DrDocument,
    idx: usize,
    src: *const u8,
    w: u32,
    h: u32,
    strength: f32,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    let body = || -> Result<Option<DrDocument>, String> {
        // Validated against the canvas dimensions before touching `src`, so
        // the raw read is bounded by the canvas buffer size.
        let Some((document, overlay)) = (unsafe { overlay_args(doc, src, w, h) }) else {
            return Ok(None);
        };
        let control = unsafe { control.as_ref() };
        let _run = control.map(DrJobControl::begin).transpose()?;
        let result = document.heal_layer(
            idx,
            overlay,
            w,
            h,
            strength,
            control.map_or(JobContext::NONE, JobContext::new),
        )?;
        finish_heal_job(control)?;
        Ok(result)
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while healing",
            ptr::null_mut(),
            body,
            |out| out.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Removes flash red inside a canvas rect on layer `idx`: red DOMINANCE
/// gated by saturation and hue, corrected only where a scoring component
/// fits within `pupil_size` times the rect's SHORTER side. `pupil_size` is a
/// fraction in (0, 1] (1.0 is the intended default) and `darken` is in
/// [0, 1] (0.5 is the default). NULL on an empty or off-canvas rect,
/// out-of-range idx, non-finite parameters, no component within the
/// pupil-size limit, or when no pixel would change.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
// The parameter list mirrors the C declaration in `doctored_core.h`
// one-for-one; bundling them into a struct would only move the count
// somewhere else.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_doc_red_eye_layer(
    doc: *const DrDocument,
    idx: usize,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    pupil_size: f32,
    darken: f32,
) -> *mut DrDocument {
    unsafe {
        doc_op(doc, |d| {
            d.red_eye_layer(idx, x, y, w, h, pupil_size, darken)
        })
    }
}

/// The canvas-sized straight-RGBA8 SAMPLING PLATE a retouch gesture reads
/// from: `sample` is `DR_RETOUCH_SAMPLE_*` (0 current layer, 1 current and
/// below, 2 all layers) and `ignore_adjustments` drops adjustment ENTRIES
/// above `idx` — see `retouch_sampling`'s module doc for what each mode
/// means, and for the deliberate boundary that a layer style, a Blend If or a
/// tone-blended raster layer is ordinary content and is NOT dropped.
///
/// This exists so that no host ever samples a DISPLAY image: the plate is
/// built from the document's own layers by the same compositor the projection
/// uses, in the document's own numbers, with no conversion anywhere.
///
/// `control` may be NULL. When it is not, the plate reserves its scratch
/// against that token before allocating and polls cancellation per tile, so a
/// plate on a large document is as interruptible as the solve that follows it.
///
/// NULL with no message for a NULL document, an unknown `sample` code, or
/// (mode 0 only) an `idx` that names no raster layer. NULL WITH a message on a
/// rendering refusal, a reservation refusal or cancellation.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `control`
/// must be NULL or a valid pointer to a live `DrJobControl`; `err_out` must be
/// NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_retouch_source(
    doc: *const DrDocument,
    idx: usize,
    sample: u32,
    ignore_adjustments: bool,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    let body = || -> Result<Option<DrImage>, String> {
        if doc.is_null() {
            return Ok(None);
        }
        let document = unsafe { &*doc };
        let Some(mode) = RetouchSampling::from_code(sample) else {
            return Ok(None);
        };
        let control = unsafe { control.as_ref() };
        // Single-use like every other job token, and released by the run
        // guard on every path including a panic.
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        // What the caller will still be holding when the plate lands: the
        // plate itself. `sampling_source` measures its own composite scratch
        // on top of this.
        let base = u64::from(document.width)
            .saturating_mul(u64::from(document.height))
            .saturating_mul(4)
            .saturating_add(BOOKKEEPING_BYTES);
        let pixels = crate::inpaint_source::sampling_source(
            document,
            idx,
            mode,
            ignore_adjustments,
            base,
            job,
        )?;
        Ok(pixels.map(|pixels| DrImage { pixels }))
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while building the retouch sampling source",
            ptr::null_mut(),
            body,
            |out| out.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// The name for a created retouch layer: the caller's, or the crate's default
/// when they pass NULL.
///
/// # Safety
/// `name` must be NULL or a valid NUL-terminated C string.
unsafe fn retouch_name(name: *const c_char) -> String {
    if name.is_null() {
        return RETOUCH_LAYER_NAME.to_string();
    }
    unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned()
}

/// Clone Stamp onto a NEW retouch layer: inserts a transparent canvas-sized
/// raster layer directly above `idx` — at `idx`'s own depth, inside the same
/// group, carrying `idx`'s clipped flag — and composites the same
/// premultiplied canvas-frame overlay `dr_doc_painting_layer` takes OVER it,
/// leaving `idx`'s pixels exactly as they were.
///
/// `alpha` is the stroke's own strength, as for `dr_doc_painting_layer`.
/// `name` may be NULL for the default. NULL WITH a message when the target
/// sits inside a group that would gate the new layer a second time; NULL with
/// no message for a NULL/short overlay, a wrong canvas size, an out-of-range
/// or GROUP index, a pixel-locked target, capacity growth, or a stroke that
/// deposits nothing.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `src` must be
/// NULL or valid for `w * h * 4` bytes; `name` must be NULL or a valid
/// NUL-terminated C string; `err_out` must be NULL or a valid pointer to
/// writable `*mut c_char`.
// The parameter list mirrors the C declaration in `doctored_core.h`
// one-for-one; bundling them into a struct would only move the count
// somewhere else.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_doc_clone_to_new_layer(
    doc: *const DrDocument,
    idx: usize,
    src: *const u8,
    w: u32,
    h: u32,
    alpha: f32,
    name: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    let body = || -> Result<Option<DrDocument>, String> {
        let Some((document, overlay)) = (unsafe { overlay_args(doc, src, w, h) }) else {
            return Ok(None);
        };
        document.clone_to_new_layer(idx, overlay, w, h, alpha, &unsafe { retouch_name(name) })
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while cloning onto a new layer",
            ptr::null_mut(),
            body,
            |out| out.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Healing Brush onto a NEW retouch layer. `src` is `dr_doc_heal_layer`'s
/// overlay, unchanged in meaning; `plate` is the canvas-sized STRAIGHT RGBA8
/// CURRENT-AND-BELOW sampling plate from `dr_doc_retouch_source`, which must
/// be the picture the new layer will composite onto — the membrane matches
/// ITS illumination, and a plate that is not that backdrop shows as an outline
/// in the shape of the stroke.
///
/// Every cap, contour rule and refusal is `dr_doc_heal_layer`'s, because that
/// is what runs, against a one-layer temporary document made from the plate.
/// Layer insertion and refusals as `dr_doc_clone_to_new_layer`. `control` as
/// `dr_doc_heal_layer`: NULL allowed, single-use, the NULL/shape refusals
/// (plate included) leave it unclaimed, a cancel is NULL with "operation
/// cancelled", scratch is 0 on return, and it closes at 1.0 / 5.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `src` and
/// `plate` must be NULL or valid for `w * h * 4` bytes; `name` must be NULL or
/// a valid NUL-terminated C string; `control` must be NULL or a valid pointer
/// to a live `DrJobControl`; `err_out` must be NULL or a valid pointer to
/// writable `*mut c_char`.
// The parameter list mirrors the C declaration in `doctored_core.h`
// one-for-one; bundling them into a struct would only move the count
// somewhere else.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_doc_heal_to_new_layer(
    doc: *const DrDocument,
    idx: usize,
    src: *const u8,
    w: u32,
    h: u32,
    strength: f32,
    plate: *const u8,
    name: *const c_char,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    let body = || -> Result<Option<DrDocument>, String> {
        let Some((document, overlay)) = (unsafe { overlay_args(doc, src, w, h) }) else {
            return Ok(None);
        };
        let Some(plate) = (unsafe { canvas_rgba(plate, w, h) }) else {
            return Ok(None);
        };
        let control = unsafe { control.as_ref() };
        let _run = control.map(DrJobControl::begin).transpose()?;
        let result = document.heal_to_new_layer(
            idx,
            overlay,
            w,
            h,
            strength,
            plate,
            &unsafe { retouch_name(name) },
            control.map_or(JobContext::NONE, JobContext::new),
        )?;
        finish_heal_job(control)?;
        Ok(result)
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while healing onto a new layer",
            ptr::null_mut(),
            body,
            |out| out.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Spot Healing Brush onto a NEW retouch layer. `src`'s ALPHA is the
/// footprint and its RGB carries no meaning, exactly as for
/// `dr_doc_spot_heal_layer_job`; `plate` is the current-and-below sampling plate,
/// and it is BOTH what the inpaint samples and the illumination it matches —
/// on a layer that is empty there is nothing else to read. `ring`/`seed`/
/// `spot_type` as `dr_doc_spot_heal_layer_job` (an unknown `spot_type` answers
/// NULL with no message, with the other shape refusals); `control` as
/// `dr_doc_heal_to_new_layer`'s, closing with a last checkpoint and 1.0 / 5.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `src` and
/// `plate` must be NULL or valid for `w * h * 4` bytes; `name` must be NULL or
/// a valid NUL-terminated C string; `err_out` must be NULL or a valid pointer
/// to writable `*mut c_char`.
// The parameter list mirrors the C declaration in `doctored_core.h`
// one-for-one; bundling them into a struct would only move the count
// somewhere else.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_doc_spot_heal_to_new_layer(
    doc: *const DrDocument,
    idx: usize,
    src: *const u8,
    w: u32,
    h: u32,
    strength: f32,
    ring: u32,
    seed: u64,
    spot_type: u32,
    plate: *const u8,
    name: *const c_char,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    let body = || -> Result<Option<DrDocument>, String> {
        let Some((document, overlay)) = (unsafe { overlay_args(doc, src, w, h) }) else {
            return Ok(None);
        };
        let Some(plate) = (unsafe { canvas_rgba(plate, w, h) }) else {
            return Ok(None);
        };
        let Some(kind) = SpotHealType::from_code(spot_type) else {
            return Ok(None);
        };
        let control = unsafe { control.as_ref() };
        let _run = control.map(DrJobControl::begin).transpose()?;
        let result = document.spot_heal_to_new_layer_job(
            idx,
            overlay,
            w,
            h,
            strength,
            ring,
            seed,
            kind,
            plate,
            &unsafe { retouch_name(name) },
            control.map_or(JobContext::NONE, JobContext::new),
        )?;
        finish_heal_job(control)?;
        Ok(result)
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while spot healing onto a new layer",
            ptr::null_mut(),
            body,
            |out| out.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// The close every controlled heal export makes once its kernel returned: a
/// last cancellation checkpoint (so a cancel that landed after the final
/// solve still returns no document), then progress 1.0 at stage 5 whatever
/// the result — `dr_doc_spot_heal_layer_job`'s shape. A NULL control does
/// nothing.
fn finish_heal_job(control: Option<&DrJobControl>) -> Result<(), String> {
    if let Some(control) = control {
        control.checkpoint()?;
        control.report(1.0, 5);
    }
    Ok(())
}

/// The document plus its canvas-sized RGBA8 overlay, or `None` for any of the
/// shape refusals every overlay-taking export makes before it dereferences a
/// pointer: NULL arguments, a `w`/`h` that is not the canvas size, or a length
/// that overflows. The length is recomputed from the CORE's own dimensions and
/// never trusted from the caller.
///
/// # Safety
/// As the callers' own contracts.
unsafe fn overlay_args<'a>(
    doc: *const DrDocument,
    src: *const u8,
    w: u32,
    h: u32,
) -> Option<(&'a DrDocument, &'a [u8])> {
    if doc.is_null() || src.is_null() {
        return None;
    }
    let document = unsafe { &*doc };
    if w != document.width || h != document.height {
        return None;
    }
    let len = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
    Some((document, unsafe { std::slice::from_raw_parts(src, len) }))
}

/// A second canvas-sized RGBA8 buffer under the same rule, for the exports
/// that take a sampling plate beside their overlay.
///
/// # Safety
/// As the callers' own contracts.
unsafe fn canvas_rgba<'a>(ptr: *const u8, w: u32, h: u32) -> Option<&'a [u8]> {
    if ptr.is_null() {
        return None;
    }
    let len = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
    Some(unsafe { std::slice::from_raw_parts(ptr, len) })
}
