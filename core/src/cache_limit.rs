//! The resident-cache registry's byte bound as a fraction of the working
//! limit. A leaf: it imports nothing from the crate, and `cache_registry`
//! applies it to the process working limit at every insert.
//!
//! Why an eighth: it is the threshold the host's post-projection trim used
//! before the registry replaced it (13B-6), so no admission answer moved when
//! the trim went: 1 GiB at the 8 GiB working limit of the assumed minimum
//! machine (a 16 GB Mac), 256 MiB at the 2 GiB test pin. Measured at 24 MP
//! (13B-6), one drop-shadow entry is 48,843,528 bytes and a three-effect
//! entry 98,495,376, so the bound holds about ten such styled layers' current
//! planes at 8 GiB and two at the pin. The trim compared the sum of every
//! document's non-view cache bytes against this value and then evicted all
//! of them; the registry evicts least recently used entries one at a time.

/// The registry's bound is the working limit / this.
pub const REGISTRY_BOUND_DIVISOR: u64 = 8;

/// The registry's byte bound for `working_limit` (floor).
pub fn registry_bound(working_limit: u64) -> u64 {
    working_limit / REGISTRY_BOUND_DIVISOR
}
