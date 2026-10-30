//! Quick Mask's reproducible stroke application. The live preview consumes a
//! premultiplied RGBA overlay; Actions/MCP consume its run-length gray/alpha
//! representation. Both finish at the same integer source-over operation.
//! Mutates caller-owned selection coverage only, like `doc_select` helpers.
//!
//! `selection_paint.rs` shares Quick Mask's exact integer stroke application between raw live
//! overlays and recorded run-length replay, validating before caller-buffer mutation.

/// The existing Quick Mask brush's quantized rule, moved from AppKit so live
/// preview and recorded replay cannot drift. This is the integer grayscale
/// counterpart of `doc_plane::channel_lerp`: input is already premultiplied
/// gray, so its stored byte adds directly and only the old coverage is faded.
fn paint(before: u8, gray: u8, alpha: u8) -> u8 {
    paint_scalar(u64::from(before), u64::from(gray), u64::from(alpha), 255) as u8
}
/// Exact scalar source-over shared by legacy byte Quick Mask and its additive
/// u16 contract. The caller validates premultiplied gray <= alpha <= maximum.
pub(crate) fn paint_scalar(before: u64, gray: u64, alpha: u64, maximum: u64) -> u64 {
    (gray + (before * (maximum - alpha) + maximum / 2) / maximum).min(maximum)
}

/// Validates the entire overlay before mutating the selection. Red is the
/// historical gray source (the Quick Mask UI renders monochrome). `scale`
/// multiplies premultiplied gray and alpha with the existing floor rule.
/// Returns false for wrong length or invalid premultiplied red; true for a
/// valid stroke including one that leaves every byte unchanged.
pub(crate) fn paint_overlay(mask: &mut [u8], overlay: &[u8], scale: u8) -> bool {
    if mask.len().checked_mul(4) != Some(overlay.len())
        || overlay.chunks_exact(4).any(|p| p[0] > p[3])
    {
        return false;
    }
    for (before, pixel) in mask.iter_mut().zip(overlay.chunks_exact(4)) {
        let gray = (u16::from(pixel[0]) * u16::from(scale) / 255) as u8;
        let alpha = (u16::from(pixel[3]) * u16::from(scale) / 255) as u8;
        *before = paint(*before, gray, alpha);
    }
    true
}

/// RLE records are UInt32 little-endian run length, premultiplied gray, alpha.
/// Lengths must be positive, cover exactly the mask, and have gray <= alpha.
/// Validate every record before a write, so a malformed final record cannot
/// partially change a caller's selection. Scale is already baked in records.
pub(crate) fn paint_runs(mask: &mut [u8], runs: &[u8]) -> bool {
    if runs.is_empty() || !runs.len().is_multiple_of(6) {
        return false;
    }
    let count = |r: &[u8]| u32::from_le_bytes([r[0], r[1], r[2], r[3]]) as usize;
    let mut total = 0usize;
    for record in runs.chunks_exact(6) {
        let n = count(record);
        let Some(end) = total.checked_add(n) else {
            return false;
        };
        if n == 0 || end > mask.len() || record[4] > record[5] {
            return false;
        }
        total = end;
    }
    if total != mask.len() {
        return false;
    }
    let mut at = 0;
    for record in runs.chunks_exact(6) {
        let end = at + count(record);
        for before in &mut mask[at..end] {
            *before = paint(*before, record[4], record[5]);
        }
        at = end;
    }
    true
}
