//! The CIE oracles of the Lab Color tests (#1), written here from the
//! formulas, never from the code under test: the ICC PCS white (D50), the
//! published Bradford-adapted sRGB and Display P3 → XYZ (D50) matrices, the
//! IEC 61966-2-1 curve, CIE L\*a\*b\* both ways, and the ICC v4 16-bit Lab
//! decoding. `lab_convert.rs`, `lab_structure.rs` and `lab_save.rs` share
//! them; since #1 phase 2d also ProPhoto RGB both ways (Kodak's colorants,
//! the pure 461/256 power) for the colour ops' oracles.

use crate::gray_fixture::{srgb_decode, srgb_encode};

/// The ICC PCS white (D50).
pub const WHITE: [f64; 3] = [0.9642, 1.0, 0.8249];

/// The Bradford-adapted sRGB → XYZ (D50) matrix, row-major, as the sRGB
/// profile publishes it (s15Fixed16 `rXYZ`, `gXYZ`, `bXYZ`; Apple's sRGB
/// profile and this build's built-in). The four-decimal textbook matrix
/// differs by up to 5e-5, which the inverse amplifies to ~40/65535 in a
/// dark channel beside bright ones — the oracle must be the profile's.
pub const SRGB_TO_XYZ: [[f64; 3]; 3] = [
    [
        0x6FA0 as f64 / 65536.0,
        0x6297 as f64 / 65536.0,
        0x249F as f64 / 65536.0,
    ],
    [
        0x38F5 as f64 / 65536.0,
        0xB787 as f64 / 65536.0,
        0x0F84 as f64 / 65536.0,
    ],
    [
        0x0390 as f64 / 65536.0,
        0x18D9 as f64 / 65536.0,
        0xB6C3 as f64 / 65536.0,
    ],
];

/// The CIE `f` of L\*a\*b\*.
pub fn f(t: f64) -> f64 {
    let e = 216.0 / 24389.0;
    if t > e {
        t.cbrt()
    } else {
        (24389.0 / 27.0 * t + 16.0) / 116.0
    }
}

pub fn f_inverse(t: f64) -> f64 {
    let cube = t * t * t;
    if cube > 216.0 / 24389.0 {
        cube
    } else {
        (116.0 * t - 16.0) * 27.0 / 24389.0
    }
}

pub fn xyz_to_lab(xyz: [f64; 3]) -> [f64; 3] {
    let (fx, fy, fz) = (
        f(xyz[0] / WHITE[0]),
        f(xyz[1] / WHITE[1]),
        f(xyz[2] / WHITE[2]),
    );
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// The hand-computed L\*a\*b\* of an sRGB colour in unit codes.
pub fn srgb_to_lab(rgb: [f64; 3]) -> [f64; 3] {
    let linear = rgb.map(srgb_decode);
    let xyz = SRGB_TO_XYZ.map(|row| (0..3).map(|c| row[c] * linear[c]).sum::<f64>());
    xyz_to_lab(xyz)
}

/// Display P3 → XYZ (D50), row-major, as Apple's `Display P3.icc`
/// publishes it (s15Fixed16 `rXYZ`, `gXYZ`, `bXYZ`; the red Z is −69 raw).
pub const P3_TO_XYZ: [[f64; 3]; 3] = [
    [
        0x83DF as f64 / 65536.0,
        0x4ABF as f64 / 65536.0,
        0x2838 as f64 / 65536.0,
    ],
    [
        0x3DBF as f64 / 65536.0,
        0xB137 as f64 / 65536.0,
        0x110B as f64 / 65536.0,
    ],
    [
        -69.0 / 65536.0,
        0x0AB9 as f64 / 65536.0,
        0xC8B9 as f64 / 65536.0,
    ],
];

/// The hand-computed L\*a\*b\* of a Display P3 colour in unit codes (the
/// sRGB curve, `P3_TO_XYZ`).
pub fn p3_to_lab(rgb: [f64; 3]) -> [f64; 3] {
    let linear = rgb.map(srgb_decode);
    let xyz = P3_TO_XYZ.map(|row| (0..3).map(|c| row[c] * linear[c]).sum::<f64>());
    xyz_to_lab(xyz)
}

/// The hand-computed sRGB (unit, unclamped) of an L\*a\*b\* colour.
pub fn lab_to_srgb(lab: [f64; 3]) -> [f64; 3] {
    lab_to_rgb(lab, SRGB_TO_XYZ)
}

/// The hand-computed RGB (unit, unclamped) of an L\*a\*b\* colour in the
/// matrix/IEC-curve space whose RGB → XYZ (D50) matrix is `m` (sRGB and
/// Display P3 share the curve).
pub fn lab_to_rgb(lab: [f64; 3], m: [[f64; 3]; 3]) -> [f64; 3] {
    lab_to_linear(lab, m).map(srgb_encode)
}

/// The LINEAR RGB (unclamped) of an L\*a\*b\* colour in the matrix space
/// whose RGB → XYZ (D50) matrix is `m`.
pub fn lab_to_linear(lab: [f64; 3], m: [[f64; 3]; 3]) -> [f64; 3] {
    let fy = (lab[0] + 16.0) / 116.0;
    let xyz = [
        f_inverse(fy + lab[1] / 500.0) * WHITE[0],
        f_inverse(fy) * WHITE[1],
        f_inverse(fy - lab[2] / 200.0) * WHITE[2],
    ];
    // The inverse of `m` by cofactors.
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let inv = [
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) / det,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) / det,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) / det,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) / det,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) / det,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) / det,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) / det,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) / det,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) / det,
        ],
    ];
    inv.map(|row| (0..3).map(|c| row[c] * xyz[c]).sum::<f64>())
}

/// ProPhoto RGB → XYZ (D50), row-major, as Kodak's `ProPhoto.icm` publishes
/// it (s15Fixed16 `rXYZ`, `gXYZ`, `bXYZ`; its white is D50, so unadapted).
pub const PROPHOTO_TO_XYZ: [[f64; 3]; 3] = [
    [
        0xCC34 as f64 / 65536.0,
        0x229C as f64 / 65536.0,
        0x0806 as f64 / 65536.0,
    ],
    [
        0x49BD as f64 / 65536.0,
        0xB63E as f64 / 65536.0,
        0x0006 as f64 / 65536.0,
    ],
    [0.0, 0.0, 0xD32D as f64 / 65536.0],
];

/// ProPhoto RGB's curve: the pure 461/256 power (`ProPhoto.icm`'s `curv`),
/// odd-extended so an out-of-range component stays unclamped.
const PROPHOTO_GAMMA: f64 = 461.0 / 256.0;

/// The hand-computed ENCODED ProPhoto RGB (unit, unclamped) of an
/// L\*a\*b\* colour.
pub fn lab_to_prophoto(lab: [f64; 3]) -> [f64; 3] {
    lab_to_linear(lab, PROPHOTO_TO_XYZ).map(|v| v.signum() * v.abs().powf(1.0 / PROPHOTO_GAMMA))
}

/// The hand-computed L\*a\*b\* of an ENCODED ProPhoto RGB colour (unit,
/// unclamped).
pub fn prophoto_to_lab(rgb: [f64; 3]) -> [f64; 3] {
    let linear = rgb.map(|v| v.signum() * v.abs().powf(PROPHOTO_GAMMA));
    let xyz = PROPHOTO_TO_XYZ.map(|row| (0..3).map(|c| row[c] * linear[c]).sum::<f64>());
    xyz_to_lab(xyz)
}

/// The 8-bit `#LLaabb` of an L\*a\*b\* colour's Lab codes: round(255·L\*/100),
/// round(a\* + 128), round(b\* + 128), each clamped to 0…255.
pub fn lab_code_hex(lab: [f64; 3]) -> String {
    let byte = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    let codes = [
        byte(255.0 * lab[0] / 100.0),
        byte(lab[1] + 128.0),
        byte(lab[2] + 128.0),
    ];
    format!("#{:02x}{:02x}{:02x}", codes[0], codes[1], codes[2])
}

/// A u16 Lab sample as L\*, a\*, b\* (the ICC v4 encoding).
pub fn decode(sample: [u16; 4]) -> [f64; 3] {
    let v = |c: u16| f64::from(c) / 65535.0;
    [
        v(sample[0]) * 100.0,
        v(sample[1]) * 255.0 - 128.0,
        v(sample[2]) * 255.0 - 128.0,
    ]
}

pub fn grid(alpha: u16) -> u16 {
    ((u32::from(alpha) + 128) / 257 * 257) as u16
}
