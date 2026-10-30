//! An independent 16-bit classic-PSD byte builder and reader, written from
//! Adobe's published framing. It has its own PackBits packer and unpacker, its
//! own record/resource layout and its own white-matte arithmetic, and it never
//! calls the writer, decoder or matte it is used to check.
#![allow(dead_code)]

use std::collections::HashMap;

// ------------------------------------------------------------- PackBits --

/// A deliberately different packer from the core's: it emits a repeat run for
/// any run of 2 or more equal bytes (the core requires 3), so its output is a
/// legal but distinct encoding of the same row.
pub fn pack(row: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut literal: Vec<u8> = Vec::new();
    let mut cursor = 0usize;
    let flush = |out: &mut Vec<u8>, literal: &mut Vec<u8>| {
        for chunk in literal.chunks(128) {
            out.push((chunk.len() - 1) as u8);
            out.extend_from_slice(chunk);
        }
        literal.clear();
    };
    while cursor < row.len() {
        let mut run = 1usize;
        while run < 128 && cursor + run < row.len() && row[cursor + run] == row[cursor] {
            run += 1;
        }
        if run >= 2 {
            flush(&mut out, &mut literal);
            out.push((1i16 - run as i16) as i8 as u8);
            out.push(row[cursor]);
            cursor += run;
        } else {
            literal.push(row[cursor]);
            cursor += 1;
        }
    }
    flush(&mut out, &mut literal);
    out
}

/// The matching unpacker, written from the same published description.
pub fn unpack(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let control = bytes[cursor] as i8;
        cursor += 1;
        if control >= 0 {
            let count = control as usize + 1;
            out.extend_from_slice(&bytes[cursor..cursor + count]);
            cursor += count;
        } else if control != -128 {
            let count = (1 - i32::from(control)) as usize;
            out.extend(std::iter::repeat_n(bytes[cursor], count));
            cursor += 1;
        }
    }
    out
}

// -------------------------------------------------------------- planes --

pub fn be16(samples: &[u16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_be_bytes()).collect()
}

/// A Raw plane: compression word 0 and big-endian samples.
pub fn raw_plane(samples: &[u16]) -> Vec<u8> {
    [vec![0, 0], be16(samples)].concat()
}

/// An RLE plane: compression word 1, a byte count per row, then packed rows of
/// `width * 2` bytes each.
pub fn rle_plane(samples: &[u16], width: usize) -> Vec<u8> {
    let bytes = be16(samples);
    let rows: Vec<Vec<u8>> = bytes.chunks(width * 2).map(pack).collect();
    let mut out = vec![0, 1];
    for row in &rows {
        out.extend((row.len() as u16).to_be_bytes());
    }
    for row in &rows {
        out.extend(row);
    }
    out
}

// ----------------------------------------------------------------- ZIP --

/// Adler-32 of `data`, written from RFC 1950's definition.
fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    b << 16 | a
}

/// A legal zlib stream of `data` built ONLY out of RFC 1951 stored blocks:
/// the `0x78 0x01` header (CM 8, and a header word divisible by 31), then
/// `BFINAL|BTYPE=00`, `LEN`, `~LEN` and the literal bytes, then Adler-32.
/// This is a deliberately different encoder from any inflater under test.
pub fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78u8, 0x01];
    let mut chunks: Vec<&[u8]> = data.chunks(65535).collect();
    if chunks.is_empty() {
        chunks.push(&[]);
    }
    let last = chunks.len() - 1;
    for (index, chunk) in chunks.iter().enumerate() {
        out.push(u8::from(index == last));
        out.extend((chunk.len() as u16).to_le_bytes());
        out.extend((!(chunk.len() as u16)).to_le_bytes());
        out.extend(*chunk);
    }
    out.extend(adler32(data).to_be_bytes());
    out
}

/// A ZIP plane: compression word 2, or 3 with each row's big-endian u16
/// samples replaced by their wrapping difference from the previous sample of
/// the SAME row (the first sample of a row is stored as it stands).
pub fn zip_plane(samples: &[u16], width: usize, predicted: bool) -> Vec<u8> {
    let mut body = Vec::with_capacity(samples.len() * 2);
    for row in samples.chunks(width) {
        let mut previous = 0u16;
        for (index, value) in row.iter().enumerate() {
            let stored = if index == 0 {
                *value
            } else {
                value.wrapping_sub(previous)
            };
            body.extend(if predicted { stored } else { *value }.to_be_bytes());
            previous = *value;
        }
    }
    let mut out = if predicted { vec![0, 3] } else { vec![0, 2] };
    out.extend(zlib_stored(&body));
    out
}

/// A 32-bit ZIP plane: compression word 2 over the samples' big-endian f32
/// bytes as they stand, or 3 with Photoshop's byte-plane prediction, written
/// from its own files' layout: each row becomes four planes of `width` bytes
/// (every sample's first byte, then every second, third and fourth), and
/// every byte of the row after its first is replaced by its wrapping
/// difference from the byte before it. Rows never predict across each other.
pub fn zip_plane32(samples: &[f32], width: usize, predicted: bool) -> Vec<u8> {
    let mut body = Vec::with_capacity(samples.len() * 4);
    for row in samples.chunks(width) {
        let be: Vec<[u8; 4]> = row.iter().map(|value| value.to_be_bytes()).collect();
        if !predicted {
            body.extend(be.iter().flatten());
            continue;
        }
        let planes: Vec<u8> = (0..4)
            .flat_map(|byte| be.iter().map(move |sample| sample[byte]))
            .collect();
        for (index, byte) in planes.iter().enumerate() {
            body.push(if index == 0 {
                *byte
            } else {
                byte.wrapping_sub(planes[index - 1])
            });
        }
    }
    let mut out = if predicted { vec![0, 3] } else { vec![0, 2] };
    out.extend(zlib_stored(&body));
    out
}

/// This reader's OWN inflater: it accepts the stored-block zlib
/// [`zlib_stored`] writes and nothing else, so it shares no code at all with
/// the decoder under test.
fn inflate_stored(bytes: &[u8]) -> Vec<u8> {
    assert_eq!(bytes[0] & 0x0f, 8, "zlib deflate method");
    assert_eq!(
        (u16::from(bytes[0]) << 8 | u16::from(bytes[1])) % 31,
        0,
        "zlib header check"
    );
    let mut out = Vec::new();
    let mut at = 2usize;
    loop {
        let header = bytes[at];
        assert_eq!(header & 6, 0, "this reader inflates stored blocks only");
        let len = u16::from_le_bytes([bytes[at + 1], bytes[at + 2]]) as usize;
        let nlen = u16::from_le_bytes([bytes[at + 3], bytes[at + 4]]);
        assert_eq!(nlen, !(len as u16), "stored block length complement");
        out.extend_from_slice(&bytes[at + 5..at + 5 + len]);
        at += 5 + len;
        if header & 1 != 0 {
            break;
        }
    }
    assert_eq!(
        u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()),
        adler32(&out),
        "zlib Adler-32"
    );
    out
}

// ------------------------------------------------------------- records --

pub fn block(key: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [
        b"8BIM".as_slice(),
        key,
        &(body.len() as u32).to_be_bytes(),
        body,
    ]
    .concat()
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
pub struct Record {
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
    /// The layer blending ranges section's bytes, verbatim (empty = none).
    pub ranges: Vec<u8>,
    /// Additional layer blocks appended after the section divider, verbatim.
    pub blocks: Vec<u8>,
}

impl Record {
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

    pub fn folder(name: &str, divider: u32, blend: [u8; 4]) -> Self {
        Self {
            bounds: [0; 4],
            channels: [-1i16, 0, 1, 2]
                .into_iter()
                .map(|id| (id, vec![0, 0]))
                .collect(),
            name: name.to_string(),
            divider,
            blend,
            opacity: 255,
            clipping: 0,
            flags: 0x18,
            mask_body: Vec::new(),
            ranges: Vec::new(),
            blocks: Vec::new(),
        }
    }

    pub fn mask(mut self, bounds: [i32; 4], outside: u8, enabled: bool, plane: Vec<u8>) -> Self {
        self.mask_body = mask_body(bounds, outside, if enabled { 0 } else { 2 });
        self.channels.push((-2, plane));
        self
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for value in self.bounds {
            out.extend(value.to_be_bytes());
        }
        out.extend((self.channels.len() as u16).to_be_bytes());
        for (id, data) in &self.channels {
            out.extend(id.to_be_bytes());
            out.extend((data.len() as u32).to_be_bytes());
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

fn resource(id: u16, body: &[u8]) -> Vec<u8> {
    let mut out = b"8BIM".to_vec();
    out.extend(id.to_be_bytes());
    out.extend([0, 0]);
    out.extend((body.len() as u32).to_be_bytes());
    out.extend(body);
    if !body.len().is_multiple_of(2) {
        out.push(0);
    }
    out
}

pub fn resolution_info(x: f32, y: f32) -> Vec<u8> {
    let fixed = |v: f32| (((v as f64) * 65536.0).round() as u32).to_be_bytes();
    let mut out = fixed(x).to_vec();
    out.extend([0, 1, 0, 1]);
    out.extend(fixed(y));
    out.extend([0, 1, 0, 1]);
    out
}

/// One colour sample composited over white, this file's own rational form of
/// the matte every classic writer applies to a stored composite.
pub fn matte(color: u16, alpha: u16) -> u16 {
    let (c, a) = (f64::from(color), f64::from(alpha));
    ((c * a + 65535.0 * (65535.0 - a)) / 65535.0).round() as u16
}

/// Its inverse, the value a reader must recover from a matted composite.
pub fn unmatte(color: u16, alpha: u16) -> u16 {
    match alpha {
        0 => 0,
        65535 => color,
        _ => {
            let excess = f64::from(color) + f64::from(alpha) - 65535.0;
            (excess.max(0.0) * 65535.0 / f64::from(alpha))
                .round()
                .min(65535.0) as u16
        }
    }
}

/// A complete 16-bit RGB PSD. `composite` is STRAIGHT RGBA16; it is stored
/// white-matted with four channels when `merged_alpha`, else three channels.
pub fn fixture(
    canvas: (u32, u32),
    records: &[Record],
    icc: &[u8],
    ppi: (f32, f32),
    composite: &[[u16; 4]],
    merged_alpha: bool,
    rle_composite: bool,
) -> Vec<u8> {
    let mut info = if records.is_empty() {
        Vec::new()
    } else {
        let count = records.len() as i16;
        if merged_alpha { -count } else { count }
            .to_be_bytes()
            .to_vec()
    };
    for record in records {
        info.extend(record.encode());
    }
    for record in records {
        for (_, data) in &record.channels {
            info.extend(data);
        }
    }
    if !info.len().is_multiple_of(2) {
        info.push(0);
    }
    let mut section = (info.len() as u32).to_be_bytes().to_vec();
    section.extend(&info);
    section.extend(0u32.to_be_bytes()); // no global layer mask
    assemble(
        canvas,
        &section,
        icc,
        ppi,
        composite,
        merged_alpha,
        rle_composite,
    )
}

/// Photoshop's own 16-bit layout: an EMPTY layer-information section, an
/// empty global layer mask, then tagged tail blocks — `leading_blocks`
/// verbatim, and the records in an `Lr16` block. Every tail block is padded
/// to a multiple of four, which is what Photoshop 26.10.0 writes.
pub fn fixture_lr16(
    canvas: (u32, u32),
    records: &[Record],
    icc: &[u8],
    ppi: (f32, f32),
    composite: &[[u16; 4]],
    merged_alpha: bool,
    leading_blocks: &[(&[u8; 4], &[u8])],
) -> Vec<u8> {
    let mut body = (if merged_alpha {
        -(records.len() as i16)
    } else {
        records.len() as i16
    })
    .to_be_bytes()
    .to_vec();
    for record in records {
        body.extend(record.encode());
    }
    for record in records {
        for (_, data) in &record.channels {
            body.extend(data);
        }
    }
    let mut tail = Vec::new();
    for (key, block_body) in leading_blocks {
        tail.extend(block(key, block_body));
        while !tail.len().is_multiple_of(4) {
            tail.push(0);
        }
    }
    tail.extend(block(b"Lr16", &body));
    while !tail.len().is_multiple_of(4) {
        tail.push(0);
    }
    let mut section = 0u32.to_be_bytes().to_vec(); // empty layer information
    section.extend(0u32.to_be_bytes()); // no global layer mask
    section.extend(&tail);
    assemble(canvas, &section, icc, ppi, composite, merged_alpha, false)
}

/// The header, resources and stored composite shared by both layouts.
fn assemble(
    canvas: (u32, u32),
    section: &[u8],
    icc: &[u8],
    ppi: (f32, f32),
    composite: &[[u16; 4]],
    merged_alpha: bool,
    rle_composite: bool,
) -> Vec<u8> {
    let (width, height) = canvas;
    let channels: u16 = if merged_alpha { 4 } else { 3 };
    let mut resources = resource(1039, icc);
    resources.extend(resource(1005, &resolution_info(ppi.0, ppi.1)));

    let mut bytes = b"8BPS\0\x01\0\0\0\0\0\0".to_vec();
    bytes.extend(channels.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend(width.to_be_bytes());
    bytes.extend(16u16.to_be_bytes());
    bytes.extend(3u16.to_be_bytes());
    bytes.extend(0u32.to_be_bytes()); // no colour-mode data
    bytes.extend((resources.len() as u32).to_be_bytes());
    bytes.extend(&resources);
    bytes.extend((section.len() as u32).to_be_bytes());
    bytes.extend_from_slice(section);

    let planes: Vec<Vec<u16>> = (0..channels as usize)
        .map(|component| {
            composite
                .iter()
                .map(|pixel| {
                    if component == 3 {
                        pixel[3]
                    } else if merged_alpha {
                        matte(pixel[component], pixel[3])
                    } else {
                        pixel[component]
                    }
                })
                .collect()
        })
        .collect();
    if rle_composite {
        bytes.extend(1u16.to_be_bytes());
        let mut table = Vec::new();
        let mut body = Vec::new();
        for plane in &planes {
            for row in be16(plane).chunks(width as usize * 2) {
                let packed = pack(row);
                table.extend((packed.len() as u16).to_be_bytes());
                body.extend(packed);
            }
        }
        bytes.extend(table);
        bytes.extend(body);
    } else {
        bytes.extend(0u16.to_be_bytes());
        for plane in &planes {
            bytes.extend(be16(plane));
        }
    }
    bytes
}

// -------------------------------------------------------------- reading --

#[derive(Debug)]
pub struct ReadMask {
    pub bounds: [i32; 4],
    pub outside: u8,
    pub flags: u8,
}

#[derive(Debug)]
pub struct ReadRecord {
    pub bounds: [i32; 4],
    pub name: String,
    pub pascal: String,
    pub blend: [u8; 4],
    pub opacity: u8,
    pub clipping: u8,
    pub flags: u8,
    pub divider: Option<u32>,
    pub divider_blend: Option<[u8; 4]>,
    pub mask: Option<ReadMask>,
    /// The layer blending ranges section's bytes, verbatim.
    pub ranges: Vec<u8>,
    pub channels: Vec<(i16, Vec<u16>)>,
    /// Every additional layer block after the name, verbatim: key and body.
    pub blocks: Vec<([u8; 4], Vec<u8>)>,
}

impl ReadRecord {
    pub fn plane(&self, id: i16) -> &[u16] {
        &self
            .channels
            .iter()
            .find(|(channel, _)| *channel == id)
            .unwrap_or_else(|| panic!("channel {id} of “{}”", self.name))
            .1
    }
    /// The record's straight RGBA16 samples, alpha first in PSD channel order.
    pub fn rgba(&self) -> Vec<[u16; 4]> {
        let alpha = self.plane(-1);
        let (r, g, b) = (self.plane(0), self.plane(1), self.plane(2));
        (0..alpha.len())
            .map(|i| [r[i], g[i], b[i], alpha[i]])
            .collect()
    }
}

pub struct ReadFile {
    pub width: u32,
    pub height: u32,
    pub depth: u16,
    pub color: u16,
    pub channels: u16,
    pub icc: Vec<u8>,
    pub resolution: Vec<u8>,
    pub records: Vec<ReadRecord>,
    pub merged_alpha: bool,
    /// The stored composite, exactly as written: still white-matted.
    pub composite: Vec<[u16; 4]>,
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes(bytes[at..at + 2].try_into().unwrap())
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn i32_at(bytes: &[u8], at: usize) -> i32 {
    i32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// Decode one channel plane of `height` rows of `width` samples.
fn plane(bytes: &[u8], width: usize, height: usize) -> Vec<u16> {
    let pairs = |data: &[u8]| -> Vec<u16> {
        data.chunks_exact(2)
            .map(|s| u16::from_be_bytes([s[0], s[1]]))
            .collect()
    };
    match u16_at(bytes, 0) {
        0 => pairs(&bytes[2..]),
        1 => {
            let table_end = 2 + height * 2;
            let mut offset = table_end;
            let mut out = Vec::new();
            for row in 0..height {
                let count = u16_at(bytes, 2 + row * 2) as usize;
                let decoded = unpack(&bytes[offset..offset + count]);
                assert_eq!(decoded.len(), width * 2, "decoded RLE row width");
                out.extend(pairs(&decoded));
                offset += count;
            }
            assert_eq!(offset, bytes.len(), "trailing RLE data");
            out
        }
        // This reader's OWN ZIP path: inflate, then undo the per-row
        // prediction by running each row's wrapping sum forward again.
        word @ (2 | 3) => {
            let raw = inflate_stored(&bytes[2..]);
            assert_eq!(raw.len(), width * height * 2, "ZIP plane length");
            let mut out = Vec::with_capacity(width * height);
            for row in raw.chunks_exact(width * 2) {
                let mut previous = 0u16;
                for (index, sample) in row.chunks_exact(2).enumerate() {
                    let stored = u16::from_be_bytes([sample[0], sample[1]]);
                    let value = if word == 2 || index == 0 {
                        stored
                    } else {
                        previous.wrapping_add(stored)
                    };
                    out.push(value);
                    previous = value;
                }
            }
            out
        }
        other => panic!("unsupported compression {other}"),
    }
}

pub fn read(bytes: &[u8]) -> ReadFile {
    assert_eq!(&bytes[..4], b"8BPS");
    assert_eq!(u16_at(bytes, 4), 1);
    let channels = u16_at(bytes, 12);
    let height = u32_at(bytes, 14);
    let width = u32_at(bytes, 18);
    let depth = u16_at(bytes, 22);
    let color = u16_at(bytes, 24);
    let mut cursor = 26;
    let mode_len = u32_at(bytes, cursor) as usize;
    cursor += 4 + mode_len;
    let resources_len = u32_at(bytes, cursor) as usize;
    let resources = &bytes[cursor + 4..cursor + 4 + resources_len];
    cursor += 4 + resources_len;

    let (mut icc, mut resolution) = (Vec::new(), Vec::new());
    let mut at = 0usize;
    while at < resources.len() {
        assert_eq!(&resources[at..at + 4], b"8BIM");
        let id = u16_at(resources, at + 4);
        at += 6;
        let name_len = resources[at] as usize;
        at += (name_len + 2) & !1;
        let len = u32_at(resources, at) as usize;
        at += 4;
        let body = &resources[at..at + len];
        at += len + len % 2;
        match id {
            1039 => icc = body.to_vec(),
            1005 => resolution = body.to_vec(),
            _ => {}
        }
    }

    let section_len = u32_at(bytes, cursor) as usize;
    let section = &bytes[cursor + 4..cursor + 4 + section_len];
    cursor += 4 + section_len;
    let info_len = u32_at(section, 0) as usize;
    let mut info = &section[4..4 + info_len];
    if info_len == 0 {
        // Photoshop's 16-bit layout: the records live in an `Lr16` tail block
        // after the empty global layer mask, every block padded to a multiple
        // of four.
        let mut at = 4;
        if section.len() >= at + 4 {
            at += 4 + u32_at(section, at) as usize;
        }
        while at + 12 <= section.len() {
            assert_eq!(&section[at..at + 4], b"8BIM", "tail block signature");
            let key = &section[at + 4..at + 8];
            let len = u32_at(section, at + 8) as usize;
            if key == b"Lr16" {
                info = &section[at + 12..at + 12 + len];
                break;
            }
            at += 12 + len + (len.wrapping_neg() & 3);
        }
    }
    let mut records = Vec::new();
    let mut merged_alpha = false;
    if !info.is_empty() {
        let declared = i16::from_be_bytes(info[..2].try_into().unwrap());
        merged_alpha = declared < 0;
        let count = declared.unsigned_abs() as usize;
        let mut at = 2usize;
        let mut shapes: Vec<Vec<(i16, usize)>> = Vec::new();
        for _ in 0..count {
            let bounds = [
                i32_at(info, at),
                i32_at(info, at + 4),
                i32_at(info, at + 8),
                i32_at(info, at + 12),
            ];
            at += 16;
            let channel_count = u16_at(info, at) as usize;
            at += 2;
            let mut shape = Vec::new();
            for _ in 0..channel_count {
                let id = i16::from_be_bytes(info[at..at + 2].try_into().unwrap());
                let len = u32_at(info, at + 2) as usize;
                at += 6;
                shape.push((id, len));
            }
            assert_eq!(&info[at..at + 4], b"8BIM");
            let blend: [u8; 4] = info[at + 4..at + 8].try_into().unwrap();
            let opacity = info[at + 8];
            let clipping = info[at + 9];
            let flags = info[at + 10];
            at += 12;
            let extra_len = u32_at(info, at) as usize;
            at += 4;
            let extra = &info[at..at + extra_len];
            at += extra_len;

            let mut cursor = 0usize;
            let mask_len = u32_at(extra, cursor) as usize;
            cursor += 4;
            let mask = (mask_len >= 20).then(|| ReadMask {
                bounds: [
                    i32_at(extra, cursor),
                    i32_at(extra, cursor + 4),
                    i32_at(extra, cursor + 8),
                    i32_at(extra, cursor + 12),
                ],
                outside: extra[cursor + 16],
                flags: extra[cursor + 17],
            });
            cursor += mask_len;
            let ranges_len = u32_at(extra, cursor) as usize;
            let ranges = extra[cursor + 4..cursor + 4 + ranges_len].to_vec();
            cursor += 4 + ranges_len;
            let pascal_len = extra[cursor] as usize;
            let pascal = String::from_utf8_lossy(&extra[cursor + 1..cursor + 1 + pascal_len]);
            cursor += 1 + pascal_len;
            while !cursor.is_multiple_of(4) {
                cursor += 1;
            }
            let (mut name, mut divider, mut divider_blend) = (String::new(), None, None);
            let mut blocks = Vec::new();
            while cursor + 12 <= extra.len() {
                assert_eq!(&extra[cursor..cursor + 4], b"8BIM");
                let key: [u8; 4] = extra[cursor + 4..cursor + 8].try_into().unwrap();
                let len = u32_at(extra, cursor + 8) as usize;
                let body = &extra[cursor + 12..cursor + 12 + len];
                cursor += 12 + len + len % 2;
                blocks.push((key, body.to_vec()));
                match &key {
                    b"luni" => {
                        let units: Vec<u16> = body[4..]
                            .chunks_exact(2)
                            .map(|u| u16::from_be_bytes([u[0], u[1]]))
                            .collect();
                        name = String::from_utf16(&units).expect("luni name");
                    }
                    b"lsct" => {
                        divider = Some(u32_at(body, 0));
                        if len >= 12 {
                            divider_blend = Some(body[8..12].try_into().unwrap());
                        }
                    }
                    _ => {}
                }
            }
            shapes.push(shape);
            records.push(ReadRecord {
                bounds,
                name,
                pascal: pascal.into_owned(),
                blend,
                opacity,
                clipping,
                flags,
                divider,
                divider_blend,
                mask: None,
                ranges,
                channels: Vec::new(),
                blocks,
            });
            records.last_mut().unwrap().mask = mask;
        }
        for (record, shape) in records.iter_mut().zip(shapes) {
            for (id, len) in shape {
                let data = &info[at..at + len];
                at += len;
                let (w, h) = if id == -2 {
                    let mask = record.mask.as_ref().expect("mask body");
                    (
                        (mask.bounds[3] - mask.bounds[1]) as usize,
                        (mask.bounds[2] - mask.bounds[0]) as usize,
                    )
                } else {
                    (
                        (record.bounds[3] - record.bounds[1]) as usize,
                        (record.bounds[2] - record.bounds[0]) as usize,
                    )
                };
                record.channels.push((id, plane(data, w, h)));
            }
        }
    }

    let composite_bytes = &bytes[cursor..];
    let pixels = width as usize * height as usize;
    let count = usize::from(channels);
    let planes: Vec<Vec<u16>> = match u16_at(composite_bytes, 0) {
        0 => (0..count)
            .map(|index| {
                composite_bytes[2 + index * pixels * 2..2 + (index + 1) * pixels * 2]
                    .chunks_exact(2)
                    .map(|s| u16::from_be_bytes([s[0], s[1]]))
                    .collect()
            })
            .collect(),
        1 => {
            let rows = count * height as usize;
            let mut offset = 2 + rows * 2;
            let mut out = vec![Vec::new(); count];
            for row in 0..rows {
                let len = u16_at(composite_bytes, 2 + row * 2) as usize;
                let decoded = unpack(&composite_bytes[offset..offset + len]);
                assert_eq!(decoded.len(), width as usize * 2);
                offset += len;
                out[row / height as usize].extend(
                    decoded
                        .chunks_exact(2)
                        .map(|s| u16::from_be_bytes([s[0], s[1]])),
                );
            }
            assert_eq!(offset, composite_bytes.len());
            out
        }
        other => panic!("unsupported composite compression {other}"),
    };
    let composite = (0..pixels)
        .map(|i| {
            [
                planes[0][i],
                planes[1][i],
                planes[2][i],
                if count > 3 { planes[3][i] } else { 65535 },
            ]
        })
        .collect();
    ReadFile {
        width,
        height,
        depth,
        color,
        channels,
        icc,
        resolution,
        records,
        merged_alpha,
        composite,
    }
}

/// A convenience index of the records by name, for assertions that do not care
/// about file order.
pub fn by_name(file: &ReadFile) -> HashMap<String, &ReadRecord> {
    file.records
        .iter()
        .map(|record| (record.name.clone(), record))
        .collect()
}
