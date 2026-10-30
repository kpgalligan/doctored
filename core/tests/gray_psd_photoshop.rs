//! Photoshop-authored layered Grayscale files (#547 phase 3a): take1 of
//! `scripts/photoshop-probe/gray_files_take.py` (Photoshop 26.10.0), exported
//! into `tests/fixtures/psd_photoshop/gray/` by its `--export-fixture`, whose
//! docstring fixed every bound below before the take. SKIPS with an eprintln
//! when the folder is absent.
//!
//! The scenes were authored in RGB (sRGB) and converted by Image ▸ Mode ▸
//! Grayscale without flattening. Per file: the rows EXACTLY against
//! Photoshop's own readback in `photoshop.json` (names, order and depth,
//! kinds, Pass Through, clipping, modes, opacity and Fill bytes, visibility,
//! mask presence and enabled); every imported raster's own pixels against
//! Photoshop's per-layer TIFF (alpha everywhere, gray where alpha > 0; 0
//! codes at 8 bits, ≤ 1 of 65535 at 16; the TIFF's associated alpha
//! premultiplies its gray, and ours is compared in that form), read through
//! the `tiff` crate; the
//! layered import's composite within 1 8-bit code of Photoshop's composite
//! TIFF at every cell centre; the Gray profile bytes equal resource 1039.
//! The hypotheses as the take measured them (`psd_import_gray`'s module doc,
//! a recorded deviation from the spec's H-L and H-B): a gray file's Levels
//! is its Gray CHANNEL record (record 1, read as `r`) with the composite
//! record the identity — carried as r = g = b; Blend If Gray is SLOT 1 (read
//! as Red) with slot 0 the default — carried as the Gray range; and H-C, a
//! `Grsc` colour is `Gry ` = K % of the Gray profile's code, the overlay's
//! gray within 1 8-bit code of Photoshop's composite and reported carried.

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_export::{
    dr_export_result_bytes, dr_export_result_free, dr_precision_doc_export_encode,
};
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_psd::{dr_precision_doc_open_gray_psd, dr_psd_import_report};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use serde_json::Value;

use crate::common::{cpath, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const W: u32 = 32;
const H: u32 = 24;
const CELL: u32 = 8;

fn folder() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd_photoshop/gray");
    if root.join("photoshop.json").is_file() {
        Some(root)
    } else {
        eprintln!("SKIP: the Photoshop fixtures {} are absent", root.display());
        None
    }
}

fn manifest(root: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(root.join("photoshop.json")).unwrap()).unwrap()
}

fn report(path: &Path) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value =
        serde_json::from_str(unsafe { std::ffi::CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn open(path: &Path) -> *mut PrecisionDocument {
    let fingerprint = CString::new(report(path)["source_fingerprint"].as_str().unwrap()).unwrap();
    let working = CString::new("dot_gain_20").unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_gray_psd(
            cpath(path).as_ptr(),
            0,
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
    doc
}

/// One row in Photoshop's terms.
#[derive(Debug, PartialEq)]
struct Row {
    name: String,
    depth: u64,
    kind: String,
    blend: String,
    opacity: u64,
    fill: u64,
    visible: bool,
    clipped: bool,
    mask: Option<bool>,
}

fn constant(mode: BlendMode) -> String {
    let name = match mode {
        BlendMode::PassThrough => "PASSTHROUGH".to_string(),
        other => format!("{other:?}").to_uppercase(),
    };
    format!("BlendMode.{name}")
}

fn byte(v: f32) -> u64 {
    (f64::from(v) * 255.0).round() as u64
}

/// Our tree top-first, a group's members after it one level deeper.
fn ours(layers: &[PrecisionDocumentLayer], depth: u64, out: &mut Vec<Row>) {
    for layer in layers.iter().rev() {
        let kind = if layer.children().is_some() {
            "LayerKind.LAYERSET"
        } else if layer.source().is_none() {
            "LayerKind.LEVELS"
        } else {
            "LayerKind.NORMAL"
        };
        out.push(Row {
            name: layer.name().to_string(),
            depth,
            kind: kind.to_string(),
            blend: constant(layer.blend_mode()),
            opacity: byte(layer.opacity()),
            fill: layer.style().map_or(255, |s| byte(s.fill_opacity)),
            visible: layer.visible(),
            clipped: layer.clipped(),
            mask: layer.mask().map(|m| m.enabled()),
        });
        if let Some(children) = layer.children() {
            ours(children, depth + 1, out);
        }
    }
}

fn photoshop_rows(rows: &Value) -> Vec<Row> {
    rows.as_array()
        .unwrap()
        .iter()
        .map(|r| Row {
            name: r["name"].as_str().unwrap().to_string(),
            depth: r["depth"].as_u64().unwrap(),
            kind: r["kind"].as_str().unwrap().to_string(),
            blend: r["blend"].as_str().unwrap().to_string(),
            opacity: r["opacity_255"].as_u64().unwrap(),
            fill: r["fill_opacity_255"].as_u64().unwrap(),
            visible: r["visible"].as_bool().unwrap(),
            clipped: r["clipped"].as_bool().unwrap_or(false),
            mask: r["has_user_mask"]
                .as_bool()
                .unwrap_or(false)
                .then(|| r["user_mask_enabled"].as_bool().unwrap_or(false)),
        })
        .collect()
}

/// The other rasters in the folder that holds `name` (none at the root).
fn siblings<'a>(
    layers: &'a [PrecisionDocumentLayer],
    name: &str,
    nested: bool,
) -> Option<Vec<&'a PrecisionDocumentLayer>> {
    if layers.iter().any(|l| l.name() == name) {
        return Some(if nested {
            layers
                .iter()
                .filter(|l| l.name() != name && l.source().is_some())
                .collect()
        } else {
            Vec::new()
        });
    }
    layers
        .iter()
        .filter_map(|l| l.children())
        .find_map(|children| siblings(children, name, true))
}

fn find<'a>(
    layers: &'a [PrecisionDocumentLayer],
    name: &str,
) -> Option<&'a PrecisionDocumentLayer> {
    for layer in layers {
        if layer.name() == name {
            return Some(layer);
        }
        if let Some(found) = layer.children().and_then(|c| find(c, name)) {
            return Some(found);
        }
    }
    None
}

/// A gray (and alpha) TIFF's samples as (depth, gray, alpha), through the
/// `tiff` crate.
fn tiff_samples(path: &Path) -> (u32, Vec<u32>, Vec<u32>) {
    let (depth, gray, alpha, _) = tiff_samples_with(path);
    (depth, gray, alpha)
}

/// [`tiff_samples`] and whether the alpha is ASSOCIATED (ExtraSamples 1:
/// the gray premultiplied by it, the form Photoshop's TIFF with
/// transparency takes).
fn tiff_samples_with(path: &Path) -> (u32, Vec<u32>, Vec<u32>, bool) {
    let mut decoder = tiff::decoder::Decoder::new(std::fs::File::open(path).unwrap()).unwrap();
    let associated = decoder
        .find_tag_unsigned_vec::<u16>(tiff::tags::Tag::ExtraSamples)
        .unwrap()
        .is_some_and(|v| v.first() == Some(&1));
    let (w, h) = decoder.dimensions().unwrap();
    assert_eq!((w, h), (W, H), "{}", path.display());
    let (depth, samples): (u32, Vec<u32>) = match decoder.read_image().unwrap() {
        tiff::decoder::DecodingResult::U8(v) => (8, v.into_iter().map(u32::from).collect()),
        tiff::decoder::DecodingResult::U16(v) => (16, v.into_iter().map(u32::from).collect()),
        _ => panic!("{}: not 8 or 16 bits", path.display()),
    };
    let pixels = (W * H) as usize;
    let channels = samples.len() / pixels;
    let top = (1u32 << depth) - 1;
    let gray = samples.iter().step_by(channels).copied().collect();
    let alpha = if channels > 1 {
        samples.iter().skip(1).step_by(channels).copied().collect()
    } else {
        vec![top; pixels]
    };
    (depth, gray, alpha, associated)
}

/// A raster's own canvas-sized (gray, alpha) codes at `depth`.
fn own(layer: &PrecisionDocumentLayer, depth: u32) -> (Vec<u32>, Vec<u32>) {
    let image = layer.source().unwrap();
    let (w, h) = image.dimensions();
    let (ox, oy) = layer.offset();
    let samples = image.to_rgba16().unwrap();
    let scale = |v: u16| {
        if depth == 8 {
            u32::from(v) / 257
        } else {
            u32::from(v)
        }
    };
    let mut gray = vec![0u32; (W * H) as usize];
    let mut alpha = vec![0u32; (W * H) as usize];
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let (cx, cy) = (x + ox, y + oy);
            if !(0..W as i32).contains(&cx) || !(0..H as i32).contains(&cy) {
                continue;
            }
            let p = samples[(y as u32 * w + x as u32) as usize];
            assert!(p[0] == p[1] && p[1] == p[2], "{}: R = G = B", layer.name());
            let at = (cy as u32 * W + cx as u32) as usize;
            gray[at] = scale(p[0]);
            alpha[at] = scale(p[3]);
        }
    }
    (gray, alpha)
}

/// Our render's gray at each cell centre, as 8-bit codes.
fn centres(doc: *const PrecisionDocument) -> Vec<u32> {
    let render = unsafe { &*doc }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap();
    let mut out = Vec::new();
    for row in 0..H / CELL {
        for col in 0..W / CELL {
            let (x, y) = (col * CELL + CELL / 2, row * CELL + CELL / 2);
            out.push((u32::from(render[(y * W + x) as usize][0]) + 128) / 257);
        }
    }
    out
}

fn tiff_centres(path: &Path) -> Vec<u32> {
    let (depth, gray, _) = tiff_samples(path);
    let mut out = Vec::new();
    for row in 0..H / CELL {
        for col in 0..W / CELL {
            let (x, y) = (col * CELL + CELL / 2, row * CELL + CELL / 2);
            let v = gray[(y * W + x) as usize];
            out.push(if depth == 16 { (v + 128) / 257 } else { v });
        }
    }
    out
}

fn assert_centres(ours: &[u32], photoshop: &[u32], what: &str) {
    for (i, (a, b)) in ours.iter().zip(photoshop).enumerate() {
        assert!(
            (*a as i32 - *b as i32).abs() <= 1,
            "{what}: cell {i}: ours {a} against Photoshop's {b} (ours {ours:?}, Photoshop {photoshop:?})"
        );
    }
}

#[test]
fn photoshops_gray_files_open_with_their_rows_planes_and_composite() {
    let Some(root) = folder() else { return };
    let manifest = manifest(&root);
    for (name, file) in manifest["files"].as_object().unwrap() {
        let scene = file["scene"].as_str().unwrap();
        let path = root.join(name);
        let notice = report(&path);
        assert_eq!(notice["import_precision"], "gray", "{name}: {notice}");
        assert_eq!(notice["profile"], "Dot Gain 20%", "{name}");
        let doc = open(&path);
        let d = unsafe { &*doc };
        assert!(d.is_gray(), "{name}");
        let mut rows = Vec::new();
        ours(d.root_layers(), 0, &mut rows);
        assert_eq!(
            rows,
            photoshop_rows(&manifest["scenes"][scene]["rows"]),
            "{name}: the rows"
        );
        if let Some(composite) = file["composite"].as_str() {
            assert_centres(&centres(doc), &tiff_centres(&root.join(composite)), name);
        }
        unsafe { dr_precision_doc_free(doc) };
    }
    // The TIFF and the PSB against the PSDs' composites at their depth.
    for (name, composite) in [
        ("layers8.tif", "composite8.tif"),
        ("layers16.psb", "composite16.tif"),
    ] {
        let doc = open(&root.join(name));
        assert_centres(&centres(doc), &tiff_centres(&root.join(composite)), name);
        unsafe { dr_precision_doc_free(doc) };
    }
}

#[test]
fn every_imported_raster_is_photoshops_own_layer() {
    let Some(root) = folder() else { return };
    let manifest = manifest(&root);
    for scene in ["layers", "styled"] {
        for row in manifest["scenes"][scene]["layers"].as_array().unwrap() {
            if row["copied"] != true {
                continue;
            }
            let file = row["file"].as_str().unwrap();
            let target = row["target"].as_str().unwrap();
            let doc = open(&root.join(file));
            let layer = find(unsafe { &*doc }.root_layers(), target)
                .unwrap_or_else(|| panic!("{file}: no layer {target}"));
            let (depth, gray, alpha, associated) =
                tiff_samples_with(&root.join(row["tiff"].as_str().unwrap()));
            let bound = if depth == 8 { 0 } else { 1 };
            let (ours_gray, ours_alpha) = own(layer, depth);
            // The take's per-layer export hides the ROOT layers only, so a
            // target's visible siblings in its folder show in its TIFF too
            // (Group A and Group B overlap): their pixels are outside the
            // compared domain (`gray_files_take.py`'s RECORDED RESULT).
            let others: Vec<Vec<u32>> = siblings(unsafe { &*doc }.root_layers(), target, false)
                .unwrap_or_default()
                .into_iter()
                .map(|l| own(l, depth).1)
                .collect();
            let top = f64::from((1u32 << depth) - 1);
            // Photoshop's TIFF stores the gray premultiplied by an associated
            // alpha; ours is compared in that same form.
            let ours_gray: Vec<u32> = ours_gray
                .iter()
                .zip(&ours_alpha)
                .map(|(g, a)| {
                    if associated {
                        (f64::from(*g) * f64::from(*a) / top).round() as u32
                    } else {
                        *g
                    }
                })
                .collect();
            for i in 0..gray.len() {
                if others.iter().any(|alpha| alpha[i] > 0) {
                    continue;
                }
                let what = format!("{file} {target} pixel {i}");
                assert!(ours_alpha[i].abs_diff(alpha[i]) <= bound, "{what} alpha");
                if alpha[i] > 0 {
                    assert!(
                        ours_gray[i].abs_diff(gray[i]) <= bound,
                        "{what}: ours {} against {}",
                        ours_gray[i],
                        gray[i]
                    );
                }
            }
            unsafe { dr_precision_doc_free(doc) };
        }
    }
}

/// A PSD's resource 1039.
fn icc_resource(bytes: &[u8]) -> Option<Vec<u8>> {
    let be32 = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let mut at = 26 + 4 + be32(26);
    let end = at + 4 + be32(at);
    at += 4;
    while at < end {
        let id = u16::from_be_bytes([bytes[at + 4], bytes[at + 5]]);
        let name = usize::from(bytes[at + 6]);
        at += 6 + ((name + 2) & !1);
        let len = be32(at);
        at += 4;
        if id == 1039 {
            return Some(bytes[at..at + len].to_vec());
        }
        at += len + (len & 1);
    }
    None
}

/// An ICC profile's `kTRC` at `x` in 0…1, parsed per ICC.1 (the tag table
/// at byte 128; a `curv` of 0 entries the identity, 1 a u8Fixed8 gamma, else
/// a u16 table interpolated linearly).
fn ktrc(icc: &[u8], x: f64) -> f64 {
    let be32 = |at: usize| u32::from_be_bytes(icc[at..at + 4].try_into().unwrap()) as usize;
    let be16 = |at: usize| f64::from(u16::from_be_bytes([icc[at], icc[at + 1]]));
    let tag = (0..be32(128))
        .map(|i| 132 + 12 * i)
        .find(|&at| &icc[at..at + 4] == b"kTRC")
        .expect("a kTRC tag");
    let at = be32(tag + 4);
    assert_eq!(&icc[at..at + 4], b"curv", "kTRC is a curv");
    let count = be32(at + 8);
    match count {
        0 => x,
        1 => x.powf(be16(at + 12) / 256.0),
        _ => {
            let position = x * (count - 1) as f64;
            let i = (position.floor() as usize).min(count - 2);
            let t = position - i as f64;
            let entry = |k: usize| be16(at + 12 + 2 * k) / 65535.0;
            entry(i) * (1.0 - t) + entry(i + 1) * t
        }
    }
}

/// An sRGB byte's linear value (IEC 61966-2-1).
fn srgb_decode(v: u8) -> f64 {
    let c = f64::from(v) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[test]
fn the_gray_profile_travels_verbatim() {
    // The open adopts resource 1039's bytes as they are; the kept-gray TIFF
    // export (`gray_export`, phase 1b: the Gray ICC verbatim in tag 34675)
    // reads them back without the PSD writer, whose Gray form is #547
    // phase 3b.
    let Some(root) = folder() else { return };
    let path = root.join("layers8.psd");
    let source = icc_resource(&std::fs::read(&path).unwrap()).expect("resource 1039");
    let doc = open(&path);
    let request =
        CString::new(r#"{"format":"tiff","bit_depth":8,"profile":"document","dither":"none"}"#)
            .unwrap();
    let mut err = ptr::null_mut();
    let result = unsafe {
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
    assert!(!result.is_null(), "{}", take_err_string(err));
    let mut len = 0usize;
    let bytes = unsafe {
        let p = dr_export_result_bytes(result, &mut len);
        std::slice::from_raw_parts(p, len).to_vec()
    };
    unsafe { dr_export_result_free(result) };
    assert!(
        bytes.windows(source.len()).any(|w| w == source),
        "the Gray ICC verbatim"
    );
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn the_levels_overlay_and_blend_if_hypotheses_as_measured() {
    let Some(root) = folder() else { return };
    let path = root.join("styled8.psd");
    let notice = report(&path);
    // Photoshop writes every effect kind (the disabled defaults too); the
    // overlay is among the carried ones.
    let carried = notice["observations"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .find(|o| o.contains("(“Overlay”) has Photoshop layer effects ("))
        .unwrap_or_else(|| panic!("{notice}"))
        .to_string();
    assert!(
        carried.contains("color_overlay") && carried.contains("layered import carries them"),
        "{carried}"
    );
    let doc = open(&path);
    let d = unsafe { &*doc };
    // H-L as measured: the Gray channel record, carried r = g = b.
    let index = (0..d.layer_count())
        .find(|&i| d.layer(i).unwrap().name() == "Levels")
        .expect("the Levels layer is carried");
    let levels: Value = serde_json::from_str(d.adjustment_description(index).unwrap()).unwrap();
    let params = &levels["params"];
    assert_eq!(levels["op"], "levels", "{levels}");
    let r = &params["r"];
    assert!(
        (r["black"].as_f64().unwrap() - 10.0 / 255.0).abs() < 1e-6,
        "{levels}"
    );
    assert!(
        (r["white"].as_f64().unwrap() - 240.0 / 255.0).abs() < 1e-6,
        "{levels}"
    );
    assert!(
        (r["gamma"].as_f64().unwrap() - 1.2).abs() < 1e-6,
        "{levels}"
    );
    assert_eq!(params["g"], *r, "{levels}");
    assert_eq!(params["b"], *r, "{levels}");
    // H-C: Photoshop's `Gry ` 52.43 % read back, our gray its code.
    let overlay = find(d.root_layers(), "Overlay").unwrap();
    let style: Value = serde_json::from_str(&overlay.reported_style().unwrap()).unwrap();
    let effect = style["effects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "color_overlay")
        .expect("the Color Overlay is carried")
        .clone();
    let hex = effect["color"].as_str().unwrap();
    assert_eq!(&hex[1..3], &hex[3..5], "{effect}");
    // H-C's value: the hex's sRGB-decoded Y through the inverse of the
    // file's own `kTRC` is Photoshop's 1 − 52.43 % K, within 1 8-bit code,
    // and so is Photoshop's own composite at the overlay's cell (cell 0).
    let expected = 255.0 * (1.0 - 0.5243);
    let icc = icc_resource(&std::fs::read(&path).unwrap()).expect("resource 1039");
    let y = srgb_decode(u8::from_str_radix(&hex[1..3], 16).unwrap());
    let code = (0..=255u32)
        .min_by(|&a, &b| {
            let da = (ktrc(&icc, f64::from(a) / 255.0) - y).abs();
            let db = (ktrc(&icc, f64::from(b) / 255.0) - y).abs();
            da.total_cmp(&db)
        })
        .unwrap();
    assert!(
        (f64::from(code) - expected).abs() <= 1.0,
        "{hex}: code {code} against {expected}"
    );
    let photoshop = tiff_centres(&root.join("styled_composite8.tif"))[0];
    assert!(
        (f64::from(photoshop) - expected).abs() <= 1.0,
        "Photoshop's overlay cell {photoshop} against {expected}"
    );
    // H-B as measured: slot 1, the Gray channel's range, carried as Gray.
    let ranged = find(d.root_layers(), "Ranged").unwrap();
    let blend_if = ranged.blend_if().expect("the Blend If is carried");
    let ranges: Vec<_> = blend_if.ranges().collect();
    assert_eq!(ranges.len(), 1, "{blend_if:?}");
    assert_eq!(format!("{:?}", ranges[0].channel), "Gray");
    assert_eq!(ranges[0].this_layer, [0, 0, 255, 255]);
    assert_eq!(ranges[0].underlying, [0, 0, 96, 160]);
    // The composite at every cell centre (the overlay's cell, the Blend If
    // row, the Levels row) within 1 8-bit code of Photoshop's.
    assert_centres(
        &centres(doc),
        &tiff_centres(&root.join("styled_composite8.tif")),
        "styled8.psd",
    );
    unsafe { dr_precision_doc_free(doc) };
}
