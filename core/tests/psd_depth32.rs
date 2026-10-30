//! 32-bit RGB PSD and PSB read and write for float documents, through
//! the C API. The source file is built here by this file's OWN writer
//! (records in an `Lr32` block, Raw and ZIP f32 planes, the ZIP-predicted
//! byte-plane form from `psd16_fixture::zip_plane32`), and every written file
//! is parsed back by this file's OWN reader; neither shares a line with the
//! core. Every float sample and u16 mask code round-trips bit for bit.
use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi::dr_image_open;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{
    dr_precision_doc_open_psd, dr_precision_doc_psd_export_report, dr_precision_doc_save_psd,
    dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::Value;
use tempfile::TempDir;

use crate::common;
use crate::psd16_fixture as fixture;
use common::{cpath, take_err_string};

const LAYERS: i32 = 0;
const FLATTENED: i32 = 1;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 6;
const H: u32 = 4;

const FLOAT_TIFF_FIRST: &str = "Only a 32-bit RGB TIFF is written for a 32-bit float document: a flat image of linear-light float samples with Photoshop's layer block beside it, layer records in Photoshop's Lr32 block, uncompressed (Raw) channels, Macintosh byte order, under the profile's linear (gamma 1.0) twin. A style or adjustment layer that 8 and 16 bits would rasterize refuses the layered mode instead, and a 32-bit document is never written at a lower depth.";
const FLOAT_PSD_FIRST: &str = "Only 32-bit RGB classic PSD is written for a 32-bit float document: linear-light float samples in the document's own primaries under its profile's linear (gamma 1.0) twin, uncompressed (Raw) channels and layer records in Photoshop's Lr32 block. A style or adjustment layer that 8 and 16 bits would rasterize refuses the layered mode instead, and a 32-bit document is never written at a lower depth.";

// ------------------------------------------------------------ C plumbing --

/// A typed document handle freed through the C API on drop.
struct Owned(*mut PrecisionDocument);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { dr_precision_doc_free(self.0) };
    }
}
impl std::ops::Deref for Owned {
    type Target = PrecisionDocument;
    fn deref(&self) -> &PrecisionDocument {
        unsafe { &*self.0 }
    }
}

fn import_report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("report JSON")
}

fn open(path: &Path, mode: i32) -> Result<Owned, String> {
    let report = import_report(path);
    let expected = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let document = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            mode,
            expected.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if document.is_null() {
        Err(take_err_string(error))
    } else {
        assert!(error.is_null());
        Ok(Owned(document))
    }
}

fn export_report(document: &PrecisionDocument, container: i32, mode: i32) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe { dr_precision_doc_psd_export_report(document, container, mode, &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("export report JSON")
}

fn save(
    document: &PrecisionDocument,
    projection: &PrecisionImage,
    path: &Path,
    container: i32,
    mode: i32,
) -> Result<(), String> {
    let mut error = ptr::null_mut();
    if unsafe {
        dr_precision_doc_save_psd(
            document,
            projection,
            cpath(path).as_ptr(),
            container,
            mode,
            LIMIT,
            0,
            &mut error,
        )
    } {
        assert!(error.is_null());
        Ok(())
    } else {
        Err(take_err_string(error))
    }
}

fn edit(document: &PrecisionDocument, request: &str) -> Owned {
    let c = CString::new(request).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            document,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    Owned(out)
}

fn add_adjustment(document: &PrecisionDocument, name: &str, description: &str) -> Owned {
    let (name, description) = (
        CString::new(name).unwrap(),
        CString::new(description).unwrap(),
    );
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            document,
            name.as_ptr(),
            description.as_ptr(),
            1.0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    Owned(out)
}

/// A layer's own f32 samples, as bit patterns.
fn bits_of(image: &PrecisionImage) -> Vec<[u32; 4]> {
    image
        .runs_rgba32()
        .expect("an f32 image")
        .flatten()
        .map(|p| p.map(f32::to_bits))
        .collect()
}

fn bits(pixels: &[[f32; 4]]) -> Vec<[u32; 4]> {
    pixels.iter().map(|p| p.map(f32::to_bits)).collect()
}

// --------------------------------------------- this file's own 32-bit writer --

fn be32(samples: &[f32]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_be_bytes()).collect()
}

fn raw32(samples: &[f32]) -> Vec<u8> {
    [vec![0, 0], be32(samples)].concat()
}

/// Channels -1, 0, 1, 2 of `rgba`: Raw, or ZIP (predicted or not).
fn planes32(rgba: &[[f32; 4]], width: usize, zip: Option<bool>) -> Vec<(i16, Vec<u8>)> {
    [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)]
        .into_iter()
        .map(|(id, c)| {
            let samples: Vec<f32> = rgba.iter().map(|p| p[c]).collect();
            (
                id,
                match zip {
                    None => raw32(&samples),
                    Some(predicted) => fixture::zip_plane32(&samples, width, predicted),
                },
            )
        })
        .collect()
}

/// A u16 coverage code as the f32 a 32-bit mask stores.
fn coverage32(code: u16) -> f32 {
    (f64::from(code) / 65535.0) as f32
}

struct Rec {
    bounds: [i32; 4],
    channels: Vec<(i16, Vec<u8>)>,
    name: &'static str,
    divider: u32,
    blend: [u8; 4],
    opacity: u8,
    clipping: u8,
    flags: u8,
    /// Rectangle, outside default, flags.
    mask: Option<([i32; 4], u8, u8)>,
    blocks: Vec<([u8; 4], Vec<u8>)>,
}

impl Rec {
    fn raster(name: &'static str, bounds: [i32; 4], channels: Vec<(i16, Vec<u8>)>) -> Self {
        Rec {
            bounds,
            channels,
            name,
            divider: 0,
            blend: *b"norm",
            opacity: 255,
            clipping: 0,
            flags: 8,
            mask: None,
            blocks: Vec::new(),
        }
    }

    fn folder(name: &'static str, divider: u32, blend: [u8; 4]) -> Self {
        let mut rec = Rec::raster(
            name,
            [0; 4],
            [-1i16, 0, 1, 2]
                .into_iter()
                .map(|id| (id, vec![0, 0]))
                .collect(),
        );
        rec.divider = divider;
        rec.blend = blend;
        rec.flags = 0x18;
        rec
    }

    fn with_mask(mut self, bounds: [i32; 4], outside: u8, enabled: bool, codes: &[u16]) -> Self {
        let samples: Vec<f32> = codes.iter().map(|c| coverage32(*c)).collect();
        self.mask = Some((bounds, outside, if enabled { 0 } else { 2 }));
        self.channels.push((-2, raw32(&samples)));
        self
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for v in self.bounds {
            out.extend(v.to_be_bytes());
        }
        out.extend((self.channels.len() as u16).to_be_bytes());
        for (id, data) in &self.channels {
            out.extend(id.to_be_bytes());
            out.extend((data.len() as u32).to_be_bytes());
        }
        out.extend(b"8BIM");
        out.extend(self.blend);
        out.extend([self.opacity, self.clipping, self.flags, 0]);
        let mut extra = Vec::new();
        match self.mask {
            Some((bounds, outside, flags)) => {
                extra.extend(20u32.to_be_bytes());
                for v in bounds {
                    extra.extend(v.to_be_bytes());
                }
                extra.extend([outside, flags, 0, 0]);
            }
            None => extra.extend(0u32.to_be_bytes()),
        }
        extra.extend(0u32.to_be_bytes());
        extra.push(self.name.len() as u8);
        extra.extend(self.name.as_bytes());
        while !extra.len().is_multiple_of(4) {
            extra.push(0);
        }
        let units: Vec<u16> = self.name.encode_utf16().collect();
        let mut luni = (units.len() as u32).to_be_bytes().to_vec();
        units.iter().for_each(|u| luni.extend(u.to_be_bytes()));
        extra.extend(fixture::block(b"luni", &luni));
        if self.divider != 0 {
            let mut body = self.divider.to_be_bytes().to_vec();
            if matches!(self.divider, 1 | 2) {
                body.extend(b"8BIM");
                body.extend(self.blend);
            }
            extra.extend(fixture::block(b"lsct", &body));
        }
        for (key, body) in &self.blocks {
            extra.extend(fixture::block(key, body));
        }
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

/// A classic 32-bit RGB PSD in Photoshop's layout: an empty
/// layer-information section, an empty global mask, the records in `Lr32`;
/// the composite Raw, matted white `c·a + (1 − a)`, merged alpha declared.
fn file32(records: &[Rec], icc: &[u8], composite: &[[f32; 4]]) -> Vec<u8> {
    let mut body = (-(records.len() as i16)).to_be_bytes().to_vec();
    records.iter().for_each(|r| body.extend(r.encode()));
    for record in records {
        for (_, data) in &record.channels {
            body.extend(data);
        }
    }
    let mut section = 0u32.to_be_bytes().to_vec();
    section.extend(0u32.to_be_bytes());
    section.extend(fixture::block(b"Lr32", &body));
    while !section.len().is_multiple_of(4) {
        section.push(0);
    }
    let mut resources = resource(1039, icc);
    resources.extend(resource(1005, &fixture::resolution_info(300.0, 240.0)));
    let mut bytes = b"8BPS\0\x01\0\0\0\0\0\0".to_vec();
    bytes.extend(4u16.to_be_bytes());
    bytes.extend(H.to_be_bytes());
    bytes.extend(W.to_be_bytes());
    bytes.extend(32u16.to_be_bytes());
    bytes.extend(3u16.to_be_bytes());
    bytes.extend(0u32.to_be_bytes());
    bytes.extend((resources.len() as u32).to_be_bytes());
    bytes.extend(resources);
    bytes.extend((section.len() as u32).to_be_bytes());
    bytes.extend(section);
    bytes.extend(0u16.to_be_bytes());
    for component in 0..4 {
        let plane: Vec<f32> = composite
            .iter()
            .map(|p| {
                if component == 3 {
                    p[3]
                } else {
                    (f64::from(p[component]) * f64::from(p[3]) + 1.0 - f64::from(p[3])) as f32
                }
            })
            .collect();
        bytes.extend(be32(&plane));
    }
    bytes
}

// --------------------------------------------- this file's own 32-bit reader --

struct Read32Record {
    bounds: [i32; 4],
    name: String,
    blend: [u8; 4],
    opacity: u8,
    clipping: u8,
    flags: u8,
    mask: Option<([i32; 4], u8, u8)>,
    divider: Option<u32>,
    blocks: Vec<([u8; 4], Vec<u8>)>,
    /// id, compression word, samples.
    channels: Vec<(i16, u16, Vec<f32>)>,
}

impl Read32Record {
    fn plane(&self, id: i16) -> &[f32] {
        &self.channels.iter().find(|c| c.0 == id).expect("channel").2
    }
    fn rgba(&self) -> Vec<[f32; 4]> {
        let (a, r, g, b) = (self.plane(-1), self.plane(0), self.plane(1), self.plane(2));
        (0..a.len()).map(|i| [r[i], g[i], b[i], a[i]]).collect()
    }
}

struct Read32 {
    version: u16,
    depth: u16,
    channels: u16,
    /// The color-mode data section.
    color_mode: Vec<u8>,
    icc: Vec<u8>,
    info_len: u64,
    /// Every tail block before `Lr32`, and `Lr32` itself: key and length.
    tail_keys: Vec<([u8; 4], u64)>,
    merged_alpha: bool,
    negative_count: bool,
    records: Vec<Read32Record>,
    composite_word: u16,
    composite: Vec<[f32; 4]>,
}

fn be_u16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes(b[at..at + 2].try_into().unwrap())
}
fn be_u32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(b[at..at + 4].try_into().unwrap())
}
fn be_u64(b: &[u8], at: usize) -> u64 {
    u64::from_be_bytes(b[at..at + 8].try_into().unwrap())
}
fn floats(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|s| f32::from_be_bytes(s.try_into().unwrap()))
        .collect()
}

fn read32(bytes: &[u8]) -> Read32 {
    assert_eq!(&bytes[..4], b"8BPS");
    let version = be_u16(bytes, 4);
    let large = version == 2;
    let wide = |b: &[u8], at: usize| -> (u64, usize) {
        if large {
            (be_u64(b, at), 8)
        } else {
            (u64::from(be_u32(b, at)), 4)
        }
    };
    let channels = be_u16(bytes, 12);
    let (height, width) = (be_u32(bytes, 14) as usize, be_u32(bytes, 18) as usize);
    let depth = be_u16(bytes, 22);
    let color_mode = bytes[30..30 + be_u32(bytes, 26) as usize].to_vec();
    let mut at = 26 + 4 + be_u32(bytes, 26) as usize;
    let resources_len = be_u32(bytes, at) as usize;
    let resources = &bytes[at + 4..at + 4 + resources_len];
    at += 4 + resources_len;
    let mut icc = Vec::new();
    let mut r = 0usize;
    while r < resources.len() {
        let id = be_u16(resources, r + 4);
        r += 6;
        r += (resources[r] as usize + 2) & !1;
        let len = be_u32(resources, r) as usize;
        if id == 1039 {
            icc = resources[r + 4..r + 4 + len].to_vec();
        }
        r += 4 + len + len % 2;
    }
    let (section_len, n) = wide(bytes, at);
    let section = &bytes[at + n..at + n + section_len as usize];
    at += n + section_len as usize;
    let (info_len, n) = wide(section, 0);
    let mut s = n + info_len as usize;
    s += 4 + be_u32(section, s) as usize; // global layer mask
    let mut tail_keys = Vec::new();
    let mut info: &[u8] = &[];
    while s + 12 <= section.len() {
        let key: [u8; 4] = section[s + 4..s + 8].try_into().unwrap();
        // Adobe's u64-in-a-PSB keys this file meets.
        let (len, n) = if matches!(&key, b"Lr32" | b"Mt32") {
            wide(section, s + 8)
        } else {
            (u64::from(be_u32(section, s + 8)), 4)
        };
        let body = &section[s + 8 + n..s + 8 + n + len as usize];
        tail_keys.push((key, len));
        if &key == b"Lr32" {
            info = body;
            break;
        }
        s += 8 + n + len as usize + ((len as usize).wrapping_neg() & 3);
    }
    let declared = i16::from_be_bytes(info[..2].try_into().unwrap());
    let negative_count = declared < 0;
    let mut i = 2usize;
    let mut records = Vec::new();
    let mut shapes = Vec::new();
    for _ in 0..declared.unsigned_abs() {
        let bounds = std::array::from_fn(|k| be_u32(info, i + 4 * k) as i32);
        i += 16;
        let count = be_u16(info, i) as usize;
        i += 2;
        let mut shape = Vec::new();
        for _ in 0..count {
            let id = be_u16(info, i) as i16;
            let (len, n) = wide(info, i + 2);
            shape.push((id, len as usize));
            i += 2 + n;
        }
        assert_eq!(&info[i..i + 4], b"8BIM");
        let blend = info[i + 4..i + 8].try_into().unwrap();
        let (opacity, clipping, flags) = (info[i + 8], info[i + 9], info[i + 10]);
        i += 12;
        let extra_len = be_u32(info, i) as usize;
        let extra = &info[i + 4..i + 4 + extra_len];
        i += 4 + extra_len;
        let mask_len = be_u32(extra, 0) as usize;
        let mask = (mask_len >= 20).then(|| {
            (
                std::array::from_fn(|k| be_u32(extra, 4 + 4 * k) as i32),
                extra[20],
                extra[21],
            )
        });
        let mut c = 4 + mask_len;
        c += 4 + be_u32(extra, c) as usize;
        c += 1 + extra[c] as usize;
        while !c.is_multiple_of(4) {
            c += 1;
        }
        let (mut name, mut divider, mut blocks) = (String::new(), None, Vec::new());
        while c + 12 <= extra.len() {
            assert_eq!(&extra[c..c + 4], b"8BIM");
            let key: [u8; 4] = extra[c + 4..c + 8].try_into().unwrap();
            let len = be_u32(extra, c + 8) as usize;
            let body = extra[c + 12..c + 12 + len].to_vec();
            c += 12 + len + len % 2;
            match &key {
                b"luni" => {
                    let units: Vec<u16> = body[4..].chunks_exact(2).map(|u| be_u16(u, 0)).collect();
                    name = String::from_utf16(&units).unwrap();
                }
                b"lsct" => divider = Some(be_u32(&body, 0)),
                _ => {}
            }
            blocks.push((key, body));
        }
        shapes.push(shape);
        records.push(Read32Record {
            bounds,
            name,
            blend,
            opacity,
            clipping,
            flags,
            mask,
            divider,
            blocks,
            channels: Vec::new(),
        });
    }
    for (record, shape) in records.iter_mut().zip(shapes) {
        for (id, len) in shape {
            let data = &info[i..i + len];
            i += len;
            let word = be_u16(data, 0);
            assert_eq!(word, 0, "every 32-bit plane is written Raw");
            record.channels.push((id, word, floats(&data[2..])));
        }
    }
    let composite_word = be_u16(bytes, at);
    let pixels = width * height;
    let planes: Vec<Vec<f32>> = (0..usize::from(channels))
        .map(|k| floats(&bytes[at + 2 + k * pixels * 4..at + 2 + (k + 1) * pixels * 4]))
        .collect();
    assert_eq!(at + 2 + usize::from(channels) * pixels * 4, bytes.len());
    let composite = (0..pixels)
        .map(|p| [planes[0][p], planes[1][p], planes[2][p], planes[3][p]])
        .collect();
    Read32 {
        version,
        depth,
        channels,
        color_mode,
        icc,
        info_len,
        merged_alpha: declared < 0 || tail_keys.iter().any(|(k, l)| k == b"Mt32" && *l == 0),
        tail_keys,
        negative_count,
        records,
        composite_word,
        composite,
    }
}

// --------------------------------------------------------------- the scene --

const VALUES: [f32; 8] = [0.0, 1e-6, 0.18, 4.0, 65504.0, -0.25, 0.5, 2.5];

struct Scene {
    bytes: Vec<u8>,
    base: Vec<[f32; 4]>,
    masked: Vec<[f32; 4]>,
    masked_mask: Vec<u16>,
    inside: Vec<[f32; 4]>,
    clip: Vec<[f32; 4]>,
    iso_mask: Vec<u16>,
    deep: Vec<[f32; 4]>,
    composite: Vec<[f32; 4]>,
}

fn ramp(count: usize, seed: usize, alphas: &[f32]) -> Vec<[f32; 4]> {
    (0..count)
        .map(|i| {
            [
                VALUES[(i + seed) % 8],
                VALUES[(i + seed + 3) % 8],
                VALUES[(i + seed + 5) % 8],
                alphas[i % alphas.len()],
            ]
        })
        .collect()
}

/// Bottom first: Base (Raw, opaque); Masked (ZIP predicted, signed offset,
/// partial and zero alpha keeping RGB, 50 % Multiply, hidden, label 3,
/// position lock, an f32 mask); folder Iso (Normal, 200, a disabled group
/// mask outside 0) holding Inside (ZIP) and Clip (clipped, Raw); a closed
/// Pass Through folder Pass holding Deep.
fn scene() -> Scene {
    let base = ramp(24, 0, &[1.0]);
    let masked = ramp(9, 1, &[0.0, 0.25, 0.5, 1.0, 0.75, 1e-6]);
    let masked_mask = vec![0u16, 1, 65535, 32768, 12345, 65534, 2, 40000, 7];
    let inside = ramp(8, 2, &[1.0, 0.5]);
    let clip = ramp(4, 3, &[1.0]);
    let iso_mask: Vec<u16> = (0..24u16).map(|i| i.wrapping_mul(2731)).collect();
    let deep = ramp(6, 4, &[1.0, 0.125]);
    let composite = ramp(24, 5, &[0.0, 0.25, 1.0, 0.5])
        .into_iter()
        .map(|p| [p[0].clamp(0.0, 8.0), p[1].clamp(0.0, 8.0), p[2], p[3]])
        .collect::<Vec<_>>();

    let mut masked_rec = Rec::raster("Masked", [-1, 2, 2, 5], planes32(&masked, 3, Some(true)))
        .with_mask([0, 1, 3, 4], 255, true, &masked_mask);
    masked_rec.opacity = 128;
    masked_rec.blend = *b"mul ";
    masked_rec.flags = 8 | 2;
    masked_rec.blocks = vec![
        (*b"lspf", 4u32.to_be_bytes().to_vec()),
        (*b"lclr", vec![0, 3, 0, 0, 0, 0, 0, 0]),
    ];
    let mut clip_rec = Rec::raster("Clip", [0, 0, 2, 2], planes32(&clip, 2, None));
    clip_rec.clipping = 1;
    let mut iso = Rec::folder("Iso", 1, *b"norm").with_mask([0, 0, 4, 6], 0, false, &iso_mask);
    iso.opacity = 200;
    let records = vec![
        Rec::raster("Base", [0, 0, 4, 6], planes32(&base, 6, None)),
        masked_rec,
        Rec::folder("</Layer group>", 3, *b"norm"),
        Rec::raster("Inside", [1, 1, 3, 5], planes32(&inside, 4, Some(false))),
        clip_rec,
        iso,
        Rec::folder("</Layer group>", 3, *b"norm"),
        Rec::raster("Deep", [2, 3, 4, 6], planes32(&deep, 3, None)),
        Rec::folder("Pass", 2, *b"pass"),
    ];
    Scene {
        bytes: file32(&records, IccProfile::display_p3().bytes(), &composite),
        base,
        masked,
        masked_mask,
        inside,
        clip,
        iso_mask,
        deep,
        composite,
    }
}

fn write(directory: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = directory.path().join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

/// Every declared fact of the scene, on an opened or reopened document whose
/// first seven records (postorder) are the scene's.
fn assert_scene(document: &PrecisionDocument, scene: &Scene, profile: &[u8], label: &str) {
    assert!(document.is_float(), "{label}");
    assert_eq!(document.dimensions(), (W, H), "{label}");
    assert_eq!(&document.profile().bytes()[..], profile, "{label}");
    let layer = |i: usize| document.layer(i).expect("layer");
    let names: Vec<&str> = (0..7).map(|i| layer(i).name()).collect();
    assert_eq!(
        names,
        ["Base", "Masked", "Inside", "Clip", "Iso", "Deep", "Pass"],
        "{label}"
    );
    for (index, want) in [
        (0usize, &scene.base),
        (1, &scene.masked),
        (2, &scene.inside),
        (3, &scene.clip),
        (5, &scene.deep),
    ] {
        assert_eq!(
            bits_of(layer(index).source().expect("raster")),
            bits(want),
            "{label}: record {index} bit for bit"
        );
    }
    let masked = layer(1);
    assert_eq!(masked.offset(), (2, -1), "{label}");
    assert_eq!(masked.opacity(), 128.0 / 255.0, "{label}");
    assert!(!masked.visible(), "{label}");
    assert_eq!(masked.blend_mode(), BlendMode::Multiply, "{label}");
    assert_eq!(masked.label(), 3, "{label}");
    assert_eq!(masked.locks(), 4, "{label}: the position lock");
    let mask = masked.mask().expect("mask");
    assert_eq!(mask.plane().samples(), &scene.masked_mask[..], "{label}");
    assert_eq!(mask.properties().offset, (1, 0), "{label}");
    assert_eq!(mask.properties().outside_coverage, 255, "{label}");
    assert!(mask.enabled(), "{label}");
    assert_eq!(layer(2).offset(), (1, 1), "{label}");
    assert!(layer(3).clipped(), "{label}");
    let iso = layer(4);
    assert!(
        iso.children().is_some() && !iso.is_pass_through(),
        "{label}"
    );
    assert_eq!(iso.opacity(), 200.0 / 255.0, "{label}");
    let group_mask = iso.mask().expect("group mask");
    assert_eq!(group_mask.plane().samples(), &scene.iso_mask[..], "{label}");
    assert_eq!(group_mask.properties().outside_coverage, 0, "{label}");
    assert!(!group_mask.enabled(), "{label}");
    assert_eq!(layer(5).offset(), (3, 2), "{label}");
    let pass = layer(6);
    assert!(pass.is_pass_through(), "{label}");
    assert!(!pass.open(), "{label}: the folder is closed");
}

// ------------------------------------------- the profile and the color mode --

/// An ICC profile's tags in table order: signature and data.
fn icc_tags(icc: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    (0..be_u32(icc, 128) as usize)
        .map(|k| {
            let entry = 132 + 12 * k;
            let (offset, size) = (
                be_u32(icc, entry + 4) as usize,
                be_u32(icc, entry + 8) as usize,
            );
            (
                icc[entry..entry + 4].try_into().unwrap(),
                icc[offset..offset + size].to_vec(),
            )
        })
        .collect()
}

fn icc_tag<'a>(tags: &'a [([u8; 4], Vec<u8>)], sig: &[u8; 4]) -> Option<&'a [u8]> {
    tags.iter()
        .find(|(s, _)| s == sig)
        .map(|(_, d)| d.as_slice())
}

/// The ASCII string of a v2 `desc` tag.
fn desc_text(data: &[u8]) -> String {
    assert_eq!(&data[..4], b"desc");
    let count = be_u32(data, 8) as usize;
    String::from_utf8(data[12..12 + count - 1].to_vec()).unwrap()
}

/// Photoshop's take1 file, whose color mode data and profile are the oracle
/// of Photoshop's 32-bit convention.
fn photoshop_file() -> Read32Header {
    let bytes = std::fs::read(crate::common::psd_photoshop::fixture(
        "depth32",
        "source32.psd",
    ))
    .expect("the take1 fixture");
    read_header(&bytes)
}

fn photoshop_color_mode() -> Vec<u8> {
    photoshop_file().color_mode
}

struct Read32Header {
    color_mode: Vec<u8>,
    icc: Vec<u8>,
}

/// The color-mode data and resource 1039 of any PSD (Photoshop's fixture is
/// ZIP-compressed, which `read32` does not read).
fn read_header(bytes: &[u8]) -> Read32Header {
    let color_mode = bytes[30..30 + be_u32(bytes, 26) as usize].to_vec();
    let at = 30 + color_mode.len();
    let resources = &bytes[at + 4..at + 4 + be_u32(bytes, at) as usize];
    let mut icc = Vec::new();
    let mut r = 0usize;
    while r < resources.len() {
        let id = be_u16(resources, r + 4);
        r += 6;
        r += (resources[r] as usize + 2) & !1;
        let len = be_u32(resources, r) as usize;
        if id == 1039 {
            icc = resources[r + 4..r + 4 + len].to_vec();
        }
        r += 4 + len + len % 2;
    }
    Read32Header { color_mode, icc }
}

/// `twin` is `source`'s linear twin as `icc_builtin::linear_twin` builds it,
/// Photoshop's tag order: `cprt`, `desc` ("<name> (Linear RGB Profile)"), the
/// three colorant tags and `wtpt` as the source's own bytes, three TRC tags
/// that are one `curv` gamma of exactly 1.0, then the source's `chad` if it
/// has one.
fn assert_linear_twin(twin: &[u8], source: &[u8], name: &str) {
    let (tags, from) = (icc_tags(twin), icc_tags(source));
    let mut sigs = vec![
        *b"cprt", *b"desc", *b"rXYZ", *b"gXYZ", *b"bXYZ", *b"wtpt", *b"rTRC", *b"gTRC", *b"bTRC",
    ];
    if icc_tag(&from, b"chad").is_some() {
        sigs.push(*b"chad");
    }
    assert_eq!(tags.iter().map(|(s, _)| *s).collect::<Vec<_>>(), sigs);
    for sig in [b"rXYZ", b"gXYZ", b"bXYZ", b"wtpt", b"chad"] {
        assert_eq!(
            icc_tag(&tags, sig),
            icc_tag(&from, sig),
            "{}",
            sig.escape_ascii()
        );
    }
    for sig in [b"rTRC", b"gTRC", b"bTRC"] {
        assert_eq!(
            icc_tag(&tags, sig).unwrap(),
            b"curv\0\0\0\0\0\0\0\x01\x01\0",
            "a gamma of 1.0"
        );
    }
    let desc = desc_text(icc_tag(&tags, b"desc").unwrap());
    assert_eq!(desc, format!("{name} (Linear RGB Profile)"));
    let parsed = IccProfile::parse(twin).expect("the twin parses");
    assert_eq!(parsed.name(), desc);
    assert!(parsed.model().is_some(), "a matrix/TRC profile");
}

// ------------------------------------------------------------------ tests --

#[test]
fn a_thirty_two_bit_file_reports_rgba32_and_opens_as_a_float_document() {
    let directory = TempDir::new().unwrap();
    let scene = scene();
    let path = write(&directory, "scene32.psd", &scene.bytes);
    let report = import_report(&path);
    assert_eq!(report["depth"], 32);
    assert_eq!(report["import_precision"], "rgba32");
    assert_eq!(report["layered_available"], true, "{report}");
    assert_eq!(report["stored_composite_available"], true, "{report}");
    assert!(report["observations"]
        .to_string()
        .contains("This file opens as a 32-bit float document, including through Import Layers."));
    let document = open(&path, LAYERS).expect("layered open");
    assert_scene(&document, &scene, IccProfile::display_p3().bytes(), "open");

    // The stored composite, un-matted: alpha exact, opaque colour exact,
    // zero alpha the canonical 0, the rest within float rounding.
    let flat = open(&path, FLATTENED).expect("composite open");
    assert!(flat.is_float());
    let got: Vec<[f32; 4]> = flat
        .layer(0)
        .unwrap()
        .source()
        .unwrap()
        .runs_rgba32()
        .unwrap()
        .flatten()
        .copied()
        .collect();
    for (g, want) in got.iter().zip(&scene.composite) {
        assert_eq!(g[3].to_bits(), want[3].to_bits());
        for c in 0..3 {
            match want[3] {
                0.0 => assert_eq!(g[c], 0.0),
                1.0 => assert_eq!(g[c].to_bits(), want[c].to_bits()),
                _ => assert!(
                    (g[c] - want[c]).abs() <= 1e-5 * want[c].abs().max(1.0),
                    "{g:?} vs {want:?}"
                ),
            }
        }
    }

    // The byte open refuses by name, and so does the flat image open.
    let mut error = ptr::null_mut();
    let image = unsafe { dr_image_open(cpath(&path).as_ptr(), &mut error) };
    assert!(image.is_null());
    assert!(
        take_err_string(error).contains(
            "32-bit PSD has no 8-bit composite; only 8-bit RGB and grayscale composites are decoded here, a 16-bit PSD opens only as a 16-bit document, and a 32-bit RGB PSD opens only as a 32-bit float document"
        )
    );
}

#[test]
fn a_float_document_round_trips_through_its_own_psd_and_psb_bit_for_bit() {
    let directory = TempDir::new().unwrap();
    let scene = scene();
    let source = open(&write(&directory, "scene32.psd", &scene.bytes), LAYERS).unwrap();
    let levels =
        r#"{"type":"adjust","evaluation":"precision_v1","op":"levels","params":{"gamma":1.4}}"#;
    let document = add_adjustment(&source, "Levels", levels);
    let projection = document.render(LIMIT, 0, None).expect("projection");
    assert_eq!(
        projection.storage(),
        doctored_core::precision_image::PrecisionStorage::Rgba32Float
    );

    for (container, name, version) in [(0, "rt.psd", 1u16), (1, "rt.psb", 2)] {
        let report = export_report(&document, container, LAYERS);
        assert_eq!(report["depth"], 32);
        assert_eq!(report["available"], true, "{report}");
        if container == 0 {
            assert_eq!(report["general_limitations"][0], FLOAT_PSD_FIRST);
        }
        let path = directory.path().join(name);
        save(&document, &projection, &path, container, LAYERS).expect("layered save");
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.len() as u64 <= report["worst_case_bytes"].as_u64().unwrap());

        // The bytes, through this file's own reader.
        let file = read32(&bytes);
        assert_eq!(file.version, version);
        assert_eq!((file.depth, file.channels), (32, 4));
        // Photoshop's 32-bit convention (take2): the profile's linear
        // twin, and Photoshop's own color-mode data.
        assert_linear_twin(&file.icc, IccProfile::display_p3().bytes(), "Display P3");
        assert_eq!(file.color_mode, photoshop_color_mode());
        assert_eq!(file.info_len, 0, "the layer-information section is empty");
        // Photoshop's own form (take1): an empty `Mt32`, then `Lr32`
        // with a positive count.
        let keys: Vec<_> = file.tail_keys.iter().map(|(k, l)| (*k, *l > 0)).collect();
        assert_eq!(keys, [(*b"Mt32", false), (*b"Lr32", true)]);
        assert!(file.merged_alpha);
        assert!(!file.negative_count, "the count is positive");
        assert_eq!(file.composite_word, 0, "the composite is Raw");
        let names: Vec<&str> = file.records.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Base",
                "Masked",
                "</Layer group>",
                "Inside",
                "Clip",
                "Iso",
                "</Layer group>",
                "Deep",
                "Pass",
                "Levels"
            ]
        );
        let by = |n: &str| file.records.iter().find(|r| r.name == n).unwrap();
        assert_eq!(bits(&by("Base").rgba()), bits(&scene.base));
        let masked = by("Masked");
        assert_eq!(bits(&masked.rgba()), bits(&scene.masked));
        assert_eq!(masked.bounds, [-1, 2, 2, 5]);
        assert_eq!((masked.opacity, masked.blend), (128, *b"mul "));
        assert_eq!(masked.flags & 2, 2);
        assert_eq!(masked.mask, Some(([0, 1, 3, 4], 255, 0)));
        let codes: Vec<u16> = masked
            .plane(-2)
            .iter()
            .map(|v| (f64::from(*v) * 65535.0).round() as u16)
            .collect();
        assert_eq!(codes, scene.masked_mask);
        assert!(masked.blocks.iter().any(|(k, b)| k == b"lclr" && b[1] == 3));
        assert!(masked
            .blocks
            .iter()
            .any(|(k, b)| k == b"lspf" && b == &4u32.to_be_bytes()));
        assert_eq!(by("Clip").clipping, 1);
        assert_eq!(by("Iso").divider, Some(1));
        assert_eq!(by("Iso").mask, Some(([0, 0, 4, 6], 0, 2)));
        assert_eq!(by("Pass").divider, Some(2));
        assert_eq!(by("Pass").blend, *b"pass");
        assert!(by("Levels").blocks.iter().any(|(k, _)| k == b"levl"));
        // The composite: the projection matted over white, alpha straight.
        for (stored, p) in file
            .composite
            .iter()
            .zip(projection.runs_rgba32().unwrap().flatten())
        {
            assert_eq!(stored[3].to_bits(), p[3].to_bits());
            for c in 0..3 {
                let want = (f64::from(p[c]) * f64::from(p[3]) + (1.0 - f64::from(p[3]))) as f32;
                assert_eq!(stored[c].to_bits(), want.to_bits());
            }
        }

        // …and back through the float importer.
        // The twin reopens as the Display P3 it was made from.
        let back = open(&path, LAYERS).expect("reopen");
        assert_scene(&back, &scene, IccProfile::display_p3().bytes(), name);
        let levels = back.adjustment_description(7).expect("the Levels record");
        assert!(levels.contains("levels"), "{levels}");

        // The reopened Display P3 resaves the same twin, byte for byte.
        let again = directory.path().join(format!("again-{name}"));
        let projection = back.render(LIMIT, 0, None).expect("projection");
        save(&back, &projection, &again, container, LAYERS).expect("resave");
        assert_eq!(read32(&std::fs::read(&again).unwrap()).icc, file.icc);
    }
}

#[test]
fn the_flattened_mode_writes_exactly_the_supplied_projection() {
    let directory = TempDir::new().unwrap();
    let scene = scene();
    let document = open(&write(&directory, "scene32.psd", &scene.bytes), LAYERS).unwrap();
    let projection = document.render(LIMIT, 0, None).unwrap();
    let path = directory.path().join("flat.psd");
    save(&document, &projection, &path, 0, FLATTENED).expect("flattened save");
    let file = read32(&std::fs::read(&path).unwrap());
    assert_eq!(file.records.len(), 1);
    assert_eq!(file.records[0].name, "Composite");
    assert_eq!(bits(&file.records[0].rgba()), bits_of(&projection));
}

#[test]
fn every_thirty_two_bit_refusal_names_itself_and_keeps_the_destination() {
    let directory = TempDir::new().unwrap();
    let scene = scene();
    let source = open(&write(&directory, "scene32.psd", &scene.bytes), LAYERS).unwrap();
    let path = directory.path().join("kept.psd");
    let sentinel = b"an existing file".to_vec();

    let sepia = add_adjustment(
        &source,
        "Sepia",
        r#"{"type":"adjust","evaluation":"precision_v1","op":"sepia","params":{}}"#,
    );
    let styled = edit(
        &source,
        r#"{"op":"layer_style","version":1,"layer":1,"style":{"fill_opacity":0.5}}"#,
    );
    let styled = edit(
        &styled,
        r#"{"op":"mask_properties","layer":1,"x":1,"y":0,"linked":false,"density":0.5,"feather":0.0,"enabled":true}"#,
    );
    let cases: Vec<(&PrecisionDocument, i32, &str)> = vec![
        (
            &sepia,
            0,
            "the sepia adjustment layer “Sepia” has no Photoshop record and is not baked at 32 bits; export Flattened",
        ),
        (
            &styled,
            0,
            "the layer style on “Masked” would have to be rasterized, which Doctored does not do at 32 bits; export Flattened",
        ),
    ];
    for (document, container, sentence) in cases {
        std::fs::write(&path, &sentinel).unwrap();
        let projection = document.render(LIMIT, 0, None).unwrap();
        let report = export_report(document, container, LAYERS);
        assert_eq!(report["refusal"], sentence, "{report}");
        assert_eq!(report["depth"], 32);
        let error = save(document, &projection, &path, container, LAYERS).expect_err("refused");
        assert_eq!(error, sentence);
        assert_eq!(std::fs::read(&path).unwrap(), sentinel);
        if container == 0 {
            // The flattened mode always writes.
            assert_eq!(export_report(document, 0, FLATTENED)["available"], true);
        }
    }
    // The layered TIFF is written (`tiff_layers32.rs`), and refuses
    // what the PSD refuses.
    let tiff = export_report(&source, 2, FLATTENED);
    assert_eq!(tiff["refusal"], Value::Null, "{tiff}");
    assert_eq!(tiff["general_limitations"][0], FLOAT_TIFF_FIRST);
    assert_eq!(
        export_report(&styled, 2, LAYERS)["refusal"],
        "the layer style on “Masked” would have to be rasterized, which Doctored does not do at 32 bits; export Flattened"
    );
}

#[test]
fn photoshops_predicted_zip_form_decodes_from_an_independent_encoder() {
    let directory = TempDir::new().unwrap();
    // A row that crosses every byte boundary: negative, denormal, huge.
    let pixels: Vec<[f32; 4]> = (0..24)
        .map(|i| {
            let v = [
                -3.5e-39f32,
                f32::MIN_POSITIVE,
                1.0,
                65504.0,
                -0.0,
                0.333_333_34,
            ][i % 6];
            [v, -v, v * 0.5, [1.0, 0.0, 0.5][i % 3]]
        })
        .collect();
    let mask: Vec<u16> = (0..24u16).map(|i| i * 2700).collect();
    let mask_plane: Vec<f32> = mask.iter().map(|c| coverage32(*c)).collect();
    let mut rec = Rec::raster("Zip", [0, 0, 4, 6], planes32(&pixels, 6, Some(true)));
    rec.mask = Some(([0, 0, 4, 6], 0, 0));
    rec.channels
        .push((-2, fixture::zip_plane32(&mask_plane, 6, true)));
    let bytes = file32(
        &[rec],
        IccProfile::srgb().bytes(),
        &[[0.0, 0.0, 0.0, 1.0]; 24],
    );
    let document = open(&write(&directory, "zip.psd", &bytes), LAYERS).expect("open");
    let layer = document.layer(0).unwrap();
    assert_eq!(bits_of(layer.source().unwrap()), bits(&pixels));
    assert_eq!(layer.mask().unwrap().plane().samples(), &mask[..]);
}

#[test]
fn a_profile_with_no_matrix_model_refuses_both_choices_by_name() {
    use crate::icc_lut_builders::{build, tag_desc, tag_mft2, tag_wtpt};
    let lut = build(
        [0x02, 0x20, 0, 0],
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("Synthetic LUT RGB")),
            (b"wtpt", tag_wtpt([0.9642, 1.0, 0.8249])),
            (b"A2B0", tag_mft2(3, 3, 2, &|c| c.to_vec())),
            (b"B2A0", tag_mft2(3, 3, 2, &|c| c.to_vec())),
        ],
    );
    assert!(IccProfile::parse(&lut).unwrap().model().is_none());
    let directory = TempDir::new().unwrap();
    let rec = Rec::raster(
        "Base",
        [0, 0, 4, 6],
        planes32(&ramp(24, 0, &[1.0]), 6, None),
    );
    let path = write(
        &directory,
        "lut.psd",
        &file32(&[rec], &lut, &[[0.5, 0.5, 0.5, 1.0]; 24]),
    );
    let sentence = "a 32-bit float document requires an RGB matrix/TRC profile";
    let report = import_report(&path);
    assert_eq!(report["layered_refusal"], sentence);
    assert_eq!(report["stored_composite_refusal"], sentence);
    for mode in [LAYERS, FLATTENED] {
        let error = open(&path, mode).err().expect("refused");
        assert!(error.ends_with(sentence), "{error}");
    }
}

/// A 16- or 32-bit file that is not RGB (here the scene relabelled
/// Grayscale) reports neither typed RGB precision, so the host never says it
/// opens as a 16-bit or float RGB document: it is a Gray file.
#[test]
fn a_non_rgb_deep_file_reports_no_typed_precision() {
    let directory = TempDir::new().unwrap();
    for depth in [16u16, 32] {
        let mut bytes = scene().bytes;
        bytes[22..24].copy_from_slice(&depth.to_be_bytes());
        bytes[24..26].copy_from_slice(&1u16.to_be_bytes());
        let path = write(&directory, &format!("gray{depth}.psd"), &bytes);
        let report = import_report(&path);
        assert_eq!(report["color_mode"], "Grayscale", "{report}");
        // #1 phases 3a and 3c: a 16- or 32-bit gray file opens as a
        // Grayscale document (its precision "gray").
        assert_eq!(report["import_precision"], "gray", "{report}");
    }
}

#[test]
fn the_linear_twin_matches_photoshops_own_tag_for_tag() {
    // Photoshop's own twin, as take1's file embeds it, of the profile the
    // take's scene was built in.
    let srgb = Path::new("/System/Library/ColorSync/Profiles/sRGB Profile.icc");
    let Ok(source) = std::fs::read(srgb) else {
        eprintln!("skipped: {} is absent", srgb.display());
        return;
    };
    let photoshop = photoshop_file().icc;
    let directory = TempDir::new().unwrap();
    let base = ramp(24, 0, &[1.0]);
    let bytes = file32(
        &[Rec::raster(
            "Base",
            [0, 0, H as i32, W as i32],
            planes32(&base, 6, None),
        )],
        &source,
        &base,
    );
    let document = open(&write(&directory, "srgb32.psd", &bytes), LAYERS).unwrap();
    assert_eq!(&document.profile().bytes()[..], &source[..]);
    let projection = document.render(LIMIT, 0, None).unwrap();
    let path = directory.path().join("twin.psd");
    save(&document, &projection, &path, 0, LAYERS).expect("save");
    let ours = read32(&std::fs::read(&path).unwrap()).icc;
    assert_linear_twin(&ours, &source, "sRGB IEC61966-2.1");

    // Against Photoshop's: the same tags in the same order, the same name
    // and the same gamma-1.0 curves byte for byte; the colorants and white
    // within 12/65536 (Photoshop recomputes them: macOS sRGB's bXYZ Z moved
    // 0xB6CF → 0xB6C3); the copyright is the source's, not Adobe's.
    let (ours, theirs) = (icc_tags(&ours), icc_tags(&photoshop));
    assert_eq!(
        ours.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
        theirs.iter().map(|(s, _)| *s).collect::<Vec<_>>()
    );
    for sig in [b"desc", b"rTRC", b"gTRC", b"bTRC"] {
        assert_eq!(
            icc_tag(&ours, sig),
            icc_tag(&theirs, sig),
            "{}",
            sig.escape_ascii()
        );
    }
    for sig in [b"rXYZ", b"gXYZ", b"bXYZ", b"wtpt"] {
        let (a, b) = (icc_tag(&ours, sig).unwrap(), icc_tag(&theirs, sig).unwrap());
        assert_eq!((&a[..4], a.len()), (&b[..4], b.len()));
        for k in 0..3 {
            let delta = (be_u32(a, 8 + 4 * k) as i32 - be_u32(b, 8 + 4 * k) as i32).abs();
            assert!(delta <= 12, "{} component {k}: {delta}", sig.escape_ascii());
        }
    }
    assert_eq!(
        icc_tag(&ours, b"cprt"),
        icc_tag(&icc_tags(&source), b"cprt")
    );
}

#[test]
fn photoshops_linear_profile_reopens_as_srgb_and_resaves_its_twin() {
    // Photoshop's recomputed twin of macOS sRGB is recognised as the twin of
    // the built-in sRGB, so the document takes the gamma source.
    let path = crate::common::psd_photoshop::fixture("depth32", "source32.psd");
    let photoshop = photoshop_file();
    let document = open(&path, LAYERS).expect("Photoshop's file opens");
    assert_eq!(
        &document.profile().bytes()[..],
        &IccProfile::srgb().bytes()[..],
        "the built-in sRGB"
    );
    assert_eq!(document.profile().name(), "sRGB IEC61966-2.1");
    let directory = TempDir::new().unwrap();
    let projection = document.render(LIMIT, 0, None).unwrap();
    let out = directory.path().join("back.psd");
    save(&document, &projection, &out, 0, FLATTENED).expect("save");
    let file = read32(&std::fs::read(&out).unwrap());
    assert_linear_twin(&file.icc, IccProfile::srgb().bytes(), "sRGB IEC61966-2.1");
    assert_eq!(file.color_mode, photoshop.color_mode);
}
