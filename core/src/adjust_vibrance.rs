//! Vibrance at 8 and 16 bits, bound to the document profile's TRC (#2): a
//! thin adapter over the `adjust_color` kernel, in the `adjust_photo_filter`
//! pattern. It names `IccProfile`, so it is not a kernel module.
//!
//! Photoshop 26.10.0 runs BOTH stages on the document's own LINEAR values
//! (`adjust_color`'s module doc: the Saturation stage `Ddoc` in
//! `scripts/photoshop-probe/adjust_v1x_take.py`, the Vibrance stage V3t-P in
//! `adjust_arith_578_take.py`). So a pixel is decoded once through the
//! profile's TRC, takes `Vibrance::vibrance_linear` then
//! `Vibrance::saturation_linear` (each skipped at 0), and is encoded once by
//! the render depth's encoder.
//!
//! The basis: at 8 bits the profile's model (`in_profile`); at 16 bits the
//! profile's model when it passes the precision checks. A profile failing them
//! whose curves `MatrixTrc::exposure_domain` still reads binds that per-channel
//! POWER instead (decode `x^g`, encode `u^(1/g)`): Apple's ROMM RGB at its
//! 1.8 (ITU-709 at 1.883), which take1's ROMM 16-bit rows measured within
//! 16.8 / 0.40 codes where the builtin sRGB curve missed by 72.8 / 4.01
//! (`adjust_arith_578_take.py`, ROMM 16). Any other profile, or one with no
//! model, binds the builtin sRGB model (the `legacy_srgb` rule, as Photo
//! Filter's).
//!
//! The stored `vibrance`/`saturation` are Photoshop's own sliders, so the
//! description carries no version tag: every saved 8/16-bit Vibrance
//! re-renders this way (the 185d precedent).

use std::sync::{Arc, OnceLock};

use crate::adjust_calibrated::{precision_model, Encoder};
use crate::adjust_color::Vibrance;
use crate::adjust_photo_filter::srgb_model;
use crate::icc::IccProfile;
use crate::icc_transform::{MatrixTrc, PrecisionToneEncoder, ToneEncoder};
use crate::resources::Inventory;

/// A Vibrance bound to a profile's decode and the render depth's encoder.
#[derive(Clone)]
pub(crate) struct BoundVibrance {
    op: Vibrance,
    basis: Arc<TrcBasis>,
}

/// The decode and encode a binding runs: the profile's model and the render
/// depth's encoder, or a per-channel power (module doc). A basis only ever
/// lives behind the binding's `Arc`, so the arms' size difference is never
/// copied.
#[allow(clippy::large_enum_variant)]
enum TrcBasis {
    Model { source: MatrixTrc, encoder: Encoder },
    Power([f64; 3]),
}

impl TrcBasis {
    fn decode(&self, rgb: [f32; 3]) -> [f32; 3] {
        match self {
            Self::Model { source, .. } => source.decode_rgb(rgb).map(|v| v as f32),
            Self::Power(gammas) => {
                std::array::from_fn(|c| (f64::from(rgb[c]).max(0.0).powf(gammas[c])) as f32)
            }
        }
    }

    fn encode(&self, linear: [f32; 3]) -> [f32; 3] {
        match self {
            Self::Model { encoder, .. } => encoder.rgb(linear.map(f64::from)),
            Self::Power(gammas) => std::array::from_fn(|c| {
                (f64::from(linear[c]).clamp(0.0, 1.0).powf(1.0 / gammas[c])) as f32
            }),
        }
    }
}

/// The builtin sRGB byte basis, built once and shared by every unbound parse
/// (`Adjustment::from_op`) and every model-less byte binding.
fn srgb_byte_basis() -> Arc<TrcBasis> {
    static BASIS: OnceLock<Arc<TrcBasis>> = OnceLock::new();
    BASIS
        .get_or_init(|| {
            let source = srgb_model();
            let encoder = Encoder::Byte(ToneEncoder::new(&source));
            Arc::new(TrcBasis::Model { source, encoder })
        })
        .clone()
}

impl BoundVibrance {
    /// Bound to the builtin sRGB profile at 8 bits: what an unbound parse
    /// evaluates under (`dr_image_adjust_op`, `apply_to_image`).
    pub(crate) fn srgb(op: Vibrance) -> BoundVibrance {
        BoundVibrance {
            op,
            basis: srgb_byte_basis(),
        }
    }

    /// Rebound to `profile` at 8 bits. Never fails: the layer render binds
    /// through `in_profile(..).ok()`, where an `Err` would silently drop the
    /// layer. A profile without a matrix/TRC model keeps the builtin sRGB
    /// binding.
    pub(crate) fn in_profile(self, profile: &IccProfile) -> BoundVibrance {
        let Some(model) = profile.model() else {
            return BoundVibrance::srgb(self.op);
        };
        BoundVibrance {
            basis: Arc::new(TrcBasis::Model {
                source: model.clone(),
                encoder: Encoder::Byte(ToneEncoder::new(model)),
            }),
            op: self.op,
        }
    }

    /// Rebound to `profile` at 16 bits, through the continuous encoder (module
    /// doc): the profile's model when it passes the precision checks, else
    /// its `exposure_domain` power when there is one, else the builtin sRGB
    /// model. `Err` only when the encoder tables cannot be allocated.
    pub(crate) fn prepare_precision(self, profile: &IccProfile) -> Result<BoundVibrance, String> {
        let source = match precision_model(profile) {
            Ok(model) => model.clone(),
            Err(_) => match profile.model().and_then(MatrixTrc::exposure_domain) {
                Some(gammas) => {
                    return Ok(BoundVibrance {
                        basis: Arc::new(TrcBasis::Power(gammas)),
                        op: self.op,
                    })
                }
                None => srgb_model(),
            },
        };
        let encoder = Encoder::Precision(PrecisionToneEncoder::new(&source)?);
        Ok(BoundVibrance {
            basis: Arc::new(TrcBasis::Model { source, encoder }),
            op: self.op,
        })
    }

    /// The kernel: the 32-bit Scaled form runs its unbound sRGB-curve
    /// master (`adjust_float`).
    pub(crate) fn op(&self) -> &Vibrance {
        &self.op
    }

    /// Both sliders at 0.
    pub(crate) fn is_identity(&self) -> bool {
        self.op.is_identity()
    }

    /// One ENCODED straight RGB triple in [0, 1] (module doc): decoded once,
    /// both stages, encoded once. Identity returns `rgb` bit for bit.
    pub(crate) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.op.is_identity() {
            return rgb;
        }
        let basis = &*self.basis;
        let linear = self.op.vibrance_linear(basis.decode(rgb));
        basis.encode(self.op.saturation_linear(linear))
    }

    /// An ADMISSION input (`PrecisionAdjustment::record_resources`): the
    /// basis object and, under a model, its sampled curves and the precision
    /// encoder's tables.
    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        inventory.object(&*self.basis, "precision_adjustment", false);
        if let TrcBasis::Model { source, encoder } = &*self.basis {
            source.record_resources(inventory);
            if let Encoder::Precision(encoder) = encoder {
                encoder.record_resources(inventory);
            }
        }
    }
}
