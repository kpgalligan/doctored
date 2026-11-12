//! File Info through the C API, at both depths. Every packet a test
//! starts from is assembled by hand in this file, and every result is read
//! back by this file's own TIFF-stream, 8BIM, XMP, JPEG and PNG readers,
//! never by asking the core what it wrote. No golden files.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_export::ExportResult;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_color::{dr_doc_metadata, dr_doc_metadata_len, dr_doc_set_metadata};
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_open, dr_doc_save_native};
use doctored_core::ffi_export::{
    dr_doc_export_encode, dr_export_result_bytes, dr_export_result_free, dr_export_result_info,
    dr_precision_doc_export_encode,
};
use doctored_core::ffi_file_info::{
    dr_doc_file_info, dr_doc_set_file_info, dr_precision_doc_file_info,
    dr_precision_doc_set_file_info,
};
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_open, dr_precision_doc_save_native,
};
use doctored_core::ffi_precision_input::dr_precision_doc_from_rgba16;
use doctored_core::ffi_precision_inspect::dr_precision_doc_metadata;
use doctored_core::ffi_precision_metadata::dr_precision_doc_set_metadata;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES as LIMIT;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use common::metadata_fixtures::{bim, jpeg_segments, png_chunks};
use common::{cpath, solid, take_err_string};

const EXIF: i32 = 0;
const XMP: i32 = 1;
const IPTC: i32 = 2;

// -------------------------------------------------- a document, either depth --

/// One document handle of either depth, freed through the C API on drop, so
/// every test below runs the same steps against both.
enum Doc {
    Byte(*mut DrDocument),
    Typed(*mut PrecisionDocument),
}

impl Drop for Doc {
    fn drop(&mut self) {
        unsafe {
            match *self {
                Doc::Byte(doc) => dr_doc_free(doc),
                Doc::Typed(doc) => dr_precision_doc_free(doc),
            }
        }
    }
}

fn owned(text: *mut c_char) -> String {
    assert!(!text.is_null());
    let out = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(text) };
    out
}

impl Doc {
    /// A 4 x 3 document with no metadata at all.
    fn new(typed: bool) -> Doc {
        if !typed {
            let doc = DrDocument::from_pixels(solid(4, 3, [200, 120, 60, 255]));
            return Doc::Byte(Box::into_raw(Box::new(doc)));
        }
        let samples: Vec<u16> = (0..12u16)
            .flat_map(|i| [i * 4000, 65535 - i * 3000, 1234, 65535])
            .collect();
        let profile = IccProfile::srgb();
        let name = CString::new("Layer").unwrap();
        let mut err = ptr::null_mut();
        let doc = unsafe {
            dr_precision_doc_from_rgba16(
                samples.as_ptr(),
                samples.len(),
                4,
                3,
                profile.bytes().as_ptr(),
                profile.bytes().len(),
                name.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!doc.is_null(), "{}", take_err_string(err));
        Doc::Typed(doc)
    }

    fn info(&self) -> Value {
        let text = unsafe {
            match *self {
                Doc::Byte(doc) => dr_doc_file_info(doc),
                Doc::Typed(doc) => dr_precision_doc_file_info(doc),
            }
        };
        let value: Value = serde_json::from_str(&owned(text)).expect("file info json");
        assert_eq!(
            value.as_object().unwrap().len(),
            4,
            "all four keys: {value}"
        );
        value
    }

    /// `Ok(None)` is NULL with no error: nothing would change.
    fn try_set(&self, json: &str) -> Result<Option<Doc>, String> {
        let json = CString::new(json).unwrap();
        let mut err = ptr::null_mut();
        let next = unsafe {
            match *self {
                Doc::Byte(doc) => {
                    let out = dr_doc_set_file_info(doc, json.as_ptr(), &mut err);
                    (!out.is_null()).then_some(Doc::Byte(out))
                }
                Doc::Typed(doc) => {
                    let out = dr_precision_doc_set_file_info(doc, json.as_ptr(), &mut err);
                    (!out.is_null()).then_some(Doc::Typed(out))
                }
            }
        };
        if err.is_null() {
            Ok(next)
        } else {
            assert!(next.is_none(), "an error comes with no document");
            Err(take_err_string(err))
        }
    }

    fn set(&self, info: &Value) -> Doc {
        self.try_set(&info.to_string())
            .expect("set file info")
            .expect("a changed document")
    }

    fn packet(&self, kind: i32) -> Option<Vec<u8>> {
        unsafe {
            match *self {
                Doc::Byte(doc) => {
                    let len = dr_doc_metadata_len(doc, kind);
                    if len == 0 {
                        return None;
                    }
                    let mut out = vec![0u8; len];
                    assert!(dr_doc_metadata(doc, kind, out.as_mut_ptr(), len));
                    Some(out)
                }
                Doc::Typed(doc) => {
                    let mut len = 0usize;
                    let bytes = dr_precision_doc_metadata(doc, kind as u32, &mut len);
                    (!bytes.is_null() && len > 0)
                        .then(|| std::slice::from_raw_parts(bytes, len).to_vec())
                }
            }
        }
    }

    fn with_packet(&self, kind: i32, bytes: &[u8]) -> Doc {
        let mut err = ptr::null_mut();
        let out = unsafe {
            match *self {
                Doc::Byte(doc) => {
                    let out = dr_doc_set_metadata(doc, kind, bytes.as_ptr(), bytes.len(), &mut err);
                    (!out.is_null()).then_some(Doc::Byte(out))
                }
                Doc::Typed(doc) => {
                    let out = dr_precision_doc_set_metadata(
                        doc,
                        kind,
                        bytes.as_ptr(),
                        bytes.len(),
                        &mut err,
                    );
                    (!out.is_null()).then_some(Doc::Typed(out))
                }
            }
        };
        out.unwrap_or_else(|| panic!("set packet {kind}: {}", take_err_string(err)))
    }

    /// The encoded deliverable and its info, through the export request.
    fn export(&self, request: &str) -> (Vec<u8>, Value) {
        let json = CString::new(request).unwrap();
        let mut err = ptr::null_mut();
        let result: *mut ExportResult = unsafe {
            match *self {
                Doc::Byte(doc) => {
                    dr_doc_export_encode(doc, ptr::null(), json.as_ptr(), ptr::null(), &mut err)
                }
                Doc::Typed(doc) => dr_precision_doc_export_encode(
                    doc,
                    ptr::null(),
                    json.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                ),
            }
        };
        assert!(!result.is_null(), "{request}: {}", take_err_string(err));
        let mut length = 0usize;
        let bytes = unsafe {
            let pointer = dr_export_result_bytes(result, &mut length);
            assert!(!pointer.is_null());
            std::slice::from_raw_parts(pointer, length).to_vec()
        };
        let info = unsafe { dr_export_result_info(result, &mut err) };
        assert!(!info.is_null(), "{}", take_err_string(err));
        let info = serde_json::from_str(&owned(info)).expect("export info");
        unsafe { dr_export_result_free(result) };
        (bytes, info)
    }

    /// Saved as `.rz` and opened again.
    fn reopened(&self, dir: &TempDir) -> Doc {
        let path = cpath(&dir.path().join("saved.rz"));
        let mut err = ptr::null_mut();
        unsafe {
            match *self {
                Doc::Byte(doc) => {
                    assert!(
                        dr_doc_save_native(doc, path.as_ptr(), std::ptr::null(), &mut err),
                        "{}",
                        take_err_string(err)
                    );
                    let back = dr_doc_open(path.as_ptr(), std::ptr::null(), &mut err);
                    assert!(!back.is_null(), "{}", take_err_string(err));
                    Doc::Byte(back)
                }
                Doc::Typed(doc) => {
                    assert!(
                        dr_precision_doc_save_native(
                            doc,
                            path.as_ptr(),
                            LIMIT,
                            0,
                            ptr::null(),
                            &mut err
                        ),
                        "{}",
                        take_err_string(err)
                    );
                    let back =
                        dr_precision_doc_open(path.as_ptr(), LIMIT, 0, ptr::null(), &mut err);
                    assert!(!back.is_null(), "{}", take_err_string(err));
                    Doc::Typed(back)
                }
            }
        }
    }
}

fn both(test: impl Fn(Doc)) {
    for typed in [false, true] {
        test(Doc::new(typed));
    }
}

/// The value the tests author: every field holds characters XML escapes, a
/// non-ASCII character, and the description a line feed.
fn authored() -> Value {
    json!({
        "creator": ["Ada <L> & Co", "Bo"],
        "copyright": "\u{a9} 2026 Ada \"L\"",
        "description": "Line one\nLine 'two' \u{2014} \u{fc}n\u{ef}code",
        "keywords": ["sea", "north & south", "<tag>"],
    })
}

const EMPTY: &str = r#"{"creator":[],"copyright":"","description":"","keywords":[]}"#;

// ------------------------------------------------------- this file's readers --

/// The `rdf:li` texts of the ONE `dc:<name>` element of an XMP packet, with
/// the five entities decoded. Panics on a second element of that name.
fn dc(packet: &[u8], name: &str) -> Vec<String> {
    let text = String::from_utf8(packet.to_vec()).expect("utf-8 packet");
    let open = format!("<dc:{name}>");
    let close = format!("</dc:{name}>");
    assert!(
        text.matches(&open).count() <= 1,
        "more than one dc:{name}: {text}"
    );
    let Some(start) = text.find(&open) else {
        return Vec::new();
    };
    let body = &text[start + open.len()..text.find(&close).expect("closed")];
    body.split("<rdf:li")
        .skip(1)
        .map(|part| {
            let from = part.find('>').unwrap() + 1;
            let to = part.find("</rdf:li>").unwrap();
            part[from..to]
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&apos;", "'")
                .replace("&amp;", "&")
        })
        .collect()
}

/// The four fields as an XMP packet states them, in file-info JSON shape.
fn xmp_info(packet: &[u8]) -> Value {
    let one = |name: &str| dc(packet, name).into_iter().next().unwrap_or_default();
    json!({
        "creator": dc(packet, "creator"),
        "copyright": one("rights"),
        "description": one("description"),
        "keywords": dc(packet, "subject"),
    })
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

/// A classic TIFF stream reader: nothing but offsets and byte order.
struct Read<'a> {
    bytes: &'a [u8],
    be: bool,
}

impl<'a> Read<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        let be = &bytes[..2] == b"MM";
        let read = Read { bytes, be };
        assert_eq!(read.u16(2), 42);
        read
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
    fn tags(&self, ifd: usize) -> Vec<u16> {
        (0..usize::from(self.u16(ifd)))
            .map(|i| self.u16(ifd + 2 + 12 * i))
            .collect()
    }
    /// The offset of the IFD's next-IFD link.
    fn link(&self, ifd: usize) -> usize {
        ifd + 2 + 12 * usize::from(self.u16(ifd))
    }
    fn entry(&self, ifd: usize, tag: u16) -> Option<usize> {
        (0..usize::from(self.u16(ifd)))
            .map(|i| ifd + 2 + 12 * i)
            .find(|&at| self.u16(at) == tag)
    }
    /// Where the entry's data starts and how many bytes it is.
    fn extent(&self, ifd: usize, tag: u16) -> Option<(usize, usize)> {
        let at = self.entry(ifd, tag)?;
        let unit = match self.u16(at + 2) {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 => 4,
            5 | 10 | 12 => 8,
            other => panic!("type {other}"),
        };
        let length = self.u32(at + 4) as usize * unit;
        let start = if length <= 4 {
            at + 8
        } else {
            self.u32(at + 8) as usize
        };
        Some((start, length))
    }
    fn data(&self, ifd: usize, tag: u16) -> Option<&'a [u8]> {
        let (start, length) = self.extent(ifd, tag)?;
        Some(&self.bytes[start..start + length])
    }
    /// An ASCII tag as text, its terminating NUL checked and removed.
    fn ascii(&self, ifd: usize, tag: u16) -> Option<String> {
        let at = self.entry(ifd, tag)?;
        assert_eq!(self.u16(at + 2), 2, "tag {tag} is ASCII");
        let data = self.data(ifd, tag)?;
        assert_eq!(data.last(), Some(&0), "tag {tag} ends with NUL");
        Some(String::from_utf8(data[..data.len() - 1].to_vec()).unwrap())
    }
    fn sub(&self) -> usize {
        let at = self.entry(self.root(), 34665).expect("an Exif IFD");
        self.u32(at + 8) as usize
    }
}

type Field = (u16, u16, Vec<u8>);

/// An EXIF packet laid out the way a camera writes one: the header, IFD0 at
/// offset 8, IFD0's out-of-line values, then the Exif IFD and its values.
/// A field is (tag, type, data already in the packet's byte order).
fn exif(be: bool, mut ifd0: Vec<Field>, mut sub: Vec<Field>) -> Vec<u8> {
    let u16b = |v: u16| {
        if be {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    let u32b = |v: u32| {
        if be {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    let unit = |kind: u16| match kind {
        1 | 2 | 6 | 7 => 1usize,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        _ => 8,
    };
    let even = |n: usize| n + n % 2;
    let spilled = |fields: &[Field]| -> usize {
        fields
            .iter()
            .filter(|f| f.2.len() > 4)
            .map(|f| even(f.2.len()))
            .sum()
    };
    if !sub.is_empty() {
        let table = 2 + 12 * (ifd0.len() + 1) + 4;
        let at = 8 + table + spilled(&ifd0);
        ifd0.push((34665, 4, u32b(at as u32).to_vec()));
    }
    ifd0.sort_by_key(|f| f.0);
    sub.sort_by_key(|f| f.0);
    let mut out = if be { b"MM".to_vec() } else { b"II".to_vec() };
    out.extend(u16b(42));
    out.extend(u32b(8));
    let mut write = |fields: &[Field]| {
        let at = out.len();
        let mut spill = at + 2 + 12 * fields.len() + 4;
        let mut tail = Vec::new();
        out.extend(u16b(fields.len() as u16));
        for (tag, kind, data) in fields {
            out.extend(u16b(*tag));
            out.extend(u16b(*kind));
            out.extend(u32b((data.len() / unit(*kind)) as u32));
            if data.len() <= 4 {
                let mut value = [0u8; 4];
                value[..data.len()].copy_from_slice(data);
                out.extend(value);
            } else {
                out.extend(u32b(spill as u32));
                tail.extend(data);
                if !data.len().is_multiple_of(2) {
                    tail.push(0);
                }
                spill += even(data.len());
            }
        }
        out.extend(u32b(0));
        out.extend(tail);
    };
    write(&ifd0);
    if !sub.is_empty() {
        write(&sub);
    }
    out
}

fn ascii(text: &str) -> Vec<u8> {
    [text.as_bytes(), &[0]].concat()
}

/// A MakerNote's bytes: a recognisable, non-repeating pattern.
fn note(length: usize) -> Vec<u8> {
    (0..length).map(|i| (i * 7 + i / 251) as u8).collect()
}

/// A camera-like packet: Make and Software in IFD0, a date and a MakerNote
/// in the Exif IFD.
fn camera(be: bool, maker_note: &[u8], ifd0_extra: Vec<Field>) -> Vec<u8> {
    let mut ifd0 = vec![
        (271, 2, ascii("TestMaker")),
        (305, 2, ascii("Firmware 1.0")),
    ];
    ifd0.extend(ifd0_extra);
    exif(
        be,
        ifd0,
        vec![
            (36867, 2, ascii("2026:08:24 12:34:56")),
            (37500, 7, maker_note.to_vec()),
        ],
    )
}

/// One IIM dataset: marker, record, dataset, a two-byte length, the value.
fn iim(record: u8, dataset: u8, value: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1C, record, dataset];
    out.extend((value.len() as u16).to_be_bytes());
    out.extend(value);
    out
}

/// The 8BIM resources of a run as (id, data).
fn bims(run: &[u8]) -> Vec<(u16, Vec<u8>)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < run.len() {
        assert_eq!(&run[i..i + 4], b"8BIM");
        let id = u16::from_be_bytes([run[i + 4], run[i + 5]]);
        let name = (1 + usize::from(run[i + 6])).next_multiple_of(2);
        let size_at = i + 6 + name;
        let size = u32::from_be_bytes(run[size_at..size_at + 4].try_into().unwrap()) as usize;
        out.push((id, run[size_at + 4..size_at + 4 + size].to_vec()));
        i = size_at + 4 + size + size % 2;
    }
    out
}

/// The (record, dataset) pairs of an IIM block.
fn iim_datasets(block: &[u8]) -> Vec<(u8, u8)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < block.len() {
        assert_eq!(block[i], 0x1C);
        out.push((block[i + 1], block[i + 2]));
        i += 5 + usize::from(u16::from_be_bytes([block[i + 3], block[i + 4]]));
    }
    out
}

/// The XMP packet a deliverable carries, found by this file's own walk of
/// the container.
fn delivered_xmp(format: &str, bytes: &[u8]) -> Vec<u8> {
    match format {
        "jpeg" => {
            const ID: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
            jpeg_segments(bytes)
                .into_iter()
                .find(|(marker, payload)| *marker == 0xE1 && payload.starts_with(ID))
                .map(|(_, payload)| payload[ID.len()..].to_vec())
                .expect("an XMP APP1")
        }
        "png" => png_chunks(bytes)
            .into_iter()
            .filter(|(kind, _)| kind == b"iTXt")
            .find_map(|(_, data)| {
                let rest = data.strip_prefix(b"XML:com.adobe.xmp\0")?;
                // compression flag, compression method, language\0, translated\0
                let mut i = 2;
                i += rest[i..].iter().position(|&b| b == 0)? + 1;
                i += rest[i..].iter().position(|&b| b == 0)? + 1;
                Some(rest[i..].to_vec())
            })
            .expect("an XMP iTXt"),
        _ => {
            let t = Read::new(bytes);
            t.data(t.root(), 700).expect("TIFF tag 700").to_vec()
        }
    }
}

/// The EXIF packet a JPEG carries.
fn jpeg_exif(bytes: &[u8]) -> Option<Vec<u8>> {
    jpeg_segments(bytes)
        .into_iter()
        .find(|(marker, payload)| *marker == 0xE1 && payload.starts_with(b"Exif\0\0"))
        .map(|(_, payload)| payload[6..].to_vec())
}

// ------------------------------------------------------------------- tests --

#[test]
fn a_document_with_no_metadata_reads_four_empty_fields() {
    both(|doc| {
        assert_eq!(doc.info(), serde_json::from_str::<Value>(EMPTY).unwrap());
        // Setting nothing on nothing changes nothing, and creates no packet.
        assert!(doc.try_set(EMPTY).unwrap().is_none());
        assert!(doc.packet(XMP).is_none());
    });
}

#[test]
fn each_field_reads_from_xmp_then_iim_then_exif() {
    both(|doc| {
        // XMP states the creator only; IIM states the by-line, copyright and
        // keywords; EXIF states the artist, copyright and description.
        let xmp = b"<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description rdf:about='' xmlns:dc='http://purl.org/dc/elements/1.1/'><dc:creator><rdf:Seq><rdf:li>From &amp; XMP</rdf:li><rdf:li> Second </rdf:li></rdf:Seq></dc:creator></rdf:Description></rdf:RDF></x:xmpmeta>";
        // 0xA9 alone is not UTF-8, so this notice is read as Latin-1.
        let block = [
            iim(2, 80, b"From IIM"),
            iim(2, 116, b"\xA9 IIM"),
            iim(2, 25, b"one"),
            iim(2, 25, b"two"),
        ]
        .concat();
        let run = bim(0x0404, b"", &block);
        let packet = exif(
            false,
            vec![
                (270, 2, ascii("From EXIF\r\nsecond line")),
                (315, 2, ascii("Artist A; Artist B")),
                (33432, 2, ascii("EXIF notice")),
            ],
            vec![],
        );
        let doc = doc
            .with_packet(XMP, xmp)
            .with_packet(IPTC, &run)
            .with_packet(EXIF, &packet);
        assert_eq!(
            doc.info(),
            json!({
                "creator": ["From & XMP", "Second"],
                "copyright": "\u{a9} IIM",
                "description": "From EXIF\nsecond line",
                "keywords": ["one", "two"],
            })
        );
        // Without the XMP the creator is the IIM by-line; without the IIM it
        // is the EXIF artist, split at its semicolons.
        let no_xmp = Doc::new(matches!(doc, Doc::Typed(_)))
            .with_packet(IPTC, &run)
            .with_packet(EXIF, &packet);
        assert_eq!(no_xmp.info()["creator"], json!(["From IIM"]));
        let exif_only = Doc::new(matches!(doc, Doc::Typed(_))).with_packet(EXIF, &packet);
        assert_eq!(
            exif_only.info(),
            json!({
                "creator": ["Artist A", "Artist B"],
                "copyright": "EXIF notice",
                "description": "From EXIF\nsecond line",
                "keywords": [],
            })
        );
        // A language alternative reads its x-default entry.
        let alt = b"<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description rdf:about='' xmlns:dc='http://purl.org/dc/elements/1.1/'><dc:rights><rdf:Alt><rdf:li xml:lang='fr'>Droits</rdf:li><rdf:li xml:lang='x-default'>Rights &#169;</rdf:li></rdf:Alt></dc:rights></rdf:Description></rdf:RDF></x:xmpmeta>";
        let alt = Doc::new(matches!(doc, Doc::Typed(_))).with_packet(XMP, alt);
        assert_eq!(alt.info()["copyright"], json!("Rights \u{a9}"));
    });
}

#[test]
fn a_set_value_reads_back_and_setting_it_again_is_no_change() {
    both(|doc| {
        let wanted = authored();
        let set = doc.set(&wanted);
        assert_eq!(set.info(), wanted);
        assert_eq!(doc.info()["creator"], json!([]), "the input is untouched");
        // The same value again: NULL, and no error.
        assert!(set.try_set(&wanted.to_string()).unwrap().is_none());
        // Entries are trimmed and blank ones dropped, so this is the same
        // value too.
        let mut padded = wanted.clone();
        padded["creator"] = json!(["  Ada <L> & Co ", "", "Bo", "   "]);
        padded["copyright"] = json!(format!("  {}  ", wanted["copyright"].as_str().unwrap()));
        assert!(set.try_set(&padded.to_string()).unwrap().is_none());
        // Clearing every field removes the elements and reads back empty.
        let cleared = set.try_set(EMPTY).unwrap().expect("a change");
        assert_eq!(
            cleared.info(),
            serde_json::from_str::<Value>(EMPTY).unwrap()
        );
        let packet = cleared.packet(XMP).expect("the packet stays");
        assert_eq!(count(&packet, b"<dc:"), 0);
        assert_eq!(count(&packet, b"<rdf:Description"), 0);
    });
}

#[test]
fn every_limit_and_malformed_value_refuses_by_name() {
    both(|doc| {
        let refusal = |info: Value| -> String {
            let error = doc
                .try_set(&info.to_string())
                .err()
                .unwrap_or_else(|| panic!("accepted: {info}"));
            assert!(error.starts_with("file info"), "{error}");
            assert_eq!(error, error.to_lowercase(), "{error}");
            error
        };
        let with = |key: &str, value: Value| {
            let mut info: Value = serde_json::from_str(EMPTY).unwrap();
            info[key] = value;
            info
        };
        for list in ["creator", "keywords"] {
            assert!(refusal(with(list, json!(["x".repeat(257)]))).contains("256 characters"));
            let many: Vec<String> = (0..257).map(|i| format!("k{i}")).collect();
            assert!(refusal(with(list, json!(many))).contains("256 entries"));
            assert!(refusal(with(list, json!(["a\u{7}b"]))).contains("control characters"));
            assert!(refusal(with(list, json!(["a\nb"]))).contains("control characters"));
            assert!(refusal(with(list, json!("one string"))).contains("array of strings"));
            assert!(refusal(with(list, json!([1]))).contains("array of strings"));
            // At the limits it is accepted: 256 entries of 256 characters,
            // counted in characters, not bytes.
            let full: Vec<String> = (0..256)
                .map(|i| format!("{i:0>3}{}", "\u{e9}".repeat(253)))
                .collect();
            let set = doc.set(&with(list, json!(full)));
            assert_eq!(set.info()[list], json!(full));
        }
        assert!(refusal(with("copyright", json!("c".repeat(2001)))).contains("2000 characters"));
        assert!(refusal(with("copyright", json!("a\nb"))).contains("control characters"));
        assert!(refusal(with("copyright", json!(7))).contains("must be a string"));
        assert!(refusal(with("description", json!("d".repeat(8001)))).contains("8000 characters"));
        assert!(refusal(with("description", json!("a\tb"))).contains("control characters"));
        assert!(refusal(with("description", json!("a\r\nb"))).contains("control characters"));
        doc.set(&with("copyright", json!("c".repeat(2000))));
        let lines = doc.set(&with(
            "description",
            json!(format!("a\nb{}", "d".repeat(7997))),
        ));
        assert_eq!(
            lines.info()["description"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            8000
        );

        // Exactly the four keys.
        let mut extra: Value = serde_json::from_str(EMPTY).unwrap();
        extra["title"] = json!("x");
        assert!(refusal(extra).contains("unknown key \"title\""));
        for key in ["creator", "copyright", "description", "keywords"] {
            let mut missing: Value = serde_json::from_str(EMPTY).unwrap();
            missing.as_object_mut().unwrap().remove(key);
            assert!(refusal(missing).contains(&format!("requires the key \"{key}\"")));
        }
        assert!(refusal(json!([])).contains("json object"));
        let error = doc.try_set("{not json").err().unwrap();
        assert!(error.starts_with("file info is not valid json"), "{error}");
    });
}

#[test]
fn an_attribute_only_packet_gains_one_description_and_repeat_edits_replace_it() {
    both(|doc| {
        // A self-closing Description has no content to add a property to.
        let source = b"<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description rdf:about='' xmlns:tiff='http://ns.adobe.com/tiff/1.0/' tiff:Orientation='6'/></rdf:RDF></x:xmpmeta>";
        let doc = doc.with_packet(XMP, source);
        let set = doc.set(&authored());
        assert_eq!(set.info(), authored());
        let packet = set.packet(XMP).unwrap();
        assert_eq!(xmp_info(&packet), authored());
        // The original Description is where it was; the new one sits just
        // before the end of rdf:RDF.
        let text = String::from_utf8(packet.clone()).unwrap();
        let kept = "<rdf:Description rdf:about='' xmlns:tiff='http://ns.adobe.com/tiff/1.0/' tiff:Orientation='6'/>";
        assert!(text.contains(kept), "{text}");
        assert!(text.ends_with("</dc:subject></rdf:Description></rdf:RDF></x:xmpmeta>"));
        assert_eq!(count(&packet, b"<rdf:Description"), 2);
        // A second and a third edit replace that Description; none piles up.
        let mut next = authored();
        next["keywords"] = json!(["only"]);
        let twice = set.set(&next);
        let mut last = next.clone();
        last["description"] = json!("shorter");
        let thrice = twice.set(&last);
        for (doc, wanted) in [(&twice, &next), (&thrice, &last)] {
            let packet = doc.packet(XMP).unwrap();
            assert_eq!(count(&packet, b"<rdf:Description"), 2);
            assert_eq!(&xmp_info(&packet), wanted);
        }
        // Editing back to the first value gives the first packet exactly.
        assert_eq!(thrice.set(&authored()).packet(XMP).unwrap(), packet);
    });
}

#[test]
fn another_programs_dc_elements_are_replaced_wherever_they_are() {
    both(|doc| {
        // Two Descriptions: one holds dc:creator beside a property that must
        // stay, the other holds only dc:subject and dc:rights (and goes).
        let source = b"<?xpacket begin='' id='W5M0MpCehiHzreSzNTczkc9d'?><x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\n <rdf:Description rdf:about='' xmlns:dc='http://purl.org/dc/elements/1.1/' xmlns:xmp='http://ns.adobe.com/xap/1.0/'>\n  <xmp:CreatorTool>Camera</xmp:CreatorTool>\n  <dc:creator><rdf:Seq><rdf:li>Old</rdf:li></rdf:Seq></dc:creator>\n  <dc:format>image/jpeg</dc:format>\n </rdf:Description>\n <rdf:Description rdf:about='' xmlns:d='http://purl.org/dc/elements/1.1/'><d:subject><rdf:Bag><rdf:li>old</rdf:li></rdf:Bag></d:subject><d:rights><rdf:Alt><rdf:li xml:lang='x-default'>old</rdf:li></rdf:Alt></d:rights></rdf:Description>\n</rdf:RDF></x:xmpmeta><?xpacket end='w'?>";
        let doc = doc.with_packet(XMP, source);
        assert_eq!(doc.info()["creator"], json!(["Old"]));
        assert_eq!(
            doc.info()["keywords"],
            json!(["old"]),
            "any prefix bound to dc"
        );
        let set = doc.set(&authored());
        let packet = set.packet(XMP).unwrap();
        let text = String::from_utf8(packet.clone()).unwrap();
        assert_eq!(xmp_info(&packet), authored());
        assert!(text.contains("<xmp:CreatorTool>Camera</xmp:CreatorTool>"));
        assert!(text.contains("<dc:format>image/jpeg</dc:format>"));
        assert!(!text.contains("Old") && !text.contains("<d:"), "{text}");
        assert!(text
            .ends_with("</dc:subject></rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end='w'?>"));
        assert_eq!(
            count(&packet, b"<rdf:Description"),
            2,
            "the emptied one is gone"
        );
        assert_eq!(set.info(), authored());

        // A packet with no rdf:RDF cannot take the edit, and says so.
        let broken = Doc::new(matches!(doc, Doc::Typed(_)))
            .with_packet(XMP, b"<x:xmpmeta xmlns:x='adobe:ns:meta/'></x:xmpmeta>");
        let error = broken.try_set(&authored().to_string()).err().unwrap();
        assert!(
            error.starts_with("file info cannot be written") && error.contains("xmp packet"),
            "{error}"
        );
    });
}

#[test]
fn a_document_without_xmp_gets_a_minimal_packet() {
    both(|doc| {
        let set = doc.set(&authored());
        let packet = set.packet(XMP).expect("a packet is created");
        let text = String::from_utf8(packet.clone()).unwrap();
        assert!(text.starts_with("<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:creator><rdf:Seq><rdf:li>Ada &lt;L&gt; &amp; Co</rdf:li><rdf:li>Bo</rdf:li></rdf:Seq></dc:creator><dc:rights><rdf:Alt><rdf:li xml:lang=\"x-default\">"), "{text}");
        assert!(text.ends_with(
            "</rdf:Bag></dc:subject></rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
        ));
        assert_eq!(xmp_info(&packet), authored());
        // No EXIF or IPTC packet is invented.
        assert!(set.packet(EXIF).is_none() && set.packet(IPTC).is_none());
    });
}

#[test]
fn the_iim_datasets_and_their_digest_are_dropped() {
    both(|doc| {
        // 2:05 (object name) is not a File Info field and stays; so does an
        // unrelated resource.
        let block = [
            iim(2, 0, &[0, 2]),
            iim(2, 5, b"Title"),
            iim(2, 25, b"old keyword"),
            iim(2, 80, b"Old By-line"),
            iim(2, 116, b"Old notice"),
            iim(2, 120, b"Old caption"),
        ]
        .concat();
        let guides = bim(0x0408, b"", &[1, 2, 3]);
        let run = [
            bim(0x0404, b"", &block),
            guides.clone(),
            bim(0x0425, b"", &[9u8; 16]),
        ]
        .concat();
        let doc = doc.with_packet(IPTC, &run);
        assert_eq!(doc.info()["creator"], json!(["Old By-line"]));
        let set = doc.set(&authored());
        assert_eq!(set.info(), authored());
        let resources = bims(&set.packet(IPTC).expect("the run stays"));
        let ids: Vec<u16> = resources.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, [0x0404, 0x0408], "the digest resource is gone");
        assert_eq!(iim_datasets(&resources[0].1), [(2, 0), (2, 5)]);
        assert_eq!(
            resources[0].1,
            [iim(2, 0, &[0, 2]), iim(2, 5, b"Title")].concat()
        );
        assert_eq!(resources[1].1, [1, 2, 3]);

        // A run holding nothing else leaves no packet at all.
        let only = bim(0x0404, b"", &iim(2, 25, b"old"));
        let typed = matches!(doc, Doc::Typed(_));
        let cleared = Doc::new(typed).with_packet(IPTC, &only).set(&authored());
        assert!(cleared.packet(IPTC).is_none());
        // A run that does not walk, or whose IIM block does not, is left
        // exactly as it is.
        for broken in [
            b"8BIM\x04\x04\0\0\0\0\0\x09short".to_vec(),
            bim(0x0404, b"", b"\x1c\x02\x19\0\x09cut"),
        ] {
            let kept = Doc::new(typed).with_packet(IPTC, &broken).set(&authored());
            assert_eq!(kept.packet(IPTC).unwrap(), broken);
        }
    });
}

#[test]
fn the_exif_strings_are_set_and_no_other_byte_moves() {
    for be in [false, true] {
        both(|doc| {
            let maker = note(64);
            let source = camera(be, &maker, vec![(315, 2, ascii("Old Artist"))]);
            let before = Read::new(&source);
            let (note_at, note_len) = before.extent(before.sub(), 37500).unwrap();
            let doc = doc.with_packet(EXIF, &source);
            let set = doc.set(&authored());
            let packet = set.packet(EXIF).unwrap();
            let t = Read::new(&packet);

            // The three tags, as ASCII with a NUL, creators joined.
            let root = t.root();
            let authored = authored();
            assert_eq!(t.ascii(root, 315).unwrap(), "Ada <L> & Co; Bo");
            assert_eq!(
                t.ascii(root, 33432).unwrap(),
                authored["copyright"].as_str().unwrap()
            );
            assert_eq!(
                t.ascii(root, 270).unwrap(),
                authored["description"].as_str().unwrap()
            );
            // IFD0 is a new table at the end, in tag order, with every other
            // entry it had and the same next-IFD link.
            assert_eq!(t.tags(root), [270, 271, 305, 315, 33432, 34665]);
            assert_eq!(t.link(root) + 4, packet.len());
            assert_eq!(t.u32(t.link(root)), before.u32(before.link(before.root())));
            assert_eq!(t.ascii(root, 271).unwrap(), "TestMaker");
            assert_eq!(t.ascii(root, 305).unwrap(), "Firmware 1.0");
            // Nothing the source held moved: every byte after the header's
            // IFD0 offset is where it was, the MakerNote included.
            assert!(packet.len() > source.len());
            assert_eq!(&packet[..4], &source[..4]);
            assert_eq!(&packet[8..source.len()], &source[8..]);
            assert_eq!(t.sub(), before.sub());
            assert_eq!(t.extent(t.sub(), 37500), Some((note_at, note_len)));
            assert_eq!(t.data(t.sub(), 37500).unwrap(), maker);
            assert_eq!(set.info(), authored);

            // An emptied field's tag is removed.
            let mut fewer = authored.clone();
            fewer["creator"] = json!([]);
            fewer["copyright"] = json!("");
            let removed = set.set(&fewer);
            let packet = removed.packet(EXIF).unwrap();
            let t = Read::new(&packet);
            assert_eq!(t.tags(t.root()), [270, 271, 305, 34665]);
            assert_eq!(t.data(t.sub(), 37500).unwrap(), maker);
        });
    }
}

#[test]
fn repeat_edits_reuse_the_tail_and_an_unrelated_edit_leaves_the_packet_alone() {
    both(|doc| {
        let maker = note(300);
        let source = camera(false, &maker, vec![]);
        let doc = doc.with_packet(EXIF, &source);
        let mut info = authored();
        info["description"] = json!("description 0");
        let mut current = doc.set(&info);
        let first = current.packet(EXIF).unwrap();
        // Six more descriptions of the same length: the packet never grows.
        for round in 1..=6 {
            info["description"] = json!(format!("description {round}"));
            current = current.set(&info);
            let packet = current.packet(EXIF).unwrap();
            assert_eq!(packet.len(), first.len(), "round {round}");
            let t = Read::new(&packet);
            assert_eq!(
                t.ascii(t.root(), 270).unwrap(),
                format!("description {round}")
            );
            assert_eq!(t.ascii(t.root(), 315).unwrap(), "Ada <L> & Co; Bo");
            assert_eq!(t.data(t.sub(), 37500).unwrap(), maker);
            assert_eq!(&packet[8..source.len()], &source[8..]);
        }
        // A longer one grows it, and going back gives the same length again.
        info["description"] = json!("a much longer description than before");
        let longer = current.set(&info);
        assert!(longer.packet(EXIF).unwrap().len() > first.len());
        info["description"] = json!("description 9");
        let back = longer.set(&info);
        assert_eq!(back.packet(EXIF).unwrap().len(), first.len());

        // Keywords have no EXIF tag: the packet is the same bytes.
        let before = back.packet(EXIF).unwrap();
        info["keywords"] = json!(["only", "keywords", "changed"]);
        let keywords = back.set(&info);
        assert_eq!(keywords.packet(EXIF).unwrap(), before);
        assert_eq!(keywords.info(), info);
    });
}

#[test]
fn a_packet_near_the_jpeg_limit_loses_its_three_tags_instead_of_its_place_in_a_jpeg() {
    both(|doc| {
        // 60,000 bytes, nearly all of it MakerNote: there is no room for an
        // 8,000-character description inside one JPEG segment (65,533 bytes
        // less the 6-byte Exif prefix).
        let probe = camera(false, &[], vec![(315, 2, ascii("Old Artist"))]);
        let maker = note(60_000 - probe.len());
        let source = camera(false, &maker, vec![(315, 2, ascii("Old Artist"))]);
        assert_eq!(source.len(), 60_000);
        let doc = doc.with_packet(EXIF, &source);
        let (jpeg, info) = doc.export(r#"{"format":"jpeg"}"#);
        assert_eq!(info["metadata"]["exif"], true, "it fits before the edit");
        assert!(jpeg_exif(&jpeg).is_some());

        let mut wanted = authored();
        wanted["description"] = json!("d".repeat(8000));
        let set = doc.set(&wanted);
        assert_eq!(set.info(), wanted, "XMP carries every field");
        let packet = set.packet(EXIF).unwrap();
        assert_eq!(packet.len(), source.len(), "the length is unchanged");
        let t = Read::new(&packet);
        assert_eq!(t.root(), 8, "IFD0 is edited where it is");
        assert_eq!(t.tags(8), [271, 305, 34665], "no 270, 315 or 33432");
        assert_eq!(t.u32(t.link(8)), 0);
        assert_eq!(t.ascii(8, 271).unwrap(), "TestMaker");
        assert_eq!(t.data(t.sub(), 37500).unwrap(), maker);
        let xmp = set.packet(XMP).unwrap();
        assert_eq!(dc(&xmp, "description"), ["d".repeat(8000)]);

        // The JPEG still holds the EXIF packet, MakerNote and all.
        let (jpeg, info) = set.export(r#"{"format":"jpeg"}"#);
        assert_eq!(info["metadata"]["exif"], true);
        let carried = jpeg_exif(&jpeg).expect("the EXIF APP1");
        let t = Read::new(&carried);
        assert_eq!(t.data(t.sub(), 37500).unwrap(), maker);
        assert_eq!(xmp_info(&delivered_xmp("jpeg", &jpeg)), wanted);

        // Another edit that still does not fit changes nothing in the packet;
        // one that fits writes the strings again.
        wanted["description"] = json!("e".repeat(7000));
        let again = set.set(&wanted);
        assert_eq!(again.packet(EXIF).unwrap(), packet);
        wanted["description"] = json!("short");
        let fits = again.set(&wanted);
        let packet = fits.packet(EXIF).unwrap();
        let t = Read::new(&packet);
        assert_eq!(t.ascii(t.root(), 270).unwrap(), "short");
        assert!(packet.len() + 6 <= 65533);
        let (jpeg, info) = fits.export(r#"{"format":"jpeg"}"#);
        assert_eq!(info["metadata"]["exif"], true);
        assert!(jpeg_exif(&jpeg).is_some());
    });
}

#[test]
fn file_info_survives_a_native_save_and_reopen() {
    both(|doc| {
        let dir = TempDir::new().unwrap();
        let maker = note(40);
        let set = doc
            .with_packet(EXIF, &camera(false, &maker, vec![]))
            .set(&authored());
        let back = set.reopened(&dir);
        assert_eq!(back.info(), authored());
        for kind in [EXIF, XMP, IPTC] {
            assert_eq!(back.packet(kind), set.packet(kind), "packet {kind}");
        }
        // And it is still the same value: setting it again changes nothing.
        assert!(back.try_set(&authored().to_string()).unwrap().is_none());
    });
}

#[test]
fn jpeg_png_and_tiff_exports_carry_the_four_fields() {
    both(|doc| {
        let typed = matches!(doc, Doc::Typed(_));
        let set = doc.set(&authored());
        for format in ["jpeg", "png", "tiff"] {
            let request = if typed && format != "jpeg" {
                format!(r#"{{"format":"{format}","bit_depth":16}}"#)
            } else {
                format!(r#"{{"format":"{format}"}}"#)
            };
            let (bytes, info) = set.export(&request);
            assert_eq!(info["metadata"]["xmp"], true, "{format}");
            let packet = delivered_xmp(format, &bytes);
            assert_eq!(xmp_info(&packet), authored(), "{format}");
        }
    });
}

#[test]
fn a_jpeg_sourced_exif_with_a_maker_note_survives_tiff_export() {
    both(|doc| {
        let typed = matches!(doc, Doc::Typed(_));
        let maker = note(500);
        let set = doc
            .with_packet(EXIF, &camera(true, &maker, vec![]))
            .set(&authored());
        let request = if typed {
            r#"{"format":"tiff","bit_depth":16}"#
        } else {
            r#"{"format":"tiff"}"#
        };
        let (bytes, info) = set.export(request);
        assert_eq!(info["metadata"]["exif"], true);
        let t = Read::new(&bytes);
        let root = t.root();
        assert_eq!(t.data(t.sub(), 37500).unwrap(), maker);
        assert_eq!(t.ascii(root, 271).unwrap(), "TestMaker");
        assert_eq!(t.ascii(root, 315).unwrap(), "Ada <L> & Co; Bo");
        assert_eq!(
            t.ascii(root, 270).unwrap(),
            authored()["description"].as_str().unwrap()
        );
    });
}

#[test]
fn a_stamp_over_an_authored_credit_leaves_one_element_of_each() {
    both(|doc| {
        let typed = matches!(doc, Doc::Typed(_));
        let depth = if typed { r#","bit_depth":16"# } else { "" };
        let set = doc.set(&authored());
        // The stamp sets the author only: the creator is replaced, and the
        // authored rights, description and keywords stay as they were.
        let (bytes, info) = set.export(&format!(
            r#"{{"format":"png"{depth},"metadata":{{"stamp":{{"author":"Stamp & Co"}}}}}}"#
        ));
        assert_eq!(info["metadata"]["stamped"], true);
        let packet = delivered_xmp("png", &bytes);
        let mut wanted = authored();
        wanted["creator"] = json!(["Stamp & Co"]);
        assert_eq!(xmp_info(&packet), wanted);
        assert_eq!(count(&packet, b"<dc:creator>"), 1);
        assert_eq!(count(&packet, b"<dc:rights>"), 1);
        // Both stamped: both replaced, still one of each.
        let (bytes, info) = set.export(&format!(
            r#"{{"format":"png"{depth},"metadata":{{"stamp":{{"author":"S","copyright":"(c) S"}}}}}}"#
        ));
        assert_eq!(info["metadata"]["stamped"], true);
        let packet = delivered_xmp("png", &bytes);
        wanted["creator"] = json!(["S"]);
        wanted["copyright"] = json!("(c) S");
        assert_eq!(xmp_info(&packet), wanted);
        // The stored packet is not the stamp's to change.
        assert_eq!(set.info(), authored());
    });
}
