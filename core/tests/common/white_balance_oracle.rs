//! Independent published-formula white-balance oracle, shared by legacy and
//! calibrated tests; never calls production colour or adjustment math.
#![allow(dead_code)]

fn s2l(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn l2s(u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    if u <= 0.0031308 {
        12.92 * u
    } else {
        1.055 * u.powf(1.0 / 2.4) - 0.055
    }
}

pub const M_A: [[f64; 3]; 3] = [
    [0.8951, 0.2664, -0.1614],
    [-0.7502, 1.7135, 0.0367],
    [0.0389, -0.0685, 1.0296],
];
pub const M_A_INV: [[f64; 3]; 3] = [
    [0.9869929, -0.1470543, 0.1599627],
    [0.4323053, 0.5183603, 0.0492912],
    [-0.0085287, 0.0400428, 0.9684867],
];
pub const D50_XYZ: [f64; 3] = [0.96422, 1.0, 0.82521];
pub const D65_XYZ: [f64; 3] = [0.95047, 1.0, 1.08883];
pub const SRGB_TO_XYZ: [[f64; 3]; 3] = [
    [0.4124564, 0.3575761, 0.1804375],
    [0.2126729, 0.7151522, 0.0721750],
    [0.0193339, 0.1191920, 0.9503041],
];
pub const XYZ_TO_SRGB: [[f64; 3]; 3] = [
    [3.2404542, -1.5371385, -0.4985314],
    [-0.9692660, 1.8760108, 0.0415560],
    [0.0556434, -0.2040259, 1.0572252],
];

pub fn mat_vec(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    m.map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
}

pub fn mat_mul(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut out = [[0.0f64; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, entry) in row.iter_mut().enumerate() {
            *entry = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

/// Bradford adaptation from one white to another, both XYZ.
pub fn bradford(source: [f64; 3], dest: [f64; 3]) -> [[f64; 3]; 3] {
    let cs = mat_vec(M_A, source);
    let cd = mat_vec(M_A, dest);
    let mut diag = [[0.0f64; 3]; 3];
    for i in 0..3 {
        diag[i][i] = cd[i] / cs[i];
    }
    mat_mul(M_A_INV, mat_mul(diag, M_A))
}

/// CCT to chromaticity: the CIE D-series daylight cubics at and above
/// 4000 K, Kim et al.'s Planckian cubics below it.
pub fn locus_xy(t: f64) -> (f64, f64) {
    if t >= 4000.0 {
        let x = if t <= 7000.0 {
            -4.6070e9 / t.powi(3) + 2.9678e6 / (t * t) + 0.09911e3 / t + 0.244063
        } else {
            -2.0064e9 / t.powi(3) + 1.9018e6 / (t * t) + 0.24748e3 / t + 0.237040
        };
        (x, -3.0 * x * x + 2.87 * x - 0.275)
    } else {
        let x = -0.2661239e9 / t.powi(3) - 0.2343589e6 / (t * t) + 0.8776956e3 / t + 0.179910;
        let y = if t >= 2222.0 {
            -0.9549476 * x.powi(3) - 1.37418593 * x * x + 2.09137015 * x - 0.16748867
        } else {
            -1.1063814 * x.powi(3) - 1.34811020 * x * x + 2.18555832 * x - 0.20219683
        };
        (x, y)
    }
}

/// CIE xy to the CIE 1960 UCS uv.
pub fn xy_to_uv(x: f64, y: f64) -> (f64, f64) {
    let den = -2.0 * x + 12.0 * y + 3.0;
    (4.0 * x / den, 6.0 * y / den)
}

/// The unit locus tangent at `t` in uv, toward rising temperature. Taken by
/// a one-sided difference held inside `locus_xy`'s own branch, because the
/// Planckian and daylight fits do not meet at their 4000 K seam.
pub fn locus_tangent_uv(t: f64) -> (f64, f64) {
    let step = (t * 1e-3).max(0.5);
    let hi_bound = if t < 2222.0 {
        2222.0
    } else if t < 4000.0 {
        4000.0
    } else if t <= 7000.0 {
        7000.0
    } else {
        f64::MAX
    };
    let (lo, hi) = if hi_bound - t >= step {
        (t, t + step)
    } else {
        (t - step, t)
    };
    let (u0, v0) = xy_to_uv(locus_xy(lo).0, locus_xy(lo).1);
    let (u1, v1) = xy_to_uv(locus_xy(hi).0, locus_xy(hi).1);
    let (du, dv) = (u1 - u0, v1 - v0);
    let len = du.hypot(dv);
    (du / len, dv / len)
}

/// The illuminant a (temperature, tint) pair describes, as XYZ. A POSITIVE
/// tint is a GREENER source white, stepped `tint/3000` along the ISOTHERM —
/// the locus normal, whose green side is `(dv, -du)` for a tangent pointing
/// at rising temperature — which is why removing it pushes the picture
/// toward magenta without moving the white's CCT.
pub fn source_white(temperature: f64, tint: f64) -> [f64; 3] {
    let (u0, v0) = xy_to_uv(locus_xy(temperature).0, locus_xy(temperature).1);
    let (du, dv) = locus_tangent_uv(temperature);
    let k = tint / 3000.0;
    let u = u0 + k * dv;
    let v = v0 - k * du;
    let den2 = 2.0 * u - 8.0 * v + 4.0;
    let (x2, y2) = (3.0 * u / den2, 2.0 * v / den2);
    [x2 / y2, 1.0, (1.0 - x2 - y2) / y2]
}

/// White balance on one encoded triple: adapt the stated illuminant to D65.
pub fn white_balance_oracle(rgb: [f64; 3], temperature: f64, tint: f64) -> [f64; 3] {
    if temperature == 6504.0 && tint == 0.0 {
        return rgb;
    }
    linear_white_balance(rgb.map(s2l), temperature, tint).map(l2s)
}

/// The same independently written Bradford reference without an 8-bit sRGB
/// intermediate; calibrated profile equivalence needs its unclipped XYZ path.
pub fn linear_white_balance(rgb: [f64; 3], temperature: f64, tint: f64) -> [f64; 3] {
    if temperature == 6504.0 && tint == 0.0 {
        return rgb;
    }
    let m = mat_mul(
        XYZ_TO_SRGB,
        mat_mul(
            bradford(source_white(temperature, tint), D65_XYZ),
            SRGB_TO_XYZ,
        ),
    );
    mat_vec(m, rgb)
}
