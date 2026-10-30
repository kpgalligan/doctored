//! The 32-bit layered TIFF, through the C API: a float document's
//! File ▸ Export as PSD… writes Photoshop's big-endian form (the
//! records in an `Lr32` block in tag 37724, an IEEE-float flat image), the
//! layered open reads it back and reads the little-endian (IBM PC) form
//! Photoshop itself writes, and the little-endian seam
//! (`doctored_core::layered_tiff_little_endian`) writes Doctored's own
//! file in that form.
//!
//! Oracles: the scene is this file's OWN 32-bit PSD (its own writer, the
//! samples declared here), so every reopened sample is compared with the
//! declared f32 bits; the structural oracle is a test-local little-endian
//! parser run over Photoshop's own `layered32.tif`
//! (`fixtures/psd_photoshop/float32_679`) and over the seam's copy of the
//! same layers, field by field, never as a golden file. The little-endian
//! refusal files are built here byte by byte.

use std::ffi::{c_char, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{
    dr_precision_doc_open_psd, dr_precision_doc_psd_export_report, dr_precision_doc_save_psd,
    dr_psd_import_report, dr_tiff_has_layers,
};
use doctored_core::icc::IccProfile;
use doctored_core::layered_tiff_little_endian;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::Value;
use tempfile::TempDir;

use crate::common::psd_photoshop::fixture;
use crate::common::{cpath, take_err_string};

const LAYERS: i32 = 0;
const TIFF: i32 = 2;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 6;
const H: u32 = 4;
const PREFIX: &[u8] = b"Adobe Photoshop Document Data Block\0";

// ------------------------------------------------------------ C plumbing --

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

fn report(path: &Path) -> Result<Value, String> {
    let mut error = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    if json.is_null() {
        return Err(take_err_string(error));
    }
    Ok(serde_json::from_str(&take_err_string(json)).expect("report JSON"))
}

fn open(path: &Path) -> Owned {
    let report = report(path).expect("an import report");
    assert_eq!(report["import_precision"], "rgba32", "{report}");
    assert_eq!(report["layered_available"], true, "{report}");
    let expected = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let document = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            LAYERS,
            expected.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!document.is_null(), "{}", take_err_string(error));
    Owned(document)
}

fn save(document: &PrecisionDocument, projection: &PrecisionImage, path: &Path) {
    let mut error = ptr::null_mut();
    let saved = unsafe {
        dr_precision_doc_save_psd(
            document,
            projection,
            cpath(path).as_ptr(),
            TIFF,
            LAYERS,
            LIMIT,
            0,
            &mut error,
        )
    };
    assert!(saved, "{}", take_err_string(error));
}

fn export_report(document: &PrecisionDocument) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe { dr_precision_doc_psd_export_report(document, TIFF, LAYERS, &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("export report JSON")
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

fn write(directory: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = directory.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

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

// ------------------------------------------ this file's own 32-bit PSD --

/// Extended-range samples, as a float document holds them.
const VALUES: [f32; 8] = [0.0, 1e-6, 0.18, 4.0, 65504.0, -0.25, 0.5, 2.5];

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

fn be32(samples: &[f32]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_be_bytes()).collect()
}

fn raw32(samples: &[f32]) -> Vec<u8> {
    [vec![0, 0], be32(samples)].concat()
}

fn planes(rgba: &[[f32; 4]]) -> Vec<(i16, Vec<u8>)> {
    [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)]
        .into_iter()
        .map(|(id, c)| (id, raw32(&rgba.iter().map(|p| p[c]).collect::<Vec<_>>())))
        .collect()
}

fn block(key: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [b"8BIM", key, &(body.len() as u32).to_be_bytes()[..], body].concat()
}

struct Rec {
    bounds: [i32; 4],
    channels: Vec<(i16, Vec<u8>)>,
    name: &'static str,
    divider: u32,
    blend: [u8; 4],
    opacity: u8,
    flags: u8,
    mask: Option<[i32; 4]>,
}

impl Rec {
    fn raster(name: &'static str, bounds: [i32; 4], rgba: &[[f32; 4]]) -> Self {
        Rec {
            bounds,
            channels: planes(rgba),
            name,
            divider: 0,
            blend: *b"norm",
            opacity: 255,
            flags: 8,
            mask: None,
        }
    }

    fn folder(name: &'static str, divider: u32, blend: [u8; 4]) -> Self {
        Rec {
            bounds: [0; 4],
            channels: [-1i16, 0, 1, 2]
                .into_iter()
                .map(|id| (id, vec![0, 0]))
                .collect(),
            name,
            divider,
            blend,
            opacity: 255,
            flags: 0x18,
            mask: None,
        }
    }

    fn encode(&self) -> Vec<u8> {
        let mut out: Vec<u8> = self.bounds.iter().flat_map(|v| v.to_be_bytes()).collect();
        out.extend((self.channels.len() as u16).to_be_bytes());
        for (id, data) in &self.channels {
            out.extend(id.to_be_bytes());
            out.extend((data.len() as u32).to_be_bytes());
        }
        out.extend(b"8BIM");
        out.extend(self.blend);
        out.extend([self.opacity, 0, self.flags, 0]);
        let mut extra = Vec::new();
        match self.mask {
            Some(bounds) => {
                extra.extend(20u32.to_be_bytes());
                bounds.iter().for_each(|v| extra.extend(v.to_be_bytes()));
                extra.extend([255, 0, 0, 0]);
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
        extra.extend(block(b"luni", &luni));
        if self.divider != 0 {
            let mut body = self.divider.to_be_bytes().to_vec();
            if self.divider != 3 {
                body.extend(b"8BIM");
                body.extend(self.blend);
            }
            extra.extend(block(b"lsct", &body));
        }
        out.extend((extra.len() as u32).to_be_bytes());
        out.extend(extra);
        out
    }
}

/// A classic 32-bit RGB PSD in Photoshop's layout (the `Lr32` tail block),
/// sRGB, a white composite.
fn file32(records: &[Rec]) -> Vec<u8> {
    let mut body = (-(records.len() as i16)).to_be_bytes().to_vec();
    records.iter().for_each(|r| body.extend(r.encode()));
    for record in records {
        record
            .channels
            .iter()
            .for_each(|(_, data)| body.extend(data));
    }
    let mut section = 0u32.to_be_bytes().to_vec();
    section.extend(0u32.to_be_bytes());
    section.extend(block(b"Lr32", &body));
    while !section.len().is_multiple_of(4) {
        section.push(0);
    }
    let icc = IccProfile::srgb();
    let mut resources = b"8BIM".to_vec();
    resources.extend(1039u16.to_be_bytes());
    resources.extend([0, 0]);
    resources.extend((icc.bytes().len() as u32).to_be_bytes());
    resources.extend(icc.bytes().iter());
    if !resources.len().is_multiple_of(2) {
        resources.push(0);
    }
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
    bytes.extend(be32(&vec![1.0; 4 * (W * H) as usize]));
    bytes
}

struct Scene {
    bytes: Vec<u8>,
    base: Vec<[f32; 4]>,
    dodge: Vec<[f32; 4]>,
    mask: Vec<u16>,
    inside: Vec<[f32; 4]>,
}

/// Bottom first: Base (opaque, extended values); Dodge (Linear Dodge, 50 %,
/// hidden, a signed offset, partial and zero alpha, a 16-bit coverage mask);
/// a Pass Through folder Pass holding Inside.
fn scene() -> Scene {
    let base = ramp(24, 0, &[1.0]);
    let dodge = ramp(9, 1, &[0.0, 0.25, 0.5, 1.0, 0.75, 1e-6]);
    let mask = vec![0u16, 1, 65535, 32768, 12345, 65534, 2, 40000, 7];
    let inside = ramp(8, 2, &[1.0, 0.5]);
    let mut dodged = Rec::raster("Dodge", [-1, 2, 2, 5], &dodge);
    dodged.blend = *b"lddg";
    dodged.opacity = 128;
    dodged.flags = 8 | 2;
    dodged.mask = Some([0, 1, 3, 4]);
    let coverage: Vec<f32> = mask
        .iter()
        .map(|c| (f64::from(*c) / 65535.0) as f32)
        .collect();
    dodged.channels.push((-2, raw32(&coverage)));
    let records = vec![
        Rec::raster("Base", [0, 0, 4, 6], &base),
        dodged,
        Rec::folder("</Layer group>", 3, *b"norm"),
        Rec::raster("Inside", [1, 1, 3, 5], &inside),
        Rec::folder("Pass", 1, *b"pass"),
    ];
    Scene {
        bytes: file32(&records),
        base,
        dodge,
        mask,
        inside,
    }
}

fn assert_scene(document: &PrecisionDocument, scene: &Scene, label: &str) {
    assert!(document.is_float(), "{label}");
    assert_eq!(document.dimensions(), (W, H), "{label}");
    let layer = |i: usize| document.layer(i).expect("layer");
    let names: Vec<&str> = (0..4).map(|i| layer(i).name()).collect();
    assert_eq!(names, ["Base", "Dodge", "Inside", "Pass"], "{label}");
    for (index, want) in [(0usize, &scene.base), (1, &scene.dodge), (2, &scene.inside)] {
        assert_eq!(
            bits_of(layer(index).source().expect("raster")),
            bits(want),
            "{label}: record {index} bit for bit"
        );
    }
    let base = layer(0);
    assert!(base.visible(), "{label}");
    assert_eq!(base.blend_mode(), BlendMode::Normal, "{label}");
    assert_eq!(base.opacity(), 1.0, "{label}");
    let dodge = layer(1);
    assert_eq!(dodge.offset(), (2, -1), "{label}");
    // Photoshop's Linear Dodge (Add), `lddg`.
    assert_eq!(dodge.blend_mode(), BlendMode::Addition, "{label}");
    assert_eq!(dodge.opacity(), 128.0 / 255.0, "{label}");
    assert!(!dodge.visible(), "{label}");
    let mask = dodge.mask().expect("the mask");
    assert_eq!(mask.plane().samples(), &scene.mask[..], "{label}");
    assert_eq!(mask.properties().offset, (1, 0), "{label}");
    assert!(mask.enabled(), "{label}");
    assert_eq!(layer(2).offset(), (1, 1), "{label}");
    let pass = layer(3);
    assert!(pass.children().is_some(), "{label}");
    assert!(pass.is_pass_through(), "{label}");
}

// ------------------------------------------------ the TIFF, read back --

fn u16_at(bytes: &[u8], at: usize, little: bool) -> u16 {
    let pair = [bytes[at], bytes[at + 1]];
    if little {
        u16::from_le_bytes(pair)
    } else {
        u16::from_be_bytes(pair)
    }
}

fn u32_at(bytes: &[u8], at: usize, little: bool) -> u32 {
    let quad: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
    if little {
        u32::from_le_bytes(quad)
    } else {
        u32::from_be_bytes(quad)
    }
}

/// Tag 37724's value in either byte order.
fn data_block(bytes: &[u8]) -> Vec<u8> {
    let little = &bytes[..4] == b"II*\0";
    let first = u32_at(bytes, 4, little) as usize;
    let count = u16_at(bytes, first, little) as usize;
    for i in 0..count {
        let at = first + 2 + 12 * i;
        if u16_at(bytes, at, little) == 37724 {
            let length = u32_at(bytes, at + 4, little) as usize;
            let offset = u32_at(bytes, at + 8, little) as usize;
            return bytes[offset..offset + length].to_vec();
        }
    }
    panic!("no tag 37724");
}

/// One tag-37724 entry: signature and key as stored, length, body.
type Entry = ([u8; 4], [u8; 4], usize, Vec<u8>);

/// Tag 37724's entries.
fn entries(block: &[u8], little: bool) -> Vec<Entry> {
    assert!(block.starts_with(PREFIX));
    let mut out = Vec::new();
    let mut at = PREFIX.len();
    while at + 12 <= block.len() {
        let length = u32_at(block, at + 8, little) as usize;
        out.push((
            block[at..at + 4].try_into().unwrap(),
            block[at + 4..at + 8].try_into().unwrap(),
            length,
            block[at + 12..at + 12 + length].to_vec(),
        ));
        at += 12 + length.next_multiple_of(4);
    }
    assert_eq!(at, block.len(), "every entry is padded to 4");
    out
}

#[test]
fn a_float_document_round_trips_through_its_layered_tiff_in_both_byte_orders() {
    let directory = TempDir::new().unwrap();
    let scene = scene();
    let source = open(&write(&directory, "scene32.psd", &scene.bytes));
    assert_scene(&source, &scene, "the source");
    let projection = source.render(LIMIT, 0, None).expect("projection");
    let preflight = export_report(&source);
    assert_eq!(preflight["available"], true, "{preflight}");
    assert_eq!(preflight["depth"], 32);

    let mm = directory.path().join("scene.tif");
    save(&source, &projection, &mm);
    let bytes = std::fs::read(&mm).unwrap();
    assert_eq!(&bytes[..4], b"MM\0*");
    assert!(bytes.len() as u64 <= preflight["worst_case_bytes"].as_u64().unwrap());
    // Photoshop's `layered32.tif` form: one `Lr32` entry with a POSITIVE
    // count (no `Mt32`: the flat image carries the alpha).
    let listed = entries(&data_block(&bytes), false);
    let keys: Vec<[u8; 4]> = listed.iter().map(|e| e.1).collect();
    assert_eq!(keys, [*b"Lr32"]);
    assert_eq!(listed[0].0, *b"8BIM");
    assert_eq!(i16::from_be_bytes([listed[0].3[0], listed[0].3[1]]), 5);
    assert!(unsafe { dr_tiff_has_layers(cpath(&mm).as_ptr()) });
    let back = open(&mm);
    assert_scene(&back, &scene, "big-endian");

    // The seam's little-endian copy reopens to the same bits.
    let little = layered_tiff_little_endian(&bytes).expect("the little-endian copy");
    assert_eq!(&little[..4], b"II*\0");
    let ii = write(&directory, "scene_le.tif", &little);
    assert!(unsafe { dr_tiff_has_layers(cpath(&ii).as_ptr()) });
    let listed = entries(&data_block(&little), true);
    assert_eq!((listed[0].0, listed[0].1), (*b"MIB8", *b"23rL"));
    assert_eq!(i16::from_le_bytes([listed[0].3[0], listed[0].3[1]]), 5);
    assert_eq!(report(&ii).unwrap()["format"], "TIFF");
    let back_le = open(&ii);
    assert_scene(&back_le, &scene, "little-endian");

    // Both flat images are the projection, bit for bit.
    for (path, label) in [(&mm, "MM"), (&ii, "II")] {
        let flat = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
            .expect("the flat open");
        assert!(flat.is_float(), "{label}");
        assert_eq!(flat.layer_count(), 1, "{label}");
        let image = flat.layer(0).unwrap().source().expect("the flat image");
        assert_eq!(image.dimensions(), (W, H), "{label}");
        assert_eq!(
            bits_of(image),
            bits_of(&projection),
            "{label}: the flat image"
        );
    }
}

// ----------------------------------- the structural oracle: Photoshop's --

/// One record of a little-endian `Lr32` block, as this parser reads it.
#[derive(Debug, PartialEq)]
struct LeRecord {
    rect: [i32; 4],
    ids: Vec<i16>,
    signature: [u8; 4],
    blend: [u8; 4],
    opacity: u8,
    clipping: u8,
    flags: u8,
    mask_length: u32,
    luni: Vec<u16>,
    /// Each channel's compression word and its INFLATED payload (word 3).
    planes: Vec<(u16, Vec<u8>)>,
}

/// A little-endian `Lr32` body: the count and the records.
fn le_records(info: &[u8]) -> (i16, Vec<LeRecord>) {
    let le32 = |at: usize| u32::from_le_bytes(info[at..at + 4].try_into().unwrap());
    let le16 = |at: usize| u16::from_le_bytes([info[at], info[at + 1]]);
    let count = le16(0) as i16;
    let mut at = 2;
    let mut records = Vec::new();
    let mut lengths = Vec::new();
    for _ in 0..count.unsigned_abs() {
        let rect = [0, 4, 8, 12].map(|k| le32(at + k) as i32);
        at += 16;
        let n = le16(at) as usize;
        at += 2;
        let mut ids = Vec::new();
        let mut channel_lengths = Vec::new();
        for _ in 0..n {
            ids.push(le16(at) as i16);
            channel_lengths.push(le32(at + 2) as usize);
            at += 6;
        }
        let signature: [u8; 4] = info[at..at + 4].try_into().unwrap();
        let blend: [u8; 4] = info[at + 4..at + 8].try_into().unwrap();
        let (opacity, clipping, flags) = (info[at + 8], info[at + 9], info[at + 10]);
        at += 12;
        let extra = le32(at) as usize;
        at += 4;
        let end = at + extra;
        let mask_length = le32(at);
        let mut q = at + 4 + mask_length as usize;
        let ranges = le32(q) as usize;
        q += 4 + ranges;
        q += (1 + info[q] as usize).next_multiple_of(4);
        let mut luni = Vec::new();
        while q + 12 <= end {
            assert_eq!(&info[q..q + 4], b"MIB8", "a record block's signature");
            let length = le32(q + 8) as usize;
            if &info[q + 4..q + 8] == b"inul" {
                let units = le32(q + 12) as usize;
                luni = (0..units).map(|u| le16(q + 16 + 2 * u)).collect();
            }
            q += 12 + length;
        }
        at = end;
        records.push(LeRecord {
            rect,
            ids,
            signature,
            blend,
            opacity,
            clipping,
            flags,
            mask_length,
            luni,
            planes: Vec::new(),
        });
        lengths.push(channel_lengths);
    }
    for (record, channel_lengths) in records.iter_mut().zip(lengths) {
        for length in channel_lengths {
            let data = &info[at..at + length];
            at += length;
            let word = u16::from_le_bytes([data[0], data[1]]);
            let payload = if word == 3 {
                miniz_oxide::inflate::decompress_to_vec_zlib(&data[2..]).expect("a zlib stream")
            } else {
                data[2..].to_vec()
            };
            record.planes.push((word, payload));
        }
    }
    (count, records)
}

#[test]
fn the_seams_little_endian_records_match_photoshops_field_by_field() {
    let directory = TempDir::new().unwrap();
    let photoshop = std::fs::read(fixture("float32_679", "layered32.tif"))
        .expect("Photoshop's layered32.tif is committed");
    assert_eq!(&photoshop[..4], b"II*\0");
    let theirs = entries(&data_block(&photoshop), true);
    assert_eq!((theirs[0].0, theirs[0].1), (*b"MIB8", *b"23rL"));

    // Photoshop's file, opened, saved big-endian, then written little-endian.
    let document = open(&fixture("float32_679", "layered32.tif"));
    let projection = document.render(LIMIT, 0, None).unwrap();
    let mm = directory.path().join("resaved.tif");
    save(&document, &projection, &mm);
    let ours = layered_tiff_little_endian(&std::fs::read(&mm).unwrap()).unwrap();
    let block = data_block(&ours);
    let mine = entries(&block, true);
    assert_eq!(mine.len(), 1, "the records alone");
    assert_eq!((mine[0].0, mine[0].1), (*b"MIB8", *b"23rL"));
    let stored = u32::from_le_bytes(
        block[PREFIX.len() + 8..PREFIX.len() + 12]
            .try_into()
            .unwrap(),
    );
    assert_eq!(stored as usize, mine[0].2, "the length is little-endian");
    assert_eq!(
        block.len(),
        PREFIX.len() + 12 + mine[0].2.next_multiple_of(4)
    );

    let (their_count, their_records) = le_records(&theirs[0].3);
    let (count, records) = le_records(&mine[0].3);
    assert_eq!((count, their_count), (2, 2), "a positive count of two");
    assert_eq!(records.len(), their_records.len());
    for (mine, theirs) in records.iter().zip(&their_records) {
        let name = String::from_utf16_lossy(&theirs.luni);
        assert_eq!(mine.rect, theirs.rect, "{name}: the rectangle");
        assert_eq!(mine.ids, theirs.ids, "{name}: the channel ids");
        assert_eq!(mine.signature, *b"MIB8", "{name}");
        assert_eq!(theirs.signature, *b"MIB8", "{name}");
        assert_eq!(mine.blend, *b"mron", "{name}");
        assert_eq!(theirs.blend, *b"mron", "{name}");
        assert_eq!(
            (mine.opacity, mine.clipping, mine.flags),
            (theirs.opacity, theirs.clipping, theirs.flags),
            "{name}: opacity, clipping and flags"
        );
        assert_eq!((mine.mask_length, theirs.mask_length), (0, 0), "{name}");
        assert_eq!(mine.luni, theirs.luni, "{name}: luni count and units");
        assert_eq!(mine.planes.len(), theirs.planes.len(), "{name}");
        for (k, (m, t)) in mine.planes.iter().zip(&theirs.planes).enumerate() {
            assert_eq!((m.0, t.0), (3, 3), "{name} channel {k}: word 3");
            assert_eq!(m.1, t.1, "{name} channel {k}: the inflated byte planes");
        }
    }
}

// ------------------------------------------------------------ refusals --

/// A minimal little-endian layered TIFF in Photoshop's form: a 1 × 1 RGB
/// float flat image (three samples) and tag 37724 holding `info` as
/// `MIB8 23rL`.
fn little_file(info: &[u8]) -> Vec<u8> {
    let mut out = b"II*\0".to_vec();
    out.extend([0; 4]);
    let strip = out.len() as u32;
    out.extend([0u8; 12]);
    let bits = out.len() as u32;
    out.extend([32u8, 0, 32, 0, 32, 0]);
    let formats = out.len() as u32;
    out.extend([3u8, 0, 3, 0, 3, 0]);
    let data = out.len() as u32;
    out.extend(PREFIX);
    out.extend(b"MIB8");
    out.extend(b"23rL");
    out.extend((info.len() as u32).to_le_bytes());
    out.extend(info);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    let length = out.len() as u32 - data;
    let directory = out.len() as u32;
    let short = |v: u16| {
        let b = v.to_le_bytes();
        [b[0], b[1], 0, 0]
    };
    let entries: Vec<(u16, u16, u32, [u8; 4])> = vec![
        (256, 4, 1, 1u32.to_le_bytes()),
        (257, 4, 1, 1u32.to_le_bytes()),
        (258, 3, 3, bits.to_le_bytes()),
        (259, 3, 1, short(1)),
        (262, 3, 1, short(2)),
        (273, 4, 1, strip.to_le_bytes()),
        (277, 3, 1, short(3)),
        (278, 4, 1, 1u32.to_le_bytes()),
        (279, 4, 1, 12u32.to_le_bytes()),
        (339, 3, 3, formats.to_le_bytes()),
        (37724, 7, length, data.to_le_bytes()),
    ];
    out.extend((entries.len() as u16).to_le_bytes());
    for (tag, kind, count, value) in entries {
        out.extend(tag.to_le_bytes());
        out.extend(kind.to_le_bytes());
        out.extend(count.to_le_bytes());
        out.extend(value);
    }
    out.extend([0; 4]);
    out[4..8].copy_from_slice(&directory.to_le_bytes());
    out
}

/// One little-endian record named "A" over a 1 × 1 rectangle: `blocks`
/// (little-endian, after its `luni`) and `channels` (id, word, payload).
fn little_info(blocks: &[u8], mask: &[u8], channels: &[(i16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut info = 1i16.to_le_bytes().to_vec();
    for edge in [0i32, 0, 1, 1] {
        info.extend(edge.to_le_bytes());
    }
    info.extend((channels.len() as u16).to_le_bytes());
    for (id, _, payload) in channels {
        info.extend(id.to_le_bytes());
        info.extend((2 + payload.len() as u32).to_le_bytes());
    }
    info.extend(b"MIB8");
    info.extend(b"mron");
    info.extend([255, 0, 8, 0]);
    let mut extra = (mask.len() as u32).to_le_bytes().to_vec();
    extra.extend(mask);
    extra.extend(0u32.to_le_bytes());
    extra.extend([1, b'A', 0, 0]);
    extra.extend(b"MIB8inul");
    extra.extend(8u32.to_le_bytes());
    extra.extend(1u32.to_le_bytes());
    extra.extend([b'A', 0, 0, 0]);
    extra.extend(blocks);
    info.extend((extra.len() as u32).to_le_bytes());
    info.extend(extra);
    for (_, word, payload) in channels {
        info.extend(word.to_le_bytes());
        info.extend(payload);
    }
    info
}

fn empty_channels() -> Vec<(i16, u16, Vec<u8>)> {
    [-1i16, 0, 1, 2]
        .into_iter()
        .map(|id| (id, 0, Vec::new()))
        .collect()
}

const READ_TAIL: &str =
    "is not read; open it flat, or resave it from Photoshop with Macintosh byte order";

fn assert_refused(directory: &TempDir, name: &str, info: &[u8], thing: &str) {
    let path = write(directory, name, &little_file(info));
    assert!(
        unsafe { dr_tiff_has_layers(cpath(&path).as_ptr()) },
        "{name}: offered the layered open"
    );
    let error = report(&path).expect_err("refused");
    assert!(
        error.ends_with(&format!(
            "a little-endian (IBM PC) layered TIFF's {thing} {READ_TAIL}"
        )),
        "{name}: {error}"
    );
}

#[test]
fn a_little_endian_file_refuses_what_its_walk_does_not_read_by_name() {
    let directory = TempDir::new().unwrap();
    // The builder's own positive case: the same record with nothing extra
    // reports one layered record.
    let plain = write(
        &directory,
        "plain.tif",
        &little_file(&little_info(&[], &[], &empty_channels())),
    );
    let plain = report(&plain).expect("a plain little-endian record reads");
    assert_eq!(plain["import_precision"], "rgba32", "{plain}");
    assert_eq!(plain["format"], "TIFF");

    // A layer style (`lfx2`, a descriptor): not walked in little-endian.
    let mut lfx2 = b"MIB82xfl".to_vec();
    lfx2.extend(4u32.to_le_bytes());
    lfx2.extend([0; 4]);
    assert_refused(
        &directory,
        "lfx2.tif",
        &little_info(&lfx2, &[], &empty_channels()),
        "“lfx2” record",
    );

    // A Raw 32-bit channel: Photoshop writes word 3; its float bytes would
    // be little-endian, which no other reader of this block expects.
    let mut channels = empty_channels();
    channels[1] = (0, 0, 0.5f32.to_le_bytes().to_vec());
    assert_refused(
        &directory,
        "raw.tif",
        &little_info(&[], &[], &channels),
        "Raw 32-bit channel",
    );
}

#[test]
fn the_little_endian_seam_refuses_a_style_by_name() {
    let directory = TempDir::new().unwrap();
    let scene = scene();
    let source = open(&write(&directory, "scene32.psd", &scene.bytes));
    let styled = edit(
        &source,
        r#"{"op":"layer_style","version":1,"layer":0,"style":{"effects":[{"type":"drop_shadow"}]}}"#,
    );
    let projection = styled.render(LIMIT, 0, None).unwrap();
    let mm = directory.path().join("styled.tif");
    save(&styled, &projection, &mm);
    assert_eq!(
        layered_tiff_little_endian(&std::fs::read(&mm).unwrap()).unwrap_err(),
        "a little-endian layered TIFF does not carry a “lfx2” record"
    );
}
