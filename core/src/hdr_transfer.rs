//! The BT.2100 transfer functions an HDR still is coded in (PQ and HLG), and
//! the ONE reference-white rule that puts their light into a 32-bit float
//! document (HDR photos).
//!
//! # Reference white
//!
//! 1.0 in a float document is SDR reference white. A gain-map still opens
//! with its SDR base's white at 1.0 (Core Image's own expansion, host side);
//! a PQ or HLG still opens with [`REFERENCE_WHITE_NITS`] = 203 cd/m² at 1.0,
//! ITU-R BT.2408's graphics white and Apple's convention for the same
//! mapping. So a PQ code meaning 1000 cd/m² opens as 1000 / 203 ≈ 4.93, and
//! the export writes `203 · v` back into PQ.
//!
//! # PQ (SMPTE ST 2084, BT.2100 Table 4)
//!
//! `pq_eotf_nits(E′) = 10000 · (max(E′^(1/m2) − c1, 0) / (c2 − c3·E′^(1/m2)))^(1/m1)`
//! with the published rationals m1 = 2610/16384, m2 = 2523/4096·128,
//! c1 = 3424/4096, c2 = 2413/4096·32, c3 = 2392/4096·32, and
//! [`pq_signal`] its exact inverse over 0…10000 cd/m².
//!
//! # HLG (BT.2100 Table 5)
//!
//! The signal goes through the inverse OETF to scene light `E` in 0…1
//! (`E′²/3` up to 0.5, `(exp((E′ − c)/a) + b)/12` above, a = 0.17883277,
//! b = 1 − 4a, c = 0.5 − a·ln(4a)), then the OOTF at the nominal display:
//! `F_D = α · Y_s^(γ−1) · E` with α = Lw = [`HLG_PEAK_NITS`] (Lb = 0, so
//! β = 0) and γ = 1.2, `Y_s` the scene luminance of the stated primaries
//! (`float_primaries::luminance_row`). A 75 % HLG grey lands on about
//! 203 cd/m², i.e. 1.0 — BT.2408's reference level, which is why the one
//! rule needs no HLG-specific scale.
//!
//! A kernel module: plain numbers in and out, no crate type.

/// The light, in cd/m², that 1.0 means in a float document (module doc).
pub const REFERENCE_WHITE_NITS: f64 = 203.0;

/// HLG's nominal display peak Lw (BT.2100's reference display), with black
/// Lb = 0 and the system gamma 1.2 that peak implies.
pub const HLG_PEAK_NITS: f64 = 1000.0;

/// PQ's absolute ceiling, cd/m².
const PQ_PEAK_NITS: f64 = 10000.0;

const M1: f64 = 2610.0 / 16384.0;
const M2: f64 = 2523.0 / 4096.0 * 128.0;
const C1: f64 = 3424.0 / 4096.0;
const C2: f64 = 2413.0 / 4096.0 * 32.0;
const C3: f64 = 2392.0 / 4096.0 * 32.0;

const HLG_A: f64 = 0.178_832_77;
const HLG_GAMMA: f64 = 1.2;

/// The two BT.2100 transfers a still can be coded in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HdrTransfer {
    Pq,
    Hlg,
}

impl HdrTransfer {
    /// `"pq"` or `"hlg"`; anything else refuses by name.
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "pq" => Ok(Self::Pq),
            "hlg" => Ok(Self::Hlg),
            other => Err(format!("HDR transfer \"{other}\" is not one of pq or hlg")),
        }
    }
}

/// The PQ EOTF: a signal in 0…1 (clamped) as absolute light in cd/m².
pub fn pq_eotf_nits(signal: f64) -> f64 {
    let e = signal.clamp(0.0, 1.0).powf(1.0 / M2);
    let numerator = (e - C1).max(0.0);
    let denominator = C2 - C3 * e;
    PQ_PEAK_NITS * (numerator / denominator).powf(1.0 / M1)
}

/// The inverse PQ EOTF: light in cd/m², clamped to 0…10000, as a signal in
/// 0…1.
pub fn pq_signal(nits: f64) -> f64 {
    let y = (nits / PQ_PEAK_NITS).clamp(0.0, 1.0);
    let p = y.powf(M1);
    ((C1 + C2 * p) / (1.0 + C3 * p)).powf(M2)
}

/// HLG's inverse OETF: a signal in 0…1 (clamped) as scene light in 0…1.
fn hlg_scene(signal: f64) -> f64 {
    let e = signal.clamp(0.0, 1.0);
    let b = 1.0 - 4.0 * HLG_A;
    let c = 0.5 - HLG_A * (4.0 * HLG_A).ln();
    if e <= 0.5 {
        e * e / 3.0
    } else {
        (((e - c) / HLG_A).exp() + b) / 12.0
    }
}

/// One coded RGB triple as float-document linear light (module doc): PQ per
/// channel, HLG through the OOTF with `luma` the scene-luminance row of the
/// stated primaries, both divided by [`REFERENCE_WHITE_NITS`].
pub fn signal_to_linear(transfer: HdrTransfer, signal: [f64; 3], luma: [f64; 3]) -> [f64; 3] {
    match transfer {
        HdrTransfer::Pq => signal.map(|s| pq_eotf_nits(s) / REFERENCE_WHITE_NITS),
        HdrTransfer::Hlg => {
            let scene = signal.map(hlg_scene);
            let ys = luma[0] * scene[0] + luma[1] * scene[1] + luma[2] * scene[2];
            // At Ys = 0 every channel is 0 too (Ys is a positive mix of
            // non-negative scene light), so the factor is never needed.
            let factor = if ys > 0.0 {
                HLG_PEAK_NITS * ys.powf(HLG_GAMMA - 1.0)
            } else {
                0.0
            };
            scene.map(|e| factor * e / REFERENCE_WHITE_NITS)
        }
    }
}

/// Float-document linear light as a PQ signal per channel: `203 · v` cd/m²,
/// held to 0…10000 (a negative component writes black), through
/// [`pq_signal`].
pub fn linear_to_pq_signal(linear: [f64; 3]) -> [f64; 3] {
    linear.map(|v| pq_signal(v * REFERENCE_WHITE_NITS))
}
