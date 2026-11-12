//! The codecs poll for Cancel as their bytes move, not only between stages.
//! Black-box through the FFI.
//!
//! Under a job control every export encoder writes through a polling sink
//! and the 16-bit decoders read through a polling source (`cancel_io`): one
//! checkpoint per `POLL_BYTES` (65,536, restated here) of encoded bytes
//! moved, and a cancel raised there answers exactly `operation cancelled`.
//! The oracle is the count: an export's checkpoints beyond its
//! size-independent stage checks are at least `len / POLL_BYTES - 1` for an
//! encoding of `len` bytes, where the stage-only codecs had none; a worker
//! PARKED at a checkpoint past the stage checks is inside the codec call, and
//! its cancel returns no result. A controlled export's bytes equal the
//! uncontrolled one's, so the polling changes no output.
//!
//! The fixtures are seeded noise, so every encoding is at least four poll
//! intervals long. Which codecs compute before their first write (and so
//! poll only while copying out) is the module doc's, not a claim here.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba};
use doctored_core::ffi_color::{dr_builtin_profile, dr_builtin_profile_len};
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_from_image};
use doctored_core::ffi_export::{
    dr_doc_export_encode, dr_export_result_bytes, dr_export_result_free,
    dr_precision_doc_export_encode,
};
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_open, dr_precision_doc_render, dr_precision_image_free,
};
use doctored_core::ffi_precision_input::dr_precision_doc_from_rgba16;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::DrImage;
use tempfile::TempDir;

use crate::common;
use common::probe::{cancelled_parked_at, parked_line, reports};
use common::{cpath, take_err_string};

/// `cancel_io::POLL_BYTES`, restated.
const POLL_BYTES: u64 = 65_536;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const BYTE_FORMATS: [&str; 6] = ["png", "jpeg", "tiff", "bmp", "gif", "webp"];
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

/// An opaque noise document and the same pixels as its caller-supplied
/// composite, so the stages before the encoder poll the same count at any
/// size. Answered as addresses, so a worker closure can carry them.
fn byte_fixture(w: u32, h: u32) -> (usize, usize) {
    let mut pixels = noise((w * h * 4) as usize, u64::from(w) * 7919 + u64::from(h));
    for alpha in pixels.iter_mut().skip(3).step_by(4) {
        *alpha = 255;
    }
    let image = unsafe { dr_image_from_rgba(pixels.as_ptr(), w, h) };
    assert!(!image.is_null());
    let doc = unsafe { dr_doc_from_image(image) };
    assert!(!doc.is_null());
    (doc as usize, image as usize)
}

fn free_byte_fixture((doc, image): (usize, usize)) {
    unsafe { dr_doc_free(doc as *mut DrDocument) };
    unsafe { dr_image_free(image as *mut DrImage) };
}

fn result_bytes(result: *mut doctored_core::doc_export::ExportResult) -> Vec<u8> {
    let mut length = 0usize;
    let pointer = unsafe { dr_export_result_bytes(result, &mut length) };
    assert!(!pointer.is_null());
    let bytes = unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec();
    unsafe { dr_export_result_free(result) };
    bytes
}

fn export8(
    (doc, image): (usize, usize),
    format: &str,
    control: *const DrJobControl,
) -> Result<Vec<u8>, String> {
    let request = CString::new(format!(r#"{{"format":"{format}"}}"#)).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let result = unsafe {
        dr_doc_export_encode(
            doc as *const DrDocument,
            image as *const DrImage,
            request.as_ptr(),
            control,
            &mut err,
        )
    };
    if result.is_null() {
        return Err(take_err_string(err));
    }
    Ok(result_bytes(result))
}

/// The bytes and the checkpoint count of one completed run under a fresh
/// control.
fn counted(run: impl FnOnce(*const DrJobControl) -> Result<Vec<u8>, String>) -> (Vec<u8>, u64) {
    let control = DrJobControl::default();
    let bytes =
        run(&control as *const DrJobControl).unwrap_or_else(|e| panic!("the run failed: {e}"));
    (bytes, control.checkpoints())
}

fn outcome(result: Result<Vec<u8>, String>) -> (bool, String) {
    match result {
        Ok(_) => (true, String::new()),
        Err(e) => (false, e),
    }
}

#[test]
fn the_byte_encoders_poll_as_their_bytes_move() {
    let small = byte_fixture(2, 2);
    let large = byte_fixture(768, 768);
    for format in BYTE_FORMATS {
        // 2×2 crosses no interval: its count is the stage checks alone.
        let (_, stages) = counted(|c| export8(small, format, c));
        let (bytes, total) = counted(|c| export8(large, format, c));
        let len = bytes.len() as u64;
        assert!(
            len >= 4 * POLL_BYTES,
            "{format}: the fixture encodes to {len} bytes"
        );
        assert_eq!(
            export8(large, format, ptr::null()).unwrap(),
            bytes,
            "{format}: polling changed the bytes"
        );
        assert!(
            total - stages >= len / POLL_BYTES - 1,
            "{format}: {total} checkpoints for {len} encoded bytes, {stages} for the stages alone"
        );
        // The stage checks before the encoder are `stages - 1` (the last is
        // the closing report's); `stages + 1` is the encoder's second poll.
        let parked = cancelled_parked_at(stages + 1, &|p, s| p < 1.0 && s == 1, |token| {
            outcome(export8(large, format, token as *const DrJobControl))
        });
        println!("{format}: {}", parked_line(&parked));
    }
    free_byte_fixture(small);
    free_byte_fixture(large);
}

fn srgb() -> Vec<u8> {
    let len = dr_builtin_profile_len(PROFILE_SRGB);
    let mut bytes = vec![0u8; len];
    assert!(unsafe { dr_builtin_profile(PROFILE_SRGB, bytes.as_mut_ptr(), len) });
    bytes
}

/// A typed noise document and its rendered projection, as addresses.
fn typed_fixture(w: u32, h: u32) -> (usize, usize) {
    let samples: Vec<u16> = noise((w * h * 8) as usize, 0x9E37_79B9)
        .chunks_exact(2)
        .map(|b| u16::from_ne_bytes([b[0], b[1]]))
        .collect();
    let profile = srgb();
    let mut err: *mut c_char = ptr::null_mut();
    let document = unsafe {
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
    assert!(!document.is_null(), "{}", take_err_string(err));
    let mut err: *mut c_char = ptr::null_mut();
    let projection = unsafe { dr_precision_doc_render(document, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!projection.is_null(), "{}", take_err_string(err));
    (document as usize, projection as usize)
}

fn export16(
    (document, projection): (usize, usize),
    format: &str,
    control: *const DrJobControl,
) -> Result<Vec<u8>, String> {
    let request = CString::new(format!(r#"{{"format":"{format}","bit_depth":16}}"#)).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_export_encode(
            document as *const PrecisionDocument,
            projection as *const PrecisionImage,
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
    Ok(result_bytes(result))
}

#[test]
fn the_sixteen_bit_encoders_poll_as_their_bytes_move() {
    let fixture = typed_fixture(512, 512);
    for format in ["png", "tiff"] {
        let log = reports(|token| outcome(export16(fixture, format, token as *const DrJobControl)));
        // `precision_codec::encode` reports 0.3 just before the dependency
        // encoder's call; the export's closing report follows it.
        let before = log
            .iter()
            .rfind(|r| r.stage == 1 && r.progress < 1.0)
            .expect("the encoder's last stage report")
            .checkpoints;
        let closing = log.last().expect("the closing report").checkpoints;
        let (bytes, _) = counted(|c| export16(fixture, format, c));
        let len = bytes.len() as u64;
        assert!(
            len >= 4 * POLL_BYTES,
            "{format}: the fixture encodes to {len} bytes"
        );
        assert_eq!(
            export16(fixture, format, ptr::null()).unwrap(),
            bytes,
            "{format}: polling changed the bytes"
        );
        // The encoder's closing check and the export's closing report are
        // two; the rest are polls inside the dependency call.
        assert!(
            closing - before >= 2 + len / POLL_BYTES - 1,
            "{format}: {} checkpoints after the encoder's last report for {len} bytes",
            closing - before
        );
        let parked = cancelled_parked_at(before + 3, &|p, s| p < 1.0 && s == 1, |token| {
            outcome(export16(fixture, format, token as *const DrJobControl))
        });
        println!("{format}16: {}", parked_line(&parked));
    }
    unsafe { dr_precision_image_free(fixture.1 as *mut PrecisionImage) };
    unsafe { dr_precision_doc_free(fixture.0 as *mut PrecisionDocument) };
}

fn open16(path: &std::path::Path, control: *const DrJobControl) -> (bool, String) {
    let path = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let document = unsafe { dr_precision_doc_open(path.as_ptr(), LIMIT, 0, control, &mut err) };
    if document.is_null() {
        return (false, take_err_string(err));
    }
    unsafe { dr_precision_doc_free(document) };
    (true, String::new())
}

#[test]
fn the_sixteen_bit_decoders_poll_as_their_input_moves() {
    let fixture = typed_fixture(512, 512);
    let dir = TempDir::new().unwrap();
    for format in ["png", "tiff"] {
        let bytes = export16(fixture, format, ptr::null()).unwrap();
        let len = bytes.len() as u64;
        assert!(
            len >= 4 * POLL_BYTES,
            "{format}: the fixture encodes to {len} bytes"
        );
        let path = dir.path().join(format!("noise16.{format}"));
        std::fs::write(&path, &bytes).unwrap();
        let log = reports(|token| open16(&path, token as *const DrJobControl));
        let at = |progress: f64| {
            log.iter()
                .find(|r| r.progress == progress)
                .unwrap_or_else(|| panic!("{format}: no report at {progress}"))
                .checkpoints
        };
        // `decode_with_context` reports 0.1 before the dependency decoder's
        // call and 0.75 after it; between them sit at most two stage checks.
        let (started, decoded) = (at(0.1), at(0.75));
        assert!(
            decoded - started >= 2 + len / POLL_BYTES / 2,
            "{format}: {} checkpoints inside the decode of {len} bytes",
            decoded - started
        );
        let parked = cancelled_parked_at(started + 3, &|p, _| p < 0.75, |token| {
            open16(&path, token as *const DrJobControl)
        });
        println!("open {format}16: {}", parked_line(&parked));
    }
    unsafe { dr_precision_image_free(fixture.1 as *mut PrecisionImage) };
    unsafe { dr_precision_doc_free(fixture.0 as *mut PrecisionDocument) };
}

mod bench {
    use super::*;
    use crate::common::probe::{cancelled_after_checkpoints, verdict};

    /// The bound the original qualification held on 24 and 48 MP fixtures
    /// (`designs/phase-4-jobs-resource-status.md`), declared before measuring.
    const BOUND_S: f64 = 2.0;

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn a_24_mp_jpeg_export_cancelled_inside_its_encoder_returns_promptly() {
        let small = byte_fixture(2, 2);
        let (_, stages) = counted(|c| export8(small, "jpeg", c));
        free_byte_fixture(small);
        let large = byte_fixture(6000, 4000);
        // Entry: the encoder's first poll; cancel at once.
        let probe = cancelled_after_checkpoints(
            &|_, _, checkpoints| checkpoints >= stages,
            0,
            &|p, s| p < 1.0 && s == 1,
            |token| outcome(export8(large, "jpeg", token as *const DrJobControl)),
        );
        println!(
            "24 MP JPEG export cancelled inside its encoder: {:.3} s → {} (bound {BOUND_S} s)",
            probe.latency.as_secs_f64(),
            verdict(probe.latency, BOUND_S)
        );
        free_byte_fixture(large);
    }
}
