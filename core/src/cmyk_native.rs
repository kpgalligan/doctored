//! The CMYK document in the typed `.rz` (#1: phase 1 DRDC 82, phase 2
//! DRDC 83, phase 4a DRDC 84, the channels batch DRDC 85, the styles batch
//! DRDC 86, the artwork batch DRDC 87; stacked styles DRDC 88).
//!
//! The CMYK word is the FIRST word inside the identity wrapper 46, written
//! iff the document is CMYK (`46 -> 82|83|84|85|86|87|88 -> 79 -> ...`). The
//! format words that follow the rung are the RGB format's own: a CMYK file
//! carries 114 (`clipping_path_native`, a clipping path; the next free word
//! is 115), 113 (`background_native`, a Background mark), 112 (`spot_channel_native`, a spot channel, #1), 111 (`gradient_map_method_native`, a Gradient Map with a `method`),
//! 105 (`fill_align_native`, a Gradient Fill aligned with the layer), 104
//! (`gradient_midpoint_native`, a gradient stop midpoint other
//! than 50 %), 103 (`gradient_smoothness_native`, a gradient Smoothness below
//! 100 %), 102 (`large_description_native`, an artwork description over
//! 64 KiB), 101 (`blend_if_channels_native`, a multichannel Blend If — Gray
//! plus the plates — in `LBIF` schema 2), 100 (`wide_color_native`, a style
//! colour's wide colour, whose slot still prints its ink or `color`), 99 (`glow_gradient_native`, a glow's gradient and Jitter, whose
//! stops convert to ink at composite time), 98 (`texture_native`, a Bevel &
//! Emboss texture, read as height, never inked), 97 (`noise_native`, a layer style Noise), 96 (`contour_native`,
//! a layer style contour, Anti-aliased or Range), 95 (`stroke_pattern_native`, a stroke carrying a pattern fill,
//! whose tile converts to ink at composite time), 94
//! (`gradient_options_native`, a Shape Burst or dithered gradient), 93 (`pattern_angle_native`, an angled pattern overlay), 92
//! (`generated_native`, live Generative Layer variations: its `GVAR`
//! table, with `ink` 1, after PATH and before `LREF`), 91
//! (`reference_native`, its `LREF` table before the CMYK one) and 90
//! (`style_pattern`, a pattern overlay, whose tile converts to ink at
//! composite time) after its rung, in that order, so there is no CMYK rung
//! for any. The words are a ladder, each admitting everything below it:
//! [`STACKED_VERSION`] 88 when the document [`needs_stacked`] (a stacked
//! style — two or more instances of one effect kind, `style_instances` — on
//! a record or in a comp's recorded appearance; the instances ride in the
//! ordinary `LSTY` JSON, so 88 adds no table and admits everything 87 does;
//! under 82 to 87 such a style refuses ([`STACKED_REFUSAL`]) and an 88
//! holding none refuses as "invalid native CMYK stacked effects word", both
//! checked over the records and the parsed comps before any pixel decodes;
//! the RGB format's word 89 is never written beside it), else
//! [`DESCRIPTION_VERSION`] 87 when the document [`needs_descriptions`] (a
//! type, shape or fill description on any record of the tree), else
//! [`EFFECTS_VERSION`] 86 when any record anywhere in the tree
//! [`needs_effects`] (an enabled layer effect, knockout other than None,
//! Blend Interior Effects as Group — the style rides the ordinary `LSTY`
//! table with its inks — or a Blend If, the ordinary `LBIF` table with
//! channel codes 0 (Gray) and 4–7 (Cyan, Magenta, Yellow, Black)), else [`CHANNEL_VERSION`] 85 when the document [`needs_channels`] (an alpha
//! channel, or a live vector mask on any record of the tree, groups
//! included), else [`STRUCTURE_VERSION`] 84 when any record anywhere in the
//! tree [`needs_structure`] (a group, a clipped record, a blend mode other
//! than Normal, or a layer style — Fill Opacity), else [`EDIT_VERSION`] 83
//! when any record holds a layer mask or is a CMYK adjustment record, else
//! [`PRECISION_VERSION`] 82, so a document of an earlier phase writes exactly
//! as it did (paths alone stay pre-85: the PATH table and its word 75 are the
//! RGB format's). Under 87 the descriptions are the ordinary typed `ADSC`
//! table (`artwork_native`, the body written as version 20 or later as for
//! RGB) on CMYK raster records; 87 admits everything 86 does, under 82 to 86
//! an `ADSC` entry refuses ([`DESCRIPTION_REFUSAL`]) and an 87 holding none
//! refuses as an invalid word. Under 85 the channels are the body's ordinary typed channel
//! list (with its metadata table) and the vector masks the PATH table's mask
//! entries, adopted as the RGB reader adopts them; 85 admits everything 84
//! does, and under 82 to 84 a channel or a vector-mask entry refuses
//! ([`CHANNEL_REFUSAL`]) while an 85 holding neither refuses as an invalid
//! word. Under 84 groups are the ordinary record kinds 1 and 2 and the
//! modes, clipping and styles the ordinary RGB tables (`LBLD`, the clip
//! table, `LSTY`) with their own words; 84 admits everything 83 does. Under either the body header's storage tag is 3 (straight CMYKA16)
//! and each raster record's payload is u32 width, u32 height, u64 length =
//! `w · h · 10`, then raw little-endian u16 C, M, Y, K, A in row order — no
//! PNG. A mask is the ordinary per-record mask payload; a CMYK adjustment
//! record is record kind 3 with its description verbatim
//! (`cmyk_adjust_layer`), which never carries the table-less description
//! words 62, 64 or 71 (`levels_native::precision_any` answers false for
//! it). Under 82 a mask or a kind-3 record refuses; under 82 or 83 a group
//! record (kind 1 or 2), a clipped record, a mode or a style refuses
//! ("a CMYK layer group, clipping mask, blend mode or fill opacity requires
//! DRDC 84"), and an 84 holding none of them refuses as an invalid word.
//! 86 admits everything 85 does; under 82 to 85 a record that needs 86
//! refuses ([`EFFECTS_REFUSAL`]), and an 86 holding none refuses as "invalid
//! native CMYK effects word" — both claims checked over every record before
//! any pixel decodes. The `CMYK` table — u32
//! schema 1, u8 declared depth (8 or 16), u32 length plus the CMYK ICC bytes
//! verbatim — is written last before `LIDS`. Storage tag 3 is admitted only
//! under 82 to 88, and those only with tag 3, so a pre-82 reader (which
//! knows none) and a pre-83, pre-84, pre-85, pre-86, pre-87 or pre-88 reader meeting a newer word refuse the
//! file rather than misreading it, and every prior version still reads exactly as
//! before. The body's own profile slot holds the document's view profile
//! (built-in Display P3), never the CMYK profile.

use crate::cmyk_document::{CmykDepth, CmykPolicy};
use crate::cmyk_image::{CmykImage, CmykSample, CMYK_BYTES_PER_PIXEL};
use crate::drdc::{Reader, MAX_DRDC_BLOB_LEN};
use crate::job_control::JobContext;
use crate::native_stream::{NativeSink, Payload};
use crate::precision_document::PrecisionDocumentLayer;
use crate::precision_image::pixel_count;
use crate::precision_native::{append, MAX_DECODED_BYTES};

/// The CMYK reopen's charge a raster pixel beside the decoded tiles and the
/// held header, which `precision_native_plan::reopen_scratch` adds for a
/// CMYK document: the row read and the reopened document's display
/// preparation the host runs right after (640 MB measured at 24 MP against
/// the float figure's 9 B/px, 462 MB); a miss raises this term. Raised from
/// 24 to 25 by the channels batch: `native-reopen-85` (two alpha channels
/// and a vector mask at 24 MP) measured E 776,421,976 against a plan of
/// 957,315,588 (1.233×).
pub(crate) const REOPEN_MARGIN_PER_PIXEL: u64 = 25;

/// The CMYK document word.
pub(crate) const PRECISION_VERSION: u32 = 82;

/// The CMYK document word of a document holding a mask or a CMYK
/// adjustment record (module doc).
pub(crate) const EDIT_VERSION: u32 = 83;

/// The CMYK document word of a document holding a group, a clipping mask,
/// a blend mode or a layer style anywhere (module doc).
pub(crate) const STRUCTURE_VERSION: u32 = 84;

/// The structure refusal of a pre-84 word (module doc).
pub(crate) const STRUCTURE_REFUSAL: &str =
    "a CMYK layer group, clipping mask, blend mode or fill opacity requires DRDC 84";

/// The CMYK document word of a document holding an alpha channel or a
/// vector mask (module doc).
pub(crate) const CHANNEL_VERSION: u32 = 85;

/// The channel refusal of a pre-85 word (module doc).
pub(crate) const CHANNEL_REFUSAL: &str = "a CMYK alpha channel or vector mask requires DRDC 85";

/// The CMYK document word of a document holding a type, shape or fill
/// description on any record (module doc).
pub(crate) const DESCRIPTION_VERSION: u32 = 87;

/// The description refusal of a pre-87 word (module doc).
pub(crate) const DESCRIPTION_REFUSAL: &str = "a CMYK text, shape or fill layer requires DRDC 87";

/// The CMYK document word of a document holding a stacked style on a record
/// or in a comp (module doc).
pub(crate) const STACKED_VERSION: u32 = 88;

/// The stacked refusal of a pre-88 word (module doc).
pub(crate) const STACKED_REFUSAL: &str = "a CMYK stacked layer effect requires DRDC 88";

/// The invalid-word sentence of an 88 holding no stacked style.
pub(crate) const STACKED_INVALID: &str = "invalid native CMYK stacked effects word";

/// True for a document that needs word 88 (module doc): a stacked style on
/// any record of the tree or in any comp's recorded appearance
/// (`style_instances::styles_stacked16`).
pub(crate) fn needs_stacked(document: &crate::precision_document::PrecisionDocument) -> bool {
    crate::style_instances::styles_stacked16(document)
}

/// True for a document that needs word 87 (module doc): a description on any
/// record of the tree.
pub(crate) fn needs_descriptions(document: &crate::precision_document::PrecisionDocument) -> bool {
    any_record(document.root_layers(), &|layer| layer.description.is_some())
}

/// True for a document that needs word 85 (module doc): an alpha channel, or
/// a live vector mask on any record (`vector_mask_table` walks the whole
/// tree, groups included).
pub(crate) fn needs_channels(document: &crate::precision_document::PrecisionDocument) -> bool {
    !document.channels().is_empty() || !document.vector_mask_table().is_empty()
}

/// The CMYK document word of a document holding a layer effect, knockout or
/// Blend Interior anywhere (module doc).
pub(crate) const EFFECTS_VERSION: u32 = 86;

/// The effects refusal of a pre-86 word (module doc).
pub(crate) const EFFECTS_REFUSAL: &str = "a CMYK layer style requires DRDC 86";

/// True for a record that needs word 86 (module doc): an enabled effect,
/// knockout other than None, Blend Interior Effects as Group, or a Blend If.
pub(crate) fn needs_effects(layer: &PrecisionDocumentLayer) -> bool {
    layer.blend_if().is_some()
        || layer.style().is_some_and(|style| {
            style.has_enabled_effects()
                || style.knockout != crate::style::Knockout::None
                || style.blend_interior_as_group
        })
}

/// True for a record that needs word 84 (module doc): a group, a clipped
/// record, a mode other than Normal, or a style.
pub(crate) fn needs_structure(layer: &PrecisionDocumentLayer) -> bool {
    layer.children().is_some()
        || layer.clipped()
        || layer.blend_mode != crate::blend::BlendMode::Normal
        || layer.style().is_some()
}

/// Whether any record of `layers`, at any depth, answers `test`.
fn any_record(
    layers: &[PrecisionDocumentLayer],
    test: &dyn Fn(&PrecisionDocumentLayer) -> bool,
) -> bool {
    layers
        .iter()
        .any(|layer| test(layer) || layer.children().is_some_and(|c| any_record(c, test)))
}

/// The word a CMYK document writes (module doc), every record walked.
pub(crate) fn version(document: &crate::precision_document::PrecisionDocument) -> u32 {
    if needs_stacked(document) {
        return STACKED_VERSION;
    }
    if needs_descriptions(document) {
        return DESCRIPTION_VERSION;
    }
    let layers = document.root_layers();
    if any_record(layers, &needs_effects) {
        return EFFECTS_VERSION;
    }
    if needs_channels(document) {
        return CHANNEL_VERSION;
    }
    if any_record(layers, &needs_structure) {
        return STRUCTURE_VERSION;
    }
    let edited = any_record(layers, &|layer| {
        layer.mask().is_some()
            || matches!(
                layer.content,
                crate::precision_document::Content::CmykAdjustment(_)
            )
    });
    if edited {
        EDIT_VERSION
    } else {
        PRECISION_VERSION
    }
}

/// The body's storage tag under word 82.
pub(crate) const STORAGE_TAG_CMYK: u32 = 3;

/// The word plus the table: 4 + `"CMYK"` 4 + schema 4 + depth 1 + length 4
/// + the profile; 0 for an RGB document.
pub(crate) fn header_bytes(policy: Option<&CmykPolicy>) -> u64 {
    policy.map_or(0, |p| 17 + p.profile_bytes().len() as u64)
}

/// One CMYK raster's raw payload, `w · h · 10` bytes, counted against the
/// typed format's decoded cap.
pub(crate) fn raster_capacity(dimensions: (u32, u32), total: &mut u64) -> Result<u64, String> {
    let bytes = pixel_count(dimensions.0, dimensions.1)? as u64 * CMYK_BYTES_PER_PIXEL;
    *total = total
        .checked_add(bytes)
        .ok_or("CMYK native raster byte overflow")?;
    if *total > MAX_DECODED_BYTES {
        return Err(
            "high-depth native serialized pixels exceed the shared byte limit; remove layers or reduce dimensions"
                .into(),
        );
    }
    Ok(bytes)
}

/// Writes one CMYK raster record's payload (module doc), a row at a time.
pub(crate) fn put_raster(
    out: &mut dyn NativeSink,
    image: &CmykImage,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    let (width, height) = image.dimensions();
    for value in [width, height] {
        append(out, &value.to_le_bytes(), bound, job)?;
    }
    let length = u64::from(width) * u64::from(height) * CMYK_BYTES_PER_PIXEL;
    append(out, &length.to_le_bytes(), bound, job)?;
    let mut row = Vec::with_capacity(width as usize * 10);
    for y in 0..height {
        row.clear();
        for (_, run) in image.row_segments(y, 0, width) {
            for sample in run {
                for value in sample {
                    row.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        append(out, &row, bound, job)?;
    }
    Ok(())
}

/// Takes one record's payload span after its dimensions.
pub(crate) fn take_raster<'a>(
    reader: &mut Reader<'a>,
    dimensions: (u32, u32),
    decoded: &mut u64,
) -> Result<Payload<'a>, String> {
    let bytes = raster_capacity(dimensions, decoded)?;
    if reader.u64()? != bytes {
        return Err("CMYK native raster byte length differs from its dimensions".into());
    }
    reader.take_payload(bytes)
}

/// Decodes a payload row by row straight into tiles (a file span is read a
/// row at a time).
pub(crate) fn decode_raster(
    data: &Payload<'_>,
    dimensions: (u32, u32),
    job: JobContext<'_>,
) -> Result<CmykImage, String> {
    let (width, height) = dimensions;
    let count = pixel_count(width, height)? as u64;
    if data.len() != count * CMYK_BYTES_PER_PIXEL {
        return Err("CMYK native raster length does not match its dimensions".into());
    }
    let stride = width as usize * 10;
    let mut block = vec![0u8; stride];
    let decode = |row: &mut [CmykSample], bytes: &[u8]| {
        for (pixel, chunk) in row.iter_mut().zip(bytes.chunks_exact(10)) {
            *pixel = std::array::from_fn(|c| u16::from_le_bytes([chunk[2 * c], chunk[2 * c + 1]]));
        }
    };
    CmykImage::from_rows_with_job(
        width,
        height,
        |y, row| {
            job.check()?;
            let at = y as usize * stride;
            match data {
                Payload::Bytes(bytes) => decode(row, &bytes[at..at + stride]),
                Payload::File { .. } => {
                    data.read_at(at as u64, &mut block)?;
                    decode(row, &block);
                }
            }
            Ok(())
        },
        job,
    )
}

/// Writes the `CMYK` table for a CMYK document; nothing otherwise.
pub(crate) fn encode(
    policy: Option<&CmykPolicy>,
    out: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    let Some(policy) = policy else {
        return Ok(());
    };
    append(out, b"CMYK", bound, job)?;
    append(out, &1_u32.to_le_bytes(), bound, job)?;
    append(out, &[policy.depth().bits() as u8], bound, job)?;
    let bytes = policy.profile_bytes();
    append(out, &(bytes.len() as u32).to_le_bytes(), bound, job)?;
    append(out, bytes, bound, job)
}

/// Reads the table into a policy. A schema, depth or profile this build does
/// not admit refuses before any pixel decodes.
pub(crate) fn parse(reader: &mut Reader<'_>) -> Result<CmykPolicy, String> {
    if reader.take(4)? != b"CMYK" || reader.u32()? != 1 {
        return Err("unsupported native CMYK schema".into());
    }
    let depth = CmykDepth::from_bits(u32::from(reader.u8()?)).ok_or("invalid native CMYK depth")?;
    let length = reader.u32()?;
    if length > MAX_DRDC_BLOB_LEN {
        return Err("native CMYK profile exceeds its byte limit".into());
    }
    CmykPolicy::new(reader.take(length as usize)?, depth)
}
