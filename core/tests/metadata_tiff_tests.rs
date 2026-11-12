//! TIFF fixtures and readers authored independently of the production TIFF
//! crate. Exercise all document operations through the FFI, then also ask
//! libtiff/ImageMagick to decode output when installed. No golden images.

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_color::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_metadata_report::dr_path_metadata_import_report;
use doctored_core::ffi_preservation::dr_doc_flat_save_losses;
use doctored_core::icc::IccProfile;
use std::ffi::CStr;
use std::path::Path;
use std::process::Command;
use std::ptr;
use tempfile::TempDir;

use crate::common;
use common::metadata_fixtures::{iptc_run, png_chunk_bytes, png_with, IIM};
use common::{cpath, flat_pixels, take_err_string};

/// A classic TIFF (8-byte header, u16 IFD counts, 12-byte entries, 4-byte
/// value fields and links) or, with `big`, a BigTIFF (16-byte header, u64
/// counts, 20-byte entries, 8-byte value fields and links).
struct Builder {
    bytes: Vec<u8>,
    be: bool,
    big: bool,
}
type Field = (u16, u16, u32, Vec<u8>);
impl Builder {
    fn new(be: bool) -> Self {
        Self::with(be, false)
    }
    fn with(be: bool, big: bool) -> Self {
        let mut b = Self {
            bytes: if be { b"MM".to_vec() } else { b"II".to_vec() },
            be,
            big,
        };
        let version = b.short(if big { 43 } else { 42 });
        b.bytes.extend(version);
        if big {
            let (size, reserved) = (b.short(8), b.short(0));
            b.bytes.extend(size);
            b.bytes.extend(reserved);
            b.bytes.extend([0; 8]);
        } else {
            b.bytes.extend([0; 4]);
        }
        b
    }
    fn short(&self, v: u16) -> Vec<u8> {
        if self.be {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
        .to_vec()
    }
    fn long(&self, v: u32) -> Vec<u8> {
        if self.be {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
        .to_vec()
    }
    fn long8(&self, v: u64) -> Vec<u8> {
        if self.be {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
        .to_vec()
    }
    fn rational(&self, n: u32, d: u32) -> Vec<u8> {
        [self.long(n), self.long(d)].concat()
    }
    /// An offset in this stream's width: LONG classic, u64 BigTIFF.
    fn offset(&self, v: u32) -> Vec<u8> {
        if self.big {
            self.long8(u64::from(v))
        } else {
            self.long(v)
        }
    }
    /// A child-IFD pointer field: LONG classic, IFD8 (type 18) BigTIFF.
    fn pointer(&self, tag: u16, offset: u32) -> Field {
        if self.big {
            (tag, 18, 1, self.long8(u64::from(offset)))
        } else {
            (tag, 4, 1, self.long(offset))
        }
    }
    fn ifd(&mut self, mut fields: Vec<Field>) -> u32 {
        fields.sort_by_key(|f| f.0);
        if !self.bytes.len().is_multiple_of(2) {
            self.bytes.push(0);
        }
        // (count width, entry size, value-field position, field width)
        let (head, size, value, width) = if self.big {
            (8, 20, 12, 8)
        } else {
            (2, 12, 8, 4)
        };
        let at = self.bytes.len();
        let count = if self.big {
            self.long8(fields.len() as u64)
        } else {
            self.short(fields.len() as u16)
        };
        self.bytes.extend(count);
        self.bytes
            .resize(at + head + size * fields.len() + width, 0);
        for (i, (tag, kind, count, data)) in fields.into_iter().enumerate() {
            let e = at + head + size * i;
            let tag = self.short(tag);
            let kind = self.short(kind);
            let count = if self.big {
                self.long8(u64::from(count))
            } else {
                self.long(count)
            };
            self.bytes[e..e + 2].copy_from_slice(&tag);
            self.bytes[e + 2..e + 4].copy_from_slice(&kind);
            self.bytes[e + 4..e + 4 + count.len()].copy_from_slice(&count);
            let field = e + value;
            if data.len() <= width {
                self.bytes[field..field + data.len()].copy_from_slice(&data);
            } else {
                if !self.bytes.len().is_multiple_of(2) {
                    self.bytes.push(0);
                }
                let offset = self.offset(self.bytes.len() as u32);
                self.bytes[field..field + width].copy_from_slice(&offset);
                self.bytes.extend(data);
            }
        }
        at as u32
    }
    fn root(&mut self, fields: Vec<Field>) {
        let offset = self.ifd(fields);
        let bytes = self.offset(offset);
        let at = if self.big { 8 } else { 4 };
        self.bytes[at..at + bytes.len()].copy_from_slice(&bytes);
    }
}

const XMP: &[u8] = b"<x:xmpmeta xmlns:x='adobe:ns:meta/' xmlns:tiff='http://ns.adobe.com/tiff/1.0/'><tiff:Orientation>6</tiff:Orientation><tiff:XResolution>300/1</tiff:XResolution><tiff:YResolution>150/1</tiff:YResolution><tiff:ResolutionUnit>2</tiff:ResolutionUnit></x:xmpmeta>";

fn fixture(be: bool, big: bool, maker: bool, late: bool, raw_iptc: bool) -> Vec<u8> {
    let mut b = Builder::with(be, big);
    let interop = b.ifd(vec![
        (1, 2, 4, b"R98\0".to_vec()),
        (2, 7, 4, b"0100".to_vec()),
    ]);
    let mut exif = vec![
        (33434, 5, 1, b.rational(1, 125)),
        (36867, 2, 20, b"2026:08:24 12:34:56\0".to_vec()),
        (40961, 3, 1, b.short(1)),
        (40962, 4, 1, b.long(2)),
        (40963, 4, 1, b.long(3)),
        b.pointer(40965, interop),
    ];
    if maker {
        exif.push((37500, 7, 12, b"private-note".to_vec()));
    }
    let exif = b.ifd(exif);
    let gps = b.ifd(vec![
        (1, 2, 2, b"N\0".to_vec()),
        (
            2,
            5,
            3,
            [b.rational(42, 1), b.rational(30, 1), b.rational(0, 1)].concat(),
        ),
    ]);
    let strip = b.bytes.len() as u32;
    // Unique opaque RGB values make rotation observable independently.
    b.bytes.extend([
        11, 21, 31, 41, 51, 61, 71, 81, 91, 101, 111, 121, 131, 141, 151, 161, 171, 181,
    ]);
    if late {
        b.bytes.resize(17 * 1024 * 1024, 0);
    }
    let mut fields = vec![
        (256, 4, 1, b.long(2)),
        (257, 4, 1, b.long(3)),
        (258, 3, 3, [b.short(8), b.short(8), b.short(8)].concat()),
        (259, 3, 1, b.short(1)),
        (262, 3, 1, b.short(2)),
        (271, 2, 10, b"TestMaker\0".to_vec()),
        (272, 2, 8, b"Camera3\0".to_vec()),
        (273, 4, 1, b.long(strip)),
        (274, 3, 1, b.short(6)),
        (277, 3, 1, b.short(3)),
        (278, 4, 1, b.long(3)),
        (279, 4, 1, b.long(18)),
        (282, 5, 1, b.rational(300, 1)),
        (283, 5, 1, b.rational(150, 1)),
        (284, 3, 1, b.short(1)),
        (296, 3, 1, b.short(2)),
        b.pointer(34665, exif),
        b.pointer(34853, gps),
        (
            34675,
            7,
            IccProfile::display_p3().bytes().len() as u32,
            IccProfile::display_p3().bytes().to_vec(),
        ),
        (700, 1, XMP.len() as u32, XMP.to_vec()),
    ];
    if raw_iptc {
        fields.push((33723, 1, IIM.len() as u32, IIM.to_vec()));
    } else {
        let run = iptc_run();
        fields.push((34377, 1, run.len() as u32, run));
    }
    b.root(fields);
    b.bytes
}

// Independent byte-stream reader. Unlike the production code it has no
// relocation, filtering, caps, or TIFF encoder dependencies.
struct Read<'a> {
    bytes: &'a [u8],
    be: bool,
}
impl<'a> Read<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            be: &bytes[..2] == b"MM",
        }
    }
    fn u16(&self, at: usize) -> u16 {
        let a = self.bytes[at..at + 2].try_into().unwrap();
        if self.be {
            u16::from_be_bytes(a)
        } else {
            u16::from_le_bytes(a)
        }
    }
    fn u32(&self, at: usize) -> u32 {
        let a = self.bytes[at..at + 4].try_into().unwrap();
        if self.be {
            u32::from_be_bytes(a)
        } else {
            u32::from_le_bytes(a)
        }
    }
    fn root(&self) -> usize {
        self.u32(4) as usize
    }
    fn entry(&self, ifd: usize, tag: u16) -> Option<usize> {
        (0..usize::from(self.u16(ifd)))
            .map(|i| ifd + 2 + 12 * i)
            .find(|&at| self.u16(at) == tag)
    }
    fn data(&self, ifd: usize, tag: u16) -> Option<&'a [u8]> {
        let at = self.entry(ifd, tag)?;
        let width = match self.u16(at + 2) {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 => 4,
            5 | 10 | 12 => 8,
            _ => panic!("unknown fixture type"),
        };
        let n = self.u32(at + 4) as usize * width;
        let offset = if n <= 4 {
            at + 8
        } else {
            self.u32(at + 8) as usize
        };
        Some(&self.bytes[offset..offset + n])
    }
    fn integer(&self, ifd: usize, tag: u16) -> u32 {
        let at = self.entry(ifd, tag).unwrap();
        if self.u16(at + 2) == 3 {
            u32::from(self.u16(at + 8))
        } else {
            self.u32(at + 8)
        }
    }
    fn rational(&self, ifd: usize, tag: u16) -> f64 {
        let at = self.entry(ifd, tag).unwrap();
        let offset = self.u32(at + 8) as usize;
        f64::from(self.u32(offset)) / f64::from(self.u32(offset + 4))
    }
}

fn open(path: &Path) -> *mut DrDocument {
    let path = cpath(path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_doc_open(path.as_ptr(), std::ptr::null(), &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}
fn save(doc: *const DrDocument, path: &Path, strip: bool) -> u32 {
    let path = cpath(path);
    let mut err = ptr::null_mut();
    let mut carried = 0;
    assert!(
        unsafe {
            dr_doc_save_image(
                doc,
                ptr::null(),
                path.as_ptr(),
                2,
                100,
                true,
                strip,
                &mut carried,
                std::ptr::null(),
                &mut err,
            )
        },
        "{}",
        take_err_string(err)
    );
    carried
}
fn meta(doc: *const DrDocument, kind: i32) -> Vec<u8> {
    let size = unsafe { dr_doc_metadata_len(doc, kind) };
    let mut bytes = vec![0; size];
    assert!(unsafe { dr_doc_metadata(doc, kind, bytes.as_mut_ptr(), size) });
    bytes
}
fn report(path: &Path) -> serde_json::Value {
    let path = cpath(path);
    let mut err = ptr::null_mut();
    let json = unsafe { dr_path_metadata_import_report(path.as_ptr(), &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(json) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(json) };
    value
}

#[test]
fn classic_and_big_tiff_both_orders_relocate_capture_and_normalize_orientation() {
    for big in [false, true] {
        for be in [false, true] {
            for raw_iptc in [false, true] {
                let dir = TempDir::new().unwrap();
                let source = dir.path().join("source.tif");
                std::fs::write(&source, fixture(be, big, false, false, raw_iptc)).unwrap();
                let doc = open(&source);
                assert_eq!(unsafe { (dr_doc_width(doc), dr_doc_height(doc)) }, (3, 2));
                assert_eq!(
                    unsafe { (dr_doc_resolution_x(doc), dr_doc_resolution_y(doc)) },
                    (150.0, 300.0)
                );
                let rgb = [
                    131, 141, 151, 71, 81, 91, 11, 21, 31, 161, 171, 181, 101, 111, 121, 41, 51, 61,
                ];
                let expected: Vec<u8> = rgb
                    .chunks_exact(3)
                    .flat_map(|p| [p[0], p[1], p[2], 255])
                    .collect();
                assert_eq!(flat_pixels(doc), expected);
                let profile = IccProfile::display_p3();
                let len = unsafe { dr_doc_icc_profile_len(doc) };
                let mut icc = vec![0; len];
                assert!(unsafe { dr_doc_icc_profile(doc, icc.as_mut_ptr(), len) });
                assert_eq!(icc, profile.bytes().as_ref());
                assert_eq!(meta(doc, 2), iptc_run());
                assert_eq!(meta(doc, 1), XMP);
                let packet = meta(doc, 0);
                let exif = Read::new(&packet);
                assert_eq!(exif.data(exif.root(), 271), Some(b"TestMaker\0".as_slice()));
                assert!(exif.entry(exif.root(), 273).is_none());
                let path = dir.path().join("out.tif");
                assert_eq!(save(doc, &path, false), 31);
                let bytes = std::fs::read(&path).unwrap();
                let t = Read::new(&bytes);
                assert_eq!(t.u16(2), 42, "an opened BigTIFF saves as classic TIFF");
                let root = t.root();
                assert_eq!(t.integer(root, 274), 1);
                assert_eq!(t.rational(root, 282), 150.0);
                assert_eq!(t.rational(root, 283), 300.0);
                assert_eq!(t.integer(root, 296), 2);
                let sub = t.integer(root, 34665) as usize;
                assert_eq!(t.rational(sub, 33434), 1.0 / 125.0);
                assert_eq!(t.integer(sub, 40961), 65535);
                assert_eq!(t.integer(sub, 40962), 3);
                assert_eq!(t.integer(sub, 40963), 2);
                assert_eq!(
                    t.data(sub, 36867),
                    Some(b"2026:08:24 12:34:56\0".as_slice())
                );
                let interop = t.integer(sub, 40965) as usize;
                assert_eq!(t.data(interop, 1), Some(b"R98\0".as_slice()));
                let gps = t.integer(root, 34853) as usize;
                assert_eq!(t.data(gps, 1), Some(b"N\0".as_slice()));
                assert_eq!(t.rational(gps, 2), 42.0);
                let xmp = std::str::from_utf8(t.data(root, 700).unwrap()).unwrap();
                assert!(xmp.contains("<tiff:Orientation>1</tiff:Orientation>"));
                let back = open(&path);
                assert_eq!(flat_pixels(back), expected);
                assert_eq!(meta(back, 2), iptc_run());
                let source_report = report(&source);
                assert_eq!(source_report["warnings"], serde_json::json!([]));
                assert_eq!(source_report["container"], "tiff");
                assert_eq!(
                    source_report["resolution"],
                    serde_json::json!({"x":150.0,"y":300.0})
                );
                unsafe {
                    dr_doc_free(back);
                    dr_doc_free(doc)
                };
            }
        }
    }
}

#[test]
fn metadata_at_large_file_offsets_is_compacted_under_packet_cap() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("late.tif");
    std::fs::write(&path, fixture(false, false, false, true, false)).unwrap();
    let doc = open(&path);
    let packet = meta(doc, 0);
    assert!(packet.len() < 4096);
    let t = Read::new(&packet);
    assert_eq!(t.data(t.root(), 271), Some(b"TestMaker\0".as_slice()));
    unsafe { dr_doc_free(doc) };
}

/// The warning a dropped private capture field gets.
const PRIVATE_WARNING: &str = "TIFF private capture fields were not retained";

#[test]
fn a_maker_note_survives_tiff_open_and_tiff_export_as_one_opaque_value() {
    // The MakerNote relocates byte for byte, in either byte order, and
    // nothing is disclosed as lost.
    for be in [false, true] {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("maker.tif");
        std::fs::write(&path, fixture(be, false, true, false, false)).unwrap();
        assert!(
            !warnings(&path).iter().any(|w| w.contains("private")),
            "{:?}",
            warnings(&path)
        );
        let doc = open(&path);
        let exif = meta(doc, 0);
        let t = Read::new(&exif);
        let sub = t.integer(t.root(), 34665) as usize;
        assert_eq!(t.data(sub, 37500), Some(b"private-note".as_slice()));
        assert_eq!(t.u16(t.entry(sub, 37500).unwrap() + 2), 7, "UNDEFINED");
        let output = dir.path().join("out.tif");
        let carried = save(doc, &output, false);
        assert_eq!(carried & 2, 2, "the EXIF packet is written");
        let bytes = std::fs::read(&output).unwrap();
        let t = Read::new(&bytes);
        let sub = t.integer(t.root(), 34665) as usize;
        assert_eq!(t.data(sub, 37500), Some(b"private-note".as_slice()));
        // The written file opens with the note again.
        let back = open(&output);
        let exif = meta(back, 0);
        let t = Read::new(&exif);
        let sub = t.integer(t.root(), 34665) as usize;
        assert_eq!(t.data(sub, 37500), Some(b"private-note".as_slice()));
        unsafe {
            dr_doc_free(back);
            dr_doc_free(doc)
        };
    }
}

#[test]
fn other_private_fields_are_disclosed_on_import_and_refuse_entire_tiff_exif_export() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("private.tif");
    // The fixture's MakerNote entry renumbered to 37499, a tag no standard
    // defines: it is private, so it is dropped and disclosed.
    let mut bytes = fixture(false, false, true, false, false);
    let t = Read::new(&bytes);
    let at = t.entry(t.integer(t.root(), 34665) as usize, 37500).unwrap();
    bytes[at..at + 2].copy_from_slice(&37499u16.to_le_bytes());
    std::fs::write(&path, bytes).unwrap();
    assert!(warnings(&path).iter().any(|w| w == PRIVATE_WARNING));
    let doc = open(&path);
    let exif = meta(doc, 0);
    let t = Read::new(&exif);
    let sub = t.integer(t.root(), 34665) as usize;
    assert!(t.entry(sub, 37499).is_none());
    assert!(t.entry(sub, 37500).is_none());
    // Assign a standalone packet with a private field, and one whose
    // MakerNote is not a byte array. Other formats keep such a packet's
    // original offsets; TIFF cannot relocate either, so it refuses the packet.
    for (tag, kind, count) in [(37499u16, 7u16, 12u32), (37500, 4, 3)] {
        let mut b = Builder::new(false);
        let sub = b.ifd(vec![(tag, kind, count, b"private-note".to_vec())]);
        b.root(vec![(34665, 4, 1, b.long(sub))]);
        let changed = unsafe {
            dr_doc_set_metadata(doc, 0, b.bytes.as_ptr(), b.bytes.len(), ptr::null_mut())
        };
        assert!(!changed.is_null());
        let output = dir.path().join(format!("out-{tag}-{kind}.tif"));
        let carried = save(changed, &output, false);
        assert_eq!(carried & 2, 0, "{tag} type {kind}");
        assert_eq!(carried & 29, 29);
        let mut err = ptr::null_mut();
        let losses = unsafe { dr_doc_flat_save_losses(changed, 2, true, false, &mut err) };
        assert!(!losses.is_null());
        assert!(unsafe { CStr::from_ptr(losses) }
            .to_str()
            .unwrap()
            .contains("EXIF metadata"));
        unsafe {
            dr_string_free(losses);
            dr_doc_free(changed);
        };
    }
    unsafe { dr_doc_free(doc) };
}

#[test]
fn malformed_cycle_and_oversize_capture_refuse_without_losing_pixels() {
    let dir = TempDir::new().unwrap();
    for mode in ["cycle", "oversize"] {
        let mut bytes = fixture(false, false, false, false, false);
        let t = Read::new(&bytes);
        let at = t.entry(t.root(), 34665).unwrap();
        let root = t.root() as u32;
        if mode == "cycle" {
            bytes[at + 8..at + 12].copy_from_slice(&root.to_le_bytes());
        } else {
            let t = Read::new(&bytes);
            let e = t.entry(t.root(), 700).unwrap();
            let offset = bytes.len() as u32;
            bytes[e + 4..e + 8].copy_from_slice(&(16 * 1024 * 1024u32 + 1).to_le_bytes());
            bytes[e + 8..e + 12].copy_from_slice(&offset.to_le_bytes());
            bytes.resize(bytes.len() + 16 * 1024 * 1024 + 1, b'x');
        }
        let path = dir.path().join(format!("{mode}.tif"));
        std::fs::write(&path, bytes).unwrap();
        let doc = open(&path);
        assert_eq!(flat_pixels(doc).len(), 24);
        assert!(!report(&path)["warnings"].as_array().unwrap().is_empty());
        assert_eq!(
            unsafe { dr_doc_metadata_len(doc, if mode == "cycle" { 0 } else { 1 }) },
            0
        );
        unsafe { dr_doc_free(doc) };
    }
}

#[test]
fn strip_metadata_keeps_profile_and_fractional_document_ppi_in_independent_readers() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("source.tif");
    std::fs::write(&path, fixture(false, false, false, false, false)).unwrap();
    let doc = open(&path);
    let changed = unsafe { dr_doc_set_resolution(doc, 300.0, 150.125, ptr::null_mut()) };
    assert!(!changed.is_null());
    let output = dir.path().join("print.tif");
    assert_eq!(save(changed, &output, true), 17);
    let bytes = std::fs::read(&output).unwrap();
    let t = Read::new(&bytes);
    assert!(t.entry(t.root(), 34665).is_none());
    assert!(t.entry(t.root(), 700).is_none());
    assert!(t.entry(t.root(), 34377).is_none());
    assert_eq!(t.rational(t.root(), 282), 300.0);
    assert_eq!(t.rational(t.root(), 283), 150.125);
    let back = open(&output);
    assert_eq!(unsafe { dr_doc_resolution_y(back) }, 150.125);
    if let Ok(result) = Command::new("tiffinfo").arg("-D").arg(&output).output() {
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let report = String::from_utf8_lossy(&result.stdout);
        assert!(report.contains("300, 150.125"), "{report}");
        assert!(report.contains("ICC Profile"), "{report}");
    } else {
        eprintln!("tiffinfo absent: skipping independent libtiff decode");
    }
    if let Ok(result) = Command::new("magick")
        .args(["identify", "-format", "%[icc:description]|%x|%y"])
        .arg(&output)
        .output()
    {
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let text = String::from_utf8_lossy(&result.stdout);
        assert!(text.contains("Display P3"), "{text}");
        assert!(text.contains("300|150.125"), "{text}");
    } else {
        eprintln!("ImageMagick absent: skipping independent profile/density check");
    }
    unsafe {
        dr_doc_free(back);
        dr_doc_free(changed);
        dr_doc_free(doc)
    };
}

#[test]
fn straight_alpha_is_declared_and_decodes_in_an_independent_reader() {
    let dir = TempDir::new().unwrap();
    let expected = vec![120, 60, 200, 64, 30, 90, 210, 192];
    let doc = DrDocument::from_pixels(image::RgbaImage::from_raw(2, 1, expected.clone()).unwrap());
    let path = dir.path().join("alpha.tif");
    save(&doc, &path, false);
    let bytes = std::fs::read(&path).unwrap();
    let tags = Read::new(&bytes);
    assert_eq!(tags.integer(tags.root(), 338), 2, "unassociated alpha");
    assert_eq!(tags.integer(tags.root(), 277), 4, "exactly four samples");
    let reopened = open(&path);
    assert_eq!(flat_pixels(reopened), expected);
    unsafe { dr_doc_free(reopened) };
    if let Ok(result) = Command::new("magick")
        .arg(&path)
        .args(["-depth", "8", "rgba:-"])
        .output()
    {
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(result.stdout, expected, "independent straight RGBA pixels");
    } else {
        eprintln!("ImageMagick absent: skipping independent alpha decode");
    }
}

#[test]
fn metadata_report_reads_upright_raw_density_without_a_pixel_decoder() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("density-only.dng");
    for be in [false, true] {
        let mut b = Builder::new(be);
        // DNG-flavoured TIFF metadata with no dimensions, strips or image.
        // Any successful density result therefore came from the scanner.
        b.root(vec![
            (274, 3, 1, b.short(6)),
            (282, 5, 1, b.rational(300, 1)),
            (283, 5, 1, b.rational(150, 1)),
            (296, 3, 1, b.short(2)),
            (50706, 1, 4, vec![1, 4, 0, 0]),
        ]);
        std::fs::write(&path, &b.bytes).unwrap();
        assert_eq!(
            report(&path)["resolution"],
            serde_json::json!({"x":150.0,"y":300.0})
        );
        let t = Read::new(&b.bytes);
        let at = t.entry(t.root(), 296).unwrap() + 8;
        let unit = b.short(1);
        b.bytes[at..at + 2].copy_from_slice(&unit);
        std::fs::write(&path, b.bytes).unwrap();
        assert!(
            report(&path).get("resolution").is_none(),
            "aspect ratio is not PPI"
        );
    }
}

#[test]
fn import_report_null_and_missing_path_sweep() {
    unsafe {
        assert!(dr_path_metadata_import_report(ptr::null(), ptr::null_mut()).is_null());
        let mut err = ptr::null_mut();
        assert!(dr_path_metadata_import_report(ptr::null(), &mut err).is_null());
        assert!(take_err_string(err).contains("NULL"));
    }
    let path = cpath(Path::new("/does-not-exist/metadata.tif"));
    let mut err = ptr::null_mut();
    assert!(unsafe { dr_path_metadata_import_report(path.as_ptr(), &mut err) }.is_null());
    assert!(take_err_string(err).contains("/does-not-exist/metadata.tif"));
}

/// The byte offset of `tag`'s entry in a little-endian BigTIFF IFD — an
/// independent locator for the BigTIFF fixtures below.
fn big_entry(bytes: &[u8], ifd: usize, tag: u16) -> usize {
    let count = u64::from_le_bytes(bytes[ifd..ifd + 8].try_into().unwrap()) as usize;
    (0..count)
        .map(|i| ifd + 8 + 20 * i)
        .find(|&at| u16::from_le_bytes([bytes[at], bytes[at + 1]]) == tag)
        .unwrap()
}

fn big_root(bytes: &[u8]) -> usize {
    u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize
}

fn warnings(path: &Path) -> Vec<String> {
    report(path)["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_bigtiff_maker_note_is_kept_in_the_rebuilt_packet() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("maker-big.tif");
    std::fs::write(&path, fixture(false, true, true, false, false)).unwrap();
    assert!(!warnings(&path).iter().any(|w| w.contains("private")));
    let doc = open(&path);
    let exif = meta(doc, 0);
    let t = Read::new(&exif);
    assert_eq!(t.u16(2), 42, "the rebuilt EXIF packet is classic");
    assert_eq!(
        t.data(t.integer(t.root(), 34665) as usize, 37500),
        Some(b"private-note".as_slice())
    );
    unsafe { dr_doc_free(doc) };
}

#[test]
fn malformed_bigtiff_capture_refuses_without_losing_pixels() {
    let dir = TempDir::new().unwrap();
    for mode in ["past_eof", "max", "count", "cycle"] {
        let mut bytes = fixture(false, true, false, false, false);
        let root = big_root(&bytes);
        let at = big_entry(&bytes, root, 34665) + 12;
        let exif = u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()) as usize;
        match mode {
            "past_eof" => {
                let past = bytes.len() as u64 + 4096;
                bytes[at..at + 8].copy_from_slice(&past.to_le_bytes());
            }
            "max" => bytes[at..at + 8].copy_from_slice(&u64::MAX.to_le_bytes()),
            "count" => bytes[exif..exif + 8].copy_from_slice(&1001u64.to_le_bytes()),
            _ => bytes[at..at + 8].copy_from_slice(&(root as u64).to_le_bytes()),
        }
        let path = dir.path().join(format!("{mode}.tif"));
        std::fs::write(&path, bytes).unwrap();
        let doc = open(&path);
        assert_eq!(flat_pixels(doc).len(), 24, "{mode}");
        assert!(!warnings(&path).is_empty(), "{mode}");
        assert_eq!(unsafe { dr_doc_metadata_len(doc, 0) }, 0, "{mode}");
        unsafe { dr_doc_free(doc) };
    }
}

#[test]
fn bigtiff_ifd0_past_eof_reports_a_malformed_header() {
    let dir = TempDir::new().unwrap();
    let mut bytes = fixture(false, true, false, false, false);
    let past = bytes.len() as u64 + 64;
    bytes[8..16].copy_from_slice(&past.to_le_bytes());
    let path = dir.path().join("header.tif");
    std::fs::write(&path, bytes).unwrap();
    assert!(warnings(&path)
        .iter()
        .any(|w| w == "TIFF metadata header is malformed"));
}

#[test]
fn a_bigtiff_headed_png_exif_blob_is_not_read() {
    // EXIF blobs are always classic: a BigTIFF-shaped eXIf states nothing.
    let mut b = Builder::with(false, true);
    b.root(vec![
        (274, 3, 1, b.short(6)),
        (282, 5, 1, b.rational(300, 1)),
        (283, 5, 1, b.rational(300, 1)),
        (296, 3, 1, b.short(2)),
    ]);
    let pixels = image::RgbaImage::from_fn(3, 2, |x, y| {
        image::Rgba([(x * 40) as u8, (y * 90) as u8, 7, 255])
    });
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("big-exif.png");
    std::fs::write(
        &path,
        png_with(&pixels, &[png_chunk_bytes(b"eXIf", &b.bytes)]),
    )
    .unwrap();
    let doc = open(&path);
    assert_eq!(unsafe { (dr_doc_width(doc), dr_doc_height(doc)) }, (3, 2));
    assert_eq!(
        unsafe { (dr_doc_resolution_x(doc), dr_doc_resolution_y(doc)) },
        (72.0, 72.0)
    );
    assert_eq!(flat_pixels(doc), pixels.into_raw());
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_classic_type_13_exif_pointer_is_still_refused() {
    let dir = TempDir::new().unwrap();
    let mut bytes = fixture(false, false, false, false, false);
    let t = Read::new(&bytes);
    let at = t.entry(t.root(), 34665).unwrap();
    bytes[at + 2..at + 4].copy_from_slice(&13u16.to_le_bytes());
    let path = dir.path().join("ifd13.tif");
    std::fs::write(&path, bytes).unwrap();
    assert!(warnings(&path)
        .iter()
        .any(|w| w == "TIFF EXIF is malformed or exceeds the metadata limits"));
    let doc = open(&path);
    assert_eq!(unsafe { dr_doc_metadata_len(doc, 0) }, 0);
    unsafe { dr_doc_free(doc) };
}
