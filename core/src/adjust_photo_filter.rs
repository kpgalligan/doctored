//! Photo Filter at 8 and 16 bits, bound to the document profile: a
//! thin adapter over the `adjust_mix` kernel. It names `IccProfile`, so it is
//! not a kernel module.
//!
//! # The form, fitted to Photoshop 26.10.0
//!
//! `M` is the profile's own tag matrix (D50 PCS, with NO native adaptation)
//! and `decode`/`encode` are its TRC:
//!
//! ```text
//! ratio = M·decode(color) / D50
//! y     = M⁻¹·(M·decode(v) ⊙ (1 − d + d·ratio))      # PhotoFilter::multiply_xyz
//! e     = encode(clamp01(y))
//! out   = SetLum(e, Lum(v))   with Preserve Luminosity  # PhotoFilter::preserve_encoded
//!       = e                   without it
//! ```
//!
//! SetLum is the W3C/PDF SetLum+ClipColor with `Lum = 0.3R + 0.59G + 0.11B`
//! on ENCODED values (`blend::set_lum`): Photoshop's Luminosity blend of the
//! filtered result over the input.
//!
//! Receipts: the take2 16-bit twins
//! (`doctored-fixtures/photoshop/float-adjust-473/take2/job/photo_filter/*_create_{srgb,wide}16.tif`)
//! and the phase-10 64×64 picture
//! (`doctored-fixtures/photoshop/adjustments/photoshop/{base,phfl}_{8,16}.png`:
//! sRGB, `#ec8a00`, density 0.4, preserve on). Max abs error, encoded units:
//! take2 16-bit sRGB 7e-4 (Lab, d 1, off), 6e-4 (`#ec8a00`, d 1, off), 6e-4
//! (Lab, d 0.25, on); ROMM median 5e-5, worst 3.4e-3 at the toe. Phase-10,
//! all 4096 pixels: 16-bit max 9.0e-4 (0.23 codes), median 4.8e-5; 8-bit max
//! 7.3 codes, p99 2.9, median 0.6, the worst all in ClipColor's `x > 1`
//! branch. The per-channel multiply with a Rec. 709 rescale this replaced
//! missed by 105 codes on phase-10.
//!
//! Why D50 PCS here and the NATIVE basis at 32 bits (`adjust_float`):
//! Photoshop itself renders the two depths differently. The 32-bit form
//! (native basis, Y held) misses 16-bit sRGB by 0.058 (basis) and 0.28 on a
//! preserve-on set (Y hold vs Luminosity blend); the two agree only on a
//! D50-white profile such as ROMM. Measured, so do not unify them.
//!
//! The stored `color`/`density`/`preserve_luminosity` are Photoshop's own
//! parameters, so the description carries no version tag (the 185d
//! precedent): every saved 8/16-bit Photo Filter re-renders this way.

use std::sync::{Arc, OnceLock};

use crate::adjust_calibrated::{precision_model, Encoder};
use crate::adjust_mix::PhotoFilter;
use crate::icc::IccProfile;
use crate::icc_transform::{MatrixTrc, PrecisionToneEncoder, ToneEncoder};
use crate::resources::Inventory;

/// A Photo Filter bound to a document profile's D50 PCS basis, its filter
/// colour's ratio already derived in that profile.
#[derive(Clone)]
pub(crate) struct BoundPhotoFilter {
    filter: PhotoFilter,
    basis: Arc<PcsBasis>,
    ratio: [f64; 3],
}

/// The profile's own decode and PCS matrices (`source`) and the encoder of
/// the render's depth.
#[derive(Clone)]
struct PcsBasis {
    source: MatrixTrc,
    encoder: Encoder,
}

/// The builtin sRGB model. Every byte binding without a model of its own
/// falls back to it — the `legacy_srgb` rule: a profile this crate cannot
/// model (a LUT profile) evaluates as sRGB.
/// `adjust_vibrance` binds through it too.
pub(crate) fn srgb_model() -> MatrixTrc {
    IccProfile::srgb()
        .model()
        .cloned()
        .expect("the builtin sRGB profile has a matrix/TRC model")
}

/// The builtin sRGB byte basis, built once and shared by every unbound
/// parse (`Adjustment::from_op`) and every model-less byte binding.
fn srgb_byte_basis() -> Arc<PcsBasis> {
    static BASIS: OnceLock<Arc<PcsBasis>> = OnceLock::new();
    BASIS
        .get_or_init(|| {
            let source = srgb_model();
            let encoder = Encoder::Byte(ToneEncoder::new(&source));
            Arc::new(PcsBasis { source, encoder })
        })
        .clone()
}

/// The filter colour's white-normalized PCS XYZ, `M·decode(color) / D50`,
/// with `color` the document's ENCODED numbers as every sheet writes them.
/// The ONE statement of the ratio: the 32-bit form (`adjust_float`) reads it
/// too.
pub(crate) fn pcs_ratio(model: &MatrixTrc, color: [f32; 3]) -> [f64; 3] {
    let xyz = model.to_pcs_xyz64(color.map(f64::from));
    let white = crate::icc_lut::D50;
    std::array::from_fn(|c| xyz[c] / white[c])
}

impl BoundPhotoFilter {
    /// Bound to the builtin sRGB profile at 8 bits: what an unbound parse
    /// evaluates under (`dr_image_adjust_op`, `apply_to_image`).
    pub(crate) fn srgb(filter: PhotoFilter) -> BoundPhotoFilter {
        let basis = srgb_byte_basis();
        let ratio = pcs_ratio(&basis.source, filter.color());
        BoundPhotoFilter {
            filter,
            basis,
            ratio,
        }
    }

    /// Rebound to `profile` at 8 bits. Never fails: the layer render binds
    /// through `in_profile(..).ok()`, where an `Err` would silently drop the
    /// layer. A profile without a matrix/TRC model keeps the builtin sRGB
    /// binding (the `legacy_srgb` rule, [`srgb_model`]).
    pub(crate) fn in_profile(self, profile: &IccProfile) -> BoundPhotoFilter {
        let Some(model) = profile.model() else {
            return BoundPhotoFilter::srgb(self.filter);
        };
        let ratio = pcs_ratio(model, self.filter.color());
        BoundPhotoFilter {
            basis: Arc::new(PcsBasis {
                source: model.clone(),
                encoder: Encoder::Byte(ToneEncoder::new(model)),
            }),
            filter: self.filter,
            ratio,
        }
    }

    /// Rebound to `profile` at 16 bits, through the continuous encoder. A
    /// profile with no model, or one failing the precision checks
    /// (`precision_model`), binds the builtin sRGB model instead (the
    /// `legacy_srgb` rule). `Err` only when the encoder tables cannot be
    /// allocated.
    pub(crate) fn prepare_precision(
        self,
        profile: &IccProfile,
    ) -> Result<BoundPhotoFilter, String> {
        let source = match precision_model(profile) {
            Ok(model) => model.clone(),
            Err(_) => srgb_model(),
        };
        let encoder = Encoder::Precision(PrecisionToneEncoder::new(&source)?);
        let ratio = pcs_ratio(&source, self.filter.color());
        Ok(BoundPhotoFilter {
            basis: Arc::new(PcsBasis { source, encoder }),
            filter: self.filter,
            ratio,
        })
    }

    /// The kernel, for the 32-bit form (`adjust_float`).
    pub(crate) fn filter(&self) -> &PhotoFilter {
        &self.filter
    }

    /// Density 0.
    pub(crate) fn is_identity(&self) -> bool {
        self.filter.is_identity()
    }

    /// One ENCODED straight RGB triple in [0, 1] (module doc). Identity
    /// returns `rgb` bit for bit.
    pub(crate) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.is_identity() {
            return rgb;
        }
        let basis = &*self.basis;
        let x = basis.source.to_pcs_xyz64(rgb.map(f64::from));
        let y = self.filter.multiply_xyz(x, self.ratio);
        let e = basis.encoder.rgb(basis.source.pcs_to_linear(y));
        self.filter.preserve_encoded(rgb, e)
    }

    /// An ADMISSION input (`PrecisionAdjustment::record_resources`): the
    /// basis object, the model's sampled curves and the precision encoder's
    /// tables.
    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        inventory.object(&*self.basis, "precision_adjustment", false);
        self.basis.source.record_resources(inventory);
        if let Encoder::Precision(encoder) = &self.basis.encoder {
            encoder.record_resources(inventory);
        }
    }
}
