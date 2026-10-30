//! The retouch tools on a Lab Color document (#547 phase 2e,
//! `designs/lab-mode.md`): the Sponge's neutral and the source
//! refusals of the History Brush and the clone from plates; since 2f's third
//! part the Art History Brush's arm ([`LAB_ART`]) and Red Eye ([`red_eye`]).
//!
//! # The rules (Doctored's own; no take measured Photoshop's)
//!
//! * **Dodge and Burn shape L\* alone** (`precision_paint_tools`' `DodgeBurn`
//!   arm), the phase 2b rule that a tone op acts on Lightness. Protect Tones
//!   runs `doc_retouch::protected` on the one-element `[L]` group, which is
//!   the plain curve (`shaped(0)` at L\* = 0): a\* and b\* never enter the
//!   group's maximum and keep their codes.
//! * **The Sponge doubles or zeroes chroma about a\* = b\* = 0** ([`NEUTRAL`],
//!   `retouch_brush::Vibrancy::Lab`), keeping L\*; Vibrance is ignored, as on
//!   Gray.
//! * **Blur, Sharpen, Smudge, the Clone Stamp, Healing, Spot Healing, Patch,
//!   Content-Aware Fill and Content-Aware Move are per channel on the
//!   codes**, unchanged: each output channel reads its own channel, and
//!   `doc_inpaint` reads no profile.
//! * **The History Brush needs a Lab state** ([`history_source_refusal`]),
//!   and the clone from plates (Content-Aware Move) Lab plates
//!   ([`CLONE_SOURCE_REFUSAL`]): the codes of an RGB state are not Lab codes.
//! * **The Art History Brush (8 bits) is per channel on the codes**
//!   (`gray_art_history`'s u16 walk under [`LAB_ART`]): a Normal composite of
//!   `[L, a, b]`, from a Lab state only ([`LAB_ART_SOURCE_REFUSAL`]). Its
//!   strokes head along L\* ([`lab_luma`]); its Tolerance test compares the
//!   L\*, a\*, b\* and alpha codes (the kernel's largest premultiplied
//!   difference over every channel), so a change in a\* or b\* alone counts.
//!   Its modes are `lab_document::blend_refusal`'s: Darken and Lighten
//!   refuse; every other composites by the 8-bit Lab arms
//!   (`lab_blend::lab_blend_kind8`). 16 bits refuse as a 16-bit
//!   document does (`precision_art_history`).
//! * **Red Eye is a colour operation, run through 16-bit ProPhoto RGB**
//!   ([`red_eye`]), as the colour adjustments are (phase 2d): the red test
//!   and the correction read each pixel's ProPhoto colour as u16 codes, as
//!   on a 16-bit ProPhoto RGB document, so a wide-gamut colour survives; a
//!   pixel the correction leaves alone keeps its codes bit for bit, and a
//!   changed one is written back as the nearest u16 code at either depth
//!   (no ×257 rounding at 8 bits, the paint rule).
//! * **Remove** feeds its model the Display P3 view and converts the model's
//!   fill back to Lab codes (`doc_remove_model`'s "Lab Color").

use crate::adjust_precision::color_pipelines;
use crate::gray_art_history::U16Art;
use crate::icc::IccProfile;
use crate::icc_intent::ColorPipeline;
use crate::job_control::JobContext;
use crate::lab_document::lab_twin;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::reserved_vec;

/// The History Brush on a Lab document from a state that is not Lab.
pub(crate) const HISTORY_SOURCE_REFUSAL: &str = "The History Brush paints a Lab Color document from a Lab Color state only: this state is from before Image ▸ Mode ▸ Lab Color.";
/// The History Brush on a document that is not Lab from a Lab state.
pub(crate) const HISTORY_LAB_STATE_REFUSAL: &str = "The History Brush cannot paint from a Lab Color state: this state is from before Image ▸ Mode ▸ RGB Color.";
/// A clone into a Lab document from a source that is not Lab.
pub(crate) const CLONE_SOURCE_REFUSAL: &str =
    "A Lab Color document clones from a Lab Color document only: convert the source to Lab Color first.";

/// a\* = b\* = 0: code 0x8080 as a unit sample.
pub(crate) const NEUTRAL: f32 = 32896.0 / 65535.0;

/// `Some(NEUTRAL)` on a Lab document, else None.
pub(crate) fn sponge_neutral(doc: &PrecisionDocument) -> Option<f32> {
    doc.is_lab().then_some(NEUTRAL)
}

/// A Lab `doc` with a `source` state that is not Lab →
/// [`HISTORY_SOURCE_REFUSAL`]; a `doc` that is not Lab with a Lab `source` →
/// [`HISTORY_LAB_STATE_REFUSAL`]; None otherwise.
pub(crate) fn history_source_refusal(
    doc: &PrecisionDocument,
    source: &PrecisionDocument,
) -> Option<&'static str> {
    match (doc.is_lab(), source.is_lab()) {
        (true, false) => Some(HISTORY_SOURCE_REFUSAL),
        (false, true) => Some(HISTORY_LAB_STATE_REFUSAL),
        _ => None,
    }
}

/// A target that exists but is not a u16 raster.
pub(crate) const LAB_ART_TARGET_REFUSAL: &str =
    "The Art History Brush paints only a Lab Color raster layer";
/// A source state the Art History Brush cannot paint a Lab document from.
pub(crate) const LAB_ART_SOURCE_REFUSAL: &str =
    "The Art History Brush's source state is not a Lab Color raster";

/// The luma a Lab stroke heads along: L\* (the first code as a unit
/// sample) times alpha, premultiplied as `doc_art_history::rgb_luma` is.
pub(crate) fn lab_luma(p: [f32; 4]) -> f64 {
    f64::from(p[0]) * f64::from(p[3])
}

/// The 8-bit Lab document's Art History arm (module doc).
pub(crate) const LAB_ART: U16Art = U16Art {
    target_refusal: LAB_ART_TARGET_REFUSAL,
    source_refusal: LAB_ART_SOURCE_REFUSAL,
    source_mode: PrecisionDocument::is_lab,
    blend_refusal: crate::lab_document::blend_refusal,
    luma: lab_luma,
    kind: crate::lab_blend::lab_blend_kind8,
};

/// Red Eye's two pipelines, [twin → ProPhoto RGB, ProPhoto RGB → twin]
/// (the colour adjustments' route, `adjust_precision::color_pipelines`).
fn red_eye_pipelines() -> Result<Box<[ColorPipeline; 2]>, String> {
    color_pipelines(&lab_twin(), &IccProfile::prophoto_rgb())
}

/// Red Eye's scratch on a Lab layer beyond the RGB plan: the ProPhoto work
/// copy of the `output` destination pixels (8 B a pixel, u16 RGBA) and both
/// pipelines' prepared tables.
pub(crate) fn red_eye_scratch_bytes(output: u64) -> Result<u64, String> {
    let pipelines = red_eye_pipelines()?;
    Ok(output
        .saturating_mul(8)
        .saturating_add(pipelines[0].scratch_bytes())
        .saturating_add(pipelines[1].scratch_bytes()))
}

/// One unit sample as its nearest u16 code, clamped to 0…1.
fn code(v: f32) -> u16 {
    (v.clamp(0., 1.) * 65535.).round() as u16
}

/// `pixel`'s colour through `pipeline` as u16 codes, alpha kept.
fn converted(pipeline: &ColorPipeline, pixel: [u16; 4]) -> [u16; 4] {
    let out = pipeline.color([pixel[0], pixel[1], pixel[2]].map(|v| f32::from(v) / 65535.));
    [code(out[0]), code(out[1]), code(out[2]), pixel[3]]
}

/// Red Eye on Lab codes (module doc): `precision_redeye::apply`'s
/// parameters, run on the plane's ProPhoto RGB work copy; only the pixels
/// whose work codes it changed are converted back (the forward conversion
/// recomputed to compare, as `float_retouch` decodes only what changed).
#[allow(clippy::too_many_arguments)]
pub(crate) fn red_eye(
    canvas: (u32, u32),
    pixels: &mut [[u16; 4]],
    dimensions: (u32, u32),
    offset: (i32, i32),
    selection: &[u16],
    rect: (i32, i32, u32, u32),
    pupil: f32,
    darken: f32,
    job: JobContext<'_>,
) -> Result<(), String> {
    let pipelines = red_eye_pipelines()?;
    let [forward, back] = &*pipelines;
    let mut work: Vec<[u16; 4]> = reserved_vec(pixels.len())?;
    for (i, pixel) in pixels.iter().enumerate() {
        job.every(i)?;
        work.push(converted(forward, *pixel));
    }
    crate::precision_redeye::apply(
        canvas, &mut work, dimensions, offset, selection, rect, pupil, darken, job,
    )?;
    for (i, (pixel, result)) in pixels.iter_mut().zip(&work).enumerate() {
        job.every(i)?;
        if converted(forward, *pixel) != *result {
            *pixel = converted(back, *result);
        }
    }
    Ok(())
}
