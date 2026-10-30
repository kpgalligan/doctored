//! #547 spot channels 1b-1 (`psd_spot`, `psd_spot_records`): PSD and PSB
//! spot channels in and out, through the C API.
//!
//! Oracles, all written here from Adobe's file format specification and the
//! form Photoshop 26.10.0's own files take (take1 of
//! `scripts/photoshop-probe/spot_psd_take.py`, read field by field in
//! `spot_psd_photoshop.rs`): this file's own header/resource/composite
//! reader and PackBits decoder, its own resource encoder for the byte-built
//! files, and the polarity Photoshop stores (`max − coverage`). Every
//! written record is checked field by field; every round trip compares the
//! reopened channels exactly. Never a golden file.

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_cmyk::dr_precision_doc_save_cmyk;
use doctored_core::ffi_psd::{dr_precision_doc_open_gray_psd, dr_precision_doc_save_psd};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use doctored_core::spot_channel::SpotInk;
use image::{Rgba, RgbaImage};
use serde_json::Value;
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::common::{cpath, take_err_string};
use crate::duotone_document::{duotone, monotone_black};
use crate::psb_fixture::{
    export_report16, export_report8, import_report, open16, open8, save16, save8, strings,
    zlib_stored, FLATTENED, LAYERS, PSB, PSD, TIFF,
};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const W: u32 = 32;
const H: u32 = 8;
const RAMP: &str = "Spot Ramp";
const BLOC: &str = "Spot Bloc \u{e9}";
/// Photoshop's merged-transparency entry (take1's rgb8_t.psb).
const TRANSPARENCY: (u16, [u16; 4], u16, u8) = (0, [65535, 0, 0, 0], 100, 1);

fn ramp_ink() -> SpotInk {
    // Every component a whole number of hundredths: 1077 holds it exactly.
    SpotInk::new([55.25, 70.5, -30.75], 40).unwrap()
}

fn bloc_ink() -> SpotInk {
    SpotInk::new([80.0, -20.0, 60.0], 100).unwrap()
}

fn ramp8() -> Vec<u8> {
    (0..H)
        .flat_map(|y| (0..W).map(move |x| ((x * 37 + y * 11) % 256) as u8))
        .collect()
}

fn bloc8() -> Vec<u8> {
    (0..H)
        .flat_map(|y| {
            (0..W).map(move |x| {
                if (8..24).contains(&x) && (2..6).contains(&y) {
                    255
                } else {
                    0
                }
            })
        })
        .collect()
}

fn ramp16() -> Vec<u16> {
    (0..H * W)
        .map(|i| ((i * 2053 + 17) % 65536) as u16)
        .collect()
}

fn wide(plane: &[u8]) -> Vec<u16> {
    plane.iter().map(|&v| u16::from(v) * 257).collect()
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

// ---------------------------------------------------------- the reader --

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

fn be32(b: &[u8], at: usize) -> usize {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize
}

fn be64(b: &[u8], at: usize) -> usize {
    u64::from_be_bytes(b[at..at + 8].try_into().unwrap()) as usize
}

/// One PackBits row of `width` bytes (Adobe's TIFF PackBits).
fn unpack(row: &[u8], width: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < row.len() {
        let n = row[at] as i8;
        at += 1;
        if n >= 0 {
            out.extend_from_slice(&row[at..at + n as usize + 1]);
            at += n as usize + 1;
        } else if n != -128 {
            out.extend(std::iter::repeat_n(row[at], (1 - i16::from(n)) as usize));
            at += 1;
        }
    }
    assert_eq!(out.len(), width, "a PackBits row's width");
    out
}

pub struct Parsed {
    pub version: u16,
    pub channels: usize,
    pub depth: u16,
    pub color: u16,
    pub resources: Vec<(u16, Vec<u8>)>,
    /// The layer information's signed count, `None` when it is empty.
    pub count: Option<i16>,
    pub planes: Vec<Vec<u8>>,
    /// Where each RLE row starts in the file, plane-major.
    pub row_at: Vec<usize>,
}

impl Parsed {
    pub fn resource(&self, id: u16) -> Option<&[u8]> {
        self.resources
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, body)| body.as_slice())
    }

    pub fn samples(&self, plane: usize) -> Vec<u16> {
        if self.depth == 16 {
            self.planes[plane]
                .chunks_exact(2)
                .map(|s| u16::from_be_bytes([s[0], s[1]]))
                .collect()
        } else {
            self.planes[plane].iter().map(|&v| u16::from(v)).collect()
        }
    }
}

pub fn parse(b: &[u8]) -> Parsed {
    assert_eq!(&b[..4], b"8BPS");
    let version = be16(b, 4);
    let large = version == 2;
    let channels = usize::from(be16(b, 12));
    let (h, w) = (be32(b, 14), be32(b, 18));
    let (depth, color) = (be16(b, 22), be16(b, 24));
    let mut at = 26;
    at += 4 + be32(b, at);
    let resources_end = at + 4 + be32(b, at);
    at += 4;
    let mut resources = Vec::new();
    while at < resources_end {
        assert_eq!(&b[at..at + 4], b"8BIM");
        let id = be16(b, at + 4);
        let name = usize::from(b[at + 6]);
        at += 6 + ((name + 2) & !1);
        let length = be32(b, at);
        resources.push((id, b[at + 4..at + 4 + length].to_vec()));
        at += 4 + length + length % 2;
    }
    let (section, start) = if large {
        (be64(b, at), at + 8)
    } else {
        (be32(b, at), at + 4)
    };
    let count = (section > 0)
        .then(|| {
            let (info, body) = if large {
                (be64(b, start), start + 8)
            } else {
                (be32(b, start), start + 4)
            };
            (info >= 2).then(|| be16(b, body) as i16)
        })
        .flatten();
    at = start + section;
    let sample_bytes = usize::from(depth / 8);
    let row_bytes = w * sample_bytes;
    let word = be16(b, at);
    let mut planes = Vec::new();
    let mut row_at = Vec::new();
    match word {
        0 => {
            for k in 0..channels {
                let from = at + 2 + k * row_bytes * h;
                planes.push(b[from..from + row_bytes * h].to_vec());
            }
        }
        1 => {
            let width = if large { 4 } else { 2 };
            let table = at + 2;
            let mut row = table + channels * h * width;
            for k in 0..channels {
                let mut plane = Vec::new();
                for y in 0..h {
                    let entry = table + (k * h + y) * width;
                    let n = if large {
                        be32(b, entry)
                    } else {
                        usize::from(be16(b, entry))
                    };
                    row_at.push(row);
                    plane.extend_from_slice(&unpack(&b[row..row + n], row_bytes));
                    row += n;
                }
                planes.push(plane);
            }
            assert_eq!(row, b.len(), "the RLE body ends the file");
        }
        other => panic!("compression {other}"),
    }
    Parsed {
        version,
        channels,
        depth,
        color,
        resources,
        count,
        planes,
        row_at,
    }
}

// --------------------------------------------- Photoshop's forms, encoded --

fn pascal(names: &[&str]) -> Vec<u8> {
    names
        .iter()
        .flat_map(|n| std::iter::once(n.len() as u8).chain(n.bytes()))
        .collect()
}

fn unicode(names: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    for name in names {
        let units: Vec<u16> = name.encode_utf16().chain([0]).collect();
        out.extend_from_slice(&(units.len() as u32).to_be_bytes());
        for unit in units {
            out.extend_from_slice(&unit.to_be_bytes());
        }
    }
    out
}

fn display(records: &[(u16, [u16; 4], u16, u8)]) -> Vec<u8> {
    let mut out = 1u32.to_be_bytes().to_vec();
    for (space, colour, opacity, kind) in records {
        out.extend_from_slice(&space.to_be_bytes());
        for word in colour {
            out.extend_from_slice(&word.to_be_bytes());
        }
        out.extend_from_slice(&opacity.to_be_bytes());
        out.push(*kind);
    }
    out
}

fn identifiers(ids: &[u32]) -> Vec<u8> {
    ids.iter().flat_map(|id| id.to_be_bytes()).collect()
}

fn spot_record(ink: SpotInk) -> (u16, [u16; 4], u16, u8) {
    let [l, a, b] = ink.lab();
    let word = |v: f64| (v * 100.0).round() as i16 as u16;
    (
        7,
        [word(l), word(a), word(b), 0],
        u16::from(ink.solidity()),
        2,
    )
}

/// The four resources for `spots` in Photoshop's form, the merged
/// transparency's entry first when `merged`.
fn assert_resources(parsed: &Parsed, spots: &[(&str, SpotInk)], merged: bool, what: &str) {
    let mut names: Vec<&str> = Vec::new();
    let mut records = Vec::new();
    let mut ids = Vec::new();
    if merged {
        names.push("Transparency");
        records.push(TRANSPARENCY);
        ids.push(0);
    }
    for (k, (name, ink)) in spots.iter().enumerate() {
        names.push(name);
        records.push(spot_record(*ink));
        ids.push(3 + k as u32);
    }
    assert_eq!(
        parsed.resource(1006),
        Some(pascal(&names).as_slice()),
        "{what}: 1006"
    );
    assert_eq!(
        parsed.resource(1045),
        Some(unicode(&names).as_slice()),
        "{what}: 1045"
    );
    assert_eq!(
        parsed.resource(1077),
        Some(display(&records).as_slice()),
        "{what}: 1077"
    );
    assert_eq!(
        parsed.resource(1053),
        Some(identifiers(&ids).as_slice()),
        "{what}: 1053"
    );
    assert_eq!(
        parsed.resource(1007),
        None,
        "{what}: Photoshop writes no 1007"
    );
}

/// `plane` as stored: `max − coverage`.
fn stored(coverage: &[u16], max: u16) -> Vec<u16> {
    coverage.iter().map(|&c| max - c).collect()
}

// ----------------------------------------------------------- documents --

fn byte_doc() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(W, H, |x, y| {
        Rgba([(x * 8) as u8, (y * 30) as u8, 90, 255])
    }))
    .add_spot_channel(RAMP, ramp_ink(), &ramp8())
    .unwrap()
    .add_spot_channel(BLOC, bloc_ink(), &bloc8())
    .unwrap()
}

fn rgb16() -> PrecisionDocument {
    let pixels = (0..W * H)
        .map(|i| [(i * 1500) as u16, 40000, (60000 - i * 200) as u16, 65535])
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}

fn with_spots(doc: &PrecisionDocument, ramp: &[u16], bloc: &[u16]) -> PrecisionDocument {
    doc.add_spot_channel(RAMP, ramp_ink(), ramp, LIMIT, 0, None)
        .unwrap()
        .add_spot_channel(BLOC, bloc_ink(), bloc, LIMIT, 0, None)
        .unwrap()
}

fn assert_typed_spots(doc: &PrecisionDocument, ramp: &[u16], bloc: &[u16], what: &str) {
    let spots: Vec<(&str, Option<SpotInk>, &[u16])> = doc
        .channels()
        .iter()
        .map(|c| (c.name(), c.metadata().spot, c.plane().samples()))
        .collect();
    assert_eq!(
        spots,
        vec![
            (RAMP, Some(ramp_ink()), ramp),
            (BLOC, Some(bloc_ink()), bloc)
        ],
        "{what}"
    );
}

pub fn open_gray(path: &Path, mode: i32) -> Box<PrecisionDocument> {
    let fingerprint =
        CString::new(import_report(path)["source_fingerprint"].as_str().unwrap()).unwrap();
    let working = CString::new("dot_gain_20").unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_gray_psd(
            cpath(path).as_ptr(),
            mode,
            fingerprint.as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(
        !doc.is_null(),
        "{}: {}",
        path.display(),
        take_err_string(err)
    );
    unsafe { Box::from_raw(doc) }
}

pub fn save_null_projection(doc: &PrecisionDocument, path: &Path, container: i32, mode: i32) {
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
            &mut err,
        )
    };
    assert!(ok, "{}: {}", path.display(), take_err_string(err));
}

fn save_cmyk_psd(doc: &PrecisionDocument, path: &Path) {
    let request = CString::new(r#"{"format":"psd"}"#).unwrap();
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_cmyk(
            doc,
            cpath(path).as_ptr(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{}: {}", path.display(), take_err_string(err));
}

// ------------------------------------------------------------ the byte side --

#[test]
fn a_byte_document_writes_photoshops_spot_layout_in_psd_and_psb() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc();
    let spots = [(RAMP, ramp_ink()), (BLOC, bloc_ink())];
    for (container, version) in [(PSD, 1), (PSB, 2)] {
        for mode in [LAYERS, FLATTENED] {
            let what = format!("container {container}, mode {mode}");
            let path = dir.path().join(format!("byte{container}{mode}.psd"));
            save8(&doc, &path, container, mode).unwrap();
            let parsed = parse(&std::fs::read(&path).unwrap());
            assert_eq!(
                (parsed.version, parsed.channels, parsed.depth, parsed.color),
                (version, 6, 8, 3),
                "{what}: R, G, B, merged alpha, then the two spot channels"
            );
            assert_eq!(parsed.count, Some(-1), "{what}: merged alpha declared");
            assert_resources(&parsed, &spots, true, &what);
            let narrow = |p: Vec<u8>| p.into_iter().map(u16::from).collect::<Vec<_>>();
            assert_eq!(
                parsed.samples(4),
                stored(&narrow(ramp8()), 255),
                "{what}: ramp"
            );
            assert_eq!(
                parsed.samples(5),
                stored(&narrow(bloc8()), 255),
                "{what}: bloc"
            );
        }
    }
}

#[test]
fn a_byte_spot_channel_round_trips_and_is_reported() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc()
        .add_channel("Alpha 1", &ramp8(), W, H, [255, 0, 0], 0.5)
        .unwrap();
    let report = export_report8(&doc, PSD, LAYERS);
    let losses = strings(&report, "losses");
    let observations = strings(&report, "observations");
    assert!(
        losses.contains(
            &"1 saved channels are not written; this export stores no PSD alpha channels."
                .to_string()
        ),
        "{losses:?}"
    );
    assert!(observations.contains(&"2 spot channels are written as Photoshop spot channels: name, D50 Lab ink (to 0.01), solidity and coverage.".to_string()), "{observations:?}");
    for (container, mode) in [(PSD, LAYERS), (PSB, FLATTENED)] {
        let path = dir.path().join(format!("trip{container}{mode}.psd"));
        save8(&doc, &path, container, mode).unwrap();
        let report = import_report(&path);
        let observations = strings(&report, "observations");
        assert!(observations.contains(&format!("2 spot channels open as spot channels with their names, inks (D50 Lab) and solidity: \u{201c}{RAMP}\u{201d}, \u{201c}{BLOC}\u{201d}.")), "{observations:?}");
        assert!(
            !strings(&report, "verified_losses")
                .iter()
                .any(|l| l.contains("saved channels")),
            "{report}"
        );
        for open_mode in [LAYERS, FLATTENED] {
            let back = open8(&path, open_mode).unwrap();
            let channels: Vec<(&str, Option<SpotInk>, &[u8])> = back
                .channels
                .iter()
                .map(|c| (c.name.as_str(), c.spot, c.data.as_raw().as_slice()))
                .collect();
            let (ramp, bloc) = (ramp8(), bloc8());
            assert_eq!(
                channels,
                vec![
                    (RAMP, Some(ramp_ink()), ramp.as_slice()),
                    (BLOC, Some(bloc_ink()), bloc.as_slice())
                ],
                "container {container}, mode {mode}, opened {open_mode}: the alpha channel is not written"
            );
        }
    }
}

#[test]
fn fifty_six_channels_bound_the_written_spot_channels() {
    let dir = TempDir::new().unwrap();
    let mut doc = DrDocument::from_pixels(RgbaImage::from_pixel(4, 2, Rgba([9, 9, 9, 255])));
    for k in 0..53u8 {
        let plane: Vec<u8> = (0..8).map(|i| k.wrapping_mul(5).wrapping_add(i)).collect();
        let ink = SpotInk::new([f64::from(k), 0.0, 0.0], k % 101).unwrap();
        doc = doc
            .add_spot_channel(&format!("Spot {k}"), ink, &plane)
            .unwrap();
    }
    let report = export_report8(&doc, PSD, FLATTENED);
    assert!(strings(&report, "losses").contains(&"1 spot channels are not written; a PSD holds at most 56 channels, and the picture's own planes take 4.".to_string()), "{report}");
    assert!(strings(&report, "observations").contains(&"52 spot channels are written as Photoshop spot channels: name, D50 Lab ink (to 0.01), solidity and coverage.".to_string()), "{report}");
    let path = dir.path().join("bound.psd");
    save8(&doc, &path, PSD, FLATTENED).unwrap();
    assert_eq!(parse(&std::fs::read(&path).unwrap()).channels, 56);
    let back = open8(&path, FLATTENED).unwrap();
    assert_eq!(back.channels.len(), 52);
    for (k, channel) in back.channels.iter().enumerate() {
        assert_eq!(channel.name, format!("Spot {k}"));
        assert_eq!(channel.spot, doc.channels[k].spot);
        assert_eq!(channel.data.as_raw(), doc.channels[k].data.as_raw());
    }
}

#[test]
fn a_damaged_spot_plane_leaves_the_layers_open_and_is_disclosed() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("good.psd");
    save8(&byte_doc(), &path, PSD, LAYERS).unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    let parsed = parse(&bytes);
    // Plane 5 (the block) row 0 is all 255 stored: ONE repeat run of 32,
    // control 0xE1. 0xE2 decodes 31 bytes, a wrong width.
    let at = parsed.row_at[5 * H as usize];
    assert_eq!(&bytes[at..at + 2], &[0xE1, 0xFF]);
    bytes[at] = 0xE2;
    let path = write(&dir, "damaged.psd", &bytes);
    let report = import_report(&path);
    let losses = strings(&report, "verified_losses");
    assert!(losses.contains(&"Spot channel planes in this file are not readable (PSD RLE row has incorrect decoded width); no spot channel is retained.".to_string()), "{losses:?}");
    assert!(losses.contains(&"2 additional saved channels detected in the header; neither import choice retains them as editable channels.".to_string()), "{losses:?}");
    assert!(!strings(&report, "observations")
        .iter()
        .any(|o| o.contains("spot channels open")));
    let layered = open8(&path, LAYERS).expect("the layers still open");
    assert_eq!(layered.layers.len(), 1);
    assert!(layered.channels.is_empty());
}

// ----------------------------------------------------------- the typed side --

#[test]
fn a_sixteen_bit_rgb_document_round_trips_its_spot_channels() {
    let dir = TempDir::new().unwrap();
    let bloc = wide(&bloc8());
    let doc = with_spots(&rgb16(), &ramp16(), &bloc);
    for (container, mode) in [(PSD, LAYERS), (PSB, FLATTENED)] {
        let what = format!("container {container}, mode {mode}");
        let path = dir.path().join(format!("rgb16{container}{mode}.psd"));
        save16(&doc, &path, container, mode).unwrap();
        let parsed = parse(&std::fs::read(&path).unwrap());
        assert_eq!((parsed.channels, parsed.depth), (6, 16), "{what}");
        assert_resources(
            &parsed,
            &[(RAMP, ramp_ink()), (BLOC, bloc_ink())],
            true,
            &what,
        );
        assert_eq!(parsed.samples(4), stored(&ramp16(), 65535), "{what}");
        assert_eq!(parsed.samples(5), stored(&bloc, 65535), "{what}");
        for open_mode in [LAYERS, FLATTENED] {
            let back = open16(&path, open_mode).unwrap();
            assert_typed_spots(&back, &ramp16(), &bloc, &what);
        }
    }
}

#[test]
fn a_cmyk_document_round_trips_its_spot_channels_at_eight_and_sixteen_bits() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Test");
    for bits in [8u16, 16] {
        let cmyk = rgb16()
            .convert_to_cmyk(
                Some(&profile),
                &format!(r#"{{"depth":{bits}}}"#),
                LIMIT,
                0,
                None,
            )
            .unwrap();
        // At 8 bits every coverage is a whole code ×257, so it survives.
        let ramp = if bits == 8 { wide(&ramp8()) } else { ramp16() };
        let bloc = wide(&bloc8());
        let doc = with_spots(&cmyk, &ramp, &bloc);
        let max = if bits == 8 { 255 } else { 65535 };
        let file = |c: &[u16]| -> Vec<u16> {
            if bits == 8 {
                c.iter().map(|v| v / 257).collect()
            } else {
                c.to_vec()
            }
        };
        let layered = dir.path().join(format!("cmyk{bits}.psd"));
        save_null_projection(&doc, &layered, PSD, LAYERS);
        let flat = dir.path().join(format!("cmyk{bits}_flat.psb"));
        save_null_projection(&doc, &flat, PSB, FLATTENED);
        let saved = dir.path().join(format!("cmyk{bits}_save.psd"));
        save_cmyk_psd(&doc, &saved);
        for (path, channels, merged) in [(&layered, 7, true), (&flat, 6, false), (&saved, 6, false)]
        {
            let what = format!("{bits} bits, {}", path.display());
            let parsed = parse(&std::fs::read(path).unwrap());
            assert_eq!(
                (parsed.channels, parsed.depth, parsed.color),
                (channels, bits, 4),
                "{what}"
            );
            assert_resources(
                &parsed,
                &[(RAMP, ramp_ink()), (BLOC, bloc_ink())],
                merged,
                &what,
            );
            let first = channels - 2;
            assert_eq!(parsed.samples(first), stored(&file(&ramp), max), "{what}");
            assert_eq!(
                parsed.samples(first + 1),
                stored(&file(&bloc), max),
                "{what}"
            );
            for open_mode in [LAYERS, FLATTENED] {
                let back = open16(path, open_mode).unwrap();
                assert!(back.is_cmyk());
                assert_typed_spots(&back, &ramp, &bloc, &what);
            }
        }
    }
}

#[test]
fn gray_and_duotone_documents_round_trip_their_spot_channels() {
    let dir = TempDir::new().unwrap();
    for bits in [8u16, 16] {
        let gray = rgb16()
            .convert_to_gray(
                None,
                &format!(r#"{{"space":"gray_gamma_22","depth":{bits}}}"#),
                LIMIT,
                0,
                None,
            )
            .unwrap();
        let ramp = if bits == 8 { wide(&ramp8()) } else { ramp16() };
        let bloc = wide(&bloc8());
        let doc = with_spots(&gray, &ramp, &bloc);
        let max = if bits == 8 { 255 } else { 65535 };
        let file: Vec<u16> = if bits == 8 {
            ramp.iter().map(|v| v / 257).collect()
        } else {
            ramp.clone()
        };
        for (container, mode) in [(PSD, LAYERS), (PSB, FLATTENED)] {
            let what = format!("gray {bits}, container {container}, mode {mode}");
            let path = dir.path().join(format!("gray{bits}{container}{mode}.psd"));
            save16(&doc, &path, container, mode).unwrap();
            let parsed = parse(&std::fs::read(&path).unwrap());
            assert_eq!(
                (parsed.channels, parsed.depth, parsed.color),
                (3, bits, 1),
                "{what}"
            );
            assert!(parsed.count.unwrap_or(0) >= 0, "{what}: no merged alpha");
            assert_resources(
                &parsed,
                &[(RAMP, ramp_ink()), (BLOC, bloc_ink())],
                false,
                &what,
            );
            assert_eq!(parsed.samples(1), stored(&file, max), "{what}");
            for open_mode in [LAYERS, FLATTENED] {
                let back = open_gray(&path, open_mode);
                assert!(back.is_gray());
                assert_typed_spots(&back, &ramp, &bloc, &what);
            }
        }
    }
    // Duotone writes 8 bits at either document depth (`psd_export_gray`,
    // #547 1b-2's RULING-1: Photoshop opened no 16-bit Duotone file): a
    // 16-bit document from its 8-bit copy, each coverage rounded to the
    // nearest code, the document itself unchanged.
    for bits in [16u16, 8] {
        let gray = rgb16()
            .convert_to_gray(
                None,
                &format!(r#"{{"space":"gray_gamma_22","depth":{bits}}}"#),
                LIMIT,
                0,
                None,
            )
            .unwrap();
        let gray_ptr = Box::into_raw(Box::new(gray));
        let duo_ptr = duotone(gray_ptr, &monotone_black());
        let duo = unsafe { Box::from_raw(duo_ptr) };
        drop(unsafe { Box::from_raw(gray_ptr) });
        let ramp = if bits == 8 { wide(&ramp8()) } else { ramp16() };
        let bloc = wide(&bloc8());
        let doc = with_spots(&duo, &ramp, &bloc);
        // The nearest 8-bit code of each coverage, as the file holds it.
        let codes: Vec<u16> = ramp
            .iter()
            .map(|&v| ((f64::from(v) / 257.0).round()) as u16)
            .collect();
        for (container, mode) in [(PSD, LAYERS), (PSB, FLATTENED)] {
            let what = format!("Duotone {bits}, container {container}, mode {mode}");
            let path = dir
                .path()
                .join(format!("duotone{bits}{container}{mode}.psd"));
            save16(&doc, &path, container, mode).unwrap();
            let parsed = parse(&std::fs::read(&path).unwrap());
            assert_eq!(
                (parsed.channels, parsed.depth, parsed.color),
                (3, 8, 8),
                "{what}: an 8-bit Duotone file"
            );
            assert_resources(
                &parsed,
                &[(RAMP, ramp_ink()), (BLOC, bloc_ink())],
                false,
                &what,
            );
            assert_eq!(parsed.samples(1), stored(&codes, 255), "{what}");
            assert_eq!(parsed.samples(2), stored(&bloc8w(), 255), "{what}");
            let back = open_gray(&path, LAYERS);
            assert!(back.is_duotone(), "{what}");
            assert_typed_spots(&back, &wide8(&codes), &bloc, &what);
        }
        assert_typed_spots(&doc, &ramp, &bloc, "the document keeps its own depth");
        let report = export_report16(&doc, PSD, LAYERS);
        assert_eq!(report["depth"], 8, "Duotone {bits}: the report's depth");
        let loss = "16-bit Duotone samples written at 8 bits (Photoshop's Duotone is 8-bit)";
        assert_eq!(
            strings(&report, "losses").iter().any(|l| l == loss),
            bits == 16,
            "Duotone {bits}: the depth loss line"
        );
    }
}

/// `bloc8` as file codes (0 or 255).
fn bloc8w() -> Vec<u16> {
    bloc8().iter().map(|&v| u16::from(v)).collect()
}

/// 8-bit codes widened ×257, as an 8-bit document holds them.
fn wide8(codes: &[u16]) -> Vec<u16> {
    codes.iter().map(|&v| v * 257).collect()
}

#[test]
fn lab_and_tiff_exports_disclose_the_spot_channels_they_do_not_write() {
    let lab = rgb16()
        .convert_to_lab(r#"{"depth":16}"#, LIMIT, 0, None)
        .unwrap();
    let doc = with_spots(&lab, &ramp16(), &wide(&bloc8()));
    let report = export_report16(&doc, PSD, LAYERS);
    let losses = strings(&report, "losses");
    assert!(losses.contains(&"2 spot channels are not written; Doctored writes PSD spot channels only in RGB, CMYK, Grayscale and Duotone at 8 and 16 bits.".to_string()), "{losses:?}");
    assert!(
        !losses
            .iter()
            .any(|l| l.contains("saved channels are not written")),
        "{losses:?}"
    );
    assert!(!strings(&report, "observations")
        .iter()
        .any(|o| o.contains("spot channels are written")));
    // Since 1c-2b a layered TIFF writes them (`spot_tiff_layers.rs`): they
    // leave the alpha sentence and are named as written.
    let report = export_report8(&byte_doc(), TIFF, LAYERS);
    let losses = strings(&report, "losses");
    assert!(
        !losses
            .iter()
            .any(|l| l.contains("saved channels are not written")),
        "{losses:?}"
    );
    assert!(
        !losses
            .iter()
            .any(|l| l.contains("spot channels are not written")),
        "{losses:?}"
    );
    assert!(strings(&report, "observations")
        .iter()
        .any(|o| o.starts_with("2 spot channels are written as Photoshop spot channels")));
}

// ---------------------------------------------------- byte-built files --

fn block(id: u16, body: &[u8]) -> Vec<u8> {
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

/// An 8-bit RGB 4 x 2 PSD with no layer records: R, G, B at 128, then
/// `saved` stored planes; Raw, or one zlib stream (compression 2).
fn synthetic(saved: &[[u8; 8]], resources: &[(u16, Vec<u8>)], zip: bool) -> Vec<u8> {
    let mut out = b"8BPS".to_vec();
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    out.extend_from_slice(&((3 + saved.len()) as u16).to_be_bytes());
    out.extend_from_slice(&2u32.to_be_bytes());
    out.extend_from_slice(&4u32.to_be_bytes());
    out.extend_from_slice(&8u16.to_be_bytes());
    out.extend_from_slice(&3u16.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    let body: Vec<u8> = resources.iter().flat_map(|(id, b)| block(*id, b)).collect();
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    out.extend_from_slice(&0u32.to_be_bytes());
    let mut planes = vec![128u8; 24];
    for plane in saved {
        planes.extend_from_slice(plane);
    }
    if zip {
        out.extend_from_slice(&2u16.to_be_bytes());
        out.extend_from_slice(&zlib_stored(&planes));
    } else {
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&planes);
    }
    out
}

const PLANE: [u8; 8] = [255, 200, 128, 0, 10, 20, 30, 255];

fn lab_words(l: i16, a: i16, b: i16) -> [u16; 4] {
    [(l as u16), a as u16, b as u16, 0]
}

/// The report's losses and observations and the composite open's channels.
fn read(dir: &TempDir, name: &str, bytes: &[u8]) -> (Vec<String>, Vec<String>, Box<DrDocument>) {
    let path = write(dir, name, bytes);
    let report: Value = import_report(&path);
    let doc = open8(&path, FLATTENED).unwrap();
    (
        strings(&report, "verified_losses"),
        strings(&report, "observations"),
        doc,
    )
}

#[test]
fn byte_built_files_keep_their_spot_channels_and_disclose_the_rest() {
    let dir = TempDir::new().unwrap();
    let alpha = (0u16, [65535, 0, 0, 0], 50u16, 0u8);
    let ink = (7u16, lab_words(5000, 1000, -1000), 70u16, 2u8);
    let names = |n: &[&str]| vec![(1006, pascal(n)), (1045, unicode(n))];
    let saved_alpha = "1 additional saved channels detected in the header; neither import choice retains them as editable channels.";

    // An alpha channel, then a spot channel: the spot is kept, the alpha
    // stays the disclosed loss.
    let mut resources = names(&["Alpha 1", "Ink"]);
    resources.push((1077, display(&[alpha, ink])));
    let (losses, observations, doc) = read(
        &dir,
        "kept.psd",
        &synthetic(&[[7; 8], PLANE], &resources, false),
    );
    assert!(losses.contains(&saved_alpha.to_string()), "{losses:?}");
    assert!(observations.contains(&"1 spot channels open as spot channels with their names, inks (D50 Lab) and solidity: \u{201c}Ink\u{201d}.".to_string()), "{observations:?}");
    assert_eq!(doc.channels.len(), 1);
    assert_eq!(doc.channels[0].name, "Ink");
    assert_eq!(
        doc.channels[0].spot,
        Some(SpotInk::new([50.0, 10.0, -10.0], 70).unwrap())
    );
    assert_eq!(
        doc.channels[0].data.as_raw(),
        &PLANE.map(|v| 255 - v).to_vec()
    );

    // Only 1006, a byte per code point.
    let resources = vec![
        (1006, vec![6, b'B', b'l', b'o', b'c', b' ', 0xE9]),
        (1077, display(&[ink])),
    ];
    let (_, _, doc) = read(&dir, "pascal.psd", &synthetic(&[PLANE], &resources, false));
    assert_eq!(doc.channels[0].name, "Bloc \u{e9}");

    // An ink in another colour space, an opacity past 100 and an L past 100.
    for (name, record, sentence) in [
        ("Odd", (0, [65535, 0, 0, 0], 50, 2), "Spot channel \u{201c}Odd\u{201d} stores its ink in colour space 0, which Doctored does not read; it is not retained."),
        ("Over", (7, lab_words(5000, 0, 0), 101, 2), "Spot channel \u{201c}Over\u{201d} stores an ink Doctored cannot hold; it is not retained."),
        ("Bright", (7, lab_words(12000, 0, 0), 50, 2), "Spot channel \u{201c}Bright\u{201d} stores an ink Doctored cannot hold; it is not retained."),
    ] {
        let mut resources = names(&[name]);
        resources.push((1077, display(&[record])));
        let (losses, observations, doc) =
            read(&dir, &format!("{name}.psd"), &synthetic(&[PLANE], &resources, false));
        assert!(losses.contains(&sentence.to_string()), "{name}: {losses:?}");
        assert!(losses.contains(&saved_alpha.to_string()), "{name}: {losses:?}");
        assert!(!observations.iter().any(|o| o.contains("spot channels open")), "{name}");
        assert!(doc.channels.is_empty(), "{name}");
    }

    // 1077 one record short.
    let mut resources = names(&["A", "B"]);
    resources.push((1077, display(&[ink])));
    let (losses, _, doc) = read(
        &dir,
        "short.psd",
        &synthetic(&[PLANE, PLANE], &resources, false),
    );
    assert!(losses.contains(&"This file's saved-channel resources (1006, 1045, 1077) do not match its 2 saved channels; no spot channel is retained.".to_string()), "{losses:?}");
    assert!(losses.contains(&"2 additional saved channels detected in the header; neither import choice retains them as editable channels.".to_string()), "{losses:?}");
    assert!(doc.channels.is_empty());

    // A ZIP composite: disclosed, not read.
    let mut resources = names(&["Ink"]);
    resources.push((1077, display(&[ink])));
    let (losses, _, doc) = read(&dir, "zip.psd", &synthetic(&[PLANE], &resources, true));
    assert!(
        losses.contains(
            &"This file's stored composite is ZIP-compressed; its 1 spot channels are not read."
                .to_string()
        ),
        "{losses:?}"
    );
    assert!(doc.channels.is_empty());

    // No 1077: an alpha channel as before, and nothing more.
    let (losses, _, doc) = read(
        &dir,
        "plain.psd",
        &synthetic(&[PLANE], &names(&["Ink"]), false),
    );
    assert!(losses.contains(&saved_alpha.to_string()), "{losses:?}");
    assert!(
        !losses
            .iter()
            .any(|l| l.contains("spot") || l.contains("1077")),
        "{losses:?}"
    );
    assert!(doc.channels.is_empty());
}
