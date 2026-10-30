//! #547 Indexed phase 2a: Photoshop's menu-walk and painting take of an
//! Indexed Color document (`~/devel/doctored-fixtures/photoshop/
//! indexed-mode-547/take1`, never committed; `scripts/photoshop-probe/
//! indexed_mode_take.py`, whose docstring fixes the scene, the candidates, the
//! gate and the build table), read in place: SKIP when it is absent.
//!
//! The record: the take stopped nowhere, its menu and paint readings are
//! VALID, and the paint verdict is the one take1 recorded ([`RECORDED_PAINT`]:
//! MISS, neither nearest-by-RGB nor nearest-by-ΔE predicts the stroke's
//! anti-aliased edge, though every pixel of the ink itself took the
//! nearest-by-RGB entry). The gate: every op of the build's `ops.disabled`
//! refuses with the permanent sentence, and `phase_3` and `phase_2b` with
//! the RGB-only sentence (since phase 3c; a bare `selection` names no kind,
//! so it still refuses), except the ops phases 2b to 3b admit
//! ([`ADMITTED`]), the ones phases 3a and 3b refuse for good ([`PERMANENT`], the
//! permanent sentence) and the ones that need more than one layer
//! ([`ONE_LAYER`], the one-layer sentence), on an Indexed document through
//! the FFI. Photoshop's
//! own palette TIFFs (`indexed.tif`, `painted.tif`) open through
//! `dr_precision_doc_open_indexed` with the six colours of the scene's
//! palette. The H0-only replay of the stroke (the spec's third check) does not
//! apply to a MISS.
//!
//! Phase 2b: the edge rule shipped (`indexed_edge`, THRESH at k = 131, the
//! controller's RULING-1 fallback) replayed on Photoshop's own pixels of
//! take1 and take2 (`indexed.tif` the plane before, `rgb-painted.tif` the
//! RGB stroke read as RGBA8 by `dr_image_open`, `painted.tif` Photoshop's
//! Indexed stroke): it predicts exactly
//! the counts the runner recorded for THRESH(131) ([`THRESH_RECORD`]) and
//! every pixel of the ink itself. take2's `judged.json` is VALID,
//! NONDETERMINISTIC (one stroke painted twice from one history state gave
//! two edges) and NOT CONFIRMED, the verdict that ships the fallback. Each
//! SKIPs when its folder is absent.

use std::ffi::CString;
use std::path::PathBuf;
use std::ptr;

use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::indexed_edge::reindex_codes;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::indexed_fixture::{
    color_table, free, indices, open_indexed, open_rgba8, palette_of, rgb_doc, to_indexed, LIMIT,
};

/// take1's paint verdict (`decided.json` `paint.verdict`).
const RECORDED_PAINT: &str = "MISS";

/// The ops phases 3a and 3b refuse for good in Indexed Color (the layers
/// paste, Puppet Warp, a mask transform, HDR Toning).
const PERMANENT: &[&str] = &[
    "hdr_toning",
    "paste_layers",
    "puppet_selected",
    "puppet_warp",
    "transform_mask",
];

/// The ops that need more than one layer (phases 3a and 3b): the one-layer
/// sentence.
const ONE_LAYER: &[&str] = &[
    "align_set",
    "auto_blend_layers",
    "distribute_set",
    "flatten",
    "remove_layers",
];

/// The core ops phases 2b, 2c, 2d and 3a admit on an Indexed document
/// (`indexed_document::admit_edit`); phase 3b's `set_channel`,
/// `read_plane` and selection kinds need their own fields.
const ADMITTED: &[&str] = &[
    "paint_blend",
    "bucket_fill",
    "stroke_layer_edge",
    "paint",
    "erase",
    "erase_native",
    "resize",
    "crop",
    "canvas_size",
    "straighten",
    "rotate_canvas",
    "rotate90",
    "rotate180",
    "rotate270",
    "flip_horizontal",
    "flip_vertical",
    "perspective_crop",
    "affine",
    "perspective",
    "affine_set",
    "perspective_set",
    "transform_selected",
    "rename_layer",
    "layer_label",
    "layer_lock",
    "move_selected",
    "layer_visible",
    "move_layer_to",
    "layer_open",
];

/// THRESH(131)'s matched and changed pixels as the runner recorded them
/// (`fit1/fit.json` for take1; take2's `judged.json` for its twin and
/// fresh stroke), by take and job folder.
const THRESH_RECORD: &[(&str, &str, usize, usize)] = &[
    ("take1", "jobs/indexed", 751, 806),
    ("take2", "jobs/twin", 749, 803),
    ("take2", "jobs/fresh", 992, 1156),
];

/// The scene's palette (`indexed_mode_take.PALETTE`).
const PALETTE: [[u8; 3]; 6] = [
    [0, 0, 0],
    [255, 255, 255],
    [200, 40, 40],
    [40, 160, 60],
    [40, 60, 200],
    [128, 128, 128],
];

fn edit(doc: *const PrecisionDocument, request: Value) -> String {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    take_err_string(err)
}

fn names(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("not a list: {value}"))
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn photoshops_indexed_take_is_the_gates_table() {
    let Some(root) = std::env::var("HOME").ok().map(|home| {
        PathBuf::from(home).join("devel/doctored-fixtures/photoshop/indexed-mode-547/take1")
    }) else {
        eprintln!("SKIP: HOME is unset, so the Photoshop take cannot be found");
        return;
    };
    let Some(decided) = std::fs::read_to_string(root.join("decided.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    else {
        eprintln!("SKIP: {} has no decided.json", root.display());
        return;
    };
    assert!(decided["stop"].is_null(), "{}", decided["stop"]);
    assert_eq!(decided["menu"]["valid"], json!(true), "{}", decided["menu"]);
    assert_eq!(
        decided["paint"]["valid"],
        json!(true),
        "{}",
        decided["paint"]
    );
    assert_eq!(decided["paint"]["verdict"], json!(RECORDED_PAINT));

    let rgb = rgb_doc(2, 1, &[[1, 2, 3, 255], [4, 5, 6, 255]]);
    let indexed = to_indexed(rgb, &json!({"palette":"exact"})).unwrap();
    let ops = &decided["build"]["ops"];
    let mut checked = 0;
    for (group, sentence) in [
        ("disabled", "{} is not available in Indexed Color."),
        (
            "phase_3",
            "{} is unavailable in an Indexed Color document: Doctored offers it on RGB \
             documents only.",
        ),
        (
            "phase_2b",
            "{} is unavailable in an Indexed Color document: Doctored offers it on RGB \
             documents only.",
        ),
    ] {
        for op in names(&ops[group]) {
            if ADMITTED.contains(&op.as_str()) {
                continue;
            }
            let sentence = if PERMANENT.contains(&op.as_str()) {
                "{} is not available in Indexed Color."
            } else if ONE_LAYER.contains(&op.as_str()) {
                "{} needs more than one layer, and an Indexed Color document holds one."
            } else {
                sentence
            };
            assert_eq!(
                edit(indexed, json!({ "op": op })),
                sentence.replace("{}", &op),
                "{group}"
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "the build lists no op");
    free(indexed);
    free(rgb);

    // Photoshop's own palette TIFFs open as Indexed documents with the scene's palette.
    let folder = PathBuf::from(decided["job_folder"].as_str().unwrap());
    for name in ["indexed.tif", "painted.tif"] {
        let doc = open_indexed(&folder.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let table = color_table(doc);
        let colors: Vec<[u8; 3]> = table["colors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                let c = c.as_array().unwrap();
                [0, 1, 2].map(|i| c[i].as_u64().unwrap() as u8)
            })
            .collect();
        for entry in PALETTE {
            assert!(
                colors.contains(&entry),
                "{name}: {entry:?} not in {colors:?}"
            );
        }
        free(doc);
    }
}

fn takes() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join("devel/doctored-fixtures/photoshop/indexed-mode-547"))
}

/// The shipped rule replayed on one job folder: (matched, changed, every
/// ink pixel exact).
fn replay(folder: &std::path::Path, ink: [u8; 3]) -> (usize, usize, bool) {
    let before_doc = open_indexed(&folder.join("indexed.tif")).unwrap();
    let (palette, t) = palette_of(before_doc);
    let before: Vec<u8> = indices(before_doc).iter().map(|&i| i as u8).collect();
    let (width, height) = unsafe { &*before_doc }.dimensions();
    let (w, h, after) = open_rgba8(&folder.join("rgb-painted.tif"));
    assert_eq!((w, h), (width, height), "{}", folder.display());
    let painted = open_indexed(&folder.join("painted.tif")).unwrap();
    let (painted_palette, _) = palette_of(painted);
    let expected = indices(painted);
    let out = reindex_codes(&palette, t, width, height, &before, &after, ink).unwrap();
    let (mut matched, mut changed, mut inks) = (0, 0, true);
    for i in 0..out.len() {
        let s = palette[usize::from(before[i])];
        let c = [after[i][0], after[i][1], after[i][2]];
        if c == s {
            continue;
        }
        changed += 1;
        let exact = palette[usize::from(out[i])] == painted_palette[expected[i] as usize];
        matched += usize::from(exact);
        if c == ink && !exact {
            inks = false;
        }
    }
    free(painted);
    free(before_doc);
    (matched, changed, inks)
}

fn read_json(path: &std::path::Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

#[test]
fn the_shipped_edge_rule_replays_photoshops_takes() {
    let Some(root) = takes() else {
        eprintln!("SKIP: HOME is unset, so the Photoshop takes cannot be found");
        return;
    };
    for &(take, job, matched, changed) in THRESH_RECORD {
        let folder = root.join(take).join(job);
        let name = job.rsplit('/').next().unwrap();
        let inputs = root.join(take).join("inputs").join(format!("{name}.json"));
        let Some(stimulus) = read_json(&inputs).filter(|_| folder.is_dir()) else {
            eprintln!("SKIP: {} is absent", folder.display());
            continue;
        };
        let ink = [0, 1, 2].map(|c| stimulus["ink"][c].as_u64().unwrap() as u8);
        let got = replay(&folder, ink);
        assert_eq!(got, (matched, changed, true), "{take}/{job}");
    }
}

#[test]
fn take2_is_recorded_valid_nondeterministic_and_not_confirmed() {
    let Some(root) = takes() else {
        eprintln!("SKIP: HOME is unset, so the Photoshop takes cannot be found");
        return;
    };
    let Some(judged) = read_json(&root.join("take2/judged.json")) else {
        eprintln!("SKIP: take2 has no judged.json");
        return;
    };
    assert!(judged["stop"].is_null(), "{}", judged["stop"]);
    assert_eq!(judged["valid"], json!(true));
    assert_eq!(judged["deterministic"], json!(false));
    assert_eq!(judged["verdict"], json!("NOT CONFIRMED"));
    assert!(judged["hypothesis"]["candidate"].is_null());
    // The record behind THRESH_RECORD's take2 rows.
    for (job, matched, changed) in [("twin", 749, 803), ("fresh", 992, 1156)] {
        let frozen = &judged[job]["fit1"]["candidates"]["THRESH"]["frozen"];
        assert_eq!(frozen["params"]["k"], json!(131), "{job}");
        assert_eq!(frozen["matched"], json!(matched), "{job}");
        assert_eq!(frozen["of"], json!(changed), "{job}");
        assert_eq!(frozen["inks"], json!(true), "{job}");
    }
}
