//! KERNEL: a CMYK document's type, shape and fill artwork (#1) — the
//! component list in ink and the ONE ink assembly loop. The adapter that
//! publishes the result is `cmyk_artwork`.
//!
//! The components are `artwork_render`'s four with INK in place of colour:
//! `{"kind":"coverage","ink":[c,m,y,k,a]}` (one u16 coverage plane, the RGB
//! path's own `artwork_coverage` planes), `{"kind":"intrinsic_rgba8",
//! "opacity":o}` (one RGBA8 sRGB colour-glyph plane), both with the RGB
//! component's optional `window` (`artwork_window`), `{"kind":"fill",
//! "ink":[c,m,y,k,a]}` and `{"kind":"gradient","gradient":{GradientFill},
//! "inks":[[c,m,y,k],…]}` (with the RGB component's optional `box`),
//! every ink and alpha in [0, 1] (1 = 100 %), and
//! `{"kind":"pattern","fill":{…}}`, the RGB render's own
//! pattern component with no ink key. The bounds (16 MiB of JSON,
//! `artwork_document::MAX_LARGE_DESCRIPTION`, so a large custom tile is
//! admitted; 1–256 components; 64,000,000 samples per component), the gradient's parse
//! (`artwork_render::gradient_fill`: Strict, one entry per stop, stops listed
//! in non-decreasing position) and the pattern's (`artwork_render::
//! pattern_fill`, Strict) are the RGB render's.
//!
//! The loop is `artwork_render::assemble`'s walk on ink: per pixel, each
//! component in order composites Normal through `cmyk_composite::over`
//! (master `blend::composite_source_into`). A coverage component's alpha is
//! `checked_alpha(a, 1, coverage / 65535)` — the RGB path's own coverage,
//! so a type or shape edge is the RGB edge (C-same) and the interior is the
//! stored ink exactly; a fill is its ink at its alpha everywhere; a gradient
//! interpolates C, M and Y as one `GradientSampler::color_with` call
//! and K as a second (each stop's K in all three slots, channel 0 read), so
//! both use the one bracket and the one parameter `GradientSampler::t` over
//! the raster `(0, 0, width, height)` or the component's `box`
//! (`artwork_render::gradient_frame`), the Classic term held by
//! `style_gradient::fill_bound` as the RGB fill's is (unmeasured on
//! ink); an intrinsic glyph pixel composites
//! the ink its sRGB converts to (the caller's `intrinsic_ink`, the document
//! profile's relative + BPC leg) at `opacity × A / 255`. A pattern's tile is
//! separated texel by texel by the caller's `texel_ink` (the Pattern
//! Overlay's leg, `cmyk_style::TexelInk`: sRGB → ink, on the document's grid)
//! into one ink sampler per component (`pattern_tile::ink_sampler`, placed
//! at `PatternFill::origin((0, 0))`, as the RGB render places it), and each
//! pixel composites the sampled ink at its alpha; its level's bytes are
//! [`pattern_scratch`]. A pattern's fill is parsed ONCE: [`cmyk_components`]
//! keeps it beside the JSON (`parsed`), and [`pattern_scratch`] and
//! [`assemble_cmyk`] read it there, parsing only a component built without
//! one. The finished sample is `cmyk_composite::codes`.

use crate::artwork_render::{gradient_fill, gradient_frame, pattern_fill, MAX_COMPONENTS};
use crate::artwork_window::{totals, Window};
use crate::cmyk_composite::{codes, over};
use crate::job_control::JobContext;
use crate::pattern_tile::{ink_sampler, ink_scratch_bytes, InkSampler};
use crate::precision_image::{pixel_count, reserved_vec};
use crate::precision_render::checked_alpha;
use crate::style_gradient::{fill_bound, GradientSampler};
use crate::style_pattern_fill::PatternFill;
use serde::Deserialize;

/// One ordered ink component (module doc).
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum CmykComponent {
    Coverage {
        ink: [f32; 5],
        /// The RGB component's `window` (`artwork_window`); absent: the
        /// raster.
        #[serde(default)]
        window: Option<[i64; 4]>,
    },
    IntrinsicRgba8 {
        #[serde(default = "full_opacity")]
        opacity: f32,
        /// As `Coverage`'s.
        #[serde(default)]
        window: Option<[i64; 4]>,
    },
    Fill {
        ink: [f32; 5],
    },
    Gradient {
        gradient: serde_json::Value,
        inks: Vec<[f32; 4]>,
        /// The RGB component's `box` (`artwork_render::gradient_frame`);
        /// absent: the raster.
        #[serde(default, rename = "box")]
        frame: Option<[i64; 4]>,
    },
    /// The RGB render's pattern component: `fill` the pattern fill's keys,
    /// its texels separated at render (module doc). No ink key. `parsed` is
    /// `fill` parsed once by [`cmyk_components`] (module doc).
    Pattern {
        fill: serde_json::Value,
        #[serde(skip)]
        parsed: Option<Box<PatternFill>>,
    },
}

fn full_opacity() -> f32 {
    1.0
}

/// A gradient's stops as the three-entry list `gradient_fill` counts.
fn stop_list(inks: &[[f32; 4]]) -> Vec<[f32; 3]> {
    inks.iter().map(|ink| [ink[0], ink[1], ink[2]]).collect()
}

/// The ordered components of `json`, validated as `artwork_render::components`
/// validates the RGB list (module doc), every ink and alpha finite in [0, 1].
pub(crate) fn cmyk_components(json: &str) -> Result<Vec<CmykComponent>, String> {
    if json.len() > crate::artwork_document::MAX_LARGE_DESCRIPTION {
        return Err("artwork component description exceeds 16 MiB".into());
    }
    let mut list: Vec<CmykComponent> =
        serde_json::from_str(json).map_err(|e| format!("invalid CMYK artwork components: {e}"))?;
    if list.is_empty() || list.len() > MAX_COMPONENTS {
        return Err("artwork requires between 1 and 256 ordered components".into());
    }
    let unit = |v: &f32| v.is_finite() && (0.0..=1.0).contains(v);
    for component in &mut list {
        let valid = match component {
            CmykComponent::Coverage { ink, .. } | CmykComponent::Fill { ink } => {
                ink.iter().all(unit)
            }
            CmykComponent::IntrinsicRgba8 { opacity, .. } => {
                if !unit(opacity) {
                    return Err("intrinsic glyph opacity must be normalized".into());
                }
                true
            }
            CmykComponent::Gradient {
                gradient,
                inks,
                frame,
            } => {
                gradient_fill(gradient, &stop_list(inks))?;
                gradient_frame(*frame, 1, 1)?;
                inks.iter().all(|ink| ink.iter().all(unit))
            }
            CmykComponent::Pattern { fill, parsed } => {
                *parsed = Some(Box::new(pattern_fill(fill)?));
                true
            }
        };
        if !valid {
            return Err("CMYK artwork ink must be finite with every ink and alpha in 0…1".into());
        }
    }
    Ok(list)
}

/// A pattern component's fill: the one [`cmyk_components`] parsed, else
/// parsed now (module doc).
fn parsed_fill<'a>(
    fill: &serde_json::Value,
    parsed: &'a Option<Box<PatternFill>>,
) -> Result<std::borrow::Cow<'a, PatternFill>, String> {
    match parsed {
        Some(parsed) => Ok(std::borrow::Cow::Borrowed(parsed)),
        None => pattern_fill(fill).map(std::borrow::Cow::Owned),
    }
}

/// The bytes the pattern components' ink samplers allocate (module doc):
/// [`ink_scratch_bytes`] per pattern component, each built once. A fill that
/// does not parse counts nothing ([`cmyk_components`] refuses it).
pub(crate) fn pattern_scratch(components: &[CmykComponent]) -> u64 {
    components
        .iter()
        .filter_map(|c| match c {
            CmykComponent::Pattern { fill, parsed } => parsed_fill(fill, parsed).ok(),
            _ => None,
        })
        .map(|fill| {
            let tile = &fill.pattern.tile;
            ink_scratch_bytes(
                tile.width,
                tile.height,
                f64::from(fill.scale),
                f64::from(fill.angle),
            )
        })
        .sum()
}

/// Each plane-owning component's window over a `width × height` raster
/// (`artwork_window`), `None` for the planeless ones: what the FFI recomputes
/// its slice lengths from (`artwork_window::totals`) and the loop reads by.
pub(crate) fn cmyk_windows(
    components: &[CmykComponent],
    width: u32,
    height: u32,
) -> Result<Vec<Option<(Window, bool)>>, String> {
    components
        .iter()
        .map(|c| match c {
            CmykComponent::Coverage { window, .. } => {
                Window::plane(*window, (width, height), false)
            }
            CmykComponent::IntrinsicRgba8 { window, .. } => {
                Window::plane(*window, (width, height), true)
            }
            _ => Ok(None),
        })
        .collect()
}

/// The ONE ink assembly loop (module doc): straight CMYKA16, row-major.
/// `samples` holds one coverage plane per coverage component over its window
/// (`artwork_window`), component-major; `intrinsic` one RGBA8 plane per
/// intrinsic component, likewise. `intrinsic_ink` answers a glyph pixel's
/// sRGB as `[c, m, y, k]`; `texel_ink` a pattern texel's sRGB, on the
/// document's grid.
#[allow(clippy::too_many_arguments)]
pub(crate) fn assemble_cmyk(
    width: u32,
    height: u32,
    components: &[CmykComponent],
    samples: &[u16],
    intrinsic: &[u8],
    intrinsic_ink: &mut dyn FnMut([u8; 3]) -> [f32; 4],
    texel_ink: &mut dyn FnMut([u8; 3]) -> [f32; 4],
    job: JobContext<'_>,
) -> Result<Vec<[u16; 5]>, String> {
    let n = pixel_count(width, height)?;
    let windows = cmyk_windows(components, width, height)?;
    if totals(windows.iter().flatten().copied()) != (samples.len(), intrinsic.len()) {
        return Err("artwork samples do not match their bounded component layout".into());
    }
    // Each component's plane offset, and a gradient's sampler over the raster.
    let mut offsets = Vec::with_capacity(components.len());
    let mut samplers: Vec<Option<GradientSampler>> = Vec::with_capacity(components.len());
    let mut patterns: Vec<Option<InkSampler>> = Vec::with_capacity(components.len());
    let (mut plane, mut asset) = (0, 0);
    for (component, window) in components.iter().zip(&windows) {
        let pixels = window.map_or(0, |(w, _)| w.pixels());
        patterns.push(match component {
            CmykComponent::Pattern { fill, parsed } => {
                let fill = parsed_fill(fill, parsed)?;
                Some(ink_sampler(
                    &fill.pattern.tile,
                    f64::from(fill.scale),
                    f64::from(fill.angle),
                    fill.origin((0, 0)),
                    texel_ink,
                ))
            }
            _ => None,
        });
        match component {
            CmykComponent::Coverage { .. } => {
                offsets.push(plane);
                plane += pixels;
                samplers.push(None);
            }
            CmykComponent::IntrinsicRgba8 { .. } => {
                offsets.push(asset);
                asset += pixels * 4;
                samplers.push(None);
            }
            CmykComponent::Fill { .. } | CmykComponent::Pattern { .. } => {
                offsets.push(0);
                samplers.push(None);
            }
            CmykComponent::Gradient {
                gradient,
                inks,
                frame,
            } => {
                let fill = gradient_fill(gradient, &stop_list(inks))?;
                let (bx, by, bw, bh) = gradient_frame(*frame, width, height)?;
                offsets.push(0);
                samplers.push(Some(
                    GradientSampler::new(&fill, bx, by, bw, bh)
                        .with_classic_bound(fill_bound(fill.style)),
                ));
            }
        }
    }
    let mut out = reserved_vec(n)?;
    let w = width as usize;
    let (mut px, mut py) = (0usize, 0usize);
    for i in 0..n {
        job.every(i)?;
        let mut pixel = [0.0f32; 5];
        for (index, component) in components.iter().enumerate() {
            let offset = offsets[index];
            // Outside its window a plane reads 0 (`artwork_window`).
            let at = windows[index].and_then(|(window, _)| window.at(px, py));
            match component {
                CmykComponent::Coverage { ink, .. } => {
                    let coverage = at.map_or(0, |k| samples[offset + k]);
                    let alpha = checked_alpha(ink[4], 1.0, f32::from(coverage) / 65535.0)?;
                    over(&mut pixel, [ink[0], ink[1], ink[2], ink[3]], alpha);
                }
                CmykComponent::IntrinsicRgba8 { opacity, .. } => {
                    let s = at.map_or([0; 4], |k| {
                        let at = offset + k * 4;
                        [
                            intrinsic[at],
                            intrinsic[at + 1],
                            intrinsic[at + 2],
                            intrinsic[at + 3],
                        ]
                    });
                    let alpha = checked_alpha(*opacity, 1.0, f32::from(s[3]) / 255.0)?;
                    if alpha > 0.0 {
                        over(&mut pixel, intrinsic_ink([s[0], s[1], s[2]]), alpha);
                    }
                }
                CmykComponent::Fill { ink } => {
                    let alpha = checked_alpha(ink[4], 1.0, 1.0)?;
                    over(&mut pixel, [ink[0], ink[1], ink[2], ink[3]], alpha);
                }
                CmykComponent::Gradient { inks, .. } => {
                    let Some(sampler) = samplers[index].as_ref() else {
                        return Err("artwork gradient component has no sampler".into());
                    };
                    let t = sampler.t(px as f32 + 0.5, py as f32 + 0.5);
                    let (cmy, opacity) =
                        sampler.color_with(t, |k| [inks[k][0], inks[k][1], inks[k][2]]);
                    let (k, _) = sampler.color_with(t, |k| [inks[k][3]; 3]);
                    let alpha = checked_alpha(opacity, 1.0, 1.0)?;
                    over(&mut pixel, [cmy[0], cmy[1], cmy[2], k[0]], alpha);
                }
                CmykComponent::Pattern { .. } => {
                    let Some(sampler) = patterns[index].as_ref() else {
                        return Err("artwork pattern component has no sampler".into());
                    };
                    let s = sampler.at(px as i64, py as i64);
                    let alpha = checked_alpha(s[4], 1.0, 1.0)?;
                    if alpha > 0.0 {
                        over(&mut pixel, [s[0], s[1], s[2], s[3]], alpha);
                    }
                }
            }
        }
        out.push(codes(pixel));
        px += 1;
        if px == w {
            (px, py) = (0, py + 1);
        }
    }
    Ok(out)
}
