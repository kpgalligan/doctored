//! An independent Display P3 ↔ sRGB oracle: each space's RGB → XYZ
//! matrix built from its published primaries under the D65 white (both
//! spaces share it, so no adaptation enters), and the sRGB transfer curve
//! Display P3 shares. Nothing here reads the core's profiles.

const SRGB_PRIMARIES: [[f64; 2]; 3] = [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06]];
const P3_PRIMARIES: [[f64; 2]; 3] = [[0.680, 0.320], [0.265, 0.690], [0.150, 0.060]];

/// The RGB → XYZ matrix of `primaries` (xy) under the D65 white.
fn to_xyz(primaries: [[f64; 2]; 3]) -> [[f64; 3]; 3] {
    let white = [0.3127 / 0.3290, 1.0, (1.0 - 0.3127 - 0.3290) / 0.3290];
    let p: [[f64; 3]; 3] = std::array::from_fn(|row| {
        std::array::from_fn(|c| {
            let [x, y] = primaries[c];
            [x / y, 1.0, (1.0 - x - y) / y][row]
        })
    });
    let s = mul_vec(inverse(p), white);
    std::array::from_fn(|row| std::array::from_fn(|c| p[row][c] * s[c]))
}

fn inverse(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let c = |r: usize, k: usize| {
        let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
        let (k1, k2) = ((k + 1) % 3, (k + 2) % 3);
        m[r1][k1] * m[r2][k2] - m[r1][k2] * m[r2][k1]
    };
    let det: f64 = (0..3).map(|k| m[0][k] * c(0, k)).sum();
    std::array::from_fn(|r| std::array::from_fn(|k| c(k, r) / det))
}

fn mul(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
}

fn mul_vec(a: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|r| (0..3).map(|k| a[r][k] * v[k]).sum())
}

/// The sRGB EOTF (Display P3's too).
pub fn decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// The sRGB OETF (Display P3's too).
pub fn encode(v: f64) -> f64 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// Display P3 numbers as linear sRGB components (extended range).
pub fn p3_to_linear_srgb(numbers: [f64; 3]) -> [f64; 3] {
    let m = mul(inverse(to_xyz(SRGB_PRIMARIES)), to_xyz(P3_PRIMARIES));
    mul_vec(m, numbers.map(decode))
}

/// Linear sRGB components as linear Display P3 components.
pub fn linear_srgb_to_linear_p3(linear: [f64; 3]) -> [f64; 3] {
    let m = mul(inverse(to_xyz(P3_PRIMARIES)), to_xyz(SRGB_PRIMARIES));
    mul_vec(m, linear)
}
