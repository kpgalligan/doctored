//! Native color/scalar gradients share one bounded pixel-center field. The
//! byte gradient retains its established f32 arithmetic and rounding.
//!
//! Scalar gradients share the same canvas field with independent endpoint alpha.
//!
//! Stops gradients add three shapes over the same drag
//! (`styled_parameter`), a 2..32-stop ramp with a colour and an alpha per stop
//! (`stop_color`), and the ONE bracket search of a stops list (`segment`),
//! which the layer-style gradient also takes its bracket from. This module
//! imports nothing from the crate.
//!
//! # The Classic ramp
//!
//! The Classic gradient at Smoothness 100 % (its default) does not
//! interpolate linearly: between stops `k` and `k + 1`, at the segment
//! fraction `f`, the colour is the cubic Hermite through the two stops whose
//! tangents are `(v[k + 1] − v[k − 1]) / 2` in SEGMENT-FRACTION units, the
//! end stops repeated (a uniform Catmull-Rom; an end tangent is half its one
//! step) — [`classic_color`], the ONE implementation. On two stops it is
//! `S(f) = (f + 3f² − 2f³) / 2`, the glow's smoothed fraction
//! (`style_glow_gradient::GLOW_RAMP_SMOOTH`). Opacity stays linear.
//!
//! Why: `scripts/photoshop-probe/gradient_identify_673_fit.py` described one
//! identification take (`gradient_identify_673_take.py`, Photoshop 26.10.0,
//! 254 cases): two stops at 16 bits are `S` within 0.012 / 0.0035 codes, where
//! linear is 12.8 / 7.97 off; three stops are this spline within 1.0 / 0.035
//! codes at 8 bits, where `S` per segment is 19 / 9.8 and the spline with
//! tangents per unit `t` 2.0 / 1.10; Smoothness 0 % is exactly linear (no
//! model here holds a smoothness). It ships for Linear, Angle, Reflected and
//! Radial ([`FieldStyle::classic`],
//! `style_gradient::GradientSampler::color_with`) and for the Stroke's Shape
//! Burst over its own field (`style_stroke_fill`): a style is
//! one kernel, and only those styles' geometry is identified (the
//! controller's style split, its phase-2 ruling for Reflected, its take-2
//! RULING-1 for Radial and phase 3's `--judge-phase3` for the Shape Burst;
//! Diamond keeps the linear ramp and today's geometry until its is).
//!
//! # Smoothness
//!
//! Smoothness `sm` blends the linear ramp and the Classic one by
//! `k = sm / 100` ([`smooth_color`]): take 2's 20 Smoothness rows
//! (`gradient_identify_673_take2.py`, Linear, two and three stops, 0 to 100 %)
//! meet this law (K1) at 2 / 0.5 codes with nothing fitted, where a fitted
//! power `k = (sm / 100)^p` lands on p = 1.00. Opacity stays linear; a style
//! that is not [`FieldStyle::classic`] (Diamond) stays linear at any
//! Smoothness. The fill layers and the Gradient tool share it: the tool's
//! stops ramp ([`stop_color`], [`scalar_over`], `cmyk_paint`'s ink) takes the
//! same `k` (a fraction, [`validate_smoothness`], default
//! [`full_smoothness`]).
//!
//! # Midpoints
//!
//! Every colour stop and every opacity stop carries a midpoint
//! `m` (5..95 %, [`MIDPOINT_MIN`]..[`MIDPOINT_MAX`], default 50 %): the
//! fraction of the way to the NEXT stop where the ramp reaches half way.
//! Between stops `i` and `i + 1` the segment fraction `f` is remapped
//! piecewise-linearly through stop `i`'s midpoint before any ramp reads it
//! ([`midpoint_fraction`], the ONE remap): `f / (2m)` up to `m`, then `1/2 +
//! (f − m) / (2 (1 − m))`. Both the Classic Hermite and the linear term of
//! [`smooth_color`] read the remapped fraction; opacity reads its own
//! midpoint the same way ([`stop_color`]). At exactly 50 % the remap returns
//! `f` untouched, so a gradient whose midpoints are all 50 % takes today's
//! arithmetic bit for bit. The last stop's midpoint governs nothing.
//!
//! Why: `gradient_identify_673_fit.py --judge-midpoints` scored this form
//! (`pwl.pre`, no free number) on take 2's 10 midpoint rows (three stops,
//! md 10, 25, 50, 75 and 90, 8 and 16 bits): every row within 2 / 0.5
//! codes, worst 1.0 / 0.035, where ignoring the midpoints is 122 / 50.2 at
//! md 10. In the model stop `i`'s midpoint governs segment `i..i + 1`.
//! Photoshop's file carries it on the NEXT stop: segment `i` reads stop
//! `i + 1`'s `Mdpn` (`gradient_map_678_take.py`'s confirm take: a
//! Gradient Fill with four stops at different midpoints, `f_seg_n4_i0`,
//! 1.0 / 0.005 codes under that reading where stop `i`'s own is 63 / 21.9 off;
//! the Gradient Map's SEG rows agree). The index is translated only at the
//! two file boundaries, `psd_layer_effects_fx`'s `Grdn` and
//! `psd_adjustment_grdm`, so this sampler, the model and `.rz` keep one rule.
//!
//! # The bracket bound
//!
//! [`ClassicBound::Bracket`] holds each channel of [`smooth_color`]'s
//! Classic term to the range of its two bracketing stops before the
//! Smoothness blend, so the spline never undershoots or overshoots a stop.
//! Its users:
//! - the Gradient Map's `method: classic` (`adjust_map`): fitted offline by
//!   `scripts/photoshop-probe/gradient_map_678_take.py` on gradient-map-673
//!   take1 (all 58 GRID and SEG rows within 2 / 0.5 codes, where the
//!   unbounded spline misses C3 at Smoothness 100 % by 3.1 codes) and held
//!   by one confirm take (every SMOOTH and SEG row within 2.0 / 0.095; the
//!   unbounded spline and a clamp to the whole gradient's range both miss
//!   the four-stop N4 at 100 % by 4 codes);
//! - the Gradient Map's `method: perceptual`, the same bound over the stops'
//!   Oklab values (the confirm take's six Perceptual rows within 1.0 /
//!   0.042);
//! - the Gradient Fill LAYER (`style_gradient::fill_bound`: Linear, Radial,
//!   Angle and Reflected), measured on the confirm take's `f_n4_i100` rows
//!   (8 and 16 bits; the bound 1.0 / 0.010 codes, the unbounded spline 4.0
//!   / 0.15), every earlier Photoshop fill row kept within 2 / 0.5 or its
//!   pin. 32-bit linear stops and CMYK ink run the same kernel, unmeasured.
//!
//! Every other caller passes [`ClassicBound::Unbounded`], unmeasured: the
//! Gradient tool ([`stop_color`]), the Gradient Overlay, the Stroke's
//! gradient and the gradient glows keep the spline as it is.

/// The smallest midpoint a stop may carry: 5 %.
pub(crate) const MIDPOINT_MIN: f32 = 0.05;

/// The largest midpoint a stop may carry: 95 %.
pub(crate) const MIDPOINT_MAX: f32 = 0.95;

/// The segment fraction `f` remapped through the midpoint `m` (module doc,
/// "Midpoints"): `f` itself at exactly 0.5, so the default is today's
/// arithmetic; else `f / (2m)` for `f <= m`, `0.5 + (f − m) / (2 (1 − m))`
/// past it. Monotone, 0 at 0, 1/2 at `m`, 1 at 1. `m` is in
/// [`MIDPOINT_MIN`]..[`MIDPOINT_MAX`] wherever a stop list is validated.
/// The host's route to it is `ffi_gradient::dr_gradient_midpoint_fraction`
/// (the gradient editor's strip), so no host copy of the formula exists.
pub(crate) fn midpoint_fraction(f: f32, m: f32) -> f32 {
    if m == 0.5 {
        return f;
    }
    if f <= m {
        f / (2.0 * m)
    } else {
        0.5 + (f - m) / (2.0 * (1.0 - m))
    }
}

pub(crate) fn validate(from: [f64; 2], to: [f64; 2]) -> Result<(), String> {
    if from
        .into_iter()
        .chain(to)
        .any(|v| !v.is_finite() || v.abs() > 1e9)
        || from == to
    {
        return Err("gradient endpoints must be distinct, finite and bounded".into());
    }
    Ok(())
}
pub(crate) fn parameter(point: [f64; 2], from: [f64; 2], to: [f64; 2], radial: bool) -> f64 {
    let delta = [to[0] - from[0], to[1] - from[1]];
    let length = delta[0].hypot(delta[1]);
    if radial {
        (point[0] - from[0]).hypot(point[1] - from[1]) / length
    } else {
        // Normalize first to retain stable finite behavior for tiny segments.
        ((point[0] - from[0]) * (delta[0] / length) + (point[1] - from[1]) * (delta[1] / length))
            / length
    }
    .clamp(0., 1.)
}

/// The five shapes a stops gradient takes over the drag `from → to`.
/// Linear and Radial are [`parameter`]'s two; the other three are
/// [`styled_parameter`]'s.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldStyle {
    Linear,
    Radial,
    Angle,
    Reflected,
    Diamond,
}

impl FieldStyle {
    /// Whether this style's colour follows the Classic ramp
    /// ([`classic_color`], module doc): Linear, Angle, Reflected and Radial,
    /// the identified four. `GradientSampler::classic` (style_gradient.rs)
    /// repeats this list for a fill with no attached field: change the two
    /// together.
    pub(crate) fn classic(self) -> bool {
        matches!(
            self,
            Self::Linear | Self::Angle | Self::Reflected | Self::Radial
        )
    }

    /// The `DrGradientKind` code, 0..=4 in declaration order.
    pub fn from_c(code: u32) -> Option<Self> {
        match code {
            0 => Some(Self::Linear),
            1 => Some(Self::Radial),
            2 => Some(Self::Angle),
            3 => Some(Self::Reflected),
            4 => Some(Self::Diamond),
            _ => None,
        }
    }
}

/// One stop of a stops gradient: its position along the ramp, 0..1, its
/// straight RGBA colour in the caller's unit (0..1 at 8 and 16 bits; a scalar
/// plane passes `[g, g, g, alpha]`), and its colour and opacity midpoints
/// toward the next stop (module doc, "Midpoints"; 0.5 when absent).
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldStop {
    pub position: f32,
    pub color: [f32; 4],
    #[serde(default = "half")]
    pub midpoint: f32,
    #[serde(default = "half")]
    pub opacity_midpoint: f32,
}

/// A midpoint's default (serde); `cmyk_paint::CmykStop` uses it too.
pub(crate) fn half() -> f32 {
    0.5
}

/// A Smoothness's default (serde): 1, i.e. 100 %, the Classic ramp
/// untouched (module doc, "Smoothness").
pub(crate) fn full_smoothness() -> f32 {
    1.0
}

/// Whether `k` is a Smoothness a stops gradient may take: a fraction finite
/// in [0, 1] (module doc, "Smoothness").
pub(crate) fn validate_smoothness(k: f32) -> Result<(), String> {
    if !k.is_finite() || !(0.0..=1.0).contains(&k) {
        return Err("gradient smoothness must be finite in [0,1]".into());
    }
    Ok(())
}

/// Whether `m` is a midpoint a stop may carry: finite in
/// [`MIDPOINT_MIN`]..=[`MIDPOINT_MAX`].
pub(crate) fn midpoint_valid(m: f32) -> bool {
    m.is_finite() && (MIDPOINT_MIN..=MIDPOINT_MAX).contains(&m)
}

/// A parsed midpoint rounded to 0.01: Photoshop's whole percent, the `Mdpn`
/// it writes, so a map renders the same before and after a file round trip.
pub(crate) fn round_midpoint(m: f32) -> f32 {
    ((f64::from(m) * 100.0).round() / 100.0) as f32
}

/// The rules every stops list obeys before [`stop_color`] reads it: at least
/// one stop, every position finite in [0, 1] and not below the one before
/// it, every colour component finite and alpha in [0, 1], both midpoints
/// finite in [`MIDPOINT_MIN`]..=[`MIDPOINT_MAX`]. The stop COUNT
/// limit is the caller's (`style_json::GRADIENT_STOPS`): this module imports
/// nothing from the crate.
pub(crate) fn validate_stops(stops: &[FieldStop]) -> Result<(), String> {
    if stops.is_empty() {
        return Err("gradient stops must not be empty".into());
    }
    let mut previous = 0.0f32;
    for stop in stops {
        if !stop.position.is_finite() || !(0.0..=1.0).contains(&stop.position) {
            return Err("gradient stop positions must be finite in [0,1]".into());
        }
        if stop.position < previous {
            return Err("gradient stop positions must be in non-decreasing order".into());
        }
        previous = stop.position;
        if stop.color.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&stop.color[3]) {
            return Err("gradient stop colors must be finite with alpha in [0,1]".into());
        }
        if !midpoint_valid(stop.midpoint) || !midpoint_valid(stop.opacity_midpoint) {
            return Err("gradient stop midpoints must be finite in [0.05,0.95]".into());
        }
    }
    Ok(())
}

/// The ramp parameter in [0, 1] at `point` for a `style` gradient dragged
/// `from → to`. With `L` the drag length, `u` the point's projection on the
/// drag over `L` and `v` its projection on the drag's perpendicular over `L`
/// (the perpendicular is the drag turned a quarter turn clockwise as the
/// y-down canvas shows it):
///
/// - Linear and Radial are [`parameter`] exactly, so a two-stop request and a
///   stops request with the same two colours share one field.
/// - Angle is `frac(atan2(v, u) / 2π)`: 0 along the drag, growing clockwise
///   on screen, just under 1 on the last degree before the drag again.
/// - Reflected is `clamp(|u|)`: the linear ramp mirrored about the start.
/// - Diamond is `clamp(|u| + |v|)`: the L1 distance, so the level sets are
///   squares standing on a corner along the drag.
pub(crate) fn styled_parameter(
    point: [f64; 2],
    from: [f64; 2],
    to: [f64; 2],
    style: FieldStyle,
) -> f64 {
    match style {
        FieldStyle::Linear => return parameter(point, from, to, false),
        FieldStyle::Radial => return parameter(point, from, to, true),
        FieldStyle::Angle | FieldStyle::Reflected | FieldStyle::Diamond => {}
    }
    let delta = [to[0] - from[0], to[1] - from[1]];
    let length = delta[0].hypot(delta[1]);
    let unit = [delta[0] / length, delta[1] / length];
    let offset = [point[0] - from[0], point[1] - from[1]];
    let u = (offset[0] * unit[0] + offset[1] * unit[1]) / length;
    let v = (offset[1] * unit[0] - offset[0] * unit[1]) / length;
    let t = match style {
        FieldStyle::Angle => {
            let turn = v.atan2(u) / std::f64::consts::TAU;
            turn - turn.floor()
        }
        FieldStyle::Reflected => u.abs(),
        _ => u.abs() + v.abs(),
    };
    if t.is_finite() {
        t.clamp(0., 1.)
    } else {
        0.
    }
}

/// The ONE bracket search of a sorted stops list, shared by the layer-style
/// gradient (`style_gradient::gradient_color_with`) and [`stop_color`].
/// `position_of(i)` is stop `i`'s position, asked only for `i < count`.
/// `None` for `count` 0; `(i, i, 0.0)` when `t` is NaN or at or past an end
/// stop (the colour is stop `i`'s, untouched); else `(i, i + 1, f)` with `f`
/// the fraction of the way from stop `i` to stop `i + 1`. Of two stops at
/// the same position the later one wins past it.
pub(crate) fn segment(
    count: usize,
    t: f32,
    position_of: impl Fn(usize) -> f32,
) -> Option<(usize, usize, f32)> {
    if count == 0 {
        return None;
    }
    let (first, last) = (0, count - 1);
    if t.is_nan() || t <= position_of(first) {
        return Some((first, first, 0.0));
    }
    if t >= position_of(last) {
        return Some((last, last, 0.0));
    }
    for i in 0..last {
        let (a, b) = (position_of(i), position_of(i + 1));
        if t >= a && t < b {
            let span = b - a;
            let f = if span > 0.0 { (t - a) / span } else { 1.0 };
            return Some((i, i + 1, f));
        }
    }
    Some((last, last, 0.0))
}

/// The Classic ramp's colour (module doc) of a sorted stops list at `t`: the
/// bracket is [`segment`]'s; between stops `i` and `j = i + 1` at the
/// fraction `f`, remapped through `midpoint_of(i)` ([`midpoint_fraction`]),
/// it is the cubic Hermite `h00 a + h10 m_i + h01 b + h11 m_j`,
/// `m_k = (v[k + 1] − v[k − 1]) / 2` with the end stops repeated; an end stop
/// or NaN reads the stop untouched, and an empty list zero. `position_of(i)`,
/// `midpoint_of(i)` and `color_of(i)` are asked only for `i < count`. Not clamped: a
/// non-monotone run of stops may overshoot its stops a little, as the
/// spline does, and every byte and 16-bit consumer clamps when it stores.
/// Allocates nothing.
pub(crate) fn classic_color<const N: usize>(
    count: usize,
    t: f32,
    position_of: impl Fn(usize) -> f32,
    midpoint_of: impl Fn(usize) -> f32,
    color_of: impl Fn(usize) -> [f32; N],
) -> [f32; N] {
    let Some((i, j, f)) = segment(count, t, position_of) else {
        return [0.0; N];
    };
    let a = color_of(i);
    if i == j {
        return a;
    }
    let f = midpoint_fraction(f, midpoint_of(i));
    let b = color_of(j);
    let before = if i > 0 { color_of(i - 1) } else { a };
    let after = if j + 1 < count { color_of(j + 1) } else { b };
    let (f2, f3) = (f * f, f * f * f);
    let h00 = 2.0 * f3 - 3.0 * f2 + 1.0;
    let h10 = f3 - 2.0 * f2 + f;
    let h01 = -2.0 * f3 + 3.0 * f2;
    let h11 = f3 - f2;
    std::array::from_fn(|c| {
        let (m_i, m_j) = ((b[c] - before[c]) / 2.0, (after[c] - a[c]) / 2.0);
        h00 * a[c] + h10 * m_i + h01 * b[c] + h11 * m_j
    })
}

/// Where the Classic term of [`smooth_color`] may run (module doc, "The
/// bracket bound").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClassicBound {
    /// The spline as it is: the Gradient tool, the Gradient Overlay, the
    /// Stroke's gradient, the glows and a Diamond fill (unmeasured).
    Unbounded,
    /// Each channel of the Classic term clamped to its bracketing stops'
    /// range before the blend: the Gradient Map's `method: classic` and
    /// `perceptual`, and the Gradient Fill layer of a Classic style.
    Bracket,
}

/// Smoothness (module doc) over the Classic ramp: `k` at or
/// above 1 (100 %, the default) is [`classic_color`] untouched, so a default
/// gradient takes today's path exactly; below it the colour is
/// `(1 − k)·L + k·C`, `L` the linear interpolation of [`segment`]'s bracket
/// and `C` [`classic_color`]'s, both at the fraction remapped through
/// `midpoint_of(i)` ([`midpoint_fraction`]). `k` 0 is the linear ramp.
/// Under [`ClassicBound::Bracket`] each channel of `C` is first held to the
/// range of the two bracketing stops, at every `k` (module doc, "The
/// bracket bound"); [`ClassicBound::Unbounded`] is the arithmetic
/// above statement for statement. Allocates nothing.
pub(crate) fn smooth_color<const N: usize>(
    count: usize,
    t: f32,
    k: f32,
    bound: ClassicBound,
    position_of: impl Fn(usize) -> f32,
    midpoint_of: impl Fn(usize) -> f32,
    color_of: impl Fn(usize) -> [f32; N],
) -> [f32; N] {
    if k >= 1.0 && bound == ClassicBound::Unbounded {
        return classic_color(count, t, position_of, midpoint_of, color_of);
    }
    let Some((i, j, f)) = segment(count, t, &position_of) else {
        return [0.0; N];
    };
    let f = if i == j {
        f
    } else {
        midpoint_fraction(f, midpoint_of(i))
    };
    let (a, b) = (color_of(i), color_of(j));
    let mut classic = classic_color(count, t, position_of, midpoint_of, color_of);
    if bound == ClassicBound::Bracket {
        // `max`/`min`, not `clamp`: never a panic, whatever the numbers.
        for (c, v) in classic.iter_mut().enumerate() {
            *v = v.max(a[c].min(b[c])).min(a[c].max(b[c]));
        }
        if k >= 1.0 {
            return classic;
        }
    }
    std::array::from_fn(|c| {
        let linear = a[c] + (b[c] - a[c]) * f;
        (1.0 - k) * linear + k * classic[c]
    })
}

/// The Classic ramp of two colours at 0 and 1 ([`classic_color`]; on two
/// stops `a + (b − a) S(t)`), for the tool's two-colour gradients; the
/// caller keeps alpha linear.
pub(crate) fn classic_two<const N: usize>(a: [f32; N], b: [f32; N], t: f32) -> [f32; N] {
    classic_color(2, t, |i| i as f32, |_| 0.5, |i| if i == 0 { a } else { b })
}

/// The straight RGBA of `stops` at `t` under a `style` gradient at the
/// Smoothness `k` (a fraction, module doc): the colour is [`smooth_color`]
/// (the Classic ramp blended with the linear one by `k`; at `k` 1 exactly
/// [`classic_color`]) where [`FieldStyle::classic`], else linear between the
/// two bracketing stops ([`segment`]) at any `k`; alpha is linear in both.
/// The colour reads the segment fraction remapped through stop `i`'s
/// `midpoint`, alpha through its `opacity_midpoint` ([`midpoint_fraction`],
/// module doc). The end stop's colour at or past an end. Transparent zero for
/// an empty list. Allocates nothing.
pub(crate) fn stop_color(stops: &[FieldStop], t: f32, style: FieldStyle, k: f32) -> [f32; 4] {
    let Some((i, j, f)) = segment(stops.len(), t, |i| stops[i].position) else {
        return [0.0; 4];
    };
    let a = stops[i].color;
    if i == j {
        return a;
    }
    let b = stops[j].color;
    let fc = midpoint_fraction(f, stops[i].midpoint);
    let fa = midpoint_fraction(f, stops[i].opacity_midpoint);
    let mut out: [f32; 4] = std::array::from_fn(|c| {
        let g = if c == 3 { fa } else { fc };
        a[c] + (b[c] - a[c]) * g
    });
    if style.classic() {
        let rgb = smooth_color(
            stops.len(),
            t,
            k,
            ClassicBound::Unbounded,
            |s| stops[s].position,
            |s| stops[s].midpoint,
            |s| {
                let c = stops[s].color;
                [c[0], c[1], c[2]]
            },
        );
        out[..3].copy_from_slice(&rgb);
    }
    out
}

/// A scalar plane sample under a `style` stops gradient whose stops are
/// `[g, g, g, alpha]` with `g` the gray over 65535:
/// `original · (1 − a) + g · 65535 · a`, `a = opacity · alpha`, rounded once;
/// the colour is [`stop_color`]'s at the Smoothness `k`.
pub(crate) fn scalar_over(
    original: u16,
    stops: &[FieldStop],
    t: f32,
    opacity: f64,
    style: FieldStyle,
    k: f32,
) -> u16 {
    let color = stop_color(stops, t, style, k);
    let a = opacity * f64::from(color[3]);
    (f64::from(original) * (1. - a) + f64::from(color[0]) * 65535. * a)
        .clamp(0., 65535.)
        .round() as u16
}
