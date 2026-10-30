//! Pattern Overlay in PSD, PSB and layered TIFF: our export
//! reopened through our own importer equals the document it came from (the
//! `patternFill` effect, the `fxrp` reference point and the `Patt` table at
//! psd8, tif8, psd16 and psb16), a style without a pattern writes no pattern
//! block, a hand-built RLE and Gray `Patt` record is read, every import
//! decline names its reason, and the Angle travels as `Angl` both ways
//! (take3's Photoshop-authored AN45 reads within the residual the runner
//! recorded).

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_doc_save_psd, dr_precision_doc_open_psd, dr_precision_doc_save_psd,
    dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, rect_layer_doc, styled, take_err_string};
use crate::psd16_fixture::pack;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CANVAS: (u32, u32) = (96, 64);
const GREEN: [u8; 4] = [0x30, 0xa0, 0x50, 255];
const BLUE: [u8; 4] = [0x20, 0x40, 0xc0, 255];

// ---------------------------------------------------------------- plumbing --

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                T[((n >> (18 - 6 * i)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

fn tile_rgba(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut rgba = Vec::new();
    for y in 0..h {
        for x in 0..w {
            rgba.extend_from_slice(&f(x, y));
        }
    }
    rgba
}

/// take1's tile T (12 x 8, opaque).
fn tile_t() -> Vec<u8> {
    tile_rgba(12, 8, |x, y| {
        [
            (20 * x + 10) as u8,
            (30 * y + 15) as u8,
            if (x + y) % 2 == 0 { 200 } else { 60 },
            255,
        ]
    })
}

fn custom(name: &str, w: u32, h: u32, rgba: &[u8]) -> Value {
    json!({"name": name, "width": w, "height": h, "rgba": base64(rgba)})
}

fn overlay(pattern: Value, extra: Value) -> Value {
    let mut e = json!({"type": "pattern_overlay", "pattern": pattern});
    for (k, v) in extra.as_object().unwrap() {
        e[k] = v.clone();
    }
    json!({"effects": [e]})
}

fn import_report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(raw)).expect("import report JSON")
}

fn open8(path: &Path) -> DrDocument {
    let report = import_report(path);
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_doc_open_psd(cpath(path).as_ptr(), 0, token.as_ptr(), &mut error) };
    assert!(!raw.is_null(), "open_psd: {}", take_err_string(error));
    *unsafe { Box::from_raw(raw) }
}

fn open16(path: &Path) -> PrecisionDocument {
    let report = import_report(path);
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            0,
            token.as_ptr(),
            LIMIT,
            0,
            ptr::null::<DrJobControl>(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "typed open: {}", take_err_string(error));
    let doc = unsafe { (*raw).clone() };
    unsafe { doctored_core::ffi_precision::dr_precision_doc_free(raw) };
    doc
}

fn save8(doc: &DrDocument, path: &Path, container: i32) {
    let mut error: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_doc_save_psd(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            container,
            0,
            &mut error,
        )
    };
    assert!(ok, "save: {}", take_err_string(error));
}

fn save16(doc: &PrecisionDocument, path: &Path, container: i32) {
    let projection = doc.render(LIMIT, 0, None).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            &projection,
            cpath(path).as_ptr(),
            container,
            0,
            LIMIT,
            0,
            &mut error,
        )
    };
    assert!(ok, "save16: {}", take_err_string(error));
}

fn edit16(doc: &PrecisionDocument, request: &Value) -> PrecisionDocument {
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
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
    *unsafe { Box::from_raw(out) }
}

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle)
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

/// The three overlays the round trips carry: linked and moved after the
/// style (custom T), unlinked with a phase (preset dots), linked at 37 %
/// with a phase (custom T).
fn styles() -> [Value; 3] {
    let t = custom("T", 12, 8, &tile_t());
    [
        overlay(t.clone(), json!({})),
        overlay(
            json!({"preset": "dots"}),
            json!({"link_with_layer": false, "phase": [5, 2]}),
        ),
        overlay(t, json!({"scale": 0.37, "phase": [5, 2]})),
    ]
}

/// A white canvas, three rects, the three styles, then the first rect
/// moved (+7, +3).
fn byte_doc() -> DrDocument {
    let [a, b, c] = styles();
    let doc = rect_layer_doc(CANVAS, (13, 9, 30, 20), GREEN);
    let doc = doc
        .adding_image_layer(
            1,
            image::RgbaImage::from_pixel(30, 20, image::Rgba(BLUE)),
            "B",
        )
        .unwrap()
        .with_layer_offset(2, 50, 6)
        .unwrap();
    // Each rect is MADE where it is placed: its reference point stays (0, 0).
    let mut doc = doc;
    doc.layers[2].reference = (0, 0);
    let doc = doc
        .adding_image_layer(
            2,
            image::RgbaImage::from_pixel(40, 22, image::Rgba(GREEN)),
            "C",
        )
        .unwrap()
        .with_layer_offset(3, 30, 36)
        .unwrap();
    let mut doc = doc;
    doc.layers[3].reference = (0, 0);
    let doc = styled(&doc, 1, &a.to_string());
    let doc = styled(&doc, 2, &b.to_string());
    let doc = styled(&doc, 3, &c.to_string());
    doc.with_layer_offset(1, 20, 12).unwrap()
}

fn byte_styles(doc: &DrDocument) -> Vec<Option<String>> {
    doc.layers
        .iter()
        .map(|l| l.style.as_deref().map(LayerStyle::to_json))
        .collect()
}

// ------------------------------------------------------------ round trips --

#[test]
fn our_8_bit_psd_and_tiff_reopen_as_the_document() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc();
    for (name, container) in [("p.psd", 0), ("p.tif", 2)] {
        let path = dir.path().join(name);
        save8(&doc, &path, container);
        let bytes = std::fs::read(&path).unwrap();
        assert!(contains(&bytes, b"8BIMPatt"), "{name}: the pattern table");
        assert!(contains(&bytes, b"8BIMfxrp"), "{name}: the reference point");
        let reopened = open8(&path);
        assert_eq!(byte_styles(&reopened), byte_styles(&doc), "{name}: styles");
        assert_eq!(
            reopened.flattened().unwrap(),
            doc.flattened().unwrap(),
            "{name}: the picture"
        );
    }
}

fn typed_doc() -> PrecisionDocument {
    let image = |w: u32, h: u32, px: [u8; 4]| {
        PrecisionImage::from_rgba16(
            w,
            h,
            vec![px.map(|v| u16::from(v) * 257); (w * h) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap()
    };
    let layers = vec![
        Layer::raster(
            "Base",
            image(CANVAS.0, CANVAS.1, [255; 4]),
            (0, 0),
            1.0,
            true,
        )
        .unwrap(),
        Layer::raster("A", image(30, 20, GREEN), (20, 12), 1.0, true).unwrap(),
        Layer::raster("B", image(30, 20, BLUE), (50, 6), 1.0, true).unwrap(),
        Layer::raster("C", image(40, 22, GREEN), (30, 36), 1.0, true).unwrap(),
    ];
    let mut doc = PrecisionDocument::from_parts(
        CANVAS.0,
        CANVAS.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap();
    let [mut a, b, c] = styles();
    // Layer A was set at (13, 9) and then moved: its anchor says so.
    a["effects"][0]["anchor"] = json!([13, 9]);
    for (i, s) in [(1, a), (2, b), (3, c)] {
        doc = edit16(
            &doc,
            &json!({"op":"layer_style","version":1,"layer":i,"style":s}),
        );
    }
    doc
}

fn typed_styles(doc: &PrecisionDocument) -> Vec<Option<String>> {
    (0..doc.layer_count())
        .map(|i| doc.layer(i).unwrap().reported_style())
        .collect()
}

#[test]
fn our_16_bit_psd_and_psb_reopen_as_the_document() {
    let dir = TempDir::new().unwrap();
    let doc = typed_doc();
    let want = doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
    for (name, container) in [("p.psd", 0), ("p.psb", 1)] {
        let path = dir.path().join(name);
        save16(&doc, &path, container);
        let reopened = open16(&path);
        assert_eq!(
            typed_styles(&reopened),
            typed_styles(&doc),
            "{name}: styles"
        );
        let got = reopened
            .render(LIMIT, 0, None)
            .unwrap()
            .to_rgba16()
            .unwrap();
        assert!(got == want, "{name}: the picture");
    }
}

#[test]
fn a_style_without_a_pattern_writes_no_pattern_block() {
    let dir = TempDir::new().unwrap();
    let doc = styled(
        &rect_layer_doc(CANVAS, (13, 9, 30, 20), GREEN),
        1,
        "{\"effects\":[{\"type\":\"drop_shadow\"}]}",
    );
    let path = dir.path().join("plain.psd");
    save8(&doc, &path, 0);
    let bytes = std::fs::read(&path).unwrap();
    assert!(!contains(&bytes, b"Patt"), "no pattern table");
    assert!(!contains(&bytes, b"fxrp"), "no reference point");
    assert!(!contains(&bytes, b"patternFill"), "no pattern effect");
}

fn worst_case(report: *mut c_char, err: *mut c_char) -> u64 {
    assert!(!report.is_null(), "{}", take_err_string(err));
    let value: Value = serde_json::from_str(&take_err_string(report)).unwrap();
    assert_eq!(value["available"], true, "{value}");
    value["worst_case_bytes"].as_u64().unwrap()
}

#[test]
fn a_large_custom_tile_stays_within_the_preflights_worst_case() {
    // A small canvas whose one style holds a 512² tile with alpha: the
    // pattern table (about 1 MiB) dwarfs everything else the file holds,
    // so a preflight that left it out could not bound the file.
    let dir = TempDir::new().unwrap();
    let rgba = tile_rgba(512, 512, |x, y| {
        [(x % 251) as u8, (y % 241) as u8, ((x ^ y) % 239) as u8, 128]
    });
    let style = overlay(custom("Big", 512, 512, &rgba), json!({}));
    let small = (16, 12);
    let doc = styled(
        &rect_layer_doc(small, (2, 2, 8, 6), GREEN),
        1,
        &style.to_string(),
    );
    for (name, container) in [("big.psd", 0), ("big.tif", 2)] {
        let mut err = ptr::null_mut();
        let report = unsafe {
            doctored_core::ffi_psd::dr_doc_psd_export_report(&doc, container, 0, &mut err)
        };
        let bound = worst_case(report, err);
        let path = dir.path().join(name);
        save8(&doc, &path, container);
        let len = std::fs::metadata(&path).unwrap().len();
        assert!(len > 1 << 20, "{name}: the table is written ({len})");
        assert!(len <= bound, "{name}: {len} > worst case {bound}");
    }
    let base = PrecisionImage::from_rgba16(
        small.0,
        small.1,
        vec![[65535; 4]; (small.0 * small.1) as usize],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let parsed = LayerStyle::from_json(&style.to_string()).unwrap();
    let layers = vec![Layer::raster("Base", base, (0, 0), 1.0, true)
        .unwrap()
        .with_layer_style(Some(&parsed), small)
        .unwrap()];
    let typed = PrecisionDocument::from_parts(
        small.0,
        small.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap();
    for (name, container) in [("big16.psd", 0), ("big16.psb", 1), ("big16.tif", 2)] {
        let mut err = ptr::null_mut();
        let report = unsafe {
            doctored_core::ffi_psd::dr_precision_doc_psd_export_report(
                &typed, container, 0, &mut err,
            )
        };
        let bound = worst_case(report, err);
        let path = dir.path().join(name);
        save16(&typed, &path, container);
        let len = std::fs::metadata(&path).unwrap().len();
        assert!(len > 1 << 20, "{name}: the table is written ({len})");
        assert!(len <= bound, "{name}: {len} > worst case {bound}");
    }
}

// --------------------------------------------------------- the Patt table --

fn be32(bytes: &[u8], at: usize) -> usize {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

/// `psd` (a classic PSD this crate wrote) with its document tail blocks
/// replaced by `tail` (already framed).
fn with_tail(psd: &[u8], tail: &[u8]) -> Vec<u8> {
    let mut c = 26;
    c += 4 + be32(psd, c);
    c += 4 + be32(psd, c);
    let (start, len) = (c, be32(psd, c));
    let section = &psd[c + 4..c + 4 + len];
    let mut k = 4 + be32(section, 0);
    k += 4 + be32(section, k);
    let mut body = section[..k].to_vec();
    body.extend_from_slice(tail);
    let mut out = psd[..start].to_vec();
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    out.extend_from_slice(&psd[c + 4 + len..]);
    out
}

fn u32b(v: usize) -> [u8; 4] {
    (v as u32).to_be_bytes()
}

/// One array: written, length, depth 8, rect, pixel depth 8, compression,
/// data (raw, or RLE: u16 row counts then PackBits rows).
fn array(plane: &[u8], w: u32, h: u32, rle: bool) -> Vec<u8> {
    let data = if rle {
        let rows: Vec<Vec<u8>> = plane.chunks(w as usize).map(pack).collect();
        let mut d = Vec::new();
        for r in &rows {
            d.extend_from_slice(&(r.len() as u16).to_be_bytes());
        }
        for r in &rows {
            d.extend_from_slice(r);
        }
        d
    } else {
        plane.to_vec()
    };
    let mut out = Vec::new();
    out.extend_from_slice(&u32b(1));
    out.extend_from_slice(&u32b(23 + data.len()));
    out.extend_from_slice(&u32b(8));
    for v in [0, 0, h as usize, w as usize] {
        out.extend_from_slice(&u32b(v));
    }
    out.extend_from_slice(&8u16.to_be_bytes());
    out.push(u8::from(rle));
    out.extend_from_slice(&data);
    out
}

/// One Adobe "Patterns" record, framed (length + record, padded to 4), with
/// `planes` as its written arrays (colour channels, then alpha).
pub(crate) fn record(
    mode: u32,
    w: u32,
    h: u32,
    name: &str,
    planes: &[Vec<u8>],
    rle: bool,
) -> Vec<u8> {
    let mut r = Vec::new();
    r.extend_from_slice(&u32b(1));
    r.extend_from_slice(&u32b(mode as usize));
    r.extend_from_slice(&(h as u16).to_be_bytes());
    r.extend_from_slice(&(w as u16).to_be_bytes());
    let units: Vec<u16> = name.encode_utf16().chain([0]).collect();
    r.extend_from_slice(&u32b(units.len()));
    for u in units {
        r.extend_from_slice(&u.to_be_bytes());
    }
    r.push(4);
    r.extend_from_slice(b"nope");
    let mut list = Vec::new();
    for v in [0, 0, h as usize, w as usize] {
        list.extend_from_slice(&u32b(v));
    }
    let colours = if mode == 1 { 1 } else { 3 };
    list.extend_from_slice(&u32b(colours));
    for plane in planes {
        list.extend_from_slice(&array(plane, w, h, rle));
    }
    for _ in planes.len()..colours + 2 {
        list.extend_from_slice(&u32b(0));
    }
    r.extend_from_slice(&u32b(3));
    r.extend_from_slice(&u32b(list.len()));
    r.extend_from_slice(&list);
    let mut out = u32b(r.len()).to_vec();
    out.extend_from_slice(&r);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    out
}

pub(crate) fn patt(records: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = records.concat();
    let mut out = b"8BIMPatt".to_vec();
    out.extend_from_slice(&u32b(body.len()));
    out.extend_from_slice(&body);
    out
}

/// A PSD whose one styled layer names the pattern "Hand" (a 4 x 3 tile).
fn hand_psd(dir: &TempDir) -> Vec<u8> {
    let rgba = tile_rgba(4, 3, |_, _| [9, 9, 9, 255]);
    let doc = styled(
        &rect_layer_doc(CANVAS, (13, 9, 30, 20), GREEN),
        1,
        &overlay(custom("Hand", 4, 3, &rgba), json!({})).to_string(),
    );
    let path = dir.path().join("hand-source.psd");
    save8(&doc, &path, 0);
    std::fs::read(&path).unwrap()
}

fn imported_pattern(dir: &TempDir, name: &str, bytes: &[u8]) -> Value {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    let doc = open8(&path);
    let style = doc.layers[1]
        .style
        .as_deref()
        .expect("the style is carried");
    serde_json::from_str::<Value>(&style.to_json()).unwrap()["effects"][0]["pattern"].clone()
}

#[test]
fn a_hand_built_rle_record_and_a_gray_record_are_read() {
    let dir = TempDir::new().unwrap();
    let source = hand_psd(&dir);
    let (w, h) = (4u32, 3u32);
    let r: Vec<u8> = (0..12).map(|i| (i * 20) as u8).collect();
    let g: Vec<u8> = vec![7; 12];
    let b: Vec<u8> = (0..12).map(|i| 250 - i as u8).collect();
    let a: Vec<u8> = (0..12).map(|i| if i % 3 == 0 { 0 } else { 255 }).collect();
    let rle = with_tail(
        &source,
        &patt(&[record(
            3,
            w,
            h,
            "Hand",
            &[r.clone(), g.clone(), b.clone(), a.clone()],
            true,
        )]),
    );
    let want: Vec<u8> = (0..12).flat_map(|i| [r[i], g[i], b[i], a[i]]).collect();
    let pattern = imported_pattern(&dir, "rle.psd", &rle);
    assert_eq!(pattern["rgba"], base64(&want), "the RLE record's RGBA");
    assert_eq!(pattern["name"], "Hand");
    let gray = with_tail(
        &source,
        &patt(&[record(1, w, h, "Hand", std::slice::from_ref(&r), false)]),
    );
    let want: Vec<u8> = r.iter().flat_map(|&v| [v, v, v, 255]).collect();
    assert_eq!(
        imported_pattern(&dir, "gray.psd", &gray)["rgba"],
        base64(&want)
    );
}

#[test]
fn every_import_decline_names_its_reason() {
    let dir = TempDir::new().unwrap();
    let source = hand_psd(&dir);
    let plane = vec![9u8; 12];
    let three = [plane.clone(), plane.clone(), plane.clone()];
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        (
            "missing",
            with_tail(&source, &[]),
            "pattern overlay names a pattern the file does not carry",
        ),
        (
            "mode",
            with_tail(&source, &patt(&[record(4, 4, 3, "Hand", &three, false)])),
            "pattern uses a colour mode Doctored does not read",
        ),
        (
            "side",
            with_tail(&source, &patt(&[record(3, 4, 600, "Hand", &[], false)])),
            "pattern is larger than 512 px on a side",
        ),
    ];
    for (name, bytes, reason) in cases {
        let path = dir.path().join(format!("{name}.psd"));
        std::fs::write(&path, &bytes).unwrap();
        let report = import_report(&path);
        let losses = strings(&report, "verified_losses");
        assert!(
            losses.iter().any(|l| l.contains(reason)),
            "{name}: {losses:?}"
        );
        assert!(open8(&path).layers[1].style.is_none(), "{name}: declined");
    }
}

// ------------------------------------------------- Photoshop's own files --

/// take2's Photoshop-authored files (`fixtures/psd_photoshop/pattern_overlay`,
/// `photoshop.json` there): P5 at 8 bits (the layer moved before its style,
/// `fxrp` (7, 3)) with Photoshop's flattened TIFF, and P2 at 16 bits.
fn photoshop_fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/pattern_overlay")
        .join(name)
}

#[test]
fn photoshops_own_files_read_with_their_tile_and_reference_point() {
    // P5: Photoshop anchored the tile where the layer was MADE, and its
    // `fxrp` says so; the import's anchor (top left − fxrp) renders what
    // Photoshop rendered (take2's READ, within 2 codes).
    let doc = open8(&photoshop_fixture("p5_rgb8.psd"));
    let (index, layer) = doc
        .layers
        .iter()
        .enumerate()
        .find(|(_, l)| l.style.is_some())
        .expect("the styled layer");
    let style: Value = serde_json::from_str(&layer.style.as_deref().unwrap().to_json()).unwrap();
    let pattern_of = |style: &Value| -> Value {
        style["effects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["type"] == "pattern_overlay")
            .cloned()
            .expect("a pattern overlay")
    };
    let e = &pattern_of(&style);
    assert_eq!(e["pattern"]["rgba"], base64(&tile_t()), "the tile T");
    let (ox, oy) = doc.layers[index].offset;
    assert_eq!(
        e["anchor"],
        json!([ox - 7, oy - 3]),
        "anchor = top left − fxrp"
    );
    let ours = doc.flattened().unwrap();
    let theirs = DrDocument::open(photoshop_fixture("p5_rgb8.tif").to_str().unwrap())
        .unwrap()
        .flattened()
        .unwrap();
    let worst = ours
        .as_raw()
        .chunks(4)
        .zip(theirs.as_raw().chunks(4))
        .flat_map(|(a, b)| (0..3).map(move |c| (i32::from(a[c]) - i32::from(b[c])).abs()))
        .max()
        .unwrap();
    assert!(worst <= 2, "P5 renders {worst} codes from Photoshop's TIFF");
    // P2 at 16 bits: the table keyed `Pat2` reads, and the anchor follows.
    let typed = open16(&photoshop_fixture("p2_rgb16.psd"));
    let reported = (0..typed.layer_count())
        .filter_map(|i| typed.layer(i).unwrap().reported_style().map(|s| (i, s)))
        .next()
        .expect("the styled record");
    let e = pattern_of(&serde_json::from_str(&reported.1).unwrap());
    assert_eq!(e["pattern"]["rgba"], base64(&tile_t()));
    let (ox, oy) = typed.layer(reported.0).unwrap().offset();
    assert_eq!(e["anchor"], json!([ox - 7, oy - 3]));
}

#[test]
fn a_16_bit_export_keys_its_pattern_table_as_photoshop_does() {
    let dir = TempDir::new().unwrap();
    let authored = std::fs::read(photoshop_fixture("p2_rgb16.psd")).unwrap();
    assert!(contains(&authored, b"8BIMPat2") && !contains(&authored, b"8BIMPatt"));
    let doc = typed_doc();
    for (name, container) in [("k.psd", 0), ("k.psb", 1)] {
        let path = dir.path().join(name);
        save16(&doc, &path, container);
        let bytes = std::fs::read(&path).unwrap();
        assert!(
            contains(&bytes, b"8BIMPat2"),
            "{name}: Pat2, Photoshop's 16-bit key"
        );
        assert!(!contains(&bytes, b"8BIMPatt"), "{name}: no Patt");
    }
    let path = dir.path().join("k8.psd");
    save8(&byte_doc(), &path, 0);
    assert!(
        contains(&std::fs::read(&path).unwrap(), b"8BIMPatt"),
        "8 bits: Patt"
    );
}

// ------------------------------------------------------------------ Angl --

/// `bytes` with its first written `Angl` set to `degrees`.
fn with_angl(bytes: &[u8], degrees: f64) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let at = out
        .windows(12)
        .position(|w| w == b"AnglUntF#Ang")
        .expect("the written Angl");
    out[at + 12..at + 20].copy_from_slice(&degrees.to_be_bytes());
    out
}

#[test]
fn an_angl_imports_as_the_angle_canonicalized() {
    let dir = TempDir::new().unwrap();
    let source = hand_psd(&dir);
    for (given, want) in [(30.0, 30.0), (-45.0, -45.0), (190.0, -170.0), (0.0, 0.0)] {
        let path = dir.path().join(format!("angl{given}.psd"));
        std::fs::write(&path, with_angl(&source, given)).unwrap();
        let doc = open8(&path);
        let style = doc.layers[1]
            .style
            .as_deref()
            .expect("the style is carried");
        let e = &serde_json::from_str::<Value>(&style.to_json()).unwrap()["effects"][0];
        assert_eq!(e["angle"], want, "Angl {given}");
    }
}

/// `bytes` holds an `Angl` of `degrees` (UntF #Ang).
fn holds_angl(bytes: &[u8], degrees: f64) -> bool {
    let mut needle = b"AnglUntF#Ang".to_vec();
    needle.extend_from_slice(&degrees.to_be_bytes());
    contains(bytes, &needle)
}

#[test]
fn the_angle_round_trips_through_our_8_and_16_bit_files() {
    let dir = TempDir::new().unwrap();
    let t = custom("T", 12, 8, &tile_t());
    let angled = overlay(t, json!({"angle": 30, "scale": 0.5}));
    let doc = styled(
        &rect_layer_doc(CANVAS, (13, 9, 30, 20), GREEN),
        1,
        &angled.to_string(),
    );
    for (name, container) in [("a.psd", 0), ("a.tif", 2)] {
        let path = dir.path().join(name);
        save8(&doc, &path, container);
        assert!(holds_angl(&std::fs::read(&path).unwrap(), 30.0), "{name}");
        let reopened = open8(&path);
        assert_eq!(byte_styles(&reopened), byte_styles(&doc), "{name}: styles");
        assert_eq!(
            reopened.flattened().unwrap(),
            doc.flattened().unwrap(),
            "{name}: the picture"
        );
    }
    let mut typed = typed_doc();
    let mut style = overlay(
        custom("T", 12, 8, &tile_t()),
        json!({"angle": -45, "scale": 0.37}),
    );
    style["effects"][0]["anchor"] = json!([20, 12]);
    typed = edit16(
        &typed,
        &json!({"op":"layer_style","version":1,"layer":1,"style":style}),
    );
    let want = typed.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
    for (name, container) in [("a16.psd", 0), ("a16.psb", 1)] {
        let path = dir.path().join(name);
        save16(&typed, &path, container);
        assert!(holds_angl(&std::fs::read(&path).unwrap(), -45.0), "{name}");
        let reopened = open16(&path);
        assert_eq!(
            typed_styles(&reopened),
            typed_styles(&typed),
            "{name}: styles"
        );
        let got = reopened
            .render(LIMIT, 0, None)
            .unwrap()
            .to_rgba16()
            .unwrap();
        assert!(got == want, "{name}: the picture");
    }
}

/// take3's AN45 (Photoshop's Angle −45 at 100 %, the 12 x 8 tile T, linked
/// at the canvas origin) as the runner recorded it under the shipped model:
/// (max, mean) codes over the layer.
const AN45_RESIDUAL: (f64, f64) = (136.407, 55.954);

#[test]
fn photoshops_angled_file_reads_within_the_recorded_residual() {
    let doc = open8(&photoshop_fixture("an45_rgb8.psd"));
    let (index, layer) = doc
        .layers
        .iter()
        .enumerate()
        .find(|(_, l)| l.style.is_some())
        .expect("the styled layer");
    let style: Value = serde_json::from_str(&layer.style.as_deref().unwrap().to_json()).unwrap();
    let e = style["effects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "pattern_overlay")
        .cloned()
        .expect("a pattern overlay");
    assert_eq!(e["angle"], -45.0, "Photoshop's Angl");
    assert_eq!(e["pattern"]["rgba"], base64(&tile_t()), "the tile T");
    let (ox, oy) = doc.layers[index].offset;
    assert_eq!(
        e["anchor"],
        json!([ox, oy]),
        "never moved: anchor = top left"
    );
    let ours = doc.flattened().unwrap();
    let theirs = DrDocument::open(photoshop_fixture("an45_rgb8.tif").to_str().unwrap())
        .unwrap()
        .flattened()
        .unwrap();
    let (x0, y0) = doc.layers[index].offset;
    let (w, h) = (layer.pixels.width() as i32, layer.pixels.height() as i32);
    let (mut worst, mut total, mut n) = (0f64, 0f64, 0f64);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            let (a, b) = (
                ours.get_pixel(x as u32, y as u32).0,
                theirs.get_pixel(x as u32, y as u32).0,
            );
            let d = (0..3)
                .map(|c| f64::from((i32::from(a[c]) - i32::from(b[c])).abs()))
                .fold(0.0, f64::max);
            worst = worst.max(d);
            total += d;
            n += 1.0;
        }
    }
    let mean = total / n;
    assert!(
        worst <= AN45_RESIDUAL.0 + 2.0 && mean <= AN45_RESIDUAL.1 + 0.5,
        "AN45: max {worst} mean {mean:.3} against the recorded {AN45_RESIDUAL:?}"
    );
}
