//! A layer style in a 32-bit FLOAT document: the two statements
//! that differ from an encoded document's, each MEASURED in Photoshop 26.10.0
//! before it was built (`scripts/photoshop-probe/float_styles.py`, receipt
//! `doctored-fixtures/photoshop/float-styles-473/take5`; the rule was fixed
//! in the runner before the take). Everything else about a style — the u8
//! shape planes (`style_render::render_effects_with_job` over
//! `precision_style_render::typed_shape`), the Below / pixels / Interior
//! order, fill opacity, knockout, Blend Interior Effects as Group, the
//! package model — is the shipped typed path, unchanged: Photoshop's 32-bit
//! effect geometry matched its 16-bit geometry within 2/255 on every effect,
//! and its blending options matched the shipped typed rule at 1e-4.
//!
//! - **Colour.** A 32-bit document's effect colour numbers are its own LINEAR
//!   components (a descriptor colour of (255, 128, 0) composites as
//!   (1, 0.502, 0), every effect, 1e-7), and a 16-bit layer pasted into it
//!   has its effect colours converted (128 → 55.044 = 255 · decode(128/255)),
//!   so a style keeps its look across the depth change. Doctored authors
//!   every style colour in sRGB, so a float document composites it as
//!   sRGB → document LINEAR light ([`LinearInk`], `float_paint`'s ink rule;
//!   on a document `MatrixTrc::approx_eq` calls sRGB, the document's own
//!   decode, as an encoded document keeps the numbers verbatim),
//!   at composite time like every other depth (`PlaneColor::converted`'s
//!   placement), and the cache and the style JSON stay depth-free. A gradient
//!   interpolates between its stops carried into linear light
//!   (`PlaneColor::LinearGradient`; Photoshop's 32-bit red → blue ramp held
//!   R + B = 1 in linear light at 6e-8, where the 16-bit one held it in
//!   encoded numbers).
//! - **Blend.** The effect's mode composites through
//!   `blend_float::effect_blend_kind_at` — the layer's float formula, but for
//!   two measured effect variants (Darken and Lighten). The modes a 32-bit
//!   effect admits are `float_document::style_refusal`'s question.
//!
//! The pixels' Blend If at float is `style_blend_if::source_weight_float`;
//! opacity stays exact f32 at every depth (Photoshop's opacity is an 8-bit
//! byte).

use crate::blend::{BlendKind, BlendMode};
use crate::blend_float::effect_blend_kind_at;
use crate::icc::IccProfile;
use crate::icc_transform::LinearInk;
use crate::style_render::{unit_color, PlaneColor};

/// A float document's style statements (module doc), built ONCE per render
/// beside the encoded scenes' `StyleEnv::colors_for`.
pub(crate) struct FloatStyle {
    ink: LinearInk,
}

impl FloatStyle {
    /// sRGB → `profile`'s linear light. On a document whose profile
    /// `MatrixTrc::approx_eq` calls sRGB the authored numbers ARE the
    /// document's — `PlaneColor::converted`'s rule, under which an 8- or
    /// 16-bit document keeps them verbatim — so they are decoded by the
    /// document's own curves alone (`LinearInk::decoding`); any other profile
    /// takes sRGB's curves and the sRGB → document matrix. `Err` for a
    /// profile with no matrix/TRC model — which a float document cannot carry
    /// (`FloatPolicy::for_profile`), so a float render never meets one.
    pub(crate) fn for_profile(profile: &IccProfile) -> Result<Self, String> {
        let srgb = IccProfile::srgb();
        let source = srgb.model().ok_or("missing sRGB model")?;
        let destination = profile
            .model()
            .ok_or("a 32-bit float layer style requires an RGB matrix/TRC profile")?;
        let ink = if source.approx_eq(destination) {
            LinearInk::decoding(destination)
        } else {
            LinearInk::new(source, destination)
        };
        Ok(Self { ink })
    }

    /// An effect's blend behaviour at float (`blend_float::effect_blend_kind_at`).
    pub(crate) fn effect_kind(&self, mode: BlendMode) -> BlendKind {
        effect_blend_kind_at(mode, true)
    }

    /// The AUTHORED colour in the document's linear light: a solid colour
    /// through the ink, a gradient's stops through the ink with the sampler
    /// (the geometry) shared, a wide colour through the ink's matrix alone.
    /// A `LinearGradient` is already there.
    pub(crate) fn plane_color(&self, color: &PlaneColor) -> PlaneColor {
        match color {
            PlaneColor::Solid(rgb) => PlaneColor::Solid(self.descriptor_numbers(*rgb, None)),
            // A wide colour is linear sRGB already: the matrix alone.
            PlaneColor::Wide(linear) => {
                PlaneColor::Solid(self.descriptor_numbers(*linear, Some(*linear)))
            }
            // Dither at half a 16-bit code (`style_gradient`), as
            // `adjust_map` dithers a precision document.
            PlaneColor::Gradient(sampler) => PlaneColor::LinearGradient(
                sampler.clone().with_dither_divisor(65535.0),
                sampler
                    .fill
                    .stops
                    .iter()
                    .map(|stop| self.linear(unit_color(stop.color)))
                    .collect(),
            ),
            PlaneColor::LinearGradient(..) => color.clone(),
            // Document values already (take1's FLOAT = VERBATIM).
            PlaneColor::Pattern(_) => color.clone(),
        }
    }

    /// An authored colour as a 32-bit descriptor's numbers in 0…1: the
    /// document's linear light (`linear`'s neutral pins); a wide `linear`
    /// sRGB colour through the matrix alone. Photoshop's 32-bit `RGBC` and
    /// `SoCo` numbers are these × 255 (module doc's colour statement; the
    /// `fill_style32.psd` fixture's `SoCo`), so the composite and the
    /// written file share one conversion.
    pub(crate) fn descriptor_numbers(&self, color: [f32; 3], wide: Option<[f32; 3]>) -> [f32; 3] {
        match wide {
            Some(linear) => self.ink.undecoded(linear).map(|v| v as f32),
            None => self.linear(color),
        }
    }

    /// Authored white and black are decided on the AUTHORED numbers, not the
    /// converted ones: white is every matrix/TRC document's white, exactly 1
    /// per component, and black exactly 0 — the neutral components of the
    /// Darken / Lighten effect variants (`blend_float::b_darken_effect_float`),
    /// which Photoshop reads off its doc-space numbers where white IS 1. The
    /// sRGB → document matrix only lands near them (Display P3 white's blue
    /// reads 1 − 2e-5), and a component a hair under 1 would take the
    /// minimum and tint the backdrop. Only the whole neutral is pinned: a
    /// chromatic colour's 255 component is not 1 in a wider document's
    /// primaries.
    fn linear(&self, rgb: [f32; 3]) -> [f32; 3] {
        if rgb == [1.0; 3] || rgb == [0.0; 3] {
            return rgb;
        }
        self.ink.linear(rgb).map(|v| v as f32)
    }
}
