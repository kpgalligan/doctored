//! Every typed clipboard READ — Copy, a group's Copy, Copy Merged and Cut —
//! also admits the HOST's stage: the public rendering the host draws from the
//! payload (`publicRendering(of:)`, an 8-bit or 16-bit CGImage) and its
//! TIFF/PNG encode for the pasteboard, which run after the core's read
//! returns and while the payload and its RZCB encoding are still held.
//!
//! # The rule
//!
//! `host_stage_bytes(w, h, storage, covered) = 3 · payload_bytes(w, h,
//! storage, covered) + PUBLIC_RENDERING_BYTES_PER_PIXEL · w · h` — the
//! payload, its encoding and the pasteboard's copy of the encoding, plus the
//! fitted per-window-pixel cost of the rendering and its encode. It is
//! admitted ONCE in `clipboard_copy_with_context`, after the window and
//! coverage checks and before any read, so it is a separate `admit` beside
//! the read's own charges: the read's threshold is the MAX of them.
//! `group_copy_plan` / `group_copy_allocation` take the same max, so their
//! plan−1/plan threshold stays exact, and
//! [`PrecisionDocument::clipboard_read_allocation`] states the NEW memory of
//! one read (resident pixels excluded) — the quantity the measurement gate
//! compares with the measured footprint.
//!
//! # Measurement record (batch 171, 2026-10-05)
//!
//! Fixture: a Release app on port 4997 with `DR_WORKING_LIMIT_BYTES =
//! 8589934592`, `MallocLargeCache=0`, `DR_BACKGROUND=1`, private `DR_*_DIR`s
//! and a private pasteboard; one measured MCP call per fresh instance, E =
//! `ri_interval_max_phys_footprint` − the footprint before the call, A =
//! [`PrecisionDocument::clipboard_read_allocation`] of the same read (for a
//! layers copy, of the clip `duplicate_to_new_document` builds). 16 bits:
//! batch 170's 6000×4000 `g16.rz` (a Background plus a Screen 50 % group of
//! three children, one Multiply 70 %, one clipped). Float: the float_11f
//! scene resized to 4500×3000 (13.5 MP), three duplicates (Multiply 70 %,
//! one clipped) grouped, the group Lighten 50 %, the Background outside it.
//! At 6000×4000 float every copy's pasteboard write was then refused by the
//! system pasteboard (a typed payload of 384 MB or more: `_setData` returns
//! false), so the float cases were measured where the write succeeds; such
//! a payload is now held in the host's process (the 24 MP record below).
//!
//! - Without the term (K = 0): the 16-bit Copy Merged (`select_all`) E =
//!   1,450,476,720 / 1,450,558,664 / 1,449,460,936 against A = 1,729,048,576
//!   (1.19); the 16-bit Copy of layer 0 E = 1,444,938,808 / 1,444,987,960 /
//!   1,444,955,168 against A = 720,000,000; the float Copy Merged E =
//!   1,321,305,144 / 1,321,272,376 / 1,321,927,736 against A = 1,189,048,576;
//!   the float layers copy of the group E = 1,186,972,632 / 1,186,923,432 /
//!   1,186,923,432 against A = 1,296,000,000, and of the group plus the
//!   Background (Copy Merged of the clip) E = 1,186,956,248 / 1,186,874,328
//!   / 1,186,923,432 against A = 1,135,048,576. The 16-bit layers copy of the
//!   group passed already (E ≤ 1,245,135,760, A = 1,728,000,000, the group
//!   margin's staged term).
//! - Fitted: the smallest integer K with `3 · payload + K · w·h ≥ ⌈1.25 ·
//!   max E⌉` in every case is 69 B/px (the float Copy Merged decides it; 68
//!   gives 1,647,000,000 < 1,652,409,670).
//! - With K = 69, re-measured on a rebuilt app (E per run; A; A / max E):
//!   16-bit Copy Merged 1,449,395,376 / 1,453,049,032 / 1,449,379,016,
//!   A = 2,376,000,000, 1.635; 16-bit Copy 1,444,906,040 / 1,444,922,424 /
//!   1,444,906,040, A = 2,376,000,000, 1.644; 16-bit layers copy (one root)
//!   1,246,217,104 / 1,245,135,760 / 1,245,152,168, A = 2,232,000,000,
//!   1.791; float Copy Merged 1,321,272,376 / 1,321,878,560 / 1,321,223,200,
//!   A = 1,660,500,000, 1.256; float layers copy (one root) 1,186,890,712 /
//!   1,186,939,840 / 1,186,890,688, A = 1,579,500,000, 1.331; float layers
//!   copy (two roots) 1,186,874,304 / 1,186,972,632 / 1,186,857,920, A =
//!   1,579,500,000, 1.331. The `list_documents` twin E = 0 / 0 / 0. Every
//!   gate A ≥ ⌈1.25 × max E⌉ holds.
//!
//! # Measurement record (batch 172, 2026-10-05)
//!
//! The same method and K = 69, on a rebuilt app whose typed payloads over
//! 255 MiB are held in process (every measured reply `typed_store:
//! "process"`; the default inline limit). Float: batch 171's 6000×4000
//! fixture (24 MP), and a one-layer 8200×5500 float_11f scene (45.1 MP).
//! E per run; A; A / max E:
//!
//! - 24 MP Copy of layer 0 (`select_all`) 1,062,699,944 / 1,059,062,720 /
//!   1,061,225,384, A = 2,952,000,000, 2.778.
//! - 24 MP Copy Merged (`select_all`) 1,296,778,176 / 1,293,108,160 /
//!   1,296,745,408, A = 2,952,000,000, 2.276.
//! - 24 MP layers copy (one root) 1,245,086,584 / 1,245,070,200 /
//!   1,245,332,344, A = 2,808,000,000, 2.255; (two roots) 1,245,102,968 /
//!   1,245,070,200 / 1,245,070,200, A = 2,808,000,000, 2.255.
//! - 45 MP Copy of layer 0 (`select_all`) 1,984,693,952 / 1,982,334,656 /
//!   1,988,347,608, A = 5,547,300,000, 2.790.
//! - The `list_documents` twin E = 0 / 0 / 0. Every gate A ≥ ⌈1.25 × max E⌉
//!   holds, so K stays 69: a held write never copies the payload into the
//!   pasteboard, so the third payload term is now slack at these sizes.
//!
//! The HOST module is the host's public-rendering + TIFF/PNG stage every typed read admits
//! once in `clipboard_copy_with_context` (`host_stage_bytes` = `3·payload +
//! PUBLIC_RENDERING_BYTES_PER_PIXEL·window`, fitted at 69, re-exported from `precision_clipboard`),
//! which `group_copy_plan`/`group_copy_allocation` max in, and `clipboard_read_allocation`, one
//! read's new memory (its module doc is the measurement record).

use crate::precision_clipboard::ClipboardRead;
use crate::precision_clipboard_codec::payload_bytes;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{pixel_count, PrecisionStorage};

/// The fitted per-window-pixel cost of the host's public rendering and its
/// TIFF/PNG encode, on top of three payloads. Why: the measured peaks of the
/// 16-bit and float copies (the module doc's record) are above what the
/// core's own terms charge, and the house rule asks the allocation figure to
/// be at least 1.25× each peak; 69 is the smallest integer that passes every
/// case (68 does not — the float Copy Merged decides it).
pub(crate) const PUBLIC_RENDERING_BYTES_PER_PIXEL: u64 = 69;

/// The host stage of one read of a `width`×`height` window in `storage`
/// (coverage present iff `covered`):
/// `3·payload_bytes(w, h, storage, covered) + PUBLIC_RENDERING_BYTES_PER_PIXEL·w·h`.
pub fn host_stage_bytes(
    width: u32,
    height: u32,
    storage: PrecisionStorage,
    covered: bool,
) -> Result<u64, String> {
    let pixels = pixel_count(width, height)? as u64;
    payload_bytes(width, height, storage, covered)
        .checked_mul(3)
        .and_then(|n| n.checked_add(PUBLIC_RENDERING_BYTES_PER_PIXEL.checked_mul(pixels)?))
        .ok_or_else(|| "clipboard size overflow".into())
}

impl PrecisionDocument {
    /// NEW memory of one read (resident pixels excluded): max(host stage, Copy raster 2·payload |
    /// group_copy_allocation | Copy Merged max(canvas·bpp + 2·payload, render_plan.scratch + 2·payload)).
    pub fn clipboard_read_allocation(
        &self,
        read: &ClipboardRead,
        covered: bool,
    ) -> Result<u64, String> {
        let (width, height) = match *read {
            ClipboardRead::Copy { width, height, .. }
            | ClipboardRead::CopyMerged { width, height, .. } => (width, height),
        };
        let host = host_stage_bytes(width, height, self.render_storage(), covered)?;
        let core = match *read {
            ClipboardRead::Copy { layer, .. } => {
                let record = self
                    .layer(layer)
                    .ok_or("clipboard layer index is out of range")?;
                match &record.content {
                    Content::CmykRaster { .. } | Content::CmykAdjustment(_) => return Err(crate::cmyk_document::refused_sentence("Copy")),
                    Content::Raster { image, .. } => {
                        2 * payload_bytes(width, height, image.storage(), covered)
                    }
                    Content::Group(_) | Content::PassThrough(_) => {
                        self.group_copy_allocation(layer, width, height, covered)?
                    }
                    Content::Adjustment(_) => {
                        return Err(format!(
                            "Copy requires a raster layer; layer {layer} is an adjustment layer (use Copy Merged)"
                        ))
                    }
                }
            }
            ClipboardRead::CopyMerged { .. } => {
                let storage = self.render_storage();
                let held = 2 * payload_bytes(width, height, storage, covered);
                let (w, h) = self.dimensions();
                let render = (pixel_count(w, h)? as u64)
                    .checked_mul(storage.bytes_per_pixel())
                    .ok_or("clipboard size overflow")?;
                let plan = self.render_plan()?;
                render
                    .checked_add(held)
                    .ok_or("clipboard size overflow")?
                    .max(
                        plan.scratch_bytes
                            .checked_add(held)
                            .ok_or("clipboard size overflow")?,
                    )
            }
        };
        Ok(host.max(core))
    }
}
