//! The CIE L*a*b* readout: one straight RGB triple of a DOCUMENT's pixels,
//! through that document's own profile, against the D50 PCS white.
//!
//! # Which white, and why nothing is adapted here
//!
//! **D50.** This Lab is defined on the ICC PCS white, which is D50, and
//! the Info panel's readout is that Lab. More usefully for this
//! codebase, the correct pipeline falls out of what the colour-management
//! work already built and costs nothing extra: a matrix/TRC profile's
//! `rXYZ`/`gXYZ`/`bXYZ` columns **are already D50-adapted** (the ICC spec
//! requires it; `icc_transform`'s module doc has the proof), so the readout
//! is simply
//!
//! ```text
//! encoded RGB -> the profile's own TRCs -> linear
//!             -> the profile's own to_pcs -> XYZ, already D50
//!             -> Lab against (0.96422, 1.0, 0.82521)
//! ```
//!
//! No extra adaptation and no sRGB assumption — which is the entire point:
//! the SAME bytes read differently in an sRGB and a Display P3 document, and
//! that difference is what tells a user their numbers are being honoured.
//! Worked values the tests pin: `(200, 150, 120)` is
//! `L 66.408, a 16.507, b 23.547` in an sRGB document and
//! `L 66.743, a 21.179, b 27.018` in a Display P3 one.
//!
//! A profile this build cannot model — the LUT-based profiles the core
//! deliberately keeps on a document rather than refusing — has no
//! matrix/TRC model, so [`DrDocument::lab`] returns `None` and the host says
//! so. A silent sRGB fallback would be exactly the bug the colour-management
//! work spent a page warning about.

use crate::doc::DrDocument;

/// The ICC PCS white as XYZ.
const D50: [f32; 3] = [0.96422, 1.00000, 0.82521];

/// CIE's `epsilon` and `kappa`, in their exact rational form.
const EPSILON: f32 = 216.0 / 24389.0;
const KAPPA: f32 = 24389.0 / 27.0;

/// The CIE L*a*b* companding function.
fn f(t: f32) -> f32 {
    if t > EPSILON {
        t.cbrt()
    } else {
        (KAPPA * t + 16.0) / 116.0
    }
}

/// XYZ (already D50-adapted) to L*a*b*.
pub(crate) fn xyz_to_lab(xyz: [f32; 3]) -> [f32; 3] {
    let fx = f(xyz[0] / D50[0]);
    let fy = f(xyz[1] / D50[1]);
    let fz = f(xyz[2] / D50[2]);
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// [`f`]'s inverse, on the same `epsilon`/`kappa` pair.
fn f_inverse(t: f32) -> f32 {
    let cube = t * t * t;
    if cube > EPSILON {
        cube
    } else {
        (116.0 * t - 16.0) / KAPPA
    }
}

/// How far outside `[0, 1]` a LINEAR sRGB component may land and still count
/// as in gamut for [`lab_to_srgb`]. Not zero because the built-in sRGB
/// profile's white is stored as s15Fixed16 and so is not exactly [`D50`]:
/// its Z column sums to `54060 / 65536 = 0.82489` against `0.82521`, which
/// sends Lab white `(100, 0, 0)` to a linear blue of about `1.00045`. A
/// tolerance of `1e-3` absorbs that representation gap with room to spare
/// while staying far below one 8-bit step of linear light near white
/// (about `0.0045`), so a genuinely out-of-gamut colour is still flagged.
const GAMUT_SLACK: f64 = 1.0e-3;

/// One ENCODED sRGB colour (each component in `[0, 1]`, clamped there) as
/// CIE L*a*b* against the D50 PCS white — exactly [`DrDocument::lab`]'s
/// pipeline on the BUILT-IN sRGB profile, in float rather than on bytes, so
/// the colour picker and the Info panel read the same numbers. `None` for a
/// non-finite component, or when the built-in sRGB profile has no
/// matrix/TRC model.
pub(crate) fn srgb_to_lab(rgb: [f32; 3]) -> Option<[f32; 3]> {
    if rgb.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let srgb = crate::icc::IccProfile::srgb();
    let model = srgb.model()?;
    Some(xyz_to_lab(model.to_pcs_xyz(rgb.map(|v| v.clamp(0.0, 1.0)))))
}

/// [`xyz_to_lab`]'s twin: Lab against the same D50 PCS white to XYZ, then
/// through the BUILT-IN sRGB model's inverse matrix
/// ([`crate::icc_transform::MatrixTrc::pcs_to_linear`]) and
/// `adjust_math::linear_to_srgb`, which clamps. Answers the ENCODED sRGB
/// components in `[0, 1]` and whether the colour was in gamut — every
/// linear component within [`GAMUT_SLACK`] of `[0, 1]` — before that clamp.
/// `None` for a non-finite component, or when the built-in sRGB profile has
/// no matrix/TRC model, so the caller declines rather than carrying a
/// silently black colour.
///
/// sRGB is not an assumption here, it is the destination: a Lab colour
/// typed into the picker or stored in a `phfl` is a colour, not a pixel,
/// and every authored colour this crate stores is sRGB (`style_composite`
/// converts one into a document's space downstream, which is exactly what
/// `psd_adjustment`'s `to_document` does with [`lab_to_srgb8`]'s result).
/// The encode is the analytic sRGB OETF rather than the profile's sampled
/// TRC because there is no curve inverter in this crate and adding one is a
/// documented regression.
pub(crate) fn lab_to_srgb(lab: [f32; 3]) -> Option<([f32; 3], bool)> {
    if lab.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let fy = (lab[0] + 16.0) / 116.0;
    let fx = fy + lab[1] / 500.0;
    let fz = fy - lab[2] / 200.0;
    let xyz = [
        f_inverse(fx) * D50[0],
        f_inverse(fy) * D50[1],
        f_inverse(fz) * D50[2],
    ];
    let srgb = crate::icc::IccProfile::srgb();
    let model = srgb.model()?;
    let linear = model.pcs_to_linear(xyz.map(f64::from));
    let in_gamut = linear
        .iter()
        .all(|v| (-GAMUT_SLACK..=1.0 + GAMUT_SLACK).contains(v));
    Some((
        linear.map(|v| crate::adjust_math::linear_to_srgb(v as f32)),
        in_gamut,
    ))
}

/// The one place a Photoshop Lab colour becomes an sRGB byte triple:
/// [`lab_to_srgb`], rounded and clamped. `None` exactly when it is.
pub(crate) fn lab_to_srgb8(lab: [f32; 3]) -> Option<[u8; 3]> {
    lab_to_srgb(lab).map(|(c, _)| c.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8))
}

impl DrDocument {
    /// The CIE L*a*b* of one straight RGB triple of THIS document's pixels,
    /// read through the document's own profile against the D50 PCS white
    /// (module doc). `None` for a profile with no matrix/TRC model, where
    /// the host must say so rather than assume sRGB.
    pub fn lab(&self, rgb: [u8; 3]) -> Option<[f32; 3]> {
        let model = self.profile.model()?;
        let encoded = rgb.map(|v| f32::from(v) / 255.0);
        Some(xyz_to_lab(model.to_pcs_xyz(encoded)))
    }
}
