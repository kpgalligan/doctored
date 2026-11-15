//! The Photoshop takes' records of Select ▸ Similar and Grow, the Magic Wand
//! and the Paint Bucket on RGB, Gray and CMYK documents, read IN PLACE and
//! skipped with a note when absent (under
//! `~/devel/doctored-fixtures/photoshop/`, Photoshop 26.10.0):
//! `scripts/photoshop-probe/similar_trim_61_take.py`'s take1 and take2
//! (`similar-trim-61`, 8 and 16 bits),
//! `scripts/photoshop-probe/key_resolution_80_take.py`'s take1 and take2
//! (`key-resolution-80`, 16 bits, codes off the ×257 grid) and
//! `scripts/photoshop-probe/wand_key_79_take.py`'s take2 (`wand-key-79`, the
//! wand on RGB and CMYK and the bucket on RGB, 8 and 16 bits). The takes'
//! gate is EXACT membership on every pixel (a capture is the selection black
//! on white for RGB and Gray, K on paper for CMYK, Anti-alias off; a bucket
//! PNG's selection is its fill colour). Each mode's scene at each depth is
//! the take's own `none` save, opened by the core: RGB at 8 bits as a byte
//! document (`dr_doc_open`) through `dr_doc_select_similar`,
//! `dr_doc_magic_wand` and `dr_doc_bucket_fill` with the byte tolerance t;
//! RGB at 16 bits and CMYK by `dr_precision_doc_open`, Gray by
//! `dr_precision_doc_open_gray`, each through the typed `similar` and
//! `magic_wand` kinds and the `bucket_fill` edit with 257·t. Similar
//! (global) and Grow (contiguous) run from each case's rectangle, the wand
//! and the bucket from where Photoshop's click landed (the job's x + 1,
//! y + 1), read from the job files. Bound, declared before measuring: every
//! take is off by 0 pixels in every mode, depth and tool — the halved keys
//! with the per-key trim (`selection_similar`'s module doc). Nothing from the
//! takes is committed.
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::{dr_doc_bucket_fill, dr_doc_free, dr_doc_magic_wand, dr_doc_open};
use doctored_core::ffi_gray::dr_precision_doc_open_gray;
use doctored_core::ffi_precision::dr_precision_doc_open;
use doctored_core::ffi_selection_tools::dr_doc_select_similar;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::common::{cpath, layer_pixels, take_err_string};
use crate::gray_ops::{edit, free, layer, plan_samples, LIMIT};
use crate::lab_psd_fixture::read_lab_tiff;
use crate::precision_select::select;

const MODES: [&str; 3] = ["rgb", "gray", "cmyk"];

/// A take's folder under the fixtures, or None when this machine has none.
fn take(name: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let folder = PathBuf::from(home)
        .join("devel/doctored-fixtures/photoshop")
        .join(name);
    folder.join("decided.json").is_file().then_some(folder)
}

fn decided(folder: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(folder.join("decided.json")).unwrap()).unwrap()
}

/// One scene as the app holds it: 8-bit RGB on the byte path, the rest typed.
enum Scene {
    Byte(*mut DrDocument, (u32, u32)),
    Typed(*mut PrecisionDocument),
}

impl Scene {
    fn open(mode: &str, bits: u32, path: &Path) -> Scene {
        let c = cpath(path);
        let mut err = ptr::null_mut();
        let what = path.display();
        if mode == "rgb" && bits == 8 {
            let doc = unsafe { dr_doc_open(c.as_ptr(), ptr::null(), &mut err) };
            assert!(!doc.is_null(), "{what}: {}", take_err_string(err));
            let size = read_lab_tiff(&std::fs::read(path).unwrap());
            return Scene::Byte(doc, (size.width, size.height));
        }
        let doc = if mode == "gray" {
            let working = CString::new("gray_gamma_22").unwrap();
            unsafe {
                dr_precision_doc_open_gray(
                    c.as_ptr(),
                    working.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                )
            }
        } else {
            unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) }
        };
        assert!(!doc.is_null(), "{what}: {}", take_err_string(err));
        Scene::Typed(doc)
    }

    fn size(&self) -> (u32, u32) {
        match self {
            Scene::Byte(_, size) => *size,
            Scene::Typed(doc) => unsafe { &**doc }.dimensions(),
        }
    }

    /// Similar (`grow` false) or Grow from `rect` at byte tolerance `t`:
    /// whether each pixel came back fully selected, and how many came back
    /// neither empty nor full.
    fn run(&self, rect: [u32; 4], t: u32, grow: bool) -> (Vec<bool>, usize) {
        let (w, h) = self.size();
        let inside = |i: u32| {
            let (x, y) = (i % w, i / w);
            (rect[0]..rect[0] + rect[2]).contains(&x) && (rect[1]..rect[1] + rect[3]).contains(&y)
        };
        match self {
            Scene::Byte(doc, _) => {
                let mask: Vec<u8> = (0..w * h)
                    .map(|i| if inside(i) { 255 } else { 0 })
                    .collect();
                let mut out = vec![0u8; mask.len()];
                let mut err = ptr::null_mut();
                let ok = unsafe {
                    dr_doc_select_similar(
                        *doc,
                        mask.as_ptr(),
                        mask.len(),
                        t as u8,
                        grow,
                        out.as_mut_ptr(),
                        &mut err,
                    )
                };
                assert!(ok, "{}", take_err_string(err));
                let partial = out.iter().filter(|v| **v != 0 && **v != 255).count();
                (out.iter().map(|v| *v == 255).collect(), partial)
            }
            Scene::Typed(doc) => {
                let coverage: Vec<u16> = (0..w * h)
                    .map(|i| if inside(i) { 65535 } else { 0 })
                    .collect();
                let operation = json!({"kind":"similar","tolerance":257 * t,"contiguous":grow});
                let out = select(*doc, &operation, &coverage).unwrap_or_else(|e| panic!("{e}"));
                let partial = out.iter().filter(|v| **v != 0 && **v != 65535).count();
                (out.iter().map(|v| *v == 65535).collect(), partial)
            }
        }
    }
}

impl Scene {
    /// The typed Magic Wand from `seed` at byte tolerance `t` (Point
    /// Sample): whether each pixel came back fully selected.
    fn wand(&self, seed: (u32, u32), t: u32, contiguous: bool) -> Vec<bool> {
        let Scene::Typed(doc) = self else {
            panic!("the wand records are typed documents")
        };
        let operation = json!({"kind":"magic_wand","x":seed.0,"y":seed.1,
            "tolerance":257 * t,"contiguous":contiguous});
        let out = select(*doc, &operation, &[]).unwrap_or_else(|e| panic!("{e}"));
        assert!(out.iter().all(|v| *v == 0 || *v == 65535), "a hard wand");
        out.iter().map(|v| *v == 65535).collect()
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        match self {
            Scene::Byte(doc, _) => unsafe { dr_doc_free(*doc) },
            Scene::Typed(doc) => free(*doc),
        }
    }
}

/// The capture's selection: RGB and Gray black (below half) on white, CMYK
/// K (at least half) on paper.
fn capture(mode: &str, path: &Path, size: (u32, u32)) -> Vec<bool> {
    let tiff = read_lab_tiff(&std::fs::read(path).unwrap());
    assert_eq!((tiff.width, tiff.height), size, "{}", path.display());
    let half = if tiff.bits == 8 { 128 } else { 32768 };
    if mode == "cmyk" {
        tiff.planes[3].iter().map(|v| *v >= half).collect()
    } else {
        tiff.planes[0].iter().map(|v| *v < half).collect()
    }
}

/// Every Similar and Grow case of `mode` in the take at `folder` (its job
/// files `inputs/<mode>*.json`, each depth's scene the `none` save of the
/// job that holds it): per depth, the cases run and the pixels off
/// Photoshop's capture over all of them. Every result is all-or-nothing.
fn mode_cases(folder: &Path, mode: &str) -> [(u64, u64); 2] {
    let mut jobs: Vec<(String, Value)> = std::fs::read_dir(folder.join("inputs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            name.starts_with(mode) && name.ends_with(".json")
        })
        .map(|p| {
            let stem = p.file_stem().unwrap().to_string_lossy().into_owned();
            let body = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
            (stem, body)
        })
        .collect();
    jobs.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = [(0, 0); 2];
    for (slot, bits) in [8u32, 16].into_iter().enumerate() {
        let doc = format!("scene{bits}");
        let at_depth: Vec<&(String, Value)> = jobs
            .iter()
            .filter(|(_, body)| {
                body["cases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["doc"] == doc.as_str())
            })
            .collect();
        let Some(scene_job) = at_depth.iter().find(|(_, body)| {
            body["cases"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["name"] == format!("none{bits}"))
        }) else {
            continue;
        };
        let scene = Scene::open(
            mode,
            bits,
            &folder.join(format!("jobs/{}/none{bits}.tif", scene_job.0)),
        );
        let size = scene.size();
        for (job, body) in at_depth {
            for case in body["cases"].as_array().unwrap() {
                let kind = case["kind"].as_str().unwrap();
                if kind == "none" {
                    continue;
                }
                let name = case["name"].as_str().unwrap();
                let path = folder.join(format!("jobs/{job}/{name}.tif"));
                let r = case["rect"].as_array().unwrap();
                let rect = [0, 1, 2, 3].map(|k| r[k].as_u64().unwrap() as u32);
                let t = case["tolerance"].as_u64().unwrap() as u32;
                let (got, partial) = scene.run(rect, t, kind == "grow");
                assert_eq!(partial, 0, "{}: a partial result", path.display());
                let want = capture(mode, &path, size);
                out[slot].0 += 1;
                out[slot].1 += got.iter().zip(&want).filter(|(g, w)| g != w).count() as u64;
            }
        }
    }
    out
}

/// Both takes cover every mode and depth with Similar and Grow; the core
/// meets every case of each (the seven-projection box the RGB and CMYK
/// kernels shipped before missed 6716 and 9142 pixels at 8 bits on take1).
#[test]
fn rgb_gray_and_cmyk_similar_and_grow_against_photoshop() {
    for name in ["similar-trim-61/take1", "similar-trim-61/take2"] {
        let Some(folder) = take(name) else {
            eprintln!("SKIP: {name} is absent on this machine");
            continue;
        };
        let decided = decided(&folder);
        for mode in MODES {
            let row = &decided["modes"][mode];
            assert_eq!(row["valid"], true, "{name} {mode} is not VALID");
            assert_ne!(row["status"], "UNAVAILABLE", "{name} {mode}");
            let ran = mode_cases(&folder, mode);
            assert_eq!(
                ran[0].0 + ran[1].0,
                row["expected"].as_u64().unwrap(),
                "{name} {mode}'s cases"
            );
            for (slot, bits) in [8u32, 16].into_iter().enumerate() {
                assert!(ran[slot].0 > 0, "{name} {mode} ran no {bits}-bit case");
                assert_eq!(
                    ran[slot].1, 0,
                    "{name} {mode} {bits} bits: pixels off Photoshop's"
                );
            }
        }
    }
}

fn job_file(folder: &Path, name: &str) -> Value {
    let path = folder.join(format!("inputs/{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Every wand, Similar and Grow case of `mode` in a `key-resolution-80`
/// take (its job files `inputs/<mode>_wand.json` and `<mode>_items.json`,
/// the scene the wand job's `none` save): per tool, the cases run and the
/// pixels off Photoshop's capture over all of them.
fn resolution_cases(folder: &Path, mode: &str) -> [(&'static str, u64, u64); 3] {
    let scene = Scene::open(
        mode,
        16,
        &folder.join(format!("jobs/{mode}_wand/scene16.tif")),
    );
    let size = scene.size();
    let mut out = [("similar", 0, 0), ("grow", 0, 0), ("wand", 0, 0)];
    let mut tally = |tool: &str, got: Vec<bool>, path: PathBuf| {
        let want = capture(mode, &path, size);
        let slot = out.iter_mut().find(|row| row.0 == tool).unwrap();
        slot.1 += 1;
        slot.2 += got.iter().zip(&want).filter(|(g, w)| g != w).count() as u64;
    };
    for case in job_file(folder, &format!("{mode}_wand"))["wand"]
        .as_array()
        .unwrap()
    {
        let at = |key: &str| case[key].as_u64().unwrap() as u32;
        // Photoshop's click at (x + 0.5, y + 0.5) lands on pixel (x + 1, y + 1).
        let got = scene.wand(
            (at("x") + 1, at("y") + 1),
            at("tolerance"),
            case["contiguous"].as_bool().unwrap(),
        );
        let name = case["name"].as_str().unwrap();
        tally(
            "wand",
            got,
            folder.join(format!("jobs/{mode}_wand/{name}.tif")),
        );
    }
    for case in job_file(folder, &format!("{mode}_items"))["cases"]
        .as_array()
        .unwrap()
    {
        let kind = case["kind"].as_str().unwrap();
        let r = case["rect"].as_array().unwrap();
        let rect = [0, 1, 2, 3].map(|k| r[k].as_u64().unwrap() as u32);
        let t = case["tolerance"].as_u64().unwrap() as u32;
        let (got, partial) = scene.run(rect, t, kind == "grow");
        let name = case["name"].as_str().unwrap();
        assert_eq!(partial, 0, "{name}: a partial result");
        tally(
            kind,
            got,
            folder.join(format!("jobs/{mode}_items/{name}.tif")),
        );
    }
    out
}

/// The core's Magic Wand at (x, y), byte tolerance `t`, Point Sample: the
/// byte `dr_doc_magic_wand` on the byte scene, the typed kind otherwise.
fn scene_wand(scene: &Scene, (x, y): (u32, u32), t: u32, contiguous: bool) -> Vec<bool> {
    let Scene::Byte(doc, (w, h)) = scene else {
        return scene.wand((x, y), t, contiguous);
    };
    let mut mask = vec![7u8; (w * h) as usize];
    let done = unsafe { dr_doc_magic_wand(*doc, x, y, t as u8, contiguous, 1, mask.as_mut_ptr()) };
    assert!(done, "the byte wand at ({x}, {y})");
    assert!(mask.iter().all(|m| *m == 0 || *m == 255), "a hard wand");
    mask.iter().map(|m| *m == 255).collect()
}

/// The core's Paint Bucket at (x, y), byte tolerance `t`, filling the
/// opaque green no scene pixel holds on layer 0: which pixels hold it after.
fn scene_bucket(scene: &Scene, (x, y): (u32, u32), t: u32, contiguous: bool) -> Vec<bool> {
    match scene {
        Scene::Byte(doc, _) => {
            let fill = [0u8, 255, 0, 255];
            let out = unsafe {
                dr_doc_bucket_fill(
                    *doc,
                    0,
                    x as i32,
                    y as i32,
                    t as u8,
                    fill.as_ptr(),
                    contiguous,
                    ptr::null(),
                )
            };
            assert!(!out.is_null(), "the byte bucket at ({x}, {y})");
            let pixels = layer_pixels(out, 0);
            unsafe { dr_doc_free(out) };
            pixels.chunks_exact(4).map(|p| p == fill).collect()
        }
        Scene::Typed(doc) => {
            let request = json!({"op":"bucket_fill","layer":0,"color":[0.0,1.0,0.0,1.0],
                "color_space":"document","opacity":1.0,"x":x,"y":y,"tolerance":257 * t,
                "contiguous":contiguous});
            // The planned input is the full-coverage stroke the host sends.
            let n = plan_samples(*doc, &request).unwrap_or_else(|e| panic!("{e}"));
            let out = edit(*doc, &request, &vec![65535; n])
                .unwrap_or_else(|e| panic!("{e}"))
                .expect("the bucket fills something");
            let pixels = layer(out, 0, 16);
            free(out);
            pixels.iter().map(|p| *p == [0, 65535, 0, 65535]).collect()
        }
    }
}

/// A bucket PNG's selection: the pixels holding the fill (0, 255, 0) at its
/// depth.
fn bucket_capture(path: &Path, size: (u32, u32)) -> Vec<bool> {
    let png = image::open(path).unwrap().to_rgba16();
    assert_eq!(png.dimensions(), size, "{}", path.display());
    png.pixels().map(|p| p.0 == [0, 65535, 0, 65535]).collect()
}

/// Every wand and bucket case of `mode` at `bits` in a `wand-key-79` take
/// (its job files `inputs/<mode><bits>_wand.json` and, on RGB,
/// `<mode><bits>_bucket.json`; the scene the wand job's `none` save): per
/// tool, the cases run and the pixels off Photoshop's over all of them.
fn wand_key_cases(folder: &Path, mode: &str, bits: u32) -> [(&'static str, u64, u64); 2] {
    let job = format!("{mode}{bits}_wand");
    let scene = Scene::open(mode, bits, &folder.join(format!("jobs/{job}/scene.tif")));
    let size = scene.size();
    let mut out = [("wand", 0, 0), ("bucket", 0, 0)];
    let at = |case: &Value, key: &str| case[key].as_u64().unwrap() as u32;
    for case in job_file(folder, &job)["wand"].as_array().unwrap() {
        // Photoshop's click at (x + 0.5, y + 0.5) lands on pixel (x + 1, y + 1).
        let seed = (at(case, "x") + 1, at(case, "y") + 1);
        let contiguous = case["contiguous"].as_bool().unwrap();
        let got = scene_wand(&scene, seed, at(case, "tolerance"), contiguous);
        let name = case["name"].as_str().unwrap();
        let want = capture(mode, &folder.join(format!("jobs/{job}/{name}.tif")), size);
        out[0].1 += 1;
        out[0].2 += got.iter().zip(&want).filter(|(g, w)| g != w).count() as u64;
    }
    let job = format!("{mode}{bits}_bucket");
    if !folder.join(format!("inputs/{job}.json")).is_file() {
        return out;
    }
    for case in job_file(folder, &job)["cases"].as_array().unwrap() {
        let seed = (at(case, "x") + 1, at(case, "y") + 1);
        let contiguous = case["contiguous"].as_bool().unwrap();
        let got = scene_bucket(&scene, seed, at(case, "tolerance"), contiguous);
        let name = case["name"].as_str().unwrap();
        let want = bucket_capture(&folder.join(format!("jobs/{job}/{name}.png")), size);
        out[1].1 += 1;
        out[1].2 += got.iter().zip(&want).filter(|(g, w)| g != w).count() as u64;
    }
    out
}

/// `wand-key-79`'s take2 (take1 repeated by it, its bucket cases lost to the
/// harness): the Magic Wand on RGB and CMYK and the Paint Bucket on RGB, at
/// 8 and 16 bits, Contiguous off and on, t 11, 19 and 24, from fields whose
/// cyclic sums take both parities, with probes on mixed-sign rays and at
/// each halved difference's edge. The core meets every case; the
/// seven-projection box the wand shipped before missed 674 pixels a tool
/// and depth on RGB and 837 on CMYK.
#[test]
fn the_wand_and_the_bucket_on_rgb_and_cmyk_against_photoshop() {
    let name = "wand-key-79/take2";
    let Some(folder) = take(name) else {
        eprintln!("SKIP: {name} is absent on this machine");
        return;
    };
    let decided = decided(&folder);
    assert_eq!(decided["valid"], true, "{name} is not VALID");
    assert_eq!(decided["build"][0], 1, "{name}'s build row");
    for mode in ["rgb", "cmyk"] {
        for bits in [8u32, 16] {
            let row = &decided["rows"][format!("{mode}{bits}")];
            assert_eq!(row["valid"], true, "{name} {mode}{bits} is not VALID");
            let ran = wand_key_cases(&folder, mode, bits);
            assert_eq!(
                ran.iter().map(|r| r.1).sum::<u64>(),
                row["expected"].as_u64().unwrap(),
                "{name} {mode}{bits}'s cases"
            );
            assert!(ran[0].1 > 0, "{name} {mode}{bits} ran no wand case");
            assert_eq!(
                ran[1].1 > 0,
                mode == "rgb",
                "{name} {mode}{bits}'s bucket cases"
            );
            for (tool, _, off) in ran {
                assert_eq!(off, 0, "{name} {mode}{bits} {tool}: pixels off Photoshop's");
            }
        }
    }
}

#[test]
fn the_16_bit_tools_compare_bytes_against_photoshop() {
    for name in ["key-resolution-80/take1", "key-resolution-80/take2"] {
        let Some(folder) = take(name) else {
            eprintln!("SKIP: {name} is absent on this machine");
            continue;
        };
        let decided = decided(&folder);
        if name.ends_with("take2") {
            for mode in MODES {
                assert_eq!(
                    decided["build"][mode][0], "bytes",
                    "take2's build of {mode}"
                );
            }
        }
        for mode in MODES {
            let row = &decided["modes"][mode];
            assert_eq!(row["valid"], true, "{name} {mode} is not VALID");
            let ran = resolution_cases(&folder, mode);
            assert_eq!(
                ran.iter().map(|r| r.1).sum::<u64>(),
                row["expected"].as_u64().unwrap(),
                "{name} {mode}'s cases"
            );
            for (tool, cases, off) in ran {
                assert!(cases > 0, "{name} {mode} ran no {tool} case");
                assert_eq!(off, 0, "{name} {mode} {tool}: pixels off Photoshop's");
            }
        }
    }
}
