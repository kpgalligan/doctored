//! #683: the Photoshop takes' records, read IN PLACE and skipped
//! with a note when absent: `scripts/photoshop-probe/lab_select_682_take.py`'s
//! take1 (`~/devel/doctored-fixtures/photoshop/lab-select-682/take1`,
//! Photoshop 26.10.0) and the wand cases of `lab_select_take.py`'s take1
//! (`lab-select-547/take1`), which the new runner re-scored. Bound, declared
//! before measuring: EXACT membership on every pixel (the take's gate; the
//! capture's mask is L\* < 50, Anti-alias off). Each saved scene is opened by
//! `dr_precision_doc_open_lab`; the wand runs at the click (Photoshop's
//! (x + 0.5, y + 0.5) lands on (x + 1, y + 1): (25, 59) on take1, (21, 37) on
//! #547's) with the host's tolerance 257·t, Contiguous; at 16 bits Select ▸
//! Similar (global) and Grow (contiguous) run from the full rectangle (8, 8,
//! 24, 24); at 8 bits they refuse naming #683. Nothing from the takes is
//! committed.
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

#[test]
fn lab_similar_and_grow_match_photoshop_at_16_bits() {
    let Some(folder) = take("lab-select-682/take1") else {
        eprintln!("SKIP: lab-select-682/take1 is absent on this machine");
        return;
    };
    let decided = decided(&folder);
    assert_eq!(decided["valid"], true, "take1 is not VALID");
    assert_eq!(
        decided["final"]["select_similar"],
        json!({"8": null, "16": "C9"})
    );
    assert_eq!(
        decided["final"]["select_grow"],
        json!({"8": null, "16": "C9"})
    );
    let (x0, y0, w, h) = SIMILAR_RECT;
    let coverage: Vec<u16> = (0..300 * 96u32)
        .map(|i| {
            let (x, y) = (i % 300, i / 300);
            if (x0..x0 + w).contains(&x) && (y0..y0 + h).contains(&y) {
                65535
            } else {
                0
            }
        })
        .collect();
    let mut seen = 0;
    for kind in ["similar", "grow"] {
        let rows = decided[kind].as_array().unwrap();
        for row in rows {
            let name = row["name"].as_str().unwrap();
            let bits = row["bits"].as_u64().unwrap() as u32;
            let t = row["t"].as_u64().unwrap() as u32;
            let doc = open(&folder.join(format!("jobs/lab{bits}/scene.tif")));
            let operation = json!({"kind":"similar","tolerance":257 * t,
                "contiguous":kind == "grow"});
            let result = select(doc, &operation, &coverage);
            if bits == 8 {
                let label = if kind == "grow" {
                    "select_similar with contiguous true (Grow)"
                } else {
                    "select_similar"
                };
                assert_eq!(
                    result.unwrap_err(),
                    format!(
                        "{label} is unavailable in an 8-bit Lab Color document until its Lab \
                         tolerance rule is measured (#683)."
                    ),
                    "{name}"
                );
            } else {
                let got = result.unwrap_or_else(|e| panic!("{name}: {e}"));
                let want = mask(&folder.join(format!("jobs/items/{name}.tif")), (300, 96));
                assert_eq!(mismatched(&got, &want), 0, "{name}: pixels off Photoshop's");
                seen += 1;
            }
            free(doc);
        }
    }
    assert_eq!(seen, 4, "the 16-bit Similar and Grow cases");
}
