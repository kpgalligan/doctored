//! Brush dynamics (`designs/brush-dynamics.md`): one nominal stamp of a
//! Brush, Pencil or Eraser stroke — its position, the brush size, the pen
//! sample and the direction of travel — resolved into the dabs actually laid.
//!
//! This module never draws. A stroke is a list of resolved dabs
//! (`x, y, diameter, deposit`) and every rasterizer, commit path, colour mode,
//! recording and replay downstream already works on that list, so a dynamic
//! changes WHICH dabs are laid and never how one is composited. That is why no
//! colour mode needs a rule of its own here and no document state changes.
//!
//! THE RULE. Controls, curves and factors are in 0…1.
//!
//! - A response (`size`, `opacity`) has a control — off, pen pressure or pen
//!   tilt — a curve `C` (the natural cubic through 2…16 points running from
//!   x = 0 to x = 1, absent the identity), a minimum and a jitter. Its control
//!   value `v` is the pressure, or `min(1, hypot(tilt_x, tilt_y))` for tilt
//!   (0 upright, 1 flat). Its factor is 1 when the control is off or the stamp
//!   has no sample for it (a NaN pressure or tilt: a mouse, or an MCP call
//!   without `pressures` or `tilts`), else `minimum + (1 − minimum) · C(v)`.
//! - The nominal diameter is `D0 = max(1, size · f_size)` (the walk's spacing
//!   follows it) and the nominal deposit factor `O0 = f_opacity`.
//! - Stamp `i` (the walk's count of nominal stamps) lays `n` dabs: the scatter
//!   count, or with count jitter `max(1, round(count · (1 − count_jitter · u)))`,
//!   `u` the first unit draw of `SplitMix64::at(seed, i, 0, 1)`.
//! - Dab `k` takes four unit draws `u1…u4` from `SplitMix64::at(seed, i, k, 2)`:
//!   diameter `max(1, D0 · (1 − size_jitter · u1))`, deposit factor
//!   `O0 · (1 − opacity_jitter · u2)`, and an offset of `amount · D0 · (2·u3 − 1)`
//!   perpendicular to the direction of travel plus, with `both_axes`,
//!   `amount · D0 · (2·u4 − 1)` along it. With no direction (a click, a
//!   stroke's first stamp) the offset `amount · D0 · (2·u3 − 1)` runs at the
//!   angle `2π·u4`. Amount 0 leaves the position bit for bit.
//! - THE TIP SHAPE (slice 2). Dab `k` also takes the fifth and sixth draws
//!   `u5`, `u6` of its stream: angle `wrap(tip.angle + a + 180·jitter·(2·u5 − 1))`
//!   into (−180, 180], where `a` is the sample's pen rotation (degrees,
//!   counter-clockwise, canvas frame), the tilt's direction
//!   `atan2(tilt_y, tilt_x)` (tilt in a y-up frame) or the heading's
//!   `atan2(−heading_y, heading_x)` (canvas y runs down), and 0 for off, a
//!   NaN rotation or a zero tilt or heading; roundness
//!   `clamp(tip.roundness · f · (1 − jitter·u6), 0.01, 1)`, `f` the roundness
//!   control's `minimum + (1 − minimum)·v` (no curve; 1 for off or no sample).
//!   The heading is the caller's (the last non-zero move between nominal
//!   stamps, so a stamp at rest keeps the angle of the travel before it); the
//!   scatter keeps reading the direction. Absent `tip`, `angle` and
//!   `roundness` sections resolve to angle 0 and roundness 1.
//!
//! Why these choices: jitter only SHRINKS a dab, so no dab is wider than the
//! brush the cursor shows, and the canvas session's up-front work charge stays
//! an upper bound. The streams are positional, so stamp `i` resolves the same
//! however the stroke was split into drag events, and a recorded stroke's dabs
//! are what replay draws (the step holds the resolved dabs, never the seed).
//! The curve is `adjust_curves::Spline`, the crate's one natural cubic, which
//! works in the 0…255 code space: the points go in scaled by 255 and the value
//! comes back divided by it (the scaling is exact up to rounding: a natural
//! cubic through scaled points is the scaled cubic).
use crate::adjust_curves::Spline;
use crate::rng::SplitMix64;
use serde::Deserialize;

/// A request's byte bound, checked before parsing.
pub const MAX_REQUEST_BYTES: usize = 4096;
/// The most dabs one stamp lays.
pub const MAX_SCATTER_COUNT: u32 = 16;
/// The most points a response curve holds.
pub const MAX_CURVE_POINTS: usize = 16;
/// The largest scatter amount, in nominal diameters.
pub const MAX_SCATTER_AMOUNT: f64 = 10.0;
/// The values one stamp's sample holds: x, y, size, pressure, tilt_x, tilt_y,
/// dir_x, dir_y, rotation_degrees, heading_x, heading_y.
pub const SAMPLE_LEN: usize = 11;
/// The values one resolved dab holds: x, y, diameter, deposit factor,
/// angle_degrees, roundness.
pub const DAB_LEN: usize = 6;
/// The narrowest roundness a dab takes, the native brush request's own floor.
const MIN_ROUNDNESS: f64 = 0.01;

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Control {
    Off,
    Pressure,
    Tilt,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum AngleControl {
    Off,
    Rotation,
    Tilt,
    Direction,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AngleText {
    #[serde(default = "angle_off")]
    control: AngleControl,
    #[serde(default)]
    jitter: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoundnessText {
    #[serde(default = "off")]
    control: Control,
    #[serde(default)]
    minimum: f64,
    #[serde(default)]
    jitter: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TipText {
    #[serde(default)]
    angle_degrees: f64,
    #[serde(default = "full")]
    roundness: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponseText {
    #[serde(default = "off")]
    control: Control,
    #[serde(default)]
    curve: Option<Vec<[f64; 2]>>,
    #[serde(default)]
    minimum: f64,
    #[serde(default)]
    jitter: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScatterText {
    #[serde(default)]
    amount: f64,
    #[serde(default)]
    both_axes: bool,
    #[serde(default = "one")]
    count: u32,
    #[serde(default)]
    count_jitter: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestText {
    version: u32,
    #[serde(default)]
    seed: u32,
    #[serde(default)]
    size: Option<ResponseText>,
    #[serde(default)]
    opacity: Option<ResponseText>,
    #[serde(default)]
    scatter: Option<ScatterText>,
    #[serde(default)]
    tip: Option<TipText>,
    #[serde(default)]
    angle: Option<AngleText>,
    #[serde(default)]
    roundness: Option<RoundnessText>,
}

fn off() -> Control {
    Control::Off
}
fn angle_off() -> AngleControl {
    AngleControl::Off
}
fn one() -> u32 {
    1
}
fn full() -> f64 {
    1.
}

/// The tip shape's request: the tip's own angle and roundness, the angle's
/// control and jitter, and the roundness response (its curve always absent).
struct Shape {
    angle: f64,
    roundness: f64,
    angle_control: AngleControl,
    angle_jitter: f64,
    response: Response,
}

impl Shape {
    fn parse(
        tip: Option<TipText>,
        angle: Option<AngleText>,
        roundness: Option<RoundnessText>,
    ) -> Result<Self, String> {
        let tip = tip.unwrap_or(TipText {
            angle_degrees: 0.,
            roundness: 1.,
        });
        if !tip.angle_degrees.is_finite()
            || !(-180. ..=180.).contains(&tip.angle_degrees)
            || !tip.roundness.is_finite()
            || !(MIN_ROUNDNESS..=1.).contains(&tip.roundness)
        {
            return Err(
                "brush dynamics tip holds an angle_degrees from -180 to 180 and a \
                        roundness from 0.01 to 1"
                    .into(),
            );
        }
        let angle = angle.unwrap_or(AngleText {
            control: AngleControl::Off,
            jitter: 0.,
        });
        if !unit(angle.jitter) {
            return Err("brush dynamics angle jitter must be a number from 0 to 1".into());
        }
        let response = Response::parse(
            roundness.map(|r| ResponseText {
                control: r.control,
                curve: None,
                minimum: r.minimum,
                jitter: r.jitter,
            }),
            "roundness",
        )?;
        Ok(Shape {
            angle: tip.angle_degrees,
            roundness: tip.roundness,
            angle_control: angle.control,
            angle_jitter: angle.jitter,
            response,
        })
    }

    /// One dab's `(angle_degrees, roundness)` (module doc, THE TIP SHAPE).
    fn resolve(&self, sample: &[f64; SAMPLE_LEN], u5: f64, u6: f64) -> (f64, f64) {
        let [_, _, _, pressure, tilt_x, tilt_y, _, _, rotation, heading_x, heading_y] = *sample;
        let a = match self.angle_control {
            AngleControl::Off => 0.,
            AngleControl::Rotation => rotation,
            AngleControl::Tilt => direction_degrees(tilt_y, tilt_x),
            AngleControl::Direction => direction_degrees(-heading_y, heading_x),
        };
        let a = if a.is_finite() { a } else { 0. };
        let angle = wrap(self.angle + a + 180. * self.angle_jitter * (2. * u5 - 1.));
        let f = self.response.factor(pressure, (tilt_x, tilt_y));
        let roundness =
            (self.roundness * f * (1. - self.response.jitter * u6)).clamp(MIN_ROUNDNESS, 1.);
        (angle, roundness)
    }
}

/// `atan2(y, x)` in degrees, 0 for a zero or non-finite vector.
fn direction_degrees(y: f64, x: f64) -> f64 {
    let length = x.hypot(y);
    if length.is_finite() && length > 0. {
        y.atan2(x).to_degrees()
    } else {
        0.
    }
}

/// An angle in degrees wrapped into (−180, 180].
fn wrap(degrees: f64) -> f64 {
    180. - (180. - degrees).rem_euclid(360.)
}

/// One response: the parsed `size` or `opacity` section.
struct Response {
    control: Control,
    curve: Option<Spline>,
    minimum: f64,
    jitter: f64,
}

impl Response {
    const NONE: Response = Response {
        control: Control::Off,
        curve: None,
        minimum: 0.,
        jitter: 0.,
    };

    fn parse(text: Option<ResponseText>, name: &str) -> Result<Self, String> {
        let Some(text) = text else {
            return Ok(Self::NONE);
        };
        if !unit(text.minimum) || !unit(text.jitter) {
            return Err(format!(
                "brush dynamics {name} minimum and jitter must be numbers from 0 to 1"
            ));
        }
        let curve = match text.curve {
            None => None,
            Some(points) => Some(curve(&points, name)?),
        };
        Ok(Response {
            control: text.control,
            curve,
            minimum: text.minimum,
            jitter: text.jitter,
        })
    }

    /// The response's factor for a stamp's pressure and tilt (module doc).
    fn factor(&self, pressure: f64, tilt: (f64, f64)) -> f64 {
        let v = match self.control {
            Control::Off => return 1.,
            Control::Pressure => pressure,
            Control::Tilt => tilt.0.hypot(tilt.1),
        };
        // Checked before the clamp, which would turn a NaN into an end value.
        if !v.is_finite() {
            return 1.;
        }
        let v = v.clamp(0., 1.);
        let c = match &self.curve {
            None => v,
            Some(spline) => (spline.value(v * 255.) / 255.).clamp(0., 1.),
        };
        self.minimum + (1. - self.minimum) * c
    }
}

/// A curve's points checked and handed to the spline in its 0…255 space.
fn curve(points: &[[f64; 2]], name: &str) -> Result<Spline, String> {
    let refusal = || {
        format!(
            "brush dynamics {name} curve holds 2 to {MAX_CURVE_POINTS} points [x, y] in 0…1, \
             x strictly increasing from 0 to 1"
        )
    };
    if points.len() < 2 || points.len() > MAX_CURVE_POINTS {
        return Err(refusal());
    }
    let ordered = points.windows(2).all(|w| w[0][0] < w[1][0]);
    let first = points[0][0] == 0.;
    let last = points[points.len() - 1][0] == 1.;
    if !ordered || !first || !last || !points.iter().all(|p| unit(p[0]) && unit(p[1])) {
        return Err(refusal());
    }
    let scaled: Vec<(f64, f64)> = points.iter().map(|p| (p[0] * 255., p[1] * 255.)).collect();
    Ok(Spline::new(&scaled))
}

fn unit(v: f64) -> bool {
    v.is_finite() && (0. ..=1.).contains(&v)
}

/// A parsed dynamics request (module doc), immutable once made.
pub struct Dynamics {
    seed: u64,
    size: Response,
    opacity: Response,
    amount: f64,
    both_axes: bool,
    count: u32,
    count_jitter: f64,
    shape: Shape,
}

impl Dynamics {
    /// Strict: version 1, known keys only, every value in its range.
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > MAX_REQUEST_BYTES {
            return Err(format!(
                "brush dynamics request exceeds {MAX_REQUEST_BYTES} bytes"
            ));
        }
        let request: RequestText = serde_json::from_str(text)
            .map_err(|e| format!("invalid brush dynamics request: {e}"))?;
        if request.version != 1 {
            return Err("brush dynamics requires version 1".into());
        }
        let size = Response::parse(request.size, "size")?;
        let opacity = Response::parse(request.opacity, "opacity")?;
        let scatter = request.scatter.unwrap_or(ScatterText {
            amount: 0.,
            both_axes: false,
            count: 1,
            count_jitter: 0.,
        });
        let scattered = Self::scatter_only(
            u64::from(request.seed),
            scatter.amount,
            scatter.both_axes,
            scatter.count,
            scatter.count_jitter,
        )?;
        let shape = Shape::parse(request.tip, request.angle, request.roundness)?;
        Ok(Dynamics {
            size,
            opacity,
            shape,
            ..scattered
        })
    }

    /// Dynamics with only a scatter: no size, opacity or shape response, so
    /// every dab has the nominal diameter `max(1, size)`, deposit 1, angle 0
    /// and roundness 1. The scatter range checks' one home (`parse` routes
    /// through it): the amount in 0…[`MAX_SCATTER_AMOUNT`], the count in
    /// 1…[`MAX_SCATTER_COUNT`] and the count jitter in 0…1. The dual brush's
    /// second tip (`brush_dual`) is resolved by it with its own seed, which
    /// may lie above `u32::MAX`.
    pub(crate) fn scatter_only(
        seed: u64,
        amount: f64,
        both_axes: bool,
        count: u32,
        count_jitter: f64,
    ) -> Result<Self, String> {
        if !amount.is_finite() || !(0. ..=MAX_SCATTER_AMOUNT).contains(&amount) {
            return Err(format!(
                "brush dynamics scatter amount must be a number from 0 to {MAX_SCATTER_AMOUNT}"
            ));
        }
        if !(1..=MAX_SCATTER_COUNT).contains(&count) {
            return Err(format!(
                "brush dynamics scatter count must be a whole number from 1 to {MAX_SCATTER_COUNT}"
            ));
        }
        if !unit(count_jitter) {
            return Err("brush dynamics scatter count_jitter must be a number from 0 to 1".into());
        }
        Ok(Dynamics {
            seed,
            size: Response::NONE,
            opacity: Response::NONE,
            amount,
            both_axes,
            count,
            count_jitter,
            shape: Shape::parse(None, None, None)?,
        })
    }

    /// The nominal stamp at `size` and the pen sample: `(D0, O0)` (module
    /// doc). NaN pressure or tilt is a stamp without that sample. None when
    /// `size` is not a finite number at least 0.
    pub fn nominal(&self, size: f64, pressure: f64, tilt: (f64, f64)) -> Option<(f64, f64)> {
        if !size.is_finite() || size < 0. {
            return None;
        }
        let diameter = (size * self.size.factor(pressure, tilt)).max(1.);
        Some((diameter, self.opacity.factor(pressure, tilt)))
    }

    /// The most dabs [`Self::dabs`] can return for one stamp.
    pub fn max_dabs(&self) -> usize {
        self.count as usize
    }

    /// Stamp `index` resolved into its dabs (module doc), [`DAB_LEN`] values
    /// each (`x, y, diameter, deposit factor, angle_degrees, roundness`)
    /// appended to `out`. `sample` is [`SAMPLE_LEN`] values: x, y, size,
    /// pressure, tilt_x, tilt_y, dir_x, dir_y, rotation_degrees, heading_x,
    /// heading_y; the direction and heading need not be unit length, and a
    /// zero or non-finite one is none. None (nothing appended) when the
    /// position or size is not finite.
    pub fn dabs(
        &self,
        index: u64,
        sample: &[f64; SAMPLE_LEN],
        out: &mut Vec<f64>,
    ) -> Option<usize> {
        let [x, y, size, pressure, tilt_x, tilt_y, dir_x, dir_y, ..] = *sample;
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let (d0, o0) = self.nominal(size, pressure, (tilt_x, tilt_y))?;
        let n = if self.count_jitter > 0. {
            let u = SplitMix64::at(self.seed, index, 0, 1).next_unit_f64();
            ((f64::from(self.count) * (1. - self.count_jitter * u)).round() as u32).max(1)
        } else {
            self.count
        };
        let length = dir_x.hypot(dir_y);
        let direction =
            (length.is_finite() && length > 0.).then(|| (dir_x / length, dir_y / length));
        for k in 0..n {
            let mut rng = SplitMix64::at(self.seed, index, u64::from(k), 2);
            // Slice 1's four draws keep their order; the tip shape takes the next two.
            let u: [f64; 6] = std::array::from_fn(|_| rng.next_unit_f64());
            let diameter = (d0 * (1. - self.size.jitter * u[0])).max(1.);
            let deposit = o0 * (1. - self.opacity.jitter * u[1]);
            let (mut px, mut py) = (x, y);
            if self.amount > 0. {
                let across = self.amount * d0 * (2. * u[2] - 1.);
                match direction {
                    Some((dx, dy)) => {
                        px += -dy * across;
                        py += dx * across;
                        if self.both_axes {
                            let along = self.amount * d0 * (2. * u[3] - 1.);
                            px += dx * along;
                            py += dy * along;
                        }
                    }
                    None => {
                        let (sin, cos) = (std::f64::consts::TAU * u[3]).sin_cos();
                        px += cos * across;
                        py += sin * across;
                    }
                }
            }
            let (angle, roundness) = self.shape.resolve(sample, u[4], u[5]);
            out.extend_from_slice(&[px, py, diameter, deposit, angle, roundness]);
        }
        Some(n as usize)
    }
}
