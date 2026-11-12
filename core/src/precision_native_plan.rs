//! One metadata-only typed native capacity plan for construction, writing and
//! hypothetical geometry/mask edits. Serialized allocations count individually.
//!
//! # Reopen decode scratch
//! Before the banded reopen (below), the decode term `finish_plan` charged
//! for the largest raster PNG was `native_png_decode_scratch`, 27 B/px (was
//! 32): the reopen peak measured on 17°-turned 5700×3800 placed images, with
//! the reader's figure at ≥ 1.2589× it. The record (method, peaks, envelopes
//! and the header slack) is `precision_png`'s module doc, beside the
//! constant. Each PNG term is the exact stored length
//! (`native_png_encoded_bound`), so no per-plane slack hides a header term
//! that counts short.
//!
//! # Banded reopen
//! The reader decodes each 16-bit plane row by row into tiles
//! (`precision_png_rows`), so the reopen held the file, the decoded planes and
//! one band (the file's term is now the held term, below). `checked_plan`
//! tracks, over the planes, `banded` (the band figure
//! of a 16-bit plane, an 8-bit original's whole-plane figure, since
//! `image::load_from_memory` still decodes it whole) and `whole` (HEAD's
//! whole-plane figure), and its decode scratch is [`reopen_scratch`]: base =
//! `reopen_retained + held + banded + bookkeeping` (`encoded_bound` in place
//! of `held` before the streamed reader), plus a quarter; the cap at HEAD's
//! figure was dropped (it existed only to admit work at 2 GiB), and a float
//! document keeps HEAD's figure exactly. The
//! reader admits the same function over its own inputs, which the plan's
//! dominate, so H ≥ A. The record (method, peaks, figures) is
//! `precision_png_rows`' module doc: every case's max E sits below its base,
//! and the 6000×4000 turned placed image's envelope falls from 2,047,871,054
//! to 1,259,995,018.
//!
//! RS0 and RS1 (the shapes below, the source original the largest plane)
//! were re-measured banded: A covers 1.2799× and 1.2748× their max E, so
//! RS's shortfall (1.17–1.18× at HEAD, below) is gone for a 16-bit original.
//! An 8-bit original still decodes whole at 27 B/px and was not re-measured
//! as the largest plane.
//!
//! # Float reopen
//! A float document's reopen held the file and the decoded tiles, ≈ 32 B/px,
//! against a figure (`retained + file + whole + bookkeeping`) of ≈ 32 B/px,
//! below 5/4 (with the streamed reader it holds the tiles alone,
//! ≈ 16 B/px, against `retained + held + whole + bookkeeping` plus the
//! margin; re-measured below). [`reopen_scratch`] adds
//! `FLOAT_REOPEN_MARGIN_PER_PIXEL` (9,
//! fitted: the smallest integer ≥ 8 covering ⌈1.25 × max E⌉ at both sizes; 8
//! misses by 2.7 MB at 24 MP) per float raster pixel, which the plan and the
//! reader sum over the same raster records. Measured with `precision_render`'s
//! `# Admission` method (E from before `open_native` of one
//! opaque float layer; A = the saved document's `reopen_retained_bytes +
//! decode_scratch_bytes`): 24 MP max E 771,834,456, A 986,110,324 (1.2776×);
//! 100 MP max E 3,209,873,688, A 4,102,110,324 (1.2780×).
//!
//! # Write term
//! `finish_plan`'s `write_scratch_bytes` was the file (`encoded_bound`), the
//! header, the largest writer transient (one row for a 16-bit plane) and
//! bookkeeping, plus a quarter: the banded writer
//! (`precision_png::put_native_png`) held nothing else beside the retained
//! tiles. The record (method, peaks, decision) is `precision_png`'s module
//! doc, `# Banded native write`. The file is now streamed and not held
//! (below).
//!
//! # Header and retained payloads counted twice
//! `finish_plan` charges header bytes (metadata, profiles, descriptors) and
//! retained payloads (a RAW original) in `reopen_retained` AND, through
//! the held term (`encoded_bound` before the streamed reader), in the decode
//! scratch. That is the reader's own figure: `parse` keeps every small take
//! (the header bytes, RAW and binary payloads among them; once the whole
//! file) alive until it returns, copies metadata (`Arc::from`) before any
//! layer PNG decodes, and copies RAW and binary payloads (`raw.materialize`)
//! in `decode_assets`, and its admission charges them in both `retained` and
//! `held_bytes`. Lowering the plan alone would let the writer produce files
//! the reader refuses, so the plan keeps it.
//! Measured (3 runs each, one case per process, E =
//! `/usr/bin/time -l` peak − `ri_phys_footprint` read just before
//! `open_native(path, REFERENCE_WORKING_BYTES, 0, None)`, bytes) on a 4000×3000
//! RGBA16 sRGB placed image (`retain`) without and with a 25,000,000-byte
//! xorshift RAW attached through `dr_precision_doc_attach_raw_source`, saved
//! with `save_native`; A is the reader's figure (from its refusal at a
//! probing `other_retained`), H the plan's `reopen_retained_bytes +
//! decode_scratch_bytes`. RL0, Rotate Canvas 17° (a 4704×4040 raster, the
//! largest plane): peaks 803,373,992 / 803,390,376 / 803,341,224 over pre
//! 1,245,472 / 1,261,856 / 1,245,472, max E 802,128,520, L 248,064,761, A
//! 1,015,522,379 (1.2660×), H 1,015,522,619; RL1, the same with the RAW:
//! peaks 853,443,544 / 853,427,160 / 853,443,544 over pre 1,245,472 /
//! 1,261,856 / 1,245,472, max E 852,198,072, L 273,065,040, A 1,065,584,354
//! (1.2504×), H 1,065,584,628; RS0, source `transform` [0.5, 0, 0, 0.5, 0,
//! 0] bicubic (a 2000×1500 raster, so the 4000×3000 source PNG is the
//! largest plane): peaks 482,575,032 / 482,722,488 / 482,443,960 over pre
//! 1,261,856 / 1,261,856 / 1,245,472, max E 481,460,632, L 120,019,144, A
//! 570,331,162 (1.1846×), H 570,331,370; RS1, the same with the RAW: peaks
//! 532,562,664 / 532,480,744 / 532,497,128 over pre 1,261,856 ×3, max E
//! 531,300,808, L 145,019,423, A 620,393,137 (1.1677×), H 620,393,379. The
//! baseline twin's peaks (1,311,008 / 1,278,240 / 1,294,624 and 1,278,240 /
//! 1,294,624 / 1,278,240) sit at pre. ΔE = E(with) − E(without) is
//! 50,069,552 for RL (2.0028× the payload) and 49,840,176 for RS (1.9936×):
//! the reader holds the RAW twice at its peak in both shapes, so the double
//! charge stays. (RS's A covers only 1.17–1.18× its peak: K = 27 was
//! measured with the layer raster as the largest plane.)
//!
//! # Caps
//! `checked_plan` refuses serialized planes past `MAX_DECODED_BYTES` and a
//! file bound past `MAX_NATIVE_BYTES`, both 8 GiB (once 1.6 GB and
//! 2 GiB; `precision_native`'s `# Size caps`). At an 8 GiB
//! working limit the reopen `admit` binds before either cap. With the
//! streamed reader (below) eight 100 MP 16-bit layers plan (a 6.4 GB file, a
//! reopen figure of 8,014,707,220 under the limit's 8,589,934,592) and a
//! ninth answers `too_large_to_reopen` (9,014,707,220); three 100 MP float
//! layers plan (7,506,291,504) and a fourth answers it. Before it, when the
//! figure charged the file, four 16-bit layers planned and a fifth was
//! refused. `tests/native_file_size.rs` holds the caps and the 8 GiB
//! answers.
//!
//! # Streamed reader and writer
//! The reader streams the file (`native_stream`): pixel payloads are spans,
//! recorded and read only when they decode, and every other byte is a small
//! take held until the parse returns. Its figure's `held` term is the bytes
//! `take` returned plus `NATIVE_STREAM_WINDOW_BYTES` (`Reader::held_bytes`,
//! the same over a slice); the plan's is `encoded_bound − streamed +
//! NATIVE_STREAM_WINDOW_BYTES`, `streamed` being every PNG bound, every f32
//! body (`w · h · 16`) and every mask or channel body (`2 · w · h`) it plans.
//! Because `encoded_bound − file ≥ streamed_planned − streamed_actual` (a
//! span's planned length is at least its actual one), H ≥ A. The writer
//! streams to the temporary file (`precision_native_write`): beside the
//! retained tiles it holds the pre-built tables and one scratch table (each
//! at most `header`), one writer transient and the sink's window, so
//! `write_scratch_bytes` = base + ⌈base/4⌉ with base = 2·header + transient +
//! `NATIVE_STREAM_WINDOW_BYTES` + bookkeeping, at both depths.
//!
//! Method (rules' admission method): Release through `make t`,
//! `MallocLargeCache=0`, one case per process, three runs and a baseline
//! twin; E = `ri_interval_max_phys_footprint` − pre after
//! `proc_reset_footprint_interval` (`RUSAGE_INFO_V4`), the result held until
//! E is read. A reopen's file is built and saved by a child process, so the
//! measured process never held the document (a freed document's tiles,
//! reused by the reopen, read E 99,057,736 for reopen16 in the same
//! process). H is the plan's figure; A is what the call admitted: a save's
//! `write_scratch_bytes`, a reopen's final reservation (`retained +
//! scratch`) read inside `precision_native::open_native_observed`'s hook.
//! `tests/native_file_size.rs`'s `stream_measure` (2026-10-11): base16,
//! save16, reopen16 a 6000×4000 16-bit document of two xorshift-noise
//! layers, a noise mask and a noise channel; base32, save32, reopen32 one
//! opaque formula float layer at 6000×4000, reopen32l at 10000×10000. Raw
//! output: `~/.claude/litecode/rasterize/lane-b-v1-rz-stream/measure/nfs/`.
//!
//! | case | H | A | E (three runs) | A / max E |
//! | --- | --- | --- | --- | --- |
//! | base16 | — | — | 0 ×3 | — |
//! | save16 | 11,863,460 | 11,863,460 | 1,261,568 ×3 | 9.40 |
//! | reopen16 | 614,083,830 | 614,083,328 | 482,705,768 / 482,722,152 / 482,689,384 | 1.2721 |
//! | base32 | — | — | 0 ×3 | — |
//! | save32 | 6,560,415 | 6,560,415 | 1,179,648 / 1,163,264 / 1,163,264 | 5.56 |
//! | reopen32 | 606,304,628 | 606,304,332 | 388,546,848 ×2 / 388,563,232 | 1.5604 |
//! | reopen32l | 2,506,304,628 | 2,506,304,332 | 1,609,974,912 / 1,609,991,296 ×2 | 1.5567 |
//!
//! Every case holds H ≥ A ≥ ⌈1.25 × max E⌉, so `NATIVE_STREAM_WINDOW_BYTES`
//! stays 4 MiB and `FLOAT_REOPEN_MARGIN_PER_PIXEL` stays 9 (raise-only: it is
//! shared with the byte reopen plan). The 16-bit reopen's E is the decoded
//! planes (480,000,000) plus 2.7 MB; the float reopen's the tiles plus
//! 4.5 MB (24 MP) and 9.99 MB (100 MP). `float_native`'s module doc holds
//! the placed-master shapes (`float_master_24mp.rs`), and
//! `float_above_cache_bound.rs`'s `O` floors are reopen32's and reopen32l's.
//!
//! `precision_native_plan.rs` owns hypothetical and actual raster/mask/channel capacity.
use crate::drdc::{
    MAX_DRDC_NAME_LEN, MAX_DRDC_PNG_LEN, MAX_DRDC_TOTAL_CHANNEL_PIXELS, MAX_DRDC_TOTAL_MASK_PIXELS,
};
use crate::native_stream::NATIVE_STREAM_WINDOW_BYTES;
use crate::precision_adjustment::AdjustmentAdmission;
use crate::precision_codec::{
    native_png_band_scratch, native_png_decode_scratch, native_png_encode_scratch,
};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    admit, gib_text, pixel_count, process_working_limit, BOOKKEEPING_BYTES,
};
use crate::precision_native::{
    plane_capacity, PrecisionNativePlan, MAX_DECODED_BYTES, MAX_NATIVE_BYTES,
};

/// Serialized raster extents followed by independent mask extents.
pub(crate) type NativeDimensions = (Vec<(u32, u32)>, Vec<(u32, u32)>);
pub(crate) fn scalar_capacity(dimensions: (u32, u32), total: &mut u64) -> Result<u64, String> {
    let pixels = pixel_count(dimensions.0, dimensions.1)? as u64;
    *total = total
        .checked_add(pixels * 2)
        .ok_or("typed native scalar byte overflow")?;
    if *total > MAX_DECODED_BYTES {
        return Err("high-depth native serialized pixels exceed the shared byte limit".into());
    }
    Ok(pixels)
}
impl PrecisionDocument {
    pub(crate) fn native_dimensions(&self) -> NativeDimensions {
        fn walk(
            nodes: &[PrecisionDocumentLayer],
            r: &mut Vec<(u32, u32)>,
            m: &mut Vec<(u32, u32)>,
        ) {
            for node in nodes {
                if let Some(mask) = &node.mask {
                    m.push(mask.plane.dimensions());
                }
                match &node.content {
                    Content::Raster { image, .. } => r.push(image.dimensions()),
                    Content::CmykRaster { image, .. } => r.push(image.dimensions()),
                    Content::Group(c) | Content::PassThrough(c) => walk(c, r, m),
                    _ => {}
                }
            }
        }
        let (mut rasters, mut masks) = (Vec::new(), Vec::new());
        walk(&self.layers, &mut rasters, &mut masks);
        // A live vector mask's raster part is one more mask plane of its
        // record (`precision_vector_mask`), stored in the PATH table.
        masks.extend(
            self.vector_mask_raster_parts()
                .iter()
                .map(|(_, part)| part.plane.dimensions()),
        );
        (rasters, masks)
    }
    pub(crate) fn native_preflight(&self) -> Result<PrecisionNativePlan, String> {
        for channel in self.channels() {
            channel.metadata().validate()?;
        }
        for index in 0..self.layer_count() {
            let layer = self.layer(index).ok_or("invalid typed layer")?;
            crate::precision_layer_blend::validate(layer.blend_mode(), layer.blend_target())?;
            crate::precision_blend_if::validate(layer.blend_if(), self.is_cmyk())?;
            crate::precision_style::validate(
                layer.style().map(|s| &**s),
                layer.style_plane(self.dimensions()),
            )?;
            if let Some(text) = self.artwork_description(index) {
                crate::artwork_document::validate(text)?;
                let layer = self.layer(index).ok_or("invalid artwork layer")?;
                // A CMYK raster carries one too (#1's artwork batch).
                if (layer.source().is_none() && layer.cmyk_source().is_none())
                    || layer.retained_source.is_some()
                {
                    return Err("artwork descriptions require ordinary raster layers".into());
                }
            }
        }
        crate::native_sources::precision_sources(self)?.validate_fallbacks(|index| {
            let layer = self.layer(index)?;
            Some((layer.source()?.dimensions(), layer.offset()))
        })?;
        let (r, m) = self.native_dimensions();
        self.native_geometry_plan(
            self.dimensions(),
            &r,
            &m,
            self.channels.len(),
            false,
            0,
            self.is_float(),
        )
    }
    /// Geometry supplies every eventual serialized raster/mask extent, including
    /// hidden/shared records, before allocating destination pixels. Channels use
    /// the new canvas dimensions. Unknown new channel names use the full cap.
    pub(crate) fn native_fits_geometry(
        &self,
        canvas: (u32, u32),
        rasters: &[(u32, u32)],
        masks: &[(u32, u32)],
        channel_count: usize,
    ) -> Result<(), String> {
        self.native_geometry_plan(
            canvas,
            rasters,
            masks,
            channel_count,
            true,
            0,
            self.is_float(),
        )
        .map(|_| ())
    }
    /// [`Self::native_fits_geometry`] with these records charged as a 32-bit
    /// float document's (`precision_conversion_float`): raw 16 B/px rasters
    /// against `MAX_DECODED_BYTES`, the float header and the float reopen
    /// against the process working limit. Its refusals are the plan's own
    /// sentences (`too_large_to_reopen(true)`, `too_large_to_save(true)`).
    pub(crate) fn native_fits_geometry_as_float(
        &self,
        canvas: (u32, u32),
        rasters: &[(u32, u32)],
        masks: &[(u32, u32)],
        channel_count: usize,
    ) -> Result<(), String> {
        self.native_geometry_plan(canvas, rasters, masks, channel_count, true, 0, true)
            .map(|_| ())
    }
    pub(crate) fn preflight_raster_addition(
        &self,
        dimensions: (u32, u32),
        name: &str,
    ) -> Result<(), String> {
        crate::precision_document::scalar_and_name(name, 1.0)?;
        if self.records >= crate::precision_render::MAX_RECORDS {
            return Err("too many high-depth document records".into());
        }
        let (mut r, m) = self.native_dimensions();
        r.push(dimensions);
        self.native_geometry_plan(
            self.dimensions(),
            &r,
            &m,
            self.channels.len(),
            false,
            37 + name.len() as u64,
            self.is_float(),
        )
        .map(|_| ())
    }
    /// `float` charges the records as a 32-bit float document's; every caller
    /// but [`Self::native_fits_geometry_as_float`] passes `self.is_float()`.
    #[allow(clippy::too_many_arguments)]
    fn native_geometry_plan(
        &self,
        canvas: (u32, u32),
        rasters: &[(u32, u32)],
        masks: &[(u32, u32)],
        channel_count: usize,
        hypothetical: bool,
        extra_header: u64,
        float: bool,
    ) -> Result<PrecisionNativePlan, String> {
        pixel_count(canvas.0, canvas.1)?;
        if rasters.len() > crate::precision_render::MAX_RECORDS
            || masks.len() > crate::precision_render::MAX_RECORDS
            || channel_count > crate::doc_channel::MAX_CHANNELS
        {
            return Err("typed native mask/channel count exceeds its limit".into());
        }
        let outside_count = if hypothetical {
            masks.len()
        } else {
            crate::mask_outside_native::precision_indices(self).len()
        };
        let clipped_count = if hypothetical {
            self.records
        } else {
            crate::precision_clip_native::precision_indices(self).len()
        };
        let blend_if_count = if hypothetical {
            self.records
        } else {
            crate::precision_blend_if_native::precision_entries(self).len()
        };
        let label_count = if hypothetical {
            self.records
        } else {
            crate::label_native::precision_entries(self).len()
        };
        // A hypothetical plan (geometry) may move every record, so it
        // charges a reference point for each.
        let reference_count = if hypothetical {
            self.records
        } else {
            crate::reference_native::precision_entries(self).len()
        };
        // A hypothetical plan (geometry) charges the actual style entries
        // plus 128 bytes per styled record for Scale Effects re-quantization.
        let style_entries = crate::precision_style_native::precision_entries(self);
        let style_header = crate::precision_style_native::header_bytes(&style_entries)
            + if hypothetical {
                style_entries.len() as u64 * 128
            } else {
                0
            };
        // The comp payload is built ONCE, exactly as the writer builds it,
        // so the plan charges the bytes that will actually be written.
        let comps_payload = crate::comps_native::payload(self.layer_comps(), |id| {
            Some(self.layer(self.layer_index_by_id(id)?)?.name().to_string())
        })?;
        // The guide table's payload is built ONCE, exactly as the writer
        // builds it (the comps precedent), so the plan charges the bytes
        // that will actually be written.
        let guide_entries = crate::precision_guide_native::precision_entries(self);
        let mut header = crate::identity_native::header_bytes(self.records)
            + crate::precision_guide_native::header_bytes(guide_entries.as_ref())
            + crate::precision_lock_native::header_bytes(
                crate::precision_lock_native::precision_entries(self).len(),
            )
            + crate::precision_group_open_native::header_bytes(
                crate::precision_group_open_native::precision_entries(self).len(),
            )
            + crate::precision_light_native::header_bytes(self.global_light())
            + crate::levels_native::header_bytes(crate::levels_native::precision_carries(self))
            + crate::curves_native::header_bytes(crate::curves_native::precision_carries(self))
            + crate::look_native::header_bytes(crate::look_native::precision_carries(self))
            + crate::comps_native::header_bytes(comps_payload.len() as u64)
            + crate::label_native::header_bytes(label_count)
            + crate::reference_native::header_bytes(reference_count)
            + crate::spot_channel_native::header_bytes(
                crate::spot_channel_native::precision_entries(self).len(),
            )
            + crate::background_native::header_bytes(
                crate::background_native::precision_entry(self),
            )
            + crate::float_native::header_bytes(float)
            + crate::cmyk_native::header_bytes(self.cmyk_policy().map(|p| &**p))
            + crate::gray_native::header_bytes(self.gray_policy().map(|p| &**p))
            + crate::duotone_native::header_bytes(
                self.gray_policy().and_then(|p| p.duotone()).map(|d| &**d),
            )
            + crate::lab_native::header_bytes(self.lab_policy().map(|p| &**p))
            + crate::indexed_native::header_bytes(self.indexed_policy().map(|p| &**p))
            + crate::bitmap_native::header_bytes(self.bitmap_policy().map(|p| &**p))
            // A float document is linear light by construction, so charged
            // as one it carries the policy's table even when this one is
            // encoded (unchanged for every caller passing `is_float()`).
            + crate::evaluation_native::header_bytes(
                float || self.evaluation() == crate::doc_evaluation::Evaluation::LinearLight,
            )
            + crate::mask_outside_native::header_bytes(outside_count)
            + crate::precision_clip_native::header_bytes(clipped_count)
            // Schema 2's bound when some record holds a multichannel Blend
            // If (`blend_if_channels_native`; geometry never changes one).
            + crate::precision_blend_if_native::header_bytes(
                blend_if_count,
                crate::blend_if_channels_native::styles_multichannel16(self),
            )
            + style_header
            // Word 50 has NO table: four bytes for the version word, and only
            // when some stored style carries a key (`precision_style_knockout`).
            + if crate::precision_style_knockout::styles_carry_keys16(self) {
                4
            } else {
                0
            }
            + crate::precision_blend_native::header_bytes(self)
            // Word 90 has NO table (`style_pattern`).
            + if crate::style_pattern::styles_pattern16(self) {
                4
            } else {
                0
            }
            // Word 93 has NO table (`pattern_angle_native`).
            + if crate::pattern_angle_native::styles_angle16(self) {
                4
            } else {
                0
            }
            // Word 94 has NO table (`gradient_options_native`).
            + if crate::gradient_options_native::styles_options16(self) {
                4
            } else {
                0
            }
            // Word 95 has NO table (`stroke_pattern_native`).
            + if crate::stroke_pattern_native::styles_stroke_pattern16(self) {
                4
            } else {
                0
            }
            // Word 96 has NO table (`contour_native`).
            + if crate::contour_native::styles_contour16(self) {
                4
            } else {
                0
            }
            // Word 97 has NO table (`noise_native`).
            + if crate::noise_native::styles_noise16(self) {
                4
            } else {
                0
            }
            // Word 111 has NO table (`gradient_map_method_native`).
            + if crate::gradient_map_method_native::precision_carries(self) {
                4
            } else {
                0
            }
            // Word 105 has NO table (`fill_align_native`).
            + if crate::fill_align_native::descriptions_align16(self) {
                4
            } else {
                0
            }
            // Word 104 has NO table (`gradient_midpoint_native`).
            + if crate::gradient_midpoint_native::styles_midpoints16(self) {
                4
            } else {
                0
            }
            // Word 103 has NO table (`gradient_smoothness_native`).
            + if crate::gradient_smoothness_native::styles_smoothness16(self) {
                4
            } else {
                0
            }
            // Word 102 has NO table (`large_description_native`).
            + if crate::large_description_native::descriptions_large16(self) {
                4
            } else {
                0
            }
            // Word 101 has NO table of its own (`blend_if_channels_native`;
            // its ranges are in the LBIF bound above).
            + if crate::blend_if_channels_native::styles_multichannel16(self) {
                4
            } else {
                0
            }
            // Word 100 has NO table (`wide_color_native`).
            + if crate::wide_color_native::styles_wide16(self) {
                4
            } else {
                0
            }
            // Word 99 has NO table (`glow_gradient_native`).
            + if crate::glow_gradient_native::styles_glow_gradient16(self) {
                4
            } else {
                0
            }
            // Word 98 has NO table (`texture_native`).
            + if crate::texture_native::styles_texture16(self) {
                4
            } else {
                0
            }
            // Word 89 has NO table: four bytes, only for a NON-CMYK document
            // holding a stacked style (`style_instances`; a CMYK document's
            // rung 88 is its CMYK word, already counted).
            + if self.cmyk_policy().is_none() && crate::style_instances::styles_stacked16(self) {
                4
            } else {
                0
            }
            + 32
            + 4
            + self.profile.bytes().len() as u64
            + 8
            + 3
            + extra_header
            + 4;
        for b in [&self.metadata.exif, &self.metadata.xmp, &self.metadata.iptc]
            .into_iter()
            .flatten()
        {
            header += 4 + b.len() as u64;
        }
        fn nodes(
            layers: &[PrecisionDocumentLayer],
            profile: &std::sync::Arc<crate::icc::IccProfile>,
            header: &mut u64,
            admission: &mut AdjustmentAdmission,
        ) -> Result<(), String> {
            for layer in layers {
                *header += 21 + layer.name.len() as u64; // common record and mask flag
                match &layer.content {
                    Content::Raster { .. } | Content::CmykRaster { .. } => *header += 16,
                    // A CMYK record's retention and its preparation, the
                    // reader's own charge (`AdjustmentAdmission::record_bytes`).
                    Content::CmykAdjustment(state) => {
                        *header += 4 + state.description().len() as u64;
                        admission.prepared = admission
                            .prepared
                            .checked_add(state.retained_bytes())
                            .ok_or("native adjustment retention overflow")?;
                        admission.peak = admission.peak.max(
                            crate::cmyk_adjust_layer::CmykAdjustmentState::prepare_scratch_bytes(
                                state.description(),
                            ),
                        );
                    }
                    Content::Adjustment(state) => {
                        *header += 4 + state.description().len() as u64;
                        admission.plan_add(state, profile)?;
                    }
                    Content::Group(c) | Content::PassThrough(c) => {
                        *header += 4;
                        nodes(c, profile, header, admission)?;
                    }
                }
            }
            Ok(())
        }
        let mut admission = AdjustmentAdmission::default();
        nodes(&self.layers, &self.profile, &mut header, &mut admission)?;
        header += masks.len() as u64 * 36;
        for index in 0..channel_count {
            header += 12
                + if hypothetical {
                    u64::from(MAX_DRDC_NAME_LEN)
                } else {
                    self.channels[index].name.len() as u64
                };
        }
        header += if hypothetical {
            // New/imported channels may carry one maximum-sized descriptor.
            12 + channel_count as u64
                * (12 + crate::precision_channel_metadata::MAX_PROVENANCE_BYTES as u64)
        } else {
            crate::precision_channel_metadata::header_bytes(self)
        };
        let sources = crate::native_sources::precision_sources(self)?;
        let warp = crate::warp_native::typed_capacity(self)?;
        let artwork = crate::artwork_native::header_bytes(self);
        header += crate::source_transform_native::encoded_len(
            crate::source_transform_native::entries(&sources.bindings).len(),
        ) + if crate::source_transform_native::entries(&sources.bindings).is_empty() {
            0
        } else {
            4
        };
        // Word 67 (`float_native`): four bytes and no table.
        if crate::float_native::masters_present(&sources.assets) {
            header += 4;
        }
        // Word 78 (`float_native`): four bytes and no table.
        if crate::float_native::raw_masters_present(&sources.assets) {
            header += 4;
        }
        // Word 79 (`raw_locals_native`): four bytes and no table.
        header += crate::raw_locals_native::header_bytes(crate::raw_locals_native::carries(
            sources.assets.iter().map(|a| &**a),
        ));
        let refusals = crate::source_refusal_native::entries(&sources.assets);
        header += crate::source_refusal_native::encoded_len(&refusals)
            + if refusals.is_empty() { 0 } else { 4 };
        // Word 60 and its CSMP table (`sampler_native`). A geometry plan's
        // hypothetical can only drop samplers, so the current list bounds it.
        header += crate::sampler_native::encoded_len(&self.color_samplers)
            + if self.color_samplers.is_empty() { 0 } else { 4 };
        // Word 75 and its PATH table (`path_native`), by the table's bound,
        // and beside it what building or parsing the table holds. Both
        // depend only on counts and name lengths, which no geometry edit
        // changes, so the current list bounds a geometry plan's hypothetical.
        // The table's own bytes are charged by its JSON records alone
        // (sample size 0): the raster-part planes are mask planes of
        // `masks`, which `finish_plan` charges with every other. The
        // transient is charged WITH the planes: the writer builds the whole
        // table, planes included, beside the file.
        // Word 92 (a CMYK document's too) and its GVAR table
        // (`generated_native`): the table's bytes are held by the
        // writer and by the reader, so the header charges them, which covers
        // the encoded bound, the reopen's held and retained terms and the
        // write scratch.
        let variations = crate::generated_native::encoded_len(&self.generated_entries());
        header += variations + if variations == 0 { 0 } else { 4 };
        let vector_masks = self.vector_mask_table();
        let path_table = crate::path_native::encoded_bound(&self.paths.entries, &vector_masks, 0);
        header += path_table
            + crate::path_native::transient_bound(&self.paths.entries, &vector_masks, 2)
            + if path_table == 0 { 0 } else { 4 };
        header += sources.header_bytes()
            + if matches!(
                sources.version(),
                crate::native_sources::PRECISION_RESOURCE_VERSION
                    | crate::native_sources::PRECISION_FILTER_VERSION
            ) {
                // v22's source tail already owns its schema prefix; ADSC is present
                // even with zero descriptions, so only its eight-byte framing remains.
                artwork.max(12) - 4
            } else {
                artwork
            };
        header = header
            .checked_add(warp.0)
            .ok_or("WARP native header capacity overflow")?;
        let originals: Vec<_> = sources
            .assets
            .iter()
            .map(|a| (a.dimensions(), a.depth() as u8))
            .collect();
        // Source profiles may differ from the document's profile. Header bytes
        // count their serialized blobs once; three extra copies conservatively
        // account parsed TRCs just as for the captured document profile.
        finish_plan(
            canvas,
            (rasters, &originals),
            masks,
            channel_count,
            header,
            admission.total()? + sources.extra_nonpixel_retained(),
            self.profile.bytes().len(),
            float,
            warp.1,
            self.is_cmyk(),
        )
    }
}

/// A one-raster conversion uses the SAME native plan before allocating pixels.
/// The fixed header counts the default `Background` raster; the `channel_count`
/// saved channels the conversion carries are counted as canvas-sized planes.
pub(crate) fn flattened_plan(
    dimensions: (u32, u32),
    profile: &crate::icc::IccProfile,
    metadata: &crate::metadata::Metadata,
    channel_count: usize,
) -> Result<PrecisionNativePlan, String> {
    use crate::drdc::MAX_DRDC_BLOB_LEN;
    if profile.bytes().len() > MAX_DRDC_BLOB_LEN as usize {
        return Err("high-depth profile exceeds the native byte limit".into());
    }
    let mut header = 32 + 4 + profile.bytes().len() as u64 + 8 + 3 + 4 + 37 + 10;
    for bytes in [&metadata.exif, &metadata.xmp, &metadata.iptc]
        .into_iter()
        .flatten()
    {
        if bytes.len() > MAX_DRDC_BLOB_LEN as usize {
            return Err("high-depth metadata packet exceeds the native limit".into());
        }
        header += 4 + bytes.len() as u64;
    }
    finish_plan(
        dimensions,
        (&[dimensions], &[]),
        &[],
        channel_count,
        header,
        0,
        profile.bytes().len(),
        false,
        0,
        false,
    )
}

type NativeColorPlanes<'a> = (&'a [(u32, u32)], &'a [((u32, u32), u8)]);

/// The plan of `rasters` (layer planes, 16-bit or, with `float`, raw f32)
/// beside embedded `originals` (extent, depth 8, 16 or 32), with no header,
/// masks or channels, under the process working limit. A black-box seam: a
/// placed-image shape near the plan's limits cannot otherwise be reached
/// without allocating it.
#[doc(hidden)]
pub fn native_color_plan(
    canvas: (u32, u32),
    rasters: &[(u32, u32)],
    originals: &[((u32, u32), u8)],
    float: bool,
) -> Result<PrecisionNativePlan, String> {
    finish_plan(
        canvas,
        (rasters, originals),
        &[],
        0,
        0,
        0,
        0,
        float,
        0,
        false,
    )
}

/// The float reopen's margin per float raster pixel: fitted so the figure
/// covers the measured peak (the file plus the decoded tiles, ≈ 32 B/px) at
/// 5/4 (module doc, `# Float reopen`).
pub(crate) const FLOAT_REOPEN_MARGIN_PER_PIXEL: u64 = 9;

/// The reopen's scratch beside `retained` (`# Banded reopen`):
/// base = `retained + held + banded + BOOKKEEPING_BYTES`, the whole-plane
/// figure = `retained + held + whole + BOOKKEEPING_BYTES`, where `held` is
/// what the streamed reader holds beside the decoded planes (its small takes
/// and its window, `native_stream`; once the whole file); the figure is
/// the whole-plane one plus `float_raster_pixels ·
/// FLOAT_REOPEN_MARGIN_PER_PIXEL` for a float document (unbanded), else
/// `base + ⌈base/4⌉`, with no cap at the whole-plane figure (the 16-bit arm
/// ignores `float_raster_pixels`).
/// Returns figure − `retained`. Monotone in every input, so the plan's figure
/// (built from inputs that dominate the reader's) covers the reader's.
pub(crate) fn reopen_scratch(
    retained: u64,
    held: u64,
    banded: u64,
    whole: u64,
    float: bool,
    float_raster_pixels: u64,
    cmyk: bool,
) -> Result<u64, String> {
    const OVERFLOW: &str = "typed native reopen scratch overflow";
    let figure = |decode: u64| {
        retained
            .checked_add(held)
            .and_then(|v| v.checked_add(decode))
            .and_then(|v| v.checked_add(BOOKKEEPING_BYTES))
            .ok_or(OVERFLOW)
    };
    // A CMYK document's raw rasters take the float figure's shape with
    // their own margin (`cmyk_native::REOPEN_MARGIN_PER_PIXEL`).
    let margin = if cmyk {
        crate::cmyk_native::REOPEN_MARGIN_PER_PIXEL
    } else {
        FLOAT_REOPEN_MARGIN_PER_PIXEL
    };
    let figure = if float || cmyk {
        float_raster_pixels
            .checked_mul(margin)
            .and_then(|margin| figure(whole).ok()?.checked_add(margin))
            .ok_or(OVERFLOW)?
    } else {
        let base = figure(banded)?;
        base.checked_add(base.div_ceil(4)).ok_or(OVERFLOW)?
    };
    Ok(figure - retained)
}

/// The reopen `admit`'s refusal: the plan alone reaches the whole
/// working limit, so closing other documents cannot help.
pub(crate) fn too_large_to_reopen(float: bool) -> String {
    format!(
        "At {} bits the result needs more than the {} GB working limit to save and reopen on its own, so closing other documents will not help. Use a smaller image or fewer layers, masks or channels.",
        if float { 32 } else { 16 },
        gib_text(process_working_limit())
    )
}

/// Every fixed-cap refusal of the plan.
pub(crate) fn too_large_to_save(float: bool) -> String {
    format!(
        "At {} bits the result is too large to save and reopen on its own, so closing other documents will not help. Use a smaller image or fewer layers, masks or channels.",
        if float { 32 } else { 16 }
    )
}

/// [`checked_plan`] with every refusal mapped to one of the two plain
/// sentences: the reopen admit's own, else [`too_large_to_save`].
#[allow(clippy::too_many_arguments)]
fn finish_plan(
    canvas: (u32, u32),
    planes: NativeColorPlanes<'_>,
    masks: &[(u32, u32)],
    channel_count: usize,
    header: u64,
    preparation: u64,
    profile_bytes: usize,
    float: bool,
    warp_originals: u64,
    cmyk: bool,
) -> Result<PrecisionNativePlan, String> {
    checked_plan(
        canvas,
        planes,
        masks,
        channel_count,
        header,
        preparation,
        profile_bytes,
        float,
        warp_originals,
        cmyk,
    )
    .map_err(|e| {
        if e == too_large_to_reopen(float) {
            e
        } else {
            too_large_to_save(float)
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn checked_plan(
    canvas: (u32, u32),
    (rasters, originals): NativeColorPlanes<'_>,
    masks: &[(u32, u32)],
    channel_count: usize,
    header: u64,
    preparation: u64,
    profile_bytes: usize,
    float: bool,
    warp_originals: u64,
    cmyk: bool,
) -> Result<PrecisionNativePlan, String> {
    let (mut decoded, mut raw, mut mask_pixels) = (0, 0, 0);
    for dims in masks {
        let n = scalar_capacity(*dims, &mut decoded)?;
        raw += 2 * n;
        mask_pixels += n;
    }
    if mask_pixels > MAX_DRDC_TOTAL_MASK_PIXELS {
        return Err("typed masks exceed the shared native pixel limit".into());
    }
    let channel_pixels = u64::from(canvas.0) * u64::from(canvas.1) * channel_count as u64;
    if channel_pixels > MAX_DRDC_TOTAL_CHANNEL_PIXELS {
        return Err("typed channels exceed the shared native pixel limit".into());
    }
    for _ in 0..channel_count {
        let n = scalar_capacity(canvas, &mut decoded)?;
        raw += 2 * n;
    }
    let (mut encoded, mut banded, mut whole, mut transient) = (0_u64, 0_u64, 0_u64, 0_u64);
    let mut float_raster_pixels = 0_u64;
    // The span bodies the reader records and never holds (`native_stream`):
    // every mask and channel body, each f32 body and each PNG bound. A
    // vector mask's raster-part plane (in `masks`) is the one mask body the
    // reader takes rather than spans (`path_native::parse`); `header` covers
    // it, since it charges the PATH table's `transient_bound` at two bytes a
    // sample, which counts every such plane twice.
    let mut streamed = raw;
    let rasters: &[(u32, u32)] = if float {
        // A float raster is RAW: `w · h · 16` bytes serialized and the same
        // again retained, with no codec bound and no codec scratch on either
        // side (`float_native`). Only the layer rasters: an embedded 8/16-bit
        // master is a PNG at every document depth (`native_sources_assets`),
        // so it takes the PNG terms below, as the reader's `inspect_pixels`
        // charges it. A 32-bit float master is raw too (below).
        for dimensions in rasters {
            let bytes = crate::float_native::raster_capacity(*dimensions, &mut decoded)?;
            encoded += 16 + bytes;
            streamed += bytes;
            float_raster_pixels += u64::from(dimensions.0) * u64::from(dimensions.1);
        }
        &[]
    } else if cmyk {
        // A CMYK raster is RAW too: `w · h · 10` bytes serialized and
        // retained, no codec bound or scratch (`cmyk_native`); its reopen is
        // charged on the float figure's terms (the decode holds one row
        // beside the tiles).
        for dimensions in rasters {
            let bytes = crate::cmyk_native::raster_capacity(*dimensions, &mut decoded)?;
            encoded += 16 + bytes;
            streamed += bytes;
            float_raster_pixels += u64::from(dimensions.0) * u64::from(dimensions.1);
        }
        &[]
    } else {
        rasters
    };
    for (dimensions, depth) in rasters
        .iter()
        .map(|d| (*d, 16))
        .chain(originals.iter().copied())
    {
        if depth == 32 {
            // A 32-bit float master is charged as a float
            // raster: `w · h · 16` raw bytes serialized and retained, no
            // codec bound, transient or decode scratch. Its u64 length word
            // is already in the source table's `header_bytes`.
            let bytes = crate::float_native::raster_capacity(dimensions, &mut decoded)?;
            encoded += bytes;
            streamed += bytes;
            float_raster_pixels += u64::from(dimensions.0) * u64::from(dimensions.1);
            continue;
        }
        if depth == 16 {
            plane_capacity(dimensions, &mut decoded)?;
        } else {
            crate::native_sources::source_plane_capacity(
                dimensions,
                depth,
                &mut decoded,
                MAX_DECODED_BYTES,
            )?;
        }
        let bound = crate::native_sources::source_png_encoded_bound(dimensions, depth)?;
        if bound > u64::from(MAX_DRDC_PNG_LEN) {
            return Err("high-depth native PNG bound exceeds the record limit".into());
        }
        // The banded writer's row (16-bit planes) or the 8-bit original's
        // encode, beside the file.
        transient = transient.max(if depth == 16 {
            native_png_encode_scratch(dimensions)?
        } else {
            crate::native_sources::byte_source_png_encode_scratch(dimensions)?
        });
        // The banded reader's rows for a 16-bit plane; an 8-bit original
        // still decodes whole.
        let decode = native_png_decode_scratch(dimensions)?;
        whole = whole.max(decode);
        banded = banded.max(if depth == 16 {
            native_png_band_scratch(dimensions)?
        } else {
            decode
        });
        encoded += bound;
        streamed += bound;
    }
    decoded = decoded
        .checked_add(warp_originals)
        .ok_or("WARP decoded capacity overflow")?;
    if decoded > MAX_DECODED_BYTES {
        return Err("WARP originals and fallback exceed native capacity".into());
    }
    let serialized_pixel_bytes = decoded;
    let encoded_bound = header + raw + encoded;
    if encoded_bound > MAX_NATIVE_BYTES {
        return Err("high-depth native file exceeds the encoded size bound".into());
    }
    // Header bytes (a RAW payload among them) are charged here AND in the
    // held term below on purpose: the reader holds them twice, its small
    // takes and their copies (the module doc's record).
    let reopen_retained =
        decoded + header + 3 * profile_bytes as u64 + BOOKKEEPING_BYTES + preparation;
    // What the streamed reader holds (`native_stream`): every byte but the
    // span bodies, and its window. `encoded_bound − streamed` dominates the
    // reader's small takes, since each span's planned length is at least
    // its actual one.
    let held = (encoded_bound - streamed)
        .checked_add(NATIVE_STREAM_WINDOW_BYTES)
        .ok_or("typed native reopen scratch overflow")?;
    let decode_scratch_bytes = reopen_scratch(
        reopen_retained,
        held,
        banded,
        whole,
        float,
        float_raster_pixels,
        cmyk,
    )?;
    admit(
        reopen_retained,
        decode_scratch_bytes,
        0,
        process_working_limit(),
    )
    .map_err(|_| too_large_to_reopen(float))?;
    // The streamed save holds, beside the retained tiles, the
    // pre-built header tables and one scratch table (each at most `header`),
    // one writer transient (a 16-bit plane's row, an 8-bit original's encode;
    // 0 for a float raster) and the sink's window, plus bookkeeping; a
    // quarter above that, at both depths (the module doc's record).
    let base = header
        .checked_mul(2)
        .and_then(|v| v.checked_add(transient))
        .and_then(|v| v.checked_add(NATIVE_STREAM_WINDOW_BYTES + BOOKKEEPING_BYTES))
        .ok_or("typed native write scratch overflow")?;
    let write_scratch_bytes = base + base.div_ceil(4);
    Ok(PrecisionNativePlan {
        encoded_bound,
        serialized_pixel_bytes,
        reopen_retained_bytes: reopen_retained,
        decode_scratch_bytes,
        write_scratch_bytes,
    })
}
