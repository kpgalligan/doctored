//! Bounded layered-container framing for import choices, ICC/PPI and the saved
//! composite: a classic PSD (version 1), a large-document PSB (version 2) or a
//! TIFF carrying Photoshop's layer block (`tiff_layers`). This reads section
//! lengths and image resources, never discarded layer records.
//! Container/channel framing follows Adobe's public file format specification:
//! https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/
//! and every width that differs between the containers comes from
//! `psd_framing` (`Container::kind`).
//!
//! The layer-information section admits up to three trailing pad bytes, because Photoshop rounds
//! its 8-bit section to a multiple of 4 rather than to an even count. `Container::layer_info` is
//! the ONE place that decides WHERE the records are: a TIFF's own block; else the ordinary
//! layer-information sub-section when it is non-empty (any depth); else, at depth 16 only, the
//! first `Lr16` tail block after the global-mask section (Adobe's signature + key + length walk,
//! each block padded to a MULTIPLE OF 4), and at depth 32 the first `Lr32` block the same way;
//! else empty — which is the form Photoshop's own 16- and 32-bit files take, and is where
//! `merged_alpha`'s sign comes from too. The `8B64` signature is admitted
//! only in a PSB, where the `psd_framing::LONG_LENGTH_KEYS` blocks carry a u64 length; in a
//! classic file it, or a bad tail signature, refuses the LAYERED choice only, never the stored
//! composite. Saved merged transparency alone is reconstructed from white-matted RGB with exact
//! alpha, nearest/clamped byte color and canonical zero-alpha RGB; ordinary layer/mask planes
//! never pass through that reconstruction.
//!
//! The stored composite is decoded in-crate at both depths: Raw planes copy, RLE rows come from
//! `psd_framing::rle_rows` and the one PackBits decoder, and a ZIP composite (compression words 2
//! and 3) is ONE zlib stream of every channel's rows, of which `psd_zip::unpack_zip_rows` inflates
//! only the colour planes and merged alpha this open keeps. A layered TIFF has no stored
//! composite here: its flat image is the TIFF's own strips, which the ordinary TIFF open reads.
//! The stored composite's spot planes (Raw or RLE, after the colour planes and merged alpha) are
//! `psd_spot`'s; [`Container::tag`] attaches them to a byte document.
//!
//! A CMYK file (colour mode 4, #1 phase 3) — PSD, PSB or a big-endian layered CMYK TIFF —
//! has its own admission ([`Container::cmyk_layered_admission`] for the layers,
//! [`Container::cmyk_admission`] for the saved composite) and its own profile read
//! ([`Container::cmyk_profile_bytes`]); `support_refusal`, `profile` and the byte/16-bit
//! composites stay RGB/gray only.
//!
//! A Grayscale file (colour mode 1, #1 phase 3a) is admitted by `support_refusal` at 8 and
//! 16 bits — PSD, PSB or a big-endian layered gray TIFF (one colour channel) — and opens as a
//! Gray document (`psd_import_gray`): [`Container::composite16`] reads its one plane into
//! R = G = B with the merged alpha at channel 1, and its profile bytes are
//! [`Container::profile_bytes`] (resource 1039, else a TIFF's ICC tag), read through
//! `GrayPolicy` rather than [`Container::profile`].
//!
//! A Lab file (colour mode 9, #1 Lab phase 3a) at 8 or 16 bits is refused by `support_refusal`
//! by name ("opens only as a Lab Color document"), so the RGB and gray opens and every composite
//! here keep refusing it; `psd_import_lab::lab_admission` admits it and
//! `psd_import_lab::lab_composite` reads its stored composite.
//!
//! A Duotone file (colour mode 8, #1 Duotone phase 1b) at 8 or 16 bits is refused by
//! `support_refusal` by name ([`DUOTONE_ONLY`]); the gray open and report read it through
//! [`Container::duotone_gray`] (the same container as colour mode 1) and its colour-mode data
//! ([`Container::color_mode_data`]) through `duotone_files`.
//!
//! An Indexed Color file (colour mode 2, #1) is refused by `support_refusal` by name at every
//! depth ([`INDEXED_ONLY`]), so the byte, 16-bit and 32-bit opens and composites refuse it;
//! `indexed_psd` reports and opens it from its [`Container::color_mode_data`] table and its
//! stored composite.
//!
//! A Multichannel file (colour mode 7, #1, `designs/multichannel-mode.md`) is refused by
//! `support_refusal` by name at every depth ([`MULTICHANNEL_REFUSAL`]), so the byte, 16-bit and
//! 32-bit opens and the report refuse it; the Gray, Lab and CMYK admissions already refuse it
//! as "a Multichannel file is not …".
//!
//! A 32-bit RGB file is admitted by `support_refusal` and opens only as a 32-bit float
//! document (`psd_import_float`, whose `composite32` reads the stored composite through
//! [`Container::composite_planes`]); the byte [`Container::composite`] refuses it by name. A
//! 32-bit float RGB layered TIFF is the same container with its `Lr32` block's records
//! and, as every TIFF, no stored composite.
//! A 32-bit Grayscale file (#1 phase 3c) is admitted the same way and opens as a 32-bit
//! Gray document (`psd_import_gray32`; `composite32` reads its one plane into R = G = B).

use std::sync::Arc;

use image::{Rgba, RgbaImage};

use crate::doc::{DrDocument, MAX_PIXELS};
use crate::icc::IccProfile;
use crate::metadata::Resolution;
use crate::psd_framing::{
    rle_rows, validate_packbits, LayeredContainer, LONG_LENGTH_KEYS, MAX_PSD_CHANNELS,
};
use crate::psd_samplers::SamplerImport;
use crate::style::GlobalLight;

pub use crate::psd_framing::unpack_packbits;

/// What a TIFF answers when its stored composite is asked for.
pub(crate) const TIFF_COMPOSITE: &str = "a TIFF's flat image opens through the ordinary TIFF open";

/// The sentence every PSD/PSB open and the compatibility report give for colour mode 7.
pub(crate) const MULTICHANNEL_REFUSAL: &str = "Multichannel files are not supported";

/// `support_refusal`'s sentence for colour mode 8 (#1 Duotone phase 1b).
pub(crate) const DUOTONE_ONLY: &str = "a Duotone file opens only as a Duotone document";

/// `support_refusal`'s sentence for colour mode 2 (#1 Indexed Color PSD).
pub(crate) const INDEXED_ONLY: &str =
    "an Indexed Color file opens only as an Indexed Color document";

pub(crate) struct Container<'a> {
    /// Which file this is; `kind.large()` selects every PSB width.
    pub kind: LayeredContainer,
    pub width: u32,
    pub height: u32,
    pub depth: u16,
    pub color: u16,
    pub channels: u16,
    pub merged_alpha: bool,
    pub layer_section: &'a [u8],
    pub resources: &'a [u8],
    pub composite: &'a [u8],
    /// The colour-mode data section, verbatim (a Duotone file's duotone
    /// specification, `duotone_ado`); empty for a TIFF.
    pub color_mode_data: &'a [u8],
    /// A layered TIFF's layer-information body (the `Layr`/`Lr16` block) and
    /// its IFD ICC tag; `None` for a PSD or PSB.
    tiff: Option<(&'a [u8], Option<&'a [u8]>)>,
    /// A layered TIFF's whole tagged-block list (empty for a PSD or PSB).
    tiff_blocks: &'a [u8],
    /// A layered TIFF's whole file, whose trailing extra samples hold its
    /// spot channels (`tiff_spot::layered_spot_records`); `None` for a PSD
    /// or PSB.
    pub(crate) tiff_file: Option<&'a [u8]>,
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let b = bytes
        .get(offset..offset + 2)
        .ok_or("truncated PSD header")?;
    Ok(u16::from_be_bytes([b[0], b[1]]))
}
fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let b = bytes
        .get(offset..offset + 4)
        .ok_or("truncated PSD section")?;
    Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}
/// One length-prefixed section: a u32 length, or a u64 one when `wide`.
fn section_of<'a>(bytes: &'a [u8], cursor: &mut usize, wide: bool) -> Result<&'a [u8], String> {
    let len = if wide {
        let b = bytes
            .get(*cursor..cursor.checked_add(8).ok_or("PSD section overflow")?)
            .ok_or("truncated PSD section")?;
        let len = u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
        *cursor += 8;
        usize::try_from(len).map_err(|_| "PSD section overflow")?
    } else {
        let len = u32_at(bytes, *cursor)? as usize;
        *cursor = cursor.checked_add(4).ok_or("PSD section overflow")?;
        len
    };
    let end = cursor.checked_add(len).ok_or("PSD section overflow")?;
    let out = bytes.get(*cursor..end).ok_or("truncated PSD section")?;
    *cursor = end;
    Ok(out)
}
/// The tagged-block walk from `cursor` (signature, key, length, body, then
/// the next multiple of four): the body of the first block keyed `key`.
fn walk_tail(bytes: &[u8], mut cursor: usize, key: [u8; 4], large: bool) -> Option<&[u8]> {
    while cursor.checked_add(12)? <= bytes.len() {
        match bytes.get(cursor..cursor + 4)? {
            b"8BIM" => {}
            b"8B64" if large => {}
            _ => return None,
        }
        let found: [u8; 4] = bytes.get(cursor + 4..cursor + 8)?.try_into().ok()?;
        let mut start = cursor + 8;
        let body = section_of(
            bytes,
            &mut start,
            large && LONG_LENGTH_KEYS.contains(&found),
        )
        .ok()?;
        if found == key {
            return Some(body);
        }
        cursor = start.checked_add(body.len().wrapping_neg() & 3)?;
    }
    None
}

fn section<'a>(bytes: &'a [u8], cursor: &mut usize) -> Result<&'a [u8], String> {
    section_of(bytes, cursor, false)
}

/// Image resource `wanted`'s body in an 8BIM run (a PSD's resource section,
/// a TIFF's tag 34377): `None` when absent, `Err` for a malformed run or a
/// duplicate. [`Container::resource`] and `tiff_spot` both walk through it.
pub(crate) fn resource_in(resources: &[u8], wanted: u16) -> Result<Option<&[u8]>, String> {
    let mut cursor = 0;
    let mut found = None;
    while cursor < resources.len() {
        if resources.get(cursor..cursor + 4) != Some(b"8BIM") {
            return Err("invalid PSD image resource signature".into());
        }
        let id = u16_at(resources, cursor + 4)?;
        cursor += 6;
        let name_len = *resources.get(cursor).ok_or("truncated PSD resource name")? as usize;
        cursor = cursor
            .checked_add((name_len + 2) & !1)
            .ok_or("PSD resource overflow")?;
        let body = section(resources, &mut cursor)?;
        if body.len() % 2 != 0 {
            cursor += 1;
        }
        if cursor > resources.len() {
            return Err("truncated PSD resource padding".into());
        }
        if id == wanted {
            if found.is_some() {
                return Err(format!("duplicate PSD image resource {wanted}"));
            }
            found = Some(body);
        }
    }
    Ok(found)
}

impl<'a> Container<'a> {
    pub fn read(bytes: &'a [u8]) -> Result<Self, String> {
        if matches!(bytes.get(..2), Some(b"MM" | b"II")) {
            return Self::read_tiff(bytes);
        }
        if bytes.get(..4) != Some(b"8BPS") {
            return Err("not a PSD file".into());
        }
        let kind = match u16_at(bytes, 4)? {
            1 => LayeredContainer::Psd,
            2 => LayeredContainer::Psb,
            _ => return Err("unknown PSD version; only PSD (1) and PSB (2) are read".into()),
        };
        if bytes.get(6..12) != Some(&[0; 6]) {
            return Err("invalid PSD reserved header bytes".into());
        }
        let channels = u16_at(bytes, 12)?;
        let height = u32_at(bytes, 14)?;
        let width = u32_at(bytes, 18)?;
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
            return Err("PSD canvas exceeds the 100 MP editing limit or is empty".into());
        }
        if !(1..=MAX_PSD_CHANNELS as u16).contains(&channels) {
            return Err("invalid PSD channel count".into());
        }
        let mut cursor = 26;
        let color_mode_data = section(bytes, &mut cursor)?;
        let resources = section(bytes, &mut cursor)?;
        let layers = section_of(bytes, &mut cursor, kind.large())?;
        let mut container = Self {
            kind,
            width,
            height,
            channels,
            depth: u16_at(bytes, 22)?,
            color: u16_at(bytes, 24)?,
            resources,
            layer_section: layers,
            merged_alpha: false,
            composite: bytes.get(cursor..).ok_or("truncated PSD composite")?,
            color_mode_data,
            tiff: None,
            tiff_blocks: &[],
            tiff_file: None,
        };
        container.merged_alpha = container.declares_merged_alpha();
        Ok(container)
    }

    /// A TIFF whose tag 37724 holds a Photoshop layer block, as the same
    /// container a PSD is. Only what `tiff_layers::locate` admits arrives
    /// here — a classic big-endian unsigned RGB, CMYK, gray or Lab TIFF at 8
    /// or 16 bits, or a big-endian 32-bit float RGB one (a
    /// little-endian one arrives as `tiff_layers_order::big_endian_form`'s
    /// big-endian form) — and the colour mode is the one it located (3, 4, 1 or 9). The channel count
    /// is that model's colour planes plus the block's own merged-alpha
    /// declaration: a TIFF block carries no saved channels.
    fn read_tiff(bytes: &'a [u8]) -> Result<Self, String> {
        let located = crate::tiff_layers::locate(bytes)
            .ok_or("this TIFF has no readable Photoshop layer block")?;
        if located.width == 0
            || located.height == 0
            || u64::from(located.width) * u64::from(located.height) > MAX_PIXELS
        {
            return Err("TIFF canvas exceeds the 100 MP editing limit or is empty".into());
        }
        let mut container = Self {
            kind: LayeredContainer::Tiff,
            width: located.width,
            height: located.height,
            channels: match located.color {
                4 => 4,
                1 => 1,
                _ => 3,
            },
            depth: located.depth,
            color: located.color,
            resources: located.resources,
            layer_section: &[],
            merged_alpha: false,
            composite: &[],
            color_mode_data: &[],
            tiff: Some((located.info, located.icc)),
            tiff_blocks: located.blocks,
            tiff_file: Some(bytes),
        };
        container.merged_alpha = container.declares_merged_alpha();
        container.channels += u16::from(container.merged_alpha);
        Ok(container)
    }

    /// The sign alone describes merged transparency; no layer record is
    /// parsed. Additional channels otherwise remain saved alpha channels.
    /// The count lives wherever the records do — the ordinary section at
    /// 8 bits, the `Lr16` tail at 16, a TIFF's block — and a file whose
    /// records cannot be located at all simply has no merged alpha to declare.
    /// A 32-bit file may instead declare it the way Photoshop 26.10.0 writes
    /// its own: a POSITIVE count and an empty `Mt32` tail block,
    /// whose presence alone is the declaration (Adobe's "Saving Merged
    /// Transparency" keys). Read at depth 32 only, so 8- and 16-bit files
    /// answer exactly as before.
    fn declares_merged_alpha(&self) -> bool {
        let signed = self
            .layer_info()
            .ok()
            .filter(|info| info.len() >= 2)
            .map(|info| (u16::from_be_bytes([info[0], info[1]]) as i16) < 0)
            .unwrap_or(false);
        signed || (self.depth == 32 && self.tail_block(*b"Mt32").is_some())
    }

    /// The body `GroupRecords::read` consumes — the record count, the records
    /// themselves and their channel data — wherever this file keeps it.
    ///
    /// Photoshop writes 8-bit records into the ordinary layer-information
    /// sub-section, but a 16-bit file's section is EMPTY and its records live
    /// in an `Lr16` tagged block in the tail of the layer-and-mask section,
    /// after the global layer mask. The tail walk is Adobe's: signature,
    /// four-byte key, length, body, then advance to the next multiple of
    /// four. In a PSB the section length is a u64, the signature may be
    /// `8B64`, and a `LONG_LENGTH_KEYS` block's length is a u64 (`Lr16` is one
    /// of them). In a classic file `8B64`, or in either a bad
    /// signature/truncation, refuses the LAYERED choice only; the stored
    /// composite is unaffected. A TIFF's block IS this body.
    pub fn layer_info(&self) -> Result<&'a [u8], String> {
        if let Some((info, _)) = self.tiff {
            return Ok(info);
        }
        let large = self.kind.large();
        let mut cursor = 0usize;
        // This sub-section used to be framed by the record walk's own cursor,
        // whose sentence for an overrunning length this keeps byte-for-byte.
        let info = section_of(self.layer_section, &mut cursor, large)
            .map_err(|_| "truncated PSD layer record".to_string())?;
        if !info.is_empty() || !matches!(self.depth, 16 | 32) {
            return Ok(info);
        }
        // Photoshop's own high-depth form: `Lr16` at 16 bits, `Lr32` at 32.
        let wanted = if self.depth == 32 { b"Lr32" } else { b"Lr16" };
        // The global layer mask section, then the tagged blocks.
        if cursor + 4 > self.layer_section.len() {
            return Ok(&[]);
        }
        section(self.layer_section, &mut cursor)?;
        while cursor + 12 <= self.layer_section.len() {
            match self.layer_section.get(cursor..cursor + 4) {
                Some(b"8BIM") => {}
                Some(b"8B64") if large => {}
                Some(b"8B64") => return Err(
                    "PSD 64-bit tagged layer blocks (8B64) are unsupported by the layer decoder"
                        .into(),
                ),
                _ => return Err("invalid PSD tagged layer block signature".into()),
            }
            let key: [u8; 4] = self.layer_section[cursor + 4..cursor + 8]
                .try_into()
                .unwrap();
            let mut start = cursor + 8;
            let body = section_of(
                self.layer_section,
                &mut start,
                large && LONG_LENGTH_KEYS.contains(&key),
            )
            .map_err(|_| "truncated PSD tagged layer block".to_string())?;
            if &key == wanted {
                return Ok(body);
            }
            // Photoshop pads every tail block to a MULTIPLE OF 4 (its own
            // `Lr16` body of 9413 bytes is followed by the next block at
            // +9416), not to an even count.
            cursor = start
                .checked_add(body.len().wrapping_neg() & 3)
                .ok_or("PSD tail block overflow")?;
        }
        Ok(&[])
    }

    /// The FIRST document-level tail block keyed `key` — the walk
    /// [`layer_info`](Self::layer_info) runs for `Lr16`, generalised: the
    /// tagged blocks after the global layer mask in a PSD or PSB, or the
    /// tag-37724 list of a layered TIFF. `None` when absent or when the walk
    /// cannot frame a block (a pattern reference then declines by name).
    pub fn tail_block(&self, key: [u8; 4]) -> Option<&'a [u8]> {
        if self.tiff.is_some() {
            return walk_tail(self.tiff_blocks, 0, key, false);
        }
        let large = self.kind.large();
        let mut cursor = 0usize;
        section_of(self.layer_section, &mut cursor, large).ok()?;
        if cursor + 4 > self.layer_section.len() {
            return None;
        }
        section(self.layer_section, &mut cursor).ok()?;
        walk_tail(self.layer_section, cursor, key, large)
    }

    pub fn support_refusal(&self) -> Option<String> {
        // Multichannel (#1, `designs/multichannel-mode.md`) is refused by name
        // at every depth and size, before any other check: it is not built.
        if self.color == 7 {
            return Some(MULTICHANNEL_REFUSAL.into());
        }
        let side = self.kind.max_side();
        if self.width > side || self.height > side {
            return Some(match self.kind {
                LayeredContainer::Psd => {
                    "PSD canvas exceeds the classic format's 30000-pixel dimension limit".into()
                }
                kind => format!(
                    "{} canvas exceeds the format's {side}-pixel dimension limit",
                    kind.name()
                ),
            });
        }
        // A Lab file (#1 Lab phase 3a) is `psd_import_lab::lab_admission`'s:
        // the RGB and gray opens, and every composite here, refuse it.
        if self.color == 9 && matches!(self.depth, 8 | 16) {
            return Some("a Lab Color file opens only as a Lab Color document".into());
        }
        // A Duotone file (#1 Duotone phase 1b) opens through
        // [`Container::duotone_gray`]: the RGB opens and every composite
        // here refuse it.
        if self.color == 8 && matches!(self.depth, 8 | 16) {
            return Some(DUOTONE_ONLY.into());
        }
        // An Indexed Color file (#1) opens through `indexed_psd`: the RGB
        // opens and every composite here refuse it.
        if self.color == 2 {
            return Some(INDEXED_ONLY.into());
        }
        if !matches!((self.depth, self.color), (8 | 16 | 32, 1 | 3)) {
            return Some(format!("{}-bit {} PSD cannot be imported without an explicit color/depth conversion; only 8- and 16-bit RGB and grayscale and 32-bit RGB and grayscale are supported", self.depth, self.color_name()));
        }
        let base = if self.color == 3 { 3 } else { 1 };
        if self.channels < base {
            return Some("PSD has too few color channels".into());
        }
        None
    }

    /// Bytes per stored sample: 1 for an 8-bit file, 2 for a 16-bit one, 4
    /// for a 32-bit one.
    /// Every framing length, row width and decode budget scales by this.
    pub fn sample_bytes(&self) -> u64 {
        u64::from(self.depth) / 8
    }

    /// A Duotone file (colour mode 8) at 8 or 16 bits as the Grayscale file
    /// it stores (#1 Duotone phase 1b): the same container, colour mode 1,
    /// so the gray open and report read it unchanged; `None` for every other
    /// file (a TIFF holds no colour mode 8).
    pub(crate) fn duotone_gray(&self) -> Option<Container<'a>> {
        (self.color == 8 && matches!(self.depth, 8 | 16) && self.tiff.is_none())
            .then_some(Container { color: 1, ..*self })
    }

    pub fn color_name(&self) -> &'static str {
        match self.color {
            0 => "Bitmap",
            1 => "Grayscale",
            2 => "Indexed",
            3 => "RGB",
            4 => "CMYK",
            7 => "Multichannel",
            8 => "Duotone",
            9 => "Lab",
            _ => "Unknown",
        }
    }

    pub fn resource(&self, wanted: u16) -> Result<Option<&'a [u8]>, String> {
        resource_in(self.resources, wanted)
    }

    /// The embedded profile's bytes, verbatim: image resource 1039, else (a
    /// TIFF whose block has none) the IFD's ICC tag. [`Container::profile`]
    /// reads through it; a gray file's policy reads it directly
    /// (`psd_import_gray::gray_policy`). A CMYK file keeps its own order
    /// ([`Container::cmyk_profile_bytes`], the TIFF tag first).
    pub fn profile_bytes(&self) -> Result<Option<&'a [u8]>, String> {
        Ok(match self.resource(1039)? {
            Some(bytes) => Some(bytes),
            None => self.tiff.and_then(|(_, icc)| icc),
        })
    }

    pub fn profile(&self) -> Result<Option<Arc<IccProfile>>, String> {
        // A TIFF's profile is Photoshop's own resource when the block has one,
        // else the IFD's ICC tag.
        let bytes = self.profile_bytes()?;
        bytes.map(|bytes| IccProfile::parse(bytes).map(Arc::new)
            .ok_or_else(|| "PSD embedded ICC profile is unsupported or invalid; convert to an RGB profile before importing".into())).transpose()
    }

    pub fn resolution(&self) -> Result<Option<Resolution>, String> {
        let Some(body) = self.resource(1005)? else {
            return Ok(None);
        };
        if body.len() != 16 {
            return Err("invalid PSD resolution resource".into());
        }
        // ResolutionInfo stores 16.16 pixels/inch independently of the UI
        // unit selectors that follow each value (Adobe's resource 1005).
        let x = u32_at(body, 0)? as f32 / 65536.0;
        let y = u32_at(body, 8)? as f32 / 65536.0;
        if !(1.0..=30000.0).contains(&x) || !(1.0..=30000.0).contains(&y) {
            return Err("PSD resolution is outside the supported 1–30000 PPI range".into());
        }
        Ok(Some(Resolution { x, y }.sane()))
    }

    /// The document's shared light direction, from image resources 1037
    /// (global angle) and 1049 (global altitude), each a big-endian i32.
    /// `None` when neither is present; a missing half takes its default, and
    /// the pair goes through the one `GlobalLight::sane` canonicalizer.
    pub fn global_light(&self) -> Result<Option<GlobalLight>, String> {
        let read = |id: u16| -> Result<Option<f32>, String> {
            Ok(self
                .resource(id)?
                .filter(|body| body.len() >= 4)
                .map(|body| i32::from_be_bytes([body[0], body[1], body[2], body[3]]) as f32))
        };
        let (angle, altitude) = (read(1037)?, read(1049)?);
        if angle.is_none() && altitude.is_none() {
            return Ok(None);
        }
        let default = GlobalLight::default();
        Ok(Some(
            GlobalLight {
                angle: angle.unwrap_or(default.angle),
                altitude: altitude.unwrap_or(default.altitude),
            }
            .sane(),
        ))
    }

    /// The file's colour samplers: image resource 1073 decoded by
    /// `psd_samplers`, with its report sentences. Never a refusal of its
    /// own — `Err` only from the resource walk, which every other resource
    /// read shares. Photoshop also writes the OBSOLETE resource 1038; a file
    /// carrying it WITHOUT 1073 gets one observation, and it is never read.
    pub fn color_samplers(&self) -> Result<SamplerImport, String> {
        match self.resource(1073)? {
            Some(body) => Ok(crate::psd_samplers::decode(body, self.width, self.height)),
            None => Ok(SamplerImport {
                kept: Vec::new(),
                losses: Vec::new(),
                observations: if self.resource(1038)?.is_some() {
                    vec!["This file carries only Photoshop's obsolete color sampler resource (1038), which Doctored does not read; its color samplers are not imported.".into()]
                } else {
                    Vec::new()
                },
            }),
        }
    }

    pub fn tag(&self, mut document: DrDocument) -> Result<DrDocument, String> {
        if let Some(profile) = self.profile()? {
            document.profile = profile;
        }
        if let Some(resolution) = self.resolution()? {
            document.resolution = resolution;
        }
        if let Some(light) = self.global_light()? {
            document.global_light = light;
        }
        document.color_samplers = self.color_samplers()?.kept;
        document.with_psd_spots(self)
    }

    /// The kept planes of the stored composite, plane-major: the first
    /// `kept` channels of `sample_bytes`-wide samples, `kept * pixels *
    /// sample_bytes` bytes. Saved channels past them are framed (Raw, RLE) or
    /// never read at all (ZIP), and none is ever decoded.
    pub(crate) fn composite_planes(
        &self,
        kept: usize,
        sample_bytes: usize,
    ) -> Result<Vec<u8>, String> {
        let pixels = self.width as usize * self.height as usize;
        let row_bytes = self.width as usize * sample_bytes;
        let height = self.height as usize;
        let word = u16_at(self.composite, 0)?;
        match word {
            0 => {
                let expected = pixels
                    .checked_mul(self.channels as usize * sample_bytes)
                    .and_then(|v| v.checked_add(2))
                    .ok_or("PSD composite size overflow")?;
                if self.composite.len() != expected {
                    return Err("PSD raw composite length mismatch".into());
                }
                Ok(self.composite[2..2 + pixels * kept * sample_bytes].to_vec())
            }
            1 => {
                let rows = rle_rows(
                    &self.composite[2..],
                    self.channels as usize * height,
                    self.kind.large(),
                )?;
                let mut out = Vec::with_capacity(kept * pixels * sample_bytes);
                for (index, row) in rows.iter().enumerate() {
                    if index < kept * height {
                        out.extend_from_slice(&unpack_packbits(row, row_bytes)?);
                    } else {
                        validate_packbits(row, row_bytes)?;
                    }
                }
                Ok(out)
            }
            // ONE zlib stream of `channels * height` rows; only the kept
            // planes' rows are inflated (`psd_zip::unpack_zip_rows`).
            2 | 3 => crate::psd_zip::unpack_zip_rows(
                &self.composite[2..],
                row_bytes,
                kept * height,
                sample_bytes,
                word == 3,
            ),
            _ => Err("PSD stored composite uses unsupported compression".into()),
        }
    }

    /// Use only the composite's color planes and verified merged alpha,
    /// excluding saved channels. A broken layer record cannot hide an
    /// otherwise valid saved composite, because no record is read here.
    /// Merged transparency colors are white-matted, unlike straight layer
    /// samples. Their reconstruction belongs here, never in the shared codec.
    pub fn composite(&self) -> Result<RgbaImage, String> {
        if self.tiff.is_some() {
            return Err(TIFF_COMPOSITE.into());
        }
        if let Some(reason) = self.support_refusal() {
            return Err(reason);
        }
        if self.depth != 8 {
            // The flat/byte composite path is unchanged by the 16- and
            // 32-bit work: it still refuses, and names the typed paths
            // (`composite16`, `psd_import_float::composite32`).
            return Err(format!(
                "{}-bit PSD has no 8-bit composite; only 8-bit RGB and grayscale composites are decoded here, a 16-bit PSD opens only as a 16-bit document, and a 32-bit RGB PSD opens only as a 32-bit float document",
                self.depth
            ));
        }
        let base = if self.color == 3 { 3 } else { 1 };
        let alpha = self.merged_alpha && usize::from(self.channels) > base;
        let kept = base + usize::from(alpha);
        let pixels = self.width as usize * self.height as usize;
        let planes = self.composite_planes(kept, 1)?;
        if planes.len() != kept * pixels {
            return Err("PSD composite plane length mismatch".into());
        }
        let mut image = RgbaImage::from_pixel(self.width, self.height, Rgba([0, 0, 0, 255]));
        for (index, pixel) in image.pixels_mut().enumerate() {
            if self.color == 3 {
                for component in 0..kept {
                    pixel[component] = planes[component * pixels + index];
                }
            } else {
                let gray = planes[index];
                let a = if alpha { planes[pixels + index] } else { 255 };
                *pixel = Rgba([gray, gray, gray, a]);
            }
        }
        if alpha {
            // GIMP 3.2.6 psd-export.c/create_merged_image writes colors blended
            // against white; psd-load.c/add_merged_image removes that matte:
            // https://github.com/GNOME/gimp/tree/GIMP_3_2_6/plug-ins/file-psd
            // In byte codes the inverse is (stored + alpha - 255) * 255 / alpha.
            // Use exact widened arithmetic, nearest rounding (ties upward),
            // and saturation. Alpha0 has no recoverable color; canonicalize it.
            // Low-alpha source color was already quantized by the PSD writer
            // and cannot be recovered exactly. Raw layers/masks never enter
            // this branch, and every opaque sample remains byte-identical.
            for pixel in image.pixels_mut() {
                let a = i32::from(pixel[3]);
                match a {
                    0 => pixel.0[..3].fill(0),
                    255 => {}
                    _ => {
                        for color in &mut pixel.0[..3] {
                            let numerator = (i32::from(*color) + a - 255) * 255;
                            *color = ((numerator.max(0) + a / 2) / a).min(255) as u8;
                        }
                    }
                }
            }
        }
        Ok(image)
    }
}

/// A CMYK file's saved composite (#1, `cmyk_io`): its own admission —
/// colour mode 4, 8 or 16 bits, at least the four ink channels — and the four
/// ink planes as straight CMYKA16 INK (PSD stores `max − ink`; flipped here,
/// an 8-bit code ×257, alpha opaque). Merged alpha is not read. The byte
/// `composite` and `support_refusal` stay RGB/gray only: this is the one
/// place a CMYK saved composite is decoded (a CMYK layer's planes are
/// `psd_layer_pixels::decode_cmyk`'s).
impl<'a> Container<'a> {
    pub fn cmyk_admission(&self) -> Result<(), String> {
        if self.tiff.is_some() {
            return Err(TIFF_COMPOSITE.into());
        }
        let side = self.kind.max_side();
        if self.width > side || self.height > side {
            return Err(format!(
                "{} canvas exceeds the format's {side}-pixel dimension limit",
                self.kind.name()
            ));
        }
        if self.color != 4 {
            return Err(format!("a {} PSD is not CMYK", self.color_name()));
        }
        if !matches!(self.depth, 8 | 16) {
            return Err(format!(
                "{}-bit CMYK PSD cannot be opened; only 8- and 16-bit CMYK composites are read",
                self.depth
            ));
        }
        if self.channels < 4 {
            return Err("CMYK PSD has too few ink channels".into());
        }
        Ok(())
    }

    /// The LAYERED choice's admission of a CMYK file (#1 phase 3): the
    /// container's side limit, colour mode 4, 8 or 16 bits and the four ink
    /// channels. A layered CMYK TIFF is admitted (its records are the
    /// layers); only its stored composite is the ordinary TIFF open's.
    pub fn cmyk_layered_admission(&self) -> Result<(), String> {
        let side = self.kind.max_side();
        if self.width > side || self.height > side {
            return Err(format!(
                "{} canvas exceeds the format's {side}-pixel dimension limit",
                self.kind.name()
            ));
        }
        if self.color != 4 {
            return Err(format!("a {} PSD is not CMYK", self.color_name()));
        }
        if !matches!(self.depth, 8 | 16) {
            return Err(format!(
                "{}-bit CMYK PSD cannot be opened; only 8- and 16-bit CMYK files are read",
                self.depth
            ));
        }
        if self.channels < 4 {
            return Err("CMYK PSD has too few ink channels".into());
        }
        Ok(())
    }

    /// The embedded CMYK profile's bytes, verbatim: a TIFF's IFD ICC tag,
    /// else (and for a PSD or PSB) image resource 1039. Read through
    /// `CmykPolicy` by the caller, never through [`Container::profile`],
    /// which reads RGB and gray profiles only.
    pub fn cmyk_profile_bytes(&self) -> Result<Option<&'a [u8]>, String> {
        if let Some((_, Some(icc))) = self.tiff {
            return Ok(Some(icc));
        }
        self.resource(1039)
    }

    pub fn cmyk_composite(&self) -> Result<Vec<[u16; 5]>, String> {
        self.cmyk_admission()?;
        let pixels = self.width as usize * self.height as usize;
        let bytes = self.sample_bytes() as usize;
        let planes = self.composite_planes(4, bytes)?;
        if planes.len() != 4 * pixels * bytes {
            return Err("PSD composite plane length mismatch".into());
        }
        let mut out = vec![[0u16, 0, 0, 0, 65535]; pixels];
        for (component, plane) in planes.chunks_exact(pixels * bytes).enumerate() {
            for (pixel, sample) in out.iter_mut().zip(plane.chunks_exact(bytes)) {
                pixel[component] = if bytes == 2 {
                    65535 - u16::from_be_bytes([sample[0], sample[1]])
                } else {
                    (255 - u16::from(sample[0])) * 257
                };
            }
        }
        Ok(out)
    }
}

/// The 16-bit twin of [`Container::composite`]: the saved composite's colour
/// planes and verified merged alpha as straight RGBA16, through the same
/// plane walk; nothing else changes, including the reconstruction from white.
/// A gray file's one plane fills R = G = B and its merged alpha is channel 1
/// (#1 phase 3a), as the byte composite reads it.
impl Container<'_> {
    pub fn composite16(&self) -> Result<Vec<[u16; 4]>, String> {
        if self.tiff.is_some() {
            return Err(TIFF_COMPOSITE.into());
        }
        if let Some(reason) = self.support_refusal() {
            return Err(reason);
        }
        if self.depth != 16 {
            return Err("this PSD is not 16-bit".into());
        }
        let base = if self.color == 3 { 3 } else { 1 };
        let alpha = self.merged_alpha && usize::from(self.channels) > base;
        let kept = base + usize::from(alpha);
        let pixels = self.width as usize * self.height as usize;
        let planes = self.composite_planes(kept, 2)?;
        if planes.len() != kept * pixels * 2 {
            return Err("PSD composite plane length mismatch".into());
        }
        let mut image = vec![[0u16, 0, 0, 65535]; pixels];
        for (index, plane) in planes.chunks_exact(pixels * 2).enumerate() {
            // A gray file's plane 0 is R = G = B, and its alpha is plane 1.
            let component = if index == base { 3 } else { index };
            for (pixel, sample) in image.iter_mut().zip(plane.chunks_exact(2)) {
                let value = u16::from_be_bytes([sample[0], sample[1]]);
                if base == 1 && component == 0 {
                    pixel[..3].fill(value);
                } else {
                    pixel[component] = value;
                }
            }
        }
        if alpha {
            // The exact 16-bit twin of the byte reconstruction in
            // `composite`: widened arithmetic, nearest rounding, saturation,
            // and a canonical zero-alpha colour.
            for pixel in &mut image {
                let a = i64::from(pixel[3]);
                match a {
                    0 => pixel[..3].fill(0),
                    65535 => {}
                    _ => {
                        for color in &mut pixel[..3] {
                            let numerator = (i64::from(*color) + a - 65535) * 65535;
                            *color = (((numerator.max(0) + a / 2) / a).min(65535)) as u16;
                        }
                    }
                }
            }
        }
        Ok(image)
    }
}
