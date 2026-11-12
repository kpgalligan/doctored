//! Paths in the FLAT TIFF of every colour mode `tiff_paths.rs` does not
//! cover (`tiff_paths`): Grayscale at 8 and 16 bits, Lab Color at 8 and 16,
//! Indexed Color, Bitmap, and 32 bits as RGB and as Grayscale, each through
//! its own TIFF writer (the export with profile `document`; Lab's save) and
//! its own open, all through the FFI.
//!
//! Oracles: `tiff_paths.rs`' TIFF tag and 8BIM readers and the records it
//! computes from Adobe's layout (`expected_resources`).
//!
//! Per mode: the file holds exactly the expected path resources; the mode's
//! open reopens it in the same mode with the paths and the clipping path and
//! a run holding none; with its paths deleted it writes none; stored rotated
//! (Orientation 6) it reads, keeps and writes none and says so; and its
//! flat-state losses name "paths" off a TIFF, or for a path past 128
//! canvases, only. Every failing check is collected, so a red run names
//! every mode.
//!
//! A 32-bit TIFF also carries the EXIF packet and the 8BIM run the export's
//! `carried` bits report.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc_export::ExportResult;
use doctored_core::ffi_bitmap::{dr_precision_doc_convert_to_bitmap, dr_precision_doc_open_bitmap};
use doctored_core::ffi_export::dr_precision_doc_export_encode;
use doctored_core::ffi_gray::{dr_precision_doc_convert_to_gray, dr_precision_doc_open_gray};
use doctored_core::ffi_indexed::{
    dr_precision_doc_convert_to_indexed, dr_precision_doc_open_indexed,
};
use doctored_core::ffi_lab::{
    dr_precision_doc_convert_to_lab, dr_precision_doc_open_lab, dr_precision_doc_save_lab,
};
use doctored_core::ffi_precision::dr_precision_doc_open;
use doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES as LIMIT;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::psd_clipping_path::tiff_run;
use crate::psd_paths::assert_paths_close;
use crate::tiff_paths::{
    block, byte_doc, clipping_names, expected_resources, held, is_path_id, path_list,
    path_resources, promoted, ramp_jpeg, rotated, segment, typed_losses, typed_with_list, H,
    ROTATED, W,
};

const WORKING_GRAY: &str = "gray_gamma_22";

/// The colour modes, each with its own TIFF writer and open.
#[derive(Clone, Copy, Debug)]
enum Mode {
    Gray8,
    Gray16,
    Lab8,
    Lab16,
    Indexed,
    Bitmap,
    Float,
    Gray32,
}

const MODES: [Mode; 8] = [
    Mode::Gray8,
    Mode::Gray16,
    Mode::Lab8,
    Mode::Lab16,
    Mode::Indexed,
    Mode::Bitmap,
    Mode::Float,
    Mode::Gray32,
];

/// A document handle out of an FFI call, or the call's error.
fn taken(out: *mut PrecisionDocument, err: *mut c_char) -> Result<Box<PrecisionDocument>, String> {
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(unsafe { Box::from_raw(out) })
    }
}

fn to_gray(doc: &PrecisionDocument, options: &Value) -> Box<PrecisionDocument> {
    let options = CString::new(options.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_gray(
            doc,
            ptr::null(),
            0,
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    taken(out, err).expect("Grayscale")
}

fn to_float(doc: &PrecisionDocument) -> Box<PrecisionDocument> {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_promote_to_float(doc, LIMIT, 0, ptr::null(), &mut err) };
    taken(out, err).expect("32 bits")
}

/// One of the mode conversions that take an options object and a job.
type Convert = unsafe extern "C" fn(
    *const PrecisionDocument,
    *const c_char,
    u64,
    u64,
    *const doctored_core::job_control::DrJobControl,
    *mut *mut c_char,
) -> *mut PrecisionDocument;

fn convert(doc: &PrecisionDocument, call: Convert, options: &Value) -> Box<PrecisionDocument> {
    let options = CString::new(options.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { call(doc, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    taken(out, err).expect("mode conversion")
}

/// The flat TIFF bytes of an export `request`.
fn exported(doc: &PrecisionDocument, request: &Value) -> Result<Vec<u8>, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_export_encode(
            doc,
            ptr::null(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let result: Box<ExportResult> = unsafe { Box::from_raw(out) };
    Ok(result.bytes().to_vec())
}

impl Mode {
    fn is_mode(self, doc: &PrecisionDocument) -> bool {
        match self {
            Mode::Gray8 | Mode::Gray16 => doc.is_gray() && !doc.is_float() && !doc.is_bitmap(),
            Mode::Lab8 | Mode::Lab16 => doc.is_lab(),
            Mode::Indexed => doc.is_indexed(),
            Mode::Bitmap => doc.is_bitmap(),
            Mode::Float => doc.is_float() && !doc.is_gray(),
            Mode::Gray32 => doc.is_float() && doc.is_gray(),
        }
    }

    /// The mode's document of `byte_doc`'s picture, holding `path_list`.
    fn document(self) -> Box<PrecisionDocument> {
        let rgb = promoted(&byte_doc());
        let gray = |depth: u32| to_gray(&rgb, &json!({"space": WORKING_GRAY, "depth": depth}));
        let lab = |depth: u32| {
            convert(
                &rgb,
                dr_precision_doc_convert_to_lab,
                &json!({"depth": depth}),
            )
        };
        let doc = match self {
            Mode::Gray8 => gray(8),
            Mode::Gray16 => gray(16),
            Mode::Lab8 => lab(8),
            Mode::Lab16 => lab(16),
            Mode::Indexed => convert(
                &rgb,
                dr_precision_doc_convert_to_indexed,
                &json!({"palette": "adaptive", "dither": "none"}),
            ),
            Mode::Bitmap => convert(
                &gray(8),
                dr_precision_doc_convert_to_bitmap,
                &json!({"method": "threshold"}),
            ),
            Mode::Float => to_float(&rgb),
            Mode::Gray32 => to_gray(&to_float(&rgb), &json!({"space": WORKING_GRAY})),
        };
        assert!(self.is_mode(&doc), "{self:?}: the conversion");
        let doc = typed_with_list(&doc, &path_list());
        assert_eq!(doc.paths().len(), 3, "{self:?}: the path list");
        doc
    }

    /// The mode's flat TIFF of `doc` at `path`, as its Save or Save a Copy
    /// writes it.
    fn save(self, doc: &PrecisionDocument, path: &Path) -> Result<Vec<u8>, String> {
        let depth = match self {
            Mode::Gray16 => 16,
            Mode::Float | Mode::Gray32 => 32,
            _ => 8,
        };
        let bytes = match self {
            Mode::Lab8 | Mode::Lab16 => {
                let request = CString::new(r#"{"format":"tiff"}"#).unwrap();
                let mut err = ptr::null_mut();
                let ok = unsafe {
                    dr_precision_doc_save_lab(
                        doc,
                        ptr::null(),
                        cpath(path).as_ptr(),
                        request.as_ptr(),
                        LIMIT,
                        0,
                        ptr::null(),
                        &mut err,
                    )
                };
                if !ok {
                    return Err(take_err_string(err));
                }
                return Ok(std::fs::read(path).unwrap());
            }
            _ => exported(
                doc,
                &json!({"format": "tiff", "profile": "document", "bit_depth": depth}),
            )?,
        };
        std::fs::write(path, &bytes).unwrap();
        Ok(bytes)
    }

    /// The mode's own open of `path`.
    fn open(self, path: &Path) -> Result<Box<PrecisionDocument>, String> {
        let file = cpath(path);
        let working = CString::new(WORKING_GRAY).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            match self {
                Mode::Gray8 | Mode::Gray16 => dr_precision_doc_open_gray(
                    file.as_ptr(),
                    working.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                ),
                Mode::Lab8 | Mode::Lab16 => {
                    dr_precision_doc_open_lab(file.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
                }
                Mode::Indexed => {
                    dr_precision_doc_open_indexed(file.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
                }
                Mode::Bitmap => dr_precision_doc_open_bitmap(
                    file.as_ptr(),
                    working.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                ),
                Mode::Float | Mode::Gray32 => {
                    dr_precision_doc_open(file.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
                }
            }
        };
        taken(out, err)
    }
}

fn import_warnings(doc: &PrecisionDocument) -> Vec<String> {
    serde_json::from_value(doc.info().unwrap()["import_warnings"].clone()).unwrap()
}

/// The failing checks of one mode, each named.
struct Checks {
    mode: Mode,
    failures: Vec<String>,
}

impl Checks {
    fn check(&mut self, ok: bool, what: impl std::fmt::Display) {
        if !ok {
            self.failures.push(format!("{:?}: {what}", self.mode));
        }
    }
}

/// Each mode's flat TIFF writes the document's paths and its clipping path
/// as the published records, its open reads them back in the same mode, a
/// deleted path is not written again, a file stored rotated reads, keeps and
/// writes none and says so, and the flat save names "paths" only when the
/// file drops one.
#[test]
fn every_modes_flat_tiff_carries_its_paths_and_clipping_path() {
    let dir = TempDir::new().unwrap();
    let mut failures = Vec::new();
    for mode in MODES {
        let mut c = Checks {
            mode,
            failures: Vec::new(),
        };
        let doc = mode.document();
        let want = doc.paths().to_vec();
        let expected = expected_resources(&want, (W, H));
        let name = |what: &str| dir.path().join(format!("{mode:?}-{what}.tif"));

        let file = mode.save(&doc, &name("first")).expect("the save");
        let found = path_resources(&file);
        c.check(
            found == expected,
            format!("the file's path resources {found:?}"),
        );
        match mode.open(&name("first")) {
            Err(e) => c.check(false, format!("the reopen: {e}")),
            Ok(back) => {
                c.check(mode.is_mode(&back), "the reopen is in the same mode");
                c.check(
                    back.paths().len() == want.len(),
                    format!("the reopen holds {} paths", back.paths().len()),
                );
                if back.paths().len() == want.len() {
                    assert_paths_close(back.paths(), &want, &format!("{mode:?} reopen"));
                }
                c.check(clipping_names(back.paths()) == ["Box"], "the clipping path");
                let ids = held(back.metadata().iptc.as_deref());
                c.check(
                    !ids.iter().any(|id| is_path_id(*id)),
                    format!("the reopen's run holds {ids:?}"),
                );
                let cleared = back.with_paths(Vec::new());
                c.check(cleared.is_some(), "the paths delete");
                if let Some(cleared) = cleared {
                    let file = mode.save(&cleared, &name("cleared")).expect("the save");
                    let found = path_resources(&file);
                    c.check(found.is_empty(), format!("deleted, it writes {found:?}"));
                }
            }
        }

        std::fs::write(name("rotated"), rotated(file)).unwrap();
        match mode.open(&name("rotated")) {
            Err(e) => c.check(false, format!("the rotated open: {e}")),
            Ok(turned) => {
                c.check(turned.paths().is_empty(), "rotated: no path read");
                let ids = held(turned.metadata().iptc.as_deref());
                c.check(
                    !ids.iter().any(|id| is_path_id(*id)),
                    format!("rotated: the run holds {ids:?}"),
                );
                let warnings = import_warnings(&turned);
                c.check(
                    warnings.iter().any(|w| w == ROTATED),
                    format!("rotated: the warnings {warnings:?}"),
                );
                let file = mode.save(&turned, &name("turned")).expect("the save");
                let found = path_resources(&file);
                c.check(found.is_empty(), format!("rotated, saved again: {found:?}"));
            }
        }

        let paths = "paths".to_string();
        c.check(
            !typed_losses(&doc, true).contains(&paths),
            "a TIFF loses no path",
        );
        c.check(
            typed_losses(&doc, false).contains(&paths),
            "a flat PNG loses them",
        );
        let mut far = path_list();
        far["paths"].as_array_mut().unwrap().push(json!({
            "kind": "saved", "name": "Far", "subpaths": [
                {"closed": false, "anchors": [[0, 0], [10000, 0]]}]}));
        let far = typed_with_list(&doc, &far);
        c.check(
            typed_losses(&far, true).contains(&paths),
            "a path past 128 canvases is lost in a TIFF",
        );
        failures.extend(c.failures);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The tags of a classic TIFF's first directory.
fn first_directory_tags(file: &[u8]) -> Vec<u16> {
    assert_eq!(&file[..2], b"II", "this writer is little-endian");
    let ifd = u32::from_le_bytes(file[4..8].try_into().unwrap()) as usize;
    let count = usize::from(u16::from_le_bytes([file[ifd], file[ifd + 1]]));
    (0..count)
        .map(|e| {
            let at = ifd + 2 + e * 12;
            u16::from_le_bytes([file[at], file[at + 1]])
        })
        .collect()
}

/// A 32-bit document from a JPEG holding an EXIF packet and an IPTC caption
/// (resource 1028) exports a float TIFF holding both, as its `carried` bits
/// say, and reopens with both; at 32 bits as RGB and as Grayscale.
#[test]
fn a_32_bit_tiff_carries_the_exif_packet_and_the_run() {
    use crate::common::metadata_fixtures::build_exif;
    use doctored_core::ffi_doc::dr_doc_open;

    let dir = TempDir::new().unwrap();
    let jpeg = ramp_jpeg(false);
    let at = 4 + usize::from(u16::from_be_bytes([jpeg[4], jpeg[5]]));
    // With an Exif sub-IFD, which a TIFF points to from its first directory.
    let exif = build_exif(false, 1, (72, 72), Some((W, H)), false).bytes;
    let caption = block(1028, "", &[0x1c, 0x02, 0x00, 0x00, 0x00]);
    let file = [
        &jpeg[..at],
        &segment(0xE1, &[b"Exif\0\0".as_slice(), &exif].concat()),
        &segment(0xED, &[b"Photoshop 3.0\0".as_slice(), &caption].concat()),
        &jpeg[at..],
    ]
    .concat();
    let source = dir.path().join("source.jpg");
    std::fs::write(&source, file).unwrap();
    let mut err = ptr::null_mut();
    let byte = unsafe { dr_doc_open(cpath(&source).as_ptr(), std::ptr::null(), &mut err) };
    assert!(!byte.is_null(), "{}", take_err_string(err));
    let byte = unsafe { Box::from_raw(byte) };
    let float = to_float(&promoted(&byte));
    let gray = to_gray(&float, &json!({"space": WORKING_GRAY}));
    for (doc, what) in [(&float, "RGB"), (&gray, "Grayscale")] {
        assert!(doc.metadata().exif.is_some(), "{what}: the document's EXIF");
        assert_eq!(
            held(doc.metadata().iptc.as_deref()),
            [1028],
            "{what}: its run"
        );
        let request = json!({"format": "tiff", "profile": "document", "bit_depth": 32});
        let text = CString::new(request.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_export_encode(
                &**doc,
                ptr::null(),
                text.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!out.is_null(), "{what}: {}", take_err_string(err));
        let result: Box<ExportResult> = unsafe { Box::from_raw(out) };
        let carried = result.info_json()["carried"].as_u64().unwrap();
        assert_eq!(carried & 0b1010, 0b1010, "{what}: carried EXIF and IPTC");
        let bytes = result.bytes().to_vec();
        assert!(
            first_directory_tags(&bytes).contains(&34665),
            "{what}: the EXIF IFD pointer"
        );
        assert_eq!(held(Some(&tiff_run(&bytes))), [1028], "{what}: tag 34377");
        let path = dir.path().join(format!("{what}.tif"));
        std::fs::write(&path, &bytes).unwrap();
        let back = Mode::Float.open(&path).unwrap();
        assert!(back.metadata().exif.is_some(), "{what}: the reopen's EXIF");
        assert_eq!(
            held(back.metadata().iptc.as_deref()),
            [1028],
            "{what}: reopen"
        );
    }
}
