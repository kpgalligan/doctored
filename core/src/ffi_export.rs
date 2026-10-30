//! The export workspace at the C boundary: the two pure queries a host asks
//! before it encodes anything (output dimensions, name expansion), the two
//! encoders, and the owned [`ExportResult`] handle a host previews from and
//! then writes.

use std::ffi::{c_char, CString};
use std::ptr;

use serde_json::Value;

use crate::contact_sheet::{compose, ContactSheetRequest};
use crate::doc::DrDocument;
use crate::doc_export::{
    expand_name_pattern, output_dimensions, ExportRequest, ExportResult, NameTokens,
};
use crate::export_metadata::MetadataPolicy;
use crate::ffi_util::{borrow_cstr_bounded, boxed, fallible_op, free_handle, required_handle};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_codec::MAX_ENCODED_BYTES;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::PrecisionImage;
use crate::DrImage;

/// The largest request/tokens payload accepted across the boundary. A
/// request is a handful of scalars; anything larger is a caller bug, and
/// bounding it keeps a hostile string out of the JSON parser.
const MAX_REQUEST_BYTES: usize = 64 * 1024;

/// Every host string this file borrows is UTF-8 and bounded.
unsafe fn request_of(json: *const c_char) -> Result<ExportRequest, String> {
    ExportRequest::parse(unsafe { borrow_cstr_bounded(json, "export request", MAX_REQUEST_BYTES)? })
}

/// The contact-sheet twin of [`request_of`]: same bound, same UTF-8 rule.
unsafe fn sheet_request_of(json: *const c_char) -> Result<ContactSheetRequest, String> {
    ContactSheetRequest::parse(unsafe {
        borrow_cstr_bounded(json, "contact sheet request", MAX_REQUEST_BYTES)?
    })
}

fn owned(value: String) -> Result<CString, String> {
    CString::new(value).map_err(|e| e.to_string())
}

/// Owned JSON `{"width":…,"height":…}` for the output dimensions `request`
/// derives from a `w` x `h` source. Aspect ratio is kept; a value of 0, a
/// side past the request format's own cap (65535 for JPEG and GIF, 16384 for
/// WebP, 130048 for HEIC, none for PNG, TIFF and BMP), a scale outside (0,16]
/// and a pixel count past the document ceiling all refuse. Free with
/// dr_string_free.
///
/// # Safety
/// `request_json` is NULL or a NUL-terminated UTF-8 string of at most 64 KiB;
/// `err_out` is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_export_output_size(
    w: u32,
    h: u32,
    request_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning export dimensions",
            ptr::null_mut(),
            || {
                let request = request_of(request_json)?;
                let (width, height) = output_dimensions((w, h), &request)?;
                owned(serde_json::json!({"width": width, "height": height}).to_string())
            },
            CString::into_raw,
        )
    }
}

/// Owned expanded BASE file name (no extension, no directory) for `pattern`.
/// `tokens_json` is `{"name","width","height","format","date","preset"}` plus
/// the OPTIONAL `{"layer","index","count"}` and the OPTIONAL `"layer_export"`
/// bool; the host supplies `date` as a local YYYY-MM-DD string.
/// `{name}` is mandatory unless `layer_export`, which makes `{layer}` or
/// `{index}` mandatory instead; an unknown token, an empty
/// result, more than 200 characters, a `/`, `\`, `:` or control character, or
/// a leading dot all refuse, and so does `{layer}` or `{index}` when the
/// matching token was not supplied. `{layer}` is sanitized (`/ \ :` and
/// control characters become `-`, trimmed, empty becomes `layer`) and
/// `{index}` is zero-padded to `count`'s digit width. Free with
/// dr_string_free.
///
/// # Safety
/// `pattern` / `tokens_json` are NULL or NUL-terminated UTF-8 strings of at
/// most 64 KiB; `err_out` is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_export_expand_name(
    pattern: *const c_char,
    tokens_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "expanding an export name pattern",
            ptr::null_mut(),
            || {
                let pattern = borrow_cstr_bounded(pattern, "naming pattern", MAX_REQUEST_BYTES)?;
                let tokens = borrow_cstr_bounded(tokens_json, "name tokens", MAX_REQUEST_BYTES)?;
                let value: Value = serde_json::from_str(tokens)
                    .map_err(|e| format!("export name tokens are not valid JSON: {e}"))?;
                let object = value
                    .as_object()
                    .ok_or("export name tokens must be a JSON object")?;
                let text = |key: &str| -> Result<String, String> {
                    match object.get(key) {
                        Some(Value::String(value)) => Ok(value.clone()),
                        Some(Value::Number(value)) => Ok(value.to_string()),
                        _ => Err(format!("export name tokens require a {key}")),
                    }
                };
                let number = |key: &str| -> Result<u32, String> {
                    object
                        .get(key)
                        .and_then(Value::as_u64)
                        .and_then(|v| u32::try_from(v).ok())
                        .ok_or_else(|| format!("export name tokens require an integer {key}"))
                };
                let (name, format, date, preset) = (
                    text("name")?,
                    text("format")?,
                    text("date")?,
                    text("preset")?,
                );
                let layer = match object.get("layer") {
                    Some(Value::String(value)) => Some(value.clone()),
                    _ => None,
                };
                let optional = |key: &str| -> Option<u32> {
                    object
                        .get(key)
                        .and_then(Value::as_u64)
                        .and_then(|v| u32::try_from(v).ok())
                };
                // `count` defaults to the index itself, so a caller that
                // supplies only `index` gets an unpadded number rather than a
                // refusal about a token it never mentioned.
                let index = optional("index").map(|i| (i, optional("count").unwrap_or(i)));
                let layer_export = match object.get("layer_export") {
                    None => false,
                    Some(Value::Bool(value)) => *value,
                    Some(_) => {
                        return Err("export name tokens' layer_export must be true or false".into())
                    }
                };
                let tokens = NameTokens {
                    name: &name,
                    width: number("width")?,
                    height: number("height")?,
                    format: &format,
                    date: &date,
                    preset: &preset,
                    layer: layer.as_deref(),
                    index,
                    layer_export,
                };
                owned(expand_name_pattern(pattern, &tokens)?)
            },
            CString::into_raw,
        )
    }
}

/// Encode an 8-bit document as a deliverable and return the owned encoded
/// bytes as a DrExportResult (free with dr_export_result_free). `flat` MAY
/// BE NULL, with exactly dr_doc_save_image's semantics. `bit_depth` must be
/// 8. No file is written. `control` is optional and single-use; a cancelled
/// token returns NULL with "operation cancelled".
///
/// # Safety
/// `doc` / `flat` / `control` are NULL or live handles kept alive for the
/// call; `request_json` is NULL or a bounded NUL-terminated UTF-8 string;
/// `err_out` is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_export_encode(
    doc: *const DrDocument,
    flat: *const DrImage,
    request_json: *const c_char,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut ExportResult {
    unsafe {
        fallible_op(
            err_out,
            "encoding an export",
            ptr::null_mut(),
            || {
                let request = request_of(request_json)?;
                required_handle(doc)?.export_encoded(flat.as_ref(), &request, control.as_ref())
            },
            boxed,
        )
    }
}

/// Owned plan JSON (free with dr_string_free) for one typed export:
/// retained_bytes is the unique union of the document and, when supplied,
/// its projection; scratch_bytes covers the source, the resampled, sharpened
/// and converted planes, any byte reduction and the codec. Also width and
/// height. No pixel is allocated or rendered.
///
/// # Safety
/// `document` / `projection` are NULL or live handles; `request_json` is
/// NULL or a bounded NUL-terminated UTF-8 string; `err_out` is NULL or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_export_plan(
    document: *const PrecisionDocument,
    projection: *const PrecisionImage,
    request_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning a high-depth export",
            ptr::null_mut(),
            || {
                let request = request_of(request_json)?;
                let plan = required_handle(document)?.export_plan(projection.as_ref(), &request)?;
                owned(plan.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Encode a typed document as a deliverable at 16 or 8 bits and return the
/// owned encoded bytes as a DrExportResult (free with
/// dr_export_result_free). `projection` MAY BE NULL, in which case the
/// document is rendered here; when it is non-NULL its storage, dimensions
/// and exact ICC bytes must match the canvas. `other_retained_bytes`
/// excludes the union of BOTH inputs. No file is written.
///
/// # Safety
/// `document` / `projection` / `control` are NULL or live handles kept alive
/// for the call; `request_json` is NULL or a bounded NUL-terminated UTF-8
/// string; `err_out` is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_export_encode(
    document: *const PrecisionDocument,
    projection: *const PrecisionImage,
    request_json: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut ExportResult {
    unsafe {
        fallible_op(
            err_out,
            "encoding a high-depth export",
            ptr::null_mut(),
            || {
                let request = request_of(request_json)?;
                required_handle(document)?.export_encoded(
                    projection.as_ref(),
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

/// Borrowed encoded bytes; `length_out` (may be NULL) receives the length.
/// The pointer is valid only while the result handle is alive. NULL result
/// yields NULL and a zero length.
///
/// # Safety
/// `result` is NULL or a live handle; `length_out` is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_export_result_bytes(
    result: *const ExportResult,
    length_out: *mut usize,
) -> *const u8 {
    unsafe {
        let Some(result) = result.as_ref() else {
            if !length_out.is_null() {
                *length_out = 0;
            }
            return ptr::null();
        };
        let bytes = result.bytes();
        if !length_out.is_null() {
            *length_out = bytes.len();
        }
        bytes.as_ptr()
    }
}

/// Owned JSON describing the encode: bytes, width, height, carried (the
/// DR_CARRIES_* bits actually written), bit_depth, format and profile. Free
/// with dr_string_free.
///
/// # Safety
/// `result` is NULL or a live handle; `err_out` is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_export_result_info(
    result: *const ExportResult,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "reading an export result",
            ptr::null_mut(),
            || owned(required_handle(result)?.info_json().to_string()),
            CString::into_raw,
        )
    }
}

/// Decode the encoded bytes into a new DrImage (free with dr_image_free) —
/// the preview contract: this decodes the bytes dr_export_result_write will
/// write, never a second encode.
///
/// # Safety
/// `result` is NULL or a live handle; `err_out` is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_export_result_preview(
    result: *const ExportResult,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "decoding an export preview",
            ptr::null_mut(),
            || required_handle(result)?.decode_preview(),
            boxed,
        )
    }
}

/// Write the encoded bytes to `path`, atomically. An EXISTING path is
/// refused unless `overwrite`, leaving it byte-identical.
///
/// # Safety
/// `result` is NULL or a live handle; `path` is NULL or a NUL-terminated
/// UTF-8 string of at most 64 KiB; `err_out` is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_export_result_write(
    result: *const ExportResult,
    path: *const c_char,
    overwrite: bool,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "writing an export",
            false,
            || {
                let path = borrow_cstr_bounded(path, "path", 64 * 1024)?;
                required_handle(result)?.write(path, overwrite)
            },
            |()| true,
        )
    }
}

/// Adopt the bytes a HOST encoder wrote for a host-encoded export (today:
/// HEIC, whose result's bytes are a TIFF intermediate until this is called).
/// Returns a NEW result (free with dr_export_result_free) carrying those
/// bytes, the `bit_depth` the host actually wrote and the `carried`
/// DR_CARRIES_* bits an inspection of the WRITTEN file measured. A result
/// that is not awaiting host bytes is refused.
///
/// # Safety
/// `result` is NULL or a live handle; `bytes` is NULL or readable for `len`
/// bytes; `err_out` is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_export_result_adopt_host_bytes(
    result: *const ExportResult,
    bytes: *const u8,
    len: usize,
    bit_depth: u32,
    carried: u32,
    err_out: *mut *mut c_char,
) -> *mut ExportResult {
    unsafe {
        fallible_op(
            err_out,
            "adopting host-encoded export bytes",
            ptr::null_mut(),
            || {
                let result = required_handle(result)?;
                if bytes.is_null() || len == 0 {
                    return Err("host-encoded export bytes are required".into());
                }
                if len as u64 > MAX_ENCODED_BYTES {
                    return Err("the encoded export exceeds the bounded encoded-byte limit".into());
                }
                let owned = std::slice::from_raw_parts(bytes, len).to_vec();
                result.adopt_host_bytes(owned, bit_depth, carried)
            },
            boxed,
        )
    }
}

/// Release one encoded export result.
///
/// # Safety
/// `result` is NULL or an unfreed pointer from one of the encode exports.
#[no_mangle]
pub unsafe extern "C" fn dr_export_result_free(result: *mut ExportResult) {
    unsafe { free_handle(result) }
}

// ------------------------------------- production outputs (Phase 11B) --

/// Encode ONE top-level layer or group of an 8-bit document as a deliverable
/// and return the owned encoded bytes as a DrExportResult (free with
/// dr_export_result_free). `idx` is a stack index at depth 0; the item is
/// rendered in isolation with its root forced visible. `trimmed` delivers the
/// alpha bounding box — a fully transparent item is REFUSED there and
/// delivers a transparent canvas frame otherwise. `bit_depth` must be 8. No
/// file is written. `control` is optional and single-use.
///
/// # Safety
/// `doc` / `control` are NULL or live handles kept alive for the call;
/// `request_json` is NULL or a bounded NUL-terminated UTF-8 string; `err_out`
/// is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_export_layer_encode(
    doc: *const DrDocument,
    idx: usize,
    trimmed: bool,
    request_json: *const c_char,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut ExportResult {
    unsafe {
        fallible_op(
            err_out,
            "encoding a layer export",
            ptr::null_mut(),
            || {
                let request = request_of(request_json)?;
                required_handle(doc)?.export_layer_encoded(idx, trimmed, &request, control.as_ref())
            },
            boxed,
        )
    }
}

/// Owned plan JSON (free with dr_string_free) for one typed LAYER export:
/// the isolated document's own export plan, widened by the scratch its
/// one-root render and any trim copy spend. No pixel is allocated.
///
/// # Safety
/// `document` is NULL or a live handle; `request_json` is NULL or a bounded
/// NUL-terminated UTF-8 string; `err_out` is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_export_layer_plan(
    document: *const PrecisionDocument,
    idx: usize,
    trimmed: bool,
    request_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning a high-depth layer export",
            ptr::null_mut(),
            || {
                let request = request_of(request_json)?;
                let plan = required_handle(document)?.export_layer_plan(idx, trimmed, &request)?;
                owned(plan.to_string())
            },
            CString::into_raw,
        )
    }
}

/// Encode ONE top-level record of a typed document as a deliverable at 16 or
/// 8 bits and return the owned encoded bytes as a DrExportResult (free with
/// dr_export_result_free). `idx` is a postorder record index at depth 0.
/// `other_retained_bytes` excludes the document itself. No file is written.
///
/// # Safety
/// `document` / `control` are NULL or live handles kept alive for the call;
/// `request_json` is NULL or a bounded NUL-terminated UTF-8 string; `err_out`
/// is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_export_layer_encode(
    document: *const PrecisionDocument,
    idx: usize,
    trimmed: bool,
    request_json: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut ExportResult {
    unsafe {
        fallible_op(
            err_out,
            "encoding a high-depth layer export",
            ptr::null_mut(),
            || {
                let request = request_of(request_json)?;
                required_handle(document)?.export_layer_encoded(
                    idx,
                    trimmed,
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

/// Owned JSON `{"width","height","rows"}` for a contact sheet of `count`
/// cells under `request_json`. 0 cells, more than 256, a side past 30000 and
/// a pixel count past the document ceiling all refuse. Free with
/// dr_string_free.
///
/// # Safety
/// `request_json` is NULL or a bounded NUL-terminated UTF-8 string; `err_out`
/// is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_contact_sheet_size(
    request_json: *const c_char,
    count: usize,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning a contact sheet",
            ptr::null_mut(),
            || {
                let request = sheet_request_of(request_json)?;
                let (width, height, rows) = request.dimensions(count)?;
                owned(
                    serde_json::json!({"width": width, "height": height, "rows": rows}).to_string(),
                )
            },
            CString::into_raw,
        )
    }
}

/// Compose a contact sheet from `count` documents and return it as a new
/// DrImage (free with dr_image_free). Cells are 8-bit sRGB, scaled to fit
/// without upscaling and centred on the background colour. `captions` is
/// positional with `sources`: it MAY BE NULL, and any entry may be NULL.
/// `control` is optional and single-use.
///
/// # Safety
/// `sources` points to `count` live DrDocument pointers and `captions`, when
/// non-NULL, to `count` DrImage pointers or NULLs, all kept alive for the
/// call; `request_json` is NULL or a bounded NUL-terminated UTF-8 string;
/// `err_out` is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_contact_sheet_compose(
    request_json: *const c_char,
    sources: *const *const DrDocument,
    captions: *const *const DrImage,
    count: usize,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut DrImage {
    unsafe {
        fallible_op(
            err_out,
            "composing a contact sheet",
            ptr::null_mut(),
            || {
                let request = sheet_request_of(request_json)?;
                if count == 0 {
                    return Err("a contact sheet needs at least one cell".into());
                }
                if count > crate::contact_sheet::MAX_CELLS {
                    return Err(format!(
                        "a contact sheet holds at most {} cells",
                        crate::contact_sheet::MAX_CELLS
                    ));
                }
                if sources.is_null() {
                    return Err("a contact sheet requires its source documents".into());
                }
                let raw = std::slice::from_raw_parts(sources, count);
                let mut documents: Vec<&DrDocument> = Vec::with_capacity(count);
                for pointer in raw {
                    documents.push(
                        pointer
                            .as_ref()
                            .ok_or("a contact sheet source document is NULL")?,
                    );
                }
                let labels: Vec<Option<&DrImage>> = if captions.is_null() {
                    Vec::new()
                } else {
                    std::slice::from_raw_parts(captions, count)
                        .iter()
                        .map(|pointer| pointer.as_ref())
                        .collect()
                };
                let _run = control.as_ref().map(DrJobControl::begin).transpose()?;
                let job = control.as_ref().map_or(JobContext::NONE, JobContext::new);
                compose(&request, &documents, &labels, job)
            },
            boxed,
        )
    }
}

/// Owned canonical JSON echo of a metadata policy (free with
/// dr_string_free): every rule spelled out and the stamp as an object or
/// null. An unknown key, an unknown rule, a stamp string past 512 characters
/// or holding a control character, and a stamp with neither field all refuse.
/// This validates the policy ALONE; the stamp and density cross-field rules
/// depend on the format and are checked when the export request is parsed.
///
/// # Safety
/// `policy_json` is NULL or a bounded NUL-terminated UTF-8 string; `err_out`
/// is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_metadata_policy_validate(
    policy_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "validating a metadata policy",
            ptr::null_mut(),
            || {
                let text = borrow_cstr_bounded(policy_json, "metadata policy", MAX_REQUEST_BYTES)?;
                let value: Value = serde_json::from_str(text)
                    .map_err(|e| format!("metadata policy is not valid JSON: {e}"))?;
                owned(MetadataPolicy::parse(&value)?.to_json().to_string())
            },
            CString::into_raw,
        )
    }
}
