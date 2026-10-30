//! Additive native coverage-buffer contract. Legacy u8 selection functions keep
//! their existing meaning. Staged native outputs publish atomically on success.
use crate::ffi_util::{borrow_cstr_bounded, fallible_op};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_image::{admit, pixel_count, BOOKKEEPING_BYTES};
use crate::precision_selection_plane::PlaneOperation;
use serde_json::{json, Value};
use std::ffi::{c_char, CString};
use std::ptr;
unsafe fn request(
    width: u32,
    height: u32,
    text: *const c_char,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<(PlaneOperation, Value), String> {
    admit(0, 4 * BOOKKEEPING_BYTES, other, limit)?;
    job.reserve(4 * BOOKKEEPING_BYTES)?;
    let n = pixel_count(width, height)? as u64;
    let text = unsafe { borrow_cstr_bounded(text, "selection request", 64 * 1024) }?;
    let op: PlaneOperation = serde_json::from_str(text)
        .map_err(|e| format!("invalid native selection operation: {e}"))?;
    let scratch = 4 * BOOKKEEPING_BYTES
        + op.plan((width, height))?
        + n * (op.planes() as u64 + op.output_planes() as u64) * 2;
    admit(0, scratch, other, limit)?;
    job.reserve(scratch)?;
    let plan = json!({"retained_bytes":0,"scratch_bytes":scratch,"width":width,"height":height,"input_width":width,"input_height":height,"input_planes":op.planes(),"input_samples":n*op.planes() as u64,"output_samples":n*op.output_planes() as u64,"output_planes":op.output_planes()});
    Ok((op, plan))
}
/// Plan independent authoritative u16 coverage edits, including all borrowed
/// input planes and output staging. Free returned JSON with dr_string_free.
/// # Safety
/// request NULL/terminated UTF-8; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_selection_u16_plan(
    width: u32,
    height: u32,
    text: *const c_char,
    limit: u64,
    other: u64,
    err_out: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        fallible_op(
            err_out,
            "planning native selection",
            ptr::null_mut(),
            || {
                let (_, plan) = request(width, height, text, limit, other, JobContext::NONE)?;
                CString::new(plan.to_string()).map_err(|e| e.to_string())
            },
            CString::into_raw,
        )
    }
}
/// Compute a native selection edit into a staged output. Input planes are base,
/// then incoming for combine, coverage for paint(value), or premultiplied gray
/// and coverage for paint(no value). Error/cancellation leaves output unchanged.
/// Since 13A-6a feather/smooth/grow/shrink/border and refine's stages evaluate
/// over the coverage's nonzero SUPPORT grown by the kernel's reach, equal sample
/// for sample to the whole-plane result. No kind refuses for work: the
/// run is bounded by the planned scratch and the control's cancel.
/// # Safety
/// request NULL/terminated UTF-8; input/output NULL or readable/writable for the
/// core-planned lengths; buffers must not alias. control NULL/live until return;
/// err_out NULL/writable. A control is single-use and freed only by its owner.
#[no_mangle]
pub unsafe extern "C" fn dr_selection_u16(
    width: u32,
    height: u32,
    text: *const c_char,
    input: *const u16,
    input_len: usize,
    out: *mut u16,
    out_len: usize,
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "editing native selection",
            false,
            || {
                let control = control.as_ref();
                let _run = control.map(DrJobControl::begin).transpose()?;
                let job = control.map_or(JobContext::NONE, JobContext::new);
                let (op, plan) = request(width, height, text, limit, other, job)?;
                let expected = plan["input_samples"].as_u64().unwrap() as usize;
                let count = plan["output_samples"].as_u64().unwrap() as usize;
                if input.is_null() || out.is_null() || input_len != expected || out_len != count {
                    return Err("native selection buffer lengths differ from the core plan".into());
                }
                let input = std::slice::from_raw_parts(input, expected);
                let output = op.apply(input, (width, height), job)?;
                job.check()?;
                ptr::copy_nonoverlapping(output.as_ptr(), out, count);
                job.report(1., 5)?;
                Ok(())
            },
            |()| true,
        )
    }
}

/// Convert continuous encoded RGB ink to a native scalar through the shared
/// Rec.709 luminance coefficients. RGB must be finite and within [0,1]. Output
/// remains untouched on refusal; this interprets authored ink, not image pixels.
/// # Safety
/// out NULL/writable for one u16; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_scalar_ink_u16(
    red: f64,
    green: f64,
    blue: f64,
    out: *mut u16,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "converting native scalar ink",
            false,
            || {
                if out.is_null() {
                    return Err("native scalar ink output is null".into());
                }
                if [red, green, blue]
                    .iter()
                    .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
                {
                    return Err("scalar ink RGB must be finite and within [0,1]".into());
                }
                let gray = f64::from(crate::blend::LUMA_R) * red
                    + f64::from(crate::blend::LUMA_G) * green
                    + f64::from(crate::blend::LUMA_B) * blue;
                *out = (gray.clamp(0., 1.) * 65535.).round() as u16;
                Ok(())
            },
            |()| true,
        )
    }
}

/// The code Fill and Stroke write into ONE colour channel of an 8-bit layer:
/// the swatch's 30/59/11 luma over its 8-bit codes, rounded to a code
/// (`plane_swatch::plane_ink_u8`, Photoshop's measured `3059r`). A NULL out
/// refuses.
/// # Safety
/// out NULL/writable for one u8; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_plane_ink_u8(
    red: u8,
    green: u8,
    blue: u8,
    out: *mut u8,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "converting colour plane ink",
            false,
            || {
                if out.is_null() {
                    return Err("colour plane ink output is null".into());
                }
                *out = crate::plane_swatch::plane_ink_u8([red, green, blue]);
                Ok(())
            },
            |()| true,
        )
    }
}

/// The sample Fill and Stroke write into ONE colour channel of a 16-bit layer:
/// the encoded swatch's 30/59/11 luma, unrounded until its one
/// quantization (`plane_swatch::plane_ink_u16`, Photoshop's measured `3059x`).
/// RGB must be finite and within [0,1]. Output remains untouched on refusal.
/// # Safety
/// out NULL/writable for one u16; err_out NULL/writable.
#[no_mangle]
pub unsafe extern "C" fn dr_plane_ink_u16(
    red: f64,
    green: f64,
    blue: f64,
    out: *mut u16,
    err_out: *mut *mut c_char,
) -> bool {
    unsafe {
        fallible_op(
            err_out,
            "converting colour plane ink",
            false,
            || {
                if out.is_null() {
                    return Err("colour plane ink output is null".into());
                }
                if [red, green, blue]
                    .iter()
                    .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
                {
                    return Err("colour plane ink RGB must be finite and within [0,1]".into());
                }
                *out = crate::plane_swatch::plane_ink_u16([red, green, blue]);
                Ok(())
            },
            |()| true,
        )
    }
}
