//! The Background mark, layer comps and raster-less vector masks through
//! the PSD codecs of every typed colour mode and of 32 bits
//! (`psd_typed_exchange`), through the FFI: Grayscale at 8 and 16 bits,
//! Duotone at 8 and 16, Grayscale at 32, Lab at 8 and 16, CMYK at 8 and 16
//! and float RGB, over PSD, PSB and the layered TIFF where the mode writes
//! one.
//!
//! - **Background.** An unmarked opaque bottom under a Deep knockout is
//!   written as Photoshop's Background record — this file's own walk reads
//!   record 0's authored form: no channel `-1`, flag bit 0, `lnsr bgnd`,
//!   `lspf 0x0d` — and reopens marked Background, through `.doctored` and a
//!   second export too; a bottom with a transparent pixel stays an ordinary
//!   record with the export's sentence and reopens marked Ordinary.
//! - **Comps.** Two visibility comps and one that also records position:
//!   the file's 1065 names them, every record's `cmls` `LyrI` is its `lyid`,
//!   and the reopened document, and its `.doctored`, hold the same names,
//!   comments and per-layer visibility. The CMYK source is a CMYK file built
//!   here carrying the 1065 and `shmd` bodies of the RGB document's own
//!   16-bit file, so the comps arrive through a file's records (the comp
//!   tools on a CMYK document are `cmyk_comps.rs`'s).
//! - **Vector masks.** A raster-less vector mask on a raster and on a group
//!   is written as its `vmsk` (this file's records from Adobe's layout,
//!   `psd_paths::expected_records`) with no `-2` channel, and reopens with
//!   its shape, enabled and linked state; one beside a raster mask keeps its
//!   baked mask and its sentence.
//! - **The fill guard.** A `vmsk` over a fill record the mode's open does not
//!   import as a fill layer is not carried, so a shape layer keeps its stored
//!   pixels and the dropped-mask sentence (Gray and Lab at 8 bits, Gray at
//!   32, a gradient fill at 32), where a float `SoCo` imports as the fill
//!   under its vector mask.
//! - **Record 0 not kept.** A file whose record 0 the open drops marks no
//!   bottom (Gray, Lab, and a 32-bit Invert the float form refuses), and the
//!   32-bit open binds a vector mask to the layer built from its record.

use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_background::dr_precision_doc_background_role;
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_comps::dr_precision_doc_layer_comps_json;
use doctored_core::ffi_duotone::dr_precision_doc_set_duotone;
use doctored_core::ffi_gray::dr_precision_doc_convert_to_gray;
use doctored_core::ffi_lab::dr_precision_doc_convert_to_lab;
use doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{
    dr_precision_doc_open_gray_psd, dr_precision_doc_open_lab_psd, dr_precision_doc_open_psd,
    dr_precision_doc_psd_export_report, dr_precision_doc_save_psd,
};
use doctored_core::ffi_vector_mask::dr_precision_doc_set_vector_mask;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::path_geometry::PathShape;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES as LIMIT};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::style::LayerStyle;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_psd_fixture as cmyk_file;
use crate::common::psd_photoshop::{blocks_in, fixture as family_file, import_report, strings};
use crate::common::{cpath, take_err_string};
use crate::psd_comps::{lyr_i, names_in_1065, record_blocks, resource_body};
use crate::psd_mask_fixture as fixture;
use crate::psd_paths::{assert_shapes_close, closed_subpath, expected_records, vmsk, DROPPED};

const W: u32 = 16;
const H: u32 = 12;
const PSD: i32 = 0;
const PSB: i32 = 1;
const TIFF: i32 = 2;
const LAYERS: i32 = 0;

const ROLE_NONE: i32 = 0;
const ROLE_BACKGROUND: i32 = 1;
const ROLE_ORDINARY: i32 = 2;

const UNWRITABLE: &str = "so a PSD cannot store it as a Photoshop Background layer";
const BACKGROUND_SENTENCES: &str = "which Doctored treats as the Background";

// ------------------------------------------------------------- the modes --

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Gray8,
    Gray16,
    Duotone8,
    Duotone16,
    Gray32,
    Lab8,
    Lab16,
    Cmyk8,
    Cmyk16,
    Float,
}

const MODES: [Mode; 10] = [
    Mode::Gray8,
    Mode::Gray16,
    Mode::Duotone8,
    Mode::Duotone16,
    Mode::Gray32,
    Mode::Lab8,
    Mode::Lab16,
    Mode::Cmyk8,
    Mode::Cmyk16,
    Mode::Float,
];

impl Mode {
    /// The containers this mode's writer writes layered: a Duotone and a
    /// 32-bit Grayscale document refuse the layered TIFF.
    fn containers(self) -> &'static [(i32, &'static str)] {
        match self {
            Mode::Duotone8 | Mode::Duotone16 | Mode::Gray32 => &[(PSD, "psd"), (PSB, "psb")],
            _ => &[(PSD, "psd"), (PSB, "psb"), (TIFF, "tif")],
        }
    }

    fn cmyk(self) -> bool {
        matches!(self, Mode::Cmyk8 | Mode::Cmyk16)
    }
}

// ------------------------------------------------------------ plumbing --

fn owned(out: *mut PrecisionDocument, err: *mut c_char, what: &str) -> Box<PrecisionDocument> {
    assert!(!out.is_null(), "{what}: {}", take_err_string(err));
    unsafe { Box::from_raw(out) }
}

fn to_gray(doc: &PrecisionDocument, options: &str) -> Box<PrecisionDocument> {
    let options = CString::new(options).unwrap();
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
    owned(out, err, "to Gray")
}

fn to_float(doc: &PrecisionDocument) -> Box<PrecisionDocument> {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_promote_to_float(doc, LIMIT, 0, ptr::null(), &mut err) };
    owned(out, err, "to float")
}

fn to_lab(doc: &PrecisionDocument, depth: u32) -> Box<PrecisionDocument> {
    let options = CString::new(json!({ "depth": depth }).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_lab(doc, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    owned(out, err, "to Lab")
}

fn to_cmyk(doc: &PrecisionDocument, depth: u32) -> Box<PrecisionDocument> {
    let profile = press_profile("Press Test");
    let options = CString::new(json!({ "depth": depth }).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            doc,
            profile.as_ptr(),
            profile.len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    owned(out, err, "to CMYK")
}

/// Two inks, each a straight curve between its two ends.
fn to_duotone(gray: &PrecisionDocument) -> Box<PrecisionDocument> {
    let curve = |low: f64, high: f64| {
        let mut curve = vec![Value::Null; 13];
        curve[0] = json!(low);
        curve[12] = json!(high);
        Value::Array(curve)
    };
    let options = json!({"inks": [
        {"name": "Warm Black", "lab": [20, 2, 4], "curve": curve(0.0, 100.0)},
        {"name": "Rust", "lab": [55, 45, 55], "curve": curve(10.0, 70.0)}]});
    let options = CString::new(options.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_duotone(gray, options.as_ptr(), &mut err) };
    owned(out, err, "to Duotone")
}

/// The 16-bit sRGB `source` as a `mode` document, by the mode's own
/// conversion.
fn convert(source: &PrecisionDocument, mode: Mode) -> Box<PrecisionDocument> {
    let gray = |depth: u32| {
        to_gray(
            source,
            &json!({"space": "gray_gamma_22", "depth": depth}).to_string(),
        )
    };
    match mode {
        Mode::Gray8 => gray(8),
        Mode::Gray16 => gray(16),
        Mode::Duotone8 => to_duotone(&gray(8)),
        Mode::Duotone16 => to_duotone(&gray(16)),
        Mode::Gray32 => to_gray(&to_float(source), r#"{"space":"gray_gamma_22"}"#),
        Mode::Lab8 => to_lab(source, 8),
        Mode::Lab16 => to_lab(source, 16),
        Mode::Cmyk8 => to_cmyk(source, 8),
        Mode::Cmyk16 => to_cmyk(source, 16),
        Mode::Float => to_float(source),
    }
}

/// The layered file of `doc` in `container`, its own render the projection
/// (a CMYK document renders its own composite).
fn save(doc: &PrecisionDocument, path: &Path, container: i32) -> Vec<u8> {
    let projection = (!doc.is_cmyk()).then(|| doc.render(LIMIT, 0, None).expect("a projection"));
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            projection
                .as_ref()
                .map_or(ptr::null(), |p| p as *const PrecisionImage),
            cpath(path).as_ptr(),
            container,
            LAYERS,
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(ok, "save {}: {}", path.display(), take_err_string(err));
    std::fs::read(path).unwrap()
}

/// Import Layers of `path` through the open its report names.
fn open(path: &Path) -> Box<PrecisionDocument> {
    let report = import_report(path);
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let file = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let doc = unsafe {
        match report["import_precision"].as_str() {
            Some("gray") => dr_precision_doc_open_gray_psd(
                file.as_ptr(),
                LAYERS,
                token.as_ptr(),
                c"dot_gain_20".as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            ),
            Some("lab") => dr_precision_doc_open_lab_psd(
                file.as_ptr(),
                LAYERS,
                token.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            ),
            _ => dr_precision_doc_open_psd(
                file.as_ptr(),
                LAYERS,
                token.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            ),
        }
    };
    owned(doc, err, &format!("open {}", path.display()))
}

/// The document through `.doctored` and back.
fn native(doc: &PrecisionDocument) -> PrecisionDocument {
    let bytes = doc.encode_native(LIMIT, 0, None).expect("encode native");
    PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("parse native")
}

fn role(doc: &PrecisionDocument) -> i32 {
    unsafe { dr_precision_doc_background_role(doc) }
}

fn export_losses(doc: &PrecisionDocument) -> Vec<String> {
    let mut err: *mut c_char = ptr::null_mut();
    let text = unsafe { dr_precision_doc_psd_export_report(doc, PSD, LAYERS, &mut err) };
    assert!(!text.is_null(), "export report: {}", take_err_string(err));
    let value: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    strings(&value, "losses")
}

fn edit(doc: &PrecisionDocument, request: Value) -> Box<PrecisionDocument> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    owned(out, err, &request.to_string())
}

fn index(doc: &PrecisionDocument, name: &str) -> usize {
    (0..doc.layer_count())
        .find(|&i| doc.layer(i).is_some_and(|l| l.name() == name))
        .unwrap_or_else(|| panic!("no layer {name}"))
}

// ------------------------------------------------------ the source docs --

/// A canvas-sized sRGB raster of `rgba`, its pixel (0, 0) at `corner_alpha`.
fn raster(name: &str, rgba: [u16; 4], corner_alpha: u16) -> Layer {
    let mut pixels = vec![rgba; (W * H) as usize];
    pixels[0][3] = corner_alpha;
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    Layer::raster(name, image, (0, 0), 1.0, true).unwrap()
}

fn document(layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

/// `Bottom` (opaque, or with one transparent pixel) under `Top`, a Deep
/// knockout at fill 0.
fn knockout_source(holed: bool) -> PrecisionDocument {
    let style = LayerStyle::from_json(r#"{"knockout":"deep","fill_opacity":0}"#).unwrap();
    let top = raster("Top", [50_000, 10_000, 20_000, 65535], 65535)
        .with_layer_style(Some(&style), (W, H))
        .unwrap();
    document(vec![
        raster(
            "Bottom",
            [10_000, 30_000, 50_000, 65535],
            if holed { 0 } else { 65535 },
        ),
        top,
    ])
}

/// `Bottom`, `Mid`, the group `Set` holding `Inner`, and `Top` (with a
/// raster mask when `top_mask`): postorder indices 0, 1, 2 (Inner), 3 (Set),
/// 4; FILE records Bottom, Mid, Set's closer, Inner, Set, Top.
fn tree_source(top_mask: bool) -> PrecisionDocument {
    let inner = raster("Inner", [20_000, 40_000, 60_000, 65535], 65535);
    let set = Layer::isolated_normal("Set", vec![inner], 1.0, true).unwrap();
    let mut top = raster("Top", [30_000, 30_000, 30_000, 65535], 65535);
    if top_mask {
        let plane = PrecisionPlane::from_u16(W, H, vec![40_000; (W * H) as usize], None).unwrap();
        let mask = PrecisionMask::new(plane, MaskProperties::at((0, 0)), true).unwrap();
        top = top.with_mask(Some(mask));
    }
    document(vec![
        raster("Bottom", [10_000, 30_000, 50_000, 65535], 65535),
        raster("Mid", [60_000, 20_000, 10_000, 40_000], 40_000),
        set,
        top,
    ])
}

/// [`tree_source`] with three comps: "Both" (everything shown), "No Mid"
/// (Mid hidden, with a comment) and "Where" (Inner hidden; it records
/// position too, which a PSD does not carry).
fn comps_source() -> PrecisionDocument {
    let save = |doc: &PrecisionDocument, name: &str, comment: &str, position: bool| {
        edit(
            doc,
            json!({"op": "layer_comp", "action": "save", "name": name, "comment": comment,
                   "visibility": true, "position": position, "appearance": false}),
        )
    };
    let visible = |doc: &PrecisionDocument, name: &str, visible: bool| {
        edit(
            doc,
            json!({"op": "layer_visible", "layer": index(doc, name), "visible": visible}),
        )
    };
    let doc = save(&tree_source(false), "Both", "", false);
    let doc = visible(&doc, "Mid", false);
    let doc = save(&doc, "No Mid", "Mid is off", false);
    let doc = visible(&doc, "Mid", true);
    let doc = visible(&doc, "Inner", false);
    let doc = save(&doc, "Where", "", true);
    *visible(&doc, "Inner", true)
}

/// A triangle on the canvas.
fn triangle(at: (f64, f64)) -> Value {
    json!({"fill_rule": "nonzero", "subpaths": [{"closed": true, "anchors": [
        [at.0, at.1], [at.0 + 9.0, at.1 + 1.5], [at.0 + 4.5, at.1 + 7.25]]}]})
}

fn with_vector_mask(
    doc: &PrecisionDocument,
    layer: usize,
    shape: &Value,
) -> Box<PrecisionDocument> {
    let text = CString::new(shape.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_set_vector_mask(doc, layer, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    owned(out, err, "set vector mask")
}

/// [`tree_source`] with a raster-less vector mask on `Mid` and on the group
/// `Set`, and one beside a raster mask on `Top`.
fn masks_source() -> PrecisionDocument {
    let doc = tree_source(true);
    let doc = with_vector_mask(&doc, index(&doc, "Mid"), &triangle((2.0, 1.0)));
    let doc = with_vector_mask(&doc, index(&doc, "Set"), &triangle((5.5, 3.25)));
    *with_vector_mask(&doc, index(&doc, "Top"), &triangle((1.0, 4.0)))
}

// ------------------------------------------------------ this file's walk --

fn be32(bytes: &[u8], at: usize) -> usize {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

/// One layer record of a classic PSD, by Adobe's layout: its channel ids,
/// its flags byte and its additional blocks.
struct FileRecord {
    channels: Vec<i16>,
    flags: u8,
    blocks: Vec<([u8; 4], Vec<u8>)>,
}

impl FileRecord {
    fn block(&self, key: &[u8; 4]) -> Option<&[u8]> {
        self.blocks
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, body)| body.as_slice())
    }

    /// Photoshop's authored Background form (`knockout/source.psd` record 0).
    fn is_background(&self) -> bool {
        !self.channels.contains(&-1)
            && self.flags & 1 == 1
            && self.block(b"lnsr") == Some(b"bgnd".as_slice())
            && self.block(b"lspf") == Some([0u8, 0, 0, 0x0d].as_slice())
    }
}

/// The layer information of a classic PSD: the section's own, or a 16- or
/// 32-bit file's `Lr16`/`Lr32` tail block when the section is empty.
fn layer_info(bytes: &[u8]) -> &[u8] {
    assert_eq!(&bytes[..6], b"8BPS\0\x01", "a classic PSD");
    let mut at = 26;
    at += 4 + be32(bytes, at); // colour-mode data
    at += 4 + be32(bytes, at); // image resources
    let end = at + 4 + be32(bytes, at);
    at += 4;
    let info = be32(bytes, at);
    if info != 0 {
        return &bytes[at + 4..at + 4 + info];
    }
    at += 4;
    at += 4 + be32(bytes, at); // global layer mask
    while at + 12 <= end {
        let key = &bytes[at + 4..at + 8];
        let len = be32(bytes, at + 8);
        if matches!(key, b"Lr16" | b"Lr32") {
            return &bytes[at + 12..at + 12 + len];
        }
        at += 12 + len;
    }
    panic!("the file holds no layer records");
}

fn file_records(bytes: &[u8]) -> Vec<FileRecord> {
    let info = layer_info(bytes);
    let count = i16::from_be_bytes(info[..2].try_into().unwrap()).unsigned_abs();
    let mut at = 2;
    let mut out = Vec::new();
    for _ in 0..count {
        at += 16;
        let n = usize::from(u16::from_be_bytes(info[at..at + 2].try_into().unwrap()));
        at += 2;
        let channels = (0..n)
            .map(|i| i16::from_be_bytes(info[at + 6 * i..at + 6 * i + 2].try_into().unwrap()))
            .collect();
        at += 6 * n;
        assert_eq!(&info[at..at + 4], b"8BIM");
        let flags = info[at + 10];
        let extra_len = be32(info, at + 12);
        let extra = &info[at + 16..at + 16 + extra_len];
        at += 16 + extra_len;
        let mut cursor = 4 + be32(extra, 0); // mask data
        cursor += 4 + be32(extra, cursor); // blending ranges
        cursor += (usize::from(extra[cursor]) + 4) & !3; // Pascal name
        let mut blocks = Vec::new();
        while cursor + 12 <= extra.len() {
            let key: [u8; 4] = extra[cursor + 4..cursor + 8].try_into().unwrap();
            let len = be32(extra, cursor + 8);
            blocks.push((key, extra[cursor + 12..cursor + 12 + len].to_vec()));
            cursor += 12 + len + len % 2;
        }
        out.push(FileRecord {
            channels,
            flags,
            blocks,
        });
    }
    out
}

// ----------------------------------------------------------- Background --

#[test]
fn the_background_mark_crosses_every_mode() {
    let dir = TempDir::new().unwrap();
    for mode in MODES {
        let what = format!("{mode:?}");
        // An unmarked opaque bottom under a Deep knockout: the Background.
        let doc = convert(&knockout_source(false), mode);
        assert_eq!(role(&doc), ROLE_NONE, "{what}: unmarked");
        let losses = export_losses(&doc);
        assert!(
            !losses.iter().any(|l| l.contains(BACKGROUND_SENTENCES)),
            "{what}: nothing to say: {losses:?}"
        );
        for &(container, ext) in mode.containers() {
            let path = dir.path().join(format!("bg-{what}.{ext}"));
            let bytes = save(&doc, &path, container);
            let back = open(&path);
            assert_eq!(role(&back), ROLE_BACKGROUND, "{what} {ext}: reopened");
            if container != PSD {
                continue;
            }
            assert!(
                file_records(&bytes)[0].is_background(),
                "{what}: record 0 is the authored Background"
            );
            // …through `.doctored`, and written again.
            let again = native(&back);
            assert_eq!(role(&again), ROLE_BACKGROUND, "{what}: .doctored");
            let path = dir.path().join(format!("bg-{what}-again.psd"));
            let bytes = save(&again, &path, PSD);
            assert!(file_records(&bytes)[0].is_background(), "{what}: again");
            assert_eq!(role(&open(&path)), ROLE_BACKGROUND, "{what}: again");
        }

        // One transparent pixel: an ordinary record, said by the export.
        let doc = convert(&knockout_source(true), mode);
        let losses = export_losses(&doc);
        assert!(
            losses.iter().any(|l| l.contains(UNWRITABLE)),
            "{what}: the transparent bottom is said: {losses:?}"
        );
        let path = dir.path().join(format!("holed-{what}.psd"));
        let record = &file_records(&save(&doc, &path, PSD))[0];
        assert!(
            record.channels.contains(&-1) && !record.is_background(),
            "{what}: an ordinary record 0"
        );
        let back = open(&path);
        assert_eq!(role(&back), ROLE_ORDINARY, "{what}: reopened ordinary");
        // An Ordinary mark is written as an ordinary record, with nothing to
        // say, and stays Ordinary.
        assert!(
            !export_losses(&back)
                .iter()
                .any(|l| l.contains(BACKGROUND_SENTENCES)),
            "{what}: an Ordinary bottom says nothing"
        );
        let path = dir.path().join(format!("holed-{what}-again.psd"));
        assert!(file_records(&save(&native(&back), &path, PSD))[0]
            .channels
            .contains(&-1));
        assert_eq!(role(&open(&path)), ROLE_ORDINARY, "{what}: again");
    }
}

// ---------------------------------------------------------------- comps --

/// Each comp as (name, comment, [(layer name, visible)] sorted).
type CompRow = (String, String, Vec<(String, bool)>);

fn comp_rows(doc: &PrecisionDocument) -> Vec<CompRow> {
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_precision_doc_layer_comps_json(doc, &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    let report: Value = serde_json::from_str(&take_err_string(raw)).unwrap();
    report["comps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|comp| {
            let mut entries: Vec<(String, bool)> = comp["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    assert_eq!(e["stale"], false, "{e}");
                    let layer = e["layer"].as_u64().expect("a bound entry") as usize;
                    (
                        doc.layer(layer).unwrap().name().to_string(),
                        e["visible"].as_bool().unwrap(),
                    )
                })
                .collect();
            entries.sort();
            (
                comp["name"].as_str().unwrap().to_string(),
                comp["comment"].as_str().unwrap().to_string(),
                entries,
            )
        })
        .collect()
}

/// A CMYK file of [`tree_source`]'s records carrying the 1065 and `shmd`
/// bodies of `rgb`, the 16-bit RGB file of the same tree (module doc).
fn cmyk_comps_file(rgb: &[u8], depth: u16) -> Vec<u8> {
    let shmd: Vec<Option<Vec<u8>>> = record_blocks(rgb)
        .into_iter()
        .map(|blocks| {
            blocks
                .into_iter()
                .find(|(key, _)| key == b"shmd")
                .map(|(_, body)| body)
        })
        .collect();
    let ink = vec![[20_000u16, 30_000, 10_000, 5_000, 65535]; (W * H) as usize];
    let raster = |name: &str| {
        cmyk_file::Rec::raster(
            name,
            [0, 0, H as i32, W as i32],
            cmyk_file::ink_channels(&ink, W as usize, depth, true, cmyk_file::Comp::Rle, false),
        )
    };
    let records = [
        raster("Bottom"),
        raster("Mid"),
        cmyk_file::Rec::folder("</Layer group>", 3, *b"norm"),
        raster("Inner"),
        cmyk_file::Rec::folder("Set", 1, *b"norm"),
        raster("Top"),
    ];
    assert_eq!(shmd.len(), records.len(), "the same records");
    let records: Vec<cmyk_file::Rec> = records
        .into_iter()
        .zip(shmd)
        .map(|(record, body)| match body {
            Some(body) => record.with_block(b"shmd", &body),
            None => record,
        })
        .collect();
    let profile = press_profile("Press Test");
    let max: u16 = if depth == 8 { 255 } else { 65535 };
    cmyk_file::psd(
        &cmyk_file::PsdSpec {
            canvas: (W, H),
            depth,
            icc: Some(&profile),
            composite: vec![vec![max; (W * H) as usize]; 4],
            merged_alpha: false,
            large: false,
            lr16: false,
            resources: vec![(1065, resource_body(rgb, 1065).expect("1065"))],
        },
        &records,
    )
}

#[test]
fn layer_comps_cross_every_mode() {
    let dir = TempDir::new().unwrap();
    let source = comps_source();
    let want = comp_rows(&source);
    assert_eq!(want.len(), 3);
    let rgb = save(&source, &dir.path().join("rgb16.psd"), PSD);
    for mode in MODES {
        let what = format!("{mode:?}");
        let doc = if mode.cmyk() {
            let path = dir.path().join(format!("comps-{what}-source.psd"));
            let depth = if mode == Mode::Cmyk8 { 8 } else { 16 };
            std::fs::write(&path, cmyk_comps_file(&rgb, depth)).unwrap();
            open(&path)
        } else {
            convert(&source, mode)
        };
        assert_eq!(comp_rows(&doc), want, "{what}: the source's comps");
        let losses = export_losses(&doc);
        assert!(
            !losses
                .iter()
                .any(|l| l.contains("stores no Photoshop layer comps")),
            "{what}: {losses:?}"
        );
        if !mode.cmyk() {
            // The CMYK source came through a PSD, which already narrowed it.
            assert!(
                losses.iter().any(|l| l == "1 layer comp is written with its visibility only; the layer position and appearance it records are not written."),
                "{what}: {losses:?}"
            );
        }
        for &(container, ext) in mode.containers() {
            let path = dir.path().join(format!("comps-{what}.{ext}"));
            let bytes = save(&doc, &path, container);
            let back = open(&path);
            assert_eq!(comp_rows(&back), want, "{what} {ext}: reopened");
            if container != PSD {
                continue;
            }
            assert_eq!(
                names_in_1065(&resource_body(&bytes, 1065).expect("1065 is written")),
                ["Both", "No Mid", "Where"],
                "{what}"
            );
            for record in file_records(&bytes) {
                let lyid = record.block(b"lyid").expect("lyid");
                let lyid = i32::from_be_bytes(lyid[..4].try_into().unwrap());
                if let Some(shmd) = record.block(b"shmd") {
                    assert_eq!(lyr_i(shmd), lyid, "{what}: each LyrI is its lyid");
                }
            }
            assert_eq!(comp_rows(&native(&back)), want, "{what}: .doctored");
        }
    }
}

// --------------------------------------------------------- vector masks --

#[test]
fn raster_less_vector_masks_cross_every_mode() {
    let dir = TempDir::new().unwrap();
    let source = masks_source();
    let shape = |doc: &PrecisionDocument, name: &str| -> PathShape {
        doc.vector_mask(index(doc, name))
            .unwrap_or_else(|| panic!("{name}'s vector mask"))
            .shape
    };
    let mid = shape(&source, "Mid");
    let set = shape(&source, "Set");
    for mode in MODES {
        let what = format!("{mode:?}");
        let doc = convert(&source, mode);
        assert_shapes_close(&shape(&doc, "Mid"), &mid, &what);
        assert_shapes_close(&shape(&doc, "Set"), &set, &what);
        // A mode whose conversion re-sets only raster-less vector masks
        // (`cmyk_convert`) keeps Top's raster mask alone.
        let top_vector = doc.vector_mask(index(&doc, "Top")).is_some();
        let losses = export_losses(&doc);
        for name in ["Mid", "Set"] {
            assert!(
                !losses
                    .iter()
                    .any(|l| l.starts_with(&format!("The vector mask on “{name}”"))),
                "{what}: {name}: {losses:?}"
            );
        }
        assert_eq!(
            losses.iter().any(|l| l
                == "The vector mask on “Top” is written as pixels, an ordinary layer mask of its rendered shape; its path is not written."),
            top_vector,
            "{what}: {losses:?}"
        );
        for &(container, ext) in mode.containers() {
            let path = dir.path().join(format!("masks-{what}.{ext}"));
            let bytes = save(&doc, &path, container);
            let back = open(&path);
            for (name, want) in [("Mid", &mid), ("Set", &set)] {
                let view = back
                    .vector_mask(index(&back, name))
                    .unwrap_or_else(|| panic!("{what} {ext}: {name}'s vector mask"));
                assert!(
                    view.enabled && view.linked && !view.has_raster,
                    "{what} {ext}: {name}"
                );
                assert_shapes_close(&view.shape, want, &format!("{what} {ext}: {name}"));
            }
            let top = back.layer(index(&back, "Top")).unwrap();
            assert!(
                back.vector_mask(index(&back, "Top")).is_none() && top.mask().is_some(),
                "{what} {ext}: Top keeps its baked mask"
            );
            if container != PSD {
                continue;
            }
            // FILE order: Bottom, Mid, Set's closer, Inner, Set, Top.
            let records = file_records(&bytes);
            for (r, name, want) in [(1, "Mid", &mid), (4, "Set", &set)] {
                let mut body = 3u32.to_be_bytes().to_vec();
                body.extend_from_slice(&0u32.to_be_bytes());
                body.extend(expected_records(want, (f64::from(W), f64::from(H))));
                body.resize(body.len().next_multiple_of(4), 0);
                assert_eq!(
                    records[r].block(b"vmsk"),
                    Some(body.as_slice()),
                    "{what}: {name}'s vmsk"
                );
                assert!(!records[r].channels.contains(&-2), "{what}: {name}");
            }
            assert!(
                records[5].block(b"vmsk").is_none() && records[5].channels.contains(&-2),
                "{what}: Top's baked mask"
            );
            let again = native(&back);
            for name in ["Mid", "Set"] {
                assert!(
                    again.vector_mask(index(&again, name)).is_some(),
                    "{what}: .doctored keeps {name}'s"
                );
            }
        }
    }
}

// ------------------------------------------------------ synthesized files --

/// A rectangle `vmsk` over (2, 1)–(13, 9) on the canvas.
fn rectangle_vmsk() -> Vec<u8> {
    vmsk(
        0,
        &[closed_subpath(
            &[(2.0, 1.0), (13.0, 1.0), (13.0, 9.0), (2.0, 9.0)],
            1,
            (f64::from(W), f64::from(H)),
        )],
    )
}

/// An 8-bit record over the canvas holding `rgba` (Gray when `gray`).
fn record8(rgba: [u8; 4], gray: bool) -> fixture::Record {
    fixture::Record::raster(
        [0, 0, H as i32, W as i32],
        &rgba.repeat((W * H) as usize),
        gray,
    )
}

/// A pixel-less 8-bit record carrying `block`.
fn pixel_less8(key: &[u8; 4], body: &[u8], gray: bool) -> fixture::Record {
    let mut record = record8([0; 4], gray);
    record.bounds = [0; 4];
    for (_, channel) in &mut record.channels {
        *channel = vec![0, 0];
    }
    record.flags = 0x18;
    record.blocks = fixture::block(key, body);
    record
}

/// `records` as an 8-bit file: Gray, or Lab — the RGB layout with colour
/// mode 9, its planes L, a, b.
fn file8(records: &[fixture::Record], lab: bool) -> Vec<u8> {
    let mut bytes = fixture::fixture(records, [W, H], !lab);
    if lab {
        bytes[24..26].copy_from_slice(&9u16.to_be_bytes());
    }
    bytes
}

/// A record over the canvas of one f32 sample per plane: Gray (`-1`, `0`)
/// or RGB (`-1`, `0`–`2`), Raw.
fn record32(value: f32, gray: bool) -> fixture::Record {
    let plane = |v: f32| {
        let mut out = vec![0, 0];
        for _ in 0..W * H {
            out.extend_from_slice(&v.to_be_bytes());
        }
        out
    };
    let ids: &[i16] = if gray { &[-1, 0] } else { &[-1, 0, 1, 2] };
    let mut record = record8([0; 4], true);
    record.channels = ids
        .iter()
        .map(|&id| (id, plane(if id == -1 { 1.0 } else { value })))
        .collect();
    record
}

/// `records` as a classic 32-bit file in Photoshop's 32-bit layout (an
/// empty layer-information section and global mask, the records in `Lr32`,
/// a Raw composite of 1.0): colour mode 1 when `gray`, else 3.
fn file32(records: &[fixture::Record], gray: bool) -> Vec<u8> {
    let mut info = (records.len() as i16).to_be_bytes().to_vec();
    for record in records {
        info.extend(record.encode());
    }
    for record in records {
        for (_, channel) in &record.channels {
            info.extend(channel);
        }
    }
    let mut section = 0u32.to_be_bytes().to_vec();
    section.extend(0u32.to_be_bytes());
    section.extend(fixture::block(b"Lr32", &info));
    while !section.len().is_multiple_of(4) {
        section.push(0);
    }
    let planes: u16 = if gray { 1 } else { 3 };
    let mut bytes = b"8BPS\0\x01\0\0\0\0\0\0".to_vec();
    bytes.extend(planes.to_be_bytes());
    bytes.extend(H.to_be_bytes());
    bytes.extend(W.to_be_bytes());
    bytes.extend(32u16.to_be_bytes());
    bytes.extend((if gray { 1u16 } else { 3 }).to_be_bytes());
    bytes.extend(0u32.to_be_bytes());
    bytes.extend(0u32.to_be_bytes());
    bytes.extend((section.len() as u32).to_be_bytes());
    bytes.extend(section);
    bytes.extend(0u16.to_be_bytes());
    for _ in 0..u32::from(planes) * W * H {
        bytes.extend(1.0f32.to_be_bytes());
    }
    bytes
}

fn soco8() -> Vec<u8> {
    crate::common::psd_photoshop::soco_body()
}

fn soco32() -> Vec<u8> {
    let bytes = std::fs::read(family_file("float32_679", "fill_style32.psd")).unwrap();
    blocks_in(&bytes, b"SoCo")
        .into_iter()
        .next()
        .expect("fill_style32.psd's SoCo")
}

fn gdfl() -> Vec<u8> {
    let bytes = std::fs::read(family_file("gradient_fill_layer", "GL8.psd")).unwrap();
    blocks_in(&bytes, b"GdFl")
        .into_iter()
        .next()
        .expect("GL8.psd's GdFl")
}

/// `record` as a shape layer: its fill block, then `vmsk`, its pixel bit.
fn shape_layer(mut record: fixture::Record, key: &[u8; 4], body: &[u8]) -> fixture::Record {
    record.flags = 0x18;
    record.blocks = [
        fixture::block(key, body),
        fixture::block(b"vmsk", &rectangle_vmsk()),
    ]
    .concat();
    record
}

/// The dropped-mask sentence is reported, and the shape layer opens as its
/// stored pixels with no vector mask, opaque outside the path.
fn assert_kept_as_pixels(path: &Path, what: &str) {
    let losses = strings(&import_report(path), "verified_losses");
    assert!(
        losses.iter().any(|l| l.ends_with(DROPPED)),
        "{what}: {losses:?}"
    );
    let doc = open(path);
    assert_eq!(doc.layer_count(), 1, "{what}");
    assert_eq!(doc.layer(0).unwrap().name(), "Duplicate", "{what}");
    assert!(doc.vector_mask(0).is_none(), "{what}: no vector mask");
    let flat = doc.render(LIMIT, 0, None).unwrap();
    let alpha = match flat.to_rgba16() {
        Ok(pixels) => f64::from(pixels[(11 * W + 15) as usize][3]) / 65535.0,
        Err(_) => f64::from(flat.to_rgba32().unwrap()[(11 * W + 15) as usize][3]),
    };
    assert_eq!(alpha, 1.0, "{what}: its pixels outside the path");
}

#[test]
fn a_shape_layer_keeps_its_pixels_where_the_mode_imports_no_such_fill() {
    let dir = TempDir::new().unwrap();
    let write = |name: &str, bytes: Vec<u8>| {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    };
    // Gray and Lab drop every fill record, and a 32-bit Gray file too.
    let gray = write(
        "gray8.psd",
        file8(
            &[shape_layer(
                record8([90, 0, 0, 255], true),
                b"SoCo",
                &soco8(),
            )],
            false,
        ),
    );
    assert_kept_as_pixels(&gray, "Gray 8");
    let lab = write(
        "lab8.psd",
        file8(
            &[shape_layer(
                record8([90, 128, 128, 255], false),
                b"SoCo",
                &soco8(),
            )],
            true,
        ),
    );
    assert_kept_as_pixels(&lab, "Lab 8");
    let gray32 = write(
        "gray32.psd",
        file32(
            &[shape_layer(record32(0.25, true), b"SoCo", &soco32())],
            true,
        ),
    );
    assert_kept_as_pixels(&gray32, "Gray 32");
    // A 32-bit gradient fill refuses the layered choice; under a `vmsk` it
    // stays a shape layer's pixels, so the file still opens layered.
    let gradient = write(
        "gradient32.psd",
        file32(
            &[shape_layer(record32(0.25, false), b"GdFl", &gdfl())],
            false,
        ),
    );
    assert_kept_as_pixels(&gradient, "GdFl 32");

    // A 32-bit Solid Color fill is imported: the fill under its vector mask.
    let solid = write(
        "solid32.psd",
        file32(
            &[shape_layer(record32(0.25, false), b"SoCo", &soco32())],
            false,
        ),
    );
    let doc = open(&solid);
    assert_eq!(doc.layer_count(), 1);
    let view = doc.vector_mask(0).expect("the fill's vector mask");
    assert!(view.enabled && view.linked && !view.has_raster);
    assert!(
        doc.artwork_description(0)
            .is_some_and(|d| d.contains("\"type\":\"fill\"")),
        "a fill description"
    );
    let flat = doc.render(LIMIT, 0, None).unwrap().to_rgba32().unwrap();
    assert_eq!(
        flat[(11 * W + 15) as usize][3],
        0.0,
        "nothing outside the path"
    );
    assert_eq!(flat[(5 * W + 7) as usize][3], 1.0, "the fill inside it");
}

// ------------------------------------------------------ record 0 dropped --

#[test]
fn record_0_the_open_drops_marks_no_bottom() {
    let dir = TempDir::new().unwrap();
    let write = |name: &str, bytes: Vec<u8>| {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    };
    // Record 0 is a fill, which a Gray and a Lab open drop.
    for (lab, name) in [(false, "gray.psd"), (true, "lab.psd")] {
        let path = write(
            name,
            file8(
                &[
                    pixel_less8(b"SoCo", &soco8(), !lab),
                    record8([90, 128, 128, 255], !lab),
                ],
                lab,
            ),
        );
        let doc = open(&path);
        assert_eq!(doc.layer_count(), 1, "{name}");
        assert_eq!(role(&doc), ROLE_NONE, "{name}: entry 0 is file record 1");
    }
    // Record 0 is an Invert, which the float form refuses; record 1's vector
    // mask lands on the layer built from it.
    let mut invert = record32(0.0, false);
    invert.bounds = [0; 4];
    for (_, channel) in &mut invert.channels {
        *channel = vec![0, 0];
    }
    invert.flags = 0x18;
    invert.blocks = fixture::block(b"nvrt", &[]);
    let mut masked = record32(0.25, false);
    masked.blocks = fixture::block(b"vmsk", &rectangle_vmsk());
    let path = write("float.psd", file32(&[invert, masked], false));
    let doc = open(&path);
    assert_eq!(doc.layer_count(), 1);
    assert_eq!(role(&doc), ROLE_NONE, "entry 0 is file record 1");
    assert!(
        doc.vector_mask(0).is_some(),
        "record 1's vector mask is on its layer"
    );
}
