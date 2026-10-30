//! Phase 12G-1's typed clipboard shims (`precision_clipboard`), plus 12G-2's
//! byte-origin payload, its RGBA8 read-back and New from Clipboard. The
//! [`DrClipboardPayload`] handle owns the payload AND its RZCB encoding,
//! encoded once when the handle is made, so the host can hand the bytes to a
//! pasteboard without asking the core to encode again.
//!
//! Since #547 phase 4c the handle holds either kind of payload
//! (`cmyk_clipboard_codec::AnyClipboardPayload`): an RGB one (RZCB v1–v3) or
//! a CMYK one (v4). Each export dispatches on the document's mode and the
//! payload's kind — a CMYK document reads and pastes through
//! `cmyk_clipboard`, an RGB payload pasted into a CMYK document is converted
//! first (`ClipboardPayload::converted_to_cmyk`) and a CMYK payload pasted
//! into an RGB document is converted into its profile
//! (`CmykClipboardPayload::converted_to_rgb`, which takes no job: the RGB
//! paste that follows begins the job control) before today's RGB paste. The
//! RGB arms are today's code unchanged.
//!
//! Since Grayscale phase 4 the handle also holds a Gray payload (v5,
//! `gray_clipboard`): Copy, Copy Merged and Cut of a Gray document wrap the
//! RGB read with the document's Gray policy, every paste into a Gray
//! document or of a Gray payload is `gray_clipboard::paste`'s, New from
//! Clipboard of a Gray payload is a Gray document of its policy, and the
//! image, editable and trim exports read through the payload's RGB pixels
//! and re-wrap.
//!
//! Since #547 Lab phase 4c the handle also holds a Lab payload (v6,
//! `lab_clipboard`), by the same pattern: Copy, Copy Merged and Cut of a
//! Lab document wrap the RGB read with the document's Lab policy, every
//! paste into a Lab document or of a Lab payload is `lab_clipboard::paste`'s
//! (asked before Gray's arm), New from Clipboard of a Lab payload is a Lab
//! document of its depth (an 8-bit payload put on the ×257 grid first,
//! phase 6a-i: `LabClipboardPayload::quantized_to`), and the image, editable
//! and trim exports read through the payload's codes and re-wrap.

use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::cmyk_clipboard_codec::AnyClipboardPayload;
use crate::dr_image::DrImage;
use crate::ffi_color::required_profile;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, free_handle, required_handle};
use crate::gray_clipboard::GrayClipboardPayload;
use crate::job_control::{DrJobControl, JobContext};
use crate::lab_clipboard::LabClipboardPayload;
use crate::lab_document::LabDepth;
use crate::metadata::Resolution;
use crate::precision_clipboard::{ClipboardPayload, ClipboardRead, PasteProfile, PasteRequest};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{PrecisionImage, PrecisionStorage};

const REQUEST_LIMIT: usize = 64 * 1024;

/// A paste request (`dr_precision_doc_clipboard_paste`'s JSON), parsed once
/// for that export and the Indexed Color paste exports in `ffi_indexed`.
///
/// # Safety
/// `request` NULL or NUL-terminated.
pub(crate) unsafe fn paste_request(request: *const c_char) -> Result<PasteRequest, String> {
    let text = unsafe { borrow_cstr_bounded(request, "request", REQUEST_LIMIT) }?;
    serde_json::from_str(text).map_err(|e| format!("invalid clipboard paste request: {e}"))
}

/// One typed clipboard payload (RGB, CMYK or Gray) with its canonical RZCB
/// bytes and, for a Gray payload, its one-channel TIFF once asked for
/// (`dr_clipboard_payload_gray_tiff`).
pub struct DrClipboardPayload {
    pub(crate) payload: AnyClipboardPayload,
    pub(crate) encoded: Vec<u8>,
    pub(crate) gray_tiff: OnceLock<Vec<u8>>,
}

impl DrClipboardPayload {
    pub(crate) fn new(payload: AnyClipboardPayload) -> Result<Self, String> {
        let encoded = payload.encode()?;
        Ok(Self {
            payload,
            encoded,
            gray_tiff: OnceLock::new(),
        })
    }

    fn rgb(payload: ClipboardPayload) -> Result<Self, String> {
        Self::new(AnyClipboardPayload::Rgb(payload))
    }

    /// A payload read from `doc`: wrapped with the document's Gray policy
    /// when it is a Gray document (#547 phase 4), with its Lab policy when
    /// it is a Lab document (#547 Lab phase 4c), else an RGB payload. A
    /// Duotone document's payload is Gray with its inks dropped (#547
    /// Duotone phase 2a), as its RZCB v5 encoding already holds it: New from
    /// Clipboard of it is a Grayscale document.
    fn read_from(doc: &PrecisionDocument, payload: ClipboardPayload) -> Result<Self, String> {
        if let Some(policy) = doc.lab_policy() {
            return Self::lab(LabClipboardPayload::new(payload, policy.clone())?);
        }
        match doc.gray_policy() {
            Some(policy) if policy.duotone().is_some() => Self::gray(GrayClipboardPayload::new(
                payload,
                std::sync::Arc::new(policy.with_duotone(None)),
            )?),
            Some(policy) => Self::gray(GrayClipboardPayload::new(payload, policy.clone())?),
            None => Self::rgb(payload),
        }
    }

    fn lab(payload: LabClipboardPayload) -> Result<Self, String> {
        Self::new(AnyClipboardPayload::Lab(payload))
    }

    fn gray(payload: GrayClipboardPayload) -> Result<Self, String> {
        Self::new(AnyClipboardPayload::Gray(payload))
    }
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum CutRequest {
    Cut {
        layer: usize,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
}

/// The optional canvas coverage: NULL with length 0 means fully covered;
/// otherwise the length must be the document's own canvas pixel count.
///
/// # Safety
/// `coverage` is NULL or readable for `length` u16 values.
pub(crate) unsafe fn canvas_coverage<'a>(
    document: &PrecisionDocument,
    coverage: *const u16,
    length: usize,
) -> Result<Option<&'a [u16]>, String> {
    if coverage.is_null() {
        if length != 0 {
            return Err("clipboard coverage is NULL with a nonzero length".into());
        }
        return Ok(None);
    }
    let (w, h) = document.dimensions();
    let canvas = crate::precision_image::pixel_count(w, h)?;
    if length != canvas {
        return Err("clipboard coverage length must match the canvas".into());
    }
    Ok(Some(unsafe {
        std::slice::from_raw_parts(coverage, canvas)
    }))
}

/// Copy / Copy Merged one canvas window into a new payload handle. Request
/// `{"op":"copy","layer","x","y","width","height"}` or
/// `{"op":"copy_merged","x","y","width","height"}`; coverage is canvas-sized
/// u16 or NULL with length 0. A group layer copies its raw projection (its
/// children composited without the group's own opacity, blend, mask, style
/// or Blend If). Free with `dr_clipboard_payload_free`.
/// # Safety
/// document/control NULL/live; request NULL/terminated UTF-8; coverage NULL or
/// readable for coverage_length u16; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_clipboard_copy(
    document: *const PrecisionDocument,
    request: *const c_char,
    coverage: *const u16,
    coverage_length: usize,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrClipboardPayload {
    unsafe {
        fallible_op(
            err_out,
            "copying high-depth pixels",
            ptr::null_mut(),
            || {
                let doc = required_handle(document)?;
                let text = borrow_cstr_bounded(request, "request", REQUEST_LIMIT)?;
                let read: ClipboardRead = serde_json::from_str(text)
                    .map_err(|e| format!("invalid clipboard copy request: {e}"))?;
                let coverage = canvas_coverage(doc, coverage, coverage_length)?;
                if doc.is_cmyk() {
                    let control = control.as_ref();
                    let _run = control.map(DrJobControl::begin).transpose()?;
                    let job = control.map_or(JobContext::NONE, JobContext::new);
                    let payload =
                        doc.cmyk_clipboard_copy(&read, coverage, limit, other_retained_bytes, job)?;
                    return DrClipboardPayload::new(AnyClipboardPayload::Cmyk(payload));
                }
                let payload = doc.clipboard_copy(
                    &read,
                    coverage,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )?;
                DrClipboardPayload::read_from(doc, payload)
            },
            boxed,
        )
    }
}

/// Cut: request `{"op":"cut","layer","x","y","width","height"}`. On success
/// `payload_out` receives the payload; a NULL document WITHOUT an error means
/// the erase changed no byte (a transparency lock). On error `payload_out`
/// is NULL.
/// # Safety
/// As `dr_precision_doc_clipboard_copy`; payload_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_clipboard_cut(
    document: *const PrecisionDocument,
    request: *const c_char,
    coverage: *const u16,
    coverage_length: usize,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    payload_out: *mut *mut DrClipboardPayload,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        if !payload_out.is_null() {
            *payload_out = ptr::null_mut();
        }
        fallible_op(
            err_out,
            "cutting high-depth pixels",
            ptr::null_mut(),
            || {
                let doc = required_handle(document)?;
                if payload_out.is_null() {
                    return Err("payload_out is NULL".into());
                }
                let text = borrow_cstr_bounded(request, "request", REQUEST_LIMIT)?;
                // #547 Indexed phase 3b: an Indexed Cut may name the
                // background colour (`indexed_paint::BACKGROUND_KEY`), and
                // since Bitmap phase 2a-ii a Bitmap Cut too.
                let (text, background) = if doc.is_indexed() || doc.is_bitmap() {
                    crate::indexed_paint::split_background_as(
                        text,
                        "invalid clipboard cut request",
                    )?
                } else {
                    (text.to_string(), None)
                };
                let CutRequest::Cut {
                    layer,
                    x,
                    y,
                    width,
                    height,
                } = serde_json::from_str(&text)
                    .map_err(|e| format!("invalid clipboard cut request: {e}"))?;
                let coverage = canvas_coverage(doc, coverage, coverage_length)?;
                // #547 Bitmap phase 2a-ii: the Gray Cut, then the erase put
                // back on the plane (`bitmap_edit::cut`).
                if doc.is_bitmap() {
                    let (payload, next) = crate::bitmap_edit::cut(
                        doc,
                        layer,
                        x,
                        y,
                        width,
                        height,
                        coverage,
                        background.as_ref(),
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    )?;
                    *payload_out = boxed(DrClipboardPayload::read_from(doc, payload)?);
                    return Ok(next);
                }
                // #547 Indexed phase 2c: the RGB Cut, then the erase re-indexed.
                if doc.is_indexed() {
                    let (payload, next) = crate::indexed_paint::cut(
                        doc,
                        layer,
                        x,
                        y,
                        width,
                        height,
                        coverage,
                        background.as_ref(),
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    )?;
                    *payload_out = boxed(DrClipboardPayload::read_from(doc, payload)?);
                    return Ok(next);
                }
                if doc.is_cmyk() {
                    let control = control.as_ref();
                    let _run = control.map(DrJobControl::begin).transpose()?;
                    let job = control.map_or(JobContext::NONE, JobContext::new);
                    let (payload, next) = doc.cmyk_clipboard_cut(
                        layer,
                        x,
                        y,
                        width,
                        height,
                        coverage,
                        limit,
                        other_retained_bytes,
                        job,
                    )?;
                    *payload_out =
                        boxed(DrClipboardPayload::new(AnyClipboardPayload::Cmyk(payload))?);
                    return Ok(next);
                }
                let (payload, next) = doc.clipboard_cut(
                    layer,
                    x,
                    y,
                    width,
                    height,
                    coverage,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )?;
                let handle = DrClipboardPayload::read_from(doc, payload)?;
                *payload_out = boxed(handle);
                Ok(next)
            },
            |next| next.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Paste one payload as a new raster record. Request
/// `{"name","anchor","position":"above"|"top"|"into_group","placement":"centered"|"in_place"|"into","frame"?:[x,y,w,h],"profile"?:"convert"|"preserve"}`.
/// A payload of another depth or profile is converted into the document's
/// depth and profile first (a 16-bit payload is promoted into a float
/// document); `profile` (default "convert") chooses whether another
/// profile's pixels keep their appearance ("convert") or their numbers
/// ("preserve", "Don't Convert"). Across colour modes (an RGB
/// payload into a CMYK document, a CMYK one into an RGB document) the
/// pixels are always converted, relative colorimetric with black-point
/// compensation through the profiles' own tables, and "preserve" refuses:
/// "Don't Convert keeps numbers within one colour mode; pixels pasted
/// between RGB and CMYK are always converted". Between CMYK documents the
/// same profile bytes keep the ink bit for bit; another profile converts
/// under "convert" and keeps the ink under "preserve". Into or out of a
/// Grayscale document the rule is `gray_clipboard::paste`'s: between Gray
/// documents the same Gray ICC keeps the codes bit for bit and another one
/// converts under "convert" and keeps the numbers under "preserve"; across
/// modes the pixels always convert (into Gray by Image ▸ Mode ▸ Grayscale's
/// luminance, a CMYK payload through ProPhoto RGB) and "preserve" refuses
/// with "Don't Convert keeps numbers within one colour mode; pixels pasted
/// between Grayscale and another mode are always converted". Into or out of
/// a Lab Color document the rule is `lab_clipboard::paste`'s: between Lab
/// documents both modes keep the codes (a 16-bit payload into an 8-bit
/// document lands on the ×257 grid); RGB → Lab is Image ▸ Mode ▸ Lab
/// Color's rule (relative colorimetric, no BPC) and Lab → RGB relative
/// colorimetric with BPC, Gray and CMYK through 16-bit ProPhoto RGB; across
/// modes "preserve" refuses with "Don't Convert keeps numbers within one
/// colour mode; pixels pasted between Lab Color and another mode are always
/// converted".
/// # Safety
/// document/payload/control NULL/live; request NULL/terminated UTF-8;
/// err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_clipboard_paste(
    document: *const PrecisionDocument,
    payload: *const DrClipboardPayload,
    request: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "pasting high-depth pixels",
            ptr::null_mut(),
            || {
                let doc = required_handle(document)?;
                let payload = required_handle(payload)?;
                let request = paste_request(request)?;
                dispatch_paste(
                    doc,
                    &payload.payload,
                    &request,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// The paste's dispatch on the document's mode and the payload's kind
/// (module doc); `lab_clipboard::paste` re-enters it with the RGB payload
/// a Lab payload converts to for a Gray or CMYK document.
pub(crate) fn dispatch_paste(
    doc: &PrecisionDocument,
    payload: &AnyClipboardPayload,
    request: &PasteRequest,
    limit: u64,
    other: u64,
    control: Option<&DrJobControl>,
) -> Result<PrecisionDocument, String> {
    match (doc.is_gray(), doc.is_cmyk(), payload) {
        // #547 Bitmap phase 3: Paste is disabled in a Bitmap document
        // (phase 2b's walk), before the Gray arm a Bitmap document would
        // otherwise reach.
        _ if doc.is_bitmap() => Err(crate::bitmap_document::refused_sentence("paste")),
        // #547 Indexed phase 2d: the RGB paste merged into the one layer and
        // re-indexed (`indexed_clipboard`).
        _ if doc.is_indexed() => {
            crate::indexed_clipboard::paste(doc, payload, request, None, limit, other, control)
        }
        // #547 Lab phase 4c: every paste into a Lab document or of a Lab
        // payload, before the Gray arm.
        _ if doc.is_lab() => {
            crate::lab_clipboard::paste(doc, payload, request, limit, other, control)
        }
        (_, _, AnyClipboardPayload::Lab(_)) => {
            crate::lab_clipboard::paste(doc, payload, request, limit, other, control)
        }
        // #547 phase 4: every paste into a Gray document or of a Gray payload.
        (true, ..) | (_, _, AnyClipboardPayload::Gray(_)) => {
            crate::gray_clipboard::paste(doc, payload, request, limit, other, control)
        }
        (false, false, AnyClipboardPayload::Rgb(payload)) => {
            doc.clipboard_paste(payload, request, limit, other, control)
        }
        (false, false, AnyClipboardPayload::Cmyk(payload)) => {
            crate::cmyk_clipboard::paste_request_refusal(request)?;
            let rgb = payload.converted_to_rgb(
                doc.profile(),
                PrecisionStorage::Rgba16Unorm,
                request.profile,
                limit,
                other,
                JobContext::NONE,
            )?;
            let held = other
                .checked_add(2 * rgb.payload_bytes())
                .ok_or("clipboard size overflow")?;
            doc.clipboard_paste(&rgb, request, limit, held, control)
        }
        (false, true, AnyClipboardPayload::Cmyk(payload)) => {
            let _run = control.map(DrJobControl::begin).transpose()?;
            let job = control.map_or(JobContext::NONE, JobContext::new);
            doc.cmyk_clipboard_paste(payload, request, limit, other, job)
        }
        (false, true, AnyClipboardPayload::Rgb(payload)) => {
            let _run = control.map(DrJobControl::begin).transpose()?;
            let job = control.map_or(JobContext::NONE, JobContext::new);
            crate::cmyk_clipboard::paste_request_refusal(request)?;
            let policy = doc.require_cmyk_policy("Paste")?.clone();
            let converted = payload.converted_to_cmyk(
                &policy,
                policy.depth(),
                request.profile,
                limit,
                other,
                job.range(0.0, 0.6),
            )?;
            let held = other
                .checked_add(2 * converted.payload_bytes())
                .ok_or("clipboard size overflow")?;
            doc.cmyk_clipboard_paste(&converted, request, limit, held, job.range(0.6, 1.0))
        }
    }
}

/// Decode an RZCB stream into a payload handle, admitted before its samples
/// are allocated. Free with `dr_clipboard_payload_free`.
/// # Safety
/// bytes NULL or readable for length bytes; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_clipboard_payload_decode(
    bytes: *const u8,
    length: usize,
    limit: u64,
    other_retained_bytes: u64,
    err_out: *mut *mut c_char,
) -> *mut DrClipboardPayload {
    unsafe {
        fallible_op(
            err_out,
            "decoding a clipboard payload",
            ptr::null_mut(),
            || {
                if bytes.is_null() {
                    return Err("clipboard bytes are NULL".into());
                }
                let input = std::slice::from_raw_parts(bytes, length);
                DrClipboardPayload::new(AnyClipboardPayload::decode(
                    input,
                    limit,
                    other_retained_bytes,
                )?)
            },
            boxed,
        )
    }
}

/// The payload's borrowed RZCB bytes, valid until the handle is freed; NULL
/// with length 0 for a NULL handle.
/// # Safety
/// payload NULL/live; length_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_clipboard_payload_bytes(
    payload: *const DrClipboardPayload,
    length_out: *mut usize,
) -> *const u8 {
    unsafe {
        let (pointer, length) = match payload.as_ref() {
            Some(p) => (p.encoded.as_ptr(), p.encoded.len()),
            None => (ptr::null(), 0),
        };
        if !length_out.is_null() {
            *length_out = length;
        }
        pointer
    }
}

/// Owned JSON description (`ClipboardPayload::info`). Free with dr_string_free.
/// # Safety
/// payload NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_clipboard_payload_info(
    payload: *const DrClipboardPayload,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "describing a clipboard payload",
            ptr::null_mut(),
            || {
                let payload = required_handle(payload)?;
                CString::new(payload.payload.info().to_string()).map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}

/// The payload's pixels as a typed image owner sharing the payload's samples
/// (a Gray payload's R = G = B pixels in its twin, a Lab payload's L\*a\*b\*
/// codes in the Lab twin); free with
/// `dr_precision_image_free`. NULL for a NULL handle, and NULL with no error
/// for a CMYK payload, whose ink is no RGB image.
/// # Safety
/// payload NULL/live.
#[no_mangle]
pub unsafe extern "C" fn dr_clipboard_payload_image(
    payload: *const DrClipboardPayload,
) -> *mut PrecisionImage {
    unsafe {
        crate::ffi_util::handle_get(payload, ptr::null_mut(), |p| match &p.payload {
            AnyClipboardPayload::Rgb(payload) => boxed(payload.image().clone()),
            AnyClipboardPayload::Gray(payload) => boxed(payload.rgb.image().clone()),
            AnyClipboardPayload::Lab(payload) => boxed(payload.rgb.image().clone()),
            AnyClipboardPayload::Cmyk(_) => ptr::null_mut(),
        })
    }
}

/// A byte-origin payload from straight RGBA8 (`ClipboardPayload::from_rgba8`):
/// u16 `v · 257`, no coverage, the exact matrix/TRC `profile` bytes, the
/// canvas offset and PPI. Free with `dr_clipboard_payload_free`.
/// # Safety
/// rgba NULL or readable for length bytes; profile NULL or readable for
/// profile_length bytes; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_clipboard_payload_from_rgba8(
    rgba: *const u8,
    length: usize,
    width: u32,
    height: u32,
    profile: *const u8,
    profile_length: usize,
    offset_x: i32,
    offset_y: i32,
    ppi_x: f32,
    ppi_y: f32,
    limit: u64,
    other_retained_bytes: u64,
    err_out: *mut *mut c_char,
) -> *mut DrClipboardPayload {
    unsafe {
        fallible_op(
            err_out,
            "making a clipboard payload from 8-bit pixels",
            ptr::null_mut(),
            || {
                if rgba.is_null() {
                    return Err("clipboard RGBA8 pixels are NULL".into());
                }
                if profile.is_null() {
                    return Err(
                        "clipboard RGBA8 input requires a bounded original ICC profile".into(),
                    );
                }
                let pixels = std::slice::from_raw_parts(rgba, length);
                let profile = std::slice::from_raw_parts(profile, profile_length);
                DrClipboardPayload::rgb(ClipboardPayload::from_rgba8(
                    pixels,
                    width,
                    height,
                    profile,
                    (offset_x, offset_y),
                    Resolution { x: ppi_x, y: ppi_y },
                    limit,
                    other_retained_bytes,
                )?)
            },
            boxed,
        )
    }
}

/// The payload's pixels as 8-bit RGBA in `profile` (NULL with length 0 =
/// the payload's own profile; parsed by `ffi_color::required_profile`, the
/// `profile_arg` rule) — Convert keeps their appearance, `preserve` keeps
/// their numbers; a float payload maps through its own tone map
/// (`ClipboardPayload::to_rgba8_in`). A CMYK payload converts into `profile`
/// (NULL = Display P3, the CMYK view's space) and refuses `preserve`
/// (`CmykClipboardPayload::to_rgba8_in`). A Gray payload reads its twin's
/// pixels as an RGB payload does, and refuses `preserve` with a `profile`
/// ("Don't Convert keeps numbers within one colour mode; pixels pasted
/// between Grayscale and another mode are always converted"). A Lab payload
/// converts into `profile` (NULL = Display P3, the Lab view's space) and
/// refuses `preserve` with a `profile` ("…between Lab Color and another
/// mode are always converted": `LabClipboardPayload::to_rgba8_in`). Free
/// with `dr_image_free`.
/// # Safety
/// payload/control NULL/live; profile NULL or readable for profile_len
/// bytes; err_out NULL/writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_clipboard_payload_rgba8(
    payload: *const DrClipboardPayload,
    profile: *const u8,
    profile_len: usize,
    preserve: bool,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "reading a clipboard payload as 8-bit pixels",
            ptr::null_mut(),
            || {
                let payload = required_handle(payload)?;
                let target = if profile.is_null() && profile_len == 0 {
                    None
                } else {
                    Some(required_profile(profile, profile_len)?)
                };
                let mode = if preserve {
                    PasteProfile::Preserve
                } else {
                    PasteProfile::Convert
                };
                let pixels = match &payload.payload {
                    AnyClipboardPayload::Rgb(payload) => payload.to_rgba8_in(
                        target.as_ref(),
                        mode,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    )?,
                    AnyClipboardPayload::Cmyk(payload) => payload.to_rgba8_in(
                        target.as_ref(),
                        mode,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    )?,
                    AnyClipboardPayload::Gray(_) if preserve && target.is_some() => {
                        return Err(crate::gray_clipboard::CROSS_MODE_PRESERVE.into());
                    }
                    AnyClipboardPayload::Gray(payload) => payload.rgb.to_rgba8_in(
                        target.as_ref(),
                        mode,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    )?,
                    AnyClipboardPayload::Lab(payload) => payload.to_rgba8_in(
                        target.as_ref(),
                        mode,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    )?,
                };
                Ok(DrImage { pixels })
            },
            boxed,
        )
    }
}

/// New from Clipboard: a one-record document at the payload's own depth,
/// profile, dimensions and PPI (`PrecisionDocument::from_clipboard`); a CMYK
/// payload makes a CMYK document of its own policy and declared depth
/// (`cmyk_from_clipboard`), a Gray payload a Gray document of its own
/// policy, a Lab payload a Lab document of its depth (an 8-bit payload's
/// codes on the ×257 grid first: `lab_clipboard`'s module doc, "Paste").
/// Free with `dr_precision_doc_free`.
/// # Safety
/// payload/control NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_from_clipboard(
    payload: *const DrClipboardPayload,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "making a document from the clipboard",
            ptr::null_mut(),
            || {
                let payload = required_handle(payload)?;
                match &payload.payload {
                    AnyClipboardPayload::Rgb(payload) => PrecisionDocument::from_clipboard(
                        payload,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    ),
                    AnyClipboardPayload::Cmyk(payload) => {
                        let control = control.as_ref();
                        let _run = control.map(DrJobControl::begin).transpose()?;
                        let job = control.map_or(JobContext::NONE, JobContext::new);
                        PrecisionDocument::cmyk_from_clipboard(
                            payload,
                            limit,
                            other_retained_bytes,
                            job,
                        )
                    }
                    AnyClipboardPayload::Gray(payload) => PrecisionDocument::from_clipboard(
                        &payload.rgb,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    )?
                    .with_gray(payload.policy.clone()),
                    AnyClipboardPayload::Lab(payload) => {
                        // An 8-bit payload may hold off-grid codes
                        // (`lab_clipboard`'s module doc): on the grid first,
                        // its bytes held while the document is made.
                        let eight = payload.policy.depth() == LabDepth::Eight;
                        let quantized =
                            payload.quantized_to(&payload.policy, limit, other_retained_bytes)?;
                        let held = if eight {
                            other_retained_bytes
                                .checked_add(quantized.rgb.payload_bytes())
                                .ok_or("clipboard size overflow")?
                        } else {
                            other_retained_bytes
                        };
                        PrecisionDocument::from_clipboard(
                            &quantized.rgb,
                            limit,
                            held,
                            control.as_ref(),
                        )?
                        .with_lab(payload.policy.clone())
                    }
                }
            },
            boxed,
        )
    }
}

/// A new payload handle carrying `section` as its editable section (RZCB
/// v3, or v4 for a CMYK payload since #547's artwork batch), replacing any
/// existing one (`ClipboardPayload::with_editable` /
/// `CmykClipboardPayload::with_editable`: the section is validated as the
/// decoder validates it, and the new handle, its encoding and the parse are
/// admitted first). Free with `dr_clipboard_payload_free`.
/// # Safety
/// payload NULL/live; section NULL or terminated UTF-8; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_clipboard_payload_with_editable(
    payload: *const DrClipboardPayload,
    section: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    err_out: *mut *mut c_char,
) -> *mut DrClipboardPayload {
    unsafe {
        fallible_op(
            err_out,
            "attaching an editable clipboard record",
            ptr::null_mut(),
            || {
                let payload = required_handle(payload)?;
                let section = borrow_cstr_bounded(
                    section,
                    "editable section",
                    crate::drdc::MAX_DRDC_BLOB_LEN as usize,
                )?;
                match &payload.payload {
                    AnyClipboardPayload::Rgb(payload) => DrClipboardPayload::rgb(
                        payload.with_editable(section, limit, other_retained_bytes)?,
                    ),
                    AnyClipboardPayload::Cmyk(payload) => {
                        DrClipboardPayload::new(AnyClipboardPayload::Cmyk(payload.with_editable(
                            section,
                            limit,
                            other_retained_bytes,
                        )?))
                    }
                    AnyClipboardPayload::Gray(payload) => {
                        DrClipboardPayload::gray(GrayClipboardPayload {
                            rgb: payload
                                .rgb
                                .with_editable(section, limit, other_retained_bytes)?,
                            policy: payload.policy.clone(),
                        })
                    }
                    AnyClipboardPayload::Lab(payload) => {
                        DrClipboardPayload::lab(LabClipboardPayload {
                            rgb: payload
                                .rgb
                                .with_editable(section, limit, other_retained_bytes)?,
                            policy: payload.policy.clone(),
                        })
                    }
                }
            },
            boxed,
        )
    }
}

/// The payload's editable section as an owned string (an RGB or a CMYK
/// payload's); NULL WITHOUT an error when it carries none. Free with
/// `dr_string_free`.
/// # Safety
/// payload NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_clipboard_payload_editable(
    payload: *const DrClipboardPayload,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "reading a clipboard payload's editable record",
            ptr::null_mut(),
            || {
                let payload = required_handle(payload)?;
                let section = match &payload.payload {
                    AnyClipboardPayload::Rgb(payload) => payload.editable(),
                    AnyClipboardPayload::Cmyk(payload) => payload.editable(),
                    AnyClipboardPayload::Gray(payload) => payload.rgb.editable(),
                    AnyClipboardPayload::Lab(payload) => payload.rgb.editable(),
                };
                section
                    .map(|s| CString::new(s).map_err(|e| e.to_string()))
                    .transpose()
            },
            |section| section.map_or(ptr::null_mut(), CString::into_raw),
        )
    }
}

/// New from Clipboard's trim (`ClipboardPayload::trim`): a new handle holding
/// exactly the payload's content box, offset moved by the box origin, every
/// other field carried. NULL WITHOUT an error when there is nothing to trim;
/// NULL + message for a NULL payload or a refusal. Free with
/// `dr_clipboard_payload_free`.
/// # Safety
/// payload NULL/live; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_clipboard_payload_trim(
    payload: *const DrClipboardPayload,
    limit: u64,
    other_retained_bytes: u64,
    err_out: *mut *mut c_char,
) -> *mut DrClipboardPayload {
    unsafe {
        fallible_op(
            err_out,
            "trimming a clipboard payload",
            ptr::null_mut(),
            || {
                let payload = required_handle(payload)?;
                match &payload.payload {
                    AnyClipboardPayload::Rgb(payload) => payload
                        .trim(limit, other_retained_bytes)?
                        .map(DrClipboardPayload::rgb)
                        .transpose(),
                    AnyClipboardPayload::Cmyk(payload) => payload
                        .trim(limit, other_retained_bytes)?
                        .map(|p| DrClipboardPayload::new(AnyClipboardPayload::Cmyk(p)))
                        .transpose(),
                    AnyClipboardPayload::Gray(payload) => payload
                        .rgb
                        .trim(limit, other_retained_bytes)?
                        .map(|rgb| {
                            DrClipboardPayload::gray(GrayClipboardPayload {
                                rgb,
                                policy: payload.policy.clone(),
                            })
                        })
                        .transpose(),
                    AnyClipboardPayload::Lab(payload) => payload
                        .rgb
                        .trim(limit, other_retained_bytes)?
                        .map(|rgb| {
                            DrClipboardPayload::lab(LabClipboardPayload {
                                rgb,
                                policy: payload.policy.clone(),
                            })
                        })
                        .transpose(),
                }
            },
            |trimmed| trimmed.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Release a payload handle; NULL is a no-op.
/// # Safety
/// payload NULL or an unfreed handle from this module.
#[no_mangle]
pub unsafe extern "C" fn dr_clipboard_payload_free(payload: *mut DrClipboardPayload) {
    unsafe { free_handle(payload) }
}
