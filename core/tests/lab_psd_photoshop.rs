//! Photoshop-authored Lab files (#547 Lab phase 3a), from
//! `scripts/photoshop-probe/lab_files_take.py` (Photoshop 26.10.0), read IN
//! PLACE under its receipts (`~/devel/doctored-fixtures/photoshop/
//! lab-files-547/`, never committed); each test SKIPS with an eprintln when
//! its take is absent. The runner's docstring fixed every bound below before
//! the take that is judged:
//!
//! * (a) through the core, on take1's `codesN` files: each file's samples,
//!   opened by the Lab PSD open (mode 1) and the flat Lab TIFF open, within T
//!   of Photoshop's own `color.lab` reading at each of the 104 sampler points
//!   (T8 = 0.40 L\* and 1.0 a\*/b\*, T16 = 0.05; mean ≤ T/2), the 16-bit
//!   PSD a\*/b\* now decoded on the 15-bit grid;
//! * on take2 (`take2()`: its `decided.json` must say `"valid": true`), a
//!   NEW scene, so take1's cells (fitting data) are not reused:
//!   * the rows EXACTLY against Photoshop's readback (`cmykFilesRows`);
//!   * every imported raster against Photoshop's per-layer TIFF, decoded by
//!     the independent CIELab oracle (`lab_psd_fixture`) and compared in its
//!     ASSOCIATED-alpha form (Photoshop premultiplies the unsigned offset
//!     sample, a\*/b\* = signed + 128): 0 codes at 8 bits, ≤ 2 of 65535 at
//!     16; a clipped target's TIFF keeps its clip base visible (Gray take4's
//!     rule: the expected value is our layer over our clip base through
//!     `over`);
//!   * 3a's composite gate, unchanged: each layered composite within 1 8-bit
//!     code (257 at 16) of Photoshop's composite TIFF at every cell centre;
//!   * the twins: `layers16.psb` and `layers16.tif` import EQUAL to
//!     `layers16.psd`, `layers8.tif` to `layers8.psd` (codes, masks,
//!     records);
//!   * the hypotheses: H-C (the Color Overlay's `LbCl`), H-B (Blend If in
//!     slot 1, carried as one Lightness range) and H-T (the twins)
//!     re-checked; H-L (whether Mode ▸ Lab Color keeps the Levels layer)
//!     recorded only.

use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_lab::dr_precision_doc_open_lab;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_psd::{dr_precision_doc_open_lab_psd, dr_psd_import_report};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use serde_json::Value;

use crate::common::{cpath, take_err_string};
use crate::lab_psd_fixture::{cielab_units, code_of, read_lab_tiff, units_of};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const CELL: u32 = 8;

fn take_root(name: &str) -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var("HOME").ok()?)
            .join("devel/doctored-fixtures/photoshop/lab-files-547")
            .join(name),
    )
}

/// take1 (the (a) codes files).
fn take() -> Option<PathBuf> {
    let root = take_root("take1")?;
    if root.join("decided.json").is_file() {
        Some(root)
    } else {
        eprintln!("SKIP: the Photoshop take {} is absent", root.display());
        None
    }
}

/// take2 (the layered files), only when `--decide2` judged it VALID.
fn take2() -> Option<PathBuf> {
    let root = take_root("take2")?;
    let decided = std::fs::read_to_string(root.join("decided.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    match decided {
        Some(d) if d["valid"] == true => Some(root),
        Some(d) => panic!("{}: take2 is not VALID: {d}", root.display()),
        None => {
            eprintln!("SKIP: the Photoshop take {} is absent", root.display());
            None
        }
    }
}

fn measured(root: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(root.join("measured.json")).unwrap()).unwrap()
}

fn file(root: &Path, job: &str, name: &str) -> PathBuf {
    root.join("jobs").join(job).join(name)
}

fn report(path: &Path) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn open(path: &Path, mode: i32) -> *mut PrecisionDocument {
    let fingerprint = CString::new(report(path)["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_lab_psd(
            cpath(path).as_ptr(),
            mode,
            fingerprint.as_ptr(),
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

fn open_flat(path: &Path) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let doc =
        unsafe { dr_precision_doc_open_lab(cpath(path).as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(
        !doc.is_null(),
        "{}: {}",
        path.display(),
        take_err_string(err)
    );
    doc
}

fn render(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
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

// ------------------------------------------------------------------ (a) --

#[test]
fn the_core_reads_photoshops_lab_codes() {
    let Some(root) = take() else { return };
    let measured = measured(&root);
    for bits in [8u16, 16] {
        let key = format!("codes{bits}");
        let rows = measured["results"][&key]["report"]["lab"]
            .as_array()
            .unwrap();
        let bound = if bits == 8 {
            [0.40, 1.0, 1.0]
        } else {
            [0.05, 0.05, 0.05]
        };
        for name in [format!("{key}.psd"), format!("{key}.tif")] {
            let path = file(&root, &key, &name);
            let doc = if name.ends_with(".psd") {
                open(&path, 1)
            } else {
                open_flat(&path)
            };
            let d = unsafe { &*doc };
            assert!(d.is_lab(), "{name}");
            let (w, _) = d.dimensions();
            let codes = render(d);
            let mut worst = [0f64; 3];
            let mut total = [0f64; 3];
            for row in rows {
                let (x, y) = (row["x"].as_u64().unwrap(), row["y"].as_u64().unwrap());
                let p = codes[(y * u64::from(w) + x) as usize];
                for (c, key) in ["l", "a", "b"].iter().enumerate() {
                    let r = (units_of(c, p[c]) - row[key].as_f64().unwrap()).abs();
                    worst[c] = worst[c].max(r);
                    total[c] += r;
                }
            }
            for c in 0..3 {
                let mean = total[c] / rows.len() as f64;
                assert!(
                    worst[c] <= bound[c] + 1e-9 && mean <= bound[c] / 2.0 + 1e-9,
                    "{name} channel {c}: max {} mean {mean} against {}",
                    worst[c],
                    bound[c]
                );
            }
            eprintln!("{name}: max {worst:?}");
            unsafe { dr_precision_doc_free(doc) };
        }
    }
}

// ----------------------------------------------------------------- rows --

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
    format!("BlendMode.{}", format!("{mode:?}").to_uppercase())
}

fn byte(v: f32) -> u64 {
    (f64::from(v) * 255.0).round() as u64
}

fn ours(layers: &[PrecisionDocumentLayer], depth: u64, out: &mut Vec<Row>) {
    for layer in layers.iter().rev() {
        let kind = if layer.children().is_some() {
            "LayerKind.LAYERSET"
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

/// The layered files of each scene, with their composite TIFF.
const LAYERED: &[(&str, &str, Option<&str>)] = &[
    ("layers2", "layers8.psd", Some("composite8.tif")),
    ("layers2", "layers8.tif", Some("composite8.tif")),
    ("layers2", "layers16.psd", Some("composite16.tif")),
    ("layers2", "layers16.psb", Some("composite16.tif")),
    ("layers2", "layers16.tif", Some("composite16.tif")),
    ("styled2", "styled8.psd", Some("styled_composite8.tif")),
];

/// A composite TIFF's codes (CIELab oracle) at every cell centre.
fn tiff_centres(path: &Path) -> (u16, Vec<[u16; 3]>) {
    let tiff = read_lab_tiff(&std::fs::read(path).unwrap());
    assert_eq!(tiff.photometric, 8, "{}", path.display());
    let mut out = Vec::new();
    for row in 0..tiff.height / CELL {
        for col in 0..tiff.width / CELL {
            let i = ((row * CELL + CELL / 2) * tiff.width + col * CELL + CELL / 2) as usize;
            out.push(std::array::from_fn(|c| {
                code_of(c, cielab_units(tiff.bits, c, tiff.planes[c][i]))
            }));
        }
    }
    (tiff.bits, out)
}

fn render_centres(doc: &PrecisionDocument) -> Vec<[u16; 3]> {
    let (w, h) = doc.dimensions();
    let codes = render(doc);
    let mut out = Vec::new();
    for row in 0..h / CELL {
        for col in 0..w / CELL {
            let p = codes[((row * CELL + CELL / 2) * w + col * CELL + CELL / 2) as usize];
            out.push([p[0], p[1], p[2]]);
        }
    }
    out
}

#[test]
fn photoshops_lab_files_open_with_their_rows_and_composite() {
    let Some(root) = take2() else { return };
    let measured = measured(&root);
    let mut failures = Vec::new();
    for (scene, name, composite) in LAYERED {
        let path = file(&root, scene, name);
        let notice = report(&path);
        assert_eq!(notice["import_precision"], "lab", "{name}: {notice}");
        assert_eq!(notice["color_mode"], "Lab", "{name}");
        assert!(notice["profile"].is_null(), "{name}");
        assert!(notice["layered_plan_bytes"].as_u64().unwrap() > 0, "{name}");
        assert_eq!(notice["layered_available"], true, "{name}: {notice}");
        let doc = open(&path, 0);
        let d = unsafe { &*doc };
        assert!(d.is_lab(), "{name}");
        let mut rows = Vec::new();
        ours(d.root_layers(), 0, &mut rows);
        assert_eq!(
            rows,
            photoshop_rows(&measured["results"][scene]["report"]["rows"]),
            "{name}: the rows"
        );
        if let Some(composite) = composite {
            // 1 8-bit code: 257 of the document's codes at either depth.
            let (_, photoshop) = tiff_centres(&file(&root, scene, composite));
            let mine = render_centres(d);
            let mut misses = Vec::new();
            for (i, (a, b)) in mine.iter().zip(&photoshop).enumerate() {
                let worst = (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap();
                eprintln!("{name}: cell {i}: ours {a:?} Photoshop {b:?} (max {worst})");
                if worst > 257 {
                    misses.push(i);
                }
            }
            if !misses.is_empty() {
                failures.push(format!("{name}: cells {misses:?} miss the composite bound"));
            }
        }
        unsafe { dr_precision_doc_free(doc) };
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

// --------------------------------------------------------- per-layer TIFFs --

/// A raster's own canvas-sized straight codes ([L, a, b, alpha]; transparent
/// outside its rectangle).
fn own(layer: &PrecisionDocumentLayer, canvas: (u32, u32)) -> Vec<[u16; 4]> {
    let image = layer.source().unwrap();
    let (w, h) = image.dimensions();
    let (ox, oy) = layer.offset();
    let samples = image.to_rgba16().unwrap();
    let mut out = vec![[0u16; 4]; (canvas.0 * canvas.1) as usize];
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let (cx, cy) = (x + ox, y + oy);
            if !(0..canvas.0 as i32).contains(&cx) || !(0..canvas.1 as i32).contains(&cy) {
                continue;
            }
            out[(cy as u32 * canvas.0 + cx as u32) as usize] =
                samples[(y as u32 * w + x as u32) as usize];
        }
    }
    out
}

/// Porter–Duff `over` of straight codes (the clipped-layer rule).
fn over(top: [u16; 4], under: [u16; 4]) -> [u16; 4] {
    let at = f64::from(top[3]) / 65535.0;
    let ab = f64::from(under[3]) / 65535.0;
    let ao = at + ab * (1.0 - at);
    if ao <= 0.0 {
        return [0; 4];
    }
    let mut out = [0u16; 4];
    for c in 0..3 {
        let v = (f64::from(top[c]) * at + f64::from(under[c]) * ab * (1.0 - at)) / ao;
        out[c] = v.round().clamp(0.0, 65535.0) as u16;
    }
    out[3] = (ao * 65535.0).round() as u16;
    out
}

/// A per-layer TIFF's associated samples in the offset form (L as stored,
/// a\*/b\* signed + 128), at the file's depth, and its depth.
fn associated(path: &Path) -> (u16, Vec<[u32; 4]>) {
    let tiff = read_lab_tiff(&std::fs::read(path).unwrap());
    assert_eq!(tiff.photometric, 8, "{}", path.display());
    // An opaque layer's TIFF has no alpha sample at all.
    if tiff.samples == 4 {
        assert_eq!(
            tiff.extra_samples,
            vec![1],
            "{}: an associated alpha",
            path.display()
        );
    } else {
        assert_eq!(tiff.samples, 3, "{}", path.display());
    }
    let half = 1u32 << (tiff.bits - 1);
    let top = (1u32 << tiff.bits) - 1;
    let pixels = (tiff.width * tiff.height) as usize;
    let out = (0..pixels)
        .map(|i| {
            [
                u32::from(tiff.planes[0][i]),
                u32::from(tiff.planes[1][i]) ^ half,
                u32::from(tiff.planes[2][i]) ^ half,
                tiff.planes.get(3).map_or(top, |a| u32::from(a[i])),
            ]
        })
        .collect();
    (tiff.bits, out)
}

#[test]
fn every_imported_raster_is_photoshops_own_layer() {
    let Some(root) = take2() else { return };
    let measured = measured(&root);
    for scene in ["layers2", "styled2"] {
        for row in measured["results"][scene]["report"]["layers"]
            .as_array()
            .unwrap()
        {
            let name = row["file"].as_str().unwrap();
            let target = row["target"].as_str().unwrap();
            let doc = open(&file(&root, scene, name), 0);
            let d = unsafe { &*doc };
            let canvas = d.dimensions();
            let layer = find(d.root_layers(), target).unwrap_or_else(|| panic!("{name}: {target}"));
            let mut expected = own(layer, canvas);
            // The clipped-layer rule (Gray take4's): the clip base stays
            // visible in a clipped target's readback.
            if layer.clipped() {
                let base = find(d.root_layers(), "Clip base").unwrap();
                let under = own(base, canvas);
                for (e, u) in expected.iter_mut().zip(under) {
                    *e = over(*e, u);
                }
            }
            let (bits, photoshop) = associated(&file(&root, scene, row["tiff"].as_str().unwrap()));
            let (bound, top) = if bits == 8 { (0, 255.0) } else { (2, 65535.0) };
            let mut worst = 0;
            for (i, (e, p)) in expected.iter().zip(&photoshop).enumerate() {
                // Ours at the file's depth, premultiplied as Photoshop stores it.
                let at_depth = |v: u16, c: usize| -> f64 {
                    if bits == 8 {
                        f64::from(v) / 257.0
                    } else if c == 0 || c == 3 {
                        f64::from(v)
                    } else {
                        // The document code back on the PSD's a* scale.
                        f64::from(v) * 256.0 / 257.0
                    }
                };
                let alpha = at_depth(e[3], 3);
                let mine: [u32; 4] = std::array::from_fn(|c| {
                    if c == 3 {
                        alpha.round() as u32
                    } else {
                        (at_depth(e[c], c) * alpha / top).round() as u32
                    }
                });
                for c in 0..4 {
                    let diff = mine[c].abs_diff(p[c]);
                    worst = worst.max(diff);
                    assert!(
                        diff <= bound,
                        "{name} {target} pixel {i} channel {c}: ours {} Photoshop {}",
                        mine[c],
                        p[c]
                    );
                }
            }
            eprintln!("{name} {target}: max {worst}");
            unsafe { dr_precision_doc_free(doc) };
        }
    }
}

// ----------------------------------------------------------------- twins --

/// Every layer's record and codes, top-first, for an equality check.
fn snapshot(layers: &[PrecisionDocumentLayer], out: &mut Vec<String>) {
    for layer in layers.iter().rev() {
        let pixels = layer
            .source()
            .map(|image| (image.dimensions(), image.to_rgba16().unwrap()));
        let mask = layer.mask().map(|m| {
            let p = m.properties();
            (
                m.enabled(),
                m.plane().dimensions(),
                m.plane().samples().to_vec(),
                p.offset,
                p.outside_coverage,
            )
        });
        out.push(format!(
            "{} {:?} {} {} {} {:?} {:?} {:?} {:?}",
            layer.name(),
            layer.blend_mode(),
            layer.opacity(),
            layer.visible(),
            layer.clipped(),
            layer.offset(),
            mask,
            layer.blend_if(),
            pixels
        ));
        if let Some(children) = layer.children() {
            snapshot(children, out);
        }
    }
}

#[test]
fn the_psb_and_the_layered_tiffs_are_their_psds_twins() {
    let Some(root) = take2() else { return };
    for (twin, source) in [
        ("layers16.psb", "layers16.psd"),
        ("layers16.tif", "layers16.psd"),
        ("layers8.tif", "layers8.psd"),
    ] {
        let a = open(&file(&root, "layers2", twin), 0);
        let b = open(&file(&root, "layers2", source), 0);
        let (mut left, mut right) = (Vec::new(), Vec::new());
        snapshot(unsafe { &*a }.root_layers(), &mut left);
        snapshot(unsafe { &*b }.root_layers(), &mut right);
        assert_eq!(left.len(), right.len(), "{twin}");
        for (l, r) in left.iter().zip(&right) {
            assert!(l == r, "{twin} against {source}: a record differs");
        }
        unsafe {
            dr_precision_doc_free(a);
            dr_precision_doc_free(b);
        }
    }
}

// ------------------------------------------------------------ hypotheses --

#[test]
fn the_overlay_blend_if_and_levels_hypotheses_as_measured() {
    let Some(root) = take2() else { return };
    let measured = measured(&root);
    let path = file(&root, "styled2", "styled8.psd");
    let notice = report(&path);
    let carried = notice["observations"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .find(|o| o.contains("(\u{201c}Overlay\u{201d}) has Photoshop layer effects ("))
        .unwrap_or_else(|| panic!("{notice}"))
        .to_string();
    assert!(carried.contains("color_overlay"), "{carried}");
    let doc = open(&path, 0);
    let d = unsafe { &*doc };
    // H-L, recorded only: whether Photoshop's Mode ▸ Lab Color (Mrge false)
    // kept the Levels adjustment layer (take1: discarded).
    let rows = measured["results"]["styled2"]["report"]["rows"]
        .as_array()
        .unwrap();
    eprintln!(
        "H-L: Photoshop's rows hold a Levels layer: {}; the import holds one: {}",
        rows.iter().any(|r| r["name"] == "Levels"),
        find(d.root_layers(), "Levels").is_some()
    );
    // H-C: the `LbCl` read as the sRGB hex of its Lab colour, the authored
    // #40a0c8 back within one code a channel.
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
    eprintln!("H-C: the Color Overlay reads back as {hex}");
    for (c, authored) in [0x40u8, 0xa0, 0xc8].into_iter().enumerate() {
        let v = u8::from_str_radix(&hex[1 + 2 * c..3 + 2 * c], 16).unwrap();
        assert!(v.abs_diff(authored) <= 1, "{hex} channel {c}");
    }
    // H-B: slot 1, carried as ONE Lightness (red) range.
    let ranged = find(d.root_layers(), "Ranged").unwrap();
    let blend_if = ranged.blend_if().expect("the Blend If is carried");
    let ranges: Vec<_> = blend_if.ranges().collect();
    assert_eq!(ranges.len(), 1, "{blend_if:?}");
    assert_eq!(format!("{:?}", ranges[0].channel), "Red");
    assert_eq!(ranges[0].this_layer, [0, 0, 255, 255]);
    assert_eq!(ranges[0].underlying, [0, 0, 96, 160]);
    unsafe { dr_precision_doc_free(doc) };
}
