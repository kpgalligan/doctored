//! The C exports of the retouch brushes: Blur / Sharpen / Sponge and
//! the History Brush on the byte document, Smudge on both documents, and the
//! History Brush on the typed one; and the Art History Brush on the
//! byte document and an 8-bit CMYK one, with its plan. The 16-bit Blur / Sharpen / Sponge is the
//! `retouch_brush` op of `dr_precision_doc_edit` (`ffi_precision_edit`). The
//! typed exports run a 32-bit float document too (`float_retouch_brush`):
//! linear light, colour never clamped to 0…1.
use std::ffi::{c_char, CString};
use std::ptr;

use crate::art_history_stroke::ArtHistoryRequest;
use crate::brush_coverage::{BrushRequest, MAX_BRUSH_DABS};
use crate::doc::DrDocument;
use crate::ffi_util::{
    borrow_cstr_bounded, boxed, doc_op, fallible_doc_op, fallible_op, required_handle,
};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, BOOKKEEPING_BYTES};

/// The parse charge of a brush request and its dab list, as `dr_brush_u16`
/// charges it.
fn request_bytes(count: usize) -> u64 {
    4 * BOOKKEEPING_BYTES + count as u64 * 32
}

/// The brush request and its packed dabs, bounded before either is read.
///
/// # Safety
/// `text` NULL or NUL-terminated UTF-8; `dabs` NULL for count 0 or readable
/// and aligned for `count * 4` doubles, for the returned lifetime.
unsafe fn stroke<'a>(
    text: *const c_char,
    dabs: *const f64,
    count: usize,
) -> Result<(BrushRequest, &'a [f64]), String> {
    if count > MAX_BRUSH_DABS || (count > 0 && dabs.is_null()) {
        return Err(
            "native brush requires a valid array of at most 50000 dabs; use a shorter stroke or wider spacing"
                .into(),
        );
    }
    let text = unsafe { borrow_cstr_bounded(text, "native brush request", 4096) }?;
    let request = BrushRequest::parse(text)?;
    let dabs = if count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(dabs, count * 4) }
    };
    Ok((request, dabs))
}

/// Blurs (`kind` 0), sharpens (1), saturates (2) or desaturates (3) layer
/// `idx` where a stroke overlay covers it. `src` is the canvas-frame
/// premultiplied RGBA8 overlay `dr_doc_dodge_burn_layer` takes (`w`/`h` must
/// equal the canvas size); only its alpha is read, as stroke coverage. Each
/// covered pixel moves toward the kind's target by coverage × `strength`
/// (clamped to [0, 1]). Layer alpha is never touched and the layer does not
/// grow. `vibrance` (Sponge only) takes the Vibrance target;
/// `sample_all_layers` (Blur and Sharpen only) reads the sampled picture and
/// writes the premultiplied mix of the layer toward it, alpha included. NULL
/// with no error on NULL args, a dimension mismatch, non-finite strength,
/// kind > 3, an out-of-range or group idx, a lock that forbids a pixel edit,
/// an overlay that reaches no layer pixel, or when no byte would change; NULL
/// with a message for an option on a kind it does not apply to
/// ("vibrance applies to the sponge only", "sample_all_layers applies to
/// blur, sharpen and smudge") or a failed render of the picture.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `src` must
/// be NULL or a valid pointer to at least `w * h * 4` readable bytes;
/// `err_out` NULL or writable. Free the message with dr_string_free.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_doc_retouch_brush_layer(
    doc: *const DrDocument,
    idx: usize,
    src: *const u8,
    w: u32,
    h: u32,
    kind: u8,
    strength: f32,
    vibrance: bool,
    sample_all_layers: bool,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    if src.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        fallible_doc_op(doc, err_out, "applying a retouch brush", |d| {
            // Validate against the canvas dimensions before touching `src`,
            // so the raw read below is bounded by the canvas buffer size.
            if w != d.width || h != d.height {
                return Ok(None);
            }
            let Some(len) = (w as usize)
                .checked_mul(h as usize)
                .and_then(|v| v.checked_mul(4))
            else {
                return Ok(None);
            };
            let src = std::slice::from_raw_parts(src, len);
            d.retouch_brush_layer(idx, src, w, h, kind, strength, vibrance, sample_all_layers)
        })
    }
}

/// Paints layer `idx` back toward layer `source_idx` of `source` (an earlier
/// state of the same document) where a stroke overlay covers it: the
/// premultiplied mix of all four channels toward the source layer's pixel at
/// the same canvas position by coverage × `opacity` (clamped to [0, 1]). `src`
/// is the overlay `dr_doc_dodge_burn_layer` takes; only its alpha is read. The
/// layer does not grow; under a transparency lock each pixel keeps its alpha.
/// NULL on NULL args, a dimension mismatch (the overlay's or the source
/// document's canvas), non-finite opacity, an out-of-range or group index on
/// either side, a lock that forbids a pixel edit, or when no byte would change.
///
/// # Safety
/// `doc` and `source` must be NULL or valid pointers to live `DrDocument`s;
/// `src` must be NULL or a valid pointer to at least `w * h * 4` readable
/// bytes.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_doc_history_brush_layer(
    doc: *const DrDocument,
    idx: usize,
    src: *const u8,
    w: u32,
    h: u32,
    source: *const DrDocument,
    source_idx: usize,
    opacity: f32,
) -> *mut DrDocument {
    if src.is_null() {
        return ptr::null_mut();
    }
    let Some(source) = (unsafe { source.as_ref() }) else {
        return ptr::null_mut();
    };
    let body = |d: &DrDocument| {
        if w != d.width || h != d.height {
            return None;
        }
        let len = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
        let src = unsafe { std::slice::from_raw_parts(src, len) };
        d.history_brush_layer(idx, src, w, h, source, source_idx, opacity)
    };
    unsafe { doc_op(doc, body) }
}

/// Smudges layer `idx` along a stroke. `brush_request` and `dabs` are
/// `dr_brush_u16`'s (version 1 request JSON; `dab_count` packed x / y /
/// diameter / opacity tuples in canvas coordinates, at most 50000);
/// `strength` is clamped to [0, 1]; `selection` is canvas-sized u8 coverage
/// confining the stroke, or NULL with `selection_len` 0. Each move between
/// two dabs drags the picture in steps of at most a pixel, so the result does
/// not depend on the dab spacing. The layer keeps its extent; under a
/// transparency lock each pixel keeps its alpha. `sample_all_layers` smears
/// the sampled picture into the layer over the stroke's footprint clipped to
/// the canvas (a pixel the walk leaves alone keeps its bytes). NULL with no
/// error: NULL
/// `doc`, an out-of-range or group idx, non-finite strength, a lock that
/// forbids a pixel edit, or no byte changing. NULL with a message: a bad
/// request or dab list, a stroke past Smudge's own 256 million work units, or a selection
/// whose length is not the canvas pixel count.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`;
/// `brush_request` NULL or NUL-terminated UTF-8; `dabs` NULL for count 0 or
/// readable and aligned for `dab_count * 4` doubles; `selection` NULL or
/// readable for `selection_len` bytes; `err_out` NULL or writable. Free the
/// message with dr_string_free.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_doc_smudge_layer(
    doc: *const DrDocument,
    idx: usize,
    brush_request: *const c_char,
    dabs: *const f64,
    dab_count: usize,
    strength: f32,
    selection: *const u8,
    selection_len: usize,
    sample_all_layers: bool,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_doc_op(doc, err_out, "smudging a layer", |d| {
            let (request, dabs) = stroke(brush_request, dabs, dab_count)?;
            // The selection's length is the core's own canvas pixel count.
            let n = pixel_count(d.width, d.height)?;
            let selection = if selection.is_null() && selection_len == 0 {
                None
            } else if selection.is_null() || selection_len != n {
                return Err("smudge selection length differs from the canvas".into());
            } else {
                Some(std::slice::from_raw_parts(selection, n))
            };
            d.smudge_layer(idx, &request, dabs, strength, selection, sample_all_layers)
        })
    }
}

/// Plans `dr_precision_doc_smudge` for the same layer, request and dabs:
/// JSON with `retained_bytes`, `scratch_bytes` and `input_samples` (the
/// canvas pixel count when `selection`, else 0), admitted against `limit`;
/// `sample_all_layers` adds the sampled picture's term. Refuses, before
/// allocating, what the edit refuses. Free the JSON with dr_string_free.
///
/// # Safety
/// `document` NULL or live; `brush_request` NULL or NUL-terminated UTF-8;
/// `dabs` NULL for count 0 or readable and aligned for `dab_count * 4`
/// doubles; `err_out` NULL or writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_smudge_plan(
    document: *const PrecisionDocument,
    layer: usize,
    brush_request: *const c_char,
    dabs: *const f64,
    dab_count: usize,
    selection: bool,
    sample_all_layers: bool,
    limit: u64,
    other_retained_bytes: u64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning high-depth smudge",
            ptr::null_mut(),
            || {
                let doc = required_handle(document)?;
                // #547 Bitmap phase 3: refused for good, before the Gray storage.
                if doc.is_bitmap() {
                    return Err(crate::bitmap_document::refused_sentence("smudge"));
                }
                admit(
                    doc.retained_bytes(),
                    request_bytes(dab_count.min(MAX_BRUSH_DABS)),
                    other_retained_bytes,
                    limit,
                )?;
                let (request, dabs) = stroke(brush_request, dabs, dab_count)?;
                let plan = doc.smudge_plan(layer, &request, dabs, selection, sample_all_layers)?;
                admit(
                    doc.retained_bytes(),
                    plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX),
                    other_retained_bytes,
                    limit,
                )?;
                CString::new(plan.to_string()).map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Smudges 16-bit or 32-bit float layer `layer` along a stroke:
/// `brush_request` and `dabs` are `dr_brush_u16`'s, `strength` is in [0, 1],
/// and `selection` is canvas-sized u16 coverage or NULL with `selection_len`
/// 0. A float layer's field is carried and written back unquantized (colour
/// never clamped to 0…1, an alpha below 2^-24 transparent black).
/// `sample_all_layers` smears the sampled picture, as `dr_doc_smudge_layer`
/// does. An exact no-op is NULL with no error. `control` is an optional
/// single-use job.
///
/// # Safety
/// `document`/`control` NULL or live; `brush_request` NULL or NUL-terminated
/// UTF-8; `dabs` as the plan's; `selection` NULL or readable and aligned for
/// `selection_len` u16s; `err_out` NULL or writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_smudge(
    document: *const PrecisionDocument,
    layer: usize,
    brush_request: *const c_char,
    dabs: *const f64,
    dab_count: usize,
    strength: f64,
    selection: *const u16,
    selection_len: usize,
    sample_all_layers: bool,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "smudging a high-depth layer",
            ptr::null_mut(),
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let doc = required_handle(document)?;
                if doc.is_bitmap() {
                    return Err(crate::bitmap_document::refused_sentence("smudge"));
                }
                let parse = request_bytes(dab_count.min(MAX_BRUSH_DABS));
                admit(doc.retained_bytes(), parse, other_retained_bytes, limit)?;
                job.reserve(parse)?;
                let (request, dabs) = stroke(brush_request, dabs, dab_count)?;
                // The selection's length is the core's own canvas pixel count.
                let (width, height) = doc.dimensions();
                let n = pixel_count(width, height)?;
                let selection: &[u16] = if selection.is_null() && selection_len == 0 {
                    &[]
                } else if selection.is_null() || selection_len != n {
                    return Err("smudge selection length differs from the canvas".into());
                } else {
                    std::slice::from_raw_parts(selection, n)
                };
                doc.smudge(
                    layer,
                    &request,
                    dabs,
                    strength,
                    selection,
                    sample_all_layers,
                    limit,
                    other_retained_bytes,
                    job,
                )
            },
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Plans `dr_precision_doc_history_brush` for layer `layer` painting from
/// layer `source_layer` of `source`: JSON with `retained_bytes`,
/// `scratch_bytes` and `input_samples` (the canvas pixel count, doubled when
/// `selection`), admitted against `limit`. A float `document` takes a float
/// `source` ("history brush source has a different bit depth" otherwise), and
/// a Grayscale `document` a Grayscale `source` (#547 phase 2c,
/// `gray_filter::HISTORY_SOURCE_REFUSAL`), and a Lab Color `document` a Lab
/// Color `source` while a Lab `source` paints only a Lab `document` (#547
/// phase 2e, `lab_retouch::history_source_refusal`). Free the JSON with
/// dr_string_free.
///
/// # Safety
/// `document` and `source` NULL or live; `err_out` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_history_brush_plan(
    document: *const PrecisionDocument,
    layer: usize,
    source: *const PrecisionDocument,
    source_layer: usize,
    selection: bool,
    limit: u64,
    other_retained_bytes: u64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning high-depth history brush",
            ptr::null_mut(),
            || {
                let doc = required_handle(document)?;
                let source = required_handle(source)?;
                if doc.is_gray() && !source.is_gray() {
                    return Err(crate::gray_filter::HISTORY_SOURCE_REFUSAL.into());
                }
                if let Some(reason) = crate::lab_retouch::history_source_refusal(doc, source) {
                    return Err(reason.into());
                }
                let plan = doc.history_brush_plan(layer, source, source_layer, selection)?;
                admit(
                    doc.retained_bytes(),
                    plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX),
                    other_retained_bytes,
                    limit,
                )?;
                CString::new(plan.to_string()).map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Paints 16-bit or 32-bit float layer `layer` back toward layer
/// `source_layer` of `source` (an earlier state of the same document, at the
/// same depth) by coverage × `opacity` (in [0, 1]). `samples` is the
/// canvas-sized u16 stroke coverage, followed by the canvas-sized selection
/// plane when the plan was made with `selection` (`length` n or 2n,
/// recomputed from the document's canvas). In a float document coverage ×
/// opacity of 1 writes the source sample's bits. A Grayscale `document`
/// refuses a `source` that is not Grayscale, and a Lab Color `document` or
/// `source` the other's mode, as the plan does. An exact no-op
/// is NULL with no error. `control` is an optional single-use job.
///
/// # Safety
/// `document`/`source`/`control` NULL or live; `samples` NULL or readable and
/// aligned for `length` u16s; `err_out` NULL or writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_history_brush(
    document: *const PrecisionDocument,
    layer: usize,
    source: *const PrecisionDocument,
    source_layer: usize,
    opacity: f64,
    samples: *const u16,
    length: usize,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "applying high-depth history brush",
            ptr::null_mut(),
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let doc = required_handle(document)?;
                let source = required_handle(source)?;
                if doc.is_gray() && !source.is_gray() {
                    return Err(crate::gray_filter::HISTORY_SOURCE_REFUSAL.into());
                }
                if let Some(reason) = crate::lab_retouch::history_source_refusal(doc, source) {
                    return Err(reason.into());
                }
                // The sample length is one or two of the core's own canvas
                // planes, never the caller's word alone.
                let (width, height) = doc.dimensions();
                let n = pixel_count(width, height)?;
                if samples.is_null() || (length != n && length != 2 * n) {
                    return Err(
                        "high-depth input sample length does not match the core plan".into(),
                    );
                }
                let samples = std::slice::from_raw_parts(samples, length);
                doc.history_brush(
                    layer,
                    source,
                    source_layer,
                    opacity,
                    samples,
                    limit,
                    other_retained_bytes,
                    job,
                )
            },
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// The Art History Brush's request, bounded before it is read.
///
/// # Safety
/// `text` NULL or NUL-terminated UTF-8.
unsafe fn art_request(text: *const c_char) -> Result<ArtHistoryRequest, String> {
    let text = unsafe { borrow_cstr_bounded(text, "art history brush request", 4096) }?;
    ArtHistoryRequest::parse(text)
}

/// Paints layer `idx` with the Art History Brush from layer
/// `source_idx` of `source` (an earlier state of the same document).
/// `request` is the tool's options, JSON `{"version":1,"style":S,"area":
/// 0..500,"tolerance":0..100,"opacity":0..1,"blend_mode":M,"seed":
/// 0..4294967295}` (unknown fields refused; S one of tight_short,
/// tight_medium, tight_long, loose_medium, loose_long, dab, tight_curl,
/// tight_curl_long, loose_curl, loose_curl_long; M Normal 0, Darken 6,
/// Lighten 7, Hue 23, Saturation 24, Color 25 or Luminosity 26).
/// `brush_request` and `dabs` are `dr_brush_u16`'s (`dab_count` packed x / y
/// / diameter / opacity tuples in canvas coordinates, at most 50000); each
/// click among the dabs lays the style's seeded strokes, coloured from the
/// source at their starts. `selection` is canvas-sized u8 coverage or NULL
/// with `selection_len` 0. The layer keeps its extent; under a transparency
/// lock each pixel keeps its alpha. NULL with no error: NULL `doc` or
/// `source`, an out-of-range or group index on either side, a source of
/// another canvas size, a lock that forbids a pixel edit, or no byte
/// changing. NULL with a message: a bad request or dab list, the work past 2
/// billion units, or a selection whose length is not the canvas pixel count.
///
/// # Safety
/// `doc` and `source` NULL or live `DrDocument`s; `request` and
/// `brush_request` NULL or NUL-terminated UTF-8; `dabs` NULL for count 0 or
/// readable and aligned for `dab_count * 4` doubles; `selection` NULL or
/// readable for `selection_len` bytes; `err_out` NULL or writable. Free the
/// message with dr_string_free.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_doc_art_history_brush_layer(
    doc: *const DrDocument,
    idx: usize,
    source: *const DrDocument,
    source_idx: usize,
    request: *const c_char,
    brush_request: *const c_char,
    dabs: *const f64,
    dab_count: usize,
    selection: *const u8,
    selection_len: usize,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    let Some(source) = (unsafe { source.as_ref() }) else {
        return ptr::null_mut();
    };
    unsafe {
        fallible_doc_op(doc, err_out, "applying the art history brush", |d| {
            let art = art_request(request)?;
            let (brush, dabs) = stroke(brush_request, dabs, dab_count)?;
            // The selection's length is the core's own canvas pixel count.
            let n = pixel_count(d.width, d.height)?;
            let selection = if selection.is_null() && selection_len == 0 {
                None
            } else if selection.is_null() || selection_len != n {
                return Err("art history brush selection length differs from the canvas".into());
            } else {
                Some(std::slice::from_raw_parts(selection, n))
            };
            d.art_history_brush_layer(idx, source, source_idx, &art, &brush, dabs, selection)
        })
    }
}

/// Plans `dr_precision_doc_art_history_brush` for the same layer, source,
/// requests and dabs: JSON with `retained_bytes`, `scratch_bytes` and
/// `input_samples` (the canvas pixel count when `selection`, else 0),
/// admitted against `limit`. The tool runs on 8-bit documents only: a
/// 16-bit RGB, 32-bit float or 16-bit CMYK document refuses by
/// name ("The Art History Brush is unavailable in a 16-bit document.",
/// "… in a 32-bit document …", "… in a 16-bit CMYK document
/// …"); an 8-bit CMYK document paints only a CMYK raster `layer` ("The Art
/// History Brush paints only a CMYK raster layer" otherwise, judged before
/// the source) and takes a CMYK `source` ("The Art History Brush's source
/// state is not a CMYK raster" otherwise). An 8-bit Grayscale document
/// (#547 phase 2e) paints only a u16 raster `layer` ("The Art History Brush
/// paints only a Grayscale raster layer", before the source) from a Gray
/// `source` ("The Art History Brush's source state is not a Grayscale
/// raster"), its `blend_mode` Normal, Darken or Lighten; a 16-bit one refuses
/// as a 16-bit document. An 8-bit Lab Color document (#547 Lab 2f, third
/// part) paints per channel on its L\*a\*b\* codes, its strokes along L\*,
/// only a u16 raster `layer` ("The Art History Brush paints only a Lab Color
/// raster layer", before the source) from a Lab `source` ("The Art History
/// Brush's source state is not a Lab Color raster"), its `blend_mode` in
/// `lab_document::blend_refusal`'s list; a 16-bit one refuses as a 16-bit
/// document. Free the JSON with dr_string_free.
///
/// # Safety
/// `document` and `source` NULL or live; `request` and `brush_request` NULL
/// or NUL-terminated UTF-8; `dabs` NULL for count 0 or readable and aligned
/// for `dab_count * 4` doubles; `err_out` NULL or writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_art_history_brush_plan(
    document: *const PrecisionDocument,
    layer: usize,
    source: *const PrecisionDocument,
    source_layer: usize,
    request: *const c_char,
    brush_request: *const c_char,
    dabs: *const f64,
    dab_count: usize,
    selection: bool,
    limit: u64,
    other_retained_bytes: u64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning the art history brush",
            ptr::null_mut(),
            || {
                let doc = required_handle(document)?;
                // #547 Bitmap phase 3: refused for good, before the Gray storage.
                if doc.is_bitmap() {
                    return Err(crate::bitmap_document::refused_sentence(
                        "art_history_brush",
                    ));
                }
                let source = required_handle(source)?;
                admit(
                    doc.retained_bytes(),
                    request_bytes(dab_count.min(MAX_BRUSH_DABS)),
                    other_retained_bytes,
                    limit,
                )?;
                let art = art_request(request)?;
                let (brush, dabs) = stroke(brush_request, dabs, dab_count)?;
                let plan = doc.art_history_brush_plan(
                    layer,
                    source,
                    source_layer,
                    &art,
                    &brush,
                    dabs,
                    selection,
                )?;
                admit(
                    doc.retained_bytes(),
                    plan["scratch_bytes"].as_u64().unwrap_or(u64::MAX),
                    other_retained_bytes,
                    limit,
                )?;
                CString::new(plan.to_string()).map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Paints layer `layer` of an 8-bit CMYK document with the Art History
/// Brush from layer `source_layer` of `source`, as
/// `dr_doc_art_history_brush_layer` paints a byte layer, in ink (or, on an
/// 8-bit Grayscale document, on its R = G = B raster, #547 phase 2e; on an
/// 8-bit Lab Color document, per channel on its codes, #547 Lab 2f):
/// `selection` is the canvas-sized u16 selection plane or NULL with
/// `selection_len` 0. Refuses what the plan refuses, by depth first. An
/// exact no-op is NULL with no error. `control` is an optional single-use
/// job.
///
/// # Safety
/// `document`/`source`/`control` NULL or live; `request` and `brush_request`
/// NULL or NUL-terminated UTF-8; `dabs` as the plan's; `selection` NULL or
/// readable and aligned for `selection_len` u16s; `err_out` NULL or
/// writable.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_art_history_brush(
    document: *const PrecisionDocument,
    layer: usize,
    source: *const PrecisionDocument,
    source_layer: usize,
    request: *const c_char,
    brush_request: *const c_char,
    dabs: *const f64,
    dab_count: usize,
    selection: *const u16,
    selection_len: usize,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "applying the art history brush",
            ptr::null_mut(),
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let doc = required_handle(document)?;
                if doc.is_bitmap() {
                    return Err(crate::bitmap_document::refused_sentence(
                        "art_history_brush",
                    ));
                }
                let source = required_handle(source)?;
                let parse = request_bytes(dab_count.min(MAX_BRUSH_DABS));
                admit(doc.retained_bytes(), parse, other_retained_bytes, limit)?;
                job.reserve(parse)?;
                let art = art_request(request)?;
                let (brush, dabs) = stroke(brush_request, dabs, dab_count)?;
                // The selection's length is the core's own canvas pixel count.
                let (width, height) = doc.dimensions();
                let n = pixel_count(width, height)?;
                let selection: &[u16] = if selection.is_null() && selection_len == 0 {
                    &[]
                } else if selection.is_null() || selection_len != n {
                    return Err("art history brush selection length differs from the canvas".into());
                } else {
                    std::slice::from_raw_parts(selection, n)
                };
                doc.art_history_brush(
                    layer,
                    source,
                    source_layer,
                    &art,
                    &brush,
                    dabs,
                    selection,
                    limit,
                    other_retained_bytes,
                    job,
                )
            },
            |d| d.map_or(ptr::null_mut(), boxed),
        )
    }
}
