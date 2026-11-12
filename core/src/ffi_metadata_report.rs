//! Read-only metadata import diagnostics, generated from the same bounded
//! TIFF walk used by the document opener. Reports are transient file facts;
//! they add no document field and make no blanket preservation guarantee.
//!
//! `ffi_metadata_report.rs` reports scan limitations without decoding pixels.

use crate::ffi_color::blob;
use crate::ffi_util::{fallible_op, read_cstr};
use crate::open_profile::{platform_conversion_sentence, PlatformSource};
use crate::working_cmyk::set_working_cmyk_profile;
use std::ffi::{c_char, c_int, CString};
use std::io::Read;
use std::ptr;

/// The bytes [`dr_path_metadata_import_report`] reads: the whole file, except
/// a native `.rz`, whose report depends on its four signature bytes alone.
/// Reading only those keeps a typed native file, up to 8 GiB, from
/// being buffered outside every plan (Contact Sheet asks for the report of
/// each file it has just opened). The cut rests on three callees returning
/// nothing for a native body: `metadata::scan_report` (no packet, no
/// resolution, no warning a native report keeps), `metadata_tiff::orientation_of_file`
/// and `dr_image::embedded_profile_report`. A change that makes any of them
/// read a native body must read the whole file here again.
fn report_bytes(path: &str) -> Result<Vec<u8>, String> {
    let failed = |e: std::io::Error| format!("failed to read {path}: {e}");
    let mut file = std::fs::File::open(path).map_err(failed)?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(4)
        .read_to_end(&mut bytes)
        .map_err(failed)?;
    if !crate::native_magic::is_native_magic(&bytes) {
        file.read_to_end(&mut bytes).map_err(failed)?;
    }
    Ok(bytes)
}

/// JSON object with container, metadata_walked, supported_packets,
/// limitations, warnings, and optional upright resolution {x,y} in PPI.
/// The density query also supports TIFF-based RAW without decoding pixels.
/// `profile_refusal`, present only for a flat file whose embedded colour
/// profile the open converts from or ignores (`open_profile`), is that
/// sentence, which is also the last entry of `warnings`. `tiff_pages`,
/// present only for a TIFF of more than one page, is its page count
/// (`tiff_pages`); `warnings` carries the byte open's sentences about a
/// TIFF's pages, and a turned JPEG's about its path records
/// (`tiff_paths::jpeg_report_line`).
/// Free with dr_string_free. NULL on read/input
/// error, with err_out explaining the failure. Never decodes pixels.
///
/// # Safety
/// path must be NULL or a valid NUL-terminated UTF-8 string; err_out must
/// be NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn dr_path_metadata_import_report(
    path: *const c_char,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "reporting metadata import",
            ptr::null_mut(),
            || {
                let path = read_cstr(path, "path")?;
                let bytes = report_bytes(&path)?;
                let (scan, scan_warnings) = crate::metadata::scan_report(&bytes);
                let orientation = crate::metadata_tiff::orientation_of_file(&bytes).or_else(|| {
                    scan.metadata
                        .exif
                        .as_deref()
                        .and_then(crate::metadata_tiff::orientation)
                });
                let quarter_turned = orientation.is_some_and(|o| (5..=8).contains(&o.to_exif()));
                let resolution = scan.resolution.map(|r| r.transposed_if(quarter_turned));
                let (container, packets, limitations, warnings) = if crate::metadata_tiff::is_tiff(
                    &bytes,
                ) {
                    ("tiff", vec!["standard EXIF/GPS/Interop", "XMP", "IPTC"],
                vec!["TIFF capture fields are rebuilt with valid offsets; a MakerNote is carried as one opaque value, and other private capture fields are not preserved"], scan_warnings)
                } else if crate::native_magic::is_native_magic(&bytes) {
                    ("native", vec!["EXIF", "XMP", "IPTC"], vec![], vec![])
                } else if bytes.starts_with(b"\xff\xd8") {
                    (
                        "jpeg",
                        vec!["EXIF", "XMP (including Extended XMP)", "IPTC"],
                        vec![],
                        scan_warnings,
                    )
                } else if bytes.starts_with(&crate::metadata::PNG_SIGNATURE) {
                    ("png", vec!["EXIF", "XMP"], vec!["IPTC is not read"], vec![])
                } else {
                    (
                        "other",
                        vec![],
                        vec!["Capture metadata is not read by the core for this container"],
                        vec![],
                    )
                };
                let mut report = serde_json::json!({"container":container,
            "metadata_walked": container != "other", "supported_packets":packets,
            "limitations": limitations, "warnings":warnings});
                if let Some(resolution) = resolution {
                    report["resolution"] = serde_json::json!({"x":resolution.x,"y":resolution.y});
                }
                // A multipage TIFF (`tiff_pages`): how many pages it has, and
                // the byte open's sentences about them, read from headers as
                // the open decides them.
                if container == "tiff" {
                    if let Some(pages) = crate::tiff_pages::read(&bytes) {
                        if pages.count() > 1 {
                            report["tiff_pages"] = serde_json::Value::from(pages.count());
                        }
                        if let Some(warnings) = report["warnings"].as_array_mut() {
                            warnings.retain(|w| w != crate::metadata_tiff::FIRST_IMAGE_ONLY);
                            warnings.extend(pages.notes().into_iter().map(serde_json::Value::from));
                        }
                    }
                }
                // A TIFF's spot channels (#1 1c-2a, `tiff_spot`): the
                // observation and loss lines of the records every open reads.
                if container == "tiff" {
                    if let Some((_, records)) =
                        crate::tiff_spot::tiff_spot_records(&bytes, scan.metadata.iptc.as_deref())
                    {
                        let lines = records.observation().into_iter().chain(records.losses);
                        if let Some(warnings) = report["warnings"].as_array_mut() {
                            warnings.extend(lines.map(serde_json::Value::from));
                        }
                    }
                    // Its paths (`tiff_paths`): the sentences of the records
                    // the opens read, from the run alone.
                    let lines =
                        crate::tiff_paths::report_lines(&bytes, scan.metadata.iptc.as_deref());
                    if let Some(warnings) = report["warnings"].as_array_mut() {
                        warnings.extend(lines.into_iter().map(serde_json::Value::from));
                    }
                }
                // A JPEG's path records, which an open that turns it drops
                // (`tiff_paths::jpeg_opened`).
                if container == "jpeg" {
                    let orientation = orientation.map_or(1, |o| o.to_exif());
                    let run = scan.metadata.iptc.as_deref();
                    if let Some(line) = crate::tiff_paths::jpeg_report_line(run, orientation) {
                        if let Some(warnings) = report["warnings"].as_array_mut() {
                            warnings.push(serde_json::Value::from(line));
                        }
                    }
                }
                // Header work only, and never a failure of the report: a file
                // the flat decoder cannot read simply has no sentence.
                if let Some(sentence) =
                    crate::dr_image::embedded_profile_report(&bytes, scan.icc.clone())
                {
                    if let Some(warnings) = report["warnings"].as_array_mut() {
                        warnings.push(serde_json::Value::from(sentence.clone()));
                    }
                    report["profile_refusal"] = serde_json::Value::from(sentence);
                }
                CString::new(report.to_string())
                    .map_err(|e| format!("cannot report metadata import for {path}: {e}"))
            },
            CString::into_raw,
        )
    }
}

/// The sentence for an image a PLATFORM decoder (ImageIO, CoreGraphics)
/// converted to sRGB as it read it: `bytes` are the
/// encoded image it decoded — a file's contents, a pasteboard's own TIFF or
/// PNG — and `source` its colour model, one of `DR_PLATFORM_SOURCE_*`. The
/// converted sentence naming the embedded profile when the core's own open
/// would convert such samples from it, else the plain sentence naming the
/// model (`open_profile::platform_conversion_sentence`). NULL or empty
/// `bytes` is an image with no profile. NULL with err_out ("unknown
/// platform source N") for any other `source`. Never decodes pixels. Free
/// with dr_string_free.
///
/// # Safety
/// `bytes` must be NULL or valid for `len` bytes; err_out must be NULL or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn dr_platform_conversion_sentence(
    bytes: *const u8,
    len: usize,
    source: c_int,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "describing a platform conversion",
            ptr::null_mut(),
            || {
                let model = PlatformSource::from_c(source)
                    .ok_or_else(|| format!("unknown platform source {source}"))?;
                let icc = blob(bytes, len)
                    .filter(|bytes| !bytes.is_empty())
                    .and_then(crate::dr_image::embedded_icc);
                CString::new(platform_conversion_sentence(icc.as_deref(), model))
                    .map_err(|e| format!("cannot describe a platform conversion: {e}"))
            },
            CString::into_raw,
        )
    }
}

/// Sets the process-wide working CMYK profile an untagged CMYK TIFF
/// or JPEG is converted from as the flat open reads it, and which its
/// `profile_refusal` sentence names (`working_cmyk`). NULL `icc` or
/// `len == 0` clears it and returns true. False with err_out when the bytes
/// are not a CMYK ICC profile with a relative-colorimetric device-to-PCS
/// table; the profile already set is then kept.
///
/// # Safety
/// `icc` must be NULL or valid for `len` bytes; err_out must be NULL or
/// writable.
#[no_mangle]
pub unsafe extern "C" fn dr_set_working_cmyk_profile(
    icc: *const u8,
    len: usize,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "setting the working CMYK profile",
            false,
            || {
                let icc = blob(icc, len).filter(|bytes| !bytes.is_empty());
                set_working_cmyk_profile(icc)
            },
            |()| true,
        )
    }
}
