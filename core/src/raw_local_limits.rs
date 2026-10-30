//! The RAW-local counts and the combined source-resource cap, in one
//! leaf that imports nothing from the crate.
//!
//! A local is a native u16 coverage mask (`raw_local_mask`, `16 + 2·pixels`
//! bytes) blended over the global development. Since the local tone path
//! (`raw_local_tone`) a local whose controls are tone and colour reads the
//! development itself, so only a local that changes white balance or camera
//! exposure still develops a whole camera variant: at most
//! [`MAX_RAW_LOCAL_VARIANTS`] of those, and [`MAX_RAW_LOCALS`] locals in all.
//!
//! Sixteen is what the combined cap holds at the size the app is built
//! around: sixteen 24 MP masks are 768,000,256 bytes, and beside the largest
//! admitted original (256 MiB) that is 988 MiB, under the 1 GiB of
//! [`MAX_SOURCE_RESOURCE_TOTAL_BYTES`]. Above about 32 MP the cap holds fewer
//! ([`fitting_locals`]): 11 for a D850 file, 8 for an A7R IV, 14 for a K-1 II.
//! RAW originals alone keep their own 512 MiB aggregate (`raw_source`).

/// Locals one RAW layer may hold.
pub const MAX_RAW_LOCALS: usize = 16;
/// Locals that develop a camera variant (white balance or camera exposure).
pub const MAX_RAW_LOCAL_VARIANTS: usize = 4;
/// The resource count one attachment could carry before DRDC 79
/// (`raw_locals_native`); a file with more needs the word.
pub const PRE_79_ATTACHMENT_RESOURCES: usize = 4;
/// RAW originals and attachment resources together, deduplicated by
/// allocation (`source_attachment::ResourceCapacity`).
pub const MAX_SOURCE_RESOURCE_TOTAL_BYTES: u64 = 1 << 30;

/// How many locals of `pixels` samples fit beside an original of
/// `original_bytes` under [`MAX_SOURCE_RESOURCE_TOTAL_BYTES`]:
/// `min(16, (cap − original) / (16 + 2·pixels))`, 0 when the original alone
/// exceeds the cap. Arithmetic only, so a test reaches the figures without a
/// 1 GiB allocation; the host mirrors it (`RawLocalWork.maximumLocals`).
#[doc(hidden)]
pub fn fitting_locals(pixels: u64, original_bytes: u64) -> usize {
    let Some(room) = MAX_SOURCE_RESOURCE_TOTAL_BYTES.checked_sub(original_bytes) else {
        return 0;
    };
    let mask = pixels.saturating_mul(2).saturating_add(16);
    usize::try_from(room / mask)
        .unwrap_or(MAX_RAW_LOCALS)
        .min(MAX_RAW_LOCALS)
}
