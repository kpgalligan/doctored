//! C FFI for the agent's exact pixel writes (`doc_pixels`). Thin shims only.

use crate::doc::DrDocument;
use crate::ffi_util::{doc_op, mask_slice};

/// Writes canvas rect (x, y, w, h) onto layer `idx` with REPLACE semantics:
/// every sample where `mask` is NULL or nonzero replaces all four straight
/// RGBA8 bytes verbatim (no blending; alpha 0 writes transparency). Elsewhere
/// the layer is unchanged. `rgba`: w*h*4 bytes, row 0 top. `mask`: NULL or
/// w*h bytes. Mapped through the layer offset. Samples outside the layer's
/// pixel rect are ignored; the layer never grows. Lock-gated like every pixel
/// writer (a transparency lock restores alpha). NULL on NULL doc/rgba, w or h
/// 0, a rect not fully inside the canvas, a non-raster or out-of-range idx, a
/// rect that misses the layer, a pixel-locked layer, or when no byte would
/// change.
///
/// # Safety
/// `doc` must be NULL or a valid pointer to a live `DrDocument`; `rgba` must
/// be NULL or valid for w*h*4 bytes and `mask` NULL or valid for w*h bytes
/// whenever the rect lies inside the canvas (neither is read otherwise).
// The parameter list deliberately mirrors the C signature one-for-one.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn dr_doc_put_pixels(
    doc: *const DrDocument,
    idx: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    rgba: *const u8,
    mask: *const u8,
) -> *mut DrDocument {
    let body = |d: &DrDocument| {
        // Validate the rect against the canvas before touching either
        // pointer, so both slice lengths are bounded by the canvas size.
        if w == 0
            || h == 0
            || u64::from(x) + u64::from(w) > u64::from(d.width)
            || u64::from(y) + u64::from(h) > u64::from(d.height)
        {
            return None;
        }
        let count = (w as usize).checked_mul(h as usize)?;
        let rgba = unsafe { mask_slice(rgba, count.checked_mul(4)?) }?;
        let mask = unsafe { mask_slice(mask, count) };
        d.put_pixels(idx, (x, y, w, h), rgba, mask)
    };
    unsafe { doc_op(doc, body) }
}
