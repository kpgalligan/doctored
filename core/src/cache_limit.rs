//! The resident cache bound as a fraction of the working limit.
//! A leaf: it imports nothing from the crate, and `precision_image`'s
//! `process_cache_limit` / `process_cache_trim_limit` apply it to the process
//! working limit.
//!
//! Where the figures come from: they were sized for live-filter bindings,
//! which were removed on 2026-10-10. A binding's whole neighbourhood plane was
//! straight f32 RGBA, 16 B/px, so a 24 MP plane was 384 MB; 1/16 of the 8 GiB
//! working limit of the assumed minimum machine (a 16 GB Mac) is 512 MiB and
//! held one, and the host trim held two so that two such layers re-edited
//! side by side both stayed cached.
//!
//! The trim threshold keeps that value, the working limit / 8 (256 MiB at the
//! 2 GiB test pin), for the disposable caches that remain. The trim compares
//! the sum of every document's non-view cache bytes against the one threshold
//! and then evicts all of them. It is all-or-nothing.

/// One cache bound is the working limit / this.
pub const CACHE_LIMIT_DIVISOR: u64 = 16;

/// The host's post-projection trim holds this many cache bounds.
pub const CACHE_TRIM_BINDINGS: u64 = 2;

/// One cache bound for `working_limit` (floor).
pub fn cache_limit(working_limit: u64) -> u64 {
    working_limit / CACHE_LIMIT_DIVISOR
}

/// The host's trim threshold for non-view cache bytes for `working_limit`:
/// [`CACHE_TRIM_BINDINGS`] cache bounds.
pub fn cache_trim_limit(working_limit: u64) -> u64 {
    cache_limit(working_limit).saturating_mul(CACHE_TRIM_BINDINGS)
}
