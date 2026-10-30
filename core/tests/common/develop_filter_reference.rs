//! Develop Filter's reference: each stage restated in binary64 from
//! the test oracle of the op the filter calls, never from production code,
//! and composed in the filter's documented order. Presence has no closed form
//! here: the tests compare it against Filters ▸ Presence itself, or use a
//! flat picture, which Presence leaves unchanged (`filter_presence.rs`).
#![allow(dead_code)]

use crate::common::white_balance_oracle::white_balance_oracle;
use crate::vibrance_reference;

/// The thirteen controls, in the filter's units (Exposure in stops, the rest
/// −100…100).
#[derive(Clone, Copy, Debug, Default)]
pub struct Controls {
    pub temperature: f64,
    pub tint: f64,
    pub exposure: f64,
    pub contrast: f64,
    pub highlights: f64,
    pub shadows: f64,
    pub whites: f64,
    pub blacks: f64,
    pub texture: f64,
    pub clarity: f64,
    pub dehaze: f64,
    pub vibrance: f64,
    pub saturation: f64,
}

impl Controls {
    /// The controls a `develop_filter` params object sets; absent keys are 0.
    pub fn of(params: &serde_json::Value) -> Controls {
        let at = |key: &str| params.get(key).and_then(|v| v.as_f64()).unwrap_or(0.0);
        Controls {
            temperature: at("temperature"),
            tint: at("tint"),
            exposure: at("exposure"),
            contrast: at("contrast"),
            highlights: at("highlights"),
            shadows: at("shadows"),
            whites: at("whites"),
            blacks: at("blacks"),
            texture: at("texture"),
            clarity: at("clarity"),
            dehaze: at("dehaze"),
            vibrance: at("vibrance"),
            saturation: at("saturation"),
        }
    }
}

// ---- per-op oracles, each restated from the test file it names ---------------

/// `adjust_tone_tests.rs`'s `s2l`.
pub fn s2l(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// `adjust_tone_tests.rs`'s `l2s`.
pub fn l2s(u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    if u <= 0.0031308 {
        12.92 * u
    } else {
        1.055 * u.powf(1.0 / 2.4) - 0.055
    }
}

/// Rec. 709 luma, as every oracle file writes it.
pub fn luma(rgb: [f64; 3]) -> f64 {
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
}

/// `adjust_tone_tests.rs`'s `exposure_oracle` with offset 0 and gamma 1.
pub fn exposure(v: f64, stops: f64) -> f64 {
    l2s((s2l(v) * 2f64.powf(stops)).max(0.0))
}

/// `adjust_tone_tests.rs`'s `shadows_oracle` at tones 0.5, Color 0 and no
/// midtone contrast, for the neighbourhood estimate `b`.
pub fn shadows_highlights(rgb: [f64; 3], b: f64, a_s: f64, a_h: f64) -> [f64; 3] {
    let (t_s, t_h) = (0.5, 0.5);
    let l = luma(rgb);
    let b = b.clamp(0.0, 1.0);
    let w_s = (1.0 - b / t_s).clamp(0.0, 1.0).powi(2);
    let w_h = (1.0 - (1.0 - b) / t_h).clamp(0.0, 1.0).powi(2);
    let e = (1.0 + a_h * w_h) / (1.0 + a_s * w_s);
    let lifted = if l > 0.0 { l.powf(e) } else { 0.0 };
    let k = if l > 0.0 { lifted / l } else { 1.0 };
    rgb.map(|c| (lifted + (c * k - lifted)).clamp(0.0, 1.0))
}

/// `adjust_bcs_modern_tests.rs`'s `modern` with brightness and saturation 0:
/// the S-curve about 0.5 at power `2^(1.5·c)`, `c` in −1…1.
pub fn contrast(rgb: [f64; 3], c: f64) -> [f64; 3] {
    let k = (1.5_f64 * c).exp2();
    rgb.map(|y| {
        if c == 0.0 {
            y
        } else if y < 0.5 {
            0.5 * (2.0 * y).powf(k)
        } else {
            1.0 - 0.5 * (2.0 * (1.0 - y)).powf(k)
        }
    })
}

/// `adjust_levels.rs`'s `stage`: (black, white, gamma, output_black,
/// output_white) on one component.
pub fn levels_stage(s: (f64, f64, f64, f64, f64), v: f64) -> f64 {
    let (black, white, gamma, ob, ow) = s;
    let t = ((v - black) / (white - black))
        .clamp(0.0, 1.0)
        .powf(1.0 / gamma);
    ob + (ow - ob) * t
}

/// The Vibrance adjustment's unbound form (#2, `adjust_vibrance_posterize_parity`'s
/// oracle with the IEC sRGB curve): `v` and `saturation` in −1…1, each stage
/// `c' = clamp01(A + (c − A)·K)` on the DECODED values. Vibrance first, the
/// shipped law (`common/vibrance_reference.rs`'s `stage`: one anchor and one
/// gain from the runner's tables over the pixel's linear chroma and maximum),
/// then Saturation about the ROMM Y row with `K = 1 + saturation`. A slider at
/// 0 skips its stage.
pub fn vibrance(rgb: [f64; 3], v: f64, saturation: f64) -> [f64; 3] {
    if v == 0.0 && saturation == 0.0 {
        return rgb;
    }
    let l = vibrance_reference::stage(rgb.map(s2l), v);
    if saturation == 0.0 {
        return l.map(l2s);
    }
    let a = 0.288040 * l[0] + 0.711874 * l[1] + 0.000086 * l[2];
    l.map(|c| (a + (c - a) * (1.0 + saturation).max(0.0)).clamp(0.0, 1.0))
        .map(l2s)
}

// ---- the filter's mappings, restated from its header row ----------------------

/// The kelvin the white-balance op reads for a relative Temperature `t`: one
/// mired a unit away from 6504 K, exactly 6504 at 0.
pub fn kelvin(t: f64) -> f64 {
    if t == 0.0 {
        6504.0
    } else {
        1.0e6 / (1.0e6 / 6504.0 - t)
    }
}

/// Stages 1–3: white balance, exposure, contrast.
pub fn front(rgb: [f64; 3], c: &Controls) -> [f64; 3] {
    let mut rgb = rgb;
    if c.temperature != 0.0 || c.tint != 0.0 {
        rgb = white_balance_oracle(rgb, kelvin(c.temperature), c.tint);
    }
    if c.exposure != 0.0 {
        rgb = rgb.map(|v| exposure(v, c.exposure));
    }
    if c.contrast != 0.0 {
        rgb = contrast(rgb, c.contrast / 100.0);
    }
    rgb
}

/// Stages 4 and 5 on stage 3's output, `guide` its neighbourhood's luma.
pub fn back(rgb: [f64; 3], guide: f64, c: &Controls) -> [f64; 3] {
    let mut rgb = rgb;
    if c.highlights != 0.0 || c.shadows != 0.0 {
        let a_s = if c.shadows < 0.0 {
            c.shadows / 200.0
        } else {
            c.shadows / 100.0
        };
        let a_h = if c.highlights > 0.0 {
            -c.highlights / 200.0
        } else {
            -c.highlights / 100.0
        };
        rgb = shadows_highlights(rgb, guide, a_s, a_h);
    }
    if c.whites != 0.0 || c.blacks != 0.0 {
        let (mut black, mut white, mut ob, mut ow) = (0.0, 1.0, 0.0, 1.0);
        if c.whites >= 0.0 {
            white = 1.0 - 0.2 * c.whites / 100.0;
        } else {
            ow = 1.0 + 0.2 * c.whites / 100.0;
        }
        if c.blacks <= 0.0 {
            black = -0.2 * c.blacks / 100.0;
        } else {
            ob = 0.2 * c.blacks / 100.0;
        }
        rgb = rgb.map(|v| levels_stage((black, white, 1.0, ob, ow), v));
    }
    rgb
}

/// Stage 7: vibrance and saturation.
pub fn finish(rgb: [f64; 3], c: &Controls) -> [f64; 3] {
    if c.vibrance == 0.0 && c.saturation == 0.0 {
        return rgb;
    }
    vibrance(rgb, c.vibrance / 100.0, c.saturation / 100.0)
}

/// Every stage but Presence on a pixel whose neighbourhood luma after stage
/// 3 is `guide`: right for a flat picture (where the guide is the pixel's own
/// stage-3 luma and Presence is the identity) and for settings without
/// Highlights, Shadows and Presence.
pub fn chain(rgb: [f64; 3], guide: f64, c: &Controls) -> [f64; 3] {
    finish(back(front(rgb, c), guide, c), c)
}

/// [`chain`] with the guide of a flat picture: the pixel's own stage-3 luma.
pub fn flat(rgb: [f64; 3], c: &Controls) -> [f64; 3] {
    let toned = front(rgb, c);
    finish(back(toned, luma(toned).clamp(0.0, 1.0), c), c)
}
