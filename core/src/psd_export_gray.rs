//! A Grayscale document's PSD, PSB and layered-TIFF export (#547 phase 3b):
//! its report, its save, the one-plane composite and the two-plane channel
//! writer the shared record walk calls.
//!
//! Everything but the colour mode is the 16-bit RGB export's
//! (`psd_export16`): the same inventory (`gray`: one composite plane, the
//! Gray profile's bytes and name, preflighted at the DECLARED depth), the
//! same refusals, losses and observations (`psd_export_preflight`) and the
//! same record walk (`psd_export16::records` with `Walk::gray`), whose rasters,
//! adjustment stamps and bakes are written from R (R = G = B) and alpha at
//! the declared depth ([`channels_gray`]: ids `-1`, `0`; 8 bits `round(v /
//! 257)`), whose masks and placeholders take the same depth, whose effect
//! colours are `Grsc` (`FxColours::Gray`) and whose adjustments are
//! `psd_adjustment::encode`'s. The file is colour mode 1 with the Gray ICC
//! bytes verbatim in resource 1039 (a TIFF's tag 34675 too). The composite is
//! the host's projection of this revision, as an RGB document's is. The plan
//! is `save_psd`'s 16-bit formula (one plane written of the four it
//! charges). A 32-bit Gray document (phase 3c) takes the float preflight
//! (`limited_psd_report` at 4 B a sample: a layered TIFF refuses with
//! `psd_export_gray32::TIFF_REFUSAL`, a record with no 32-bit form with
//! `layers_refusal`) and writes through `psd_export_gray32`.
//!
//! **Photoshop's own gray layout** (#547 phase 3b). Phase 3a's writer
//! crashed Photoshop on open (`gray-files-547/take1`); an offline diff of
//! its files against Photoshop's own gray PSD, PSB and TIFF (the read
//! fixtures `tests/fixtures/psd_photoshop/gray/`) moved it, one reasoned
//! difference at a time, to the form Photoshop writes:
//!
//! * **M1** — a PSD or PSB holds ONE composite plane, the white-matted gray
//!   ([`composite_plane_gray`]), header channels 1 and a POSITIVE record
//!   count (`psd_export_encode::layer_info_counted`): Photoshop's gray files
//!   carry no merged-alpha plane, where 3a wrote two planes and a negative
//!   count.
//! * **M2** — no move: the ICC stays the built-in Dot Gain profile
//!   (`icc_builtin::gray_blob`, 8,532 bytes) where Photoshop embeds its own
//!   912-byte one, which Doctored cannot ship; the take opens a flat
//!   one-channel TIFF with the same ICC first, which isolates it.
//! * **M3** — a pixel-less adjustment record sets flag bit 4 (0x18), as
//!   Photoshop's Levels record does, where 3a wrote 0x08.
//! * **M4** — an open or closed folder's `lsct` is 16 bytes, its sub-type (0)
//!   after the blend key, as Photoshop writes it, where 3a wrote 12.
//! * **M5** — Blend If's Gray range is slot 1 with slot 0 the default, and a
//!   Levels is the Gray channel record: with no channel stage record 1 is the
//!   master and record 0 the identity, with r = g = b record 0 is the master
//!   and record 1 the channel; records 2–28 are the identity
//!   ([`photoshop_form`]). This is where `psd_import_gray` reads Photoshop's
//!   own (3a's take), where 3a's writer used slot 0 and record 0.
//! * **M6** — a layered TIFF's strip is ONE sample, the white-matted gray,
//!   under Photometric 1 with BitsPerSample one SHORT inline and no
//!   ExtraSamples, and its `Layr`/`Lr16` count is positive, as Photoshop's
//!   `layers8.tif`; 3a wrote straight gray and alpha and a negative count.
//!
//! **A Duotone document** (#547 Duotone phase 1b) is written by this same
//! body and post-processed by `duotone_files::into_duotone_file`: colour
//! mode 8, its duotone specification as the colour-mode data and its carried
//! duotone resources, the report saying `"color_mode":"duotone"`; a layered
//! TIFF refuses. **A 16-bit Duotone document** (#547 spot channels 1b-2,
//! RULING-1) is written at 8 bits: Photoshop 26.10 refused to open every
//! 16-bit Duotone file Doctored wrote, spot channels or none
//! (`scripts/photoshop-probe/spot_psd_readback_take.py` take1). The file is
//! written from the copy Image ▸ Mode ▸ 8 Bits/Channel makes
//! (`gray_profile`'s depth op: the inks kept, every raster, mask and channel,
//! spot channels included, on the 8-bit grid), admitted by that op's own
//! plan and held beside the document, which is unchanged (`.rz` keeps 16
//! bits). The report says depth 8 and carries one loss line
//! ([`DUOTONE_EIGHT_LOSS`]), and the copy's plan rides in
//! `bake_scratch_bytes`, which the host's admission adds.
//!
//! Forms the RGB writer shares and Photoshop opens are kept: untrimmed
//! record bounds, ranges only on a record with Blend If, the `luni`
//! padding, the 292-byte `levl` without its `Lvls` tail, `pass` in a Pass
//! Through folder's key, the absent `lyid`/`shmd` blocks, a 16-bit file's
//! records in the layer-information section rather than an `Lr16` block,
//! and RLE composite planes.

use serde_json::{json, Value};

use crate::gray_document::GrayDepth;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, PrecisionImage, PrecisionStorage};
use crate::precision_style_render::StyleEnv;
use crate::psd_export16::{exported_fills16, record_styles, records, Walk};
use crate::psd_export_cmyk::Depth;
use crate::psd_export_encode::{
    encode_plane, image_resources, layer_info_counted, matte, matte16, write_psd, Record,
};
use crate::psd_export_preflight::PsdExportMode;
use crate::psd_framing::LayeredContainer;
use crate::psd_layer_pixels::import_envelope_bytes;

/// The 16 → 8 request of a 16-bit Duotone document's export copy: Image ▸
/// Mode ▸ 8 Bits/Channel (`gray_profile`'s depth op, which keeps the inks).
const DUOTONE_EIGHT_REQUEST: &str = r#"{"op":"depth","depth":8}"#;

/// A 16-bit Duotone document's export loss line (module doc).
const DUOTONE_EIGHT_LOSS: &str =
    "16-bit Duotone samples written at 8 bits (Photoshop's Duotone is 8-bit)";

/// A Duotone document's first general limitation (#547 Duotone phase 1b).
const DUOTONE_LIMITATION: &str = "A Duotone document is written as a Duotone PSD or PSB (colour mode 8) at 8 bits, Photoshop's Duotone depth (a 16-bit document from an 8-bit copy, the document unchanged): its gray channel as a Grayscale file's, with PackBits (RLE) channel compression, and its inks as Photoshop's duotone specification. Inks unchanged since the file was opened keep its duotone data byte for byte.";

/// A Duotone document's refusal of a layered TIFF.
const DUOTONE_TIFF_REFUSAL: &str = "A Duotone document is written as a PSD or PSB; a TIFF with layers has no Duotone form. Export as PSD (PSD or PSB), or convert to Grayscale first.";

/// The first general limitation of a Gray export, in place of the RGB one;
/// `thirty_two` for a 32-bit Gray document (phase 3c).
fn first_limitation(container: LayeredContainer, thirty_two: bool) -> &'static str {
    if thirty_two {
        return match container {
            LayeredContainer::Psb => "A 32-bit Grayscale document is written as a 32-bit Grayscale large-document PSB with Raw (uncompressed) channels and the linear (gamma 1.0) twin of its Gray profile embedded, as Photoshop embeds at 32 bits; a 32-bit Grayscale document is never written as RGB.",
            _ => "A 32-bit Grayscale document is written as a 32-bit Grayscale classic PSD with Raw (uncompressed) channels and the linear (gamma 1.0) twin of its Gray profile embedded, as Photoshop embeds at 32 bits; a TIFF with layers is not written at 32 bits, and a 32-bit Grayscale document is never written as RGB.",
        };
    }
    match container {
        LayeredContainer::Psd => "A Grayscale document is written as a Grayscale classic PSD at its declared depth (8 or 16 bits) with its Gray profile embedded, with PackBits (RLE) channel compression; ZIP is not written, and a Grayscale document is never written as RGB.",
        LayeredContainer::Psb => "A Grayscale document is written as a Grayscale large-document PSB at its declared depth (8 or 16 bits) with its Gray profile embedded, with PackBits (RLE) channel compression; ZIP is not written, and a Grayscale document is never written as RGB.",
        LayeredContainer::Tiff => "A Grayscale document is written as a Grayscale TIFF at its declared depth (8 or 16 bits): an uncompressed flat one-channel image of the gray composited over white with Photoshop's layer block beside it, its channels PackBits (RLE) compressed, and its Gray profile embedded.",
    }
}

/// An RGBA16 image's alpha and gray (its R) channels at `depth`, ids `-1`
/// and `0` in that order, from ONE walk of its samples; `None` for f32
/// storage.
pub(crate) fn channels_gray(
    image: &PrecisionImage,
    depth: Depth,
    large: bool,
) -> Option<Vec<(i16, Vec<u8>)>> {
    let (width, height) = image.dimensions();
    let count = width as usize * height as usize * depth.bytes() as usize;
    let mut planes: [Vec<u8>; 2] = std::array::from_fn(|_| Vec::with_capacity(count));
    for pixel in image.runs_rgba16()?.flatten() {
        let [a, gray] = &mut planes;
        depth.push(a, pixel[3]);
        depth.push(gray, pixel[0]);
    }
    Some(
        [-1i16, 0]
            .into_iter()
            .zip(planes)
            .map(|(id, bytes)| {
                (
                    id,
                    encode_plane(&bytes, width * depth.bytes(), height, large),
                )
            })
            .collect(),
    )
}

/// A Gray document's ONE composite plane (M1): its gray, white-matted, at
/// the file's `depth`. At 8 bits each code is reduced first (`round(v /
/// 257)`, `Depth::push`'s rule) and matted in bytes (`matte`); at 16 bits
/// `matte16`, big-endian. The alpha is not written: Photoshop's own gray
/// files carry no merged-alpha plane.
pub(crate) fn composite_plane_gray(
    composite: &PrecisionImage,
    width: u32,
    height: u32,
    depth: Depth,
) -> Result<Vec<u8>, String> {
    if composite.dimensions() != (width, height) {
        return Err("PSD composite preview does not match the canvas".into());
    }
    let runs = composite
        .runs_rgba16()
        .ok_or("a Grayscale PSD export requires an RGBA16 UNORM projection")?;
    let mut plane = Vec::with_capacity(width as usize * height as usize * depth.bytes() as usize);
    for pixel in runs.flatten() {
        if depth.eight {
            let byte = |v: u16| ((u32::from(v) + 128) / 257) as u8;
            plane.push(matte(byte(pixel[0]), byte(pixel[3])));
        } else {
            plane.extend_from_slice(&matte16(pixel[0], pixel[3]).to_be_bytes());
        }
    }
    Ok(plane)
}

/// The default blending-range slot (`0000ffff0000ffff`): the full ramp for
/// this layer and the underlying one.
const RANGE_DEFAULT: [u8; 8] = [0, 0, 0xff, 0xff, 0, 0, 0xff, 0xff];

/// A `levl` record that changes nothing: input 0–255, output 0–255, gamma
/// 1.00 (`100`).
const LEVELS_IDENTITY: [u8; 10] = [0, 0, 0, 0xff, 0, 0, 0, 0xff, 0, 100];

/// The record walk's output in Photoshop's own Grayscale form (module doc,
/// M3, M4 and M5), applied after `psd_export16::records`.
pub(crate) fn photoshop_form(records: &mut [Record]) {
    for record in records.iter_mut() {
        // M3: a pixel-less adjustment record sets bit 4 (pixel data
        // irrelevant to the appearance), 0x18 as Photoshop writes it.
        if record.adjustment.is_some() {
            record.flags |= 0x10;
        }
        // M4: an open or closed folder's `lsct` is 16 bytes, the sub-type
        // (0, normal) after the blend key; the block moves to the head of
        // `blocks`, the same byte position `divider` gave it.
        crate::psd_export_encode::photoshop_folder_lsct(record);
        // M5: Blend If's Gray range in slot 1, every other slot the default.
        if record.ranges.len() == 40 {
            let gray: [u8; 8] = record.ranges[..8].try_into().unwrap_or(RANGE_DEFAULT);
            let mut ranges = Vec::with_capacity(40);
            for slot in 0..5 {
                ranges.extend_from_slice(if slot == 1 { &gray } else { &RANGE_DEFAULT });
            }
            record.ranges = ranges;
        }
        // M5: Levels on the Gray channel record. With no channel stage the
        // master moves to record 1 and record 0 is the identity; with one,
        // record 0 stays the master and record 1 is the channel's. Records
        // 2–28 (no Green or Blue in a gray file) are the identity.
        if let Some((key, body)) = record.adjustment.as_mut() {
            if key == b"levl" && body.len() >= 2 + 29 * 10 {
                let at = |i: usize| 2 + 10 * i;
                let master: [u8; 10] = body[at(0)..at(1)].try_into().unwrap_or(LEVELS_IDENTITY);
                let channel: [u8; 10] = body[at(1)..at(2)].try_into().unwrap_or(LEVELS_IDENTITY);
                let (zero, one) = if channel == LEVELS_IDENTITY {
                    (LEVELS_IDENTITY, master)
                } else {
                    (master, channel)
                };
                body[at(0)..at(1)].copy_from_slice(&zero);
                body[at(1)..at(2)].copy_from_slice(&one);
                for i in 2..29 {
                    body[at(i)..at(i + 1)].copy_from_slice(&LEVELS_IDENTITY);
                }
            }
        }
    }
}

impl PrecisionDocument {
    /// Whether this export writes the document from its 8-bit copy: a
    /// 16-bit Duotone document's PSD or PSB (module doc).
    fn duotone_written_at_eight(&self, container: LayeredContainer) -> bool {
        container != LayeredContainer::Tiff
            && self
                .gray_policy()
                .is_some_and(|p| p.duotone().is_some() && p.depth() == GrayDepth::Sixteen)
    }

    /// The Gray export report (`dr_precision_doc_psd_export_report`'s Gray
    /// arm): the shared preflight's report at the declared depth (at 32 bits
    /// the float one, module doc) with `"color_mode":"gray"` and the Gray
    /// first limitation.
    pub(crate) fn gray_psd_report_json(
        &self,
        container: LayeredContainer,
        mode: PsdExportMode,
        file: u64,
        reopen: u64,
    ) -> Value {
        let depth = self.gray_policy().map_or(GrayDepth::Sixteen, |p| p.depth());
        let report = self.limited_psd_report(container, mode, file, reopen);
        let eight_copy = self.duotone_written_at_eight(container);
        let written = if eight_copy { GrayDepth::Eight } else { depth };
        let mut out = report.json(container, mode, written.bits() as u16);
        if eight_copy {
            // The file is written from an 8-bit copy (module doc): its loss
            // line, and the copy's plan (the depth op's own,
            // `gray_profile_plan`) beside the stamps' scratch, which the host
            // adds to its admission.
            if let Some(losses) = out["losses"].as_array_mut() {
                losses.push(json!(DUOTONE_EIGHT_LOSS));
            }
            // A copy that cannot be planned refuses here, with the plan's
            // own sentence, rather than handing the host a charge it adds.
            let plan = self.gray_profile_plan(None, DUOTONE_EIGHT_REQUEST);
            match plan
                .as_ref()
                .ok()
                .and_then(|plan| plan["working_memory_plan_bytes"].as_u64())
            {
                Some(copy) => {
                    out["bake_scratch_bytes"] =
                        json!(report.bake_scratch_bytes.saturating_add(copy));
                }
                None => {
                    out["available"] = json!(false);
                    out["refusal"] = json!(plan
                        .err()
                        .unwrap_or_else(|| "the 8-bit Duotone copy could not be planned".into()));
                }
            }
        }
        out["color_mode"] = json!("gray");
        if let Some(first) = out["general_limitations"].get_mut(0) {
            *first = json!(first_limitation(container, depth == GrayDepth::ThirtyTwo));
        }
        // A Duotone document (#547 Duotone phase 1b) writes colour mode 8
        // (`duotone_files`); a layered TIFF has no Duotone form.
        if self.is_duotone() {
            out["color_mode"] = json!("duotone");
            if let Some(first) = out["general_limitations"].get_mut(0) {
                *first = json!(DUOTONE_LIMITATION);
            }
            if container == LayeredContainer::Tiff {
                out["available"] = json!(false);
                out["refusal"] = json!(DUOTONE_TIFF_REFUSAL);
            }
        }
        out
    }

    /// Writes this Gray document as a colour-mode-1 PSD, PSB or layered TIFF
    /// at its declared depth (module doc). `projection` is the host's
    /// RGBA16 rendering of THIS revision in the document's twin. Pure, and
    /// atomic through the shared temp-file rename; the preflight's refusal is
    /// this call's error.
    pub(crate) fn save_gray_psd(
        &self,
        projection: Option<&PrecisionImage>,
        path: &str,
        container: LayeredContainer,
        mode: PsdExportMode,
        limit: u64,
        other: u64,
    ) -> Result<(), String> {
        let policy = self
            .gray_policy()
            .ok_or("a Grayscale PSD export needs a Grayscale document")?
            .clone();
        if policy.depth() == GrayDepth::ThirtyTwo {
            return self.save_gray_psd32(projection, path, container, mode, limit, other);
        }
        if policy.duotone().is_some() && container == LayeredContainer::Tiff {
            return Err(DUOTONE_TIFF_REFUSAL.into());
        }
        // A 16-bit Duotone document is written from its 8-bit copy (module
        // doc); the copy is held beside this document while it is written.
        // The file's composite is the 16-bit document's `projection` rounded
        // to 8 bits, deliberately: it is the picture the user saw. Where
        // layers blend, the copy's 8-bit layers can composite up to a code
        // away from it.
        if self.duotone_written_at_eight(container) {
            let eight = self
                .gray_profile_edit(None, DUOTONE_EIGHT_REQUEST, limit, other, None)?
                .ok_or("the 8-bit Duotone copy for the PSD export changed nothing")?;
            return eight.save_gray_psd(
                projection,
                path,
                container,
                mode,
                limit,
                other.saturating_add(self.retained_bytes()),
            );
        }
        let report = self.limited_psd_report(
            container,
            mode,
            container.max_file_bytes(),
            import_envelope_bytes(),
        );
        if let Some(refusal) = report.refusal {
            return Err(refusal);
        }
        let projection = projection.ok_or(
            "a Grayscale document's PSD export requires its projection (required handle is NULL)",
        )?;
        let large = container.large();
        let (width, height) = self.dimensions();
        if projection.dimensions() != (width, height) {
            return Err("the supplied projection does not match this document's canvas".into());
        }
        if projection.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("a Grayscale PSD export requires an RGBA16 UNORM projection".into());
        }
        if projection.profile().bytes() != self.profile().bytes() {
            return Err("the supplied projection does not carry this document's ICC bytes".into());
        }
        // `save_psd`'s 16-bit formula: the file's worst case, the canvas at
        // 8 B a pixel, the stamps' scratch and a wide style encoder.
        let canvas_bytes = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(8))
            .ok_or("16-bit PSD export size calculation overflow")?;
        let wide_bytes = if crate::wide_color_native::styles_wide16(self) {
            crate::authored_color::wide_scratch_bytes()
        } else {
            0
        };
        // A Duotone file is one more copy of the Gray writer's bytes
        // (`duotone_files::into_duotone_file`).
        let duotone_bytes = if policy.duotone().is_some() {
            report.worst_case_bytes
        } else {
            0
        };
        admit(
            self.retained_bytes(),
            report
                .worst_case_bytes
                .saturating_add(canvas_bytes)
                .saturating_add(report.bake_scratch_bytes)
                .saturating_add(wide_bytes)
                .saturating_add(duotone_bytes),
            other,
            limit,
        )?;
        let depth = Depth {
            eight: policy.depth() == GrayDepth::Eight,
        };
        let colors = StyleEnv::colors_for(self.profile());
        let env = StyleEnv {
            light: self.global_light(),
            canvas: (width, height),
            colors: colors.as_ref(),
            transfer: None,
            float: false,
            float_style: None,
            knockout: crate::style_knockout::KnockoutBackdrop::NONE,
            capture: None,
            lab: None,
        };
        let mut out = Vec::new();
        match mode {
            PsdExportMode::Layers => records(
                self.root_layers(),
                self.profile(),
                &env,
                &Walk {
                    document: self,
                    limit,
                    other,
                    large,
                    float: false,
                    gray: Some((depth, &policy)),
                },
                &mut 0,
                &mut out,
            )
            .map(|()| photoshop_form(&mut out))?,
            PsdExportMode::Flattened => out.push(Record {
                bounds: [0, 0, height as i32, width as i32],
                channels: channels_gray(projection, depth, large)
                    .ok_or("a Grayscale PSD export requires an RGBA16 UNORM projection")?,
                blend: *b"norm",
                opacity: 255,
                clipping: 0,
                flags: 0x08,
                mask: None,
                name: "Composite".to_string(),
                divider: None,
                adjustment: None,
                blocks: Vec::new(),
                ranges: Vec::new(),
            }),
        }
        // The document's light, declared only when some record writes
        // effects (`psd_export16`'s rule).
        let light = out
            .iter()
            .any(|record| {
                record
                    .blocks
                    .iter()
                    .any(|(key, _)| matches!(key, b"lfx2" | b"lfxs" | b"lmfx"))
            })
            .then_some(self.global_light());
        // The pattern table (`psd_patterns`: `Patt` at 8 bits, `Pat2` at 16).
        let tail = match mode {
            PsdExportMode::Layers => {
                let mut styles = Vec::new();
                record_styles(self.root_layers(), &mut styles);
                let mut fills = Vec::new();
                exported_fills16(self.root_layers(), &mut fills);
                let fills: Vec<_> = fills
                    .iter()
                    .filter_map(crate::psd_fill_layer::Exported::pattern)
                    .collect();
                crate::psd_patterns::tail_blocks_with(styles, &fills, !depth.eight)
            }
            PsdExportMode::Flattened => Vec::new(),
        };
        let bits = (depth.bytes() * 8) as u16;
        let bytes = if container == LayeredContainer::Tiff {
            // M6: the flat picture is the TIFF's own ONE-sample strip, the
            // white-matted gray of the PSD's composite plane, big-endian,
            // and the `Layr`/`Lr16` count is positive, as Photoshop writes.
            let flat = composite_plane_gray(projection, width, height, depth)?;
            crate::tiff_layers::write(
                width,
                height,
                bits,
                1,
                policy.bytes(),
                self.resolution(),
                &image_resources(
                    policy.bytes(),
                    self.resolution(),
                    light,
                    self.color_samplers(),
                ),
                &layer_info_counted(&out, false, false)?,
                &flat,
                &tail,
            )?
        } else {
            write_psd(
                width,
                height,
                bits,
                1,
                policy.bytes(),
                self.resolution(),
                light,
                self.color_samplers(),
                &out,
                &[composite_plane_gray(projection, width, height, depth)?],
                large,
                &tail,
                &self.psd_spot_planes(depth.eight),
            )?
        };
        let bytes = match policy.duotone() {
            Some(inks) => crate::duotone_files::into_duotone_file(bytes, inks)?,
            None => bytes,
        };
        crate::dr_image::save_atomically(path, |tmp| {
            std::fs::write(tmp, &bytes).map_err(|e| format!("failed to write {path}: {e}"))
        })
    }
}
