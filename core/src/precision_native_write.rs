//! The typed `.rz` writer: `precision_native`'s format, written in
//! order to a [`NativeSink`] — the temporary file `save_native` renames into
//! place, or the caller's `Vec` in `encode_native` — so the encoded file is
//! never assembled in memory. Its admission (`admit_native_write`) runs
//! before the temporary file exists; its figure is the plan's
//! `write_scratch_bytes` (`precision_native_plan`'s `# Streamed reader and
//! writer`).

use crate::job_control::JobContext;
use crate::native_stream::NativeSink;
use crate::precision_codec::put_native_png;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::admit;

use super::{PrecisionNativePlan, EVALUATION_TAG, STORAGE_TAG};

/// The table scratch's up-front capacity: one `append` chunk.
const TABLE_SCRATCH_BYTES: usize = 64 * 1024;

impl PrecisionDocument {
    /// The typed save's admission, before any byte is written (and before
    /// `save_atomically` creates the temporary file): the plan, then the
    /// document's retained bytes beside its `write_scratch_bytes`, which the
    /// job reserves.
    pub(crate) fn admit_native_write(
        &self,
        limit: u64,
        other_retained: u64,
        job: JobContext<'_>,
    ) -> Result<PrecisionNativePlan, String> {
        job.check()?;
        let plan = self.native_preflight()?;
        // The writer enforces the reader's GRAY checks (`gray_native::check`:
        // 32 ⇔ float, body profile = the twin), so a drifted Gray document
        // refuses its save by name instead of writing a file it cannot reopen.
        if let Some(policy) = self.gray_policy() {
            crate::gray_native::check(policy, self.is_float(), self.profile.bytes())?;
            // …and the reader's DUOT checks (`duotone_native::check`: u16
            // storage, a description the reader accepts).
            if let Some(inks) = policy.duotone() {
                crate::duotone_native::check(inks, self.is_float())?;
            }
        }
        // …and the reader's LAB checks (`lab_native::check`: u16 storage,
        // body profile = the Lab twin), by the same rule.
        if let Some(policy) = self.lab_policy() {
            crate::lab_native::check(policy, self.is_float(), self.profile.bytes())?;
        }
        // …and the reader's INDX checks (`indexed_native::check`: u16
        // storage, one raster at the origin, its pixels the plane's render).
        if let Some(policy) = self.indexed_policy() {
            crate::indexed_native::check(policy, self, job)?;
        }
        // …and the reader's BTMP checks (`bitmap_native::check`: 8-bit Gray
        // in u16 storage, one raster at the origin, no alpha channel, its
        // pixels the plane's render).
        if let Some(policy) = self.bitmap_policy() {
            crate::bitmap_native::check(policy, self, job)?;
        }
        admit(
            self.retained_bytes(),
            plan.write_scratch_bytes,
            other_retained,
            limit,
        )?;
        job.reserve(plan.write_scratch_bytes)?;
        Ok(plan)
    }

    /// Writes the document to `out` in the format's order, every byte under
    /// `plan.encoded_bound`. Records, channels and the source table stream
    /// straight to `out`; each small table is encoded into one reused scratch
    /// `Vec` (its encoder keeps its `&mut Vec<u8>` signature) and then
    /// appended, so beside the retained tiles the writer holds the pre-built
    /// tables, that scratch, one row and the sink's window
    /// (`precision_native_plan`'s write term).
    pub(crate) fn write_native(
        &self,
        plan: &PrecisionNativePlan,
        out: &mut dyn NativeSink,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        job.check()?;
        let sources = crate::native_sources::precision_sources(self)?;
        let projective = crate::source_transform_native::entries(&sources.bindings);
        let warp = crate::warp_native::encode_typed(self)?;
        // Reserved once so the encoders' small exact `append` reservations
        // reuse its capacity instead of reallocating per append; a table
        // past it still grows (exactly) to its own size. The reservation is
        // one 64 KiB chunk, inside the write term's `BOOKKEEPING_BYTES`.
        let mut scratch = Vec::new();
        scratch
            .try_reserve_exact(TABLE_SCRATCH_BYTES)
            .map_err(|_| "could not allocate high-depth native output")?;
        append(
            out,
            crate::native_magic::NATIVE_MAGIC,
            plan.encoded_bound,
            job,
        )?;
        // The OUTERMOST wrapper (`identity_native`), and the one that is
        // ALWAYS written: 46 before every prior version word, 44 included,
        // and one LIDS table after every prior tail. Ids are a property of
        // every record, so there is no "document without the table".
        let ids = crate::identity_native::precision_ids(self);
        append(
            out,
            &crate::identity_native::PRECISION_VERSION.to_le_bytes(),
            plan.encoded_bound,
            job,
        )?;
        // Then the CMYK document word (`cmyk_native`, #1) — the FIRST
        // word inside 46, written iff the document is CMYK: 88 when it
        // holds a stacked style (a record's or a comp's), else 87 when it
        // holds a description, else 86 when any record holds a layer effect,
        // knockout or Blend Interior, else 85 when it holds an alpha channel
        // or a vector mask, else 84 when any record holds a group, a clipping
        // mask, a blend mode or a style,
        // else 83 when one holds a mask or is a CMYK adjustment record, else
        // 82 (`cmyk_native::version`); its CMYK table is the last one before
        // LIDS and its rasters are raw CMYKA16.
        let cmyk = self.cmyk_policy().map(|policy| &**policy);
        if cmyk.is_some() {
            append(
                out,
                &crate::cmyk_native::version(self).to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // …or, in the same slot, the Gray document word 106 (`gray_native`,
        // #1): written iff the document is Gray (never beside a CMYK
        // word); its GRAY table goes where the CMYK table goes. A Duotone
        // document (a Gray one with a description) writes 109 in its place
        // (`duotone_native`, #1 Duotone phase 1a), its DUOT table right
        // after GRAY. A Bitmap document (Gray underneath) writes the Bitmap
        // word 110 (`bitmap_native`) in its place, its GRAY table then its
        // BTMP table where the CMYK table goes.
        let gray = self.gray_policy().map(|policy| &**policy);
        let duotone = gray.and_then(|policy| policy.duotone()).map(|inks| &**inks);
        let bitmap = self.bitmap_policy().map(|policy| &**policy);
        if gray.is_some() {
            let word = if duotone.is_some() {
                crate::duotone_native::WORD
            } else if bitmap.is_some() {
                crate::bitmap_native::WORD
            } else {
                crate::gray_native::WORD
            };
            append(out, &word.to_le_bytes(), plan.encoded_bound, job)?;
        }
        // …or, in the same slot, the Lab document word 107 (`lab_native`,
        // #1): written iff the document is Lab (never beside a CMYK or
        // Gray word); its LAB table goes where the CMYK table goes.
        let lab = self.lab_policy().map(|policy| &**policy);
        if lab.is_some() {
            append(
                out,
                &crate::lab_native::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // …or, in the same slot, the Indexed document word 108
        // (`indexed_native`, #1): written iff the document is Indexed
        // (never beside a CMYK, Gray or Lab word); its INDX table goes where
        // the CMYK table goes.
        let indexed = self.indexed_policy().map(|policy| &**policy);
        if indexed.is_some() {
            append(
                out,
                &crate::indexed_native::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `background_native` word 113 — right after the
        // CMYK slot and ahead of 112, written iff the Background mark names a
        // record the document holds. Its LBKG table follows SPOT.
        let background = crate::background_native::precision_entry(self);
        if background.is_some() {
            append(
                out,
                &crate::background_native::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `spot_channel_native` word 112 (#1) — after
        // the CMYK slot and 113, ahead of 111, written iff some channel is a
        // spot channel. Its SPOT table follows LREF, before the CMYK table.
        let spots = crate::spot_channel_native::precision_entries(self);
        if !spots.is_empty() {
            append(
                out,
                &crate::spot_channel_native::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `gradient_map_method_native` word 111
        // — after the CMYK slot and 112 and ahead of 105, written iff some
        // adjustment record's description (a CMYK record's included) is a
        // Gradient Map with a `method`. No table: the key rides in the
        // description.
        if crate::gradient_map_method_native::precision_carries(self) {
            append(
                out,
                &crate::gradient_map_method_native::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `fill_align_native` word 105 — after
        // the CMYK slot and 111 and ahead of 104, written iff some record's
        // artwork description is a Gradient Fill aligned with the layer (a
        // CMYK document's included). No table: the flag rides in ADSC.
        if crate::fill_align_native::descriptions_align16(self) {
            append(
                out,
                &crate::fill_align_native::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `gradient_midpoint_native` word 104 —
        // after the CMYK slot and 105 and ahead of 103, written iff some
        // record's or comp's style, or some record's Gradient Fill
        // description, holds a stop midpoint other than 50 %. No table.
        if crate::gradient_midpoint_native::styles_midpoints16(self) {
            append(
                out,
                &crate::gradient_midpoint_native::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `gradient_smoothness_native` word 103
        // — after the CMYK slot, 105 and 104 and ahead of 102, written iff some
        // record's or comp's style, or some record's Gradient Fill
        // description, holds a Smoothness below 100 %. No table.
        if crate::gradient_smoothness_native::styles_smoothness16(self) {
            append(
                out,
                &crate::gradient_smoothness_native::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `large_description_native` word 102 —
        // after the CMYK slot, 105, 104 and 103 and ahead of 101, written iff some
        // record's artwork description is over 64 KiB (a CMYK document's
        // included). No table: the description rides in ADSC as before.
        if crate::large_description_native::descriptions_large16(self) {
            append(
                out,
                &crate::large_description_native::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `blend_if_channels_native` word 101 — after
        // the CMYK slot and word 102 and ahead of 100, written iff
        // the document holds a multichannel Blend If (a record's, whose
        // ranges ride in LBIF schema 2, or a comp's). No table of its own.
        if crate::blend_if_channels_native::styles_multichannel16(self) {
            append(
                out,
                &crate::blend_if_channels_native::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `wide_color_native` word 100 — after
        // the CMYK slot and word 101 and ahead of 99, written iff the
        // document holds a style colour slot carrying a wide colour (a
        // record's or a comp's, a CMYK document's included). No table.
        if crate::wide_color_native::styles_wide16(self) {
            append(
                out,
                &crate::wide_color_native::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `glow_gradient_native` word 99 — after
        // the CMYK slot and words 101 and 100 and ahead of 98, written iff the document holds
        // a glow carrying a gradient or Jitter (a record's or a comp's, a
        // CMYK document's included). No table.
        if crate::glow_gradient_native::styles_glow_gradient16(self) {
            append(
                out,
                &crate::glow_gradient_native::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `texture_native` word 98 —
        // after the CMYK slot and word 99 and ahead of 97, written iff the
        // document holds a Bevel & Emboss carrying a texture (a record's or a
        // comp's, a CMYK document's included). No table.
        if crate::texture_native::styles_texture16(self) {
            append(
                out,
                &crate::texture_native::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `noise_native` word 97 —
        // after the CMYK slot and word 98 and ahead of 96, written iff the
        // document holds a layer style Noise above 0 (a record's or a comp's,
        // a CMYK document's included). No table.
        if crate::noise_native::styles_noise16(self) {
            append(
                out,
                &crate::noise_native::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `contour_native` word 96 —
        // after the CMYK slot and word 97 and ahead of 95, written iff the
        // document holds a layer style contour-family key (a record's or a
        // comp's, a CMYK document's included). No table.
        if crate::contour_native::styles_contour16(self) {
            append(
                out,
                &crate::contour_native::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `stroke_pattern_native` word 95 —
        // after the CMYK slot and word 96 and ahead of 94, written iff the
        // document holds a stroke carrying a pattern fill (a record's or a
        // comp's, a CMYK document's included). No table.
        if crate::stroke_pattern_native::styles_stroke_pattern16(self) {
            append(
                out,
                &crate::stroke_pattern_native::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `gradient_options_native` word 94 —
        // after the CMYK slot and word 95 and ahead of 93, written iff the
        // document holds a Shape Burst stroke gradient or a dithered gradient
        // (a record's or a comp's, a CMYK document's included). No table.
        if crate::gradient_options_native::styles_options16(self) {
            append(
                out,
                &crate::gradient_options_native::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `pattern_angle_native` word 93 —
        // after the CMYK slot and word 94 and ahead of 92, written iff the
        // document holds a pattern overlay with a non-zero angle (a record's
        // or a comp's, a CMYK document's included). No table.
        if crate::pattern_angle_native::styles_angle16(self) {
            append(
                out,
                &crate::pattern_angle_native::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `generated_native` word 92 — after the
        // CMYK slot and word 93 and ahead of 91, written iff the document
        // holds live Generative Layer variations (a CMYK document's included,
        // its entries' `ink` 1). Its GVAR table follows PATH and precedes
        // LREF.
        let variations = crate::generated_native::encode(&self.generated_entries())?;
        if !variations.is_empty() {
            append(
                out,
                &crate::generated_native::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `reference_native` word 91 — after the
        // CMYK slot and words 93 and 92 and ahead of 90, written iff some
        // record moved since it was made; its LREF table follows every prior
        // tail but CMYK's and LIDS.
        let references = crate::reference_native::precision_entries(self);
        if !references.is_empty() {
            append(
                out,
                &crate::reference_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `style_pattern` word 90 — after
        // the CMYK slot and word 91 and ahead of 89, written iff the document
        // holds a pattern overlay (a record's or a comp's, a CMYK document's
        // included). No table.
        if crate::style_pattern::styles_pattern16(self) {
            append(
                out,
                &crate::style_pattern::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `style_instances` word 89 — after the
        // CMYK slot and before 79, written iff a NON-CMYK document holds a
        // stacked style (a record's or a comp's). A CMYK document claims its
        // stacked styles with `cmyk_native`'s rung 88 instead. No table: the
        // instances ride in each style's JSON.
        if cmyk.is_none() && crate::style_instances::styles_stacked16(self) {
            append(
                out,
                &crate::style_instances::WORD.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `raw_locals_native` word 79 — the FIRST word
        // inside 46, with no table of its own. Written only when the written
        // RAW sources hold more than four masks on one attachment or over
        // 512 MiB of resources.
        if crate::raw_locals_native::carries(sources.assets.iter().map(|a| &**a)) {
            append(
                out,
                &crate::raw_locals_native::VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `path_native` word 75 — the next word inside
        // 46. Written only when the document holds a path or a live vector
        // mask, and its PATH table is the last one before LIDS.
        let paths = crate::path_native::encode(&self.paths.entries, &self.vector_mask_table(), 2)?;
        if !paths.is_empty() {
            append(
                out,
                &crate::path_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `look_native` word 71 — the next word inside
        // 46, ahead of 67, with no table of its own. Written only when some
        // Color Lookup description keeps an abstract profile.
        if crate::look_native::precision_carries(self) {
            append(
                out,
                &crate::look_native::VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional float RAW master word 78 (`float_native`),
        // immediately before 67 and with no table. Written only when
        // some 32-bit float master carries a camera original.
        if crate::float_native::raw_masters_present(&sources.assets) {
            append(
                out,
                &crate::float_native::RAW_MASTER_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional float-master word (`float_native`) —
        // the next word inside 46, with no table. Written only when some
        // written asset is a 32-bit float master.
        if crate::float_native::masters_present(&sources.assets) {
            append(
                out,
                &crate::float_native::MASTER_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `curves_native` word — the next word inside
        // 46, with no table of its own. Written only when some curves
        // adjustment carries a map.
        if crate::curves_native::precision_carries(self) {
            append(
                out,
                &crate::curves_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `levels_native` word — the next word inside
        // 46, with no table of its own. Written only when some levels
        // adjustment carries an output range or a per-channel stage.
        if crate::levels_native::precision_carries(self) {
            append(
                out,
                &crate::levels_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `sampler_native` word — the next word inside
        // 46. Written only when the document holds a colour sampler, and its
        // CSMP table is the last one before LIDS.
        let samplers = crate::sampler_native::encode(&self.color_samplers)?;
        if !samplers.is_empty() {
            append(
                out,
                &crate::sampler_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `precision_light_native` word — next inside
        // 46. Written only when the global light is not the default,
        // and its LGLT table is the last one before LIDS.
        let light = self.global_light();
        if crate::precision_light_native::header_bytes(light) != 0 {
            append(
                out,
                &crate::precision_light_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `source_refusal_native` word — next
        // inside 46. Written only when some retained source asset carries a
        // profile refusal, and its RSPR table is the last one before LIDS.
        let refusals = crate::source_refusal_native::entries(&sources.assets);
        if !refusals.is_empty() {
            append(
                out,
                &crate::source_refusal_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `precision_group_open_native` word — next
        // inside 46. Written only when some group is CLOSED, and its LOPN
        // table follows WARP.
        let closed = crate::precision_group_open_native::precision_entries(self);
        if !closed.is_empty() {
            append(
                out,
                &crate::precision_group_open_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        if !warp.is_empty() {
            append(
                out,
                &crate::warp_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        if !projective.is_empty() {
            append(
                out,
                &crate::source_transform_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `precision_lock_native` word — the ONE word
        // that sits INSIDE 46 rather than outside it, so 12E-1's two
        // invariants (46 first, LIDS last) stay true. Written only when some
        // record carries a lock or a link, and its LLNK table lands after
        // LCMP and before LIDS.
        let locks = crate::precision_lock_native::precision_entries(self);
        if !locks.is_empty() {
            append(
                out,
                &crate::precision_lock_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `precision_guide_native` word — the SECOND
        // word inside 46, beside 47. Written only when the document carries
        // a guide or has moved its ruler origin, and its LGDE table lands
        // after LLNK and before LIDS. Built ONCE here and handed to the
        // encoder, which is the same quantity `precision_native_plan`
        // charges.
        let guides = crate::precision_guide_native::precision_entries(self);
        if guides.is_some() {
            append(
                out,
                &crate::precision_guide_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional KNOCKOUT word — the THIRD word inside 46,
        // after 47 and 48, with NO table of its own: the two keys ride inside
        // the style JSON each `LSTY` record already carries, and the word is
        // the claim that some record uses them
        // (`precision_style_knockout`, D10's typed twin). Clearing the last
        // key restores the 12E-1c writer's exact bytes.
        if crate::precision_style_knockout::styles_carry_keys16(self) {
            append(
                out,
                &crate::precision_style_knockout::TYPED_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then the conditional `comps_native` wrapper: 44 before every
        // prior version word, 42 included, and one LCMP table after every
        // prior tail. A comp-less document writes neither. The payload is
        // built ONCE here and handed to the encoder, which is the same
        // quantity `precision_native_plan` charges.
        let comps = crate::comps_native::payload(&self.comps, |id| {
            Some(self.layer(self.layer_index_by_id(id)?)?.name().to_string())
        })?;
        if !comps.is_empty() {
            append(
                out,
                &crate::comps_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then `label_native`: 42 before every prior version word, 40
        // included, and one LLBL table after every prior tail. An unlabelled
        // document writes neither.
        let labels = crate::label_native::precision_entries(self);
        if !labels.is_empty() {
            append(
                out,
                &crate::label_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then `float_native`: 40 before every prior version word, 39
        // included, and one FLOT table after every prior tail. A u16 document
        // writes neither.
        let float = self.is_float();
        if float {
            append(
                out,
                &crate::float_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        // Then `evaluation_native`: 39 before every prior version word, one
        // EVAL table after every prior tail. An ENCODED document writes
        // neither; a FLOAT document is always linear light, so under 40 the
        // word and the table are always present.
        let linear_light = self.evaluation() == crate::doc_evaluation::Evaluation::LinearLight;
        if linear_light {
            append(
                out,
                &crate::evaluation_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        let styles = crate::precision_style_native::precision_entries(self);
        if !styles.is_empty() {
            append(
                out,
                &crate::precision_style_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        let blend_if = crate::precision_blend_if_native::precision_entries(self);
        if !blend_if.is_empty() {
            append(
                out,
                &crate::precision_blend_if_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        let clipped = crate::precision_clip_native::precision_indices(self);
        if !clipped.is_empty() {
            append(
                out,
                &crate::precision_clip_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        let outside = crate::mask_outside_native::precision_indices(self);
        if !outside.is_empty() {
            append(
                out,
                &crate::mask_outside_native::PRECISION_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        if let Some(version) = crate::precision_blend_native::version(self) {
            append(out, &version.to_le_bytes(), plan.encoded_bound, job)?;
        }
        let body_version = if matches!(
            sources.version(),
            crate::native_sources::PRECISION_RESOURCE_VERSION
                | crate::native_sources::PRECISION_FILTER_VERSION
        ) {
            sources.version()
        } else if crate::artwork_native::header_bytes(self) > 0 {
            crate::artwork_document::NATIVE_VERSION
        } else {
            sources.version()
        };
        if crate::precision_channel_metadata::present(self) {
            append(
                out,
                &crate::precision_channel_metadata::NATIVE_VERSION.to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        for value in [
            body_version,
            if float {
                crate::float_native::STORAGE_TAG_FLOAT
            } else if cmyk.is_some() {
                crate::cmyk_native::STORAGE_TAG_CMYK
            } else {
                STORAGE_TAG
            },
            EVALUATION_TAG,
            self.width,
            self.height,
            self.layers.len() as u32,
            self.records as u32,
        ] {
            append(out, &value.to_le_bytes(), plan.encoded_bound, job)?;
        }
        put_blob(out, self.profile.bytes(), plan.encoded_bound, job)?;
        for value in [self.resolution.x, self.resolution.y] {
            append(out, &value.to_le_bytes(), plan.encoded_bound, job)?;
        }
        for packet in [&self.metadata.exif, &self.metadata.xmp, &self.metadata.iptc] {
            append(out, &[u8::from(packet.is_some())], plan.encoded_bound, job)?;
            if let Some(bytes) = packet {
                put_blob(out, bytes, plan.encoded_bound, job)?;
            }
        }
        #[allow(clippy::too_many_arguments)]
        fn records(
            layers: &[PrecisionDocumentLayer],
            out: &mut dyn NativeSink,
            bound: u64,
            job: JobContext<'_>,
            finished: &mut usize,
            total: usize,
            float: bool,
        ) -> Result<(), String> {
            for layer in layers {
                job.check()?;
                let kind = match layer.content {
                    Content::Raster { .. } | Content::CmykRaster { .. } => 0,
                    Content::Group(_) => 1,
                    Content::PassThrough(_) => 2,
                    Content::Adjustment(_) | Content::CmykAdjustment(_) => 3,
                };
                append(out, &[kind, u8::from(layer.visible), 0, 0], bound, job)?;
                append(out, &layer.opacity.to_le_bytes(), bound, job)?;
                put_blob(out, layer.name.as_bytes(), bound, job)?;
                let offset = layer.offset();
                append(out, &offset.0.to_le_bytes(), bound, job)?;
                append(out, &offset.1.to_le_bytes(), bound, job)?;
                append(out, &[u8::from(layer.mask.is_some())], bound, job)?;
                if let Some(mask) = &layer.mask {
                    let p = mask.properties();
                    let dims = mask.plane().dimensions();
                    append(
                        out,
                        &[u8::from(mask.enabled()), u8::from(p.linked), 0, 0],
                        bound,
                        job,
                    )?;
                    for value in [p.offset.0, p.offset.1] {
                        append(out, &value.to_le_bytes(), bound, job)?;
                    }
                    for value in [dims.0, dims.1] {
                        append(out, &value.to_le_bytes(), bound, job)?;
                    }
                    for value in [p.density, p.feather] {
                        append(out, &value.to_le_bytes(), bound, job)?;
                    }
                    put_plane(out, mask.plane().samples(), bound, job)?;
                }
                match &layer.content {
                    Content::Raster { image, .. } if float => {
                        crate::float_native::put_raster(out, image, bound, job)?
                    }
                    Content::Raster { image, .. } => {
                        let dimensions = image.dimensions();
                        for value in [dimensions.0, dimensions.1] {
                            append(out, &value.to_le_bytes(), bound, job)?;
                        }
                        put_native_png(
                            out,
                            image,
                            bound,
                            job.range(
                                *finished as f64 / total as f64,
                                (*finished + 1) as f64 / total as f64,
                            ),
                        )?;
                    }
                    Content::Adjustment(state) => {
                        put_blob(out, state.description().as_bytes(), bound, job)?
                    }
                    Content::CmykRaster { image, .. } => {
                        crate::cmyk_native::put_raster(out, image, bound, job)?
                    }
                    Content::CmykAdjustment(state) => {
                        put_blob(out, state.description().as_bytes(), bound, job)?
                    }
                    Content::Group(children) | Content::PassThrough(children) => {
                        append(out, &(children.len() as u32).to_le_bytes(), bound, job)?;
                        records(children, out, bound, job, finished, total, float)?;
                    }
                }
                *finished += 1;
                job.report(*finished as f64 / total as f64, 6)?;
            }
            Ok(())
        }
        records(
            &self.layers,
            out,
            plan.encoded_bound,
            job,
            &mut 0,
            self.records,
            float,
        )?;
        append(
            out,
            &(self.channels.len() as u32).to_le_bytes(),
            plan.encoded_bound,
            job,
        )?;
        for channel in self.channels.iter() {
            put_blob(out, channel.name().as_bytes(), plan.encoded_bound, job)?;
            put_plane(out, channel.plane().samples(), plan.encoded_bound, job)?;
        }
        if crate::artwork_native::header_bytes(self) > 0
            && !matches!(
                sources.version(),
                crate::native_sources::PRECISION_RESOURCE_VERSION
                    | crate::native_sources::PRECISION_FILTER_VERSION
            )
        {
            append(
                out,
                &sources.version().to_le_bytes(),
                plan.encoded_bound,
                job,
            )?;
        }
        sources.encode(out, plan.encoded_bound, job)?;
        if crate::artwork_native::header_bytes(self) > 0
            || matches!(
                sources.version(),
                crate::native_sources::PRECISION_RESOURCE_VERSION
                    | crate::native_sources::PRECISION_FILTER_VERSION
            )
        {
            table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
                crate::artwork_native::encode(self, t, room, job)
            })?;
        }
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::precision_channel_metadata::encode(self, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::precision_blend_native::encode(self, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::mask_outside_native::encode(&outside, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::precision_clip_native::encode(&clipped, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::precision_blend_if_native::encode(&blend_if, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::precision_style_native::encode(&styles, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::evaluation_native::encode(linear_light, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::float_native::encode(self, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::label_native::encode(&labels, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::comps_native::encode(&comps, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::precision_lock_native::encode(&locks, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::precision_guide_native::encode(&guides, t, room, job)
        })?;
        append(
            out,
            &crate::source_transform_native::encode(&projective)?,
            plan.encoded_bound,
            job,
        )?;
        append(out, &warp, plan.encoded_bound, job)?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::precision_group_open_native::encode(&closed, t, room, job)
        })?;
        append(
            out,
            &crate::source_refusal_native::encode(&refusals)?,
            plan.encoded_bound,
            job,
        )?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::precision_light_native::encode(light, t, room, job)
        })?;
        append(out, &samplers, plan.encoded_bound, job)?;
        append(out, &paths, plan.encoded_bound, job)?;
        append(out, &variations, plan.encoded_bound, job)?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::reference_native::encode(&references, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::spot_channel_native::encode(&spots, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::background_native::encode(background, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::cmyk_native::encode(cmyk, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::gray_native::encode(gray, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::duotone_native::encode(duotone, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::lab_native::encode(lab, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::indexed_native::encode(indexed, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::bitmap_native::encode(bitmap, t, room, job)
        })?;
        table(out, &mut scratch, plan.encoded_bound, job, |t, room| {
            crate::identity_native::encode(&ids, t, room, job)
        })?;
        job.check()
    }
}

fn put_blob(
    out: &mut dyn NativeSink,
    bytes: &[u8],
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    let length = u32::try_from(bytes.len()).map_err(|_| "native slot length overflow")?;
    append(out, &length.to_le_bytes(), bound, job)?;
    append(out, bytes, bound, job)
}
/// Appends `bytes` to `out` in 64 KiB chunks, polling `job` before each,
/// refusing past `bound`. Every typed table encoder writes through it: the
/// sixteen that keep a `&mut Vec<u8>` (coerced here), and the streamed
/// records, channels and sources.
pub(crate) fn append(
    out: &mut dyn NativeSink,
    bytes: &[u8],
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    if out
        .position()
        .checked_add(bytes.len() as u64)
        .is_none_or(|size| size > bound)
    {
        return Err("high-depth native encoding exceeds its admitted size".into());
    }
    if !out.reserve(bytes.len()) {
        return Err("could not allocate high-depth native output".into());
    }
    for chunk in bytes.chunks(64 * 1024) {
        job.check()?;
        out.put(chunk)?;
    }
    Ok(())
}

/// One small table: encoded into `scratch` under the room left before
/// `bound`, then appended to `out`. `scratch` is cleared first and reused,
/// so the writer holds at most the largest table (≤ the plan's header).
fn table(
    out: &mut dyn NativeSink,
    scratch: &mut Vec<u8>,
    bound: u64,
    job: JobContext<'_>,
    encode: impl FnOnce(&mut Vec<u8>, u64) -> Result<(), String>,
) -> Result<(), String> {
    scratch.clear();
    encode(scratch, bound.saturating_sub(out.position()))?;
    append(out, scratch, bound, job)
}

fn put_plane(
    out: &mut dyn NativeSink,
    samples: &[u16],
    bound: u64,
    job: JobContext<'_>,
) -> Result<(), String> {
    append(out, &(samples.len() as u64 * 2).to_le_bytes(), bound, job)?;
    let mut bytes = [0_u8; 8192];
    for chunk in samples.chunks(4096) {
        job.check()?;
        for (sample, dest) in chunk.iter().zip(bytes.chunks_exact_mut(2)) {
            dest.copy_from_slice(&sample.to_le_bytes());
        }
        append(out, &bytes[..chunk.len() * 2], bound, job)?;
    }
    Ok(())
}
