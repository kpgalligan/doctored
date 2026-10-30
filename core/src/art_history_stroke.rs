//! KERNEL: the Art History Brush — a click turns into a set of
//! seeded, stylized strokes, each coloured from the History Brush's source
//! state at its start and painted through the caller's composite.
//!
//! The request is the tool's options: a Style (one of ten), the Area (px)
//! the strokes start in, the Tolerance (%) that keeps them where the source
//! differs from the current pixels, the Opacity, the blend mode (one of
//! seven) and the seed. The brush tip and the dabs are `dr_brush_u16`'s.
//! Everything here is in CANVAS coordinates; the adapters
//! (`doc_art_history`, `cmyk_art_history`) read the samples and write the
//! layer.
//!
//! # Clicks
//!
//! The first dab is a click, then every dab at least `max(area/2, its
//! diameter, 1)` px from the last click: a drag lays fresh clusters as it
//! moves, never a cluster per spacing step. Click `k` draws stroke `s` from
//! `SplitMix64::at(seed, k, s, 0)`, a positional stream, so a dab prefix is
//! a stroke prefix: the canvas's growing stroke and the committed one agree.
//!
//! # A click's strokes
//!
//! `N = clamp(round(ν·(A/D)²), 1, 64)` strokes (`N = 1` at `A = 0`), `D` the
//! click dab's diameter and `ν` the style's density (the table, fitted to
//! Photoshop's whole-stroke counts per click). Each starts uniform in the
//! disc of radius `A/2` about the click and is skipped (its draws still
//! consumed) when the source there is `None` (off the canvas or the source
//! layer), or when the premultiplied max-channel `|source − current| × 100 <
//! tolerance` (Photoshop's take: no stroke STARTS where the states agree,
//! though one may run into such a region). Its colour is the source at the
//! start pixel, ONE colour for the whole stroke (take1: every whole stroke
//! of Photoshop's is one exact colour, the source at one end of it); its
//! alpha is the source's alpha × the click dab's opacity × the request's
//! opacity.
//!
//! The path steps `D/4` for `L·D·(1 + u)` px, `u` uniform in ±0.2 (Tight) or
//! ±0.5 (Loose). The heading at each step is `base + jitter + curl`: jitter
//! is drawn once a stroke (±`TIGHT_JITTER_DEGREES` Tight,
//! ±`LOOSE_JITTER_DEGREES` Loose); the curl offset starts at 0 and adds
//! `CURL_SIGN·κ/4` a step, so a curl style turns `κ` radians per brush
//! diameter of travel, always the same way, on top of the base. Every style
//! with a path, Tight and Loose alike, takes its base from the source's
//! iso-LUMINANCE direction (`iso_luma_direction`: the minor eigenvector of
//! the luma structure tensor around the step, oriented so the brighter side
//! lies on the stroke's right; take1 found Photoshop's straight strokes
//! along the luma contours, every one travelling the same way). The first
//! non-flat reading sets the base; later readings are sign-continued from
//! it, so a stroke never reverses; a flat tensor holds the previous base,
//! and a stroke whose every reading is flat keeps its random base. Each step
//! is one dab of diameter `D`, opacity 1.
//!
//! # Painting
//!
//! Every stroke is generated before any pixel is painted (the tolerance
//! compares against the layer BEFORE the call). A stroke paints the field
//! `max over its dabs of BrushRequest::dab_coverage` over its box, at
//! `alpha = field · stroke alpha · gate`, through the caller's composite, in
//! stroke order; the max, not the brush's accumulation, because a stroke is
//! one opaque mark whose overlapping dabs must not darken its own interior.
//! The work is `Σ dab_work` over every stroke dab, refused above
//! `MAX_BRUSH_WORK` before anything is painted.
use serde::Deserialize;

use crate::brush_coverage::{footprint, BrushRequest, MAX_BRUSH_WORK};
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;
use crate::rng::SplitMix64;

/// The blend modes the Art History Brush offers: Normal, Darken,
/// Lighten, Hue, Saturation, Color and Luminosity (`DrBlendMode` values).
pub(crate) const BLEND_MODES: [u32; 7] = [0, 6, 7, 23, 24, 25, 26];
pub(crate) const BLEND_MODE_REFUSAL: &str = "art history brush blend_mode must be Normal, Darken, Lighten, Hue, Saturation, Color or Luminosity";
const STYLE_REFUSAL: &str = "art history brush style must be tight_short, tight_medium, tight_long, loose_medium, loose_long, dab, tight_curl, tight_curl_long, loose_curl or loose_curl_long";
const WORK_REFUSAL: &str = "art history brush exceeds 2 billion work units; use a smaller Area, a shorter stroke or a smaller brush";
/// The Area's ceiling, in pixels: the field takes 0–500.
const MAX_AREA: f64 = 500.;
/// The stroke count's ceiling per click: at a 1 px brush and Area 500 the
/// density formula asks for thousands, which would only repaint the same
/// disc; 64 keeps a click's work bounded by its longest strokes.
const MAX_STROKES: f64 = 64.;
/// A Tight stroke's jitter, ± this many degrees about its base (shipped).
const TIGHT_JITTER_DEGREES: f64 = 10.;
/// A Loose stroke's jitter. With `J(R)` the half-width whose uniform jitter
/// gives a doubled-angle resultant `R` (`sin 2J / 2J = R`), take1's mean
/// axis resultants (`art_history_take.py` THE REFIT) were 0.96 Loose and
/// 0.965 Tight: `10° + J(0.96) − J(0.965) = 10° + 14.1° − 13.2°`, the
/// difference cancelling the axis noise both kinds share.
const LOOSE_JITTER_DEGREES: f64 = 11.;
/// The curl's turn: the heading angle DECREASES, counter-clockwise on screen
/// (y down): take1's curl styles all travel 36°–98° on the decreasing side
/// of their straight twins' travel, never the other.
const CURL_SIGN: f64 = -1.;
/// The largest length factor `1 + u` (Loose's +0.5).
const MAX_SPREAD: f64 = 1.5;
/// A tensor is flat below this mean trace (squared unit change per pixel),
/// an RMS gradient of 10⁻³ a pixel — a quarter of an 8-bit code.
const FLAT_TRACE: f64 = 1e-6;
/// ... or when its eigenvalues differ by less than this share of the trace,
/// which takes an isotropic tensor (luma varying alike in every direction)
/// as flat: it names no direction.
const FLAT_ANISOTROPY: f64 = 0.1;

/// The ten Styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArtStyle {
    TightShort,
    TightMedium,
    TightLong,
    LooseMedium,
    LooseLong,
    Dab,
    TightCurl,
    TightCurlLong,
    LooseCurl,
    LooseCurlLong,
}

/// One Style's row: its wire name, the length `L` in brush diameters, the
/// curl `κ` in radians per diameter, the density `ν` (strokes per `(A/D)²`)
/// and whether it is Loose.
struct StyleRow {
    style: ArtStyle,
    name: &'static str,
    length: f64,
    kappa: f64,
    density: f64,
    loose: bool,
}

/// The table, fitted from Photoshop's take1 by `art_history_take.py`'s THE
/// REFIT (pass 0, one proportional step from H0's `L` 2/4/8, `κ` 0.75/0.5
/// and `ν = 0.5/max(L, 1)`): `L` scales the whole strokes' length past
/// their round caps, `ν` their count per click, a curl's `κ` the travel
/// offset from its straight twin. `dab` keeps H0's row (Photoshop's dab
/// leaves no whole stroke to fit).
const TABLE: [StyleRow; 10] = [
    row(ArtStyle::TightShort, "tight_short", 0.54, 0., 0.45, false),
    row(
        ArtStyle::TightMedium,
        "tight_medium",
        1.77,
        0.,
        0.3229,
        false,
    ),
    row(ArtStyle::TightLong, "tight_long", 2.99, 0., 0.3229, false),
    row(
        ArtStyle::LooseMedium,
        "loose_medium",
        1.73,
        0.,
        0.3594,
        true,
    ),
    row(ArtStyle::LooseLong, "loose_long", 2.25, 0., 0.3021, true),
    row(ArtStyle::Dab, "dab", 0., 0., 0.5, false),
    row(ArtStyle::TightCurl, "tight_curl", 2.6, 1.212, 0.3073, false),
    row(
        ArtStyle::TightCurlLong,
        "tight_curl_long",
        1.58,
        0.655,
        0.3073,
        false,
    ),
    row(ArtStyle::LooseCurl, "loose_curl", 3.55, 0.522, 0.3177, true),
    row(
        ArtStyle::LooseCurlLong,
        "loose_curl_long",
        3.42,
        0.582,
        0.3385,
        true,
    ),
];

const fn row(
    style: ArtStyle,
    name: &'static str,
    length: f64,
    kappa: f64,
    density: f64,
    loose: bool,
) -> StyleRow {
    StyleRow {
        style,
        name,
        length,
        kappa,
        density,
        loose,
    }
}

impl ArtStyle {
    fn row(self) -> &'static StyleRow {
        TABLE.iter().find(|r| r.style == self).unwrap_or(&TABLE[0])
    }

    fn named(name: &str) -> Option<Self> {
        TABLE.iter().find(|r| r.name == name).map(|r| r.style)
    }
}

/// The table's largest `L`: what the plans size the longest stroke by.
pub(crate) const fn max_length() -> f64 {
    let mut largest = 0.;
    let mut i = 0;
    while i < TABLE.len() {
        if TABLE[i].length > largest {
            largest = TABLE[i].length;
        }
        i += 1;
    }
    largest
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    version: u32,
    style: String,
    area: f64,
    tolerance: f64,
    opacity: f64,
    blend_mode: u32,
    seed: u64,
}

/// The tool's options (module doc): `area` in px, `tolerance` in percent,
/// `opacity` in 0…1, `blend_mode` one of [`BLEND_MODES`], `seed` a `u32`.
pub(crate) struct ArtHistoryRequest {
    pub style: ArtStyle,
    pub area: f64,
    pub tolerance: f64,
    pub opacity: f64,
    pub blend_mode: u32,
    pub seed: u64,
}

impl ArtHistoryRequest {
    /// `{"version":1,"style":S,"area":0..500,"tolerance":0..100,
    /// "opacity":0..1,"blend_mode":M,"seed":0..4294967295}`, unknown fields
    /// refused; each bad value is refused by name.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let wire: Wire = serde_json::from_str(text)
            .map_err(|e| format!("invalid art history brush request: {e}"))?;
        if wire.version != 1 {
            return Err("art history brush request requires version 1".into());
        }
        let style = ArtStyle::named(&wire.style).ok_or(STYLE_REFUSAL)?;
        if !wire.area.is_finite() || !(0. ..=MAX_AREA).contains(&wire.area) {
            return Err("art history brush area must be 0 to 500 px".into());
        }
        if !wire.tolerance.is_finite() || !(0. ..=100.).contains(&wire.tolerance) {
            return Err("art history brush tolerance must be 0 to 100".into());
        }
        if !wire.opacity.is_finite() || !(0. ..=1.).contains(&wire.opacity) {
            return Err("art history brush opacity must be 0 to 1".into());
        }
        if !BLEND_MODES.contains(&wire.blend_mode) {
            return Err(BLEND_MODE_REFUSAL.into());
        }
        if wire.seed > u64::from(u32::MAX) {
            return Err("art history brush seed must be 0 to 4294967295".into());
        }
        Ok(Self {
            style,
            area: wire.area,
            tolerance: wire.tolerance,
            opacity: wire.opacity,
            blend_mode: wire.blend_mode,
            seed: wire.seed,
        })
    }
}

/// One generated stroke: its straight colour (last component alpha), its
/// alpha (module doc) and its dabs `[x, y, diameter, opacity]` in canvas
/// coordinates.
pub(crate) struct ArtStroke<const C: usize> {
    pub color: [f32; C],
    pub alpha: f32,
    pub dabs: Vec<[f64; 4]>,
}

/// The two states a click reads, at canvas pixel `(x, y)`: straight samples,
/// the last component alpha; `None` off the canvas or the layer.
pub(crate) trait ArtSamples<const C: usize> {
    fn source(&self, x: i64, y: i64) -> Option<[f32; C]>;
    fn current(&self, x: i64, y: i64) -> Option<[f32; C]>;
    /// The source's premultiplied luminance at (x, y); None where `source`
    /// is None.
    fn source_luma(&self, x: i64, y: i64) -> Option<f64>;
}

/// The clicks of a dab list (module doc): `(ordinal, dab)`.
fn clicks<'a>(req: &ArtHistoryRequest, dabs: &'a [f64]) -> Vec<(u64, &'a [f64])> {
    let mut out: Vec<(u64, &[f64])> = Vec::new();
    for dab in dabs.chunks_exact(4) {
        let far = match out.last() {
            None => true,
            Some((_, last)) => {
                let gap = (req.area / 2.).max(dab[2]).max(1.);
                (dab[0] - last[0]).hypot(dab[1] - last[1]) >= gap
            }
        };
        if far {
            out.push((out.len() as u64, dab));
        }
    }
    out
}

/// A click's stroke count at diameter `d` (module doc).
fn stroke_count(req: &ArtHistoryRequest, d: f64) -> u64 {
    if req.area == 0. {
        return 1;
    }
    (req.style.row().density * (req.area / d).powi(2))
        .round()
        .clamp(1., MAX_STROKES) as u64
}

/// The largest step count of a stroke of `req`'s style: its length over
/// `D/4`, which does not depend on `D`.
fn max_steps(req: &ArtHistoryRequest) -> u64 {
    let row = req.style.row();
    let spread = if row.loose { 0.5 } else { 0.2 };
    (4. * row.length * (1. + spread)).round() as u64
}

/// How far from a click dab's centre a stroke can reach, plus the tensor
/// window around its last step and a pixel of margin: the adapters read the
/// layer and the source within this of the dabs' bounding box.
pub(crate) fn reach(req: &ArtHistoryRequest, dabs: &[f64]) -> f64 {
    let largest = dabs.chunks_exact(4).map(|d| d[2]).fold(0., f64::max);
    req.area / 2. + MAX_SPREAD * req.style.row().length * largest + largest + 2.
}

/// A bound on the dabs [`generate`] can return for `req` and `dabs`, which
/// the plans charge before anything is generated.
pub(crate) fn dab_bound(req: &ArtHistoryRequest, dabs: &[f64]) -> u64 {
    let steps = max_steps(req) + 1;
    clicks(req, dabs)
        .iter()
        .filter(|(_, d)| d[2] > 0. && d[3] > 0.)
        .map(|(_, d)| stroke_count(req, d[2]) * steps)
        .sum()
}

/// Uniform in [0, 1) from the top 53 bits.
fn unit(rng: &mut SplitMix64) -> f64 {
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// Premultiplied: the colour components times the last (alpha).
fn premultiplied<const C: usize>(p: [f32; C]) -> [f64; C] {
    let a = f64::from(p[C - 1]);
    std::array::from_fn(|c| if c == C - 1 { a } else { f64::from(p[c]) * a })
}

/// The minor-eigenvector angle of the source's LUMA structure tensor
/// around `(x, y)` (module doc), or `None` when the tensor is flat. The
/// window has radius `max(1, d/2)`; it is sampled on a grid of stride `h =
/// max(1, ⌊r/4⌋)` with central differences of the same spacing, so a large
/// brush reads at most 81 points and sees structure at its own scale. The
/// angle is ORIENTED by the travel rule BRIGHT-RIGHT: the window's mean
/// luma gradient `(mx, my)` lies on the stroke's right (y down),
/// `−sin θ·mx + cos θ·my > 0`; a mean gradient below `FLAT_TRACE` in
/// squared length (a ridge or a valley, whose sides cancel) leaves the axis
/// unoriented.
fn iso_luma_direction<const C: usize>(
    samples: &impl ArtSamples<C>,
    x: f64,
    y: f64,
    d: f64,
) -> Option<f64> {
    let radius = (d / 2.).max(1.);
    let h = (radius / 4.).floor().max(1.) as i64;
    let m = (radius as i64 / h).max(1);
    let (px, py) = (x.floor() as i64, y.floor() as i64);
    let (mut jxx, mut jxy, mut jyy, mut count) = (0., 0., 0., 0.);
    let (mut mx, mut my) = (0., 0.);
    for j in -m..=m {
        for i in -m..=m {
            if ((i * i + j * j) * h * h) as f64 > radius * radius {
                continue;
            }
            let (sx, sy) = (px + i * h, py + j * h);
            let (Some(l), Some(r), Some(t), Some(b)) = (
                samples.source_luma(sx - h, sy),
                samples.source_luma(sx + h, sy),
                samples.source_luma(sx, sy - h),
                samples.source_luma(sx, sy + h),
            ) else {
                continue;
            };
            let gx = (r - l) / (2 * h) as f64;
            let gy = (b - t) / (2 * h) as f64;
            jxx += gx * gx;
            jxy += gx * gy;
            jyy += gy * gy;
            mx += gx;
            my += gy;
            count += 1.;
        }
    }
    if count == 0. {
        return None;
    }
    let (jxx, jxy, jyy) = (jxx / count, jxy / count, jyy / count);
    let trace = jxx + jyy;
    let spread = ((jxx - jyy).powi(2) + 4. * jxy * jxy).sqrt();
    if trace < FLAT_TRACE || spread < FLAT_ANISOTROPY * trace {
        return None;
    }
    // The major eigenvector is the luma gradient's direction; the
    // iso-luminance direction is perpendicular to it.
    let angle = 0.5 * (2. * jxy).atan2(jxx - jyy) + std::f64::consts::FRAC_PI_2;
    let (mx, my) = (mx / count, my / count);
    if mx * mx + my * my < FLAT_TRACE {
        return Some(angle);
    }
    if -angle.sin() * mx + angle.cos() * my > 0. {
        Some(angle)
    } else {
        Some(angle + std::f64::consts::PI)
    }
}

/// `angle` or its opposite, whichever is nearer `previous`.
fn continued(angle: f64, previous: f64) -> f64 {
    if (angle - previous).cos() < 0. {
        angle + std::f64::consts::PI
    } else {
        angle
    }
}

/// Every stroke of every click of `dabs` (packed `dr_brush_u16` tuples in
/// canvas coordinates), in paint order (module doc). An empty list paints
/// nothing.
pub(crate) fn generate<const C: usize>(
    req: &ArtHistoryRequest,
    dabs: &[f64],
    samples: &impl ArtSamples<C>,
) -> Result<Vec<ArtStroke<C>>, String> {
    let row = req.style.row();
    let (spread, jitter) = if row.loose {
        (0.5, LOOSE_JITTER_DEGREES.to_radians())
    } else {
        (0.2, TIGHT_JITTER_DEGREES.to_radians())
    };
    let mut out: Vec<ArtStroke<C>> = Vec::new();
    for (k, click) in clicks(req, dabs) {
        let d = click[2];
        if d <= 0. || click[3] <= 0. {
            continue;
        }
        let step = d / 4.;
        for s in 0..stroke_count(req, d) {
            let mut rng = SplitMix64::at(req.seed, k, s, 0);
            // Every draw is taken whether or not the stroke survives, in one
            // fixed order, so a skipped stroke moves no other.
            let (u_radius, u_angle, u_length, u_jitter, _u_sign, u_base) = (
                unit(&mut rng),
                unit(&mut rng),
                unit(&mut rng),
                unit(&mut rng),
                unit(&mut rng),
                unit(&mut rng),
            );
            let r = req.area / 2. * u_radius.sqrt();
            let phi = std::f64::consts::TAU * u_angle;
            let (sx, sy) = (click[0] + r * phi.cos(), click[1] + r * phi.sin());
            let (px, py) = (sx.floor() as i64, sy.floor() as i64);
            let Some(source) = samples.source(px, py) else {
                continue;
            };
            let current = samples.current(px, py).unwrap_or([0.; C]);
            let (a, b) = (premultiplied(source), premultiplied(current));
            let difference = (0..C).map(|c| (a[c] - b[c]).abs()).fold(0., f64::max);
            if difference * 100. < req.tolerance {
                continue;
            }
            let alpha = (f64::from(source[C - 1]) * click[3] * req.opacity) as f32;
            if alpha <= 0. {
                continue;
            }
            let length = row.length * d * (1. + spread * (2. * u_length - 1.));
            let steps = (length / step).round().max(0.) as usize;
            let jitter = jitter * (2. * u_jitter - 1.);
            let mut base = std::f64::consts::TAU * u_base;
            let mut oriented = false;
            let mut curl = 0.;
            let mut dabs_out = reserved_vec(steps + 1)?;
            let (mut x, mut y) = (sx, sy);
            dabs_out.push([x, y, d, 1.]);
            for _ in 0..steps {
                // The first reading sets the heading, travel rule and all;
                // later ones are continued from it, so a stroke never
                // reverses where the orientation is unread.
                if let Some(angle) = iso_luma_direction(samples, x, y, d) {
                    base = if oriented {
                        continued(angle, base)
                    } else {
                        angle
                    };
                    oriented = true;
                }
                let heading = base + jitter + curl;
                x += step * heading.cos();
                y += step * heading.sin();
                dabs_out.push([x, y, d, 1.]);
                curl += CURL_SIGN * row.kappa * step / d;
            }
            out.push(ArtStroke {
                color: source,
                alpha,
                dabs: dabs_out,
            });
        }
    }
    Ok(out)
}

/// Paints `strokes` into `pixels`, the row-major samples of `rect = (x, y,
/// w, h)` in canvas coordinates, through `composite(pixel, colour, alpha,
/// canvas xy)` at `alpha = field · stroke alpha · gate(x, y)` (module doc).
/// Refuses, before painting, work past `MAX_BRUSH_WORK`. `Ok(true)` when
/// any pixel was composited.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint<const C: usize>(
    strokes: &[ArtStroke<C>],
    brush: &BrushRequest,
    rect: (i64, i64, u32, u32),
    pixels: &mut [[f32; C]],
    gate: impl Fn(i64, i64) -> f32,
    mut composite: impl FnMut(&mut [f32; C], [f32; C], f32, (i64, i64)),
    job: JobContext<'_>,
) -> Result<bool, String> {
    let (rx, ry, rw, rh) = rect;
    let plane = (rw, rh);
    let local = |d: &[f64; 4]| [d[0] - rx as f64, d[1] - ry as f64, d[2], d[3]];
    let mut work = 0_u64;
    for stroke in strokes {
        for dab in &stroke.dabs {
            work = work.saturating_add(brush.dab_work(plane, &local(dab)));
            if work > MAX_BRUSH_WORK {
                return Err(WORK_REFUSAL.into());
            }
        }
    }
    let rotation = brush.rotation();
    let mut field: Vec<f32> = reserved_vec(0)?;
    let mut touched = false;
    for (n, stroke) in strokes.iter().enumerate() {
        job.check()?;
        let shifted: Vec<f64> = stroke.dabs.iter().flat_map(local).collect();
        let Some([x0, y0, x1, y1]) = footprint(plane, &shifted) else {
            continue;
        };
        let width = x1 - x0;
        field.clear();
        field
            .try_reserve(width * (y1 - y0))
            .map_err(|_| "could not allocate the art history stroke field".to_string())?;
        field.resize(width * (y1 - y0), 0.);
        for dab in shifted.chunks_exact(4) {
            let Some([bx0, by0, bx1, by1]) = footprint(plane, dab) else {
                continue;
            };
            for y in by0..by1 {
                for x in bx0..bx1 {
                    let c = brush.dab_coverage(dab, rotation, x, y) as f32;
                    let at = (y - y0) * width + (x - x0);
                    if c > field[at] {
                        field[at] = c;
                    }
                }
            }
        }
        for y in y0..y1 {
            for x in x0..x1 {
                let c = field[(y - y0) * width + (x - x0)];
                if c <= 0. {
                    continue;
                }
                let (cx, cy) = (x as i64 + rx, y as i64 + ry);
                let alpha = c * stroke.alpha * gate(cx, cy);
                if alpha <= 0. {
                    continue;
                }
                composite(
                    &mut pixels[y * rw as usize + x],
                    stroke.color,
                    alpha.min(1.),
                    (cx, cy),
                );
                touched = true;
            }
        }
        job.report(n as f64 / strokes.len() as f64 * 0.9, 3)?;
    }
    Ok(touched)
}
