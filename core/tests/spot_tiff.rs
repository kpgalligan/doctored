//! #1 spot channels 1c-2a (`tiff_spot`, `tiff_spot_samples`): flat TIFF
//! spot channels in and out, through the C API where one exists.
//!
//! Oracles, all written here: this file's own TIFF directory and strip
//! reader, its own 8BIM resource reader and encoder, and the layout
//! Photoshop 26.10.0 writes (`scripts/photoshop-probe/spot_tiff_take.py`
//! take2, read field by field in `spot_tiff_photoshop.rs`): the spot
//! channels are the trailing extra samples (ExtraSamples 0) after the
//! colour and any transparency, stored `max − coverage`, named and inked by
//! resources 1006/1045/1077/1053 in tag 34377, leading with "Transparency"
//! when the file has one. Never a golden file.

use std::collections::HashMap;
use std::ffi::CString;
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_export::ExportRequest;
use doctored_core::ffi_cmyk::dr_precision_doc_save_cmyk;
use doctored_core::ffi_color::dr_doc_save_image;
use doctored_core::ffi_doc::dr_doc_open;
use doctored_core::ffi_metadata_report::dr_path_metadata_import_report;
use doctored_core::ffi_precision::{dr_precision_doc_info, dr_precision_doc_open};
use doctored_core::ffi_preservation::dr_doc_flat_save_losses;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use doctored_core::spot_channel::{SpotInk, FLAT_LOSS, TIFF_REFUSAL};
use image::{Rgba, RgbaImage};
use serde_json::Value;
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::common::{cpath, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const TIFF: i32 = 2;
const PNG: i32 = 0;
const W: u32 = 32;
const H: u32 = 8;
const RAMP: &str = "Spot Ramp";
const BLOC: &str = "Spot Bloc \u{e9}";

fn ramp_ink() -> SpotInk {
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
            (0..W).map(move |x| u8::from((8..24).contains(&x) && (2..6).contains(&y)) * 255)
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

/// An 8-bit plane as u16 samples, unchanged.
fn narrow8(plane: &[u8]) -> Vec<u16> {
    plane.iter().map(|&v| u16::from(v)).collect()
}

fn inverted(coverage: &[u16], max: u16) -> Vec<u16> {
    coverage.iter().map(|&c| max - c).collect()
}

// ------------------------------------------------------ the TIFF oracle --

/// One TIFF's first directory and its chunky, uncompressed samples.
pub struct Tiff {
    shorts: HashMap<u16, Vec<u32>>,
    pub bytes: HashMap<u16, Vec<u8>>,
    /// Plane-major samples, one plane a sample of the pixel.
    pub planes: Vec<Vec<u16>>,
}

impl Tiff {
    pub fn tag(&self, tag: u16) -> Vec<u32> {
        self.shorts.get(&tag).cloned().unwrap_or_default()
    }

    pub fn one(&self, tag: u16) -> u32 {
        self.tag(tag)[0]
    }
}

pub fn read_tiff(b: &[u8]) -> Tiff {
    let little = &b[..2] == b"II";
    let u16_at = |at: usize| {
        let v = [b[at], b[at + 1]];
        if little {
            u16::from_le_bytes(v)
        } else {
            u16::from_be_bytes(v)
        }
    };
    let u32_at = |at: usize| {
        let v = [b[at], b[at + 1], b[at + 2], b[at + 3]];
        if little {
            u32::from_le_bytes(v)
        } else {
            u32::from_be_bytes(v)
        }
    };
    assert_eq!(u16_at(2), 42);
    let ifd = u32_at(4) as usize;
    let mut shorts = HashMap::new();
    let mut bytes = HashMap::new();
    for e in 0..usize::from(u16_at(ifd)) {
        let at = ifd + 2 + e * 12;
        let (tag, kind, count) = (u16_at(at), u16_at(at + 2), u32_at(at + 4) as usize);
        let size = match kind {
            1 | 7 => 1,
            3 => 2,
            4 => 4,
            _ => continue,
        };
        let value = if size * count <= 4 {
            at + 8
        } else {
            u32_at(at + 8) as usize
        };
        match kind {
            1 | 7 => {
                bytes.insert(tag, b[value..value + count].to_vec());
            }
            3 => {
                shorts.insert(
                    tag,
                    (0..count)
                        .map(|i| u32::from(u16_at(value + 2 * i)))
                        .collect(),
                );
            }
            _ => {
                shorts.insert(tag, (0..count).map(|i| u32_at(value + 4 * i)).collect());
            }
        }
    }
    let mut tiff = Tiff {
        shorts,
        bytes,
        planes: Vec::new(),
    };
    assert_eq!(
        tiff.tag(259).first().copied().unwrap_or(1),
        1,
        "uncompressed"
    );
    assert_eq!(tiff.tag(284).first().copied().unwrap_or(1), 1, "chunky");
    let samples = tiff.one(277) as usize;
    let depth = tiff.one(258);
    let mut data = Vec::new();
    for (offset, count) in tiff.tag(273).into_iter().zip(tiff.tag(279)) {
        data.extend_from_slice(&b[offset as usize..(offset + count) as usize]);
    }
    let values: Vec<u16> = if depth == 16 {
        data.chunks_exact(2)
            .map(|v| {
                if little {
                    u16::from_le_bytes([v[0], v[1]])
                } else {
                    u16::from_be_bytes([v[0], v[1]])
                }
            })
            .collect()
    } else {
        data.iter().map(|&v| u16::from(v)).collect()
    };
    let pixels = (tiff.one(256) * tiff.one(257)) as usize;
    assert_eq!(
        values.len(),
        pixels * samples,
        "the strips hold every sample"
    );
    tiff.planes = (0..samples)
        .map(|s| (0..pixels).map(|p| values[p * samples + s]).collect())
        .collect();
    tiff
}

/// The 8BIM run's resources by id.
pub fn resources(run: &[u8]) -> HashMap<u16, Vec<u8>> {
    let mut out = HashMap::new();
    let mut at = 0;
    while at < run.len() {
        assert_eq!(&run[at..at + 4], b"8BIM");
        let id = u16::from_be_bytes([run[at + 4], run[at + 5]]);
        let name = (1 + usize::from(run[at + 6])).next_multiple_of(2);
        let size_at = at + 6 + name;
        let size = u32::from_be_bytes(run[size_at..size_at + 4].try_into().unwrap()) as usize;
        let body = run[size_at + 4..size_at + 4 + size].to_vec();
        assert!(out.insert(id, body).is_none(), "resource {id} once");
        at = size_at + 4 + size + size % 2;
    }
    out
}

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

/// A 1077 record: (colour space, colour words, opacity, kind).
type Display = (u16, [u16; 4], u16, u8);
const TRANSPARENCY: Display = (0, [65535, 0, 0, 0], 100, 1);

fn spot_display(ink: SpotInk) -> Display {
    let lab = ink.lab();
    let word = |v: f64| (v * 100.0).round() as i16 as u16;
    (
        7,
        [word(lab[0]), word(lab[1]), word(lab[2]), 0],
        u16::from(ink.solidity()),
        2,
    )
}

/// The four lists a file with `spots` (and a leading transparency) holds.
fn channel_run(spots: &[(&str, Display)], transparency: bool) -> Vec<u8> {
    let mut entries: Vec<(&str, Display, u32)> = Vec::new();
    if transparency {
        entries.push(("Transparency", TRANSPARENCY, 0));
    }
    for (k, (name, display)) in spots.iter().enumerate() {
        entries.push((name, *display, 3 + k as u32));
    }
    let (mut pascal, mut unicode, mut ids) = (Vec::new(), Vec::new(), Vec::new());
    let mut display = 1u32.to_be_bytes().to_vec();
    for (name, (space, words, opacity, kind), id) in &entries {
        pascal.push(name.len() as u8);
        pascal.extend_from_slice(name.as_bytes());
        let units: Vec<u16> = name.encode_utf16().chain([0]).collect();
        unicode.extend_from_slice(&(units.len() as u32).to_be_bytes());
        units
            .iter()
            .for_each(|u| unicode.extend_from_slice(&u.to_be_bytes()));
        display.extend_from_slice(&space.to_be_bytes());
        words
            .iter()
            .for_each(|w| display.extend_from_slice(&w.to_be_bytes()));
        display.extend_from_slice(&opacity.to_be_bytes());
        display.push(*kind);
        ids.extend_from_slice(&id.to_be_bytes());
    }
    [
        block(1006, &pascal),
        block(1045, &unicode),
        block(1077, &display),
        block(1053, &ids),
    ]
    .concat()
}

/// The file's channel resources equal [`channel_run`]'s, and no resource
/// is duplicated (the inherited run's copies were dropped).
fn assert_channel_resources(tiff: &Tiff, transparency: bool, what: &str) {
    let found = resources(tiff.bytes.get(&34377).expect("tag 34377"));
    let spots = [
        (RAMP, spot_display(ramp_ink())),
        (BLOC, spot_display(bloc_ink())),
    ];
    let want = resources(&channel_run(&spots, transparency));
    for id in [1006u16, 1045, 1077, 1053] {
        assert_eq!(found.get(&id), want.get(&id), "{what}: resource {id}");
    }
}

// ------------------------------------------------------------ documents --

fn byte_doc(alpha: u8) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(W, H, |x, y| {
        Rgba([
            (x * 8) as u8,
            (y * 30) as u8,
            90,
            if x == 3 { alpha } else { 255 },
        ])
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

pub fn byte_spots(doc: &DrDocument) -> Vec<(String, Option<SpotInk>, Vec<u8>)> {
    doc.channels
        .iter()
        .map(|c| (c.name.clone(), c.spot, c.data.as_raw().clone()))
        .collect()
}

pub fn typed_spots(doc: &PrecisionDocument) -> Vec<(String, Option<SpotInk>, Vec<u16>)> {
    doc.channels()
        .iter()
        .map(|c| {
            (
                c.name().to_string(),
                c.metadata().spot,
                c.plane().samples().to_vec(),
            )
        })
        .collect()
}

pub fn save_byte(doc: &DrDocument, path: &Path, format: i32) {
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_doc_save_image(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            format,
            90,
            true,
            false,
            ptr::null_mut(),
            &mut err,
        )
    };
    assert!(ok, "{}: {}", path.display(), take_err_string(err));
}

/// The byte open of `path`: its error, or the document.
fn try_open_byte(path: &Path) -> Result<Box<DrDocument>, String> {
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_doc_open(cpath(path).as_ptr(), &mut err) };
    if doc.is_null() {
        return Err(take_err_string(err));
    }
    Ok(unsafe { Box::from_raw(doc) })
}

pub fn open_byte(path: &Path) -> Box<DrDocument> {
    try_open_byte(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

pub fn open_typed(path: &Path) -> Result<Box<PrecisionDocument>, String> {
    let mut err = ptr::null_mut();
    let doc =
        unsafe { dr_precision_doc_open(cpath(path).as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    if doc.is_null() {
        return Err(take_err_string(err));
    }
    Ok(unsafe { Box::from_raw(doc) })
}

pub fn import_warnings(doc: &PrecisionDocument) -> Vec<String> {
    let mut err = ptr::null_mut();
    let info = unsafe { dr_precision_doc_info(doc, &mut err) };
    assert!(!info.is_null(), "{}", take_err_string(err));
    let info: Value = serde_json::from_str(&take_err_string(info)).unwrap();
    info["import_warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn report_warnings(path: &Path) -> Vec<String> {
    let mut err = ptr::null_mut();
    let report = unsafe { dr_path_metadata_import_report(cpath(path).as_ptr(), &mut err) };
    assert!(!report.is_null(), "{}", take_err_string(err));
    let report: Value = serde_json::from_str(&take_err_string(report)).unwrap();
    report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn flat_losses(doc: &DrDocument, format: i32) -> Vec<String> {
    let mut err = ptr::null_mut();
    let losses = unsafe { dr_doc_flat_save_losses(doc, format, true, false, &mut err) };
    assert!(!losses.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(losses)).unwrap()
}

fn observation() -> String {
    format!("2 spot channels open as spot channels with their names, inks (D50 Lab) and solidity: \u{201c}{RAMP}\u{201d}, \u{201c}{BLOC}\u{201d}.")
}

pub fn save_cmyk_tiff(doc: &PrecisionDocument, path: &Path) {
    let request = CString::new(r#"{"format":"tiff"}"#).unwrap();
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

// ---------------------------------------------------------------- tests --

#[test]
fn a_byte_tiff_save_keeps_its_spot_channels_as_photoshop_does() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc(255);
    assert!(!flat_losses(&doc, TIFF).contains(&FLAT_LOSS.to_string()));
    assert!(flat_losses(&doc, PNG).contains(&FLAT_LOSS.to_string()));
    let path = dir.path().join("spots.tif");
    save_byte(&doc, &path, TIFF);
    let tiff = read_tiff(&std::fs::read(&path).unwrap());
    assert_eq!(
        (tiff.one(262), tiff.one(277), tiff.tag(338)),
        (2, 5, vec![0, 0]),
        "RGB, no transparency, two trailing spot samples"
    );
    let flat = doc.flattened().unwrap();
    for c in 0..3 {
        let want: Vec<u16> = flat.pixels().map(|p| u16::from(p[c])).collect();
        assert_eq!(
            tiff.planes[c], want,
            "colour sample {c}: the plain composite"
        );
    }
    assert_eq!(
        tiff.planes[3],
        inverted(&narrow8(&ramp8()), 255),
        "ramp, H1"
    );
    assert_eq!(
        tiff.planes[4],
        inverted(&narrow8(&bloc8()), 255),
        "bloc, H1"
    );
    assert_channel_resources(&tiff, false, "byte save");
    assert!(report_warnings(&path).contains(&observation()));
    let back = open_byte(&path);
    assert_eq!(byte_spots(&back), byte_spots(&doc), "reopened");
    // The reopened document's inherited run carries the channel resources;
    // a second save writes them once, its own.
    let again = dir.path().join("again.tif");
    save_byte(&back, &again, TIFF);
    let tiff = read_tiff(&std::fs::read(&again).unwrap());
    assert_channel_resources(&tiff, false, "second save");
}

#[test]
fn a_transparent_byte_tiff_leads_with_its_transparency() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc(40);
    let path = dir.path().join("clear.tif");
    save_byte(&doc, &path, TIFF);
    let tiff = read_tiff(&std::fs::read(&path).unwrap());
    assert_eq!((tiff.one(277), tiff.tag(338)), (6, vec![2, 0, 0]));
    let alpha: Vec<u16> = doc
        .flattened()
        .unwrap()
        .pixels()
        .map(|p| u16::from(p[3]))
        .collect();
    assert_eq!(tiff.planes[3], alpha, "the straight alpha first");
    assert_eq!(tiff.planes[5], inverted(&narrow8(&bloc8()), 255));
    assert_channel_resources(&tiff, true, "transparent save");
    let back = open_byte(&path);
    assert_eq!(byte_spots(&back), byte_spots(&doc), "reopened");
    assert_eq!(back.flattened().unwrap(), doc.flattened().unwrap());
}

#[test]
fn a_sixteen_bit_rgb_document_keeps_its_spot_channels_in_a_tiff_export() {
    let dir = TempDir::new().unwrap();
    let (ramp, bloc) = (ramp16(), wide(&bloc8()));
    let doc = with_spots(&rgb16(), &ramp, &bloc);
    for bits in [16u32, 8] {
        let request = ExportRequest::parse(&format!(
            r#"{{"format":"tiff","bit_depth":{bits},"spot_channels":"keep"}}"#
        ))
        .unwrap();
        let kept = doc.export_plan(None, &request).unwrap()["scratch_bytes"]
            .as_u64()
            .unwrap();
        assert!(kept > 0);
        let result = doc.export_encoded(None, &request, LIMIT, 0, None).unwrap();
        let path = dir.path().join(format!("rgb{bits}.tif"));
        std::fs::write(&path, result.bytes()).unwrap();
        let tiff = read_tiff(result.bytes());
        assert_eq!(
            (tiff.one(258), tiff.one(277), tiff.tag(338)),
            (bits, 5, vec![0, 0])
        );
        assert_channel_resources(&tiff, false, &format!("{bits}-bit export"));
        let file = |c: &[u16]| -> Vec<u16> {
            if bits == 16 {
                inverted(c, 65535)
            } else {
                c.iter()
                    .map(|&v| ((u32::from(65535 - v) + 128) / 257) as u16)
                    .collect()
            }
        };
        assert_eq!(tiff.planes[3], file(&ramp), "{bits}: ramp");
        assert_eq!(tiff.planes[4], file(&bloc), "{bits}: bloc");
        if bits == 16 {
            let back = open_typed(&path).unwrap();
            assert_eq!(typed_spots(&back), typed_spots(&doc), "16: reopened");
            assert!(import_warnings(&back).contains(&observation()));
        } else {
            // An 8-bit RGB TIFF is the byte document's (the typed open is 16-bit only).
            let narrow =
                |c: &[u16]| -> Vec<u8> { file(c).iter().map(|&s| (255 - s) as u8).collect() };
            let want = vec![
                (RAMP.to_string(), Some(ramp_ink()), narrow(&ramp)),
                (BLOC.to_string(), Some(bloc_ink()), narrow(&bloc)),
            ];
            assert_eq!(byte_spots(&open_byte(&path)), want, "8: reopened");
            assert!(report_warnings(&path).contains(&observation()));
        }
    }
}

#[test]
fn a_cmyk_document_saves_its_spot_channels_in_its_tiff() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Test");
    for bits in [8u32, 16] {
        let cmyk = rgb16()
            .convert_to_cmyk(
                Some(&profile),
                &format!(r#"{{"depth":{bits}}}"#),
                LIMIT,
                0,
                None,
            )
            .unwrap();
        let ramp = if bits == 8 { wide(&ramp8()) } else { ramp16() };
        let bloc = wide(&bloc8());
        let doc = with_spots(&cmyk, &ramp, &bloc);
        let plan = |d: &PrecisionDocument| {
            d.save_cmyk_plan(r#"{"format":"tiff"}"#).unwrap()["working_memory_plan_bytes"]
                .as_u64()
                .unwrap()
        };
        assert!(
            plan(&doc) > plan(&cmyk),
            "{bits}: the plan charges the spot planes"
        );
        let path = dir.path().join(format!("cmyk{bits}.tif"));
        save_cmyk_tiff(&doc, &path);
        let tiff = read_tiff(&std::fs::read(&path).unwrap());
        assert_eq!(
            (tiff.one(262), tiff.one(277), tiff.tag(338)),
            (5, 6, vec![0, 0])
        );
        assert_channel_resources(&tiff, false, &format!("cmyk {bits}"));
        let max = if bits == 8 { 255 } else { 65535 };
        let file = |c: &[u16]| -> Vec<u16> {
            let c: Vec<u16> = if bits == 8 {
                c.iter().map(|v| v / 257).collect()
            } else {
                c.to_vec()
            };
            inverted(&c, max)
        };
        assert_eq!(tiff.planes[4], file(&ramp), "{bits}: ramp, H1 in CMYK too");
        assert_eq!(tiff.planes[5], file(&bloc), "{bits}: bloc");
        let back = open_typed(&path).unwrap();
        assert!(back.is_cmyk());
        assert_eq!(typed_spots(&back), typed_spots(&doc), "{bits}: reopened");
        let warnings = import_warnings(&back);
        assert!(warnings.contains(&observation()), "{warnings:?}");
        assert!(
            !warnings.iter().any(|w| w.contains("extra samples")),
            "{warnings:?}"
        );
    }
}

#[test]
fn keep_requests_and_tiff_saves_refuse_what_they_cannot_carry() {
    let parse = |text: &str| ExportRequest::parse(text).err().unwrap_or_default();
    let needs = "export request spot_channels \"keep\" requires format tiff and size none";
    assert_eq!(parse(r#"{"format":"png","spot_channels":"keep"}"#), needs);
    assert_eq!(
        parse(r#"{"format":"tiff","spot_channels":"keep","size":{"mode":"scale","value":50}}"#),
        needs
    );
    assert_eq!(
        parse(r#"{"format":"tiff","spot_channels":"ink"}"#),
        "export spot_channels \"ink\" is not one of overprint or keep"
    );
    let keep = ExportRequest::parse(r#"{"format":"tiff","spot_channels":"keep"}"#).unwrap();
    let cmyk = rgb16()
        .convert_to_cmyk(
            Some(&press_profile("Press Test")),
            r#"{"depth":16}"#,
            LIMIT,
            0,
            None,
        )
        .unwrap();
    assert_eq!(
        cmyk.export_encoded(None, &keep, LIMIT, 0, None).err().unwrap_or_default(),
        "export request spot_channels \"keep\" is not available for a CMYK document; its TIFF save keeps the spot channels"
    );
    let gray = rgb16()
        .convert_to_gray(
            None,
            r#"{"space":"gray_gamma_22","depth":16}"#,
            LIMIT,
            0,
            None,
        )
        .unwrap();
    let gray = with_spots(&gray, &ramp16(), &wide(&bloc8()));
    assert_eq!(
        gray.export_plan(None, &keep).err().unwrap_or_default(),
        TIFF_REFUSAL
    );
    assert_eq!(
        gray.export_encoded(None, &keep, LIMIT, 0, None)
            .err()
            .unwrap_or_default(),
        TIFF_REFUSAL
    );
    let lab = rgb16()
        .convert_to_lab(r#"{"depth":16}"#, LIMIT, 0, None)
        .unwrap();
    let lab = with_spots(&lab, &ramp16(), &wide(&bloc8()));
    let plan = lab.save_lab_plan(None, r#"{"format":"tiff"}"#).unwrap();
    assert_eq!(plan["refusal"], TIFF_REFUSAL);
}

// ------------------------------------------------- hand-built files --

/// A little-endian classic TIFF of chunky uncompressed `samples` with the
/// given entries (tag, SHORT values) and an optional 8BIM run.
fn hand_tiff(
    bits: u16,
    samples: &[u16],
    shorts: &[(u16, Vec<u16>)],
    run: Option<&[u8]>,
) -> Vec<u8> {
    let mut out = b"II*\0\0\0\0\0".to_vec();
    let data_at = out.len() as u32;
    for &v in samples {
        if bits == 16 {
            out.extend_from_slice(&v.to_le_bytes());
        } else {
            out.push(v as u8);
        }
    }
    let data_len = out.len() as u32 - data_at;
    let mut entries: Vec<(u16, u16, u32, Vec<u8>)> = shorts
        .iter()
        .map(|(tag, values)| {
            (
                *tag,
                3,
                values.len() as u32,
                values.iter().flat_map(|v| v.to_le_bytes()).collect(),
            )
        })
        .collect();
    entries.push((256, 4, 1, W.to_le_bytes().to_vec()));
    entries.push((257, 4, 1, H.to_le_bytes().to_vec()));
    entries.push((273, 4, 1, data_at.to_le_bytes().to_vec()));
    entries.push((278, 4, 1, H.to_le_bytes().to_vec()));
    entries.push((279, 4, 1, data_len.to_le_bytes().to_vec()));
    if let Some(run) = run {
        entries.push((34377, 7, run.len() as u32, run.to_vec()));
    }
    entries.sort_by_key(|e| e.0);
    let mut fields = Vec::new();
    for (_, _, _, value) in &mut entries {
        if value.len() > 4 {
            if out.len() % 2 == 1 {
                out.push(0);
            }
            let at = out.len() as u32;
            out.extend_from_slice(value);
            fields.push(at.to_le_bytes().to_vec());
        } else {
            let mut inline = value.clone();
            inline.resize(4, 0);
            fields.push(inline);
        }
    }
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let ifd = out.len() as u32;
    out[4..8].copy_from_slice(&ifd.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for ((tag, kind, count, _), field) in entries.iter().zip(fields) {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&field);
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out
}

/// A TIFF's byte order and where each first-directory entry's value field
/// sits: this file's own walk, for the strip rewrites below.
struct Fields<'a> {
    file: &'a [u8],
    little: bool,
    value_at: HashMap<u16, usize>,
}

impl Fields<'_> {
    fn of(file: &[u8]) -> Fields<'_> {
        let mut fields = Fields {
            file,
            little: &file[..2] == b"II",
            value_at: HashMap::new(),
        };
        let ifd = fields.u32(4) as usize;
        for e in 0..usize::from(fields.u16(ifd)) {
            let at = ifd + 2 + e * 12;
            fields.value_at.insert(fields.u16(at), at + 8);
        }
        fields
    }

    fn u16(&self, at: usize) -> u16 {
        let v = [self.file[at], self.file[at + 1]];
        if self.little {
            u16::from_le_bytes(v)
        } else {
            u16::from_be_bytes(v)
        }
    }

    fn u32(&self, at: usize) -> u32 {
        let v: [u8; 4] = self.file[at..at + 4].try_into().unwrap();
        if self.little {
            u32::from_le_bytes(v)
        } else {
            u32::from_be_bytes(v)
        }
    }

    fn put16(&self, v: u16) -> [u8; 2] {
        if self.little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    }

    fn put32(&self, v: u32) -> [u8; 4] {
        if self.little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    }

    /// The value field of `tag`, which must be one LONG.
    fn one_long(&self, tag: u16) -> usize {
        let at = self.value_at[&tag];
        assert_eq!(
            (self.u16(at - 6), self.u32(at - 4)),
            (4, 1),
            "tag {tag}: one LONG"
        );
        at
    }
}

/// `file` with its one strip copied to its end and StripOffsets pointing
/// at the copy, and the strip's length: a cut of the tail then falls inside
/// the strip and leaves the directory and every other value whole
/// ([`hand_tiff`] and the layered writer both place the strip first).
pub fn with_strip_last(file: &[u8]) -> (Vec<u8>, usize) {
    let fields = Fields::of(file);
    let offset = fields.one_long(273);
    let (start, length) = (
        fields.u32(offset) as usize,
        fields.u32(fields.one_long(279)) as usize,
    );
    let mut out = file.to_vec();
    out[offset..offset + 4].copy_from_slice(&fields.put32(file.len() as u32));
    out.extend_from_slice(&file[start..start + length]);
    (out, length)
}

/// `file`, one uncompressed strip ([`hand_tiff`]'s, or the layered
/// writer's), rewritten as PackBits strips of `rows` rows (literal runs
/// only), the offset and count tables before the strips; and where its last
/// strip starts, so a cut there leaves every other strip whole.
pub fn with_packbits_strips(file: &[u8], rows: u32) -> (Vec<u8>, usize) {
    let height = read_tiff(file).one(257) as usize;
    let fields = Fields::of(file);
    let (offsets, counts) = (fields.one_long(273), fields.one_long(279));
    let start = fields.u32(offsets) as usize;
    let data = &file[start..start + fields.u32(counts) as usize];
    let strips: Vec<Vec<u8>> = data
        .chunks(data.len() / height * rows as usize)
        .map(|strip| {
            strip
                .chunks(128)
                .flat_map(|run| [&[(run.len() - 1) as u8][..], run].concat())
                .collect()
        })
        .collect();
    let n = strips.len();
    let mut out = file.to_vec();
    let tables = out.len();
    out.resize(tables + 8 * n, 0);
    let mut last = 0;
    for (k, strip) in strips.iter().enumerate() {
        last = out.len();
        let (offset_at, count_at) = (tables + 4 * k, tables + 4 * (n + k));
        out[offset_at..offset_at + 4].copy_from_slice(&fields.put32(last as u32));
        out[count_at..count_at + 4].copy_from_slice(&fields.put32(strip.len() as u32));
        out.extend_from_slice(strip);
    }
    // StripOffsets and StripByteCounts become the two tables: count, then
    // the table's offset.
    for (field, table) in [(offsets, tables), (counts, tables + 4 * n)] {
        out[field - 4..field].copy_from_slice(&fields.put32(n as u32));
        out[field..field + 4].copy_from_slice(&fields.put32(table as u32));
    }
    let rows_at = fields.one_long(278);
    out[rows_at..rows_at + 4].copy_from_slice(&fields.put32(rows));
    let compression = fields.value_at[&259];
    out[compression..compression + 2].copy_from_slice(&fields.put16(32773));
    (out, last)
}

/// `file` (a [`with_packbits_strips`] rewrite) with its last StripByteCounts
/// entry `by` bytes short, so that strip's stream ends before its rows do.
pub fn with_last_count_short(file: &[u8], by: u32) -> Vec<u8> {
    let fields = Fields::of(file);
    let field = fields.value_at[&279];
    let n = fields.u32(field - 4) as usize;
    let at = fields.u32(field) as usize + 4 * (n - 1);
    let mut out = file.to_vec();
    out[at..at + 4].copy_from_slice(&fields.put32(fields.u32(at) - by));
    out
}

/// The loss line of a TIFF whose spot planes the file cuts short.
pub const CUT_SHORT: &str = "Spot channel planes in this file are not readable (the file ends inside its image data); no spot channel is retained.";

/// An RGB file's entries: `bits` on every sample, uncompressed, chunky, and
/// `extra` the ExtraSamples after the three colour samples.
fn rgb_shape(bits: u16, extra: Vec<u16>) -> Vec<(u16, Vec<u16>)> {
    let n = 3 + extra.len();
    vec![
        (258, vec![bits; n]),
        (259, vec![1]),
        (262, vec![2]),
        (277, vec![n as u16]),
        (284, vec![1]),
        (338, extra),
    ]
}

fn rgb_with_spots(bits: u16) -> Vec<u16> {
    let (ramp, bloc) = (ramp8(), bloc8());
    let scale = if bits == 16 { 257 } else { 1 };
    (0..(W * H) as usize)
        .flat_map(|p| {
            [
                (p as u16 % 200) * scale,
                90 * scale,
                30 * scale,
                (255 - u16::from(ramp[p])) * scale,
                (255 - u16::from(bloc[p])) * scale,
            ]
        })
        .collect()
}

#[test]
fn hand_built_files_keep_their_spot_channels_or_say_why_not() {
    let dir = TempDir::new().unwrap();
    let spots = [
        (RAMP, spot_display(ramp_ink())),
        (BLOC, spot_display(bloc_ink())),
    ];
    let run = channel_run(&spots, false);
    let shape = rgb_shape;
    // Photoshop's layout, little-endian here (its own files are `MM`,
    // `spot_tiff_photoshop.rs`), opens with its spot channels at both depths.
    for bits in [8u16, 16] {
        let path = dir.path().join(format!("hand{bits}.tif"));
        std::fs::write(
            &path,
            hand_tiff(
                bits,
                &rgb_with_spots(bits),
                &shape(bits, vec![0, 0]),
                Some(&run),
            ),
        )
        .unwrap();
        let back = open_byte(&path);
        let want = vec![
            (RAMP.to_string(), Some(ramp_ink()), ramp8()),
            (BLOC.to_string(), Some(bloc_ink()), bloc8()),
        ];
        assert_eq!(byte_spots(&back), want, "{bits}: byte open");
        if bits == 16 {
            let typed = open_typed(&path).unwrap();
            let want: Vec<_> = want.into_iter().map(|(n, i, p)| (n, i, wide(&p))).collect();
            assert_eq!(typed_spots(&typed), want, "typed open");
        }
    }
    // Rotated: nothing kept, the sentence says why.
    let mut rotated = shape(8, vec![0, 0]);
    rotated.push((274, vec![6]));
    let path = dir.path().join("rotated.tif");
    std::fs::write(
        &path,
        hand_tiff(8, &rgb_with_spots(8), &rotated, Some(&run)),
    )
    .unwrap();
    assert!(report_warnings(&path).contains(
        &"Spot channels in this TIFF are not read: it is stored rotated (Orientation 6)."
            .to_string()
    ));
    assert!(open_byte(&path).channels.is_empty());
    // Planar: the report alone (it decodes nothing).
    let mut planar = shape(8, vec![0, 0]);
    planar.retain(|(tag, _)| *tag != 284);
    planar.push((284, vec![2]));
    let path = dir.path().join("planar.tif");
    std::fs::write(&path, hand_tiff(8, &rgb_with_spots(8), &planar, Some(&run))).unwrap();
    assert!(report_warnings(&path)
        .contains(&"Spot channels in this TIFF are not read: its samples are planar.".to_string()));
    // A 16-bit RGB file whose extra sample no resource explains still
    // refuses, as before 1c-2a.
    let one: Vec<u16> = rgb_with_spots(16)
        .chunks_exact(5)
        .flat_map(|p| p[..4].to_vec())
        .collect();
    let path = dir.path().join("unexplained.tif");
    std::fs::write(&path, hand_tiff(16, &one, &shape(16, vec![0]), None)).unwrap();
    let error = open_typed(&path).err().unwrap_or_default();
    assert!(
        error.contains("RGBA TIFF must declare one unassociated alpha sample"),
        "{error}"
    );
}

#[test]
fn a_tiff_cut_short_inside_its_image_data_reports_its_spot_channels_unread() {
    let dir = TempDir::new().unwrap();
    let spots = [
        (RAMP, spot_display(ramp_ink())),
        (BLOC, spot_display(bloc_ink())),
    ];
    let run = channel_run(&spots, false);
    let both = vec![
        (RAMP.to_string(), Some(ramp_ink()), ramp8()),
        (BLOC.to_string(), Some(bloc_ink()), bloc8()),
    ];
    for bits in [8u16, 16] {
        let file = hand_tiff(
            bits,
            &rgb_with_spots(bits),
            &rgb_shape(bits, vec![0, 0]),
            Some(&run),
        );
        // The decoder never reads an uncompressed strip's byte count, so a
        // count that understates the strip loses nothing.
        let mut understated = file.clone();
        let fields = Fields::of(&file);
        let count = fields.one_long(279);
        understated[count..count + 4].copy_from_slice(&fields.put32(1));
        let path = dir.path().join(format!("understated{bits}.tif"));
        std::fs::write(&path, &understated).unwrap();
        assert!(report_warnings(&path).contains(&observation()), "{bits}");
        assert_eq!(byte_spots(&open_byte(&path)), both, "{bits}: understated");
        // Each rewrite, whole, still opens with its spot channels, and its
        // report says so; cut short, the report names the loss instead.
        let (moved, strip) = with_strip_last(&file);
        let inside = moved.len() - strip / 2;
        let (packed, last) = with_packbits_strips(&file, 4);
        let cuts = [("uncompressed", moved, inside), ("packbits", packed, last)];
        for (kind, whole, cut) in cuts {
            let what = format!("{bits}-bit {kind}");
            let path = dir.path().join(format!("whole{bits}{kind}.tif"));
            std::fs::write(&path, &whole).unwrap();
            assert!(report_warnings(&path).contains(&observation()), "{what}");
            assert_eq!(byte_spots(&open_byte(&path)), both, "{what}: whole");
            let path = dir.path().join(format!("cut{bits}{kind}.tif"));
            std::fs::write(&path, &whole[..cut]).unwrap();
            let warnings = report_warnings(&path);
            assert!(
                warnings.contains(&CUT_SHORT.to_string()),
                "{what}: {warnings:?}"
            );
            assert!(!warnings.contains(&observation()), "{what}: {warnings:?}");
            // The byte open decodes the same strips first, so the cut file
            // does not open at all.
            assert!(try_open_byte(&path).is_err(), "{what}: byte open");
            if bits == 16 {
                // The typed open admits the file by its description and fails
                // at the decode with the decoder's own error, not a refusal
                // of samples the file does declare.
                let error = open_typed(&path).err().unwrap_or_default();
                assert!(!error.is_empty(), "{what}: typed open");
                assert!(
                    !error.contains("RGBA TIFF must declare one unassociated alpha sample"),
                    "{what}: {error}"
                );
            }
        }
    }
}
