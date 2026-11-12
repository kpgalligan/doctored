//! VECTOR MASKS at 8 bits, through the `dr_doc_*vector_mask*` exports
//! and the ordinary mask and geometry ops a vector-masked layer meets.
//!
//! The oracle is written here and reads nothing from the core: the shape is
//! flattened by evaluating each cubic's Bernstein form at 48 parameters, and
//! a pixel's coverage is the fraction of its 16 × 16 sample centres the
//! flattened polygon winds around ([`oracle`]). A sample grid that fine is
//! within one row of samples of the true area on a pixel an edge crosses, so
//! the composite is asked to match it to [`EDGE`]; a pixel the oracle puts
//! wholly inside or outside must be exactly 255 or 0.
//!
//! The picture under test is one opaque layer, so the flattened ALPHA of the
//! document is the mask's effective coverage — which is how a vector mask is
//! read here without asking the core what it baked.

use std::ffi::CString;
use std::ptr;

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::doc_transform::Affine;
use doctored_core::ffi_psd::dr_doc_psd_export_report;
use doctored_core::ffi_vector_mask::*;
use image::imageops::FilterType;
use image::{GrayImage, Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{byte_geometry_edit, drdc_word, mask_bytes, selection, solid, take_err_string};
use crate::path_format::{open_byte, path_table, save_byte, with_table};
use crate::path_geometry::{byte, changed, polygon, refused, saved, set_byte, take, unchanged};

const W: u32 = 48;
const H: u32 = 40;
/// How far a composite may sit from the 16 × 16 oracle on an edge pixel:
/// one row of samples (1/16) and the byte rounding, in 8-bit levels.
const EDGE: f64 = 20.0;
const CAP: &str = "too many path anchors in the document (max 65536)";

/// One anchor: its point and its two handles.
#[derive(Clone, Copy, Debug)]
struct Node {
    p: [f64; 2],
    i: [f64; 2],
    o: [f64; 2],
}

/// A closed four-anchor blob with every side curved, inside the canvas.
fn blob() -> Vec<Node> {
    vec![
        Node {
            p: [12.0, 8.0],
            i: [8.0, 12.0],
            o: [22.0, 3.0],
        },
        Node {
            p: [36.0, 12.0],
            i: [30.0, 5.0],
            o: [42.0, 20.0],
        },
        Node {
            p: [30.0, 32.0],
            i: [40.0, 30.0],
            o: [20.0, 34.0],
        },
        Node {
            p: [8.0, 24.0],
            i: [10.0, 34.0],
            o: [6.0, 16.0],
        },
    ]
}

fn mapped(nodes: &[Node], map: impl Fn([f64; 2]) -> [f64; 2]) -> Vec<Node> {
    nodes
        .iter()
        .map(|n| Node {
            p: map(n.p),
            i: map(n.i),
            o: map(n.o),
        })
        .collect()
}

/// The shape JSON of one closed subpath.
fn shape_of(nodes: &[Node]) -> Value {
    let anchors: Vec<Value> = nodes
        .iter()
        .map(|n| json!({"x": n.p[0], "y": n.p[1], "in": n.i, "out": n.o}))
        .collect();
    json!({"fill_rule": "nonzero", "subpaths": [{"closed": true, "anchors": anchors}]})
}

/// The closed polygon of `nodes`: each cubic at 48 Bernstein parameters.
fn flattened(nodes: &[Node]) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    for k in 0..nodes.len() {
        let (a, b) = (nodes[k], nodes[(k + 1) % nodes.len()]);
        let p = [a.p, a.o, b.i, b.p];
        for step in 0..48 {
            let t = f64::from(step) / 48.0;
            let s = 1.0 - t;
            let w = [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t];
            out.push([0, 1].map(|axis| (0..4).map(|j| w[j] * p[j][axis]).sum()));
        }
    }
    out
}

/// The winding number of `polygon` around `at`.
fn winding(polygon: &[[f64; 2]], at: [f64; 2]) -> i32 {
    let mut turns = 0;
    for k in 0..polygon.len() {
        let (a, b) = (polygon[k], polygon[(k + 1) % polygon.len()]);
        let side = (b[0] - a[0]) * (at[1] - a[1]) - (at[0] - a[0]) * (b[1] - a[1]);
        if a[1] <= at[1] && b[1] > at[1] && side > 0.0 {
            turns += 1;
        } else if a[1] > at[1] && b[1] <= at[1] && side < 0.0 {
            turns -= 1;
        }
    }
    turns
}

/// The shape's coverage of each pixel of a `w` × `h` canvas, in 0…1: the
/// fraction of the pixel's 16 × 16 sample centres inside the flattened
/// polygon under the nonzero rule.
fn oracle(nodes: &[Node], w: u32, h: u32) -> Vec<f64> {
    let polygon = flattened(nodes);
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut inside = 0;
            for sy in 0..16 {
                for sx in 0..16 {
                    let at = [
                        f64::from(x) + (f64::from(sx) + 0.5) / 16.0,
                        f64::from(y) + (f64::from(sy) + 0.5) / 16.0,
                    ];
                    inside += i32::from(winding(&polygon, at) != 0);
                }
            }
            out.push(f64::from(inside) / 256.0);
        }
    }
    out
}

/// The flattened document's alpha plane.
fn alpha(doc: &DrDocument) -> Vec<u8> {
    doc.flattened()
        .expect("flatten")
        .pixels()
        .map(|p| p[3])
        .collect()
}

/// `alpha` is `expected` (0…255 a pixel, `w` pixels a row) to the oracle's
/// tolerance, and EXACTLY 0 or 255 wherever `expected` is that value on the
/// pixel and on all its neighbours — a pixel no edge comes near. (A pixel the
/// sample grid alone calls full may still lose a sliver between samples.)
fn assert_coverage(alpha: &[u8], expected: &[f64], w: u32, what: &str) {
    assert_eq!(alpha.len(), expected.len(), "{what}");
    for (index, (&got, &want)) in alpha.iter().zip(expected).enumerate() {
        let (x, y) = (index as u32 % w, index as u32 / w);
        if (want == 0.0 || want == 255.0) && level(expected, w, index) {
            assert_eq!(f64::from(got), want, "{what}: pixel ({x}, {y})");
        } else {
            assert!(
                (f64::from(got) - want).abs() <= EDGE,
                "{what}: pixel ({x}, {y}) is {got}, the oracle says {want:.1}"
            );
        }
    }
}

/// Whether `values` (`w` a row) holds one value on pixel `index` and on all
/// its neighbours: no edge comes near.
fn level(values: &[f64], w: u32, index: usize) -> bool {
    let (w, h) = (w as i64, values.len() as i64 / w as i64);
    let (x, y) = (index as i64 % w, index as i64 / w);
    (-1..=1).all(|dy| {
        (-1..=1).all(|dx| {
            let (nx, ny) = (x + dx, y + dy);
            nx < 0
                || ny < 0
                || nx >= w
                || ny >= h
                || values[(ny * w + nx) as usize] == values[index]
        })
    })
}

/// Two bakes of one shape over different windows agree to one level: the
/// rasterizer's sums are taken in a different order.
fn assert_same_bake(a: &[u8], b: &[u8], what: &str) {
    assert_eq!(a.len(), b.len(), "{what}");
    for (index, (&x, &y)) in a.iter().zip(b).enumerate() {
        assert!(x.abs_diff(y) <= 1, "{what}: pixel {index} is {x} and {y}");
    }
}

fn levels(coverage: &[f64]) -> Vec<f64> {
    coverage.iter().map(|c| c * 255.0).collect()
}

/// `dr_doc_set_vector_mask`'s three answers.
fn set_shape(doc: &DrDocument, layer: usize, shape: &Value) -> Result<Option<DrDocument>, String> {
    let text = CString::new(shape.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_doc_set_vector_mask(doc, layer, text.as_ptr(), ptr::null(), &mut error) };
    answer(out, error)
}

fn set_flags(
    doc: &DrDocument,
    layer: usize,
    enabled: bool,
    linked: bool,
) -> Result<Option<DrDocument>, String> {
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_doc_set_vector_mask_properties(doc, layer, enabled, linked, ptr::null(), &mut error)
    };
    answer(out, error)
}

fn answer(
    out: *mut DrDocument,
    error: *mut std::ffi::c_char,
) -> Result<Option<DrDocument>, String> {
    if !out.is_null() {
        assert!(error.is_null());
        return Ok(Some(*unsafe { Box::from_raw(out) }));
    }
    if error.is_null() {
        Ok(None)
    } else {
        Err(take_err_string(error))
    }
}

fn removed(doc: &DrDocument, layer: usize) -> Option<DrDocument> {
    let out = unsafe { dr_doc_remove_vector_mask(doc, layer) };
    (!out.is_null()).then(|| *unsafe { Box::from_raw(out) })
}

/// The layer's vector mask as the core reports it, or `None`.
fn view(doc: &DrDocument, layer: usize) -> Option<Value> {
    let out = unsafe { dr_doc_vector_mask_json(doc, layer) };
    (!out.is_null()).then(|| serde_json::from_str(&take(out)).expect("vector mask JSON"))
}

/// The reported shape's nodes are `nodes`, to the model's four decimals.
fn assert_shape(view: &Value, nodes: &[Node], what: &str) {
    let anchors = view["subpaths"][0]["anchors"].as_array().expect("anchors");
    assert_eq!(anchors.len(), nodes.len(), "{what}");
    let near = |got: f64, want: f64| (got - want).abs() <= 1e-4;
    for (anchor, node) in anchors.iter().zip(nodes) {
        let got = [
            [anchor["x"].as_f64().unwrap(), anchor["y"].as_f64().unwrap()],
            [
                anchor["in"][0].as_f64().unwrap(),
                anchor["in"][1].as_f64().unwrap(),
            ],
            [
                anchor["out"][0].as_f64().unwrap(),
                anchor["out"][1].as_f64().unwrap(),
            ],
        ];
        for (g, w) in got.iter().zip([node.p, node.i, node.o]) {
            assert!(
                near(g[0], w[0]) && near(g[1], w[1]),
                "{what}: {g:?} should be {w:?}"
            );
        }
    }
}

/// A canvas-sized raster coverage that varies in both axes and has no zero.
fn pattern(x: u32, y: u32) -> u8 {
    (60 + (x * 3 + y * 2) % 190) as u8
}

/// The base document with the blob as layer 0's vector mask.
fn masked() -> DrDocument {
    changed(set_shape(&byte(W, H), 0, &shape_of(&blob())))
}

// ------------------------------------------------------------- the bake --

#[test]
fn a_vector_mask_composites_as_the_shapes_coverage() {
    let base = byte(W, H);
    assert!(view(&base, 0).is_none(), "no vector mask yet");
    assert!(removed(&base, 0).is_none());
    assert!(unchanged(set_flags(&base, 0, false, true)));
    assert!(unchanged(set_shape(&base, 7, &shape_of(&blob()))));

    let doc = masked();
    let seen = view(&doc, 0).expect("a vector mask");
    assert_shape(&seen, &blob(), "the stored shape");
    assert_eq!(
        (
            &seen["enabled"],
            &seen["linked"],
            &seen["has_raster"],
            &seen["raster_enabled"],
            &seen["fill_rule"],
        ),
        (
            &json!(true),
            &json!(true),
            &json!(false),
            &json!(false),
            &json!("nonzero")
        )
    );
    let want = levels(&oracle(&blob(), W, H));
    assert_coverage(&alpha(&doc), &want, W, "the blob");
    // The colour is the layer's own wherever the layer shows in full.
    let flat = doc.flattened().unwrap();
    assert!(flat
        .pixels()
        .all(|p| p[3] < 255 || [p[0], p[1], p[2]] == [200, 100, 50]));
    // The same shape again is no edit; a malformed one says why.
    assert!(unchanged(set_shape(&doc, 0, &shape_of(&blob()))));
    assert!(
        refused(set_shape(&doc, 0, &json!({"subpaths": [{"anchors": []}]})))
            .contains("at least one anchor")
    );

    // A new shape replaces the old one and keeps the flags.
    let square = polygon(&[[4.0, 4.0], [20.0, 4.0], [20.0, 20.0], [4.0, 20.0]], true);
    let squared = changed(set_shape(&doc, 0, &json!({"subpaths": [square]})));
    let inside = |x: u32, y: u32| (4..20).contains(&x) && (4..20).contains(&y);
    let got = alpha(&squared);
    for y in 0..H {
        for x in 0..W {
            let want = if inside(x, y) { 255 } else { 0 };
            assert_eq!(got[(y * W + x) as usize], want, "square at ({x}, {y})");
        }
    }

    // An empty shape is Reveal All: a vector mask that hides nothing.
    let open = changed(set_shape(&doc, 0, &json!({"subpaths": []})));
    assert!(alpha(&open).iter().all(|&a| a == 255));
    assert!(open.layers[0].mask.is_none());
    let seen = view(&open, 0).expect("still a vector mask");
    assert_eq!(seen["subpaths"], json!([]));
    // …and removing it leaves a layer with no mask at all.
    let bare = removed(&open, 0).expect("removed");
    assert!(view(&bare, 0).is_none());
    assert!(bare.layers[0].mask.is_none());
}

#[test]
fn a_raster_mask_beside_the_shape_multiplies_in() {
    let coverage = selection(W, H, pattern);
    let rastered = byte(W, H)
        .add_mask(0, MaskKind::FromSelection(&coverage))
        .expect("a raster mask");
    let doc = changed(set_shape(&rastered, 0, &shape_of(&blob())));
    let seen = view(&doc, 0).expect("a vector mask");
    assert_eq!(
        (&seen["has_raster"], &seen["raster_enabled"]),
        (&json!(true), &json!(true))
    );
    // The product of the two factors, each known on its own: the raster
    // bytes written above and the shape's coverage from the oracle.
    let shape = oracle(&blob(), W, H);
    let want: Vec<f64> = shape
        .iter()
        .zip(&coverage)
        .map(|(s, &r)| s * f64::from(r))
        .collect();
    let got = alpha(&doc);
    for (index, (&g, &w)) in got.iter().zip(&want).enumerate() {
        // Away from the shape's edge only the product's rounding is left.
        let slack = if level(&shape, W, index) { 1.0 } else { EDGE };
        assert!(
            (f64::from(g) - w).abs() <= slack,
            "pixel {index} is {g}, the product is {w:.1}"
        );
    }

    // Removing the vector mask puts the raster part back verbatim.
    let back = removed(&doc, 0).expect("removed");
    assert!(view(&back, 0).is_none());
    assert_eq!(mask_bytes(&back, 0), coverage);
    assert_eq!(alpha(&back), coverage);
    assert_eq!(
        back.layers[0].mask_properties.offset,
        rastered.layers[0].mask_properties.offset
    );
}

#[test]
fn the_two_flags_bake_and_report() {
    let doc = masked();
    let lit = alpha(&doc);
    // Disabled with no raster part: nothing is hidden, and the mask the
    // layer keeps is a disabled one.
    let off = changed(set_flags(&doc, 0, false, true));
    assert!(alpha(&off).iter().all(|&a| a == 255));
    assert!(!off.layers[0].mask_enabled);
    assert_eq!(view(&off, 0).unwrap()["enabled"], json!(false));
    assert!(unchanged(set_flags(&off, 0, false, true)), "the same flags");
    let on = changed(set_flags(&off, 0, true, true));
    assert_eq!(alpha(&on), lit, "enabling bakes the same plane back");

    // Unlinked: the layer moves under a mask that stays put.
    let loose = changed(set_flags(&doc, 0, true, false));
    assert_eq!(view(&loose, 0).unwrap()["linked"], json!(false));
    assert_eq!(alpha(&loose), lit);
    let slid = loose.move_layers(&[0], 6, 0).expect("moved");
    assert_shape(&view(&slid, 0).expect("still there"), &blob(), "unlinked");
    let got = alpha(&slid);
    for y in 0..H {
        for x in 0..W {
            // The layer now starts at column 6; the mask did not move.
            let want = if x < 6 { 0 } else { lit[(y * W + x) as usize] };
            assert_eq!(got[(y * W + x) as usize], want, "unlinked at ({x}, {y})");
        }
    }

    // With a raster part, disabling the shape shows the raster part alone.
    let coverage = selection(W, H, pattern);
    let both = changed(set_shape(
        &byte(W, H)
            .add_mask(0, MaskKind::FromSelection(&coverage))
            .unwrap(),
        0,
        &shape_of(&blob()),
    ));
    let raster_only = changed(set_flags(&both, 0, false, true));
    assert_eq!(alpha(&raster_only), coverage);
    assert_eq!(view(&raster_only, 0).unwrap()["has_raster"], json!(true));
    assert_eq!(
        alpha(&changed(set_flags(&raster_only, 0, true, true))),
        alpha(&both)
    );
}

// ------------------------------------------------------------ following --

#[test]
fn a_vector_mask_follows_a_move_a_free_transform_a_quarter_turn_and_a_resize() {
    let doc = masked();

    // MOVE: the shape is read shifted, and a rebake at the new place gives
    // the very plane the move carried.
    let moved = doc.move_layers(&[0], 5, 3).expect("moved");
    let shifted = mapped(&blob(), |p| [p[0] + 5.0, p[1] + 3.0]);
    assert_shape(&view(&moved, 0).expect("moved mask"), &shifted, "move");
    let coverage = oracle(&shifted, W, H);
    let want: Vec<f64> = (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .map(|(x, y)| {
            // The layer left columns 0…4 and rows 0…2 empty.
            if x < 5 || y < 3 {
                0.0
            } else {
                coverage[(y * W + x) as usize] * 255.0
            }
        })
        .collect();
    assert_coverage(&alpha(&moved), &want, W, "move");
    let rebaked = changed(set_flags(
        &changed(set_flags(&moved, 0, false, true)),
        0,
        true,
        true,
    ));
    assert_same_bake(&alpha(&rebaked), &alpha(&moved), "a rebake after the move");

    // FREE TRANSFORM (a scale and a shift): the shape goes through the
    // matrix and is baked again, so the edge is the oracle's, not a
    // resampled plane's.
    let m = [1.25, 0.0, 0.0, 1.25, -3.0, -2.0];
    let scaled = doc
        .transform_layer(0, Affine::from_array(m), FilterType::Triangle)
        .expect("transformed");
    let through = mapped(&blob(), |p| [p[0] * 1.25 - 3.0, p[1] * 1.25 - 2.0]);
    let seen = view(&scaled, 0).expect("the mask followed the transform");
    assert_shape(&seen, &through, "free transform");
    assert_eq!(seen["has_raster"], json!(false));
    assert_coverage(
        &alpha(&scaled),
        &levels(&oracle(&through, W, H)),
        W,
        "free transform",
    );

    // ROTATE 90 clockwise: (x, y) → (H − y, x) on an H × W canvas.
    let turned = doc.rotate90().expect("turned");
    assert_eq!((turned.width, turned.height), (H, W));
    let through = mapped(&blob(), |p| [f64::from(H) - p[1], p[0]]);
    assert_shape(
        &view(&turned, 0).expect("turned mask"),
        &through,
        "rotate90",
    );
    assert_coverage(
        &alpha(&turned),
        &levels(&oracle(&through, H, W)),
        H,
        "rotate90",
    );

    // IMAGE SIZE to 96 × 60: (x, y) → (2x, 1.5y).
    let grown = doc.resize(96, 60, FilterType::Triangle).expect("resized");
    let through = mapped(&blob(), |p| [p[0] * 2.0, p[1] * 1.5]);
    assert_shape(&view(&grown, 0).expect("resized mask"), &through, "resize");
    assert_coverage(
        &alpha(&grown),
        &levels(&oracle(&through, 96, 60)),
        96,
        "resize",
    );

    // CROP is a translation of the canvas: the shape shifts with it.
    let cropped = doc.crop(4, 2, 40, 36).expect("cropped");
    let through = mapped(&blob(), |p| [p[0] - 4.0, p[1] - 2.0]);
    assert_shape(&view(&cropped, 0).expect("cropped mask"), &through, "crop");
    assert_coverage(
        &alpha(&cropped),
        &levels(&oracle(&through, 40, 36)),
        40,
        "crop",
    );
}

#[test]
fn a_vector_mask_with_a_raster_part_becomes_a_raster_mask_under_a_resample() {
    let coverage = selection(W, H, pattern);
    let both = changed(set_shape(
        &byte(W, H)
            .add_mask(0, MaskKind::FromSelection(&coverage))
            .unwrap(),
        0,
        &shape_of(&blob()),
    ));
    let before = alpha(&both);
    let turned = both.rotate90().expect("turned");
    assert!(view(&turned, 0).is_none(), "the record died with the turn");
    assert!(
        turned.layers[0].mask.is_some(),
        "the product stays as a mask"
    );
    // A quarter turn permutes pixels: (x, y) lands on (H − 1 − y, x).
    let after = alpha(&turned);
    for y in 0..H {
        for x in 0..W {
            assert_eq!(
                after[(x * H + (H - 1 - y)) as usize],
                before[(y * W + x) as usize],
                "({x}, {y})"
            );
        }
    }
    // A move is not a resample: the record survives it.
    let moved = both.move_layers(&[0], 2, 1).expect("moved");
    assert_eq!(view(&moved, 0).unwrap()["has_raster"], json!(true));
}

#[test]
fn an_arbitrary_canvas_rotation_leaves_a_plain_mask_and_the_same_picture() {
    let doc = masked();
    // The twin: the same layer with the baked plane as an ordinary mask.
    let layer = &doc.layers[0];
    let plane: GrayImage = (**layer.mask.as_ref().expect("a baked mask")).clone();
    let twin = byte(W, H)
        .set_mask(0, plane, layer.mask_properties.offset, true, 0)
        .expect("a plain mask");
    assert!(view(&twin, 0).is_none());
    assert_eq!(alpha(&twin), alpha(&doc));

    let request = json!({"op": "rotate_canvas", "degrees": 30, "filter": "bilinear"});
    let turn = |doc: &DrDocument| {
        let (out, error) = byte_geometry_edit(doc, &request);
        assert_eq!(error, None);
        assert!(!out.is_null());
        *unsafe { Box::from_raw(out) }
    };
    let (turned, plain) = (turn(&doc), turn(&twin));
    assert!(view(&turned, 0).is_none(), "no vector mask after the turn");
    assert!(turned.layers[0].mask.is_some());
    assert_eq!(
        turned.flattened().unwrap().as_raw(),
        plain.flattened().unwrap().as_raw(),
        "the picture is the plain-mask twin's"
    );

    // A whole quarter turn asked for by angle is exact, and the mask follows.
    let request = json!({"op": "rotate_canvas", "degrees": 90, "filter": "bilinear"});
    let (out, error) = byte_geometry_edit(&doc, &request);
    assert_eq!(error, None);
    let quarter = *unsafe { Box::from_raw(out) };
    let through = mapped(&blob(), |p| [f64::from(H) - p[1], p[0]]);
    assert_shape(
        &view(&quarter, 0).expect("followed"),
        &through,
        "90 degrees",
    );
}

// ----------------------------------------------------------- mask edits --

#[test]
fn a_mask_edit_edits_the_raster_part_and_keeps_the_shape() {
    let doc = masked();
    let lit = alpha(&doc);
    // With no raster part there is no layer mask to paint, enable or delete.
    let black = [0u8, 0, 0, 255].repeat((W * H) as usize);
    assert!(doc.paint_mask(0, &black).is_none());
    assert!(doc.set_mask_enabled(0, false).is_none());
    assert!(doc.remove_mask(0, false).is_none());

    // Adding a layer mask gives the vector mask a raster part.
    let both = doc.add_mask(0, MaskKind::RevealAll).expect("a raster part");
    let seen = view(&both, 0).expect("still a vector mask");
    assert_shape(&seen, &blob(), "after add_mask");
    assert_eq!(seen["has_raster"], json!(true));
    assert_same_bake(&alpha(&both), &lit, "a reveal-all part changes nothing");
    let lit_beside = alpha(&both);

    // Painting black over a block hides it there and nowhere else, and the
    // shape is untouched.
    let mut overlay = vec![0u8; (W * H * 4) as usize];
    let hit = |x: u32, y: u32| (16..28).contains(&x) && (10..22).contains(&y);
    for y in 0..H {
        for x in 0..W {
            if hit(x, y) {
                let at = ((y * W + x) * 4) as usize;
                overlay[at..at + 4].copy_from_slice(&[0, 0, 0, 255]);
            }
        }
    }
    let painted = both.paint_mask(0, &overlay).expect("painted");
    assert_shape(&view(&painted, 0).expect("kept"), &blob(), "after paint");
    let got = alpha(&painted);
    for y in 0..H {
        for x in 0..W {
            let at = (y * W + x) as usize;
            let want = if hit(x, y) { 0 } else { lit_beside[at] };
            assert_eq!(got[at], want, "paint at ({x}, {y})");
        }
    }
    // The raster part itself holds the paint, not the product: taking the
    // vector mask away shows a mask that is black in the block and white
    // everywhere else — outside the shape too.
    let part = removed(&painted, 0).expect("removed");
    let got = alpha(&part);
    for y in 0..H {
        for x in 0..W {
            let want = if hit(x, y) { 0 } else { 255 };
            assert_eq!(got[(y * W + x) as usize], want, "part at ({x}, {y})");
        }
    }

    // The raster part keeps its own placement and properties, and reports
    // them: the layer's mask getters describe the baked mask instead.
    let seen = view(&painted, 0).expect("kept");
    assert_eq!(
        seen["raster"],
        json!({"offset_x": 0, "offset_y": 0, "width": W, "height": H, "linked": true,
               "density": 1.0, "feather": 0.0, "outside_coverage": 0})
    );
    // Density is the raster part's: at one half the painted block shows at
    // half strength inside the shape, and the baked mask itself stays a
    // plain plane at density 1.
    let dense = painted
        .set_mask_properties(0, true, 0.5, 0.0)
        .expect("density");
    assert_eq!(view(&dense, 0).unwrap()["raster"]["density"], json!(0.5));
    assert_eq!(dense.layers[0].mask_properties.density, 1.0);
    let got = alpha(&dense);
    assert!(
        got[(16 * W + 22) as usize].abs_diff(128) <= 1,
        "the block, halved"
    );
    // Outside the block the raster part is white, which density leaves alone.
    assert_same_bake(
        &got[..(10 * W) as usize],
        &lit_beside[..(10 * W) as usize],
        "above",
    );
    // The link is one flag for both halves, whichever side sets it.
    let loose = painted
        .set_mask_properties(0, false, 1.0, 0.0)
        .expect("unlinked");
    let seen = view(&loose, 0).unwrap();
    assert_eq!(
        (&seen["linked"], &seen["raster"]["linked"]),
        (&json!(false), &json!(false))
    );
    assert!(!loose.layers[0].mask_properties.linked);
    let tied = changed(set_flags(&loose, 0, true, true));
    let seen = view(&tied, 0).unwrap();
    assert_eq!(
        (&seen["linked"], &seen["raster"]["linked"]),
        (&json!(true), &json!(true))
    );

    // Disabling the layer mask disables the raster part only.
    let quiet = painted.set_mask_enabled(0, false).expect("disabled");
    let seen = view(&quiet, 0).expect("kept");
    assert_eq!(
        (&seen["has_raster"], &seen["raster_enabled"]),
        (&json!(true), &json!(false))
    );
    assert_eq!(alpha(&quiet), lit);
    // Deleting the layer mask deletes the raster part only.
    let alone = painted.remove_mask(0, false).expect("deleted");
    assert_eq!(view(&alone, 0).unwrap()["has_raster"], json!(false));
    assert_eq!(alpha(&alone), lit);
}

#[test]
fn applying_the_mask_consumes_the_shape_and_the_raster_part() {
    let both = masked().add_mask(0, MaskKind::RevealAll).unwrap();
    let mut overlay = vec![0u8; (W * H * 4) as usize];
    overlay[..(W * 4 * 12) as usize]
        .chunks_exact_mut(4)
        .for_each(|p| p.copy_from_slice(&[0, 0, 0, 255]));
    let painted = both.paint_mask(0, &overlay).expect("painted");
    let before = painted.flattened().unwrap();
    let applied = painted.remove_mask(0, true).expect("applied");
    assert!(view(&applied, 0).is_none());
    assert!(applied.layers[0].mask.is_none());
    assert_eq!(applied.flattened().unwrap().as_raw(), before.as_raw());
    // The alpha now lives in the pixels.
    let pixels = applied.layers[0].pixels.to_contiguous().unwrap();
    assert!(pixels.pixels().any(|p| p[3] == 0));
}

// -------------------------------------------------------------- the cap --

#[test]
fn a_shape_that_crosses_the_document_anchor_cap_refuses_with_the_cap_sentence() {
    let ring = |count: usize| {
        let corners: Vec<[f64; 2]> = (0..count)
            .map(|k| [(k % 40) as f64 + 2.0, (k / 40 % 30) as f64 + 2.0])
            .collect();
        json!([polygon(&corners, true)])
    };
    // 65533 anchors in the path list: three more fit, four do not.
    let list = json!({"paths": [saved("A", ring(32768)), saved("B", ring(32765))]});
    let full = changed(set_byte(&byte(W, H), &list));
    let triangle = [[4.0, 4.0], [30.0, 6.0], [10.0, 30.0]];
    let square = [[4.0, 4.0], [30.0, 4.0], [30.0, 30.0], [4.0, 30.0]];
    assert_eq!(
        refused(set_shape(
            &full,
            0,
            &json!({"subpaths": [polygon(&square, true)]})
        )),
        CAP
    );
    assert!(view(&full, 0).is_none(), "a refused shape changes nothing");
    let at_cap = changed(set_shape(
        &full,
        0,
        &json!({"subpaths": [polygon(&triangle, true)]}),
    ));
    // The vector mask's anchors count against the path list too.
    let one_more = json!({"paths": [
        saved("A", ring(32768)), saved("B", ring(32765)),
        saved("C", json!([polygon(&[[1.0, 1.0]], false)])),
    ]});
    assert_eq!(refused(set_byte(&at_cap, &one_more)), CAP);
    // …and replacing the mask's shape with a larger one is counted net of
    // the shape it replaces.
    assert_eq!(
        refused(set_shape(
            &at_cap,
            0,
            &json!({"subpaths": [polygon(&square, true)]})
        )),
        CAP
    );
}

// ----------------------------------------------------------------- .rz --

/// Three layers: a vector mask alone; one beside a feathered raster mask at
/// density 0.8; and a disabled one beside a raster mask.
fn layered() -> (DrDocument, Vec<u8>) {
    let coverage = selection(W, H, pattern);
    let doc = byte(W, H)
        .adding_image_layer(0, solid(W, H, [10, 200, 90, 255]), "Feathered")
        .expect("layer 1")
        .adding_image_layer(1, solid(W, H, [90, 10, 200, 255]), "Disabled")
        .expect("layer 2");
    let doc = changed(set_shape(&doc, 0, &shape_of(&blob())));
    let doc = doc
        .add_mask(1, MaskKind::FromSelection(&coverage))
        .expect("mask 1")
        .set_mask_properties(1, true, 0.8, 2.0)
        .expect("properties 1");
    let triangle = polygon(&[[6.0, 6.0], [44.0, 10.0], [20.0, 36.0]], true);
    let doc = changed(set_shape(&doc, 1, &json!({"subpaths": [triangle]})));
    let doc = doc
        .add_mask(2, MaskKind::FromSelection(&coverage))
        .expect("mask 2");
    let square = polygon(&[[8.0, 8.0], [40.0, 8.0], [40.0, 32.0], [8.0, 32.0]], true);
    let doc = changed(set_shape(
        &doc,
        2,
        &json!({"fill_rule": "evenodd", "subpaths": [square]}),
    ));
    (changed(set_flags(&doc, 2, false, true)), coverage)
}

#[test]
fn vector_masks_round_trip_through_rz() {
    let dir = TempDir::new().unwrap();
    let (doc, coverage) = layered();
    let bytes = save_byte(&dir, "masks.rz", &doc);
    // No path, and still the path word: the table carries the vector masks.
    assert_eq!((drdc_word(&bytes, 0), drdc_word(&bytes, 1)), (45, 74));
    let reopened = open_byte(&dir, "again.rz", &bytes).expect("reopen");
    for layer in 0..3 {
        assert_eq!(view(&reopened, layer), view(&doc, layer), "layer {layer}");
    }
    assert_eq!(
        reopened.flattened().unwrap().as_raw(),
        doc.flattened().unwrap().as_raw()
    );
    assert_eq!(
        save_byte(&dir, "resave.rz", &reopened),
        bytes,
        "reopen → resave is byte-identical"
    );
    // The raster parts came back whole: removing each vector mask shows the
    // mask the layer was given, with its properties.
    for layer in [1, 2] {
        let back = removed(&reopened, layer).expect("removed");
        assert_eq!(mask_bytes(&back, layer), coverage, "layer {layer}");
    }
    let back = removed(&reopened, 1).unwrap();
    let properties = &back.layers[1].mask_properties;
    assert_eq!((properties.density, properties.feather), (0.8, 2.0));
    // A reopened vector mask is live: it still edits and still follows.
    let turned = reopened.rotate90().expect("turned");
    assert!(view(&turned, 0).is_some(), "the raster-less mask follows");
    assert!(
        view(&turned, 1).is_none(),
        "the one with a raster part does not"
    );
    let enabled = changed(set_flags(&reopened, 2, true, true));
    assert_ne!(alpha(&enabled), alpha(&reopened));

    // Removing every vector mask removes the word and the table.
    let mut plain = reopened;
    for layer in 0..3 {
        plain = removed(&plain, layer).expect("removed");
    }
    let bytes = save_byte(&dir, "plain.rz", &plain);
    assert_ne!(drdc_word(&bytes, 1), 74, "no vector mask, no path word");
}

#[test]
fn a_crafted_vector_mask_record_refuses() {
    let dir = TempDir::new().unwrap();
    let good = save_byte(&dir, "good.rz", &masked());
    let (_, table) = path_table(&good, 1);
    let record = table["vector_masks"][0].clone();
    assert_eq!(record["layer"], json!(0));
    assert_eq!(record["raster"], Value::Null);
    let rebuilt = |record: Value, planes: u32| {
        let text = json!({"paths": [], "vector_masks": [record]}).to_string();
        with_table(&good, 1, &text, planes)
    };
    let with = |key: &str, value: Value| {
        let mut record = record.clone();
        record[key] = value;
        record
    };
    let without = |key: &str| {
        let mut record = record.clone();
        record.as_object_mut().unwrap().remove(key);
        record
    };
    let part = json!({"plane": null, "x": 0, "y": 0, "linked": true, "density": 1.0,
                      "feather": 0.0, "outside": 0, "enabled": true});
    let cases: Vec<(Vec<u8>, &str)> = vec![
        (
            rebuilt(with("layer", json!(1)), 0),
            "a layer that is not there",
        ),
        (rebuilt(without("layer"), 0), "no layer"),
        (rebuilt(without("enabled"), 0), "no enabled flag"),
        (
            rebuilt(with("linked", json!(1)), 0),
            "a linked flag that is a number",
        ),
        (rebuilt(without("raster"), 0), "no raster key"),
        (
            rebuilt(with("subpaths", json!([])), 0),
            "an empty shape on a layer that stores a mask",
        ),
        (
            rebuilt(with("raster", part.clone()), 0),
            "a raster part that is the stored mask beside an enabled shape",
        ),
        (
            rebuilt(with("raster", with_plane(&part, json!(0))), 0),
            "a plane index with no plane",
        ),
        (rebuilt(record.clone(), 1), "a plane no raster part names"),
        (
            rebuilt(with("raster", with_outside(&part, 7)), 0),
            "an outside coverage that is neither 0 nor 255",
        ),
        (
            with_table(
                &good,
                1,
                &json!({"paths": [], "vector_masks": [record.clone(), record.clone()]}).to_string(),
                0,
            ),
            "a layer named twice",
        ),
    ];
    for (bytes, what) in cases {
        let message = match open_byte(&dir, "bad.rz", &bytes) {
            Err(message) => message,
            Ok(_) => panic!("{what} must refuse"),
        };
        assert!(message.contains("vector mask section"), "{what}: {message}");
    }
    // The record as written opens.
    assert!(open_byte(&dir, "ok.rz", &rebuilt(record.clone(), 0)).is_ok());
}

fn with_plane(part: &Value, plane: Value) -> Value {
    let mut part = part.clone();
    part["plane"] = plane;
    part
}

fn with_outside(part: &Value, outside: u8) -> Value {
    let mut part = part.clone();
    part["outside"] = json!(outside);
    part
}

/// A raster mask placed away from the layer's own rectangle keeps its place
/// as the raster part, and the product is right where the two overlap.
#[test]
fn a_raster_part_off_the_layers_rectangle_keeps_its_place() {
    let plane = GrayImage::from_pixel(20, 16, image::Luma([255]));
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([1, 2, 3, 255])))
        .set_mask(0, plane, (10, 6), true, 0)
        .expect("a placed mask");
    let both = changed(set_shape(&doc, 0, &shape_of(&blob())));
    let shape = oracle(&blob(), W, H);
    let want: Vec<f64> = (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .map(|(x, y)| {
            if (10..30).contains(&x) && (6..22).contains(&y) {
                shape[(y * W + x) as usize] * 255.0
            } else {
                0.0
            }
        })
        .collect();
    assert_coverage(&alpha(&both), &want, W, "the placed part");
    let back = removed(&both, 0).expect("removed");
    assert_eq!(back.layers[0].mask_properties.offset, (10, 6));
    assert_eq!(back.layers[0].mask.as_ref().unwrap().dimensions(), (20, 16));
}

/// A GROUP takes a vector mask as it takes a layer mask: canvas-sized,
/// clipping everything inside it, moving with the group and following a
/// quarter turn.
#[test]
fn a_group_takes_a_vector_mask() {
    let (grouped, group, _, _) = byte(W, H).group_layers(&[0], "G").expect("a group");
    assert_eq!(group, 1);
    let doc = changed(set_shape(&grouped, group, &shape_of(&blob())));
    assert_shape(&view(&doc, group).expect("a vector mask"), &blob(), "group");
    assert!(view(&doc, 0).is_none(), "the child has none");
    assert_eq!(
        doc.layers[group].mask.as_ref().unwrap().dimensions(),
        (W, H),
        "a group's mask is canvas-sized"
    );
    assert_coverage(&alpha(&doc), &levels(&oracle(&blob(), W, H)), W, "group");
    // Moving the group moves its mask, and the shape with it.
    let moved = doc.move_layers(&[group], 4, 3).expect("moved");
    let through = mapped(&blob(), |p| [p[0] + 4.0, p[1] + 3.0]);
    assert_shape(
        &view(&moved, group).expect("moved"),
        &through,
        "moved group",
    );
    let turned = doc.rotate90().expect("turned");
    let through = mapped(&blob(), |p| [f64::from(H) - p[1], p[0]]);
    assert_shape(
        &view(&turned, group).expect("turned"),
        &through,
        "turned group",
    );
    assert_coverage(
        &alpha(&turned),
        &levels(&oracle(&through, H, W)),
        H,
        "turned group",
    );
    let bare = removed(&doc, group).expect("removed");
    assert!(bare.layers[group].mask.is_none());
    assert_eq!(bare.layers[group].offset, (0, 0));
}

/// `dr_doc_psd_export_report`'s losses for a layered PSD.
fn psd_losses(doc: &DrDocument) -> Vec<String> {
    let mut error = ptr::null_mut();
    let text = take(unsafe { dr_doc_psd_export_report(doc, 0, 0, &mut error) });
    let report: Value = serde_json::from_str(&text).expect("report JSON");
    report["losses"]
        .as_array()
        .expect("losses")
        .iter()
        .map(|line| line.as_str().expect("a sentence").to_string())
        .collect()
}

/// A layered PSD writes a vector mask with no raster part as its `vmsk`
/// (`psd_paths`), so nothing is said; one beside a raster mask, and one
/// reaching past the 128 canvases a PSD path can hold, are written as their
/// rendered raster, and the preflight says so by the layer's name; an
/// ordinary raster mask is not named.
#[test]
fn psd_export_names_a_vector_mask_written_as_pixels() {
    let doc = masked();
    let name = doc.layers[0].name.clone();
    let sentence = format!(
        "The vector mask on “{name}” is written as pixels, an ordinary layer mask of its rendered shape; its path is not written."
    );
    let losses = psd_losses(&doc);
    assert!(!losses.contains(&sentence), "{losses:?}");
    // 128 × 48 = 6,144 px: an anchor at x 7,000 has no 8.24 fraction.
    let far = json!({"fill_rule": "nonzero", "subpaths": [
        {"closed": true, "anchors": [[4, 4], [44, 4], [7000, 36]]}]});
    let losses = psd_losses(&changed(set_shape(&byte(W, H), 0, &far)));
    assert!(losses.contains(&sentence), "{losses:?}");
    let ordinary = byte(W, H)
        .add_mask(0, MaskKind::FromSelection(&selection(W, H, pattern)))
        .expect("a raster mask");
    let losses = psd_losses(&ordinary);
    assert!(
        !losses
            .iter()
            .any(|line| line.starts_with("The vector mask on")),
        "{losses:?}"
    );
    let both = changed(set_shape(&ordinary, 0, &shape_of(&blob())));
    let losses = psd_losses(&both);
    assert!(losses.contains(&sentence), "{losses:?}");
}
