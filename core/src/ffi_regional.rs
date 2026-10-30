//! C entry points for the Phase 13A regional document render (`regional`,
//! `doc_regional`, `precision_regional`): one render at 8 bits and one at
//! 16 bits / 32-bit float. The plan is `DrDocument::render_region_plan` /
//! `PrecisionDocument::render_region_plan` — no export; the render shares its
//! admission, so a plan that succeeds names a render that will not refuse for
//! structure.

use std::ffi::c_char;
use std::ptr;

use crate::doc::DrDocument;
use crate::ffi_util::{boxed, fallible_op, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::PrecisionImage;
use crate::regional::RegionRequest;
use crate::DrImage;

fn request(x: u32, y: u32, width: u32, height: u32, preview_divisor: u32) -> RegionRequest {
    RegionRequest {
        x,
        y,
        width,
        height,
        preview_divisor,
    }
}

/// The 8-bit regional render: ceil(w/d) × ceil(h/d) straight RGBA8, output
/// pixel (i, j) equal to `dr_doc_flattened`'s pixel (x + i·d, y + j·d). A
/// divisor above 1 is a nearest-sampled PREVIEW, never an authoritative
/// replacement. Admits rasters — since 13A-3b-i with any layer style, enabled
/// effects included — pointwise adjustments and — since 13A-3a — isolated and
/// pass-through GROUPS (plain or gated, nested, hidden or empty; since
/// 13A-3b-ii any layer style — a group with an enabled effect is materialized
/// by the reference's own `rendered_group`, charged against the group budget;
/// since 13A-3c a knockout and "Blend Interior Effects as Group" on any style,
/// run through the reference's unit pass one sample at a time) at every level
/// — and, since 13A-4a, a guided Shadows/Highlights at the ROOT (lone or
/// baseless clipped), evaluated through STAGES: the layers below it at
/// divisor 1 over a declared plane, its guide built from that plane by the
/// reference's own guide builder — with or without layer masks (placement,
/// density, feather — a feathered mask declares its blur reach as a spatial
/// halo), grouped into clip units (hidden and zero-opacity members,
/// adjustment and group bases, group members, baseless clipped entries).
/// Refuses by name — never falls back to a full frame — in `regional`'s fixed
/// order: after the unit skip, entry by entry (recursing into groups) a guided
/// Shadows/Highlights inside a group or a clip run ("is Shadows/Highlights
/// with a guide"), plus every refusal `dr_doc_flattened` would raise. The only
/// structure refusal is the reference's own: a contributing group past its
/// nesting limit refuses "rendering exceeds the group nesting limit"; a
/// materialized group's nest refuses with the reference's budget sentence
/// where the reference would. Cancellation is checked every 4096 samples; a
/// cancelled or refused render returns NULL plus an owned error and no partial
/// pixels. NULL document returns NULL without an error.
///
/// # Safety
/// `doc` and `control` must be NULL or live for the whole call (the control
/// may be cancelled concurrently, never freed). `err_out` must be NULL or
/// writable for one pointer. Free the image with `dr_image_free` and errors
/// with `dr_string_free`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_doc_render_region(
    doc: *const DrDocument,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    preview_divisor: u32,
    max_output_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    if doc.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "panic while rendering a document region",
            ptr::null_mut(),
            || {
                let pixels = required_handle(doc)?.render_region(
                    request(x, y, w, h, preview_divisor),
                    max_output_bytes,
                    control.as_ref(),
                )?;
                Ok(DrImage { pixels })
            },
            boxed,
        )
    }
}

/// The typed regional render in the document's render storage (RGBA16, or
/// extended-range RGBA32F for a float document), sample for sample
/// `dr_precision_doc_render`'s projection at (x + i·d, y + j·d), with layer
/// indices in postorder. Admits and refuses as the byte render does — groups
/// included, at every level — and additionally admits typed Blend If (a lone
/// raster or isolated group, a clip base or a member); every typed raster and
/// isolated-group style is admitted (a group with an enabled effect is
/// materialized into a canvas-sized buffer as `render_styled` does), and so
/// are a knockout and a grouped interior (13A-3c); its clip units are the
/// scene builder's (a run of only hidden members is empty here). Raises the
/// evaluation policy's (11E / float) refusal first. Admitted against `limit`
/// with `other_retained_bytes`; cancellation is checked every 4096 samples. A
/// divisor above 1 is a preview. NULL document returns NULL without an error.
///
/// # Safety
/// `document` and `control` must be NULL or live for the whole call. `err_out`
/// must be NULL or writable for one pointer. Free the image with
/// `dr_precision_image_free` and errors with `dr_string_free`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_render_region(
    document: *const PrecisionDocument,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    preview_divisor: u32,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionImage {
    if document.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_op(
            err_out,
            "panic while rendering a high-depth region",
            ptr::null_mut(),
            || {
                required_handle(document)?.render_region(
                    request(x, y, w, h, preview_divisor),
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}
