//! The Grayscale document's filters and retouch tools (#547 phase 2c,
//! `designs/grayscale-mode.md`): the filter gate, the one-channel
//! fold, the Sponge's middle gray and the three retouch refusals.
//!
//! # No one-channel kernel
//!
//! A Gray document's storage is R = G = B (`gray_document`'s module doc), so
//! every filter and retouch tool runs through its RGB kernel unchanged, and a
//! kernel that keeps R = G = B gives its RGB twin's result bit for bit. A
//! kernel that makes colour — Lens Correction's fringes, the Develop
//! Filter's Temperature and Tint, Depth Blur's Warmth — has its result folded
//! back to one channel by the float plane filters' rule
//! (`float_plane_filter`'s module doc, "Result"; [`fold`]): the value itself
//! when the three agree bit for bit, else the Rec. 709 luma. Add Noise is the
//! one exception: its three channels are independent draws, whose luma would
//! carry about 0.75 of the Amount, so a Gray document takes channel 0's draw
//! and its noise has the Amount's full amplitude (the RNG consumes exactly
//! what it does on RGB). The fold runs on the filter's sample before the
//! coverage mix, in `selected_adjustment` (8 and 16 bits) and
//! `float_filter_apply` (32 bits), and allocates nothing.
//!
//! A colour the REQUEST names — Clouds' foreground and background — is
//! greyed before the kernel by Image ▸ Mode ▸ Grayscale's rule
//! (`gray_adjust::grey_hex`), as paint inks and Gradient Map stops are.
//! Liquify runs through its RGB kernel since phase 4.
//!
//! # Retouch
//!
//! Dodge, Burn, Blur, Sharpen, Smudge, the History Brush, Clone Stamp,
//! Healing, Spot Healing and Patch keep R = G = B through their RGB kernels
//! (`gray_document::admit_edit` admits them). The Sponge moves gray levels
//! around the middle gray ([`sponge_pivot`], `retouch_brush::Vibrancy::Gray`).
//! The History Brush paints a Gray document from a Gray state only
//! ([`HISTORY_SOURCE_REFUSAL`]), a Gray document clones from a Gray source
//! only ([`CLONE_SOURCE_REFUSAL`]), and Remove Red Eye, a colour repair, is
//! offered on RGB documents only ([`RED_EYE_REFUSAL`]).

use crate::precision_document::PrecisionDocument;
use crate::selected_adjustment::SelectedEdit;

/// Remove Red Eye on a Gray document.
pub(crate) const RED_EYE_REFUSAL: &str =
    "Remove Red Eye is unavailable in a Grayscale document: Doctored offers it on RGB documents only.";
/// The History Brush on a Gray document from a state that is not Gray.
pub(crate) const HISTORY_SOURCE_REFUSAL: &str = "The History Brush paints a Grayscale document from a Grayscale state only: this state is from before Image ▸ Mode ▸ Grayscale.";
/// A clone into a Gray document from a source that is not Gray.
pub(crate) const CLONE_SOURCE_REFUSAL: &str =
    "A Grayscale document clones from a Grayscale document only: convert the source to Grayscale first.";

/// Gray filter gate, asked by `gray_adjust::admit_selected` for every op that
/// is not `adjustment`, Auto, `equalize` or `levels_channels`; no-op off
/// Gray. Liquify is admitted since phase 4 (its kernel keeps R = G = B); `clouds` /
/// `difference_clouds` → `foreground_color` / `background_color` replaced by
/// `grey_hex` (absent keys keep the black / white defaults, already gray);
/// every other op admitted (its parse refuses an unknown one).
pub(crate) fn admit_filter(doc: &PrecisionDocument, edit: &mut SelectedEdit) -> Result<(), String> {
    if !doc.is_gray() {
        return Ok(());
    }
    match edit.op.as_str() {
        "clouds" | "difference_clouds" => {
            for key in ["foreground_color", "background_color"] {
                let grey = edit
                    .params
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .and_then(|hex| crate::gray_adjust::grey_hex(doc, hex));
                if let Some(grey) = grey {
                    edit.params.insert(key.into(), grey.into());
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// One filtered sample on a Gray layer, returned `[v, v, v, sample[3]]`
/// (module doc): `v = sample[0]` when `noise` (Add Noise: channel 0's draw,
/// one full-amplitude delta, RNG consumption unchanged), else
/// `v = float_plane_filter::result_of(sample)`.
pub(crate) fn fold(sample: [f32; 4], noise: bool) -> [f32; 4] {
    let v = if noise {
        sample[0]
    } else {
        crate::float_plane_filter::result_of(sample)
    };
    [v, v, v, sample[3]]
}

/// The Sponge's middle gray: `None` off Gray; 0.5 for u16 storage (8 and 16
/// bits, unit codes); at 32 bits the linear value of code 0.5 through the
/// document's profile (the Gray profile's twin), so the pivot is the same
/// perceptual middle at every depth.
pub(crate) fn sponge_pivot(doc: &PrecisionDocument) -> Option<f32> {
    if !doc.is_gray() {
        return None;
    }
    if !doc.is_float() {
        return Some(0.5);
    }
    let model = doc.profile().model()?;
    Some(model.decode_rgb([0.5; 3])[0] as f32)
}
