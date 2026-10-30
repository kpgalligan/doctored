//! Paste and Paste Special on an Indexed Color document (#547 Indexed phase
//! 2d, `designs/indexed-mode.md`). An adapter, not a kernel: it names
//! the document types.
//!
//! An Indexed Color document holds one layer, so a paste lands IN it. The
//! payload is pasted onto the RGB storage as today's paste does
//! (`ffi_clipboard::dispatch_paste`, so an RGB, Gray, CMYK or Lab payload
//! lands converted into the storage's RGB profile by its own rule), as a new
//! record directly above the Index layer; Paste Into and Paste Outside then
//! give that record the selection as its mask (`mask_edit_with_context`'s
//! `set_mask`, canvas-sized, at the request's outside coverage); Merge Down
//! bakes it into the Index layer (`merge_down`, whose lock rule refuses a
//! locked Index layer by name, and which keeps the Index record's name,
//! label and locks); and the merged layer is re-indexed against the old
//! plane with each pixel's own colour (`indexed_paint::republish`,
//! `EdgeInk::Own`): an unchanged pixel keeps its index, a changed one takes t
//! below alpha 128, else the nearest entry (ties to the lowest index). A
//! pasted colour equal to an entry therefore takes that entry, so a paste
//! from an Indexed document of the same palette with no duplicate entries
//! keeps every index. Indices never cross the clipboard: an Indexed copy is
//! an RGB payload (phase 2c).
//!
//! A paste that moves no index still answers the document (a paste is a
//! history step).
//!
//! The job control is begun here, so the inner RGB paste runs uncontrolled
//! (a control begins once, `indexed_paint::cut`'s precedent) and the mask,
//! merge and re-index stages under it, each with the plan as its
//! reservation floor.
//!
//! The plan's scratch is the RGB paste's own term (`clipboard_paste_plan`
//! less the document's retained bytes for an RGB payload; for a CMYK, Gray
//! or Lab payload, whose conversion admits itself, twice its pixels at
//! 8 B/px), the merge's [`PASTE_MERGE_BYTES_PER_PIXEL`] over the canvas and
//! the payload, and the re-index's `indexed_paint::reindex_bytes`.

use serde_json::{json, Value};

use crate::cmyk_clipboard_codec::AnyClipboardPayload;
use crate::indexed_edge::{EdgeInk, BACKGROUND};
use crate::indexed_paint::{reindex_bytes, republish, NOT_INDEXED};
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_insert::InsertPosition;
use crate::precision_clipboard::PasteRequest;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::admit;
use crate::precision_mask_edit::MaskEdit;

/// The merge's working memory per pixel of the canvas and of the payload:
/// Merge Down's two captured planes (32 B/px over the canvas, `32·n`), its
/// members' render windows over the union frame (32 B/px) and the merged
/// RGBA16 output (8 B/px). Declared before measuring
/// (`scripts/indexed-admission/measure.py`, case `paste-indexed`).
pub(crate) const PASTE_MERGE_BYTES_PER_PIXEL: u64 = 72;

/// The inner RGB paste's request (module doc): `request` as a new record
/// directly above the Index layer.
fn inner_request(request: &PasteRequest) -> PasteRequest {
    PasteRequest {
        name: request.name.clone(),
        anchor: 0,
        position: InsertPosition::Above,
        placement: request.placement,
        profile: request.profile,
        frame: request.frame,
    }
}

/// The paste's plan (module doc): `{"retained_bytes","scratch_bytes"}`.
pub(crate) fn paste_plan(
    doc: &PrecisionDocument,
    payload: &AnyClipboardPayload,
    request: &PasteRequest,
) -> Result<Value, String> {
    doc.indexed_policy().ok_or(NOT_INDEXED)?;
    let rgb = doc.without_indexed();
    let pixels = |(w, h): (u32, u32)| u64::from(w) * u64::from(h);
    let (paste, payload_pixels) = match payload {
        AnyClipboardPayload::Rgb(p) => (
            rgb.clipboard_paste_plan(p, &inner_request(request))?
                .saturating_sub(rgb.retained_bytes()),
            pixels(p.dimensions()),
        ),
        AnyClipboardPayload::Cmyk(p) => {
            let n = pixels(p.dimensions());
            (n.saturating_mul(16), n)
        }
        AnyClipboardPayload::Gray(p) => {
            let n = pixels(p.rgb.dimensions());
            (n.saturating_mul(16), n)
        }
        AnyClipboardPayload::Lab(p) => {
            let n = pixels(p.rgb.dimensions());
            (n.saturating_mul(16), n)
        }
    };
    let (w, h) = doc.dimensions();
    let scratch = paste
        .saturating_add(
            pixels((w, h))
                .saturating_add(payload_pixels)
                .saturating_mul(PASTE_MERGE_BYTES_PER_PIXEL),
        )
        .saturating_add(reindex_bytes(w, h));
    Ok(json!({"retained_bytes": doc.retained_bytes(), "scratch_bytes": scratch}))
}

/// The paste (module doc). `mask` is Paste Into's or Paste Outside's
/// canvas-sized coverage and outside coverage.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paste(
    doc: &PrecisionDocument,
    payload: &AnyClipboardPayload,
    request: &PasteRequest,
    mask: Option<(&[u16], u8)>,
    limit: u64,
    other: u64,
    control: Option<&DrJobControl>,
) -> Result<PrecisionDocument, String> {
    let scratch = paste_plan(doc, payload, request)?["scratch_bytes"]
        .as_u64()
        .ok_or("indexed paste plan has no scratch_bytes")?;
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    admit(doc.retained_bytes(), scratch, other, limit)?;
    job.reserve(scratch)?;
    let pasted = crate::ffi_clipboard::dispatch_paste(
        &doc.without_indexed(),
        payload,
        &inner_request(request),
        limit,
        other,
        None,
    )?;
    job.report(0.3, 5)?;
    let pasted = match mask {
        Some((coverage, outside_coverage)) => {
            let (width, height) = pasted.dimensions();
            let edit = MaskEdit::SetMask {
                layer: 1,
                width,
                height,
                x: 0,
                y: 0,
                linked: false,
                density: 1.0,
                feather: 0.0,
                enabled: true,
                outside_coverage,
            };
            let inner = job.range(0.3, 0.4).with_reservation_floor(scratch);
            pasted
                .mask_edit_with_context(&edit, coverage, limit, other, inner)?
                .unwrap_or(pasted)
        }
        None => pasted,
    };
    let inner = job.range(0.4, 0.7).with_reservation_floor(scratch);
    let Some(merged) = pasted.merge_down(1, limit, other, inner)? else {
        return Ok(doc.clone());
    };
    let out = republish(
        doc,
        &merged,
        EdgeInk::Own,
        true,
        BACKGROUND,
        job.range(0.7, 1.0),
    )?;
    Ok(out.unwrap_or_else(|| doc.clone()))
}
