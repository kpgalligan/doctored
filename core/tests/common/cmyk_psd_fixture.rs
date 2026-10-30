//! An independent CMYK PSD / PSB / layered-TIFF byte builder and reader
//! (#1 phase 3), written from Adobe's published "Photoshop File Formats"
//! framing and the TIFF 6.0 specification. It has its own PackBits packer and
//! unpacker, its own record, resource and tagged-block layout, its own ZIP
//! prediction, and it never calls the importer, exporter or codec it is used
//! to check (the zlib stream itself is `miniz_oxide`'s, a library, not the
//! core's `psd_zip` framing). A CMYK ink plane is stored `max − ink`; the
//! transparency (`-1`) and masks (`-2`) are stored as they are.
#![allow(dead_code)]

use doctored_core::cmyk_image::CmykSample;

// ------------------------------------------------------------- PackBits --

/// A packer that emits a repeat run for any run of 2 or more equal bytes
/// (the core requires 3), so its output is a legal but distinct encoding.
pub fn pack(row: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < row.len() {
        let mut run = 1;
        while i + run < row.len() && run < 128 && row[i + run] == row[i] {
            run += 1;
        }
        if run >= 2 {
            out.push((1i16 - run as i16) as u8);
            out.push(row[i]);
            i += run;
            continue;
        }
        let start = i;
        while i < row.len() && i - start < 128 && (i + 1 >= row.len() || row[i + 1] != row[i]) {
            i += 1;
        }
        if i == start {
            i += 1;
        }
        out.push((i - start - 1) as u8);
        out.extend_from_slice(&row[start..i]);
    }
    out
}

pub fn unpack(mut bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some((&n, rest)) = bytes.split_first() {
        let n = n as i8;
        if n >= 0 {
            let len = n as usize + 1;
            out.extend_from_slice(&rest[..len]);
            bytes = &rest[len..];
        } else if n != -128 {
            out.extend(std::iter::repeat_n(rest[0], (1 - n as isize) as usize));
            bytes = &rest[1..];
        } else {
            bytes = rest;
        }
    }
    out
}

/// How a built plane is compressed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Comp {
    Raw,
    Rle,
    Zip,
    ZipPredicted,
}

/// Samples at the file's depth as stored bytes, row-major.
pub fn sample_bytes(values: &[u16], depth: u16) -> Vec<u8> {
    if depth == 8 {
        values.iter().map(|v| *v as u8).collect()
    } else {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }
}

/// One plane (`values` at the file's depth, `width × height`) with its
/// compression word. `large` writes a PSB's u32 RLE row counts.
pub fn plane(
    values: &[u16],
    width: usize,
    height: usize,
    depth: u16,
    comp: Comp,
    large: bool,
) -> Vec<u8> {
    let bytes = sample_bytes(values, depth);
    let row_bytes = width * usize::from(depth / 8);
    assert_eq!(bytes.len(), row_bytes * height, "plane size");
    match comp {
        Comp::Raw => [vec![0, 0], bytes].concat(),
        Comp::Rle => {
            let mut table = Vec::new();
            let mut body = Vec::new();
            for row in bytes.chunks(row_bytes.max(1)).take(height) {
                let packed = pack(row);
                if large {
                    table.extend((packed.len() as u32).to_be_bytes());
                } else {
                    table.extend((packed.len() as u16).to_be_bytes());
                }
                body.extend(packed);
            }
            [vec![0, 1], table, body].concat()
        }
        Comp::Zip | Comp::ZipPredicted => {
            let mut data = bytes.clone();
            if comp == Comp::ZipPredicted {
                for row in data.chunks_mut(row_bytes.max(1)) {
                    if depth == 8 {
                        for i in (1..row.len()).rev() {
                            row[i] = row[i].wrapping_sub(row[i - 1]);
                        }
                    } else {
                        let samples: Vec<u16> = row
                            .chunks_exact(2)
                            .map(|s| u16::from_be_bytes([s[0], s[1]]))
                            .collect();
                        for i in (1..samples.len()).rev() {
                            let delta = samples[i].wrapping_sub(samples[i - 1]);
                            row[i * 2..i * 2 + 2].copy_from_slice(&delta.to_be_bytes());
                        }
                    }
                }
            }
            let word: u16 = if comp == Comp::Zip { 2 } else { 3 };
            [
                word.to_be_bytes().to_vec(),
                miniz_oxide::deflate::compress_to_vec_zlib(&data, 6),
            ]
            .concat()
        }
    }
}

/// A raster's channels from straight CMYKA16 `samples`: `-1` (when `alpha`)
/// then `0`–`3` stored `max − ink`, at `depth` (8-bit = `round(v / 257)`).
pub fn ink_channels(
    samples: &[CmykSample],
    width: usize,
    depth: u16,
    alpha: bool,
    comp: Comp,
    large: bool,
) -> Vec<(i16, Vec<u8>)> {
    let height = samples.len() / width.max(1);
    let at_depth = |v: u16| -> u16 {
        if depth == 8 {
            ((u32::from(v) + 128) / 257) as u16
        } else {
            v
        }
    };
    let max: u16 = if depth == 8 { 255 } else { 65535 };
    let mut out = Vec::new();
    if alpha {
        let values: Vec<u16> = samples.iter().map(|s| at_depth(s[4])).collect();
        out.push((-1, plane(&values, width, height, depth, comp, large)));
    }
    for c in 0..4 {
        let values: Vec<u16> = samples.iter().map(|s| max - at_depth(s[c])).collect();
        out.push((c as i16, plane(&values, width, height, depth, comp, large)));
    }
    out
}

// ------------------------------------------------------------- records --

pub fn block(key: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = [
        b"8BIM".as_slice(),
        key,
        &(body.len() as u32).to_be_bytes(),
        body,
    ]
    .concat();
    if out.len() % 2 == 1 {
        out.push(0);
    }
    out
}

pub fn mask_body(bounds: [i32; 4], outside: u8, flags: u8) -> Vec<u8> {
    let mut body = Vec::new();
    for value in bounds {
        body.extend(value.to_be_bytes());
    }
    body.extend([outside, flags, 0, 0]);
    body
}

#[derive(Clone)]
pub struct Rec {
    pub bounds: [i32; 4],
    pub channels: Vec<(i16, Vec<u8>)>,
    pub name: String,
    /// 0 none, 1 open folder, 2 closed folder, 3 bounding record.
    pub divider: u32,
    pub blend: [u8; 4],
    pub opacity: u8,
    pub clipping: u8,
    pub flags: u8,
    pub mask_body: Vec<u8>,
    pub ranges: Vec<u8>,
    pub blocks: Vec<u8>,
}

impl Rec {
    pub fn raster(name: &str, bounds: [i32; 4], channels: Vec<(i16, Vec<u8>)>) -> Self {
        Self {
            bounds,
            channels,
            name: name.to_string(),
            divider: 0,
            blend: *b"norm",
            opacity: 255,
            clipping: 0,
            flags: 8,
            mask_body: Vec::new(),
            ranges: Vec::new(),
            blocks: Vec::new(),
        }
    }

    /// A pixel-less record with the five placeholder channels.
    pub fn empty(name: &str) -> Self {
        Self::raster(
            name,
            [0; 4],
            [-1i16, 0, 1, 2, 3]
                .into_iter()
                .map(|id| (id, vec![0, 0]))
                .collect(),
        )
    }

    pub fn folder(name: &str, divider: u32, blend: [u8; 4]) -> Self {
        Self {
            divider,
            blend,
            flags: 0x18,
            ..Self::empty(name)
        }
    }

    pub fn mask(mut self, bounds: [i32; 4], outside: u8, enabled: bool, plane: Vec<u8>) -> Self {
        self.mask_body = mask_body(bounds, outside, if enabled { 0 } else { 2 });
        self.channels.push((-2, plane));
        self
    }

    pub fn with_block(mut self, key: &[u8; 4], body: &[u8]) -> Self {
        self.blocks.extend(block(key, body));
        self
    }

    fn encode(&self, large: bool) -> Vec<u8> {
        let mut out = Vec::new();
        for value in self.bounds {
            out.extend(value.to_be_bytes());
        }
        out.extend((self.channels.len() as u16).to_be_bytes());
        for (id, data) in &self.channels {
            out.extend(id.to_be_bytes());
            if large {
                out.extend((data.len() as u64).to_be_bytes());
            } else {
                out.extend((data.len() as u32).to_be_bytes());
            }
        }
        out.extend(b"8BIM");
        out.extend(self.blend);
        out.extend([self.opacity, self.clipping, self.flags, 0]);
        let mut extra = (self.mask_body.len() as u32).to_be_bytes().to_vec();
        extra.extend(&self.mask_body);
        extra.extend((self.ranges.len() as u32).to_be_bytes());
        extra.extend(&self.ranges);
        let pascal = self.name.as_bytes();
        let pascal = &pascal[..pascal.len().min(255)];
        extra.push(pascal.len() as u8);
        extra.extend(pascal);
        while !extra.len().is_multiple_of(4) {
            extra.push(0);
        }
        let units: Vec<u16> = self.name.encode_utf16().collect();
        let mut luni = (units.len() as u32).to_be_bytes().to_vec();
        for unit in units {
            luni.extend(unit.to_be_bytes());
        }
        extra.extend(block(b"luni", &luni));
        if self.divider != 0 {
            let mut body = self.divider.to_be_bytes().to_vec();
            if matches!(self.divider, 1 | 2) {
                body.extend(b"8BIM");
                body.extend(self.blend);
            }
            extra.extend(block(b"lsct", &body));
        }
        extra.extend(&self.blocks);
        out.extend((extra.len() as u32).to_be_bytes());
        out.extend(extra);
        out
    }
}

/// The layer information (count, records, channel data, even padding).
pub fn layer_info(records: &[Rec], merged_alpha: bool, large: bool) -> Vec<u8> {
    let count = records.len() as i16;
    let mut info = if merged_alpha { -count } else { count }
        .to_be_bytes()
        .to_vec();
    for record in records {
        info.extend(record.encode(large));
    }
    for record in records {
        for (_, data) in &record.channels {
            info.extend(data);
        }
    }
    if info.len() % 2 == 1 {
        info.push(0);
    }
    info
}

fn resource(id: u16, body: &[u8]) -> Vec<u8> {
    let mut out = b"8BIM".to_vec();
    out.extend(id.to_be_bytes());
    out.extend([0, 0]);
    out.extend((body.len() as u32).to_be_bytes());
    out.extend(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

pub fn resources(icc: Option<&[u8]>, extra: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(icc) = icc {
        out.extend(resource(1039, icc));
    }
    for (id, body) in extra {
        out.extend(resource(*id, body));
    }
    out
}

/// The file's options beyond its records.
pub struct PsdSpec<'a> {
    pub canvas: (u32, u32),
    pub depth: u16,
    pub icc: Option<&'a [u8]>,
    /// The composite's planes at the file's depth, STORED values (inks
    /// already `max − ink`), one per header channel.
    pub composite: Vec<Vec<u16>>,
    pub merged_alpha: bool,
    pub large: bool,
    /// Photoshop's 16-bit layout: an empty layer-information section and the
    /// records in an `Lr16` tail block.
    pub lr16: bool,
    pub resources: Vec<(u16, Vec<u8>)>,
}

/// A colour-mode-4 PSD (or PSB).
pub fn psd(spec: &PsdSpec<'_>, records: &[Rec]) -> Vec<u8> {
    let (width, height) = spec.canvas;
    let large = spec.large;
    let info = if records.is_empty() {
        Vec::new()
    } else {
        layer_info(records, spec.merged_alpha, large)
    };
    let len = |n: usize, out: &mut Vec<u8>| {
        if large {
            out.extend((n as u64).to_be_bytes());
        } else {
            out.extend((n as u32).to_be_bytes());
        }
    };
    let mut section = Vec::new();
    if spec.lr16 {
        len(0, &mut section);
        section.extend(0u32.to_be_bytes());
        let mut tail = b"8BIMLr16".to_vec();
        len(info.len(), &mut tail);
        tail.extend(&info);
        while !tail.len().is_multiple_of(4) {
            tail.push(0);
        }
        section.extend(tail);
    } else {
        len(info.len(), &mut section);
        section.extend(&info);
        section.extend(0u32.to_be_bytes());
    }
    let mut bytes = b"8BPS".to_vec();
    bytes.extend((if large { 2u16 } else { 1 }).to_be_bytes());
    bytes.extend([0; 6]);
    bytes.extend((spec.composite.len() as u16).to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend(width.to_be_bytes());
    bytes.extend(spec.depth.to_be_bytes());
    bytes.extend(4u16.to_be_bytes());
    bytes.extend(0u32.to_be_bytes());
    let res = resources(spec.icc, &spec.resources);
    bytes.extend((res.len() as u32).to_be_bytes());
    bytes.extend(res);
    len(section.len(), &mut bytes);
    bytes.extend(section);
    // An RLE composite of every header channel.
    bytes.extend(1u16.to_be_bytes());
    let row_bytes = width as usize * usize::from(spec.depth / 8);
    let mut table = Vec::new();
    let mut body = Vec::new();
    for values in &spec.composite {
        for row in sample_bytes(values, spec.depth).chunks(row_bytes) {
            let packed = pack(row);
            if large {
                table.extend((packed.len() as u32).to_be_bytes());
            } else {
                table.extend((packed.len() as u16).to_be_bytes());
            }
            body.extend(packed);
        }
    }
    bytes.extend(table);
    bytes.extend(body);
    bytes
}

/// A big-endian layered CMYK TIFF (Photometric 5, InkSet 1): one
/// uncompressed chunky strip of `strip` (straight ink and alpha at `depth`,
/// five samples), the ICC tag and tag 37724 holding the `Layr`/`Lr16` block.
pub fn layered_tiff(
    canvas: (u32, u32),
    depth: u16,
    icc: Option<&[u8]>,
    strip: &[[u16; 5]],
    records: &[Rec],
) -> Vec<u8> {
    let (w, h) = canvas;
    let mut data = Vec::new();
    for sample in strip {
        for v in sample {
            if depth == 8 {
                data.push(*v as u8);
            } else {
                data.extend(v.to_be_bytes());
            }
        }
    }
    let info = layer_info(records, true, false);
    let mut blockdata = b"Adobe Photoshop Document Data Block\0".to_vec();
    blockdata.extend(b"8BIM");
    blockdata.extend(if depth == 8 { b"Layr" } else { b"Lr16" });
    blockdata.extend((info.len() as u32).to_be_bytes());
    blockdata.extend(&info);
    while !blockdata.len().is_multiple_of(4) {
        blockdata.push(0);
    }
    let mut out = b"MM\0*".to_vec();
    out.extend([0; 4]);
    let strip_at = out.len() as u32;
    out.extend(&data);
    let place = |out: &mut Vec<u8>, bytes: &[u8]| -> u32 {
        if out.len() % 2 == 1 {
            out.push(0);
        }
        let at = out.len() as u32;
        out.extend(bytes);
        at
    };
    let bits: Vec<u8> = (0..5).flat_map(|_| depth.to_be_bytes()).collect();
    let bits_at = place(&mut out, &bits);
    let icc_at = icc.map(|icc| place(&mut out, icc));
    let block_at = place(&mut out, &blockdata);
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let directory = out.len() as u32;
    let short = |v: u16| [(v >> 8) as u8, v as u8, 0, 0];
    let mut entries: Vec<(u16, u16, u32, [u8; 4])> = vec![
        (256, 4, 1, w.to_be_bytes()),
        (257, 4, 1, h.to_be_bytes()),
        (258, 3, 5, bits_at.to_be_bytes()),
        (259, 3, 1, short(1)),
        (262, 3, 1, short(5)),
        (273, 4, 1, strip_at.to_be_bytes()),
        (277, 3, 1, short(5)),
        (278, 4, 1, h.to_be_bytes()),
        (279, 4, 1, (data.len() as u32).to_be_bytes()),
        (284, 3, 1, short(1)),
        (332, 3, 1, short(1)),
        (338, 3, 1, short(2)),
    ];
    if let (Some(icc), Some(at)) = (icc, icc_at) {
        entries.push((34675, 7, icc.len() as u32, at.to_be_bytes()));
    }
    entries.push((37724, 7, blockdata.len() as u32, block_at.to_be_bytes()));
    out.extend((entries.len() as u16).to_be_bytes());
    for (tag, kind, count, value) in entries {
        out.extend(tag.to_be_bytes());
        out.extend(kind.to_be_bytes());
        out.extend(count.to_be_bytes());
        out.extend(value);
    }
    out.extend(0u32.to_be_bytes());
    out[4..8].copy_from_slice(&directory.to_be_bytes());
    out
}

// -------------------------------------------------------------- reading --

#[derive(Debug, Clone)]
pub struct ReadMask {
    pub bounds: [i32; 4],
    pub outside: u8,
    pub flags: u8,
}

#[derive(Debug, Clone)]
pub struct ReadRecord {
    pub bounds: [i32; 4],
    pub name: String,
    pub blend: [u8; 4],
    pub opacity: u8,
    pub clipping: u8,
    pub flags: u8,
    pub divider: Option<u32>,
    pub divider_blend: Option<[u8; 4]>,
    pub mask: Option<ReadMask>,
    pub ranges: Vec<u8>,
    /// Every channel's samples at the file's depth, as stored.
    pub channels: Vec<(i16, Vec<u16>)>,
    pub blocks: Vec<([u8; 4], Vec<u8>)>,
}

impl ReadRecord {
    pub fn plane(&self, id: i16) -> Option<&[u16]> {
        self.channels
            .iter()
            .find(|(channel, _)| *channel == id)
            .map(|(_, v)| v.as_slice())
    }

    pub fn block(&self, key: &[u8; 4]) -> Option<&[u8]> {
        self.blocks
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, b)| b.as_slice())
    }

    pub fn width(&self) -> usize {
        (self.bounds[3] - self.bounds[1]).max(0) as usize
    }

    /// The record's straight CMYKA16 ink: `max − stored` per ink plane, an
    /// 8-bit code ×257, alpha 65535 without a `-1` channel.
    pub fn ink(&self, depth: u16) -> Vec<CmykSample> {
        let scale = if depth == 8 { 257 } else { 1 };
        let max: u16 = if depth == 8 { 255 } else { 65535 };
        let count = self.plane(0).map_or(0, <[u16]>::len);
        (0..count)
            .map(|i| {
                let mut out = [0u16; 5];
                for (c, slot) in out.iter_mut().take(4).enumerate() {
                    *slot = (max - self.plane(c as i16).unwrap()[i]) * scale;
                }
                out[4] = self.plane(-1).map_or(65535, |a| a[i] * scale);
                out
            })
            .collect()
    }

    /// The `-2` plane as u16 coverage (8-bit ×257).
    pub fn mask16(&self, depth: u16) -> Option<Vec<u16>> {
        let scale = if depth == 8 { 257 } else { 1 };
        self.plane(-2)
            .map(|p| p.iter().map(|v| v * scale).collect())
    }
}

#[derive(Debug)]
pub struct ReadFile {
    pub tiff: bool,
    pub large: bool,
    pub width: u32,
    pub height: u32,
    pub depth: u16,
    pub color: u16,
    pub channels: u16,
    pub icc: Option<Vec<u8>>,
    pub resource_ids: Vec<u16>,
    pub records: Vec<ReadRecord>,
    pub merged_alpha: bool,
    /// The flat picture's planes at the file's depth, as stored (a PSD's
    /// composite channels; a TIFF's strip split into its samples).
    pub composite: Vec<Vec<u16>>,
}

impl ReadFile {
    pub fn by_name(&self, name: &str) -> &ReadRecord {
        self.records
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("no record {name}"))
    }
}

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}
fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(b[at..at + 4].try_into().unwrap())
}
fn be64(b: &[u8], at: usize) -> u64 {
    u64::from_be_bytes(b[at..at + 8].try_into().unwrap())
}

/// One stored plane of `width × height` samples at `depth`.
pub fn decode_plane(
    bytes: &[u8],
    width: usize,
    height: usize,
    depth: u16,
    large: bool,
) -> Vec<u16> {
    let sb = usize::from(depth / 8);
    let row_bytes = width * sb;
    let raw: Vec<u8> = match be16(bytes, 0) {
        0 => bytes[2..].to_vec(),
        1 => {
            let table = if large { 4 } else { 2 };
            let mut at = 2 + height * table;
            let mut out = Vec::new();
            for row in 0..height {
                let count = if large {
                    be32(bytes, 2 + row * 4) as usize
                } else {
                    usize::from(be16(bytes, 2 + row * 2))
                };
                let decoded = unpack(&bytes[at..at + count]);
                assert_eq!(decoded.len(), row_bytes, "RLE row width");
                out.extend(decoded);
                at += count;
            }
            out
        }
        word @ (2 | 3) => {
            let mut data =
                miniz_oxide::inflate::decompress_to_vec_zlib(&bytes[2..]).expect("a zlib plane");
            assert_eq!(data.len(), row_bytes * height, "ZIP plane size");
            if word == 3 {
                for row in data.chunks_mut(row_bytes.max(1)) {
                    if sb == 1 {
                        for i in 1..row.len() {
                            row[i] = row[i].wrapping_add(row[i - 1]);
                        }
                    } else {
                        let mut previous = 0u16;
                        for i in 0..row.len() / 2 {
                            let stored = u16::from_be_bytes([row[i * 2], row[i * 2 + 1]]);
                            let value = if i == 0 {
                                stored
                            } else {
                                previous.wrapping_add(stored)
                            };
                            row[i * 2..i * 2 + 2].copy_from_slice(&value.to_be_bytes());
                            previous = value;
                        }
                    }
                }
            }
            data
        }
        other => panic!("unsupported compression {other}"),
    };
    assert!(raw.len() >= row_bytes * height, "plane too short");
    if sb == 1 {
        raw[..row_bytes * height]
            .iter()
            .map(|v| u16::from(*v))
            .collect()
    } else {
        raw[..row_bytes * height]
            .chunks_exact(2)
            .map(|s| u16::from_be_bytes([s[0], s[1]]))
            .collect()
    }
}

/// The records of one layer-information body.
fn read_info(info: &[u8], depth: u16, large: bool) -> (Vec<ReadRecord>, bool) {
    if info.len() < 2 {
        return (Vec::new(), false);
    }
    let declared = be16(info, 0) as i16;
    let count = declared.unsigned_abs() as usize;
    let mut at = 2usize;
    let mut records = Vec::new();
    let mut shapes: Vec<Vec<(i16, usize)>> = Vec::new();
    for _ in 0..count {
        let bounds = [0, 4, 8, 12].map(|o| be32(info, at + o) as i32);
        at += 16;
        let channel_count = usize::from(be16(info, at));
        at += 2;
        let mut shape = Vec::new();
        for _ in 0..channel_count {
            let id = be16(info, at) as i16;
            let len = if large {
                be64(info, at + 2) as usize
            } else {
                be32(info, at + 2) as usize
            };
            at += if large { 10 } else { 6 };
            shape.push((id, len));
        }
        assert_eq!(&info[at..at + 4], b"8BIM", "record signature");
        let blend: [u8; 4] = info[at + 4..at + 8].try_into().unwrap();
        let (opacity, clipping, flags) = (info[at + 8], info[at + 9], info[at + 10]);
        at += 12;
        let extra_len = be32(info, at) as usize;
        at += 4;
        let extra = &info[at..at + extra_len];
        at += extra_len;
        let mask_len = be32(extra, 0) as usize;
        let mask = (mask_len >= 20).then(|| ReadMask {
            bounds: [4, 8, 12, 16].map(|o| be32(extra, o) as i32),
            outside: extra[20],
            flags: extra[21],
        });
        let mut cursor = 4 + mask_len;
        let ranges_len = be32(extra, cursor) as usize;
        let ranges = extra[cursor + 4..cursor + 4 + ranges_len].to_vec();
        cursor += 4 + ranges_len;
        let pascal_len = usize::from(extra[cursor]);
        let pascal =
            String::from_utf8_lossy(&extra[cursor + 1..cursor + 1 + pascal_len]).into_owned();
        cursor += 1 + pascal_len;
        while !cursor.is_multiple_of(4) {
            cursor += 1;
        }
        let (mut name, mut divider, mut divider_blend) = (pascal, None, None);
        let mut blocks = Vec::new();
        while cursor + 12 <= extra.len() {
            assert!(
                matches!(&extra[cursor..cursor + 4], b"8BIM" | b"8B64"),
                "block signature"
            );
            let key: [u8; 4] = extra[cursor + 4..cursor + 8].try_into().unwrap();
            let len = be32(extra, cursor + 8) as usize;
            let body = extra[cursor + 12..cursor + 12 + len].to_vec();
            cursor += 12 + len + len % 2;
            match &key {
                b"luni" => {
                    let units: Vec<u16> = body[4..]
                        .chunks_exact(2)
                        .map(|u| u16::from_be_bytes([u[0], u[1]]))
                        .take(be32(&body, 0) as usize)
                        .collect();
                    name = String::from_utf16(&units).expect("luni name");
                }
                b"lsct" => {
                    divider = Some(be32(&body, 0));
                    if body.len() >= 12 {
                        divider_blend = Some(body[8..12].try_into().unwrap());
                    }
                }
                _ => {}
            }
            blocks.push((key, body));
        }
        shapes.push(shape);
        records.push(ReadRecord {
            bounds,
            name,
            blend,
            opacity,
            clipping,
            flags,
            divider,
            divider_blend,
            mask,
            ranges,
            channels: Vec::new(),
            blocks,
        });
    }
    for (record, shape) in records.iter_mut().zip(shapes) {
        for (id, len) in shape {
            let data = &info[at..at + len];
            at += len;
            let (w, h) = if id == -2 {
                let m = record.mask.as_ref().expect("mask body");
                (
                    (m.bounds[3] - m.bounds[1]) as usize,
                    (m.bounds[2] - m.bounds[0]) as usize,
                )
            } else {
                (
                    (record.bounds[3] - record.bounds[1]).max(0) as usize,
                    (record.bounds[2] - record.bounds[0]).max(0) as usize,
                )
            };
            // A pixel-less record's placeholder, or an empty mask rectangle,
            // carries no samples to decode.
            let samples = if w == 0 || h == 0 {
                Vec::new()
            } else {
                decode_plane(data, w, h, depth, large)
            };
            record.channels.push((id, samples));
        }
    }
    (records, declared < 0)
}

/// Reads a CMYK PSD, PSB or big-endian layered TIFF (module doc).
pub fn read(bytes: &[u8]) -> ReadFile {
    if bytes.starts_with(b"MM\0*") {
        return read_tiff(bytes);
    }
    assert_eq!(&bytes[..4], b"8BPS");
    let large = be16(bytes, 4) == 2;
    let channels = be16(bytes, 12);
    let height = be32(bytes, 14);
    let width = be32(bytes, 18);
    let depth = be16(bytes, 22);
    let color = be16(bytes, 24);
    let mut cursor = 26;
    cursor += 4 + be32(bytes, cursor) as usize;
    let resources_len = be32(bytes, cursor) as usize;
    let resources = &bytes[cursor + 4..cursor + 4 + resources_len];
    cursor += 4 + resources_len;
    let mut icc = None;
    let mut resource_ids = Vec::new();
    let mut at = 0usize;
    while at < resources.len() {
        assert_eq!(&resources[at..at + 4], b"8BIM");
        let id = be16(resources, at + 4);
        at += 6;
        let name_len = usize::from(resources[at]);
        at += (name_len + 2) & !1;
        let len = be32(resources, at) as usize;
        at += 4;
        if id == 1039 {
            icc = Some(resources[at..at + len].to_vec());
        }
        resource_ids.push(id);
        at += len + len % 2;
    }
    let section_len = if large {
        be64(bytes, cursor) as usize
    } else {
        be32(bytes, cursor) as usize
    };
    let head = if large { 8 } else { 4 };
    let section = &bytes[cursor + head..cursor + head + section_len];
    cursor += head + section_len;
    let info_len = if large {
        be64(section, 0) as usize
    } else {
        be32(section, 0) as usize
    };
    let mut info = &section[head..head + info_len];
    if info_len == 0 {
        let mut at = head;
        if section.len() >= at + 4 {
            at += 4 + be32(section, at) as usize;
        }
        while at + 12 <= section.len() {
            let key = &section[at + 4..at + 8];
            let long = large
                && matches!(
                    key,
                    b"Lr16"
                        | b"Layr"
                        | b"LMsk"
                        | b"Lr32"
                        | b"Mt16"
                        | b"Mt32"
                        | b"Mtrn"
                        | b"Alph"
                        | b"FMsk"
                        | b"lnk2"
                        | b"FEid"
                        | b"FXid"
                        | b"PxSD"
                );
            let (len, body_at) = if long {
                (be64(section, at + 8) as usize, at + 16)
            } else {
                (be32(section, at + 8) as usize, at + 12)
            };
            if key == b"Lr16" {
                info = &section[body_at..body_at + len];
                break;
            }
            at = body_at + len + (len.wrapping_neg() & 3);
        }
    }
    let (records, merged_alpha) = read_info(info, depth, large);
    let composite_bytes = &bytes[cursor..];
    let pixels = width as usize * height as usize;
    let count = usize::from(channels);
    let sb = usize::from(depth / 8);
    let composite: Vec<Vec<u16>> = match be16(composite_bytes, 0) {
        0 => (0..count)
            .map(|index| {
                let plane =
                    &composite_bytes[2 + index * pixels * sb..2 + (index + 1) * pixels * sb];
                if sb == 1 {
                    plane.iter().map(|v| u16::from(*v)).collect()
                } else {
                    plane
                        .chunks_exact(2)
                        .map(|s| u16::from_be_bytes([s[0], s[1]]))
                        .collect()
                }
            })
            .collect(),
        1 => {
            let rows = count * height as usize;
            let table = if large { 4 } else { 2 };
            let mut offset = 2 + rows * table;
            let mut out = vec![Vec::new(); count];
            for row in 0..rows {
                let len = if large {
                    be32(composite_bytes, 2 + row * 4) as usize
                } else {
                    usize::from(be16(composite_bytes, 2 + row * 2))
                };
                let decoded = unpack(&composite_bytes[offset..offset + len]);
                assert_eq!(decoded.len(), width as usize * sb);
                offset += len;
                let values: Vec<u16> = if sb == 1 {
                    decoded.iter().map(|v| u16::from(*v)).collect()
                } else {
                    decoded
                        .chunks_exact(2)
                        .map(|s| u16::from_be_bytes([s[0], s[1]]))
                        .collect()
                };
                out[row / height as usize].extend(values);
            }
            out
        }
        other => panic!("unsupported composite compression {other}"),
    };
    ReadFile {
        tiff: false,
        large,
        width,
        height,
        depth,
        color,
        channels,
        icc,
        resource_ids,
        records,
        merged_alpha,
        composite,
    }
}

/// A big-endian layered TIFF: its directory's ICC and strip (uncompressed,
/// chunky), and tag 37724's `Layr`/`Lr16` block.
fn read_tiff(bytes: &[u8]) -> ReadFile {
    let directory = be32(bytes, 4) as usize;
    let count = usize::from(be16(bytes, directory));
    let mut tags = std::collections::HashMap::new();
    for i in 0..count {
        let at = directory + 2 + i * 12;
        let (tag, kind, n) = (
            be16(bytes, at),
            be16(bytes, at + 2),
            be32(bytes, at + 4) as usize,
        );
        let size = match kind {
            3 => 2,
            4 => 4,
            _ => 1,
        };
        let span = size * n;
        let start = if span <= 4 {
            at + 8
        } else {
            be32(bytes, at + 8) as usize
        };
        let raw = &bytes[start..start + span];
        let values: Vec<u64> = match kind {
            3 => raw
                .chunks_exact(2)
                .map(|s| u64::from(u16::from_be_bytes([s[0], s[1]])))
                .collect(),
            4 => raw
                .chunks_exact(4)
                .map(|s| u64::from(u32::from_be_bytes(s.try_into().unwrap())))
                .collect(),
            _ => Vec::new(),
        };
        tags.insert(tag, (values, raw.to_vec()));
    }
    let width = tags[&256].0[0] as u32;
    let height = tags[&257].0[0] as u32;
    let depth = tags[&258].0[0] as u16;
    let samples = tags[&277].0[0] as usize;
    assert_eq!(tags[&262].0[0], 5, "photometric");
    assert_eq!(tags.get(&259).map_or(1, |t| t.0[0]), 1, "uncompressed");
    let mut strip = Vec::new();
    for (offset, length) in tags[&273].0.iter().zip(&tags[&279].0) {
        strip.extend_from_slice(&bytes[*offset as usize..(*offset + *length) as usize]);
    }
    let sb = usize::from(depth / 8);
    let values: Vec<u16> = if sb == 1 {
        strip.iter().map(|v| u16::from(*v)).collect()
    } else {
        strip
            .chunks_exact(2)
            .map(|s| u16::from_be_bytes([s[0], s[1]]))
            .collect()
    };
    let pixels = width as usize * height as usize;
    let composite: Vec<Vec<u16>> = (0..samples)
        .map(|c| (0..pixels).map(|i| values[i * samples + c]).collect())
        .collect();
    let block = &tags[&37724].1;
    let header = b"Adobe Photoshop Document Data Block\0";
    assert_eq!(&block[..header.len()], header);
    let wanted: &[u8; 4] = if depth == 8 { b"Layr" } else { b"Lr16" };
    let mut at = header.len();
    let mut info: &[u8] = &[];
    while at + 12 <= block.len() {
        assert_eq!(&block[at..at + 4], b"8BIM");
        let len = be32(block, at + 8) as usize;
        if &block[at + 4..at + 8] == wanted {
            info = &block[at + 12..at + 12 + len];
            break;
        }
        at += 12 + len + (len.wrapping_neg() & 3);
    }
    let (records, merged_alpha) = read_info(info, depth, false);
    ReadFile {
        tiff: true,
        large: false,
        width,
        height,
        depth,
        color: 4,
        channels: samples as u16,
        icc: tags.get(&34675).map(|t| t.1.clone()),
        resource_ids: Vec::new(),
        records,
        merged_alpha,
        composite,
    }
}

/// An uncompressed CMYK TIFF of either byte order, chunky or planar, as its
/// planes (one per sample, at the file's depth) and its ExtraSamples values.
pub struct FlatTiff {
    pub width: u32,
    pub height: u32,
    pub depth: u16,
    pub planes: Vec<Vec<u16>>,
    pub extra_samples: Vec<u16>,
    pub planar: bool,
}

pub fn read_flat_tiff(bytes: &[u8]) -> FlatTiff {
    let little = &bytes[..2] == b"II";
    let u16_at = |at: usize| {
        let b = [bytes[at], bytes[at + 1]];
        if little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        }
    };
    let u32_at = |at: usize| {
        let b: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
        if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    };
    let directory = u32_at(4) as usize;
    let count = usize::from(u16_at(directory));
    let mut tags = std::collections::HashMap::new();
    for i in 0..count {
        let at = directory + 2 + i * 12;
        let (tag, kind, n) = (u16_at(at), u16_at(at + 2), u32_at(at + 4) as usize);
        let size = match kind {
            3 => 2,
            4 => 4,
            _ => continue,
        };
        let start = if size * n <= 4 {
            at + 8
        } else {
            u32_at(at + 8) as usize
        };
        let values: Vec<u64> = (0..n)
            .map(|k| {
                if size == 2 {
                    u64::from(u16_at(start + k * 2))
                } else {
                    u64::from(u32_at(start + k * 4))
                }
            })
            .collect();
        tags.insert(tag, values);
    }
    assert_eq!(tags.get(&259).map_or(1, |t| t[0]), 1, "uncompressed");
    assert_eq!(tags[&262][0], 5, "separated");
    let width = tags[&256][0] as u32;
    let height = tags[&257][0] as u32;
    let depth = tags[&258][0] as u16;
    let samples = tags[&277][0] as usize;
    let planar = tags.get(&284).map_or(1, |t| t[0]) == 2;
    let mut data = Vec::new();
    for (offset, length) in tags[&273].iter().zip(&tags[&279]) {
        data.extend_from_slice(&bytes[*offset as usize..(*offset + *length) as usize]);
    }
    let values: Vec<u16> = if depth == 8 {
        data.iter().map(|v| u16::from(*v)).collect()
    } else {
        data.chunks_exact(2)
            .map(|s| {
                if little {
                    u16::from_le_bytes([s[0], s[1]])
                } else {
                    u16::from_be_bytes([s[0], s[1]])
                }
            })
            .collect()
    };
    let pixels = width as usize * height as usize;
    let planes = (0..samples)
        .map(|c| {
            (0..pixels)
                .map(|i| {
                    if planar {
                        values[c * pixels + i]
                    } else {
                        values[i * samples + c]
                    }
                })
                .collect()
        })
        .collect();
    FlatTiff {
        width,
        height,
        depth,
        planes,
        extra_samples: tags
            .get(&338)
            .map_or(Vec::new(), |t| t.iter().map(|v| *v as u16).collect()),
        planar,
    }
}
