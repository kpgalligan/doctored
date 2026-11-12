//! The float containers and the kept colour-mode files poll for Cancel as
//! their bytes move, not only between stages. Black-box through the FFI.
//!
//! Every kept-mode and float export reports one stage-1 progress step just
//! before its encoder and closes with its `(1.0, 5)` report, so the
//! checkpoints between those two reports are the encoder's own (plus the
//! closing report's check, and a float export's check after the encode).
//! The encoder writes through `cancel_io::Polled`: at most one checkpoint per
//! `POLL_BYTES` (65,536, restated) of encoded bytes, and at least half that
//! (a write's excess past an interval's end is not carried), so over a
//! seeded-noise fixture whose encoding is at least four intervals those
//! checkpoints are at least `len / POLL_BYTES / 2` beyond the fixed ones,
//! where a stage-only encoder had none. A worker PARKED at the encoder's
//! second poll is cancelled there. The Lab TIFF is a save, so its file's
//! write, one buffer, adds `len / POLL_BYTES - 1` more. A controlled encoding
//! equals the uncontrolled one, so the polling changes no byte.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;

use doctored_core::ffi_color::{dr_builtin_profile, dr_builtin_profile_len};
use doctored_core::ffi_export::{
    dr_export_result_bytes, dr_export_result_free, dr_precision_doc_export_encode,
};
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_input::dr_precision_doc_from_rgba16;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::common;
use common::probe::{cancelled_parked_at, parked_line, reports};
use common::{cpath, take_err_string};

/// `cancel_io::POLL_BYTES`, restated.
const POLL_BYTES: u64 = 65_536;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const PROFILE_SRGB: i32 = 0;

/// Seeded noise (xorshift64*), `len` bytes.
fn noise(len: usize, mut state: u64) -> Vec<u8> {
    (0..len)
        .map(|_| {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
        })
        .collect()
}

fn checked(doc: *mut PrecisionDocument, err: *mut c_char, what: &str) -> usize {
    assert!(!doc.is_null(), "{what}: {}", take_err_string(err));
    doc as usize
}

/// An opaque 16-bit RGB noise document.
fn rgb16(w: u32, h: u32) -> usize {
    let mut samples: Vec<u16> = noise((w * h * 8) as usize, 0x51_7CC1 ^ u64::from(w))
        .chunks_exact(2)
        .map(|b| u16::from_ne_bytes([b[0], b[1]]))
        .collect();
    for alpha in samples.iter_mut().skip(3).step_by(4) {
        *alpha = u16::MAX;
    }
    let len = dr_builtin_profile_len(PROFILE_SRGB);
    let mut profile = vec![0u8; len];
    assert!(unsafe { dr_builtin_profile(PROFILE_SRGB, profile.as_mut_ptr(), len) });
    let mut err: *mut c_char = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_from_rgba16(
            samples.as_ptr(),
            samples.len(),
            w,
            h,
            profile.as_ptr(),
            profile.len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    checked(doc, err, "rgb16")
}

fn doc(address: usize) -> *const PrecisionDocument {
    address as *const PrecisionDocument
}

fn free(address: usize) {
    unsafe { dr_precision_doc_free(address as *mut PrecisionDocument) };
}

fn to_gray(rgb: usize, depth: u32) -> usize {
    let options = CString::new(format!(r#"{{"space":"gray_gamma_22","depth":{depth}}}"#)).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        doctored_core::ffi_gray::dr_precision_doc_convert_to_gray(
            doc(rgb),
            ptr::null(),
            0,
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    checked(out, err, "to gray")
}

fn to_float(rgb: usize) -> usize {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float(
            doc(rgb),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    checked(out, err, "to float")
}

fn to_indexed(rgb: usize) -> usize {
    let options = CString::new(r#"{"palette":"web","dither":"none"}"#).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        doctored_core::ffi_indexed::dr_precision_doc_convert_to_indexed(
            doc(rgb),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    checked(out, err, "to indexed")
}

fn to_bitmap(gray8: usize) -> usize {
    let options = CString::new(r#"{"method":"diffusion"}"#).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        doctored_core::ffi_bitmap::dr_precision_doc_convert_to_bitmap(
            doc(gray8),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    checked(out, err, "to bitmap")
}

fn to_cmyk(rgb: usize) -> usize {
    let profile = press_profile("Press Codec Cancel");
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk(
            doc(rgb),
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    checked(out, err, "to cmyk")
}

fn to_lab(rgb: usize) -> usize {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        doctored_core::ffi_lab::dr_precision_doc_convert_to_lab(
            doc(rgb),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    checked(out, err, "to lab")
}

fn export(document: usize, request: &str, control: *const DrJobControl) -> Result<Vec<u8>, String> {
    let request = CString::new(request).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_export_encode(
            doc(document),
            ptr::null(),
            request.as_ptr(),
            LIMIT,
            0,
            control,
            &mut err,
        )
    };
    if result.is_null() {
        return Err(take_err_string(err));
    }
    let mut length = 0usize;
    let pointer = unsafe { dr_export_result_bytes(result, &mut length) };
    assert!(!pointer.is_null());
    let bytes = unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec();
    unsafe { dr_export_result_free(result) };
    Ok(bytes)
}

fn save_lab(document: usize, path: &Path, control: *const DrJobControl) -> Result<Vec<u8>, String> {
    let request = CString::new(r#"{"format":"tiff","strip_metadata":false}"#).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        doctored_core::ffi_lab::dr_precision_doc_save_lab(
            doc(document),
            ptr::null(),
            cpath(path).as_ptr(),
            request.as_ptr(),
            LIMIT,
            0,
            control,
            &mut err,
        )
    };
    if !ok {
        return Err(take_err_string(err));
    }
    Ok(std::fs::read(path).unwrap())
}

fn outcome(result: Result<Vec<u8>, String>) -> (bool, String) {
    match result {
        Ok(_) => (true, String::new()),
        Err(e) => (false, e),
    }
}

/// One encode under test: its name, what it runs, the fixed checkpoints
/// after the encoder (`tail`) and whether the encoding's bytes also move
/// through a file's write (`written`, a save).
struct Row<'a> {
    name: &'a str,
    run: &'a (dyn Fn(*const DrJobControl) -> Result<Vec<u8>, String> + Sync),
    tail: u64,
    written: bool,
}

fn assert_polls(row: Row<'_>) {
    let name = row.name;
    let log = reports(|token| outcome((row.run)(token as *const DrJobControl)));
    // The encoder's stage-1 report is the last before the closing one (an
    // earlier stage may already have reported full progress, so it is found
    // by place, not by value).
    let [.., anchor, last] = log.as_slice() else {
        panic!("{name}: fewer than two reports: {log:?}");
    };
    assert_eq!(
        (anchor.stage, last.stage),
        (1, 5),
        "{name}: the encoder's report, then the closing one: {log:?}"
    );
    let (before, closing) = (anchor.checkpoints, last.checkpoints);
    let control = DrJobControl::default();
    let bytes = (row.run)(&control).unwrap_or_else(|e| panic!("{name}: {e}"));
    let len = bytes.len() as u64;
    assert!(
        len >= 4 * POLL_BYTES,
        "{name}: the fixture encodes to {len} bytes"
    );
    assert_eq!(
        (row.run)(ptr::null()).unwrap(),
        bytes,
        "{name}: polling changed the bytes"
    );
    // A file's write hands `Polled` the whole buffer, so it polls once per
    // interval but the last; an encoder's writes are its own sizes, and the
    // excess of a write past an interval's end is not carried, so it is held
    // to half that.
    let write = if row.written { len / POLL_BYTES - 1 } else { 0 };
    assert!(
        closing - before >= row.tail + write + len / POLL_BYTES / 2,
        "{name}: {} checkpoints after the encoder's report for {len} encoded bytes \
         (reports {:?})",
        closing - before,
        &log[log.len().saturating_sub(4)..]
    );
    // The encoder's second poll, while its stage-1 report stands (an earlier
    // stage may already have reported full progress).
    let parked = cancelled_parked_at(before + 2, &|_, s| s == 1, |token| {
        outcome((row.run)(token as *const DrJobControl))
    });
    println!("{name}: {}", parked_line(&parked));
}

#[test]
fn the_float_container_encoders_poll_as_their_bytes_move() {
    let rgb = rgb16(512, 512);
    let float = to_float(rgb);
    for (name, request) in [
        ("float TIFF f32", r#"{"format":"tiff","bit_depth":32}"#),
        (
            "float TIFF f16",
            r#"{"format":"tiff","bit_depth":32,"float_samples":"f16"}"#,
        ),
        ("OpenEXR", r#"{"format":"exr","bit_depth":32}"#),
        ("Radiance", r#"{"format":"hdr","bit_depth":32}"#),
    ] {
        assert_polls(Row {
            name,
            run: &|c| export(float, request, c),
            tail: 2,
            written: false,
        });
    }
    free(float);
    free(rgb);
}

#[test]
fn the_kept_gray_indexed_and_bitmap_encoders_poll_as_their_bytes_move() {
    let rgb = rgb16(1024, 1024);
    let gray16 = to_gray(rgb, 16);
    let gray8 = to_gray(rgb, 8);
    let indexed = to_indexed(rgb);
    free(rgb);
    let wide = rgb16(2048, 2048);
    let wide_gray8 = to_gray(wide, 8);
    free(wide);
    let bitmap = to_bitmap(wide_gray8);
    free(wide_gray8);
    let rows: [(&str, usize, &str); 11] = [
        ("Gray PNG 16", gray16, r#"{"format":"png","bit_depth":16}"#),
        (
            "Gray TIFF 16",
            gray16,
            r#"{"format":"tiff","bit_depth":16}"#,
        ),
        ("Gray PNG 8", gray8, r#"{"format":"png","bit_depth":8}"#),
        ("Gray TIFF 8", gray8, r#"{"format":"tiff","bit_depth":8}"#),
        (
            "Gray JPEG",
            gray8,
            r#"{"format":"jpeg","bit_depth":8,"jpeg_quality":95}"#,
        ),
        ("Indexed PNG", indexed, r#"{"format":"png","bit_depth":8}"#),
        ("Indexed GIF", indexed, r#"{"format":"gif","bit_depth":8}"#),
        (
            "Indexed TIFF",
            indexed,
            r#"{"format":"tiff","bit_depth":8}"#,
        ),
        ("Bitmap PNG", bitmap, r#"{"format":"png","bit_depth":8}"#),
        ("Bitmap TIFF", bitmap, r#"{"format":"tiff","bit_depth":8}"#),
        ("Bitmap BMP", bitmap, r#"{"format":"bmp","bit_depth":8}"#),
    ];
    for (name, document, request) in rows {
        assert_polls(Row {
            name,
            run: &|c| export(document, request, c),
            tail: 1,
            written: false,
        });
    }
    for document in [gray16, gray8, indexed, bitmap] {
        free(document);
    }
}

#[test]
fn the_cmyk_and_lab_tiff_encoders_poll_as_their_bytes_move() {
    let rgb = rgb16(512, 512);
    let cmyk = to_cmyk(rgb);
    let lab = to_lab(rgb);
    free(rgb);
    assert_polls(Row {
        name: "CMYK TIFF",
        run: &|c| export(cmyk, r#"{"format":"tiff","bit_depth":16}"#, c),
        tail: 1,
        written: false,
    });
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("lab.tif");
    // The encode's check after it and the save's check before the rename
    // are fixed; the encoding moves through the encoder's sink and again
    // through the file's write.
    assert_polls(Row {
        name: "Lab TIFF",
        run: &|c| save_lab(lab, &path, c),
        tail: 2,
        written: true,
    });
    free(cmyk);
    free(lab);
}
