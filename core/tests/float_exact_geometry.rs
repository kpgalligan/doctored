//! Phase 12F-1b: the EXACT canvas geometry ops at 32-BIT FLOAT — a zero-angle
//! `crop`, the three quarter turns and the two flips.
//!
//! These ops read no weight. A crop keeps every raster and mask ALLOCATION
//! and walks each u16 channel with Nearest; a turn or a flip is a Nearest
//! PERMUTATION, `plan::affine`'s `exact_form` having pinned the filter. So an
//! extended-range component — a negative one, or one above 1.0 — is only ever
//! COPIED, bit for bit. The FILTERED geometry (Image Size, straighten,
//! arbitrary rotation, free transform) is admitted at float too;
//! its oracle lives in `core/tests/float_filtered_geometry.rs`, and T6 here
//! only pins that it is admitted and that a Nearest per-layer quarter turn
//! stays a permutation.
//!
//! The yardsticks are independent, never an answer compared with itself:
//!
//! * exactness is checked the strongest way there is — for a crop every
//!   raster/mask buffer is POINTER-equal before and after, because a sample
//!   comparison would also pass on a copy;
//! * every permutation is written out BY HAND below (`turned`), from the five
//!   matrices in `precision_geometry_plan::whole`, and compared on `to_bits`;
//! * the chrome is this file's own `chrome_after`, restating `doc_guide`'s
//!   rules rather than calling them.
//!
//! The fixture is the committed 11F scene, which holds both a NEGATIVE and an
//! ABOVE-WHITE component (asserted, not assumed). Absent, every test here
//! returns rather than failing, exactly as the other float suites do.

use doctored_core::doc_mask::MaskProperties;
use doctored_core::metadata::Metadata;
use doctored_core::precision_capability::capability;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::precision_mask_edit::MaskEdit;
use doctored_core::tiff_float::FloatTiff;
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

/// The chrome the fixture carries, so a crop has a guide to shift and a turn
/// a guide to re-orient.
const GUIDE_V: f64 = 10.0;
const GUIDE_H: f64 = 6.0;
const RULER: (f64, f64) = (5.0, 7.0);
const PPI: (f32, f32) = (300.0, 150.0);

// ---------------------------------------------------------------- helpers --

fn parse(value: Value) -> GeometryEdit {
    GeometryEdit::parse(&value.to_string()).expect("a geometry request")
}

fn try_apply(doc: &PrecisionDocument, request: Value) -> Result<Option<PrecisionDocument>, String> {
    doc.geometry(&parse(request), LIMIT, 0, None)
}

fn apply(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    try_apply(doc, request)
        .expect("the edit runs")
        .expect("the edit changes something")
}

/// Every raster record's f32 image, in postorder (compared by IDENTITY).
fn rasters(doc: &PrecisionDocument) -> Vec<PrecisionImage> {
    (0..doc.layer_count())
        .filter_map(|i| doc.layer_source(i))
        .inspect(|image| {
            assert_eq!(
                image.storage(),
                PrecisionStorage::Rgba32Float,
                "a float document's rasters are RGBA32"
            )
        })
        .cloned()
        .collect()
}

/// The same raster IMAGES, pairwise by identity (`PrecisionImage::ptr_eq`).
fn same_rasters(a: &[PrecisionImage], b: &[PrecisionImage]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| PrecisionImage::ptr_eq(x, y))
}

fn mask_addresses(doc: &PrecisionDocument) -> Vec<usize> {
    (0..doc.layer_count())
        .filter_map(|i| doc.layer(i).unwrap().mask())
        .map(|m| m.plane().samples().as_ptr() as usize)
        .collect()
}

fn layer_bits(doc: &PrecisionDocument, index: usize) -> Vec<u32> {
    doc.layer_source(index)
        .expect("a raster record")
        .to_rgba32()
        .expect("float storage")
        .iter()
        .flat_map(|pixel| pixel.iter().map(|v| v.to_bits()))
        .collect()
}

/// The canvas-sized u16 plane a mask and a channel are both built from, so
/// the permutation check has two independent scalar planes to run on.
fn plane_samples(w: u32, h: u32, seed: u32) -> Vec<u16> {
    (0..w * h)
        .map(|i| ((i * 37 + seed) % 65_521) as u16)
        .collect()
}

/// The fixture: the committed float scene, plus a canvas-sized mask, a saved
/// channel, two guides, a moved ruler origin and a NON-SQUARE resolution (so
/// "the PPI transposes" is a real question for the quarter turns).
fn fixture() -> Option<PrecisionDocument> {
    if !std::path::Path::new(FIXTURE).exists() {
        return None;
    }
    let doc = PrecisionDocument::open_preserving(FIXTURE, LIMIT, 0, None).expect("the fixture");
    assert!(doc.is_float());
    let (w, h) = doc.dimensions();
    let samples = doc
        .layer_source(0)
        .expect("the scene's raster")
        .to_rgba32()
        .expect("float storage");
    assert!(
        samples.iter().any(|p| p[..3].iter().any(|v| *v < 0.0)),
        "the fixture must hold a NEGATIVE component, or this suite proves nothing"
    );
    assert!(
        samples.iter().any(|p| p[..3].iter().any(|v| *v > 1.0)),
        "the fixture must hold an ABOVE-WHITE component"
    );

    let mask = PrecisionMask::new(
        PrecisionPlane::from_u16(w, h, plane_samples(w, h, 3), None).expect("a mask plane"),
        MaskProperties::at((0, 0)),
        true,
    )
    .expect("a mask");
    let doc = doc
        .set_layer_mask(0, Some(mask), LIMIT, 0, None)
        .expect("a float mask is admitted")
        .expect("attaching a mask is a change");
    let doc = doc
        .mask_edit(
            &MaskEdit::SetChannel {
                index: None,
                name: "Coverage".into(),
            },
            &plane_samples(w, h, 11),
            LIMIT,
            0,
            None,
        )
        .expect("a float channel is admitted")
        .expect("adding a channel is a change");
    let doc = doc
        .set_resolution(PPI.0, PPI.1)
        .expect("a positive resolution is admitted")
        .expect("the resolution moves");
    let doc = doc
        .add_guide(
            doctored_core::doc_guide::GuideOrientation::Vertical,
            GUIDE_V,
        )
        .expect("a vertical guide");
    let doc = doc
        .add_guide(
            doctored_core::doc_guide::GuideOrientation::Horizontal,
            GUIDE_H,
        )
        .expect("a horizontal guide");
    Some(
        doc.set_ruler_origin(RULER.0, RULER.1)
            .expect("the origin moves"),
    )
}

/// The five permutations, written out here from the matrices in
/// `precision_geometry_plan::whole` rather than read from the core. `values`
/// is row-major with `stride` components per pixel; the answer is the same
/// layout at the destination dimensions.
fn turned<T: Copy + Default>(
    op: &str,
    values: &[T],
    (w, h): (u32, u32),
    stride: usize,
) -> (Vec<T>, (u32, u32)) {
    let (w, h) = (w as usize, h as usize);
    let (dw, dh) = match op {
        "rotate90" | "rotate270" => (h, w),
        _ => (w, h),
    };
    let mut out = vec![T::default(); dw * dh * stride];
    for y in 0..dh {
        for x in 0..dw {
            // (sx, sy) is the SOURCE pixel this destination pixel copies.
            let (sx, sy) = match op {
                // x' = h - y, y' = x  =>  dest (X, Y) reads (Y, h-1-X)
                "rotate90" => (y, h - 1 - x),
                // x' = w - x, y' = h - y
                "rotate180" => (w - 1 - x, h - 1 - y),
                // x' = y, y' = w - x  =>  dest (X, Y) reads (w-1-Y, X)
                "rotate270" => (w - 1 - y, x),
                "flip_horizontal" => (w - 1 - x, y),
                "flip_vertical" => (x, h - 1 - y),
                other => panic!("no hand permutation for {other}"),
            };
            for c in 0..stride {
                out[(y * dw + x) * stride + c] = values[(sy * w + sx) * stride + c];
            }
        }
    }
    (out, (dw as u32, dh as u32))
}

/// This file's own `doc_guide` restatement: what the two guides and the ruler
/// origin become under one exact whole-document op on a `w x h` canvas.
fn chrome_after(op: &str, (w, h): (u32, u32)) -> (Vec<(&'static str, f64)>, (f64, f64)) {
    let (fw, fh) = (f64::from(w), f64::from(h));
    let (guides, origin) = match op {
        "rotate90" => (
            vec![("horizontal", GUIDE_V), ("vertical", fh - GUIDE_H)],
            (fh - RULER.1, RULER.0),
        ),
        "rotate180" => (
            vec![("vertical", fw - GUIDE_V), ("horizontal", fh - GUIDE_H)],
            (fw - RULER.0, fh - RULER.1),
        ),
        "rotate270" => (
            vec![("horizontal", fw - GUIDE_V), ("vertical", GUIDE_H)],
            (RULER.1, fw - RULER.0),
        ),
        "flip_horizontal" => (
            vec![("vertical", fw - GUIDE_V), ("horizontal", GUIDE_H)],
            (fw - RULER.0, RULER.1),
        ),
        "flip_vertical" => (
            vec![("vertical", GUIDE_V), ("horizontal", fh - GUIDE_H)],
            (RULER.0, fh - RULER.1),
        ),
        other => panic!("no hand chrome for {other}"),
    };
    // The model's order: horizontal before vertical, then by position.
    let mut guides = guides;
    guides.sort_by(|a, b| {
        (a.0 == "vertical", a.1)
            .partial_cmp(&(b.0 == "vertical", b.1))
            .unwrap()
    });
    (guides, origin)
}

fn guide_pairs(doc: &PrecisionDocument) -> Vec<(&'static str, f64)> {
    doc.guides()
        .iter()
        .map(|g| {
            (
                match g.orientation {
                    doctored_core::doc_guide::GuideOrientation::Horizontal => "horizontal",
                    doctored_core::doc_guide::GuideOrientation::Vertical => "vertical",
                },
                g.position,
            )
        })
        .collect()
}

/// The operator table a host reads carries no geometry refusal row and
/// admits every geometry family but warp.
fn float_geometry_admitted() -> Value {
    let table: Value =
        serde_json::from_str(&doctored_core::float_document::float_operators_json()).unwrap();
    assert!(
        table["refused"].as_array().unwrap().iter().all(|row| {
            !row["reason"]
                .as_str()
                .is_some_and(|r| r.contains("resampling oracle"))
        }),
        "the geometry row is gone: {table}"
    );
    let admitted = table["admitted"].as_array().unwrap();
    for family in [
        "geometry_crop",
        "geometry_rotate",
        "geometry_flip",
        "geometry_resize",
        "geometry_straighten",
        "geometry_rotate_canvas",
        "geometry_affine",
        "geometry_perspective",
        "geometry_affine_set",
        "geometry_perspective_set",
        "mask_transform",
    ] {
        assert!(admitted.iter().any(|v| v == family), "{family} is admitted");
    }
    table
}

// ------------------------------------------------------ T1: zero-angle crop --

#[test]
fn a_float_crop_keeps_every_allocation_and_moves_the_window_only() {
    let Some(base) = fixture() else {
        return;
    };
    let (w, h) = base.dimensions();
    let base_rasters = rasters(&base);
    let masks = mask_addresses(&base);
    let channel = base.channels()[0].plane().samples().to_vec();

    // A ZERO-origin window: the one `image_equal` used to error on before
    // 12F-1 (the planned location equals the layer's own offset).
    let zero = apply(
        &base,
        json!({"op": "crop", "x": 0, "y": 0, "width": w - 8, "height": h - 6}),
    );
    assert_eq!(zero.dimensions(), (w - 8, h - 6));
    assert!(
        same_rasters(&rasters(&zero), &base_rasters),
        "the f32 buffer is SHARED"
    );
    assert_eq!(mask_addresses(&zero), masks, "and so is the mask plane");
    assert_eq!(zero.layer(0).unwrap().offset(), (0, 0));

    // A NON-ZERO origin.
    let (x, y, cw, ch) = (4u32, 3u32, 30u32, 20u32);
    let cropped = apply(
        &base,
        json!({"op": "crop", "x": x, "y": y, "width": cw, "height": ch}),
    );
    assert_eq!(cropped.dimensions(), (cw, ch));
    assert!(
        same_rasters(&rasters(&cropped), &base_rasters),
        "a crop reads no sample, so the allocation is the input's"
    );
    assert_eq!(mask_addresses(&cropped), masks);
    assert_eq!(
        cropped.layer(0).unwrap().offset(),
        (-(x as i32), -(y as i32)),
        "the window moved, the pixels did not"
    );
    assert_eq!(
        cropped
            .layer(0)
            .unwrap()
            .mask()
            .unwrap()
            .properties()
            .offset,
        (-(x as i32), -(y as i32))
    );

    // The CHANNEL is re-laid out into the window, sample for sample.
    let after = cropped.channels()[0].plane().samples();
    assert_eq!(
        cropped.channels()[0].plane().dimensions(),
        (cw, ch),
        "a channel is canvas-sized"
    );
    let mut want: Vec<u16> = Vec::with_capacity((cw * ch) as usize);
    for j in 0..ch {
        for i in 0..cw {
            want.push(channel[((j + y) * w + (i + x)) as usize]);
        }
    }
    assert_eq!(
        after,
        want.as_slice(),
        "the channel is this file's own crop"
    );

    // The CHROME: shift by (-x, -y), drop what left, clamp the origin.
    assert_eq!(
        guide_pairs(&cropped),
        vec![
            ("horizontal", GUIDE_H - f64::from(y)),
            ("vertical", GUIDE_V - f64::from(x)),
        ]
    );
    assert_eq!(
        cropped.ruler_origin(),
        (RULER.0 - f64::from(x), RULER.1 - f64::from(y))
    );
    assert_eq!(
        cropped.resolution().x,
        PPI.0,
        "a crop never touches the PPI"
    );

    // The FULL window changes nothing at all.
    assert!(
        try_apply(
            &base,
            json!({"op": "crop", "x": 0, "y": 0, "width": w, "height": h}),
        )
        .expect("a full-window crop is not an error")
        .is_none(),
        "the whole canvas is an exact no-op, so the purity rule answers None"
    );
}

// --------------------------------------------- T2: the quarter turns/flips --

#[test]
fn each_quarter_turn_and_flip_permutes_every_plane_at_float() {
    let Some(base) = fixture() else {
        return;
    };
    let (w, h) = base.dimensions();
    let source_bits = layer_bits(&base, 0);
    let source_mask = base
        .layer(0)
        .unwrap()
        .mask()
        .unwrap()
        .plane()
        .samples()
        .to_vec();
    let source_channel = base.channels()[0].plane().samples().to_vec();

    for op in [
        "rotate90",
        "rotate180",
        "rotate270",
        "flip_horizontal",
        "flip_vertical",
    ] {
        let out = apply(&base, json!({ "op": op }));
        let swaps = matches!(op, "rotate90" | "rotate270");
        let expected_canvas = if swaps { (h, w) } else { (w, h) };
        assert_eq!(out.dimensions(), expected_canvas, "{op} canvas");

        // The RASTER, bit for bit against this file's own permutation.
        let (want, dims) = turned(op, &source_bits, (w, h), 4);
        assert_eq!(dims, expected_canvas);
        assert_eq!(
            layer_bits(&out, 0),
            want,
            "{op}: the f32 bits are not this file's permutation"
        );
        assert_eq!(out.layer(0).unwrap().offset(), (0, 0), "{op} offset");

        // The MASK and the CHANNEL, on the same permutation.
        let mask = out.layer(0).unwrap().mask().unwrap();
        let (want_mask, _) = turned(op, &source_mask, (w, h), 1);
        assert_eq!(mask.plane().samples(), want_mask.as_slice(), "{op} mask");
        assert_eq!(mask.plane().dimensions(), expected_canvas);
        assert_eq!(mask.properties().offset, (0, 0));

        let (want_channel, _) = turned(op, &source_channel, (w, h), 1);
        assert_eq!(
            out.channels()[0].plane().samples(),
            want_channel.as_slice(),
            "{op} channel"
        );

        // The PPI transposes for the quarter turns and only for them.
        let resolution = out.resolution();
        if swaps {
            assert_eq!((resolution.x, resolution.y), (PPI.1, PPI.0), "{op} PPI");
        } else {
            assert_eq!((resolution.x, resolution.y), PPI, "{op} PPI");
        }

        // The CHROME.
        let (want_guides, want_origin) = chrome_after(op, (w, h));
        assert_eq!(guide_pairs(&out), want_guides, "{op} guides");
        assert_eq!(out.ruler_origin(), want_origin, "{op} ruler origin");
    }
}

// ---------------------------------------------- T3: the self-equal flip --

/// A float document authored HERE, horizontally symmetric on purpose, with a
/// negative and an above-white component in it. Flipping it horizontally
/// produces the same picture at the same offset, so the purity rule must
/// answer `None` — which is only reachable through `image_equal`'s F32 arm.
fn symmetric_float_document() -> PrecisionDocument {
    let (w, h) = (6u32, 4u32);
    let mut samples = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let mirrored = x.min(w - 1 - x);
            let v = f32::from(mirrored as u16) * 0.75 - 1.5 + f32::from(y as u16) * 0.5;
            samples.extend_from_slice(&[v, v + 1.25, -v, 1.0]);
        }
    }
    assert!(samples.iter().any(|v| *v < 0.0));
    assert!(samples.iter().any(|v| *v > 1.0));
    let image = FloatTiff {
        width: w,
        height: h,
        channels: 4,
        alpha: Some(false),
        samples,
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let bytes = doctored_core::tiff_float::encode_to_vec(&image, false).expect("float TIFF bytes");
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("symmetric.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the authored float document opens");
    assert!(doc.is_float());
    // The CHROME has to be self-equal too, or the flip is a real edit for a
    // reason that has nothing to do with the pixels: `geometry_origin` maps
    // the ruler zero to `(w - x, y)` under a horizontal flip, which fixes
    // exactly one point — the canvas's horizontal centre.
    doc.set_ruler_origin(f64::from(w) / 2.0, f64::from(h) / 2.0)
        .expect("the origin moves")
}

#[test]
fn a_symmetric_flip_that_changes_nothing_answers_none_at_float() {
    let doc = symmetric_float_document();
    assert!(
        try_apply(&doc, json!({"op": "flip_horizontal"}))
            .expect("a self-equal flip is not an error")
            .is_none(),
        "the picture is its own mirror, so the flip changes nothing"
    );
    // The VERTICAL flip is not symmetric here, so it is a real change — the
    // None above is the purity rule and not a stuck answer.
    assert!(try_apply(&doc, json!({"op": "flip_vertical"}))
        .expect("the edit runs")
        .is_some());
}

// ---------------------------------------------------- T4: v40 persistence --

#[test]
fn an_exact_float_canvas_op_survives_the_v40_round_trip() {
    let Some(base) = fixture() else {
        return;
    };
    let (w, h) = base.dimensions();
    for request in [
        json!({"op": "crop", "x": 4, "y": 3, "width": w - 10, "height": h - 7}),
        json!({"op": "rotate90"}),
        json!({"op": "flip_vertical"}),
    ] {
        let out = apply(&base, request.clone());
        let bytes = out.encode_native(LIMIT, 0, None).expect("v40 bytes");
        let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
        assert!(reopened.is_float(), "{request}");
        assert_eq!(reopened.dimensions(), out.dimensions(), "{request}");
        assert_eq!(
            reopened.encode_native(LIMIT, 0, None).expect("resave"),
            bytes,
            "{request}: the v40 round trip is identical"
        );
    }
}

// ------------------------------------------------ T5: the capability rows --

#[test]
fn the_exact_canvas_families_are_admitted_at_depth_thirty_two() {
    for family in ["geometry_crop", "geometry_rotate", "geometry_flip"] {
        let row = capability(family, "document", 32);
        assert_eq!(row["supported"], true, "{family} at depth 32: {row}");
        assert_eq!(row["reason"], Value::Null);
        // And at 16 bits, unchanged.
        assert_eq!(capability(family, "document", 16)["supported"], true);
        // A LAYER target is still not what a document op takes.
        assert_eq!(capability(family, "raster", 32)["supported"], false);
    }
    // The FILTERED families are admitted at 32 too, on their own
    // targets, and warp with them.
    for (family, target) in [
        ("geometry_resize", "document"),
        ("geometry_straighten", "document"),
        ("geometry_rotate_canvas", "document"),
        ("geometry_affine", "raster"),
        ("geometry_affine", "mask"),
        ("geometry_perspective", "raster"),
        ("geometry_perspective", "mask"),
        ("geometry_affine_set", "group"),
        ("geometry_perspective_set", "group"),
        ("mask_transform", "mask"),
        ("geometry_warp", "raster"),
        ("geometry_warp", "mask"),
    ] {
        let row = capability(family, target, 32);
        assert_eq!(row["supported"], true, "{family}/{target} at float: {row}");
    }
}

// ------------------------------------------- T6: the filtered set admitted --

#[test]
fn the_filtered_geometry_is_admitted_at_float() {
    let Some(base) = fixture() else {
        return;
    };
    float_geometry_admitted();
    let (w, h) = base.dimensions();
    for request in [
        json!({"op": "resize", "width": w / 2, "height": h / 2, "filter": "bilinear"}),
        json!({"op": "affine", "layer": 0, "target": "pixels",
               "matrix": [1.0, 0.0, 0.0, 1.0, 0.25, 0.5], "filter": "bilinear"}),
        json!({"op": "affine", "layer": 0, "target": "mask",
               "matrix": [1.0, 0.0, 0.0, 1.0, 0.25, 0.5], "filter": "bilinear"}),
        json!({"op": "perspective", "layer": 0, "target": "pixels",
               "quad": [0.0, 0.0, f64::from(w), 1.0, f64::from(w), f64::from(h),
                        0.0, f64::from(h)], "filter": "bilinear"}),
    ] {
        let edit = parse(request.clone());
        base.geometry_plan(&edit)
            .unwrap_or_else(|e| panic!("geometry_plan admits {request}: {e}"));
        let out = apply(&base, request.clone());
        assert!(out.is_float(), "{request}");
        assert_eq!(out.float_refusal(), None, "{request}");
    }

    // A 90-degree PER-LAYER affine with Nearest: exact as a matrix, so the
    // plan pins Nearest and the raster is the hand-written permutation.
    let turned_doc = apply(
        &base,
        json!({"op": "affine", "layer": 0, "target": "pixels",
               "matrix": [0.0, 1.0, -1.0, 0.0, f64::from(h), 0.0], "filter": "nearest"}),
    );
    let source = base.layer_source(0).unwrap();
    let (sw, sh) = source.dimensions();
    let values: Vec<u32> = layer_bits(&base, 0);
    let (want, size) = turned("rotate90", &values, (sw, sh), 4);
    assert_eq!(
        turned_doc.layer_source(0).unwrap().dimensions(),
        size,
        "the turned extent"
    );
    assert_eq!(
        layer_bits(&turned_doc, 0),
        want,
        "a Nearest quarter-turn affine copies every f32 bit"
    );
}
