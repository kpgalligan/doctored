//! Edit ▸ Sky Replacement's pixel math (M2a): the sky mask's Shift and
//! Fade Edge, the replacement sky's adjustments and placement, the
//! foreground's harmonization toward it, and the one composite that makes the
//! "Duplicate Layer" output. Free functions over one canvas-sized picture;
//! the adapters (`doc_sky_replace`) decide what a sample is. Steps 1–5 are
//! [`prepare`], shared with the "New Layers" output (M2b, `sky_group`), whose
//! layers composite to step 6–7's result.
//!
//! Every value here is display-encoded RGB in 0…1: a byte code over 255 or a
//! u16 code over 65535 (the command is disabled at 32 bits, so there is no
//! float arm). The sky is RGBA8 over 255 and its alpha is ignored: a sky is a
//! photograph, and a transparent corner in a user's file must not punch a
//! hole in the composite.
//!
//! # The steps, in order
//!
//! 1. **Coverage.** `m0` is the sky model's coverage over 65535. The edge
//!    widths are fractions of `R = clamp(Lc/64, 2, 64)·L/Lc` ([`edge_radii`]),
//!    where `Lc` is the coverage's long side and `L` the processed picture's:
//!    the clamp is in CANVAS pixels, so a reduced preview gets the commit's
//!    widths scaled to its own grid. `m` is `precision_selection_refine::
//!    refine` of `m0` with shift `shift_edge/100·R` (positive grows the sky)
//!    and feather `fade_edge/100·R` — Select ▸ Modify's own kernels.
//! 2. **Edge band.** With Edge Lighting on, `e` is the same refine of `m`
//!    with feather `R_e = clamp(Lc/32, 2, 64)·L/Lc`, and the band is
//!    `b = max(0, e − m)`: the foreground just inside the horizon.
//! 3. **Sky adjustments.** Brightness and Temperature are Develop Filter's
//!    Exposure (`brightness/50` stops) and Temperature on the sky at its own
//!    size ([`adjusted_sky`]); both 0 skips the pass.
//! 4. **Placement.** The sky covers the canvas at `s = max(W/sw, H/sh)` times
//!    Scale, centred horizontally and anchored at the top, moved by `offset`
//!    in canvas fractions, mirrored under Flip ([`SkyPlacement`]). Framing
//!    depends only on canvas fractions, so a reduced sky frames identically.
//! 5. **Statistics.** With weights `w = m·a`, `K̄` is the placed sky's mean and
//!    `Ō` the replaced sky's (the foreground under the mask). The cast shift
//!    is `d = (K̄ − Y(K̄)) − (Ō − Y(Ō))`: the change in the sky's chroma, with
//!    `Y` from `blend::LUMA_*` ([`harmony`]).
//! 6. **Foreground** ([`pixel`]): `F1 = F + color·COLOR_TRANSFER·d`, clamped
//!    to 0…1 (as the New Layers output's Curves layer clamps it), then
//!    `F2 = F1 + lighting·(B(F1, K̄) − F1)` with `B` Multiply or Screen from
//!    `blend::blend_kind`, then `F3 = F2 + edge·b·(K − F2)`. `color` and
//!    `lighting` are the sliders' bytes (`opacity_byte::quantized`, #578):
//!    the New Layers output carries them as layer opacities, which composite
//!    at their byte (`opacity_byte`), so both outputs read the same weights.
//! 7. **Composite.** `o = m + (1 − m)·a`, `RGB = (m·K + (1 − m)·a·F3)/o`
//!    (0 when `o = 0`), alpha `o`, clamped to 0…1; the adapter quantizes once.
//!
//! With `decontaminate` on, [`prepare`] also builds
//! `sky_decontaminate::Decontamination` from the source and `m0`, and step
//! 6's `F` is [`Prepared::foreground`]: the old sky's colour solved out of the
//! edge band's pixels, so a dark new sky shows no light halo. Off (the
//! default, and every params object without the key) every output is the
//! one above, unchanged.

use serde_json::Value;

use crate::blend::{blend_kind, BlendKind, BlendMode, LUMA_B, LUMA_G, LUMA_R};
use crate::filter_kernels::SampleRange;
use crate::job_control::JobContext;
use crate::precision_selection_refine::refine;
use crate::sampling_develop_filter::DevelopFilter;
use crate::sampling_radial::sample_exact;
use crate::sky_decontaminate::Decontamination;

/// The parameters' byte cap, checked before parsing.
pub(crate) const MAX_PARAMS_BYTES: usize = 4096;
/// The output layer name's byte cap.
const MAX_NAME_BYTES: usize = 255;
/// The default output layer name.
pub(crate) const DEFAULT_NAME: &str = "Sky Replacement";
/// The kernel's sentence for a picture with no sky weight; the adapters name
/// the layer (`doc_sky_replace`).
pub(crate) const NO_SKY: &str = "sky replacement found no sky";

/// How much of the sky's chroma change a full Color Adjustment carries into
/// the foreground. Half, because the foreground is lit by the sky but also by
/// the sun and by bounce light the sky does not change: a full transfer
/// paints a sunset's orange onto a white wall as strongly as onto the clouds,
/// which reads as a colour filter rather than as light. At 0.5 a 100 setting
/// moves a neutral foreground half way to the sky's cast and leaves its own
/// colours legible.
pub(crate) const COLOR_TRANSFER: f32 = 0.5;

/// The working set of one replacement per canvas pixel beyond its output:
/// the coverage `m0` (2), the refined mask `m` (2), refine's intermediate
/// stage plane (2) and the feather's own f32 pass (4), the edge band's
/// refined plane (2) and its intermediate (2), and the 8-bit canvas image
/// the byte adapter reads (4; the typed adapter reads its record in place):
/// 18, first rounded up to 20.
///
/// Raised to 38 by the measured admission
/// (`scripts/sky-replacement-admission/measure.py`, 2026-10-15: the Release
/// app at `DR_WORKING_LIMIT_BYTES` 8 GiB, `MallocLargeCache=0`, one case per
/// process beside a baseline twin at E ≤ 49,152, `sky_replacement` with
/// fade 3, lighting 50, color 50 and edge lighting 50 on sky.jpg at
/// 6000×4000 with a 3000×1139 library sky; P = max(the M1 mask plan,
/// `planned_bytes` + the prepared display 480,000,000)). At 20 it missed:
/// 8 bits E 1,026,343,704 / 1,027,031,832 / 1,026,409,240 against P
/// 1,183,206,304 (need 1,283,789,790); 16 bits E 1,109,427,016 /
/// 1,109,033,752 / 1,337,705,408 against P 1,279,206,304 (need
/// 1,672,131,760). The call's own host terms (the Swift coverage array, the
/// sky decode, the retained new layer) ride on this per-pixel figure. At 38
/// the 16-bit P is 1,711,206,304 ≥ 1,672,131,760 and the 8-bit P
/// 1,615,206,304 ≥ 1,283,789,790. The confirming run at 38 passed: 8 bits
/// E 1,025,819,416 / 1,026,507,568 / 804,012,656 (need 1,283,134,460,
/// P 1,615,206,304); 16 bits E 1,337,164,760 / 1,338,065,856 /
/// 1,109,132,104 (need 1,672,582,320, P 1,711,206,304). The 16-bit E is
/// bimodal (about 1.11 GB or 1.34 GB per run), and on its high mode the
/// margin is thin: P exceeds the need by 2.3 %.
///
/// Raised to 57 when the replacement's own plan had to cover its need
/// without the prepared display beside it (the same harness, its gate
/// now P = max(the M1 mask plan, `planned_bytes`), 2026-10-15, all four
/// cases, 3 runs). At 38 every case missed: Duplicate Layer 8 bits E
/// 1,026,228,968 / 791,659,024 / 791,544,336 against P 1,135,206,304 (need
/// 1,282,786,210), 16 bits E 1,103,512,320 / 1,103,184,640 / 1,338,344,408
/// against P 1,231,206,304 (need 1,672,930,510); New Layers 8 bits need
/// 1,283,625,950 against P 1,207,206,304, 16 bits need 1,672,725,710
/// against P 1,375,206,304. 57 is the least whole value clearing the 16-bit
/// Duplicate Layer need (P 1,687,206,304); at 57 New Layers' P is
/// 1,663,206,304 and 1,831,206,304, so `sky_group`'s
/// `NEW_LAYERS_BYTES_8|16` stay. The confirming run at 57 passed all four:
/// Duplicate Layer 16 bits E 1,337,803,712 / 1,337,885,632 / 1,103,135,488
/// (need 1,672,357,040, P 1,687,206,304), 8 bits need 1,282,642,850
/// (P 1,591,206,304); New Layers 16 bits need 1,672,213,680
/// (P 1,831,206,304), 8 bits need 1,282,663,330 (P 1,663,206,304).
const SCRATCH_BYTES_PER_PIXEL: u64 = 57;
/// The working set per SKY pixel: the f32 sky the placement samples (16),
/// Develop Filter's toned plane while the adjustments run (16) and the RGBA8
/// source (4).
const SKY_BYTES_PER_PIXEL: u64 = 36;
/// Bookkeeping beside the planes (statistics, job state, vectors' slack).
const BOOKKEEPING_BYTES: u64 = 4 * 1024 * 1024;

/// Output To: one flattened "Duplicate Layer", or an editable "New Layers"
/// group (`sky_group`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SkyOutput {
    DuplicateLayer,
    NewLayers,
}

/// Multiply or Screen: the Lighting Mode of the foreground's lighting pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LightingMode {
    Multiply,
    Screen,
}

/// The parsed parameters (the header's `params_json`), each in its own
/// units; `offset` is in canvas fractions.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SkyParams {
    pub shift_edge: f32,
    pub fade_edge: f32,
    pub brightness: f32,
    pub temperature: f32,
    pub scale: f32,
    pub flip: bool,
    pub offset: [f32; 2],
    pub lighting_mode: LightingMode,
    pub lighting: f32,
    pub color_adjustment: f32,
    pub edge_lighting: f32,
    pub output: SkyOutput,
    pub name: String,
    pub decontaminate: bool,
}

impl Default for SkyParams {
    fn default() -> Self {
        Self {
            shift_edge: 0.0,
            fade_edge: 3.0,
            brightness: 0.0,
            temperature: 0.0,
            scale: 100.0,
            flip: false,
            offset: [0.0, 0.0],
            lighting_mode: LightingMode::Multiply,
            lighting: 50.0,
            color_adjustment: 50.0,
            edge_lighting: 0.0,
            output: SkyOutput::DuplicateLayer,
            name: DEFAULT_NAME.into(),
            decontaminate: false,
        }
    }
}

/// One finite number of `key` within `range`, else `Err` naming the key.
fn number(key: &str, value: &Value, range: (f64, f64)) -> Result<f32, String> {
    let v = value
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("sky replacement {key} must be a number"))?;
    if v < range.0 || v > range.1 {
        return Err(format!(
            "sky replacement {key} must be within {}…{}",
            range.0, range.1
        ));
    }
    Ok(v as f32)
}

impl SkyParams {
    /// The strict JSON object of the header: every key optional with its
    /// default, an unknown key or an out-of-range value `Err` naming the key,
    /// at most [`MAX_PARAMS_BYTES`]. An absent `name` is [`DEFAULT_NAME`] for
    /// Duplicate Layer and `sky_group::GROUP_NAME` for New Layers.
    pub(crate) fn parse(json: &str) -> Result<SkyParams, String> {
        if json.len() > MAX_PARAMS_BYTES {
            return Err("sky replacement params exceed 4096 bytes".into());
        }
        let value: Value = serde_json::from_str(json)
            .map_err(|e| format!("sky replacement params are not valid JSON: {e}"))?;
        let object = value
            .as_object()
            .ok_or("sky replacement params must be a JSON object")?;
        let mut p = SkyParams::default();
        let mut named = false;
        for (key, value) in object {
            match key.as_str() {
                "shift_edge" => p.shift_edge = number(key, value, (-100.0, 100.0))?,
                "fade_edge" => p.fade_edge = number(key, value, (0.0, 100.0))?,
                "brightness" => p.brightness = number(key, value, (-100.0, 100.0))?,
                "temperature" => p.temperature = number(key, value, (-100.0, 100.0))?,
                "scale" => p.scale = number(key, value, (25.0, 400.0))?,
                "lighting" => p.lighting = number(key, value, (0.0, 100.0))?,
                "color_adjustment" => p.color_adjustment = number(key, value, (0.0, 100.0))?,
                "edge_lighting" => p.edge_lighting = number(key, value, (0.0, 100.0))?,
                "flip" => {
                    p.flip = value
                        .as_bool()
                        .ok_or("sky replacement flip must be a boolean")?
                }
                "decontaminate" => {
                    p.decontaminate = value
                        .as_bool()
                        .ok_or("sky replacement decontaminate must be a boolean")?
                }
                "offset" => {
                    let pair = value
                        .as_array()
                        .filter(|a| a.len() == 2)
                        .ok_or("sky replacement offset must be [fx, fy]")?;
                    p.offset = [
                        number(key, &pair[0], (-1.0, 1.0))?,
                        number(key, &pair[1], (-1.0, 1.0))?,
                    ];
                }
                "lighting_mode" => {
                    p.lighting_mode = match value.as_str() {
                        Some("multiply") => LightingMode::Multiply,
                        Some("screen") => LightingMode::Screen,
                        _ => {
                            return Err(
                                "sky replacement lighting_mode must be multiply or screen".into()
                            )
                        }
                    }
                }
                "output" => {
                    p.output = match value.as_str() {
                        Some("duplicate_layer") => SkyOutput::DuplicateLayer,
                        Some("new_layers") => SkyOutput::NewLayers,
                        _ => {
                            return Err(
                                "sky replacement output must be duplicate_layer or new_layers"
                                    .into(),
                            )
                        }
                    }
                }
                "name" => {
                    named = true;
                    let name = value
                        .as_str()
                        .ok_or("sky replacement name must be a string")?;
                    if name.len() > MAX_NAME_BYTES {
                        return Err("sky replacement name exceeds 255 bytes".into());
                    }
                    p.name = name.to_string();
                }
                other => return Err(format!("sky replacement has no parameter {other}")),
            }
        }
        if !named && p.output == SkyOutput::NewLayers {
            p.name = crate::sky_group::GROUP_NAME.into();
        }
        Ok(p)
    }
}

/// `(shift, fade, edge light)` radii in pixels of the `dimensions` grid
/// (module doc, steps 1 and 2): `R = clamp(Lc/64, 2, 64)·L/Lc` and `R_e =
/// clamp(Lc/32, 2, 64)·L/Lc` with `Lc` the coverage's long side and `L` the
/// grid's. The doc ops pass the canvas for both. Each radius is held within
/// refine's ±64 so a grid LARGER than its coverage (never the case in the
/// app) cannot ask for a refused width.
pub(crate) fn edge_radii(
    dimensions: (u32, u32),
    coverage: (u32, u32),
    p: &SkyParams,
) -> (f32, f32, f32) {
    let lc = coverage.0.max(coverage.1).max(1) as f32;
    let l = dimensions.0.max(dimensions.1) as f32;
    let scale = l / lc;
    let r = (lc / 64.0).clamp(2.0, 64.0) * scale;
    let re = (lc / 32.0).clamp(2.0, 64.0) * scale;
    (
        (p.shift_edge / 100.0 * r).clamp(-64.0, 64.0),
        (p.fade_edge / 100.0 * r).min(64.0),
        re.min(64.0),
    )
}

/// The working set of one replacement on a `dimensions` canvas with a `sky`
/// of its own size, writing `output_bytes_per_pixel` per canvas pixel (4 at 8
/// bits, 8 at 16); 0 for a zero size.
pub(crate) fn scratch_bytes(
    dimensions: (u32, u32),
    sky: (u32, u32),
    output_bytes_per_pixel: u64,
) -> u64 {
    let n = u64::from(dimensions.0) * u64::from(dimensions.1);
    let k = u64::from(sky.0) * u64::from(sky.1);
    if n == 0 || k == 0 {
        return 0;
    }
    n.saturating_mul(SCRATCH_BYTES_PER_PIXEL + output_bytes_per_pixel)
        .saturating_add(k.saturating_mul(SKY_BYTES_PER_PIXEL))
        .saturating_add(BOOKKEEPING_BYTES)
}

/// The coverage `coverage` (`grid` sized) read at each pixel centre of a
/// `dimensions` picture with `sampling_radial::sample_exact`'s bilinear
/// tent, as u16 codes; the codes themselves when the sizes match.
pub(crate) fn coverage_at(
    coverage: &[u16],
    grid: (u32, u32),
    dimensions: (u32, u32),
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    if grid == dimensions {
        return Ok(coverage.to_vec());
    }
    let (w, h) = (dimensions.0 as usize, dimensions.1 as usize);
    let (gw, gh) = (grid.0 as usize, grid.1 as usize);
    let read = |i: usize| {
        let c = f32::from(coverage[i]) / 65535.0;
        [c, c, c, 1.0]
    };
    let (sx, sy) = (gw as f64 / w as f64, gh as f64 / h as f64);
    let mut out = Vec::new();
    out.try_reserve_exact(w * h)
        .map_err(|_| "could not allocate the sky coverage".to_string())?;
    for y in 0..h {
        for x in 0..w {
            job.every(y * w + x)?;
            let q = ((x as f64 + 0.5) * sx, (y as f64 + 0.5) * sy);
            let v = sample_exact(&read, (gw, gh), q, true)[0];
            out.push((v * 65535.0).round().clamp(0.0, 65535.0) as u16);
        }
    }
    Ok(out)
}

/// The sky as straight f32 RGB in 0…1 with alpha 1, Brightness and
/// Temperature applied by Develop Filter's own stages (module doc, step
/// 3); the plain conversion when both are 0.
pub(crate) fn adjusted_sky(
    rgba8: &[u8],
    dims: (u32, u32),
    p: &SkyParams,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let n = dims.0 as usize * dims.1 as usize;
    if rgba8.len() != n * 4 {
        return Err("sky replacement sky has the wrong length".into());
    }
    let read = |i: usize| {
        let at = i * 4;
        [
            f32::from(rgba8[at]) / 255.0,
            f32::from(rgba8[at + 1]) / 255.0,
            f32::from(rgba8[at + 2]) / 255.0,
            1.0,
        ]
    };
    if p.brightness == 0.0 && p.temperature == 0.0 {
        let mut out = Vec::new();
        out.try_reserve_exact(n)
            .map_err(|_| "could not allocate the sky".to_string())?;
        for i in 0..n {
            job.every(i)?;
            out.push(read(i));
        }
        return Ok(out);
    }
    let raw = DevelopFilter {
        temperature: f64::from(p.temperature),
        tint: 0.0,
        exposure: f64::from(p.brightness) / 50.0,
        contrast: 0.0,
        highlights: 0.0,
        shadows: 0.0,
        whites: 0.0,
        blacks: 0.0,
        texture: 0.0,
        clarity: 0.0,
        dehaze: 0.0,
        vibrance: 0.0,
        saturation: 0.0,
    };
    raw.plane_in(read, dims, u64::MAX, &SampleRange::Unit, job)
}

/// The adjusted sky placed over a `dimensions` canvas (module doc, step 4).
pub(crate) struct SkyPlacement {
    sky: Vec<[f32; 4]>,
    sky_dims: (u32, u32),
    /// Canvas pixels per sky pixel.
    s: f64,
    /// The canvas point the sky's top centre lands on.
    origin: (f64, f64),
    flip: bool,
}

impl SkyPlacement {
    pub(crate) fn new(
        sky: Vec<[f32; 4]>,
        sky_dims: (u32, u32),
        dimensions: (u32, u32),
        p: &SkyParams,
    ) -> Self {
        let (w, h) = (f64::from(dimensions.0), f64::from(dimensions.1));
        let (sw, sh) = (f64::from(sky_dims.0), f64::from(sky_dims.1));
        let s = (w / sw).max(h / sh) * f64::from(p.scale) / 100.0;
        let origin = (
            w / 2.0 + f64::from(p.offset[0]) * w,
            f64::from(p.offset[1]) * h,
        );
        Self {
            sky,
            sky_dims,
            s,
            origin,
            flip: p.flip,
        }
    }

    /// The sky's RGB at canvas pixel `(x, y)`: the bilinear read at the sky
    /// position of the pixel centre, edges clamped. `sample_exact` takes
    /// continuous positions with pixel centres at half-integers, which is the
    /// space `u` and `v` are in.
    pub(crate) fn sample(&self, x: u32, y: u32) -> [f32; 3] {
        let (sw, sh) = (f64::from(self.sky_dims.0), f64::from(self.sky_dims.1));
        let mut u = (f64::from(x) + 0.5 - self.origin.0) / self.s + sw / 2.0;
        let v = (f64::from(y) + 0.5 - self.origin.1) / self.s;
        if self.flip {
            u = sw - u;
        }
        let read = |i: usize| self.sky[i];
        let k = sample_exact(&read, (sw as usize, sh as usize), (u, v), true);
        [k[0] as f32, k[1] as f32, k[2] as f32]
    }
}

/// The scene statistics the foreground is harmonized with (module doc, step
/// 5): the placed sky's weighted mean `K̄` and the raw chroma change `d`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Harmony {
    pub sky_mean: [f32; 3],
    pub cast_shift: [f32; 3],
}

fn luma(c: [f64; 3]) -> f64 {
    f64::from(LUMA_R) * c[0] + f64::from(LUMA_G) * c[1] + f64::from(LUMA_B) * c[2]
}

/// `K̄`, `Ō` and `d` over the picture with weights `m·a`; `Err(NO_SKY)` when
/// every weight is 0.
pub(crate) fn harmony(
    read: impl Fn(usize) -> [f32; 4],
    coverage: &[u16],
    placement: &SkyPlacement,
    dimensions: (u32, u32),
    job: JobContext<'_>,
) -> Result<Harmony, String> {
    let w = dimensions.0 as usize;
    let (mut total, mut sky, mut old) = (0.0f64, [0.0f64; 3], [0.0f64; 3]);
    for (i, &c) in coverage.iter().enumerate() {
        job.every(i)?;
        if c == 0 {
            continue;
        }
        let f = read(i);
        let weight = f64::from(c) / 65535.0 * f64::from(f[3]);
        if weight <= 0.0 {
            continue;
        }
        let k = placement.sample((i % w) as u32, (i / w) as u32);
        total += weight;
        for ch in 0..3 {
            sky[ch] += weight * f64::from(k[ch]);
            old[ch] += weight * f64::from(f[ch]);
        }
    }
    if total <= 0.0 {
        return Err(NO_SKY.into());
    }
    let sky = sky.map(|v| v / total);
    let old = old.map(|v| v / total);
    let (ys, yo) = (luma(sky), luma(old));
    Ok(Harmony {
        sky_mean: sky.map(|v| v as f32),
        cast_shift: std::array::from_fn(|ch| ((sky[ch] - ys) - (old[ch] - yo)) as f32),
    })
}

/// One output pixel (module doc, steps 6 and 7): foreground `fg` (straight,
/// alpha `fg[3]`), placed sky `sky`, refined mask `m` and edge band `band`,
/// both in 0…1.
pub(crate) fn pixel(
    fg: [f32; 4],
    sky: [f32; 3],
    m: f32,
    band: f32,
    h: &Harmony,
    p: &SkyParams,
) -> [f32; 4] {
    let a = fg[3];
    let o = m + (1.0 - m) * a;
    if o <= 0.0 {
        return [0.0; 4];
    }
    let blend: fn(f32, f32) -> f32 = match blend_kind(match p.lighting_mode {
        LightingMode::Multiply => BlendMode::Multiply,
        LightingMode::Screen => BlendMode::Screen,
    }) {
        BlendKind::Separable(b) => b,
        // Multiply and Screen are separable (`blend::blend_kind`).
        _ => |cb: f32, _| cb,
    };
    let color = crate::opacity_byte::quantized(p.color_adjustment / 100.0) * COLOR_TRANSFER;
    let lighting = crate::opacity_byte::quantized(p.lighting / 100.0);
    let edge = p.edge_lighting / 100.0 * band;
    let mut out = [0.0f32; 4];
    for c in 0..3 {
        let f1 = (fg[c] + color * h.cast_shift[c]).clamp(0.0, 1.0);
        let f2 = f1 + lighting * (blend(f1, h.sky_mean[c]) - f1);
        let f3 = f2 + edge * (sky[c] - f2);
        out[c] = ((m * sky[c] + (1.0 - m) * a * f3) / o).clamp(0.0, 1.0);
    }
    out[3] = o.clamp(0.0, 1.0);
    out
}

/// Steps 1–5 of the module doc, shared by both outputs: the refined mask
/// `m`, the edge-light plane `edge` (refine of `m`, present only with Edge
/// Lighting on), the placed adjusted sky, the scene statistics and, with
/// `decontaminate` on, the edge decontamination's grids.
pub(crate) struct Prepared {
    pub m: Vec<u16>,
    pub edge: Option<Vec<u16>>,
    pub placement: SkyPlacement,
    pub harmony: Harmony,
    pub decontamination: Option<Decontamination>,
}

impl Prepared {
    /// `(m, band)` at pixel `i`, both in 0…1: the refined mask and the edge
    /// band `max(0, e − m)` (0 without Edge Lighting).
    pub(crate) fn mask_at(&self, i: usize) -> (f32, f32) {
        let band = self.edge.as_ref().map_or(0.0, |e| {
            (f32::from(e[i]) - f32::from(self.m[i])).max(0.0) / 65535.0
        });
        (f32::from(self.m[i]) / 65535.0, band)
    }

    /// The foreground step 6 composites at pixel `i`: source `fg` with model
    /// coverage `m0`, decontaminated inside the band, `fg` as is otherwise
    /// or with `decontaminate` off.
    pub(crate) fn foreground(&self, i: usize, fg: [f32; 4], m0: u16) -> [f32; 4] {
        self.decontamination.as_ref().map_or(fg, |d| {
            let w = d.width();
            d.foreground((i % w) as u32, (i / w) as u32, fg, m0, self.m[i])
        })
    }

    /// Whether pixel `i` is in the decontaminated band (false with
    /// `decontaminate` off): the "Foreground Edge" layer's mask.
    pub(crate) fn in_edge(&self, i: usize, fg: [f32; 4], m0: u16) -> bool {
        self.decontamination.is_some() && Decontamination::in_band(m0, self.m[i], fg[3])
    }
}

/// Steps 1–5 over a `dimensions` picture read through `read`: `m0` is the
/// coverage already on this grid (`coverage_at`), `coverage` the size of the
/// grid it came from (for [`edge_radii`]). Reports through `job` up to 0.75;
/// the caller's per-pixel pass owns the rest.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare(
    read: impl Fn(usize) -> [f32; 4],
    dimensions: (u32, u32),
    m0: &[u16],
    coverage: (u32, u32),
    sky: &[u8],
    sky_dims: (u32, u32),
    p: &SkyParams,
    job: JobContext<'_>,
) -> Result<Prepared, String> {
    let (shift, fade, edge) = edge_radii(dimensions, coverage, p);
    let m = refine(m0, dimensions, 0.0, fade, shift, 0.0, job.range(0.0, 0.3))?;
    let edge = if p.edge_lighting > 0.0 {
        Some(refine(
            &m,
            dimensions,
            0.0,
            edge,
            0.0,
            0.0,
            job.range(0.3, 0.45),
        )?)
    } else {
        None
    };
    let adjusted = adjusted_sky(sky, sky_dims, p, job.range(0.45, 0.6))?;
    let placement = SkyPlacement::new(adjusted, sky_dims, dimensions, p);
    let harmony = harmony(&read, &m, &placement, dimensions, job.range(0.6, 0.7))?;
    let decontamination = if p.decontaminate {
        Some(Decontamination::new(
            &read,
            m0,
            dimensions,
            job.range(0.7, 0.75),
        )?)
    } else {
        None
    };
    Ok(Prepared {
        m,
        edge,
        placement,
        harmony,
        decontamination,
    })
}

/// The whole replacement over a `dimensions` picture read through `read`
/// ([`prepare`], then [`pixel`] per pixel). Each output pixel is handed to
/// `write` once, in order.
#[allow(clippy::too_many_arguments)]
pub(crate) fn replace(
    read: impl Fn(usize) -> [f32; 4],
    dimensions: (u32, u32),
    m0: &[u16],
    coverage: (u32, u32),
    sky: &[u8],
    sky_dims: (u32, u32),
    p: &SkyParams,
    job: JobContext<'_>,
    mut write: impl FnMut(usize, [f32; 4]),
) -> Result<(), String> {
    let prepared = prepare(&read, dimensions, m0, coverage, sky, sky_dims, p, job)?;
    let finish = job.range(0.75, 1.0);
    let w = dimensions.0 as usize;
    let rows = dimensions.1 as usize;
    for y in 0..rows {
        for x in 0..w {
            let i = y * w + x;
            finish.every(i)?;
            let (mi, band) = prepared.mask_at(i);
            let k = prepared.placement.sample(x as u32, y as u32);
            let fg = prepared.foreground(i, read(i), m0[i]);
            write(i, pixel(fg, k, mi, band, &prepared.harmony, p));
        }
        finish.report((y + 1) as f64 / rows as f64, 6)?;
    }
    Ok(())
}
