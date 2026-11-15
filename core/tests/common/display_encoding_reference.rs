//! The display encoding of a 32-bit float document under the built-in sRGB
//! profile, restated for the float oracles (`display_encoding`'s E and D):
//! the profile's 1024-entry `curv` table read by linear interpolation, and
//! the logarithm above white at the table's slope at white.

/// The IEC 61966-2.1 electro-optical transfer function.
fn srgb(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// The built-in sRGB profile's 1024 u16 `curv` samples, over 65535.
fn table() -> &'static [f64] {
    static TABLE: std::sync::OnceLock<Vec<f64>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        (0..1024)
            .map(|i| (srgb(f64::from(i) / 1023.0) * 65535.0).round() / 65535.0)
            .collect()
    })
}

/// The profile's decode on 0…1: the table, linearly interpolated.
pub(crate) fn eotf(x: f64) -> f64 {
    let t = table();
    let pos = x.clamp(0.0, 1.0) * 1023.0;
    let i = (pos.floor() as usize).min(1022);
    let f = pos - i as f64;
    t[i] * (1.0 - f) + t[i + 1] * f
}

/// The inverse of [`eotf`] on 0…1: the segment holding `v`, inverted.
fn oetf(v: f64) -> f64 {
    let t = table();
    if v <= 0.0 {
        return 0.0;
    }
    let i = t[1..].partition_point(|x| *x < v).min(1022);
    (i as f64 + (v - t[i]) / (t[i + 1] - t[i])).min(1023.0) / 1023.0
}

/// The logarithm's scale above white: the curve's slope at 1, its last
/// segment's `Δcode / Δlight`.
fn shoulder() -> f64 {
    let t = table();
    (1.0 / 1023.0) / (t[1023] - t[1022])
}

/// `E(v)`: the profile's curve inverted to 1, `1 + s·ln v` above, odd.
pub(crate) fn encode(v: f64) -> f64 {
    let u = v.abs();
    let e = if u <= 1.0 {
        oetf(u)
    } else {
        1.0 + shoulder() * u.ln()
    };
    e.copysign(v)
}

/// The inverse of [`encode`], inside ±65504.
pub(crate) fn decode(f: f64) -> f64 {
    let u = f.abs();
    let l = if u <= 1.0 {
        eotf(u)
    } else {
        ((u - 1.0) / shoulder()).exp()
    };
    l.min(65504.0).copysign(f)
}
