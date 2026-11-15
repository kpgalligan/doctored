//! The Photoshop takes' records of Select ▸ Similar and Grow on RGB, Gray
//! and CMYK documents at 8 and 16 bits, read IN PLACE and skipped with a note
//! when absent (under `~/devel/doctored-fixtures/photoshop/`, Photoshop
//! 26.10.0): `scripts/photoshop-probe/similar_trim_61_take.py`'s take1 and
//! take2 (`similar-trim-61`). The takes' gate is EXACT membership on every
//! pixel (a capture is the selection black on white for RGB and Gray, K on
//! paper for CMYK, Anti-alias off). Each mode's scene at each depth is the
//! take's own `none` save, opened by the core: RGB at 8 bits as a byte
//! document (`dr_doc_open`) through `dr_doc_select_similar` with the byte
//! tolerance t; RGB at 16 bits and CMYK by `dr_precision_doc_open`, Gray by
//! `dr_precision_doc_open_gray`, each through the typed `similar` kind with
//! 257·t. Similar (global) and Grow (contiguous) run from each case's
//! rectangle, read from the job files. Bound, declared before measuring: the
//! core is off each take by exactly the pixels the runner's independent
//! predictor of the shipped rule recorded (`decided.json`): 0 for Gray at 8
//! bits; at 16 bits only probes one u16 code past the bound (the 16-bit key
//! form); for RGB and CMYK, whose measured key form is not the shipped one
//! (#61, held), the full box's recorded miss. Nothing from the takes is
//! committed.
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
/// key form the core compares (bytes at 8 bits, u16 codes at 16; cmyk's PN7)
/// with its reference rule (Gray's trim, PCTK-1000; RGB's and CMYK's full
/// box, ALL). Gray's are 0 at 8 bits and, at 16, only the probes one u16
/// code past the bound.
fn recorded_off(row: &Value, mode: &str, bits: u32) -> u64 {
    let form = match (mode, bits) {
        ("cmyk", 8) => "PN7/K8",
        ("cmyk", _) => "PN7/K16",
        (_, 8) => "native/K8",
        _ => "native/K16",
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
                assert_eq!(ran[0].1, 0, "{name}: Gray at 8 bits is exact");
            }
        }
    }
}
