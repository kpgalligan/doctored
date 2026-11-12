//! #1 phase 3b: a Gray document's PSD, PSB and layered-TIFF export,
//! through the C API.
//!
//! Oracles: the round trip reads our files back through the gray open (its
//! own oracles are `gray_psd.rs`'s byte-built files), and every layout move
//! (`psd_export_gray`'s module doc, M1 and M3–M6) is checked byte by byte,
//! by a reader written here from Adobe's published framing, against
//! Photoshop's OWN gray files — the read fixtures
//! `tests/fixtures/psd_photoshop/gray/` of `gray-files-547/take1` — read the
//! same way (the fixture half SKIPS with an eprintln when the folder is
//! absent).

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_gray::dr_precision_doc_convert_to_gray;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::ffi_precision_inspect::dr_precision_doc_profile;
use doctored_core::ffi_psd::{
    dr_precision_doc_open_gray_psd, dr_precision_doc_psd_export_report, dr_precision_doc_save_psd,
    dr_psd_import_report, dr_tiff_has_layers,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::gray_ops::{edit, LIMIT};

const W: u32 = 8;
const H: u32 = 6;
const IDENTITY_RANGE: [u8; 8] = [0, 0, 0xff, 0xff, 0, 0, 0xff, 0xff];
const IDENTITY_LEVELS: [u16; 5] = [0, 255, 0, 255, 100];

// ------------------------------------------------------------ the C API --

fn report_of(path: &Path) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value =
        serde_json::from_str(unsafe { std::ffi::CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn open(path: &Path, mode: i32, working: &str) -> Result<*mut PrecisionDocument, String> {
    let report = report_of(path);
    let fingerprint = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let working = CString::new(working).unwrap();
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
    if doc.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(doc)
    }
}

fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
}

fn profile_bytes(doc: *const PrecisionDocument) -> Vec<u8> {
    let mut length = 0;
    let bytes = unsafe { dr_precision_doc_profile(doc, &mut length) };
    assert!(!bytes.is_null());
    unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec()
}

fn samples(layer: &PrecisionDocumentLayer) -> Vec<[u16; 4]> {
    layer.source().unwrap().to_rgba16().unwrap()
}

fn index(doc: *const PrecisionDocument, name: &str) -> usize {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no record {name}"))
}

fn export_report(doc: *const PrecisionDocument, container: i32, mode: i32) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_psd_export_report(doc, container, mode, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value =
        serde_json::from_str(unsafe { std::ffi::CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn save(
    doc: *const PrecisionDocument,
    path: &Path,
    container: i32,
    mode: i32,
) -> Result<(), String> {
    let projection = unsafe { &*doc }.render(LIMIT, 0, None).unwrap();
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            &projection,
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

fn render16(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

// ------------------------------------------------------ the gray document --

fn pattern(i: u32, seed: u32) -> [u16; 4] {
    let r = (i * 7919 + seed * 101) % 251;
    let g = (i * 104_729 + seed * 37) % 241;
    let b = (i * 1299 + seed * 7) % 239;
    [(r * 257) as u16, (g * 265) as u16, (b * 270) as u16, 65535]
}

fn raster(
    name: &str,
    w: u32,
    h: u32,
    offset: (i32, i32),
    seed: u32,
    partial: bool,
) -> PrecisionDocumentLayer {
    let pixels: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let mut p = pattern(i, seed);
            if partial && i % 3 == 1 {
                p[3] = 30_000 + (i as u16) * 300;
            }
            p
        })
        .collect();
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocumentLayer::raster(name, image, offset, 1.0, true).unwrap()
}

fn then(doc: *mut PrecisionDocument, request: Value, input: &[u16]) -> *mut PrecisionDocument {
    let out = edit(doc, &request, input)
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"));
    free(doc);
    out
}

/// The structured Gray document (Dot Gain 20 %) at `depth`: an opaque
/// Backdrop, a masked Multiply 50 % Shape with partial alpha in a group, a
/// Color Overlay + Blend If Gray style on Top, and a Levels layer (master
/// only) whose black and white points sit on the `levl` record's 8-bit grid.
fn structured(depth: u32) -> *mut PrecisionDocument {
    let rgb = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            raster("Backdrop", W, H, (0, 0), 1, false),
            raster("Shape", 5, 4, (2, 1), 2, true),
            raster("Top", 4, 3, (1, 2), 3, false),
        ],
    )
    .unwrap();
    let rgb = Box::into_raw(Box::new(rgb));
    let options = CString::new(format!(r#"{{"space":"dot_gain_20","depth":{depth}}}"#)).unwrap();
    let mut err = ptr::null_mut();
    let mut doc = unsafe {
        dr_precision_doc_convert_to_gray(
            rgb,
            ptr::null(),
            0,
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    free(rgb);
    let mask: Vec<u16> = (0..20u16).map(|i| 65535 - i * 3000).collect();
    let shape = index(doc, "Shape");
    doc = then(
        doc,
        json!({"op":"set_mask","layer":shape,"width":5,"height":4,"x":2,"y":1,"linked":true}),
        &mask,
    );
    doc = then(
        doc,
        json!({"op":"layer_blend","version":1,"layer":shape,"blend_mode":1}),
        &[],
    );
    let faded = unsafe {
        doctored_core::ffi_precision::dr_precision_doc_set_layer_opacity(
            doc,
            shape,
            0.5,
            ptr::null_mut(),
        )
    };
    assert!(!faded.is_null());
    free(doc);
    doc = faded;
    let top = index(doc, "Top");
    doc = then(
        doc,
        json!({"op":"layer_style","version":1,"layer":top,"style":{"effects":[{"type":"color_overlay","color":"#c08040","opacity":0.7}]}}),
        &[],
    );
    doc = then(
        doc,
        json!({"op":"layer_blend_if","version":1,"layer":top,"blend_if":{"channel":"gray","this_layer":[20,40,220,240],"underlying":[0,0,255,255]}}),
        &[],
    );
    let shape = index(doc, "Shape");
    doc = then(
        doc,
        json!({"op":"group_layers","layers":[shape],"name":"Group"}),
        &[],
    );
    let name = CString::new("Levels").unwrap();
    let levels = CString::new(
        r#"{"type":"adjust","evaluation":"precision_v1","op":"levels","params":{"black":0.10196078431372549,"white":0.9019607843137255,"gamma":1.25}}"#,
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            doc,
            name.as_ptr(),
            levels.as_ptr(),
            1.0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    free(doc);
    out
}

/// Two trees alike: names, kinds, modes, opacity bytes, visibility,
/// clipping, offsets, the planes (`grid` = the 8-bit ×257 grid), masks,
/// styles and Blend If.
fn same_tree(a: &[PrecisionDocumentLayer], b: &[PrecisionDocumentLayer], grid: bool, what: &str) {
    assert_eq!(a.len(), b.len(), "{what}");
    let quantize = |v: u16| {
        if grid {
            ((u32::from(v) + 128) / 257 * 257) as u16
        } else {
            v
        }
    };
    for (x, y) in a.iter().zip(b) {
        let at = format!("{what} {}", x.name());
        assert_eq!(x.name(), y.name(), "{at}");
        assert_eq!(x.blend_mode(), y.blend_mode(), "{at}");
        assert_eq!(x.is_pass_through(), y.is_pass_through(), "{at}");
        assert_eq!(
            (x.opacity() * 255.0).round(),
            (y.opacity() * 255.0).round(),
            "{at}"
        );
        assert_eq!(x.visible(), y.visible(), "{at}");
        assert_eq!(x.clipped(), y.clipped(), "{at}");
        assert_eq!(x.offset(), y.offset(), "{at}");
        assert_eq!(x.blend_if(), y.blend_if(), "{at}");
        match (x.source(), y.source()) {
            (Some(p), Some(q)) => {
                let (p, q) = (p.to_rgba16().unwrap(), q.to_rgba16().unwrap());
                assert_eq!(p.len(), q.len(), "{at}");
                for (i, (s, t)) in p.iter().zip(&q).enumerate() {
                    assert_eq!(s.map(quantize), *t, "{at} pixel {i}");
                }
            }
            (None, None) => {}
            _ => panic!("{at}: kinds differ"),
        }
        match (x.mask(), y.mask()) {
            (Some(m), Some(n)) => {
                let want: Vec<u16> = m.plane().samples().iter().map(|v| quantize(*v)).collect();
                assert_eq!(n.plane().samples(), &want[..], "{at} mask");
            }
            (None, None) => {}
            _ => panic!("{at}: mask presence differs"),
        }
        match (x.reported_style(), y.reported_style()) {
            (Some(s), Some(t)) => {
                let (s, t): (Value, Value) = (
                    serde_json::from_str(&s).unwrap(),
                    serde_json::from_str(&t).unwrap(),
                );
                let (sc, tc) = (&s["effects"][0]["color"], &t["effects"][0]["color"]);
                let byte = |v: &Value| u8::from_str_radix(&v.as_str().unwrap()[1..3], 16).unwrap();
                assert!(
                    (i32::from(byte(sc)) - i32::from(byte(tc))).abs() <= 1,
                    "{at}: {s} against {t}"
                );
                assert_eq!(s["effects"][0]["type"], t["effects"][0]["type"], "{at}");
            }
            (None, None) => {}
            (s, t) => panic!("{at}: style {s:?} against {t:?}"),
        }
        if let (Some(p), Some(q)) = (x.children(), y.children()) {
            same_tree(p, q, grid, what);
        }
    }
}

// ---------------------------------------------- the file reader (Adobe's) --

fn be16(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([bytes[at], bytes[at + 1]])
}

fn be32(bytes: &[u8], at: usize) -> usize {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

fn be64(bytes: &[u8], at: usize) -> usize {
    u64::from_be_bytes(bytes[at..at + 8].try_into().unwrap()) as usize
}

/// One layer record as the file holds it.
struct Rec {
    name: String,
    flags: u8,
    ranges: Vec<u8>,
    blocks: Vec<([u8; 4], Vec<u8>)>,
}

impl Rec {
    fn block(&self, key: &[u8; 4]) -> Option<&[u8]> {
        self.blocks
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, body)| &body[..])
    }
}

/// A layer-information body (count, records, channel data): the signed
/// count and the records. `large` reads a PSB's u64 channel lengths.
fn records_of(info: &[u8], large: bool) -> (i16, Vec<Rec>) {
    let count = be16(info, 0) as i16;
    let mut at = 2;
    let mut out = Vec::new();
    for _ in 0..count.unsigned_abs() {
        let channels = usize::from(be16(info, at + 16));
        at += 18 + channels * if large { 10 } else { 6 };
        let flags = info[at + 10];
        at += 12;
        let extra = be32(info, at);
        let body = &info[at + 4..at + 4 + extra];
        at += 4 + extra;
        let mut c = 4 + be32(body, 0);
        let ranges_len = be32(body, c);
        let ranges = body[c + 4..c + 4 + ranges_len].to_vec();
        c += 4 + ranges_len;
        let pascal = usize::from(body[c]);
        let mut name = String::from_utf8_lossy(&body[c + 1..c + 1 + pascal]).into_owned();
        c += (1 + pascal + 3) & !3;
        let mut blocks = Vec::new();
        while c + 12 <= body.len() {
            let key: [u8; 4] = body[c + 4..c + 8].try_into().unwrap();
            let len = be32(body, c + 8);
            let block = body[c + 12..c + 12 + len].to_vec();
            if &key == b"luni" {
                let units: Vec<u16> = (0..be32(&block, 0))
                    .map(|i| be16(&block, 4 + 2 * i))
                    .collect();
                name = String::from_utf16_lossy(&units);
            }
            blocks.push((key, block));
            c += 12 + len;
        }
        out.push(Rec {
            name,
            flags,
            ranges,
            blocks,
        });
    }
    (count, out)
}

/// A PSD or PSB: (header channels, depth, colour mode, the signed record
/// count, the records), read from its layer-information section.
fn psd_layout(bytes: &[u8]) -> (u16, u16, u16, i16, Vec<Rec>) {
    assert_eq!(&bytes[..4], b"8BPS");
    let large = be16(bytes, 4) == 2;
    let (channels, depth, mode) = (be16(bytes, 12), be16(bytes, 22), be16(bytes, 24));
    let mut at = 26;
    at += 4 + be32(bytes, at);
    at += 4 + be32(bytes, at);
    at += if large { 8 } else { 4 };
    let len = if large {
        be64(bytes, at)
    } else {
        be32(bytes, at)
    };
    at += if large { 8 } else { 4 };
    // Photoshop's 16-bit files keep their records in an `Lr16` block; only
    // their header is read here.
    let (count, records) = if len == 0 {
        (0, Vec::new())
    } else {
        records_of(&bytes[at..at + len], large)
    };
    (channels, depth, mode, count, records)
}

/// One TIFF directory entry: (tag, type, count, the four value bytes).
type TiffEntry = (u16, u16, usize, [u8; 4]);

/// A big-endian TIFF's first directory as (tag, type, count, the four value
/// bytes), and its tag-37724 `Layr`/`Lr16` body.
fn tiff_layout(bytes: &[u8]) -> (Vec<TiffEntry>, Vec<u8>) {
    assert_eq!(&bytes[..4], b"MM\0*");
    let ifd = be32(bytes, 4);
    let mut entries = Vec::new();
    let mut block = Vec::new();
    for e in 0..usize::from(be16(bytes, ifd)) {
        let at = ifd + 2 + 12 * e;
        let (tag, kind, n) = (be16(bytes, at), be16(bytes, at + 2), be32(bytes, at + 4));
        let value: [u8; 4] = bytes[at + 8..at + 12].try_into().unwrap();
        if tag == 37724 {
            let offset = be32(&value, 0);
            block = bytes[offset..offset + n].to_vec();
        }
        entries.push((tag, kind, n, value));
    }
    let header = b"Adobe Photoshop Document Data Block\0".len();
    let len = be32(&block, header + 8);
    (entries, block[header + 12..header + 12 + len].to_vec())
}

fn entry(entries: &[(u16, u16, usize, [u8; 4])], tag: u16) -> Option<(u16, usize, [u8; 4])> {
    entries
        .iter()
        .find(|(t, ..)| *t == tag)
        .map(|&(_, kind, n, value)| (kind, n, value))
}

fn photoshop(name: &str) -> Option<Vec<u8>> {
    let path: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd_photoshop/gray");
    if path.join("photoshop.json").is_file() {
        Some(std::fs::read(path.join(name)).unwrap())
    } else {
        eprintln!("SKIP: the Photoshop fixtures {} are absent", path.display());
        None
    }
}

fn find_rec<'a>(records: &'a [Rec], name: &str) -> &'a Rec {
    records
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no record {name}"))
}

/// A `levl` body's records (version first, then 29 of five u16 each).
fn levels_records(body: &[u8]) -> Vec<[u16; 5]> {
    (0..29)
        .map(|i| std::array::from_fn(|k| be16(body, 2 + 10 * i + 2 * k)))
        .collect()
}

// ------------------------------------------------------------- the tests --

#[test]
fn a_gray_document_round_trips_through_psd_psb_and_tiff_at_eight_and_sixteen_bits() {
    let dir = TempDir::new().unwrap();
    for depth in [8u32, 16] {
        let source = structured(depth);
        let before = render16(source);
        for (container, name) in [(0, "psd"), (1, "psb"), (2, "tif")] {
            let what = format!("{name} {depth}");
            let report = export_report(source, container, 0);
            assert_eq!(report["available"], true, "{what}: {report}");
            assert_eq!(report["depth"], depth, "{what}: {report}");
            assert_eq!(report["color_mode"], "gray", "{what}");
            assert!(
                report["general_limitations"][0]
                    .as_str()
                    .unwrap()
                    .starts_with("A Grayscale document is written as a Grayscale"),
                "{what}: {report}"
            );
            assert!(
                report["observations"]
                    .to_string()
                    .contains("The embedded Gray profile “Dot Gain 20%”"),
                "{what}: {report}"
            );
            let path = dir.path().join(format!("rt{depth}.{name}"));
            save(source, &path, container, 0).unwrap_or_else(|e| panic!("{what}: {e}"));
            let bytes = std::fs::read(&path).unwrap();
            if container == 2 {
                assert!(
                    unsafe { dr_tiff_has_layers(cpath(&path).as_ptr()) },
                    "{what}"
                );
            } else {
                let (channels, bits, mode, count, _) = psd_layout(&bytes);
                assert_eq!((channels, bits, mode), (1, depth as u16, 1), "{what}");
                assert!(count > 0, "{what}: count {count}");
            }
            let back = open(&path, 0, "gray_gamma_22").unwrap_or_else(|e| panic!("{what}: {e}"));
            let (a, b) = (unsafe { &*source }, unsafe { &*back });
            assert!(b.is_gray(), "{what}");
            assert_eq!(profile_bytes(back), profile_bytes(source), "{what}");
            same_tree(a.root_layers(), b.root_layers(), depth == 8, &what);
            // M5's Levels form: the master-only Levels is the Gray channel
            // record, read back as r = g = b with the master the identity.
            let levels: Value =
                serde_json::from_str(b.adjustment_description(index(back, "Levels")).unwrap())
                    .unwrap();
            assert_eq!(levels["op"], "levels", "{what}");
            let params = &levels["params"];
            let r = &params["r"];
            assert!(
                (r["gamma"].as_f64().unwrap() - 1.25).abs() < 0.006,
                "{what}: {levels}"
            );
            assert!(
                (r["black"].as_f64().unwrap() - 26.0 / 255.0).abs() < 1e-6,
                "{what}: {levels}"
            );
            assert_eq!(params["g"], *r, "{what}: {levels}");
            assert_eq!(params["b"], *r, "{what}: {levels}");
            for (i, (p, q)) in before.iter().zip(render16(back)).enumerate() {
                let tolerance = if depth == 8 { 257 } else { 1 };
                for c in 0..4 {
                    assert!(
                        (i32::from(p[c]) - i32::from(q[c])).abs() <= tolerance,
                        "{what} render {i}: {p:?} against {q:?}"
                    );
                }
            }
            free(back);

            // The stored composite (M1: one white-matted gray plane; the
            // document is opaque, so it is the render itself).
            if container != 2 {
                let back = open(&path, 1, "dot_gain_20").unwrap();
                let b = unsafe { &*back };
                assert_eq!(b.root_layers().len(), 1, "{what}");
                for (p, q) in before.iter().zip(samples(&b.root_layers()[0])) {
                    let want = if depth == 8 {
                        p.map(|v| ((u32::from(v) + 128) / 257 * 257) as u16)
                    } else {
                        *p
                    };
                    assert_eq!([want[0], want[0], want[0], 65535], q, "{what} composite");
                }
                free(back);
            }

            // Flattened: one layer, the projection.
            let flat = dir.path().join(format!("flat{depth}.{name}"));
            save(source, &flat, container, 1).unwrap_or_else(|e| panic!("{what}: {e}"));
            let back = open(&flat, 0, "dot_gain_20").unwrap();
            let b = unsafe { &*back };
            assert_eq!(b.root_layers().len(), 1, "{what}");
            for (p, q) in before.iter().zip(samples(&b.root_layers()[0])) {
                let want = if depth == 8 {
                    p.map(|v| ((u32::from(v) + 128) / 257 * 257) as u16)
                } else {
                    *p
                };
                assert_eq!([want[0], want[0], want[0], want[3]], q, "{what} flat");
            }
            free(back);
        }
        free(source);
    }
}

/// Our 8-bit export of the structured document in `container` (0 PSD, 1
/// PSB, 2 layered TIFF) at `depth`.
fn exported(dir: &TempDir, depth: u32, container: i32, name: &str) -> Vec<u8> {
    let source = structured(depth);
    let path = dir.path().join(name);
    save(source, &path, container, 0).unwrap();
    free(source);
    std::fs::read(path).unwrap()
}

#[test]
fn m1_the_header_holds_one_channel_and_a_positive_count_as_photoshops_own() {
    let dir = TempDir::new().unwrap();
    for (depth, container, ours, theirs) in [
        (8, 0, "m1.psd", "layers8.psd"),
        (16, 0, "m1_16.psd", "layers16.psd"),
        (16, 1, "m1.psb", "layers16.psb"),
    ] {
        let (channels, bits, mode, count, records) =
            psd_layout(&exported(&dir, depth, container, ours));
        assert_eq!((channels, bits, mode), (1, depth as u16, 1), "{ours}");
        assert_eq!(count as usize, records.len(), "{ours}: positive {count}");
        if let Some(bytes) = photoshop(theirs) {
            let (channels, bits, mode, ..) = psd_layout(&bytes);
            assert_eq!((channels, bits, mode), (1, depth as u16, 1), "{theirs}");
        }
    }
    if let Some(bytes) = photoshop("layers8.psd") {
        let (_, _, _, count, records) = psd_layout(&bytes);
        assert_eq!(
            count as usize,
            records.len(),
            "Photoshop's count is positive"
        );
    }
}

#[test]
fn m3_m4_m5_records_take_photoshops_gray_forms() {
    let dir = TempDir::new().unwrap();
    let (.., records) = psd_layout(&exported(&dir, 8, 0, "m.psd"));
    // M3: the pixel-less Levels record sets bit 4.
    let levels = find_rec(&records, "Levels");
    assert_eq!(
        levels.flags & 0x18,
        0x18,
        "Levels flags {:#x}",
        levels.flags
    );
    // M4: the open folder's lsct is 16 bytes, sub-type 0 last.
    let group = find_rec(&records, "Group");
    let lsct = group.block(b"lsct").expect("the folder's lsct");
    assert_eq!(lsct.len(), 16);
    assert_eq!(&lsct[4..8], b"8BIM");
    assert_eq!(&lsct[12..], &[0, 0, 0, 0]);
    assert_eq!(group.blocks[1].0, *b"lsct", "lsct right after luni");
    // M5: Blend If's Gray range in slot 1, every other slot the default.
    let top = find_rec(&records, "Top");
    assert_eq!(top.ranges.len(), 40);
    assert_eq!(&top.ranges[8..16], &[20, 40, 220, 240, 0, 0, 255, 255]);
    for slot in [0, 2, 3, 4] {
        assert_eq!(
            &top.ranges[slot * 8..slot * 8 + 8],
            &IDENTITY_RANGE,
            "slot {slot}"
        );
    }
    // M5: the master-only Levels is record 1, record 0 and 2–28 identity.
    let levl = levels_records(levels.block(b"levl").expect("levl"));
    assert_eq!(levl[0], IDENTITY_LEVELS);
    assert_eq!(levl[1], [26, 230, 0, 255, 125]);
    assert!(levl[2..].iter().all(|r| *r == IDENTITY_LEVELS));
    // …each as Photoshop's own gray files hold it.
    let Some(bytes) = photoshop("styled8.psd") else {
        return;
    };
    let (.., theirs) = psd_layout(&bytes);
    let their_levels = find_rec(&theirs, "Levels");
    assert_eq!(their_levels.flags & 0x18, 0x18);
    let their = levels_records(their_levels.block(b"levl").unwrap());
    assert_eq!(their[0], IDENTITY_LEVELS);
    assert_eq!(their[1], [10, 240, 0, 255, 120]);
    assert!(their[2..].iter().all(|r| *r == IDENTITY_LEVELS));
    let ranged = find_rec(&theirs, "Ranged");
    assert_eq!(&ranged.ranges[8..16], &[0, 0, 255, 255, 0, 0, 0x60, 0xa0]);
    for slot in [0, 2, 3, 4] {
        assert_eq!(&ranged.ranges[slot * 8..slot * 8 + 8], &IDENTITY_RANGE);
    }
    let Some(bytes) = photoshop("layers8.psd") else {
        return;
    };
    let (.., theirs) = psd_layout(&bytes);
    let normal = find_rec(&theirs, "Normal group").block(b"lsct").unwrap();
    assert_eq!(normal, b"\0\0\0\x018BIMnorm\0\0\0\0");
    // Our Group is Pass Through (`group_layers`), as Photoshop's Pass group.
    let pass = find_rec(&theirs, "Pass group").block(b"lsct").unwrap();
    let ours = group.block(b"lsct").unwrap();
    assert_eq!(ours, pass, "the same 16 bytes");
}

#[test]
fn m6_the_layered_tiff_is_one_gray_sample_with_a_positive_count() {
    let dir = TempDir::new().unwrap();
    for (depth, name) in [(8u32, "m6.tif"), (16, "m6_16.tif")] {
        let (entries, info) = tiff_layout(&exported(&dir, depth, 2, name));
        let short = |tag: u16| {
            let (kind, n, value) = entry(&entries, tag).unwrap_or_else(|| panic!("tag {tag}"));
            assert_eq!((kind, n), (3, 1), "{name} tag {tag}");
            be16(&value, 0)
        };
        assert_eq!(short(277), 1, "{name} SamplesPerPixel");
        assert_eq!(short(262), 1, "{name} Photometric");
        assert_eq!(short(258), depth as u16, "{name} BitsPerSample inline");
        assert!(entry(&entries, 338).is_none(), "{name}: no ExtraSamples");
        let (count, records) = records_of(&info, false);
        assert_eq!(count as usize, records.len(), "{name}: positive {count}");
        // The strip is the white-matted gray: W × H samples at the depth.
        let (_, _, bytes) = entry(&entries, 279).unwrap();
        assert_eq!(
            be32(&bytes, 0),
            (W * H * depth / 8) as usize,
            "{name} strip"
        );
    }
    let Some(bytes) = photoshop("layers8.tif") else {
        return;
    };
    let (entries, info) = tiff_layout(&bytes);
    let value = |tag: u16| be16(&entry(&entries, tag).unwrap().2, 0);
    assert_eq!((value(277), value(262), value(258)), (1, 1, 8));
    assert!(entry(&entries, 338).is_none());
    let (count, records) = records_of(&info, false);
    assert_eq!(count as usize, records.len());
}

#[test]
fn a_thirty_two_bit_gray_document_refuses_a_layered_tiff_and_a_null_projection() {
    // #1 phase 3c: the PSD is written (`gray32_files.rs`); a layered TIFF
    // is refused (the 32-bit layered TIFF is RGB only), and the
    // projection is required.
    let doc = crate::gray_ops::gray_doc(4, 3, 32);
    let report = export_report(doc, 0, 0);
    assert_eq!(report["available"], true, "{report}");
    assert_eq!(report["depth"], 32);
    assert_eq!(report["color_mode"], "gray");
    let tiff = export_report(doc, 2, 0);
    assert_eq!(tiff["available"], false);
    assert_eq!(
        tiff["refusal"],
        "A TIFF with layers is not written for a 32-bit Grayscale document; export a PSD or PSB, or a flat 32-bit TIFF (File ▸ Export…)."
    );
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("x.psd");
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            ptr::null(),
            cpath(&path).as_ptr(),
            0,
            0,
            LIMIT,
            0,
            std::ptr::null(),
            &mut err,
        )
    };
    assert!(!ok);
    assert_eq!(
        take_err_string(err),
        "a Grayscale document's PSD export requires its projection (required handle is NULL)"
    );
    free(doc);
}
