//! The CMYK document tests' synthetic press profile (#547) and its oracle,
//! written from the ICC specification and the CIE formulas, never read off
//! the implementation.
//!
//! The profile is a v2 `prtr` CMYK profile with a Lab PCS and `mft2`
//! (lut16) tables on a 2-node grid. Every table is AFFINE in its inputs with
//! INTEGER code values at the corners, so any interpolation a CMM runs over
//! the grid — multilinear or tetrahedral — returns the affine function
//! exactly, and the oracle evaluates it in closed form:
//!
//! * `A2B0` = `A2B1` (ink → legacy Lab codes):
//!   `L = 65280 − 12800c − 12800m − 6400y − 28000k`,
//!   `a = 32768 − 4800c + 6400m`, `b = 32768 − 3200c − 1600m + 8000y`;
//! * `B2A1` (legacy Lab, each normalized to [0, 1] → ink codes):
//!   `C = 30000(1 − l) + 12000(1 − a)`, `M = 30000(1 − l) + 12000a`,
//!   `Y = 30000(1 − l) + 12000b`, `K = 20000(1 − l)`;
//! * `B2A0` (perceptual): every ink `65535(1 − l)`, so Lab(0, 0, 0) is full
//!   ink — the perceptual black lcms's `cmsDetectBlackPoint` reads for an
//!   output profile.
//!
//! The lut16 Lab encoding is the legacy one in both ICC versions:
//! `L* = code / 65280 · 100`, `a* = code / 256 − 128`.
//!
//! A SECOND press (#547 phase 5, `press_profile_b`) is built the same way
//! with other coefficients, so a CMYK → CMYK conversion and a CMYK proof
//! have a destination whose tables differ from the source's:
//!
//! * `A2B0` = `A2B1`: `L = 65280 − 14000c − 11000m − 5000y − 26000k`,
//!   `a = 32768 − 5200c + 7000m`, `b = 32768 − 2800c − 2000m + 9000y`;
//! * `B2A1`: `C = 28000(1 − l) + 14000(1 − a)`, `M = 28000(1 − l) + 14000a`,
//!   `Y = 28000(1 − l) + 14000b`, `K = 22000(1 − l)`;
//! * `B2A0`: every ink `65535(1 − l)`.
//!
//! Its destination black at relative and at perceptual is ZERO by
//! `destination_black`'s derivation: every table is affine, so the Lab →
//! device → Lab round trip of the black-point ramp is affine in L* (62 →
//! 95 L* at relative, 14 → 99.7 at perceptual: never straight), the
//! normalized ramp is exactly L* / 100, and its fitted root is L* = 0.
#![allow(dead_code)]

use crate::common::icc_chain::{apply, be32, inverse, tag, Chain};
use crate::icc_lut_builders::{build, tag_desc, tag_mft2, tag_wtpt};
use doctored_core::icc::IccProfile;

pub const D50: [f64; 3] = [0.9642, 1.0, 0.8249];
const EPSILON: f64 = 216.0 / 24389.0;
const KAPPA: f64 = 24389.0 / 27.0;

/// The A2B tables' legacy Lab codes for ink in [0, 1].
pub fn a2b_codes(d: [f64; 4]) -> [f64; 3] {
    [
        65280.0 - 12800.0 * d[0] - 12800.0 * d[1] - 6400.0 * d[2] - 28000.0 * d[3],
        32768.0 - 4800.0 * d[0] + 6400.0 * d[1],
        32768.0 - 3200.0 * d[0] - 1600.0 * d[1] + 8000.0 * d[2],
    ]
}

/// The B2A1 table's ink codes for normalized legacy Lab in [0, 1].
pub fn b2a1_codes(lab: [f64; 3]) -> [f64; 4] {
    let (l, a, b) = (lab[0], lab[1], lab[2]);
    [
        30000.0 * (1.0 - l) + 12000.0 * (1.0 - a),
        30000.0 * (1.0 - l) + 12000.0 * a,
        30000.0 * (1.0 - l) + 12000.0 * b,
        20000.0 * (1.0 - l),
    ]
}

/// The synthetic profile's bytes, named `name`.
pub fn press_profile(name: &str) -> Vec<u8> {
    let a2b = tag_mft2(4, 3, 2, &|d: &[f64]| {
        a2b_codes([d[0], d[1], d[2], d[3]])
            .iter()
            .map(|v| v / 65535.0)
            .collect()
    });
    let b2a1 = tag_mft2(3, 4, 2, &|lab: &[f64]| {
        b2a1_codes([lab[0], lab[1], lab[2]])
            .iter()
            .map(|v| v / 65535.0)
            .collect()
    });
    let b2a0 = tag_mft2(3, 4, 2, &|lab: &[f64]| vec![1.0 - lab[0]; 4]);
    build(
        [2, 0x10, 0, 0],
        b"prtr",
        b"CMYK",
        b"Lab ",
        &[
            (b"desc", tag_desc(name)),
            (b"wtpt", tag_wtpt(D50)),
            (b"A2B0", a2b.clone()),
            (b"A2B1", a2b),
            (b"B2A0", b2a0),
            (b"B2A1", b2a1),
        ],
    )
}

/// The second press's A2B codes for ink in [0, 1] (module doc).
pub fn a2b_codes_b(d: [f64; 4]) -> [f64; 3] {
    [
        65280.0 - 14000.0 * d[0] - 11000.0 * d[1] - 5000.0 * d[2] - 26000.0 * d[3],
        32768.0 - 5200.0 * d[0] + 7000.0 * d[1],
        32768.0 - 2800.0 * d[0] - 2000.0 * d[1] + 9000.0 * d[2],
    ]
}

/// The second press's B2A1 ink codes for normalized legacy Lab in [0, 1].
pub fn b2a1_codes_b(lab: [f64; 3]) -> [f64; 4] {
    let (l, a, b) = (lab[0], lab[1], lab[2]);
    [
        28000.0 * (1.0 - l) + 14000.0 * (1.0 - a),
        28000.0 * (1.0 - l) + 14000.0 * a,
        28000.0 * (1.0 - l) + 14000.0 * b,
        22000.0 * (1.0 - l),
    ]
}

/// The second synthetic press's bytes, named `name` (module doc).
pub fn press_profile_b(name: &str) -> Vec<u8> {
    press_profile_b_on_paper(name, D50)
}

/// The second press with the media white `paper` in its `wtpt` (its tables
/// unchanged), for Simulate Paper Color.
pub fn press_profile_b_on_paper(name: &str, paper: [f64; 3]) -> Vec<u8> {
    let a2b = tag_mft2(4, 3, 2, &|d: &[f64]| {
        a2b_codes_b([d[0], d[1], d[2], d[3]])
            .iter()
            .map(|v| v / 65535.0)
            .collect()
    });
    let b2a1 = tag_mft2(3, 4, 2, &|lab: &[f64]| {
        b2a1_codes_b([lab[0], lab[1], lab[2]])
            .iter()
            .map(|v| v / 65535.0)
            .collect()
    });
    let b2a0 = tag_mft2(3, 4, 2, &|lab: &[f64]| vec![1.0 - lab[0]; 4]);
    build(
        [2, 0x10, 0, 0],
        b"prtr",
        b"CMYK",
        b"Lab ",
        &[
            (b"desc", tag_desc(name)),
            (b"wtpt", tag_wtpt(paper)),
            (b"A2B0", a2b.clone()),
            (b"A2B1", a2b),
            (b"B2A0", b2a0),
            (b"B2A1", b2a1),
        ],
    )
}

/// A CMYK press with an A2B and NO B2A: nothing can be converted into it.
pub fn press_profile_without_b2a(name: &str) -> Vec<u8> {
    let a2b = tag_mft2(4, 3, 2, &|d: &[f64]| {
        a2b_codes([d[0], d[1], d[2], d[3]])
            .iter()
            .map(|v| v / 65535.0)
            .collect()
    });
    build(
        [2, 0x10, 0, 0],
        b"prtr",
        b"CMYK",
        b"Lab ",
        &[
            (b"desc", tag_desc(name)),
            (b"wtpt", tag_wtpt(D50)),
            (b"A2B0", a2b.clone()),
            (b"A2B1", a2b),
        ],
    )
}

/// An RGB LUT profile (an affine Lab table both ways), which a proof setup
/// can name but which has no plates and cannot proof a CMYK document.
pub fn rgb_lut_profile(name: &str) -> Vec<u8> {
    let a2b = tag_mft2(3, 3, 2, &|rgb: &[f64]| {
        vec![0.2 + 0.7 * rgb[1], 0.4 + 0.2 * rgb[0], 0.4 + 0.2 * rgb[2]]
    });
    let b2a = tag_mft2(3, 3, 2, &|lab: &[f64]| vec![lab[1], lab[0], lab[2]]);
    build(
        [2, 0x10, 0, 0],
        b"mntr",
        b"RGB ",
        b"Lab ",
        &[
            (b"desc", tag_desc(name)),
            (b"wtpt", tag_wtpt(D50)),
            (b"A2B0", a2b),
            (b"B2A0", b2a),
        ],
    )
}

/// Ink → the second press's own Lab (relative, D50).
pub fn ink_lab_b(ink: [f64; 4]) -> [f64; 3] {
    let c = a2b_codes_b(ink);
    [
        c[0] / 65280.0 * 100.0,
        c[1] / 256.0 - 128.0,
        c[2] / 256.0 - 128.0,
    ]
}

/// The second press's source black: full ink's L* (its perceptual black
/// at relative, its darker colorant at perceptual — the same table), made
/// neutral and clipped to 50, as XYZ.
pub fn source_black_b() -> [f64; 3] {
    let lstar = ink_lab_b([1.0; 4])[0].min(50.0);
    lab_to_xyz([lstar, 0.0, 0.0])
}

/// Lab as the lut16 legacy encoding, normalized and clamped to [0, 1].
pub fn encode_lab(lab: [f64; 3]) -> [f64; 3] {
    [
        (lab[0] / 100.0 * 65280.0 / 65535.0).clamp(0.0, 1.0),
        ((lab[1] + 128.0) * 256.0 / 65535.0).clamp(0.0, 1.0),
        ((lab[2] + 128.0) * 256.0 / 65535.0).clamp(0.0, 1.0),
    ]
}

/// The ISO 18619 BPC affine from `black` onto a zero destination black.
pub fn bpc_to_zero(xyz: [f64; 3], black: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| {
        let a = D50[i] / (D50[i] - black[i]);
        let b = -D50[i] * black[i] / (D50[i] - black[i]);
        a * xyz[i] + b
    })
}

/// CMYK → CMYK, the first press to the second (module doc): the first's
/// A2B (A2B0 = A2B1), BPC from its source black onto the second's zero
/// destination black when `bpc`, then the second's B2A0 (`perceptual`) or
/// B2A1, ink in [0, 1].
pub fn press_to_press_b(ink: [f64; 4], perceptual: bool, bpc: bool) -> [f64; 4] {
    let mut xyz = lab_to_xyz(ink_lab(ink));
    if bpc {
        xyz = bpc_to_zero(xyz, source_black());
    }
    let encoded = encode_lab(xyz_to_lab(xyz));
    if perceptual {
        return [1.0 - encoded[0]; 4];
    }
    b2a1_codes_b(encoded).map(|v| (v / 65535.0).clamp(0.0, 1.0))
}

/// The gamut rule's ΔE76 for an encoded sRGB colour against the first
/// press: its Lab against the Lab of its B2A1 → A2B1 round trip, relative,
/// no BPC.
pub fn gamut_delta_srgb(rgb: [f64; 3]) -> f64 {
    let side = RgbSide::of(&IccProfile::srgb());
    let xyz = apply(side.to_pcs, rgb.map(|v| side.decode(v)));
    let lab = xyz_to_lab(xyz);
    let ink = b2a1_codes(encode_lab(lab)).map(|v| v / 65535.0);
    let back = ink_lab(ink);
    ((lab[0] - back[0]).powi(2) + (lab[1] - back[1]).powi(2) + (lab[2] - back[2]).powi(2)).sqrt()
}

/// Lab (D50) → encoded RGB of the matrix/TRC `profile` through its own
/// tags, with the ISO 18619 BPC affine from `black` onto the matrix
/// profile's zero black when it is `Some`.
pub fn lab_to_rgb_in(lab: [f64; 3], black: Option<[f64; 3]>, profile: &IccProfile) -> [f64; 3] {
    let mut xyz = lab_to_xyz(lab);
    if let Some(black) = black {
        xyz = bpc_to_zero(xyz, black);
    }
    let side = RgbSide::of(profile);
    let linear = apply(inverse(side.to_pcs), xyz);
    linear.map(|v| side.encode(v))
}

pub fn lab_to_xyz(lab: [f64; 3]) -> [f64; 3] {
    let fy = (lab[0] + 16.0) / 116.0;
    let fx = fy + lab[1] / 500.0;
    let fz = fy - lab[2] / 200.0;
    let inv = |f: f64| {
        if f * f * f > EPSILON {
            f * f * f
        } else {
            (116.0 * f - 16.0) / KAPPA
        }
    };
    let y = if lab[0] > KAPPA * EPSILON {
        fy * fy * fy
    } else {
        lab[0] / KAPPA
    };
    [D50[0] * inv(fx), D50[1] * y, D50[2] * inv(fz)]
}

pub fn xyz_to_lab(xyz: [f64; 3]) -> [f64; 3] {
    let f = |t: f64| {
        if t > EPSILON {
            t.cbrt()
        } else {
            (KAPPA * t + 16.0) / 116.0
        }
    };
    let (fx, fy, fz) = (f(xyz[0] / D50[0]), f(xyz[1] / D50[1]), f(xyz[2] / D50[2]));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// Ink → the profile's own Lab (relative, D50).
pub fn ink_lab(ink: [f64; 4]) -> [f64; 3] {
    let c = a2b_codes(ink);
    [
        c[0] / 65280.0 * 100.0,
        c[1] / 256.0 - 128.0,
        c[2] / 256.0 - 128.0,
    ]
}

/// The perceptual black lcms reads (module doc): full ink's L*, made
/// neutral and clipped to 50, as XYZ.
pub fn source_black() -> [f64; 3] {
    let lstar = ink_lab([1.0; 4])[0].min(50.0);
    lab_to_xyz([lstar, 0.0, 0.0])
}

/// A matrix/TRC RGB profile's own DATA: its colorant columns and its tone
/// curve — a sampled `curv` through `common::icc_chain::Chain`, or a
/// count-1 `curv`, whose one u8Fixed8 entry is a gamma exponent (the
/// built-in Adobe RGB's 563/256), as a pure power written here.
struct RgbSide {
    to_pcs: [[f64; 3]; 3],
    gamma: Option<f64>,
    table: Option<Chain>,
}

impl RgbSide {
    fn of(profile: &IccProfile) -> Self {
        let bytes = profile.bytes();
        let trc = tag(bytes, b"rTRC");
        assert_eq!(&trc[..4], b"curv", "a matrix/TRC profile with a curv TRC");
        if be32(trc, 8) != 1 {
            let chain = Chain::of(profile);
            return Self {
                to_pcs: chain.to_pcs,
                gamma: None,
                table: Some(chain),
            };
        }
        let columns: [[f64; 3]; 3] = [b"rXYZ", b"gXYZ", b"bXYZ"].map(|key| {
            let data = tag(bytes, key);
            std::array::from_fn(|r| f64::from(be32(data, 8 + r * 4) as i32) / 65536.0)
        });
        Self {
            to_pcs: std::array::from_fn(|r| std::array::from_fn(|c| columns[c][r])),
            gamma: Some(f64::from(u16::from_be_bytes([trc[12], trc[13]])) / 256.0),
            table: None,
        }
    }

    fn decode(&self, x: f64) -> f64 {
        match (&self.table, self.gamma) {
            (Some(chain), _) => chain.decode(x),
            (None, Some(g)) => x.clamp(0.0, 1.0).powf(g),
            (None, None) => unreachable!(),
        }
    }

    fn encode(&self, y: f64) -> f64 {
        match (&self.table, self.gamma) {
            (Some(chain), _) => chain.encode(y),
            (None, Some(g)) => y.clamp(0.0, 1.0).powf(1.0 / g),
            (None, None) => unreachable!(),
        }
    }
}

/// Ink → encoded RGB of the matrix/TRC `profile` through its own tags,
/// relative colorimetric, with the ISO 18619 BPC affine (source black → the
/// matrix profile's zero black) when `bpc`.
pub fn ink_to_rgb_in(ink: [f64; 4], profile: &IccProfile, bpc: bool) -> [f64; 3] {
    let mut xyz = lab_to_xyz(ink_lab(ink));
    if bpc {
        let s = source_black();
        for i in 0..3 {
            let a = D50[i] / (D50[i] - s[i]);
            let b = -D50[i] * s[i] / (D50[i] - s[i]);
            xyz[i] = a * xyz[i] + b;
        }
    }
    let side = RgbSide::of(profile);
    let linear = apply(inverse(side.to_pcs), xyz);
    linear.map(|v| side.encode(v))
}

/// Ink → encoded sRGB through the built-in sRGB's own tags, relative
/// colorimetric, with the ISO 18619 BPC affine (source black → sRGB's zero
/// black) when `bpc`.
pub fn ink_to_srgb(ink: [f64; 4], bpc: bool) -> [f64; 3] {
    ink_to_rgb_in(ink, &IccProfile::srgb(), bpc)
}

/// Encoded sRGB → ink in [0, 1] through `B2A1`, relative, no BPC.
pub fn srgb_to_ink(rgb: [f64; 3]) -> [f64; 4] {
    srgb_to_ink_bpc(rgb, false)
}

/// The black lcms's `cmsDetectDestinationBlackPoint` finds for the press
/// at relative colorimetric, derived from the tables: the L* round trip
/// `B2A1` → `A2B1` is affine (every table is), L*' ≈ 60 + 0.355·L*, so it
/// fails the "nearly straight midrange" test (|L* − L*'| ≥ 4 above the
/// shadow), and the shadow fit over the normalized ramp — which an affine
/// ramp makes exactly L* / 100 — has its root at L* = 0: zero black.
pub fn destination_black() -> [f64; 3] {
    [0.0, 0.0, 0.0]
}

/// Encoded sRGB → ink in [0, 1] through `B2A1`, relative, with the ISO
/// 18619 BPC affine (sRGB's zero black → `destination_black`) when `bpc`.
pub fn srgb_to_ink_bpc(rgb: [f64; 3], bpc: bool) -> [f64; 4] {
    rgb_to_ink_in(rgb, &IccProfile::srgb(), bpc)
}

/// Encoded RGB of the matrix/TRC `profile` → ink in [0, 1] through `B2A1`,
/// relative, with the ISO 18619 BPC affine (the matrix profile's zero black
/// → `destination_black`) when `bpc`.
pub fn rgb_to_ink_in(rgb: [f64; 3], profile: &IccProfile, bpc: bool) -> [f64; 4] {
    let side = RgbSide::of(profile);
    let mut xyz = apply(side.to_pcs, rgb.map(|v| side.decode(v)));
    if bpc {
        let d = destination_black();
        for i in 0..3 {
            xyz[i] = (D50[i] - d[i]) / D50[i] * xyz[i] + d[i];
        }
    }
    let lab = xyz_to_lab(xyz);
    let encoded = [
        (lab[0] / 100.0 * 65280.0 / 65535.0).clamp(0.0, 1.0),
        ((lab[1] + 128.0) * 256.0 / 65535.0).clamp(0.0, 1.0),
        ((lab[2] + 128.0) * 256.0 / 65535.0).clamp(0.0, 1.0),
    ];
    b2a1_codes(encoded).map(|v| v / 65535.0)
}
