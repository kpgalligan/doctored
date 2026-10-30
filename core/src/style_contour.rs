//! Layer-style CONTOUR: the Contour picker on
//! Drop Shadow, Inner Shadow, Outer Glow, Inner Glow, Satin and Bevel &
//! Emboss (its Gloss Contour, and the Contour sub-panel that reshapes the
//! bevel's height), with Anti-aliased and the glows' and the bevel
//! contour's Range. A contour is a curve `C` over the effect's 0..255
//! plane byte: the ten presets are 256-entry tables MEASURED in Photoshop,
//! a custom curve is the natural spline through its points.
//!
//! # The model
//!
//! [`effect_lut`] (and [`below_lut`], [`bevel_contour_lut`]) turn (contour,
//! range) into ONE byte table `L`: `L[c] = round(T(255 · R(c / 255)))`,
//! with `T` the curve's 256 values and `R` the range rule (when the effect
//! has a Range), capped on a Below effect (Extent below). A table equal to
//! the identity is `None`, and every caller then skips its pass, so a
//! default style renders bit for bit as it did before contours existed.
//! The pass is applied IN PLACE on the effect's own u8 plane
//! ([`map_plane_with_job`]) at the site each effect's kernel names —
//! no new plane, so the typed `style_scratch` plan and `Effect::reach` are
//! unchanged — and since every depth (8, 16, 32 bits and CMYK ink) renders
//! its effects through the one u8 plane renderer
//! (`style_render::render_effects_with_job`), the contour reaches each of
//! them with no depth-specific code. The map is per pixel and reads nothing
//! global, so a sub-plane re-render (`style_cache`) stays exact.
//!
//! - **Presets** ([`PRESET_TABLES`]): each preset's Drop Shadow (size 250,
//!   spread 0, Anti-aliased off, a straight edge) read against its Linear
//!   twin pixel by pixel in Photoshop 26.10.0 (`contour_take.py`, take1):
//!   entry `c` is the mean reading over the pixels whose twin code is `c`;
//!   where that reading sits on the Below cap (Cone - Inverted and Rolling
//!   Slope - Descending under about 30) the entry is the least-squares line
//!   through the first 16 codes under it.
//! - **Custom curves**: [`adjust_curves::Spline`], the Curves adjustment's
//!   natural cubic, run by run between corner points (a corner ends one run
//!   and starts the next), flat past the end points, clamped to 0..255.
//! - **Extent** ([`below_lut`]): a Below effect (drop shadow, outer glow)
//!   draws `min(C(R(v)), k R(v))` on its own plane `v` — in Photoshop every
//!   contoured shadow reads the same bytes in its tail whatever its curve —
//!   so a curve with `C(0) > 0` (Cone - Inverted) fades out with the plane
//!   instead of filling the pad, the contour never draws past the effect's
//!   reach, and the render does not depend on `pad`. The cap is part of the
//!   table, so the map stays one lookup per pixel. Interior planes are
//!   mapped everywhere and multiplied by the coverage after.
//! - **Gloss** ([`map_gloss`]): the bevel's signed shading `d` in -1..1 maps
//!   to `2 C((d + 1) / 2) - 1`, before it splits into the highlight and the
//!   shadow halves; its support is the union of the two pre-gloss bytes
//!   against [`SUPPORT_MIN`] (`style_fx_bevel_emboss`).
//! - **Anti-aliased** is carried (sheet, JSON, `.rz`, PSD `AntA`) but draws
//!   nothing: take1's AA rows read the AA-off table.
//! - **Residuals** (take1; `contour_take.py` and the 10C status doc): the
//!   gloss and the bevel contour ship the take's best fits, which missed its
//!   bound. The satin's site (each shifted copy, before the combine) is
//!   `style_fx_satin`'s since #2 part 3, confirmed on a fresh take.
//!
//! [`adjust_curves::Spline`]: crate::adjust_curves::Spline

use crate::adjust_curves::Spline;
use crate::job_control::JobContext;

/// The most points a custom curve may hold.
pub const MAX_CONTOUR_POINTS: usize = 16;

/// The ten contours of the default picker, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContourPreset {
    Linear,
    Cone,
    ConeInverted,
    Gaussian,
    HalfRound,
    Ring,
    RingDouble,
    RollingSlopeDescending,
    RoundedSteps,
    Sawtooth1,
}

impl ContourPreset {
    /// Every preset, in the picker's (and [`PRESET_TABLES`]') order.
    pub const ALL: [ContourPreset; 10] = [
        ContourPreset::Linear,
        ContourPreset::Cone,
        ContourPreset::ConeInverted,
        ContourPreset::Gaussian,
        ContourPreset::HalfRound,
        ContourPreset::Ring,
        ContourPreset::RingDouble,
        ContourPreset::RollingSlopeDescending,
        ContourPreset::RoundedSteps,
        ContourPreset::Sawtooth1,
    ];

    fn index(self) -> usize {
        self as usize
    }
}

/// One custom-curve point, both coordinates in 0..=255; `corner` ends a
/// smooth run (Photoshop's `Cnty` false).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContourPoint {
    pub x: f32,
    pub y: f32,
    pub corner: bool,
}

const UNUSED: ContourPoint = ContourPoint {
    x: 0.0,
    y: 0.0,
    corner: false,
};

/// A custom curve held inline (no heap, so a style's resource accounting is
/// unchanged): the first `len` of `points`, sorted by `x` with distinct `x`
/// values, 2..=16 of them; the unused slots are zero so equality compares
/// only what the curve holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContourCurve {
    pub points: [ContourPoint; MAX_CONTOUR_POINTS],
    pub len: u8,
}

impl ContourCurve {
    /// The curve through `points`, which must already be sorted by `x` with
    /// distinct values and number 2..=16 (what `style_contour_json` makes of
    /// any input); `None` otherwise.
    pub fn new(points: &[ContourPoint]) -> Option<ContourCurve> {
        if !(2..=MAX_CONTOUR_POINTS).contains(&points.len())
            || points
                .windows(2)
                .any(|w| !matches!(w[0].x.partial_cmp(&w[1].x), Some(std::cmp::Ordering::Less)))
        {
            return None;
        }
        let mut out = [UNUSED; MAX_CONTOUR_POINTS];
        out[..points.len()].copy_from_slice(points);
        Some(ContourCurve {
            points: out,
            len: points.len() as u8,
        })
    }

    /// The points the curve holds.
    pub fn points(&self) -> &[ContourPoint] {
        &self.points[..usize::from(self.len).min(MAX_CONTOUR_POINTS)]
    }
}

/// An effect's contour: a preset or a custom curve. The default is Linear.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Contour {
    Preset(ContourPreset),
    Custom(ContourCurve),
}

impl Default for Contour {
    fn default() -> Self {
        Contour::Preset(ContourPreset::Linear)
    }
}

impl Contour {
    /// Whether this is the Linear preset (the default).
    pub fn is_linear(&self) -> bool {
        *self == Contour::Preset(ContourPreset::Linear)
    }
}

/// The glows' and the bevel contour's default Range: 0.5 (50 %),
/// at which every range rule is the identity.
pub const DEFAULT_RANGE: f32 = 0.5;

/// The gloss contour's support (module doc, Gloss): a pixel's gloss is
/// mapped only where one of its two pre-gloss bytes reaches this. The
/// take's EXTENT winner (`contour_take.py`, which missed: a Below effect
/// uses the cap instead, [`BELOW_CAP_DROP_SHADOW`]).
pub(crate) const SUPPORT_MIN: u8 = 8;

/// The Below cap's slope on the drop shadow's own plane (module doc,
/// Extent): `min(C(R(v)), k R(v))`. Fitted on take1's tails (the REFIT in
/// `contour_take.py`, a hypothesis until a fresh take confirms it): every
/// contoured drop shadow reads the same bytes there whatever its curve,
/// about 8.25 times the shadow's blur (k 8.0 and 8.5 fit worse).
pub(crate) const BELOW_CAP_DROP_SHADOW: f64 = 8.25;

/// The cap on the outer glow's own plane: half the drop shadow's, because
/// the glow plane is twice the shadow's blur at the same size and
/// the cap follows the blur (take1: og_linear = 2 ds_linear).
pub(crate) const BELOW_CAP_OUTER_GLOW: f64 = BELOW_CAP_DROP_SHADOW / 2.0;

/// The bevel contour's SITE for the bevels that keep today's kernel (the
/// Chisels outside the Chisel law; a Smooth bevel's measured law maps its
/// unit height before the shading and softens the shading instead,
/// `style_bevel_smooth`, and the Chisel law maps its softened height,
/// `style_bevel_chisel`, #2):
/// true maps the height before `soften`. The two
/// sites scored the same to the code in the take, so the site that keeps
/// the render independent of the pad wins: AFTER. Mapped before, a contour
/// with `C(0) > 0` lifts the zero ground to a plateau reaching the plane
/// border, and the soften blur's zero-padded right and bottom edges
/// (Soften > 6, a downsampled blur) dip it there into a line the Below
/// halves draw.
pub(crate) const BEVEL_BEFORE_SOFTEN: bool = false;

/// How Range reshapes the LUT's input `x` in 0..1 at range `r` (0.01..=1);
/// each is the identity at 0.5.
#[derive(Clone, Copy)]
enum RangeRule {
    /// The outer glow's, and the inner glow's where Size·(1 − Choke) is 0
    /// and today's kernel runs (the take's RANGE winner): `min(1, x / 2r)`.
    /// The edge glow's Choke 100 % member applies the same rule to its
    /// partial pixels itself (`style_fx_inner_glow::full_choke_plane`).
    Divide,
    /// The bevel contour's (the take's BEVEL best fit): `max(0, 1 - (1 - x) / 2r)`.
    Complement,
}

fn range_input(x: f64, r: f64, rule: RangeRule) -> f64 {
    match rule {
        RangeRule::Divide => (x / (2.0 * r)).min(1.0),
        RangeRule::Complement => (1.0 - (1.0 - x) / (2.0 * r)).max(0.0),
    }
}

/// The take's GLOSS best fit: whether the gloss subtracts its flat level
/// `2 C(0.5) - 1` (it does not: PLAIN).
const GLOSS_FLAT: bool = false;

const IDENTITY: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        t[i] = i as u8;
        i += 1;
    }
    t
};

/// The ten presets' measured tables, in [`ContourPreset::ALL`] order (module
/// doc; `tests/fixtures/psd_photoshop/contour/tables.json`). Linear is the
/// identity.
#[rustfmt::skip]
pub(crate) const PRESET_TABLES: [[u8; 256]; 10] = [
    // linear
    IDENTITY,
    // cone
    [
        0, 3, 6, 9, 12, 15, 18, 21, 24, 27, 30, 33, 36, 38, 41, 44,
        48, 51, 54, 56, 60, 62, 66, 68, 71, 74, 76, 80, 82, 85, 88, 90,
        94, 96, 99, 102, 105, 108, 110, 112, 116, 118, 121, 124, 126, 129, 132, 134,
        137, 140, 142, 144, 147, 149, 151, 154, 156, 159, 161, 164, 166, 168, 171, 173,
        175, 178, 180, 182, 184, 186, 189, 190, 192, 195, 197, 198, 200, 203, 204, 206,
        208, 210, 212, 213, 215, 217, 219, 220, 221, 223, 224, 227, 228, 229, 230, 231,
        233, 234, 236, 237, 238, 239, 240, 241, 242, 243, 244, 246, 246, 247, 248, 249,
        249, 250, 251, 251, 252, 252, 253, 253, 254, 254, 254, 254, 255, 255, 255, 255,
        255, 255, 255, 255, 255, 254, 254, 254, 254, 253, 253, 252, 252, 251, 251, 250,
        249, 249, 248, 247, 246, 245, 244, 243, 242, 241, 240, 239, 238, 237, 236, 234,
        232, 231, 230, 229, 228, 226, 224, 222, 221, 220, 218, 216, 214, 213, 211, 209,
        207, 205, 204, 202, 199, 198, 196, 194, 191, 190, 188, 185, 184, 181, 179, 176,
        174, 172, 170, 167, 165, 162, 160, 157, 155, 153, 150, 148, 145, 143, 140, 138,
        136, 132, 130, 128, 124, 122, 120, 116, 114, 111, 109, 106, 103, 100, 97, 94,
        91, 88, 86, 82, 80, 78, 74, 71, 68, 66, 63, 60, 57, 54, 51, 48,
        45, 42, 39, 36, 33, 30, 27, 24, 21, 18, 15, 12, 9, 6, 3, 0,
    ],
    // cone_inverted
    [
        251, 248, 246, 243, 240, 237, 234, 231, 229, 226, 223, 220, 217, 215, 212, 209,
        206, 203, 200, 198, 195, 192, 189, 186, 184, 180, 178, 175, 172, 170, 167, 164,
        162, 158, 156, 152, 150, 146, 144, 142, 139, 136, 133, 131, 128, 125, 123, 120,
        117, 115, 113, 110, 108, 105, 103, 100, 98, 96, 93, 90, 88, 86, 83, 82,
        79, 77, 74, 72, 70, 68, 66, 64, 62, 59, 58, 56, 54, 52, 50, 49,
        46, 44, 43, 41, 39, 37, 36, 34, 33, 32, 30, 28, 27, 25, 24, 23,
        22, 20, 18, 17, 16, 15, 14, 13, 12, 11, 11, 10, 8, 7, 7, 6,
        5, 5, 4, 3, 3, 2, 2, 2, 1, 1, 1, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5,
        6, 7, 8, 8, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 22,
        23, 24, 26, 27, 28, 30, 32, 33, 35, 36, 37, 40, 42, 43, 45, 46,
        49, 50, 52, 54, 57, 58, 60, 62, 64, 66, 68, 71, 72, 74, 77, 80,
        82, 84, 86, 88, 91, 93, 96, 98, 101, 103, 106, 108, 110, 113, 116, 118,
        120, 124, 126, 128, 132, 134, 136, 139, 142, 144, 147, 150, 152, 156, 158, 162,
        164, 167, 170, 172, 176, 178, 180, 184, 187, 190, 193, 196, 199, 202, 204, 207,
        211, 214, 216, 219, 222, 225, 228, 231, 234, 237, 240, 243, 246, 249, 252, 255,
    ],
    // gaussian
    [
        0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2,
        2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 6, 6, 6,
        7, 8, 8, 8, 9, 10, 10, 11, 12, 12, 13, 14, 15, 16, 16, 18,
        18, 20, 20, 22, 22, 24, 24, 26, 27, 28, 29, 30, 32, 34, 35, 36,
        38, 40, 42, 43, 44, 46, 48, 50, 52, 54, 55, 57, 59, 61, 63, 64,
        67, 69, 71, 73, 75, 78, 80, 81, 83, 85, 88, 91, 93, 94, 96, 98,
        100, 103, 106, 108, 110, 112, 114, 116, 118, 120, 122, 125, 128, 130, 132, 134,
        136, 138, 140, 141, 143, 145, 147, 149, 151, 153, 155, 157, 159, 161, 162, 164,
        166, 168, 170, 171, 173, 174, 176, 178, 179, 181, 182, 184, 185, 186, 188, 189,
        191, 192, 193, 194, 196, 198, 199, 200, 201, 202, 203, 204, 206, 207, 208, 209,
        210, 211, 212, 213, 214, 216, 216, 217, 218, 219, 219, 220, 222, 222, 223, 224,
        225, 226, 226, 227, 228, 229, 229, 230, 231, 231, 232, 233, 233, 234, 234, 235,
        236, 236, 236, 237, 238, 238, 239, 239, 240, 240, 241, 241, 242, 242, 242, 243,
        243, 244, 244, 244, 245, 245, 245, 246, 246, 246, 246, 247, 247, 248, 248, 248,
        248, 249, 249, 249, 249, 250, 250, 250, 250, 250, 251, 251, 251, 251, 252, 252,
        252, 252, 252, 253, 253, 253, 253, 253, 254, 254, 254, 254, 254, 255, 255, 255,
    ],
    // half_round
    [
        0, 2, 5, 8, 10, 13, 15, 18, 21, 23, 26, 28, 30, 33, 35, 38,
        40, 43, 46, 48, 50, 53, 56, 58, 60, 62, 64, 66, 68, 70, 72, 76,
        78, 80, 82, 84, 86, 88, 90, 92, 94, 96, 98, 100, 102, 104, 106, 108,
        109, 112, 113, 114, 116, 118, 120, 122, 123, 124, 126, 128, 130, 132, 133, 134,
        136, 137, 140, 141, 142, 144, 145, 146, 148, 149, 150, 152, 154, 155, 156, 157,
        158, 160, 161, 162, 164, 165, 166, 167, 168, 169, 170, 172, 173, 174, 175, 176,
        177, 178, 180, 181, 182, 182, 183, 184, 185, 186, 187, 188, 190, 191, 191, 192,
        193, 194, 195, 196, 197, 197, 198, 199, 200, 201, 202, 202, 203, 204, 205, 205,
        206, 207, 208, 208, 209, 210, 211, 211, 212, 213, 213, 214, 215, 215, 216, 216,
        217, 218, 218, 219, 220, 220, 221, 221, 222, 222, 223, 223, 224, 224, 225, 226,
        226, 227, 227, 227, 228, 228, 229, 229, 230, 230, 230, 231, 232, 232, 232, 233,
        233, 234, 234, 234, 235, 235, 235, 236, 236, 237, 237, 237, 238, 238, 238, 239,
        239, 239, 240, 240, 240, 241, 241, 241, 242, 242, 242, 242, 243, 243, 243, 244,
        244, 244, 244, 245, 245, 245, 246, 246, 246, 246, 246, 247, 247, 247, 248, 248,
        248, 248, 249, 249, 249, 249, 250, 250, 250, 250, 250, 251, 251, 251, 251, 252,
        252, 252, 252, 252, 253, 253, 253, 253, 254, 254, 254, 254, 254, 255, 255, 255,
    ],
    // ring
    [
        0, 3, 6, 9, 12, 15, 18, 22, 25, 28, 31, 34, 37, 40, 44, 47,
        51, 54, 58, 61, 64, 68, 72, 75, 79, 83, 86, 90, 94, 97, 101, 105,
        110, 114, 118, 123, 128, 132, 136, 140, 145, 150, 154, 158, 163, 167, 171, 176,
        181, 184, 189, 192, 197, 200, 204, 208, 212, 216, 219, 222, 226, 229, 232, 234,
        237, 239, 242, 244, 245, 248, 249, 250, 252, 253, 253, 254, 255, 255, 255, 255,
        255, 255, 255, 255, 254, 254, 253, 252, 252, 251, 249, 247, 246, 245, 243, 241,
        240, 237, 234, 232, 230, 228, 226, 223, 221, 218, 216, 212, 207, 204, 201, 198,
        195, 192, 189, 185, 182, 179, 175, 172, 168, 164, 161, 157, 153, 150, 146, 142,
        138, 134, 130, 126, 123, 119, 115, 111, 107, 104, 100, 96, 93, 89, 86, 82,
        79, 76, 72, 69, 64, 60, 57, 54, 51, 48, 46, 43, 40, 38, 34, 31,
        29, 27, 25, 23, 21, 18, 16, 15, 13, 12, 11, 8, 7, 6, 6, 4,
        3, 3, 2, 2, 1, 1, 1, 1, 1, 2, 2, 3, 3, 4, 5, 6,
        7, 8, 10, 12, 14, 15, 18, 20, 22, 24, 26, 29, 32, 34, 37, 40,
        43, 47, 50, 53, 57, 60, 63, 67, 71, 75, 78, 82, 86, 91, 95, 100,
        104, 108, 112, 117, 121, 125, 130, 135, 140, 144, 149, 154, 159, 163, 168, 173,
        179, 184, 189, 194, 198, 204, 208, 213, 219, 224, 229, 234, 240, 245, 250, 255,
    ],
    // ring_double
    [
        0, 1, 3, 4, 6, 7, 9, 11, 12, 14, 16, 17, 19, 21, 23, 25,
        27, 29, 32, 34, 36, 39, 42, 44, 47, 50, 53, 56, 59, 62, 66, 70,
        74, 78, 82, 86, 92, 96, 100, 104, 110, 116, 120, 124, 130, 135, 140, 146,
        150, 155, 160, 164, 169, 174, 179, 184, 188, 192, 197, 200, 205, 209, 213, 215,
        219, 222, 225, 228, 230, 232, 234, 235, 236, 238, 239, 239, 240, 240, 240, 240,
        239, 238, 237, 235, 233, 231, 228, 226, 224, 221, 216, 211, 207, 203, 199, 194,
        189, 181, 173, 167, 160, 154, 147, 141, 134, 127, 119, 108, 98, 91, 84, 76,
        69, 63, 56, 49, 43, 37, 32, 26, 21, 17, 13, 9, 6, 4, 2, 1,
        0, 0, 1, 3, 5, 7, 11, 14, 19, 23, 28, 34, 39, 45, 51, 58,
        64, 71, 78, 85, 96, 106, 113, 120, 127, 134, 140, 147, 154, 160, 169, 177,
        183, 188, 193, 197, 202, 208, 213, 217, 220, 223, 225, 229, 231, 233, 234, 236,
        237, 237, 237, 237, 236, 236, 234, 232, 230, 228, 224, 220, 217, 212, 207, 201,
        195, 191, 184, 177, 169, 161, 154, 145, 138, 129, 121, 113, 105, 97, 90, 82,
        74, 66, 58, 52, 44, 38, 33, 26, 21, 17, 14, 10, 6, 4, 2, 2,
        2, 3, 4, 6, 10, 13, 16, 22, 28, 32, 39, 46, 54, 62, 69, 79,
        89, 99, 108, 118, 128, 138, 149, 160, 172, 184, 195, 208, 220, 231, 243, 255,
    ],
    // rolling_slope_descending
    [
        247, 244, 241, 238, 235, 232, 229, 226, 223, 219, 216, 213, 210, 207, 204, 201,
        198, 195, 191, 188, 185, 182, 179, 177, 173, 170, 166, 163, 160, 157, 154, 151,
        148, 144, 142, 138, 136, 132, 130, 128, 126, 122, 120, 118, 116, 114, 112, 110,
        108, 106, 105, 104, 102, 100, 99, 98, 96, 96, 94, 94, 93, 92, 91, 91,
        90, 90, 90, 90, 90, 90, 89, 90, 90, 90, 90, 90, 91, 91, 92, 92,
        92, 93, 94, 94, 96, 96, 97, 98, 98, 99, 100, 101, 102, 103, 104, 105,
        106, 106, 108, 109, 110, 111, 112, 113, 114, 115, 116, 118, 119, 120, 121, 122,
        123, 124, 125, 126, 127, 129, 130, 131, 132, 133, 134, 135, 136, 137, 138, 139,
        140, 141, 142, 143, 144, 144, 145, 146, 147, 148, 149, 149, 150, 151, 151, 152,
        153, 153, 154, 154, 155, 156, 156, 157, 157, 157, 158, 158, 158, 158, 159, 159,
        159, 159, 159, 159, 159, 159, 159, 159, 158, 158, 158, 158, 157, 157, 156, 156,
        155, 154, 154, 153, 152, 151, 151, 150, 148, 148, 146, 145, 144, 142, 141, 140,
        138, 137, 136, 134, 132, 131, 130, 127, 126, 124, 122, 120, 118, 117, 114, 113,
        111, 108, 106, 104, 102, 100, 98, 96, 93, 91, 90, 86, 84, 82, 79, 76,
        74, 72, 70, 66, 64, 62, 60, 57, 54, 52, 49, 46, 44, 40, 38, 35,
        33, 30, 27, 25, 22, 19, 17, 14, 11, 8, 5, 2, 0, 0, 0, 0,
    ],
    // rounded_steps
    [
        1, 2, 5, 8, 10, 13, 16, 18, 21, 23, 25, 28, 30, 32, 34, 36,
        38, 40, 42, 43, 44, 46, 48, 49, 51, 52, 53, 54, 55, 56, 57, 58,
        58, 59, 60, 60, 61, 62, 62, 62, 63, 64, 64, 64, 64, 64, 65, 65,
        65, 65, 65, 65, 65, 65, 65, 65, 65, 65, 65, 65, 64, 64, 64, 64,
        64, 66, 69, 72, 73, 76, 78, 80, 83, 85, 87, 89, 91, 94, 95, 97,
        99, 101, 103, 104, 106, 108, 109, 110, 112, 113, 114, 116, 116, 117, 118, 119,
        120, 120, 122, 122, 123, 123, 124, 124, 124, 125, 125, 126, 126, 126, 126, 126,
        126, 126, 127, 127, 127, 127, 127, 127, 127, 126, 126, 126, 126, 126, 126, 126,
        129, 131, 134, 136, 139, 141, 144, 146, 148, 151, 153, 155, 157, 159, 161, 163,
        165, 167, 168, 170, 172, 174, 176, 177, 178, 179, 180, 181, 182, 183, 184, 185,
        186, 187, 187, 188, 188, 189, 190, 190, 190, 190, 191, 191, 191, 191, 192, 192,
        192, 192, 192, 192, 192, 192, 192, 191, 191, 191, 191, 191, 191, 190, 190, 190,
        190, 191, 194, 195, 196, 198, 200, 201, 204, 205, 206, 207, 208, 210, 212, 213,
        214, 216, 216, 218, 218, 219, 220, 221, 222, 222, 223, 223, 224, 224, 224, 224,
        224, 224, 224, 224, 224, 223, 223, 222, 222, 221, 220, 219, 218, 218, 217, 215,
        214, 213, 212, 211, 210, 208, 207, 206, 205, 203, 202, 200, 199, 198, 196, 195,
    ],
    // sawtooth_1
    [
        0, 2, 4, 6, 8, 10, 12, 14, 16, 17, 20, 21, 23, 25, 27, 29,
        31, 33, 35, 37, 38, 41, 42, 44, 47, 48, 50, 52, 54, 56, 58, 60,
        62, 64, 66, 68, 70, 72, 74, 76, 78, 80, 81, 84, 86, 87, 89, 92,
        93, 95, 97, 98, 101, 102, 104, 106, 108, 110, 112, 114, 116, 118, 120, 122,
        124, 126, 127, 125, 124, 122, 119, 118, 115, 113, 111, 110, 107, 105, 103, 101,
        99, 96, 94, 93, 90, 87, 86, 84, 82, 80, 78, 75, 73, 71, 70, 68,
        66, 63, 60, 58, 56, 55, 53, 51, 49, 47, 45, 42, 39, 37, 35, 33,
        31, 29, 27, 25, 23, 21, 19, 17, 15, 13, 11, 9, 6, 4, 2, 0,
        2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24, 25, 27, 29, 31,
        33, 35, 37, 39, 41, 44, 46, 48, 50, 51, 53, 55, 57, 58, 61, 64,
        65, 67, 69, 71, 72, 75, 77, 79, 81, 82, 84, 86, 89, 91, 92, 94,
        97, 99, 100, 102, 105, 106, 108, 110, 112, 114, 116, 118, 120, 122, 124, 125,
        124, 122, 120, 118, 116, 114, 112, 110, 108, 106, 104, 102, 100, 98, 96, 95,
        92, 90, 89, 86, 84, 83, 81, 78, 76, 75, 74, 72, 68, 66, 64, 62,
        60, 58, 56, 55, 54, 52, 50, 47, 46, 44, 41, 40, 37, 36, 34, 31,
        29, 27, 25, 24, 22, 20, 18, 16, 14, 12, 10, 8, 6, 4, 2, 0,
    ],
];

/// `values` read piecewise linearly at the fractional code `at`, clamped to
/// 0..=255.
fn read(values: &[f64; 256], at: f64) -> f64 {
    let at = if at.is_finite() {
        at.clamp(0.0, 255.0)
    } else {
        0.0
    };
    let i = (at.floor() as usize).min(254);
    values[i] + (at - i as f64) * (values[i + 1] - values[i])
}

/// [`read`] over a byte table.
pub(crate) fn read_lut(lut: &[u8; 256], at: f64) -> f64 {
    let at = if at.is_finite() {
        at.clamp(0.0, 255.0)
    } else {
        0.0
    };
    let i = (at.floor() as usize).min(254);
    let lo = f64::from(lut[i]);
    lo + (at - i as f64) * (f64::from(lut[i + 1]) - lo)
}

/// The curve's value at each code 0..=255, unrounded (module doc).
fn curve_values(contour: &Contour) -> [f64; 256] {
    match contour {
        Contour::Preset(p) => {
            let table = &PRESET_TABLES[p.index()];
            std::array::from_fn(|c| f64::from(table[c]))
        }
        Contour::Custom(curve) => custom_values(curve.points()),
    }
}

/// A custom curve's values: the natural spline of each run between corners
/// (module doc). `points` is sorted with distinct `x` and holds at least 2
/// (`ContourCurve::new`); anything shorter is the identity.
fn custom_values(points: &[ContourPoint]) -> [f64; 256] {
    if points.len() < 2 {
        return std::array::from_fn(|c| c as f64);
    }
    let mut runs: Vec<Spline> = Vec::new();
    let mut run: Vec<(f64, f64)> = vec![(f64::from(points[0].x), f64::from(points[0].y))];
    for (i, p) in points.iter().enumerate().skip(1) {
        run.push((f64::from(p.x), f64::from(p.y)));
        if p.corner && i + 1 < points.len() {
            runs.push(Spline::new(&run));
            run = vec![(f64::from(p.x), f64::from(p.y))];
        }
    }
    runs.push(Spline::new(&run));
    std::array::from_fn(|c| {
        let x = c as f64;
        let run = runs
            .iter()
            .find(|s| s.points.last().is_some_and(|p| x <= p.0))
            .or(runs.last());
        run.map_or(x, |s| s.value(x)).clamp(0.0, 255.0)
    })
}

/// `L[c] = round(min(C(255 R(c / 255)), cap · 255 R(c / 255)))`; `None` when
/// it is the identity. Anti-aliased is carried but draws nothing: take1's
/// AA rows read the AA-off table (AA NONE met, max 2 / mean 0.46 codes).
fn build(
    contour: &Contour,
    range: Option<(f32, RangeRule)>,
    cap: Option<f64>,
) -> Option<[u8; 256]> {
    let values = curve_values(contour);
    let range = sanitized_range(range);
    let lut: [u8; 256] = std::array::from_fn(|c| {
        let x = ranged_input(c, range);
        let mut v = read(&values, 255.0 * x);
        if let Some(k) = cap {
            v = v.min(cap_value(x, k));
        }
        v.round().clamp(0.0, 255.0) as u8
    });
    (lut != IDENTITY).then_some(lut)
}

/// A Range that is finite, clamped into 0.01..=1 with its rule; `None` for
/// no Range (or a non-finite one, which reads as none).
fn sanitized_range(range: Option<(f32, RangeRule)>) -> Option<(f64, RangeRule)> {
    range
        .filter(|(r, _)| r.is_finite())
        .map(|(r, rule)| (f64::from(r).clamp(0.01, 1.0), rule))
}

/// The LUT input `x` of table code `c`: `c / 255` through the Range, when
/// there is one ([`range_input`]).
fn ranged_input(c: usize, range: Option<(f64, RangeRule)>) -> f64 {
    let x = c as f64 / 255.0;
    range.map_or(x, |(r, rule)| range_input(x, r, rule))
}

/// The ONE Below cap formula (module doc, Extent): `cap · 255 x`, unrounded.
fn cap_value(x: f64, cap: f64) -> f64 {
    cap * 255.0 * x
}

/// The ONE byte table an Interior effect's contour applies (module doc);
/// `None` when it is the identity, so the caller skips the pass. `range` is
/// the glow's Range (0.01..=1) where it has one. `anti_aliased` draws
/// nothing ([`build`]).
pub(crate) fn effect_lut(
    contour: &Contour,
    _anti_aliased: bool,
    range: Option<f32>,
) -> Option<[u8; 256]> {
    build(contour, range.map(|r| (r, RangeRule::Divide)), None)
}

/// A Below effect's table (module doc, Extent): [`effect_lut`] capped at
/// `cap` times the LUT's own input, so the contour fades out with the plane
/// and never draws past the effect's reach.
pub(crate) fn below_lut(
    contour: &Contour,
    _anti_aliased: bool,
    range: Option<f32>,
    cap: f64,
) -> Option<[u8; 256]> {
    build(contour, range.map(|r| (r, RangeRule::Divide)), Some(cap))
}

/// The Below cap alone (module doc, Extent): `round(min(255, k R(v)))` per
/// plane value, the bound [`below_lut`] holds its table to. The noise
/// (`style_noise`) holds a Below effect's grain to it too.
pub(crate) fn below_cap_table(range: Option<f32>, cap: f64) -> [u8; 256] {
    let range = sanitized_range(range.map(|r| (r, RangeRule::Divide)));
    std::array::from_fn(|c| {
        cap_value(ranged_input(c, range), cap)
            .round()
            .clamp(0.0, 255.0) as u8
    })
}

/// The bevel Contour sub-panel's table: its Range follows the bevel's own
/// rule ([`RangeRule::Complement`]).
pub(crate) fn bevel_contour_lut(
    contour: &Contour,
    _anti_aliased: bool,
    range: f32,
) -> Option<[u8; 256]> {
    build(contour, Some((range, RangeRule::Complement)), None)
}

/// Maps `plane` through `lut` in place, one row of `w` at a time (polling
/// `job` per row). `support`: `None` maps every pixel; `Some(s)` maps where
/// `s >= SUPPORT_MIN` and writes 0 elsewhere.
pub(crate) fn map_plane_with_job(
    plane: &mut [u8],
    w: u32,
    lut: &[u8; 256],
    support: Option<&[u8]>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let row = (w as usize).max(1);
    match support {
        None => {
            for r in plane.chunks_mut(row) {
                job.check()?;
                for v in r {
                    *v = lut[usize::from(*v)];
                }
            }
        }
        Some(support) => {
            for (r, s) in plane.chunks_mut(row).zip(support.chunks(row)) {
                job.check()?;
                for (v, &s) in r.iter_mut().zip(s) {
                    *v = if s >= SUPPORT_MIN {
                        lut[usize::from(*v)]
                    } else {
                        0
                    };
                }
            }
        }
    }
    Ok(())
}

/// Maps the unit plane `plane` (0..=1) through `lut` in place: each `v`
/// becomes `read_lut(lut, 255 · clamp(v, 0, 1)) / 255`, the table read
/// piecewise linearly as [`map_gloss`] reads it (a NaN reads as 0). One row
/// of `w` at a time, polling `job` per row. A Smooth bevel's contour site
/// (`style_bevel_smooth`, #2).
pub(crate) fn map_unit_plane_with_job(
    plane: &mut [f32],
    w: u32,
    lut: &[u8; 256],
    job: JobContext<'_>,
) -> Result<(), String> {
    let row = (w as usize).max(1);
    for r in plane.chunks_mut(row) {
        job.check()?;
        for v in r {
            *v = (read_lut(lut, 255.0 * f64::from(*v).clamp(0.0, 1.0)) / 255.0) as f32;
        }
    }
    Ok(())
}

/// The gloss contour on the bevel's signed shading `d` in -1..=1 (module
/// doc): `2 C((d + 1) / 2) - 1`, `C` read piecewise linearly from `lut`.
/// A non-finite `d` stays as it is (the bevel shades it to nothing).
pub(crate) fn map_gloss(d: f32, lut: &[u8; 256]) -> f32 {
    if !d.is_finite() {
        return d;
    }
    let at = |x: f64| 2.0 * read_lut(lut, 255.0 * x) / 255.0 - 1.0;
    let mut out = at((f64::from(d).clamp(-1.0, 1.0) + 1.0) / 2.0);
    if GLOSS_FLAT {
        out -= at(0.5);
    }
    out as f32
}
