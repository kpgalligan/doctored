//! Edit ▸ Assign Profile and Convert to Profile on an Indexed Color
//! document (#547 Indexed phase 3a, `designs/indexed-mode.md`). An
//! adapter, not a kernel: it names the document types.
//!
//! An Indexed document's profile is the space of its colour table, an RGB
//! profile (`indexed_document::is_rgb_profile`), so both commands take an
//! RGB profile only and refuse any other with [`PROFILE_NOT_RGB`].
//!
//! - **Assign** keeps every index, the table's codes and every sample: the
//!   RGB storage's own assign (`color_edit_with_context`), the unchanged
//!   policy re-attached. Only the table's meaning changes.
//! - **Convert** converts the colour TABLE and keeps every index: an
//!   Indexed image's pixels are indices, so Convert to
//!   Profile converts the table's entries and never re-quantizes the image.
//!   The palette, as a one-row RGBA16 strip in the old profile
//!   (`palette[i]·257` at alpha 65535), runs the shared
//!   `convert_to_profile_with` under the request's options; each entry is
//!   read back at its 8-bit code (`indexed_palette::code8`); the document is
//!   the storage assigned to the destination, re-rendered from the converted
//!   table with the same transparent index
//!   (`PrecisionDocument::retabled`, the Color Table edit's rebuild). The
//!   same profile bytes are no change, as is a pipeline that changes
//!   nothing on the strip (the RGB route answers no change there too).
//!
//! The plans: Assign is the storage's assign plan; Convert is that plus the
//! Color Table edit's re-render (`set_color_table_scratch`); the strip is
//! at most 256 pixels.

use std::sync::Arc;

use crate::doc_color_policy::{ConversionOptions, ConversionReport};
use crate::icc::IccProfile;
use crate::indexed_document::is_rgb_profile;
use crate::indexed_paint::NOT_INDEXED;
use crate::indexed_palette::{code8, Rgb};
use crate::job_control::JobContext;
use crate::metadata::{Metadata, Resolution};
use crate::precision_color::PrecisionColorPlan;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, PrecisionImage};

/// The refusal of a profile that is not RGB (module doc).
pub(crate) const PROFILE_NOT_RGB: &str = "An Indexed Color document's colour table is RGB: assign or convert to an RGB profile, or change the mode with Image ▸ Mode.";

/// The plan of assigning (`convert` false) or converting to `profile`
/// (module doc).
pub(crate) fn plan(
    doc: &PrecisionDocument,
    profile: &Arc<IccProfile>,
    convert: bool,
) -> Result<PrecisionColorPlan, String> {
    doc.indexed_policy().ok_or(NOT_INDEXED)?;
    if !is_rgb_profile(profile) {
        return Err(PROFILE_NOT_RGB.into());
    }
    let mut plan = doc.without_indexed().color_plan(profile, false)?;
    if convert {
        plan.scratch_bytes = plan
            .scratch_bytes
            .saturating_add(doc.set_color_table_scratch());
    }
    Ok(plan)
}

/// Assign Profile (module doc); `Ok(None)` for the document's own profile
/// bytes.
pub(crate) fn assign(
    doc: &PrecisionDocument,
    profile: &Arc<IccProfile>,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<Option<PrecisionDocument>, String> {
    let policy = doc.indexed_policy().ok_or(NOT_INDEXED)?;
    if !is_rgb_profile(profile) {
        return Err(PROFILE_NOT_RGB.into());
    }
    let Some(assigned) = doc
        .without_indexed()
        .color_edit_with_context(profile, false, limit, other, job)?
    else {
        return Ok(None);
    };
    assigned.with_indexed(policy.clone()).map(Some)
}

/// Convert to Profile (module doc); `Ok(None)` for the document's own
/// profile bytes or a pipeline that changes nothing.
pub(crate) fn convert(
    doc: &PrecisionDocument,
    profile: &Arc<IccProfile>,
    options: &ConversionOptions,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<Option<(PrecisionDocument, ConversionReport)>, String> {
    let policy = doc.indexed_policy().ok_or(NOT_INDEXED)?;
    // The no-change return goes before the admission and the reservation, so
    // the document's own profile costs nothing (phase 3b).
    if doc.profile().bytes() == profile.bytes() {
        return Ok(None);
    }
    let p = plan(doc, profile, true)?;
    admit(p.retained_bytes, p.scratch_bytes, other, limit)?;
    job.reserve(p.scratch_bytes)?;
    // The table as a one-row strip in the document's profile.
    let strip: Vec<[u16; 4]> = policy
        .palette()
        .iter()
        .map(|c| {
            [
                u16::from(c[0]) * 257,
                u16::from(c[1]) * 257,
                u16::from(c[2]) * 257,
                65535,
            ]
        })
        .collect();
    let width = strip.len() as u32;
    let image = PrecisionImage::from_rgba16(width, 1, strip, doc.profile().clone(), None)?;
    let strip = PrecisionDocument::from_image(image, Metadata::default(), Resolution::default())?;
    let held = other.saturating_add(doc.retained_bytes());
    let Some((converted, report)) =
        strip.convert_to_profile_with(profile, options, limit, held, None)?
    else {
        return Ok(None);
    };
    job.check()?;
    let [layer] = converted.root_layers() else {
        return Err("the converted colour table is one layer".into());
    };
    let Content::Raster { image, .. } = &layer.content else {
        return Err("the converted colour table is a raster".into());
    };
    let samples = image.to_rgba16()?;
    if samples.len() != policy.palette().len() {
        return Err("the converted colour table does not match the palette".into());
    }
    let table: Vec<Rgb> = samples
        .iter()
        .map(|s| [code8(s[0]), code8(s[1]), code8(s[2])])
        .collect();
    let next = policy.with_table(table, policy.transparent())?;
    let floor = job.with_reservation_floor(p.scratch_bytes);
    let assigned = doc
        .without_indexed()
        .color_edit_with_context(profile, false, limit, other, floor.range(0.0, 0.5))?
        .ok_or("the Indexed Color storage did not take the profile")?;
    let out = assigned.retabled(next, floor.range(0.5, 1.0))?;
    Ok(Some((out, report)))
}
