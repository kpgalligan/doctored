//! Bounded PSD folder-record metadata, kept separate from pixel decoding.
//! Adobe's Layer records and Section divider setting (`lsct`) specify the
//! fields: https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/
//! `psd` 0.3.5 reverses file records, assigns group IDs on folder entry, then
//! builds groups from closing dividers, losing the folder's properties. Walk
//! the same admitted records first and verify the decoder's parent mapping.
//! Ordinary mask descriptors stay borrowed; advanced masks/styles are disclosed.
//! Original channel slices and exact
//! rectangles feed the separately admitted `psd_layer_pixels` codec seam.
//!
//! Folder opacity, visibility, recognized blend key and disclosure state correlate with verified
//! decoder parent IDs; unknown keys or ambiguous framing refuse layers without hiding a valid saved
//! composite.
//!
//! A PSB's records (`large`) differ in exactly one field of this walk: a channel's data length is
//! a u64 (`psd_framing`). Its `LONG_LENGTH_KEYS` blocks would carry a u64 length inside the
//! record's extra data, where the `psd` crate's metadata view reads a u32, so a record holding one
//! refuses the LAYERED choice with the key named; the stored composite still opens.

use crate::authored_color::AuthoredColors;
use crate::blend::BlendMode;
use crate::drdc::MAX_DRDC_LAYERS;
use crate::psd_adjustment::{self, AdjustmentBody, Carried};
use crate::psd_feature_markers::FeatureMarkers;
use crate::psd_framing::LONG_LENGTH_KEYS;
use crate::psd_layer_effects::{self, RecordStyle, StyleBody};
use crate::psd_layer_pixels::{Channel, LayerPixels};
use crate::psd_raster_masks::{MaskBudget, MaskRecord};

// A native row can require both a folder and a closing record. Bound parser
// metadata/work independently of pixel sizes, before invoking the decoder.
const MAX_RECORDS: usize = MAX_DRDC_LAYERS as usize * 2;
const MAX_EXTRA_BYTES: usize = 64 * 1024 * 1024;
const MAX_BLOCKS: usize = 65_536;
// Keep the existing explicit >10-level dissolution, but bound the recursive
// legacy tree emitter's call stack even for adversarial nesting.
const MAX_RECORD_DEPTH: usize = 128;

#[derive(Clone, Copy)]
pub(crate) struct Group<'a> {
    pub opacity: f32,
    pub blend: BlendMode,
    pub visible: bool,
    pub open: bool,
    parent: Option<u32>,
    pub mask: MaskRecord<'a>,
    pub clipped: bool,
    pub scene: bool,
    pub features: FeatureMarkers,
    /// Photoshop's sheet colour (`lclr`), 0..=7 — the same numbers
    /// `doc_label` uses, which is why nothing maps them.
    pub label: u8,
    /// The record's reference point (`Layer::reference`): its FIRST `fxrp`,
    /// rounded and clamped to i32, (0, 0) when it carries none.
    pub reference: (i32, i32),
}

pub(crate) struct GroupRecords<'a> {
    pub groups: Vec<Group<'a>>, // index + 1 is psd 0.3.5's top-down folder ID
    parents: Vec<Option<u32>>,  // top-down raster order
    pub layers: Vec<LayerPixels<'a>>,
    pub masks: Vec<MaskRecord<'a>>,
    pub layer_features: Vec<FeatureMarkers>,
    /// Parallel to `layers`: the decoded adjustment body of a pixel-less
    /// raster record, `Absent` for every other record.
    pub adjustments: Vec<AdjustmentBody>,
    /// Parallel to `layers`: the decoded `PtFl` or `SoCo` of a divider-0 record (RGB or CMYK) whose
    /// first adjustment key is one, in any form (`fill_candidate`), `None` for every other record.
    pub fills: Vec<Option<crate::psd_fill_layer::CarriedFill>>,
    /// Parallel to `layers` and to `groups`: the record's `lfx2`/`iOpa`/
    /// blending-ranges constructs as one Doctored style
    /// (`psd_layer_effects`). A style never travels inside `Group`, which
    /// stays `Copy`.
    pub layer_styles: Vec<StyleBody>,
    pub group_styles: Vec<StyleBody>,
    /// Parallel to `layers`: the record's `lclr` colour code, 0 when it
    /// carries none.
    pub layer_labels: Vec<u8>,
    /// Parallel to `layers`: the record's reference point (`Group::reference`'s rule).
    pub layer_references: Vec<(i32, i32)>,
    pub divider_features: FeatureMarkers,
    /// FILE record 0 — the bottom entry — as the structural Background rule
    /// (D5) reads it. `None` when there are no records at all, or when record
    /// 0 is not an ordinary layer record (divider 0).
    pub bottom: Option<BottomRecord>,
    /// Some visible non-adjustment divider-0/1/2 record carries a CARRIED
    /// knockout that REACHES the document's bottom: Deep, or Shallow with no
    /// parent group. Recorded during the walk, where the parent stack is the
    /// only place a record's enclosing folder is known.
    reaching_knockout: bool,
    metadata: Vec<&'a [u8]>, // original record bytes after the channel table
    metadata_bytes: usize,
}

/// FILE record 0, in the three terms D5's disclosure reads.
pub(crate) struct BottomRecord {
    /// A true Photoshop Background record: a non-empty rectangle with NO
    /// channel id `-1`. Doctored never writes one.
    pub background: bool,
    /// The record satisfies D5's own clauses as far as a framed record can
    /// state them: not an adjustment record, visible, base blend key `norm`,
    /// opacity byte 255, no ENABLED mask, and file record 1 (if any)
    /// unclipped.
    pub plain: bool,
    /// The record's rectangle, `[top, left, bottom, right]`.
    pub bounds: [i32; 4],
}

struct Record<'a> {
    pixels: LayerPixels<'a>,
    lengths: Vec<(i16, usize)>,
    divider: u32,
    group: Group<'a>,
    mask_body: &'a [u8],
    advanced_mask: bool,
    /// The FIRST adjustment/fill block's key and body, verbatim.
    adjustment_block: Option<([u8; 4], &'a [u8])>,
    /// The FIRST `CgEd` body, verbatim: the descriptor Photoshop writes beside
    /// a `brit` record (`psd_adjustment_extra`).
    companion_block: Option<&'a [u8]>,
    adjustment: AdjustmentBody,
    /// The decoded `PtFl` or `SoCo` of a divider-0 record (RGB or CMYK) whose first
    /// adjustment key is one, in any form (`fill_candidate`, `psd_fill_layer`).
    fill: Option<crate::psd_fill_layer::CarriedFill>,
    /// The record's blending-ranges section and the FIRST `lfx2`/`iOpa`
    /// bodies, verbatim: the three constructs `psd_layer_effects` decodes.
    ranges: &'a [u8],
    effects_block: Option<&'a [u8]>,
    /// The FIRST `lmfx` body, verbatim: Photoshop's stacked instances.
    stacked_block: Option<&'a [u8]>,
    fill_block: Option<&'a [u8]>,
    /// The FIRST `knko` / `infx` bodies, verbatim.
    knockout_block: Option<&'a [u8]>,
    interior_block: Option<&'a [u8]>,
    /// The FIRST `fxrp` body, verbatim: a linked pattern overlay's reference
    /// point (`psd_pattern_fill`).
    reference_block: Option<&'a [u8]>,
    style: StyleBody,
    /// The record's own bytes the structural Background rule reads directly,
    /// because a divider-0 record's blend key and opacity byte never reach
    /// `Group` (its blend is forced Normal there).
    base_blend: [u8; 4],
    opacity_byte: u8,
    flags: u8,
    /// The record is a pixel-less adjustment/fill record, so it carries no
    /// style at either depth.
    adjustment_record: bool,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or("PSD record size overflow")?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or("truncated PSD layer record")?;
        self.offset = end;
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn word(&mut self) -> Result<u16, String> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn long(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn long_long(&mut self) -> Result<u64, String> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn section(&mut self) -> Result<&'a [u8], String> {
        let count = self.long()? as usize;
        self.take(count)
    }
    fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }
}

impl<'a> GroupRecords<'a> {
    /// `colors` converts a DOCUMENT colour back to the sRGB every authored
    /// style colour is stored in, and `to_document` is its reverse, the one a
    /// decoded `phfl` Lab colour takes into the document's space; the sole
    /// caller is `psd_import::parse`, which builds both from
    /// `container.profile()`.
    ///
    /// `info` is the body `Container::layer_info` located — the ordinary
    /// layer-information section at 8 bits, or the `Lr16` tail block a
    /// Photoshop 16-bit file keeps its records in, or a TIFF's block.
    /// `_sample_bytes` is the container's 1 or 2; it no longer decides
    /// anything here, because every compression word is admitted at both
    /// depths. `large` is a PSB: channel lengths are u64, and a record
    /// holding a `LONG_LENGTH_KEYS` block refuses. `color` is the
    /// container's colour mode: a CMYK file (4) admits channel id 3, its
    /// black plane, and decodes its adjustment bodies through
    /// `psd_adjustment::decode_cmyk` (#1 phase 3) — with its `CgEd`
    /// companion and `to_ink`, the file's sRGB → ink conversion (the styles
    /// batch) — and its layer styles with `FxColours::Cmyk`; every other mode
    /// keeps the −3…2 id range, so a stray id 3 on an RGB or gray record
    /// refuses. `solid_refusal` is `psd_fill_layer::solid_refusal` of the
    /// file's profile: when set, a decoded RGB `SoCo` is declined with it
    /// (pixel-less: omitted; a raster: kept as its pixels with its marker
    /// loss), so the report and the byte walk's render agree. `gray` is a
    /// gray file's policy (#1 phase 3a, `psd_import::parse_with`): with
    /// colour mode 1 and `Some`, its layer styles decode with
    /// `FxColours::Gray` (a `Grsc` colour through that Gray profile); its
    /// fills keep `FxColours::Rgb`. A Lab file (colour mode 9, #1 Lab phase
    /// 3a) decodes its layer styles with `FxColours::Lab` (an `LbCl` colour
    /// as its sRGB hex); its fills keep `FxColours::Rgb` too.
    #[allow(clippy::too_many_arguments)]
    pub fn read(
        info: &'a [u8],
        _sample_bytes: u64,
        large: bool,
        color: u16,
        colors: Option<&AuthoredColors>,
        to_document: Option<&AuthoredColors>,
        to_ink: Option<&dyn Fn([u8; 3]) -> Option<[f32; 4]>>,
        patterns: &crate::psd_patterns::PatternTable,
        solid_refusal: Option<&str>,
        gray: Option<&crate::gray_document::GrayPolicy>,
    ) -> Result<Self, String> {
        let top_channel: i16 = if color == 4 { 3 } else { 2 };
        let mut out = Self {
            groups: Vec::new(),
            parents: Vec::new(),
            layers: Vec::new(),
            masks: Vec::new(),
            layer_features: Vec::new(),
            adjustments: Vec::new(),
            fills: Vec::new(),
            layer_styles: Vec::new(),
            group_styles: Vec::new(),
            layer_labels: Vec::new(),
            layer_references: Vec::new(),
            divider_features: FeatureMarkers::default(),
            bottom: None,
            reaching_knockout: false,
            metadata: Vec::new(),
            metadata_bytes: 0,
        };
        if info.is_empty() {
            return Ok(out);
        }
        let mut cursor = Cursor::new(info);
        let count = (cursor.word()? as i16).unsigned_abs() as usize;
        if count > MAX_RECORDS {
            return Err(format!(
                "PSD has too many layer records (max {MAX_RECORDS})"
            ));
        }
        let mut records = Vec::with_capacity(count);
        let (mut extra_bytes, mut blocks) = (0usize, 0usize);
        let mut total_pixels = 0u64;
        for _ in 0..count {
            let bounds = [
                cursor.long()? as i32,
                cursor.long()? as i32,
                cursor.long()? as i32,
                cursor.long()? as i32,
            ];
            let mut lengths = Vec::new();
            let channels = cursor.word()? as usize;
            if channels > 56 {
                return Err("PSD layer has too many channels (max 56)".into());
            }
            for _ in 0..channels {
                let kind = cursor.word()? as i16;
                if !(-3..=top_channel).contains(&kind) || lengths.iter().any(|(id, _)| *id == kind)
                {
                    return Err("PSD layer has an unknown or duplicate channel ID".into());
                }
                let length = if large {
                    usize::try_from(cursor.long_long()?).map_err(|_| "PSD record size overflow")?
                } else {
                    cursor.long()? as usize
                };
                if length < 2 {
                    return Err("PSD layer channel length omits compression bytes".into());
                }
                lengths.push((kind, length));
            }
            let metadata_start = cursor.offset;
            if cursor.take(4)? != b"8BIM" {
                return Err("invalid PSD layer blend signature".into());
            }
            let base_blend: [u8; 4] = cursor.take(4)?.try_into().unwrap();
            let opacity_byte = cursor.byte()?;
            let opacity = f32::from(opacity_byte) / 255.0;
            let clipped = cursor.byte()? != 0;
            // Retain the established import convention: bit 1 set is hidden.
            let flags = cursor.byte()?;
            let visible = flags & 2 == 0;
            cursor.byte()?;
            let extra = cursor.section()?;
            extra_bytes = extra_bytes
                .checked_add(extra.len())
                .ok_or("PSD metadata size overflow")?;
            if extra_bytes > MAX_EXTRA_BYTES {
                return Err("PSD layer metadata exceeds the 64 MiB limit".into());
            }
            let mut extra = Cursor::new(extra);
            let mask_body = extra.section()?;
            let mut advanced_mask = false;
            let mut features = FeatureMarkers::default();
            let ranges = extra.section()?;
            features.ranges(ranges);
            let name_len = extra.byte()? as usize;
            extra.take(((name_len + 4) & !3) - 1)?;
            let (mut divider, mut sub_blend, mut scene) = (None, None, false);
            let mut label = 0u8;
            let mut adjustment_block = None;
            let (mut effects_block, mut fill_block) = (None, None);
            let mut stacked_block = None;
            let (mut knockout_block, mut interior_block) = (None, None);
            let mut reference_block = None;
            let mut companion_block = None;
            while extra.remaining() > 0 {
                blocks += 1;
                if blocks > MAX_BLOCKS {
                    return Err("PSD layer metadata exceeds the 65536-block limit".into());
                }
                if !matches!(extra.take(4)?, b"8BIM" | b"8B64") {
                    return Err("invalid PSD additional layer signature".into());
                }
                let key = extra.take(4)?;
                if large && LONG_LENGTH_KEYS.iter().any(|long| long == key) {
                    return Err(format!(
                        "a PSB layer record holds a 64-bit-length block ({}), which the layer decoder does not read",
                        String::from_utf8_lossy(key)
                    ));
                }
                let body = extra.section()?;
                // The dependency consumes the declared length verbatim. Adobe
                // requires an even byte count; do not let an odd block shift
                // its next record away from this bounded walk's interpretation.
                if !body.len().is_multiple_of(2) {
                    return Err("unsupported odd PSD additional layer block length".into());
                }
                let first_adjustment = features.adjustment_key().is_none();
                features.block(key, body);
                if first_adjustment {
                    if let Some(key) = features.adjustment_key() {
                        adjustment_block = Some((key, body));
                    }
                }
                if key == b"CgEd" && companion_block.is_none() {
                    companion_block = Some(body);
                }
                // A group's effects are `lfxs` in Photoshop's files, the
                // `lfx2` body under a section key; the first of either is
                // the record's.
                if (key == b"lfx2" || key == b"lfxs") && effects_block.is_none() {
                    effects_block = Some(body);
                }
                if key == b"lmfx" && stacked_block.is_none() {
                    stacked_block = Some(body);
                }
                if key == b"iOpa" && fill_block.is_none() {
                    fill_block = Some(body);
                }
                if key == b"knko" && knockout_block.is_none() {
                    knockout_block = Some(body);
                }
                if key == b"infx" && interior_block.is_none() {
                    interior_block = Some(body);
                }
                if key == b"fxrp" && reference_block.is_none() {
                    reference_block = Some(body);
                }
                if key == b"lclr" && label == 0 {
                    // Photoshop's sheet colour: u16 big-endian code then six
                    // reserved bytes. A body of any other length, or a code
                    // this build cannot name, stays 0 — a swatch is cosmetic,
                    // and losing a whole document over one would be absurd.
                    // `doc_label`'s codes ARE these codes, so nothing maps.
                    if body.len() == 8 {
                        let code = u16::from_be_bytes(body[..2].try_into().unwrap());
                        if code <= u16::from(crate::doc_label::MAX_LABEL) {
                            label = code as u8;
                        }
                    }
                }
                advanced_mask |= matches!(key, b"vmsk" | b"vsms" | b"lmgm" | b"vmgm");
                if key == b"luni" {
                    // The decoder trusts the internal UTF-16 count, then seeks
                    // back to the block end. Bound that count so a name cannot
                    // borrow samples or metadata from subsequent records.
                    let mut name = Cursor::new(body);
                    let units = name.long()? as usize;
                    let length = units
                        .checked_mul(2)
                        .ok_or("PSD Unicode name length overflow")?;
                    if length > name.remaining() {
                        return Err("PSD Unicode name exceeds its layer block".into());
                    }
                }
                if key == b"lsdk" {
                    return Err(
                        "PSD nested section-divider records are unsupported by the layer decoder"
                            .into(),
                    );
                }
                if key != b"lsct" {
                    continue;
                }
                if divider.is_some() {
                    return Err("duplicate PSD section-divider record".into());
                }
                if !matches!(body.len(), 4 | 12 | 16) {
                    return Err("unsupported PSD section-divider length".into());
                }
                let mut body = Cursor::new(body);
                let kind = body.long()?;
                if kind > 3 {
                    return Err("unknown PSD section-divider type".into());
                }
                divider = Some(kind);
                if body.remaining() >= 8 {
                    if body.take(4)? != b"8BIM" {
                        return Err("invalid PSD section-divider blend signature".into());
                    }
                    sub_blend = Some(body.take(4)?.try_into().unwrap());
                }
                if body.remaining() == 4 {
                    scene = body.long()? != 0;
                }
            }
            let divider = divider.unwrap_or(0);
            let pixels = LayerPixels {
                bounds,
                channels: Vec::new(),
                large,
            };
            // Adobe's bit4 ("pixel data irrelevant") is set on every one of
            // Photoshop's own adjustment records, which have no pixels at all;
            // reading it there as a loss would report an alternative
            // appearance that does not exist. Only a record with a rectangle
            // can declare its pixels irrelevant.
            features.flags(flags, divider == 0 && !pixels.is_empty());
            let blend = if matches!(divider, 1 | 2) {
                // The optional lsct key overrides the record key. Without it,
                // use the explicit ordinary record blend, never infer Pass Through.
                blend(sub_blend.unwrap_or(base_blend))?
            } else {
                BlendMode::Normal
            };
            let fill_record = adjustment_block
                .is_some_and(|(key, _)| fill_candidate(divider, key, advanced_mask));
            total_pixels += if divider == 0 && (pixels.is_empty() || fill_record) {
                // A pixel-less adjustment/fill or empty record has no
                // rectangle to charge here; what it becomes (nothing, or one
                // canvas-sized transparent raster) is charged by `admit`.
                // Neither has a Pattern or Solid Color Fill with pixels: it
                // becomes a canvas raster, or a raster of its rectangle when
                // its `PtFl`/`SoCo` does not decode, and `admit` charges whichever
                // (`GroupRecords::canvas_raster`).
                0
            } else if divider == 0 {
                let (w, h) = pixels.dimensions()?;
                u64::from(w) * u64::from(h)
            } else if matches!(divider, 1 | 2) {
                1
            } else {
                0
            };
            if total_pixels > crate::drdc::MAX_DRDC_TOTAL_LAYER_PIXELS {
                return Err("PSD aggregate layer pixels exceed the 400 MP native limit".into());
            }
            let metadata = &cursor.bytes[metadata_start..cursor.offset];
            out.metadata_bytes += metadata.len();
            out.metadata.push(metadata);
            records.push(Record {
                pixels,
                lengths,
                divider,
                mask_body,
                advanced_mask,
                adjustment_block,
                companion_block,
                adjustment: AdjustmentBody::Absent,
                fill: None,
                ranges,
                effects_block,
                stacked_block,
                fill_block,
                knockout_block,
                interior_block,
                reference_block,
                style: StyleBody::Absent,
                base_blend,
                opacity_byte,
                flags,
                adjustment_record: false,
                group: Group {
                    opacity,
                    blend,
                    visible,
                    open: divider == 1,
                    parent: None,
                    mask: MaskRecord::Absent,
                    clipped,
                    scene,
                    features,
                    label,
                    reference: reference_point(reference_block),
                },
            });
        }
        for record in &mut records {
            for &(kind, length) in &record.lengths {
                let bytes = cursor.take(length)?;
                // Raw, RLE, ZIP (2) and ZIP-with-prediction (3), at both
                // depths.
                let admitted = u16::from_be_bytes(bytes[..2].try_into().unwrap()) <= 3;
                if kind >= -1 && !admitted {
                    return Err("PSD layer channel uses unsupported compression; only Raw, RLE and ZIP are supported".into());
                }
                record.pixels.channels.push(Channel { kind, bytes });
            }
            record.group.mask = MaskRecord::classify(
                record.mask_body,
                &record.pixels.channels,
                record.advanced_mask,
                large,
            )?;
            // Only a pixel-less raster record is an adjustment layer; the
            // same key on a raster stays a marker loss.
            let adjustment_record = record.divider == 0
                && record.pixels.is_empty()
                && record.group.features.adjustment_key().is_some();
            record.adjustment_record = adjustment_record;
            // A Pattern Fill (CMYK included), Solid Color Fill or Gradient
            // Fill layer in any of its forms (`psd_fill_layer::Form`):
            // carried as a Doctored fill layer. A pixel-less one that does
            // not decode is declined by name; a raster one stays a raster
            // with its marker loss. An RGB solid or gradient the file's
            // profile cannot render takes the same arm.
            let fill_block = record
                .adjustment_block
                .filter(|(key, _)| fill_candidate(record.divider, *key, record.advanced_mask));
            if let Some((key, body)) = fill_block {
                let colours = if color == 4 {
                    psd_layer_effects::FxColours::Cmyk
                } else {
                    psd_layer_effects::FxColours::Rgb(colors)
                };
                let decoded =
                    crate::psd_fill_layer::decode(key, body, patterns, colours).and_then(|fill| {
                        match solid_refusal {
                            Some(reason) if fill.kind.renders_in_profile() => {
                                Err(reason.to_string())
                            }
                            _ => Ok(fill),
                        }
                    });
                match decoded {
                    Ok(mut fill) => {
                        fill.form = if record.pixels.is_empty() {
                            crate::psd_fill_layer::Form::PixelLess
                        } else if record.flags & 24 == 24 {
                            crate::psd_fill_layer::Form::Irrelevant
                        } else {
                            crate::psd_fill_layer::Form::Unflagged
                        };
                        record.fill = Some(fill);
                        record.group.features.carry_fill();
                    }
                    Err(reason) if adjustment_record => {
                        record.adjustment = AdjustmentBody::Declined {
                            key,
                            kind: crate::psd_fill_layer::kind_of(key),
                            reason,
                        }
                    }
                    Err(_) => {}
                }
            } else if adjustment_record {
                if let Some((key, body)) = record.adjustment_block {
                    // Brightness/Contrast is the one kind whose values live
                    // in a second block of the same record.
                    record.adjustment = if color == 4 {
                        psd_adjustment::decode_cmyk(key, body, record.companion_block, to_ink)
                    } else if &key == b"brit" {
                        psd_adjustment::decode_brightness(body, record.companion_block)
                    } else {
                        psd_adjustment::decode(key, body, to_document)
                    };
                    if matches!(record.adjustment, AdjustmentBody::Carried(_)) {
                        record.group.features.carry_adjustment();
                    }
                }
            }
            // A CLOSING divider's decoded style is attached to nothing
            // (the `3` arm below discards it), so it is never offered the
            // two knockout keys and its `knko` marker loss stays.
            let carries_keys = matches!(record.divider, 0..=2);
            record.style = psd_layer_effects::decode(
                RecordStyle {
                    effects: record.effects_block,
                    fill: record.fill_block,
                    ranges: record.ranges,
                    stacked: record.stacked_block,
                    knockout: record.knockout_block.filter(|_| carries_keys),
                    interior: record.interior_block.filter(|_| carries_keys),
                    reference: record.reference_block,
                    top_left: (record.pixels.bounds[1], record.pixels.bounds[0]),
                    patterns,
                },
                match (color, gray) {
                    (4, _) => psd_layer_effects::FxColours::Cmyk,
                    (1, Some(policy)) => psd_layer_effects::FxColours::Gray(policy),
                    // A Lab file's `LbCl` colours (#1 Lab phase 3a).
                    (9, _) => psd_layer_effects::FxColours::Lab,
                    _ => psd_layer_effects::FxColours::Rgb(colors),
                },
            );
            // An adjustment or fill record the build omits carries no style
            // (P7-19): its markers stay, and the report names the discard. A
            // CARRIED adjustment record keeps its style (on CMYK too), and
            // so does a carried pattern fill, so its markers clear as a
            // raster's do.
            if !adjustment_record
                || matches!(record.adjustment, AdjustmentBody::Carried(_))
                || record.fill.is_some()
            {
                if let StyleBody::Carried(carried) = &record.style {
                    record.group.features.carry_style(
                        !carried.effects.is_empty(),
                        carried.fill,
                        carried.ranges,
                    );
                    if carried.knockout || carried.interior {
                        record
                            .group
                            .features
                            .carry_knockout(carried.knockout, carried.interior);
                    }
                }
            }
        }
        // Layer info permits padding to round its size up. Adobe's table says
        // "rounded up to an even byte count", but Photoshop 2025 rounds the
        // 8-bit section to a MULTIPLE OF 4 (its own files carry two trailing
        // zero bytes here), so admit up to three pad bytes and no more.
        if cursor.remaining() > 3 {
            return Err("PSD layer information has unclaimed record/channel bytes".into());
        }
        out.bottom = bottom_record(&records);
        if out.bottom.as_ref().is_some_and(|b| b.background) {
            records[0].group.features.background();
        }
        let mut stack = Vec::new();
        for record in records.into_iter().rev() {
            // D5's "a REACHING knockout exists": a visible non-adjustment
            // record whose CARRIED style is Deep, or Shallow with no parent
            // group. The parent stack is live only here.
            if record.divider != 3 && record.group.visible && !record.adjustment_record {
                out.reaching_knockout |= reaches_background(&record.style, stack.last().is_none());
            }
            match record.divider {
                1 | 2 => {
                    if stack.len() >= MAX_RECORD_DEPTH {
                        return Err("PSD group nesting exceeds the 128-level parsing limit".into());
                    }
                    let mut group = record.group;
                    if stack.len() >= crate::doc_group::MAX_GROUP_DEPTH as usize
                        && group.mask.present()
                    {
                        group.mask = MaskRecord::Dropped(
                            "the masked folder is dissolved beyond the 10-level nesting limit",
                        );
                    }
                    group.parent = stack.last().copied();
                    out.groups.push(group);
                    out.group_styles.push(record.style);
                    stack.push(out.groups.len() as u32);
                }
                3 => {
                    out.divider_features.merge(record.group.features);
                    if record.group.mask.present() {
                        return Err(
                            "PSD bounding divider has an unsupported mask record/channel".into(),
                        );
                    }
                    stack
                        .pop()
                        .ok_or("PSD group has an unmatched closing divider")?;
                }
                _ => {
                    out.parents.push(stack.last().copied());
                    out.masks.push(record.group.mask);
                    out.layer_features.push(record.group.features);
                    out.adjustments.push(record.adjustment);
                    out.fills.push(record.fill);
                    out.layer_styles.push(record.style);
                    out.layer_labels.push(record.group.label);
                    out.layer_references.push(record.group.reference);
                    out.layers.push(record.pixels);
                }
            }
        }
        if !stack.is_empty() {
            return Err("PSD group is missing its closing divider".into());
        }
        if out.groups.len() + out.parents.len() > MAX_DRDC_LAYERS as usize {
            return Err(format!(
                "PSD import exceeds the {MAX_DRDC_LAYERS}-row native layer limit"
            ));
        }
        Ok(out)
    }

    /// D5's disclosure predicate on the FRAMED records alone — no pixel is
    /// decoded, which is `psd_import::report`'s standing rule. True when this
    /// file has no true Photoshop Background record, its bottom entry
    /// satisfies D5, and a knockout REACHES it. `canvas` is the container's
    /// own dimensions.
    pub(crate) fn structural_background_knockout(&self, canvas: (u32, u32)) -> bool {
        let Some(bottom) = self.bottom.as_ref() else {
            return false;
        };
        let (width, height) = canvas;
        bottom.plain
            && !bottom.background
            && bottom.bounds == [0, 0, height as i32, width as i32]
            // The bottom raster is FILE record 0, which the reversed walk
            // below pushes LAST. D5 refuses a styled entry, and a `Declined`
            // body carries nothing, so only `Carried` disqualifies it.
            && !matches!(self.layer_styles.last(), Some(StyleBody::Carried(_)))
            && self.reaching_knockout
    }

    /// The exact framed record metadata this walk measured, the input the
    /// shared peak forecast charges its wrapper/parser copies against.
    pub(crate) fn metadata_byte_count(&self) -> usize {
        self.metadata_bytes
    }

    /// A pixel-less record whose adjustment body decoded
    /// (`psd_adjustment`): it imports as a Doctored adjustment layer.
    pub fn carried(&self, index: usize) -> Option<&Carried> {
        if !self.layers.get(index).is_some_and(LayerPixels::is_empty) {
            return None;
        }
        match self.adjustments.get(index) {
            Some(AdjustmentBody::Carried(carried)) => Some(carried),
            _ => None,
        }
    }

    /// A record whose `PtFl`, `SoCo` or `GdFl` decoded (`psd_fill_layer`), in
    /// any of its forms: it imports as a Doctored pattern, solid color or
    /// gradient fill layer.
    pub fn fill(&self, index: usize) -> Option<&crate::psd_fill_layer::CarriedFill> {
        self.fills.get(index)?.as_ref()
    }

    /// THE one predicate for "this retained record imports as a canvas-sized
    /// raster at the origin", which admission charges as the canvas, never
    /// as its rectangle: a pixel-less record that is not omitted (an empty
    /// transparent raster, or a carried adjustment), and a carried fill layer
    /// whatever its rectangle (`psd_fill_layer`; Photoshop's own form has
    /// the mask's): a Pattern Fill, a Solid Color Fill or a Gradient
    /// Fill (`GdFl`); a fill kind carried later is charged here too.
    pub fn canvas_raster(&self, index: usize) -> bool {
        !self.omitted(index)
            && (self.layers.get(index).is_some_and(LayerPixels::is_empty)
                || self.fill(index).is_some())
    }

    /// THE one predicate for "this record is dropped by Import Layers": a
    /// pixel-less record carrying an adjustment or fill key whose body is not
    /// carried. Photoshop's adjustment and fill layers are exactly that.
    pub fn omitted(&self, index: usize) -> bool {
        self.layers.get(index).is_some_and(LayerPixels::is_empty)
            && self
                .layer_features
                .get(index)
                .is_some_and(|features| features.adjustment_key().is_some())
            && self.carried(index).is_none()
            && self.fill(index).is_none()
    }

    /// True when a layered import would retain no record at all — every record
    /// is omitted and there is no folder. The empty layer section satisfies it
    /// too, so it subsumes the old "no layers and no groups" test.
    pub fn retains_nothing(&self) -> bool {
        self.groups.is_empty() && (0..self.layers.len()).all(|index| self.omitted(index))
    }

    /// The scratch a carried RGB solid's or gradient's render adds to a
    /// layered import: one high-depth encoder
    /// (`authored_color::wide_scratch_bytes`) when a carried fill holds a
    /// wide `linear` colour (`FillKind::is_wide`), plus the sRGB → profile
    /// transform's (`PrecisionTransform::prepare_scratch_bytes`) when any
    /// carried fill renders in the profile (`FillKind::renders_in_profile`)
    /// and `profile` has a matrix model. 0 when none is carried.
    pub fn fill_scratch_bytes(&self, profile: Option<&crate::icc::IccProfile>) -> u64 {
        let fills = || self.fills.iter().flatten().map(|fill| &fill.kind);
        let wide = if fills().any(crate::psd_fill_layer::FillKind::is_wide) {
            crate::authored_color::wide_scratch_bytes()
        } else {
            0
        };
        let in_profile = fills().find(|kind| kind.renders_in_profile());
        let transform = match (in_profile, profile.and_then(|p| p.model())) {
            (Some(_), Some(model)) => crate::icc::IccProfile::srgb().model().map_or(0, |srgb| {
                crate::icc_transform::PrecisionTransform::prepare_scratch_bytes(srgb, model)
            }),
            _ => 0,
        };
        wide.saturating_add(transform)
    }

    /// Admission precedes both metadata-wrapper copies and any pixel codec.
    /// `profile` is the file's own, for [`GroupRecords::fill_scratch_bytes`].
    pub fn admit(
        &self,
        container: &crate::psd_container::Container<'_>,
        source_bytes: usize,
        profile: Option<&crate::icc::IccProfile>,
    ) -> Result<(), String> {
        let mut masks = MaskBudget::default();
        for mask in self.masks.iter().chain(self.groups.iter().map(|g| &g.mask)) {
            masks.add(*mask)?;
        }
        let omitted: Vec<bool> = (0..self.layers.len()).map(|i| self.omitted(i)).collect();
        let canvas: Vec<bool> = (0..self.layers.len())
            .map(|i| self.canvas_raster(i))
            .collect();
        crate::psd_layer_pixels::admit(
            &self.layers,
            &omitted,
            &canvas,
            self.groups.len(),
            source_bytes,
            self.metadata_bytes,
            u64::from(container.width) * u64::from(container.height),
            masks,
            container.sample_bytes(),
            if container.color == 4 { 5 } else { 4 },
            self.fill_scratch_bytes(profile),
        )?;
        for layer in &self.layers {
            layer.validate(container.color, container.sample_bytes())?;
        }
        for mask in self.masks.iter().chain(self.groups.iter().map(|g| &g.mask)) {
            mask.validate(container.sample_bytes())?;
        }
        Ok(())
    }

    /// Keep the existing decoder's names/properties/tree interpretation while
    /// removing all original channel data. Its signed-bound conversion and u16
    /// row count cannot corrupt raw-channel framing. Each admitted record keeps
    /// its ordinal and exact metadata bytes; channels=0, rectangle=0. The tiny
    /// RGB canvas/composite belong only to this metadata view, never the document.
    pub fn metadata_psd(&self) -> Vec<u8> {
        let info_len = 2 + self.metadata.len() * 18 + self.metadata_bytes;
        let padded = info_len + info_len % 2;
        let mut bytes = Vec::with_capacity(51 + padded);
        bytes.extend_from_slice(b"8BPS\0\x01\0\0\0\0\0\0");
        bytes.extend_from_slice(&3u16.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&8u16.to_be_bytes());
        bytes.extend_from_slice(&3u16.to_be_bytes());
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&((padded + 8) as u32).to_be_bytes());
        bytes.extend_from_slice(&(padded as u32).to_be_bytes());
        bytes.extend_from_slice(&(self.metadata.len() as i16).to_be_bytes());
        for metadata in &self.metadata {
            bytes.extend_from_slice(&[0; 18]);
            bytes.extend_from_slice(metadata);
        }
        if info_len != padded {
            bytes.push(0);
        }
        bytes.extend_from_slice(&[0; 9]); // global mask length, raw RGB composite
        bytes
    }

    pub fn verify(&self, psd: &psd::Psd) -> Result<(), String> {
        let groups_match = self.groups.len() == psd.groups().len()
            && self.groups.iter().enumerate().all(|(i, g)| {
                psd.groups()
                    .get(&(i as u32 + 1))
                    .is_some_and(|parsed| parsed.parent_id() == g.parent)
            });
        let layers_match = self.parents.len() == psd.layers().len()
            && self
                .parents
                .iter()
                .zip(psd.layers())
                .all(|(parent, layer)| *parent == layer.parent_id());
        if !groups_match || !layers_match {
            return Err("PSD layer decoder disagrees with bounded group record framing".into());
        }
        Ok(())
    }
}

/// A layer record whose first adjustment key `key` is a Pattern Fill, a
/// Solid Color Fill or a Gradient Fill, RGB or CMYK:
/// the walk decodes it
/// in every form (`psd_fill_layer::Form`) and its pixel precheck defers it
/// to `admit`'s canvas-raster charge. A record with an advanced mask
/// (`advanced`: a vector mask `vmsk`/`vsms`, or `lmgm`/`vmgm`) is not one:
/// Photoshop's SHAPE layer is a `SoCo` under a vector mask, which layered
/// import does not carry, so a fill there would cover the canvas; it keeps
/// its pixels and marker losses (a pixel-less one is omitted), as before.
fn fill_candidate(divider: u32, key: [u8; 4], advanced: bool) -> bool {
    divider == 0 && !advanced && crate::psd_fill_layer::is_key(key)
}

/// A record's reference point from its first `fxrp` body (`Group::reference`):
/// each coordinate rounded and clamped to i32; (0, 0) when absent, malformed
/// or not finite.
fn reference_point(body: Option<&[u8]>) -> (i32, i32) {
    let clamp = |v: f64| v.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
    body.and_then(crate::psd_pattern_fill::reference_of)
        .filter(|(x, y)| x.is_finite() && y.is_finite())
        .map_or((0, 0), |(x, y)| (clamp(x), clamp(y)))
}

/// FILE record 0 in D5's own terms, or `None` when there is no record 0 or it
/// is not an ordinary layer record. Every clause is framed metadata.
fn bottom_record(records: &[Record<'_>]) -> Option<BottomRecord> {
    let first = records.first()?;
    if first.divider != 0 {
        return None;
    }
    // A true Photoshop Background has a rectangle and ONLY the colour
    // channels: no id `-1` at all. Every ordinary bottom layer has one.
    let background = !first.pixels.is_empty() && !first.lengths.iter().any(|(id, _)| *id == -1);
    let plain = !first.adjustment_record
        && first.flags & 2 == 0
        && first.base_blend == *b"norm"
        && first.opacity_byte == 255
        && !first.group.mask.enabled()
        && records.get(1).is_none_or(|next| !next.group.clipped);
    Some(BottomRecord {
        background,
        plain,
        bounds: first.pixels.bounds,
    })
}

/// Whether ONE record's carried style punches all the way to the document's
/// bottom: Deep always, Shallow only with no parent group.
fn reaches_background(style: &StyleBody, root: bool) -> bool {
    let StyleBody::Carried(carried) = style else {
        return false;
    };
    match carried.style.knockout {
        crate::style::Knockout::Deep => true,
        crate::style::Knockout::Shallow => root,
        crate::style::Knockout::None => false,
    }
}

/// The dependency's documented blend discriminants as file keys, in
/// `crate::psd::map_psd_blend` order (index + 1 is that discriminant), without
/// importing its private enum. The export side reads the same table backwards
/// (`psd_export::blend_key`), so there is exactly one key table in the crate.
pub(crate) const PSD_BLEND_KEYS: [[u8; 4]; 27] = [
    *b"norm", *b"diss", *b"dark", *b"mul ", *b"idiv", *b"lbrn", *b"dkCl", *b"lite", *b"scrn",
    *b"div ", *b"lddg", *b"lgCl", *b"over", *b"sLit", *b"hLit", *b"vLit", *b"lLit", *b"pLit",
    *b"hMix", *b"diff", *b"smud", *b"fsub", *b"fdiv", *b"hue ", *b"sat ", *b"colr", *b"lum ",
];

/// Share the existing layer-mode conversion; the key table reproduces the
/// dependency's documented discriminants, without importing its private enum.
fn blend(key: [u8; 4]) -> Result<BlendMode, String> {
    if &key == b"pass" {
        return Ok(BlendMode::PassThrough);
    }
    PSD_BLEND_KEYS
        .iter()
        .position(|candidate| *candidate == key)
        .map(|index| crate::psd::map_psd_blend(index as i32 + 1))
        .ok_or_else(|| format!("unsupported PSD group blend key {:02x?}", key))
}
