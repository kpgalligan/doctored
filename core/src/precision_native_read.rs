//! Version-11/12/13 native borrowed-record validation followed by typed decoding.
//! This uses the shared bounded `native_stream::Reader` (over a caller's slice or a streamed file;
//! pixel payloads are spans read only when they decode), never the byte pixel decoder.
//!
//! Shared writer/reader capacity and complete operator/record preflight precede pixel decode. In
//! the banded reopen the reader decodes each 16-bit plane row by row straight into tiles
//! (`precision_png_rows.rs`), and the reopen decode term is
//! `precision_native_plan::reopen_scratch`: base = retained + held (the reader's small takes and
//! window, `Reader::held_bytes`; once the whole file) + the band (`native_png_band_scratch`, 16
//! rows of `8·w + 1` bytes plus 4 MiB) + bookkeeping, plus a quarter, with no cap at the 27 B/px
//! whole-plane figure (`precision_png::NATIVE_DECODE_BYTES_PER_PIXEL`, now the float figure and
//! still the 8-bit original's decode); the envelope, the reader and the asset inspector all read
//! it, its record is `precision_png_rows.rs`'s module doc, and an interlaced native plane is
//! refused (no Doctored writer emits one).
use std::sync::Arc;

use crate::doc_mask::MaskProperties;
use crate::drdc::{Reader, MAX_DRDC_BLOB_LEN, MAX_DRDC_NAME_LEN, MAX_DRDC_PNG_LEN};
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::metadata::{Metadata, Resolution};
use crate::native_stream::Payload;
use crate::precision_adjustment::{AdjustmentAdmission, PrecisionAdjustmentState};
use crate::precision_codec::{
    decode_native_png, inspect_native_payload, native_png_band_scratch, native_png_decode_scratch,
};
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, pixel_count, BOOKKEEPING_BYTES};
use crate::precision_mask::{PrecisionChannel, PrecisionMask, PrecisionPlane};
use crate::precision_native_plan::{reopen_scratch, scalar_capacity};
use crate::precision_render::{MAX_GROUP_DEPTH, MAX_RECORDS};

use super::{plane_capacity, EVALUATION_TAG, STORAGE_TAG};

enum Pixels<'a> {
    Raster {
        dimensions: (u32, u32),
        png: Payload<'a>,
    },
    Adjustment {
        description: &'a str,
        prepared: Option<PrecisionAdjustmentState>,
    },
    /// A CMYK Levels/Curves record (`cmyk_adjust_layer`), under word 83.
    CmykAdjustment {
        description: &'a str,
        prepared: Option<crate::cmyk_adjust_layer::CmykAdjustmentState>,
    },
    Group {
        children: Vec<Pending<'a>>,
        pass_through: bool,
    },
}
struct Pending<'a> {
    name: &'a str,
    opacity: f32,
    visible: bool,
    offset: (i32, i32),
    pixels: Pixels<'a>,
    mask: Option<PendingMask<'a>>,
    blend_mode: crate::blend::BlendMode,
    clipped: bool,
    blend_if: Option<crate::style::BlendIf>,
    style: Option<Arc<crate::style::LayerStyle>>,
    label: u8,
    locks: u32,
    link: u32,
    /// The group disclosure `LOPN` carries; `true` unless listed there.
    open: bool,
    /// The reference point `LREF` carries; (0, 0) unless listed there.
    reference: (i32, i32),
}

struct PendingMask<'a> {
    data: Payload<'a>,
    dimensions: (u32, u32),
    properties: MaskProperties,
    enabled: bool,
}
fn raw_plane<'a>(
    reader: &mut Reader<'a>,
    dimensions: (u32, u32),
    decoded: &mut u64,
) -> Result<Payload<'a>, String> {
    let n = scalar_capacity(dimensions, decoded)?;
    if reader.u64()? != n * 2 {
        return Err("typed native scalar byte length differs from dimensions".into());
    }
    reader.take_payload(n * 2)
}
/// The bytes of a mask or channel span converted per read: 64 KiB, so a
/// plane read from a file holds one block beside its samples.
const PLANE_BLOCK_BYTES: usize = 64 * 1024;
fn plane(
    data: &Payload<'_>,
    dimensions: (u32, u32),
    job: JobContext<'_>,
) -> Result<PrecisionPlane, String> {
    let length = data.len() as usize;
    let mut values = crate::precision_image::reserved_vec(length / 2)?;
    let mut convert = |first: usize, bytes: &[u8]| {
        for (i, bytes) in bytes.chunks_exact(2).enumerate() {
            job.every(first + i)?;
            values.push(u16::from_le_bytes([bytes[0], bytes[1]]));
        }
        Ok::<(), String>(())
    };
    match data {
        Payload::Bytes(bytes) => convert(0, bytes)?,
        Payload::File { .. } => {
            let mut block = vec![0_u8; PLANE_BLOCK_BYTES.min(length)];
            let mut at = 0;
            while at < length {
                let part = &mut block[..PLANE_BLOCK_BYTES.min(length - at)];
                data.read_at(at as u64, part)?;
                convert(at / 2, part)?;
                at += part.len();
            }
        }
    }
    job.check()?;
    PrecisionPlane::adopt(dimensions.0, dimensions.1, values)
}

fn slot<'a>(reader: &mut Reader<'a>, cap: u32, name: &str) -> Result<&'a [u8], String> {
    let length = reader.u32()?;
    if length > cap {
        return Err(format!("high-depth native {name} exceeds its byte limit"));
    }
    reader.take(length as usize)
}
fn flag(reader: &mut Reader<'_>, name: &str) -> Result<bool, String> {
    match reader.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(format!("invalid high-depth native {name} flag")),
    }
}

struct Plan {
    records: usize,
    scheduled: usize,
    decoded: u64,
    /// The largest plane's whole-plane decode figure (the float reopen figure).
    decode_scratch: u64,
    /// The largest plane's banded decode figure.
    band_scratch: u64,
    names: u64,
    version: u32,
    mask_pixels: u64,
    /// Under version 40 a raster record's payload is RAW f32 rather than a
    /// PNG (`float_native`).
    float: bool,
    /// The float raster records' summed pixel count, which the float reopen
    /// figure charges its margin over (`reopen_scratch`); a
    /// CMYK document's raw rasters are counted here too.
    float_raster_pixels: u64,
    /// Under version 82 a raster record's payload is RAW CMYKA16
    /// (`cmyk_native`).
    cmyk: bool,
    /// Version 83 (`cmyk_native::EDIT_VERSION`): the CMYK document may hold
    /// masks and CMYK adjustment records; under 82 either refuses. True
    /// under 84 as well, which admits everything 83 does.
    cmyk_edit: bool,
    /// Version 84 (`cmyk_native::STRUCTURE_VERSION`): the CMYK document may
    /// hold groups, clipping masks, blend modes and fill-only styles; under
    /// 82 or 83 any of them refuses. True under 85, 86 and 87 as well
    /// (`cmyk_native::CHANNEL_VERSION`, `EFFECTS_VERSION`,
    /// `DESCRIPTION_VERSION`), which admit everything 84 does.
    cmyk_structure: bool,
}

/// Whether any pending record of `nodes`, at any depth, answers `test` (the
/// CMYK words' claims, `cmyk_native`).
fn any_pending(nodes: &[Pending<'_>], test: &dyn Fn(&Pending<'_>) -> bool) -> bool {
    nodes.iter().any(|node| {
        test(node)
            || match &node.pixels {
                Pixels::Group { children, .. } => any_pending(children, test),
                _ => false,
            }
    })
}

fn records<'a>(
    reader: &mut Reader<'a>,
    count: usize,
    depth: usize,
    declared: usize,
    plan: &mut Plan,
    job: JobContext<'_>,
) -> Result<Vec<Pending<'a>>, String> {
    if count > MAX_RECORDS
        || depth > MAX_GROUP_DEPTH
        || count > declared.saturating_sub(plan.records)
    {
        return Err("high-depth native record count or group depth exceeds its limit".into());
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        job.check()?;
        plan.records += 1;
        if plan.records > declared {
            return Err("high-depth native record count differs from header".into());
        }
        let kind = reader.u8()?;
        let visible = flag(reader, "visibility")?;
        if reader.u16()? != 0 {
            return Err("unsupported high-depth native record flags".into());
        }
        let opacity = reader.f32()?;
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return Err("invalid high-depth native opacity".into());
        }
        let name = std::str::from_utf8(slot(reader, MAX_DRDC_NAME_LEN, "layer name")?)
            .map_err(|_| "high-depth native layer name is not UTF-8")?;
        plan.names += name.len() as u64;
        let offset = (reader.i32()?, reader.i32()?);
        let mask = if plan.version >= 13 && flag(reader, "mask presence")? {
            if plan.cmyk && !plan.cmyk_edit {
                return Err("a CMYK layer mask requires DRDC 83".into());
            }
            let enabled = flag(reader, "mask enabled")?;
            let linked = flag(reader, "mask link")?;
            if reader.u16()? != 0 {
                return Err("unsupported typed native mask flags".into());
            }
            let offset = (reader.i32()?, reader.i32()?);
            let dimensions = (reader.u32()?, reader.u32()?);
            let mut properties = MaskProperties::at(offset);
            properties.linked = linked;
            properties.density = reader.f32()?;
            properties.feather = reader.f32()?;
            if !properties.valid(dimensions) {
                return Err("invalid typed native mask properties or padded extent".into());
            }
            plan.mask_pixels += u64::from(dimensions.0) * u64::from(dimensions.1);
            if plan.mask_pixels > crate::drdc::MAX_DRDC_TOTAL_MASK_PIXELS {
                return Err("typed native mask pixel count exceeds its limit".into());
            }
            Some(PendingMask {
                data: raw_plane(reader, dimensions, &mut plan.decoded)?,
                dimensions,
                properties,
                enabled,
            })
        } else {
            None
        };
        let pixels = match kind {
            0 if plan.cmyk => {
                let dimensions = (reader.u32()?, reader.u32()?);
                let png = crate::cmyk_native::take_raster(reader, dimensions, &mut plan.decoded)?;
                plan.float_raster_pixels += u64::from(dimensions.0) * u64::from(dimensions.1);
                Pixels::Raster { dimensions, png }
            }
            0 if plan.float => {
                let dimensions = (reader.u32()?, reader.u32()?);
                let png = crate::float_native::take_raster(reader, dimensions, &mut plan.decoded)?;
                plan.float_raster_pixels += u64::from(dimensions.0) * u64::from(dimensions.1);
                Pixels::Raster { dimensions, png }
            }
            0 => {
                let dimensions = (reader.u32()?, reader.u32()?);
                plane_capacity(dimensions, &mut plan.decoded)?;
                let length = reader.u64()?;
                if length > u64::from(MAX_DRDC_PNG_LEN) {
                    return Err("high-depth native PNG exceeds its byte limit".into());
                }
                let png = reader.take_payload(length)?;
                if inspect_native_payload(&png)? != dimensions {
                    return Err("high-depth native PNG dimensions differ from the record".into());
                }
                plan.decode_scratch = plan
                    .decode_scratch
                    .max(native_png_decode_scratch(dimensions)?);
                plan.band_scratch = plan.band_scratch.max(native_png_band_scratch(dimensions)?);
                Pixels::Raster { dimensions, png }
            }
            3 if plan.cmyk => {
                if !plan.cmyk_edit {
                    return Err("a CMYK adjustment layer requires DRDC 83".into());
                }
                if offset != (0, 0) {
                    return Err(
                        "high-depth native adjustments cannot carry raster placement".into(),
                    );
                }
                let description =
                    std::str::from_utf8(slot(reader, MAX_DRDC_BLOB_LEN, "adjustment description")?)
                        .map_err(|_| "high-depth adjustment description is not UTF-8")?;
                Pixels::CmykAdjustment {
                    description,
                    prepared: None,
                }
            }
            3 if plan.version >= 12 => {
                if offset != (0, 0) {
                    return Err(
                        "high-depth native adjustments cannot carry raster placement".into(),
                    );
                }
                let description =
                    std::str::from_utf8(slot(reader, MAX_DRDC_BLOB_LEN, "adjustment description")?)
                        .map_err(|_| "high-depth adjustment description is not UTF-8")?;
                Pixels::Adjustment {
                    description,
                    prepared: None,
                }
            }
            1 | 2 => {
                if plan.cmyk && !plan.cmyk_structure {
                    return Err(crate::cmyk_native::STRUCTURE_REFUSAL.into());
                }
                if offset != (0, 0) {
                    return Err("high-depth native groups cannot carry raster placement".into());
                }
                if depth >= MAX_GROUP_DEPTH {
                    return Err("high-depth native group nesting exceeds its limit".into());
                }
                let children = reader.u32()? as usize;
                plan.scheduled = plan
                    .scheduled
                    .checked_add(children)
                    .ok_or("high-depth native record count overflow")?;
                if plan.scheduled > declared {
                    return Err(
                        "high-depth native child counts exceed the declared record total".into(),
                    );
                }
                Pixels::Group {
                    children: records(reader, children, depth + 1, declared, plan, job)?,
                    pass_through: kind == 2,
                }
            }
            _ => return Err("unsupported high-depth native record kind or group mode".into()),
        };
        out.push(Pending {
            name,
            opacity,
            visible,
            offset,
            pixels,
            mask,
            blend_mode: crate::blend::BlendMode::Normal,
            clipped: false,
            blend_if: None,
            style: None,
            label: 0,
            locks: 0,
            link: 0,
            open: true,
            reference: (0, 0),
        });
    }
    Ok(out)
}

/// The typed reader, over a caller's slice or a streamed file
/// (`native_stream`); both parse and admit identically. Words 70, 68 and 66
/// and their `SOCT`/`SOSM` tables, written until 2026-10-10 for smart
/// objects, are bounds-checked and skipped (`native_sources`' module doc):
/// every binding of an asset with contents is dropped and its record opens
/// with its stored pixels. `after_preflight` runs once the final admission
/// is reserved, before any adjustment is prepared or pixel decodes
/// (`precision_native::open_native_observed`).
pub(super) fn parse_within(
    mut reader: Reader<'_>,
    limit: u64,
    other_retained: u64,
    job: JobContext<'_>,
    after_preflight: Option<&mut dyn FnMut()>,
) -> Result<PrecisionDocument, String> {
    job.check()?;
    // Inspection retains only bounded record descriptors, small takes and
    // spans: the reader's held bytes (`native_stream`).
    admit(
        reader.held_bytes(),
        BOOKKEEPING_BYTES,
        other_retained,
        limit,
    )?;
    job.reserve(reader.held_bytes() + BOOKKEEPING_BYTES)?;
    if !crate::native_magic::is_native_magic(reader.take(4)?) {
        return Err("not a high-depth DRDC document".into());
    }
    let outermost_version = reader.u32()?;
    // Version 46 is the OUTERMOST wrapper (`identity_native`) and the only
    // one that is always written: it sits before every prior version word,
    // 44 included, and its LIDS table closes the file. A file WITHOUT it is
    // every file an earlier build wrote, and its records keep the ids
    // `materialize` mints.
    let has_ids = outermost_version == crate::identity_native::PRECISION_VERSION;
    let cmyk_word = if has_ids {
        reader.u32()?
    } else {
        outermost_version
    };
    // Version 82, 83, 84, 85, 86 or 87 is the FIRST word inside 46
    // (`cmyk_native`, #547): a CMYK document, whose body declares storage
    // tag 3 and whose CMYK table is the last one before LIDS. The words are a
    // ladder, each admitting everything below it: under 83 it may hold masks
    // and CMYK adjustment records, under 84 groups, clipping masks, blend
    // modes and fill-only styles as well, under 85 alpha channels and vector
    // masks as well, under 86 layer effects, knockout and Blend Interior as
    // well, under 87 type, shape and fill descriptions as well, under 88
    // stacked layer effects as well.
    let has_cmyk_stacked = has_ids && cmyk_word == crate::cmyk_native::STACKED_VERSION;
    let has_cmyk_descriptions =
        has_cmyk_stacked || (has_ids && cmyk_word == crate::cmyk_native::DESCRIPTION_VERSION);
    let has_cmyk_effects =
        has_cmyk_descriptions || (has_ids && cmyk_word == crate::cmyk_native::EFFECTS_VERSION);
    let has_cmyk_channels =
        has_cmyk_effects || (has_ids && cmyk_word == crate::cmyk_native::CHANNEL_VERSION);
    let has_cmyk_structure =
        has_cmyk_channels || (has_ids && cmyk_word == crate::cmyk_native::STRUCTURE_VERSION);
    let has_cmyk_edit =
        has_cmyk_structure || (has_ids && cmyk_word == crate::cmyk_native::EDIT_VERSION);
    let has_cmyk = has_cmyk_edit || (has_ids && cmyk_word == crate::cmyk_native::PRECISION_VERSION);
    // Version 106 in the same slot (`gray_native`, #547): a Grayscale
    // document, whose GRAY table is the last one before LIDS.
    let has_gray = has_ids && cmyk_word == crate::gray_native::WORD;
    // Version 109 in the same slot (`duotone_native`, #547 Duotone phase
    // 1a): a Duotone document, whose GRAY table is followed by its DUOT
    // table, the last ones before LIDS.
    let has_duotone = has_ids && cmyk_word == crate::duotone_native::WORD;
    // Version 107 in the same slot (`lab_native`, #547): a Lab Color
    // document, whose LAB table is the last one before LIDS.
    let has_lab = has_ids && cmyk_word == crate::lab_native::WORD;
    // Version 108 in the same slot (`indexed_native`, #547): an Indexed
    // Color document, whose INDX table is the last one before LIDS.
    let has_indexed = has_ids && cmyk_word == crate::indexed_native::WORD;
    // Version 110 in the same slot (`bitmap_native`, #547): a Bitmap
    // document, whose GRAY table then BTMP table are the last before LIDS.
    let has_bitmap = has_ids && cmyk_word == crate::bitmap_native::WORD;
    let spot_word = if has_cmyk || has_gray || has_duotone || has_lab || has_indexed || has_bitmap {
        reader.u32()?
    } else {
        cmyk_word
    };
    // Version 112 (`spot_channel_native`, #547) follows the CMYK slot, ahead
    // of 111: some channel is a spot channel, its SPOT table after LREF and
    // before the CMYK table. Indexed and Bitmap documents hold none.
    let has_spot = has_ids && spot_word == crate::spot_channel_native::WORD;
    if has_spot && has_indexed {
        return Err(crate::spot_channel::INDEXED_REFUSAL.into());
    }
    if has_spot && has_bitmap {
        return Err(crate::spot_channel::BITMAP_REFUSAL.into());
    }
    let method_word = if has_spot { reader.u32()? } else { spot_word };
    // Version 111 (`gradient_map_method_native`) follows the CMYK slot
    // and 112, ahead of 105, with no table: some adjustment record's description is a
    // Gradient Map with a `method`. Checked both ways with the adjustment
    // words, before any payload decodes.
    let has_method = has_ids && method_word == crate::gradient_map_method_native::WORD;
    let align_word = if has_method {
        reader.u32()?
    } else {
        method_word
    };
    // Version 105 (`fill_align_native`) follows the CMYK slot and 111,
    // ahead of 104, with no table: some record's artwork description is a Gradient
    // Fill aligned with the layer. Checked both ways right after the ADSC
    // parse, before any payload decodes.
    let has_align = has_ids && align_word == crate::fill_align_native::WORD;
    let midpoint_word = if has_align { reader.u32()? } else { align_word };
    // Version 104 (`gradient_midpoint_native`) follows the CMYK slot
    // and 105, ahead of 103, with no table: some record's or comp's style, or
    // some Gradient Fill description, holds a stop midpoint other than 50 %.
    // Checked both ways after LCMP, before any payload decodes.
    let has_midpoint = has_ids && midpoint_word == crate::gradient_midpoint_native::WORD;
    let smooth_word = if has_midpoint {
        reader.u32()?
    } else {
        midpoint_word
    };
    // Version 103 (`gradient_smoothness_native`) follows the CMYK slot,
    // 105 and 104, ahead of 102, with no table: some record's or comp's style, or
    // some Gradient Fill description, holds a Smoothness below 100 %. Checked
    // both ways after LCMP, before any payload decodes.
    let has_smooth = has_ids && smooth_word == crate::gradient_smoothness_native::WORD;
    let large_word = if has_smooth {
        reader.u32()?
    } else {
        smooth_word
    };
    // Version 102 (`large_description_native`) follows the CMYK slot,
    // ahead of 101, with no table: some record's artwork description is over
    // 64 KiB. It sets the ADSC entry cap, and its claim is checked both ways
    // when the table is read, before any payload decodes.
    let has_large = has_ids && large_word == crate::large_description_native::WORD;
    let multichannel_word = if has_large { reader.u32()? } else { large_word };
    // Version 101 (`blend_if_channels_native`) follows the
    // CMYK slot and 102, ahead of 100, with no table of its own: some record or comp
    // holds a multichannel Blend If (its records' ranges in `LBIF` schema
    // 2). Checked after LCMP, before any payload decodes.
    let has_multichannel = has_ids && multichannel_word == crate::blend_if_channels_native::WORD;
    let wide_word = if has_multichannel {
        reader.u32()?
    } else {
        multichannel_word
    };
    // Version 100 (`wide_color_native`) follows the CMYK slot and 101,
    // ahead of 99, with no table: some record or comp holds a style colour
    // slot carrying a wide colour. Checked with 99 after LCMP, before any
    // payload decodes.
    let has_wide = has_ids && wide_word == crate::wide_color_native::WORD;
    let glow_word = if has_wide { reader.u32()? } else { wide_word };
    // Version 99 (`glow_gradient_native`) follows the CMYK slot, 101
    // and 100, ahead of 98, with no table: some record or comp holds a glow carrying a
    // gradient or Jitter. Checked with 98 after LCMP, before any payload
    // decodes.
    let has_glow_gradient = has_ids && glow_word == crate::glow_gradient_native::WORD;
    let texture_word = if has_glow_gradient {
        reader.u32()?
    } else {
        glow_word
    };
    // Version 98 (`texture_native`) follows the CMYK slot and
    // 99, ahead of 97, with no table: some record or comp holds a Bevel &
    // Emboss carrying a texture. Checked with 97 after LCMP, before any
    // payload decodes.
    let has_texture = has_ids && texture_word == crate::texture_native::WORD;
    let noise_word = if has_texture {
        reader.u32()?
    } else {
        texture_word
    };
    // Version 97 (`noise_native`) follows the CMYK slot
    // and 98, ahead of 96, with no table: some record or comp holds a layer
    // style Noise above 0. Checked with 96 after LCMP, before any payload
    // decodes.
    let has_noise = has_ids && noise_word == crate::noise_native::WORD;
    let contour_word = if has_noise { reader.u32()? } else { noise_word };
    // Version 96 (`contour_native`) follows the CMYK slot
    // and 97, ahead of 95, with no table: some record or comp holds a layer
    // style contour-family key. Checked with 95 after LCMP, before any
    // payload decodes.
    let has_contour = has_ids && contour_word == crate::contour_native::WORD;
    let stroke_pattern_word = if has_contour {
        reader.u32()?
    } else {
        contour_word
    };
    // Version 95 (`stroke_pattern_native`) follows the CMYK
    // slot and 96, ahead of 94, with no table: some record or comp holds a
    // stroke carrying a pattern fill. Checked with 94 after LCMP, before any
    // payload decodes.
    let has_stroke_pattern = has_ids && stroke_pattern_word == crate::stroke_pattern_native::WORD;
    let options_word = if has_stroke_pattern {
        reader.u32()?
    } else {
        stroke_pattern_word
    };
    // Version 94 (`gradient_options_native`) follows the CMYK
    // slot and 95, ahead of 93, with no table: some record or comp holds a Shape
    // Burst stroke gradient or a dithered gradient. Checked with 93 after
    // LCMP, before any payload decodes.
    let has_options = has_ids && options_word == crate::gradient_options_native::WORD;
    let angle_word = if has_options {
        reader.u32()?
    } else {
        options_word
    };
    // Version 93 (`pattern_angle_native`) follows the CMYK
    // slot and 94, ahead of 92, with no table: some record or comp holds an angled
    // pattern overlay. Checked with 90 after LCMP, before any payload
    // decodes.
    let has_angle = has_ids && angle_word == crate::pattern_angle_native::WORD;
    let variations_word = if has_angle { reader.u32()? } else { angle_word };
    // Version 92 (`generated_native`) follows the CMYK slot and 93,
    // ahead of 91: the document (a CMYK one's too) holds Generative Layer
    // variations, its GVAR table after PATH and before LREF. Every entry is
    // checked there, before any payload decodes.
    let has_variations = has_ids && variations_word == crate::generated_native::WORD;
    let reference_word = if has_variations {
        reader.u32()?
    } else {
        variations_word
    };
    // Version 91 (`reference_native`) follows the CMYK slot, 93 and 92,
    // ahead of 90: some record moved since it was made, and its LREF table
    // follows every prior tail but CMYK's and LIDS.
    let has_reference = has_ids && reference_word == crate::reference_native::PRECISION_VERSION;
    let pattern_word = if has_reference {
        reader.u32()?
    } else {
        reference_word
    };
    // Version 90 (`style_pattern`) follows the CMYK slot and
    // word 91, ahead of 89, with no table: some record or comp holds a
    // pattern overlay. Checked with 89 after LCMP, before any payload
    // decodes.
    let has_pattern = has_ids && pattern_word == crate::style_pattern::WORD;
    let stacked_word = if has_pattern {
        reader.u32()?
    } else {
        pattern_word
    };
    // Version 89 (`style_instances`) follows the CMYK slot inside 46
    // and has no table: a NON-CMYK document holds a stacked style. The claim
    // is checked against the records and the parsed comps right after LCMP,
    // before any payload decodes.
    let has_stacked = has_ids && !has_cmyk && stacked_word == crate::style_instances::WORD;
    let locals_word = if has_stacked {
        reader.u32()?
    } else {
        stacked_word
    };
    // Version 79 is the FIRST word inside 46 (`raw_locals_native`),
    // ahead of 75, with no table: the RAW sources hold more than four masks
    // on one attachment or over 512 MiB of resources. The claim is checked
    // against the source table right after it is parsed, before any payload
    // decodes.
    let has_raw_locals = has_ids && locals_word == crate::raw_locals_native::VERSION;
    let paths_word = if has_raw_locals {
        reader.u32()?
    } else {
        locals_word
    };
    // Version 75 is the next word inside 46 (`path_native`), ahead of
    // 71: the document holds paths. Its PATH table is the last one before
    // LIDS.
    let has_paths = has_ids && paths_word == crate::path_native::PRECISION_VERSION;
    let first_word = if has_paths { reader.u32()? } else { paths_word };
    // Version 71 is the next word inside 46 (`look_native`), ahead
    // of 70, with no table: some Color Lookup description keeps an abstract
    // profile. The claim is checked beside word 62's below, before any
    // payload decodes.
    let has_looks = has_ids && first_word == crate::look_native::VERSION;
    let after_looks = if has_looks { reader.u32()? } else { first_word };
    // Version 70 is the next word inside 46 (read only), ahead of 68: some
    // record had a Stack Mode, and its SOSM table follows SOCT. It requires
    // 66 below.
    let has_stack = has_ids && after_looks == crate::native_sources::STACK_MODE_PRECISION_WORD;
    let after_stack = if has_stack {
        reader.u32()?
    } else {
        after_looks
    };
    // Version 68 is the next word inside 46 (read only), ahead of 67, with
    // no table: some smart object's contents was a 32-bit float document.
    // The claim is checked against word 66 below and against the SOCT table
    // when it is skipped.
    let has_float_contents =
        has_ids && after_stack == crate::native_sources::FLOAT_CONTENTS_PRECISION_WORD;
    let after_float_contents = if has_float_contents {
        reader.u32()?
    } else {
        after_stack
    };
    // Version 67 is the next word inside 46 (`float_native`), ahead
    // of 66, with no table: some asset is a 32-bit float master, in a float
    // or a 16-bit document. The claim is checked
    // against the source table right after it is parsed, before any payload
    // decodes.
    // Version 78 sits immediately before 67 (`float_native`), with no
    // table: some 32-bit float master carries a camera original. It requires
    // 67 behind it, and the claim is checked against the source table beside
    // 67's.
    let has_float_raw_masters =
        has_ids && after_float_contents == crate::float_native::RAW_MASTER_VERSION;
    let after_float_contents = if has_float_raw_masters {
        reader.u32()?
    } else {
        after_float_contents
    };
    let has_float_masters = has_ids && after_float_contents == crate::float_native::MASTER_VERSION;
    if has_float_raw_masters && !has_float_masters {
        return Err(crate::float_native::RAW_MASTER_WORD_REFUSAL.into());
    }
    let after_masters = if has_float_masters {
        reader.u32()?
    } else {
        after_float_contents
    };
    // Version 66 is the next word inside 46 (read only), ahead of 64: some
    // asset carried smart-object contents, and its SOCT table is the last
    // one before LIDS (and SOSM).
    let has_contents = has_ids && after_masters == crate::native_sources::CONTENTS_PRECISION_WORD;
    if (has_stack || has_float_contents) && !has_contents {
        return Err(crate::native_sources::CONTENTS_WORD_REFUSAL.into());
    }
    let after_ids_word = if has_contents {
        reader.u32()?
    } else {
        after_masters
    };
    // Version 64 is the next word inside 46 (`curves_native`),
    // ahead of 62, with no table: some curves description carries a map.
    // The claim is checked beside word 62's below, before any payload
    // decodes.
    let has_curve_maps = has_ids && after_ids_word == crate::curves_native::PRECISION_VERSION;
    let after_curve_maps = if has_curve_maps {
        reader.u32()?
    } else {
        after_ids_word
    };
    // Version 62 is the next word inside 46 (`levels_native`),
    // ahead of 60, with no table: some levels description carries an output
    // range or a per-channel stage. The claim is checked after the word-50
    // block below, before any payload decodes.
    let has_levels = has_ids && after_curve_maps == crate::levels_native::PRECISION_VERSION;
    let after_levels = if has_levels {
        reader.u32()?
    } else {
        after_curve_maps
    };
    // Version 60 is the next word inside 46 (`sampler_native`), ahead
    // of 58: the document holds colour samplers. Its CSMP table is the last
    // one before LIDS.
    let has_samplers = has_ids && after_levels == crate::sampler_native::PRECISION_VERSION;
    let after_samplers = if has_samplers {
        reader.u32()?
    } else {
        after_levels
    };
    // Version 58 is the next word inside 46 (`precision_light_native`),
    // ahead of 57: the document's global light is not the default. Its LGLT
    // table is the last one before CSMP or LIDS.
    let has_light = has_ids && after_samplers == crate::precision_light_native::PRECISION_VERSION;
    let after_ids = if has_light {
        reader.u32()?
    } else {
        after_samplers
    };
    // Version 57 is the next word inside 46 (`source_refusal_native`), ahead
    // of 55: some retained source asset carries a profile refusal. Its RSPR
    // table is the last one before LIDS.
    let has_refusals = has_ids && after_ids == crate::source_refusal_native::PRECISION_VERSION;
    let after_refusals = if has_refusals {
        reader.u32()?
    } else {
        after_ids
    };
    // Version 55 is next inside 46 (`precision_group_open_native`), ahead of
    // 54: a closed group needs neither 12E-1 invariant broken. Its LOPN table
    // follows WARP.
    let has_open =
        has_ids && after_refusals == crate::precision_group_open_native::PRECISION_VERSION;
    let after_open = if has_open {
        reader.u32()?
    } else {
        after_refusals
    };
    let has_warp = has_ids && after_open == crate::warp_native::PRECISION_VERSION;
    let warp_version = if has_warp { reader.u32()? } else { after_open };
    let has_projective =
        has_ids && warp_version == crate::source_transform_native::PRECISION_VERSION;
    let lock_version = if has_projective {
        reader.u32()?
    } else {
        warp_version
    };
    // Version 47 is the ONE word that sits INSIDE 46 rather than outside it
    // (`precision_lock_native`): 12E-1's invariants are that 46 is the first
    // word and LIDS the last table, and a LOCKS/LINKS table needs neither
    // broken. Its LLNK table lands after LCMP and before LIDS.
    let has_locks = lock_version == crate::precision_lock_native::PRECISION_VERSION;
    let guide_version = if has_locks {
        reader.u32()?
    } else {
        lock_version
    };
    // Version 48 is the SECOND word inside 46, beside 47
    // (`precision_guide_native`): guides and the ruler origin need neither
    // invariant broken either. Its LGDE table lands after LLNK and before
    // LIDS.
    let has_guides = guide_version == crate::precision_guide_native::PRECISION_VERSION;
    let knockout_version = if has_guides {
        reader.u32()?
    } else {
        guide_version
    };
    // Version 50 is the THIRD word inside 46, after 47 and 48
    // (`precision_style_knockout`), and the only one of the three with NO
    // table: the two keys already ride inside each `LSTY` record's style JSON.
    // Both of its claims are checked once and unconditionally after the
    // (optional) LSTY attach block below, so a file with NO style table is
    // checked too.
    let has_knockout = knockout_version == crate::precision_style_knockout::TYPED_VERSION;
    let comps_version = if has_knockout {
        reader.u32()?
    } else {
        knockout_version
    };
    // Version 44 is the next wrapper (`comps_native`): it sits before every
    // prior version word, 42 included, and its LCMP table closes the file
    // before LIDS.
    let has_comps = comps_version == crate::comps_native::PRECISION_VERSION;
    let label_version = if has_comps {
        reader.u32()?
    } else {
        comps_version
    };
    // Version 42 is the next wrapper (`label_native`): it sits before every
    // prior version word, 40 included, and its LLBL table closes the file
    // before LCMP.
    let has_label = label_version == crate::label_native::PRECISION_VERSION;
    let first_version = if has_label {
        reader.u32()?
    } else {
        label_version
    };
    // Version 40 is the next wrapper (`float_native`): it sits before every
    // prior version word, 39 included, and its FLOT table closes the file
    // before LLBL. A float document is linear light by construction, so the
    // word after 40 MUST be 39.
    let float = first_version == crate::float_native::PRECISION_VERSION;
    let evaluation_version = if float { reader.u32()? } else { first_version };
    if float && evaluation_version != crate::evaluation_native::PRECISION_VERSION {
        return Err("a 32-bit float native document must declare linear-light evaluation".into());
    }
    // Version 39 is the next wrapper (`evaluation_native`): it sits before
    // every prior version word and its table closes the file.
    let has_evaluation = evaluation_version == crate::evaluation_native::PRECISION_VERSION;
    let style_version = if has_evaluation {
        reader.u32()?
    } else {
        evaluation_version
    };
    if has_evaluation
        && !matches!(
            style_version,
            13 | 15
                | 17
                | 19
                | 20
                | 22
                | 24
                | 25
                | 26
                | 27
                | 29
                | 30
                | 31
                | 32
                | 33
                | 34
                | 35
                | 36
                | 37
        )
    {
        return Err("invalid native evaluation typed body version".into());
    }
    let has_style = style_version == crate::precision_style_native::PRECISION_VERSION;
    let blend_if_version = if has_style {
        reader.u32()?
    } else {
        style_version
    };
    if has_style
        && !matches!(
            blend_if_version,
            13 | 15 | 17 | 19 | 20 | 22 | 24 | 25 | 26 | 27 | 29 | 30 | 31 | 32 | 33 | 34 | 35 | 37
        )
    {
        return Err("invalid native layer style typed body version".into());
    }
    let has_blend_if = blend_if_version == crate::precision_blend_if_native::PRECISION_VERSION;
    let clip_version = if has_blend_if {
        reader.u32()?
    } else {
        blend_if_version
    };
    if has_blend_if
        && !matches!(
            clip_version,
            13 | 15 | 17 | 19 | 20 | 22 | 24 | 25 | 26 | 27 | 29 | 30 | 31 | 32 | 33 | 34 | 37
        )
    {
        return Err("invalid native blend-if typed body version".into());
    }
    let has_clipping = clip_version == crate::precision_clip_native::PRECISION_VERSION;
    let mask_version = if has_clipping {
        reader.u32()?
    } else {
        clip_version
    };
    if has_clipping
        && !matches!(
            mask_version,
            13 | 15 | 17 | 19 | 20 | 22 | 24 | 25 | 26 | 27 | 29 | 30 | 31 | 33 | 34 | 37
        )
    {
        return Err("invalid native clipping typed body version".into());
    }
    let has_outside = mask_version == crate::mask_outside_native::PRECISION_VERSION;
    let outer_version = if has_outside {
        reader.u32()?
    } else {
        mask_version
    };
    if has_outside
        && !matches!(
            outer_version,
            13 | 15 | 17 | 19 | 20 | 22 | 24 | 25 | 26 | 27 | 30 | 31 | 33 | 34 | 37
        )
    {
        return Err("invalid native mask outside typed body version".into());
    }
    let has_layer_blending = matches!(
        outer_version,
        crate::precision_blend_native::BASIC_VERSION
            | crate::precision_blend_native::SEPARABLE_VERSION
            | crate::precision_blend_native::FOUR_MODE_VERSION
            | crate::precision_blend_native::FIVE_MODE_VERSION
            | crate::precision_blend_native::HSL_VERSION
            | crate::precision_blend_native::THRESHOLD_VERSION
            | crate::precision_blend_native::ADJUSTMENT_VERSION
    );
    let header_version = if has_layer_blending {
        reader.u32()?
    } else {
        outer_version
    };
    if has_layer_blending && !matches!(header_version, 13 | 15 | 17 | 19 | 20 | 22 | 24 | 25) {
        return Err("invalid native blending body version".into());
    }
    let has_channel_metadata = header_version == crate::precision_channel_metadata::NATIVE_VERSION;
    let version = if has_channel_metadata {
        reader.u32()?
    } else {
        header_version
    };
    if has_channel_metadata && !matches!(version, 13 | 15 | 17 | 19 | 20 | 22 | 24) {
        return Err("invalid native channel metadata body version".into());
    }
    if !matches!(
        version,
        11..=13
            | crate::native_sources::PRECISION_SOURCE_VERSION
            | crate::native_sources::PRECISION_LINK_VERSION
            | crate::native_sources::PRECISION_RAW_VERSION
            | crate::artwork_document::NATIVE_VERSION
            | crate::native_sources::PRECISION_RESOURCE_VERSION
            | crate::native_sources::PRECISION_FILTER_VERSION
    ) {
        return Err(
            "not a supported version-11/12/13/15/17/19/20/22/24/25 high-depth DRDC document".into(),
        );
    }
    // Storage tag 2 is admitted ONLY under version 40, and a float document
    // may not declare tag 1: the tag and the wrapper are one statement.
    if float && has_cmyk {
        return Err("a native document cannot be both 32-bit float and CMYK".into());
    }
    let expected_storage = if float {
        crate::float_native::STORAGE_TAG_FLOAT
    } else if has_cmyk {
        crate::cmyk_native::STORAGE_TAG_CMYK
    } else {
        STORAGE_TAG
    };
    if reader.u32()? != expected_storage {
        return Err("unsupported high-depth native storage tag".into());
    }
    if reader.u32()? != EVALUATION_TAG {
        return Err("unsupported high-depth native evaluation tag".into());
    }
    let dimensions = (reader.u32()?, reader.u32()?);
    pixel_count(dimensions.0, dimensions.1)?;
    let roots = reader.u32()? as usize;
    let declared = reader.u32()? as usize;
    if roots == 0 || declared == 0 || roots > declared || declared > MAX_RECORDS {
        return Err("invalid high-depth native root or total record count".into());
    }
    let profile_bytes = slot(&mut reader, MAX_DRDC_BLOB_LEN, "profile")?;
    let resolution = Resolution {
        x: reader.f32()?,
        y: reader.f32()?,
    };
    if resolution != resolution.sane() {
        return Err("invalid high-depth native resolution".into());
    }
    let mut metadata: [Option<&[u8]>; 3] = [None; 3];
    for entry in &mut metadata {
        if flag(&mut reader, "metadata presence")? {
            *entry = Some(slot(&mut reader, MAX_DRDC_BLOB_LEN, "metadata")?);
        }
    }
    let mut plan = Plan {
        records: 0,
        scheduled: roots,
        decoded: 0,
        decode_scratch: 0,
        band_scratch: 0,
        names: 0,
        version,
        mask_pixels: 0,
        float,
        float_raster_pixels: 0,
        cmyk: has_cmyk,
        cmyk_edit: has_cmyk_edit,
        cmyk_structure: has_cmyk_structure,
    };
    let mut pending = records(&mut reader, roots, 0, declared, &mut plan, job)?;
    let mut channels = Vec::new();
    if version >= 13 {
        let count = reader.u32()? as usize;
        if count > crate::doc_channel::MAX_CHANNELS
            || u64::from(dimensions.0) * u64::from(dimensions.1) * count as u64
                > crate::drdc::MAX_DRDC_TOTAL_CHANNEL_PIXELS
        {
            return Err("typed native channel count exceeds its limit".into());
        }
        for _ in 0..count {
            job.check()?;
            let name = std::str::from_utf8(slot(&mut reader, MAX_DRDC_NAME_LEN, "channel name")?)
                .map_err(|_| "typed native channel name is not UTF-8")?;
            plan.names += name.len() as u64;
            channels.push((name, raw_plane(&mut reader, dimensions, &mut plan.decoded)?));
        }
    }
    let source_version = if version == crate::artwork_document::NATIVE_VERSION {
        let v = reader.u32()?;
        if !matches!(v, 13 | 15 | 17 | 19) {
            return Err("invalid typed artwork embedded-source schema".into());
        }
        v
    } else {
        version
    };
    let mut sources = if matches!(
        source_version,
        crate::native_sources::PRECISION_SOURCE_VERSION
            | crate::native_sources::PRECISION_LINK_VERSION
            | crate::native_sources::PRECISION_RAW_VERSION
            | crate::native_sources::PRECISION_RESOURCE_VERSION
            | crate::native_sources::PRECISION_FILTER_VERSION
    ) {
        crate::native_sources::PendingSources::parse(
            &mut reader,
            declared,
            source_version,
            has_float_masters,
            has_float_raw_masters,
        )?
    } else {
        crate::native_sources::PendingSources::default()
    };
    // Word 67 claims a float master the source table must hold (`float_native`).
    if has_float_masters && sources.float_master_pixels() == 0 {
        return Err(crate::float_native::MASTER_WORD_REFUSAL.into());
    }
    // Word 78 claims one of them carries a camera original.
    if has_float_raw_masters && !sources.float_raw_master_present() {
        return Err(crate::float_native::RAW_MASTER_WORD_REFUSAL.into());
    }
    // Word 79's claim, both directions, before any payload decodes.
    let (pool_bytes, most_resources) = sources.raw_locals_claim();
    let claimed = crate::raw_locals_native::claim(pool_bytes, most_resources);
    if has_raw_locals && !claimed {
        return Err(crate::raw_locals_native::INVALID_WORD.into());
    }
    if claimed && !has_raw_locals {
        return Err(crate::raw_locals_native::MISSING_WORD.into());
    }
    let descriptions = if matches!(
        version,
        crate::artwork_document::NATIVE_VERSION
            | crate::native_sources::PRECISION_RESOURCE_VERSION
            | crate::native_sources::PRECISION_FILTER_VERSION
    ) {
        // Word 102 (`large_description_native`) raises the entry cap to the
        // meta cap; without it an entry over 64 KiB keeps the size refusal.
        let cap = if has_large {
            crate::artwork_document::MAX_LARGE_DESCRIPTION
        } else {
            crate::artwork_document::MAX_DESCRIPTION
        };
        crate::artwork_native::parse(&mut reader, declared, cap)?
    } else {
        Vec::new()
    };
    // Word 102's claim, both directions: an entry over 64 KiB without the
    // word was refused by the cap above; the word without one is refused here.
    if has_large
        && !descriptions
            .iter()
            .any(|(_, text)| crate::large_description_native::is_large(text))
    {
        return Err(crate::large_description_native::INVALID_WORD.into());
    }
    // Word 105's claim, both directions (`fill_align_native`), over the
    // parsed descriptions before any payload decodes.
    let aligned = descriptions
        .iter()
        .any(|(_, text)| crate::fill_align_native::describes_align(text));
    if aligned && !has_align {
        return Err(crate::fill_align_native::WORD_REFUSAL.into());
    }
    if has_align && !aligned {
        return Err(crate::fill_align_native::INVALID_WORD.into());
    }
    fn raster_indices(nodes: &[Pending<'_>], out: &mut Vec<bool>) {
        for node in nodes {
            if let Pixels::Group { children, .. } = &node.pixels {
                raster_indices(children, out);
            }
            out.push(matches!(node.pixels, Pixels::Raster { .. }));
        }
    }
    let mut raster_flags = Vec::with_capacity(declared);
    raster_indices(&pending, &mut raster_flags);
    for &(index, _) in &descriptions {
        if !raster_flags.get(index).copied().unwrap_or(false)
            || sources.bindings.iter().any(|b| b.layer == index)
        {
            return Err("artwork description requires an ordinary raster fallback".into());
        }
    }
    // Word 87's claim, both directions (`cmyk_native`): a pre-87 CMYK word
    // refuses a description, and an 87 must hold one — before any payload
    // decodes.
    if has_cmyk {
        if !descriptions.is_empty() && !has_cmyk_descriptions {
            return Err(crate::cmyk_native::DESCRIPTION_REFUSAL.into());
        }
        // 88 admits 87 and need not hold a description.
        if cmyk_word == crate::cmyk_native::DESCRIPTION_VERSION && descriptions.is_empty() {
            return Err("invalid native CMYK description word".into());
        }
    }
    plan.names += descriptions
        .iter()
        .map(|(_, s)| s.len() as u64)
        .sum::<u64>();
    let channel_metadata = if has_channel_metadata {
        crate::precision_channel_metadata::parse(&mut reader, channels.len(), job)?
    } else {
        Vec::new()
    };
    plan.names += channel_metadata
        .iter()
        .map(|(_, text)| text.len() as u64 * 4)
        .sum::<u64>();
    if has_layer_blending {
        let modes =
            crate::precision_blend_native::parse(&mut reader, declared, outer_version, job)?;
        fn attach_modes(
            nodes: &mut [Pending<'_>],
            modes: &[(usize, crate::blend::BlendMode)],
            index: &mut usize,
            outer: u32,
        ) -> Result<(), String> {
            for node in nodes {
                if let Pixels::Group { children, .. } = &mut node.pixels {
                    attach_modes(children, modes, index, outer)?;
                }
                if let Some((_, mode)) = modes.iter().find(|(i, _)| *i == *index) {
                    // Schema 7 (v37) is the ONE version that admits a kind-3
                    // adjustment target; schemas 1-6 keep their frozen meaning.
                    let admitted = matches!(
                        node.pixels,
                        Pixels::Raster { .. }
                            | Pixels::Group {
                                pass_through: false,
                                ..
                            }
                    ) || (matches!(
                        node.pixels,
                        Pixels::Adjustment { .. } | Pixels::CmykAdjustment { .. }
                    ) && outer
                        == crate::precision_blend_native::ADJUSTMENT_VERSION);
                    if !admitted {
                        return Err("native blend requires a raster or isolated group".into());
                    }
                    node.blend_mode = *mode;
                }
                *index += 1;
            }
            Ok(())
        }
        attach_modes(&mut pending, &modes, &mut 0, outer_version)?;
    }
    if has_outside {
        let indices = crate::mask_outside_native::parse(&mut reader, declared, job)?;
        fn attach_outside(
            nodes: &mut [Pending<'_>],
            indices: &[usize],
            index: &mut usize,
        ) -> Result<(), String> {
            for node in nodes {
                if let Pixels::Group { children, .. } = &mut node.pixels {
                    attach_outside(children, indices, index)?;
                }
                if indices.binary_search(index).is_ok() {
                    node.mask
                        .as_mut()
                        .ok_or("native mask outside reference has no mask")?
                        .properties
                        .outside_coverage = 255;
                }
                *index += 1;
            }
            Ok(())
        }
        attach_outside(&mut pending, &indices, &mut 0)?;
    }
    if has_clipping {
        let indices = crate::precision_clip_native::parse(&mut reader, declared, job)?;
        fn attach_clipped(nodes: &mut [Pending<'_>], indices: &[usize], index: &mut usize) {
            for node in nodes {
                if let Pixels::Group { children, .. } = &mut node.pixels {
                    attach_clipped(children, indices, index);
                }
                node.clipped = indices.binary_search(index).is_ok();
                *index += 1;
            }
        }
        attach_clipped(&mut pending, &indices, &mut 0);
    }
    if has_blend_if {
        let entries =
            crate::precision_blend_if_native::parse(&mut reader, declared, has_multichannel, job)?;
        // A CMYK plate needs a CMYK document (#547): a non-CMYK file carrying
        // one in any range refuses before any pixel decodes.
        if !has_cmyk
            && entries
                .iter()
                .any(|(_, b)| b.ranges().any(|r| r.channel.is_cmyk()))
        {
            return Err("invalid native blend-if channel".into());
        }
        fn attach_blend_if(
            nodes: &mut [Pending<'_>],
            entries: &[(usize, crate::style::BlendIf)],
            index: &mut usize,
        ) -> Result<(), String> {
            for node in nodes {
                if let Pixels::Group { children, .. } = &mut node.pixels {
                    attach_blend_if(children, entries, index)?;
                }
                if let Some((_, blend_if)) = entries.iter().find(|(i, _)| *i == *index) {
                    // Every record kind carries one: an RGB or CMYK
                    // adjustment and a Pass Through group (it then
                    // composites isolated).
                    node.blend_if = Some(*blend_if);
                }
                *index += 1;
            }
            Ok(())
        }
        attach_blend_if(&mut pending, &entries, &mut 0)?;
    }
    if has_style {
        let entries = crate::precision_style_native::parse(&mut reader, declared, job)?;
        // The plane gate runs HERE, on the record's declared dimensions,
        // before any PNG decodes: a raster's own size, an adjustment's (a
        // CMYK one included) or a group's (a Pass Through one included) the
        // canvas.
        fn attach_style(
            nodes: &mut [Pending<'_>],
            entries: &[(usize, Arc<crate::style::LayerStyle>)],
            canvas: (u32, u32),
            index: &mut usize,
        ) -> Result<(), String> {
            for node in nodes {
                if let Pixels::Group { children, .. } = &mut node.pixels {
                    attach_style(children, entries, canvas, index)?;
                }
                if let Some((_, style)) = entries.iter().find(|(i, _)| *i == *index) {
                    let plane = match &node.pixels {
                        Pixels::Raster { dimensions, .. } => *dimensions,
                        Pixels::Adjustment { .. }
                        | Pixels::CmykAdjustment { .. }
                        | Pixels::Group { .. } => canvas,
                    };
                    if crate::precision_style_render::plane_pixels(plane, style.pad())?
                        > crate::doc::MAX_PIXELS
                    {
                        return Err("typed layer style plane exceeds the pixel limit".into());
                    }
                    node.style = Some(style.clone());
                }
                *index += 1;
            }
            Ok(())
        }
        attach_style(&mut pending, &entries, dimensions, &mut 0)?;
    }
    // Word 84's claim, both directions, over every record (`cmyk_native`):
    // a pre-84 CMYK word refuses a clipped, moded or styled record (a group
    // refused as it was read), and an 84 must hold one of them. Then word
    // 83's, so a structured record under 83 is named for what it needs.
    if has_cmyk {
        // Word 85's channel half first, so a pre-85 file holding channels is
        // named for what it needs (its vector-mask half waits for the PATH
        // table below).
        if !channels.is_empty() && !has_cmyk_channels {
            return Err(crate::cmyk_native::CHANNEL_REFUSAL.into());
        }
        let structured = any_pending(&pending, &|node| {
            matches!(node.pixels, Pixels::Group { .. })
                || node.clipped
                || node.blend_mode != crate::blend::BlendMode::Normal
                || node.style.is_some()
        });
        // Word 86's claim, both directions (`cmyk_native::needs_effects`):
        // a pre-86 word refuses an effect, knockout, Blend Interior or Blend
        // If, and an 86 must hold one of them.
        let effects = any_pending(&pending, &|node| {
            node.blend_if.is_some()
                || node.style.as_deref().is_some_and(|style| {
                    style.has_enabled_effects()
                        || style.knockout != crate::style::Knockout::None
                        || style.blend_interior_as_group
                })
        });
        if effects && !has_cmyk_effects {
            return Err(crate::cmyk_native::EFFECTS_REFUSAL.into());
        }
        if cmyk_word == crate::cmyk_native::EFFECTS_VERSION && !effects {
            return Err("invalid native CMYK effects word".into());
        }
        if structured && !has_cmyk_structure {
            return Err(crate::cmyk_native::STRUCTURE_REFUSAL.into());
        }
        if cmyk_word == crate::cmyk_native::STRUCTURE_VERSION && !structured {
            return Err("invalid native CMYK structure word".into());
        }
        // Word 83's claim, both directions: some record anywhere in the
        // tree holds a mask or is a CMYK adjustment (a record that needs 83
        // under 82 refused as it was read).
        if cmyk_word == crate::cmyk_native::EDIT_VERSION
            && !any_pending(&pending, &|node| {
                node.mask.is_some() || matches!(node.pixels, Pixels::CmykAdjustment { .. })
            })
        {
            return Err("invalid native CMYK edit word".into());
        }
    }
    // Word 50's two claims, evaluated ONCE and UNCONDITIONALLY here — OUTSIDE
    // the optional `has_style` block above, so a file with NO `LSTY` table is
    // checked too (`precision_style_knockout`, D10's typed twin). Both land
    // before a single PNG or f32 payload decodes.
    {
        fn carries_keys(nodes: &[Pending<'_>]) -> bool {
            nodes.iter().any(|node| {
                node.style.as_deref().is_some_and(|s| {
                    s.knockout != crate::style::Knockout::None || s.blend_interior_as_group
                }) || match &node.pixels {
                    Pixels::Group { children, .. } => carries_keys(children),
                    _ => false,
                }
            })
        }
        let keyed = carries_keys(&pending);
        if has_knockout && !keyed {
            return Err("invalid native knockout word".into());
        }
        if keyed && !has_knockout {
            return Err("native knockout styles require DRDC 50".into());
        }
    }
    // Word 62's claim, both directions, by the one predicate the writer asks
    // (`levels_native::meta_carries`), over every description in the tree.
    {
        let carried = any_description(&pending, crate::levels_native::meta_carries);
        if has_levels && !carried {
            return Err("invalid native levels word".into());
        }
        if carried && !has_levels {
            return Err("native per-channel levels require DRDC 62".into());
        }
    }
    // Word 64's claim, the same way (`curves_native::meta_carries`).
    {
        let carried = any_description(&pending, crate::curves_native::meta_carries);
        if has_curve_maps && !carried {
            return Err("invalid native curves word".into());
        }
        if carried && !has_curve_maps {
            return Err("native curve maps require DRDC 64".into());
        }
    }
    // Word 71's claim, the same way (`look_native::meta_carries`).
    {
        let carried = any_description(&pending, crate::look_native::meta_carries);
        if has_looks && !carried {
            return Err("invalid native look word".into());
        }
        if carried && !has_looks {
            return Err("Color Lookup abstract looks require DRDC 71".into());
        }
    }
    // Word 111's claim, both directions (`gradient_map_method_native`), over
    // every adjustment description in the tree, a CMYK record's included.
    {
        fn carried(nodes: &[Pending<'_>]) -> bool {
            nodes.iter().any(|node| match &node.pixels {
                Pixels::Adjustment { description, .. }
                | Pixels::CmykAdjustment { description, .. } => {
                    crate::gradient_map_method_native::describes_method(description)
                }
                Pixels::Group { children, .. } => carried(children),
                Pixels::Raster { .. } => false,
            })
        }
        let carried = carried(&pending);
        if has_method && !carried {
            return Err(crate::gradient_map_method_native::INVALID_WORD.into());
        }
        if carried && !has_method {
            return Err(crate::gradient_map_method_native::WORD_REFUSAL.into());
        }
    }
    // The EVALUATION table, after every prior tail. A bad schema, a policy
    // this build does not know or a trailing word refuses HERE, before any
    // PNG decodes — the policy is a claim about every pixel in the file.
    if has_evaluation {
        crate::evaluation_native::parse(&mut reader)?;
    }
    // The FLOT table, last of all. Its operator and white point are admitted
    // by the same rule the editor's own `set_tone_map` applies.
    let tone_map = if float {
        Some(crate::float_native::parse(&mut reader)?)
    } else {
        None
    };
    // The LLBL table, last of all — before the count/trailing check below,
    // which is therefore unchanged. A bad schema, a code this build does not
    // know or an index out of order refuses here, before any pixel decodes.
    if has_label {
        let entries = crate::label_native::parse(&mut reader, declared, job)?;
        fn attach_label(nodes: &mut [Pending<'_>], entries: &[(usize, u8)], index: &mut usize) {
            for node in nodes {
                if let Pixels::Group { children, .. } = &mut node.pixels {
                    attach_label(children, entries, index);
                }
                if let Some(&(_, code)) = entries.iter().find(|(i, _)| *i == *index) {
                    node.label = code;
                }
                *index += 1;
            }
        }
        attach_label(&mut pending, &entries, &mut 0);
    }
    // The LCMP table — still BEFORE the count/trailing check below, so every
    // refusal it carries lands before any pixel decodes. Binding is by INDEX
    // + NAME and the pending nodes have no ids yet (`materialize` mints
    // them, and `adopt` then replaces them with the file's), so `rebind` runs
    // after construction on both of this function's exits.
    let (mut comps, comp_bindings) = if has_comps {
        crate::comps_native::parse(&mut reader, declared, has_ids, job)?
    } else {
        (Vec::new(), Vec::new())
    };
    // Words 89 and 88 (`style_instances`, `cmyk_native`), both
    // directions, over the records and the parsed comps, before any payload
    // decodes: the word claims a stacked style, and a stacked style needs it.
    {
        let stacked = any_pending(&pending, &|node| {
            node.style
                .as_deref()
                .is_some_and(crate::style_instances::is_stacked)
        }) || crate::style_instances::comps_stacked(&comps);
        if has_cmyk {
            if stacked && !has_cmyk_stacked {
                return Err(crate::cmyk_native::STACKED_REFUSAL.into());
            }
            if has_cmyk_stacked && !stacked {
                return Err(crate::cmyk_native::STACKED_INVALID.into());
            }
        } else {
            if stacked && !has_stacked {
                return Err(crate::style_instances::WORD_REFUSAL.into());
            }
            if has_stacked && !stacked {
                return Err(crate::style_instances::INVALID_WORD.into());
            }
        }
        // Word 90 (`style_pattern`), both directions.
        let pattern = any_pending(&pending, &|node| {
            node.style
                .as_deref()
                .is_some_and(crate::style_pattern::holds_pattern)
        }) || crate::style_pattern::comps_pattern(&comps);
        if pattern && !has_pattern {
            return Err(crate::style_pattern::WORD_REFUSAL.into());
        }
        if has_pattern && !pattern {
            return Err(crate::style_pattern::INVALID_WORD.into());
        }
        // Word 93 (`pattern_angle_native`), both directions.
        let angled = any_pending(&pending, &|node| {
            node.style
                .as_deref()
                .is_some_and(crate::pattern_angle_native::holds_angle)
        }) || crate::pattern_angle_native::comps_angle(&comps);
        if angled && !has_angle {
            return Err(crate::pattern_angle_native::WORD_REFUSAL.into());
        }
        if has_angle && !angled {
            return Err(crate::pattern_angle_native::INVALID_WORD.into());
        }
        // Word 103 (`gradient_smoothness_native`), both directions: a
        // record's or comp's style, or a Gradient Fill description.
        let smooth = any_pending(&pending, &|node| {
            node.style
                .as_deref()
                .is_some_and(crate::gradient_smoothness_native::holds_smoothness)
        }) || crate::gradient_smoothness_native::comps_smoothness(&comps)
            || descriptions
                .iter()
                .any(|(_, text)| crate::gradient_smoothness_native::describes_smoothness(text));
        if smooth && !has_smooth {
            return Err(crate::gradient_smoothness_native::WORD_REFUSAL.into());
        }
        if has_smooth && !smooth {
            return Err(crate::gradient_smoothness_native::INVALID_WORD.into());
        }
        // Word 104 (`gradient_midpoint_native`), both directions: a
        // record's or comp's style, or a Gradient Fill description.
        let midpoints = any_pending(&pending, &|node| {
            node.style
                .as_deref()
                .is_some_and(crate::gradient_midpoint_native::holds_midpoints)
        }) || crate::gradient_midpoint_native::comps_midpoints(&comps)
            || descriptions
                .iter()
                .any(|(_, text)| crate::gradient_midpoint_native::describes_midpoints(text));
        if midpoints && !has_midpoint {
            return Err(crate::gradient_midpoint_native::WORD_REFUSAL.into());
        }
        if has_midpoint && !midpoints {
            return Err(crate::gradient_midpoint_native::INVALID_WORD.into());
        }
        // Word 94 (`gradient_options_native`), both directions.
        let options = any_pending(&pending, &|node| {
            node.style
                .as_deref()
                .is_some_and(crate::gradient_options_native::holds_options)
        }) || crate::gradient_options_native::comps_options(&comps);
        if options && !has_options {
            return Err(crate::gradient_options_native::WORD_REFUSAL.into());
        }
        if has_options && !options {
            return Err(crate::gradient_options_native::INVALID_WORD.into());
        }
        // Word 95 (`stroke_pattern_native`), both directions.
        let stroke_pattern = any_pending(&pending, &|node| {
            node.style
                .as_deref()
                .is_some_and(crate::stroke_pattern_native::holds_stroke_pattern)
        }) || crate::stroke_pattern_native::comps_stroke_pattern(&comps);
        if stroke_pattern && !has_stroke_pattern {
            return Err(crate::stroke_pattern_native::WORD_REFUSAL.into());
        }
        if has_stroke_pattern && !stroke_pattern {
            return Err(crate::stroke_pattern_native::INVALID_WORD.into());
        }
        // Word 96 (`contour_native`), both directions.
        let contour = any_pending(&pending, &|node| {
            node.style
                .as_deref()
                .is_some_and(crate::contour_native::holds_contour)
        }) || crate::contour_native::comps_contour(&comps);
        if contour && !has_contour {
            return Err(crate::contour_native::WORD_REFUSAL.into());
        }
        if has_contour && !contour {
            return Err(crate::contour_native::INVALID_WORD.into());
        }
        // Word 97 (`noise_native`), both directions.
        let noise = any_pending(&pending, &|node| {
            node.style
                .as_deref()
                .is_some_and(crate::noise_native::holds_noise)
        }) || crate::noise_native::comps_noise(&comps);
        if noise && !has_noise {
            return Err(crate::noise_native::WORD_REFUSAL.into());
        }
        if has_noise && !noise {
            return Err(crate::noise_native::INVALID_WORD.into());
        }
        // Word 98 (`texture_native`), both directions.
        let texture = any_pending(&pending, &|node| {
            node.style
                .as_deref()
                .is_some_and(crate::texture_native::holds_texture)
        }) || crate::texture_native::comps_texture(&comps);
        if texture && !has_texture {
            return Err(crate::texture_native::WORD_REFUSAL.into());
        }
        if has_texture && !texture {
            return Err(crate::texture_native::INVALID_WORD.into());
        }
        // Word 99 (`glow_gradient_native`), both directions.
        let glow_gradient = any_pending(&pending, &|node| {
            node.style
                .as_deref()
                .is_some_and(crate::glow_gradient_native::holds_glow_gradient)
        }) || crate::glow_gradient_native::comps_glow_gradient(&comps);
        if glow_gradient && !has_glow_gradient {
            return Err(crate::glow_gradient_native::WORD_REFUSAL.into());
        }
        if has_glow_gradient && !glow_gradient {
            return Err(crate::glow_gradient_native::INVALID_WORD.into());
        }
        // Word 100 (`wide_color_native`), both directions.
        let wide = any_pending(&pending, &|node| {
            node.style
                .as_deref()
                .is_some_and(crate::wide_color_native::holds_wide)
        }) || crate::wide_color_native::comps_wide(&comps);
        if wide && !has_wide {
            return Err(crate::wide_color_native::WORD_REFUSAL.into());
        }
        if has_wide && !wide {
            return Err(crate::wide_color_native::INVALID_WORD.into());
        }
        // Word 101 (`blend_if_channels_native`), both directions: a record's
        // own Blend If (`LBIF` schema 2 already refused without the word) or
        // a comp's recorded style.
        let multichannel = any_pending(&pending, &|node| {
            node.blend_if.is_some_and(|b| b.is_multichannel())
        }) || crate::blend_if_channels_native::comps_multichannel(&comps);
        if multichannel && !has_multichannel {
            return Err(crate::blend_if_channels_native::WORD_REFUSAL.into());
        }
        if has_multichannel && !multichannel {
            return Err(crate::blend_if_channels_native::INVALID_WORD.into());
        }
    }
    // The LLNK table, after LCMP and before LIDS. A bad tag or schema and a
    // count that is not the record count refuse here, before any pixel
    // decodes; reserved lock bits are MASKED rather than refused, so a file
    // from a newer build loads with the locks this build understands
    // (`doc_lock`'s module doc, `precision_lock_native`).
    if has_locks {
        let entries = crate::precision_lock_native::parse(&mut reader, declared, job)?;
        fn attach_locks(nodes: &mut [Pending<'_>], entries: &[(u32, u32)], index: &mut usize) {
            for node in nodes {
                if let Pixels::Group { children, .. } = &mut node.pixels {
                    attach_locks(children, entries, index);
                }
                if let Some(&(locks, link)) = entries.get(*index) {
                    node.locks = locks;
                    node.link = link;
                }
                *index += 1;
            }
        }
        attach_locks(&mut pending, &entries, &mut 0);
    }
    // The LGDE table, after LLNK and before LIDS. A bad tag or schema, a
    // count past the cap and an unknown orientation byte refuse here, before
    // any pixel decodes; a nonsense POSITION is repaired rather than refused,
    // which is `drdc_read`'s standing split (`precision_guide_native`). The
    // chrome is assigned after construction on both of this function's
    // exits, because a document is BUILT without it.
    let chrome = if has_guides {
        Some(crate::precision_guide_native::parse(
            &mut reader,
            dimensions.0,
            dimensions.1,
            job,
        )?)
    } else {
        None
    };
    if has_projective {
        crate::source_transform_native::parse(&mut reader, &mut sources, declared)?;
    }
    let pending_warp = if has_warp {
        if float {
            return Err("warp is unavailable at 32-bit float".into());
        }
        fn fallbacks(nodes: &[Pending<'_>], out: &mut Vec<crate::warp_native::Fallback>) {
            for node in nodes {
                if let Pixels::Group { children, .. } = &node.pixels {
                    fallbacks(children, out);
                }
                out.push(crate::warp_native::Fallback {
                    pixels: if let Pixels::Raster { dimensions, .. } = &node.pixels {
                        Some((node.offset, *dimensions))
                    } else {
                        None
                    },
                    mask: node.mask.as_ref().map(|m| {
                        (
                            m.properties.offset,
                            m.dimensions,
                            m.properties.linked,
                            m.properties.outside_coverage,
                        )
                    }),
                });
            }
        }
        let mut fallback = Vec::with_capacity(declared);
        fallbacks(&pending, &mut fallback);
        let (entries, original_bytes) =
            crate::warp_native::parse(&mut reader, declared, 2, super::MAX_DECODED_BYTES, |i| {
                fallback.get(i).copied()
            })?;
        plan.decoded = plan
            .decoded
            .checked_add(original_bytes)
            .ok_or("WARP decoded capacity overflow")?;
        if plan.decoded > super::MAX_DECODED_BYTES {
            return Err("WARP originals and fallback exceed native capacity".into());
        }
        Some(entries)
    } else {
        None
    };
    preflight_sources(&sources, &pending, &mut plan)?;
    // The LOPN table, the last one before LIDS. A bad tag or schema, an empty,
    // unordered or out-of-range list and an index naming a non-group record
    // all refuse here, before any pixel decodes
    // (`precision_group_open_native`).
    if has_open {
        let entries = crate::precision_group_open_native::parse(&mut reader, declared, job)?;
        fn attach_open(
            nodes: &mut [Pending<'_>],
            entries: &[usize],
            index: &mut usize,
        ) -> Result<(), String> {
            for node in nodes {
                let group = if let Pixels::Group { children, .. } = &mut node.pixels {
                    attach_open(children, entries, index)?;
                    true
                } else {
                    false
                };
                if entries.binary_search(index).is_ok() {
                    if !group {
                        return Err("invalid native group open index".into());
                    }
                    node.open = false;
                }
                *index += 1;
            }
            Ok(())
        }
        attach_open(&mut pending, &entries, &mut 0)?;
    }
    // The RSPR table, after LOPN and before LIDS. It only attaches borrowed
    // text to the pending assets, so every refusal lands before any PNG
    // decodes; a word with no RSRC table fails its count check.
    if has_refusals {
        crate::source_refusal_native::parse(&mut reader, &mut sources)?;
    }
    // The LGLT table, after RSPR and before LIDS. A bad tag or schema
    // refuses here, before any PNG decodes; the values are repaired through
    // `GlobalLight::sane` (`precision_light_native`).
    let light = if has_light {
        crate::precision_light_native::parse(&mut reader)?
    } else {
        crate::style::GlobalLight::default()
    };
    // The CSMP table, after LGLT and before LIDS. A bad tag, schema, count or
    // sample size refuses here, before any PNG decodes; an entry off the
    // canvas is dropped (`sampler_native`).
    let color_samplers = if has_samplers {
        crate::sampler_native::parse(&mut reader, dimensions.0, dimensions.1)?
    } else {
        Vec::new()
    };
    // The SOCT table, after CSMP and before LIDS, then SOSM: skipped, every
    // binding of an asset with contents dropped (`native_sources`).
    if has_contents {
        let (assets, float) = crate::native_sources::skip_contents_table(
            &mut reader,
            sources.assets.len(),
            has_float_contents,
            has_stack,
        )?;
        // Word 68 claims a kind-3 entry the table must hold.
        if has_float_contents && !float {
            return Err(crate::native_sources::CONTENTS_WORD_REFUSAL.into());
        }
        sources.drop_assets(&assets);
    }
    if has_stack {
        crate::native_sources::skip_stack_mode_table(&mut reader, declared)?;
    }
    // The PATH table, the last one before LIDS. A bad tag, schema, length or
    // JSON, a refused shape and a broken list rule all refuse here, before
    // any PNG decodes (`path_native`).
    let paths = if has_paths {
        crate::path_native::parse(&mut reader, declared, 2)?
    } else {
        crate::path_native::ParsedTable::empty()
    };
    // A vector mask's raster part is one more mask plane of its record: it
    // counts in the mask total and in the decoded bytes the admission below
    // charges. Each record must also agree with the mask its record stores
    // (`precision_vector_mask`) — all before any PNG decodes.
    for dimensions in paths.plane_dimensions() {
        plan.mask_pixels += scalar_capacity(dimensions, &mut plan.decoded)?;
        if plan.mask_pixels > crate::drdc::MAX_DRDC_TOTAL_MASK_PIXELS {
            return Err("typed native mask pixel count exceeds its limit".into());
        }
    }
    if !paths.masks.is_empty() {
        fn stored(pending: &[Pending<'_>], out: &mut Vec<Option<(u32, u32)>>) {
            for node in pending {
                if let Pixels::Group { children, .. } = &node.pixels {
                    stored(children, out);
                }
                out.push(node.mask.as_ref().map(|mask| mask.dimensions));
            }
        }
        let mut masks = Vec::with_capacity(declared);
        stored(&pending, &mut masks);
        for mask in &paths.masks {
            let slot = masks.get(mask.layer).copied().flatten();
            if !crate::vector_mask_common::table_mask_agrees(mask, slot) {
                return Err(crate::vector_mask_common::TABLE_MISMATCH.into());
            }
        }
    }
    let crate::path_native::ParsedTable {
        entries: paths,
        masks: vector_masks,
    } = paths;
    // Word 85's claim, both directions (`cmyk_native`): a pre-85 CMYK word
    // refuses an alpha channel or a vector-mask entry (on any record, groups
    // included), and an 85 must hold one — before any payload decodes.
    if has_cmyk {
        let carried = !channels.is_empty() || !vector_masks.is_empty();
        if carried && !has_cmyk_channels {
            return Err(crate::cmyk_native::CHANNEL_REFUSAL.into());
        }
        if cmyk_word == crate::cmyk_native::CHANNEL_VERSION && !carried {
            return Err("invalid native CMYK channel word".into());
        }
    }
    // The GVAR table (`generated_native`), after PATH and before LREF
    // (and the CMYK table): every entry names a generated, masked raster of its own
    // dimensions, and every picture's size, tag and cap is checked here,
    // before any payload decodes.
    let variations = if has_variations {
        fn candidates(
            nodes: &[Pending<'_>],
            out: &mut Vec<Option<crate::generated_native::Candidate>>,
        ) {
            for node in nodes {
                if let Pixels::Group { children, .. } = &node.pixels {
                    candidates(children, out);
                }
                out.push(match &node.pixels {
                    Pixels::Raster { dimensions, .. } => Some(crate::generated_native::Candidate {
                        raster: *dimensions,
                        tagged: false,
                        mask: node
                            .mask
                            .as_ref()
                            .map(|m| (m.dimensions, m.properties.offset)),
                    }),
                    _ => None,
                });
            }
        }
        let mut offered = Vec::with_capacity(declared);
        candidates(&pending, &mut offered);
        for &(index, text) in &descriptions {
            if let Some(Some(candidate)) = offered.get_mut(index) {
                candidate.tagged = crate::doc_gen_fill::check_tag(text).is_ok();
            }
        }
        crate::generated_native::parse(&mut reader, declared, has_cmyk, |index| {
            offered.get(index).copied().flatten()
        })?
    } else {
        Vec::new()
    };
    // The LREF table, after every prior tail (GVAR included) but CMYK's and
    // LIDS (`reference_native`): a bad schema, count, order, index or a (0, 0)
    // point refuses here, before any pixel decodes.
    if has_reference {
        let entries = crate::reference_native::parse(&mut reader, declared, job)?;
        fn attach_reference(
            nodes: &mut [Pending<'_>],
            entries: &[(usize, (i32, i32))],
            index: &mut usize,
        ) {
            for node in nodes {
                if let Pixels::Group { children, .. } = &mut node.pixels {
                    attach_reference(children, entries, index);
                }
                if let Some(&(_, point)) = entries.iter().find(|(i, _)| *i == *index) {
                    node.reference = point;
                }
                *index += 1;
            }
        }
        attach_reference(&mut pending, &entries, &mut 0);
    }
    // The SPOT table, after LREF and before the CMYK table
    // (`spot_channel_native`): a bad schema, count, order, index or ink
    // refuses here, before any pixel decodes. Applied in the channel build,
    // after CHPR's metadata.
    let spot_entries = if has_spot {
        crate::spot_channel_native::parse(&mut reader, channels.len(), job)?
    } else {
        Vec::new()
    };
    // The LIDS table, last of all and the ONE this format always writes
    // (`identity_native`), still before the single count/trailing check
    // below, which is therefore unchanged. A bad schema, a count that is not
    // the record count and a zero or repeated id all refuse here.
    // The CMYK table, the last one before LIDS (`cmyk_native`): a bad
    // schema, depth or profile refuses here, before any pixel decodes.
    let cmyk_policy = if has_cmyk {
        Some(crate::cmyk_native::parse(&mut reader)?)
    } else {
        None
    };
    // The GRAY table, in the CMYK table's place (`gray_native`): its depth
    // must agree with the storage and the body's profile must be its twin.
    let gray_policy = if has_gray || has_duotone || has_bitmap {
        // A Duotone document is 8 or 16 bits (`duotone_native`).
        if has_duotone && float {
            return Err(crate::duotone_native::FLOAT_REFUSAL.into());
        }
        let policy = crate::gray_native::parse(&mut reader)?;
        crate::gray_native::check(&policy, float, profile_bytes)?;
        // …and its DUOT table right after GRAY.
        let policy = if has_duotone {
            let inks = crate::duotone_native::parse(&mut reader)?;
            crate::duotone_native::check(&inks, float)?;
            policy.with_duotone(Some(Arc::new(inks)))
        } else {
            policy
        };
        Some(Arc::new(policy))
    } else {
        None
    };
    // The LAB table, in the same place (`lab_native`): u16 storage and the
    // body's profile byte-equal to the Lab twin.
    let lab_policy = if has_lab {
        let policy = crate::lab_native::parse(&mut reader)?;
        crate::lab_native::check(&policy, float, profile_bytes)?;
        Some(Arc::new(policy))
    } else {
        None
    };
    // The INDX table, in the same place (`indexed_native`): the palette, the
    // transparent index and the plane, checked against the body once the
    // document is built.
    let indexed_policy = if has_indexed {
        if float {
            return Err("native Indexed Color document storage must be 16-bit".into());
        }
        Some(Arc::new(crate::indexed_native::parse(&mut reader)?))
    } else {
        None
    };
    // The BTMP table, after GRAY (`bitmap_native`): the packed plane,
    // checked against the body once the document is built.
    let bitmap_policy = if has_bitmap {
        if float {
            return Err("native Bitmap document storage must be 16-bit".into());
        }
        Some(Arc::new(crate::bitmap_native::parse(&mut reader)?))
    } else {
        None
    };
    let ids = if has_ids {
        Some(crate::identity_native::parse(&mut reader, declared, job)?)
    } else {
        None
    };
    if plan.records != declared || reader.remaining() != 0 {
        return Err("high-depth native count mismatch or trailing bytes".into());
    }
    // Account every serialized plane separately, even if the writer originally
    // shared pixel Arcs. The profile can parse three independent sampled TRCs.
    let retained = plan.decoded
        + crate::generated_native::held_bytes(&variations)
        + sources.nonpixel_retained_bound()
        + plan.names
        + indexed_policy
            .as_ref()
            .map_or(0, |p| p.indices().len() as u64)
        + bitmap_policy
            .as_ref()
            .map_or(0, |p| p.packed().len() as u64)
        + 4 * profile_bytes.len() as u64
        + metadata
            .iter()
            .flatten()
            .map(|p| p.len() as u64)
            .sum::<u64>()
        + BOOKKEEPING_BYTES;
    let scratch = reopen_scratch(
        retained,
        reader.held_bytes(),
        plan.band_scratch,
        plan.decode_scratch,
        plan.float,
        plan.float_raster_pixels + sources.float_master_pixels(),
        plan.cmyk,
    )?;
    admit(retained, scratch, other_retained, limit)?;
    job.reserve(retained + scratch)?;
    job.check()?;
    if let Some(hook) = after_preflight {
        hook();
    }
    sources.validate_raw_descriptors()?;
    for &(_, text) in &descriptions {
        job.check()?;
        crate::artwork_document::validate(text)?;
    }
    let profile =
        Arc::new(IccProfile::parse(profile_bytes).ok_or("invalid high-depth native RGB profile")?);
    let source_profiles = sources.profiles()?;
    fn largest_preparation(
        pending: &[Pending<'_>],
        profile: &Arc<IccProfile>,
        job: JobContext<'_>,
    ) -> Result<u64, String> {
        let mut peak = 0_u64;
        for node in pending {
            job.check()?;
            peak = peak.max(match &node.pixels {
                Pixels::Adjustment { description, .. } => {
                    PrecisionAdjustmentState::prepare_scratch_bytes(description, profile)?
                }
                Pixels::CmykAdjustment { description, .. } => {
                    crate::cmyk_adjust_layer::CmykAdjustmentState::prepare_scratch_bytes(
                        description,
                    )
                }
                Pixels::Group { children, .. } => largest_preparation(children, profile, job)?,
                _ => 0,
            });
        }
        Ok(peak)
    }
    let peak = largest_preparation(&pending, &profile, job)?;
    let mut admission = AdjustmentAdmission {
        base: retained,
        scratch,
        other: other_retained,
        limit,
        prepared: 0,
        peak,
    };
    admission.next(peak, job)?;
    fn prepare(
        pending: &mut [Pending<'_>],
        profile: &Arc<IccProfile>,
        admission: &mut AdjustmentAdmission,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        for node in pending {
            job.check()?;
            match &mut node.pixels {
                Pixels::Adjustment {
                    description,
                    prepared,
                } => {
                    let bound =
                        PrecisionAdjustmentState::prepare_scratch_bytes(description, profile)?;
                    admission.next(bound, job)?;
                    let state = PrecisionAdjustmentState::new(description, profile)?;
                    admission.record(&state, job)?;
                    *prepared = Some(state);
                }
                Pixels::CmykAdjustment {
                    description,
                    prepared,
                } => {
                    let bound =
                        crate::cmyk_adjust_layer::CmykAdjustmentState::prepare_scratch_bytes(
                            description,
                        );
                    admission.next(bound, job)?;
                    let state = crate::cmyk_adjust_layer::CmykAdjustmentState::new(description)?;
                    admission.record_bytes(state.retained_bytes(), job)?;
                    *prepared = Some(state);
                }
                Pixels::Group { children, .. } => prepare(children, profile, admission, job)?,
                _ => {}
            }
        }
        job.check()
    }
    // Unknown operators/evaluations, including hidden records, refuse before
    // any earlier raster PNG reaches a decoder.
    prepare(&mut pending, &profile, &mut admission, job)?;
    let metadata = Metadata {
        exif: metadata[0].map(Arc::from),
        xmp: metadata[1].map(Arc::from),
        iptc: metadata[2].map(Arc::from),
    };
    fn materialize(
        pending: Vec<Pending<'_>>,
        profile: &Arc<IccProfile>,
        job: JobContext<'_>,
        completed: &mut usize,
        total: usize,
        float: bool,
        cmyk: bool,
    ) -> Result<Vec<PrecisionDocumentLayer>, String> {
        let mut layers = Vec::with_capacity(pending.len());
        for node in pending {
            job.check()?;
            let mut layer = match node.pixels {
                Pixels::Raster { dimensions, png } if cmyk => {
                    let image = crate::cmyk_native::decode_raster(
                        &png,
                        dimensions,
                        job.range(
                            *completed as f64 / total as f64,
                            (*completed + 1) as f64 / total as f64,
                        ),
                    )?;
                    PrecisionDocumentLayer::raster_cmyk(
                        node.name,
                        image,
                        node.offset,
                        node.opacity,
                        node.visible,
                    )?
                }
                Pixels::Raster { dimensions, png } if float => {
                    // `raster_float` is the ONE F32 leaf constructor; the
                    // storage gate itself lives in `PrecisionDocument::build`.
                    let image = crate::float_native::decode_raster(
                        &png,
                        dimensions,
                        profile.clone(),
                        job.range(
                            *completed as f64 / total as f64,
                            (*completed + 1) as f64 / total as f64,
                        ),
                    )?;
                    PrecisionDocumentLayer::raster_float(
                        node.name,
                        image,
                        node.offset,
                        node.opacity,
                        node.visible,
                    )?
                }
                Pixels::Raster { dimensions, png } => {
                    let image = decode_native_png(
                        &png,
                        dimensions,
                        profile.clone(),
                        job.range(
                            *completed as f64 / total as f64,
                            (*completed + 1) as f64 / total as f64,
                        ),
                    )?;
                    PrecisionDocumentLayer::raster(
                        node.name,
                        image,
                        node.offset,
                        node.opacity,
                        node.visible,
                    )?
                }
                Pixels::Adjustment { prepared, .. } => PrecisionDocumentLayer::prepared_adjustment(
                    node.name,
                    prepared.ok_or("unprepared native adjustment")?,
                    node.opacity,
                    node.visible,
                )?,
                Pixels::CmykAdjustment { prepared, .. } => PrecisionDocumentLayer::cmyk_adjustment(
                    node.name,
                    prepared.ok_or("unprepared native adjustment")?,
                    node.opacity,
                    node.visible,
                )?,
                Pixels::Group {
                    children,
                    pass_through,
                } => {
                    let children =
                        materialize(children, profile, job, completed, total, float, cmyk)?;
                    if pass_through {
                        PrecisionDocumentLayer::pass_through(
                            node.name,
                            children,
                            node.opacity,
                            node.visible,
                        )?
                    } else {
                        PrecisionDocumentLayer::isolated_normal(
                            node.name,
                            children,
                            node.opacity,
                            node.visible,
                        )?
                    }
                }
            };
            layer.blend_mode = node.blend_mode;
            layer.clipped = node.clipped;
            layer.blend_if = node.blend_if;
            layer.style = node.style;
            layer.label = node.label;
            layer.reference = node.reference;
            layer.locks = node.locks;
            layer.link = node.link;
            layer.open = node.open;
            layer.mask = node
                .mask
                .map(|m| {
                    PrecisionMask::new(plane(&m.data, m.dimensions, job)?, m.properties, m.enabled)
                })
                .transpose()?;
            layers.push(layer);
            *completed += 1;
            job.report(*completed as f64 / total as f64, 6)?;
        }
        Ok(layers)
    }
    let mut layers = materialize(pending, &profile, job, &mut 0, declared, float, has_cmyk)?;
    if let Some(entries) = pending_warp {
        crate::warp_native::attach_typed(entries, &mut layers, &profile)?;
    }
    let (assets, bindings) = sources.decode_assets(source_profiles, job)?;
    attach_sources(&mut layers, &bindings, &assets, &mut 0)?;
    crate::artwork_native::attach(&mut layers, &descriptions, &mut 0)?;
    crate::doc_gen_variations::attach_typed(&mut layers, variations, &profile);
    job.check()?;
    let channels = channels
        .into_iter()
        .enumerate()
        .map(|(index, (name, data))| {
            let mut channel = PrecisionChannel::new(name, plane(&data, dimensions, job)?)?;
            if let Some((metadata, text)) = channel_metadata.get(index) {
                channel.metadata = metadata.clone();
                if !text.is_empty() {
                    channel.metadata.provenance =
                        Some(crate::precision_channel_metadata::provenance(text)?);
                }
            }
            // SPOT after CHPR: the assignment above replaces the whole
            // metadata, whose `spot` CHPR never carries.
            if let Some(&(_, ink)) = spot_entries.iter().find(|(i, _)| *i == index) {
                channel.metadata.spot = Some(ink);
            }
            Ok(channel)
        })
        .collect::<Result<Vec<_>, String>>()?;
    // A CMYK document is built through the mode's gate with the policy the
    // CMYK table carried, its alpha channels and (under 85) vector masks
    // adopted as the RGB document's are; it holds no evaluation policy.
    if let Some(policy) = cmyk_policy {
        let mut document = PrecisionDocument::build_cmyk(
            dimensions.0,
            dimensions.1,
            metadata,
            resolution,
            layers,
            channels,
            Arc::new(policy),
        )?;
        if let Some(ids) = &ids {
            crate::identity_native::adopt(&mut document, ids)?;
        }
        bind_comps(&mut comps, &comp_bindings, &document);
        document.comps = comps;
        adopt_chrome(&mut document, chrome);
        document.global_light = light;
        document.color_samplers = color_samplers;
        document.paths.set_entries(paths);
        document.adopt_vector_masks(vector_masks, |data, dimensions| {
            plane(&Payload::Bytes(data), dimensions, job)
        })?;
        return Ok(document);
    }
    // A FLOAT document is built through its own constructor with the map the
    // FLOT table carried: it is linear light by construction, so the
    // `with_evaluation` step below is skipped entirely rather than being
    // asked to switch a policy that is already fixed.
    if let Some(tone_map) = tone_map {
        let mut document = PrecisionDocument::from_parts_float(
            dimensions.0,
            dimensions.1,
            profile,
            metadata,
            resolution,
            layers,
            channels,
            tone_map,
        )?;
        // The file's own ids REPLACE the freshly minted ones before anything
        // binds through them, so a comp entry and the host both see the
        // identity the document was saved with.
        if let Some(ids) = &ids {
            crate::identity_native::adopt(&mut document, ids)?;
        }
        bind_comps(&mut comps, &comp_bindings, &document);
        document.comps = comps;
        adopt_chrome(&mut document, chrome);
        document.global_light = light;
        document.color_samplers = color_samplers;
        document.paths.set_entries(paths);
        // A raster-part plane is a small take of the PATH table
        // (`path_native::parse`), already held, so it converts as bytes.
        document.adopt_vector_masks(vector_masks, |data, dimensions| {
            plane(&Payload::Bytes(data), dimensions, job)
        })?;
        return match gray_policy {
            Some(policy) => document.with_gray(policy),
            None => Ok(document),
        };
    }
    let mut document = PrecisionDocument::from_parts_and_channels(
        dimensions.0,
        dimensions.1,
        profile,
        metadata,
        resolution,
        layers,
        channels,
    )?;
    // The tables are built here, once, from the profile the file carried —
    // a profile the policy cannot decode refuses the OPEN rather than
    // producing a document whose every projection would refuse.
    if has_evaluation {
        document = document
            .with_evaluation(crate::doc_evaluation::Evaluation::LinearLight)?
            .ok_or("high-depth native evaluation policy could not be applied")?;
    }
    if let Some(ids) = &ids {
        crate::identity_native::adopt(&mut document, ids)?;
    }
    bind_comps(&mut comps, &comp_bindings, &document);
    document.comps = comps;
    adopt_chrome(&mut document, chrome);
    document.global_light = light;
    document.color_samplers = color_samplers;
    document.paths.set_entries(paths);
    document.adopt_vector_masks(vector_masks, |data, dimensions| {
        plane(&Payload::Bytes(data), dimensions, job)
    })?;
    // A Lab file is never float (`lab_native::check` refused it above), so
    // only this exit adopts its policy.
    if let Some(policy) = lab_policy {
        return document.with_lab(policy);
    }
    // …and an Indexed file is never float either: its body must be the
    // table's render (`indexed_native::check`).
    if let Some(policy) = indexed_policy {
        crate::indexed_native::check(&policy, &document, job)?;
        return document.with_indexed(policy);
    }
    let document = match gray_policy {
        Some(policy) => document.with_gray(policy)?,
        None => return Ok(document),
    };
    // …and a Bitmap file is 8-bit Gray whose body must be the plane's
    // render (`bitmap_native::check`).
    match bitmap_policy {
        Some(policy) => {
            crate::bitmap_native::check(&policy, &document, job)?;
            document.with_bitmap(policy)
        }
        None => Ok(document),
    }
}

/// Installs the guides and the ruler origin the LGDE table carried. Separate
/// because the reader has two exits — the float constructor's early return
/// and the ordinary one — and both must adopt through the SAME rule; a file
/// with no 48 word leaves the built document's empty list and `(0, 0)`
/// origin exactly as they are.
fn adopt_chrome(
    document: &mut PrecisionDocument,
    chrome: Option<((f64, f64), Vec<crate::doc_guide::Guide>)>,
) {
    if let Some((origin, guides)) = chrome {
        document.guides = guides;
        document.ruler_origin = origin;
    }
}

/// Binds parsed comp entries to the records the finished document holds.
/// Separate because the reader has two exits — the float constructor's early
/// return and the ordinary one — and both must bind through the SAME rule.
fn bind_comps(
    comps: &mut [crate::doc_comps::LayerComp],
    bindings: &[Vec<crate::comps_native::CompBinding>],
    document: &PrecisionDocument,
) {
    crate::comps_native::rebind(
        comps,
        bindings,
        |index| {
            Some((
                document.layer_id(index)?,
                document.layer(index)?.name().to_string(),
            ))
        },
        |id| document.layer_index_by_id(id).is_some(),
    );
}

type PendingRasterExtent = Option<((u32, u32), (i32, i32))>;

/// Inspect source headers and all postorder references before decoding any PNG.
fn preflight_sources(
    sources: &crate::native_sources::PendingSources<'_>,
    pending: &[Pending<'_>],
    plan: &mut Plan,
) -> Result<(), String> {
    fn extents(nodes: &[Pending<'_>], out: &mut Vec<PendingRasterExtent>) {
        for node in nodes {
            if let Pixels::Group { children, .. } = &node.pixels {
                extents(children, out);
            }
            out.push(match &node.pixels {
                Pixels::Raster { dimensions, .. } => Some((*dimensions, node.offset)),
                _ => None,
            });
        }
    }
    let mut layers = Vec::new();
    extents(pending, &mut layers);
    let (band, whole) = sources.inspect_pixels(&mut plan.decoded, super::MAX_DECODED_BYTES)?;
    plan.band_scratch = plan.band_scratch.max(band);
    plan.decode_scratch = plan.decode_scratch.max(whole);
    for binding in &sources.bindings {
        let (fallback, offset) =
            layers[binding.layer].ok_or("embedded source requires a raster fallback")?;
        crate::native_sources::validate_binding(
            binding,
            sources.assets[binding.asset].dimensions,
            fallback,
            offset,
        )?;
    }
    Ok(())
}

fn attach_sources(
    layers: &mut [PrecisionDocumentLayer],
    bindings: &[crate::native_sources::Binding],
    assets: &[Arc<crate::retained_source::SourceAsset>],
    index: &mut usize,
) -> Result<(), String> {
    use crate::precision_document::Content;
    for layer in layers {
        if let Content::Group(children) | Content::PassThrough(children) = &mut layer.content {
            attach_sources(
                Arc::make_mut(children).as_mut_slice(),
                bindings,
                assets,
                index,
            )?;
        }
        if let Some(binding) = bindings.iter().find(|b| b.layer == *index) {
            layer.retained_source = Some(crate::retained_source::RetainedSource16 {
                asset: assets[binding.asset].clone(),
                link: binding.link.clone(),
                transform: binding.transform,
                filter: binding.filter,
                rendered: layer
                    .source()
                    .ok_or("embedded source requires raster fallback")?
                    .clone(),
                rendered_offset: layer.offset(),
            });
        }
        *index += 1;
    }
    Ok(())
}

/// Whether any adjustment description in the pending tree, groups included,
/// satisfies a table-less word's predicate (`levels_native`'s 62,
/// `curves_native`'s 64, `look_native`'s 71) — the reader's twin of
/// `levels_native::precision_any`. A CMYK adjustment record never carries
/// one (`cmyk_native`'s module doc), so under a CMYK word each answer is
/// false and a set 62, 64 or 71 refuses as invalid.
fn any_description(nodes: &[Pending<'_>], carries: fn(&str) -> bool) -> bool {
    nodes.iter().any(|node| match &node.pixels {
        Pixels::Adjustment { description, .. } => carries(description),
        Pixels::Group { children, .. } => any_description(children, carries),
        Pixels::Raster { .. } | Pixels::CmykAdjustment { .. } => false,
    })
}
