//! C FFI for colour samplers — the "Colour samplers" section of
//! `include/doctored_core.h`, byte exports first and their typed twins
//! after them, each the byte contract verbatim (`ffi_guide`'s shape). No
//! `err_out`: every refusal is NULL / 0 / false and the reasons are in the
//! doc comments, so a host that must SAY why checks first.
//!
//! A sampler crosses as a `uint32_t` TRIPLE `x, y, sample_size`. The setter
//! takes the WHOLE list — add, move, remove and clear are one call — and
//! never trusts the caller's `count` past `doc_sampler::MAX_COLOR_SAMPLERS`
//! before it reads the buffer.

use crate::doc::DrDocument;
use crate::doc_sampler::{ColorSampler, MAX_COLOR_SAMPLERS};
use crate::ffi_util::{doc_get, doc_op, handle_get, precision_op};
use crate::precision_document::PrecisionDocument;

/// The caller's triples as samplers, or `None` for a NULL buffer with a
/// non-zero count, a count past the cap (checked BEFORE the buffer is read)
/// or a size that does not fit a byte (`validated` refuses every size but
/// 1, 3 and 5 after this).
///
/// # Safety
/// `xys` must be NULL or readable for `3 * count` `uint32_t` when
/// `count <= MAX_COLOR_SAMPLERS`.
unsafe fn triples(xys: *const u32, count: usize) -> Option<Vec<ColorSampler>> {
    if count > MAX_COLOR_SAMPLERS {
        return None;
    }
    if count == 0 {
        return Some(Vec::new());
    }
    if xys.is_null() {
        return None;
    }
    let raw = unsafe { std::slice::from_raw_parts(xys, 3 * count) };
    raw.chunks_exact(3)
        .map(|t| {
            Some(ColorSampler {
                x: t[0],
                y: t[1],
                sample_size: u8::try_from(t[2]).ok()?,
            })
        })
        .collect()
}

/// Writes sampler `i` of `list` as a triple into `out`; false for a bad
/// index or a NULL buffer.
///
/// # Safety
/// `out` must be NULL or writable for 3 `uint32_t`.
unsafe fn write_triple(list: &[ColorSampler], i: usize, out: *mut u32) -> bool {
    let Some(s) = list.get(i) else {
        return false;
    };
    if out.is_null() {
        return false;
    }
    let triple = [s.x, s.y, u32::from(s.sample_size)];
    unsafe { std::ptr::copy_nonoverlapping(triple.as_ptr(), out, 3) };
    true
}

/// Number of colour samplers on the document; 0 on a NULL doc.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_color_sampler_count(doc: *const DrDocument) -> usize {
    unsafe { doc_get(doc, 0, |d| Some(d.color_samplers().len())) }
}

/// Sampler `i` as `x, y, sample_size` into `out_xys`; false on a NULL doc, a
/// NULL buffer or an out-of-range index.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `out_xys`
/// must be NULL or writable for 3 `uint32_t`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_color_sampler(
    doc: *const DrDocument,
    i: usize,
    out_xys: *mut u32,
) -> bool {
    unsafe {
        doc_get(doc, false, |d| {
            Some(write_triple(d.color_samplers(), i, out_xys))
        })
    }
}

/// The document holding exactly the `count` samplers in `xys` (triples
/// `x, y, sample_size`; NULL is allowed when `count` is 0, which clears the
/// list). NULL on a NULL doc, more than 10 samplers, an entry off the
/// canvas, a size other than 1/3/5, or the list the document already holds.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `xys` must
/// be NULL or readable for `3 * count` `uint32_t`.
#[no_mangle]
pub unsafe extern "C" fn dr_doc_with_color_samplers(
    doc: *const DrDocument,
    xys: *const u32,
    count: usize,
) -> *mut DrDocument {
    unsafe {
        doc_op(doc, |d| {
            let list = triples(xys, count)?;
            d.with_color_samplers(&list)
        })
    }
}

/// `dr_doc_color_sampler_count` at high depth; 0 on a NULL doc.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_color_sampler_count(
    doc: *const PrecisionDocument,
) -> usize {
    unsafe { handle_get(doc, 0, |d| d.color_samplers().len()) }
}

/// `dr_doc_color_sampler` at high depth.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`;
/// `out_xys` must be NULL or writable for 3 `uint32_t`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_color_sampler(
    doc: *const PrecisionDocument,
    i: usize,
    out_xys: *mut u32,
) -> bool {
    unsafe { handle_get(doc, false, |d| write_triple(d.color_samplers(), i, out_xys)) }
}

/// `dr_doc_with_color_samplers` at high depth, refusing exactly where it
/// refuses.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrPrecisionDocument`;
/// `xys` must be NULL or readable for `3 * count` `uint32_t`.
#[no_mangle]
pub unsafe extern "C" fn dr_precision_doc_with_color_samplers(
    doc: *const PrecisionDocument,
    xys: *const u32,
    count: usize,
) -> *mut PrecisionDocument {
    unsafe {
        precision_op(doc, |d| {
            let list = triples(xys, count)?;
            d.with_color_samplers(&list)
        })
    }
}
