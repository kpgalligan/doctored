//! 13B-9b on a CMYK document's four inks: a `sample: current` Spot Heal or
//! Content-Aware Fill reads its source layer's ink IN PLACE
//! (`SynthesisPlate::Cmyk` with the layer's offset; a column off the layer
//! answers `PAPER`, the rendered Current plate's blank) and plans its hole
//! over the request's bounds. The product path (`dr_precision_doc_edit`) is
//! held to the full-frame seam (`RetouchEdit::full_frame`, set after
//! `parse`), which renders the canvas ink plate and plans over the canvas:
//! every case answers the same document to the bit
//! (`CmykImage::same_samples` on every layer, offsets and layer count equal).
//!
//! The cases: the three Types and Content-Aware Fill on a canvas layer; an
//! offset layer smaller than the canvas with the hole near its edge, so the
//! windows read gaps; `source_layer` naming that offset layer; and the
//! GAP-EDGE TWINS, a fill whose `source_mask` admits every canvas pixel with
//! a 32×32 hole beside an offset layer's left edge, the layer at x = 50 and
//! at 51 (`precision_synthesis_in_place.rs` says why). A CMYK document
//! refuses `fill_transparent`, so no case pads.

use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::cmyk_structure_fixture::{cmyk_layers, free, try_edit, RgbLayer, LIMIT};

const W: u32 = 160;
const H: u32 = 120;
const N: usize = W as usize * H as usize;
/// The smaller offset layer's rect on the canvas.
const SMALL: (i32, i32, u32, u32) = (40, 30, 70, 50);
/// The gap-edge twins' layer at `x` = 50 or 51, and their fixed hole.
const EDGE: (i32, u32, u32) = (20, 90, 80);
const EDGE_HOLE: (u32, u32, u32, u32) = (60, 44, 32, 32);

/// Samples varying in every channel, with sparse 3×3 blocks on a 40 px grid
/// of the layer's own pixels: transparent at (0, 0) and non-data (alpha
/// 30000) at (20, 20), so complete source patches remain.
fn pattern(w: u32, h: u32, seed: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let alpha = match (x % 40, y % 40) {
                (0..=2, 0..=2) => 0,
                (20..=22, 20..=22) => 30000,
                _ => 65535,
            };
            [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                (((x ^ y) * 1999 + seed) % 65536) as u16,
                alpha,
            ]
        })
        .collect()
}

fn layer(name: &'static str, (x, y, w, h): (i32, i32, u32, u32), seed: u32) -> RgbLayer {
    RgbLayer {
        name,
        w,
        h,
        offset: (x, y),
        pixels: pattern(w, h, seed),
    }
}

/// The CMYK document of `layers`, each at its offset and extent.
fn document(layers: Vec<RgbLayer>) -> *mut PrecisionDocument {
    let geometry: Vec<_> = layers.iter().map(|l| (l.offset, (l.w, l.h))).collect();
    let doc = cmyk_layers(W, H, layers);
    for (i, want) in geometry.into_iter().enumerate() {
        let layer = unsafe { &*doc }.layer(i).unwrap();
        let got = (layer.offset(), layer.cmyk_source().unwrap().dimensions());
        assert_eq!(got, want, "CMYK layer {i} keeps its offset and extent");
    }
    doc
}

/// A coverage plane: full over `rect` with a two-pixel feathered ring
/// (40000, then 16000) when `feather`; and the request's bounds holding it.
fn cover(rect: (u32, u32, u32, u32), feather: bool) -> (Vec<u16>, Value) {
    let (x, y, w, h) = rect;
    let plane = (0..N)
        .map(|i| {
            let (px, py) = ((i % W as usize) as i64, (i / W as usize) as i64);
            let dx = (i64::from(x) - px).max(px - i64::from(x + w) + 1).max(0);
            let dy = (i64::from(y) - py).max(py - i64::from(y + h) + 1).max(0);
            match (dx.max(dy), feather) {
                (0, _) => 65535,
                (1, true) => 40000,
                (2, true) => 16000,
                _ => 0,
            }
        })
        .collect();
    let grow = if feather { 2 } else { 0 };
    let (x0, y0) = (x.saturating_sub(grow), y.saturating_sub(grow));
    let (x1, y1) = ((x + w + grow).min(W), (y + h + grow).min(H));
    let bounds = json!({"x": x0, "y": y0, "width": x1 - x0, "height": y1 - y0});
    (plane, bounds)
}

/// `base` with `extra`'s fields set.
fn with(base: &Value, extra: Value) -> Value {
    let mut out = base.clone();
    for (k, v) in extra.as_object().unwrap() {
        out[k] = v.clone();
    }
    out
}

/// The full-frame seam: the canvas ink plate and the canvas-wide hole plan.
fn full_frame(
    doc: &PrecisionDocument,
    request: &Value,
    coverage: &[u16],
) -> Result<Option<PrecisionDocument>, String> {
    let mut edit = doctored_core::precision_retouch::RetouchEdit::parse(&request.to_string())?;
    edit.full_frame = true;
    doc.retouch(&edit, coverage, LIMIT, 0, None)
}

/// Where `a` and `b` differ, or `None` when their layers match to the bit.
fn difference(a: &PrecisionDocument, b: &PrecisionDocument) -> Option<String> {
    if a.layer_count() != b.layer_count() {
        return Some(format!(
            "{} layers against {}",
            a.layer_count(),
            b.layer_count()
        ));
    }
    for i in 0..a.layer_count() {
        let (la, lb) = (a.layer(i).unwrap(), b.layer(i).unwrap());
        if la.offset() != lb.offset() {
            return Some(format!(
                "layer {i} offset {:?} against {:?}",
                la.offset(),
                lb.offset()
            ));
        }
        match (la.cmyk_source(), lb.cmyk_source()) {
            (Some(x), Some(y)) if x.same_samples(y) => {}
            (None, None) => {}
            _ => return Some(format!("layer {i}'s inks differ")),
        }
    }
    None
}

/// One case: the product and the seam both change the document, alike.
fn check(
    failures: &mut Vec<String>,
    label: &str,
    doc: *mut PrecisionDocument,
    request: Value,
    coverage: &[u16],
) {
    let product = try_edit(doc, &request.to_string(), coverage);
    let oracle = full_frame(unsafe { &*doc }, &request, coverage);
    match (product, oracle) {
        (Ok(product), Ok(Some(oracle))) if !product.is_null() => {
            if let Some(why) = difference(unsafe { &*product }, &oracle) {
                failures.push(format!("{label}: {why}"));
            }
            free(product);
        }
        (product, oracle) => {
            let product = product.map(|p| {
                if !p.is_null() {
                    free(p);
                }
                !p.is_null()
            });
            failures.push(format!(
                "{label}: product {product:?}, full frame {:?}",
                oracle.map(|d| d.is_some())
            ))
        }
    }
}

#[test]
fn in_place_ink_synthesis_equals_the_full_frame_path() {
    let canvas = || layer("L", (0, 0, W, H), 1);
    let edge = |x: i32| layer("E", (x, EDGE.0, EDGE.1, EDGE.2), 4);
    let one = document(vec![canvas()]);
    let offset = document(vec![canvas(), layer("S", SMALL, 2)]);
    let twins = [
        document(vec![canvas(), edge(50)]),
        document(vec![canvas(), edge(51)]),
    ];
    let mut failures = Vec::new();
    let f = &mut failures;
    let spot = |layer: usize, bounds: &Value| {
        with(
            bounds,
            json!({"op": "spot_heal", "layer": layer, "sample": "current", "seed": 7}),
        )
    };
    let fill = |layer: usize, bounds: &Value| {
        with(
            bounds,
            json!({"op": "content_aware_fill", "layer": layer, "sample": "current", "seed": 7}),
        )
    };

    // The three Types and Content-Aware Fill on a canvas layer.
    let (hole, bounds) = cover((70, 52, 14, 12), true);
    for kind in ["content_aware", "create_texture", "proximity_match"] {
        let request = with(&spot(0, &bounds), json!({"type": kind}));
        check(f, &format!("{kind} canvas"), one, request, &hole);
    }
    check(f, "fill canvas", one, fill(0, &bounds), &hole);

    // The smaller offset layer, the hole by its left edge, ring 12.
    let (near, bounds) = cover((43, 45, 12, 12), true);
    let ring = with(&spot(1, &bounds), json!({"ring": 12}));
    check(f, "offset layer spot heal", offset, ring, &near);
    let ring = with(&fill(1, &bounds), json!({"ring": 12}));
    check(f, "offset layer fill", offset, ring, &near);

    // A canvas target sampling the offset layer across its right edge.
    let (across, bounds) = cover((100, 58, 14, 12), true);
    let sampled = with(&spot(0, &bounds), json!({"source_layer": 1}));
    check(f, "source_layer", offset, sampled, &across);

    // The gap-edge twins.
    let (edge_hole, bounds) = cover(EDGE_HOLE, false);
    let everywhere = json!({"version": 1, "initial": "all", "operations": []});
    for (doc, x) in twins.iter().zip([50, 51]) {
        let request = with(&fill(1, &bounds), json!({"source_mask": everywhere}));
        check(
            f,
            &format!("gap-edge twin at x {x}"),
            *doc,
            request,
            &edge_hole,
        );
    }

    for doc in [one, offset, twins[0], twins[1]] {
        free(doc);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
