//! C FFI for Select ▸ Color Range ▸ Out Of Gamut: the coverage export
//! over `color_range_gamut::out_of_gamut` and the scratch figure it admits,
//! so the host reserves exactly what the core charges. Same conventions as
//! `ffi_output_color`'s gamut mask, whose argument parsers and scratch
//! formula these reuse.

use std::ffi::c_char;
use std::ptr;

use crate::color_range_gamut;
use crate::ffi_color::required_profile;
use crate::ffi_output_color::{byte_proof_scratch_pixels, proof_arg, setup_arg};
use crate::ffi_util::{fallible_op, required_handle};
use crate::job_control::{DrJobControl, JobContext};
use crate::DrImage;

/// Color Range ▸ Out Of Gamut: writes into `out` (the caller declares `len`
/// bytes, which must equal width*height recomputed from `flat`'s own
/// dimensions) the source alpha where the gamut warning's rule flags the
/// pixel — inverted when `invert` — else 0. `flat`'s numbers are in the
/// profile `doc_icc` describes; the proof is `proof_icc` under `setup_json`.
/// Admits `flat`'s RGBA bytes plus `dr_selection_out_of_gamut_scratch`'s
/// figure (the gamut mask's scratch plus the output) against `limit` with
/// `other` retained elsewhere, then begins `control` (may be NULL) and
/// reserves that figure. Returns false and leaves `out` untouched on refusal
/// or cancel.
///
/// # Safety
/// `flat` must be NULL or a valid pointer to a live `DrImage`; both profile
/// buffers must be NULL or valid for their stated lengths; `setup_json` must
/// be NULL or a valid NUL-terminated C string; `control` must be NULL or a
/// valid pointer to a live `DrJobControl`; `out` must be NULL or writable for
/// `len` bytes; `err_out` must be NULL or a valid pointer to writable
/// `*mut c_char`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dr_selection_out_of_gamut(
    flat: *const DrImage,
    doc_icc: *const u8,
    doc_len: usize,
    proof_icc: *const u8,
    proof_len: usize,
    setup_json: *const c_char,
    invert: bool,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    out: *mut u8,
    len: usize,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "selecting out-of-gamut colours",
            false,
            || {
                if out.is_null() {
                    return Err("the Out Of Gamut buffer is NULL".into());
                }
                let image = required_handle(flat)?;
                let (w, h) = image.pixels.dimensions();
                // Recomputed from the core's own dimensions, never trusted.
                let pixels = (w as usize) * (h as usize);
                if len != pixels {
                    return Err("the Out Of Gamut buffer length does not match the image".into());
                }
                let document = required_profile(doc_icc, doc_len)?;
                let proof = proof_arg(proof_icc, proof_len)?;
                let setup = setup_arg(setup_json)?;
                let scratch =
                    byte_proof_scratch_pixels(pixels as u64, doc_len, &proof) + pixels as u64;
                let retained = image.pixels.as_raw().len() as u64;
                crate::precision_image::admit(retained, scratch, other, limit)?;
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                job.reserve(scratch)?;
                color_range_gamut::out_of_gamut(
                    &image.pixels,
                    &document,
                    &proof,
                    &setup,
                    invert,
                    job,
                )
            },
            |coverage| {
                ptr::copy_nonoverlapping(coverage.as_ptr(), out, coverage.len());
                true
            },
        )
    }
}

/// The bytes `dr_selection_out_of_gamut` admits for a `width` x `height`
/// image beyond the image itself: the byte gamut mask's scratch
/// (`dr_proof_gamut_mask`'s figure) plus the width*height output. 0 when
/// `proof_icc` is NULL or does not parse.
///
/// # Safety
/// `proof_icc` must be NULL or valid for `proof_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn dr_selection_out_of_gamut_scratch(
    width: u32,
    height: u32,
    doc_len: usize,
    proof_icc: *const u8,
    proof_len: usize,
) -> u64 {
    unsafe {
        fallible_op(
            ptr::null_mut(),
            "sizing an Out Of Gamut selection",
            0,
            || {
                let proof = proof_arg(proof_icc, proof_len)?;
                let pixels = u64::from(width) * u64::from(height);
                Ok(byte_proof_scratch_pixels(pixels, doc_len, &proof) + pixels)
            },
            |bytes| bytes,
        )
    }
}
