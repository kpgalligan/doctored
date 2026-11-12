//! Native record preflight and decoding. All record and PNG dimensions are
//! validated before allocating any decoded pixel or mask planes.

use std::io::Cursor;
use std::sync::Arc;

use image::codecs::png::PngDecoder;
use image::{GrayImage, ImageDecoder, RgbaImage};

use super::*;
use crate::doc::LayerRaster;
use crate::doc_channel::Channel;
use crate::doc_group::MAX_GROUP_DEPTH;
use crate::doc_guide::{normalized, Guide};
use crate::icc::IccProfile;
use crate::metadata::Metadata;
use crate::style::{GlobalLight, LayerStyle};

/// Parses a DRDC buffer of version 1 to 9 (version 1 predates layer masks
/// and layer meta, which default to absent; versions 1 and 2 predate the
/// clipped flag, which defaults to false; versions 1 to 3 predate the layer
/// style and the global light, which default to absent / (120°, 30°);
/// versions 1 to 4 predate the channel list, which defaults to empty;
/// versions 1 to 5 predate the document tail, so the resolution defaults to
/// 72 x 72 ppi, the colour profile to the built-in sRGB and the three
/// metadata packets to absent; versions 1 to 6 predate group structure,
/// locks and links, so every record loads as an unlocked, unlinked, open
/// RASTER entry at depth 0 — a flat stack, exactly what those files hold;
/// versions 1 to 7 predate the guide block, so a file of those versions loads
/// with no guides and its ruler origin at the canvas's top-left).
/// Corrupt or truncated input produces `Err`, never a panic; unknown
/// blend-mode values fall back to Normal, unknown LOCK bits are masked off,
/// opacity is clamped, the light and the resolution are sanitized, the four
/// document blobs are length-capped but otherwise uninterpreted, a style
/// is read LENIENTLY (`LayerStyle::from_json_lenient`: a style from a newer
/// build keeps the effects this build knows; only a structurally malformed
/// style — or an identity — loads as no style), a non-finite ruler-origin
/// component becomes 0.0, a guide with a non-finite position is dropped, an
/// out-of-canvas guide position or origin component is CLAMPED to the nearest
/// canvas edge, and the guide list is then re-sorted and de-duplicated into
/// the model's canonical form (`doc_guide::normalized`).
///
/// **Why the reader CLAMPS an out-of-canvas guide where `DrDocument::crop`
/// DROPS one.** The two are not in conflict: a guide that leaves a CROP
/// window was cut out of the picture by a deliberate user op, and keeping it
/// would leave a line that is invisible, unclickable and undeletable; a
/// nonsense position in a crafted file is a broken VALUE, and this reader's
/// standing rule is to put a broken value at the nearest legal one rather
/// than throw away the author's claim that a guide exists there.
///
/// What is REFUSED rather than repaired, because each is a RELATION between
/// stored quantities and a guess would be silent data loss: a mask length
/// that matches neither the layer nor the canvas, or that matches the wrong
/// one for the entry's kind (see [`DrDocument::encode_native`]'s deferred
/// mask construction); a kind byte other than 0 or 1; a depth past
/// `doc_group::MAX_GROUP_DEPTH`; a malformed depth SEQUENCE, caught by one
/// `doc_group::validate_structure` walk after the records are read; a guide
/// ORIENTATION byte other than 0 or 1 and a guide COUNT over
/// `doc_guide::MAX_GUIDES`, which are the guide block's two structural
/// claims; and a file whose isolated groups declare more canvas area than
/// [`MAX_DRDC_GROUP_CANVAS_PIXELS`], checked as soon as the records are read,
/// before decoding. PNG dimensions and all record structure are inspected in
/// a first pass; pixel decoding starts only after the complete file fits.
///
/// Words 73, 69 and 65 and their `SOCT`/`SOSM` tables, written until
/// 2026-10-10 for smart objects, are bounds-checked and skipped
/// (`native_sources`' module doc): every binding of an asset with contents
/// is dropped and its layer opens with its stored pixels.
pub(crate) fn parse_native(bytes: &[u8]) -> Result<DrDocument, String> {
    let mut r = Reader::new(bytes);
    if !crate::native_magic::is_native_magic(r.take(4)?) {
        return Err("not a DRDC document".to_string());
    }
    let outermost_version = r.u32()?;
    // Version 45 is the OUTERMOST wrapper (`identity_native`) and the only
    // one that is always written: it sits before every prior version word,
    // 43 included, and its LIDS table closes the file. A file WITHOUT it is
    // every file an earlier build wrote, and it mints fresh ids exactly as
    // this reader always did.
    let has_ids = outermost_version == crate::identity_native::BYTE_VERSION;
    let clipping_word = if has_ids { r.u32()? } else { outermost_version };
    // Version 114 is the FIRST word inside 45 (`clipping_path_native`),
    // ahead of 113, with no table: some path is the clipping path. Checked
    // both ways against the PATH table before any PNG decodes.
    let has_clipping_path = has_ids && clipping_word == crate::clipping_path_native::WORD;
    let background_word = if has_clipping_path {
        r.u32()?
    } else {
        clipping_word
    };
    // Version 113 is the next word inside 45 (`background_native`), ahead
    // of 112: the Background mark names a layer, and its LBKG table follows
    // SPOT, checked before any PNG decodes.
    let has_background = has_ids && background_word == crate::background_native::WORD;
    let spot_word = if has_background {
        r.u32()?
    } else {
        background_word
    };
    // Version 112 is the next word inside 45 (`spot_channel_native`, #1),
    // ahead of 111: some channel is a spot channel, and its SPOT table
    // follows LREF, every entry checked before any PNG decodes.
    let has_spot = has_ids && spot_word == crate::spot_channel_native::WORD;
    let method_word = if has_spot { r.u32()? } else { spot_word };
    // Version 111 is the next word inside 45 (`gradient_map_method_native`),
    // ahead of 105, with no table: some layer meta is a Gradient Map
    // with a `method`. Checked both ways against the records before any PNG
    // decodes.
    let has_method = has_ids && method_word == crate::gradient_map_method_native::WORD;
    let align_word = if has_method { r.u32()? } else { method_word };
    // Version 105 is the next word inside 45 (`fill_align_native`),
    // ahead of 104, with no table: some layer meta is a Gradient Fill
    // description aligned with the layer. Checked both ways against the
    // records before any PNG decodes.
    let has_align = has_ids && align_word == crate::fill_align_native::WORD;
    let midpoint_word = if has_align { r.u32()? } else { align_word };
    // Version 104 is the next word inside 45 (`gradient_midpoint_native`),
    // ahead of 103, with no table: some stored style, comp or Gradient
    // Fill description holds a stop midpoint other than 50 %. Checked both
    // ways against the records and the parsed comps before any PNG decodes.
    let has_midpoint = has_ids && midpoint_word == crate::gradient_midpoint_native::WORD;
    let smooth_word = if has_midpoint {
        r.u32()?
    } else {
        midpoint_word
    };
    // Version 103 is the next word inside 45 (`gradient_smoothness_native`),
    // ahead of 102, with no table: some stored style, comp or Gradient
    // Fill description holds a Smoothness below 100 %. Checked both ways
    // against the records and the parsed comps before any PNG decodes.
    let has_smooth = has_ids && smooth_word == crate::gradient_smoothness_native::WORD;
    let large_word = if has_smooth { r.u32()? } else { smooth_word };
    // Version 102 is the next word inside 45 (`large_description_native`),
    // ahead of 101, with no table: some layer meta is an artwork
    // description over 64 KiB. A byte meta has always been opaque up to the
    // meta cap, so only the word without such a meta is refused, against the
    // records before any PNG decodes.
    let has_large = has_ids && large_word == crate::large_description_native::WORD;
    let multichannel_word = if has_large { r.u32()? } else { large_word };
    // Version 101 is the next word inside 45 (`blend_if_channels_native`),
    // ahead of 100, with no table: some stored style holds a
    // multichannel Blend If. Checked against the records and the parsed
    // comps before any PNG decodes.
    let has_multichannel = has_ids && multichannel_word == crate::blend_if_channels_native::WORD;
    let wide_word = if has_multichannel {
        r.u32()?
    } else {
        multichannel_word
    };
    // Version 100 is the next word inside 45 (`wide_color_native`),
    // ahead of 99, with no table: some stored style holds a colour slot
    // carrying a wide colour. Checked against the records and the parsed
    // comps before any PNG decodes.
    let has_wide = has_ids && wide_word == crate::wide_color_native::WORD;
    let glow_word = if has_wide { r.u32()? } else { wide_word };
    // Version 99 is the next word inside 45 (`glow_gradient_native`),
    // ahead of 98, with no table: some stored style holds a glow carrying a
    // gradient or Jitter. Checked against the records and the parsed comps
    // before any PNG decodes.
    let has_glow_gradient = has_ids && glow_word == crate::glow_gradient_native::WORD;
    let texture_word = if has_glow_gradient {
        r.u32()?
    } else {
        glow_word
    };
    // Version 98 is the next word inside 45 (`texture_native`),
    // ahead of 97, with no table: some stored style holds a bevel
    // carrying a texture. Checked against the records and the parsed comps
    // before any PNG decodes.
    let has_texture = has_ids && texture_word == crate::texture_native::WORD;
    let noise_word = if has_texture { r.u32()? } else { texture_word };
    // Version 97 is the next word inside 45 (`noise_native`),
    // ahead of 96, with no table: some stored style holds a Noise
    // above 0. Checked against the records and the parsed comps before any
    // PNG decodes.
    let has_noise = has_ids && noise_word == crate::noise_native::WORD;
    let contour_word = if has_noise { r.u32()? } else { noise_word };
    // Version 96 is the next word inside 45 (`contour_native`),
    // ahead of 95, with no table: some stored style holds an effect
    // that writes a contour-family key. Checked against the records and the
    // parsed comps before any PNG decodes.
    let has_contour = has_ids && contour_word == crate::contour_native::WORD;
    let stroke_pattern_word = if has_contour { r.u32()? } else { contour_word };
    // Version 95 is the next word inside 45 (`stroke_pattern_native`),
    // ahead of 94, with no table: some stored style holds a stroke
    // carrying a pattern fill. Checked against the records and the parsed
    // comps before any PNG decodes.
    let has_stroke_pattern = has_ids && stroke_pattern_word == crate::stroke_pattern_native::WORD;
    let options_word = if has_stroke_pattern {
        r.u32()?
    } else {
        stroke_pattern_word
    };
    // Version 94 is the next word inside 45 (`gradient_options_native`),
    // ahead of 93, with no table: some stored style holds a
    // Shape Burst stroke gradient or a dithered gradient. Checked against
    // the records and the parsed comps before any PNG decodes.
    let has_options = has_ids && options_word == crate::gradient_options_native::WORD;
    let angle_word = if has_options { r.u32()? } else { options_word };
    // Version 93 is the next word inside 45 (`pattern_angle_native`),
    // ahead of 92, with no table: some stored style holds an
    // angled pattern overlay. Checked against the records and the parsed
    // comps before any PNG decodes.
    let has_angle = has_ids && angle_word == crate::pattern_angle_native::WORD;
    let variations_word = if has_angle { r.u32()? } else { angle_word };
    // Version 92 is the next word inside 45 (`generated_native`),
    // ahead of 91: the document holds Generative Layer variations, and its
    // GVAR table follows PATH and precedes LREF, every entry checked before
    // any PNG decodes.
    let has_variations = has_ids && variations_word == crate::generated_native::WORD;
    let reference_word = if has_variations {
        r.u32()?
    } else {
        variations_word
    };
    // Version 91 is the next word inside 45 (`reference_native`),
    // ahead of 90: some layer moved since it was made, and its LREF table is
    // the last one before LIDS.
    let has_reference = has_ids && reference_word == crate::reference_native::BYTE_VERSION;
    let pattern_word = if has_reference {
        r.u32()?
    } else {
        reference_word
    };
    // Version 90 is the next word inside 45 (`style_pattern`),
    // ahead of 89, with no table: some stored style holds a pattern overlay.
    // Checked against the records and the parsed comps before any PNG
    // decodes.
    let has_pattern = has_ids && pattern_word == crate::style_pattern::WORD;
    let stacked_word = if has_pattern { r.u32()? } else { pattern_word };
    // Version 89 is the next word inside 45 (`style_instances`) and
    // has no table: some stored style (a record's or a comp's) is stacked.
    // The claim is checked against the records and the parsed comps before
    // any PNG decodes.
    let has_stacked = has_ids && stacked_word == crate::style_instances::WORD;
    let locals_word = if has_stacked { r.u32()? } else { stacked_word };
    // Version 79 is the next word inside 45 (`raw_locals_native`) and
    // has no table: the RAW sources hold more than four masks on one
    // attachment or over 512 MiB of resources. The claim is checked against
    // the source table right after it is parsed, before any PNG decodes.
    let has_raw_locals = has_ids && locals_word == crate::raw_locals_native::VERSION;
    let paths_word = if has_raw_locals {
        r.u32()?
    } else {
        locals_word
    };
    // Version 74 is the next word inside 45 (`path_native`): the
    // document holds paths, and its PATH table is the last one before LIDS.
    let has_paths = has_ids && paths_word == crate::path_native::BYTE_VERSION;
    let contents_word = if has_paths { r.u32()? } else { paths_word };
    // Version 73 is the next word inside 45 and has no table: some smart
    // object kept 32-bit float contents, kind-3 SOCT entries (read only,
    // `native_sources`). The claim is checked against word 65 below and
    // against the SOCT table when it is skipped.
    let float_contents =
        has_ids && contents_word == crate::native_sources::FLOAT_CONTENTS_BYTE_WORD;
    let float_word = if float_contents {
        r.u32()?
    } else {
        contents_word
    };
    // Version 72 is the next word inside 45 (`float_native`)
    // and has no table: some asset is a 32-bit float master. Only under it
    // may an RSRC asset be depth 32, and the claim is checked against the
    // source table right after it is parsed, before any PNG decodes.
    let float_masters = has_ids && float_word == crate::float_native::BYTE_MASTER_VERSION;
    let first_word = if float_masters { r.u32()? } else { float_word };
    // Version 71 is the next word inside 45 (`look_native`) and has
    // no table: it claims some Color Lookup meta keeps an abstract profile,
    // and that claim is checked against the records below, before any PNG
    // decodes.
    let has_looks = has_ids && first_word == crate::look_native::VERSION;
    let after_looks = if has_looks { r.u32()? } else { first_word };
    // Version 69 is the next word inside 45 (read only): some record had a
    // Stack Mode, and its SOSM table follows SOCT. It requires 65 next.
    let has_stack = has_ids && after_looks == crate::native_sources::STACK_MODE_BYTE_WORD;
    let ids_word = if has_stack { r.u32()? } else { after_looks };
    // Version 65 is the next word inside 45 (read only): some asset carried
    // smart-object contents, and its SOCT table is the last one before LIDS
    // (and SOSM).
    let has_contents = has_ids && ids_word == crate::native_sources::CONTENTS_BYTE_WORD;
    if (has_stack || float_contents) && !has_contents {
        return Err(crate::native_sources::CONTENTS_WORD_REFUSAL.into());
    }
    let after_ids_word = if has_contents { r.u32()? } else { ids_word };
    // Version 63 is the next word inside 45 (`curves_native`)
    // and has no table: it claims some curves meta carries a map, and that
    // claim is checked against the records below, before any PNG decodes.
    let has_curve_maps = has_ids && after_ids_word == crate::curves_native::BYTE_VERSION;
    let after_curve_maps = if has_curve_maps {
        r.u32()?
    } else {
        after_ids_word
    };
    // Version 61 is the next word inside 45 (`levels_native`)
    // and has no table: it claims some levels meta carries an output range
    // or a per-channel stage, and that claim is checked against the records
    // below, before any PNG decodes.
    let has_levels = has_ids && after_curve_maps == crate::levels_native::BYTE_VERSION;
    let after_levels = if has_levels {
        r.u32()?
    } else {
        after_curve_maps
    };
    // Version 59 is the next word inside 45 (`sampler_native`): the
    // document holds colour samplers, and its CSMP table is the last one
    // before LIDS.
    let has_samplers = has_ids && after_levels == crate::sampler_native::BYTE_VERSION;
    let after_ids = if has_samplers { r.u32()? } else { after_levels };
    // Version 56 is the next word inside 45 (`source_refusal_native`): some
    // retained source asset carries a profile refusal, and its RSPR table is
    // the last one before CSMP or LIDS.
    let has_refusals = has_ids && after_ids == crate::source_refusal_native::BYTE_VERSION;
    let after_refusals = if has_refusals { r.u32()? } else { after_ids };
    let has_warp = has_ids && after_refusals == crate::warp_native::BYTE_VERSION;
    let warp_version = if has_warp { r.u32()? } else { after_refusals };
    let has_projective = has_ids && warp_version == crate::source_transform_native::BYTE_VERSION;
    let after_projective = if has_projective {
        r.u32()?
    } else {
        warp_version
    };
    // Version 49 is the ONE word INSIDE 45 (`style_knockout`, D10) and the one
    // with no table: it claims that some record's style carries a non-default
    // `knockout` or `blend_interior_as_group`, and that claim is checked
    // against the records themselves below, before any PNG decodes. Only a
    // file that also carries 45 can carry it; a bare 49 falls through to the
    // body-version check and is refused by name, which is what every older
    // build does with it too.
    let has_knockout = has_ids && after_projective == crate::style_knockout::BYTE_VERSION;
    let comps_version = if has_knockout {
        r.u32()?
    } else {
        after_projective
    };
    // Version 43 is the next wrapper (`comps_native`): it sits before every
    // prior version word, 41 included, and its LCMP table closes the file
    // before LIDS.
    let has_comps = comps_version == crate::comps_native::BYTE_VERSION;
    let outer_version = if has_comps { r.u32()? } else { comps_version };
    // Version 41 is the next wrapper (`label_native`): it sits before every
    // prior version word, 38 included, and its LLBL table closes the file
    // before LCMP.
    let has_label = outer_version == crate::label_native::BYTE_VERSION;
    let evaluation_version = if has_label { r.u32()? } else { outer_version };
    // Version 38 is the next wrapper (`evaluation_native`): it sits before
    // every prior version word, 28 included, and its table closes the file
    // before LLBL. Its own body word is therefore either 28 or an ordinary
    // body version, both of which the checks below already police.
    let has_evaluation = evaluation_version == crate::evaluation_native::BYTE_VERSION;
    let wrapped_version = if has_evaluation {
        r.u32()?
    } else {
        evaluation_version
    };
    let has_outside = wrapped_version == crate::mask_outside_native::BYTE_VERSION;
    let version = if has_outside {
        r.u32()?
    } else {
        wrapped_version
    };
    if has_outside && !matches!(version, 10 | 14 | 16 | 18 | 21 | 23) {
        return Err("invalid native mask outside byte body version".into());
    }
    if version == 0 || version > DRDC_VERSION {
        return Err(format!("unsupported DRDC version {version}"));
    }
    if version > LEGACY_DRDC_VERSION
        && !matches!(
            version,
            crate::native_sources::BYTE_SOURCE_VERSION
                | crate::native_sources::BYTE_LINK_VERSION
                | crate::native_sources::BYTE_RAW_VERSION
                | crate::native_sources::BYTE_RESOURCE_VERSION
                | crate::native_sources::BYTE_FILTER_VERSION
        )
    {
        return Err(format!("unsupported DRDC version {version} in the RGBA8 reader; use the preserving high-depth document reader (no RGBA8 fallback)"));
    }
    // Version 1 layer records stop after the pixel PNG; version 2 records
    // after the mask and meta fields; version 3 records after the clipped
    // byte.
    let has_mask_and_meta = version >= 2;
    let has_clipped = version >= 3;
    let has_style = version >= 4;
    let has_channels = version >= 5;
    let has_document_tail = version >= 6;
    // Version 7 records end with the structure, the locks and the link.
    let has_groups = version >= 7;
    // Version 8 closes the FILE with the ruler origin and the guide list.
    let has_guides = version >= 8;
    let width = r.u32()?;
    let height = r.u32()?;
    capacity::canvas_size(width, height)?;
    let count = capacity::layer_count(r.u32()? as usize)?;
    // Version 4 document fields; lenient on values, like opacity.
    let global_light = if version >= 4 {
        GlobalLight {
            angle: r.f32()?,
            altitude: r.f32()?,
        }
        .sane()
    } else {
        GlobalLight::default()
    };
    let mut layers = Vec::with_capacity(count as usize);
    let mut total_pixels: u64 = 0;
    let mut total_mask_pixels: u64 = 0;
    // The placeholders are private to this parse. Metadata/structure can be
    // checked without allocating decoded planes; every placeholder is replaced
    // before the document is returned. Stored slices borrow the input buffer.
    let pending_pixel = Arc::new(RgbaImage::new(0, 0));
    let pending_mask = Arc::new(GrayImage::new(0, 0));
    let mut layer_planes = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let name_len = r.u32()?;
        if name_len > MAX_DRDC_NAME_LEN {
            return Err(format!("layer name length {name_len} out of range"));
        }
        let name = String::from_utf8_lossy(r.take(name_len as usize)?).into_owned();
        let off_x = r.i32()?;
        let off_y = r.i32()?;
        let opacity = sane_opacity(r.f32()?);
        let blend_raw = r.u32()?;
        let blend = i32::try_from(blend_raw)
            .ok()
            .and_then(BlendMode::from_c)
            .unwrap_or(BlendMode::Normal);
        let visible = r.u8()? != 0;
        let png_len = r.u32()?;
        if png_len > MAX_DRDC_PNG_LEN {
            return Err(format!("layer PNG length {png_len} out of range"));
        }
        let png = r.take(png_len as usize)?;
        let (lw, lh) = png_dimensions(png, "layer pixels")?;
        total_pixels = total_pixels.saturating_add(u64::from(lw) * u64::from(lh));
        capacity::layer_pixels(total_pixels)?;
        // The mask BYTES are taken here but the image is NOT built yet: a
        // group's mask is canvas-sized and a raster layer's is layer-sized,
        // and the kind byte that tells them apart comes later in the record.
        // Both lengths are accepted now and the pairing is checked once the
        // kind is known, which for every version-1 to -6 file (always Raster)
        // can only defer a refusal that is then made identically.
        let mut mask_raw: Option<&[u8]> = None;
        let mut independent_mask_dims = None;
        let mut mask_enabled = true;
        let mut meta = None;
        let layer_px = u64::from(lw) * u64::from(lh);
        let canvas_px = u64::from(width) * u64::from(height);
        if has_mask_and_meta {
            let present = r.u8()? != 0;
            mask_enabled = r.u8()? != 0;
            if present {
                let mask_len = r.u32()?;
                if version >= 10 {
                    let dims = (r.u32()?, r.u32()?);
                    capacity::mask_dimensions(dims)?;
                    if u64::from(dims.0) * u64::from(dims.1) != u64::from(mask_len) {
                        return Err(
                            "layer mask length does not match independent dimensions".to_string()
                        );
                    }
                    independent_mask_dims = Some(dims);
                } else if u64::from(mask_len) != layer_px && u64::from(mask_len) != canvas_px {
                    return Err(format!(
                        "layer mask length {mask_len} does not match the layer's {layer_px} pixels"
                    ));
                }
                total_mask_pixels = total_mask_pixels.saturating_add(u64::from(mask_len));
                if version >= 10 {
                    capacity::mask_pixels(total_mask_pixels)?;
                }
                mask_raw = Some(r.take(mask_len as usize)?);
            }
            meta = take_opt_string(&mut r, "meta")?;
        }
        let clipped = has_clipped && r.u8()? != 0;
        let style = if has_style {
            take_opt_string(&mut r, "style")?
                .and_then(|s| LayerStyle::from_json_lenient(&s).ok())
                // A byte document has no plates: a CMYK-channel Blend If (a
                // file this build's setter never writes) is dropped (#1).
                .map(|s| s.without_cmyk_blend_if())
                .filter(|s| !s.is_identity())
                .map(Arc::new)
        } else {
            None
        };
        // Version 7 fields. Versions 1-6 have no structure, no locks and no
        // links: every record is a top-level raster layer, which is exactly
        // what those files hold.
        let (locks, link, depth, kind, open) = if has_groups {
            let locks = r.u32()? & LOCK_ALL;
            let link = r.u32()?;
            let depth = r.u16()?;
            if depth > MAX_GROUP_DEPTH {
                return Err(format!(
                    "layer nesting deeper than {MAX_GROUP_DEPTH} levels ({depth})"
                ));
            }
            let kind = match r.u8()? {
                0 => LayerKind::Raster,
                1 => LayerKind::Group,
                other => return Err(format!("unsupported layer kind {other}")),
            };
            (locks, link, depth, kind, r.u8()? != 0)
        } else {
            (0, 0, 0, LayerKind::Raster, true)
        };
        // Now the kind is known, so the mask can be built at the size it
        // requires — and a mask sized to the other one is refused, because the
        // pairing must be RIGHT and not merely plausible.
        let (mw, mh) = independent_mask_dims
            .unwrap_or_else(|| expected_mask_dims(kind, (lw, lh), width, height));
        let mut mask_properties = crate::doc_mask::MaskProperties::at((off_x, off_y));
        if version >= 10 && mask_raw.is_some() {
            mask_properties.offset = (r.i32()?, r.i32()?);
            mask_properties.linked = match r.u8()? {
                0 => false,
                1 => true,
                _ => return Err("invalid mask link flag".to_string()),
            };
            mask_properties.density = r.f32()?;
            mask_properties.feather = r.f32()?;
            if !mask_properties.valid((mw, mh)) {
                return Err("invalid mask properties or feather extent".to_string());
            }
        }
        let mask = match mask_raw {
            None => None,
            Some(raw) => {
                let expected = u64::from(mw) * u64::from(mh);
                if raw.len() as u64 != expected {
                    let n = raw.len();
                    return Err(match kind {
                        LayerKind::Raster => format!(
                            "layer mask length {n} does not match the layer's {expected} pixels"
                        ),
                        LayerKind::Group => format!(
                            "layer mask length {n} does not match the canvas's {expected} pixels"
                        ),
                    });
                }
                Some(Arc::clone(&pending_mask))
            }
        };
        layer_planes.push((png, (lw, lh), mask_raw, (mw, mh)));
        layers.push(Layer {
            // A fresh identity per entry: layer ids are deliberately NOT
            // persisted, so a reopened document mints its own (`doc_identity`).
            id: crate::doc_identity::mint_layer_id(),
            retained_source: None,
            warp: None,
            generated: None,
            pixels: LayerRaster::shared(Arc::clone(&pending_pixel)),
            offset: (off_x, off_y),
            name,
            opacity,
            blend,
            visible,
            mask,
            mask_enabled,
            mask_properties,
            meta,
            clipped,
            style,
            kind,
            depth,
            locks,
            link,
            open,
            // Labels live in the conditional LLBL tail, never in a record,
            // so every entry starts unlabelled (`label_native`).
            label: 0,
            // Likewise the LREF tail (`reference_native`).
            reference: (0, 0),
        });
    }
    // ONE walk over the finished records: a malformed depth sequence is a
    // relation between stored values, so it is refused like a mask length and
    // never repaired. Without it a child with no enclosing group would simply
    // DISAPPEAR from every projection, export and save with no error anywhere.
    if !validate_structure(&layers) {
        return Err("malformed layer group structure".to_string());
    }
    if isolated_group_pixels(&layers, width, height) > MAX_DRDC_GROUP_CANVAS_PIXELS {
        return Err("layer groups would need more memory than this build allows".to_string());
    }
    // D10, both directions and BEFORE the first PNG decode: word 49 is the
    // file's claim that some record's style carries a non-default knockout
    // key, and a claim that does not match the records is a broken RELATION,
    // never something to repair (`style_knockout`).
    let carries_knockout = crate::style_knockout::styles_carry_keys(&layers);
    if has_knockout && !carries_knockout {
        return Err("invalid native knockout word".to_string());
    }
    if carries_knockout && !has_knockout {
        return Err("native knockout styles require DRDC 49".to_string());
    }
    // Word 61's claim, both directions and BEFORE the first PNG decode, by
    // the same rule (`levels_native`).
    let carries_levels = crate::levels_native::byte_carries(&layers);
    if has_levels && !carries_levels {
        return Err("invalid native levels word".to_string());
    }
    if carries_levels && !has_levels {
        return Err("native per-channel levels require DRDC 61".to_string());
    }
    // Word 63's claim, both directions and BEFORE the first PNG decode, by
    // the same rule (`curves_native`).
    let carries_curve_maps = crate::curves_native::byte_carries(&layers);
    if has_curve_maps && !carries_curve_maps {
        return Err("invalid native curves word".to_string());
    }
    if carries_curve_maps && !has_curve_maps {
        return Err("native curve maps require DRDC 63".to_string());
    }
    // Word 71's claim, both directions and BEFORE the first PNG decode, by
    // the same rule (`look_native`).
    let carries_looks = crate::look_native::byte_carries(&layers);
    if has_looks && !carries_looks {
        return Err("invalid native look word".to_string());
    }
    if carries_looks && !has_looks {
        return Err("Color Lookup abstract looks require DRDC 71".to_string());
    }
    let mut channels = Vec::new();
    let mut channel_planes = Vec::new();
    if has_channels {
        let count = capacity::channel_count(r.u32()? as usize, width, height)?;
        channels = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let name_len = r.u32()?;
            if name_len > MAX_DRDC_NAME_LEN {
                return Err(format!("channel name length {name_len} out of range"));
            }
            let name = String::from_utf8_lossy(r.take(name_len as usize)?).into_owned();
            let overlay_color = [r.u8()?, r.u8()?, r.u8()?];
            let overlay_opacity = sane_opacity(r.f32()?);
            let color_indicates_selected = r.u8()? != 0;
            let png_len = r.u32()?;
            if png_len > MAX_DRDC_PNG_LEN {
                return Err(format!("channel PNG length {png_len} out of range"));
            }
            let png = r.take(png_len as usize)?;
            let (cw, ch) = png_dimensions(png, "channel plane")?;
            if (cw, ch) != (width, height) {
                return Err(format!(
                    "channel plane {cw}x{ch} does not match the {width}x{height} canvas"
                ));
            }
            channel_planes.push(png);
            // Through `Channel::new`, so the loaded channel gets a fresh
            // identity: `Channel::id` is a host-facing handle for as long as a
            // document is open, never a persisted property (see its doc).
            channels.push(Channel {
                overlay_color,
                overlay_opacity,
                color_indicates_selected,
                ..Channel::new(&name, Arc::clone(&pending_mask))
            });
        }
    }
    // Version 6: the document tail. Sanitized rather than refused, like the
    // global light; the blobs are capped before a byte is taken and are
    // otherwise passed through untouched.
    let (resolution, profile, metadata) = if has_document_tail {
        let resolution = Resolution {
            x: r.f32()?,
            y: r.f32()?,
        }
        .sane();
        let icc = take_opt_bytes(&mut r, "ICC profile", MAX_DRDC_BLOB_LEN)?;
        let exif = take_opt_bytes(&mut r, "EXIF packet", MAX_DRDC_BLOB_LEN)?;
        let xmp = take_opt_bytes(&mut r, "XMP packet", MAX_DRDC_BLOB_LEN)?;
        let iptc = take_opt_bytes(&mut r, "IPTC packet", MAX_DRDC_BLOB_LEN)?;
        // An absent slot is the built-in sRGB (the writer elides it); a
        // present blob that no longer parses as an RGB profile falls back to
        // it too, rather than refusing a file whose pixels are fine.
        let profile = if matches!(
            version,
            crate::native_sources::BYTE_SOURCE_VERSION
                | crate::native_sources::BYTE_LINK_VERSION
                | crate::native_sources::BYTE_RAW_VERSION
                | crate::native_sources::BYTE_RESOURCE_VERSION
                | crate::native_sources::BYTE_FILTER_VERSION
        ) {
            // A retained master and its fallback are interpreted in distinct
            // captured profiles. New source-bearing files cannot silently
            // reinterpret a corrupt fallback profile as sRGB. Older byte files
            // retain their established repair policy below.
            icc.as_deref()
                .map(|bytes| {
                    IccProfile::parse(bytes)
                        .map(Arc::new)
                        .ok_or_else(|| "invalid embedded-source document RGB profile".to_string())
                })
                .transpose()?
                .unwrap_or_else(IccProfile::srgb)
        } else {
            icc.and_then(|b| IccProfile::parse(&b))
                .map(Arc::new)
                .unwrap_or_else(IccProfile::srgb)
        };
        (
            resolution,
            profile,
            Metadata {
                exif: exif.map(Arc::from),
                xmp: xmp.map(Arc::from),
                iptc: iptc.map(Arc::from),
            },
        )
    } else {
        (
            Resolution::default(),
            IccProfile::srgb(),
            Metadata::default(),
        )
    };
    // Version 8: the ruler origin and the guide list, closing the file.
    // Values are REPAIRED (a non-finite origin component becomes 0.0, a
    // non-finite position drops that guide, an out-of-canvas position or
    // origin is clamped to the nearest edge, everything is quantized) while
    // the two structural claims — the orientation byte and the count — are
    // REFUSED, which is this reader's standing split.
    let (ruler_origin, guides) = if has_guides {
        let origin = sane_origin((r.f64()?, r.f64()?), width, height);
        let count = r.u32()?;
        if count as usize > MAX_GUIDES {
            return Err(format!("invalid guide count {count}"));
        }
        let mut list = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let raw = r.u8()?;
            let orientation = GuideOrientation::from_c(i32::from(raw))
                .ok_or_else(|| format!("unsupported guide orientation {raw}"))?;
            let extent = match orientation {
                GuideOrientation::Horizontal => f64::from(height),
                GuideOrientation::Vertical => f64::from(width),
            };
            // Through `Guide::new`, so the loaded guide gets a fresh
            // identity: `Guide::id` is a host-facing handle for as long as a
            // document is open, never a persisted property (see its doc).
            if let Some(position) = sane_position(r.f64()?, extent) {
                list.push(Guide::new(orientation, position));
            }
        }
        (origin, normalized(list))
    } else {
        ((0.0, 0.0), Vec::new())
    };
    let mut sources = if matches!(
        version,
        crate::native_sources::BYTE_SOURCE_VERSION
            | crate::native_sources::BYTE_LINK_VERSION
            | crate::native_sources::BYTE_RAW_VERSION
            | crate::native_sources::BYTE_RESOURCE_VERSION
            | crate::native_sources::BYTE_FILTER_VERSION
    ) {
        // No byte word admits a camera original on a float master (word 78
        // is typed only, `float_native`), so the claim is always false here.
        crate::native_sources::PendingSources::parse(
            &mut r,
            layers.len(),
            version,
            float_masters,
            false,
        )?
    } else {
        crate::native_sources::PendingSources::default()
    };
    // Word 72 claims a float master the source table must hold.
    if float_masters && sources.float_master_pixels() == 0 {
        return Err(crate::float_native::MASTER_WORD_REFUSAL.into());
    }
    // Word 79's claim, both directions, before any PNG decodes.
    let (pool_bytes, most_resources) = sources.raw_locals_claim();
    let claimed = crate::raw_locals_native::claim(pool_bytes, most_resources);
    if has_raw_locals && !claimed {
        return Err(crate::raw_locals_native::INVALID_WORD.into());
    }
    if claimed && !has_raw_locals {
        return Err(crate::raw_locals_native::MISSING_WORD.into());
    }
    if has_outside {
        let indices = crate::mask_outside_native::parse(
            &mut r,
            layers.len(),
            crate::job_control::JobContext::NONE,
        )?;
        for index in indices {
            let layer = &mut layers[index];
            if layer.mask.is_none() {
                return Err("native mask outside reference has no mask".into());
            }
            layer.mask_properties.outside_coverage = 255;
        }
        if !has_evaluation && !has_label && !has_comps && !has_ids && r.remaining() != 0 {
            return Err("native mask outside has trailing bytes".into());
        }
    }
    // The EVALUATION table, after every prior tail. A bad schema, a policy
    // this build does not know, a trailing word or a profile the policy
    // cannot decode all refuse HERE — before the first PNG is decoded —
    // because the policy is a claim about every pixel in the file.
    let linear_light = if has_evaluation {
        crate::evaluation_native::parse(&mut r)?;
        if !has_label && !has_comps && !has_ids && r.remaining() != 0 {
            return Err("native evaluation has trailing bytes".into());
        }
        Some(Arc::new(crate::doc_evaluation::ByteTransfer::for_profile(
            &profile,
        )?))
    } else {
        None
    };
    // The LABEL table, after every prior tail, EVAL included. A bad schema, a
    // code this build does not know, an index out of order and a trailing
    // word all refuse HERE — before the first PNG is decoded — like every
    // other conditional table. A file without the word loads with label 0 on
    // every entry, which is what every document before this version holds.
    if has_label {
        let entries =
            crate::label_native::parse(&mut r, layers.len(), crate::job_control::JobContext::NONE)?;
        for (index, code) in entries {
            layers[index].label = code;
        }
        if !has_comps && !has_ids && r.remaining() != 0 {
            return Err("native layer labels have trailing bytes".into());
        }
    }
    // The COMP table, after every prior tail, LLBL included. A bad schema, a
    // length this build refuses, malformed JSON, a bad or repeated reference
    // and a trailing word all refuse HERE — before the first PNG is decoded
    // — like every other conditional table. Binding is by the PERSISTED ID
    // at schema 2 and by INDEX + NAME at schema 1 (`comps_native`), and it
    // runs only once the records carry their FINAL ids — the file's when
    // LIDS is present, the freshly minted ones otherwise — which is why the
    // parse and the rebind are separated by the identity table below. A
    // schema-2 payload in a file with no LIDS is refused outright: its ids
    // would be meaningless.
    let parsed_comps = if has_comps {
        Some(crate::comps_native::parse(
            &mut r,
            layers.len(),
            has_ids,
            crate::job_control::JobContext::NONE,
        )?)
    } else {
        None
    };
    if has_projective {
        crate::source_transform_native::parse(&mut r, &mut sources, layers.len())?;
    }
    let pending_warp = if has_warp {
        let (entries, original_bytes) =
            crate::warp_native::parse(&mut r, layers.len(), 1, MAX_DRDC_DECODED_BYTES, |index| {
                let layer = layers.get(index)?;
                let (offset, dims) = layer_planes.get(index).map(|p| (layer.offset, p.1))?;
                Some(crate::warp_native::Fallback {
                    pixels: (layer.kind == LayerKind::Raster).then_some((offset, dims)),
                    mask: layer.mask.as_ref().map(|_| {
                        (
                            layer.mask_properties.offset,
                            layer_planes[index].3,
                            layer.mask_properties.linked,
                            layer.mask_properties.outside_coverage,
                        )
                    }),
                })
            })?;
        let source_bytes = sources.assets.iter().fold(0u64, |sum, asset| {
            sum.saturating_add(
                u64::from(asset.dimensions.0)
                    * u64::from(asset.dimensions.1)
                    * u64::from(asset.depth / 8)
                    * 4,
            )
        });
        capacity::warp_decoded_capacity(
            original_bytes,
            total_pixels.saturating_mul(4),
            total_mask_pixels,
            u64::from(width) * u64::from(height) * channels.len() as u64,
            source_bytes,
        )?;
        Some(entries)
    } else {
        None
    };
    // The RSPR table, after WARP and before LIDS. It only attaches borrowed
    // text to the pending assets, so every refusal lands before any PNG
    // decodes; a word with no RSRC table fails its count check.
    if has_refusals {
        crate::source_refusal_native::parse(&mut r, &mut sources)?;
    }
    // The CSMP table, after RSPR and before LIDS. A bad tag, schema, count or
    // sample size refuses here, before any PNG decodes; an entry off the
    // canvas is dropped (`sampler_native`).
    let color_samplers = if has_samplers {
        crate::sampler_native::parse(&mut r, width, height)?
    } else {
        Vec::new()
    };
    // The SOCT table, after CSMP and before LIDS, then SOSM: skipped, every
    // binding of an asset with contents dropped (`native_sources`).
    if has_contents {
        let (assets, float) = crate::native_sources::skip_contents_table(
            &mut r,
            sources.assets.len(),
            float_contents,
            has_stack,
        )?;
        // Word 73 claims a kind-3 entry the table must hold.
        if float_contents && !float {
            return Err(crate::native_sources::CONTENTS_WORD_REFUSAL.into());
        }
        sources.drop_assets(&assets);
    }
    if has_stack {
        crate::native_sources::skip_stack_mode_table(&mut r, layers.len())?;
    }
    // The PATH table, before GVAR and LREF. A bad tag, schema, length or
    // JSON, a refused shape and a broken list rule all refuse here, before
    // any PNG decodes (`path_native`).
    let paths = if has_paths {
        crate::path_native::parse(&mut r, layers.len(), 1)?
    } else {
        crate::path_native::ParsedTable::empty()
    };
    // Word 114's claim, both ways (`clipping_path_native`).
    crate::clipping_path_native::check(has_clipping_path, &paths.entries)?;
    // The GVAR table, after PATH and before LREF (`generated_native`): every
    // entry names a generated, masked raster of its own dimensions, checked
    // before any PNG decodes.
    let variations = if has_variations {
        crate::generated_native::parse(&mut r, layers.len(), false, |index| {
            let layer = layers.get(index)?;
            let (_, dims, mask, mask_dims) = layer_planes.get(index)?;
            (layer.kind == LayerKind::Raster).then(|| crate::generated_native::Candidate {
                raster: *dims,
                tagged: layer
                    .meta
                    .as_deref()
                    .is_some_and(|m| crate::doc_gen_fill::check_tag(m).is_ok()),
                mask: (*mask).map(|_| (*mask_dims, layer.mask_properties.offset)),
            })
        })?
    } else {
        Vec::new()
    };
    // The LREF table, before SPOT and LIDS. A bad schema, count, order,
    // index or a (0, 0) point refuses here, before any PNG decodes
    // (`reference_native`).
    if has_reference {
        let entries = crate::reference_native::parse(
            &mut r,
            layers.len(),
            crate::job_control::JobContext::NONE,
        )?;
        for (index, reference) in entries {
            layers[index].reference = reference;
        }
    }
    // The SPOT table, after LREF and before LIDS. A bad schema, count,
    // order, index or ink refuses here, before any PNG decodes
    // (`spot_channel_native`).
    if has_spot {
        let entries = crate::spot_channel_native::parse(
            &mut r,
            channels.len(),
            crate::job_control::JobContext::NONE,
        )?;
        for (index, ink) in entries {
            channels[index].spot = Some(ink);
        }
    }
    // The LBKG table, after SPOT and before LIDS: a bad schema, index or
    // role refuses here, before any PNG decodes (`background_native`). It
    // binds to the record's final id below, once LIDS is read.
    let background_entry = if has_background {
        Some(crate::background_native::parse(&mut r, layers.len())?)
    } else {
        None
    };
    // A vector mask's raster part is one more mask plane of its layer, and
    // each record must agree with the mask its layer stores
    // (`doc_vector_mask`) — both before the first PNG decodes.
    if version >= 10 {
        capacity::mask_pixels(
            paths
                .plane_dimensions()
                .fold(total_mask_pixels, |n, (w, h)| {
                    n.saturating_add(u64::from(w) * u64::from(h))
                }),
        )?;
    }
    for mask in &paths.masks {
        let stored = layer_planes
            .get(mask.layer)
            .and_then(|plane| plane.2.map(|_| plane.3));
        if !crate::vector_mask_common::table_mask_agrees(mask, stored) {
            return Err(crate::vector_mask_common::TABLE_MISMATCH.into());
        }
    }
    // The IDENTITY table, after every prior tail, LCMP included, and the ONE
    // table this format always writes (`identity_native`). A bad schema, a
    // count that is not the record count, a zero or repeated id and a
    // trailing word all refuse HERE, before the first PNG is decoded. A file
    // without the word keeps the ids `Layer::new` minted while reading it.
    if has_ids {
        let ids = crate::identity_native::parse(
            &mut r,
            layers.len(),
            crate::job_control::JobContext::NONE,
        )?;
        for (layer, id) in layers.iter_mut().zip(ids) {
            layer.id = id;
        }
        if r.remaining() != 0 {
            return Err("native layer ids have trailing bytes".into());
        }
    }
    let comps = if let Some((mut comps, bindings)) = parsed_comps {
        crate::comps_native::rebind(
            &mut comps,
            &bindings,
            |index| layers.get(index).map(|l| (l.id, l.name.clone())),
            |id| layers.iter().any(|l| l.id == id),
        );
        if !has_ids && r.remaining() != 0 {
            return Err("native layer comps have trailing bytes".into());
        }
        comps
    } else {
        Vec::new()
    };
    // Word 89's claim, both directions and BEFORE the first PNG decode, over
    // the records and the parsed comps (`style_instances`).
    let stacked = crate::style_instances::pending_stacked(&layers, &comps);
    if has_stacked && !stacked {
        return Err(crate::style_instances::INVALID_WORD.into());
    }
    if stacked && !has_stacked {
        return Err(crate::style_instances::WORD_REFUSAL.into());
    }
    // Word 90's claim, the same way (`style_pattern`).
    let pattern = crate::style_pattern::pending_pattern(&layers, &comps);
    if has_pattern && !pattern {
        return Err(crate::style_pattern::INVALID_WORD.into());
    }
    if pattern && !has_pattern {
        return Err(crate::style_pattern::WORD_REFUSAL.into());
    }
    // Word 93's claim, the same way (`pattern_angle_native`).
    let angled = crate::pattern_angle_native::pending_angle(&layers, &comps);
    if has_angle && !angled {
        return Err(crate::pattern_angle_native::INVALID_WORD.into());
    }
    if angled && !has_angle {
        return Err(crate::pattern_angle_native::WORD_REFUSAL.into());
    }
    // Word 94's claim, the same way (`gradient_options_native`).
    let options = crate::gradient_options_native::pending_options(&layers, &comps);
    if has_options && !options {
        return Err(crate::gradient_options_native::INVALID_WORD.into());
    }
    if options && !has_options {
        return Err(crate::gradient_options_native::WORD_REFUSAL.into());
    }
    // Word 95's claim, the same way (`stroke_pattern_native`).
    let stroke_pattern = crate::stroke_pattern_native::pending_stroke_pattern(&layers, &comps);
    if has_stroke_pattern && !stroke_pattern {
        return Err(crate::stroke_pattern_native::INVALID_WORD.into());
    }
    if stroke_pattern && !has_stroke_pattern {
        return Err(crate::stroke_pattern_native::WORD_REFUSAL.into());
    }
    // Word 96's claim, the same way (`contour_native`).
    let contour = crate::contour_native::pending_contour(&layers, &comps);
    if has_contour && !contour {
        return Err(crate::contour_native::INVALID_WORD.into());
    }
    if contour && !has_contour {
        return Err(crate::contour_native::WORD_REFUSAL.into());
    }
    // Word 97's claim, the same way (`noise_native`).
    let noise = crate::noise_native::pending_noise(&layers, &comps);
    if has_noise && !noise {
        return Err(crate::noise_native::INVALID_WORD.into());
    }
    if noise && !has_noise {
        return Err(crate::noise_native::WORD_REFUSAL.into());
    }
    // Word 98's claim, the same way (`texture_native`).
    let texture = crate::texture_native::pending_texture(&layers, &comps);
    if has_texture && !texture {
        return Err(crate::texture_native::INVALID_WORD.into());
    }
    if texture && !has_texture {
        return Err(crate::texture_native::WORD_REFUSAL.into());
    }
    // Word 99's claim, the same way (`glow_gradient_native`).
    let glow_gradient = crate::glow_gradient_native::pending_glow_gradient(&layers, &comps);
    if has_glow_gradient && !glow_gradient {
        return Err(crate::glow_gradient_native::INVALID_WORD.into());
    }
    if glow_gradient && !has_glow_gradient {
        return Err(crate::glow_gradient_native::WORD_REFUSAL.into());
    }
    // Word 100's claim, the same way (`wide_color_native`).
    let wide = crate::wide_color_native::pending_wide(&layers, &comps);
    if has_wide && !wide {
        return Err(crate::wide_color_native::INVALID_WORD.into());
    }
    if wide && !has_wide {
        return Err(crate::wide_color_native::WORD_REFUSAL.into());
    }
    // Word 101's claim, the same way (`blend_if_channels_native`).
    let multichannel = crate::blend_if_channels_native::pending_multichannel(&layers, &comps);
    if has_multichannel && !multichannel {
        return Err(crate::blend_if_channels_native::INVALID_WORD.into());
    }
    if multichannel && !has_multichannel {
        return Err(crate::blend_if_channels_native::WORD_REFUSAL.into());
    }
    // Word 102's claim, one way only (`large_description_native`): a byte
    // meta has always been opaque up to the meta cap, so a large meta without
    // the word reads as it always did; the word without one is refused.
    if has_large && !crate::large_description_native::pending_large(&layers) {
        return Err(crate::large_description_native::INVALID_WORD.into());
    }
    // Word 103's claim, both ways (`gradient_smoothness_native`).
    let smooth = crate::gradient_smoothness_native::pending_smoothness(&layers, &comps);
    if has_smooth && !smooth {
        return Err(crate::gradient_smoothness_native::INVALID_WORD.into());
    }
    if smooth && !has_smooth {
        return Err(crate::gradient_smoothness_native::WORD_REFUSAL.into());
    }
    // Word 104's claim, both ways (`gradient_midpoint_native`).
    let midpoints = crate::gradient_midpoint_native::pending_midpoints(&layers, &comps);
    if has_midpoint && !midpoints {
        return Err(crate::gradient_midpoint_native::INVALID_WORD.into());
    }
    if midpoints && !has_midpoint {
        return Err(crate::gradient_midpoint_native::WORD_REFUSAL.into());
    }
    // Word 105's claim, both ways (`fill_align_native`).
    let align = crate::fill_align_native::pending_align(&layers);
    if has_align && !align {
        return Err(crate::fill_align_native::INVALID_WORD.into());
    }
    if align && !has_align {
        return Err(crate::fill_align_native::WORD_REFUSAL.into());
    }
    // Word 111's claim, both ways (`gradient_map_method_native`).
    let method = crate::gradient_map_method_native::byte_carries(&layers);
    if has_method && !method {
        return Err(crate::gradient_map_method_native::INVALID_WORD.into());
    }
    if method && !has_method {
        return Err(crate::gradient_map_method_native::WORD_REFUSAL.into());
    }
    if matches!(
        version,
        crate::native_sources::BYTE_SOURCE_VERSION
            | crate::native_sources::BYTE_LINK_VERSION
            | crate::native_sources::BYTE_RAW_VERSION
            | crate::native_sources::BYTE_RESOURCE_VERSION
            | crate::native_sources::BYTE_FILTER_VERSION
    ) && !has_ids
        && r.remaining() != 0
    {
        return Err("embedded source document has trailing bytes".into());
    }
    let layer_dims: Vec<(u32, u32)> = layer_planes.iter().map(|p| p.1).collect();
    sources.preflight_byte(&layers, &layer_dims, total_pixels)?;
    let source_profiles = sources.profiles()?;
    // All aggregate capacities, structure, masks, channel dimensions and tail
    // slots passed before the first PNG decode or raw mask allocation.
    for (layer, (png, dims, mask, mask_dims)) in layers.iter_mut().zip(layer_planes) {
        let pixels = image::load_from_memory_with_format(png, image::ImageFormat::Png)
            .map_err(|e| format!("failed to decode layer pixels: {e}"))?
            // One decoded buffer at a time (`drdc_reopen_plan`'s layer stage).
            .into_rgba8();
        if pixels.dimensions() != dims {
            return Err("decoded layer dimensions changed after preflight".to_string());
        }
        layer.pixels = LayerRaster::unseeded(pixels);
        layer.mask = mask
            .map(|raw| {
                GrayImage::from_raw(mask_dims.0, mask_dims.1, raw.to_vec())
                    .map(Arc::new)
                    .ok_or_else(|| "invalid layer mask".to_string())
            })
            .transpose()?;
    }
    for (channel, png) in channels.iter_mut().zip(channel_planes) {
        channel.data = Arc::new(
            image::load_from_memory_with_format(png, image::ImageFormat::Png)
                .map_err(|e| format!("failed to decode channel plane: {e}"))?
                .to_luma8(),
        );
    }
    sources.materialize_byte(&mut layers, &profile, source_profiles)?;
    if let Some(entries) = pending_warp {
        crate::warp_native::attach_byte(entries, &mut layers, &profile)?;
    }
    crate::doc_gen_variations::attach_byte(&mut layers, variations, &profile);
    // The vector-mask records, bound to the layers' final ids and to the
    // mask planes just built: each stored mask is the plane its record baked.
    let vector_masks = crate::doc_vector_mask_carry::adopted_records(&layers, paths.masks)?;
    let background = background_entry.map(|(index, role)| crate::doc_background::BackgroundMark {
        layer: layers[index].id,
        role,
    });
    Ok(DrDocument {
        width,
        height,
        layers,
        global_light,
        channels,
        guides,
        ruler_origin,
        color_samplers,
        paths: crate::doc_path::DocPaths {
            entries: Arc::new(paths.entries),
            vector_masks: Arc::new(vector_masks),
        },
        profile,
        metadata,
        resolution,
        linear_light,
        comps,
        background,
    })
}

/// Reads a slot written by [`put_opt_bytes`]; the cap error names the slot,
/// and the cap is checked BEFORE a byte is taken.
fn take_opt_bytes(r: &mut Reader<'_>, what: &str, cap: u32) -> Result<Option<Vec<u8>>, String> {
    if r.u8()? == 0 {
        return Ok(None);
    }
    let len = r.u32()?;
    capacity::slot_length(len as usize, cap, what)?;
    Ok(Some(r.take(len as usize)?.to_vec()))
}

/// Reads an optional string slot written by [`put_opt_string`]; the cap
/// error names the slot. Lenient on UTF-8 (lossy), like names and meta.
fn take_opt_string(r: &mut Reader<'_>, what: &str) -> Result<Option<String>, String> {
    Ok(
        take_opt_bytes(r, &format!("layer {what}"), MAX_DRDC_META_LEN)?
            .map(|b| String::from_utf8_lossy(&b).into_owned()),
    )
}

/// Read PNG dimensions without inflating its pixels. Match the decoder's
/// existing allocation limit while inspecting any auxiliary PNG chunks too.
fn png_dimensions(png: &[u8], what: &str) -> Result<(u32, u32), String> {
    let decoder = PngDecoder::with_limits(Cursor::new(png), image::Limits::default())
        .map_err(|e| format!("failed to inspect {what}: {e}"))?;
    capacity::decoded_bytes(decoder.total_bytes())?;
    Ok(decoder.dimensions())
}
