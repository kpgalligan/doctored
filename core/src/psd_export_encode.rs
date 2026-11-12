//! RGB PSD and PSB serialization at 8, 16 and 32 bits: PackBits rows, channel
//! planes, layer records, image resources and the white-matted composite
//! preview. At 32 bits (`psd_export_float`, [`write_psd32`]) every
//! plane is Raw ([`raw_plane`]), the records travel in Photoshop's `Lr32` tail
//! block after an EMPTY layer-information sub-section and the global layer
//! mask, and the composite is stored Raw. Framing follows Adobe's public file format specification:
//! https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/
//! Nothing here decides WHAT is written — `psd_export` owns the record walk,
//! the refusals and the loss inventory; this module only lays bytes down.
//!
//! `large` is the PSB form (`psd_framing`): version 2, u64 layer-section and
//! channel lengths, u32 RLE row counts. `large == false` writes exactly the
//! bytes the classic writer always wrote. [`layer_info`] and
//! [`image_resources`] are the two bodies a layered TIFF carries in its own
//! tags (`tiff_layers`), so all three containers share one record serializer.
//!
//! `tests/fixtures/psd_10c/*.psd` are committed goldens of the 8-bit writer's exact bytes:
//! regenerate them ONLY with an intentional 8-bit writer change, with
//! `DR_PSD_EXPORT_FIXTURES=tests/fixtures/psd_10c make t F=psd_export`.

use image::RgbaImage;

use crate::doc_sampler::ColorSampler;
use crate::metadata::Resolution;
use crate::precision_image::PrecisionImage;
use crate::psd_framing::LayeredContainer;
use crate::style::GlobalLight;

/// The layer count is a signed 16-bit field (negative = merged alpha declared),
/// so a file can hold no more records than `i16::MAX`.
pub(crate) const PSD_MAX_RECORDS: usize = 32_767;

/// PackBits one row. Repeat runs are emitted only for length 3 or more, so a
/// two-sample repeat stays inside its literal run; literal runs never exceed
/// 128 bytes; the control byte -128 (a no-op for some readers, a terminator
/// for others) is never produced.
///
/// The length bound this guarantees is `len <= w + ceil(w/128)`: an all-literal
/// row pays exactly one control byte per 128 samples, and every repeat run of
/// n >= 3 replaces n sample bytes with 2, saving at least one byte — which
/// pays for the single extra literal control byte that splitting a literal run
/// in two can cost. So no substitution of a repeat run can push a row past the
/// all-literal worst case.
pub(crate) fn packbits_row(row: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(row.len() + row.len().div_ceil(128));
    let mut literal = 0usize;
    let mut cursor = 0usize;
    while cursor < row.len() {
        let mut run = 1usize;
        while run < 128 && cursor + run < row.len() && row[cursor + run] == row[cursor] {
            run += 1;
        }
        if run < 3 {
            cursor += 1;
            continue;
        }
        flush_literal(&mut out, &row[literal..cursor]);
        out.push((1i16 - run as i16) as i8 as u8);
        out.push(row[cursor]);
        cursor += run;
        literal = cursor;
    }
    flush_literal(&mut out, &row[literal..]);
    out
}

fn flush_literal(out: &mut Vec<u8>, data: &[u8]) {
    for chunk in data.chunks(128) {
        out.push((chunk.len() - 1) as u8);
        out.extend_from_slice(chunk);
    }
}

/// One row's byte count in an RLE table: a u16, or a u32 in a PSB.
fn push_row_count(table: &mut Vec<u8>, count: usize, large: bool) {
    if large {
        table.extend_from_slice(&(count as u32).to_be_bytes());
    } else {
        table.extend_from_slice(&(count as u16).to_be_bytes());
    }
}

/// One channel plane: compression word 1 (RLE), a byte count per row (u16, or
/// u32 when `large`), then the rows. `samples` is exactly `row_bytes * height`
/// bytes, row 0 first. Rows are BYTES, not samples: a 16-bit plane passes
/// `width * 2`, and its big-endian pairs are packed as the byte stream Adobe's
/// format stores.
pub(crate) fn encode_plane(samples: &[u8], row_bytes: u32, height: u32, large: bool) -> Vec<u8> {
    let width = row_bytes as usize;
    let mut table = Vec::with_capacity(height as usize * if large { 4 } else { 2 });
    let mut body = Vec::new();
    for y in 0..height as usize {
        let row = samples.get(y * width..(y + 1) * width).unwrap_or(&[]);
        let encoded = packbits_row(row);
        push_row_count(&mut table, encoded.len(), large);
        body.extend_from_slice(&encoded);
    }
    let mut out = Vec::with_capacity(2 + table.len() + body.len());
    out.extend_from_slice(&[0, 1]);
    out.extend_from_slice(&table);
    out.extend_from_slice(&body);
    out
}

/// The largest [`encode_plane`] result for a plane of `height` rows of
/// `row_bytes` bytes each: the compression word, the row table (4 bytes a row
/// when `large`, else 2), and each row's all-literal worst case.
pub(crate) fn plane_worst_case(row_bytes: u64, height: u64, large: bool) -> u64 {
    2u64.saturating_add(height.saturating_mul(if large { 4 } else { 2 }))
        .saturating_add(height.saturating_mul(row_bytes.saturating_add(row_bytes.div_ceil(128))))
}

/// One Raw channel plane (the 32-bit form): compression word 0, then the
/// plane's bytes as they stand.
pub(crate) fn raw_plane(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + bytes.len());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(bytes);
    out
}

/// One layer record's 20-byte mask body: its rectangle, the coverage outside
/// it, and whether the mask is disabled. Placement is always absolute.
pub(crate) struct MaskBody {
    pub bounds: [i32; 4],
    pub outside: u8,
    pub disabled: bool,
}

/// One layer record. `channels` is written in order, and `mask.is_some()` iff
/// `channels` contains the user-mask id -2.
pub(crate) struct Record {
    pub bounds: [i32; 4],
    pub channels: Vec<(i16, Vec<u8>)>,
    pub blend: [u8; 4],
    pub opacity: u8,
    pub clipping: u8,
    pub flags: u8,
    pub mask: Option<MaskBody>,
    pub name: String,
    /// Section divider type: 1 open folder, 2 closed folder, 3 bounding
    /// (closing) record. `None` on an ordinary raster.
    pub divider: Option<u32>,
    /// A Photoshop adjustment block (key, even-length body from
    /// `psd_adjustment::encode`), written after `lsct`. `None` writes nothing.
    pub adjustment: Option<([u8; 4], Vec<u8>)>,
    /// Any further additional layer blocks (key, even-length body), written
    /// last: the `lfx2`/`iOpa` pair of a preserved layer style
    /// (`psd_layer_effects`).
    pub blocks: Vec<([u8; 4], Vec<u8>)>,
    /// The record's layer blending ranges, verbatim (empty = none written).
    pub ranges: Vec<u8>,
}

impl Record {
    fn extra(&self) -> Vec<u8> {
        let mut extra = Vec::new();
        match &self.mask {
            Some(mask) => {
                extra.extend_from_slice(&20u32.to_be_bytes());
                for edge in mask.bounds {
                    extra.extend_from_slice(&edge.to_be_bytes());
                }
                extra.extend_from_slice(&[mask.outside, u8::from(mask.disabled) * 2, 0, 0]);
            }
            None => extra.extend_from_slice(&0u32.to_be_bytes()),
        }
        // The blending ranges of a preserved Blend If; empty is the
        // zero-length section every other record writes.
        extra.extend_from_slice(&(self.ranges.len() as u32).to_be_bytes());
        extra.extend_from_slice(&self.ranges);
        let pascal = truncated_name(&self.name);
        extra.push(pascal.len() as u8);
        extra.extend_from_slice(pascal.as_bytes());
        while !extra.len().is_multiple_of(4) {
            extra.push(0);
        }
        let units: Vec<u16> = self.name.encode_utf16().collect();
        let mut luni = (units.len() as u32).to_be_bytes().to_vec();
        for unit in units {
            luni.extend_from_slice(&unit.to_be_bytes());
        }
        extra.extend_from_slice(&block(b"luni", &luni));
        if let Some(divider) = self.divider {
            let mut body = divider.to_be_bytes().to_vec();
            if matches!(divider, 1 | 2) {
                body.extend_from_slice(b"8BIM");
                body.extend_from_slice(&self.blend);
            }
            extra.extend_from_slice(&block(b"lsct", &body));
        }
        if let Some((key, body)) = &self.adjustment {
            extra.extend_from_slice(&block(key, body));
        }
        for (key, body) in &self.blocks {
            extra.extend_from_slice(&block(key, body));
        }
        extra
    }

    fn bytes(&self, large: bool) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        for edge in self.bounds {
            out.extend_from_slice(&edge.to_be_bytes());
        }
        out.extend_from_slice(&(self.channels.len() as u16).to_be_bytes());
        for (id, data) in &self.channels {
            out.extend_from_slice(&id.to_be_bytes());
            if large {
                out.extend_from_slice(&(data.len() as u64).to_be_bytes());
            } else {
                out.extend_from_slice(&u32_len(data.len(), "channel data")?.to_be_bytes());
            }
        }
        out.extend_from_slice(b"8BIM");
        out.extend_from_slice(&self.blend);
        out.extend_from_slice(&[self.opacity, self.clipping, self.flags, 0]);
        let extra = self.extra();
        out.extend_from_slice(&(extra.len() as u32).to_be_bytes());
        out.extend_from_slice(&extra);
        Ok(out)
    }
}

/// The longest prefix of `name` that fits a Pascal length byte, cut on a UTF-8
/// character boundary so the truncated name is still a valid string. The full
/// name always travels in the record's `luni` block.
pub(crate) fn truncated_name(name: &str) -> &str {
    if name.len() <= 255 {
        return name;
    }
    let mut end = 255;
    while end > 0 && !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

fn block(key: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = b"8BIM".to_vec();
    out.extend_from_slice(key);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
    out
}

/// One further image resource a document carries beside the writer's own
/// ([`image_resources`]' `extra`): its id, its Pascal name — empty but for a
/// saved path's (`psd_paths`) — and its body.
pub(crate) struct ResourceBlock {
    pub id: u16,
    pub name: String,
    pub body: Vec<u8>,
}

/// One image resource: signature, id, an empty Pascal name and an even-padded
/// body.
pub(crate) fn resource(id: u16, body: &[u8]) -> Vec<u8> {
    resource_named(id, "", body)
}

/// [`resource`] with a Pascal name: its UTF-8 bytes, cut on a character
/// boundary at 255, the length byte and the name padded to an even size.
pub(crate) fn resource_named(id: u16, name: &str, body: &[u8]) -> Vec<u8> {
    let mut end = name.len().min(255);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = b"8BIM".to_vec();
    out.extend_from_slice(&id.to_be_bytes());
    out.push(end as u8);
    out.extend_from_slice(&name.as_bytes()[..end]);
    if end.is_multiple_of(2) {
        out.push(0);
    }
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
    if !body.len().is_multiple_of(2) {
        out.push(0);
    }
    out
}

/// Adobe's ResolutionInfo (resource 1005): 16.16 pixels/inch per axis, each
/// followed by the display-unit selectors, which are always inches here.
fn resolution_info(resolution: Resolution) -> Vec<u8> {
    let fixed = |value: f32| {
        ((f64::from(value) * 65536.0)
            .round()
            .clamp(0.0, 4_294_967_295.0) as u32)
            .to_be_bytes()
    };
    let mut out = fixed(resolution.x).to_vec();
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&fixed(resolution.y));
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out
}

fn u32_len(len: usize, what: &str) -> Result<u32, String> {
    u32::try_from(len).map_err(|_| format!("PSD {what} exceeds the classic format's 4 GiB field"))
}

/// The layer information all three containers carry: the record count
/// (negative, so the first alpha channel is declared the merged
/// transparency), the records, then every record's channel data, padded to an
/// even length. `records` is FILE order — bottom-first, with each group's
/// closing divider before its contents and its folder record after them.
/// `large` writes a PSB's u64 channel lengths; a TIFF's block is classic.
pub(crate) fn layer_info(records: &[Record], large: bool) -> Result<Vec<u8>, String> {
    layer_info_counted(records, large, true)
}

/// [`layer_info`] with the count's sign chosen: negative when
/// `merged_alpha` (the first alpha channel is the merged transparency),
/// else positive — a Grayscale file whose composite has no alpha plane
/// (#1 phase 3b, `psd_export_gray`), as Photoshop writes its own.
pub(crate) fn layer_info_counted(
    records: &[Record],
    large: bool,
    merged_alpha: bool,
) -> Result<Vec<u8>, String> {
    let mut info = Vec::new();
    layer_info_counted_into(records, large, merged_alpha, &mut info)?;
    Ok(info)
}

fn check_record_count(records: &[Record]) -> Result<(), String> {
    if records.len() > PSD_MAX_RECORDS {
        return Err(format!(
            "PSD layer records ({}) exceed the classic format's {PSD_MAX_RECORDS}-record count field",
            records.len()
        ));
    }
    Ok(())
}

/// [`layer_info_counted`] appended straight to `out` (a 32-bit layered
/// TIFF's one-buffer file, `tiff_layers::write_with`); the number of
/// bytes appended, which is [`layer_info_len`].
pub(crate) fn layer_info_counted_into(
    records: &[Record],
    large: bool,
    merged_alpha: bool,
    out: &mut Vec<u8>,
) -> Result<usize, String> {
    check_record_count(records)?;
    let start = out.len();
    let count = records.len() as i32;
    out.extend_from_slice(&(if merged_alpha { -count } else { count } as i16).to_be_bytes());
    for record in records {
        out.extend_from_slice(&record.bytes(large)?);
    }
    for record in records {
        for (_, data) in &record.channels {
            out.extend_from_slice(data);
        }
    }
    if !(out.len() - start).is_multiple_of(2) {
        out.push(0);
    }
    Ok(out.len() - start)
}

/// The length [`layer_info_counted_into`] appends for `records`, without
/// copying a plane: the count, every record's header and every channel's
/// data, padded to an even length.
pub(crate) fn layer_info_len(records: &[Record], large: bool) -> Result<usize, String> {
    check_record_count(records)?;
    let mut len = 2usize;
    for record in records {
        len += record.bytes(large)?.len();
        len += record
            .channels
            .iter()
            .map(|(_, data)| data.len())
            .sum::<usize>();
    }
    Ok(len + len % 2)
}

/// The image-resource section: the ICC profile (1039) whenever `icc` is not
/// empty (a CMYK save without its profile passes none) and print resolution
/// (1005) always; `global_light` writes 1037/1049 and is `Some` exactly when
/// a record carries an `lfx2` block; `color_samplers` writes 1073
/// (`psd_samplers`) when it is non-empty; `extra` is every further
/// `(id, body)` the caller's document carries (layer comps' 1065,
/// `psd_comps`), written last and in order, so an empty slice keeps the prior
/// writer's bytes.
pub(crate) fn image_resources(
    icc: &[u8],
    resolution: Resolution,
    global_light: Option<GlobalLight>,
    color_samplers: &[ColorSampler],
    extra: &[ResourceBlock],
) -> Vec<u8> {
    let mut resources = if icc.is_empty() {
        Vec::new()
    } else {
        resource(1039, icc)
    };
    resources.extend_from_slice(&resource(1005, &resolution_info(resolution)));
    // Photoshop stores the shared light direction beside the layer effects
    // that read it, so these two resources travel only with an `lfx2`.
    if let Some(light) = global_light {
        let angle = light.angle.rem_euclid(360.0).round() as i32;
        resources.extend_from_slice(&resource(1037, &angle.to_be_bytes()));
        resources.extend_from_slice(&resource(
            1049,
            &(light.altitude.round() as i32).to_be_bytes(),
        ));
    }
    // Colour samplers, only when the document holds any, so a
    // sampler-free export keeps the prior writer's bytes.
    if !color_samplers.is_empty() {
        resources.extend_from_slice(&resource(
            1073,
            &crate::psd_samplers::encode(color_samplers),
        ));
    }
    for block in extra {
        resources.extend_from_slice(&resource_named(block.id, &block.name, &block.body));
    }
    resources
}

/// A section length: a u32, or a u64 in a PSB.
fn push_section_len(out: &mut Vec<u8>, len: usize, what: &str, large: bool) -> Result<(), String> {
    if large {
        out.extend_from_slice(&(len as u64).to_be_bytes());
    } else {
        out.extend_from_slice(&u32_len(len, what)?.to_be_bytes());
    }
    Ok(())
}

/// Serializes a complete PSD (`large == false`, version 1) or PSB (`large`,
/// version 2) at `depth` (8 or 16) in `color_mode` (3 RGB, 4 CMYK — #1's
/// flattened CMYK save, `cmyk_io` —, 1 Grayscale, #1 phase 3b, or 9 Lab,
/// #1 Lab phase 3b). `records`, `global_light` and `color_samplers` are
/// [`layer_info`]'s and [`image_resources`]'s; a Grayscale file with ONE
/// composite plane and a Lab file with THREE write a positive record count
/// (`layer_info_counted`), every other file a negative one.
/// `composite` is the canvas-sized preview as row-major byte planes, one per
/// channel the header declares: for RGB four ALREADY-MATTED planes in R, G,
/// B, A order (`composite_planes8`/`composite_planes16`), stored white-matted
/// the way every classic writer stores it; for CMYK the four ink planes as
/// stored (`max − ink`); for Grayscale the one white-matted gray plane
/// (`psd_export_gray::composite_plane_gray`); for Lab the L, a and b planes
/// matted over Lab white (`psd_export_lab::composite_planes_lab`). A 32-bit
/// file is [`write_psd32`]'s. `spots` (`psd_spot`, #1 1b-1) are declared
/// after the composite: their planes' rows follow its rows in the RLE table
/// and body, and their resources (`psd_spot_records::encode_resources`)
/// follow the others, leading with the merged transparency's entry when the
/// count is negative. `extra_resources` are [`image_resources`]' `extra`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn write_psd(
    width: u32,
    height: u32,
    depth: u16,
    color_mode: u16,
    icc: &[u8],
    resolution: Resolution,
    global_light: Option<GlobalLight>,
    color_samplers: &[ColorSampler],
    records: &[Record],
    composite: &[Vec<u8>],
    large: bool,
    tail: &[crate::psd_layer_effects::Block],
    spots: &crate::psd_spot::SpotPlanes,
    extra_resources: &[ResourceBlock],
) -> Result<Vec<u8>, String> {
    let kind = if large {
        LayeredContainer::Psb
    } else {
        LayeredContainer::Psd
    };
    let side = kind.max_side();
    if width == 0 || height == 0 || width > side || height > side {
        return Err(format!(
            "PSD canvas is empty or exceeds the {side}-pixel dimension limit"
        ));
    }
    if !matches!(depth, 8 | 16) {
        return Err("PSD export writes only 8- or 16-bit channels".into());
    }
    let row_bytes = width as usize * usize::from(depth) / 8;
    if composite
        .iter()
        .any(|plane| plane.len() != row_bytes * height as usize)
    {
        return Err("PSD composite preview does not match the canvas".into());
    }
    // Photoshop's own gray (one plane) and Lab (three planes) files carry no
    // merged-alpha plane, so their count is positive.
    let merged_alpha = !matches!((color_mode, composite.len()), (1, 1) | (9, 3));
    let info = layer_info_counted(records, large, merged_alpha)?;
    let capacity = crate::psd_framing::MAX_PSD_CHANNELS.saturating_sub(composite.len());
    if spots.inks.len() > capacity || spots.rows.len() != spots.inks.len() * height as usize {
        return Err("PSD spot channels do not fit the file".into());
    }
    let mut out = b"8BPS".to_vec();
    out.extend_from_slice(&(if large { 2u16 } else { 1 }).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    out.extend_from_slice(&((composite.len() + spots.inks.len()) as u16).to_be_bytes());
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&depth.to_be_bytes());
    out.extend_from_slice(&color_mode.to_be_bytes());
    // No colour-mode data: neither RGB, CMYK, Grayscale nor Lab mode has any.
    out.extend_from_slice(&0u32.to_be_bytes());

    let mut resources = image_resources(
        icc,
        resolution,
        global_light,
        color_samplers,
        extra_resources,
    );
    let inks: Vec<(&str, [f64; 3], u8)> = spots
        .inks
        .iter()
        .map(|(name, lab, solidity)| (name.as_str(), *lab, *solidity))
        .collect();
    // The count is negative only when some record is written.
    let transparency = merged_alpha && !records.is_empty();
    resources.extend_from_slice(&crate::psd_spot_records::encode_resources(
        &inks,
        transparency,
    ));
    out.extend_from_slice(&u32_len(resources.len(), "image resources")?.to_be_bytes());
    out.extend_from_slice(&resources);

    let mut section = Vec::with_capacity(info.len() + 12);
    push_section_len(&mut section, info.len(), "layer information", large)?;
    section.extend_from_slice(&info);
    // No global layer mask.
    section.extend_from_slice(&0u32.to_be_bytes());
    // The document-level tail blocks (the `Patt` pattern table).
    section.extend_from_slice(&crate::psd_patterns::encode_tail(tail));
    push_section_len(&mut out, section.len(), "layer and mask information", large)?;
    out.extend_from_slice(&section);

    out.extend_from_slice(&1u16.to_be_bytes());
    let mut table = Vec::with_capacity(height as usize * if large { 16 } else { 8 });
    let mut body = Vec::new();
    for plane in composite {
        for row in plane.chunks_exact(row_bytes) {
            let encoded = packbits_row(row);
            push_row_count(&mut table, encoded.len(), large);
            body.extend_from_slice(&encoded);
        }
    }
    for encoded in &spots.rows {
        push_row_count(&mut table, encoded.len(), large);
        body.extend_from_slice(encoded);
    }
    out.extend_from_slice(&table);
    out.extend_from_slice(&body);

    if out.len() as u64 > kind.max_file_bytes() {
        return Err("PSD export exceeds the classic format's 2 GiB file limit".into());
    }
    Ok(out)
}

/// An open or closed folder record's `lsct` in Photoshop's own 16-byte form
/// (#1 phase 3b, M4; Lab phase 3b, L3): the divider type, `8BIM`, the
/// blend key and the sub-type (0, normal), moved to the head of `blocks` —
/// the byte position the record's own `divider` gave it — where the writer's
/// default form is 12 bytes. A closing divider and every other record are
/// untouched.
pub(crate) fn photoshop_folder_lsct(record: &mut Record) {
    if let Some(divider @ (1 | 2)) = record.divider {
        let mut body = divider.to_be_bytes().to_vec();
        body.extend_from_slice(b"8BIM");
        body.extend_from_slice(&record.blend);
        body.extend_from_slice(&0u32.to_be_bytes());
        record.divider = None;
        record.blocks.insert(0, (*b"lsct", body));
    }
}

/// The color-mode data section of a 32-bit RGB file, byte for byte what
/// Photoshop 26.10.0 wrote into take1's `ps32.psd`: an `hdrt` block
/// (HDR Toning settings, version 3, the "Linear" curve) and an `hdra` block
/// (version 6). Photoshop writes these 110 bytes at 32 bits only (its 8- and
/// 16-bit RGB files have an empty section); they describe none of the
/// document's pixels, so Doctored writes Photoshop's own values unchanged
/// and its reader skips them, as it skips any RGB color-mode data.
const PHOTOSHOP_HDR_COLOR_MODE_DATA: [u8; 110] = [
    0x68, 0x64, 0x72, 0x74, 0x00, 0x00, 0x00, 0x03, 0x3E, 0x6B, 0x85, 0x1F, 0x00, 0x00, 0x00, 0x02,
    0x00, 0x00, 0x00, 0x07, 0x00, 0x4C, 0x00, 0x69, 0x00, 0x6E, 0x00, 0x65, 0x00, 0x61, 0x00, 0x72,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0x00, 0xFF, 0x01, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x41, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x3F, 0x80, 0x00, 0x00, 0x68, 0x64, 0x72, 0x61, 0x00, 0x00, 0x00, 0x06,
    0x00, 0x00, 0x00, 0x00, 0x41, 0xA0, 0x00, 0x00, 0x41, 0xF0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x3F, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// A complete 32-bit RGB PSD (`large == false`) or PSB, written in
/// Photoshop's 32-bit form as Photoshop 26.10.0 writes its own:
/// header depth 32, four channels, [`PHOTOSHOP_HDR_COLOR_MODE_DATA`] as the
/// color-mode data, [`image_resources`] (`icc` is the caller's: the
/// document profile's linear twin, `icc_builtin::linear_twin`), an EMPTY
/// layer-information sub-section, no global layer mask, an empty `Mt32` tail
/// block (the merged-transparency declaration), then the records in an
/// `Lr32` tail block — [`layer_info`]'s body with a POSITIVE count, since
/// `Mt32` declares the merged alpha — zero-padded to a multiple of four (both
/// blocks' lengths u64 in a PSB, `psd_framing::LONG_LENGTH_KEYS`), then
/// `tail`; the composite is Raw, its four planes (R, G, B, A, each
/// `width · height` big-endian f32) appended by `composite`.
///
/// The file is laid into ONE buffer of its exact final size, the records'
/// channel data copied straight into it: the record list and the file are
/// the only two copies of the planes this ever holds, which is what the
/// float save's admission charges (`psd_export_float`'s module doc).
///
/// `gray` (#1 phase 3c) writes Photoshop's 32-bit Grayscale form instead,
/// as its `layers32.psd` (take2): header channels 1 and colour mode 1, the
/// same color-mode data, NO `Mt32` block (Photoshop's gray files carry no
/// merged alpha, phase 3b's M1), the `Lr32` count positive as before, and a
/// composite of ONE Raw plane (`width · height` big-endian f32).
#[allow(clippy::too_many_arguments)]
pub(crate) fn write_psd32(
    width: u32,
    height: u32,
    icc: &[u8],
    resolution: Resolution,
    global_light: Option<GlobalLight>,
    color_samplers: &[ColorSampler],
    records: &[Record],
    large: bool,
    tail: &[crate::psd_layer_effects::Block],
    gray: bool,
    composite: &dyn Fn(&mut Vec<u8>) -> Result<(), String>,
    extra_resources: &[ResourceBlock],
) -> Result<Vec<u8>, String> {
    let kind = if large {
        LayeredContainer::Psb
    } else {
        LayeredContainer::Psd
    };
    let side = kind.max_side();
    if width == 0 || height == 0 || width > side || height > side {
        return Err(format!(
            "PSD canvas is empty or exceeds the {side}-pixel dimension limit"
        ));
    }
    if records.len() > PSD_MAX_RECORDS {
        return Err(format!(
            "PSD layer records ({}) exceed the classic format's {PSD_MAX_RECORDS}-record count field",
            records.len()
        ));
    }
    let headers = records
        .iter()
        .map(|record| record.bytes(large))
        .collect::<Result<Vec<_>, _>>()?;
    let data: usize = records
        .iter()
        .flat_map(|record| record.channels.iter())
        .map(|(_, plane)| plane.len())
        .sum();
    let unpadded = 2 + headers.iter().map(Vec::len).sum::<usize>() + data;
    let info_len = unpadded + unpadded % 2;
    let block_pad = info_len.wrapping_neg() & 3;
    let tail = crate::psd_patterns::encode_tail(tail);
    let field = if large { 8 } else { 4 };
    // The `Mt32` block's 8-byte signature and key and its zero length.
    let merged_block = if gray { 0 } else { 8 + field };
    let section_len = field + 4 + merged_block + 8 + field + info_len + block_pad + tail.len();
    let resources = image_resources(
        icc,
        resolution,
        global_light,
        color_samplers,
        extra_resources,
    );
    let planes = if gray { 1 } else { 4 };
    let composite_len = 2 + 4 * planes * width as usize * height as usize;
    let total = 26
        + 4
        + PHOTOSHOP_HDR_COLOR_MODE_DATA.len()
        + 4
        + resources.len()
        + field
        + section_len
        + composite_len;
    if total as u64 > kind.max_file_bytes() {
        return Err("PSD export exceeds the classic format's 2 GiB file limit".into());
    }
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"8BPS");
    out.extend_from_slice(&(if large { 2u16 } else { 1 }).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    out.extend_from_slice(&(planes as u16).to_be_bytes());
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&32u16.to_be_bytes());
    out.extend_from_slice(&(if gray { 1u16 } else { 3 }).to_be_bytes());
    out.extend_from_slice(&(PHOTOSHOP_HDR_COLOR_MODE_DATA.len() as u32).to_be_bytes());
    out.extend_from_slice(&PHOTOSHOP_HDR_COLOR_MODE_DATA);
    out.extend_from_slice(&u32_len(resources.len(), "image resources")?.to_be_bytes());
    out.extend_from_slice(&resources);
    push_section_len(&mut out, section_len, "layer and mask information", large)?;
    push_section_len(&mut out, 0, "layer information", large)?;
    out.extend_from_slice(&0u32.to_be_bytes());
    if !gray {
        out.extend_from_slice(b"8BIMMt32");
        push_section_len(&mut out, 0, "Mt32 block", large)?;
    }
    out.extend_from_slice(b"8BIMLr32");
    push_section_len(&mut out, info_len, "Lr32 block", large)?;
    out.extend_from_slice(&(records.len() as i16).to_be_bytes());
    for header in &headers {
        out.extend_from_slice(header);
    }
    for record in records {
        for (_, plane) in &record.channels {
            out.extend_from_slice(plane);
        }
    }
    out.resize(out.len() + unpadded % 2 + block_pad, 0);
    out.extend_from_slice(&tail);
    out.extend_from_slice(&0u16.to_be_bytes());
    composite(&mut out)?;
    if out.len() != total {
        return Err("internal error: the 32-bit PSD composite does not match the canvas".into());
    }
    Ok(out)
}

/// One colour sample composited over white, the matte every classic writer
/// applies to the stored composite. Exactly invertible at alpha 0 and 255;
/// `psd_container::composite` removes it on the way back in.
pub(crate) fn matte(color: u8, alpha: u8) -> u8 {
    match alpha {
        255 => color,
        0 => 255,
        _ => {
            let (c, a) = (u32::from(color), u32::from(alpha));
            ((c * a + 255 * (255 - a) + 127) / 255) as u8
        }
    }
}

/// The same white matte in 16-bit codes, the exact twin of [`matte`];
/// `psd_container::composite16` removes it on the way back in.
pub(crate) fn matte16(color: u16, alpha: u16) -> u16 {
    match alpha {
        65535 => color,
        0 => 65535,
        _ => {
            let (c, a) = (u64::from(color), u64::from(alpha));
            ((c * a + 65535 * (65535 - a) + 32767) / 65535) as u16
        }
    }
}

/// The four white-matted composite planes of an 8-bit canvas, row-major bytes
/// in R, G, B, A order. Alpha is stored straight.
pub(crate) fn composite_planes8(composite: &RgbaImage) -> [Vec<u8>; 4] {
    let samples = composite.as_raw();
    std::array::from_fn(|component| {
        samples
            .chunks_exact(4)
            .map(|pixel| {
                if component == 3 {
                    pixel[3]
                } else {
                    matte(pixel[component], pixel[3])
                }
            })
            .collect()
    })
}

/// The same four planes for a 16-bit canvas, each sample big-endian.
pub(crate) fn composite_planes16(
    composite: &PrecisionImage,
    width: u32,
    height: u32,
) -> Result<[Vec<u8>; 4], String> {
    if composite.dimensions() != (width, height) {
        return Err("PSD composite preview does not match the canvas".into());
    }
    let runs = composite
        .runs_rgba16()
        .ok_or("16-bit PSD export requires an RGBA16 UNORM projection")?;
    let count = width as usize * height as usize;
    let mut planes: [Vec<u8>; 4] = std::array::from_fn(|_| Vec::with_capacity(count * 2));
    for run in runs {
        for pixel in run {
            let [r, g, b, a] = &mut planes;
            r.extend_from_slice(&matte16(pixel[0], pixel[3]).to_be_bytes());
            g.extend_from_slice(&matte16(pixel[1], pixel[3]).to_be_bytes());
            b.extend_from_slice(&matte16(pixel[2], pixel[3]).to_be_bytes());
            a.extend_from_slice(&pixel[3].to_be_bytes());
        }
    }
    Ok(planes)
}
