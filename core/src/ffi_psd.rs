//! Layered-file compatibility preflight, explicit import mode and export at the
//! C boundary: PSD, PSB and a TIFF carrying Photoshop's layer block.

use std::ffi::{c_char, c_int, CString};
use std::ptr;

use crate::doc::DrDocument;
use crate::dr_image::DrImage;
use crate::ffi_util::{boxed, fallible_op, read_cstr, required_handle};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::PrecisionImage;
use crate::psd_framing::{LayeredContainer, UNKNOWN_CONTAINER};

/// PSD, PSB or layered-TIFF compatibility report as JSON, freed with
/// dr_string_free; its "format" names which. Header and existing-parser
/// observations are separated from general importer limits. Its
/// "import_precision" is "rgba8", "rgba16", "cmyk", "rgba32" for a 32-bit
/// RGB file, which opens as a 32-bit float document, "gray" for an
/// 8-, 16- or 32-bit Grayscale file, which opens as a Grayscale document
/// through `dr_precision_doc_open_gray_psd` (#547 phases 3a and 3c), or
/// "lab" for an 8- or 16-bit Lab file, which opens as a Lab Color document
/// through `dr_precision_doc_open_lab_psd` (#547 Lab phase 3a).
///
/// # Safety
/// path must be NULL or a NUL-terminated string; err_out NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_psd_import_report(
    path: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "inspecting PSD compatibility",
            ptr::null_mut(),
            || {
                let path = read_cstr(path, "path")?;
                let bytes =
                    std::fs::read(&path).map_err(|e| format!("failed to read {path}: {e}"))?;
                let report = crate::psd_import::report(&bytes, &path)?;
                CString::new(report.to_string())
                    .map_err(|e| format!("cannot report PSD compatibility: {e}"))
            },
            CString::into_raw,
        )
    }
}

/// Imports the layers of a PSD, PSB or layered TIFF (mode 0) or its saved
/// composite (mode 1; a TIFF has none here and refuses). Never falls back
/// between modes or converts unsupported color/depth implicitly.
///
/// # Safety
/// path and expected_fingerprint must be NULL or NUL-terminated strings;
/// err_out must be NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_open_psd(
    path: *const c_char,
    mode: c_int,
    expected_fingerprint: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut DrDocument {
    unsafe {
        fallible_op(
            err_out,
            "opening PSD",
            ptr::null_mut(),
            || {
                let path = read_cstr(path, "path")?;
                let bytes =
                    std::fs::read(&path).map_err(|e| format!("failed to read {path}: {e}"))?;
                let expected = read_cstr(expected_fingerprint, "expected_fingerprint")?;
                if expected != crate::psd_import::fingerprint(&bytes) {
                    return Err(format!(
                        "PSD {path}: file changed since compatibility preflight; inspect it again"
                    ));
                }
                crate::psd_import::open(&bytes, &path, mode)
            },
            boxed,
        )
    }
}

/// Layered export preflight for one container (DrLayeredContainer) and mode,
/// as JSON freed with dr_string_free. Reads no pixels: the answer depends only
/// on the layer model, so a host can build it while a save panel is open.
///
/// # Safety
/// err_out must be NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_psd_export_report(
    doc: *const DrDocument,
    container: c_int,
    mode: c_int,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "inspecting PSD export",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let container = LayeredContainer::from_c(container).ok_or(UNKNOWN_CONTAINER)?;
                let mode = crate::psd_export::PsdExportMode::from_c(mode)
                    .ok_or(crate::psd_export::UNKNOWN_MODE)?;
                let report = document
                    .psd_export_report(container, mode)
                    .json(container, mode, 8);
                CString::new(report.to_string())
                    .map_err(|e| format!("cannot report PSD export: {e}"))
            },
            CString::into_raw,
        )
    }
}

/// Writes an 8-bit RGB PSD, PSB or layered TIFF (`container`, an
/// DrLayeredContainer). `flat` may be NULL, in which case this
/// document is composited here; a non-NULL image must be canvas-sized. The
/// preflight runs again and its refusal is this call's error, so a refused
/// export leaves an existing destination byte-identical.
///
/// # Safety
/// path must be NULL or a NUL-terminated string; err_out NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_save_psd(
    doc: *const DrDocument,
    flat: *const DrImage,
    path: *const c_char,
    container: c_int,
    mode: c_int,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "saving PSD",
            false,
            || {
                let document = required_handle(doc)?;
                let path = read_cstr(path, "path")?;
                let container = LayeredContainer::from_c(container).ok_or(UNKNOWN_CONTAINER)?;
                let mode = crate::psd_export::PsdExportMode::from_c(mode)
                    .ok_or(crate::psd_export::UNKNOWN_MODE)?;
                document.save_psd(flat.as_ref(), &path, container, mode)
            },
            |()| true,
        )
    }
}

/// Opens a 16-bit RGB PSD, PSB or layered TIFF as a typed document: mode 0
/// imports the supported layer subset, mode 1 the saved composite (a TIFF has
/// none here and refuses). Never falls back between modes,
/// converts depth implicitly, or accepts an 8-bit RGB file. A CMYK file
/// (colour mode 4) of any of the three containers, at 8 or 16 bits, opens as
/// a CMYK document: mode 0 its layers, mode 1 its saved composite (#547). A
/// 32-bit RGB PSD or PSB opens as a 32-bit float document the same two ways,
/// and so does a 32-bit float RGB layered TIFF, its layers only, in
/// either byte order: a little-endian one through its big-endian form
/// (`tiff_layers_order::big_endian_form`), whose fingerprint is the
/// file's own. A Grayscale file is refused by name: it opens through
/// `dr_precision_doc_open_gray_psd` (#547 phase 3a); so is a Lab file, which
/// opens through `dr_precision_doc_open_lab_psd` (#547 Lab phase 3a).
///
/// # Safety
/// path and expected_fingerprint must be NULL or NUL-terminated strings;
/// control must be NULL or a live single-use token; err_out NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_open_psd(
    path: *const c_char,
    mode: c_int,
    expected_fingerprint: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "opening 16-bit PSD",
            ptr::null_mut(),
            || {
                let path = read_cstr(path, "path")?;
                let bytes =
                    std::fs::read(&path).map_err(|e| format!("failed to read {path}: {e}"))?;
                let expected = read_cstr(expected_fingerprint, "expected_fingerprint")?;
                if expected != crate::psd_import::fingerprint(&bytes) {
                    return Err(format!(
                        "PSD {path}: file changed since compatibility preflight; inspect it again"
                    ));
                }
                // A little-endian 32-bit layered TIFF opens through its
                // big-endian form, after the fingerprint of its own bytes. The
                // original is dropped there, so the open holds only the bytes
                // the admission plans count as `source_bytes`.
                let bytes = match crate::tiff_layers_order::big_endian_form(&bytes)
                    .map_err(|e| format!("PSD {path}: {e}"))?
                {
                    Some(converted) => {
                        drop(bytes);
                        converted
                    }
                    None => bytes,
                };
                // A CMYK file (colour mode 4) — PSD, PSB or a layered CMYK
                // TIFF — opens as a CMYK document (#547: its layers, mode 0,
                // `psd_import_cmyk`; its saved composite, mode 1, `cmyk_io`),
                // before the RGB importer's own admission.
                if crate::psd_container::Container::read(&bytes)
                    .is_ok_and(|container| container.color == 4)
                {
                    return PrecisionDocument::open_cmyk_psd(
                        &bytes,
                        &path,
                        mode,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    );
                }
                // A 32-bit RGB file opens as a float document.
                if crate::psd_container::Container::read(&bytes)
                    .is_ok_and(|container| container.color == 3 && container.depth == 32)
                {
                    return crate::psd_import_float::open32(
                        &bytes,
                        &path,
                        mode,
                        limit,
                        other_retained_bytes,
                        control.as_ref(),
                    );
                }
                crate::psd_import16::open16(
                    &bytes,
                    &path,
                    mode,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Opens a Grayscale (colour mode 1) PSD, PSB or layered TIFF at 8 or 16
/// bits, or a PSD or PSB at 32 (#547 phase 3c), as a Gray document (#547
/// phase 3a): mode 0 its layers, 1 its saved composite (a TIFF has none).
/// `working_gray` is the working Gray's wire name (`"dot_gain_20"`, …),
/// assigned to an untagged file under an import warning. Never falls back between modes; any other file refuses.
///
/// # Safety
/// path, expected_fingerprint and working_gray must be NULL or
/// NUL-terminated strings; control must be NULL or a live single-use token;
/// err_out NULL or writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_precision_doc_open_gray_psd(
    path: *const c_char,
    mode: c_int,
    expected_fingerprint: *const c_char,
    working_gray: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "opening Grayscale PSD",
            ptr::null_mut(),
            || {
                let path = read_cstr(path, "path")?;
                let working = read_cstr(working_gray, "working_gray")?;
                let bytes =
                    std::fs::read(&path).map_err(|e| format!("failed to read {path}: {e}"))?;
                let expected = read_cstr(expected_fingerprint, "expected_fingerprint")?;
                if expected != crate::psd_import::fingerprint(&bytes) {
                    return Err(format!(
                        "PSD {path}: file changed since compatibility preflight; inspect it again"
                    ));
                }
                PrecisionDocument::open_gray_psd(
                    &bytes,
                    &path,
                    mode,
                    &working,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Opens a Lab (colour mode 9) PSD, PSB or layered TIFF at 8 or 16 bits as
/// a Lab Color document (#547 Lab phase 3a): mode 0 its layers, 1 its saved
/// composite (a TIFF has none). Never falls back between modes; any other
/// file refuses.
///
/// # Safety
/// path and expected_fingerprint must be NULL or NUL-terminated strings;
/// control must be NULL or a live single-use token; err_out NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_open_lab_psd(
    path: *const c_char,
    mode: c_int,
    expected_fingerprint: *const c_char,
    limit: u64,
    other_retained_bytes: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> *mut PrecisionDocument {
    unsafe {
        fallible_op(
            err_out,
            "opening Lab Color PSD",
            ptr::null_mut(),
            || {
                let path = read_cstr(path, "path")?;
                let bytes =
                    std::fs::read(&path).map_err(|e| format!("failed to read {path}: {e}"))?;
                let expected = read_cstr(expected_fingerprint, "expected_fingerprint")?;
                if expected != crate::psd_import::fingerprint(&bytes) {
                    return Err(format!(
                        "PSD {path}: file changed since compatibility preflight; inspect it again"
                    ));
                }
                PrecisionDocument::open_lab_psd(
                    &bytes,
                    &path,
                    mode,
                    limit,
                    other_retained_bytes,
                    control.as_ref(),
                )
            },
            boxed,
        )
    }
}

/// Layered export preflight for one container (DrLayeredContainer) and mode
/// of a typed document, as JSON freed with dr_string_free: depth 16, or 32 for
/// a float document, which writes a PSD, PSB or big-endian layered
/// TIFF at 32 bits. A Grayscale document answers its declared depth (8,
/// 16 or 32) with `"color_mode":"gray"` (#547 phase 3b); at 32 it is the
/// float report, a layered TIFF refused (#547 phase 3c). A Lab Color
/// document answers its declared depth (8 or 16) with `"color_mode":"lab"`
/// (#547 Lab phase 3b). Reads no pixels.
///
/// # Safety
/// err_out must be NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_psd_export_report(
    doc: *const PrecisionDocument,
    container: c_int,
    mode: c_int,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "inspecting typed PSD export",
            ptr::null_mut(),
            || {
                let document = required_handle(doc)?;
                let container = LayeredContainer::from_c(container).ok_or(UNKNOWN_CONTAINER)?;
                let mode = crate::psd_export::PsdExportMode::from_c(mode)
                    .ok_or(crate::psd_export::UNKNOWN_MODE)?;
                let report = document.psd_report_json(
                    container,
                    mode,
                    container.max_file_bytes(),
                    crate::psd_layer_pixels::import_envelope_bytes(),
                );
                CString::new(report.to_string())
                    .map_err(|e| format!("cannot report PSD export: {e}"))
            },
            CString::into_raw,
        )
    }
}

/// Writes a 16-bit RGB PSD, PSB or layered TIFF (`container`, an
/// DrLayeredContainer). `projection` is REQUIRED for an RGB document and must
/// be this document's canvas-sized RGBA16 UNORM rendering in its exact ICC
/// bytes; a CMYK document takes NULL and writes a CMYK file at its declared
/// depth, its composite rendered here (#547). A float document writes a
/// 32-bit RGB PSD or PSB or big-endian layered TIFF from its
/// canvas-sized RGBA32 float projection. A Grayscale document writes a
/// colour-mode-1 PSD, PSB or layered TIFF at its declared 8 or 16 bits from
/// its REQUIRED canvas-sized RGBA16 projection (#547 phase 3b); a 32-bit one
/// writes a 32-bit colour-mode-1 PSD or PSB from its REQUIRED RGBA32 float
/// projection, and its layered TIFF is refused (#547 phase 3c). A Lab Color
/// document writes a colour-mode-9 PSD, PSB or layered TIFF at its declared
/// 8 or 16 bits, no profile embedded, from its REQUIRED canvas-sized RGBA16
/// projection in its Lab twin (#547 Lab phase 3b). The preflight runs again
/// and its refusal is this call's error, so a refused export leaves an
/// existing destination byte-identical.
///
/// # Safety
/// path must be NULL or a NUL-terminated string; err_out NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_save_psd(
    doc: *const PrecisionDocument,
    projection: *const PrecisionImage,
    path: *const c_char,
    container: c_int,
    mode: c_int,
    limit: u64,
    other_retained_bytes: u64,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "saving typed PSD",
            false,
            || {
                let document = required_handle(doc)?;
                let projection = projection.as_ref();
                let path = read_cstr(path, "path")?;
                let container = LayeredContainer::from_c(container).ok_or(UNKNOWN_CONTAINER)?;
                let mode = crate::psd_export::PsdExportMode::from_c(mode)
                    .ok_or(crate::psd_export::UNKNOWN_MODE)?;
                document.save_psd(
                    projection,
                    &path,
                    container,
                    mode,
                    limit,
                    other_retained_bytes,
                )
            },
            |()| true,
        )
    }
}

/// True when the file at `path` is a TIFF whose Photoshop layer block this
/// core reads as layers: a classic big-endian unsigned RGB, CMYK (InkSet
/// absent or 1), BlackIsZero gray (#547 phase 3a) or Lab TIFF at 8 or 16
/// bits holding a `Layr`/`Lr16` block, or a 32-bit IEEE-float RGB
/// TIFF of either byte order holding an `Lr32` block. False for NULL, an
/// unreadable path, a little-endian file at 8 or 16 bits, any other colour
/// model or sample format, and a TIFF with no readable block — each of which
/// opens flat. Reads the header, the first directory and the block keys,
/// never a pixel strip.
///
/// # Safety
/// path must be NULL or a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn dr_tiff_has_layers(path: *const c_char) -> bool {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "inspecting TIFF layers",
            false,
            || {
                let path = read_cstr(path, "path")?;
                Ok(crate::tiff_layers::has_layers(&path))
            },
            |found| found,
        )
    }
}
