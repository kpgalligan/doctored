//! #1's Indexed Color PSD and PSB files (colour mode 2) through the FFI.
//! Adobe's Photoshop File Format Specification is the oracle: depth 8, mode
//! 2, a 768-byte planar Color Mode Data section (256 reds, greens, blues),
//! resource 1046 the count in use (else 256), resource 1047 the transparent
//! index, the index plane as the first channel. Files the test writes byte
//! by byte, PSD and PSB, Raw and RLE, 13 × 3, give the import report and open
//! with the palette, the transparent index and every index as stored; each
//! refusal reads its sentence in the report and the open alike (the 768-byte
//! table, the palette size, the depth, ZIP), except "an index is outside the
//! palette", which only the decode can see. The exports are parsed here
//! field by field by this file's own reader ([`parse`]): the header, the
//! table, 1046 and 1047 (0xFFFF with no transparent index), the empty layer
//! section (Photoshop's own files hold no layer record, the `indexed_547`
//! fixtures), the composite rows (`bitmap_psd::unpackbits`), and they reopen
//! with the same plane, palette and transparent index. The TIFF container,
//! the side limit and a hidden layer refuse; a renamed layer is a loss, and
//! a record name in another writer's file is an import warning. No golden
//! files.

use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_indexed::dr_precision_doc_open_indexed_psd;
use doctored_core::ffi_psd::{
    dr_precision_doc_open_psd, dr_precision_doc_psd_export_report, dr_precision_doc_save_psd,
    dr_psd_import_report,
};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::bitmap_psd::{packbits, unpackbits};
use crate::common::{cpath, take_err_string};
use crate::indexed_fixture::{
    descriptor, free, indices, open_indexed, paint, palette_of, write_png, LIMIT,
};

const W: u32 = 13;
const H: u32 = 3;
const PALETTE: [[u8; 3]; 5] = [
    [200, 30, 40],
    [10, 220, 30],
    [0, 0, 0],
    [10, 220, 30],
    [250, 250, 250],
];
const T: u8 = 2;

pub(crate) const LAYERED: &str =
    "An Indexed Color file opens from its stored composite: an Indexed Color document holds one layer.";
const NAME_LOSS: &str =
    "The layer's name is not written: an Indexed Color PSD holds no layer record, as Photoshop writes it.";
const TIFF: &str = "An Indexed Color document writes an 8-bit Indexed PSD or PSB; a TIFF with \
                    layers cannot hold it. Save a Copy writes its palette TIFF.";
const HIDDEN: &str =
    "The Index layer is hidden: show it before exporting or saving a copy that keeps Indexed Color.";
const INDEXED_ONLY: &str = "an Indexed Color file opens only as an Indexed Color document";

/// The 13 × 3 plane: every entry used, the duplicate entry 3 too.
fn plane() -> Vec<u8> {
    (0..H)
        .flat_map(|y| (0..W).map(move |x| ((x * 3 + y) % 5) as u8))
        .collect()
}

fn json_of(text: *mut c_char, err: *mut c_char) -> Result<Value, String> {
    if text.is_null() {
        return Err(take_err_string(err));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    Ok(value)
}

// ---------------------------------------------------------------- the reader --

/// One layer record as this file's reader frames it.
pub(crate) struct ParsedRecord {
    pub name: String,
    pub bounds: [i32; 4],
    pub flags: u8,
    pub opacity: u8,
    /// (id, the channel's bytes: its compression word and data).
    pub channels: Vec<(i16, Vec<u8>)>,
    /// The tagged blocks' keys, in order.
    pub blocks: Vec<[u8; 4]>,
}

/// What this file's reader takes from an Indexed Color PSD or PSB.
pub(crate) struct Parsed {
    pub version: u16,
    pub channels: u16,
    pub width: u32,
    pub height: u32,
    pub depth: u16,
    pub mode: u16,
    pub mode_data: Vec<u8>,
    /// (id, body) in file order.
    pub resources: Vec<(u16, Vec<u8>)>,
    pub section_len: u64,
    pub info_len: u64,
    /// The layer count as stored (signed), 0 with no layer information.
    pub count: i16,
    pub records: Vec<ParsedRecord>,
    /// The global layer mask's length, when the section holds one.
    pub global_mask: Option<u32>,
    /// The tail blocks' keys after the global layer mask.
    pub tail: Vec<[u8; 4]>,
    pub composite_word: u16,
    /// The composite's first channel.
    pub indices: Vec<u8>,
}

impl Parsed {
    pub(crate) fn resource(&self, id: u16) -> Option<&[u8]> {
        self.resources
            .iter()
            .find(|(r, _)| *r == id)
            .map(|(_, b)| b.as_slice())
    }

    /// The count in use (1046, else 256), the palette and the transparent
    /// index (1047 when < count).
    pub(crate) fn table(&self) -> (Vec<[u8; 3]>, Option<u8>) {
        let count = self
            .resource(1046)
            .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
            .filter(|n| (1..=256).contains(n))
            .unwrap_or(256);
        let palette = (0..count)
            .map(|i| {
                [
                    self.mode_data[i],
                    self.mode_data[256 + i],
                    self.mode_data[512 + i],
                ]
            })
            .collect();
        let t = self
            .resource(1047)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
            .filter(|&t| usize::from(t) < count)
            .map(|t| t as u8);
        (palette, t)
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> &[u8] {
        let out = &self.bytes[self.at..self.at + n];
        self.at += n;
        out
    }
    fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }
    fn u16(&mut self) -> u16 {
        u16::from_be_bytes(self.take(2).try_into().unwrap())
    }
    fn i16(&mut self) -> i16 {
        i16::from_be_bytes(self.take(2).try_into().unwrap())
    }
    fn u32(&mut self) -> u32 {
        u32::from_be_bytes(self.take(4).try_into().unwrap())
    }
    fn i32(&mut self) -> i32 {
        i32::from_be_bytes(self.take(4).try_into().unwrap())
    }
    fn wide(&mut self, large: bool) -> u64 {
        if large {
            u64::from_be_bytes(self.take(8).try_into().unwrap())
        } else {
            u64::from(self.u32())
        }
    }
}

/// One RLE plane of `height` rows of `width` bytes (after its compression
/// word): the row table, then each row through this suite's PackBits.
pub(crate) fn rle_plane(body: &[u8], width: usize, height: usize, large: bool) -> Vec<u8> {
    let mut r = Reader { bytes: body, at: 0 };
    let counts: Vec<usize> = (0..height)
        .map(|_| {
            if large {
                r.u32() as usize
            } else {
                usize::from(r.u16())
            }
        })
        .collect();
    let mut out = Vec::new();
    for count in counts {
        let row = unpackbits(r.take(count));
        assert_eq!(row.len(), width, "an RLE row decodes to the width");
        out.extend_from_slice(&row);
    }
    out
}

/// This file's own reader of an Indexed Color PSD or PSB (module doc).
pub(crate) fn parse(bytes: &[u8]) -> Parsed {
    let mut r = Reader { bytes, at: 0 };
    assert_eq!(r.take(4), b"8BPS");
    let version = r.u16();
    let large = version == 2;
    assert_eq!(r.take(6), &[0; 6]);
    let channels = r.u16();
    let height = r.u32();
    let width = r.u32();
    let depth = r.u16();
    let mode = r.u16();
    let mode_len = r.u32() as usize;
    let mode_data = r.take(mode_len).to_vec();
    let resources_len = r.u32() as usize;
    let resources_end = r.at + resources_len;
    let mut resources = Vec::new();
    while r.at < resources_end {
        assert_eq!(r.take(4), b"8BIM");
        let id = r.u16();
        let name = usize::from(r.u8());
        r.take(name + (name + 1) % 2);
        let len = r.u32() as usize;
        resources.push((id, r.take(len).to_vec()));
        r.take(len % 2);
    }
    let section_len = r.wide(large);
    let section_end = r.at + section_len as usize;
    let (mut info_len, mut count, mut records, mut global_mask, mut tail) =
        (0, 0, Vec::new(), None, Vec::new());
    if section_len > 0 {
        info_len = r.wide(large);
        let info_end = r.at + info_len as usize;
        if info_len > 0 {
            count = r.i16();
            let mut tables = Vec::new();
            for _ in 0..count.unsigned_abs() {
                let bounds = [r.i32(), r.i32(), r.i32(), r.i32()];
                let n = r.u16();
                let table: Vec<(i16, usize)> =
                    (0..n).map(|_| (r.i16(), r.wide(large) as usize)).collect();
                assert_eq!(r.take(4), b"8BIM");
                r.take(4);
                let opacity = r.u8();
                r.u8();
                let flags = r.u8();
                r.u8();
                let extra_len = r.u32() as usize;
                let extra_end = r.at + extra_len;
                let mask = r.u32() as usize;
                r.take(mask);
                let ranges = r.u32() as usize;
                r.take(ranges);
                let name_len = usize::from(r.u8());
                let name = String::from_utf8_lossy(r.take(name_len)).into_owned();
                r.take((4 - (name_len + 1) % 4) % 4);
                let mut blocks = Vec::new();
                while r.at + 12 <= extra_end {
                    assert_eq!(r.take(4), b"8BIM");
                    let key: [u8; 4] = r.take(4).try_into().unwrap();
                    let len = r.u32() as usize;
                    r.take(len);
                    blocks.push(key);
                }
                r.at = extra_end;
                records.push(ParsedRecord {
                    name,
                    bounds,
                    flags,
                    opacity,
                    channels: Vec::new(),
                    blocks,
                });
                tables.push(table);
            }
            for (record, table) in records.iter_mut().zip(tables) {
                for (id, len) in table {
                    record.channels.push((id, r.take(len).to_vec()));
                }
            }
        }
        r.at = info_end;
        if r.at + 4 <= section_end {
            let len = r.u32();
            global_mask = Some(len);
            r.take(len as usize);
            // Adobe's tagged blocks, each padded to a multiple of 4; in a PSB
            // the long-length keys carry a u64 length (`8BIM` or `8B64`).
            while r.at + 12 <= section_end {
                let signature = r.take(4).to_vec();
                assert!(
                    signature == b"8BIM" || signature == b"8B64",
                    "{signature:?}"
                );
                let key: [u8; 4] = r.take(4).try_into().unwrap();
                let long = [
                    b"LMsk", b"Lr16", b"Lr32", b"Layr", b"Mt16", b"Mt32", b"Mtrn", b"Alph",
                    b"FMsk", b"lnk2", b"FEid", b"FXid", b"PxSD", b"cinf",
                ]
                .contains(&&key);
                let len = r.wide(large && long) as usize;
                r.take(len + (4 - len % 4) % 4);
                tail.push(key);
            }
        }
        r.at = section_end;
    }
    let composite_word = r.u16();
    let rest = &bytes[r.at..];
    let size = width as usize * height as usize;
    let indices = match composite_word {
        0 => rest[..size].to_vec(),
        1 => {
            let rows = usize::from(channels) * height as usize;
            let row_width = if large { 4 } else { 2 };
            let mut table = Reader { bytes: rest, at: 0 };
            let counts: Vec<usize> = (0..rows)
                .map(|_| {
                    if large {
                        table.u32() as usize
                    } else {
                        usize::from(table.u16())
                    }
                })
                .collect();
            let first: usize = counts[..height as usize].iter().sum();
            let mut body = rest[..height as usize * row_width].to_vec();
            body.extend_from_slice(&rest[rows * row_width..rows * row_width + first]);
            rle_plane(&body, width as usize, height as usize, large)
        }
        other => panic!("composite compression {other}"),
    };
    Parsed {
        version,
        channels,
        width,
        height,
        depth,
        mode,
        mode_data,
        resources,
        section_len,
        info_len,
        count,
        records,
        global_mask,
        tail,
        composite_word,
        indices,
    }
}

// ------------------------------------------------------------- the writers --

/// The planar 768-byte table of `palette`, zeros past it, cut to `len`.
fn planar(palette: &[[u8; 3]], len: usize) -> Vec<u8> {
    let mut out = vec![0u8; 768];
    for (i, rgb) in palette.iter().enumerate() {
        for c in 0..3 {
            out[c * 256 + i] = rgb[c];
        }
    }
    out.truncate(len);
    out
}

fn resource(id: u16, body: &[u8]) -> Vec<u8> {
    let mut out = b"8BIM".to_vec();
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

/// A hand-built colour-mode-2 file (module doc).
struct File<'a> {
    large: bool,
    depth: u16,
    table: Vec<u8>,
    count: Option<u16>,
    transparent: Option<u16>,
    word: u16,
    /// One plane per saved channel.
    planes: Vec<&'a [u8]>,
    /// One layer record of this name (another writer's form; Photoshop
    /// writes none), its index channel Raw.
    record: Option<&'a str>,
}

impl<'a> File<'a> {
    fn new(indices: &'a [u8]) -> Self {
        Self {
            large: false,
            depth: 8,
            table: planar(&PALETTE, 768),
            count: Some(PALETTE.len() as u16),
            transparent: Some(u16::from(T)),
            word: 1,
            planes: vec![indices],
            record: None,
        }
    }

    /// The layer-and-mask section's body: one record's layer information and
    /// an empty global layer mask, or nothing.
    fn layer_section(&self) -> Vec<u8> {
        let Some(name) = self.record else {
            return Vec::new();
        };
        let wide = |out: &mut Vec<u8>, len: usize| {
            if self.large {
                out.extend_from_slice(&(len as u64).to_be_bytes());
            } else {
                out.extend_from_slice(&(len as u32).to_be_bytes());
            }
        };
        let mut data = vec![0u8, 0];
        data.extend_from_slice(self.planes[0]);
        let mut info = 1i16.to_be_bytes().to_vec();
        for edge in [0, 0, H as i32, W as i32] {
            info.extend_from_slice(&edge.to_be_bytes());
        }
        info.extend_from_slice(&1u16.to_be_bytes());
        info.extend_from_slice(&0i16.to_be_bytes());
        wide(&mut info, data.len());
        info.extend_from_slice(b"8BIMnorm");
        info.extend_from_slice(&[255, 0, 0, 0]);
        let mut extra = vec![0u8; 8];
        extra.push(name.len() as u8);
        extra.extend_from_slice(name.as_bytes());
        while !extra.len().is_multiple_of(4) {
            extra.push(0);
        }
        info.extend_from_slice(&(extra.len() as u32).to_be_bytes());
        info.extend_from_slice(&extra);
        info.extend_from_slice(&data);
        if info.len() % 2 == 1 {
            info.push(0);
        }
        let mut out = Vec::new();
        wide(&mut out, info.len());
        out.extend_from_slice(&info);
        out.extend_from_slice(&0u32.to_be_bytes());
        out
    }

    fn bytes(&self) -> Vec<u8> {
        let mut out = b"8BPS".to_vec();
        out.extend_from_slice(&(if self.large { 2u16 } else { 1 }).to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        out.extend_from_slice(&(self.planes.len() as u16).to_be_bytes());
        out.extend_from_slice(&H.to_be_bytes());
        out.extend_from_slice(&W.to_be_bytes());
        out.extend_from_slice(&self.depth.to_be_bytes());
        out.extend_from_slice(&2u16.to_be_bytes());
        out.extend_from_slice(&(self.table.len() as u32).to_be_bytes());
        out.extend_from_slice(&self.table);
        let mut resources = Vec::new();
        if let Some(count) = self.count {
            resources.extend_from_slice(&resource(1046, &count.to_be_bytes()));
        }
        if let Some(t) = self.transparent {
            resources.extend_from_slice(&resource(1047, &t.to_be_bytes()));
        }
        out.extend_from_slice(&(resources.len() as u32).to_be_bytes());
        out.extend_from_slice(&resources);
        let section = self.layer_section();
        if self.large {
            out.extend_from_slice(&(section.len() as u64).to_be_bytes());
        } else {
            out.extend_from_slice(&(section.len() as u32).to_be_bytes());
        }
        out.extend_from_slice(&section);
        out.extend_from_slice(&self.word.to_be_bytes());
        match self.word {
            0 => {
                for plane in &self.planes {
                    out.extend_from_slice(plane);
                }
            }
            1 => {
                let rows: Vec<Vec<u8>> = self
                    .planes
                    .iter()
                    .flat_map(|p| p.chunks_exact(W as usize).map(packbits))
                    .collect();
                for row in &rows {
                    if self.large {
                        out.extend_from_slice(&(row.len() as u32).to_be_bytes());
                    } else {
                        out.extend_from_slice(&(row.len() as u16).to_be_bytes());
                    }
                }
                for row in rows {
                    out.extend_from_slice(&row);
                }
            }
            _ => out.extend_from_slice(&[0x78, 0x9c, 3, 0, 0, 0, 0, 1]),
        }
        out
    }
}

pub(crate) fn report(path: &Path) -> Result<Value, String> {
    let mut err = ptr::null_mut();
    json_of(
        unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut err) },
        err,
    )
}

/// The stored composite of the Indexed PSD at `path`, after its report.
pub(crate) fn open_psd(path: &Path) -> Result<*mut PrecisionDocument, String> {
    let fingerprint = report(path)?["source_fingerprint"]
        .as_str()
        .unwrap()
        .to_string();
    let fingerprint = CString::new(fingerprint).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_open_indexed_psd(
            cpath(path).as_ptr(),
            fingerprint.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

pub(crate) fn export_report(doc: *const PrecisionDocument, container: i32, mode: i32) -> Value {
    let mut err = ptr::null_mut();
    json_of(
        unsafe { dr_precision_doc_psd_export_report(doc, container, mode, &mut err) },
        err,
    )
    .unwrap()
}

pub(crate) fn save(
    doc: *const PrecisionDocument,
    path: &Path,
    container: i32,
    mode: i32,
) -> Result<(), String> {
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            container,
            mode,
            LIMIT,
            0,
            std::ptr::null(),
            &mut err,
        )
    };
    if ok {
        Ok(())
    } else {
        Err(take_err_string(err))
    }
}

/// An Indexed document of `palette`, `t` and `plane`, opened from a palette
/// PNG written in `dir`.
fn indexed_doc(
    dir: &Path,
    (w, h): (u32, u32),
    palette: &[[u8; 3]],
    t: Option<u8>,
    plane: &[u8],
) -> *mut PrecisionDocument {
    let path = dir.join(format!("source-{}-{w}x{h}.png", t.map_or(-1, i32::from)));
    let trns: Option<Vec<u8>> = t.map(|t| {
        (0..palette.len())
            .map(|i| if i == usize::from(t) { 0 } else { 255 })
            .collect()
    });
    write_png(&path, (w, h), 8, palette, trns.as_deref(), plane);
    open_indexed(&path).unwrap()
}

/// The document's import warnings (`PrecisionDocument::info`), as JSON text.
pub(crate) fn warnings_of(doc: *const PrecisionDocument) -> String {
    unsafe { &*doc }.info().unwrap()["import_warnings"].to_string()
}

pub(crate) fn plane_of(doc: *const PrecisionDocument) -> Vec<u8> {
    indices(doc).into_iter().map(|i| i as u8).collect()
}

// ------------------------------------------------------------------ the tests --

#[test]
fn a_hand_built_indexed_psd_reports_and_opens_exactly() {
    let dir = TempDir::new().unwrap();
    let plane = plane();
    for (large, word) in [(false, 0u16), (false, 1), (true, 0), (true, 1)] {
        let name = format!("{}-{word}.psd", if large { "psb" } else { "psd" });
        let path = dir.path().join(&name);
        let file = File {
            large,
            word,
            ..File::new(&plane)
        };
        std::fs::write(&path, file.bytes()).unwrap();
        let r = report(&path).unwrap();
        for (key, value) in [
            ("format", json!(if large { "PSB" } else { "PSD" })),
            ("width", json!(W)),
            ("height", json!(H)),
            ("depth", json!(8)),
            ("color_mode", json!("Indexed")),
            ("import_precision", json!("indexed")),
            ("layer_count", json!(0)),
            ("group_count", json!(0)),
            ("layered_available", json!(false)),
            ("layered_refusal", json!(LAYERED)),
            ("stored_composite_available", json!(true)),
            ("stored_composite_refusal", json!(null)),
            ("profile", json!(null)),
            ("verified_losses", json!([])),
            ("metadata_captured", json!(false)),
            ("requires_explicit_choice", json!(true)),
        ] {
            assert_eq!(r[key], value, "{name}: {key}");
        }
        for key in [
            "path",
            "source_fingerprint",
            "layered_note",
            "stored_composite_note",
            "observations",
            "general_limitations",
        ] {
            assert!(r.get(key).is_some(), "{name}: {key}");
        }
        let doc = open_psd(&path).unwrap();
        let d = descriptor(doc);
        assert_eq!(d["color_mode"], json!("indexed"), "{name}");
        assert_eq!(
            d["indexed"],
            json!({"colors": 5, "transparent_index": T}),
            "{name}"
        );
        assert_eq!(palette_of(doc), (PALETTE.to_vec(), Some(T)), "{name}");
        assert_eq!(plane_of(doc), plane, "{name}");
        let warnings = warnings_of(doc);
        assert!(
            warnings.contains("interpreted as sRGB"),
            "{name}: {warnings}"
        );
        free(doc);
    }
}

#[test]
fn each_refusal_reads_its_sentence_in_the_report_and_the_open() {
    let dir = TempDir::new().unwrap();
    let plane = plane();
    let wide = vec![0u8; plane.len() * 2];
    let zeros = vec![0u8; plane.len()];
    let cases: Vec<(&str, File, &str)> = vec![
        (
            "d16.psd",
            File {
                depth: 16,
                word: 0,
                planes: vec![&wide],
                ..File::new(&plane)
            },
            "an Indexed Color PSD stores 8 bits a pixel; this file declares 16",
        ),
        (
            "zip.psd",
            File {
                word: 2,
                ..File::new(&plane)
            },
            "an Indexed Color PSD's composite is Raw or RLE; this file's uses ZIP",
        ),
        (
            "short-table.psd",
            File {
                table: planar(&PALETTE, 384),
                ..File::new(&plane)
            },
            "an Indexed Color PSD's colour table is 768 bytes; this file's is 384",
        ),
        (
            "one-colour.psd",
            File {
                count: Some(1),
                transparent: None,
                planes: vec![&zeros],
                ..File::new(&plane)
            },
            "an Indexed Color palette holds 2 to 256 colours",
        ),
    ];
    for (name, file, sentence) in cases {
        let path = dir.path().join(name);
        std::fs::write(&path, file.bytes()).unwrap();
        let r = report(&path).unwrap();
        assert_eq!(
            (
                r["stored_composite_available"].clone(),
                r["stored_composite_refusal"].clone(),
                r["layered_available"].clone(),
                r["layered_refusal"].clone(),
            ),
            (json!(false), json!(sentence), json!(false), json!(LAYERED)),
            "{name}"
        );
        let err = open_psd(&path).unwrap_err();
        assert!(err.contains(sentence), "{name}: {err}");
    }

    // An index past the count: only the decode sees it.
    let mut outside = plane.clone();
    outside[7] = 9;
    let path = dir.path().join("outside.psd");
    std::fs::write(&path, File::new(&outside).bytes()).unwrap();
    assert_eq!(
        report(&path).unwrap()["stored_composite_available"],
        json!(true)
    );
    let err = open_psd(&path).unwrap_err();
    assert!(err.ends_with("an index is outside the palette"), "{err}");

    // No 1046 is 256 entries; a 1047 at or past the count is no transparent index.
    let path = dir.path().join("no-count.psd");
    let file = File {
        count: None,
        transparent: Some(300),
        ..File::new(&plane)
    };
    std::fs::write(&path, file.bytes()).unwrap();
    let doc = open_psd(&path).unwrap();
    let (palette, t) = palette_of(doc);
    assert_eq!((palette.len(), t), (256, None));
    assert_eq!(&palette[..5], &PALETTE);
    assert!(palette[5..].iter().all(|c| *c == [0, 0, 0]));
    free(doc);

    // A second saved channel is one loss and is never read as the plane.
    let path = dir.path().join("two.psd");
    let file = File {
        planes: vec![&plane, &zeros],
        ..File::new(&plane)
    };
    std::fs::write(&path, file.bytes()).unwrap();
    let r = report(&path).unwrap();
    assert_eq!(
        r["verified_losses"],
        json!([
            "Saved channel 2 is not retained: an Indexed Color document holds its index \
                plane only."
        ])
    );
    let doc = open_psd(&path).unwrap();
    assert_eq!(plane_of(doc), plane);
    free(doc);

    // The RGB open refuses colour mode 2 by name.
    let path = dir.path().join("rgb-open.psd");
    std::fs::write(&path, File::new(&plane).bytes()).unwrap();
    let fingerprint = CString::new(
        report(&path).unwrap()["source_fingerprint"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_open_psd(
            cpath(&path).as_ptr(),
            1,
            fingerprint.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    let err = take_err_string(err);
    assert!(err.contains(INDEXED_ONLY), "{err}");
}

#[test]
fn psd_and_psb_exports_parse_field_by_field_and_round_trip() {
    let dir = TempDir::new().unwrap();
    let plane = plane();
    for t in [None, Some(T)] {
        let doc = indexed_doc(dir.path(), (W, H), &PALETTE, t, &plane);
        for (container, large) in [(0, false), (1, true)] {
            let name = format!(
                "out-{}-{}.{}",
                t.map_or(-1, i32::from),
                container,
                if large { "psb" } else { "psd" }
            );
            let flattened = export_report(doc, container, 1);
            let layers = export_report(doc, container, 0);
            for r in [&flattened, &layers] {
                for (key, value) in [
                    ("available", json!(true)),
                    ("refusal", json!(null)),
                    ("depth", json!(8)),
                    ("bit_depth", json!(8)),
                    ("color_mode", json!("indexed")),
                    ("format", json!(if large { "PSB" } else { "PSD" })),
                    ("layer_count", json!(0)),
                    ("group_count", json!(0)),
                    ("losses", json!([])),
                ] {
                    assert_eq!(r[key], value, "{name}: {key}");
                }
                let plan = r["working_memory_plan_bytes"].as_u64().unwrap();
                let worst = r["worst_case_bytes"].as_u64().unwrap();
                assert_eq!(plan, worst + u64::from(W * H) * 6 + 1024 * 1024, "{name}");
            }
            assert!(layers["observations"][0]
                .as_str()
                .unwrap()
                .contains("one layer is its composite"));
            let path = dir.path().join(&name);
            save(doc, &path, container, 1).unwrap();
            let bytes = std::fs::read(&path).unwrap();
            let layered = dir.path().join(format!("layers-{name}"));
            save(doc, &layered, container, 0).unwrap();
            assert_eq!(std::fs::read(&layered).unwrap(), bytes, "{name}: one file");
            assert!(
                bytes.len() as u64 <= flattened["worst_case_bytes"].as_u64().unwrap(),
                "{name}: {} bytes past the worst case",
                bytes.len()
            );
            let p = parse(&bytes);
            assert_eq!(
                (p.version, p.channels, p.height, p.width, p.depth, p.mode),
                (if large { 2 } else { 1 }, 1, H, W, 8, 2),
                "{name}: version, one channel, height, width, depth 8, mode 2"
            );
            assert_eq!(
                p.mode_data,
                planar(&PALETTE, 768),
                "{name}: the planar table"
            );
            assert_eq!(p.resource(1046), Some(&[0u8, 5][..]), "{name}: 1046");
            // Photoshop's 1047 in every file: the index, else 0xFFFF.
            let t_body = t.map_or([0xFF, 0xFF], |t| [0, t]);
            assert_eq!(p.resource(1047), Some(&t_body[..]), "{name}: 1047");
            assert!(p.resource(1039).is_some(), "{name}: the RGB profile");
            assert_eq!(p.table(), (PALETTE.to_vec(), t), "{name}");
            assert!(p.records.is_empty(), "{name}: no layer record");
            assert_eq!(
                (
                    p.section_len,
                    p.info_len,
                    p.count,
                    p.global_mask,
                    p.tail.len()
                ),
                (0, 0, 0, None, 0),
                "{name}: the empty layer-and-mask section"
            );
            assert_eq!(p.composite_word, 1, "{name}: RLE composite");
            assert_eq!(p.indices, plane, "{name}: the composite is the plane");
            let r = report(&path).unwrap();
            assert_eq!(
                (r["format"].clone(), r["import_precision"].clone()),
                (json!(if large { "PSB" } else { "PSD" }), json!("indexed"))
            );
            let reopened = open_psd(&path).unwrap();
            assert_eq!(plane_of(reopened), plane, "{name}: every index reopens");
            assert_eq!(palette_of(reopened), (PALETTE.to_vec(), t), "{name}");
            free(reopened);
        }
        free(doc);
    }
}

#[test]
fn a_renamed_layer_is_a_loss_and_a_record_name_a_warning() {
    let dir = TempDir::new().unwrap();
    let plane = plane();
    for t in [None, Some(T)] {
        let doc = indexed_doc(dir.path(), (W, H), &PALETTE, t, &plane);
        assert!(!export_report(doc, 0, 1)["losses"]
            .as_array()
            .unwrap()
            .contains(&json!(NAME_LOSS)));
        let renamed = paint(
            doc,
            &json!({"op":"rename_layer","layer":0,"name":"Plate"}),
            &[],
        )
        .unwrap()
        .unwrap();
        free(doc);
        let losses = export_report(renamed, 0, 1)["losses"].clone();
        assert!(
            losses.as_array().unwrap().contains(&json!(NAME_LOSS)),
            "{t:?}: {losses}"
        );
        let path = dir.path().join(format!("renamed-{}.psd", t.is_some()));
        save(renamed, &path, 0, 1).unwrap();
        assert!(parse(&std::fs::read(&path).unwrap()).records.is_empty());
        let reopened = open_psd(&path).unwrap();
        assert!(!warnings_of(reopened).contains("Plate"));
        assert_eq!(plane_of(reopened), plane);
        free(reopened);
        free(renamed);
    }

    // Another writer's record: its name is one import warning, "Index" none,
    // and the composite is still the image.
    for (name, warned) in [("Plate", true), ("Index", false)] {
        let path = dir.path().join(format!("record-{name}.psd"));
        let file = File {
            record: Some(name),
            ..File::new(&plane)
        };
        std::fs::write(&path, file.bytes()).unwrap();
        let p = parse(&file.bytes());
        assert_eq!((p.count, p.records.len()), (1, 1));
        assert_eq!(
            (p.records[0].name.as_str(), p.records[0].blocks.len()),
            (name, 0)
        );
        assert_eq!(report(&path).unwrap()["layer_count"], json!(1));
        let doc = open_psd(&path).unwrap();
        let warnings = warnings_of(doc);
        assert_eq!(warnings.contains("“Plate”"), warned, "{name}: {warnings}");
        assert_eq!(plane_of(doc), plane);
        free(doc);
    }
}

#[test]
fn the_tiff_container_the_side_limit_and_a_hidden_layer_refuse() {
    let dir = TempDir::new().unwrap();
    let plane = plane();
    let doc = indexed_doc(dir.path(), (W, H), &PALETTE, Some(T), &plane);
    for mode in [0, 1] {
        let r = export_report(doc, 2, mode);
        assert_eq!(
            (r["available"].clone(), r["refusal"].clone()),
            (json!(false), json!(TIFF))
        );
    }
    let path = dir.path().join("t.tif");
    assert_eq!(save(doc, &path, 2, 1).unwrap_err(), TIFF);
    assert!(!path.exists());

    let hidden = paint(
        doc,
        &json!({"op":"layer_visible","layer":0,"visible":false}),
        &[],
    )
    .unwrap()
    .unwrap();
    assert_eq!(export_report(hidden, 0, 1)["refusal"], json!(HIDDEN));
    let path = dir.path().join("hidden.psd");
    assert_eq!(save(hidden, &path, 0, 1).unwrap_err(), HIDDEN);
    assert!(!path.exists());
    free(hidden);
    free(doc);

    // 30001 × 1: past a classic PSD's side, inside a PSB's.
    let long: Vec<u8> = (0..30001u32).map(|x| (x % 5) as u8).collect();
    let wide = indexed_doc(dir.path(), (30001, 1), &PALETTE, None, &long);
    let side = "This canvas is 30001 x 1 pixels; the classic PSD format stores at most 30000 \
                pixels per side. Export it as a Large Document (PSB), which stores up to 300000.";
    assert_eq!(export_report(wide, 0, 1)["refusal"], json!(side));
    let refused = dir.path().join("wide.psd");
    assert_eq!(save(wide, &refused, 0, 1).unwrap_err(), side);
    assert!(!refused.exists());
    assert_eq!(export_report(wide, 1, 1)["refusal"], json!(null));
    let psb = dir.path().join("wide.psb");
    save(wide, &psb, 1, 1).unwrap();
    let reopened = open_psd(&psb).unwrap();
    assert_eq!(unsafe { &*reopened }.dimensions(), (30001, 1));
    assert_eq!(plane_of(reopened), long);
    free(reopened);
    free(wide);
}

#[test]
fn the_open_tolerates_null() {
    unsafe {
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_open_indexed_psd(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_precision_doc_open_indexed_psd(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
    }
}
