//! C FFI for layer styles and the global light (`dr_doc_*style*`,
//! `dr_doc_*global_light*`). Same conventions as `ffi_doc`: the shared
//! helpers in `ffi_util` supply catch_unwind and NULL tolerance; the style
//! JSON is the contract documented in `style` and the header's "Layer
//! styles" section.

use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;

use crate::doc::DrDocument;
use crate::drdc::MAX_DRDC_META_LEN;
use crate::ffi_util::{boxed, doc_get, doc_op, fallible_op, read_cstr};
use crate::style::{GlobalLight, LayerStyle, Strictness};

/// Pure setter: returns a new document with layer `idx`'s style replaced by
/// a copy of `style_json` (parsed and canonicalized), or CLEARED when it is
/// NULL — and also cleared when the parsed style is an identity (no enabled
/// effect, fill opacity 1, no Blend If), so a stored style always renders
/// something. NULL with a message through `err_out` when `doc` is NULL
/// ("document is NULL", the err_out convention of `dr_doc_save_native`),
/// when the string is not UTF-8, not valid JSON, not a valid style (the
/// message names the offending key; a key the schema does not name is
/// "style: unknown layer style field <path>"), or over the 16 MiB cap; NULL with NO
/// message (a refusal, not an error) on an out-of-range `idx` or a value
/// equal to the current one, "no style" included. A Blend If on a CMYK plate
/// refuses ("Blend If on the Cyan channel needs a CMYK document") and every
/// CMYK `ink` is dropped (#1).
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`;
/// `style_json` must be NULL or a valid NUL-terminated C string; `err_out`
/// must be NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_set_layer_style(
    doc: *const DrDocument,
    idx: usize,
    style_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    let body = || -> Result<Option<DrDocument>, String> {
        if doc.is_null() {
            return Err("document is NULL".to_string());
        }
        let document = unsafe { &*doc };
        let value = if style_json.is_null() {
            None
        } else {
            let s = unsafe { read_cstr(style_json, "style") }?;
            if s.len() > MAX_DRDC_META_LEN as usize {
                return Err(format!("style too large (max {MAX_DRDC_META_LEN} bytes)"));
            }
            let json: serde_json::Value =
                serde_json::from_str(&s).map_err(|e| format!("style is not valid JSON: {e}"))?;
            let style = LayerStyle::from_value(&json, Strictness::Strict)
                .map_err(|reason| format!("style: {reason}"))?;
            // A key the schema does not name refuses by name
            // (`style_json_keys`).
            if let Some(path) = crate::style_json_keys::unknown_key(&json) {
                return Err(format!("style: unknown layer style field {path}"));
            }
            // An 8-bit document has no plates (#1): a CMYK channel refuses
            // by name in any range AS WRITTEN (an identity one included,
            // before canonicalization drops it) and every CMYK ink is
            // dropped, the RGB setters' rule.
            if let Some(reason) = json
                .get("blend_if")
                .and_then(|b| crate::precision_blend_if::requested_channel_refusal(b, false))
            {
                return Err(reason);
            }
            Some(Arc::new(style.without_inks()))
        };
        Ok(document.set_layer_style(idx, value))
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while setting layer style",
            ptr::null_mut(),
            body,
            |out| out.map_or(ptr::null_mut(), boxed),
        )
    }
}

/// Canonical JSON of layer `idx`'s style (heap, free with `dr_string_free`);
/// NULL on NULL doc, out-of-range idx, or a layer with no style.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_style(doc: *const DrDocument, idx: usize) -> *mut c_char {
    unsafe {
        doc_get(doc, ptr::null_mut(), |d| {
            let style = d.layers.get(idx)?.style.as_deref()?;
            Some(CString::new(style.to_json()).ok()?.into_raw())
        })
    }
}

/// Whether layer `idx` carries a style (a cheap badge query; false on NULL
/// doc or out-of-range idx). Because identity styles are never stored,
/// true means "renders something".
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_layer_has_style(doc: *const DrDocument, idx: usize) -> bool {
    unsafe { doc_get(doc, false, |d| Some(d.layers.get(idx)?.style.is_some())) }
}

/// Pure setter for the document's global light (degrees); NULL on NULL doc,
/// a non-finite component, or no change after sanitizing (altitude clamped
/// to [0, 90], angle normalized to [-180, 180), both quantized to four
/// decimals like every style number — so echoing a reported value back is
/// "no change").
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_set_global_light(
    doc: *const DrDocument,
    angle: f32,
    altitude: f32,
) -> *mut DrDocument {
    unsafe { doc_op(doc, |d| d.set_global_light(GlobalLight { angle, altitude })) }
}

/// The global light's angle in degrees; 0.0 on NULL doc.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_global_light_angle(doc: *const DrDocument) -> f32 {
    unsafe { doc_get(doc, 0.0, |d| Some(d.global_light.angle)) }
}

/// The global light's altitude in degrees; 0.0 on NULL doc.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_global_light_altitude(doc: *const DrDocument) -> f32 {
    unsafe { doc_get(doc, 0.0, |d| Some(d.global_light.altitude)) }
}

/// The four frozen Pattern Overlay presets as JSON, in popup order:
/// `[{"preset", "name", "width", "height", "rgba"}]` with `rgba` the
/// standard base64 of the tile's straight RGBA8 bytes (heap, free with
/// `dr_string_free`). NULL with a message through `err_out` only on an
/// internal error.
///
/// # Safety
/// `err_out` must be NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_pattern_presets_json(err_out: *mut *mut c_char) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "panic while listing pattern presets",
            ptr::null_mut(),
            || {
                CString::new(crate::style::pattern_presets_json())
                    .map_err(|_| "pattern presets JSON holds a NUL byte".to_string())
            },
            CString::into_raw,
        )
    }
}

/// A style PRESET's canonical JSON (`style_presets::preset_style`):
/// `style_json` parsed strictly, every CMYK ink and every pattern-fill
/// anchor dropped, written canonically (heap, free with `dr_string_free`).
/// NULL with a message through `err_out` when `style_json` is NULL, not
/// UTF-8, over the 16 MiB cap, not valid JSON or not a valid style (the
/// message names the offending key); never panics.
///
/// # Safety
/// `style_json` must be NULL or a valid NUL-terminated C string; `err_out`
/// must be NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_style_preset_canonical(
    style_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    let body = || -> Result<String, String> {
        let s = unsafe { read_cstr(style_json, "style") }?;
        if s.len() > MAX_DRDC_META_LEN as usize {
            return Err(format!("style too large (max {MAX_DRDC_META_LEN} bytes)"));
        }
        let style =
            crate::style_presets::preset_style(&s).map_err(|reason| format!("style: {reason}"))?;
        Ok(style.to_json())
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while canonicalizing a style preset",
            ptr::null_mut(),
            body,
            |json| CString::new(json).map_or(ptr::null_mut(), CString::into_raw),
        )
    }
}

/// The twelve built-in style presets as `[{"name", "style"}]` in
/// panel order, each style its canonical object (heap, free with
/// `dr_string_free`). NULL with a message through `err_out` only on an
/// internal error.
///
/// # Safety
/// `err_out` must be NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_style_preset_defaults_json(err_out: *mut *mut c_char) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "panic while listing the built-in style presets",
            ptr::null_mut(),
            || {
                CString::new(crate::style_presets::defaults_json())
                    .map_err(|_| "style presets JSON holds a NUL byte".to_string())
            },
            CString::into_raw,
        )
    }
}

/// Decodes the `len` bytes at `bytes` as a Photoshop style library (`.asl`,
/// `asl`) and returns `{"styles": [{"name", "style": object | null,
/// "blending": {"mode": blend name, "opacity": 0..1} | null, "dropped":
/// [sentence]}]}` (heap, free with `dr_string_free`): each style canonical
/// as a preset's, `null` when nothing Doctored models is left, `dropped`
/// naming each effect or blending option it could not keep. NULL `bytes`
/// with `len` 0 is an empty file. NULL with a message through `err_out` on a
/// NULL `bytes` with a non-zero `len`, `len` over 64 MiB (checked before the
/// bytes are read), more than 1000 styles, or a file that is not a style
/// library or is truncated; never panics.
///
/// # Safety
/// `bytes` must be NULL or readable for `len` bytes; `err_out` must be NULL
/// or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_asl_decode(
    bytes: *const u8,
    len: usize,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    let body = || -> Result<String, String> {
        if len > crate::asl::MAX_ASL_BYTES {
            return Err(crate::asl::too_large(len));
        }
        let file: &[u8] = if bytes.is_null() {
            if len != 0 {
                return Err("bytes is NULL".to_string());
            }
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(bytes, len) }
        };
        let styles = crate::asl::read(file)?;
        Ok(crate::asl::report_json(&styles))
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while decoding a Photoshop style library",
            ptr::null_mut(),
            body,
            |json| CString::new(json).map_or(ptr::null_mut(), CString::into_raw),
        )
    }
}

/// Encodes style PRESETS as a Photoshop style library (`.asl`,
/// `asl_write`): `presets_json` is `[{"name": s, "style": object,
/// "blending": {"mode", "opacity"} | null}]`; returns `{"asl": "<standard
/// base64 of the file>", "skipped": ["<name>: <reason>"]}` (heap, free with
/// `dr_string_free`), each style canonical as a preset's and every effect
/// at its own stored angle, a preset that cannot be written named in
/// `skipped`. NULL with a message through `err_out` when `presets_json` is
/// NULL, not UTF-8 or not valid JSON, holds more than 1000 presets, leaves
/// nothing to write, or the file would pass 64 MiB; never panics.
///
/// # Safety
/// `presets_json` must be NULL or a valid NUL-terminated C string; `err_out`
/// must be NULL or a valid pointer to writable `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn dr_asl_encode(
    presets_json: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    let body = || -> Result<String, String> {
        let s = unsafe { read_cstr(presets_json, "presets") }?;
        crate::asl_write::encode_json(&s)
    };
    unsafe {
        fallible_op(
            err_out,
            "panic while encoding a Photoshop style library",
            ptr::null_mut(),
            body,
            |json| CString::new(json).map_or(ptr::null_mut(), CString::into_raw),
        )
    }
}
