//! The Photoshop takes' records of the Lab wand, Select ▸ Similar and Grow,
//! read IN PLACE and skipped with a note when absent (under
//! `~/devel/doctored-fixtures/photoshop/`, Photoshop 26.10.0):
//! `scripts/photoshop-probe/lab_select_682_take.py`'s take1
//! (`lab-select-682/take1`), the wand cases of `lab_select_take.py`'s take1
//! (`lab-select-547/take1`), which that runner re-scored, and
//! `lab_select_5_take.py`'s take1 and take2 (`lab-select-5`), Similar and Grow
//! only. Bound, declared before measuring: EXACT membership on every pixel
//! (the takes' gate; the capture's mask is L\* < 50, Anti-alias off). Each
//! scene Photoshop saved is opened by `dr_precision_doc_open_lab`; the wand
//! runs at the click (Photoshop's (x + 0.5, y + 0.5) lands on (x + 1, y + 1):
//! (25, 59) on 682's take1, (21, 37) on 547's) with the host's tolerance
//! 257·t, Contiguous; Similar (global) and Grow (contiguous) run at 8 and 16
//! bits from each case's full rectangle (682's (8, 8, 24, 24); `lab-select-5`'s
//! from its job files). Nothing from the takes is committed.
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::ffi_lab::dr_precision_doc_open_lab;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::common::{cpath, take_err_string};
use crate::gray_ops::{free, LIMIT};
use crate::lab_psd_fixture::read_lab_tiff;
use crate::precision_select::select;

const SIMILAR_RECT: (u32, u32, u32, u32) = (8, 8, 24, 24);

/// A take's folder under the fixtures, or None when this machine has none.
fn take(name: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let folder = PathBuf::from(home)
        .join("devel/doctored-fixtures/photoshop")
        .join(name);
    folder.join("decided.json").is_file().then_some(folder)
}

fn open(scene: &Path) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_lab(cpath(scene).as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(
        !doc.is_null(),
        "{}: {}",
        scene.display(),
        take_err_string(err)
    );
    doc
}

/// The capture's mask (L\* < 50) at the scene's size.
fn mask(capture: &Path, size: (u32, u32)) -> Vec<bool> {
    let tiff = read_lab_tiff(&std::fs::read(capture).unwrap());
    assert_eq!((tiff.width, tiff.height), size, "{}", capture.display());
    let top = if tiff.bits == 8 { 255.0 } else { 65535.0 };
    tiff.planes[0]
        .iter()
        .map(|v| 100.0 * f64::from(*v) / top < 50.0)
        .collect()
}

fn mismatched(got: &[u16], want: &[bool]) -> usize {
    got.iter()
        .zip(want)
        .filter(|(g, w)| (**g == 65535) != **w || (**g != 0 && **g != 65535))
        .count()
}

/// The wand case `name` of the take at `folder` against its capture.
fn wand_case(folder: &Path, name: &str, bits: u32, t: u32, click: (u32, u32), size: (u32, u32)) {
    let doc = open(&folder.join(format!("jobs/lab{bits}/scene.tif")));
    assert_eq!(unsafe { &*doc }.dimensions(), size, "{name}: the scene");
    let operation = json!({"kind":"magic_wand","x":click.0,"y":click.1,"tolerance":257 * t,
        "contiguous":true});
    let got = select(doc, &operation, &[]).unwrap_or_else(|e| panic!("{name}: {e}"));
    let want = mask(&folder.join(format!("jobs/select/{name}.tif")), size);
    assert_eq!(mismatched(&got, &want), 0, "{name}: pixels off Photoshop's");
    free(doc);
}

fn decided(folder: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(folder.join("decided.json")).unwrap()).unwrap()
}

#[test]
fn lab_wand_matches_photoshop_on_both_takes() {
    match take("lab-select-682/take1") {
        None => eprintln!("SKIP: lab-select-682/take1 is absent on this machine"),
        Some(folder) => {
            let decided = decided(&folder);
            assert_eq!(decided["valid"], true, "take1 is not VALID");
            assert_eq!(
                decided["final"]["select_magic_wand"],
                json!({"8": "C6", "16": "C9"}),
                "take1's build"
            );
            let rows = decided["wand"].as_array().unwrap();
            assert_eq!(rows.len(), 4, "take1's wand cases");
            for row in rows {
                let bits = row["bits"].as_u64().unwrap() as u32;
                let t = row["t"].as_u64().unwrap() as u32;
                wand_case(
                    &folder,
                    row["name"].as_str().unwrap(),
                    bits,
                    t,
                    (25, 59),
                    (300, 96),
                );
            }
        }
    }
    match take("lab-select-547/take1") {
        None => eprintln!("SKIP: lab-select-547/take1 is absent on this machine"),
        Some(folder) => {
            for bits in [8u32, 16] {
                for t in [10u32, 30] {
                    let name = format!("wand{bits}_t{t}");
                    wand_case(&folder, &name, bits, t, (21, 37), (300, 120));
                }
            }
        }
    }
}

/// A rectangle's full coverage on a `size` canvas.
fn rectangle(size: (u32, u32), (x0, y0, w, h): (u32, u32, u32, u32)) -> Vec<u16> {
    (0..size.0 * size.1)
        .map(|i| {
            let (x, y) = (i % size.0, i / size.0);
            if (x0..x0 + w).contains(&x) && (y0..y0 + h).contains(&y) {
                65535
            } else {
                0
            }
        })
        .collect()
}

/// One Similar (`kind` "similar") or Grow ("grow") case against its capture.
fn similar_case(doc: *mut PrecisionDocument, coverage: &[u16], kind: &str, t: u32, capture: &Path) {
    let size = unsafe { &*doc }.dimensions();
    let operation = json!({"kind":"similar","tolerance":257 * t,"contiguous":kind == "grow"});
    let name = capture.display();
    let got = select(doc, &operation, coverage).unwrap_or_else(|e| panic!("{name}: {e}"));
    let want = mask(capture, size);
    assert_eq!(mismatched(&got, &want), 0, "{name}: pixels off Photoshop's");
}

/// `lab-select-5`'s take at `folder`: every Similar and Grow case of every
/// job file (`inputs/lab{bits}_t{t}_{kind}.json`), each depth's scene the
/// `none` save of its first job. The count of cases run.
fn lab_select_5_cases(folder: &Path) -> usize {
    let mut jobs: Vec<PathBuf> = std::fs::read_dir(folder.join("inputs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            name.starts_with("lab") && name.ends_with(".json")
        })
        .collect();
    jobs.sort();
    let bodies: Vec<(String, Value)> = jobs
        .iter()
        .map(|p| {
            let stem = p.file_stem().unwrap().to_string_lossy().into_owned();
            (
                stem,
                serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap(),
            )
        })
        .collect();
    let mut seen = 0;
    for bits in [8u32, 16] {
        let scene = bodies
            .iter()
            .find_map(|(job, body)| {
                body["cases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["name"] == format!("none{bits}"))
                    .then(|| folder.join(format!("jobs/{job}/none{bits}.tif")))
            })
            .unwrap_or_else(|| panic!("{}: no none{bits} case", folder.display()));
        let doc = open(&scene);
        let size = unsafe { &*doc }.dimensions();
        for (job, body) in bodies
            .iter()
            .filter(|(job, _)| job.starts_with(&format!("lab{bits}_")))
        {
            for case in body["cases"].as_array().unwrap() {
                let kind = case["kind"].as_str().unwrap();
                if kind == "none" {
                    continue;
                }
                let r = case["rect"].as_array().unwrap();
                let rect = [0, 1, 2, 3].map(|k| r[k].as_u64().unwrap() as u32);
                let capture =
                    folder.join(format!("jobs/{job}/{}.tif", case["name"].as_str().unwrap()));
                similar_case(
                    doc,
                    &rectangle(size, (rect[0], rect[1], rect[2], rect[3])),
                    kind,
                    case["tolerance"].as_u64().unwrap() as u32,
                    &capture,
                );
                seen += 1;
            }
        }
        free(doc);
    }
    seen
}

#[test]
fn lab_similar_and_grow_match_photoshop() {
    match take("lab-select-682/take1") {
        None => eprintln!("SKIP: lab-select-682/take1 is absent on this machine"),
        Some(folder) => {
            let decided = decided(&folder);
            assert_eq!(decided["valid"], true, "take1 is not VALID");
            let coverage = rectangle((300, 96), SIMILAR_RECT);
            let mut seen = 0;
            for kind in ["similar", "grow"] {
                for row in decided[kind].as_array().unwrap() {
                    let name = row["name"].as_str().unwrap();
                    let bits = row["bits"].as_u64().unwrap() as u32;
                    let t = row["t"].as_u64().unwrap() as u32;
                    let doc = open(&folder.join(format!("jobs/lab{bits}/scene.tif")));
                    let capture = folder.join(format!("jobs/items/{name}.tif"));
                    similar_case(doc, &coverage, kind, t, &capture);
                    free(doc);
                    seen += 1;
                }
            }
            assert_eq!(seen, 8, "682's Similar and Grow cases at 8 and 16 bits");
        }
    }
    for (name, cases) in [("lab-select-5/take1", 296), ("lab-select-5/take2", 336)] {
        match take(name) {
            None => eprintln!("SKIP: {name} is absent on this machine"),
            Some(folder) => {
                let decided = decided(&folder);
                assert_eq!(decided["valid"], true, "{name} is not VALID");
                if name.ends_with("take2") {
                    assert_eq!(
                        (&decided["build"]["ship8"], &decided["build"]["ship16"]),
                        (&json!("PCTK-1000"), &json!("PCTK-1000")),
                        "take2's build"
                    );
                }
                assert_eq!(lab_select_5_cases(&folder), cases, "{name}'s cases");
            }
        }
    }
}
