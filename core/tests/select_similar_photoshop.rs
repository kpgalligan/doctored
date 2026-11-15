//! The Photoshop takes' records of Select ▸ Similar and Grow, and the Magic
//! Wand, on RGB, Gray and CMYK documents, read IN PLACE and skipped with a
//! note when absent (under `~/devel/doctored-fixtures/photoshop/`, Photoshop
//! 26.10.0): `scripts/photoshop-probe/similar_trim_61_take.py`'s take1 and
//! take2 (`similar-trim-61`, 8 and 16 bits) and
//! `scripts/photoshop-probe/key_resolution_80_take.py`'s take1 and take2
//! (`key-resolution-80`, 16 bits, codes off the ×257 grid). The takes' gate
//! is EXACT membership on every pixel (a capture is the selection black on
//! white for RGB and Gray, K on paper for CMYK, Anti-alias off). Each mode's
//! scene at each depth is the take's own `none` save, opened by the core: RGB
//! at 8 bits as a byte document (`dr_doc_open`) through
//! `dr_doc_select_similar` with the byte tolerance t; RGB at 16 bits and
//! CMYK by `dr_precision_doc_open`, Gray by `dr_precision_doc_open_gray`,
//! each through the typed `similar` and `magic_wand` kinds with 257·t.
//! Similar (global) and Grow (contiguous) run from each case's rectangle,
//! the wand from where Photoshop's click landed (the job's x + 1, y + 1),
//! read from the job files. Bounds, declared before measuring:
//! `similar-trim-61` is off each take by exactly the pixels the runner's
//! independent predictor of the shipped rule recorded (`decided.json`): 0
//! for Gray at both depths; for RGB and CMYK, whose measured key form is not
//! the shipped one (#61, held), the full box's recorded miss.
//! `key-resolution-80` is off by 0 pixels in every mode and tool (its
//! scenes are neutral, so the key form does not enter). Nothing from the
//! takes is committed.
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_open};
use doctored_core::ffi_gray::dr_precision_doc_open_gray;
use doctored_core::ffi_precision::dr_precision_doc_open;
use doctored_core::ffi_selection_tools::dr_doc_select_similar;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::common::{cpath, take_err_string};
use crate::gray_ops::{free, LIMIT};
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

/// The shipped rule's pixels off each take, per mode and depth, as the
/// runner's own predictor of that rule recorded them (`decided.json`): the
/// key form the core compares (bytes at both depths; cmyk's PN7) with its
/// reference rule (Gray's trim, PCTK-1000; RGB's and CMYK's full box, ALL).
/// Gray's are 0 at both depths.
fn recorded_off(row: &Value, mode: &str, bits: u32) -> u64 {
    let form = if mode == "cmyk" {
        "PN7/K8"
    } else {
        "native/K8"
    };
    let rule = if mode == "gray" { "pctk1000" } else { "all" };
    row["depths"][bits.to_string()][rule][form]
        .as_u64()
        .unwrap()
}

#[test]
fn rgb_gray_and_cmyk_similar_and_grow_against_photoshop() {
    for name in ["similar-trim-61/take1", "similar-trim-61/take2"] {
        let Some(folder) = take(name) else {
            eprintln!("SKIP: {name} is absent on this machine");
            continue;
        };
        let decided = decided(&folder);
        if name.ends_with("take2") {
            for (key, build) in [
                ("gray8", "trim"),
                ("gray16", "trim"),
                ("rgb8", "hold"),
                ("rgb16", "hold"),
                ("cmyk8", "hold"),
                ("cmyk16", "hold"),
            ] {
                assert_eq!(decided["build"][key][0], build, "take2's build of {key}");
            }
        }
        for mode in MODES {
            let row = &decided["modes"][mode];
            assert_eq!(row["valid"], true, "{name} {mode} is not VALID");
            if row["status"] == "UNAVAILABLE" {
                continue;
            }
            let ran = mode_cases(&folder, mode);
            assert_eq!(
                ran[0].0 + ran[1].0,
                row["expected"].as_u64().unwrap(),
                "{name} {mode}'s cases"
            );
            for (slot, bits) in [8u32, 16].into_iter().enumerate() {
                assert_eq!(
                    ran[slot].1,
                    recorded_off(row, mode, bits),
                    "{name} {mode} {bits} bits: pixels off Photoshop's"
                );
            }
            if mode == "gray" {
                assert_eq!(ran, [(ran[0].0, 0), (ran[1].0, 0)], "{name}: Gray is exact");
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
