//! The CMYK document's adjustment dispatch (#547): [`parse_cmyk`], the ONE
//! place a CMYK op is parsed, and [`InkRgb`], an RGB adjustment kernel
//! (`PrecisionAdjustment`, unchanged) bound to ink. Photoshop's evidence for
//! each binding is
//! `scripts/photoshop-probe/cmyk_adjust_filter_take.py` (take1, take2).
//!
//! # The bindings
//!
//! Both read the stored domain `L = 1 − ink` and write `ink' = 1 − out`:
//!
//! * [`InkBinding::PerPlate`] (P4) runs the kernel on `[L_c, L_m, L_y]` as
//!   RGB, then on `[L_k; 3]` and keeps the first component for K — every
//!   plate, K included, through the same per-channel curve. Invert,
//!   Posterize (Photoshop's 16-bit bins: take1 exact at every sample),
//!   Brightness/Contrast (legacy and modern) and Shadows/Highlights, whose
//!   guide is the ink tone (`cmyk_adjust_ink::ink_tone`), so a K-only or
//!   rich black reads as shadow.
//! * [`InkBinding::Cmy`] (C3K) runs the kernel on `[L_c, L_m, L_y]` and keeps
//!   K: Color Balance, Photo Filter and Color Lookup. Photo Filter's colour
//!   is an ink (`ink`), sent to the kernel as the `#rrggbb` of
//!   `round(255·(1 − ink))` C/M/Y (K ignored); the kernel parses against the
//!   built-in sRGB, the profile every binding uses (the numbers are `L`,
//!   not a colour space's).
//!
//! Brightness/Contrast's Saturation has no meaning on separate plates and
//! refuses. The native ink ops (Channel Mixer, Selective Color, Threshold,
//! Gradient Map, Replace Color) are `cmyk_adjust_ink`'s; Levels, Curves and
//! Hue/Saturation `cmyk_adjust`'s.
//!
//! # Refusals
//!
//! [`ADJUSTMENT_REFUSALS`] names each op a CMYK document refuses, with its
//! sentence: Exposure, Vibrance, Black & White and Doctored's RGB-only ops
//! (Hue Rotate, Grayscale, Sepia, White Balance). Shadows/Highlights and
//! Replace Color refuse as adjustment LAYERS (they apply to pixels only).
//! Any other op keeps the mode's sentence, "The \"<op>\" adjustment is
//! unavailable in a CMYK document".

use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::adjust_precision::PrecisionAdjustment;
use crate::cmyk_adjust::CmykAdjustment;
use crate::cmyk_adjust_ink::{parse_native, InkKernel};
use crate::cmyk_document::refused_sentence;
use crate::icc::IccProfile;

/// How an RGB kernel reads ink (module doc).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum InkBinding {
    PerPlate,
    Cmy,
}

/// The RGB ops a CMYK document runs on ink, and how.
const BINDINGS: &[(&str, InkBinding)] = &[
    ("invert", InkBinding::PerPlate),
    ("posterize", InkBinding::PerPlate),
    ("bcs", InkBinding::PerPlate),
    ("shadows_highlights", InkBinding::PerPlate),
    ("color_balance", InkBinding::Cmy),
    ("photo_filter", InkBinding::Cmy),
    ("color_lookup", InkBinding::Cmy),
];

/// Each op a CMYK document refuses, with its whole sentence (module doc).
pub(crate) const ADJUSTMENT_REFUSALS: &[(&str, &str)] = &[
    ("exposure", "Exposure is unavailable in a CMYK document"),
    ("vibrance", "Vibrance is unavailable in a CMYK document"),
    (
        "black_and_white",
        "Black & White is unavailable in a CMYK document",
    ),
    (
        "hue_rotate",
        "Hue Rotate is unavailable in a CMYK document: Doctored offers it on RGB documents only",
    ),
    (
        "grayscale",
        "Grayscale is unavailable in a CMYK document: Doctored offers it on RGB documents only",
    ),
    (
        "sepia",
        "Sepia is unavailable in a CMYK document: Doctored offers it on RGB documents only",
    ),
    (
        "white_balance",
        "White Balance is unavailable in a CMYK document: Doctored offers it on RGB documents \
         only",
    ),
];

/// The ONE CMYK op dispatch (module doc): Levels, Curves and
/// Hue/Saturation through `CmykAdjustment::parse`, the native ink ops, the
/// RGB-bound ops, then the refusals.
pub(crate) fn parse_cmyk(op: &str, params: &Map<String, Value>) -> Result<CmykAdjustment, String> {
    if matches!(op, "levels" | "curves" | "hue_saturation") {
        return CmykAdjustment::parse(op, params);
    }
    if let Some((_, sentence)) = ADJUSTMENT_REFUSALS.iter().find(|(name, _)| *name == op) {
        return Err((*sentence).to_string());
    }
    if let Some(kernel) = parse_native(op, params) {
        return kernel.map(CmykAdjustment::Ink);
    }
    if let Some((name, binding)) = BINDINGS.iter().find(|(name, _)| *name == op) {
        let kernel = InkRgb::parse(name, *binding, params)?;
        return Ok(CmykAdjustment::Ink(Arc::new(kernel)));
    }
    Err(refused_sentence(&format!("The \"{op}\" adjustment")))
}

/// An RGB adjustment kernel bound to ink (module doc).
pub(crate) struct InkRgb {
    kernel: PrecisionAdjustment,
    binding: InkBinding,
    op: &'static str,
}

impl InkRgb {
    fn parse(
        op: &'static str,
        binding: InkBinding,
        params: &Map<String, Value>,
    ) -> Result<Self, String> {
        let mut params = params.clone();
        match op {
            "bcs" => {
                let saturation = params.get("saturation").and_then(Value::as_f64);
                if params.contains_key("saturation") && saturation != Some(0.0) {
                    return Err(
                        "Saturation in Brightness/Contrast is unavailable in a CMYK \
                                document: use Hue/Saturation"
                            .into(),
                    );
                }
            }
            "photo_filter" => {
                if params.contains_key("color") {
                    return Err(refused_sentence(
                        "The RGB \"color\" key (give Photo Filter's \"ink\")",
                    ));
                }
                if let Some(ink) = params.remove("ink") {
                    let ink = ink
                        .as_array()
                        .filter(|a| a.len() == 4)
                        .and_then(|a| a.iter().map(Value::as_f64).collect::<Option<Vec<_>>>())
                        .filter(|a| a.iter().all(|v| (0.0..=1.0).contains(v)))
                        .ok_or("invalid CMYK photo_filter ink")?;
                    let code = |v: f64| (255.0 * (1.0 - v)).round() as u8;
                    params.insert(
                        "color".into(),
                        Value::String(format!(
                            "#{:02x}{:02x}{:02x}",
                            code(ink[0]),
                            code(ink[1]),
                            code(ink[2])
                        )),
                    );
                }
            }
            _ => {}
        }
        let meta = json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":params});
        let kernel = PrecisionAdjustment::parse(&meta.to_string(), &IccProfile::srgb())?;
        Ok(Self {
            kernel,
            binding,
            op,
        })
    }

    fn evaluate(&self, ink: [f32; 4], guide: f32) -> [f32; 4] {
        let rgb = self
            .kernel
            .apply_at([1.0 - ink[0], 1.0 - ink[1], 1.0 - ink[2]], guide, (0, 0));
        let k = match self.binding {
            InkBinding::PerPlate => {
                let l = 1.0 - ink[3];
                1.0 - self.kernel.apply_at([l, l, l], guide, (0, 0))[0]
            }
            InkBinding::Cmy => ink[3],
        };
        [1.0 - rgb[0], 1.0 - rgb[1], 1.0 - rgb[2], k]
    }
}

impl InkKernel for InkRgb {
    fn operation(&self) -> &'static str {
        self.op
    }

    fn apply(&self, ink: [f32; 4]) -> [f32; 4] {
        self.evaluate(ink, 0.0)
    }

    fn is_identity(&self) -> bool {
        self.kernel.is_identity()
    }

    fn heap_bytes(&self) -> u64 {
        self.kernel.retained_bytes()
    }

    fn layer_refusal(&self) -> Option<String> {
        (self.op == "shadows_highlights").then(|| {
            "A Shadows/Highlights adjustment layer is unavailable in a CMYK document: \
             Shadows/Highlights applies to pixels only"
                .into()
        })
    }

    fn guide_radius(&self) -> Option<f32> {
        self.kernel.guide_radius()
    }

    fn apply_guided(&self, ink: [f32; 4], guide: f32) -> [f32; 4] {
        self.evaluate(ink, guide)
    }
}
