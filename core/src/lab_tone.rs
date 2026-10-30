//! How a prepared RGB adjustment acts on a Lab Color document's L\*a\*b\*
//! codes (#1 phases 2b to 2d, `designs/lab-mode.md`): the pixel
//! rule of each [`LabForm`], over the one adjustment kernel.
//!
//! # Why seven forms and no Lab kernel
//!
//! A Lab document stores L\*, a\* and b\* in the R, G and B samples
//! (`lab_document`'s module doc), so an RGB kernel run on them as-is would
//! treat a\* and b\* as colour intensities, which they are not. The Lab
//! adjustments come in a few shapes, and each is the RGB kernel run on a
//! sample it does mean something for:
//!
//! - **Lightness** (Levels and Curves on the composite, Brightness/Contrast):
//!   Lab Levels and Curves list Lightness, a and b, Lightness first, and
//!   the composite edits L\* alone. The kernel runs on the gray
//!   `[L, L, L]`, where every per-channel tone curve and every luma reading
//!   agree, and its first output is the new L\*; a\* and b\* are kept bit for
//!   bit.
//! - **Channels** (Invert, Posterize): they apply to every channel, so the
//!   kernel runs on `[L, a, b]` itself. Invert's `1 − v` maps
//!   the 16-bit neutral 0x8080 to 0x7F7F (a\* = −1); an 8-bit document's
//!   neutral 128 inverts to 127.
//! - **Threshold** makes a high-contrast, black and white image, so the
//!   kernel's level reads L\* (the `[L, L, L]` luma
//!   is L) and a\* = b\* become the neutral [`NEUTRAL`].
//! - **Split** (phase 2c; Levels and Curves with their channel settings):
//!   the r, g and b settings are Lightness, a and b. L\* is the whole kernel
//!   on `[L, L, L]` (the r stage under the composite, as Lightness above);
//!   a\* and b\* are a second kernel holding the channel stages alone, run
//!   on `[L, a, b]` ([`split`]).
//! - **Chroma** (phase 2c; Brightness/Contrast with a saturation, which is
//!   Doctored's own control): L\* as Lightness, and a\* and b\* scaled
//!   about the neutral ([`chroma`]).
//! - **Desaturate** (phase 2c; Hue/Saturation's master saturation −1 alone,
//!   Image ▸ Adjustments ▸ Desaturate): grayscale in the same colour mode,
//!   with each pixel's lightness unchanged, so L\* is kept bit for bit and
//!   a\* = b\* = [`NEUTRAL`] (`chroma` with the identity and scale 0).
//! - **Color** (phase 2c; Hue/Saturation, Color Balance, Photo Filter, Color
//!   Lookup, Gradient Map, Shadows/Highlights, Replace Color): these act on
//!   colour, with no per-channel Lab rule, so the kernel runs on the
//!   sample's colour in the op's RGB space as on a
//!   document in that space, and the result returns to L\*a\*b\*
//!   ([`color`]). Since phase 2d the space is the built-in 16-bit ProPhoto
//!   RGB for the five that adjust the input colour, and sRGB for Color Lookup
//!   and Gradient Map, whose result colours are sRGB numbers
//!   (`lab_adjust::color_space`). A sample the op does not change keeps its
//!   codes exactly; a colour outside the space clips only where the op
//!   changes it.
//!
//! The gray-sample pattern's master copy is `cmyk_adjust_rgb::InkRgb::evaluate`
//! (the K plate through `[l, l, l]`). This module names no document type:
//! the conversions and kernels arrive as closures.

/// How a prepared adjustment acts on L\*a\*b\* codes (module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LabForm {
    /// The kernel on `[L, L, L]`; its first output is L\*, a\* and b\* kept.
    Lightness,
    /// The kernel on `[L, a, b]`.
    Channels,
    /// The kernel's level on `[L, L, L]`; a\* = b\* = 0.
    Threshold,
    /// L\* by the kernel on `[L, L, L]`, a\* and b\* by the channel stages
    /// on `[L, a, b]` ([`split`]).
    Split,
    /// L\* by the kernel on `[L, L, L]`, a\* and b\* scaled about 0
    /// ([`chroma`]).
    Chroma,
    /// L\* kept, a\* = b\* = 0.
    Desaturate,
    /// The kernel on the sample's colour in the op's space: ProPhoto RGB,
    /// or sRGB for Color Lookup and Gradient Map ([`color`]).
    Color,
}

/// a\* = b\* = 0 as a unit sample (0x8080 / 65535; `lab_document`'s
/// encoding).
pub(crate) const NEUTRAL: f32 = 32896.0 / 65535.0;

/// `kernel` applied to the unit L\*a\*b\* sample `lab` by `form`'s rule:
/// Lightness `[k([L,L,L])[0], a, b]`; Channels `k([L,a,b])`; Threshold
/// `[k([L,L,L])[0], NEUTRAL, NEUTRAL]`. The phase 2c forms have their own
/// functions; here Split and Chroma act as Lightness, Desaturate as
/// [`chroma`] at scale 0 and Color as Channels (the caller dispatches them).
pub(crate) fn apply(
    form: LabForm,
    lab: [f32; 3],
    kernel: &dyn Fn([f32; 3]) -> [f32; 3],
) -> [f32; 3] {
    let [l, a, b] = lab;
    match form {
        LabForm::Lightness | LabForm::Split | LabForm::Chroma => [kernel([l, l, l])[0], a, b],
        LabForm::Channels | LabForm::Color => kernel(lab),
        LabForm::Threshold => [kernel([l, l, l])[0], NEUTRAL, NEUTRAL],
        LabForm::Desaturate => [l, NEUTRAL, NEUTRAL],
    }
}

/// Split: `[k([L,L,L])[0], c([L,a,b])[1], c([L,a,b])[2]]`, `c` the kernel of
/// the channel stages only (module doc).
pub(crate) fn split(
    lab: [f32; 3],
    kernel: &dyn Fn([f32; 3]) -> [f32; 3],
    channels: &dyn Fn([f32; 3]) -> [f32; 3],
) -> [f32; 3] {
    let l = lab[0];
    let c = channels(lab);
    [kernel([l, l, l])[0], c[1], c[2]]
}

/// Chroma: `[k([L,L,L])[0], clamp01(NEUTRAL + scale·(a − NEUTRAL)), same for
/// b]` — a\* and b\* about 0, the colour's own saturation (module doc).
pub(crate) fn chroma(lab: [f32; 3], kernel: &dyn Fn([f32; 3]) -> [f32; 3], scale: f32) -> [f32; 3] {
    let [l, a, b] = lab;
    let about = |v: f32| (NEUTRAL + scale * (v - NEUTRAL)).clamp(0.0, 1.0);
    [kernel([l, l, l])[0], about(a), about(b)]
}

/// Color: `rgb = clamp01(to_rgb(lab))`, `out = kernel(rgb)`; `lab` itself
/// when `out == rgb` bit for bit (a sample the op leaves alone keeps its
/// codes, out of sRGB too), else `clamp01(to_lab(out))`.
pub(crate) fn color(
    lab: [f32; 3],
    to_rgb: &dyn Fn([f32; 3]) -> [f32; 3],
    kernel: &dyn Fn([f32; 3]) -> [f32; 3],
    to_lab: &dyn Fn([f32; 3]) -> [f32; 3],
) -> [f32; 3] {
    let rgb = to_rgb(lab).map(|v| v.clamp(0.0, 1.0));
    let out = kernel(rgb);
    if out
        .iter()
        .zip(&rgb)
        .all(|(o, r)| o.to_bits() == r.to_bits())
    {
        return lab;
    }
    to_lab(out).map(|v| v.clamp(0.0, 1.0))
}
