//! KERNEL: Fill on five of the "special eight" modes (#578 take1,
//! the FILL BUILD: rule F1, form Fb). For Color Burn, Linear Burn, Color
//! Dodge, Linear Dodge (Add) and Vivid Light a Fill `f` below 1 is not a
//! plain alpha factor: Photoshop folds it into the blend, pulling the source
//! toward the mode's NEUTRAL `n` before blending —
//! `B(cb, n + f · (cs − n))`, `n` = 1 for the burns, 0 for the dodges, ½ for
//! Vivid Light — and composites the result at the layer's alpha × opacity
//! where the backdrop is opaque, the Fill reappearing only as the source's
//! own coverage over a transparent backdrop (`blend::composite_blended_split`:
//! `sa · f` on the `(1 − ab)` term, `sa` on the rest). take1 measured it
//! within 1 code at 8 bits and 0.016 at 16 on every Fill tile of those five
//! (opaque backdrops and a Normal group's transparent buffer alike). Linear
//! Light, Hard Mix and Difference keep the plain Fill (no candidate met their
//! tiles), as do 32-bit float scenes and CMYK, which take1 did not measure.
//!
//! The separable kernels are `blend.rs`'s (`b_color_burn`, `b_linear_burn`,
//! `b_color_dodge`, `b_addition`, `b_vivid_light`), the master; this module
//! only moves the source and splits the composite.

use crate::blend::{
    b_addition, b_color_burn, b_color_dodge, b_linear_burn, b_vivid_light, composite_blended,
    composite_blended_split, BlendMode,
};

/// One pixel pass's special Fill (module doc).
#[derive(Clone, Copy)]
pub(crate) struct FillBlend {
    /// `B(cb, n + fill · (cs − n))`: `(cb, cs, fill)`. RGB's five run per
    /// channel; a Lab document's (`lab_blend::fill_in`) fold the triple.
    pub f: fn([f32; 3], [f32; 3], f32) -> [f32; 3],
    /// The layer's Fill, in (0, 1).
    pub fill: f32,
    /// Form Fb (`composite_blended_split`); false is the plain W3C step.
    pub split: bool,
}

fn color_burn(cb: [f32; 3], cs: [f32; 3], fill: f32) -> [f32; 3] {
    [0, 1, 2].map(|k| b_color_burn(cb[k], 1.0 + fill * (cs[k] - 1.0)))
}

fn linear_burn(cb: [f32; 3], cs: [f32; 3], fill: f32) -> [f32; 3] {
    [0, 1, 2].map(|k| b_linear_burn(cb[k], 1.0 + fill * (cs[k] - 1.0)))
}

fn color_dodge(cb: [f32; 3], cs: [f32; 3], fill: f32) -> [f32; 3] {
    [0, 1, 2].map(|k| b_color_dodge(cb[k], fill * cs[k]))
}

fn linear_dodge(cb: [f32; 3], cs: [f32; 3], fill: f32) -> [f32; 3] {
    [0, 1, 2].map(|k| b_addition(cb[k], fill * cs[k]))
}

fn vivid_light(cb: [f32; 3], cs: [f32; 3], fill: f32) -> [f32; 3] {
    [0, 1, 2].map(|k| b_vivid_light(cb[k], 0.5 + fill * (cs[k] - 0.5)))
}

/// The special Fill of `mode` at `fill`: `Some` only for the five modes of
/// the module doc at a Fill below 1 in a scene that is not 32-bit float.
pub(crate) fn fill_blend(mode: BlendMode, fill: f32, float: bool) -> Option<FillBlend> {
    if float || fill.is_nan() || fill >= 1.0 {
        return None;
    }
    let f: fn([f32; 3], [f32; 3], f32) -> [f32; 3] = match mode {
        BlendMode::ColorBurn => color_burn,
        BlendMode::LinearBurn => linear_burn,
        BlendMode::ColorDodge => color_dodge,
        BlendMode::Addition => linear_dodge,
        BlendMode::VividLight => vivid_light,
        _ => return None,
    };
    Some(FillBlend {
        f,
        fill,
        split: true,
    })
}

impl FillBlend {
    /// `B(cb, n + fill · (cs − n))`.
    pub(crate) fn blend(&self, cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
        (self.f)(cb, cs, self.fill)
    }

    /// One source pixel `cs` at `sa` (alpha × coverage × weight × opacity,
    /// WITHOUT the Fill) onto the straight pixel `dst` (`[r, g, b, a]`).
    pub(crate) fn composite(&self, dst: &mut [f32], cs: &[f32; 3], sa: f32) {
        let blended = self.blend([dst[0], dst[1], dst[2]], *cs);
        if self.split {
            composite_blended_split(dst, cs, &blended, sa * self.fill, sa);
        } else {
            composite_blended(dst, cs, &blended, sa);
        }
    }
}
