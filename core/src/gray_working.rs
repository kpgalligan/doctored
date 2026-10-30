//! The Gray working space (Edit ▸ Color Settings ▸ Working Spaces ▸
//! Gray) as the curve Load Selection's luminosity takes at 8 and 16 bits.
//!
//! Photoshop 26.10.0 loads the RGB composite's luminosity as the document
//! profile's relative PCS Y (the encoded codes decoded through the profile)
//! carried into the working Gray: `v = Y^e`, with Dot Gain d % a power curve
//! through `0.5 − d/100` at 50 % (`e = ln 0.5 / ln(0.5 − d/100)`) and Gray
//! Gamma γ the plain `e = 1/γ`. The take
//! (`scripts/photoshop-probe/gray_luminance_take.py`, receipt
//! `gray-luminance/take1`) read this exactly — 0 codes off on all 26 patches
//! at 8 and 16 bits — under Dot Gain 20 % (Photoshop's default), Gray Gamma
//! 2.2 and Dot Gain 15 %, where Rec. 709 over the encoded codes was 30, 52 and
//! 35 codes off. A power curve is the model, not Photoshop's own dot-gain
//! tables: the take bounds it at ±1 code at those three spaces.
//!
//! sGray (#547, the Grayscale document mode) is the eighth space: IEC
//! 61966-2-1's sRGB curve on one channel, which a PNG `sRGB` chunk on a gray
//! file declares. [`GrayWorkingSpace::linear`] is each space's FORWARD curve
//! (gray code → relative luminance), the `kTRC` `icc_builtin::gray_blob`
//! tabulates.
//!
//! The space is an app preference (`ColorSettings.workingGray`), passed per
//! call; nothing here reads a document.

use serde::Deserialize;

/// The eight Gray working spaces (Color Settings' seven and sGray), by their
/// wire name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub(crate) enum GrayWorkingSpace {
    #[serde(rename = "dot_gain_10")]
    DotGain10,
    #[serde(rename = "dot_gain_15")]
    DotGain15,
    #[serde(rename = "dot_gain_20")]
    DotGain20,
    #[serde(rename = "dot_gain_25")]
    DotGain25,
    #[serde(rename = "dot_gain_30")]
    DotGain30,
    #[serde(rename = "gray_gamma_18")]
    GrayGamma18,
    #[serde(rename = "gray_gamma_22")]
    GrayGamma22,
    #[serde(rename = "sgray")]
    SGray,
}

impl GrayWorkingSpace {
    pub(crate) const ALL: [GrayWorkingSpace; 8] = [
        GrayWorkingSpace::DotGain10,
        GrayWorkingSpace::DotGain15,
        GrayWorkingSpace::DotGain20,
        GrayWorkingSpace::DotGain25,
        GrayWorkingSpace::DotGain30,
        GrayWorkingSpace::GrayGamma18,
        GrayWorkingSpace::GrayGamma22,
        GrayWorkingSpace::SGray,
    ];

    /// The space named `name` (its wire name), else `None`.
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|space| space.name() == name)
    }

    /// The wire name (`"dot_gain_20"`, `"gray_gamma_22"`, …).
    pub(crate) fn name(self) -> &'static str {
        match self {
            GrayWorkingSpace::DotGain10 => "dot_gain_10",
            GrayWorkingSpace::DotGain15 => "dot_gain_15",
            GrayWorkingSpace::DotGain20 => "dot_gain_20",
            GrayWorkingSpace::DotGain25 => "dot_gain_25",
            GrayWorkingSpace::DotGain30 => "dot_gain_30",
            GrayWorkingSpace::GrayGamma18 => "gray_gamma_18",
            GrayWorkingSpace::GrayGamma22 => "gray_gamma_22",
            GrayWorkingSpace::SGray => "sgray",
        }
    }

    /// The name Color Settings shows, which is also the built-in
    /// Gray profile's `desc` (`icc_builtin::gray_blob`).
    pub(crate) fn display_name(self) -> &'static str {
        match self {
            GrayWorkingSpace::DotGain10 => "Dot Gain 10%",
            GrayWorkingSpace::DotGain15 => "Dot Gain 15%",
            GrayWorkingSpace::DotGain20 => "Dot Gain 20%",
            GrayWorkingSpace::DotGain25 => "Dot Gain 25%",
            GrayWorkingSpace::DotGain30 => "Dot Gain 30%",
            GrayWorkingSpace::GrayGamma18 => "Gray Gamma 1.8",
            GrayWorkingSpace::GrayGamma22 => "Gray Gamma 2.2",
            GrayWorkingSpace::SGray => "sGray",
        }
    }

    /// Dot Gain d %: ln 0.5 / ln(0.5 − d/100), the power that sends a 50 %
    /// grey to the reflectance `0.5 − d/100` prints it at. Gray Gamma γ: 1/γ.
    /// sGray is not a power curve: [`Self::gray`] and [`Self::linear`] take
    /// the IEC formula for it, and this answers its nominal 1/2.2.
    fn exponent(self) -> f64 {
        let dot_gain = |d: f64| 0.5f64.ln() / (0.5 - d / 100.0).ln();
        match self {
            GrayWorkingSpace::DotGain10 => dot_gain(10.0),
            GrayWorkingSpace::DotGain15 => dot_gain(15.0),
            GrayWorkingSpace::DotGain20 => dot_gain(20.0),
            GrayWorkingSpace::DotGain25 => dot_gain(25.0),
            GrayWorkingSpace::DotGain30 => dot_gain(30.0),
            GrayWorkingSpace::GrayGamma18 => 1.0 / 1.8,
            GrayWorkingSpace::GrayGamma22 | GrayWorkingSpace::SGray => 1.0 / 2.2,
        }
    }

    /// clamp(y, 0, 1)^exponent: the gray of relative luminance `y`; for
    /// sGray the IEC 61966-2-1 encode.
    pub(crate) fn gray(self, y: f64) -> f64 {
        let y = y.clamp(0.0, 1.0);
        match self {
            GrayWorkingSpace::SGray if y <= 0.0031308 => 12.92 * y,
            GrayWorkingSpace::SGray => 1.055 * y.powf(1.0 / 2.4) - 0.055,
            _ => y.powf(self.exponent()),
        }
    }

    /// The forward curve, [`Self::gray`]'s inverse: the relative luminance a
    /// gray code `v` in [0, 1] carries (`v^(1/exponent)`; for sGray the IEC
    /// decode).
    pub(crate) fn linear(self, v: f64) -> f64 {
        let v = v.clamp(0.0, 1.0);
        match self {
            GrayWorkingSpace::SGray if v <= 0.04045 => v / 12.92,
            GrayWorkingSpace::SGray => ((v + 0.055) / 1.055).powf(2.4),
            _ => v.powf(1.0 / self.exponent()),
        }
    }
}
