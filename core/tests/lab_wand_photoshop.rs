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
//! from its job files). `lab_similar_60_take.py`'s take1 (`lab-similar-60`,
//! its VALID C and S scenes and the 8-bit RGB control, on the byte path) and
//! the three case jobs its take2 finished before it stalled start from the
//! take's own coverage instead (a plane inside a footprint, a rectangle, or
//! Photoshop's feather or elliptical marquee read from a `sel` capture);
//! bound, declared before measuring: the exact class OUT / PART / FULL of
//! every pixel (coverage up to 128 of 65535 OUT, from 65407 FULL), a pixel
//! FULL where the kernel admits it and otherwise its starting class. Nothing
//! from the takes is committed.
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_open};
use doctored_core::ffi_lab::dr_precision_doc_open_lab;
use doctored_core::ffi_selection_tools::dr_doc_select_similar;
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

/// A coverage's class (`lab_similar_60_take.py`'s): OUT up to 128, FULL from
/// 65535 − 128, PART between.
fn class(v: u16) -> u8 {
    if v <= 128 {
        0
    } else if v >= 65535 - 128 {
        2
    } else {
        1
    }
}

/// A capture's coverage: the selection filled with L\* 0 over white, so
/// (255 − L8)·257 at 8 bits and 65535 − L16 at 16.
fn captured(capture: &Path, size: (u32, u32)) -> Vec<u16> {
    let tiff = read_lab_tiff(&std::fs::read(capture).unwrap());
    assert_eq!((tiff.width, tiff.height), size, "{}", capture.display());
    tiff.planes[0]
        .iter()
        .map(|&l| {
            if tiff.bits == 8 {
                (255 - l) * 257
            } else {
                65535 - l
            }
        })
        .collect()
}

/// A manifest case's starting coverage: its scene's plane (the Grayscale TIFF
/// beside the scene) inside a footprint, a full rectangle, or a `sel` capture
/// (Photoshop's own feather or elliptical marquee).
fn start_coverage(folder: &Path, manifest: &Value, case: &Value, size: (u32, u32)) -> Vec<u16> {
    let start = &case["start"];
    let rect = |v: &Value| [0, 1, 2, 3].map(|k| v[k].as_u64().unwrap() as u32);
    let inside = |r: [u32; 4], i: u32| {
        let (x, y) = (i % size.0, i / size.0);
        (r[0]..r[0] + r[2]).contains(&x) && (r[1]..r[1] + r[3]).contains(&y)
    };
    if let Some(sel) = start.get("sel") {
        return captured(&folder.join(sel.as_str().unwrap()), size);
    }
    if let Some(r) = start.get("rect") {
        let r = rect(r);
        return (0..size.0 * size.1)
            .map(|i| if inside(r, i) { 65535 } else { 0 })
            .collect();
    }
    let r = rect(&start["plane"]);
    let doc = case["doc"].as_str().unwrap();
    let path = folder.join(manifest["planes"][doc].as_str().unwrap());
    let plane = read_lab_tiff(&std::fs::read(path).unwrap());
    let wide = if plane.bits == 8 { 257 } else { 1 };
    plane.planes[0]
        .iter()
        .enumerate()
        .map(|(i, &v)| if inside(r, i as u32) { v * wide } else { 0 })
        .collect()
}

/// One `lab-similar-60` scene as the app holds it: Lab typed, the 8-bit RGB
/// control on the byte path.
enum Scene60 {
    Lab(*mut PrecisionDocument),
    Byte(*mut DrDocument, (u32, u32)),
}

impl Scene60 {
    fn open(mode: &str, path: &Path) -> Scene60 {
        if mode != "rgb" {
            return Scene60::Lab(open(path));
        }
        let mut err = ptr::null_mut();
        let doc = unsafe { dr_doc_open(cpath(path).as_ptr(), ptr::null(), &mut err) };
        assert!(
            !doc.is_null(),
            "{}: {}",
            path.display(),
            take_err_string(err)
        );
        let tiff = read_lab_tiff(&std::fs::read(path).unwrap());
        Scene60::Byte(doc, (tiff.width, tiff.height))
    }

    fn size(&self) -> (u32, u32) {
        match self {
            Scene60::Lab(doc) => unsafe { &**doc }.dimensions(),
            Scene60::Byte(_, size) => *size,
        }
    }

    /// Similar (Grow when `grow`) from `start`: the admitted region, u16.
    fn run(&self, start: &[u16], t: u64, grow: bool, name: &str) -> Vec<u16> {
        match self {
            Scene60::Lab(doc) => {
                let operation = json!({"kind":"similar","tolerance":257 * t,"contiguous":grow});
                select(*doc, &operation, start).unwrap_or_else(|e| panic!("{name}: {e}"))
            }
            Scene60::Byte(doc, _) => {
                let mask: Vec<u8> = start.iter().map(|v| (v / 257) as u8).collect();
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
                assert!(ok, "{name}: {}", take_err_string(err));
                out.iter().map(|v| u16::from(*v) * 257).collect()
            }
        }
    }

    fn free(self) {
        match self {
            Scene60::Lab(doc) => free(doc),
            Scene60::Byte(doc, _) => unsafe { dr_doc_free(doc) },
        }
    }
}

/// `lab-similar-60`'s take at `folder`: every Similar and Grow case of its
/// manifest that `keep` accepts (Lab, and the 8-bit RGB control), each scene
/// the `none` save of its depth. A pixel's class is FULL where the kernel
/// admits it, else its starting coverage's class (the admitted region is
/// added to the selection), against the capture's. The count of cases run.
fn lab_similar_60_cases(folder: &Path, keep: impl Fn(&Value) -> bool) -> usize {
    let decided = decided(folder);
    let manifest = &decided["manifest"];
    let mut scenes: Vec<(String, Scene60)> = Vec::new();
    let mut seen = 0;
    for case in manifest["cases"].as_array().unwrap() {
        if !keep(case) {
            continue;
        }
        let key = case["doc"].as_str().unwrap();
        if !scenes.iter().any(|(k, _)| k == key) {
            let path = folder.join(manifest["none"][key].as_str().unwrap());
            let scene = Scene60::open(case["mode"].as_str().unwrap(), &path);
            scenes.push((key.to_string(), scene));
        }
        let scene = &scenes.iter().find(|(k, _)| k == key).unwrap().1;
        let size = scene.size();
        let start = start_coverage(folder, manifest, case, size);
        let capture = folder.join(case["capture"].as_str().unwrap());
        let want = captured(&capture, size);
        let name = capture.display().to_string();
        let got = scene.run(
            &start,
            case["t"].as_u64().unwrap(),
            case["kind"] == "grow",
            &name,
        );
        let off = got
            .iter()
            .zip(&start)
            .zip(&want)
            .filter(|((g, s), w)| class(if **g == 65535 { **g } else { **s }) != class(**w))
            .count();
        assert_eq!(off, 0, "{name}: pixels off Photoshop's class");
        seen += 1;
    }
    for (_, scene) in scenes {
        scene.free();
    }
    seen
}

/// The jobs `lab-similar-60/take2` finished before its stall (its fifth job,
/// `labC8_t27_grow`, timed out): read-outs of the held-out scene B.
const TAKE2_FINISHED: [&str; 3] = ["labC8_t13_similar", "labC8_t13_grow", "labC8_t27_similar"];

#[test]
fn lab_similar_and_grow_from_partial_and_ragged_selections_match_photoshop() {
    match take("lab-similar-60/take1") {
        None => eprintln!("SKIP: lab-similar-60/take1 is absent on this machine"),
        Some(folder) => {
            let decided = decided(&folder);
            assert_eq!(
                decided["valid_kinds"],
                json!({"C": true, "S": true, "T": false}),
                "take1's VALID scenes"
            );
            let cases = lab_similar_60_cases(&folder, |case| {
                decided["valid_kinds"][case["scene"].as_str().unwrap()] == true
            });
            assert_eq!(
                cases, 252,
                "take1's C and S cases at 8 and 16 bits and the RGB control's at 8"
            );
        }
    }
    match take("lab-similar-60/take2") {
        None => eprintln!("SKIP: lab-similar-60/take2 is absent on this machine"),
        Some(folder) => {
            let measured: Value = serde_json::from_str(
                &std::fs::read_to_string(folder.join("measured.json")).unwrap(),
            )
            .unwrap();
            for job in measured["jobs"].as_array().unwrap() {
                if TAKE2_FINISHED.contains(&job["job"].as_str().unwrap()) {
                    assert_eq!(job["error"], Value::Null, "take2's {}", job["job"]);
                }
            }
            let cases = lab_similar_60_cases(&folder, |case| {
                TAKE2_FINISHED.contains(&case["job"].as_str().unwrap())
            });
            assert_eq!(cases, 48, "take2's finished C cases at 8 bits");
        }
    }
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
