//! A RAW local's tone and colour controls: Exposure, Contrast,
//! Highlights, Shadows, Whites, Blacks, Saturation, Clarity, Texture,
//! Sharpness and Noise Reduction, applied to the 16-bit development through the crate's existing adjustment
//! and Presence kernels, never a copy of their math. This module is the
//! kernel half: the control values, their ranges, and the mapping of each
//! control onto an existing precision_v1 adjustment description or the
//! Presence plane. `raw_local_tone_apply` binds the descriptions to the base
//! profile and evaluates them; `raw_local_develop` blends the result through
//! the local's coverage.
//!
//! The mapping, in evaluation order (a control at 0 adds no operation):
//! - Exposure, stops, `exposure {exposure}` under `document_v2`: a gain in
//!   linear light through the base profile's own transfer, so +1 doubles the
//!   light. A camera-exposure local (`exposure_offset`) is something else: it
//!   re-develops a camera variant, which recovers clipped highlights a gain on
//!   the development cannot.
//! - Contrast, `bcs {contrast: c/100, legacy: false}`: the modern power curve
//!   about mid-grey that Brightness/Contrast uses.
//! - Highlights and Shadows, one `shadows_highlights`: Shadows lifts the
//!   dark side, Highlights recovers the bright one, each at tone 0.5 and
//!   Color 0.2 (its default), midtone contrast 0, at the Presence coarse
//!   radius for the frame so the neighbourhood is the same fraction of the
//!   picture at every size. Both bands are written explicitly because an
//!   absent `shadows` band means 35 %.
//! - Whites and Blacks, one `levels`: positive Whites moves the input white
//!   point down by up to [`WHITES_BLACKS_SPAN`] (brighter highlights), negative
//!   Whites the output white point; negative Blacks moves the input black
//!   point up (deeper shadows), positive Blacks the output black point.
//! - Saturation, `vibrance {saturation: s/100}`: the Vibrance dialog's flat
//!   Saturation (`adjust_color`, #2).
//! - Clarity and Texture, the Presence plane (`sampling_presence`) of the
//!   result of the pointwise steps, Dehaze 0. Dehaze is not a local control:
//!   its veil needs the haze estimate of the whole picture.
//! - Sharpness and Noise Reduction, last, `local_detail::LocalDetail` over
//!   the Presence plane (or the pointwise steps): Reduce Noise, then Smart
//!   Sharpen or Gaussian Blur, the Develop Filter masks' mapping.
use crate::local_detail::LocalDetail;
use crate::sampling_presence::{presence_radii, Presence};
use serde::Deserialize;
use serde_json::{json, Value};

/// How far Whites and Blacks at ±100 move a levels point, as a fraction of
/// the range: a quarter keeps the strongest setting short of clipping a
/// whole tonal zone, the way a Develop slider (not a Levels dialog) behaves.
pub(crate) const WHITES_BLACKS_SPAN: f64 = 0.25;

/// One local's tone controls. Absent keys are 0; unknown keys are refused.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LocalTone {
    pub exposure: f64,
    pub contrast: f64,
    pub highlights: f64,
    pub shadows: f64,
    pub whites: f64,
    pub blacks: f64,
    pub saturation: f64,
    pub clarity: f64,
    pub texture: f64,
    pub sharpness: f64,
    pub noise_reduction: f64,
}

impl LocalTone {
    /// Exposure −4…4 stops; Contrast, Whites, Blacks, Saturation, Clarity,
    /// Texture and Sharpness −100…100; Highlights, Shadows and Noise
    /// Reduction 0…100; every value finite.
    pub fn validate(&self) -> Result<(), String> {
        let within = |v: f64, low: f64, high: f64| v.is_finite() && (low..=high).contains(&v);
        if !within(self.exposure, -4.0, 4.0)
            || !within(self.highlights, 0.0, 100.0)
            || !within(self.shadows, 0.0, 100.0)
            || !within(self.noise_reduction, 0.0, 100.0)
            || [
                self.contrast,
                self.whites,
                self.blacks,
                self.saturation,
                self.clarity,
                self.texture,
                self.sharpness,
            ]
            .iter()
            .any(|v| !within(*v, -100.0, 100.0))
        {
            return Err("invalid local tone: exposure is -4 to 4 stops, highlights, shadows and noise reduction 0 to 100, every other control -100 to 100".into());
        }
        Ok(())
    }

    /// Every control at 0: the local changes nothing.
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    /// The pointwise steps as precision_v1 adjustment descriptions, in
    /// evaluation order (module doc), one per control group that is not 0.
    /// `dimensions` sets the Shadows/Highlights radius.
    pub(crate) fn descriptions(&self, dimensions: (u32, u32)) -> Vec<String> {
        let describe = |op: &str, params: Value| {
            json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":params}).to_string()
        };
        let mut out = Vec::new();
        if self.exposure != 0.0 {
            out.push(describe(
                "exposure",
                json!({"exposure":self.exposure,"evaluation":"document_v2"}),
            ));
        }
        if self.contrast != 0.0 {
            out.push(describe(
                "bcs",
                json!({"contrast":self.contrast / 100.0,"legacy":false}),
            ));
        }
        if self.shadows != 0.0 || self.highlights != 0.0 {
            out.push(describe(
                "shadows_highlights",
                json!({
                    "shadows":{"amount":self.shadows / 100.0,"tone":0.5},
                    "highlights":{"amount":self.highlights / 100.0,"tone":0.5},
                    "radius":presence_radii(dimensions).coarse,
                    "color":0.2,
                    "midtone_contrast":0,
                }),
            ));
        }
        if self.whites != 0.0 || self.blacks != 0.0 {
            let mut params = serde_json::Map::new();
            let mut set = |key: &str, value: f64| {
                params.insert(key.into(), json!(value));
            };
            if self.whites > 0.0 {
                set("white", 1.0 - WHITES_BLACKS_SPAN * self.whites / 100.0);
            } else if self.whites < 0.0 {
                set(
                    "output_white",
                    1.0 + WHITES_BLACKS_SPAN * self.whites / 100.0,
                );
            }
            if self.blacks < 0.0 {
                set("black", WHITES_BLACKS_SPAN * -self.blacks / 100.0);
            } else if self.blacks > 0.0 {
                set("output_black", WHITES_BLACKS_SPAN * self.blacks / 100.0);
            }
            out.push(describe("levels", Value::Object(params)));
        }
        if self.saturation != 0.0 {
            out.push(describe(
                "vibrance",
                json!({"saturation":self.saturation / 100.0}),
            ));
        }
        out
    }

    /// Clarity and Texture as the Presence setting (Dehaze 0), or None when
    /// both are 0.
    pub(crate) fn presence(&self) -> Option<Presence> {
        (self.clarity != 0.0 || self.texture != 0.0).then_some(Presence {
            texture: self.texture,
            clarity: self.clarity,
            dehaze: 0.0,
        })
    }

    /// Sharpness and Noise Reduction as the detail stage, or None when both
    /// are 0.
    pub(crate) fn detail(&self) -> Option<LocalDetail> {
        let detail = LocalDetail {
            sharpness: self.sharpness,
            noise_reduction: self.noise_reduction,
        };
        (!detail.identity()).then_some(detail)
    }

    /// Whether the local reads a neighbourhood (a Shadows/Highlights guide,
    /// the Presence plane or the detail stage), so its evaluation builds a
    /// plane before blending.
    pub(crate) fn reads_plane(&self) -> bool {
        self.shadows != 0.0
            || self.highlights != 0.0
            || self.presence().is_some()
            || self.detail().is_some()
    }
}
