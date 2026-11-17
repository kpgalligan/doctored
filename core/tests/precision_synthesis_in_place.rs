//! 13B-9b: a typed `sample: current` Spot Heal or Content-Aware Fill reads
//! its source layer IN PLACE (`precision_inpaint::SynthesisPlate` carries the
//! image and its canvas offset; a column off the image answers the canvas
//! plate's blank) and plans its hole over the request's bounds. The product
//! path (`dr_precision_doc_edit`) is held to the full-frame seam
//! (`RetouchEdit::full_frame`, set after `parse`), which renders the canvas
//! plate and plans over the canvas: every case answers the same document to
//! the bit (`PrecisionImage::same_samples` on every layer, offsets and layer
//! count equal), at 16 bits and 32-bit float.
//!
//! The cases: the three Types and Content-Aware Fill on a canvas layer; an
//! offset layer smaller than the canvas with the hole near its edge, so the
//! windows read gaps; layers overhanging the canvas; `source_layer` naming
//! an offset layer; a fill with `source_mask` and `fill_transparent`; a
//! selection plane; new-layer output; `sample: all` as a control (it renders
//! its plate on both paths, so only the hole's domain differs). The
//! GAP-EDGE TWINS run a fill whose `source_mask` admits every canvas pixel,
//! with a 32×32 hole beside an offset layer's left edge, once with the layer
//! at x = 50 and once at 51: a gap's blank equals the synthesis' initial
//! planes, so a gap read shows only through eligibility, which the pyramid's
//! reduction ANDs into the coarse block that straddles the layer's edge —
//! a block one of the two offsets puts there.
//!
//! The float documents are the 16-bit ones promoted
//! (`PrecisionDocument::promote_to_float`), every layer at its offset and
//! extent.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_retouch::RetouchEdit;
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 160;
const H: u32 = 120;
const N: usize = W as usize * H as usize;
/// The smaller offset layer's rect on the canvas.
const SMALL: (i32, i32, u32, u32) = (40, 30, 70, 50);
/// The layers overhanging the canvas: past its top-left and bottom-right.
const OVER_A: (i32, i32, u32, u32) = (-24, -18, 100, 80);
const OVER_B: (i32, i32, u32, u32) = (100, 70, 90, 70);
/// The gap-edge twins' layer at `x` = 50 or 51, and their fixed hole.
const EDGE: (i32, u32, u32) = (20, 90, 80);
const EDGE_HOLE: (u32, u32, u32, u32) = (60, 44, 32, 32);

// ------------------------------------------------------------- fixtures --

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

fn layer(name: &str, (x, y, w, h): (i32, i32, u32, u32), seed: u32) -> Layer {
    let image =
        PrecisionImage::from_rgba16(w, h, pattern(w, h, seed), IccProfile::srgb(), None).unwrap();
    Layer::raster(name, image, (x, y), 1.0, true).unwrap()
}

fn canvas_layer(seed: u32) -> Layer {
    layer("L", (0, 0, W, H), seed)
}

fn document(layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

/// The documents the cases run on, at one depth.
struct Docs {
    /// One canvas layer.
    canvas: PrecisionDocument,
    /// A canvas layer under the `SMALL` layer.
    offset: PrecisionDocument,
    /// A canvas layer under the two overhanging layers.
    overhang: PrecisionDocument,
    /// A canvas layer under the twins' layer at x = 50, then at 51.
    twins: [PrecisionDocument; 2],
}

fn docs16() -> Docs {
    let edge = |x: i32| layer("E", (x, EDGE.0, EDGE.1, EDGE.2), 4);
    Docs {
        canvas: document(vec![canvas_layer(1)]),
        offset: document(vec![canvas_layer(1), layer("S", SMALL, 2)]),
        overhang: document(vec![
            canvas_layer(1),
            layer("A", OVER_A, 3),
            layer("B", OVER_B, 5),
        ]),
        twins: [
            document(vec![canvas_layer(1), edge(50)]),
            document(vec![canvas_layer(1), edge(51)]),
        ],
    }
}

/// `doc` promoted to 32-bit float, every layer at its offset and extent.
fn promoted(doc: &PrecisionDocument) -> PrecisionDocument {
    let out = doc.promote_to_float(LIMIT, 0, None).unwrap();
    assert!(out.is_float());
    for i in 0..doc.layer_count() {
        let (a, b) = (doc.layer(i).unwrap(), out.layer(i).unwrap());
        assert_eq!(
            (a.offset(), a.source().unwrap().dimensions()),
            (b.offset(), b.source().unwrap().dimensions()),
            "layer {i} keeps its offset and extent"
        );
    }
    out
}

fn docs32() -> Docs {
    let docs = docs16();
    Docs {
        canvas: promoted(&docs.canvas),
        offset: promoted(&docs.offset),
        overhang: promoted(&docs.overhang),
        twins: [promoted(&docs.twins[0]), promoted(&docs.twins[1])],
    }
}

/// A coverage plane: full over `rect`, with a two-pixel feathered ring
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

/// `{"version":1,"initial":"all"}` with `operations`.
fn source_mask(operations: Value) -> Value {
    json!({"version": 1, "initial": "all", "operations": operations})
}

// ----------------------------------------------------------------- paths --

fn execute(
    doc: &PrecisionDocument,
    request: &Value,
    coverage: &[u16],
) -> Result<Option<Box<PrecisionDocument>>, String> {
    let text = CString::new(request.to_string()).unwrap();
    unsafe {
        let mut err = ptr::null_mut();
        let out = dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        );
        if !err.is_null() {
            let message = CStr::from_ptr(err).to_string_lossy().into_owned();
            dr_string_free(err);
            return Err(message);
        }
        Ok((!out.is_null()).then(|| Box::from_raw(out)))
    }
}

/// The full-frame seam: the canvas plate and the canvas-wide hole plan.
fn full_frame(
    doc: &PrecisionDocument,
    request: &Value,
    coverage: &[u16],
) -> Result<Option<PrecisionDocument>, String> {
    let mut edit = RetouchEdit::parse(&request.to_string())?;
    edit.full_frame = true;
    doc.retouch(&edit, coverage, LIMIT, 0, None)
}

/// Where `a` and `b` differ, or `None` when they are the same document's
/// layers to the bit.
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
        match (la.source(), lb.source()) {
            (Some(x), Some(y)) if x.same_samples(y) => {}
            (None, None) => {}
            _ => return Some(format!("layer {i}'s samples differ")),
        }
    }
    None
}

/// One case: the product and the seam both change the document, alike.
fn check(
    failures: &mut Vec<String>,
    label: &str,
    doc: &PrecisionDocument,
    request: Value,
    coverage: &[u16],
) {
    let product = execute(doc, &request, coverage);
    let oracle = full_frame(doc, &request, coverage);
    match (product, oracle) {
        (Ok(Some(product)), Ok(Some(oracle))) => {
            if let Some(why) = difference(&product, &oracle) {
                failures.push(format!("{label}: {why}"));
            }
        }
        (product, oracle) => failures.push(format!(
            "{label}: product {:?}, full frame {:?}",
            product.map(|d| d.is_some()),
            oracle.map(|d| d.is_some())
        )),
    }
}

fn cases(depth: &str, docs: &Docs) -> Vec<String> {
    let mut failures = Vec::new();
    let f = &mut failures;
    let label = |case: &str| format!("{depth} {case}");
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
        check(
            f,
            &label(&format!("{kind} canvas")),
            &docs.canvas,
            request,
            &hole,
        );
    }
    check(
        f,
        &label("fill canvas"),
        &docs.canvas,
        fill(0, &bounds),
        &hole,
    );

    // The smaller offset layer, the hole by its left edge, ring 12: the
    // windows read the gap left of it.
    let (near, near_bounds) = cover((43, 45, 12, 12), true);
    let ring = with(&spot(1, &near_bounds), json!({"ring": 12}));
    check(
        f,
        &label("offset layer spot heal"),
        &docs.offset,
        ring,
        &near,
    );
    let ring = with(&fill(1, &near_bounds), json!({"ring": 12}));
    check(f, &label("offset layer fill"), &docs.offset, ring, &near);

    // Layers overhanging the canvas, each healed by the canvas edge.
    let (corner, bounds) = cover((5, 6, 12, 12), true);
    check(
        f,
        &label("overhang top-left"),
        &docs.overhang,
        spot(1, &bounds),
        &corner,
    );
    let (corner, bounds) = cover((141, 102, 12, 12), true);
    check(
        f,
        &label("overhang bottom-right"),
        &docs.overhang,
        spot(2, &bounds),
        &corner,
    );

    // A canvas target sampling the offset layer across its right edge.
    let (across, bounds) = cover((100, 58, 14, 12), true);
    let sampled = with(&spot(0, &bounds), json!({"source_layer": 1}));
    check(f, &label("source_layer"), &docs.offset, sampled, &across);

    // A fill with a source mask and the transparent pad, over the offset
    // layer's transparent block at its (40, 40) and by its bottom edge.
    let (padded, bounds) = cover((76, 66, 12, 12), true);
    let mask = source_mask(json!([
        {"op": "exclude_rect", "x": 92.0, "y": 50.0, "width": 16.0, "height": 20.0}
    ]));
    let masked = with(
        &fill(1, &bounds),
        json!({"source_mask": mask, "fill_transparent": true}),
    );
    check(
        f,
        &label("fill source_mask fill_transparent"),
        &docs.offset,
        masked,
        &padded,
    );

    // The gap-edge twins.
    let (edge_hole, bounds) = cover(EDGE_HOLE, false);
    for (doc, x) in docs.twins.iter().zip([50, 51]) {
        let request = with(
            &fill(1, &bounds),
            json!({"source_mask": source_mask(json!([]))}),
        );
        check(
            f,
            &label(&format!("gap-edge twin at x {x}")),
            doc,
            request,
            &edge_hole,
        );
    }

    // A selection plane ramping across the canvas.
    let (hole, bounds) = cover((70, 52, 14, 12), true);
    let mut selected = hole.clone();
    selected.extend((0..N).map(|i| ((i % W as usize) * 65535 / (W as usize - 1)) as u16));
    let request = with(&spot(0, &bounds), json!({"selection": true}));
    check(f, &label("selection"), &docs.canvas, request, &selected);

    // New-layer output from the offset layer.
    let request = with(&spot(1, &near_bounds), json!({"output": "new_layer"}));
    check(f, &label("new layer"), &docs.offset, request, &near);

    // `sample: all`, the control: a rendered plate on both paths.
    let request = with(&spot(1, &near_bounds), json!({"sample": "all"}));
    check(f, &label("sample all"), &docs.offset, request, &near);
    failures
}

// ---------------------------------------------------------------- tests --

#[test]
fn the_16_bit_in_place_synthesis_equals_the_full_frame_path() {
    let failures = cases("16-bit", &docs16());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_float_in_place_synthesis_equals_the_full_frame_path() {
    let failures = cases("float", &docs32());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
