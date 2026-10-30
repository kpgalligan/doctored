//! Black & White's 8-bit Tint oracle (#578 phase 4), member h8a-LE of
//! `scripts/photoshop-probe/eight_bit_578_take.py`, restated from
//! `psd_appearance_578_take.py`'s `gray_even` and `h8_member(new_l=True)`,
//! never from the core:
//!
//! * the grey on the CODES: hi, mid, lo by value (ties to the lower channel),
//!   `g = lo + (hi − mid)·W[2·hi] + (mid − lo)·W[pair]` (pair R+G yellows, G+B
//!   cyans, else magentas) with `W` = percent/100 as an exact rational,
//!   clamped to [0, 255] and rounded half to even;
//! * `L8 = floor((77 N_r + 150 N_g + 28 N_b + 128) / 256)`, `c = g + N − L8`,
//!   kept when every component is in 0..=255; otherwise, with `l = g`, a
//!   minimum `n < 0` maps `c → l + (c − l)·l/(l − n)`, then a maximum `x >
//!   255` (and `> l`) maps `c → l + (c − l)·(255 − l)/(x − l)`, each
//!   component is rounded half up, gains one code (at most up to `g`) when
//!   below `l`, and is clamped to 0..=255.
#![allow(dead_code)]

/// The h8a-LE codes of a pixel's `codes` under weights in whole `percents`
/// (reds, yellows, greens, cyans, blues, magentas) and a tint's `numbers`.
pub fn h8a_le(codes: [u8; 3], percents: [i32; 6], numbers: [u8; 3]) -> [u8; 3] {
    let g = gray(codes, percents);
    let n = numbers.map(i64::from);
    let big_l = (77 * n[0] + 150 * n[1] + 28 * n[2] + 128).div_euclid(256);
    let c: Vec<i64> = n.iter().map(|k| g + k - big_l).collect();
    if c.iter().all(|k| (0..=255).contains(k)) {
        return [c[0] as u8, c[1] as u8, c[2] as u8];
    }
    let l = g as f64;
    let mut c: Vec<f64> = c.iter().map(|&k| k as f64).collect();
    let lowest = c.iter().cloned().fold(f64::INFINITY, f64::min);
    if lowest < 0.0 {
        c = c.iter().map(|&k| l + (k - l) * l / (l - lowest)).collect();
    }
    let highest = c.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if highest > 255.0 && highest > l {
        c = c
            .iter()
            .map(|&k| l + (k - l) * (255.0 - l) / (highest - l))
            .collect();
    }
    let out: Vec<u8> = c
        .iter()
        .map(|&k| {
            let mut q = (k + 0.5).floor() as i64;
            if k < l {
                q = (q + 1).min(g);
            }
            q.clamp(0, 255) as u8
        })
        .collect();
    [out[0], out[1], out[2]]
}

/// The grey in whole codes: `100·g` exactly as an integer, clamped, then
/// halved to even.
fn gray(codes: [u8; 3], percents: [i32; 6]) -> i64 {
    let v = codes.map(i64::from);
    let mut order = [0usize, 1, 2];
    order.sort_by(|&a, &b| v[b].cmp(&v[a]).then(a.cmp(&b)));
    let [hi, mid, lo] = order;
    let pair = match (hi.min(mid), hi.max(mid)) {
        (0, 1) => 1,
        (1, 2) => 3,
        _ => 5,
    };
    let w = percents.map(i64::from);
    let hundred = 100 * v[lo] + (v[hi] - v[mid]) * w[2 * hi] + (v[mid] - v[lo]) * w[pair];
    let hundred = hundred.clamp(0, 25500);
    let (q, r) = (hundred / 100, hundred % 100);
    if 2 * r > 100 || (2 * r == 100 && q % 2 == 1) {
        q + 1
    } else {
        q
    }
}
