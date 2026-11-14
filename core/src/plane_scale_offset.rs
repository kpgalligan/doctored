//! Apply Image's Scale and Offset: Add and Subtract on the UNCLAMPED sum,
//! `v = clamp((b ± s) / scale + offset, 0, 1)`, then the fade by opacity.
//!
//! Why the blend table cannot carry them: its Addition is `min(b + s, 1)` and
//! its Subtract `max(b − s, 0)` (`blend.rs`), so the excess is gone before
//! anything could divide it. Scale 2 with offset ½ is the one pair that keeps
//! a whole difference `b − s` in 0…1 (`(b − s)/2 + ½`), which is what a
//! split into a smooth layer and a detail layer is rebuilt from by hand.
//!
//! Units are the planes' own unit interval: `scale` in [1, 2], `offset` in
//! [−1, 1] (the app's −255…255 levels divided by 255 at every depth). The
//! identity pair (1, 0) never reaches [`ScaledArithmetic`]: at that pair the
//! formula IS the table's Addition and Subtract (both operands are in 0…1, so
//! `clamp` and `min`/`max` agree), and [`PlaneBlend::new`] keeps the table
//! for it, so every result the plane blenders gave before is bit-identical.
//! Any other pair is refused with every mode but Addition and Subtract.
//!
//! The fade is the compositor's own W3C step (`blend::composite_blended`)
//! over an opaque backdrop, exactly as `doc_plane::blend_triple` runs it for
//! the table, so there is one copy of "blend, then fade by opacity".
use crate::blend::{blend_kind, composite_blended, BlendKind, BlendMode};

/// The scale's closed range. Below 1 the sum would be amplified, which no
/// caller asks for; 2 is the most a sum of two unit values ever needs.
const SCALE_RANGE: std::ops::RangeInclusive<f32> = 1.0..=2.0;
/// The offset's closed range in plane units (±255 levels at 8 bits).
const OFFSET_RANGE: std::ops::RangeInclusive<f32> = -1.0..=1.0;

/// Add or Subtract with a non-identity scale and offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScaledArithmetic {
    subtract: bool,
    scale: f32,
    offset: f32,
}

impl ScaledArithmetic {
    /// `clamp((b ± s) / scale + offset, 0, 1)` for one component.
    fn component(self, b: f32, s: f32) -> f32 {
        let sum = if self.subtract { b - s } else { b + s };
        (sum / self.scale + self.offset).clamp(0.0, 1.0)
    }
}

/// The blend one plane pixel runs: the table, or Add/Subtract scaled.
#[derive(Clone, Copy)]
pub(crate) enum PlaneBlend {
    Table(BlendKind),
    Scaled(ScaledArithmetic),
}

impl PlaneBlend {
    /// `mode` with `scale` and `offset`, or the refusal's sentence: a
    /// non-finite or out-of-range value, or a non-identity pair with any mode
    /// but Addition and Subtract. The identity pair keeps the table.
    pub(crate) fn new(mode: BlendMode, scale: f32, offset: f32) -> Result<Self, &'static str> {
        if !scale.is_finite() || !SCALE_RANGE.contains(&scale) {
            return Err("plane blend scale must be finite in [1,2]");
        }
        if !offset.is_finite() || !OFFSET_RANGE.contains(&offset) {
            return Err("plane blend offset must be finite in [-1,1]");
        }
        if scale == 1.0 && offset == 0.0 {
            return Ok(Self::Table(blend_kind(mode)));
        }
        let subtract = match mode {
            BlendMode::Addition => false,
            BlendMode::Subtract => true,
            _ => return Err("plane blend scale and offset apply to Add and Subtract only"),
        };
        Ok(Self::Scaled(ScaledArithmetic {
            subtract,
            scale,
            offset,
        }))
    }

    /// One pixel: the (already inverted) operands as unit triples, blended
    /// and faded by `opacity` over an opaque backdrop. The table goes through
    /// `doc_plane::blend_triple` (Dissolve's dither at `xy` included).
    pub(crate) fn triple(self, b: [f32; 3], s: [f32; 3], opacity: f32, xy: (i64, i64)) -> [f32; 3] {
        match self {
            Self::Table(kind) => crate::doc_plane::blend_triple(b, s, opacity, kind, xy),
            Self::Scaled(arithmetic) => {
                let blended: [f32; 3] = std::array::from_fn(|c| arithmetic.component(b[c], s[c]));
                let mut acc = [b[0], b[1], b[2], 1.0];
                composite_blended(&mut acc, &s, &blended, opacity);
                [acc[0], acc[1], acc[2]]
            }
        }
    }
}
