//! The Vibrance stage's oracle (#2), shared by every test that renders the
//! slider: member V3t-P of `scripts/photoshop-probe/adjust_arith_578_take.py`,
//! restated in binary64 from that runner's docstring and read from the
//! runner's own record of the fit, copied byte for byte to
//! `tests/fixtures/vibrance_578/adjust_arith_578_fitted.json`, never from the
//! core's tables or code.
//!
//! On LINEAR light `l` with maximum `M`, minimum `m` and chroma `S = (M −
//! m)/M`: `c' = clamp01(A + (c − A)·K)`, `A = M·(A/M)`. Per sign of the slider
//! `v` (a fraction), `K − 1` and `A/M − 1` are tables on the knots `S ∈ {0,
//! 1/8, …, 1}` × `√M ∈ {0, 1/4, …, 1}`, bilinear in `S` and `√M`, at `|v| ∈
//! {10, 25, 50, 75, 100}` (Photoshop's units), linear in `|v|` between them
//! and the identity at 0; above 0 both deviations are scaled by `p = 1 −
//! β·max(0, 1 − d(h, h0)/hw)` on the linear hexcone hue `h` (1 on a grey);
//! `K` is clamped at 0. A slider at 0 is the identity.
#![allow(dead_code)]

use std::path::Path;
use std::sync::OnceLock;

/// One sign's two tables, flat in (v-knot, S-knot, √M-knot) order.
struct Sign {
    gain: Vec<f64>,
    share: Vec<f64>,
}

struct Law {
    down: Sign,
    up: Sign,
    skin: [f64; 3],
}

const V_KNOTS: [f64; 5] = [10.0, 25.0, 50.0, 75.0, 100.0];

fn law() -> &'static Law {
    static LAW: OnceLock<Law> = OnceLock::new();
    LAW.get_or_init(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/vibrance_578/adjust_arith_578_fitted.json");
        let fitted: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("the runner's record"))
                .expect("JSON");
        let member = fitted["V3t"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["name"] == "V3t-P")
            .expect("V3t-P");
        let flat = |v: &serde_json::Value| -> Vec<f64> {
            let out: Vec<f64> = v
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|plane| plane.as_array().unwrap().iter())
                .flat_map(|row| row.as_array().unwrap().iter())
                .map(|c| c.as_f64().unwrap())
                .collect();
            assert_eq!(out.len(), 5 * 9 * 5);
            out
        };
        let sign = |key: &str| Sign {
            gain: flat(&member[key]["K"]),
            share: flat(&member[key]["A"]),
        };
        let skin = member["skin"].as_array().unwrap();
        Law {
            down: sign("neg"),
            up: sign("pos"),
            skin: [0, 1, 2].map(|i| skin[i].as_f64().unwrap()),
        }
    })
}

/// The hexcone hue of `x` in degrees, `None` on a grey.
pub fn hexcone_hue(x: [f64; 3]) -> Option<f64> {
    let (hi, lo) = (x[0].max(x[1]).max(x[2]), x[0].min(x[1]).min(x[2]));
    let d = hi - lo;
    if d <= 0.0 {
        return None;
    }
    let h = if hi == x[0] {
        ((x[1] - x[2]) / d).rem_euclid(6.0)
    } else if hi == x[1] {
        (x[2] - x[0]) / d + 2.0
    } else {
        (x[0] - x[1]) / d + 4.0
    };
    Some((60.0 * h).rem_euclid(360.0))
}

/// One table read bilinearly at (`s`, `big`) on v-knot `knot`.
fn read(table: &[f64], knot: usize, s: f64, big: f64) -> f64 {
    let fs = s.clamp(0.0, 1.0) * 8.0;
    let fr = big.clamp(0.0, 1.0).sqrt() * 4.0;
    let (s0, r0) = ((fs.floor() as usize).min(7), (fr.floor() as usize).min(3));
    let (ts, tr) = (fs - s0 as f64, fr - r0 as f64);
    let at = |i: usize, j: usize| table[knot * 45 + i * 5 + j];
    at(s0, r0) * (1.0 - ts) * (1.0 - tr)
        + at(s0, r0 + 1) * (1.0 - ts) * tr
        + at(s0 + 1, r0) * ts * (1.0 - tr)
        + at(s0 + 1, r0 + 1) * ts * tr
}

/// `(K, A/M)` of slider `v` (a fraction) on the linear pixel `l`.
pub fn gain_and_share(v: f64, l: [f64; 3]) -> (f64, f64) {
    if v == 0.0 {
        return (1.0, 1.0);
    }
    let law = law();
    let (hi, lo) = (l[0].max(l[1]).max(l[2]), l[0].min(l[1]).min(l[2]));
    let s = if hi > 0.0 { (hi - lo) / hi } else { 0.0 };
    let sign = if v < 0.0 { &law.down } else { &law.up };
    let slider = (v.abs() * 100.0).min(100.0);
    let (mut dk, mut da) = (0.0, 0.0);
    let mut lo_knot = 0.0;
    for (i, &hi_knot) in V_KNOTS.iter().enumerate() {
        if slider <= hi_knot || i == V_KNOTS.len() - 1 {
            let f = ((slider - lo_knot) / (hi_knot - lo_knot)).min(1.0);
            dk += f * read(&sign.gain, i, s, hi);
            da += f * read(&sign.share, i, s, hi);
            if i > 0 {
                dk += (1.0 - f) * read(&sign.gain, i - 1, s, hi);
                da += (1.0 - f) * read(&sign.share, i - 1, s, hi);
            }
            break;
        }
        lo_knot = hi_knot;
    }
    let p = if v > 0.0 {
        let [beta, h0, hw] = law.skin;
        hexcone_hue(l).map_or(1.0, |h| {
            let d = ((h - h0 + 180.0).rem_euclid(360.0) - 180.0).abs();
            1.0 - beta * (1.0 - d / hw).max(0.0)
        })
    } else {
        1.0
    };
    ((1.0 + p * dk).max(0.0), 1.0 + p * da)
}

/// The Vibrance stage on LINEAR light (each component clamped to [0, 1]
/// first), slider `v` a fraction.
pub fn stage(l: [f64; 3], v: f64) -> [f64; 3] {
    if v == 0.0 {
        return l;
    }
    let l = l.map(|c| c.clamp(0.0, 1.0));
    let (k, share) = gain_and_share(v, l);
    let a = share * l[0].max(l[1]).max(l[2]);
    l.map(|c| (a + (c - a) * k).clamp(0.0, 1.0))
}

/// A profile's red TRC as its `curv` tag reads (restated from
/// `adjust_vibrance_posterize_parity.rs`'s `Curve`): a gamma, or a table
/// interpolated linearly and inverted by search. The stage runs in this
/// light on a document of that profile.
pub enum Trc {
    Gamma(f64),
    Table(Vec<f64>),
}

impl Trc {
    pub fn of(bytes: &[u8]) -> Trc {
        let be32 = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
        let tag = (0..be32(128) as usize)
            .map(|n| 132 + n * 12)
            .find(|&entry| &bytes[entry..entry + 4] == b"rTRC")
            .map(|entry| &bytes[be32(entry + 4) as usize..][..be32(entry + 8) as usize])
            .expect("an rTRC tag");
        assert_eq!(&tag[..4], b"curv");
        let n = u32::from_be_bytes(tag[8..12].try_into().unwrap()) as usize;
        let u16_at = |i: usize| f64::from(u16::from_be_bytes([tag[12 + 2 * i], tag[13 + 2 * i]]));
        match n {
            0 => Trc::Gamma(1.0),
            1 => Trc::Gamma(u16_at(0) / 256.0),
            _ => Trc::Table((0..n).map(|i| u16_at(i) / 65535.0).collect()),
        }
    }

    pub fn decode(&self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Trc::Gamma(g) => x.powf(*g),
            Trc::Table(t) => {
                let pos = x * (t.len() - 1) as f64;
                let i = (pos.floor() as usize).min(t.len() - 2);
                let f = pos - i as f64;
                t[i] * (1.0 - f) + t[i + 1] * f
            }
        }
    }

    pub fn encode(&self, y: f64) -> f64 {
        let y = y.clamp(0.0, 1.0);
        match self {
            Trc::Gamma(g) => y.powf(1.0 / g),
            Trc::Table(t) if y <= t[0] => 0.0,
            Trc::Table(t) if y >= t[t.len() - 1] => 1.0,
            Trc::Table(t) => {
                let upper = t.iter().position(|&s| s >= y).unwrap();
                let lower = upper - 1;
                let f = (y - t[lower]) / (t[upper] - t[lower]);
                (lower as f64 + f) / (t.len() - 1) as f64
            }
        }
    }

    /// The stage (slider `v`, a fraction) on one ENCODED triple through
    /// this curve.
    pub fn vibrance(&self, rgb: [f64; 3], v: f64) -> [f64; 3] {
        stage(rgb.map(|c| self.decode(c)), v).map(|c| self.encode(c))
    }
}
