//! #1 Bitmap phase 2b: Photoshop's takes of Image ▸ Mode ▸ Bitmap, read in
//! place from `~/devel/doctored-fixtures/photoshop/bitmap-mode-547/`
//! (never committed; `scripts/photoshop-probe/bitmap_take.py`, whose
//! docstring fixes the scenes, the cases, the candidates, the gates and the
//! application rule before any session). Each part SKIPs when its record is
//! absent.
//!
//! * **take1** (`take1/decided.json`): it stopped nowhere, its reading is
//!   VALID, and each method's verdict and shipped candidate is the one take1
//!   recorded ([`RECORDED`]). The replay: the core converts scene2 (built
//!   here from the docstring's formula, pinned to 72 × 72 ppi by
//!   `bitmap_methods::doc72`, so nothing resamples) with each case's
//!   options, and its bits agree with Photoshop's own 1-bit TIFF on exactly
//!   the count `decided.json` recorded for the shipped candidate.
//! * **The menu walk** (`walk2/decided.json`, the walk's record after
//!   walk1's stated fix): every op the walk left permanent refuses with the
//!   permanent sentence, every op it named phase 3 answers by phase 3's
//!   tier (the one-layer ops by the one-layer sentence, Puppet Warp by the
//!   permanent one, the built ones at the op itself, never at the gate), a
//!   Quick Selection passes the gate, and every admitted and `kept_shared`
//!   op passes the gate.
//! * **The blend probe** (the same record): each code the walk admitted
//!   passes, and the core's black fill over the walk's selection of the
//!   50%-Threshold scene2 leaves the black count Photoshop's Bitmap fill
//!   left; every other code refuses by the blend sentence.
//!   The controller's RULING-1 overrides the row rule where a leaf the walk
//!   measured maps to one op alone ([`RULED`]): each such op refuses for
//!   good, and the leaf's measurement is checked here.
//! * **Halftone Screen** (`halftone2` or `halftone1`): SKIPs, declared and
//!   unmeasured by RULING-1 (two sessions hung on it); were a smoke ever to
//!   give a clean `decided.json`, 1b's replay of HR and HL.

use std::path::{Path, PathBuf};

use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::bitmap_methods::doc72;
use crate::bitmap_mode::{bits, to_bitmap};
use crate::indexed_fixture::{free, paint};

/// take1's verdict and shipped candidate by method (`decided.json`).
const RECORDED: [(&str, &str, &str); 3] = [
    ("threshold", "CONFIRMED", "TH128"),
    ("pattern", "PASS", "PT8-MID"),
    ("diffusion", "MISS-FITTED", "LITE-SERP-FREE-128"),
];

/// RULING-1's overrides of the walk's row rule: the op, the list of `ops`
/// the row rule put it in, the leaf the ruling read and its Bitmap state.
/// Arbitrary rotation is the one op of Image ▸ Image Rotation ▸ Arbitrary,
/// which the walk measured disabled; `filter` stays refused for good though
/// its one enabled leaf, Blur ▸ Lens Blur, carried the row (the host names
/// Lens Blur's gap in its own sentence).
const RULED: [(&str, &str, &str, bool); 2] = [
    (
        "rotate_canvas",
        "admitted",
        "image > imagerotation > arbitrary",
        false,
    ),
    ("filter", "phase_3", "filter > blur > lensblur", true),
];

/// The walk's selection (`bitmap_take.WALK_SELECTION`): x, y, w, h.
const SELECTION: [usize; 4] = [40, 8, 120, 40];

/// The blend sentence (`bitmap_document::BLEND_REFUSAL`).
const BLEND_REFUSAL: &str = "The Brush, Fill, Stroke, the Paint Bucket, the Gradient and the \
                             Clone Stamp paint in Normal, Darken or Lighten mode in a Bitmap \
                             document.";

const WIDTH: usize = 256;
const HEIGHT: usize = 64;

/// The fixtures folder, or None when HOME is unset.
fn fixtures() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join("devel/doctored-fixtures/photoshop/bitmap-mode-547"))
}

/// `<folder>/decided.json`, or None (the caller SKIPs).
fn decided(folder: &Path) -> Option<Value> {
    std::fs::read_to_string(folder.join("decided.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

/// scene2: `g = (⌊9x/7⌋ + 13y) mod 256`.
fn scene2() -> Vec<u8> {
    (0..WIDTH * HEIGHT)
        .map(|i| {
            let (x, y) = (i % WIDTH, i / WIDTH);
            ((9 * x / 7 + 13 * y) % 256) as u8
        })
        .collect()
}

/// 1b's scene: rows 0–31 `g = x`; rows 32–63 127 for x < 85, 128 for
/// x < 171, else 129.
fn scene1() -> Vec<u8> {
    (0..WIDTH * HEIGHT)
        .map(|i| {
            let (x, y) = (i % WIDTH, i / WIDTH);
            if y < 32 {
                x as u8
            } else if x < 85 {
                127
            } else if x < 171 {
                128
            } else {
                129
            }
        })
        .collect()
}

/// Each case's options to the core.
fn options(case: &str) -> Value {
    match case {
        "T" => json!({"method":"threshold"}),
        "P" => json!({"method":"pattern"}),
        "D" | "D2" => json!({"method":"diffusion"}),
        "HR" => json!({"method":"halftone","frequency":12,"angle":45,"shape":"round"}),
        "HL" => json!({"method":"halftone","frequency":8,"angle":30,"shape":"line"}),
        other => panic!("no case {other}"),
    }
}

/// A classic uncompressed 1-bit one-sample TIFF in strips as bits, 1 white:
/// Photometric 0 (WhiteIsZero) makes a set bit black, 1 (BlackIsZero) white.
fn read_bitmap_tiff(path: &Path) -> (usize, usize, Vec<u8>) {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let big = match &data[..2] {
        b"II" => false,
        b"MM" => true,
        other => panic!("{}: byte order {other:?}", path.display()),
    };
    let u16_at = |at: usize| {
        let b = [data[at], data[at + 1]];
        if big {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        }
    };
    let u32_at = |at: usize| {
        let b = [data[at], data[at + 1], data[at + 2], data[at + 3]];
        if big {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        }
    };
    assert_eq!(u16_at(2), 42, "{}: not a classic TIFF", path.display());
    let ifd = u32_at(4) as usize;
    let mut tags = std::collections::HashMap::new();
    for i in 0..usize::from(u16_at(ifd)) {
        let at = ifd + 2 + 12 * i;
        let (tag, kind, count) = (u16_at(at), u16_at(at + 2), u32_at(at + 4) as usize);
        let size = match kind {
            3 => 2,
            4 => 4,
            _ => 1,
        };
        let base = if size * count <= 4 {
            at + 8
        } else {
            u32_at(at + 8) as usize
        };
        let values: Vec<u32> = (0..count)
            .map(|k| match kind {
                3 => u32::from(u16_at(base + 2 * k)),
                4 => u32_at(base + 4 * k),
                _ => u32::from(data[base + k]),
            })
            .collect();
        tags.insert(tag, values);
    }
    let one = |tag: u16, default: u32| tags.get(&tag).map_or(default, |v| v[0]);
    let (width, height) = (one(256, 0) as usize, one(257, 0) as usize);
    assert_eq!(
        (one(258, 1), one(259, 1), one(277, 1)),
        (1, 1, 1),
        "{}: not an uncompressed 1-bit one-sample TIFF",
        path.display()
    );
    let photometric = one(262, 99);
    assert!(
        photometric <= 1,
        "{}: photometric {photometric}",
        path.display()
    );
    let mut strips = Vec::new();
    for (&offset, &count) in tags[&273].iter().zip(&tags[&279]) {
        strips.extend_from_slice(&data[offset as usize..(offset + count) as usize]);
    }
    let stride = width.div_ceil(8);
    let bits = (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let set = strips[y * stride + x / 8] & (0x80 >> (x % 8)) != 0;
            u8::from(set == (photometric == 1))
        })
        .collect();
    (width, height, bits)
}

/// Replays `cases` of `record` (a method's record) on `gray`: the core's
/// bits agree with each case's Photoshop TIFF in `folder` on the count the
/// record holds for its shipped candidate. Returns the cases replayed.
fn replay(gray: *const PrecisionDocument, folder: &Path, method: &str, record: &Value) -> usize {
    let ship = record["ship"].as_str().unwrap();
    let counts = record["candidates"][ship]
        .as_object()
        .unwrap_or_else(|| panic!("{method}: no counts for {ship}"));
    for (case, count) in counts {
        let (w, h, photoshop) = read_bitmap_tiff(&folder.join(format!("{case}.tif")));
        assert_eq!((w, h), (WIDTH, HEIGHT), "{case}");
        let bitmap = to_bitmap(gray, &options(case)).unwrap();
        let ours = bits(bitmap);
        free(bitmap);
        let agree = ours.iter().zip(&photoshop).filter(|(a, b)| a == b).count();
        assert_eq!(agree as u64, count.as_u64().unwrap(), "{method} {case}");
    }
    counts.len()
}

#[test]
fn photoshops_bitmap_take_is_the_shipped_rule() {
    let Some(root) = fixtures().map(|f| f.join("take1")) else {
        eprintln!("SKIP: HOME is unset, so the Photoshop take cannot be found");
        return;
    };
    let Some(decided) = decided(&root) else {
        eprintln!("SKIP: {} has no decided.json", root.display());
        return;
    };
    assert!(decided["stop"].is_null(), "{}", decided["stop"]);
    assert_eq!(decided["valid"], json!(true), "{}", decided["invalid"]);
    assert_eq!(decided["diffusion"]["twin_equal"], json!(true));
    let folder = PathBuf::from(decided["job_folder"].as_str().unwrap());
    // Pinned to 72 × 72 ppi, so no Output resolution resamples the replay.
    let gray = doc72(WIDTH as u32, HEIGHT as u32, &scene2());
    let mut replayed = 0;
    for (method, verdict, ship) in RECORDED {
        let record = &decided[method];
        assert_eq!(
            (record["verdict"].as_str(), record["ship"].as_str()),
            (Some(verdict), Some(ship)),
            "{method}: {record}"
        );
        replayed += replay(gray, &folder, method, record);
    }
    assert_eq!(replayed, 4, "T, P, D and D2 replay");
    free(gray);
}

/// The menu walk's record (`walk2`), or None (the caller SKIPs).
fn walk() -> Option<Value> {
    let root = fixtures()?.join("walk2");
    let walk = decided(&root);
    if walk.is_none() {
        eprintln!("SKIP: {} has no decided.json", root.display());
    }
    walk
}

/// A 50%-Threshold Bitmap document of scene2 at 72 ppi.
fn thresholded_scene2() -> *mut PrecisionDocument {
    let gray = doc72(WIDTH as u32, HEIGHT as u32, &scene2());
    let bitmap = to_bitmap(gray, &json!({"method":"threshold"})).unwrap();
    free(gray);
    bitmap
}

#[test]
fn the_menu_walk_is_the_gate() {
    let Some(walk) = walk() else { return };
    assert!(walk["stop"].is_null(), "{}", walk["stop"]);
    assert_eq!(walk["menu"]["valid"], json!(true), "{}", walk["menu"]);
    let ops = &walk["build"]["ops"];
    let doc = thresholded_scene2();
    let refusal = |op: &str| match paint(doc, &json!({"op": op, "layer": 0}), &[]) {
        Ok(out) => {
            out.into_iter().for_each(free);
            None
        }
        Err(error) => Some(error),
    };
    let permanent = |op: &str| format!("{op} is not available in Bitmap mode.");
    let one_layer =
        |op: &str| format!("{op} needs more than one layer, and a Bitmap document holds one.");
    let names = |key: &str| -> Vec<String> {
        ops[key]
            .as_array()
            .unwrap_or_else(|| panic!("ops.{key}"))
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    };
    let leaves: Vec<&Value> = walk["build"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|row| row["paths"].as_array().unwrap())
        .collect();
    for (op, list, leaf, enabled) in RULED {
        assert!(names(list).iter().any(|o| o == op), "{op} in ops.{list}");
        let measured = leaves
            .iter()
            .find(|p| p["path"] == json!(leaf))
            .unwrap_or_else(|| panic!("{leaf}"));
        assert_eq!(
            (measured["gray"].clone(), measured["bitmap"].clone()),
            (json!(true), json!(enabled)),
            "{leaf}"
        );
        assert_eq!(refusal(op), Some(permanent(op)), "{op} (RULING-1)");
    }
    let ruled = |op: &String| RULED.iter().any(|(r, ..)| r == op);
    for op in names("disabled") {
        assert_eq!(refusal(&op), Some(permanent(&op)), "{op}");
    }
    // Phase 3 classified each op the walk left to it by its new tier.
    let built = [
        "rename_layer",
        "layer_lock",
        "perspective_crop",
        "selection",
        "liquify",
    ];
    for op in names("phase_3").into_iter().filter(|op| !ruled(op)) {
        match op.as_str() {
            "align_set" | "distribute_set" | "flatten" | "remove_layers" => {
                assert_eq!(refusal(&op), Some(one_layer(&op)), "{op}");
            }
            "puppet_selected" => assert_eq!(refusal(&op), Some(permanent(&op)), "{op}"),
            op if built.contains(&op) => {
                // A bare request may fail in the op itself, never at the gate.
                if let Some(error) = refusal(op) {
                    assert!(
                        !error.contains("Bitmap") && !error.contains("phase"),
                        "{op}: {error}"
                    );
                }
            }
            op => panic!("{op}: the walk's phase_3 list has an op phase 3 did not classify"),
        }
    }
    // A Quick Selection passes the gate (the plan answers; the op's output
    // is the typed selection API's).
    let quick = json!({"op":"selection","operation":{"kind":"quick","params":{"seeds":[[0,0]]}}});
    assert!(
        crate::indexed_fixture::edit_plan(doc, &quick).is_ok(),
        "a Quick Selection passes the gate"
    );
    let mut passing: Vec<String> = names("admitted")
        .into_iter()
        .filter(|op| !ruled(op))
        .collect();
    passing.extend(names("kept_shared"));
    assert!(passing.iter().any(|op| op == "affine"), "Move keeps affine");
    for op in passing {
        // A bare request may fail in the op itself, never at the gate.
        if let Some(error) = refusal(&op) {
            assert!(
                error != permanent(&op)
                    && error != one_layer(&op)
                    && !error.contains("Bitmap")
                    && error != BLEND_REFUSAL,
                "{op}: {error}"
            );
        }
    }
    assert_eq!(walk["wand"]["verdict"], json!("CONFIRMED"));
    free(doc);
}

#[test]
fn the_blend_probe_is_the_gate() {
    let Some(walk) = walk() else { return };
    let blend = &walk["blend"];
    assert_eq!(blend["valid"], json!(true), "{blend}");
    let accepted: Vec<i64> = blend["accepted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    assert_eq!(accepted, vec![0, 6, 7], "BLEND_MODES");
    let doc = thresholded_scene2();
    let before = bits(doc);
    let [x0, y0, w, h] = SELECTION;
    let coverage: Vec<u16> = (0..WIDTH * HEIGHT)
        .map(|i| {
            let (x, y) = (i % WIDTH, i / WIDTH);
            if (x0..x0 + w).contains(&x) && (y0..y0 + h).contains(&y) {
                65535
            } else {
                0
            }
        })
        .collect();
    for code in 0..27i64 {
        let request = json!({"op":"paint_blend","layer":0,"color":[0.0,0.0,0.0,1.0],
                             "color_space":"document","opacity":1.0,"blend_mode":code});
        let result = paint(doc, &request, &coverage);
        if !accepted.contains(&code) {
            assert_eq!(result.err().as_deref(), Some(BLEND_REFUSAL), "{code}");
            continue;
        }
        let out = result.unwrap_or_else(|e| panic!("{code}: {e}"));
        let black = match out {
            Some(out) => {
                let black = bits(out).iter().filter(|&&b| b == 0).count();
                free(out);
                black
            }
            None => before.iter().filter(|&&b| b == 0).count(),
        };
        let recorded = &blend["counts"][code.to_string()]["bitmap"];
        assert_eq!(Some(black as u64), recorded.as_u64(), "{code}");
    }
    free(doc);
}

#[test]
fn photoshops_halftone_screen_is_the_shipped_rule() {
    let Some(root) = fixtures() else {
        eprintln!("SKIP: HOME is unset, so the halftone smokes cannot be found");
        return;
    };
    let clean = ["halftone2", "halftone1"].into_iter().find_map(|name| {
        decided(&root.join(name)).filter(|d| d["stop"].is_null() && d["valid"] == json!(true))
    });
    let Some(decided) = clean else {
        eprintln!(
            "SKIP: Halftone Screen is Doctored's declared spot rule, unmeasured (RULING-1): \
             two Photoshop sessions (1b's smoke1, 2b's halftone1) hung on its conversion \
             (-1712) with different descriptors, so no smoke under {} gave a clean decided.json",
            root.display()
        );
        return;
    };
    let record = &decided["halftone"];
    assert_eq!(record["ship"], json!("AREA-CCW-CORNER"), "{record}");
    let folder = PathBuf::from(decided["job_folder"].as_str().unwrap());
    let gray = doc72(WIDTH as u32, HEIGHT as u32, &scene1());
    assert_eq!(
        replay(gray, &folder, "halftone", record),
        2,
        "HR and HL replay"
    );
    free(gray);
}
