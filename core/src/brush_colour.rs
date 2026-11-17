//! KERNEL: brush dynamics' COLOUR (`designs/brush-dynamics.md`, slice 5): each
//! dab of a Brush stroke takes its own ink, a seeded mix toward the
//! background colour and then hue, saturation and brightness jitter, and the
//! stroke carries that ink per pixel beside its coverage. The handle and the
//! rasterizing exports are `ffi_brush_colour`; the accumulation is
//! `brush_coverage`'s (THE COLOURED STROKE).
//!
//! # The rule
//!
//! - THE UNITS. The foreground, the background and every ink are the
//!   document's own unit triple: straight RGB 0…1 on an RGB, Grayscale or
//!   Duotone document; on a Lab document the twin's unit codes
//!   `[L/100, (a + 128)/255, (b + 128)/255]`. The kernel converts nothing.
//! - THE DRAWS. Dab `k` of stamp `i` draws `v1…v4` (top 53 bits each) from
//!   `SplitMix64::at(seed, i, k, 3)`, a stream apart from `brush_dynamics`'
//!   salts 1 and 2. All four are drawn under either model, so the mix of a dab
//!   is the same whichever amounts are set.
//! - THE MIX. `c = fg + (bg − fg)·mix·v1` per channel.
//! - MODEL `rgb`. With a non-zero `hue`, `saturation` or `brightness`:
//!   `[h, s, l] = rgb_to_hsl(c)` (`adjust_math`, the crate's one HSL), then
//!   `hsl_to_rgb([h + 180·hue·(2·v2 − 1), s·(1 − saturation·v3),
//!   l·(1 − brightness·v4)])`; else `c` unchanged. Brightness is the HSL
//!   lightness. Saturation and brightness jitter only lower: a dab is never
//!   more saturated or lighter than its mix.
//! - MODEL `lab`. Lab codes are not RGB, so no HSL: `hue` and `saturation`
//!   must be 0 ([`COLOUR_LAB_REFUSAL`]), the mix runs per code and brightness
//!   scales the L code alone, `c[0]·(1 − brightness·v4)`; a\* and b\* keep the
//!   mix's codes.
//! - Every ink is clamped to 0…1, so a recorded list always passes
//!   [`check_inks`].
//!
//! # The coloured stroke
//!
//! Per written pixel with dab coverage `c` (shape, texture and deposit
//! included: the value `BrushRequest::field` accumulates), `P[ch] +=
//! c·(ink[ch] − P[ch])` beside `F += c·(1 − F)`: `P` is the premultiplied ink
//! over the first tip's field `F`, accumulated in dab order. The straight ink
//! is `S = P / F` (0 where `F = 0`), clamped to 0…1. A uniform ink therefore
//! reads back exactly (`P` and `F` take the same steps), and where two dabs
//! overlap the later one weighs by its own coverage, as its paint would.
//! Coverage is the uncoloured stroke's bit for bit: `quantize(F)`, or with a
//! dual brush `quantize(F · F2)` (`brush_dual::product`); the ink is the first
//! tip's. Scratch beyond the plans is `P`, 24 bytes per footprint pixel.
use crate::brush_coverage::{footprint, BrushRequest};
use crate::brush_dual::product;
use crate::brush_texture::BrushTexture;
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;
use crate::rng::SplitMix64;
use serde::Deserialize;

/// A colour request's byte bound, checked before parsing.
pub const MAX_REQUEST_BYTES: usize = 1024;
/// A foreground, background or amount out of range (not finite included).
pub const COLOUR_REFUSAL: &str = "native brush colour holds a foreground and a background of three values from 0 to 1 and a mix, hue, saturation and brightness from 0 to 1";
/// A `lab` request with a non-zero hue or saturation (module doc, MODEL `lab`).
pub const COLOUR_LAB_REFUSAL: &str =
    "native brush colour in the lab model takes a mix and a brightness; its hue and saturation are 0";
/// An inks list that is not three values in 0…1 per dab.
pub const INKS_REFUSAL: &str = "native brush inks hold three values from 0 to 1 per dab";
/// The salt of the colour stream (module doc, THE DRAWS).
const SALT: u64 = 3;

/// How the jitter reads the triple (module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColourModel {
    Rgb,
    Lab,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestText {
    version: u32,
    #[serde(default = "rgb")]
    model: ColourModel,
    #[serde(default)]
    seed: u32,
    foreground: [f64; 3],
    background: [f64; 3],
    #[serde(default)]
    mix: f64,
    #[serde(default)]
    hue: f64,
    #[serde(default)]
    saturation: f64,
    #[serde(default)]
    brightness: f64,
}

fn rgb() -> ColourModel {
    ColourModel::Rgb
}

/// A parsed colour request, immutable once made (module doc).
pub struct ColourDynamics {
    model: ColourModel,
    seed: u32,
    foreground: [f64; 3],
    background: [f64; 3],
    mix: f64,
    hue: f64,
    saturation: f64,
    brightness: f64,
}

impl ColourDynamics {
    /// Strict: `{"version":1,"model":"rgb"|"lab","seed":u32,"foreground":[x,y,z],
    /// "background":[x,y,z],"mix","hue","saturation","brightness"}`, known keys
    /// only; `model` defaults to `rgb`, `seed` and the four amounts to 0. A
    /// value out of 0…1 refuses with [`COLOUR_REFUSAL`]; `lab` with a non-zero
    /// hue or saturation with [`COLOUR_LAB_REFUSAL`].
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > MAX_REQUEST_BYTES {
            return Err(format!(
                "native brush colour request exceeds {MAX_REQUEST_BYTES} bytes"
            ));
        }
        let request: RequestText = serde_json::from_str(text)
            .map_err(|e| format!("invalid native brush colour request: {e}"))?;
        if request.version != 1 {
            return Err("native brush colour requires version 1".into());
        }
        let amounts = [
            request.mix,
            request.hue,
            request.saturation,
            request.brightness,
        ];
        let admitted = request
            .foreground
            .iter()
            .chain(&request.background)
            .chain(&amounts)
            .all(|v| unit(*v));
        if !admitted {
            return Err(COLOUR_REFUSAL.into());
        }
        if request.model == ColourModel::Lab && (request.hue != 0. || request.saturation != 0.) {
            return Err(COLOUR_LAB_REFUSAL.into());
        }
        Ok(Self {
            model: request.model,
            seed: request.seed,
            foreground: request.foreground,
            background: request.background,
            mix: request.mix,
            hue: request.hue,
            saturation: request.saturation,
            brightness: request.brightness,
        })
    }

    /// The ink of dab `dab` of stamp `index` (module doc, THE DRAWS, THE MIX
    /// and the two models), each channel in 0…1.
    pub fn ink(&self, index: u64, dab: u32) -> [f64; 3] {
        let mut rng = SplitMix64::at(u64::from(self.seed), index, u64::from(dab), SALT);
        let v: [f64; 4] = std::array::from_fn(|_| rng.next_unit_f64());
        let c: [f64; 3] = std::array::from_fn(|ch| {
            let (fg, bg) = (self.foreground[ch], self.background[ch]);
            fg + (bg - fg) * self.mix * v[0]
        });
        let ink = match self.model {
            ColourModel::Lab => [c[0] * (1. - self.brightness * v[3]), c[1], c[2]],
            ColourModel::Rgb
                if self.hue == 0. && self.saturation == 0. && self.brightness == 0. =>
            {
                c
            }
            ColourModel::Rgb => {
                // `adjust_math`'s HSL is f32, the precision of every HSL
                // adjustment it serves; a dab's ink needs no more.
                let [h, s, l] = crate::adjust_math::rgb_to_hsl(c.map(|v| v as f32));
                let shifted = [
                    h + (180. * self.hue * (2. * v[1] - 1.)) as f32,
                    s * (1. - self.saturation * v[2]) as f32,
                    l * (1. - self.brightness * v[3]) as f32,
                ];
                crate::adjust_math::hsl_to_rgb(shifted).map(f64::from)
            }
        };
        ink.map(|v| v.clamp(0., 1.))
    }
}

/// The inks list's refusal ([`INKS_REFUSAL`]): three values per dab of
/// `dabs` (four values a dab), each finite in 0…1.
pub fn check_inks(dabs: &[f64], inks: &[f64]) -> Result<(), String> {
    if inks.len() != dabs.len() / 4 * 3 || !inks.iter().all(|v| unit(*v)) {
        return Err(INKS_REFUSAL.into());
    }
    Ok(())
}

/// A coloured stroke (module doc): the canvas u16 coverage, and the first
/// tip's field `F` and premultiplied ink `P` over its footprint.
pub(crate) struct ColouredStroke {
    pub coverage: Vec<u16>,
    bounds: [usize; 4],
    field: Vec<f64>,
    ink: Vec<f64>,
}

impl ColouredStroke {
    /// The straight ink `S = P / F` at canvas pixel `(x, y)`, clamped to
    /// 0…1; 0 outside the footprint or where `F = 0`.
    pub(crate) fn ink_at(&self, x: usize, y: usize) -> [f64; 3] {
        let [x0, y0, x1, y1] = self.bounds;
        if !(x0..x1).contains(&x) || !(y0..y1).contains(&y) {
            return [0.; 3];
        }
        let at = (y - y0) * (x1 - x0) + (x - x0);
        let f = self.field[at];
        if f <= 0. {
            return [0.; 3];
        }
        std::array::from_fn(|ch| (self.ink[3 * at + ch] / f).clamp(0., 1.))
    }
}

/// The coloured stroke of `first` over `dabs` (optional shapes and texture,
/// output pixel (0, 0) at the texture's canvas origin), each dab in its
/// `inks` triple, its coverage multiplied by `dual`'s second field when given
/// (module doc, THE COLOURED STROKE). The caller has admitted the lists, the
/// shapes, the texture's tip and the scratch; `output_samples` is the first
/// plan's. Refuses an inks list [`check_inks`] refuses.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_coloured(
    first: &BrushRequest,
    dimensions: (u32, u32),
    dabs: &[f64],
    shapes: Option<&[f64]>,
    texture: Option<(&BrushTexture, (i64, i64))>,
    dual: Option<(&BrushRequest, &[f64])>,
    inks: &[f64],
    output_samples: usize,
    job: JobContext<'_>,
) -> Result<ColouredStroke, String> {
    check_inks(dabs, inks)?;
    // `field_with` lays its field over this same footprint.
    let [x0, y0, x1, y1] = footprint(dimensions, dabs).unwrap_or([0; 4]);
    let samples = 3 * (x1 - x0) * (y1 - y0);
    let mut ink = reserved_vec(samples)?;
    ink.resize(samples, 0_f64);
    let end = if dual.is_some() { 0.45 } else { 0.9 };
    let (field, bounds) = first.field_with(
        dimensions,
        dabs,
        shapes,
        texture,
        (0., end),
        job,
        |at, dab, c| {
            for ch in 0..3 {
                let p = &mut ink[3 * at + ch];
                *p += c * (inks[3 * dab + ch] - *p);
            }
        },
    )?;
    let coverage = match dual {
        None => first.quantized(dimensions, output_samples, &field, bounds, job)?,
        Some((second, dual_dabs)) => {
            let f2 = second.field(dimensions, dual_dabs, None, None, (0.45, 0.9), job)?;
            product(
                first,
                dimensions,
                output_samples,
                (&field, bounds),
                (&f2.0, f2.1),
                job,
            )?
        }
    };
    Ok(ColouredStroke {
        coverage,
        bounds,
        field,
        ink,
    })
}

fn unit(v: f64) -> bool {
    v.is_finite() && (0. ..=1.).contains(&v)
}
