//! Copy of a GROUP at 16 bits and 32-bit float copies the group's RAW
//! projection — its children composited among themselves, without the
//! group's own opacity, blend mode, mask, style or Blend If, visibility
//! forced on — as the byte Copy of a group does. Cut of a group refuses.
//!
//! The oracles are independent of the read under test: a group's Copy is
//! compared with Copy Merged of a SECOND document that holds only the
//! composite — the same children in an isolated Normal group at opacity 1,
//! visible, with nothing under it.
//!
//! Tolerances, fixed before anything was measured: within 1 code at u16 and
//! 1e-5 relative at float, as in `precision_clipboard.rs`'s header.

use std::sync::Arc;

use doctored_core::doc::BlendMode;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_capability::require;
use doctored_core::precision_clipboard::{host_stage_bytes, ClipboardPayload, ClipboardRead};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::tiff_float::FloatTiff;
use serde_json::json;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 7;
const H: u32 = 6;
const COVERAGE: [u16; 5] = [0, 1, 32768, 65534, 65535];

// ------------------------------------------------------------- fixtures --

fn u16_image(w: u32, h: u32, seed: u32, profile: &Arc<IccProfile>) -> PrecisionImage {
    PrecisionImage::from_rgba16(w, h, pattern(w, h, seed), profile.clone(), None).expect("image")
}

/// A deterministic u16 pattern with sub-byte codes and alpha-0 latent RGB.
fn pattern(w: u32, h: u32, seed: u32) -> Vec<[u16; 4]> {
    let codes = [0x0101u16, 0x0100, 1, 65534, 0, 65535, 12345, 40000];
    (0..w * h)
        .map(|i| {
            let k = (i + seed) as usize;
            [
                codes[k % 8],
                codes[(k + 3) % 8],
                codes[(k + 5) % 8],
                codes[(k * 7 + seed as usize) % 8],
            ]
        })
        .collect()
}

fn coverage_plane(w: u32, h: u32) -> Vec<u16> {
    (0..w * h).map(|i| COVERAGE[i as usize % 5]).collect()
}

/// The group's three children, bottom first: a base raster, a raster
/// clipped to it at a non-zero offset, and a Multiply raster at 0.7.
fn children(profile: &Arc<IccProfile>) -> Vec<Layer> {
    let base = Layer::raster("Base", u16_image(6, 5, 1, profile), (0, 0), 1.0, true).unwrap();
    let clipped = Layer::raster("Clipped", u16_image(4, 4, 2, profile), (2, 1), 1.0, true)
        .unwrap()
        .with_clipped(true);
    let multiply = Layer::raster("Multiply", u16_image(W, H, 3, profile), (0, 0), 0.7, true)
        .unwrap()
        .with_blend_mode(BlendMode::Multiply)
        .unwrap();
    vec![base, clipped, multiply]
}

fn background(profile: &Arc<IccProfile>) -> Layer {
    Layer::raster("Background", u16_image(W, H, 4, profile), (0, 0), 1.0, true).unwrap()
}

fn document(layers: Vec<Layer>, profile: Arc<IccProfile>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        W,
        H,
        profile,
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .expect("document")
}

/// The oracle: the same children in an isolated Normal group at opacity 1,
/// visible, with nothing under it.
fn composite_only(profile: &Arc<IccProfile>) -> PrecisionDocument {
    let group = Layer::isolated_normal("Composite", children(profile), 1.0, true).unwrap();
    document(vec![group], profile.clone())
}

fn copy(layer: usize, x: u32, y: u32, width: u32, height: u32) -> ClipboardRead {
    serde_json::from_value(
        json!({"op":"copy","layer":layer,"x":x,"y":y,"width":width,"height":height}),
    )
    .unwrap()
}

fn copy_merged(x: u32, y: u32, width: u32, height: u32) -> ClipboardRead {
    serde_json::from_value(json!({"op":"copy_merged","x":x,"y":y,"width":width,"height":height}))
        .unwrap()
}

/// The window every u16 comparison reads: offset, inside the canvas.
const WINDOW: (u32, u32, u32, u32) = (1, 2, 5, 3);

fn group_copy(doc: &PrecisionDocument, layer: usize, coverage: &[u16]) -> ClipboardPayload {
    let (x, y, w, h) = WINDOW;
    doc.clipboard_copy(&copy(layer, x, y, w, h), Some(coverage), LIMIT, 0, None)
        .expect("a group copies its raw projection")
}

fn merged_copy(doc: &PrecisionDocument, coverage: &[u16]) -> ClipboardPayload {
    let (x, y, w, h) = WINDOW;
    doc.clipboard_copy(&copy_merged(x, y, w, h), Some(coverage), LIMIT, 0, None)
        .expect("copy merged")
}

fn assert_within_one_code(got: &ClipboardPayload, want: &ClipboardPayload) {
    assert_eq!(got.storage(), PrecisionStorage::Rgba16Unorm);
    assert_eq!(got.offset(), want.offset());
    assert_eq!(got.dimensions(), want.dimensions());
    assert_eq!(got.coverage(), want.coverage());
    let got = got.image().to_rgba16().unwrap();
    let want = want.image().to_rgba16().unwrap();
    assert!(want.iter().any(|p| p[3] > 0), "the oracle is not empty");
    for (i, (g, w)) in got.iter().zip(&want).enumerate() {
        for c in 0..4 {
            assert!(
                (i32::from(g[c]) - i32::from(w[c])).abs() <= 1,
                "pixel {i} ch{c}: {} vs {}",
                g[c],
                w[c]
            );
        }
    }
}

fn float_document(width: u32, height: u32, samples: &[[f32; 4]]) -> PrecisionDocument {
    let tiff = FloatTiff {
        width,
        height,
        channels: 4,
        alpha: Some(false),
        samples: samples.iter().flatten().copied().collect(),
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let bytes = doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("float TIFF");
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("layer.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    doc
}

/// Opaque extended-range samples: negatives and above-white RGB.
fn opaque(width: u32, height: u32) -> Vec<[f32; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let v = ((x * 37 + y * 101) % 1000) as f32 / 1000.0;
            [v * 6.0 - 1.0, 1.0 - v, ((x ^ y) % 97) as f32 / 24.0, 1.0]
        })
        .collect()
}

/// A float document: Background, then its duplicate grouped alone; the
/// group is index 2.
fn float_grouped() -> PrecisionDocument {
    let doc = float_document(W, H, &opaque(W, H));
    let doubled = doc.duplicate_layer(0).unwrap().expect("duplicated");
    let (grouped, _) = doubled
        .group_layers(&[1], "Group", false)
        .unwrap()
        .expect("grouped");
    assert!(grouped.layer(2).and_then(|l| l.children()).is_some());
    grouped
}

// ---------------------------------------------------------------- tests --

#[test]
fn a_u16_group_copies_its_raw_projection_not_its_own_properties() {
    let srgb = IccProfile::srgb();
    // The group is at 0.5, Screen and HIDDEN over a background: none of that
    // reaches the copy.
    let group = Layer::isolated_normal("G", children(&srgb), 0.5, false)
        .unwrap()
        .with_blend_mode(BlendMode::Screen)
        .unwrap();
    let doc = document(vec![background(&srgb), group], srgb.clone());
    assert!(doc.layer(4).and_then(|l| l.children()).is_some());
    let coverage = coverage_plane(W, H);
    let got = group_copy(&doc, 4, &coverage);
    let want = merged_copy(&composite_only(&srgb), &coverage);
    assert_within_one_code(&got, &want);
    assert_eq!(got.offset(), (WINDOW.0 as i32, WINDOW.1 as i32));
}

#[test]
fn a_pass_through_group_copies_as_the_isolated_one() {
    let srgb = IccProfile::srgb();
    let isolated = Layer::isolated_normal("G", children(&srgb), 0.5, false)
        .unwrap()
        .with_blend_mode(BlendMode::Screen)
        .unwrap();
    let pass = Layer::pass_through("P", children(&srgb), 0.5, false).unwrap();
    let coverage = coverage_plane(W, H);
    let isolated = group_copy(
        &document(vec![background(&srgb), isolated], srgb.clone()),
        4,
        &coverage,
    );
    let pass = group_copy(
        &document(vec![background(&srgb), pass], srgb.clone()),
        4,
        &coverage,
    );
    assert_within_one_code(&pass, &isolated);
    assert_within_one_code(&pass, &merged_copy(&composite_only(&srgb), &coverage));
}

#[test]
fn a_nested_group_copies_only_its_own_children() {
    let srgb = IccProfile::srgb();
    let inner = Layer::isolated_normal("Inner", children(&srgb), 0.5, true)
        .unwrap()
        .with_blend_mode(BlendMode::Screen)
        .unwrap();
    let sibling = Layer::raster("Sibling", u16_image(W, H, 5, &srgb), (0, 0), 1.0, true).unwrap();
    let outer = Layer::isolated_normal("Outer", vec![sibling, inner], 0.6, true)
        .unwrap()
        .with_blend_mode(BlendMode::Multiply)
        .unwrap();
    let doc = document(vec![background(&srgb), outer], srgb.clone());
    // Background 0, Sibling 1, Base 2, Clipped 3, Multiply 4, Inner 5, Outer 6.
    assert_eq!(doc.layer(5).unwrap().name(), "Inner");
    let coverage = coverage_plane(W, H);
    let got = group_copy(&doc, 5, &coverage);
    assert_within_one_code(&got, &merged_copy(&composite_only(&srgb), &coverage));
}

#[test]
fn a_float_group_copies_its_raw_projection_at_float() {
    let grouped = float_grouped();
    let doc = grouped.set_layer_opacity(2, 0.5).unwrap().expect("opacity");
    let oracle = grouped.remove_layers(&[0]).unwrap().expect("group only");
    let coverage = coverage_plane(W, H);
    let (x, y, w, h) = WINDOW;
    let got = doc
        .clipboard_copy(&copy(2, x, y, w, h), Some(&coverage), LIMIT, 0, None)
        .expect("a float group copies");
    let want = oracle
        .clipboard_copy(&copy_merged(x, y, w, h), Some(&coverage), LIMIT, 0, None)
        .expect("copy merged");
    assert_eq!(got.storage(), PrecisionStorage::Rgba32Float);
    assert_eq!(got.offset(), want.offset());
    assert_eq!(got.coverage(), want.coverage());
    let got = got.image().to_rgba32().unwrap();
    let want = want.image().to_rgba32().unwrap();
    assert!(want.iter().any(|p| p[3] > 0.0));
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-5 * b.abs().max(1.0);
    for (i, (g, w)) in got.iter().zip(&want).enumerate() {
        for c in 0..4 {
            assert!(
                close(f64::from(g[c]), f64::from(w[c])),
                "pixel {i} ch{c}: {} vs {}",
                g[c],
                w[c]
            );
        }
    }
}

#[test]
fn cut_of_a_group_refuses_at_every_depth() {
    let srgb = IccProfile::srgb();
    let group = Layer::isolated_normal("G", children(&srgb), 1.0, true).unwrap();
    let doc = document(vec![background(&srgb), group], srgb);
    let err = doc
        .clipboard_cut(4, 0, 0, W, H, None, LIMIT, 0, None)
        .err()
        .unwrap();
    assert_eq!(err, "Cut requires a raster layer; layer 4 is a group");

    let doc = float_grouped();
    let err = doc
        .clipboard_cut(2, 0, 0, W, H, None, LIMIT, 0, None)
        .err()
        .unwrap();
    assert_eq!(err, "Cut requires a raster layer; layer 2 is a group");
}

#[test]
fn the_capability_tables_admit_a_group_copy_and_refuse_a_group_cut() {
    for depth in [16, 32] {
        require("clipboard_copy", "group", depth).expect("group copy admitted");
        require("clipboard_copy", "raster", depth).expect("raster copy admitted");
        assert!(require("clipboard_cut", "group", depth).is_err());
    }
}

#[test]
fn a_group_copy_is_admitted_at_its_plan() {
    // 400×400 u16: 2·payload is above the bookkeeping.
    let (w, h) = (400u32, 400u32);
    let srgb = IccProfile::srgb();
    let image = |seed| u16_image(w, h, seed, &srgb);
    let base = Layer::raster("Base", image(1), (0, 0), 1.0, true).unwrap();
    let clipped = Layer::raster("Clipped", image(2), (3, 2), 1.0, true)
        .unwrap()
        .with_clipped(true);
    let multiply = Layer::raster("Multiply", image(3), (0, 0), 0.7, true)
        .unwrap()
        .with_blend_mode(BlendMode::Multiply)
        .unwrap();
    let group = Layer::isolated_normal("G", vec![base, clipped, multiply], 0.5, true)
        .unwrap()
        .with_blend_mode(BlendMode::Screen)
        .unwrap();
    let background = Layer::raster("Background", image(4), (0, 0), 1.0, true).unwrap();
    let doc = PrecisionDocument::from_parts(
        w,
        h,
        srgb.clone(),
        Metadata::default(),
        Resolution::default(),
        vec![background, group],
    )
    .unwrap();
    let plan = doc.group_copy_plan(4, w, h, false).unwrap();
    let allocation = doc.group_copy_allocation(4, w, h, false).unwrap();
    assert!(allocation <= plan);
    let read = copy(4, 0, 0, w, h);
    let err = doc
        .clipboard_copy(&read, None, plan - 1, 0, None)
        .err()
        .unwrap();
    assert!(err.contains("working limit"), "{err}");
    doc.clipboard_copy(&read, None, plan, 0, None)
        .expect("a group copy at its plan");
}

#[test]
fn a_group_copy_admission_includes_the_margin() {
    // 400×400 u16 canvas, but the group's only child is 2×2: its render plan
    // is small, so the staged term (render + 2·payload + margin) is the
    // largest admission and the margin decides the threshold.
    let (w, h) = (400u32, 400u32);
    let srgb = IccProfile::srgb();
    let child = Layer::raster("Dot", u16_image(2, 2, 1, &srgb), (5, 7), 1.0, true).unwrap();
    let group = Layer::isolated_normal("G", vec![child], 0.5, true)
        .unwrap()
        .with_blend_mode(BlendMode::Screen)
        .unwrap();
    let background =
        Layer::raster("Background", u16_image(w, h, 4, &srgb), (0, 0), 1.0, true).unwrap();
    let doc = PrecisionDocument::from_parts(
        w,
        h,
        srgb.clone(),
        Metadata::default(),
        Resolution::default(),
        vec![background, group],
    )
    .unwrap();
    assert!(doc.layer(2).and_then(|l| l.children()).is_some());
    let canvas = u64::from(w) * u64::from(h);
    // A whole-canvas window's host stage (three payloads plus the
    // public rendering) outweighs the staged term, so the plan is the max of
    // the two; a 20×20 window keeps the staged term — and so the margin —
    // deciding the threshold.
    let whole = canvas * 8 + 2 * canvas * 8 + 48 * canvas;
    let host = host_stage_bytes(w, h, PrecisionStorage::Rgba16Unorm, false).unwrap();
    assert_eq!(
        doc.group_copy_plan(2, w, h, false).unwrap(),
        whole.max(host)
    );
    let (ww, wh) = (20u32, 20u32);
    let payload = u64::from(ww * wh) * 8;
    let staged = canvas * 8 + 2 * payload + 48 * canvas;
    assert!(staged > host_stage_bytes(ww, wh, PrecisionStorage::Rgba16Unorm, false).unwrap());
    let plan = doc.group_copy_plan(2, ww, wh, false).unwrap();
    assert_eq!(
        plan, staged,
        "the staged term, margin included, is the plan"
    );
    assert_eq!(doc.group_copy_allocation(2, ww, wh, false).unwrap(), staged);
    let read = copy(2, 0, 0, ww, wh);
    let err = doc
        .clipboard_copy(&read, None, plan - 1, 0, None)
        .err()
        .unwrap();
    assert!(err.contains("working limit"), "{err}");
    doc.clipboard_copy(&read, None, plan, 0, None)
        .expect("a group copy at its plan");
}
