//! An independent PSD/PSB builder and parser for the v1 format tests, written
//! from Adobe's "Photoshop File Formats" table
//! (https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/) and sharing
//! NO code with `src/`: its own header and section framing at both widths
//! (version 1: u32 section and channel lengths, u16 RLE row counts; version 2,
//! PSB: u64 and u32), its own literal-only PackBits writer and its own
//! PackBits reader, its own stored-block zlib writer with its own Adler-32,
//! and its own per-row delta encoder for compression word 3.
//!
//! The builder writes what the importer must read; the parser reads what the
//! exporter wrote. Neither is ever compared against the crate's own framing.
//! The C plumbing at the bottom is shared by the new PSD-family test files.
#![allow(dead_code)]

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::Value;

use crate::common::{cpath, take_err_string};

pub const PSD: i32 = 0;
pub const PSB: i32 = 1;
pub const TIFF: i32 = 2;
pub const LAYERS: i32 = 0;
pub const FLATTENED: i32 = 1;

// ------------------------------------------------------------ primitives --

/// Adler-32 (RFC 1950), written here rather than borrowed.
pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// A zlib stream of STORED blocks: no compression, so the test owns every
/// byte of the stream it hands the importer.
pub fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut chunks = data.chunks(65535).peekable();
    if chunks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(chunk) = chunks.next() {
        out.push(u8::from(chunks.peek().is_none()));
        out.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
        out.extend_from_slice(&(!(chunk.len() as u16)).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// Compression word 3's per-row delta, forwards: each sample after a row's
/// first becomes its difference from the one before it, wrapping.
pub fn predict(plane: &[u8], row_bytes: usize, sample_bytes: usize) -> Vec<u8> {
    let mut out = plane.to_vec();
    for row in out.chunks_exact_mut(row_bytes) {
        if sample_bytes == 1 {
            for index in (1..row.len()).rev() {
                row[index] = row[index].wrapping_sub(row[index - 1]);
            }
        } else {
            for index in (1..row.len() / 2).rev() {
                let current = u16::from_be_bytes([row[index * 2], row[index * 2 + 1]]);
                let before = u16::from_be_bytes([row[index * 2 - 2], row[index * 2 - 1]]);
                row[index * 2..index * 2 + 2]
                    .copy_from_slice(&current.wrapping_sub(before).to_be_bytes());
            }
        }
    }
    out
}

/// PackBits with literal runs only: valid, and trivially correct.
fn pack_literal(row: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in row.chunks(128) {
        out.push((chunk.len() - 1) as u8);
        out.extend_from_slice(chunk);
    }
    out
}

/// The test's own PackBits reader.
fn unpack(row: &[u8]) -> Vec<u8> {
    let (mut out, mut at) = (Vec::new(), 0);
    while at < row.len() {
        let control = row[at] as i8;
        at += 1;
        if control >= 0 {
            let count = control as usize + 1;
            out.extend_from_slice(&row[at..at + count]);
            at += count;
        } else if control != -128 {
            out.extend(std::iter::repeat_n(
                row[at],
                (1 - i16::from(control)) as usize,
            ));
            at += 1;
        }
    }
    out
}

/// One plane's bytes behind its compression word. `plane` is row-major sample
/// bytes (big-endian pairs at 16 bits).
pub fn encode_plane(
    plane: &[u8],
    row_bytes: usize,
    sample_bytes: usize,
    word: u16,
    large: bool,
) -> Vec<u8> {
    let mut out = word.to_be_bytes().to_vec();
    match word {
        0 => out.extend_from_slice(plane),
        1 => {
            let rows: Vec<Vec<u8>> = plane.chunks(row_bytes.max(1)).map(pack_literal).collect();
            for row in &rows {
                if large {
                    out.extend_from_slice(&(row.len() as u32).to_be_bytes());
                } else {
                    out.extend_from_slice(&(row.len() as u16).to_be_bytes());
                }
            }
            for row in rows {
                out.extend_from_slice(&row);
            }
        }
        2 => out.extend_from_slice(&zlib_stored(plane)),
        _ => out.extend_from_slice(&zlib_stored(&predict(plane, row_bytes, sample_bytes))),
    }
    out
}

/// Component `c` of `rgba` as a plane of `sample_bytes`-wide samples.
pub fn plane_of(rgba: &[[u16; 4]], component: usize, sample_bytes: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len() * sample_bytes);
    for pixel in rgba {
        if sample_bytes == 1 {
            out.push(pixel[component] as u8);
        } else {
            out.extend_from_slice(&pixel[component].to_be_bytes());
        }
    }
    out
}

// --------------------------------------------------------------- builder --

/// One raster record the builder writes. `rgba` holds `sample_bytes`-range
/// codes (0..=255 at 8 bits), row-major over the rectangle `bounds`
/// (`[top, left, bottom, right]`).
#[derive(Clone)]
pub struct Layer {
    pub name: String,
    pub bounds: [i32; 4],
    pub rgba: Vec<[u16; 4]>,
    /// The compression word of every channel.
    pub word: u16,
    /// Raw additional-layer bytes appended to the record's extra data.
    pub extra_blocks: Vec<u8>,
    /// The record's raster mask, written as channel −2 behind the layer's own
    /// compression word with a 20-byte mask body.
    pub mask: Option<Mask>,
}

/// One raster mask: its own rectangle (`[top, left, bottom, right]`, canvas
/// coordinates), its row-major sample bytes (`sample_bytes` wide) and the
/// default colour outside the rectangle (0 or 255). Enabled, flags 0.
#[derive(Clone)]
pub struct Mask {
    pub bounds: [i32; 4],
    pub samples: Vec<u8>,
    pub outside: u8,
}

/// One whole file. `composite` is canvas-sized; `saved` extra composite
/// channels (each canvas-sized, `sample_bytes` wide) follow its four planes.
#[derive(Clone)]
pub struct File {
    pub version: u16,
    pub depth: u16,
    pub width: u32,
    pub height: u32,
    pub layers: Vec<Layer>,
    pub composite: Vec<[u16; 4]>,
    pub composite_word: u16,
    pub saved: Vec<Vec<u8>>,
}

impl File {
    pub fn large(&self) -> bool {
        self.version == 2
    }
    pub fn sample_bytes(&self) -> usize {
        usize::from(self.depth / 8)
    }

    fn length(&self, out: &mut Vec<u8>, len: usize) {
        if self.large() {
            out.extend_from_slice(&(len as u64).to_be_bytes());
        } else {
            out.extend_from_slice(&(len as u32).to_be_bytes());
        }
    }

    /// The layer information: count, records, channel data.
    pub fn layer_info(&self) -> Vec<u8> {
        let sb = self.sample_bytes();
        let mut info = (-(self.layers.len() as i16)).to_be_bytes().to_vec();
        let mut data = Vec::new();
        for layer in &self.layers {
            let width = (layer.bounds[3] - layer.bounds[1]) as usize;
            for edge in layer.bounds {
                info.extend_from_slice(&edge.to_be_bytes());
            }
            let channels = 4 + u16::from(layer.mask.is_some());
            info.extend_from_slice(&channels.to_be_bytes());
            for (id, component) in [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)] {
                let plane = encode_plane(
                    &plane_of(&layer.rgba, component, sb),
                    width * sb,
                    sb,
                    layer.word,
                    self.large(),
                );
                info.extend_from_slice(&id.to_be_bytes());
                self.length(&mut info, plane.len());
                data.extend_from_slice(&plane);
            }
            if let Some(mask) = &layer.mask {
                let mask_width = (mask.bounds[3] - mask.bounds[1]) as usize;
                let plane =
                    encode_plane(&mask.samples, mask_width * sb, sb, layer.word, self.large());
                info.extend_from_slice(&(-2i16).to_be_bytes());
                self.length(&mut info, plane.len());
                data.extend_from_slice(&plane);
            }
            info.extend_from_slice(b"8BIMnorm");
            info.extend_from_slice(&[255, 0, 0, 0]);
            // The mask body (its length word, then rectangle, default colour,
            // flags and two bytes of padding), then no blending ranges.
            let mut extra = Vec::new();
            match &layer.mask {
                Some(mask) => {
                    extra.extend_from_slice(&20u32.to_be_bytes());
                    for edge in mask.bounds {
                        extra.extend_from_slice(&edge.to_be_bytes());
                    }
                    extra.extend_from_slice(&[mask.outside, 0, 0, 0]);
                }
                None => extra.extend_from_slice(&[0; 4]),
            }
            extra.extend_from_slice(&[0; 4]);
            extra.push(layer.name.len() as u8);
            extra.extend_from_slice(layer.name.as_bytes());
            while !extra.len().is_multiple_of(4) {
                extra.push(0);
            }
            extra.extend_from_slice(&layer.extra_blocks);
            info.extend_from_slice(&(extra.len() as u32).to_be_bytes());
            info.extend_from_slice(&extra);
        }
        info.extend_from_slice(&data);
        if !info.len().is_multiple_of(2) {
            info.push(0);
        }
        info
    }

    /// The composite section: ONE compression word, then every channel's
    /// rows — the four picture planes and the saved channels after them.
    fn composite_section(&self) -> Vec<u8> {
        let sb = self.sample_bytes();
        let row_bytes = self.width as usize * sb;
        let mut planes: Vec<Vec<u8>> = [0usize, 1, 2, 3]
            .into_iter()
            .map(|c| plane_of(&self.composite, c, sb))
            .collect();
        planes.extend(self.saved.iter().cloned());
        let mut out = self.composite_word.to_be_bytes().to_vec();
        match self.composite_word {
            0 => planes.iter().for_each(|p| out.extend_from_slice(p)),
            1 => {
                let rows: Vec<Vec<u8>> = planes
                    .iter()
                    .flat_map(|p| p.chunks(row_bytes).map(pack_literal).collect::<Vec<_>>())
                    .collect();
                for row in &rows {
                    if self.large() {
                        out.extend_from_slice(&(row.len() as u32).to_be_bytes());
                    } else {
                        out.extend_from_slice(&(row.len() as u16).to_be_bytes());
                    }
                }
                rows.iter().for_each(|row| out.extend_from_slice(row));
            }
            word => {
                let mut all = Vec::new();
                for plane in &planes {
                    if word == 3 {
                        all.extend_from_slice(&predict(plane, row_bytes, sb));
                    } else {
                        all.extend_from_slice(plane);
                    }
                }
                out.extend_from_slice(&zlib_stored(&all));
            }
        }
        out
    }

    pub fn build(&self) -> Vec<u8> {
        let mut out = b"8BPS".to_vec();
        out.extend_from_slice(&self.version.to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        out.extend_from_slice(&((4 + self.saved.len()) as u16).to_be_bytes());
        out.extend_from_slice(&self.height.to_be_bytes());
        out.extend_from_slice(&self.width.to_be_bytes());
        out.extend_from_slice(&self.depth.to_be_bytes());
        out.extend_from_slice(&3u16.to_be_bytes());
        out.extend_from_slice(&[0; 8]); // no colour-mode data, no resources
        let info = self.layer_info();
        let mut section = Vec::new();
        self.length(&mut section, info.len());
        section.extend_from_slice(&info);
        section.extend_from_slice(&[0; 4]); // no global layer mask
        self.length(&mut out, section.len());
        out.extend_from_slice(&section);
        out.extend_from_slice(&self.composite_section());
        out
    }
}

/// The canvas of [`two_layers`].
pub const W: u32 = 9;
pub const H: u32 = 5;

/// Opaque, varied samples over a `w` x `h` rectangle; `top` scales to the
/// depth's range so 16-bit files hold values no 8-bit path could.
pub fn pattern(w: u32, h: u32, seed: u16, top: u16) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let v = |k: u32| ((u32::from(seed) + i * 37 + k * 101) % (u32::from(top) + 1)) as u16;
            [v(1), v(2), v(3), top]
        })
        .collect()
}

/// Two opaque layers: the canvas, and a 4 x 2 rectangle at (3, 1).
pub fn two_layers(version: u16, depth: u16, word: u16) -> File {
    let top = if depth == 8 { 255 } else { 65535 };
    let lower = pattern(W, H, 5, top);
    let upper = pattern(4, 2, 77, top);
    let mut composite = lower.clone();
    for y in 0..2u32 {
        for x in 0..4u32 {
            composite[((y + 1) * W + x + 3) as usize] = upper[(y * 4 + x) as usize];
        }
    }
    File {
        version,
        depth,
        width: W,
        height: H,
        layers: vec![
            Layer {
                name: "Lower".into(),
                bounds: [0, 0, H as i32, W as i32],
                rgba: lower,
                word,
                extra_blocks: Vec::new(),
                mask: None,
            },
            Layer {
                name: "Upper".into(),
                bounds: [1, 3, 3, 7],
                rgba: upper,
                word,
                extra_blocks: Vec::new(),
                mask: None,
            },
        ],
        composite,
        composite_word: word.min(1),
        saved: Vec::new(),
    }
}

// ---------------------------------------------------------------- parser --

#[derive(Debug)]
pub struct ParsedLayer {
    pub name: String,
    pub bounds: [i32; 4],
    pub blend: [u8; 4],
    pub opacity: u8,
    pub clipping: u8,
    pub flags: u8,
    /// Section divider type, when the record has an `lsct` block.
    pub divider: Option<u32>,
    /// Every additional-layer block, in file order.
    pub blocks: Vec<([u8; 4], Vec<u8>)>,
    /// Channel id and decoded row-major sample bytes.
    pub planes: Vec<(i16, Vec<u8>)>,
}

impl ParsedLayer {
    pub fn block(&self, key: &[u8; 4]) -> Option<&[u8]> {
        self.blocks
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, body)| body.as_slice())
    }

    /// The record's pixels as straight RGBA codes, or `None` for a record
    /// with no rectangle.
    pub fn rgba(&self, sample_bytes: usize) -> Option<Vec<[u16; 4]>> {
        let count =
            ((self.bounds[2] - self.bounds[0]) * (self.bounds[3] - self.bounds[1])) as usize;
        if count == 0 {
            return None;
        }
        let mut out = vec![[0u16; 4]; count];
        for (id, plane) in &self.planes {
            let component = match id {
                -1 => 3,
                0..=2 => *id as usize,
                _ => continue,
            };
            for (pixel, sample) in out.iter_mut().zip(plane.chunks_exact(sample_bytes)) {
                pixel[component] = if sample_bytes == 1 {
                    u16::from(sample[0])
                } else {
                    u16::from_be_bytes([sample[0], sample[1]])
                };
            }
        }
        Some(out)
    }
}

pub struct Parsed {
    pub version: u16,
    pub channels: u16,
    pub width: u32,
    pub height: u32,
    pub depth: u16,
    /// Image resources as (id, body).
    pub resources: Vec<(u16, Vec<u8>)>,
    pub layers: Vec<ParsedLayer>,
    /// The composite's planes, decoded, in file order.
    pub composite: Vec<Vec<u8>>,
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> &'a [u8] {
        let out = &self.bytes[self.at..self.at + count];
        self.at += count;
        out
    }
    fn u16(&mut self) -> u16 {
        u16::from_be_bytes(self.take(2).try_into().unwrap())
    }
    fn u32(&mut self) -> u32 {
        u32::from_be_bytes(self.take(4).try_into().unwrap())
    }
    fn u64(&mut self) -> u64 {
        u64::from_be_bytes(self.take(8).try_into().unwrap())
    }
    fn i32(&mut self) -> i32 {
        self.u32() as i32
    }
    fn wide(&mut self, large: bool) -> usize {
        if large {
            self.u64() as usize
        } else {
            self.u32() as usize
        }
    }
}

/// One plane's decoded bytes; `plane` starts at the compression word.
fn decode_plane(plane: &[u8], rows: usize, large: bool) -> Vec<u8> {
    match u16::from_be_bytes([plane[0], plane[1]]) {
        0 => plane[2..].to_vec(),
        1 => {
            let mut reader = Reader {
                bytes: plane,
                at: 2,
            };
            let counts: Vec<usize> = (0..rows)
                .map(|_| {
                    if large {
                        reader.u32() as usize
                    } else {
                        usize::from(reader.u16())
                    }
                })
                .collect();
            counts
                .into_iter()
                .flat_map(|count| unpack(reader.take(count)))
                .collect()
        }
        word => panic!("the exporter never writes compression word {word}"),
    }
}

/// The image-resource section as (id, body) pairs.
pub fn parse_resources(bytes: &[u8]) -> Vec<(u16, Vec<u8>)> {
    let mut reader = Reader { bytes, at: 0 };
    let mut out = Vec::new();
    while reader.at < bytes.len() {
        assert_eq!(reader.take(4), b"8BIM", "resource signature");
        let id = reader.u16();
        let name = usize::from(reader.take(1)[0]);
        reader.take(((name + 2) & !1) - 1);
        let length = reader.u32() as usize;
        out.push((id, reader.take(length).to_vec()));
        reader.at += length % 2;
    }
    out
}

/// The layer information body — the same bytes in a PSD, a PSB and a TIFF's
/// `Layr`/`Lr16` block.
pub fn parse_layer_info(info: &[u8], large: bool) -> Vec<ParsedLayer> {
    let mut reader = Reader { bytes: info, at: 0 };
    let count = (reader.u16() as i16).unsigned_abs() as usize;
    let mut layers = Vec::new();
    let mut lengths = Vec::new();
    for _ in 0..count {
        let bounds = [reader.i32(), reader.i32(), reader.i32(), reader.i32()];
        let channels = reader.u16();
        let record: Vec<(i16, usize)> = (0..channels)
            .map(|_| (reader.u16() as i16, reader.wide(large)))
            .collect();
        assert_eq!(reader.take(4), b"8BIM", "blend signature");
        let blend: [u8; 4] = reader.take(4).try_into().unwrap();
        let (opacity, clipping, flags, _) = {
            let b = reader.take(4);
            (b[0], b[1], b[2], b[3])
        };
        let extra_len = reader.u32() as usize;
        let mut extra = Reader {
            bytes: reader.take(extra_len),
            at: 0,
        };
        let mask = extra.u32() as usize;
        extra.take(mask);
        let ranges = extra.u32() as usize;
        extra.take(ranges);
        let pascal = usize::from(extra.take(1)[0]);
        let mut name = String::from_utf8_lossy(extra.take(pascal)).into_owned();
        extra.take(((pascal + 4) & !3) - 1 - pascal);
        let (mut blocks, mut divider) = (Vec::new(), None);
        while extra.at < extra.bytes.len() {
            assert_eq!(extra.take(4), b"8BIM", "additional layer signature");
            let key: [u8; 4] = extra.take(4).try_into().unwrap();
            let length = extra.u32() as usize;
            let body = extra.take(length).to_vec();
            if &key == b"luni" {
                let units: Vec<u16> = body[4..]
                    .chunks_exact(2)
                    .map(|u| u16::from_be_bytes([u[0], u[1]]))
                    .collect();
                name = String::from_utf16_lossy(&units);
            }
            if &key == b"lsct" {
                divider = Some(u32::from_be_bytes(body[..4].try_into().unwrap()));
            }
            blocks.push((key, body));
        }
        lengths.push(record);
        layers.push(ParsedLayer {
            name,
            bounds,
            blend,
            opacity,
            clipping,
            flags,
            divider,
            blocks,
            planes: Vec::new(),
        });
    }
    for (layer, record) in layers.iter_mut().zip(lengths) {
        let rows = (layer.bounds[2] - layer.bounds[0]) as usize;
        for (id, length) in record {
            let bytes = reader.take(length);
            // A mask plane has its own rectangle; these tests read none.
            if id >= -1 {
                layer.planes.push((id, decode_plane(bytes, rows, large)));
            }
        }
    }
    layers
}

/// A whole PSD or PSB the exporter wrote.
pub fn parse(bytes: &[u8]) -> Parsed {
    let mut reader = Reader { bytes, at: 0 };
    assert_eq!(reader.take(4), b"8BPS");
    let version = reader.u16();
    let large = version == 2;
    reader.take(6);
    let channels = reader.u16();
    let height = reader.u32();
    let width = reader.u32();
    let depth = reader.u16();
    assert_eq!(reader.u16(), 3, "RGB");
    let mode = reader.u32() as usize;
    reader.take(mode);
    let resources = reader.u32() as usize;
    let resources = parse_resources(reader.take(resources));
    let section = reader.wide(large);
    let section_end = reader.at + section;
    let info = reader.wide(large);
    let layers = parse_layer_info(reader.take(info), large);
    reader.at = section_end;
    let rows = height as usize;
    let row_bytes = width as usize * usize::from(depth / 8);
    let word = reader.u16();
    assert_eq!(word, 1, "the exporter writes an RLE composite");
    let counts: Vec<usize> = (0..rows * usize::from(channels))
        .map(|_| {
            if large {
                reader.u32() as usize
            } else {
                usize::from(reader.u16())
            }
        })
        .collect();
    let decoded: Vec<u8> = counts
        .into_iter()
        .flat_map(|count| unpack(reader.take(count)))
        .collect();
    assert_eq!(reader.at, bytes.len(), "nothing follows the composite");
    Parsed {
        version,
        channels,
        width,
        height,
        depth,
        resources,
        layers,
        composite: decoded
            .chunks(row_bytes * rows)
            .map(<[u8]>::to_vec)
            .collect(),
    }
}

// ------------------------------------------------------------ C plumbing --

fn json_of(pointer: *mut c_char, error: *mut c_char, what: &str) -> Value {
    assert!(!pointer.is_null(), "{what}: {}", take_err_string(error));
    serde_json::from_str(&take_err_string(pointer)).expect("JSON")
}

pub fn import_report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let json =
        unsafe { doctored_core::ffi_psd::dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    json_of(json, error, "import report")
}

pub fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} in {value}"))
        .iter()
        .map(|v| v.as_str().expect("string").to_string())
        .collect()
}

fn fingerprint(path: &Path) -> CString {
    CString::new(import_report(path)["source_fingerprint"].as_str().unwrap()).unwrap()
}

pub fn open8(path: &Path, mode: i32) -> Result<Box<DrDocument>, String> {
    let expected = fingerprint(path);
    let mut error = ptr::null_mut();
    let document = unsafe {
        doctored_core::ffi_psd::dr_doc_open_psd(
            cpath(path).as_ptr(),
            mode,
            expected.as_ptr(),
            &mut error,
        )
    };
    if document.is_null() {
        return Err(take_err_string(error));
    }
    Ok(unsafe { Box::from_raw(document) })
}

pub fn open16(path: &Path, mode: i32) -> Result<Box<PrecisionDocument>, String> {
    let expected = fingerprint(path);
    let mut error = ptr::null_mut();
    let document = unsafe {
        doctored_core::ffi_psd::dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            mode,
            expected.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null::<DrJobControl>(),
            &mut error,
        )
    };
    if document.is_null() {
        return Err(take_err_string(error));
    }
    Ok(unsafe { Box::from_raw(document) })
}

pub fn export_report8(document: &DrDocument, container: i32, mode: i32) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe {
        doctored_core::ffi_psd::dr_doc_psd_export_report(document, container, mode, &mut error)
    };
    json_of(json, error, "export report")
}

pub fn export_report16(document: &PrecisionDocument, container: i32, mode: i32) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe {
        doctored_core::ffi_psd::dr_precision_doc_psd_export_report(
            document, container, mode, &mut error,
        )
    };
    json_of(json, error, "typed export report")
}

pub fn save8(document: &DrDocument, path: &Path, container: i32, mode: i32) -> Result<(), String> {
    let mut error = ptr::null_mut();
    let ok = unsafe {
        doctored_core::ffi_psd::dr_doc_save_psd(
            document,
            ptr::null(),
            cpath(path).as_ptr(),
            container,
            mode,
            &mut error,
        )
    };
    if ok {
        Ok(())
    } else {
        Err(take_err_string(error))
    }
}

/// Renders the document's own projection and saves through the typed export.
pub fn save16(
    document: &PrecisionDocument,
    path: &Path,
    container: i32,
    mode: i32,
) -> Result<(), String> {
    let projection: PrecisionImage = document.render(REFERENCE_WORKING_BYTES, 0, None)?;
    let mut error = ptr::null_mut();
    let ok = unsafe {
        doctored_core::ffi_psd::dr_precision_doc_save_psd(
            document,
            &projection,
            cpath(path).as_ptr(),
            container,
            mode,
            REFERENCE_WORKING_BYTES,
            0,
            &mut error,
        )
    };
    if ok {
        Ok(())
    } else {
        Err(take_err_string(error))
    }
}

/// A typed document's flattened RGBA16 samples.
pub fn rendered16(document: &PrecisionDocument) -> Vec<[u16; 4]> {
    let image = document
        .render(REFERENCE_WORKING_BYTES, 0, None)
        .expect("render");
    image
        .runs_rgba16()
        .expect("RGBA16")
        .flat_map(|run| run.iter().copied())
        .collect()
}
