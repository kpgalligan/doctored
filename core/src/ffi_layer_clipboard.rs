//! The layer clipboard's two exports: Edit ▸ Paste of a layer
//! clipboard at 8 bits and at 16 bits / 32-bit float, the shims of
//! `doc_paste_layers` (whose module doc is the rule). Both take `preserve`:
//! at 8 and 16 bits it relabels a clip of another profile after the Assign
//! check. The typed one also carries the promotion into a float document
//! and the `report_out` JSON (`layer_clipboard_float`).

use std::ffi::c_char;
use std::ptr;

use crate::doc::DrDocument;
use crate::ffi_util::{boxed, fallible_doc_op, fallible_op, required_handle, write_string};
use crate::layer_insert::InsertPosition;
use crate::precision_clipboard::PasteProfile;
use crate::precision_document::PrecisionDocument;

/// Paste every record of `layers` (a layer clipboard: a document built by
/// Duplicate to New Document) into `doc` at the slot `position` names
/// relative to `anchor`, translated by `(dx, dy)` first through the Move
/// tool's rule. Every record mints a fresh id, link ids are re-numbered to
/// unused ones and every root joins the slot's clip run. A clip of another
/// profile is refused when `preserve` is false; when it is true (Don't
/// Convert) the clip is RELABELLED: refused in Assign words when this
/// document's profile fails the Assign check for the copied layers,
/// otherwise every record keeps its numbers and placed images stay bound,
/// re-pointed without a re-render. Refuses (NULL plus a message) a NULL
/// `layers`, an unknown position code, a refused slot, a profile mismatch
/// under convert, nesting past the byte limit and growth past native
/// capacity. NULL `doc` answers NULL with `err_out` untouched. The result is
/// ONE new document; neither input changes.
///
/// # Safety
/// `doc` and `layers` must each be NULL or a valid pointer to a live
/// `DrDocument`; `err_out` must be NULL or a valid pointer to writable
/// `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_paste_layers(
    doc: *const DrDocument,
    layers: *const DrDocument,
    anchor: usize,
    position: u32,
    dx: i32,
    dy: i32,
    preserve: bool,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_doc_op(doc, err_out, "pasting layers", |d| {
            let clip = required_handle(layers)?;
            let position = InsertPosition::from_code(position)?;
            let profile = if preserve {
                PasteProfile::Preserve
            } else {
                PasteProfile::Convert
            };
            d.paste_layers(clip, anchor, position, (dx, dy), profile)
                .map(Some)
        })
    }
}

/// The typed twin of [`dr_doc_paste_layers`], admitted against `limit` with
/// `other_retained_bytes` held elsewhere before anything is built. A 16-bit
/// `document` takes a 16-bit clip and refuses any other depth; a clip of
/// another profile is refused when `preserve` is false and relabelled, after
/// the Assign check (`color_plan(_, false)`), when it is true.
/// A 32-bit float `document` takes a clip of either depth and any profile:
/// the copied layers are promoted into it under `preserve` (false converts
/// into the document's profile, true keeps the numbers, a placed image's
/// included), a placed image landing placed (re-rendered from its master
/// under convert, its binding kept over the promoted plane under preserve),
/// text and shapes staying described, a stale placed image and warps
/// landing as their pixels, a Pass Through group as
/// Pass Through and the blend modes in `FLOAT_RESET_MODES` as Normal. A copied
/// layer the float mode still cannot hold refuses the whole paste, naming
/// the layer and the feature. `report_out` (may be NULL) receives, on
/// success only, an owned JSON string `{"rasterized":[{"layer","reason"}],
/// "blend_reset":[{"layer","from"}],
/// "effect_blend_reset":[{"layer","effect","from","to"}]}` (every list
/// empty when nothing was promoted); free it with `dr_string_free`. NULL plus
/// a message for every refusal, a NULL handle included. Free the result with
/// `dr_precision_doc_free`.
///
/// # Safety
/// `document` and `layers` must each be NULL or a valid pointer to a live
/// `PrecisionDocument`; `report_out` and `err_out` must each be NULL or a
/// valid pointer to writable `*mut c_char`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_paste_layers(
    document: *const PrecisionDocument,
    layers: *const PrecisionDocument,
    anchor: usize,
    position: u32,
    dx: i32,
    dy: i32,
    preserve: bool,
    limit: u64,
    other_retained_bytes: u64,
    report_out: *mut *mut c_char,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "pasting high-depth layers",
            ptr::null_mut(),
            || {
                let doc = required_handle(document)?;
                let clip = required_handle(layers)?;
                let position = InsertPosition::from_code(position)?;
                let profile = if preserve {
                    PasteProfile::Preserve
                } else {
                    PasteProfile::Convert
                };
                doc.paste_layers(
                    clip,
                    anchor,
                    position,
                    (dx, dy),
                    profile,
                    limit,
                    other_retained_bytes,
                )
            },
            |(next, report)| {
                write_string(report_out, &report.json());
                boxed(next)
            },
        )
    }
}
