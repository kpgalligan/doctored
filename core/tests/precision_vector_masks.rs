//! VECTOR MASKS at 16 bits and at 32-bit float, through the
//! `dr_precision_doc_*vector_mask*` exports — the typed twins of
//! `vector_masks.rs`'s cases, against the same kind of oracle written here:
//! the shape flattened by evaluating each cubic's Bernstein form at 48
//! parameters, and a pixel's coverage taken as the fraction of its 16 × 16
//! sample centres the polygon winds around.
//!
//! A 16-bit document of one opaque layer renders its mask's effective
//! coverage as its ALPHA, which is how the mask is read without asking the
//! core what it baked. A float document's vector mask is read off the mask
//! plane itself (its masks are the same u16 planes).

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_path::*;
use doctored_core::ffi_precision::{dr_precision_doc_open, dr_precision_doc_save_native};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::dr_precision_doc_psd_export_report;
use doctored_core::ffi_vector_mask::*;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_quantize::PrecisionDither;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, drdc_word, take_err_string};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 48;
const H: u32 = 40;
/// How far a render may sit from the 16 × 16 oracle on an edge pixel: one
/// row of samples (1/16) and a little rounding, as a fraction of full.
const EDGE: f64 = 0.08;
const CAP: &str = "too many path anchors in the document (max 65536)";

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

fn shape_of(nodes: &[Node]) -> Value {
    let anchors: Vec<Value> = nodes
        .iter()
        .map(|n| json!({"x": n.p[0], "y": n.p[1], "in": n.i, "out": n.o}))
        .collect();
    json!({"fill_rule": "nonzero", "subpaths": [{"closed": true, "anchors": anchors}]})
}

fn polygon(corners: &[[f64; 2]]) -> Value {
    json!({"closed": true, "anchors": corners})
}

/// The shape's coverage of each pixel of a `w` × `h` canvas, in 0…1.
fn oracle(nodes: &[Node], w: u32, h: u32) -> Vec<f64> {
    let mut polygon = Vec::new();
    for k in 0..nodes.len() {
        let (a, b) = (nodes[k], nodes[(k + 1) % nodes.len()]);
        let p = [a.p, a.o, b.i, b.p];
        for step in 0..48 {
            let t = f64::from(step) / 48.0;
            let s = 1.0 - t;
            let weights = [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t];
            polygon.push([0, 1].map(|axis| (0..4).map(|j| weights[j] * p[j][axis]).sum::<f64>()));
        }
    }
    let winding = |at: [f64; 2]| {
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
    };
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
                    inside += i32::from(winding(at) != 0);
                }
            }
            out.push(f64::from(inside) / 256.0);
        }
    }
    out
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

/// `alpha` (u16 a pixel, `w` a row) is `expected` (0…1) to the oracle's
/// tolerance, and exactly 0 or 65535 on a pixel no edge comes near.
fn assert_coverage(alpha: &[u16], expected: &[f64], w: u32, what: &str) {
    assert_eq!(alpha.len(), expected.len(), "{what}");
    for (index, (&got, &want)) in alpha.iter().zip(expected).enumerate() {
        let (x, y) = (index as u32 % w, index as u32 / w);
        if (want == 0.0 || want == 1.0) && level(expected, w, index) {
            assert_eq!(f64::from(got), want * 65535.0, "{what}: pixel ({x}, {y})");
        } else {
            assert!(
                (f64::from(got) / 65535.0 - want).abs() <= EDGE,
                "{what}: pixel ({x}, {y}) is {got}, the oracle says {want:.3}"
            );
        }
    }
}

/// Two alpha planes agree to one code: the render is float arithmetic over
/// the mask's u16 samples.
fn assert_near(a: &[u16], b: &[u16], what: &str) {
    assert_eq!(a.len(), b.len(), "{what}");
    for (index, (&x, &y)) in a.iter().zip(b).enumerate() {
        assert!(x.abs_diff(y) <= 1, "{what}: pixel {index} is {x} and {y}");
    }
}

fn typed(w: u32, h: u32) -> PrecisionDocument {
    let pixels = vec![[12345u16, 23456, 34567, 65535]; (w * h) as usize];
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}

/// The rendered document's alpha plane.
fn alpha(doc: &PrecisionDocument) -> Vec<u16> {
    doc.render(LIMIT, 0, None)
        .expect("render")
        .to_rgba16()
        .expect("16-bit pixels")
        .iter()
        .map(|p| p[3])
        .collect()
}

fn answer(
    out: *mut PrecisionDocument,
    error: *mut c_char,
) -> Result<Option<PrecisionDocument>, String> {
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

fn changed(result: Result<Option<PrecisionDocument>, String>) -> PrecisionDocument {
    match result {
        Ok(Some(document)) => document,
        Ok(None) => panic!("unchanged, where a new document was expected"),
        Err(message) => panic!("refused: {message}"),
    }
}

fn refused(result: Result<Option<PrecisionDocument>, String>) -> String {
    match result {
        Err(message) => message,
        Ok(_) => panic!("no refusal"),
    }
}

fn set_shape(
    doc: &PrecisionDocument,
    layer: usize,
    shape: &Value,
) -> Result<Option<PrecisionDocument>, String> {
    let text = CString::new(shape.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_set_vector_mask(
            doc,
            layer,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    answer(out, error)
}

fn set_flags(
    doc: &PrecisionDocument,
    layer: usize,
    enabled: bool,
    linked: bool,
) -> Result<Option<PrecisionDocument>, String> {
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_set_vector_mask_properties(
            doc,
            layer,
            enabled,
            linked,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    answer(out, error)
}

fn removed(doc: &PrecisionDocument, layer: usize) -> Result<Option<PrecisionDocument>, String> {
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_remove_vector_mask(doc, layer, LIMIT, 0, &mut error) };
    answer(out, error)
}

fn view(doc: &PrecisionDocument, layer: usize) -> Option<Value> {
    let out = unsafe { dr_precision_doc_vector_mask_json(doc, layer) };
    if out.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(out) };
    Some(serde_json::from_str(&text).expect("vector mask JSON"))
}

/// One `dr_precision_doc_edit` request with its u16 payload.
fn edit(
    doc: &PrecisionDocument,
    request: &Value,
    samples: &[u16],
) -> Result<Option<PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let payload = if samples.is_empty() {
        ptr::null()
    } else {
        samples.as_ptr()
    };
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            payload,
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    answer(out, error)
}

fn assert_shape(view: &Value, nodes: &[Node], what: &str) {
    let anchors = view["subpaths"][0]["anchors"].as_array().expect("anchors");
    assert_eq!(anchors.len(), nodes.len(), "{what}");
    let near = |got: &Value, want: f64| (got.as_f64().unwrap() - want).abs() <= 1e-4;
    for (anchor, node) in anchors.iter().zip(nodes) {
        assert!(
            near(&anchor["x"], node.p[0])
                && near(&anchor["y"], node.p[1])
                && near(&anchor["in"][0], node.i[0])
                && near(&anchor["in"][1], node.i[1])
                && near(&anchor["out"][0], node.o[0])
                && near(&anchor["out"][1], node.o[1]),
            "{what}: {anchor} should be {node:?}"
        );
    }
}

/// A canvas-sized raster coverage that varies in both axes and has no zero.
fn pattern() -> Vec<u16> {
    (0..H)
        .flat_map(|y| (0..W).map(move |x| (12_000 + (x * 811 + y * 523) % 50_000) as u16))
        .collect()
}

fn raster_request() -> Value {
    json!({"op": "set_mask", "layer": 0, "width": W, "height": H})
}

fn masked() -> PrecisionDocument {
    changed(set_shape(&typed(W, H), 0, &shape_of(&blob())))
}

fn save(dir: &TempDir, name: &str, doc: &PrecisionDocument) -> Vec<u8> {
    let path = dir.path().join(name);
    let mut error = ptr::null_mut();
    let saved = unsafe {
        dr_precision_doc_save_native(
            doc,
            cpath(&path).as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(saved, "{}", take_err_string(error));
    std::fs::read(path).expect("read back")
}

fn open(dir: &TempDir, name: &str, bytes: &[u8]) -> Result<PrecisionDocument, String> {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).expect("write");
    let mut error = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_open(cpath(&path).as_ptr(), LIMIT, 0, ptr::null(), &mut error) };
    if out.is_null() {
        return Err(take_err_string(error));
    }
    Ok(*unsafe { Box::from_raw(out) })
}

// ------------------------------------------------------------- the bake --

#[test]
fn a_typed_vector_mask_composites_as_the_shapes_coverage() {
    let base = typed(W, H);
    assert!(view(&base, 0).is_none());
    assert!(matches!(removed(&base, 0), Ok(None)), "nothing to remove");
    assert!(matches!(set_flags(&base, 0, false, true), Ok(None)));
    // The typed core names an index that is not there.
    assert!(refused(set_shape(&base, 7, &shape_of(&blob()))).contains("out of range"));

    let doc = masked();
    let seen = view(&doc, 0).expect("a vector mask");
    assert_shape(&seen, &blob(), "the stored shape");
    assert_eq!(
        (
            &seen["enabled"],
            &seen["linked"],
            &seen["has_raster"],
            &seen["raster_enabled"]
        ),
        (&json!(true), &json!(true), &json!(false), &json!(false))
    );
    assert_coverage(&alpha(&doc), &oracle(&blob(), W, H), W, "the blob");
    assert!(matches!(set_shape(&doc, 0, &shape_of(&blob())), Ok(None)));
    assert!(
        refused(set_shape(&doc, 0, &json!({"subpaths": [{"anchors": []}]})))
            .contains("at least one anchor")
    );

    // An axis-aligned square on pixel edges is exact at every pixel.
    let square = polygon(&[[4.0, 4.0], [20.0, 4.0], [20.0, 20.0], [4.0, 20.0]]);
    let squared = changed(set_shape(&doc, 0, &json!({"subpaths": [square]})));
    let got = alpha(&squared);
    for y in 0..H {
        for x in 0..W {
            let inside = (4..20).contains(&x) && (4..20).contains(&y);
            let want = if inside { 65535 } else { 0 };
            assert_eq!(got[(y * W + x) as usize], want, "square at ({x}, {y})");
        }
    }

    // The two flags.
    let off = changed(set_flags(&doc, 0, false, true));
    assert!(alpha(&off).iter().all(|&a| a == 65535));
    assert_eq!(view(&off, 0).unwrap()["enabled"], json!(false));
    assert!(matches!(set_flags(&off, 0, false, true), Ok(None)));
    assert_eq!(alpha(&changed(set_flags(&off, 0, true, true))), alpha(&doc));

    // Reveal All, then no vector mask at all.
    let open = changed(set_shape(&doc, 0, &json!({"subpaths": []})));
    assert!(alpha(&open).iter().all(|&a| a == 65535));
    assert!(open.layer(0).unwrap().mask().is_none());
    assert_eq!(view(&open, 0).unwrap()["subpaths"], json!([]));
    let bare = changed(removed(&open, 0));
    assert!(view(&bare, 0).is_none());
}

#[test]
fn a_typed_raster_mask_multiplies_in_and_mask_edits_edit_the_raster_part() {
    let coverage = pattern();
    let rastered = changed(edit(&typed(W, H), &raster_request(), &coverage));
    assert_near(&alpha(&rastered), &coverage, "the raster mask alone");
    let both = changed(set_shape(&rastered, 0, &shape_of(&blob())));
    let seen = view(&both, 0).expect("a vector mask");
    assert_eq!(
        (&seen["has_raster"], &seen["raster_enabled"]),
        (&json!(true), &json!(true))
    );
    // The product of the two factors, each known on its own.
    let shape = oracle(&blob(), W, H);
    let got = alpha(&both);
    for (index, (&g, &s)) in got.iter().zip(&shape).enumerate() {
        let want = s * f64::from(coverage[index]);
        let slack = if level(&shape, W, index) {
            2.0
        } else {
            EDGE * 65535.0
        };
        assert!(
            (f64::from(g) - want).abs() <= slack,
            "pixel {index} is {g}, the product is {want:.1}"
        );
    }

    // Disabling the layer mask disables the raster part; the shape stays.
    let quiet = changed(edit(
        &both,
        &json!({"op": "mask_properties", "layer": 0, "x": 0, "y": 0, "linked": true,
                "density": 1.0, "feather": 0.0, "enabled": false}),
        &[],
    ));
    let seen = view(&quiet, 0).expect("kept");
    assert_shape(&seen, &blob(), "after the properties edit");
    assert_eq!(
        (&seen["has_raster"], &seen["raster_enabled"]),
        (&json!(true), &json!(false))
    );
    assert_coverage(&alpha(&quiet), &shape, W, "the shape alone");
    // The raster part reports its own placement and properties — what a
    // host's read-modify-write of a mask property starts from — and a
    // properties edit lands on it: density one half, unlinked, and the link
    // is the vector mask's too.
    assert_eq!(
        seen["raster"],
        json!({"offset_x": 0, "offset_y": 0, "width": W, "height": H, "linked": true,
               "density": 1.0, "feather": 0.0, "outside_coverage": 0})
    );
    let dense = changed(edit(
        &both,
        &json!({"op": "mask_properties", "layer": 0, "x": 0, "y": 0, "linked": false,
                "density": 0.5, "feather": 0.0, "enabled": true}),
        &[],
    ));
    let seen = view(&dense, 0).expect("kept");
    assert_eq!(seen["raster"]["density"], json!(0.5));
    assert_eq!(
        (&seen["linked"], &seen["raster"]["linked"]),
        (&json!(false), &json!(false))
    );
    let slot = dense.layer(0).unwrap().mask().expect("the baked mask");
    assert_eq!(slot.properties().density, 1.0);
    assert!(!slot.properties().linked);

    // Replacing the layer mask replaces the raster part.
    let flat = vec![32768u16; (W * H) as usize];
    let halved = changed(edit(&both, &raster_request(), &flat));
    assert_shape(&view(&halved, 0).expect("kept"), &blob(), "after set_mask");
    for (index, (&g, &s)) in alpha(&halved).iter().zip(&shape).enumerate() {
        if level(&shape, W, index) {
            let want: u16 = if s == 1.0 { 32768 } else { 0 };
            assert!(g.abs_diff(want) <= 1, "pixel {index} is {g}");
        }
    }

    // Deleting the layer mask deletes the raster part only.
    let alone = changed(edit(&both, &json!({"op": "remove_mask", "layer": 0}), &[]));
    assert_eq!(view(&alone, 0).unwrap()["has_raster"], json!(false));
    assert_coverage(&alpha(&alone), &shape, W, "after remove_mask");
    // …and with no raster part there is no layer mask to delete.
    assert!(edit(&alone, &json!({"op": "remove_mask", "layer": 0}), &[])
        .map(|d| d.is_none())
        .unwrap_or(true));

    // Removing the vector mask puts the raster part back verbatim.
    let back = changed(removed(&both, 0));
    assert!(view(&back, 0).is_none());
    assert_eq!(
        back.layer(0).unwrap().mask().unwrap().plane().samples(),
        coverage.as_slice(),
        "the raster part's own samples"
    );
    assert_near(&alpha(&back), &coverage, "the raster part alone");

    // Applying the mask consumes both and keeps the picture.
    let before = alpha(&both);
    let applied = changed(edit(&both, &json!({"op": "apply_mask", "layer": 0}), &[]));
    assert!(view(&applied, 0).is_none());
    assert!(applied.layer(0).unwrap().mask().is_none());
    for (index, (&a, &b)) in alpha(&applied).iter().zip(&before).enumerate() {
        assert!(a.abs_diff(b) <= 1, "pixel {index}: {a} and {b}");
    }
}

// ------------------------------------------------------------ following --

#[test]
fn a_typed_vector_mask_follows_the_geometry_ops() {
    let doc = masked();
    let filter = "bilinear";

    // A whole-pixel move is a translation of the plane: the shape is read
    // shifted.
    let moved = changed(edit(
        &doc,
        &json!({"op": "affine", "layer": 0, "target": "pixels",
                "matrix": [1.0, 0.0, 0.0, 1.0, 5.0, 3.0], "filter": filter}),
        &[],
    ));
    let through = mapped(&blob(), |p| [p[0] + 5.0, p[1] + 3.0]);
    assert_shape(&view(&moved, 0).expect("moved"), &through, "move");
    let coverage = oracle(&through, W, H);
    let want: Vec<f64> = (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .map(|(x, y)| {
            if x < 5 || y < 3 {
                0.0
            } else {
                coverage[(y * W + x) as usize]
            }
        })
        .collect();
    assert_coverage(&alpha(&moved), &want, W, "move");

    // A scale and a shift: the shape goes through the matrix.
    let scaled = changed(edit(
        &doc,
        &json!({"op": "affine", "layer": 0, "target": "pixels",
                "matrix": [1.25, 0.0, 0.0, 1.25, -3.0, -2.0], "filter": filter}),
        &[],
    ));
    let through = mapped(&blob(), |p| [p[0] * 1.25 - 3.0, p[1] * 1.25 - 2.0]);
    let seen = view(&scaled, 0).expect("the mask followed the transform");
    assert_shape(&seen, &through, "affine");
    assert_coverage(&alpha(&scaled), &oracle(&through, W, H), W, "affine");

    // Rotate 90 clockwise: (x, y) → (H − y, x) on an H × W canvas.
    let turned = changed(edit(&doc, &json!({"op": "rotate90"}), &[]));
    assert_eq!(turned.dimensions(), (H, W));
    let through = mapped(&blob(), |p| [f64::from(H) - p[1], p[0]]);
    assert_shape(&view(&turned, 0).expect("turned"), &through, "rotate90");
    assert_coverage(&alpha(&turned), &oracle(&through, H, W), H, "rotate90");

    // Image Size to 96 × 60: (x, y) → (2x, 1.5y).
    let grown = changed(edit(
        &doc,
        &json!({"op": "resize", "width": 96, "height": 60, "filter": filter}),
        &[],
    ));
    let through = mapped(&blob(), |p| [p[0] * 2.0, p[1] * 1.5]);
    assert_shape(&view(&grown, 0).expect("resized"), &through, "resize");
    assert_coverage(&alpha(&grown), &oracle(&through, 96, 60), 96, "resize");

    // Crop shifts the canvas under the shape.
    let cropped = changed(edit(
        &doc,
        &json!({"op": "crop", "x": 4, "y": 2, "width": 40, "height": 36}),
        &[],
    ));
    let through = mapped(&blob(), |p| [p[0] - 4.0, p[1] - 2.0]);
    assert_shape(&view(&cropped, 0).expect("cropped"), &through, "crop");
    assert_coverage(&alpha(&cropped), &oracle(&through, 40, 36), 40, "crop");

    // An arbitrary canvas rotation leaves a plain mask, and the picture of
    // the twin whose mask is that same plane held as an ordinary mask.
    let mask = doc.layer(0).unwrap().mask().expect("a baked mask");
    let (mw, mh) = mask.plane().dimensions();
    let at = mask.properties().offset;
    let twin = changed(edit(
        &typed(W, H),
        &json!({"op": "set_mask", "layer": 0, "width": mw, "height": mh,
                "x": at.0, "y": at.1}),
        mask.plane().samples(),
    ));
    assert!(view(&twin, 0).is_none());
    assert_eq!(alpha(&twin), alpha(&doc));
    let request = json!({"op": "rotate_canvas", "degrees": 30, "filter": filter});
    let (turned, plain) = (
        changed(edit(&doc, &request, &[])),
        changed(edit(&twin, &request, &[])),
    );
    assert!(view(&turned, 0).is_none(), "no vector mask after the turn");
    assert!(turned.layer(0).unwrap().mask().is_some());
    assert_eq!(
        turned.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap(),
        plain.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
    );

    // With a raster part the mask does not follow a resample; it does
    // survive a move.
    let both = changed(set_shape(
        &changed(edit(&typed(W, H), &raster_request(), &pattern())),
        0,
        &shape_of(&blob()),
    ));
    let turned = changed(edit(&both, &json!({"op": "rotate90"}), &[]));
    assert!(view(&turned, 0).is_none());
    assert!(turned.layer(0).unwrap().mask().is_some());
    let moved = changed(edit(
        &both,
        &json!({"op": "affine", "layer": 0, "target": "pixels",
                "matrix": [1.0, 0.0, 0.0, 1.0, 2.0, 1.0], "filter": filter}),
        &[],
    ));
    assert_eq!(view(&moved, 0).unwrap()["has_raster"], json!(true));
}

// -------------------------------------------------------------- the cap --

#[test]
fn a_typed_shape_that_crosses_the_document_anchor_cap_refuses() {
    let ring = |count: usize| {
        let corners: Vec<[f64; 2]> = (0..count)
            .map(|k| [(k % 40) as f64 + 2.0, (k / 40 % 30) as f64 + 2.0])
            .collect();
        json!([{"closed": true, "anchors": corners}])
    };
    let list = json!({"paths": [
        {"kind": "saved", "name": "A", "subpaths": ring(32768)},
        {"kind": "saved", "name": "B", "subpaths": ring(32765)},
    ]});
    let text = CString::new(list.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_with_paths_json(&typed(W, H), text.as_ptr(), &mut error) };
    let full = changed(answer(out, error));
    let square = [[4.0, 4.0], [30.0, 4.0], [30.0, 30.0], [4.0, 30.0]];
    assert_eq!(
        refused(set_shape(
            &full,
            0,
            &json!({"subpaths": [polygon(&square)]})
        )),
        CAP
    );
    let triangle = [[4.0, 4.0], [30.0, 6.0], [10.0, 30.0]];
    let at_cap = changed(set_shape(
        &full,
        0,
        &json!({"subpaths": [polygon(&triangle)]}),
    ));
    assert_eq!(
        refused(set_shape(
            &at_cap,
            0,
            &json!({"subpaths": [polygon(&square)]})
        )),
        CAP
    );
}

// ----------------------------------------------------------------- .rz --

#[test]
fn typed_vector_masks_round_trip_through_rz() {
    let dir = TempDir::new().unwrap();
    // A vector mask alone, and one beside a raster part.
    for (name, doc) in [
        ("alone", masked()),
        (
            "beside",
            changed(set_shape(
                &changed(edit(&typed(W, H), &raster_request(), &pattern())),
                0,
                &shape_of(&blob()),
            )),
        ),
    ] {
        let bytes = save(&dir, &format!("{name}.rz"), &doc);
        assert_eq!(
            (drdc_word(&bytes, 0), drdc_word(&bytes, 1)),
            (46, 75),
            "{name}"
        );
        let reopened = open(&dir, &format!("{name}-again.rz"), &bytes).expect("reopen");
        assert_eq!(view(&reopened, 0), view(&doc, 0), "{name}");
        assert_eq!(alpha(&reopened), alpha(&doc), "{name}");
        assert_eq!(
            save(&dir, &format!("{name}-resave.rz"), &reopened),
            bytes,
            "{name}: reopen → resave is byte-identical"
        );
        // Live after the reopen: it can still be taken away, which shows
        // the raster part (or nothing) as it was.
        let back = changed(removed(&reopened, 0));
        if name == "beside" {
            assert_eq!(
                back.layer(0).unwrap().mask().unwrap().plane().samples(),
                pattern().as_slice()
            );
        } else {
            assert!(back.layer(0).unwrap().mask().is_none());
        }
        let bytes = save(&dir, &format!("{name}-plain.rz"), &back);
        assert_ne!(
            drdc_word(&bytes, 1),
            75,
            "{name}: no vector mask, no path word"
        );
    }
}

#[test]
fn a_float_document_takes_a_vector_mask() {
    let dir = TempDir::new().unwrap();
    let base = crate::float_fixture::shaped_pair(W, H);
    assert!(base.is_float());
    let doc = changed(set_shape(&base, 0, &shape_of(&blob())));
    assert!(doc.is_float());
    assert_shape(&view(&doc, 0).expect("a vector mask"), &blob(), "float");
    // The mask is a u16 plane: read it against the oracle where it lies.
    let mask = doc.layer(0).unwrap().mask().expect("a baked mask");
    let (mw, mh) = mask.plane().dimensions();
    let at = mask.properties().offset;
    let shape = oracle(&blob(), W, H);
    let mut canvas = vec![0u16; (W * H) as usize];
    for y in 0..mh {
        for x in 0..mw {
            let (cx, cy) = (at.0 + x as i32, at.1 + y as i32);
            assert!((0..W as i32).contains(&cx) && (0..H as i32).contains(&cy));
            canvas[(cy as u32 * W + cx as u32) as usize] =
                mask.plane().samples()[(y * mw + x) as usize];
        }
    }
    assert_coverage(&canvas, &shape, W, "the float mask plane");

    let bytes = save(&dir, "float.rz", &doc);
    let reopened = open(&dir, "float-again.rz", &bytes).expect("reopen");
    assert!(reopened.is_float());
    assert_eq!(view(&reopened, 0), view(&doc, 0));
    assert_eq!(save(&dir, "float-resave.rz", &reopened), bytes);

    // float → 16 carries the mask as the same plane, so it stays a vector
    // mask.
    let sixteen = doc
        .narrow_structure_to_u16(LIMIT, 0, None)
        .expect("float to 16");
    assert!(!sixteen.is_float());
    assert_eq!(view(&sixteen, 0), view(&doc, 0));
}

// ----------------------------------------------------- depth conversion --

#[test]
fn depth_conversion_keeps_a_vector_mask_without_a_raster_part() {
    // 16 → 8: the shape is baked again at 8 bits.
    let narrowed: DrDocument = masked()
        .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, None)
        .expect("16 to 8");
    let seen = {
        let out = unsafe { dr_doc_vector_mask_json(&narrowed, 0) };
        assert!(!out.is_null(), "the vector mask crossed to 8 bits");
        let text = unsafe { CStr::from_ptr(out) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(out) };
        serde_json::from_str::<Value>(&text).unwrap()
    };
    assert_shape(&seen, &blob(), "16 to 8");
    let bytes: Vec<u16> = narrowed
        .flattened()
        .unwrap()
        .pixels()
        .map(|p| u16::from(p[3]) * 257)
        .collect();
    assert_coverage(&bytes, &oracle(&blob(), W, H), W, "16 to 8");

    // 8 → 16: and again at 16.
    let promoted = narrowed.promote_structure(LIMIT, 0, None).expect("8 to 16");
    assert_shape(
        &view(&promoted, 0).expect("crossed back"),
        &blob(),
        "8 to 16",
    );
    assert_coverage(&alpha(&promoted), &oracle(&blob(), W, H), W, "8 to 16");

    // With a raster part the mask crosses as the plane it baked.
    let both = changed(set_shape(
        &changed(edit(&typed(W, H), &raster_request(), &pattern())),
        0,
        &shape_of(&blob()),
    ));
    let narrowed = both
        .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, None)
        .expect("16 to 8");
    assert!(unsafe { dr_doc_vector_mask_json(&narrowed, 0) }.is_null());
    assert!(narrowed.layers[0].mask.is_some());
}

/// `dr_precision_doc_psd_export_report`'s losses for a layered PSD.
fn psd_losses(doc: &PrecisionDocument) -> Vec<String> {
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_psd_export_report(doc, 0, 0, &mut error) };
    assert!(!out.is_null(), "{}", take_err_string(error));
    let text = unsafe { CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(out) };
    let report: Value = serde_json::from_str(&text).expect("report JSON");
    report["losses"]
        .as_array()
        .expect("losses")
        .iter()
        .map(|line| line.as_str().expect("a sentence").to_string())
        .collect()
}

/// The 16-bit layered PSD names a vector mask written as pixels by its
/// layer's name; an ordinary raster mask is not named.
#[test]
fn psd_export16_names_a_vector_mask_written_as_pixels() {
    let doc = masked();
    let name = doc.layer(0).unwrap().name().to_string();
    let sentence = format!(
        "The vector mask on “{name}” is written as pixels, an ordinary layer mask of its rendered shape; its path is not written."
    );
    let losses = psd_losses(&doc);
    assert!(losses.contains(&sentence), "{losses:?}");
    let ordinary = changed(edit(&typed(W, H), &raster_request(), &pattern()));
    let losses = psd_losses(&ordinary);
    assert!(
        !losses
            .iter()
            .any(|line| line.starts_with("The vector mask on")),
        "{losses:?}"
    );
}
