//! Ordered artwork components share the paint compositor and ICC transform.
//! Authored sRGB colors are continuous; intrinsic glyph pixels are explicitly
//! RGBA8 samples and cannot set the precision of authored ink.
//!
//! Capture/font shaping stays in the host. Composition and ICC conversion reuse the existing
//! precision kernels.
//!
//! # The 32-bit float render (187b)
//!
//! [`render`] and [`render_float`] are ONE assembly loop ([`assemble`]) with
//! two parameters fixed by Photoshop 26.10.0's measured 32-bit type and shape
//! layers (`scripts/photoshop-probe/float_artwork.py`, receipt
//! `doctored-fixtures/photoshop/float-artwork-473/take2`):
//!
//! - COLOUR, H-raw: at 32 bits a stored colour number `c` IS linear light,
//!   `c` taken as it is (SH32's fill and TX32's interior read `c/255` to
//!   3e-8 and 8.4e-4; sRGB decode missed by 0.29 and 0.23). The host hands
//!   [`render_float`] numbers in the document's linear ink, and they enter
//!   the document's primaries through the sRGB → document matrix with no
//!   decode (`PrecisionTransform::undecoded`). [`render`] (8/16 bits) decodes
//!   them through sRGB and encodes into the document (`PrecisionTransform::color`)
//!   as before. An H-raw colour on a non-sRGB float document is unmeasured;
//!   it takes the same matrix a float document's sRGB Fill ink takes.
//!   ABOVE WHITE: at 32 bits [`components`] admits red, green and
//!   blue in `0..=FLOAT_RANGE_MAX` (alpha stays in [0, 1]), because
//!   Photoshop 26.10.0 renders an `RGBFloatColor` (4, 0.5, 0.25) shape fill
//!   as exactly (4, 0.5, 0.25) and holds it as the foreground unscaled
//!   (`scripts/photoshop-probe/hdr_colour.py`, receipt
//!   `doctored-fixtures/photoshop/hdr-colour-317-608/take2`, row B1). The
//!   8/16-bit [`render`] keeps [0, 1].
//! - DOMAIN, H-lin: two inks mix in linear light (FS32's 56 stroke-edge
//!   pixels: every linear spread ≤ 6.7e-8, every encoded spread ≥ 0.24), so
//!   the float loop composites the linear values with `precision_paint::over`
//!   and writes them through `PrecisionImage::from_rgba32_extended`; the byte
//!   and u16 loop composites encoded values and `store`s u16 as before.
//! - COVERAGE, C-same: 32-bit antialias alpha equals 16-bit alpha (189 edge
//!   pixels, worst 1.5e-5), so alpha is `coverage / 65535` at every depth.
//! - Intrinsic RGBA8 colour-glyph pixels are platform image data, not stored
//!   numbers: at float they decode through sRGB (`PrecisionTransform::linear`),
//!   the way a placed image's pixels do, so a pasted or narrowed emoji keeps
//!   its look. Photoshop's 32-bit colour glyph is unmeasured.
//!
//! # LUT profiles
//!
//! An 8/16-bit document tagged with an RGB LUT profile has no matrix model.
//! Photoshop 26.10.0 converts type and shape colours into such a profile
//! (`scripts/photoshop-probe/lut_colours.py`, receipt
//! `doctored-fixtures/photoshop/lut-style-colours-100/take1`, B-APP), so
//! [`render`] converts every coverage colour and intrinsic RGBA8 pixel
//! through `authored_color::lut_ink` (relative colorimetric, BPC off) when
//! that pipeline converts both ways, and takes the numbers as they are when
//! it does not; it never refuses. [`render_float`] stays matrix-only: a
//! float document always has a model.
//!
//! # Fill layers
//!
//! Two components own no sample plane: `fill` composites one colour at full
//! coverage over the whole raster, and `gradient` composites a
//! `GradientFill` (the object `style_json::gradient` parses) spanning the
//! raster `(0, 0, width, height)`, or its optional `box` `[x, y, w, h]` in
//! raster pixels when present (a Gradient Fill layer's Align with Layer:
//! the host and the PSD import compute it by `fill_align_box::align_box`; a
//! width or height below 1, or a value outside `i32`, refuses): per pixel centre the parameter is
//! `GradientSampler::t` and the colour and opacity are
//! `GradientSampler::color_with` over `colors`, one RGB per stop in
//! the stops' order, converted once like a coverage colour, its Classic term
//! held by `style_gradient::fill_bound` (the bracket bound for Linear,
//! Radial, Angle and Reflected; measured at 8 and 16 bits, the same kernel
//! at 32). The stops' own
//! `#rrggbb` colours are not read: `colors` carries the continuous numbers
//! (linear light above white at 32 bits). The stops must be listed in
//! non-decreasing position, because the parser sorts them and `colors` is
//! matched to them by index.
//!
//! A `fill` may also hold `linear`: its colour as LINEAR-LIGHT
//! sRGB-primaries components, finite within ±`authored_color::LINEAR_LIMIT`
//! (a style slot's WIDE colour), the carriage of a Photoshop `SoCo`
//! colour outside sRGB. Where present it decides the colour and `color`'s RGB
//! is not read (its alpha is): at 8/16 bits it enters the document as
//! `authored_color::wide_into(AuthoredColors::into_document(profile), linear,
//! true)` — the document's numbers, built once per render and composited with
//! no further `Conversion` — and at 32 bits through the sRGB → document
//! matrix with no decode (`PrecisionTransform::undecoded`, `style_float`'s
//! rule for a wide colour). Its one high-depth encoder is
//! `authored_color::wide_scratch_bytes` ([`has_wide_fill`]).
//!
//! A `gradient` may hold `linear` too (a Photoshop `GdFl`
//! whose stops leave sRGB): one triple per stop in `colors`' order, each
//! finite within ±`LINEAR_LIMIT`. Where present each stop's numbers are the
//! `fill`'s linear conversion above (one helper, `wide_ink`), and the stops
//! interpolate between those numbers as between any others.
//!
//! # Pattern fill layers
//!
//! A third planeless component, `pattern`, tiles one pattern over the whole
//! raster: `fill` is the Pattern Overlay's keys (`style_pattern_fill::
//! parse_fill`, Strict), and every pixel (x, y) composites
//! `PatternFill::sampler((0, 0)).at(x, y)` at its own alpha. The fill's
//! `anchor` is the host's way of placing it: when the layer is linked the
//! host writes `anchor` = −(layer reference point), so `PatternFill::origin`
//! puts the tile's origin at phase + reference; unlinked (or linked with no
//! anchor) it sits at phase. The colour bypasses every conversion at every
//! ink: the tile's samples are document values (`pattern_tile`'s module doc),
//! and at 32 bits code / 255 is linear light (VERBATIM, as a 32-bit Pattern
//! Overlay reads it). An over-64 KiB description (a large custom tile) is
//! admitted up to `artwork_document::MAX_LARGE_DESCRIPTION`, and so is the
//! RGB component list; the level fits inside the FFI's existing
//! `text.len() * 32` scratch term.
//!
//! # The Lab twin (#1 Lab phase 6a-i)
//!
//! A Lab Color document's artwork renders here into the twin
//! (`lab_document::lab_twin`), with its colours still authored sRGB. The twin
//! has no matrix model, so coverage, fill and gradient stop colours, a
//! `linear` wide colour and intrinsic glyph pixels all take the LUT arm
//! above (`Conversion::Lut` = `authored_color::lut_ink` on the twin; `wide`
//! is `AuthoredColors::LabTwin`): relative colorimetric, no BPC, which is
//! Image ▸ Mode ▸ Lab Color's rule. A gradient interpolates between its
//! converted stops' L\*a\*b\* codes (Doctored's rule, no Photoshop take).
//! The one exception to "the colour bypasses every conversion" above: a
//! `pattern` on the twin reads its tile through `lab_style::
//! twin_tile_sampler`, each sRGB texel as 8-bit Lab codes before the pyramid
//! (the Lab Pattern Overlay's rule, 2f), falling back to
//! `PatternFill::sampler` when the twin pipeline cannot build.
//!
//! Masters: `PrecisionTransform::linear` / `::color` / `::undecoded`
//! (`icc_precision.rs`), `precision_paint::over`, `PrecisionImage::from_rgba32_extended`.
//! Paste into 32 bits and Image ▸ Mode 32 → 16/8 keep the description and
//! carry its bytes verbatim; the host reads them in the destination's ink
//! (take2 PT/PSH and NR: Photoshop rewrites the numbers so the layer looks the
//! same).
//!
//! # Admission
//!
//! [`render_float_scratch`] is the f32 output plane, `n · 16`; the host adds
//! its fitted per-pixel margin (`NativeArtworkEvaluation.floatMarginPerPixel`)
//! on top of it.
//!
//! # Measurement record (187b, 2026-10-09)
//!
//! 187a's method (`retained_source_float`'s record): a Release app with
//! `DR_WORKING_LIMIT_BYTES = 8589934592`, `MallocLargeCache=0`,
//! `DR_BACKGROUND=1`, private `DR_*_DIR`s and a private pasteboard, one fresh
//! instance per measured MCP call (8 s settle after setup); E =
//! `ri_interval_max_phys_footprint` − the footprint read right after
//! `proc_reset_footprint_interval`. Target: 187a's 6000×4000 float `scene`.
//! Cases: `add_shape_layer` rect 0,0,6000,4000 with a fill and an 8 px stroke
//! (a 6012×4012 raster, two coverage planes) and `edit_shape_layer`
//! recolouring it. A is what `nativeArtwork` admits: `scratchBytes` +
//! profile·64 = 48 MiB + 18n + 24w + 4·(2n) + 8n + K·n + 64·3144, n =
//! 24,120,144. Harness and raw output:
//! `doctored-fixtures/photoshop/float-artwork-473/admission/` (`measure.py`,
//! `runs/20261009-165306` built with K = 8, `runs/20261009-165853` built
//! with the final K = 15). The smallest K ≥ 8 (16 − 8) that admits
//! ⌈1.25 × max E⌉ over all six runs of each case is 15 (the K = 8 build's
//! add_shape peak 967,738,064 needs 1,209,672,580).
//!
//! | case | A (K = 15) | E (three runs, K = 15 build) | A / max E |
//! | --- | --- | --- | --- |
//! | add_shape | 1,232,564,208 | 963,625,656 / 964,788,920 / 964,870,840 | 1.277 |
//! | edit_shape | 1,232,564,208 | 584,221,080 / 585,777,560 / 587,366,808 | 2.098 |
//!
//! The K = 8 build read add_shape 963,740,368 / 967,738,064 / 963,543,760
//! and edit_shape 585,859,480 / 587,776,408 / 587,366,808. The
//! `list_documents` twin read 16,384 / 16,384 / 0 and 0 / 16,384 / 0: one
//! 16 KiB page, not the 0 × 3 187a read; it is noise against the
//! gigabyte-scale cases, recorded as measured.
//!
//! `scratchBytes`' fixed term is now 132 MiB, not the 48 MiB of this
//! record (the coverage scan's 524,288-edge buffers and its pair stage's
//! full-edge sweep): A is 88,080,384 above the table's figures, and the
//! measured E is what it was.
//!
//! # Component windows
//!
//! A `coverage` or `intrinsic_rgba8` component may declare `"window": [x, y,
//! w, h]` in raster pixels (`artwork_window`): its plane then holds only those
//! pixels, and outside them it reads as coverage 0 or a transparent pixel, so
//! the list renders as if its planes were whole-raster with the window's
//! samples embedded in zeros. Absent, the window is the raster. The bound is
//! [`MAX_COMPONENT_SAMPLES`] per component; there is no whole-layer total
//! (the host windows each path part to its bounds,
//! `NativeArtworkEvaluation`). The planeless components are unchanged.
use crate::artwork_window::{totals, Window};
use crate::authored_color::lut_ink;
use crate::icc::IccProfile;
use crate::icc_intent::ColorPipeline;
use crate::icc_transform::PrecisionTransform;
use crate::job_control::JobContext;
use crate::pattern_tile::PatternSampler;
use crate::precision_image::{pixel_count, reserved_vec, PrecisionImage};
use crate::precision_paint::{over, store};
use crate::style_gradient::{fill_bound, GradientSampler};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Component {
    Coverage {
        color: [f32; 4],
        /// The pixels its plane covers, `[x, y, w, h]` (module doc,
        /// "Component windows"); absent: the raster.
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
    /// One colour at full coverage over the whole raster (module doc, "Fill
    /// layers"). Owns no sample plane.
    Fill {
        color: [f32; 4],
        /// The colour as linear sRGB components beyond sRGB (module doc,
        /// "Fill layers"); absent for an sRGB colour.
        #[serde(default)]
        linear: Option<[f32; 3]>,
    },
    /// A `GradientFill` over the whole raster, `colors` its stops' RGB in
    /// order (module doc, "Fill layers"). Owns no sample plane.
    Gradient {
        gradient: serde_json::Value,
        colors: Vec<[f32; 3]>,
        /// Each stop's colour as linear sRGB components, in `colors`' order
        /// (module doc, "Fill layers"); absent for sRGB stops.
        #[serde(default)]
        linear: Option<Vec<[f32; 3]>>,
        /// The box the gradient spans, `[x, y, w, h]` in raster pixels
        /// (module doc, "Fill layers"; Align with Layer); absent: the
        /// raster.
        #[serde(default, rename = "box")]
        frame: Option<[i64; 4]>,
    },
    /// A pattern tiled over the whole raster, `fill` the pattern fill's keys
    /// (module doc, "Pattern fill layers"). Owns no sample plane.
    Pattern { fill: serde_json::Value },
}

/// The bounds every component list is held to (the RGB render's and
/// `cmyk_artwork_render`'s): the component count and each component's
/// coverage or intrinsic samples over its window (module doc, "Component
/// windows"). The JSON's bound is `artwork_document::MAX_LARGE_DESCRIPTION`
/// at both (module doc, "Pattern fill layers").
pub(crate) const MAX_COMPONENTS: usize = 256;
pub(crate) const MAX_COMPONENT_SAMPLES: usize = 64_000_000;

/// A `gradient` component's parsed fill: `style_json::gradient` in Strict
/// mode, with one colour per stop and the stops listed in non-decreasing
/// position (module doc, "Fill layers"). Shared with `cmyk_artwork_render`,
/// which hands one entry per ink.
pub(crate) fn gradient_fill(
    gradient: &serde_json::Value,
    colors: &[[f32; 3]],
) -> Result<crate::style::GradientFill, String> {
    let listed = gradient
        .get("stops")
        .and_then(serde_json::Value::as_array)
        .ok_or("artwork gradient requires a stops array")?;
    let mut previous = f64::NEG_INFINITY;
    for stop in listed {
        let position = stop
            .get("position")
            .and_then(serde_json::Value::as_f64)
            .ok_or("artwork gradient stops require a position")?;
        if position < previous {
            return Err("artwork gradient stops must be in non-decreasing position".into());
        }
        previous = position;
    }
    let mut holder = serde_json::Map::new();
    holder.insert("gradient".into(), gradient.clone());
    let fill =
        crate::style_json::gradient(&holder, "", "gradient", crate::style::Strictness::Strict)?;
    if fill.stops.len() != colors.len() {
        return Err("artwork gradient requires one color per stop".into());
    }
    Ok(fill)
}

/// The box `(x, y, w, h)` a `gradient` component spans (module doc, "Fill
/// layers"): its `box` when present, else the raster `(0, 0, width,
/// height)`. Refuses a box whose width or height is below 1 or whose any
/// value lies outside `i32`. Shared with `cmyk_artwork_render`.
pub(crate) fn gradient_frame(
    frame: Option<[i64; 4]>,
    width: u32,
    height: u32,
) -> Result<(f32, f32, f32, f32), String> {
    let Some([x, y, w, h]) = frame else {
        return Ok((0.0, 0.0, width as f32, height as f32));
    };
    let fits = |v: i64| i32::try_from(v).is_ok();
    if w < 1 || h < 1 || ![x, y, w, h].into_iter().all(fits) {
        return Err("artwork gradient box requires a positive width and height".into());
    }
    Ok((x as f32, y as f32, w as f32, h as f32))
}

/// A `pattern` component's parsed fill: `style_pattern_fill::parse_fill` in
/// Strict mode over the `fill` object (module doc, "Pattern fill layers").
pub(crate) fn pattern_fill(
    fill: &serde_json::Value,
) -> Result<crate::style_pattern_fill::PatternFill, String> {
    let object = fill
        .as_object()
        .ok_or("artwork pattern fill must be an object")?;
    crate::style_pattern_fill::parse_fill(object, "fill", crate::style::Strictness::Strict)
}

fn full_opacity() -> f32 {
    1.0
}

/// Whether a `fill` or `gradient` component holds a wide colour (module doc,
/// "Fill layers"): its render may build one high-depth encoder.
pub(crate) fn has_wide_fill(components: &[Component]) -> bool {
    components.iter().any(|c| {
        matches!(
            c,
            Component::Fill {
                linear: Some(_),
                ..
            } | Component::Gradient {
                linear: Some(_),
                ..
            }
        )
    })
}

/// The ordered components of `json`, at most
/// `artwork_document::MAX_LARGE_DESCRIPTION` bytes (a pattern's inline tile,
/// module doc). `extended` (the 32-bit float render)
/// admits a coverage colour's red, green and blue in `0..=FLOAT_RANGE_MAX`
/// (above white, module doc); otherwise every component is in [0, 1]. Alpha
/// is in [0, 1] either way.
pub fn components(json: &str, extended: bool) -> Result<Vec<Component>, String> {
    if json.len() > crate::artwork_document::MAX_LARGE_DESCRIPTION {
        return Err("artwork component description exceeds 16 MiB".into());
    }
    let c: Vec<Component> =
        serde_json::from_str(json).map_err(|e| format!("invalid artwork components: {e}"))?;
    if c.is_empty() || c.len() > MAX_COMPONENTS {
        return Err("artwork requires between 1 and 256 ordered components".into());
    }
    for component in &c {
        if let Component::IntrinsicRgba8 { opacity, .. } = component {
            if !opacity.is_finite() || !(0.0..=1.0).contains(opacity) {
                return Err("intrinsic glyph opacity must be normalized".into());
            }
        }
        // A coverage colour, a fill colour and each gradient stop colour
        // share one range rule; a gradient's alpha is its stops' opacity.
        if let Component::Fill {
            linear: Some(linear),
            ..
        } = component
        {
            let limit = crate::authored_color::LINEAR_LIMIT;
            if !linear.iter().all(|v| v.is_finite() && v.abs() <= limit) {
                return Err("artwork fill linear colour must be finite within ±4".into());
            }
        }
        if let Component::Gradient {
            colors,
            linear: Some(linear),
            ..
        } = component
        {
            let limit = crate::authored_color::LINEAR_LIMIT;
            if linear.len() != colors.len()
                || !linear
                    .iter()
                    .flatten()
                    .all(|v| v.is_finite() && v.abs() <= limit)
            {
                return Err(
                    "artwork gradient linear colours must match its stops and be finite within ±4"
                        .into(),
                );
            }
        }
        let checked: Vec<[f32; 4]> = match component {
            Component::Coverage { color, .. } | Component::Fill { color, .. } => vec![*color],
            Component::Gradient {
                gradient,
                colors,
                frame,
                ..
            } => {
                gradient_fill(gradient, colors)?;
                gradient_frame(*frame, 1, 1)?;
                colors.iter().map(|c| [c[0], c[1], c[2], 1.0]).collect()
            }
            Component::Pattern { fill } => {
                pattern_fill(fill)?;
                Vec::new()
            }
            Component::IntrinsicRgba8 { .. } => Vec::new(),
        };
        for color in &checked {
            let unit = |v: &f32| v.is_finite() && (0.0..=1.0).contains(v);
            if extended {
                let range = 0.0..=crate::float_document::FLOAT_RANGE_MAX;
                if !color[..3]
                    .iter()
                    .all(|v| v.is_finite() && range.contains(v))
                    || !unit(&color[3])
                {
                    return Err(
                        "float artwork color must be finite with red, green and blue \
                         in 0..=65504 and alpha in 0..=1"
                            .into(),
                    );
                }
            } else if !color.iter().all(unit) {
                return Err("artwork color must be finite normalized sRGB".into());
            }
        }
    }
    Ok(c)
}

/// How the assembly loop reads a component's colour and in which domain it
/// mixes (module doc).
#[derive(Clone, Copy)]
enum Ink {
    /// 8/16 bits: sRGB decode, then encoded into the document; mixed
    /// encoded, stored as u16.
    Encoded,
    /// 32 bits: the numbers taken as linear light (H-raw), mixed linear
    /// (H-lin), written as f32; intrinsic RGBA8 decoded through sRGB.
    Linear,
}

/// Where an authored artwork colour goes on its way into the document
/// (module doc, "LUT profiles"). Held once per render, so the larger LUT
/// variant is not boxed.
#[allow(clippy::large_enum_variant)]
enum Conversion {
    /// A matrix/TRC document: the shipped precision transform.
    Matrix(PrecisionTransform),
    /// An 8/16-bit document tagged with a LUT profile that converts both
    /// ways: `authored_color::lut_ink`.
    Lut(ColorPipeline),
    /// An 8/16-bit document tagged with a LUT profile that does not: the
    /// numbers as authored.
    Numbers,
}

impl Conversion {
    /// A coverage component's authored colour in the document's ink.
    fn coverage(&self, rgb: [f32; 3], ink: Ink) -> [f32; 3] {
        match (self, ink) {
            (Conversion::Matrix(t), Ink::Encoded) => t.color(rgb),
            (Conversion::Matrix(t), Ink::Linear) => t.undecoded(rgb).map(|v| v as f32),
            (Conversion::Lut(pipeline), _) => pipeline.color(rgb),
            (Conversion::Numbers, _) => rgb,
        }
    }

    /// An intrinsic RGBA8 pixel's colour in the document's ink.
    fn intrinsic(&self, rgb: [f32; 3], ink: Ink) -> [f32; 3] {
        match (self, ink) {
            (Conversion::Matrix(t), Ink::Encoded) => t.color(rgb),
            (Conversion::Matrix(t), Ink::Linear) => t.linear(rgb).map(|v| v as f32),
            (Conversion::Lut(pipeline), _) => pipeline.color(rgb),
            (Conversion::Numbers, _) => rgb,
        }
    }
}

/// A wide colour's numbers in the document's ink (module doc, "Fill
/// layers"), the ONE conversion a `fill`'s and a `gradient` stop's `linear`
/// take: at 8/16 bits `wide_into` of the render's into-document conversion
/// (`wide`) at high depth, at 32 bits the sRGB → document matrix with no
/// decode. `None` where neither applies, and the caller takes the colour's
/// sRGB numbers.
fn wide_ink(
    linear: [f32; 3],
    wide: Option<&crate::authored_color::AuthoredColors>,
    conversion: &Conversion,
    ink: Ink,
) -> Option<[f32; 3]> {
    match (conversion, ink) {
        (_, Ink::Encoded) => Some(crate::authored_color::wide_into(wide, linear, true)),
        (Conversion::Matrix(t), Ink::Linear) => Some(t.undecoded(linear).map(|v| v as f32)),
        _ => None,
    }
}

/// The u16 artwork raster in `profile` (8/16-bit documents).
pub(crate) fn render(
    width: u32,
    height: u32,
    components: &[Component],
    samples: &[u16],
    intrinsic: &[u8],
    profile: Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let out = assemble(
        width,
        height,
        components,
        samples,
        intrinsic,
        &profile,
        Ink::Encoded,
        job,
        store,
    )?;
    PrecisionImage::from_rgba16_unseeded(width, height, out, profile, None)
}

/// The f32 artwork raster of a 32-bit float document: `Rgba32Float`, linear
/// light in `profile`'s primaries (module doc).
pub(crate) fn render_float(
    width: u32,
    height: u32,
    components: &[Component],
    samples: &[u16],
    intrinsic: &[u8],
    profile: Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let out = assemble(
        width,
        height,
        components,
        samples,
        intrinsic,
        &profile,
        Ink::Linear,
        job,
        |pixel| pixel,
    )?;
    PrecisionImage::from_rgba32_extended(width, height, out, profile, None)
}

/// [`render_float`]'s output plane over `pixels` (module doc's Admission).
pub(crate) fn render_float_scratch(pixels: u64) -> u64 {
    pixels.saturating_mul(16)
}

/// Each plane-owning component's window over a `width × height` raster
/// (module doc, "Component windows"), `None` for the planeless ones, once the
/// `given` (coverage samples, intrinsic bytes) lengths are exactly what the
/// windows name: refuses a window `Window::place` refuses, and lengths that
/// differ. `dr_artwork_render` asks it before admission and before it builds
/// any slice, so the slices' lengths are the core's own layout, never the
/// caller's counts; the assembly loop reads by the windows it returns.
pub(crate) fn component_windows(
    components: &[Component],
    width: u32,
    height: u32,
    given: (usize, usize),
) -> Result<Vec<Option<(Window, bool)>>, String> {
    let windows = components
        .iter()
        .map(|c| match c {
            Component::Coverage { window, .. } => Window::plane(*window, (width, height), false),
            Component::IntrinsicRgba8 { window, .. } => {
                Window::plane(*window, (width, height), true)
            }
            _ => Ok(None),
        })
        .collect::<Result<Vec<_>, String>>()?;
    if totals(windows.iter().flatten().copied()) != given {
        return Err("artwork samples do not match their bounded component layout".into());
    }
    Ok(windows)
}

/// The ONE assembly loop: each component's colour by `ink`, composited in
/// order with `precision_paint::over`, each finished pixel through `finish`.
#[allow(clippy::too_many_arguments)]
fn assemble<T>(
    width: u32,
    height: u32,
    components: &[Component],
    samples: &[u16],
    intrinsic: &[u8],
    profile: &Arc<IccProfile>,
    ink: Ink,
    job: JobContext<'_>,
    finish: impl Fn([f32; 4]) -> T,
) -> Result<Vec<T>, String> {
    let n = pixel_count(width, height)?;
    let windows = component_windows(components, width, height, (samples.len(), intrinsic.len()))?;
    let srgb = IccProfile::srgb();
    let conversion = match (profile.model(), ink) {
        (Some(model), _) => Conversion::Matrix(
            PrecisionTransform::new(srgb.model().ok_or("missing sRGB model")?, model)
                .map_err(|e| crate::icc_encode_refusal::own_profile_error(profile, e))?,
        ),
        // A LUT-tagged 8/16-bit document (module doc, "LUT profiles"): the
        // authored-colour pipeline when it converts both ways, else the
        // numbers as authored, never a refusal.
        (None, Ink::Encoded) => match lut_ink(profile, true) {
            Some(pipeline) => Conversion::Lut(pipeline),
            None => Conversion::Numbers,
        },
        (None, Ink::Linear) => {
            return Err("artwork requires an RGB matrix/TRC destination profile".into())
        }
    };
    // A wide fill's document numbers at 8/16 bits (module doc, "Fill
    // layers"): one conversion, built only when a fill holds `linear`.
    let wide = match ink {
        Ink::Encoded if has_wide_fill(components) => Some(
            crate::authored_color::AuthoredColors::into_document(profile),
        ),
        _ => None,
    };
    let mut colors = Vec::with_capacity(components.len());
    let mut offsets = Vec::with_capacity(components.len());
    // One entry per component: a gradient's sampler over the whole raster
    // and its stop colours in the document's ink, `None` for the others.
    let mut gradients: Vec<Option<(GradientSampler, Vec<[f32; 3]>)>> =
        Vec::with_capacity(components.len());
    // One entry per component: a pattern's placed level (module doc,
    // "Pattern fill layers"), `None` for the others.
    let mut patterns: Vec<Option<PatternSampler>> = Vec::with_capacity(components.len());
    let mut offset = 0;
    let mut intrinsic_offset = 0;
    for (c, window) in components.iter().zip(&windows) {
        let pixels = window.map_or(0, |(w, _)| w.pixels());
        offsets.push(match c {
            Component::Coverage { .. } => offset,
            Component::IntrinsicRgba8 { .. } => intrinsic_offset,
            Component::Fill { .. } | Component::Gradient { .. } | Component::Pattern { .. } => 0,
        });
        patterns.push(match c {
            Component::Pattern { fill } => {
                let fill = pattern_fill(fill)?;
                // The Lab twin's tile is authored sRGB (module doc, "The Lab
                // twin"); every other profile reads document values.
                let twin = crate::lab_document::is_lab_twin(profile)
                    .then(|| crate::lab_style::twin_tile_sampler(&fill, (0, 0)))
                    .flatten();
                Some(twin.unwrap_or_else(|| fill.sampler((0, 0))))
            }
            _ => None,
        });
        match c {
            Component::Coverage { color, .. } => {
                let rgb = conversion.coverage([color[0], color[1], color[2]], ink);
                colors.push([rgb[0], rgb[1], rgb[2], color[3]]);
                gradients.push(None);
                offset += pixels;
            }
            Component::IntrinsicRgba8 { .. } => {
                colors.push([0.0; 4]);
                gradients.push(None);
                intrinsic_offset += pixels * 4;
            }
            Component::Fill { color, linear } => {
                let wide = wide.as_ref().and_then(Option::as_ref);
                let rgb = linear
                    .and_then(|linear| wide_ink(linear, wide, &conversion, ink))
                    .unwrap_or_else(|| conversion.coverage([color[0], color[1], color[2]], ink));
                colors.push([rgb[0], rgb[1], rgb[2], color[3]]);
                gradients.push(None);
            }
            Component::Gradient {
                gradient,
                colors: stops,
                linear,
                frame,
            } => {
                let fill = gradient_fill(gradient, stops)?;
                let (bx, by, bw, bh) = gradient_frame(*frame, width, height)?;
                let sampler = GradientSampler::new(&fill, bx, by, bw, bh)
                    .with_classic_bound(fill_bound(fill.style));
                let wide = wide.as_ref().and_then(Option::as_ref);
                let converted = stops
                    .iter()
                    .enumerate()
                    .map(|(k, &rgb)| {
                        linear
                            .as_ref()
                            .and_then(|linear| wide_ink(linear[k], wide, &conversion, ink))
                            .unwrap_or_else(|| conversion.coverage(rgb, ink))
                    })
                    .collect();
                colors.push([0.0; 4]);
                gradients.push(Some((sampler, converted)));
            }
            Component::Pattern { .. } => {
                colors.push([0.0; 4]);
                gradients.push(None);
            }
        }
    }
    let mut out = reserved_vec(n)?;
    let w = width as usize;
    let (mut px, mut py) = (0usize, 0usize);
    for i in 0..n {
        job.every(i)?;
        let mut pixel = [0.0; 4];
        for (index, c) in components.iter().enumerate() {
            let color = colors[index];
            let offset = offsets[index];
            // Outside its window a plane reads 0 (module doc, "Component
            // windows").
            let at = windows[index].and_then(|(window, _)| window.at(px, py));
            match c {
                Component::Coverage { .. } => {
                    let coverage = at.map_or(0, |k| samples[offset + k]);
                    let alpha = crate::precision_render::checked_alpha(
                        color[3],
                        1.0,
                        f32::from(coverage) / 65535.0,
                    )?;
                    if alpha > 0.0 {
                        over(&mut pixel, color, alpha);
                    }
                }
                Component::IntrinsicRgba8 { opacity, .. } => {
                    let s = at.map_or([0; 4], |k| {
                        let at = offset + k * 4;
                        [
                            intrinsic[at],
                            intrinsic[at + 1],
                            intrinsic[at + 2],
                            intrinsic[at + 3],
                        ]
                    });
                    let alpha = crate::precision_render::checked_alpha(
                        *opacity,
                        1.0,
                        f32::from(s[3]) / 255.0,
                    )?;
                    if alpha > 0.0 {
                        let rgb = [
                            f32::from(s[0]) / 255.0,
                            f32::from(s[1]) / 255.0,
                            f32::from(s[2]) / 255.0,
                        ];
                        let rgb = conversion.intrinsic(rgb, ink);
                        over(&mut pixel, [rgb[0], rgb[1], rgb[2], alpha], alpha);
                    }
                }
                Component::Fill { .. } => {
                    let alpha = crate::precision_render::checked_alpha(color[3], 1.0, 1.0)?;
                    if alpha > 0.0 {
                        over(&mut pixel, color, alpha);
                    }
                }
                Component::Gradient { .. } => {
                    let Some((sampler, stops)) = gradients[index].as_ref() else {
                        return Err("artwork gradient component has no sampler".into());
                    };
                    let (x, y) = (px as f32 + 0.5, py as f32 + 0.5);
                    let (rgb, opacity) = sampler.color_with(sampler.t(x, y), |k| stops[k]);
                    let alpha = crate::precision_render::checked_alpha(opacity, 1.0, 1.0)?;
                    if alpha > 0.0 {
                        over(&mut pixel, [rgb[0], rgb[1], rgb[2], alpha], alpha);
                    }
                }
                Component::Pattern { .. } => {
                    let Some(sampler) = patterns[index].as_ref() else {
                        return Err("artwork pattern component has no sampler".into());
                    };
                    // Document values at every ink (module doc): no
                    // `Conversion`.
                    let s = sampler.at(px as i64, py as i64);
                    let alpha = crate::precision_render::checked_alpha(s[3], 1.0, 1.0)?;
                    if alpha > 0.0 {
                        over(&mut pixel, [s[0], s[1], s[2], alpha], alpha);
                    }
                }
            }
        }
        out.push(finish(pixel));
        px += 1;
        if px == w {
            (px, py) = (0, py + 1);
        }
    }
    Ok(out)
}
