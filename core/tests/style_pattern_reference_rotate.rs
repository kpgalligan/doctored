//! The layer REFERENCE POINT (Photoshop's `fxrp`) and
//! every linked pattern anchor under a filtered (arbitrary-angle) Rotate
//! Canvas, at 8 bits (the byte geometry request through the FFI), 16 bits,
//! 32-bit float and CMYK16, on rasters and a group record. The build and the
//! helpers are `style_pattern_reference_geometry.rs`'s.
//!
//! THE ORACLE is the shipped rule written out, never the kernel: SWAP on the
//! FLOOR plan. The canvas is W′ × H′, the result's own; the old canvas's
//! top-left lands at o = (⌊(W′−W)/2⌋, ⌊(H′−H)/2⌋), so the old centre lands
//! at C = (W/2 + o.x, H/2 + o.y). The rotated old canvas's margins in the new
//! canvas are fx = C.x − bw/2 and fy = C.y − bh/2 (bw = W|cos t| + H|sin t|,
//! bh = W|sin t| + H|cos t|), and a point goes to the rotation by +t
//! (clockwise on screen, (dx, dy) -> (dx cos t − dy sin t, dx sin t +
//! dy cos t)) about the old centre, carried to C and moved by the margins
//! SWAPPED: (+fy, +fx). Each record's reference is round(oracle(r)) (half
//! away from zero), each linked anchor holds offset′ − anchor′ =
//! round(oracle(offset − anchor)), the input keeps its points, and the
//! linked tile is the same tile (a filtered rotate makes no turned copy:
//! TAKE2's IDNT).
//!
//! THE PHOTOSHOP ROWS (`scripts/photoshop-probe/reference_point_take.py`
//! take1: R30 in `fixtures/psd_photoshop/reference_point/photoshop.json`,
//! TAKE2's eight rows in `rotate.json`), each rebuilt with `take_doc8_sized`
//! and rotated through the FFI: every row's canvas is Photoshop's
//! `ceil(bw) × ceil(bh)`, every judged record's unrounded SWAP lies within
//! TOL (0.01 px, declared before measuring) of Photoshop's `fxrp` on each
//! axis, and the stored point is round(Photoshop's `fxrp`) (BOUND 0 px).
//! SWAP was found from these same records, so no held-out row confirms it.

use std::path::Path;

use doctored_core::doc::DrDocument;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::common::ffi_style;
use crate::style_pattern_reference_geometry::{
    byte_request, cmyk16, facts16, facts8, index16, index8, minus, take_doc8, take_doc8_sized,
    typed16, typed_edit, Facts, CANVAS, LIMIT,
};

/// The rows' bound on the unrounded point (module doc), per axis, in px.
const TOL: f64 = 0.01;
/// The angles of the oracle tests.
const ANGLES: [f64; 2] = [30.0, -20.0];

/// SWAP on the FLOOR plan (module doc): `p` on a `before` canvas rotated by
/// `degrees` into an `after` canvas.
fn oracle(p: (f64, f64), degrees: f64, before: (u32, u32), after: (u32, u32)) -> (f64, f64) {
    let (s, c) = degrees.to_radians().sin_cos();
    let (w, h) = (f64::from(before.0), f64::from(before.1));
    let floor_half = |grown: u32, old: u32| (i64::from(grown) - i64::from(old)).div_euclid(2);
    let centre = (
        w / 2.0 + floor_half(after.0, before.0) as f64,
        h / 2.0 + floor_half(after.1, before.1) as f64,
    );
    let bw = w * c.abs() + h * s.abs();
    let bh = w * s.abs() + h * c.abs();
    let (fx, fy) = (centre.0 - bw / 2.0, centre.1 - bh / 2.0);
    let (dx, dy) = (p.0 - w / 2.0, p.1 - h / 2.0);
    (
        dx * c - dy * s + centre.0 + fy,
        dx * s + dy * c + centre.1 + fx,
    )
}

fn rounded(p: (f64, f64)) -> (i32, i32) {
    (p.0.round() as i32, p.1.round() as i32)
}

fn float(p: (i32, i32)) -> (f64, f64) {
    (f64::from(p.0), f64::from(p.1))
}

fn request(degrees: f64) -> Value {
    json!({"op":"rotate_canvas","degrees":degrees,"filter":"bilinear"})
}

/// `after` holds `before`'s facts moved by POINT (module doc).
fn assert_rotated(before: &Facts, after: &Facts, degrees: f64, canvas: (u32, u32), what: &str) {
    assert_eq!(before.len(), after.len(), "{what}: the same records");
    for (b, a) in before.iter().zip(after) {
        assert_eq!(b.0, a.0, "{what}: record order");
        let want = rounded(oracle(float(b.1), degrees, CANVAS, canvas));
        assert_eq!(a.1, want, "{what}: {}'s reference", b.0);
        match (b.3, a.3) {
            (Some(ab), Some(aa)) => assert_eq!(
                minus(a.2, aa),
                rounded(oracle(float(minus(b.2, ab)), degrees, CANVAS, canvas)),
                "{what}: {}'s offset − anchor",
                b.0
            ),
            (None, None) => {}
            other => panic!("{what}: {}'s anchor {other:?}", b.0),
        }
    }
}

/// A styled record's pattern (the tile and its name), from its style JSON.
fn pattern_of(style: Option<String>) -> Value {
    let style: Value = serde_json::from_str(&style.expect("a styled record")).unwrap();
    style["effects"][0]["pattern"].clone()
}

fn pattern8(doc: &DrDocument, name: &str) -> Value {
    pattern_of(ffi_style(doc, index8(doc, name)))
}

fn pattern16(doc: &PrecisionDocument, name: &str) -> Value {
    pattern_of(doc.layer(index16(doc, name)).unwrap().reported_style())
}

#[test]
fn a_byte_filtered_rotate_maps_every_point_and_linked_anchor() {
    let doc = take_doc8();
    let before = facts8(&doc);
    for degrees in ANGLES {
        let out = byte_request(&doc, &request(degrees));
        let what = format!("byte {degrees}°");
        assert_rotated(
            &before,
            &facts8(&out),
            degrees,
            (out.width, out.height),
            &what,
        );
        assert_eq!(facts8(&doc), before, "{what}: purity");
        for name in ["L0", "L1"] {
            assert_eq!(
                pattern8(&out, name),
                pattern8(&doc, name),
                "{what}: {name}'s tile is the same tile"
            );
        }
    }
}

#[test]
fn a_typed_filtered_rotate_maps_every_point_at_16_bits_float_and_cmyk() {
    let sixteen = typed16();
    let float = sixteen.promote_to_float(LIMIT, 0, None).unwrap();
    let cmyk = cmyk16();
    for (doc, depth) in [(&sixteen, "RGB16"), (&float, "float"), (&cmyk, "CMYK16")] {
        let before = facts16(doc);
        for degrees in ANGLES {
            let out = typed_edit(doc, &request(degrees));
            let what = format!("{depth} {degrees}°");
            assert_rotated(&before, &facts16(&out), degrees, out.dimensions(), &what);
            assert_eq!(facts16(doc), before, "{what}: purity");
            for name in ["L0", "L1"] {
                assert_eq!(
                    pattern16(&out, name),
                    pattern16(doc, name),
                    "{what}: {name}'s tile is the same tile"
                );
            }
        }
    }
}

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/reference_point")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn size(v: &Value) -> (u32, u32) {
    (v[0].as_u64().unwrap() as u32, v[1].as_u64().unwrap() as u32)
}

fn point(v: &Value) -> (f64, f64) {
    if v.is_null() {
        return (0.0, 0.0);
    }
    (v[0].as_f64().unwrap(), v[1].as_f64().unwrap())
}

#[test]
fn photoshops_rows_match_swap_on_photoshops_canvas() {
    let take1 = fixture("photoshop.json");
    let take2 = fixture("rotate.json");
    let mut rows = vec![("R30".to_string(), 30.0, take1["cases"]["R30"].clone())];
    for (name, case) in take2["cases"].as_object().unwrap() {
        rows.push((name.clone(), case["angle"].as_f64().unwrap(), case.clone()));
    }
    let mut judged = 0;
    for (name, degrees, case) in rows {
        let before = size(&case["canvas_before"]);
        let photoshop = size(&case["canvas_after"]);
        let doc = take_doc8_sized(before);
        let out = byte_request(&doc, &request(degrees));
        assert_eq!(
            (out.width, out.height),
            photoshop,
            "{name}: Photoshop's canvas"
        );
        for record in ["L0", "L1", "L2", "G"] {
            let r = &case["records"][record];
            let r_before = point(&r["fxrp_before"]);
            assert_eq!(
                float(doc.layers[index8(&doc, record)].reference),
                r_before,
                "{name}: {record} before"
            );
            let want = oracle(r_before, degrees, before, photoshop);
            let got = point(&r["fxrp_after"]);
            assert!(
                (want.0 - got.0).abs() <= TOL && (want.1 - got.1).abs() <= TOL,
                "{name}: {record} SWAP {want:?} against Photoshop's {got:?}"
            );
            assert_eq!(
                out.layers[index8(&out, record)].reference,
                rounded(got),
                "{name}: {record} stored as round(Photoshop's fxrp)"
            );
            judged += 1;
        }
    }
    assert_eq!(judged, 9 * 4, "every record of take1's nine rows");
}
