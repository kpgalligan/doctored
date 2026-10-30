//! The work-unit reference the `#[ignore]`d calibration probes share
//! (`gaussian_radius_independent.rs`, `spatial_filters.rs`).

/// The FIR Gaussian's rate at σ3.9 (byte High Pass, charged n·(8·25+12))
/// over the rate of byte High Pass at radius 16 before the fast Gaussian
/// (charged n·(8·97+12)). Every calibration in
/// `gaussian_radius_independent.rs` and `spatial_filters.rs`
/// (`FAST_WORK_PER_PIXEL`, the Surface Blur coefficients) is stated in that
/// r16 unit, phase-1 record 1.1734e9 units/s. Byte High Pass r16 now runs
/// the fast Gaussian, so the probes time σ3.9 (still the FIR) and divide its
/// rate by this.
///
/// Measured 2026-10-03, 3000×2000 splitmix noise, min of 3, release, Apple
/// M2 Max, two runs each: R16 on the 10c8007 build 1.1901e9 / 1.1702e9;
/// R3.9 on the row-ring FIR build 3.4035e9 / 3.3502e9; ratio 2.860 / 2.863.
/// At 10c8007 itself R3.9 was 1.6673e9 / 1.6704e9 (ratio 1.41): the row
/// ring roughly halved the σ3.9 FIR's time without changing a bit of its
/// output. The FIR's charge is not proportional to its time across tap
/// counts (at 10c8007, 2.10e9 units/s at 7 taps and 1.14e9 at 145), which is
/// why the ratio is not 1.
pub const R39_PER_R16: f64 = 2.86;
