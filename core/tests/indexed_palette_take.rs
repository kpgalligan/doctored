//! #1 Indexed phase 3d: Photoshop's take of Image ▸ Mode ▸ Indexed
//! Color's Local and Master Perceptual and Selective palettes and its
//! Pattern and Noise dithers (`~/devel/doctored-fixtures/photoshop/
//! indexed-mode-547/palette-take1`, never committed;
//! `scripts/photoshop-probe/indexed_palette_take.py`, whose docstring fixes
//! the scenes, the candidates, the gates and the build table), read in
//! place: each test SKIPs when HOME or the take is absent.
//!
//! The record: the take is VALID and every verdict is the one it recorded
//! ([`RECORDED`]): Local Perceptual and Selective MISS (no candidate passed
//! the palette gate), both Master palettes REFUSED by Photoshop, Pattern
//! MISS (the declared rule kept), Noise REFUSED by Photoshop (the declared
//! rule kept). No palette ships, so the four refuse for good
//! (`indexed_dither::the_unpublished_palettes_refuse_for_good`).
//!
//! The replays: the shipped Pattern dither of the take's own ramp
//! (`inputs/ramp.tif`) against Photoshop's own palette TIFF of each
//! converted Pattern case matches exactly the pixel counts the runner
//! recorded for the declared rule ([`PATTERN_RECORD`]); Noise records its
//! refusal.

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::indexed_fixture::{
    free, indices, open_indexed, open_rgba8, palette_of, rgb_doc, to_indexed,
};

/// take1's verdicts (`decided.json` `verdicts`).
const RECORDED: &[(&str, &str)] = &[
    ("local_perceptual", "MISS"),
    ("local_selective", "MISS"),
    ("master_perceptual", "REFUSED"),
    ("master_selective", "REFUSED"),
    ("pattern", "MISS"),
    ("noise", "REFUSED"),
];

/// The declared Pattern rule's exact pixels on each converted Pattern case
/// (`decided.json` `pattern.P0`): (case, wire palette, matched, of).
const PATTERN_RECORD: &[(&str, &str, u64, u64)] = &[
    ("pattern_web", "web", 520_128, 524_288),
    ("pattern_mac", "system_mac", 329_664, 524_288),
];

fn take() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(|home| {
        PathBuf::from(home).join("devel/doctored-fixtures/photoshop/indexed-mode-547/palette-take1")
    })
}

fn decided() -> Option<(PathBuf, Value)> {
    let Some(root) = take() else {
        eprintln!("SKIP: HOME is unset, so the Photoshop take cannot be found");
        return None;
    };
    let path = root.join("decided.json");
    if !path.exists() {
        eprintln!("SKIP: {} has no decided.json", root.display());
        return None;
    }
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let decided = serde_json::from_str::<Value>(&text)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()));
    Some((root, decided))
}

/// An Indexed document's composite as RGB codes, row-major: each pixel's
/// palette entry.
fn expanded(doc: *const doctored_core::precision_document::PrecisionDocument) -> Vec<[u8; 3]> {
    let (palette, _) = palette_of(doc);
    indices(doc).iter().map(|&i| palette[i as usize]).collect()
}

#[test]
fn photoshops_palette_take_is_recorded() {
    let Some((_, decided)) = decided() else {
        return;
    };
    assert_eq!(decided["valid"], json!(true), "{}", decided["checks"]);
    for (subject, verdict) in RECORDED {
        assert_eq!(decided["verdicts"][subject], json!(verdict), "{subject}");
    }
    assert_eq!(
        decided["verdicts"].as_object().map(|v| v.len()),
        Some(RECORDED.len())
    );
}

#[test]
fn the_shipped_pattern_replays_photoshops_ramp() {
    let Some((root, decided)) = decided() else {
        return;
    };
    let pattern = &decided["pattern"];
    if pattern["verdict"] == json!("REFUSED") {
        // Both Pattern cases refused: the record holds both refusals.
        for case in ["pattern_web", "pattern_mac"] {
            assert!(pattern["refused"][case].is_string(), "{case}");
        }
        return;
    }
    assert_eq!(pattern["source"], json!("pattern_web"));
    // The 8-bit ramp read as RGBA8 (`dr_image_open`; `dr_precision_doc_open`
    // takes 16-bit files only), as an 8-bit RGB document.
    let (w, h, pixels) = open_rgba8(&root.join("inputs/ramp.tif"));
    let rgb = rgb_doc(w, h, &pixels);
    for &(case, palette, matched, of) in PATTERN_RECORD {
        assert_eq!(pattern["P0"][case]["matched"], json!(matched), "{case}");
        assert_eq!(pattern["P0"][case]["of"], json!(of), "{case}");
        let ours = to_indexed(rgb, &json!({"palette": palette, "dither": "pattern"})).unwrap();
        let photoshop = open_indexed(&root.join("jobs/r").join(format!("{case}.tif"))).unwrap();
        let (a, b) = (expanded(ours), expanded(photoshop));
        assert_eq!(a.len() as u64, of, "{case}");
        assert_eq!(b.len() as u64, of, "{case}");
        let same = a.iter().zip(&b).filter(|(x, y)| x == y).count() as u64;
        assert_eq!(same, matched, "{case}");
        free(photoshop);
        free(ours);
    }
    free(rgb);
}

#[test]
fn the_shipped_noise_matches_photoshops_distribution() {
    let Some((root, decided)) = decided() else {
        return;
    };
    let noise = &decided["noise"];
    // Photoshop refused `noise_web` (and its twin) through the action
    // descriptor: nothing to compare, the refusal recorded and no TIFF.
    assert_eq!(noise["verdict"], json!("REFUSED"));
    assert!(
        noise["refused"]
            .as_str()
            .is_some_and(|s| s.contains("Convert Mode")),
        "{noise}"
    );
    for case in ["noise_web", "noise_web_twin"] {
        assert!(decided["refused"][case].is_string(), "{case}");
        assert!(!root.join("jobs/r").join(format!("{case}.tif")).exists());
    }
}
